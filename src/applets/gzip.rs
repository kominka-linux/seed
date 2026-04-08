use std::io::{BufReader, Write};
use std::path::{Path, PathBuf};

use flate2::Compression;
use flate2::bufread::MultiGzDecoder;
use flate2::write::GzEncoder;

use crate::applets::compression::{self, Invocation, LevelRange, Options};
use crate::common::applet::finish;
use crate::common::error::AppletError;
use crate::common::io::{BUFFER_SIZE, Input, copy_stream};

const APPLET: &str = "gzip";

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

fn run(args: &[String], invocation: Invocation) -> Result<(), Vec<AppletError>> {
    compression::run(
        APPLET,
        args,
        invocation,
        LevelRange::OneToNine,
        process_target,
    )
}

#[cfg(test)]
fn parse_args(
    args: &[String],
    invocation: Invocation,
) -> Result<(Options, Vec<String>), Vec<AppletError>> {
    compression::parse_args(APPLET, args, invocation, LevelRange::OneToNine)
}

fn process_target(path: &str, options: Options) -> Result<(), AppletError> {
    compression::process_target(
        APPLET,
        path,
        options,
        compressed_path,
        decompressed_path,
        |_| None,
        process_reader_to_writer,
    )
}

fn process_reader_to_writer(
    input: Input,
    mut output: &mut dyn Write,
    options: Options,
    path: Option<&str>,
    _input_size: Option<u64>,
) -> Result<(), AppletError> {
    if options.decompress {
        let mut decoder = MultiGzDecoder::new(BufReader::with_capacity(BUFFER_SIZE, input));
        copy_stream(&mut decoder, &mut output)
            .map_err(|err| AppletError::from_io(APPLET, "processing", path, err))?;
    } else {
        {
            let mut encoder = GzEncoder::new(&mut *output, Compression::new(options.level));
            copy_stream(
                &mut BufReader::with_capacity(BUFFER_SIZE, input),
                &mut encoder,
            )
            .map_err(|err| AppletError::from_io(APPLET, "processing", path, err))?;
            encoder
                .try_finish()
                .map_err(|err| AppletError::from_io(APPLET, "processing", path, err))?;
        }
    }

    output
        .flush()
        .map_err(|err| AppletError::from_io(APPLET, "flushing", path, err))
}

fn compressed_path(path: &Path) -> PathBuf {
    compression::compressed_path_with_suffix(path, b".gz")
}

fn decompressed_path(path: &Path) -> Result<PathBuf, AppletError> {
    compression::decompressed_path_with_suffixes(
        APPLET,
        path,
        &[(b".tgz", b".tar"), (b".taz", b".tar"), (b".gz", b"")],
    )
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
