use std::io::{self, BufRead, BufReader, Write};

use crate::common::applet::{AppletResult, finish};
use crate::common::error::AppletError;
use crate::common::io::{open_input, stdout};

const APPLET: &str = "cut";

pub fn main(args: &[String]) -> i32 {
    finish(run(args))
}

#[derive(Debug, Clone)]
struct Range {
    // 1-based; None means "from the beginning" or "to the end"
    start: Option<usize>,
    end: Option<usize>,
}

#[derive(Debug)]
enum Mode {
    Bytes(Vec<Range>),
    Chars(Vec<Range>),
    Fields {
        ranges: Vec<Range>,
        delim: u8,
        suppress: bool,
    },
}

fn run(args: &[String]) -> AppletResult {
    let mut mode_kind: Option<(&str, Vec<Range>)> = None;
    let mut delim: u8 = b'\t';
    let mut suppress = false;
    let mut paths: Vec<&str> = Vec::new();
    let mut i = 0;

    while i < args.len() {
        let arg = &args[i];
        match arg.as_str() {
            "--" => {
                i += 1;
                while i < args.len() {
                    paths.push(&args[i]);
                    i += 1;
                }
                break;
            }
            "-b" | "--bytes" => {
                i += 1;
                if i >= args.len() {
                    return Err(vec![AppletError::option_requires_arg(APPLET, "b")]);
                }
                mode_kind = Some(("b", parse_ranges(&args[i])?));
            }
            "-c" | "--characters" => {
                i += 1;
                if i >= args.len() {
                    return Err(vec![AppletError::option_requires_arg(APPLET, "c")]);
                }
                mode_kind = Some(("c", parse_ranges(&args[i])?));
            }
            "-f" | "--fields" => {
                i += 1;
                if i >= args.len() {
                    return Err(vec![AppletError::option_requires_arg(APPLET, "f")]);
                }
                mode_kind = Some(("f", parse_ranges(&args[i])?));
            }
            "-d" | "--delimiter" => {
                i += 1;
                if i >= args.len() {
                    return Err(vec![AppletError::option_requires_arg(APPLET, "d")]);
                }
                let s = &args[i];
                if s.len() != 1 {
                    return Err(vec![AppletError::new(
                        APPLET,
                        "the delimiter must be a single character",
                    )]);
                }
                delim = s.as_bytes()[0];
            }
            "-s" | "--only-delimited" => suppress = true,
            a if a.starts_with('-') && a.len() > 1 => {
                return Err(vec![AppletError::invalid_option(
                    APPLET,
                    a.chars().nth(1).unwrap_or('-'),
                )]);
            }
            _ => paths.push(arg),
        }
        i += 1;
    }

    let mode = match mode_kind {
        Some(("b", ranges)) => Mode::Bytes(ranges),
        Some(("c", ranges)) => Mode::Chars(ranges),
        Some(("f", ranges)) => Mode::Fields {
            ranges,
            delim,
            suppress,
        },
        Some(_) => unreachable!(),
        None => {
            return Err(vec![AppletError::new(
                APPLET,
                "you must specify a list of bytes, characters, or fields",
            )]);
        }
    };

    let mut out = stdout();
    let mut errors = Vec::new();

    if paths.is_empty() {
        let stdin = io::stdin();
        if let Err(e) = cut_reader(&mut BufReader::new(stdin.lock()), &mut out, &mode) {
            errors.push(AppletError::from_io(APPLET, "reading stdin", None, e));
        }
    } else {
        for path in &paths {
            match open_input(path) {
                Err(e) => errors.push(AppletError::from_io(APPLET, "opening", Some(path), e)),
                Ok(input) => {
                    if let Err(e) = cut_reader(&mut BufReader::new(input), &mut out, &mode) {
                        errors.push(AppletError::from_io(APPLET, "reading", Some(path), e));
                    }
                }
            }
        }
    }

    if errors.is_empty() {
        Ok(())
    } else {
        Err(errors)
    }
}

fn cut_reader<R: BufRead, W: Write>(reader: &mut R, out: &mut W, mode: &Mode) -> io::Result<()> {
    let mut line = Vec::new();
    loop {
        line.clear();
        if reader.read_until(b'\n', &mut line)? == 0 {
            break;
        }
        let has_nl = line.last() == Some(&b'\n');
        let content = if has_nl {
            &line[..line.len() - 1]
        } else {
            &line[..]
        };
        if cut_line(content, out, mode)? {
            out.write_all(b"\n")?;
        }
    }
    Ok(())
}

fn cut_line<W: Write>(line: &[u8], out: &mut W, mode: &Mode) -> io::Result<bool> {
    match mode {
        Mode::Bytes(ranges) => {
            let positions = selected_positions(ranges, line.len());
            for &p in &positions {
                out.write_all(&line[p..p + 1])?;
            }
            Ok(true)
        }
        Mode::Chars(ranges) => {
            // Collect UTF-8 character byte-spans
            let s = std::str::from_utf8(line).unwrap_or("");
            let spans: Vec<(usize, usize)> = {
                let mut v = Vec::new();
                let mut i = 0;
                for ch in s.chars() {
                    let len = ch.len_utf8();
                    v.push((i, i + len));
                    i += len;
                }
                v
            };
            let positions = selected_positions(ranges, spans.len());
            for &p in &positions {
                out.write_all(&line[spans[p].0..spans[p].1])?;
            }
            Ok(true)
        }
        Mode::Fields {
            ranges,
            delim,
            suppress,
        } => {
            let fields: Vec<&[u8]> = line.split(|&b| b == *delim).collect();
            if fields.len() == 1 && *suppress {
                // No delimiter found; suppress this line entirely
                return Ok(false);
            }
            let positions = selected_positions(ranges, fields.len());
            for (i, &p) in positions.iter().enumerate() {
                if i > 0 {
                    out.write_all(&[*delim])?;
                }
                out.write_all(fields[p])?;
            }
            Ok(true)
        }
    }
}

// Returns sorted, deduplicated 0-based indices of selected elements.
fn selected_positions(ranges: &[Range], len: usize) -> Vec<usize> {
    let mut positions = Vec::new();
    for range in ranges {
        let start = range.start.map(|s| s - 1).unwrap_or(0);
        let end = range.end.map(|e| e - 1).unwrap_or(len.saturating_sub(1));
        for p in start..=end {
            if p < len {
                positions.push(p);
            }
        }
    }
    positions.sort_unstable();
    positions.dedup();
    positions
}

fn parse_ranges(s: &str) -> Result<Vec<Range>, Vec<AppletError>> {
    s.split(',').map(parse_range).collect()
}

fn parse_range(s: &str) -> Result<Range, Vec<AppletError>> {
    if let Some((start, end)) = s.split_once('-') {
        let start = if start.is_empty() {
            None
        } else {
            Some(parse_pos(start)?)
        };
        let end = if end.is_empty() {
            None
        } else {
            Some(parse_pos(end)?)
        };
        Ok(Range { start, end })
    } else {
        let n = parse_pos(s)?;
        Ok(Range {
            start: Some(n),
            end: Some(n),
        })
    }
}

fn parse_pos(s: &str) -> Result<usize, Vec<AppletError>> {
    let n: usize = s.parse().map_err(|_| {
        vec![AppletError::new(
            APPLET,
            format!("invalid field value: '{s}'"),
        )]
    })?;
    if n == 0 {
        return Err(vec![AppletError::new(
            APPLET,
            "fields and positions are numbered from 1",
        )]);
    }
    Ok(n)
}

#[cfg(test)]
mod tests {
    use std::io::BufReader;

    use super::{Mode, cut_line, cut_reader, parse_ranges};

    fn cut(line: &str, mode: &Mode) -> String {
        let mut buf = Vec::new();
        cut_line(line.as_bytes(), &mut buf, mode).unwrap();
        String::from_utf8(buf).unwrap()
    }

    #[test]
    fn bytes() {
        let ranges = parse_ranges("1,3,5").unwrap();
        assert_eq!(cut("abcde", &Mode::Bytes(ranges)), "ace");
    }

    #[test]
    fn range() {
        let ranges = parse_ranges("2-4").unwrap();
        assert_eq!(cut("abcde", &Mode::Bytes(ranges)), "bcd");
    }

    #[test]
    fn fields() {
        let ranges = parse_ranges("1,3").unwrap();
        let mode = Mode::Fields {
            ranges,
            delim: b':',
            suppress: false,
        };
        assert_eq!(cut("a:b:c:d", &mode), "a:c");
    }

    #[test]
    fn suppress_no_delim() {
        let ranges = parse_ranges("1").unwrap();
        let mode = Mode::Fields {
            ranges,
            delim: b':',
            suppress: true,
        };
        assert_eq!(cut("no-colon-here", &mode), "");
    }

    #[test]
    fn suppress_no_delim_does_not_emit_blank_line() {
        let ranges = parse_ranges("2").unwrap();
        let mode = Mode::Fields {
            ranges,
            delim: b':',
            suppress: true,
        };
        let mut out = Vec::new();
        cut_reader(
            &mut BufReader::new("a:b\nplain\nc:d\n".as_bytes()),
            &mut out,
            &mode,
        )
        .unwrap();
        assert_eq!(String::from_utf8(out).unwrap(), "b\nd\n");
    }
}
