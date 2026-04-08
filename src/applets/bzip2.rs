use std::io::{BufReader, Write};

use bzip2::Compression;
use bzip2::bufread::MultiBzDecoder;
use bzip2::write::BzEncoder;

use crate::applets::compression::{self, Codec, Invocation, Options};
use crate::common::applet::finish;
use crate::common::error::AppletError;
use crate::common::io::{BUFFER_SIZE, Input, copy_stream};

const APPLET: &str = "bzip2";
const DECOMPRESSED_SUFFIXES: &[(&[u8], &[u8])] =
    &[(b".tbz2", b".tar"), (b".tbz", b".tar"), (b".bz2", b"")];
const CODEC: Codec<'static> = Codec {
    applet: APPLET,
    level_range: compression::LevelRange::OneToNine,
    compressed_suffix: b".bz2",
    decompressed_suffixes: DECOMPRESSED_SUFFIXES,
    input_size_hint: |_| None,
    process_reader_to_writer,
};

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
    compression::run_with_codec(&CODEC, args, invocation)
}

#[cfg(test)]
fn parse_args(
    args: &[String],
    invocation: Invocation,
) -> Result<(Options, Vec<String>), Vec<AppletError>> {
    compression::parse_args_with_codec(&CODEC, args, invocation)
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
#[cfg(test)]
mod tests {
    use super::{APPLET, CODEC, DECOMPRESSED_SUFFIXES, Invocation, parse_args};
    use crate::applets::compression;
    use std::fs;
    use std::os::unix::ffi::OsStrExt;
    use std::path::Path;

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
            compression::decompressed_path_with_suffixes(
                APPLET,
                Path::new("archive.tbz2"),
                DECOMPRESSED_SUFFIXES,
            )
            .expect("tbz2 path")
            .as_os_str()
            .as_bytes(),
            b"archive.tar"
        );
        assert_eq!(
            compression::decompressed_path_with_suffixes(
                APPLET,
                Path::new("archive.bz2"),
                DECOMPRESSED_SUFFIXES,
            )
            .expect("bz2 path")
            .as_os_str()
            .as_bytes(),
            b"archive"
        );
        assert!(
            compression::decompressed_path_with_suffixes(
                APPLET,
                Path::new("archive"),
                DECOMPRESSED_SUFFIXES,
            )
            .is_err()
        );
    }

    #[test]
    fn compression_adds_bzip2_suffix() {
        assert_eq!(
            compression::compressed_path_with_suffix(Path::new("archive"), CODEC.compressed_suffix)
                .as_os_str()
                .as_bytes(),
            b"archive.bz2"
        );
    }

    #[test]
    fn round_trip_file_compression() {
        let dir = compression::temp_dir("bzip2");
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
