use std::ffi::CString;
use std::fs;
use std::os::unix::ffi::OsStrExt;
use std::os::unix::fs::{FileTypeExt, MetadataExt};
use std::path::{Path, PathBuf};

use crate::common::applet::{AppletResult, finish};
use crate::common::error::AppletError;
use crate::common::unix::{self, FileKind};

const APPLET: &str = "ls";

#[derive(Clone, Copy, Debug, Default)]
struct Options {
    single_column: bool,
    human_readable: bool,
    long_format: bool,
    show_blocks: bool,
}

#[derive(Debug)]
struct Entry {
    name: String,
    path: PathBuf,
    metadata: fs::Metadata,
}

#[derive(Debug)]
struct LongEntry {
    mode: String,
    links: String,
    owner: String,
    group: String,
    size: String,
    modified: String,
    name: String,
    symlink_target: Option<String>,
}

#[derive(Debug)]
struct Target {
    display: String,
    path: PathBuf,
    metadata: fs::Metadata,
    lists_directory: bool,
}

pub fn main(args: &[String]) -> i32 {
    finish(run(args))
}

fn run(args: &[String]) -> AppletResult {
    let (options, paths) = parse_args(args)?;
    let targets = if paths.is_empty() {
        vec![String::from(".")]
    } else {
        paths
    };

    let (output, errors) = render_targets(&targets, options);
    print!("{output}");

    if errors.is_empty() {
        Ok(())
    } else {
        Err(errors)
    }
}

fn render_targets(targets: &[String], options: Options) -> (String, Vec<AppletError>) {
    let mut files = Vec::new();
    let mut directories = Vec::new();
    let mut errors = Vec::new();

    for path in targets {
        match classify_target(path, options) {
            Ok(target) => {
                if target.lists_directory {
                    directories.push(target);
                } else {
                    files.push(target);
                }
            }
            Err(err) => errors.push(err),
        }
    }

    let mut output = String::new();

    for file in files {
        match render_file(&file.display, &file.path, file.metadata, options) {
            Ok(text) => output.push_str(&text),
            Err(err) => errors.push(err),
        }
    }

    let show_directory_headers = targets.len() > 1;
    for (index, directory) in directories.iter().enumerate() {
        if !output.is_empty() || index > 0 {
            output.push('\n');
        }
        if show_directory_headers {
            output.push_str(&directory.display);
            output.push_str(":\n");
        }
        match render_directory(&directory.path, options) {
            Ok(text) => output.push_str(&text),
            Err(err) => errors.push(err),
        }
    }

    (output, errors)
}

fn parse_args(args: &[String]) -> Result<(Options, Vec<String>), Vec<AppletError>> {
    let mut options = Options::default();
    let mut paths = Vec::new();
    let mut parsing_flags = true;

    for arg in args {
        if parsing_flags && arg == "--" {
            parsing_flags = false;
            continue;
        }
        if parsing_flags && arg.starts_with('-') && arg.len() > 1 {
            for flag in arg[1..].chars() {
                match flag {
                    '1' => options.single_column = true,
                    'h' => options.human_readable = true,
                    'l' => options.long_format = true,
                    's' => options.show_blocks = true,
                    _ => return Err(vec![AppletError::invalid_option(APPLET, flag)]),
                }
            }
            continue;
        }
        paths.push(arg.clone());
    }

    Ok((options, paths))
}

#[cfg(test)]
fn render_target(path: &str, options: Options) -> Result<String, AppletError> {
    let target = classify_target(path, options)?;
    if target.lists_directory {
        render_directory(&target.path, options)
    } else {
        render_file(&target.display, &target.path, target.metadata, options)
    }
}

fn classify_target(path: &str, options: Options) -> Result<Target, AppletError> {
    let path_ref = Path::new(path);
    let metadata = fs::symlink_metadata(path_ref)
        .map_err(|err| AppletError::from_io(APPLET, "reading", Some(path), err))?;
    let lists_directory = should_list_directory(path_ref, &metadata, options);

    Ok(Target {
        display: path.to_owned(),
        path: path_ref.to_path_buf(),
        metadata,
        lists_directory,
    })
}

fn should_list_directory(path: &Path, metadata: &fs::Metadata, options: Options) -> bool {
    metadata.is_dir()
        || (!options.long_format
            && metadata.file_type().is_symlink()
            && fs::metadata(path).is_ok_and(|target| target.is_dir()))
}

fn render_directory(path: &Path, options: Options) -> Result<String, AppletError> {
    let display = path.to_string_lossy();
    let mut entries = Vec::new();

    for entry in fs::read_dir(path)
        .map_err(|err| AppletError::from_io(APPLET, "reading", Some(&display), err))?
    {
        let entry =
            entry.map_err(|err| AppletError::from_io(APPLET, "reading", Some(&display), err))?;
        let name = entry.file_name();
        let name = String::from_utf8_lossy(name.as_os_str().as_bytes()).into_owned();
        let child_path = entry.path();
        let metadata = fs::symlink_metadata(&child_path)
            .map_err(|err| AppletError::from_io(APPLET, "reading", Some(&name), err))?;
        entries.push(Entry {
            name,
            path: child_path,
            metadata,
        });
    }

    entries.sort_by(|left, right| left.name.as_bytes().cmp(right.name.as_bytes()));

    render_entries(&entries, options, true)
}

fn render_file(
    display: &str,
    path: &Path,
    metadata: fs::Metadata,
    options: Options,
) -> Result<String, AppletError> {
    let entry = Entry {
        name: display.to_owned(),
        path: path.to_path_buf(),
        metadata,
    };
    render_entries(&[entry], options, false)
}

fn render_entries(
    entries: &[Entry],
    options: Options,
    show_total: bool,
) -> Result<String, AppletError> {
    let mut output = String::new();

    if show_total && (options.long_format || options.show_blocks) {
        output.push_str(&format!("total {}\n", total_blocks(entries)));
    }

    if options.long_format {
        let long_entries = entries
            .iter()
            .map(|entry| build_long_entry(entry, options))
            .collect::<Result<Vec<_>, _>>()?;
        let widths = LongWidths::from_entries(&long_entries, options);
        for entry in &long_entries {
            output.push_str(&format_long_entry(entry, widths));
        }
    } else {
        for entry in entries {
            if options.show_blocks {
                output.push_str(&format!(
                    "{} {}\n",
                    format_blocks(entry.metadata.blocks(), options.human_readable),
                    entry.name
                ));
            } else {
                let _ = options.single_column;
                output.push_str(&entry.name);
                output.push('\n');
            }
        }
    }

    Ok(output)
}

#[derive(Clone, Copy)]
struct LongWidths {
    links: usize,
    owner: usize,
    group: usize,
    size: usize,
}

impl LongWidths {
    fn from_entries(entries: &[LongEntry], options: Options) -> Self {
        let mut widths = Self {
            links: 1,
            owner: 0,
            group: 0,
            size: if options.human_readable { 6 } else { 3 },
        };

        for entry in entries {
            widths.links = widths.links.max(entry.links.len());
            widths.owner = widths.owner.max(entry.owner.len());
            widths.group = widths.group.max(entry.group.len());
            widths.size = widths.size.max(entry.size.len());
        }

        widths
    }
}

fn build_long_entry(entry: &Entry, options: Options) -> Result<LongEntry, AppletError> {
    let size = if options.human_readable {
        format_human_size(entry.metadata.size())
    } else {
        entry.metadata.size().to_string()
    };
    let symlink_target = if entry.metadata.file_type().is_symlink() {
        fs::read_link(&entry.path)
            .ok()
            .map(|target| target.to_string_lossy().into_owned())
    } else {
        None
    };

    Ok(LongEntry {
        mode: format_mode(entry)?,
        links: entry.metadata.nlink().to_string(),
        owner: unix::lookup_user(entry.metadata.uid()),
        group: unix::lookup_group(entry.metadata.gid()),
        size,
        modified: unix::format_recent_mtime(APPLET, entry.metadata.mtime())?,
        name: entry.name.clone(),
        symlink_target,
    })
}

fn format_long_entry(entry: &LongEntry, widths: LongWidths) -> String {
    let mut line = format!(
        "{} {:>links$} {:owner_width$}  {:group_width$} {:>size_width$} {} {}",
        entry.mode,
        entry.links,
        entry.owner,
        entry.group,
        entry.size,
        entry.modified,
        entry.name,
        links = widths.links,
        owner_width = widths.owner,
        group_width = widths.group,
        size_width = widths.size,
    );
    if let Some(target) = &entry.symlink_target {
        line.push_str(" -> ");
        line.push_str(target);
    }
    line.push('\n');
    line
}

fn total_blocks(entries: &[Entry]) -> u64 {
    entries.iter().map(|entry| entry.metadata.blocks()).sum()
}

fn format_mode(entry: &Entry) -> Result<String, AppletError> {
    Ok(unix::format_mode(
        file_kind(entry.metadata.file_type()),
        entry.metadata.mode(),
        Some(attribute_marker(&entry.path)?),
    ))
}

fn attribute_marker(path: &Path) -> Result<char, AppletError> {
    let bytes = path.as_os_str().as_bytes();
    let path_c = CString::new(bytes)
        .map_err(|_| AppletError::new(APPLET, format!("unsupported path '{}'", path.display())))?;
    #[cfg(target_os = "macos")]
    {
        // TODO: Drop this Darwin xattr arity special-case once the codebase is
        // Linux-only in practice.
        // SAFETY: `path_c` is a valid NUL-terminated path string. Passing a
        // null buffer with size 0 asks `listxattr` for the required size only.
        let size = unsafe { libc::listxattr(path_c.as_ptr(), std::ptr::null_mut(), 0, 0) };
        Ok(if size > 0 { '@' } else { ' ' })
    }
    #[cfg(not(target_os = "macos"))]
    {
        // SAFETY: `path_c` is a valid NUL-terminated path string. Passing a null
        // buffer with size 0 asks `listxattr` for the required size only.
        let size = unsafe { libc::listxattr(path_c.as_ptr(), std::ptr::null_mut(), 0) };
        Ok(if size > 0 { '@' } else { ' ' })
    }
}

fn file_kind(file_type: fs::FileType) -> FileKind {
    if file_type.is_dir() {
        FileKind::Directory
    } else if file_type.is_symlink() {
        FileKind::Symlink
    } else if file_type.is_block_device() {
        FileKind::BlockDevice
    } else if file_type.is_char_device() {
        FileKind::CharDevice
    } else if file_type.is_fifo() {
        FileKind::Fifo
    } else if file_type.is_socket() {
        FileKind::Socket
    } else {
        FileKind::Regular
    }
}

fn format_blocks(blocks: u64, human_readable: bool) -> String {
    if human_readable {
        format_human_size(blocks.saturating_mul(512))
    } else {
        blocks.to_string()
    }
}

fn format_human_size(size: u64) -> String {
    const UNITS: [&str; 6] = ["B", "K", "M", "G", "T", "P"];
    let mut value = size as f64;
    let mut unit = 0;
    while value >= 1024.0 && unit + 1 < UNITS.len() {
        value /= 1024.0;
        unit += 1;
    }
    if unit == 0 {
        format!("{size}B")
    } else if value >= 10.0 {
        format!("{value:.0}{}", UNITS[unit])
    } else {
        format!("{value:.1}{}", UNITS[unit])
    }
}

#[cfg(test)]
mod tests {
    use super::{Options, parse_args, render_target, render_targets};
    use crate::common::unix;
    use std::fs;
    use std::path::PathBuf;

    struct TempDir {
        path: PathBuf,
    }

    impl TempDir {
        fn new() -> Self {
            Self {
                path: unix::temp_dir("ls-test"),
            }
        }

        fn path(&self) -> &PathBuf {
            &self.path
        }
    }

    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.path);
        }
    }

    #[test]
    fn long_listing_for_file_operand_does_not_print_total() {
        let tempdir = TempDir::new();
        let file = tempdir.path().join("file");
        fs::write(&file, b"x").expect("write file");

        let output = render_target(
            file.to_str().expect("utf8 path"),
            Options {
                long_format: true,
                ..Options::default()
            },
        )
        .expect("render long file");

        assert!(!output.starts_with("total "));
    }

    #[test]
    fn block_listing_for_file_operand_does_not_print_total() {
        let tempdir = TempDir::new();
        let file = tempdir.path().join("file");
        fs::write(&file, b"x").expect("write file");

        let output = render_target(
            file.to_str().expect("utf8 path"),
            Options {
                show_blocks: true,
                ..Options::default()
            },
        )
        .expect("render block file");

        assert!(!output.starts_with("total "));
    }

    #[test]
    fn multiple_directory_operands_print_headers_and_blank_lines() {
        let tempdir = TempDir::new();
        let dir1 = tempdir.path().join("dir1");
        let dir2 = tempdir.path().join("dir2");
        fs::create_dir(&dir1).expect("create dir1");
        fs::create_dir(&dir2).expect("create dir2");
        fs::write(dir1.join("a"), b"a").expect("write dir1 file");
        fs::write(dir2.join("b"), b"b").expect("write dir2 file");

        let output = render_targets(
            &[
                dir1.to_str().expect("utf8 path").to_owned(),
                dir2.to_str().expect("utf8 path").to_owned(),
            ],
            Options {
                single_column: true,
                ..Options::default()
            },
        );

        let expected = format!("{}:\na\n\n{}:\nb\n", dir1.display(), dir2.display(),);
        assert!(output.1.is_empty(), "unexpected errors: {:?}", output.1);
        assert_eq!(output.0, expected);
    }

    #[test]
    fn file_operands_are_listed_before_directory_operands() {
        let tempdir = TempDir::new();
        let dir = tempdir.path().join("dir");
        let file = tempdir.path().join("file");
        fs::create_dir(&dir).expect("create dir");
        fs::write(dir.join("inside"), b"x").expect("write dir file");
        fs::write(&file, b"y").expect("write file");

        let output = render_targets(
            &[
                dir.to_str().expect("utf8 path").to_owned(),
                file.to_str().expect("utf8 path").to_owned(),
            ],
            Options {
                single_column: true,
                ..Options::default()
            },
        );

        let expected = format!("{}\n\n{}:\ninside\n", file.display(), dir.display());
        assert!(output.1.is_empty(), "unexpected errors: {:?}", output.1);
        assert_eq!(output.0, expected);
    }

    #[test]
    fn double_dash_stops_option_parsing() {
        let (options, paths) = parse_args(&["--".to_owned(), "-file".to_owned()]).expect("parse");

        assert!(!options.long_format);
        assert_eq!(paths, vec!["-file"]);
    }

    #[test]
    fn symlink_to_directory_lists_directory_contents() {
        let tempdir = TempDir::new();
        let dir = tempdir.path().join("dir");
        let link = tempdir.path().join("link");
        fs::create_dir(&dir).expect("create dir");
        fs::write(dir.join("A"), b"x").expect("write A");
        fs::write(dir.join("B"), b"y").expect("write B");
        std::os::unix::fs::symlink(&dir, &link).expect("create symlink");

        let output = render_target(link.to_str().expect("utf8 path"), Options::default())
            .expect("render symlink target");

        assert_eq!(output, "A\nB\n");
    }

    #[test]
    fn long_listing_for_symlink_to_directory_shows_symlink() {
        let tempdir = TempDir::new();
        let dir = tempdir.path().join("dir");
        let link = tempdir.path().join("link");
        fs::create_dir(&dir).expect("create dir");
        fs::write(dir.join("A"), b"x").expect("write A");
        std::os::unix::fs::symlink(&dir, &link).expect("create symlink");

        let output = render_target(
            link.to_str().expect("utf8 path"),
            Options {
                long_format: true,
                ..Options::default()
            },
        )
        .expect("render long symlink");

        assert!(output.contains(" -> "));
        assert!(!output.starts_with("total "));
        assert!(!output.contains("\nA\n"));
    }
}
