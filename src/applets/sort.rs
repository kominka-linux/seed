use std::cmp::Ordering;
use std::fs;
use std::io::{BufRead, BufReader, Write};

use crate::common::applet::{AppletResult, finish};
use crate::common::error::AppletError;
use crate::common::io::{Input, open_input};

const APPLET: &str = "sort";

#[derive(Clone, Debug, Default)]
struct Options {
    mode: SortMode,
    reverse: bool,
    unique: bool,
    zero_terminated: bool,
    stable: bool,
    output: Option<String>,
    delimiter: Option<u8>,
    keys: Vec<KeySpec>,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
enum SortMode {
    #[default]
    Lexicographic,
    Numeric,
    HumanNumeric,
    Month,
}

#[derive(Clone, Debug)]
struct KeySpec {
    start_field: usize,
    start_char: Option<usize>,
    end_field: Option<usize>,
    end_char: Option<usize>,
    mode: Option<SortMode>,
    reverse: bool,
}

impl KeySpec {
    fn has_local_flags(&self) -> bool {
        self.mode.is_some() || self.reverse
    }
}

#[derive(Clone, Debug)]
struct ParsedKeyPart {
    field: usize,
    char_pos: Option<usize>,
    mode: Option<SortMode>,
    reverse: bool,
}

#[derive(Clone, Debug)]
struct Record {
    text: String,
    original_index: usize,
}

pub fn main(args: &[String]) -> i32 {
    finish(run(args))
}

fn run(args: &[String]) -> AppletResult {
    let (options, files) = parse_args(args)?;
    let inputs = if files.is_empty() {
        vec![String::from("-")]
    } else {
        files
    };

    let mut records = Vec::new();
    for path in &inputs {
        let input = open_input(path)
            .map_err(|err| vec![AppletError::from_io(APPLET, "opening", Some(path), err)])?;
        read_records(input, &mut records, options.zero_terminated)
            .map_err(|err| vec![AppletError::from_io(APPLET, "reading", Some(path), err)])?;
    }

    records.sort_by(|left, right| compare_records(left, right, &options, false));
    if options.unique {
        records.dedup_by(|left, right| {
            compare_records(left, right, &options, true) == Ordering::Equal
        });
    }

    write_records(&records, &options)
        .map_err(|err| vec![AppletError::from_io(APPLET, "writing", None, err)])?;
    Ok(())
}

fn parse_args(args: &[String]) -> Result<(Options, Vec<String>), Vec<AppletError>> {
    let mut options = Options::default();
    let mut files = Vec::new();
    let mut parsing_flags = true;
    let mut index = 0;

    while index < args.len() {
        let arg = &args[index];
        if parsing_flags && arg == "--" {
            parsing_flags = false;
            index += 1;
            continue;
        }
        if parsing_flags && arg == "-k" {
            let Some(spec) = args.get(index + 1) else {
                return Err(vec![AppletError::option_requires_arg(APPLET, "k")]);
            };
            options.keys.push(parse_key_spec(spec)?);
            index += 2;
            continue;
        }
        if parsing_flags && arg == "-o" {
            let Some(path) = args.get(index + 1) else {
                return Err(vec![AppletError::option_requires_arg(APPLET, "o")]);
            };
            options.output = Some(path.clone());
            index += 2;
            continue;
        }
        if parsing_flags && arg == "-t" {
            let Some(delimiter) = args.get(index + 1) else {
                return Err(vec![AppletError::option_requires_arg(APPLET, "t")]);
            };
            options.delimiter = Some(parse_delimiter(delimiter)?);
            index += 2;
            continue;
        }
        if parsing_flags && arg.starts_with('-') && arg.len() > 1 {
            let mut chars = arg[1..].chars().peekable();
            while let Some(flag) = chars.next() {
                match flag {
                    'n' => options.mode = SortMode::Numeric,
                    'r' => options.reverse = true,
                    'u' => options.unique = true,
                    'z' => options.zero_terminated = true,
                    's' => options.stable = true,
                    'h' => options.mode = SortMode::HumanNumeric,
                    'M' => options.mode = SortMode::Month,
                    'k' => {
                        let spec = if chars.peek().is_some() {
                            chars.collect::<String>()
                        } else {
                            index += 1;
                            let Some(spec) = args.get(index) else {
                                return Err(vec![AppletError::option_requires_arg(APPLET, "k")]);
                            };
                            spec.clone()
                        };
                        options.keys.push(parse_key_spec(&spec)?);
                        break;
                    }
                    'o' => {
                        let value = if chars.peek().is_some() {
                            chars.collect::<String>()
                        } else {
                            index += 1;
                            let Some(path) = args.get(index) else {
                                return Err(vec![AppletError::option_requires_arg(APPLET, "o")]);
                            };
                            path.clone()
                        };
                        options.output = Some(value);
                        break;
                    }
                    't' => {
                        let value = if chars.peek().is_some() {
                            chars.collect::<String>()
                        } else {
                            index += 1;
                            let Some(delimiter) = args.get(index) else {
                                return Err(vec![AppletError::option_requires_arg(APPLET, "t")]);
                            };
                            delimiter.clone()
                        };
                        options.delimiter = Some(parse_delimiter(&value)?);
                        break;
                    }
                    _ => return Err(vec![AppletError::invalid_option(APPLET, flag)]),
                }
            }
            index += 1;
            continue;
        }
        files.push(arg.clone());
        index += 1;
    }

    Ok((options, files))
}

fn parse_delimiter(value: &str) -> Result<u8, Vec<AppletError>> {
    let bytes = value.as_bytes();
    if bytes.len() != 1 {
        Err(vec![AppletError::new(APPLET, "bad -t parameter")])
    } else {
        Ok(bytes[0])
    }
}

fn parse_key_spec(spec: &str) -> Result<KeySpec, Vec<AppletError>> {
    let (start, end) = match spec.split_once(',') {
        Some((start, end)) => (start, Some(end)),
        None => (spec, None),
    };
    let start_part = parse_key_part(start)?;
    let (end_field, end_char, end_mode, end_reverse) = match end {
        Some(part) => {
            let parsed = parse_key_part(part)?;
            (
                Some(parsed.field),
                parsed.char_pos,
                parsed.mode,
                parsed.reverse,
            )
        }
        None => (None, None, None, false),
    };

    Ok(KeySpec {
        start_field: start_part.field,
        start_char: start_part.char_pos,
        end_field,
        end_char,
        mode: start_part.mode.or(end_mode),
        reverse: start_part.reverse || end_reverse,
    })
}

fn parse_key_part(part: &str) -> Result<ParsedKeyPart, Vec<AppletError>> {
    let bytes = part.as_bytes();
    let mut index = 0;
    while index < bytes.len() && bytes[index].is_ascii_digit() {
        index += 1;
    }
    if index == 0 {
        return Err(vec![AppletError::new(APPLET, "bad field specification")]);
    }
    let field = part[..index]
        .parse::<usize>()
        .ok()
        .filter(|field| *field > 0)
        .ok_or_else(|| vec![AppletError::new(APPLET, "bad field specification")])?;

    let mut char_pos = None;
    if index < bytes.len() && bytes[index] == b'.' {
        index += 1;
        let char_start = index;
        while index < bytes.len() && bytes[index].is_ascii_digit() {
            index += 1;
        }
        if char_start == index {
            return Err(vec![AppletError::new(APPLET, "bad field specification")]);
        }
        char_pos = Some(
            part[char_start..index]
                .parse::<usize>()
                .ok()
                .filter(|value| *value > 0)
                .ok_or_else(|| vec![AppletError::new(APPLET, "bad field specification")])?,
        );
    }

    let mut mode = None;
    let mut reverse = false;
    for flag in part[index..].chars() {
        match flag {
            'n' => mode = Some(SortMode::Numeric),
            'r' => reverse = true,
            'M' => mode = Some(SortMode::Month),
            'h' => mode = Some(SortMode::HumanNumeric),
            _ => return Err(vec![AppletError::new(APPLET, "unknown key option")]),
        }
    }

    Ok(ParsedKeyPart {
        field,
        char_pos,
        mode,
        reverse,
    })
}

fn read_records(
    input: Input,
    records: &mut Vec<Record>,
    zero_terminated: bool,
) -> std::io::Result<()> {
    let mut reader = BufReader::new(input);
    let delimiter = if zero_terminated { b'\0' } else { b'\n' };
    let mut record = Vec::new();

    loop {
        record.clear();
        let read = reader.read_until(delimiter, &mut record)?;
        if read == 0 {
            return Ok(());
        }
        if record.ends_with(&[delimiter]) {
            record.pop();
        }
        records.push(Record {
            text: String::from_utf8_lossy(&record).into_owned(),
            original_index: records.len(),
        });
    }
}

fn write_records(records: &[Record], options: &Options) -> std::io::Result<()> {
    let delimiter = if options.zero_terminated {
        b'\0'
    } else {
        b'\n'
    };
    if let Some(path) = &options.output {
        let mut output = fs::File::create(path)?;
        for record in records {
            output.write_all(record.text.as_bytes())?;
            output.write_all(&[delimiter])?;
        }
        output.flush()
    } else {
        let stdout = std::io::stdout();
        let mut output = stdout.lock();
        for record in records {
            output.write_all(record.text.as_bytes())?;
            output.write_all(&[delimiter])?;
        }
        output.flush()
    }
}

fn compare_records(
    left: &Record,
    right: &Record,
    options: &Options,
    disable_tie_break: bool,
) -> Ordering {
    let mut ordering = if options.keys.is_empty() {
        let mut ordering = compare_texts(&left.text, &right.text, options.mode);
        if options.reverse {
            ordering = ordering.reverse();
        }
        ordering
    } else {
        compare_keys(left, right, options)
    };

    if ordering == Ordering::Equal {
        if options.stable && !disable_tie_break {
            return left.original_index.cmp(&right.original_index);
        }
        if !disable_tie_break {
            ordering = left.text.cmp(&right.text);
            if options.reverse {
                ordering = ordering.reverse();
            }
        }
    }
    ordering
}

fn compare_keys(left: &Record, right: &Record, options: &Options) -> Ordering {
    for key in &options.keys {
        let mode = if key.has_local_flags() {
            key.mode.unwrap_or(SortMode::Lexicographic)
        } else {
            options.mode
        };
        let left_key = extract_key(&left.text, key, options.delimiter);
        let right_key = extract_key(&right.text, key, options.delimiter);
        let mut ordering = compare_texts(left_key, right_key, mode);
        let reverse = if key.has_local_flags() {
            key.reverse
        } else {
            options.reverse
        };
        if reverse {
            ordering = ordering.reverse();
        }
        if ordering != Ordering::Equal {
            return ordering;
        }
    }
    Ordering::Equal
}

fn compare_texts(left: &str, right: &str, mode: SortMode) -> Ordering {
    match mode {
        SortMode::Lexicographic => left.cmp(right),
        SortMode::Numeric => compare_numeric(left, right),
        SortMode::HumanNumeric => compare_human_numeric(left, right),
        SortMode::Month => compare_month(left, right),
    }
}

fn compare_numeric(left: &str, right: &str) -> Ordering {
    match (parse_leading_float(left), parse_leading_float(right)) {
        (None, None) => Ordering::Equal,
        (None, Some(_)) => Ordering::Less,
        (Some(_), None) => Ordering::Greater,
        (Some((left_num, _)), Some((right_num, _))) => {
            left_num.partial_cmp(&right_num).unwrap_or(Ordering::Equal)
        }
    }
}

fn compare_human_numeric(left: &str, right: &str) -> Ordering {
    let left_num = parse_leading_float(left);
    let right_num = parse_leading_float(right);
    match (left_num, right_num) {
        (None, None) => Ordering::Equal,
        (None, Some(_)) => Ordering::Less,
        (Some(_), None) => Ordering::Greater,
        (Some((left_value, left_tail)), Some((right_value, right_tail))) => {
            let left_scale = scale_suffix(left_tail);
            let right_scale = scale_suffix(right_tail);
            match left_scale.cmp(&right_scale) {
                Ordering::Equal => left_value
                    .partial_cmp(&right_value)
                    .unwrap_or(Ordering::Equal),
                ordering => ordering,
            }
        }
    }
}

fn compare_month(left: &str, right: &str) -> Ordering {
    match (parse_month(left), parse_month(right)) {
        (None, None) => Ordering::Equal,
        (None, Some(_)) => Ordering::Less,
        (Some(_), None) => Ordering::Greater,
        (Some(left_month), Some(right_month)) => left_month.cmp(&right_month),
    }
}

fn extract_key<'a>(line: &'a str, key: &KeySpec, delimiter: Option<u8>) -> &'a str {
    let bytes = line.as_bytes();
    let (mut start, field_end) = field_span(bytes, key.start_field, delimiter);
    let (_, mut end) = match key.end_field {
        Some(field) => field_span(bytes, field, delimiter),
        None => (start, bytes.len()),
    };

    if let Some(start_char) = key.start_char {
        start = start
            .saturating_add(start_char.saturating_sub(1))
            .min(field_end);
    }
    if let Some(end_field) = key.end_field {
        let (field_start, field_end) = field_span(bytes, end_field, delimiter);
        end = field_end;
        if let Some(end_char) = key.end_char {
            end = field_start.saturating_add(end_char).min(field_end);
        }
    }
    if end < start {
        end = start;
    }
    &line[start..end]
}

fn field_span(line: &[u8], field: usize, delimiter: Option<u8>) -> (usize, usize) {
    if field == 0 {
        return (0, 0);
    }
    match delimiter {
        Some(delimiter) => field_span_with_delimiter(line, field, delimiter),
        None => field_span_with_blanks(line, field),
    }
}

fn field_span_with_delimiter(line: &[u8], field: usize, delimiter: u8) -> (usize, usize) {
    let mut start = 0;
    let mut current = 1;
    while current < field {
        while start < line.len() && line[start] != delimiter {
            start += 1;
        }
        if start < line.len() {
            start += 1;
        }
        current += 1;
    }
    let mut end = start;
    while end < line.len() && line[end] != delimiter {
        end += 1;
    }
    (start.min(line.len()), end.min(line.len()))
}

fn field_span_with_blanks(line: &[u8], field: usize) -> (usize, usize) {
    let mut index = 0;
    let mut current = 1;
    while current < field {
        while index < line.len() && line[index].is_ascii_whitespace() {
            index += 1;
        }
        while index < line.len() && !line[index].is_ascii_whitespace() {
            index += 1;
        }
        current += 1;
    }
    while index < line.len() && line[index].is_ascii_whitespace() {
        index += 1;
    }
    let start = index;
    while index < line.len() && !line[index].is_ascii_whitespace() {
        index += 1;
    }
    (start, index)
}

fn parse_leading_float(input: &str) -> Option<(f64, &str)> {
    let trimmed = input.trim_start();
    if trimmed.is_empty() {
        return None;
    }

    let bytes = trimmed.as_bytes();
    let mut index = 0;
    if matches!(bytes.first(), Some(b'+' | b'-')) {
        index += 1;
    }
    let integer_start = index;
    while index < bytes.len() && bytes[index].is_ascii_digit() {
        index += 1;
    }
    if index < bytes.len() && bytes[index] == b'.' {
        index += 1;
        while index < bytes.len() && bytes[index].is_ascii_digit() {
            index += 1;
        }
    }
    if index == integer_start {
        return None;
    }

    if index < bytes.len() && matches!(bytes[index], b'e' | b'E') {
        let exponent_start = index;
        let mut exponent_index = index + 1;
        if exponent_index < bytes.len() && matches!(bytes[exponent_index], b'+' | b'-') {
            exponent_index += 1;
        }
        let digit_start = exponent_index;
        while exponent_index < bytes.len() && bytes[exponent_index].is_ascii_digit() {
            exponent_index += 1;
        }
        if digit_start != exponent_index {
            index = exponent_index;
        } else {
            index = exponent_start;
        }
    }

    let number = trimmed[..index].parse::<f64>().ok()?;
    Some((number, &trimmed[index..]))
}

fn scale_suffix(tail: &str) -> i32 {
    let suffixes = b"kmgtpezy";
    let bytes = tail.as_bytes();
    let Some(&byte) = bytes.first() else {
        return -1;
    };
    let lower = byte.to_ascii_lowercase();
    let Some(index) = suffixes.iter().position(|suffix| *suffix == lower) else {
        return -1;
    };
    if index != 0 && byte.is_ascii_lowercase() {
        -1
    } else {
        index as i32
    }
}

fn parse_month(input: &str) -> Option<u8> {
    let trimmed = input.trim_start();
    if trimmed.len() < 3 {
        return None;
    }
    let prefix = trimmed[..3].to_ascii_lowercase();
    match prefix.as_str() {
        "jan" => Some(0),
        "feb" => Some(1),
        "mar" => Some(2),
        "apr" => Some(3),
        "may" => Some(4),
        "jun" => Some(5),
        "jul" => Some(6),
        "aug" => Some(7),
        "sep" => Some(8),
        "oct" => Some(9),
        "nov" => Some(10),
        "dec" => Some(11),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::{
        KeySpec, Options, SortMode, compare_human_numeric, field_span_with_blanks,
        field_span_with_delimiter, parse_key_spec, scale_suffix,
    };
    use std::cmp::Ordering;

    #[test]
    fn parses_key_spec_with_range_and_flags() {
        let key = parse_key_spec("2,3rn").expect("parse key");
        assert_eq!(key.start_field, 2);
        assert_eq!(key.end_field, Some(3));
        assert_eq!(key.mode, Some(SortMode::Numeric));
        assert!(key.reverse);
    }

    #[test]
    fn delimiter_fields_preserve_leading_empty_fields() {
        assert_eq!(field_span_with_delimiter(b"/a/2", 2, b'/'), (1, 2));
        assert_eq!(field_span_with_delimiter(b"//a/2", 3, b'/'), (2, 3));
    }

    #[test]
    fn blank_fields_skip_leading_space() {
        assert_eq!(field_span_with_blanks(b"  abc  def", 1), (2, 5));
        assert_eq!(field_span_with_blanks(b"  abc  def", 2), (7, 10));
    }

    #[test]
    fn human_suffix_rules_match_busybox_cases() {
        assert_eq!(scale_suffix(""), -1);
        assert_eq!(scale_suffix("K"), 0);
        assert_eq!(scale_suffix("k"), 0);
        assert_eq!(scale_suffix("M"), 1);
        assert_eq!(scale_suffix("m"), -1);
        assert_eq!(compare_human_numeric("3e", "1023"), Ordering::Less);
        assert_eq!(compare_human_numeric("2K", "3000"), Ordering::Greater);
    }

    #[test]
    fn key_without_local_flags_can_inherit_global_mode() {
        let options = Options {
            mode: SortMode::Numeric,
            keys: vec![KeySpec {
                start_field: 2,
                start_char: None,
                end_field: None,
                end_char: None,
                mode: None,
                reverse: false,
            }],
            ..Options::default()
        };
        assert_eq!(options.mode, SortMode::Numeric);
        assert!(options.keys[0].mode.is_none());
    }
}
