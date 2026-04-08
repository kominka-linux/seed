use std::io::{BufReader, Write};
use std::path::{Path, PathBuf};

use lzma_rust2::{LzmaOptions, LzmaReader, LzmaWriter, XzOptions, XzReader, XzWriter};

use crate::applets::compression::{
    self, Invocation as CompressionInvocation, LevelRange, Options as CompressionOptions,
};
use crate::common::applet::{AppletResult, finish};
use crate::common::error::AppletError;
use crate::common::io::{BUFFER_SIZE, Input, copy_stream};

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
        compression::compressed_path_with_suffix(path, self.default_suffix())
    }

    fn decompressed_path(self, path: &Path) -> Result<PathBuf, AppletError> {
        match self {
            Self::Xz => compression::decompressed_path_with_suffixes(
                self.applet(),
                path,
                &[(b".txz", b".tar"), (b".xz", b"")],
            ),
            Self::Lzma => compression::decompressed_path_with_suffixes(
                self.applet(),
                path,
                &[(b".tlz", b".tar"), (b".lzma", b"")],
            ),
        }
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

    fn from_common(format: Format, options: CompressionOptions) -> Self {
        Self {
            format,
            decompress: options.decompress,
            stdout: options.stdout,
            force: options.force,
            keep: options.keep,
            test: options.test,
            level: options.level,
        }
    }

    fn common(self) -> CompressionOptions {
        CompressionOptions {
            decompress: self.decompress,
            stdout: self.stdout,
            force: self.force,
            keep: self.keep,
            test: self.test,
            level: self.level,
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
    compression::run(
        invocation.format.applet(),
        args,
        CompressionInvocation {
            decompress: invocation.decompress,
            stdout: invocation.stdout,
        },
        LevelRange::ZeroToNine,
        |path, options| process_target(path, Options::from_common(invocation.format, options)),
    )
}

#[cfg(test)]
fn parse_args(
    args: &[String],
    invocation: Invocation,
) -> Result<(Options, Vec<String>), Vec<AppletError>> {
    let (options, files) = compression::parse_args(
        invocation.format.applet(),
        args,
        CompressionInvocation {
            decompress: invocation.decompress,
            stdout: invocation.stdout,
        },
        LevelRange::ZeroToNine,
    )?;
    Ok((Options::from_common(invocation.format, options), files))
}

fn process_target(path: &str, options: Options) -> Result<(), AppletError> {
    compression::process_target(
        options.applet(),
        path,
        options.common(),
        |path| options.format.compressed_path(path),
        |path| options.format.decompressed_path(path),
        compression::input_size_hint,
        |input, output, common, path, input_size| {
            process_reader_to_writer(
                input,
                output,
                Options::from_common(options.format, common),
                path,
                input_size,
            )
        },
    )
}

fn process_reader_to_writer(
    input: Input,
    mut output: &mut dyn Write,
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
                let mut encoder =
                    XzWriter::new(&mut *output, XzOptions::with_preset(options.level)).map_err(
                        |err| AppletError::from_io(options.applet(), "processing", path, err),
                    )?;
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
                    &mut *output,
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
        assert!(
            Format::Lzma
                .decompressed_path(Path::new("archive"))
                .is_err()
        );
    }

    #[test]
    fn xz_round_trip_file_compression() {
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
}
