use std::ffi::CString;
use std::fs;
use std::os::unix::ffi::OsStrExt;
use std::os::unix::fs::{FileTypeExt, MetadataExt};
use std::path::{Path, PathBuf};

use crate::common::applet::{AppletResult, finish};
use crate::common::args::argv_to_strings;
use crate::common::error::AppletError;
use crate::common::unix::{self, FileKind};

const APPLET: &str = "ls";
const SHORT_HELP: &str = "\
ls - list directory contents

usage: ls [OPTIONS] [FILE...]

Options:
  -1  one entry per line
  -a  include entries starting with .
  -A  include hidden entries except . and ..
  -d  list directories themselves, not their contents
  -F  append file type indicators
  -g  long format without owner
  -h  human-readable sizes with -l
  -i  show inode numbers
  -l  long format
  -n  long format with numeric uid/gid
  -o  long format without group
  -p  append / to directories
  -R  recurse into subdirectories
  -r  reverse sort order
  -s  show allocated blocks
  -S  sort by size
  -t  sort by modification time
  -U  do not sort

Try 'man ls' for details.
";

#[derive(Clone, Copy, Debug, Default)]
struct Options {
    hidden_mode: HiddenMode,
    indicator_style: IndicatorStyle,
    list_directory_itself: bool,
    numeric_ids: bool,
    omit_group: bool,
    omit_owner: bool,
    recursive: bool,
    reverse_sort: bool,
    show_inode: bool,
    single_column: bool,
    human_readable: bool,
    long_format: bool,
    show_blocks: bool,
    sort_mode: SortMode,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
enum HiddenMode {
    #[default]
    Omit,
    AlmostAll,
    All,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
enum SortMode {
    #[default]
    Name,
    Time,
    Size,
    Unsorted,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
enum IndicatorStyle {
    #[default]
    None,
    Slash,
    Classify,
}

#[derive(Debug)]
struct Entry {
    name: String,
    path: PathBuf,
    metadata: fs::Metadata,
}

#[derive(Debug)]
struct LongEntry {
    inode: String,
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

pub fn main(args: &[std::ffi::OsString]) -> i32 {
    finish(run(args))
}

pub(crate) fn short_help() -> &'static str {
    SHORT_HELP
}

fn run(args: &[std::ffi::OsString]) -> AppletResult {
    let args = argv_to_strings(APPLET, args)?;
    let (options, paths) = parse_args(&args)?;
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

    if !files.is_empty() {
        let mut entries = files
            .into_iter()
            .map(|file| Entry {
                name: file.display,
                path: file.path,
                metadata: file.metadata,
            })
            .collect::<Vec<_>>();
        sort_entries(&mut entries, options);
        match render_entries(&entries, options, false) {
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
                    'A' => options.hidden_mode = HiddenMode::AlmostAll,
                    'F' => options.indicator_style = IndicatorStyle::Classify,
                    'S' => options.sort_mode = SortMode::Size,
                    'U' => options.sort_mode = SortMode::Unsorted,
                    'a' => options.hidden_mode = HiddenMode::All,
                    'd' => options.list_directory_itself = true,
                    'g' => {
                        options.long_format = true;
                        options.omit_owner = true;
                    }
                    'h' => options.human_readable = true,
                    'i' => options.show_inode = true,
                    'l' => options.long_format = true,
                    'n' => {
                        options.long_format = true;
                        options.numeric_ids = true;
                    }
                    'o' => {
                        options.long_format = true;
                        options.omit_group = true;
                    }
                    'p' => options.indicator_style = IndicatorStyle::Slash,
                    'R' => options.recursive = true,
                    'r' => options.reverse_sort = true,
                    's' => options.show_blocks = true,
                    't' => options.sort_mode = SortMode::Time,
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
        render_entries(
            &[Entry {
                name: target.display,
                path: target.path,
                metadata: target.metadata,
            }],
            options,
            false,
        )
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
    if options.list_directory_itself {
        return false;
    }
    metadata.is_dir()
        || (!options.long_format
            && metadata.file_type().is_symlink()
            && fs::metadata(path).is_ok_and(|target| target.is_dir()))
}

fn render_directory(path: &Path, options: Options) -> Result<String, AppletError> {
    if options.recursive {
        return render_directory_recursive(path, options);
    }
    let entries = read_directory_entries(path, options)?;
    render_entries(&entries, options, true)
}

fn read_directory_entries(path: &Path, options: Options) -> Result<Vec<Entry>, AppletError> {
    let display = path.to_string_lossy();
    let mut entries = Vec::new();

    if options.hidden_mode == HiddenMode::All {
        entries.push(special_entry(path, ".", path)?);
        entries.push(special_entry(path, "..", &path.join(".."))?);
    }

    for entry in fs::read_dir(path)
        .map_err(|err| AppletError::from_io(APPLET, "reading", Some(&display), err))?
    {
        let entry =
            entry.map_err(|err| AppletError::from_io(APPLET, "reading", Some(&display), err))?;
        let name = entry.file_name();
        let name = String::from_utf8_lossy(name.as_os_str().as_bytes()).into_owned();
        if !should_include_name(&name, options.hidden_mode) {
            continue;
        }
        let child_path = entry.path();
        let metadata = fs::symlink_metadata(&child_path)
            .map_err(|err| AppletError::from_io(APPLET, "reading", Some(&name), err))?;
        entries.push(Entry {
            name,
            path: child_path,
            metadata,
        });
    }

    sort_entries(&mut entries, options);

    Ok(entries)
}

fn render_directory_recursive(path: &Path, options: Options) -> Result<String, AppletError> {
    let mut sections = Vec::new();
    collect_directory_sections(path, &path.to_string_lossy(), options, &mut sections)?;
    Ok(sections.join("\n"))
}

fn collect_directory_sections(
    path: &Path,
    display: &str,
    options: Options,
    sections: &mut Vec<String>,
) -> Result<(), AppletError> {
    let entries = read_directory_entries(path, options)?;
    let mut section = String::new();
    section.push_str(display);
    section.push_str(":\n");
    section.push_str(&render_entries(&entries, options, true)?);
    sections.push(section);

    for entry in entries {
        if !entry.metadata.is_dir() || entry.name == "." || entry.name == ".." {
            continue;
        }
        collect_directory_sections(&entry.path, &entry.path.to_string_lossy(), options, sections)?;
    }

    Ok(())
}

fn special_entry(base: &Path, name: &str, target: &Path) -> Result<Entry, AppletError> {
    let metadata = fs::symlink_metadata(target)
        .map_err(|err| AppletError::from_io(APPLET, "reading", Some(&base.to_string_lossy()), err))?;
    Ok(Entry {
        name: name.to_owned(),
        path: target.to_path_buf(),
        metadata,
    })
}

fn should_include_name(name: &str, hidden_mode: HiddenMode) -> bool {
    match hidden_mode {
        HiddenMode::Omit => !name.starts_with('.'),
        HiddenMode::AlmostAll | HiddenMode::All => true,
    }
}

fn sort_entries(entries: &mut [Entry], options: Options) {
    match options.sort_mode {
        SortMode::Name => entries.sort_by(compare_by_name),
        SortMode::Time => entries.sort_by(compare_by_time),
        SortMode::Size => entries.sort_by(compare_by_size),
        SortMode::Unsorted => {}
    }

    if options.reverse_sort {
        entries.reverse();
    }
}

fn compare_by_name(left: &Entry, right: &Entry) -> std::cmp::Ordering {
    left.name.as_bytes().cmp(right.name.as_bytes())
}

fn compare_by_time(left: &Entry, right: &Entry) -> std::cmp::Ordering {
    right
        .metadata
        .mtime()
        .cmp(&left.metadata.mtime())
        .then_with(|| right.metadata.mtime_nsec().cmp(&left.metadata.mtime_nsec()))
        .then_with(|| compare_by_name(left, right))
}

fn compare_by_size(left: &Entry, right: &Entry) -> std::cmp::Ordering {
    right
        .metadata
        .size()
        .cmp(&left.metadata.size())
        .then_with(|| compare_by_name(left, right))
}

fn display_name(entry: &Entry, options: Options) -> String {
    let mut name = entry.name.clone();
    if let Some(indicator) = indicator_suffix(entry, options.indicator_style) {
        name.push(indicator);
    }
    name
}

fn indicator_suffix(entry: &Entry, style: IndicatorStyle) -> Option<char> {
    match style {
        IndicatorStyle::None => None,
        IndicatorStyle::Slash => entry.metadata.is_dir().then_some('/'),
        IndicatorStyle::Classify => {
            if entry.metadata.is_dir() {
                Some('/')
            } else if entry.metadata.file_type().is_symlink() {
                Some('@')
            } else if entry.metadata.file_type().is_fifo() {
                Some('|')
            } else if entry.metadata.file_type().is_socket() {
                Some('=')
            } else if entry.metadata.is_file() && entry.metadata.mode() & 0o111 != 0 {
                Some('*')
            } else {
                None
            }
        }
    }
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
        let inode_width = if options.show_inode {
            inode_width(entries)
        } else {
            0
        };
        for entry in entries {
            if inode_width > 0 {
                output.push_str(&format!(
                    "{:>inode_width$} ",
                    entry.metadata.ino(),
                    inode_width = inode_width
                ));
            }
            if options.show_blocks {
                output.push_str(&format!(
                    "{} {}\n",
                    format_blocks(entry.metadata.blocks(), options.human_readable),
                    display_name(entry, options)
                ));
            } else {
                let _ = options.single_column;
                output.push_str(&display_name(entry, options));
                output.push('\n');
            }
        }
    }

    Ok(output)
}

#[derive(Clone, Copy)]
struct LongWidths {
    inode: usize,
    links: usize,
    owner: usize,
    group: usize,
    size: usize,
}

impl LongWidths {
    fn from_entries(entries: &[LongEntry], options: Options) -> Self {
        let mut widths = Self {
            inode: 0,
            links: 1,
            owner: 0,
            group: 0,
            size: 0,
        };

        for entry in entries {
            if options.show_inode {
                widths.inode = widths.inode.max(entry.inode.len());
            }
            widths.links = widths.links.max(entry.links.len());
            if !entry.owner.is_empty() {
                widths.owner = widths.owner.max(entry.owner.len());
            }
            if !entry.group.is_empty() {
                widths.group = widths.group.max(entry.group.len());
            }
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

    let owner = if options.omit_owner {
        String::new()
    } else if options.numeric_ids {
        entry.metadata.uid().to_string()
    } else {
        unix::lookup_user(entry.metadata.uid())
    };
    let group = if options.omit_group {
        String::new()
    } else if options.numeric_ids {
        entry.metadata.gid().to_string()
    } else {
        unix::lookup_group(entry.metadata.gid())
    };

    Ok(LongEntry {
        inode: entry.metadata.ino().to_string(),
        mode: format_mode(entry)?,
        links: entry.metadata.nlink().to_string(),
        owner,
        group,
        size,
        modified: unix::format_recent_mtime(APPLET, entry.metadata.mtime())?,
        name: display_name(entry, options),
        symlink_target,
    })
}

fn format_long_entry(entry: &LongEntry, widths: LongWidths) -> String {
    let mut line = String::new();
    if widths.inode > 0 {
        line.push_str(&format!("{:>inode_width$} ", entry.inode, inode_width = widths.inode));
    }
    line.push_str(&entry.mode);
    line.push(' ');
    line.push_str(&format!("{:>links$}", entry.links, links = widths.links));
    if widths.owner > 0 {
        line.push(' ');
        line.push_str(&format!("{:owner_width$}", entry.owner, owner_width = widths.owner));
    }
    if widths.group > 0 {
        line.push(' ');
        line.push_str(&format!("{:group_width$}", entry.group, group_width = widths.group));
    }
    line.push(' ');
    line.push_str(&format!("{:>size_width$}", entry.size, size_width = widths.size));
    line.push(' ');
    line.push_str(&entry.modified);
    line.push(' ');
    line.push_str(&entry.name);
    if let Some(target) = &entry.symlink_target {
        line.push_str(" -> ");
        line.push_str(target);
    }
    line.push('\n');
    line
}

fn total_blocks(entries: &[Entry]) -> u64 {
    entries
        .iter()
        .map(|entry| display_blocks(entry.metadata.blocks()))
        .sum()
}

fn format_mode(entry: &Entry) -> Result<String, AppletError> {
    Ok(unix::format_mode(
        file_kind(entry.metadata.file_type()),
        entry.metadata.mode(),
        attribute_marker(&entry.path)?,
    ))
}

fn attribute_marker(path: &Path) -> Result<Option<char>, AppletError> {
    let bytes = path.as_os_str().as_bytes();
    let path_c = CString::new(bytes)
        .map_err(|_| AppletError::new(APPLET, format!("unsupported path '{}'", path.display())))?;
    #[cfg(not(target_os = "macos"))]
    {
        // SAFETY: `path_c` is a valid NUL-terminated path string. Passing a null
        // buffer with size 0 asks `listxattr` for the required size only.
        let size = unsafe { libc::listxattr(path_c.as_ptr(), std::ptr::null_mut(), 0) };
        Ok(if size > 0 { Some('@') } else { None })
    }
    #[cfg(target_os = "macos")]
    {
        let _ = path_c;
        Ok(None)
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
    let blocks = display_blocks(blocks);
    if human_readable {
        format_human_size(blocks.saturating_mul(1024))
    } else {
        blocks.to_string()
    }
}

fn display_blocks(blocks: u64) -> u64 {
    blocks.div_ceil(2)
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
        size.to_string()
    } else if value >= 10.0 {
        format!("{value:.0}{}", UNITS[unit])
    } else {
        format!("{value:.1}{}", UNITS[unit])
    }
}

fn inode_width(entries: &[Entry]) -> usize {
    entries
        .iter()
        .map(|entry| entry.metadata.ino().to_string().len())
        .max()
        .unwrap_or(1)
}

#[cfg(test)]
mod tests {
    use super::{
        HiddenMode, IndicatorStyle, Options, SortMode, parse_args, render_target, render_targets,
    };
    use crate::common::unix;
    use std::fs;
    use std::os::unix::ffi::OsStrExt;
    use std::os::unix::fs::MetadataExt;
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

    #[cfg(target_os = "linux")]
    fn set_mtime(path: &std::path::Path, seconds: i64) {
        let times = [
            libc::timespec {
                tv_sec: seconds,
                tv_nsec: 0,
            },
            libc::timespec {
                tv_sec: seconds,
                tv_nsec: 0,
            },
        ];
        let path = std::ffi::CString::new(path.as_os_str().as_bytes()).expect("path cstring");
        // SAFETY: `path` is a valid nul-terminated path and `times` points to two valid timespecs.
        let rc = unsafe { libc::utimensat(libc::AT_FDCWD, path.as_ptr(), times.as_ptr(), 0) };
        assert_eq!(rc, 0, "utimensat failed: {}", std::io::Error::last_os_error());
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
    fn long_format_uses_shared_widths_for_multiple_file_operands() {
        let tempdir = TempDir::new();
        let small = tempdir.path().join("a");
        let link = tempdir.path().join("link");
        fs::write(&small, b"x").expect("write small file");
        std::os::unix::fs::symlink(&small, &link).expect("create symlink");

        let (output, errors) = render_targets(
            &[
                small.to_str().expect("utf8 path").to_owned(),
                link.to_str().expect("utf8 path").to_owned(),
            ],
            Options {
                long_format: true,
                ..Options::default()
            },
        );

        assert!(errors.is_empty(), "unexpected errors: {:?}", errors);
        let lines = output.lines().collect::<Vec<_>>();
        assert_eq!(lines.len(), 2);
        assert!(lines[0].contains(" 1 "));
        assert!(lines[1].contains(" -> "));
    }

    #[test]
    fn double_dash_stops_option_parsing() {
        let (options, paths) = parse_args(&["--".to_owned(), "-file".to_owned()]).expect("parse");

        assert!(!options.long_format);
        assert_eq!(paths, vec!["-file"]);
    }

    #[test]
    fn parses_directory_flag() {
        let (options, paths) = parse_args(&["-d".to_owned(), "dir".to_owned()]).expect("parse");

        assert!(options.list_directory_itself);
        assert_eq!(paths, vec!["dir"]);
    }

    #[test]
    fn parses_hidden_flags() {
        let (options, paths) = parse_args(&["-A".to_owned(), "-a".to_owned(), "dir".to_owned()])
            .expect("parse");

        assert_eq!(options.hidden_mode, HiddenMode::All);
        assert_eq!(paths, vec!["dir"]);
    }

    #[test]
    fn parses_sort_flags() {
        let (options, paths) =
            parse_args(&["-Urt".to_owned(), "dir".to_owned()]).expect("parse");

        assert_eq!(options.sort_mode, SortMode::Time);
        assert!(options.reverse_sort);
        assert_eq!(paths, vec!["dir"]);
    }

    #[test]
    fn parses_recursive_flag() {
        let (options, paths) = parse_args(&["-R".to_owned(), "dir".to_owned()]).expect("parse");

        assert!(options.recursive);
        assert_eq!(paths, vec!["dir"]);
    }

    #[test]
    fn parses_indicator_flags() {
        let (options, paths) =
            parse_args(&["-pF".to_owned(), "dir".to_owned()]).expect("parse");

        assert_eq!(options.indicator_style, IndicatorStyle::Classify);
        assert_eq!(paths, vec!["dir"]);
    }

    #[test]
    fn parses_inode_flag() {
        let (options, paths) = parse_args(&["-i".to_owned(), "dir".to_owned()]).expect("parse");

        assert!(options.show_inode);
        assert_eq!(paths, vec!["dir"]);
    }

    #[test]
    fn parses_long_format_variants() {
        let (options, paths) =
            parse_args(&["-ngo".to_owned(), "dir".to_owned()]).expect("parse");

        assert!(options.long_format);
        assert!(options.numeric_ids);
        assert!(options.omit_owner);
        assert!(options.omit_group);
        assert_eq!(paths, vec!["dir"]);
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
    fn default_directory_listing_omits_hidden_entries() {
        let tempdir = TempDir::new();
        let dir = tempdir.path().join("dir");
        fs::create_dir(&dir).expect("create dir");
        fs::write(dir.join("A"), b"x").expect("write A");
        fs::write(dir.join(".hidden"), b"y").expect("write hidden");

        let output = render_target(dir.to_str().expect("utf8 path"), Options::default())
            .expect("render dir");

        assert_eq!(output, "A\n");
    }

    #[test]
    fn almost_all_listing_includes_hidden_entries_without_dot_and_dotdot() {
        let tempdir = TempDir::new();
        let dir = tempdir.path().join("dir");
        fs::create_dir(&dir).expect("create dir");
        fs::write(dir.join("A"), b"x").expect("write A");
        fs::write(dir.join(".hidden"), b"y").expect("write hidden");

        let output = render_target(
            dir.to_str().expect("utf8 path"),
            Options {
                hidden_mode: HiddenMode::AlmostAll,
                ..Options::default()
            },
        )
        .expect("render dir");

        assert_eq!(output, ".hidden\nA\n");
    }

    #[test]
    fn all_listing_includes_dot_and_dotdot() {
        let tempdir = TempDir::new();
        let dir = tempdir.path().join("dir");
        fs::create_dir(&dir).expect("create dir");
        fs::write(dir.join("A"), b"x").expect("write A");
        fs::write(dir.join(".hidden"), b"y").expect("write hidden");

        let output = render_target(
            dir.to_str().expect("utf8 path"),
            Options {
                hidden_mode: HiddenMode::All,
                ..Options::default()
            },
        )
        .expect("render dir");

        assert_eq!(output, ".\n..\n.hidden\nA\n");
    }

    #[test]
    fn directory_listing_sorts_by_size() {
        let tempdir = TempDir::new();
        let dir = tempdir.path().join("dir");
        fs::create_dir(&dir).expect("create dir");
        fs::write(dir.join("small"), b"x").expect("write small");
        fs::write(dir.join("large"), b"12345").expect("write large");

        let output = render_target(
            dir.to_str().expect("utf8 path"),
            Options {
                sort_mode: SortMode::Size,
                ..Options::default()
            },
        )
        .expect("render dir");

        assert_eq!(output, "large\nsmall\n");
    }

    #[test]
    fn directory_listing_reverse_sorts_by_name() {
        let tempdir = TempDir::new();
        let dir = tempdir.path().join("dir");
        fs::create_dir(&dir).expect("create dir");
        fs::write(dir.join("a"), b"x").expect("write a");
        fs::write(dir.join("b"), b"y").expect("write b");

        let output = render_target(
            dir.to_str().expect("utf8 path"),
            Options {
                reverse_sort: true,
                ..Options::default()
            },
        )
        .expect("render dir");

        assert_eq!(output, "b\na\n");
    }

    #[test]
    #[cfg(target_os = "linux")]
    fn directory_listing_sorts_by_time() {
        let tempdir = TempDir::new();
        let dir = tempdir.path().join("dir");
        let old = dir.join("old");
        let new = dir.join("new");
        fs::create_dir(&dir).expect("create dir");
        fs::write(&old, b"x").expect("write old");
        fs::write(&new, b"y").expect("write new");
        set_mtime(&old, 1);
        set_mtime(&new, 2);

        let output = render_target(
            dir.to_str().expect("utf8 path"),
            Options {
                sort_mode: SortMode::Time,
                ..Options::default()
            },
        )
        .expect("render dir");

        assert_eq!(output, "new\nold\n");
    }

    #[test]
    fn unsorted_file_operands_preserve_input_order() {
        let tempdir = TempDir::new();
        let a = tempdir.path().join("a");
        let b = tempdir.path().join("b");
        fs::write(&a, b"x").expect("write a");
        fs::write(&b, b"y").expect("write b");

        let (output, errors) = render_targets(
            &[
                b.to_str().expect("utf8 path").to_owned(),
                a.to_str().expect("utf8 path").to_owned(),
            ],
            Options {
                sort_mode: SortMode::Unsorted,
                ..Options::default()
            },
        );

        assert!(errors.is_empty(), "unexpected errors: {:?}", errors);
        assert_eq!(output, format!("{}\n{}\n", b.display(), a.display()));
    }

    #[test]
    fn directory_flag_lists_directory_operand_itself() {
        let tempdir = TempDir::new();
        let dir = tempdir.path().join("dir");
        fs::create_dir(&dir).expect("create dir");
        fs::write(dir.join("A"), b"x").expect("write A");

        let output = render_target(
            dir.to_str().expect("utf8 path"),
            Options {
                list_directory_itself: true,
                ..Options::default()
            },
        )
        .expect("render dir with -d");

        assert_eq!(output, format!("{}\n", dir.display()));
    }

    #[test]
    fn directory_flag_lists_symlink_to_directory_itself() {
        let tempdir = TempDir::new();
        let dir = tempdir.path().join("dir");
        let link = tempdir.path().join("link");
        fs::create_dir(&dir).expect("create dir");
        fs::write(dir.join("A"), b"x").expect("write A");
        std::os::unix::fs::symlink(&dir, &link).expect("create symlink");

        let output = render_target(
            link.to_str().expect("utf8 path"),
            Options {
                list_directory_itself: true,
                ..Options::default()
            },
        )
        .expect("render symlink with -d");

        assert_eq!(output, format!("{}\n", link.display()));
    }

    #[test]
    fn recursive_listing_prints_headers_for_root_and_children() {
        let tempdir = TempDir::new();
        let dir = tempdir.path().join("dir");
        let sub = dir.join("sub");
        fs::create_dir(&dir).expect("create dir");
        fs::create_dir(&sub).expect("create sub");
        fs::write(dir.join("root-file"), b"x").expect("write root file");
        fs::write(sub.join("child-file"), b"y").expect("write child file");

        let output = render_target(
            dir.to_str().expect("utf8 path"),
            Options {
                recursive: true,
                single_column: true,
                ..Options::default()
            },
        )
        .expect("render recursive dir");

        let expected = format!(
            "{}:\nroot-file\nsub\n\n{}:\nchild-file\n",
            dir.display(),
            sub.display()
        );
        assert_eq!(output, expected);
    }

    #[test]
    fn recursive_listing_does_not_descend_into_symlinked_directories() {
        let tempdir = TempDir::new();
        let dir = tempdir.path().join("dir");
        let sub = dir.join("sub");
        let link = dir.join("link");
        fs::create_dir(&dir).expect("create dir");
        fs::create_dir(&sub).expect("create sub");
        fs::write(sub.join("child-file"), b"y").expect("write child file");
        std::os::unix::fs::symlink(&sub, &link).expect("create symlink");

        let output = render_target(
            dir.to_str().expect("utf8 path"),
            Options {
                recursive: true,
                single_column: true,
                ..Options::default()
            },
        )
        .expect("render recursive dir");

        assert!(output.contains(&format!("{}:\n", dir.display())));
        assert!(output.contains(&format!("{}:\nchild-file\n", sub.display())));
        assert!(!output.contains(&format!("{}:\n", link.display())));
    }

    #[test]
    fn slash_indicator_marks_directories_only() {
        let tempdir = TempDir::new();
        let dir = tempdir.path().join("dir");
        let file = tempdir.path().join("file");
        fs::create_dir(&dir).expect("create dir");
        fs::write(&file, b"x").expect("write file");

        let (output, errors) = render_targets(
            &[
                dir.to_str().expect("utf8 path").to_owned(),
                file.to_str().expect("utf8 path").to_owned(),
            ],
            Options {
                list_directory_itself: true,
                indicator_style: IndicatorStyle::Slash,
                sort_mode: SortMode::Unsorted,
                ..Options::default()
            },
        );

        assert!(errors.is_empty(), "unexpected errors: {:?}", errors);
        assert_eq!(output, format!("{}/\n{}\n", dir.display(), file.display()));
    }

    #[test]
    fn classify_indicator_marks_common_file_types() {
        let tempdir = TempDir::new();
        let dir = tempdir.path().join("dir");
        let exec = tempdir.path().join("exec");
        let link = tempdir.path().join("link");
        let fifo = tempdir.path().join("fifo");
        fs::create_dir(&dir).expect("create dir");
        fs::write(&exec, b"#!/bin/sh\n").expect("write exec");
        let mut perms = fs::metadata(&exec).expect("exec metadata").permissions();
        use std::os::unix::fs::PermissionsExt;
        perms.set_mode(0o755);
        fs::set_permissions(&exec, perms).expect("set exec perms");
        std::os::unix::fs::symlink(&exec, &link).expect("create symlink");
        let fifo_c = std::ffi::CString::new(fifo.as_os_str().as_bytes()).expect("fifo path");
        // SAFETY: `fifo_c` is a valid nul-terminated path for mkfifo.
        let rc = unsafe { libc::mkfifo(fifo_c.as_ptr(), 0o644) };
        assert_eq!(rc, 0, "mkfifo failed: {}", std::io::Error::last_os_error());

        let (output, errors) = render_targets(
            &[
                dir.to_str().expect("utf8 path").to_owned(),
                exec.to_str().expect("utf8 path").to_owned(),
                link.to_str().expect("utf8 path").to_owned(),
                fifo.to_str().expect("utf8 path").to_owned(),
            ],
            Options {
                list_directory_itself: true,
                indicator_style: IndicatorStyle::Classify,
                sort_mode: SortMode::Unsorted,
                ..Options::default()
            },
        );

        assert!(errors.is_empty(), "unexpected errors: {:?}", errors);
        assert_eq!(
            output,
            format!(
                "{}/\n{}*\n{}@\n{}|\n",
                dir.display(),
                exec.display(),
                link.display(),
                fifo.display()
            )
        );
    }

    #[test]
    fn inode_flag_prefixes_plain_entries() {
        let tempdir = TempDir::new();
        let file = tempdir.path().join("file");
        fs::write(&file, b"x").expect("write file");

        let output = render_target(
            file.to_str().expect("utf8 path"),
            Options {
                show_inode: true,
                ..Options::default()
            },
        )
        .expect("render inode file");

        assert!(output.ends_with(&format!(" {}\n", file.display())));
        assert!(output.split_whitespace().next().is_some());
    }

    #[test]
    fn inode_flag_prefixes_long_entries() {
        let tempdir = TempDir::new();
        let file = tempdir.path().join("file");
        fs::write(&file, b"x").expect("write file");

        let output = render_target(
            file.to_str().expect("utf8 path"),
            Options {
                show_inode: true,
                long_format: true,
                ..Options::default()
            },
        )
        .expect("render long inode file");

        let first = output.split_whitespace().next().expect("inode column");
        assert!(first.chars().all(|ch| ch.is_ascii_digit()));
        assert!(output.contains(&file.display().to_string()));
    }

    #[test]
    fn numeric_ids_use_uid_and_gid_columns() {
        let tempdir = TempDir::new();
        let file = tempdir.path().join("file");
        fs::write(&file, b"x").expect("write file");
        let metadata = fs::metadata(&file).expect("metadata");

        let output = render_target(
            file.to_str().expect("utf8 path"),
            Options {
                long_format: true,
                numeric_ids: true,
                ..Options::default()
            },
        )
        .expect("render numeric ids");

        assert!(output.contains(&format!(" {} ", metadata.uid())));
        assert!(output.contains(&format!(" {} ", metadata.gid())));
    }

    #[test]
    fn group_variant_omits_owner_column() {
        let tempdir = TempDir::new();
        let file = tempdir.path().join("file");
        fs::write(&file, b"x").expect("write file");
        let metadata = fs::metadata(&file).expect("metadata");

        let output = render_target(
            file.to_str().expect("utf8 path"),
            Options {
                long_format: true,
                omit_owner: true,
                numeric_ids: true,
                ..Options::default()
            },
        )
        .expect("render -g");

        assert!(!output.contains(&format!(" {} {} ", metadata.uid(), metadata.gid())));
        assert!(output.contains(&format!(" {} ", metadata.gid())));
    }

    #[test]
    fn owner_variant_omits_group_column() {
        let tempdir = TempDir::new();
        let file = tempdir.path().join("file");
        fs::write(&file, b"x").expect("write file");
        let metadata = fs::metadata(&file).expect("metadata");

        let output = render_target(
            file.to_str().expect("utf8 path"),
            Options {
                long_format: true,
                omit_group: true,
                numeric_ids: true,
                ..Options::default()
            },
        )
        .expect("render -o");

        assert!(output.contains(&format!(" {} ", metadata.uid())));
        assert!(!output.contains(&format!(" {} {} ", metadata.uid(), metadata.gid())));
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
