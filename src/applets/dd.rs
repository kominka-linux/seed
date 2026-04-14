use std::fs::OpenOptions;
use std::io::{Read, Seek, SeekFrom, Write};

use crate::common::applet::{AppletResult, finish};
use crate::common::error::AppletError;
use crate::common::io::{open_input, stdout};

const APPLET: &str = "dd";

#[derive(Clone, Debug)]
struct Options {
    input: String,
    output: Option<String>,
    block_size: usize,
    count: Option<usize>,
    skip: usize,
    seek: usize,
}

impl Default for Options {
    fn default() -> Self {
        Self {
            input: "-".to_string(),
            output: None,
            block_size: 512,
            count: None,
            skip: 0,
            seek: 0,
        }
    }
}

pub fn main(args: &[String]) -> i32 {
    finish(run(args))
}

fn run(args: &[String]) -> AppletResult {
    let options = parse_args(args)?;
    let mut input = open_input(&options.input).map_err(|err| {
        vec![AppletError::from_io(
            APPLET,
            "opening",
            Some(&options.input),
            err,
        )]
    })?;

    if options.skip > 0 {
        skip_blocks(&mut input, options.skip, options.block_size).map_err(|err| {
            vec![AppletError::from_io(
                APPLET,
                "reading",
                Some(&options.input),
                err,
            )]
        })?;
    }

    match &options.output {
        Some(path) => {
            let mut open = OpenOptions::new();
            open.write(true).create(true);
            if options.seek == 0 {
                open.truncate(true);
            }
            let mut output = open
                .open(path)
                .map_err(|err| vec![AppletError::from_io(APPLET, "opening", Some(path), err)])?;
            if options.seek > 0 {
                let offset = output
                    .seek(SeekFrom::Start((options.seek * options.block_size) as u64))
                    .map_err(|err| {
                        vec![AppletError::from_io(APPLET, "seeking", Some(path), err)]
                    })?;
                if options.count == Some(0) {
                    let current_len = output
                        .metadata()
                        .map_err(|err| {
                            vec![AppletError::from_io(APPLET, "reading", Some(path), err)]
                        })?
                        .len();
                    if offset > current_len {
                        output.set_len(offset).map_err(|err| {
                            vec![AppletError::from_io(APPLET, "writing", Some(path), err)]
                        })?;
                    }
                    return Ok(());
                }
            }
            copy_blocks(&mut input, &mut output, options.block_size, options.count)
                .map_err(|err| vec![AppletError::from_io(APPLET, "copying", Some(path), err)])?;
        }
        None => {
            let mut output = stdout();
            copy_blocks(&mut input, &mut output, options.block_size, options.count)
                .map_err(|err| vec![AppletError::from_io(APPLET, "copying", None, err)])?;
            output
                .flush()
                .map_err(|err| vec![AppletError::from_io(APPLET, "writing stdout", None, err)])?;
        }
    }

    Ok(())
}

fn parse_args(args: &[String]) -> Result<Options, Vec<AppletError>> {
    let mut options = Options::default();
    for arg in args {
        let Some((key, value)) = arg.split_once('=') else {
            return Err(vec![AppletError::new(
                APPLET,
                format!("unrecognized operand '{arg}'"),
            )]);
        };
        match key {
            "if" => options.input = value.to_string(),
            "of" => options.output = Some(value.to_string()),
            "bs" => options.block_size = parse_positive(value, key)?,
            "count" => options.count = Some(parse_nonnegative(value, key)?),
            "skip" => options.skip = parse_nonnegative(value, key)?,
            "seek" => options.seek = parse_nonnegative(value, key)?,
            _ => {
                return Err(vec![AppletError::new(
                    APPLET,
                    format!("unsupported operand '{key}'"),
                )]);
            }
        }
    }
    Ok(options)
}

fn parse_positive(text: &str, label: &str) -> Result<usize, Vec<AppletError>> {
    match parse_suffixed_usize(text) {
        Ok(0) | Err(_) => Err(vec![AppletError::new(
            APPLET,
            format!("invalid {label} '{text}'"),
        )]),
        Ok(value) => Ok(value),
    }
}

fn parse_nonnegative(text: &str, label: &str) -> Result<usize, Vec<AppletError>> {
    parse_suffixed_usize(text).map_err(|_| {
        vec![AppletError::new(
            APPLET,
            format!("invalid {label} '{text}'"),
        )]
    })
}

fn parse_suffixed_usize(text: &str) -> Result<usize, ()> {
    let split = text
        .find(|ch: char| !ch.is_ascii_digit())
        .unwrap_or(text.len());
    let (digits, suffix) = text.split_at(split);
    if digits.is_empty() {
        return Err(());
    }
    let value = digits.parse::<usize>().map_err(|_| ())?;
    let multiplier = match suffix {
        "" | "c" | "C" => 1,
        "b" | "B" => 512,
        "k" | "K" => 1024,
        "m" | "M" => 1024 * 1024,
        "g" | "G" => 1024 * 1024 * 1024,
        _ => return Err(()),
    };
    value.checked_mul(multiplier).ok_or(())
}

fn skip_blocks<R: Read>(reader: &mut R, blocks: usize, block_size: usize) -> std::io::Result<()> {
    let mut remaining = blocks * block_size;
    let mut buffer = vec![0_u8; block_size.max(1)];
    while remaining > 0 {
        let chunk = remaining.min(buffer.len());
        let read = reader.read(&mut buffer[..chunk])?;
        if read == 0 {
            break;
        }
        remaining -= read;
    }
    Ok(())
}

fn copy_blocks<R: Read, W: Write>(
    reader: &mut R,
    writer: &mut W,
    block_size: usize,
    count: Option<usize>,
) -> std::io::Result<()> {
    let mut buffer = vec![0_u8; block_size];
    let mut copied = 0_usize;
    loop {
        if count.is_some_and(|count| copied >= count) {
            return Ok(());
        }
        let read = reader.read(&mut buffer)?;
        if read == 0 {
            return Ok(());
        }
        writer.write_all(&buffer[..read])?;
        copied += 1;
    }
}

#[cfg(test)]
mod tests {
    use std::fs;

    use super::{parse_args, parse_suffixed_usize, run};

    fn args(values: &[&str]) -> Vec<String> {
        values.iter().map(|value| value.to_string()).collect()
    }

    #[test]
    fn parses_common_operands() {
        let options = parse_args(&args(&[
            "if=input",
            "of=output",
            "bs=2",
            "count=3",
            "skip=1",
            "seek=4",
        ]))
        .expect("parse dd");
        assert_eq!(options.input, "input");
        assert_eq!(options.output.as_deref(), Some("output"));
        assert_eq!(options.block_size, 2);
        assert_eq!(options.count, Some(3));
        assert_eq!(options.skip, 1);
        assert_eq!(options.seek, 4);
    }

    #[test]
    fn seek_with_zero_count_extends_sparse_output() {
        let path = std::env::temp_dir().join(format!("seed-dd-{}", std::process::id()));
        let args = args(&[
            "if=/dev/zero",
            &format!("of={}", path.display()),
            "bs=1",
            "count=0",
            "seek=16",
        ]);
        run(&args).expect("run dd");
        assert_eq!(fs::metadata(&path).expect("metadata").len(), 16);
        let _ = fs::remove_file(path);
    }

    #[test]
    fn parses_busybox_style_size_suffixes() {
        assert_eq!(parse_suffixed_usize("10k").unwrap(), 10 * 1024);
        assert_eq!(parse_suffixed_usize("2M").unwrap(), 2 * 1024 * 1024);
        assert_eq!(parse_suffixed_usize("4b").unwrap(), 4 * 512);
    }
}
