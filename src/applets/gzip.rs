use std::ffi::OsString;
use std::fs::{self, File, OpenOptions};
use std::io::{self, BufReader, BufWriter, Write};
use std::os::unix::ffi::{OsStrExt, OsStringExt};
use std::path::{Path, PathBuf};

use flate2::Compression;
use flate2::bufread::MultiGzDecoder;
use flate2::write::GzEncoder;

use crate::common::applet::{AppletResult, finish};
use crate::common::error::AppletError;
use crate::common::io::{BUFFER_SIZE, Input, copy_stream, open_input, stdout};

const APPLET: &str = "gzip";

#[derive(Clone, Copy, Debug)]
struct Options {
    decompress: bool,
    stdout: bool,
    force: bool,
    keep: bool,
    test: bool,
    level: u32,
}

impl Default for Options {
    fn default() -> Self {
        Self {
            decompress: false,
            stdout: false,
            force: false,
            keep: false,
            test: false,
            level: 6,
        }
    }
}

#[derive(Clone, Copy, Debug, Default)]
struct Invocation {
    decompress: bool,
    stdout: bool,
}

pub fn main(args: &[String]) -> i32 {
    finish(run(args, Invocation::default()))
}

pub fn main_gunzip(args: &[String]) -> i32 {
    finish(run(
        args,
        Invocation {
            decompress: true,
            stdout: false,
        },
    ))
}

pub fn main_zcat(args: &[String]) -> i32 {
    finish(run(
        args,
        Invocation {
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
                    '1'..='9' => options.level = flag.to_digit(10).expect("validated digit"),
                    'c' => options.stdout = true,
                    'd' => options.decompress = true,
                    'f' => options.force = true,
                    'k' => options.keep = true,
                    't' => {
                        options.decompress = true;
                        options.test = true;
                    }
                    'z' => options.decompress = false,
                    _ => return Err(vec![AppletError::invalid_option(APPLET, flag)]),
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
        return test_file(path);
    }

    if options.stdout {
        return process_file_to_stdout(path, options);
    }

    process_file_in_place(path, options)
}

fn process_stdio(options: Options) -> Result<(), AppletError> {
    let input =
        open_input("-").map_err(|err| AppletError::from_io(APPLET, "opening", None, err))?;
    if options.test {
        return process_reader_to_sink(input, options, None);
    }

    let output = stdout();
    process_reader_to_writer(input, output, options, None)
}

fn process_file_to_stdout(path: &str, options: Options) -> Result<(), AppletError> {
    let input =
        open_input(path).map_err(|err| AppletError::from_io(APPLET, "opening", Some(path), err))?;
    let output = stdout();
    process_reader_to_writer(input, output, options, Some(path))
}

fn process_file_in_place(path: &str, options: Options) -> Result<(), AppletError> {
    let source = Path::new(path);
    let destination = if options.decompress {
        decompressed_path(source)?
    } else {
        compressed_path(source)
    };

    if destination.exists() && !options.force {
        return Err(AppletError::new(
            APPLET,
            format!("not overwriting {}", destination.display()),
        ));
    }

    let input =
        open_input(path).map_err(|err| AppletError::from_io(APPLET, "opening", Some(path), err))?;
    let (temp_path, temp_file) = create_temp_output(&destination)?;
    let writer = BufWriter::with_capacity(BUFFER_SIZE, temp_file);

    let result = process_reader_to_writer(input, writer, options, Some(path));
    if let Err(err) = result {
        let _ = fs::remove_file(&temp_path);
        return Err(err);
    }

    if options.force && destination.exists() {
        fs::remove_file(&destination).map_err(|err| {
            let _ = fs::remove_file(&temp_path);
            AppletError::from_io(APPLET, "removing", Some(path_for_error(&destination)), err)
        })?;
    }

    fs::rename(&temp_path, &destination).map_err(|err| {
        let _ = fs::remove_file(&temp_path);
        AppletError::from_io(APPLET, "renaming", Some(path_for_error(&destination)), err)
    })?;

    if !options.keep {
        fs::remove_file(source)
            .map_err(|err| AppletError::from_io(APPLET, "removing", Some(path), err))?;
    }

    Ok(())
}

fn test_file(path: &str) -> Result<(), AppletError> {
    let input =
        open_input(path).map_err(|err| AppletError::from_io(APPLET, "opening", Some(path), err))?;
    process_reader_to_sink(input, Options::default(), Some(path))
}

fn process_reader_to_sink(
    input: Input,
    mut options: Options,
    path: Option<&str>,
) -> Result<(), AppletError> {
    options.decompress = true;
    process_reader_to_writer(input, io::sink(), options, path)
}

fn process_reader_to_writer<R: io::Read, W: Write>(
    input: R,
    mut output: W,
    options: Options,
    path: Option<&str>,
) -> Result<(), AppletError> {
    if options.decompress {
        let mut decoder = MultiGzDecoder::new(BufReader::with_capacity(BUFFER_SIZE, input));
        copy_stream(&mut decoder, &mut output)
            .map_err(|err| AppletError::from_io(APPLET, "processing", path, err))?;
    } else {
        let mut encoder = GzEncoder::new(&mut output, Compression::new(options.level));
        copy_stream(
            &mut BufReader::with_capacity(BUFFER_SIZE, input),
            &mut encoder,
        )
        .map_err(|err| AppletError::from_io(APPLET, "processing", path, err))?;
        encoder
            .try_finish()
            .map_err(|err| AppletError::from_io(APPLET, "processing", path, err))?;
    }

    output
        .flush()
        .map_err(|err| AppletError::from_io(APPLET, "flushing", path, err))
}

fn compressed_path(path: &Path) -> PathBuf {
    let mut name = path
        .file_name()
        .unwrap_or(path.as_os_str())
        .as_bytes()
        .to_vec();
    name.extend_from_slice(b".gz");
    path.with_file_name(OsString::from_vec(name))
}

fn decompressed_path(path: &Path) -> Result<PathBuf, AppletError> {
    let name = path.file_name().unwrap_or(path.as_os_str()).as_bytes();
    let decoded = if let Some(stem) = name.strip_suffix(b".tgz") {
        [stem, b".tar"].concat()
    } else if let Some(stem) = name.strip_suffix(b".taz") {
        [stem, b".tar"].concat()
    } else if let Some(stem) = name.strip_suffix(b".gz") {
        stem.to_vec()
    } else {
        return Err(AppletError::new(
            APPLET,
            format!("unknown suffix -- {}", path.display()),
        ));
    };

    if decoded.is_empty() {
        return Err(AppletError::new(
            APPLET,
            format!("unknown suffix -- {}", path.display()),
        ));
    }

    Ok(path.with_file_name(OsString::from_vec(decoded)))
}

fn create_temp_output(destination: &Path) -> Result<(PathBuf, File), AppletError> {
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
                    APPLET,
                    "creating",
                    Some(path_for_error(&candidate)),
                    err,
                ));
            }
        }
    }

    Err(AppletError::new(
        APPLET,
        format!(
            "failed to create temporary output for {}",
            destination.display()
        ),
    ))
}

fn path_for_error(path: &Path) -> &str {
    path.to_str().unwrap_or("<non-utf8 path>")
}

#[cfg(test)]
mod tests {
    use super::{Invocation, compressed_path, decompressed_path, parse_args};
    use std::os::unix::ffi::OsStrExt;
    use std::path::Path;

    #[test]
    fn parse_aliases_seed_default_behavior() {
        let (options, files) = parse_args(
            &["-c".to_string(), "archive.gz".to_string()],
            Invocation {
                decompress: true,
                stdout: true,
            },
        )
        .expect("parse");

        assert!(options.decompress);
        assert!(options.stdout);
        assert_eq!(files, vec![String::from("archive.gz")]);
    }

    #[test]
    fn decompression_suffixes_follow_gzip_conventions() {
        assert_eq!(
            decompressed_path(Path::new("archive.tgz"))
                .expect("tgz path")
                .as_os_str()
                .as_bytes(),
            b"archive.tar"
        );
        assert_eq!(
            decompressed_path(Path::new("archive.gz"))
                .expect("gz path")
                .as_os_str()
                .as_bytes(),
            b"archive"
        );
        assert!(decompressed_path(Path::new("archive")).is_err());
    }

    #[test]
    fn compression_adds_gzip_suffix() {
        assert_eq!(
            compressed_path(Path::new("archive")).as_os_str().as_bytes(),
            b"archive.gz"
        );
    }
}
