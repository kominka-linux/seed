use std::ffi::OsString;
use std::io::{self, Read, Write};

use crate::common::args::argv_to_strings;
use crate::common::error::AppletError;
use crate::common::io::open_input;

const APPLET: &str = "od";

#[derive(Clone, Copy, Debug)]
enum Format {
    Octal16,
    Octal8,
    NamedChar,
    Char,
    Decimal16,
    Signed16,
    Decimal32,
    Signed32,
    Octal32,
    Hex16,
    Hex32,
    Float32,
    Signed64,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum AddressBase {
    Octal,
    Decimal,
    Hexadecimal,
    None,
}

#[derive(Clone, Copy, Debug)]
struct Options {
    format: Format,
    address_base: AddressBase,
    byte_limit: Option<usize>,
}

pub fn main(args: &[OsString]) -> i32 {
    match run(args) {
        Ok(()) => 0,
        Err(message) => {
            eprintln!("{APPLET}: {message}");
            1
        }
    }
}

fn run(args: &[OsString]) -> Result<(), String> {
    let args = argv_to_strings(APPLET, args).map_err(|errors| {
        errors
            .into_iter()
            .map(|error| error.to_string())
            .collect::<Vec<_>>()
            .join("\n")
    })?;
    let (options, files) = parse_args(&args)?;
    let path = files.first().map(String::as_str).unwrap_or("-");
    let mut input = open_input(path).map_err(|err| format!("opening {path}: {err}"))?;

    let mut stdout = io::stdout().lock();
    match options.format {
        Format::Octal16 => format_words(&mut stdout, &mut input, &options, 2, |chunk| {
            format!("{:06o}", u16::from_ne_bytes([chunk[0], chunk[1]]))
        })?,
        Format::Signed16 => format_words(&mut stdout, &mut input, &options, 2, |chunk| {
            format!("{:6}", i16::from_ne_bytes([chunk[0], chunk[1]]))
        })?,
        Format::Octal8 => format_words(&mut stdout, &mut input, &options, 1, |chunk| {
            format!("{:03o}", chunk[0])
        })?,
        Format::NamedChar => format_words(&mut stdout, &mut input, &options, 1, |chunk| {
            format!("{:>3}", named_char_repr(chunk[0]))
        })?,
        Format::Char => format_words(&mut stdout, &mut input, &options, 1, |chunk| {
            format!("{:>3}", char_repr(chunk[0]))
        })?,
        Format::Decimal16 => format_words(&mut stdout, &mut input, &options, 2, |chunk| {
            format!("{:5}", u16::from_ne_bytes([chunk[0], chunk[1]]))
        })?,
        Format::Decimal32 => format_words(&mut stdout, &mut input, &options, 4, |chunk| {
            format!(
                "{:10}",
                u32::from_ne_bytes([chunk[0], chunk[1], chunk[2], chunk[3]])
            )
        })?,
        Format::Signed32 => format_words(&mut stdout, &mut input, &options, 4, |chunk| {
            format!(
                "{:11}",
                i32::from_ne_bytes([chunk[0], chunk[1], chunk[2], chunk[3]])
            )
        })?,
        Format::Octal32 => format_words(&mut stdout, &mut input, &options, 4, |chunk| {
            format!(
                "{:011o}",
                u32::from_ne_bytes([chunk[0], chunk[1], chunk[2], chunk[3]])
            )
        })?,
        Format::Hex16 => format_words(&mut stdout, &mut input, &options, 2, |chunk| {
            format!("{:04x}", u16::from_ne_bytes([chunk[0], chunk[1]]))
        })?,
        Format::Hex32 => format_words(&mut stdout, &mut input, &options, 4, |chunk| {
            format!(
                "{:08x}",
                u32::from_ne_bytes([chunk[0], chunk[1], chunk[2], chunk[3]])
            )
        })?,
        Format::Float32 => format_words(&mut stdout, &mut input, &options, 4, |chunk| {
            format!(
                "{:>15}",
                scientific_repr(
                    f32::from_ne_bytes([chunk[0], chunk[1], chunk[2], chunk[3]]) as f64,
                    7
                )
            )
        })?,
        Format::Signed64 => format_words(&mut stdout, &mut input, &options, 8, |chunk| {
            format!(
                "{:20}",
                i64::from_ne_bytes([
                    chunk[0], chunk[1], chunk[2], chunk[3], chunk[4], chunk[5], chunk[6], chunk[7]
                ])
            )
        })?,
    }
    Ok(())
}

fn parse_args(args: &[String]) -> Result<(Options, Vec<String>), String> {
    let mut options = Options {
        format: Format::Octal16,
        address_base: AddressBase::Octal,
        byte_limit: None,
    };
    let mut files = Vec::new();
    let mut parsing_flags = true;
    let mut index = 0;

    while let Some(arg) = args.get(index) {
        index += 1;
        if parsing_flags && arg == "--" {
            parsing_flags = false;
            continue;
        }
        if parsing_flags && arg == "--traditional" {
            continue;
        }
        if parsing_flags && arg == "-A" {
            let value = args
                .get(index)
                .ok_or_else(|| AppletError::option_requires_arg(APPLET, "A").to_string())?;
            index += 1;
            options.address_base = parse_address_base(value)?;
            continue;
        }
        if parsing_flags && arg == "-N" {
            let value = args
                .get(index)
                .ok_or_else(|| AppletError::option_requires_arg(APPLET, "N").to_string())?;
            index += 1;
            options.byte_limit = Some(parse_count(value)?);
            continue;
        }
        if parsing_flags && arg == "-t" {
            let value = args
                .get(index)
                .ok_or_else(|| AppletError::option_requires_arg(APPLET, "t").to_string())?;
            index += 1;
            options.format = parse_type_string(value)?;
            continue;
        }
        if parsing_flags && arg.starts_with('-') && arg.len() > 1 {
            for flag in arg[1..].chars() {
                options.format = match flag {
                    'B' | 'o' => Format::Octal16,
                    'b' => Format::Octal8,
                    'a' => Format::NamedChar,
                    'c' => Format::Char,
                    'd' => Format::Decimal16,
                    'D' => Format::Decimal32,
                    'f' => Format::Float32,
                    'H' | 'X' => Format::Hex32,
                    'h' | 'x' => Format::Hex16,
                    'i' => Format::Signed32,
                    'I' | 'L' | 'l' => Format::Signed64,
                    'O' => Format::Octal32,
                    's' => Format::Signed16,
                    _ => return Err(AppletError::invalid_option_message(flag)),
                };
            }
            continue;
        }
        files.push(arg.clone());
    }

    Ok((options, files))
}

fn parse_address_base(value: &str) -> Result<AddressBase, String> {
    match value {
        "o" => Ok(AddressBase::Octal),
        "d" => Ok(AddressBase::Decimal),
        "x" => Ok(AddressBase::Hexadecimal),
        "n" => Ok(AddressBase::None),
        _ => Err(format!("invalid address base '{value}'")),
    }
}

fn parse_count(value: &str) -> Result<usize, String> {
    value
        .parse::<usize>()
        .map_err(|_| format!("invalid count '{value}'"))
}

fn parse_type_string(value: &str) -> Result<Format, String> {
    match value {
        "a" => Ok(Format::NamedChar),
        "c" => Ok(Format::Char),
        _ => Err(format!("unsupported type string '{value}'")),
    }
}

fn format_words(
    stdout: &mut impl Write,
    input: &mut impl Read,
    options: &Options,
    unit: usize,
    render: impl Fn(&[u8]) -> String,
) -> Result<(), String> {
    let mut bytes = [0_u8; 16];
    let mut offset = 0_usize;
    let mut remaining = options.byte_limit.unwrap_or(usize::MAX);
    loop {
        if remaining == 0 {
            break;
        }
        let mut filled = 0;
        let limit = remaining.min(bytes.len());
        while filled < limit {
            let read = input
                .read(&mut bytes[filled..limit])
                .map_err(|err| format!("reading input: {err}"))?;
            if read == 0 {
                break;
            }
            filled += read;
        }
        if filled == 0 {
            break;
        }

        write_offset(stdout, options.address_base, offset)?;
        let mut index = 0;
        while index + unit <= filled {
            let value = render(&bytes[index..index + unit]);
            write!(stdout, " {}", value).map_err(|err| format!("writing stdout: {err}"))?;
            index += unit;
        }
        writeln!(stdout).map_err(|err| format!("writing stdout: {err}"))?;
        offset += filled;
        remaining = remaining.saturating_sub(filled);
    }
    write_offset(stdout, options.address_base, offset)?;
    writeln!(stdout).map_err(|err| format!("writing stdout: {err}"))?;
    Ok(())
}

fn write_offset(stdout: &mut impl Write, base: AddressBase, offset: usize) -> Result<(), String> {
    match base {
        AddressBase::Octal => {
            write!(stdout, "{offset:07o}").map_err(|err| format!("writing stdout: {err}"))
        }
        AddressBase::Decimal => {
            write!(stdout, "{offset:07}").map_err(|err| format!("writing stdout: {err}"))
        }
        AddressBase::Hexadecimal => {
            write!(stdout, "{offset:07x}").map_err(|err| format!("writing stdout: {err}"))
        }
        AddressBase::None => Ok(()),
    }
}

fn char_repr(byte: u8) -> String {
    match byte {
        b'\n' => String::from("\\n"),
        b'\t' => String::from("ht"),
        0x20..=0x7e => (byte as char).to_string(),
        _ => format!("{:03o}", byte),
    }
}

fn named_char_repr(byte: u8) -> String {
    match byte & 0x7f {
        0 => String::from("nul"),
        1 => String::from("soh"),
        2 => String::from("stx"),
        3 => String::from("etx"),
        4 => String::from("eot"),
        5 => String::from("enq"),
        6 => String::from("ack"),
        7 => String::from("bel"),
        8 => String::from("bs"),
        9 => String::from("ht"),
        10 => String::from("nl"),
        11 => String::from("vt"),
        12 => String::from("ff"),
        13 => String::from("cr"),
        14 => String::from("so"),
        15 => String::from("si"),
        16 => String::from("dle"),
        17 => String::from("dc1"),
        18 => String::from("dc2"),
        19 => String::from("dc3"),
        20 => String::from("dc4"),
        21 => String::from("nak"),
        22 => String::from("syn"),
        23 => String::from("etb"),
        24 => String::from("can"),
        25 => String::from("em"),
        26 => String::from("sub"),
        27 => String::from("esc"),
        28 => String::from("fs"),
        29 => String::from("gs"),
        30 => String::from("rs"),
        31 => String::from("us"),
        127 => String::from("del"),
        value => (value as char).to_string(),
    }
}

fn scientific_repr(value: f64, precision: usize) -> String {
    let rendered = format!("{value:.precision$e}");
    let Some((mantissa, exponent)) = rendered.split_once('e') else {
        return rendered;
    };
    let exponent_value = exponent.parse::<i32>().unwrap_or(0);
    format!("{mantissa}e{exponent_value:+03}")
}

#[cfg(test)]
mod tests {
    use super::{AddressBase, Format, parse_args};

    fn args(values: &[&str]) -> Vec<String> {
        values.iter().map(|value| value.to_string()).collect()
    }

    #[test]
    fn parses_gnu_style_options_used_by_sample_calls() {
        let (options, files) = parse_args(&args(&["-A", "o", "-t", "c", "-N", "18", "input"]))
            .expect("parse od args");

        assert!(matches!(options.address_base, AddressBase::Octal));
        assert!(matches!(options.format, Format::Char));
        assert_eq!(options.byte_limit, Some(18));
        assert_eq!(files, vec![String::from("input")]);
    }
}
