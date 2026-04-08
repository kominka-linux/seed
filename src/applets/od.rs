use std::io::{self, Read, Write};

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

pub fn main(args: &[String]) -> i32 {
    match run(args) {
        Ok(()) => 0,
        Err(message) => {
            eprintln!("{APPLET}: {message}");
            1
        }
    }
}

fn run(args: &[String]) -> Result<(), String> {
    let (format, files) = parse_args(args)?;
    let path = files.first().map(String::as_str).unwrap_or("-");
    let mut input = open_input(path).map_err(|err| format!("opening {path}: {err}"))?;

    let mut stdout = io::stdout().lock();
    match format {
        Format::Octal16 => format_words(&mut stdout, &mut input, 2, |chunk| {
            format!("{:06o}", u16::from_ne_bytes([chunk[0], chunk[1]]))
        })?,
        Format::Signed16 => format_words(&mut stdout, &mut input, 2, |chunk| {
            format!("{:6}", i16::from_ne_bytes([chunk[0], chunk[1]]))
        })?,
        Format::Octal8 => format_words(&mut stdout, &mut input, 1, |chunk| {
            format!("{:03o}", chunk[0])
        })?,
        Format::NamedChar => format_words(&mut stdout, &mut input, 1, |chunk| {
            format!("{:>3}", named_char_repr(chunk[0]))
        })?,
        Format::Char => format_words(&mut stdout, &mut input, 1, |chunk| {
            format!("{:>3}", char_repr(chunk[0]))
        })?,
        Format::Decimal16 => format_words(&mut stdout, &mut input, 2, |chunk| {
            format!("{:5}", u16::from_ne_bytes([chunk[0], chunk[1]]))
        })?,
        Format::Decimal32 => format_words(&mut stdout, &mut input, 4, |chunk| {
            format!(
                "{:10}",
                u32::from_ne_bytes([chunk[0], chunk[1], chunk[2], chunk[3]])
            )
        })?,
        Format::Signed32 => format_words(&mut stdout, &mut input, 4, |chunk| {
            format!(
                "{:11}",
                i32::from_ne_bytes([chunk[0], chunk[1], chunk[2], chunk[3]])
            )
        })?,
        Format::Octal32 => format_words(&mut stdout, &mut input, 4, |chunk| {
            format!(
                "{:011o}",
                u32::from_ne_bytes([chunk[0], chunk[1], chunk[2], chunk[3]])
            )
        })?,
        Format::Hex16 => format_words(&mut stdout, &mut input, 2, |chunk| {
            format!("{:04x}", u16::from_ne_bytes([chunk[0], chunk[1]]))
        })?,
        Format::Hex32 => format_words(&mut stdout, &mut input, 4, |chunk| {
            format!(
                "{:08x}",
                u32::from_ne_bytes([chunk[0], chunk[1], chunk[2], chunk[3]])
            )
        })?,
        Format::Float32 => format_words(&mut stdout, &mut input, 4, |chunk| {
            format!(
                "{:>15}",
                scientific_repr(
                    f32::from_ne_bytes([chunk[0], chunk[1], chunk[2], chunk[3]]) as f64,
                    7
                )
            )
        })?,
        Format::Signed64 => format_words(&mut stdout, &mut input, 8, |chunk| {
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

fn parse_args(args: &[String]) -> Result<(Format, Vec<String>), String> {
    let mut format = Format::Octal16;
    let mut files = Vec::new();
    let mut parsing_flags = true;

    for arg in args {
        if parsing_flags && arg == "--" {
            parsing_flags = false;
            continue;
        }
        if parsing_flags && arg == "--traditional" {
            continue;
        }
        if parsing_flags && arg.starts_with('-') && arg.len() > 1 {
            for flag in arg[1..].chars() {
                format = match flag {
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
                    _ => return Err(format!("invalid option -- '{flag}'")),
                };
            }
            continue;
        }
        files.push(arg.clone());
    }

    Ok((format, files))
}

fn format_words(
    stdout: &mut impl Write,
    input: &mut impl Read,
    unit: usize,
    render: impl Fn(&[u8]) -> String,
) -> Result<(), String> {
    let mut bytes = [0_u8; 16];
    let mut offset = 0_usize;
    loop {
        let mut filled = 0;
        while filled < bytes.len() {
            let read = input
                .read(&mut bytes[filled..])
                .map_err(|err| format!("reading input: {err}"))?;
            if read == 0 {
                break;
            }
            filled += read;
        }
        if filled == 0 {
            break;
        }

        write!(stdout, "{:07o}", offset).map_err(|err| format!("writing stdout: {err}"))?;
        let mut index = 0;
        while index + unit <= filled {
            let value = render(&bytes[index..index + unit]);
            write!(stdout, " {}", value).map_err(|err| format!("writing stdout: {err}"))?;
            index += unit;
        }
        writeln!(stdout).map_err(|err| format!("writing stdout: {err}"))?;
        offset += filled;
    }
    writeln!(stdout, "{:07o}", offset).map_err(|err| format!("writing stdout: {err}"))?;
    Ok(())
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
