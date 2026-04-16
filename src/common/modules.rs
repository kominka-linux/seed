use std::collections::{HashMap, HashSet};
use std::ffi::CString;
use std::fs::{self, File, OpenOptions};
use std::io::{self, Read, Write};
use std::os::fd::AsRawFd;
use std::path::{Path, PathBuf};

use bzip2::read::BzDecoder;
use flate2::read::GzDecoder;
use lzma_rust2::XzReader;

use crate::common::error::{AppletError, AppletErrorKind};

const MODULE_EXTENSIONS: &[&str] = &[".ko", ".ko.gz", ".ko.xz", ".ko.bz2"];
const MODPROBE_CONFIG_DIRS: &[&str] = &[
    "/etc/modprobe.d",
    "/run/modprobe.d",
    "/usr/local/lib/modprobe.d",
    "/usr/lib/modprobe.d",
    "/lib/modprobe.d",
];

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct ModuleEntry {
    pub name: String,
    pub relative_path: String,
    pub path: PathBuf,
    pub metadata: ModuleMetadata,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub(crate) struct ModuleMetadata {
    fields: Vec<(String, String)>,
}

#[derive(Clone, Debug, Default)]
pub(crate) struct ModuleIndex {
    pub entries: Vec<ModuleEntry>,
    pub(crate) by_name: HashMap<String, usize>,
    pub(crate) builtins: HashSet<String>,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub(crate) struct ModprobeConfig {
    pub aliases: Vec<ConfigAlias>,
    pub blacklists: HashSet<String>,
    pub install_commands: HashMap<String, String>,
    pub remove_commands: HashMap<String, String>,
    pub options: HashMap<String, Vec<String>>,
    pub softdeps: HashMap<String, Softdep>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct ConfigAlias {
    pub pattern: String,
    pub target: String,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub(crate) struct Softdep {
    pub pre: Vec<String>,
    pub post: Vec<String>,
}

impl ModuleMetadata {
    pub(crate) fn parse(bytes: &[u8]) -> Self {
        let fields = bytes
            .split(|byte| *byte == 0)
            .filter_map(|chunk| {
                if chunk.is_empty() || !chunk.is_ascii() {
                    return None;
                }
                let text = std::str::from_utf8(chunk).ok()?;
                let (key, value) = text.split_once('=')?;
                if key.is_empty()
                    || !key
                        .bytes()
                        .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_')
                {
                    return None;
                }
                Some((key.to_string(), value.to_string()))
            })
            .collect();
        Self { fields }
    }

    pub(crate) fn fields(&self) -> &[(String, String)] {
        &self.fields
    }

    pub(crate) fn values<'a>(&'a self, key: &'a str) -> impl Iterator<Item = &'a str> + 'a {
        self.fields
            .iter()
            .filter_map(move |(field, value)| (field == key).then_some(value.as_str()))
    }

    pub(crate) fn depends(&self) -> Vec<String> {
        self.values("depends")
            .flat_map(|value| value.split(','))
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(normalize_module_name)
            .collect()
    }

    pub(crate) fn aliases(&self) -> Vec<String> {
        self.values("alias").map(str::to_string).collect()
    }
}

impl ModuleIndex {
    pub(crate) fn scan(root: &Path) -> io::Result<Self> {
        let mut entries = Vec::new();
        let mut stack = vec![root.to_path_buf()];

        while let Some(path) = stack.pop() {
            let read_dir = match fs::read_dir(&path) {
                Ok(read_dir) => read_dir,
                Err(err) if err.kind() == io::ErrorKind::NotFound => continue,
                Err(err) => return Err(err),
            };

            for entry in read_dir {
                let entry = entry?;
                let entry_path = entry.path();
                let metadata = entry.metadata()?;
                if metadata.is_dir() {
                    stack.push(entry_path);
                    continue;
                }
                if !is_module_path(&entry_path) {
                    continue;
                }
                let relative_path = entry_path
                    .strip_prefix(root)
                    .unwrap_or(&entry_path)
                    .to_string_lossy()
                    .replace('\\', "/");
                let data = read_module_file(&entry_path)?;
                let metadata = ModuleMetadata::parse(&data);
                entries.push(ModuleEntry {
                    name: module_name_from_path(&relative_path),
                    relative_path,
                    path: entry_path,
                    metadata,
                });
            }
        }

        entries.sort_by(|left, right| left.relative_path.cmp(&right.relative_path));

        let by_name = entries
            .iter()
            .enumerate()
            .map(|(index, entry)| (entry.name.clone(), index))
            .collect();
        let builtins = read_builtins(root)?;
        Ok(Self {
            entries,
            by_name,
            builtins,
        })
    }

    pub(crate) fn get(&self, name: &str) -> Option<&ModuleEntry> {
        self.by_name
            .get(&normalize_module_name(name))
            .and_then(|index| self.entries.get(*index))
    }

    pub(crate) fn is_builtin(&self, name: &str) -> bool {
        self.builtins.contains(&normalize_module_name(name))
    }

    pub(crate) fn resolve_alias(&self, name_or_alias: &str) -> Option<&ModuleEntry> {
        if let Some(entry) = self.get(name_or_alias) {
            return Some(entry);
        }
        self.entries.iter().find(|entry| {
            entry
                .metadata
                .aliases()
                .iter()
                .any(|alias| fnmatch(alias, name_or_alias).unwrap_or(false))
        })
    }
}

impl ModprobeConfig {
    pub(crate) fn load() -> Result<Self, AppletError> {
        let mut config = Self::default();
        for file in modprobe_config_files()
            .map_err(|err| AppletError::from_io("modules", "reading", None, err))?
        {
            let text = fs::read_to_string(&file).map_err(|err| {
                AppletError::from_io("modules", "reading", Some(&file.to_string_lossy()), err)
            })?;
            parse_modprobe_config_file(&mut config, &file, &text)?;
        }
        Ok(config)
    }

    pub(crate) fn resolve_config_alias<'a>(&'a self, request: &str) -> Option<&'a str> {
        self.aliases.iter().find_map(|alias| {
            fnmatch(&alias.pattern, request)
                .ok()
                .filter(|matches| *matches)
                .map(|_| alias.target.as_str())
        })
    }

    pub(crate) fn is_blacklisted(&self, module: &str) -> bool {
        self.blacklists.contains(&normalize_module_name(module))
    }

    pub(crate) fn module_options(&self, module: &str) -> Vec<String> {
        self.options
            .get(&normalize_module_name(module))
            .cloned()
            .unwrap_or_default()
    }

    pub(crate) fn request_options(&self, request: &str, module: &str) -> Vec<String> {
        let request = normalize_module_name(request);
        let module = normalize_module_name(module);
        let mut options = Vec::new();
        if request != module
            && let Some(values) = self.options.get(&request)
        {
            options.extend(values.clone());
        }
        if let Some(values) = self.options.get(&module) {
            options.extend(values.clone());
        }
        options
    }

    pub(crate) fn install_command(&self, module: &str) -> Option<&str> {
        self.install_commands
            .get(&normalize_module_name(module))
            .map(String::as_str)
    }

    pub(crate) fn remove_command(&self, module: &str) -> Option<&str> {
        self.remove_commands
            .get(&normalize_module_name(module))
            .map(String::as_str)
    }

    pub(crate) fn softdep(&self, module: &str) -> Softdep {
        self.softdeps
            .get(&normalize_module_name(module))
            .cloned()
            .unwrap_or_default()
    }
}

pub(crate) fn module_tree_dir(release_override: Option<&str>) -> Result<PathBuf, AppletError> {
    if let Some(path) = std::env::var_os("SEED_MODULES_DIR") {
        return Ok(PathBuf::from(path));
    }
    let release = match release_override {
        Some(release) => release.to_string(),
        None => kernel_release()?,
    };
    Ok(PathBuf::from("/lib/modules").join(release))
}

pub(crate) fn kernel_release() -> Result<String, AppletError> {
    if let Ok(value) = std::env::var("SEED_MODULE_RELEASE") {
        return Ok(value);
    }

    kernel_release_with(|uts| {
        // SAFETY: `uname` initializes the `utsname` structure on success.
        let rc = unsafe { libc::uname(uts) };
        if rc == 0 {
            Ok(())
        } else {
            Err(io::Error::last_os_error())
        }
    })
}

fn kernel_release_with<F>(uname: F) -> Result<String, AppletError>
where
    F: FnOnce(*mut libc::utsname) -> io::Result<()>,
{
    let mut uts = std::mem::MaybeUninit::<libc::utsname>::uninit();
    uname(uts.as_mut_ptr()).map_err(|err| AppletError::from_syscall("modules", "uname", err))?;
    // SAFETY: successful `uname` wrote a valid `utsname`.
    let uts = unsafe { uts.assume_init() };
    let bytes = uts
        .release
        .iter()
        .copied()
        .take_while(|byte| *byte != 0)
        .map(|byte| byte as u8)
        .collect::<Vec<_>>();
    Ok(String::from_utf8_lossy(&bytes).into_owned())
}

pub(crate) fn read_module_file(path: &Path) -> io::Result<Vec<u8>> {
    let mut file = File::open(path)?;
    let mut data = Vec::new();
    let path_text = path.to_string_lossy();
    if path_text.ends_with(".gz") {
        GzDecoder::new(file).read_to_end(&mut data)?;
    } else if path_text.ends_with(".xz") {
        XzReader::new(file, false).read_to_end(&mut data)?;
    } else if path_text.ends_with(".bz2") {
        BzDecoder::new(file).read_to_end(&mut data)?;
    } else {
        file.read_to_end(&mut data)?;
    }
    Ok(data)
}

pub(crate) fn module_name_from_path(path: &str) -> String {
    let filename = Path::new(path)
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or(path);
    normalize_module_name(filename)
}

pub(crate) fn read_module_metadata(path: &Path) -> io::Result<ModuleMetadata> {
    read_module_file(path).map(|bytes| ModuleMetadata::parse(&bytes))
}

pub(crate) fn normalize_module_name(name: &str) -> String {
    let mut name = name;
    for extension in MODULE_EXTENSIONS {
        if let Some(stripped) = name.strip_suffix(extension) {
            name = stripped;
            break;
        }
    }
    Path::new(name)
        .file_name()
        .and_then(|value| value.to_str())
        .unwrap_or(name)
        .replace('-', "_")
}

fn modprobe_config_files() -> io::Result<Vec<PathBuf>> {
    let dirs = std::env::var_os("SEED_MODPROBE_DIRS")
        .map(|value| std::env::split_paths(&value).collect::<Vec<_>>())
        .unwrap_or_else(|| MODPROBE_CONFIG_DIRS.iter().map(PathBuf::from).collect());
    let mut files = Vec::new();
    let mut seen = HashSet::new();
    for dir in dirs {
        let read_dir = match fs::read_dir(&dir) {
            Ok(read_dir) => read_dir,
            Err(err) if err.kind() == io::ErrorKind::NotFound => continue,
            Err(err) => return Err(err),
        };
        let mut dir_files = read_dir
            .filter_map(|entry| entry.ok())
            .map(|entry| entry.path())
            .filter(|path| path.extension().and_then(|value| value.to_str()) == Some("conf"))
            .collect::<Vec<_>>();
        dir_files.sort();
        for file in dir_files {
            let Some(name) = file.file_name().map(|name| name.to_os_string()) else {
                continue;
            };
            if seen.insert(name) {
                files.push(file);
            }
        }
    }
    Ok(files)
}

fn parse_modprobe_config_file(
    config: &mut ModprobeConfig,
    path: &Path,
    text: &str,
) -> Result<(), AppletError> {
    let mut pending = String::new();
    let mut pending_line = 1;
    for (index, raw_line) in text.lines().enumerate() {
        let line_no = index + 1;
        let trimmed = raw_line.trim_end();
        if pending.is_empty() {
            pending_line = line_no;
        }
        if let Some(prefix) = trimmed.strip_suffix('\\') {
            pending.push_str(prefix);
            pending.push(' ');
            continue;
        }
        pending.push_str(trimmed);
        parse_modprobe_config_line(config, path, pending_line, &pending)?;
        pending.clear();
    }
    if !pending.trim().is_empty() {
        parse_modprobe_config_line(config, path, pending_line, &pending)?;
    }
    Ok(())
}

fn parse_modprobe_config_line(
    config: &mut ModprobeConfig,
    path: &Path,
    line_no: usize,
    line: &str,
) -> Result<(), AppletError> {
    let line = strip_modprobe_comment(line).trim();
    if line.is_empty() {
        return Ok(());
    }
    let mut parts = line.split_whitespace();
    let Some(keyword) = parts.next() else {
        return Ok(());
    };
    match keyword {
        "alias" => {
            let (Some(pattern), Some(target)) = (parts.next(), parts.next()) else {
                return Err(config_parse_error(
                    path,
                    line_no,
                    "alias requires a pattern and target",
                ));
            };
            config.aliases.push(ConfigAlias {
                pattern: pattern.to_string(),
                target: target.to_string(),
            });
        }
        "blacklist" => {
            let Some(module) = parts.next() else {
                return Err(config_parse_error(
                    path,
                    line_no,
                    "blacklist requires a module",
                ));
            };
            config.blacklists.insert(normalize_module_name(module));
        }
        "install" => {
            let Some(module) = parts.next() else {
                return Err(config_parse_error(
                    path,
                    line_no,
                    "install requires a module",
                ));
            };
            let command = parts.collect::<Vec<_>>().join(" ");
            if command.is_empty() {
                return Err(config_parse_error(
                    path,
                    line_no,
                    "install requires a command",
                ));
            }
            config
                .install_commands
                .insert(normalize_module_name(module), command);
        }
        "remove" => {
            let Some(module) = parts.next() else {
                return Err(config_parse_error(
                    path,
                    line_no,
                    "remove requires a module",
                ));
            };
            let command = parts.collect::<Vec<_>>().join(" ");
            if command.is_empty() {
                return Err(config_parse_error(
                    path,
                    line_no,
                    "remove requires a command",
                ));
            }
            config
                .remove_commands
                .insert(normalize_module_name(module), command);
        }
        "options" => {
            let Some(module) = parts.next() else {
                return Err(config_parse_error(
                    path,
                    line_no,
                    "options requires a module",
                ));
            };
            let values = parts.map(str::to_string).collect::<Vec<_>>();
            if values.is_empty() {
                return Err(config_parse_error(
                    path,
                    line_no,
                    "options requires at least one parameter",
                ));
            }
            config
                .options
                .entry(normalize_module_name(module))
                .or_default()
                .extend(values);
        }
        "softdep" => {
            let Some(module) = parts.next() else {
                return Err(config_parse_error(
                    path,
                    line_no,
                    "softdep requires a module",
                ));
            };
            let mut softdep = Softdep::default();
            let mut current = None::<bool>;
            for token in parts {
                match token {
                    "pre:" => current = Some(true),
                    "post:" => current = Some(false),
                    _ => match current {
                        Some(true) => softdep.pre.push(normalize_module_name(token)),
                        Some(false) => softdep.post.push(normalize_module_name(token)),
                        None => {
                            return Err(config_parse_error(
                                path,
                                line_no,
                                format!("softdep dependency '{token}' is missing pre: or post:"),
                            ));
                        }
                    },
                }
            }
            config
                .softdeps
                .insert(normalize_module_name(module), softdep);
        }
        _ => {}
    }
    Ok(())
}

fn config_parse_error(path: &Path, line_no: usize, message: impl Into<String>) -> AppletError {
    AppletError::from_config(
        "modules",
        &format!("parsing {}:{line_no}", path.display()),
        message,
    )
}

fn strip_modprobe_comment(line: &str) -> &str {
    let bytes = line.as_bytes();
    let mut index = 0;
    let mut in_single = false;
    let mut in_double = false;
    while index < bytes.len() {
        match bytes[index] {
            b'\'' if !in_double => in_single = !in_single,
            b'"' if !in_single => in_double = !in_double,
            b'\\' if !in_single => index = index.saturating_add(1),
            b'#' if !in_single && !in_double => return &line[..index],
            _ => {}
        }
        index += 1;
    }
    line
}

pub(crate) fn finit_module(path: &Path, params: &[std::ffi::OsString]) -> Result<(), AppletError> {
    let params = params
        .iter()
        .map(|param| {
            param.to_str().map(ToOwned::to_owned).ok_or_else(|| {
                AppletError::new(
                    "modules",
                    format!("module parameter is invalid unicode: {:?}", param),
                )
            })
        })
        .collect::<Result<Vec<_>, _>>()?;
    if let Some(log_path) = std::env::var_os("SEED_MODULE_ACTION_LOG") {
        let suffix = if params.is_empty() {
            String::new()
        } else {
            format!(" {}", params.join(" "))
        };
        append_log_line(
            Path::new(&log_path),
            &format!("insmod {}{suffix}", path.display()),
        )?;
        return Ok(());
    }

    let file = File::open(path).map_err(|err| {
        AppletError::new(
            "modules",
            format!("can't insert '{}': {err}", path.display()),
        )
    })?;
    let size = file
        .metadata()
        .map_err(|err| {
            AppletError::from_io("modules", "reading", Some(&path.to_string_lossy()), err)
        })?
        .len();
    if size == 0 {
        return Err(AppletError::new("modules", "short read"));
    }

    let params = CString::new(params.join(" "))
        .map_err(|_| AppletError::new("modules", "module parameters contain NUL byte"))?;
    if finit_module_syscall(file.as_raw_fd(), params.as_ptr()).is_ok() {
        return Ok(());
    }

    let image = read_module_file(path).map_err(|err| {
        AppletError::from_io("modules", "reading", Some(&path.to_string_lossy()), err)
    })?;
    if image.is_empty() {
        return Err(AppletError::new("modules", "short read"));
    }
    match init_module_syscall(&image, params.as_ptr()) {
        Ok(()) => Ok(()),
        Err(err) => Err(AppletError::new_with_kind(
            "modules",
            format!("can't insert '{}': {err}", path.display()),
            AppletErrorKind::Syscall,
        )),
    }
}

pub(crate) fn delete_module(module: &str, flags: libc::c_int) -> Result<(), AppletError> {
    if let Some(log_path) = std::env::var_os("SEED_MODULE_ACTION_LOG") {
        append_log_line(Path::new(&log_path), &format!("rmmod {module}"))?;
        return Ok(());
    }

    let module = CString::new(module)
        .map_err(|_| AppletError::new("modules", "module name contains NUL byte"))?;
    delete_module_with(module, flags, delete_module_syscall)
}

pub(crate) fn append_log_line(path: &Path, line: &str) -> Result<(), AppletError> {
    let mut file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
        .map_err(|err| {
            AppletError::from_io("modules", "opening", Some(&path.to_string_lossy()), err)
        })?;
    write_log_line(&mut file, path, line)
}

fn write_log_line<W: Write>(writer: &mut W, path: &Path, line: &str) -> Result<(), AppletError> {
    writer
        .write_all(line.as_bytes())
        .and_then(|_| writer.write_all(b"\n"))
        .map_err(|err| {
            AppletError::from_io("modules", "writing", Some(&path.to_string_lossy()), err)
        })
}

fn delete_module_with<F>(module: CString, flags: libc::c_int, delete: F) -> Result<(), AppletError>
where
    F: FnOnce(*const libc::c_char, libc::c_int) -> io::Result<()>,
{
    let name = module.to_string_lossy().into_owned();
    delete(module.as_ptr(), flags).map_err(|err| {
        AppletError::new_with_kind(
            "modules",
            format!("can't unload module '{name}': {err}"),
            AppletErrorKind::Syscall,
        )
    })
}

fn finit_module_syscall(fd: libc::c_int, params: *const libc::c_char) -> io::Result<()> {
    // SAFETY: file descriptor is valid, `params` is a valid NUL-terminated
    // string, and the last argument is the documented flags field.
    let rc = unsafe { libc::syscall(libc::SYS_finit_module, fd, params, 0) };
    if rc == 0 {
        Ok(())
    } else {
        Err(io::Error::last_os_error())
    }
}

fn init_module_syscall(image: &[u8], params: *const libc::c_char) -> io::Result<()> {
    // SAFETY: `image` points to the module buffer, `params` is a valid
    // NUL-terminated options string, and sizes are passed verbatim.
    let rc = unsafe { libc::syscall(libc::SYS_init_module, image.as_ptr(), image.len(), params) };
    if rc == 0 {
        Ok(())
    } else {
        Err(io::Error::last_os_error())
    }
}

fn delete_module_syscall(module: *const libc::c_char, flags: libc::c_int) -> io::Result<()> {
    // SAFETY: `module` is a valid NUL-terminated string, and `flags` only uses
    // documented delete_module bits.
    let rc = unsafe { libc::syscall(libc::SYS_delete_module, module, flags) };
    if rc == 0 {
        Ok(())
    } else {
        Err(io::Error::last_os_error())
    }
}

fn is_module_path(path: &Path) -> bool {
    let path = path.to_string_lossy();
    MODULE_EXTENSIONS
        .iter()
        .any(|extension| path.ends_with(extension))
}

fn read_builtins(root: &Path) -> io::Result<HashSet<String>> {
    let path = root.join("modules.builtin");
    let text = match fs::read_to_string(path) {
        Ok(text) => text,
        Err(err) if err.kind() == io::ErrorKind::NotFound => return Ok(HashSet::new()),
        Err(err) => return Err(err),
    };
    Ok(text
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .map(module_name_from_path)
        .collect())
}

fn fnmatch(pattern: &str, value: &str) -> Result<bool, ()> {
    let pattern = CString::new(pattern).map_err(|_| ())?;
    let value = CString::new(value).map_err(|_| ())?;
    // SAFETY: both C strings are valid NUL-terminated inputs for `fnmatch`.
    let rc = unsafe { libc::fnmatch(pattern.as_ptr(), value.as_ptr(), 0) };
    Ok(rc == 0)
}

#[cfg(test)]
mod tests {
    use super::{
        ModprobeConfig, ModuleMetadata, delete_module_with, kernel_release_with,
        module_name_from_path, normalize_module_name, parse_modprobe_config_file,
        parse_modprobe_config_line, strip_modprobe_comment, write_log_line,
    };
    use crate::common::error::AppletErrorKind;
    use crate::common::test_env;
    use std::ffi::CString;
    use std::fs;
    use std::io::{self, Write};
    use std::path::{Path, PathBuf};

    fn test_config_path() -> &'static Path {
        Path::new("/etc/modprobe.d/test.conf")
    }

    fn temp_path(label: &str) -> PathBuf {
        std::env::temp_dir().join(format!("seed-modules-{label}-{}", std::process::id()))
    }

    #[test]
    fn parses_modinfo_fields_from_binary_blob() {
        let metadata =
            ModuleMetadata::parse(b"\0license=GPL\0depends=crc32c,libcrc32c\0alias=foo*\0");
        assert_eq!(
            metadata.fields(),
            &[
                (String::from("license"), String::from("GPL")),
                (String::from("depends"), String::from("crc32c,libcrc32c"),),
                (String::from("alias"), String::from("foo*")),
            ]
        );
        assert_eq!(
            metadata.depends(),
            vec![String::from("crc32c"), String::from("libcrc32c")]
        );
    }

    #[test]
    fn normalizes_module_names_and_extensions() {
        assert_eq!(normalize_module_name("e1000e.ko.xz"), "e1000e");
        assert_eq!(
            module_name_from_path("kernel/drivers/net/virtio-net.ko"),
            "virtio_net"
        );
    }

    #[test]
    fn strips_comments_only_outside_quotes() {
        assert_eq!(
            strip_modprobe_comment(r##"install foo echo "# not comment""##),
            r##"install foo echo "# not comment""##
        );
        assert_eq!(
            strip_modprobe_comment("install foo echo '# still command'"),
            "install foo echo '# still command'"
        );
        assert_eq!(
            strip_modprobe_comment(r#"install foo echo \#still-command # comment"#),
            r#"install foo echo \#still-command "#
        );
    }

    #[test]
    fn parses_install_command_with_hash_in_quotes() {
        let mut config = ModprobeConfig::default();
        parse_modprobe_config_line(
            &mut config,
            test_config_path(),
            1,
            r#"install driver printf 'install:#%s\n' "$MODPROBE_MODULE" # comment"#,
        )
        .unwrap();
        assert_eq!(
            config.install_command("driver"),
            Some(r#"printf 'install:#%s\n' "$MODPROBE_MODULE""#),
        );
    }

    #[test]
    fn request_options_merge_alias_and_module_options() {
        let mut config = ModprobeConfig::default();
        parse_modprobe_config_line(
            &mut config,
            test_config_path(),
            1,
            "options netdev speed=1000 duplex=full",
        )
        .unwrap();
        parse_modprobe_config_line(&mut config, test_config_path(), 2, "options driver debug=1")
            .unwrap();
        assert_eq!(
            config.request_options("netdev", "driver"),
            vec![
                String::from("speed=1000"),
                String::from("duplex=full"),
                String::from("debug=1"),
            ]
        );
    }

    #[test]
    fn rejects_malformed_modprobe_config() {
        let mut config = ModprobeConfig::default();
        let err =
            parse_modprobe_config_file(&mut config, test_config_path(), "alias missing-target")
                .unwrap_err();

        assert_eq!(err.kind(), AppletErrorKind::Config);
        assert!(
            err.to_string()
                .contains("parsing /etc/modprobe.d/test.conf:1")
        );
    }

    #[test]
    fn kernel_release_classifies_syscall_failures() {
        let err =
            kernel_release_with(|_| Err(io::Error::from_raw_os_error(libc::EPERM))).unwrap_err();

        assert_eq!(err.kind(), AppletErrorKind::Syscall);
        assert!(err.to_string().contains("uname failed"));
    }

    #[test]
    fn delete_module_classifies_syscall_failures() {
        let module = CString::new("loop").unwrap();
        let err = delete_module_with(module, libc::O_NONBLOCK, |_name, _flags| {
            Err(io::Error::from_raw_os_error(libc::EPERM))
        })
        .unwrap_err();

        assert_eq!(err.kind(), AppletErrorKind::Syscall);
        assert!(err.to_string().contains("can't unload module 'loop'"));
    }

    #[test]
    fn load_reports_unreadable_config_inputs() {
        let _guard = test_env::lock();
        let dir = temp_path("load");
        let broken = dir.join("broken.conf");
        fs::create_dir_all(&broken).unwrap();
        // SAFETY: the test holds the global env lock while mutating process env.
        unsafe { std::env::set_var("SEED_MODPROBE_DIRS", &dir) };

        let err = ModprobeConfig::load().unwrap_err();

        // SAFETY: the test holds the global env lock while mutating process env.
        unsafe { std::env::remove_var("SEED_MODPROBE_DIRS") };
        let _ = fs::remove_dir_all(&dir);

        assert_eq!(err.kind(), AppletErrorKind::Io);
        assert!(err.to_string().contains("broken.conf"));
    }

    #[test]
    fn write_log_line_reports_partial_write_failures() {
        struct PartialWriter {
            written: Vec<u8>,
            calls: usize,
        }

        impl Write for PartialWriter {
            fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
                self.calls += 1;
                if self.calls == 1 {
                    let count = buf.len().min(3);
                    self.written.extend_from_slice(&buf[..count]);
                    Ok(count)
                } else {
                    Err(io::Error::new(
                        io::ErrorKind::BrokenPipe,
                        "simulated failure",
                    ))
                }
            }

            fn flush(&mut self) -> io::Result<()> {
                Ok(())
            }
        }

        let mut writer = PartialWriter {
            written: Vec::new(),
            calls: 0,
        };
        let err =
            write_log_line(&mut writer, Path::new("/tmp/modules.log"), "insmod loop").unwrap_err();

        assert_eq!(err.kind(), AppletErrorKind::Io);
        assert_eq!(writer.written, b"ins");
    }
}
