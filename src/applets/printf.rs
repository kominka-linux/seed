use std::io::{self, Write};

const APPLET: &str = "printf";

#[derive(Clone, Debug)]
struct Format {
    parts: Vec<Part>,
    stop_after_cycle: bool,
}

#[derive(Clone, Debug)]
enum Part {
    Literal(Vec<u8>),
    Spec(FormatSpec),
}

#[derive(Clone, Debug)]
struct FormatSpec {
    kind: SpecKind,
    zero_pad: bool,
    left_justify: bool,
    space_sign: bool,
    width: CountSpec,
    precision: Option<CountSpec>,
}

#[derive(Clone, Debug)]
enum CountSpec {
    Number(isize),
    Star,
}

#[derive(Clone, Copy, Debug)]
enum SpecKind {
    String,
    Bytes,
    Decimal,
    Integer,
    Hex,
    Float,
}

pub fn main(args: &[String]) -> i32 {
    match run(args) {
        Ok(code) => code,
        Err(message) => {
            eprintln!("{APPLET}: {message}");
            1
        }
    }
}

fn run(args: &[String]) -> Result<i32, String> {
    let Some((format, argv)) = args.split_first() else {
        return Ok(0);
    };

    let format = parse_format(format.as_bytes())?;
    let consumes_args = format
        .parts
        .iter()
        .any(|part| matches!(part, Part::Spec(_)));

    let mut stdout = io::stdout().lock();
    let mut stderr = io::stderr().lock();
    let mut arg_index = 0_usize;
    let mut had_error = false;
    let mut first_cycle = true;

    loop {
        let cycle_start = arg_index;
        let mut stop = false;
        for part in &format.parts {
            match part {
                Part::Literal(bytes) => stdout
                    .write_all(bytes)
                    .map_err(|err| format!("writing stdout: {err}"))?,
                Part::Spec(spec) => {
                    if render_spec(
                        spec,
                        argv,
                        &mut arg_index,
                        &mut stdout,
                        &mut stderr,
                        &mut had_error,
                    )? {
                        stop = true;
                        break;
                    }
                }
            }
        }
        if stop || format.stop_after_cycle {
            break;
        }
        if first_cycle {
            first_cycle = false;
            if argv.is_empty() {
                break;
            }
        }
        if !consumes_args || arg_index >= argv.len() || arg_index == cycle_start {
            break;
        }
    }

    stdout
        .flush()
        .map_err(|err| format!("flushing stdout: {err}"))?;
    stderr
        .flush()
        .map_err(|err| format!("flushing stderr: {err}"))?;
    Ok(if had_error { 1 } else { 0 })
}

fn parse_format(input: &[u8]) -> Result<Format, String> {
    let mut parts = Vec::new();
    let mut literal = Vec::new();
    let mut index = 0;
    let mut stop_after_cycle = false;

    while index < input.len() {
        match input[index] {
            b'%' => {
                if !literal.is_empty() {
                    parts.push(Part::Literal(std::mem::take(&mut literal)));
                }
                let (spec, next) = parse_spec(input, index + 1)?;
                index = next;
                match spec {
                    ParsedSpec::Percent => literal.push(b'%'),
                    ParsedSpec::Spec(spec) => parts.push(Part::Spec(spec)),
                }
            }
            b'\\' => {
                let (escaped, next, stop) = parse_escape(input, index + 1)?;
                literal.extend_from_slice(&escaped);
                index = next;
                if stop {
                    stop_after_cycle = true;
                    break;
                }
            }
            byte => {
                literal.push(byte);
                index += 1;
            }
        }
    }

    if !literal.is_empty() {
        parts.push(Part::Literal(literal));
    }
    Ok(Format {
        parts,
        stop_after_cycle,
    })
}

enum ParsedSpec {
    Percent,
    Spec(FormatSpec),
}

fn parse_spec(input: &[u8], mut index: usize) -> Result<(ParsedSpec, usize), String> {
    if index >= input.len() {
        return Err("%: invalid format".to_owned());
    }

    let mut zero_pad = false;
    let mut left_justify = false;
    let mut space_sign = false;
    while index < input.len() {
        match input[index] {
            b'0' => zero_pad = true,
            b'-' => left_justify = true,
            b' ' => space_sign = true,
            b'+' | b'#' => {}
            _ => break,
        }
        index += 1;
    }

    let (width, next) = parse_count(input, index);
    index = next;
    let precision = if index < input.len() && input[index] == b'.' {
        let (precision, next) = parse_count(input, index + 1);
        index = next;
        Some(precision)
    } else {
        None
    };

    while index < input.len() && matches!(input[index], b'h' | b'l' | b'L' | b'z' | b'j' | b't') {
        index += 1;
    }

    if index >= input.len() {
        return Err("%: invalid format".to_owned());
    }

    let kind = match input[index] {
        b'%' => return Ok((ParsedSpec::Percent, index + 1)),
        b's' => SpecKind::String,
        b'b' => SpecKind::Bytes,
        b'd' => SpecKind::Decimal,
        b'i' => SpecKind::Integer,
        b'x' => SpecKind::Hex,
        b'f' => SpecKind::Float,
        other => return Err(format!("%{}: invalid format", other as char)),
    };

    Ok((
        ParsedSpec::Spec(FormatSpec {
            kind,
            zero_pad,
            left_justify,
            space_sign,
            width,
            precision,
        }),
        index + 1,
    ))
}

fn parse_count(input: &[u8], mut index: usize) -> (CountSpec, usize) {
    if index < input.len() && input[index] == b'*' {
        return (CountSpec::Star, index + 1);
    }
    let start = index;
    while index < input.len() && input[index].is_ascii_digit() {
        index += 1;
    }
    if start == index {
        (CountSpec::Number(0), index)
    } else {
        let value = std::str::from_utf8(&input[start..index])
            .ok()
            .and_then(|value| value.parse::<isize>().ok())
            .unwrap_or(0);
        (CountSpec::Number(value), index)
    }
}

fn parse_escape(input: &[u8], index: usize) -> Result<(Vec<u8>, usize, bool), String> {
    if index >= input.len() {
        return Ok((vec![b'\\'], index, false));
    }
    let byte = input[index];
    let (out, next, stop) = match byte {
        b'a' => (vec![0x07], index + 1, false),
        b'b' => (vec![0x08], index + 1, false),
        b'c' => (Vec::new(), index + 1, true),
        b'f' => (vec![0x0c], index + 1, false),
        b'n' => (vec![b'\n'], index + 1, false),
        b'r' => (vec![b'\r'], index + 1, false),
        b't' => (vec![b'\t'], index + 1, false),
        b'v' => (vec![0x0b], index + 1, false),
        b'\\' => (vec![b'\\'], index + 1, false),
        b'0'..=b'7' => {
            let mut value = 0_u8;
            let mut next = index;
            let mut count = 0;
            while next < input.len() && count < 3 && matches!(input[next], b'0'..=b'7') {
                value = value.saturating_mul(8).saturating_add(input[next] - b'0');
                next += 1;
                count += 1;
            }
            (vec![value], next, false)
        }
        b'x' => {
            let mut next = index + 1;
            let mut value = 0_u8;
            let mut count = 0;
            while next < input.len() && count < 2 {
                let digit = match input[next] {
                    b'0'..=b'9' => input[next] - b'0',
                    b'a'..=b'f' => input[next] - b'a' + 10,
                    b'A'..=b'F' => input[next] - b'A' + 10,
                    _ => break,
                };
                value = value.saturating_mul(16).saturating_add(digit);
                next += 1;
                count += 1;
            }
            (vec![value], next, false)
        }
        other => (vec![b'\\', other], index + 1, false),
    };
    Ok((out, next, stop))
}

fn render_spec(
    spec: &FormatSpec,
    argv: &[String],
    arg_index: &mut usize,
    stdout: &mut impl Write,
    stderr: &mut impl Write,
    had_error: &mut bool,
) -> Result<bool, String> {
    let width = resolve_count(&spec.width, argv, arg_index, stderr, had_error)?;
    let precision = match &spec.precision {
        Some(precision) => Some(resolve_count(
            precision, argv, arg_index, stderr, had_error,
        )?),
        None => None,
    };

    let arg = next_arg(argv, arg_index);
    let rendered = match spec.kind {
        SpecKind::String => render_string(arg, width, precision, spec.left_justify),
        SpecKind::Bytes => {
            let (bytes, stop) = expand_bytes_argument(arg.as_bytes())?;
            let rendered = apply_width(bytes, width, spec.left_justify, false);
            stdout
                .write_all(&rendered)
                .map_err(|err| format!("writing stdout: {err}"))?;
            return Ok(stop);
        }
        SpecKind::Decimal | SpecKind::Integer => {
            let number = parse_integer(arg, stderr, had_error)?;
            render_integer(number, 10, width, precision, spec)
        }
        SpecKind::Hex => {
            let number = parse_integer(arg, stderr, had_error)?;
            render_hex(number, width, precision, spec)
        }
        SpecKind::Float => {
            let number = parse_float(arg, stderr, had_error)?;
            render_float(number, width, precision, spec)
        }
    };

    stdout
        .write_all(&rendered)
        .map_err(|err| format!("writing stdout: {err}"))?;
    Ok(false)
}

fn expand_bytes_argument(input: &[u8]) -> Result<(Vec<u8>, bool), String> {
    let mut output = Vec::new();
    let mut index = 0;

    while index < input.len() {
        if input[index] == b'\\' {
            let (escaped, next, stop) = parse_escape(input, index + 1)?;
            output.extend_from_slice(&escaped);
            index = next;
            if stop {
                return Ok((output, true));
            }
        } else {
            output.push(input[index]);
            index += 1;
        }
    }

    Ok((output, false))
}

fn resolve_count(
    count: &CountSpec,
    argv: &[String],
    arg_index: &mut usize,
    stderr: &mut impl Write,
    had_error: &mut bool,
) -> Result<isize, String> {
    match count {
        CountSpec::Number(value) => Ok(*value),
        CountSpec::Star => {
            Ok(parse_integer(next_arg(argv, arg_index), stderr, had_error)? as isize)
        }
    }
}

fn next_arg<'a>(argv: &'a [String], arg_index: &mut usize) -> &'a str {
    let value = argv.get(*arg_index).map(String::as_str).unwrap_or("");
    if *arg_index < argv.len() {
        *arg_index += 1;
    }
    value
}

fn parse_integer(arg: &str, stderr: &mut impl Write, had_error: &mut bool) -> Result<i64, String> {
    let trimmed = arg.trim_start();
    if trimmed.is_empty() {
        return Ok(0);
    }
    if let Some(value) = parse_quoted_char(trimmed) {
        return Ok(value as i64);
    }
    match trimmed.parse::<i64>() {
        Ok(value) => Ok(value),
        Err(_) => {
            *had_error = true;
            writeln!(stderr, "{APPLET}: invalid number '{arg}'")
                .map_err(|err| format!("writing stderr: {err}"))?;
            Ok(0)
        }
    }
}

fn parse_float(arg: &str, stderr: &mut impl Write, had_error: &mut bool) -> Result<f64, String> {
    let trimmed = arg.trim_start();
    if trimmed.is_empty() {
        return Ok(0.0);
    }
    match trimmed.parse::<f64>() {
        Ok(value) => Ok(value),
        Err(_) => {
            *had_error = true;
            writeln!(stderr, "{APPLET}: invalid number '{arg}'")
                .map_err(|err| format!("writing stderr: {err}"))?;
            Ok(0.0)
        }
    }
}

fn parse_quoted_char(arg: &str) -> Option<u8> {
    let bytes = arg.as_bytes();
    if bytes.len() >= 2 && (bytes[0] == b'\'' || bytes[0] == b'"') {
        Some(bytes[1])
    } else {
        None
    }
}

fn render_string(arg: &str, width: isize, precision: Option<isize>, left_justify: bool) -> Vec<u8> {
    let mut bytes = arg.as_bytes().to_vec();
    if let Some(precision) = precision.filter(|precision| *precision >= 0) {
        bytes.truncate(precision as usize);
    }
    apply_width(bytes, width, left_justify, false)
}

fn render_integer(
    value: i64,
    radix: u32,
    width: isize,
    precision: Option<isize>,
    spec: &FormatSpec,
) -> Vec<u8> {
    let negative = value < 0;
    let digits = if radix == 10 {
        value.unsigned_abs().to_string()
    } else {
        format!("{}", value.unsigned_abs())
    };
    let mut bytes = apply_integer_precision(digits.as_bytes().to_vec(), precision);
    if negative {
        bytes.insert(0, b'-');
    } else if spec.space_sign {
        bytes.insert(0, b' ');
    }
    apply_width(
        bytes,
        width,
        spec.left_justify,
        spec.zero_pad && precision.is_none(),
    )
}

fn render_hex(value: i64, width: isize, precision: Option<isize>, spec: &FormatSpec) -> Vec<u8> {
    let digits = format!("{:x}", value.unsigned_abs());
    let bytes = apply_integer_precision(digits.into_bytes(), precision);
    apply_width(
        bytes,
        width,
        spec.left_justify,
        spec.zero_pad && precision.is_none(),
    )
}

fn render_float(value: f64, width: isize, precision: Option<isize>, spec: &FormatSpec) -> Vec<u8> {
    let precision = precision.filter(|precision| *precision >= 0).unwrap_or(6) as usize;
    let mut rendered = format!("{value:.precision$}").into_bytes();
    if value >= 0.0 && spec.space_sign {
        rendered.insert(0, b' ');
    }
    apply_width(rendered, width, spec.left_justify, spec.zero_pad)
}

fn apply_integer_precision(mut bytes: Vec<u8>, precision: Option<isize>) -> Vec<u8> {
    if let Some(precision) = precision.filter(|precision| *precision >= 0) {
        while bytes.len() < precision as usize {
            bytes.insert(0, b'0');
        }
    }
    bytes
}

fn apply_width(mut bytes: Vec<u8>, width: isize, left_justify: bool, zero_pad: bool) -> Vec<u8> {
    let mut pad_width = width;
    let mut left = left_justify;
    if pad_width < 0 {
        pad_width = -pad_width;
        left = true;
    }
    let pad_width = pad_width.max(0) as usize;
    if bytes.len() >= pad_width {
        return bytes;
    }
    let fill = if zero_pad { b'0' } else { b' ' };
    let mut pad = vec![fill; pad_width - bytes.len()];
    if left {
        bytes.append(&mut pad);
        bytes
    } else if fill == b'0' && matches!(bytes.first(), Some(b'-' | b' ')) {
        let sign = bytes.remove(0);
        let mut output = vec![sign];
        output.append(&mut pad);
        output.extend_from_slice(&bytes);
        output
    } else {
        pad.extend_from_slice(&bytes);
        pad
    }
}

#[cfg(test)]
mod tests {
    use super::{CountSpec, parse_count, render_string};

    #[test]
    fn absent_width_does_not_pad_empty_string() {
        assert_eq!(render_string("", 0, None, false), b"");
    }

    #[test]
    fn parse_count_defaults_to_zero_width() {
        assert!(matches!(parse_count(b"s", 0), (CountSpec::Number(0), 0)));
    }
}
