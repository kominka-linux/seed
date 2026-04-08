use std::io::{BufReader, Write};
use std::path::{Path, PathBuf};

use bzip2::Compression;
use bzip2::bufread::MultiBzDecoder;
use bzip2::write::BzEncoder;

use crate::applets::compression::{self, Invocation, LevelRange, Options};
use crate::common::applet::finish;
use crate::common::error::AppletError;
use crate::common::io::{BUFFER_SIZE, Input, copy_stream};

const APPLET: &str = "bzip2";

pub fn main(args: &[String]) -> i32 {
    finish(run(args, Invocation::default()))
}

pub fn main_bunzip2(args: &[String]) -> i32 {
    finish(run(
        args,
        Invocation {
            decompress: true,
            stdout: false,
        },
    ))
}

pub fn main_bzcat(args: &[String]) -> i32 {
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
        let mut decoder = MultiBzDecoder::new(BufReader::with_capacity(BUFFER_SIZE, input));
        copy_stream(&mut decoder, &mut output)
            .map_err(|err| AppletError::from_io(APPLET, "processing", path, err))?;
    } else {
        {
            let mut encoder = BzEncoder::new(&mut *output, Compression::new(options.level));
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
    compression::compressed_path_with_suffix(path, b".bz2")
}

fn decompressed_path(path: &Path) -> Result<PathBuf, AppletError> {
    compression::decompressed_path_with_suffixes(
        APPLET,
        path,
        &[(b".tbz2", b".tar"), (b".tbz", b".tar"), (b".bz2", b"")],
    )
}

#[cfg(test)]
mod tests {
    use super::{Invocation, compressed_path, decompressed_path, parse_args};
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
    fn parse_aliases_seed_default_behavior() {
        let (options, files) = parse_args(
            &["-c".to_string(), "archive.bz2".to_string()],
            Invocation {
                decompress: true,
                stdout: true,
            },
        )
        .expect("parse");

        assert!(options.decompress);
        assert!(options.stdout);
        assert_eq!(files, vec![String::from("archive.bz2")]);
    }

    #[test]
    fn decompression_suffixes_follow_bzip2_conventions() {
        assert_eq!(
            decompressed_path(Path::new("archive.tbz2"))
                .expect("tbz2 path")
                .as_os_str()
                .as_bytes(),
            b"archive.tar"
        );
        assert_eq!(
            decompressed_path(Path::new("archive.bz2"))
                .expect("bz2 path")
                .as_os_str()
                .as_bytes(),
            b"archive"
        );
        assert!(decompressed_path(Path::new("archive")).is_err());
    }

    #[test]
    fn compression_adds_bzip2_suffix() {
        assert_eq!(
            compressed_path(Path::new("archive")).as_os_str().as_bytes(),
            b"archive.bz2"
        );
    }

    #[test]
    fn round_trip_file_compression() {
        let dir = temp_dir("bzip2");
        let input = dir.join("hello.txt");
        fs::write(&input, b"hello bzip2\n").expect("write input");

        let status = super::main(&[input.display().to_string()]);
        assert_eq!(status, 0);

        let compressed = dir.join("hello.txt.bz2");
        assert!(fs::read(&compressed).is_ok());

        let status = super::main_bunzip2(&[compressed.display().to_string()]);
        assert_eq!(status, 0);
        assert_eq!(fs::read(&input).expect("read output"), b"hello bzip2\n");

        let _ = fs::remove_dir_all(dir);
    }
}
