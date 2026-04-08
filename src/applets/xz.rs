use std::ffi::OsString;
use std::fs::{self, File, OpenOptions};
use std::io::{self, BufReader, BufWriter, Write};
use std::os::unix::ffi::{OsStrExt, OsStringExt};
use std::path::{Path, PathBuf};

use lzma_rust2::{LzmaOptions, LzmaReader, LzmaWriter, XzOptions, XzReader, XzWriter};

use crate::common::applet::{AppletResult, finish};
use crate::common::error::AppletError;
use crate::common::io::{BUFFER_SIZE, Input, copy_stream, open_input, stdout};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Format {
    Xz,
    Lzma,
}

impl Format {
    fn applet(self) -> &'static str {
        match self {
            Self::Xz => "xz",
            Self::Lzma => "lzma",
        }
    }

    fn compressed_path(self, path: &Path) -> PathBuf {
        let mut name = path
            .file_name()
            .unwrap_or(path.as_os_str())
            .as_bytes()
            .to_vec();
        name.extend_from_slice(self.default_suffix());
        path.with_file_name(OsString::from_vec(name))
    }

    fn decompressed_path(self, path: &Path) -> Result<PathBuf, AppletError> {
        let name = path.file_name().unwrap_or(path.as_os_str()).as_bytes();
        let decoded = match self {
            Self::Xz => {
                if let Some(stem) = name.strip_suffix(b".txz") {
                    [stem, b".tar"].concat()
                } else if let Some(stem) = name.strip_suffix(b".xz") {
                    stem.to_vec()
                } else {
                    return Err(AppletError::new(
                        self.applet(),
                        format!("unknown suffix -- {}", path.display()),
                    ));
                }
            }
            Self::Lzma => {
                if let Some(stem) = name.strip_suffix(b".tlz") {
                    [stem, b".tar"].concat()
                } else if let Some(stem) = name.strip_suffix(b".lzma") {
                    stem.to_vec()
                } else {
                    return Err(AppletError::new(
                        self.applet(),
                        format!("unknown suffix -- {}", path.display()),
                    ));
                }
            }
        };

        if decoded.is_empty() {
            return Err(AppletError::new(
                self.applet(),
                format!("unknown suffix -- {}", path.display()),
            ));
        }

        Ok(path.with_file_name(OsString::from_vec(decoded)))
    }

    fn default_suffix(self) -> &'static [u8] {
        match self {
            Self::Xz => b".xz",
            Self::Lzma => b".lzma",
        }
    }
}

#[derive(Clone, Copy, Debug)]
struct Options {
    format: Format,
    decompress: bool,
    stdout: bool,
    force: bool,
    keep: bool,
    test: bool,
    level: u32,
}

#[derive(Clone, Copy, Debug)]
struct Invocation {
    format: Format,
    decompress: bool,
    stdout: bool,
}

impl Options {
    fn applet(self) -> &'static str {
        self.format.applet()
    }
}

impl Default for Options {
    fn default() -> Self {
        Self {
            format: Format::Xz,
            decompress: false,
            stdout: false,
            force: false,
            keep: false,
            test: false,
            level: 6,
        }
    }
}

pub fn main(args: &[String]) -> i32 {
    finish(run(
        args,
        Invocation {
            format: Format::Xz,
            decompress: false,
            stdout: false,
        },
    ))
}

pub fn main_unxz(args: &[String]) -> i32 {
    finish(run(
        args,
        Invocation {
            format: Format::Xz,
            decompress: true,
            stdout: false,
        },
    ))
}

pub fn main_xzcat(args: &[String]) -> i32 {
    finish(run(
        args,
        Invocation {
            format: Format::Xz,
            decompress: true,
            stdout: true,
        },
    ))
}

pub fn main_lzma(args: &[String]) -> i32 {
    finish(run(
        args,
        Invocation {
            format: Format::Lzma,
            decompress: false,
            stdout: false,
        },
    ))
}

pub fn main_unlzma(args: &[String]) -> i32 {
    finish(run(
        args,
        Invocation {
            format: Format::Lzma,
            decompress: true,
            stdout: false,
        },
    ))
}

pub fn main_lzcat(args: &[String]) -> i32 {
    finish(run(
        args,
        Invocation {
            format: Format::Lzma,
            decompress: true,
            stdout: true,
        },
    ))
}

fn run(args: &[String], invocation: Invocation) -> AppletResult {
    let (options, files) = parse_args(args, invocation)?;
    let targets = if files.is_empty() {
        vec![String::from("-")]
    } else {
        files
    };

    let mut errors = Vec::new();
    for path in &targets {
        if let Err(err) = process_target(path, options) {
            errors.push(err);
        }
    }

    if errors.is_empty() {
        Ok(())
    } else {
        Err(errors)
    }
}

fn parse_args(
    args: &[String],
    invocation: Invocation,
) -> Result<(Options, Vec<String>), Vec<AppletError>> {
    let mut options = Options {
        format: invocation.format,
        decompress: invocation.decompress,
        stdout: invocation.stdout,
        ..Options::default()
    };
    let mut files = Vec::new();
    let mut parsing_flags = true;

    for arg in args {
        if parsing_flags && arg == "--" {
            parsing_flags = false;
            continue;
        }

        if parsing_flags && arg.starts_with('-') && arg.len() > 1 {
            for flag in arg[1..].chars() {
                match flag {
                    '0'..='9' => options.level = flag.to_digit(10).expect("validated digit"),
                    'c' => options.stdout = true,
                    'd' => options.decompress = true,
                    'f' => options.force = true,
                    'k' => options.keep = true,
                    't' => {
                        options.decompress = true;
                        options.test = true;
                    }
                    'z' => options.decompress = false,
                    _ => return Err(vec![AppletError::invalid_option(options.applet(), flag)]),
                }
            }
            continue;
        }

        files.push(arg.clone());
    }

    Ok((options, files))
}

fn process_target(path: &str, options: Options) -> Result<(), AppletError> {
    if path == "-" {
        return process_stdio(options);
    }

    if options.test {
        return test_file(path, options);
    }

    if options.stdout {
        return process_file_to_stdout(path, options);
    }

    process_file_in_place(path, options)
}

fn process_stdio(options: Options) -> Result<(), AppletError> {
    let input = open_input("-")
        .map_err(|err| AppletError::from_io(options.applet(), "opening", None, err))?;
    if options.test {
        return process_reader_to_sink(input, options, None);
    }

    let output = stdout();
    process_reader_to_writer(input, output, options, None, None)
}

fn process_file_to_stdout(path: &str, options: Options) -> Result<(), AppletError> {
    let input = open_input(path)
        .map_err(|err| AppletError::from_io(options.applet(), "opening", Some(path), err))?;
    let output = stdout();
    let size_hint = input_size_hint(path);
    process_reader_to_writer(input, output, options, Some(path), size_hint)
}

fn process_file_in_place(path: &str, options: Options) -> Result<(), AppletError> {
    let source = Path::new(path);
    let destination = if options.decompress {
        options.format.decompressed_path(source)?
    } else {
        options.format.compressed_path(source)
    };

    if destination.exists() && !options.force {
        return Err(AppletError::new(
            options.applet(),
            format!("not overwriting {}", destination.display()),
        ));
    }

    let input = open_input(path)
        .map_err(|err| AppletError::from_io(options.applet(), "opening", Some(path), err))?;
    let size_hint = input_size_hint(path);
    let (temp_path, temp_file) = create_temp_output(options.applet(), &destination)?;
    let writer = BufWriter::with_capacity(BUFFER_SIZE, temp_file);

    let result = process_reader_to_writer(input, writer, options, Some(path), size_hint);
    if let Err(err) = result {
        let _ = fs::remove_file(&temp_path);
        return Err(err);
    }

    if options.force && destination.exists() {
        fs::remove_file(&destination).map_err(|err| {
            let _ = fs::remove_file(&temp_path);
            AppletError::from_io(
                options.applet(),
                "removing",
                Some(path_for_error(&destination)),
                err,
            )
        })?;
    }

    fs::rename(&temp_path, &destination).map_err(|err| {
        let _ = fs::remove_file(&temp_path);
        AppletError::from_io(
            options.applet(),
            "renaming",
            Some(path_for_error(&destination)),
            err,
        )
    })?;

    if !options.keep {
        fs::remove_file(source)
            .map_err(|err| AppletError::from_io(options.applet(), "removing", Some(path), err))?;
    }

    Ok(())
}

fn test_file(path: &str, options: Options) -> Result<(), AppletError> {
    let input = open_input(path)
        .map_err(|err| AppletError::from_io(options.applet(), "opening", Some(path), err))?;
    process_reader_to_sink(input, options, Some(path))
}

fn process_reader_to_sink(
    input: Input,
    mut options: Options,
    path: Option<&str>,
) -> Result<(), AppletError> {
    options.decompress = true;
    process_reader_to_writer(input, io::sink(), options, path, None)
}

fn process_reader_to_writer<R: io::Read, W: Write>(
    input: R,
    mut output: W,
    options: Options,
    path: Option<&str>,
    input_size: Option<u64>,
) -> Result<(), AppletError> {
    if options.decompress {
        match options.format {
            Format::Xz => {
                let mut decoder = XzReader::new(BufReader::with_capacity(BUFFER_SIZE, input), true);
                copy_stream(&mut decoder, &mut output).map_err(|err| {
                    AppletError::from_io(options.applet(), "processing", path, err)
                })?;
            }
            Format::Lzma => {
                let mut decoder = LzmaReader::new_mem_limit(
                    BufReader::with_capacity(BUFFER_SIZE, input),
                    u32::MAX,
                    None,
                )
                .map_err(|err| AppletError::from_io(options.applet(), "processing", path, err))?;
                copy_stream(&mut decoder, &mut output).map_err(|err| {
                    AppletError::from_io(options.applet(), "processing", path, err)
                })?;
            }
        }
    } else {
        match options.format {
            Format::Xz => {
                let mut encoder = XzWriter::new(&mut output, XzOptions::with_preset(options.level))
                    .map_err(|err| {
                        AppletError::from_io(options.applet(), "processing", path, err)
                    })?;
                copy_stream(
                    &mut BufReader::with_capacity(BUFFER_SIZE, input),
                    &mut encoder,
                )
                .map_err(|err| AppletError::from_io(options.applet(), "processing", path, err))?;
                encoder.finish().map_err(|err| {
                    AppletError::from_io(options.applet(), "processing", path, err)
                })?;
            }
            Format::Lzma => {
                let mut encoder = LzmaWriter::new_use_header(
                    &mut output,
                    &LzmaOptions::with_preset(options.level),
                    input_size,
                )
                .map_err(|err| AppletError::from_io(options.applet(), "processing", path, err))?;
                copy_stream(
                    &mut BufReader::with_capacity(BUFFER_SIZE, input),
                    &mut encoder,
                )
                .map_err(|err| AppletError::from_io(options.applet(), "processing", path, err))?;
                encoder.finish().map_err(|err| {
                    AppletError::from_io(options.applet(), "processing", path, err)
                })?;
            }
        }
    }

    output
        .flush()
        .map_err(|err| AppletError::from_io(options.applet(), "flushing", path, err))
}

fn create_temp_output(
    applet: &'static str,
    destination: &Path,
) -> Result<(PathBuf, File), AppletError> {
    let parent = destination.parent().unwrap_or_else(|| Path::new("."));
    let base = destination
        .file_name()
        .unwrap_or(destination.as_os_str())
        .as_bytes()
        .to_vec();

    for attempt in 0..1024_u32 {
        let mut name = base.clone();
        name.extend_from_slice(format!(".seed-tmp-{}-{attempt}", std::process::id()).as_bytes());
        let candidate = parent.join(OsString::from_vec(name));
        match OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&candidate)
        {
            Ok(file) => return Ok((candidate, file)),
            Err(err) if err.kind() == io::ErrorKind::AlreadyExists => continue,
            Err(err) => {
                return Err(AppletError::from_io(
                    applet,
                    "creating",
                    Some(path_for_error(&candidate)),
                    err,
                ));
            }
        }
    }

    Err(AppletError::new(
        applet,
        format!(
            "failed to create temporary output for {}",
            destination.display()
        ),
    ))
}

fn input_size_hint(path: &str) -> Option<u64> {
    fs::metadata(path).ok().map(|metadata| metadata.len())
}

fn path_for_error(path: &Path) -> &str {
    path.to_str().unwrap_or("<non-utf8 path>")
}

#[cfg(test)]
mod tests {
    use super::{Format, Invocation, parse_args};
    use std::fs;
    use std::os::unix::ffi::OsStrExt;
    use std::path::{Path, PathBuf};
    use std::time::{SystemTime, UNIX_EPOCH};

    fn temp_dir(label: &str) -> PathBuf {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("time")
            .as_nanos();
        let dir = std::env::temp_dir().join(format!("seed-{label}-{unique}"));
        fs::create_dir_all(&dir).expect("create temp dir");
        dir
    }

    #[test]
    fn xz_aliases_seed_default_behavior() {
        let (options, files) = parse_args(
            &["-c".to_string(), "archive.xz".to_string()],
            Invocation {
                format: Format::Xz,
                decompress: true,
                stdout: true,
            },
        )
        .expect("parse");

        assert!(options.decompress);
        assert!(options.stdout);
        assert_eq!(files, vec![String::from("archive.xz")]);
    }

    #[test]
    fn lzma_suffixes_follow_conventions() {
        assert_eq!(
            Format::Lzma
                .decompressed_path(Path::new("archive.tlz"))
                .expect("tlz path")
                .as_os_str()
                .as_bytes(),
            b"archive.tar"
        );
        assert_eq!(
            Format::Lzma
                .decompressed_path(Path::new("archive.lzma"))
                .expect("lzma path")
                .as_os_str()
                .as_bytes(),
            b"archive"
        );
        assert_eq!(
            Format::Xz
                .compressed_path(Path::new("archive"))
                .as_os_str()
                .as_bytes(),
            b"archive.xz"
        );
    }

    #[test]
    fn round_trip_xz_file_compression() {
        let dir = temp_dir("xz");
        let input = dir.join("hello.txt");
        fs::write(&input, b"hello xz\n").expect("write input");

        let status = super::main(&[input.display().to_string()]);
        assert_eq!(status, 0);

        let compressed = dir.join("hello.txt.xz");
        assert!(fs::read(&compressed).is_ok());

        let status = super::main_unxz(&[compressed.display().to_string()]);
        assert_eq!(status, 0);
        assert_eq!(fs::read(&input).expect("read output"), b"hello xz\n");

        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn round_trip_lzma_file_compression() {
        let dir = temp_dir("lzma");
        let input = dir.join("hello.txt");
        fs::write(&input, b"hello lzma\n").expect("write input");

        let status = super::main_lzma(&[input.display().to_string()]);
        assert_eq!(status, 0);

        let compressed = dir.join("hello.txt.lzma");
        assert!(fs::read(&compressed).is_ok());

        let status = super::main_unlzma(&[compressed.display().to_string()]);
        assert_eq!(status, 0);
        assert_eq!(fs::read(&input).expect("read output"), b"hello lzma\n");

        let _ = fs::remove_dir_all(dir);
    }
}
