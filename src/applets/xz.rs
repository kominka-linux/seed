use std::io::{BufReader, Write};

use lzma_rust2::{LzmaOptions, LzmaReader, LzmaWriter, XzOptions, XzReader, XzWriter};

use crate::applets::compression::{self, Codec, Invocation, Options};
use crate::common::applet::finish;
use crate::common::error::AppletError;
use crate::common::io::{BUFFER_SIZE, Input, copy_stream};

const XZ_APPLET: &str = "xz";
const LZMA_APPLET: &str = "lzma";
const XZ_DECOMPRESSED_SUFFIXES: &[(&[u8], &[u8])] = &[(b".txz", b".tar"), (b".xz", b"")];
const LZMA_DECOMPRESSED_SUFFIXES: &[(&[u8], &[u8])] = &[(b".tlz", b".tar"), (b".lzma", b"")];
const XZ_CODEC: Codec<'static> = Codec {
    applet: XZ_APPLET,
    level_range: compression::LevelRange::ZeroToNine,
    compressed_suffix: b".xz",
    decompressed_suffixes: XZ_DECOMPRESSED_SUFFIXES,
    input_size_hint: compression::input_size_hint,
    process_reader_to_writer: process_xz_reader_to_writer,
};
const LZMA_CODEC: Codec<'static> = Codec {
    applet: LZMA_APPLET,
    level_range: compression::LevelRange::ZeroToNine,
    compressed_suffix: b".lzma",
    decompressed_suffixes: LZMA_DECOMPRESSED_SUFFIXES,
    input_size_hint: compression::input_size_hint,
    process_reader_to_writer: process_lzma_reader_to_writer,
};

pub fn main(args: &[String]) -> i32 {
    finish(run(args, &XZ_CODEC, Invocation::default()))
}

pub fn main_unxz(args: &[String]) -> i32 {
    finish(run(
        args,
        &XZ_CODEC,
        Invocation {
            decompress: true,
            stdout: false,
        },
    ))
}

pub fn main_xzcat(args: &[String]) -> i32 {
    finish(run(
        args,
        &XZ_CODEC,
        Invocation {
            decompress: true,
            stdout: true,
        },
    ))
}

pub fn main_lzma(args: &[String]) -> i32 {
    finish(run(args, &LZMA_CODEC, Invocation::default()))
}

pub fn main_unlzma(args: &[String]) -> i32 {
    finish(run(
        args,
        &LZMA_CODEC,
        Invocation {
            decompress: true,
            stdout: false,
        },
    ))
}

pub fn main_lzcat(args: &[String]) -> i32 {
    finish(run(
        args,
        &LZMA_CODEC,
        Invocation {
            decompress: true,
            stdout: true,
        },
    ))
}

fn run(args: &[String], codec: &Codec<'_>, invocation: Invocation) -> Result<(), Vec<AppletError>> {
    compression::run_with_codec(codec, args, invocation)
}

#[cfg(test)]
fn parse_args(
    args: &[String],
    codec: &Codec<'_>,
    invocation: Invocation,
) -> Result<(Options, Vec<String>), Vec<AppletError>> {
    compression::parse_args_with_codec(codec, args, invocation)
}

fn process_xz_reader_to_writer(
    input: Input,
    mut output: &mut dyn Write,
    options: Options,
    path: Option<&str>,
    _input_size: Option<u64>,
) -> Result<(), AppletError> {
    if options.decompress {
        let mut decoder = XzReader::new(BufReader::with_capacity(BUFFER_SIZE, input), true);
        copy_stream(&mut decoder, &mut output)
            .map_err(|err| AppletError::from_io(XZ_APPLET, "processing", path, err))?;
    } else {
        let mut encoder = XzWriter::new(&mut *output, XzOptions::with_preset(options.level))
            .map_err(|err| AppletError::from_io(XZ_APPLET, "processing", path, err))?;
        copy_stream(
            &mut BufReader::with_capacity(BUFFER_SIZE, input),
            &mut encoder,
        )
        .map_err(|err| AppletError::from_io(XZ_APPLET, "processing", path, err))?;
        encoder
            .finish()
            .map_err(|err| AppletError::from_io(XZ_APPLET, "processing", path, err))?;
    }

    output
        .flush()
        .map_err(|err| AppletError::from_io(XZ_APPLET, "flushing", path, err))
}

fn process_lzma_reader_to_writer(
    input: Input,
    mut output: &mut dyn Write,
    options: Options,
    path: Option<&str>,
    input_size: Option<u64>,
) -> Result<(), AppletError> {
    if options.decompress {
        let mut decoder =
            LzmaReader::new_mem_limit(BufReader::with_capacity(BUFFER_SIZE, input), u32::MAX, None)
                .map_err(|err| AppletError::from_io(LZMA_APPLET, "processing", path, err))?;
        copy_stream(&mut decoder, &mut output)
            .map_err(|err| AppletError::from_io(LZMA_APPLET, "processing", path, err))?;
    } else {
        let mut encoder = LzmaWriter::new_use_header(
            &mut *output,
            &LzmaOptions::with_preset(options.level),
            input_size,
        )
        .map_err(|err| AppletError::from_io(LZMA_APPLET, "processing", path, err))?;
        copy_stream(
            &mut BufReader::with_capacity(BUFFER_SIZE, input),
            &mut encoder,
        )
        .map_err(|err| AppletError::from_io(LZMA_APPLET, "processing", path, err))?;
        encoder
            .finish()
            .map_err(|err| AppletError::from_io(LZMA_APPLET, "processing", path, err))?;
    }

    output
        .flush()
        .map_err(|err| AppletError::from_io(LZMA_APPLET, "flushing", path, err))
}

#[cfg(test)]
mod tests {
    use super::{
        Invocation, LZMA_CODEC, LZMA_DECOMPRESSED_SUFFIXES, XZ_APPLET, XZ_CODEC,
        XZ_DECOMPRESSED_SUFFIXES, parse_args,
    };
    use crate::applets::compression;
    use std::fs;
    use std::os::unix::ffi::OsStrExt;
    use std::path::Path;

    #[test]
    fn xz_aliases_seed_default_behavior() {
        let (options, files) = parse_args(
            &["-c".to_string(), "archive.xz".to_string()],
            &XZ_CODEC,
            Invocation {
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
            compression::decompressed_path_with_suffixes(
                super::LZMA_APPLET,
                Path::new("archive.tlz"),
                LZMA_DECOMPRESSED_SUFFIXES,
            )
            .expect("tlz path")
            .as_os_str()
            .as_bytes(),
            b"archive.tar"
        );
        assert_eq!(
            compression::decompressed_path_with_suffixes(
                super::LZMA_APPLET,
                Path::new("archive.lzma"),
                LZMA_DECOMPRESSED_SUFFIXES,
            )
            .expect("lzma path")
            .as_os_str()
            .as_bytes(),
            b"archive"
        );
        assert!(
            compression::decompressed_path_with_suffixes(
                super::LZMA_APPLET,
                Path::new("archive"),
                LZMA_DECOMPRESSED_SUFFIXES,
            )
            .is_err()
        );
    }

    #[test]
    fn xz_suffixes_follow_conventions() {
        assert_eq!(
            compression::decompressed_path_with_suffixes(
                XZ_APPLET,
                Path::new("archive.txz"),
                XZ_DECOMPRESSED_SUFFIXES,
            )
            .expect("txz path")
            .as_os_str()
            .as_bytes(),
            b"archive.tar"
        );
    }

    #[test]
    fn xz_round_trip_file_compression() {
        let dir = compression::temp_dir("xz");
        let input = dir.join("hello.txt");
        fs::write(&input, b"hello xz\n").expect("write input");

        let status = super::main(&[input.display().to_string()]);
        assert_eq!(status, 0);

        let compressed = dir.join(
            compression::compressed_path_with_suffix(
                Path::new("hello.txt"),
                XZ_CODEC.compressed_suffix,
            )
            .file_name()
            .expect("compressed file name"),
        );
        assert!(fs::read(&compressed).is_ok());

        let status = super::main_unxz(&[compressed.display().to_string()]);
        assert_eq!(status, 0);
        assert_eq!(fs::read(&input).expect("read output"), b"hello xz\n");

        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn lzma_round_trip_file_compression() {
        let dir = compression::temp_dir("lzma");
        let input = dir.join("hello.txt");
        fs::write(&input, b"hello lzma\n").expect("write input");

        let status = super::main_lzma(&[input.display().to_string()]);
        assert_eq!(status, 0);

        let compressed = dir.join(
            compression::compressed_path_with_suffix(
                Path::new("hello.txt"),
                LZMA_CODEC.compressed_suffix,
            )
            .file_name()
            .expect("compressed file name"),
        );
        assert!(fs::read(&compressed).is_ok());

        let status = super::main_unlzma(&[compressed.display().to_string()]);
        assert_eq!(status, 0);
        assert_eq!(fs::read(&input).expect("read output"), b"hello lzma\n");

        let _ = fs::remove_dir_all(dir);
    }
}
