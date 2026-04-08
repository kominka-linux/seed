use std::io::{BufReader, Write};

use flate2::Compression;
use flate2::bufread::MultiGzDecoder;
use flate2::write::GzEncoder;

use crate::applets::compression::{self, Codec, Invocation, Options};
use crate::common::applet::finish;
use crate::common::error::AppletError;
use crate::common::io::{BUFFER_SIZE, Input, copy_stream};

const APPLET: &str = "gzip";
const DECOMPRESSED_SUFFIXES: &[(&[u8], &[u8])] =
    &[(b".tgz", b".tar"), (b".taz", b".tar"), (b".gz", b"")];
const CODEC: Codec<'static> = Codec {
    applet: APPLET,
    level_range: compression::LevelRange::OneToNine,
    compressed_suffix: b".gz",
    decompressed_suffixes: DECOMPRESSED_SUFFIXES,
    input_size_hint: |_| None,
    process_reader_to_writer,
};

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
#[cfg(test)]
mod tests {
    use super::{APPLET, CODEC, DECOMPRESSED_SUFFIXES, Invocation, parse_args};
    use crate::applets::compression;
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
            compression::decompressed_path_with_suffixes(
                APPLET,
                Path::new("archive.tgz"),
                DECOMPRESSED_SUFFIXES
            )
            .expect("tgz path")
            .as_os_str()
            .as_bytes(),
            b"archive.tar"
        );
        assert_eq!(
            compression::decompressed_path_with_suffixes(
                APPLET,
                Path::new("archive.gz"),
                DECOMPRESSED_SUFFIXES
            )
            .expect("gz path")
            .as_os_str()
            .as_bytes(),
            b"archive"
        );
        assert!(
            compression::decompressed_path_with_suffixes(
                APPLET,
                Path::new("archive"),
                DECOMPRESSED_SUFFIXES
            )
            .is_err()
        );
    }

    #[test]
    fn compression_adds_gzip_suffix() {
        assert_eq!(
            compression::compressed_path_with_suffix(Path::new("archive"), CODEC.compressed_suffix)
                .as_os_str()
                .as_bytes(),
            b"archive.gz"
        );
    }
}
