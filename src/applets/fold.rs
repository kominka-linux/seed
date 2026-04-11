use std::io::{self, BufRead, BufReader, Write};

use crate::common::applet::{AppletResult, finish};
use crate::common::error::AppletError;
use crate::common::io::{open_input, stdout};

const APPLET: &str = "fold";
const DEFAULT_WIDTH: usize = 80;

pub fn main(args: &[String]) -> i32 {
    finish(run(args))
}

fn run(args: &[String]) -> AppletResult {
    let mut byte_mode = false;
    let mut break_spaces = false;
    let mut width = DEFAULT_WIDTH;
    let mut paths: Vec<&str> = Vec::new();
    let mut i = 0;

    while i < args.len() {
        let arg = &args[i];
        match arg.as_str() {
            "--" => {
                i += 1;
                while i < args.len() { paths.push(&args[i]); i += 1; }
                break;
            }
            "-b" | "--bytes" => byte_mode = true,
            "-s" | "--spaces" => break_spaces = true,
            "-w" | "--width" => {
                i += 1;
                if i >= args.len() {
                    return Err(vec![AppletError::option_requires_arg(APPLET, "w")]);
                }
                width = parse_width(APPLET, &args[i])?;
            }
            a if a.starts_with("-w") && a.len() > 2 => {
                width = parse_width(APPLET, &a[2..])?;
            }
            // -N shorthand
            a if a.starts_with('-') && a[1..].chars().all(|c| c.is_ascii_digit()) && a.len() > 1 => {
                width = parse_width(APPLET, &a[1..])?;
            }
            a if a.starts_with('-') && a.len() > 1 => {
                return Err(vec![AppletError::invalid_option(APPLET, a.chars().nth(1).unwrap_or('-'))]);
            }
            _ => paths.push(arg),
        }
        i += 1;
    }

    let mut out = stdout();
    let mut errors = Vec::new();

    if paths.is_empty() {
        let stdin = io::stdin();
        if let Err(e) = fold_reader(&mut BufReader::new(stdin.lock()), &mut out, width, byte_mode, break_spaces) {
            errors.push(AppletError::from_io(APPLET, "reading stdin", None, e));
        }
    } else {
        for path in &paths {
            match open_input(path) {
                Err(e) => errors.push(AppletError::from_io(APPLET, "opening", Some(path), e)),
                Ok(input) => {
                    if let Err(e) = fold_reader(&mut BufReader::new(input), &mut out, width, byte_mode, break_spaces) {
                        errors.push(AppletError::from_io(APPLET, "reading", Some(path), e));
                    }
                }
            }
        }
    }

    if errors.is_empty() { Ok(()) } else { Err(errors) }
}

fn parse_width(applet: &'static str, s: &str) -> Result<usize, Vec<AppletError>> {
    let w: usize = s.parse().map_err(|_| {
        vec![AppletError::new(applet, format!("invalid width: '{s}'"))]
    })?;
    if w == 0 {
        return Err(vec![AppletError::new(applet, "width must be greater than 0")]);
    }
    Ok(w)
}

fn fold_reader<R: BufRead, W: Write>(
    reader: &mut R,
    out: &mut W,
    width: usize,
    byte_mode: bool,
    break_spaces: bool,
) -> io::Result<()> {
    let mut line = Vec::new();
    loop {
        line.clear();
        if reader.read_until(b'\n', &mut line)? == 0 {
            break;
        }
        let has_nl = line.last() == Some(&b'\n');
        let content = if has_nl { &line[..line.len() - 1] } else { &line[..] };
        if byte_mode {
            fold_bytes(content, out, width, break_spaces)?;
        } else {
            fold_columns(content, out, width, break_spaces)?;
        }
        if has_nl {
            out.write_all(b"\n")?;
        }
    }
    Ok(())
}

fn fold_bytes<W: Write>(data: &[u8], out: &mut W, width: usize, break_spaces: bool) -> io::Result<()> {
    let mut pos = 0;
    while pos < data.len() {
        let end = (pos + width).min(data.len());
        let chunk = &data[pos..end];

        if break_spaces && end < data.len() {
            // Find last space/tab in chunk to break at
            if let Some(sp) = chunk.iter().rposition(|&b| b == b' ' || b == b'\t') {
                let break_at = pos + sp + 1;
                out.write_all(&data[pos..break_at])?;
                out.write_all(b"\n")?;
                pos = break_at;
                continue;
            }
        }

        out.write_all(chunk)?;
        if end < data.len() {
            out.write_all(b"\n")?;
        }
        pos = end;
    }
    Ok(())
}

fn fold_columns<W: Write>(data: &[u8], out: &mut W, width: usize, break_spaces: bool) -> io::Result<()> {
    // Count columns: tabs advance to next multiple-of-8 tab stop; all other
    // printable characters occupy 1 column. We work on bytes (assuming UTF-8
    // or ASCII), treating each non-tab byte as 1 column.
    let mut col: usize = 0;
    let mut chunk_start: usize = 0;
    // For -s: track the byte position just past the last space/tab we saw
    let mut last_space_end: Option<usize> = None;
    let mut i = 0;

    while i < data.len() {
        let b = data[i];
        let ch_cols = if b == b'\t' {
            // Tab to next multiple of 8
            8 - (col % 8)
        } else {
            1
        };

        if b == b' ' || b == b'\t' {
            last_space_end = Some(i + 1);
        }

        if col + ch_cols > width {
            // Wrap here
            let wrap_at = if break_spaces {
                last_space_end.unwrap_or(i)
            } else {
                i
            };

            out.write_all(&data[chunk_start..wrap_at])?;
            out.write_all(b"\n")?;
            chunk_start = wrap_at;
            last_space_end = None;
            // Recount columns from the new chunk_start up to (not including) the
            // current byte position, since we haven't advanced i yet.
            col = count_columns(&data[chunk_start..i]);
            // Now add this character
            col += ch_cols;
            i += 1;
            continue;
        }

        col += ch_cols;
        i += 1;
    }

    out.write_all(&data[chunk_start..])?;
    Ok(())
}

fn count_columns(data: &[u8]) -> usize {
    let mut col = 0;
    for &b in data {
        col += if b == b'\t' { 8 - (col % 8) } else { 1 };
    }
    col
}

#[cfg(test)]
mod tests {
    use super::{count_columns, fold_bytes, fold_columns};

    fn fold_b(data: &str, width: usize, break_spaces: bool) -> String {
        let mut out = Vec::new();
        fold_bytes(data.as_bytes(), &mut out, width, break_spaces).unwrap();
        String::from_utf8(out).unwrap()
    }

    fn fold_c(data: &str, width: usize, break_spaces: bool) -> String {
        let mut out = Vec::new();
        fold_columns(data.as_bytes(), &mut out, width, break_spaces).unwrap();
        String::from_utf8(out).unwrap()
    }

    #[test]
    fn tab_column_count() {
        assert_eq!(count_columns(b""), 0);
        assert_eq!(count_columns(b"\t"), 8);      // tab at col 0 → col 8
        assert_eq!(count_columns(b"a\t"), 8);     // 'a' at col 0, tab → col 8
        assert_eq!(count_columns(b"abcdefg\t"), 8); // 7 chars then tab → col 8
        assert_eq!(count_columns(b"abcdefgh\t"), 16); // 8 chars then tab → col 16
    }

    #[test]
    fn bytes_wraps_exactly_at_width() {
        assert_eq!(fold_b("abcdefgh", 4, false), "abcd\nefgh");
    }

    #[test]
    fn bytes_no_wrap_when_shorter() {
        assert_eq!(fold_b("abc", 80, false), "abc");
    }

    #[test]
    fn bytes_break_at_space() {
        assert_eq!(fold_b("hello world!", 8, true), "hello \nworld!");
    }

    #[test]
    fn bytes_break_at_space_no_candidate_falls_back_to_width() {
        assert_eq!(fold_b("abcdefghij", 5, true), "abcde\nfghij");
    }

    #[test]
    fn columns_wraps_at_width() {
        assert_eq!(fold_c("abcdefgh", 4, false), "abcd\nefgh");
    }

    #[test]
    fn columns_tab_counts_toward_width() {
        // "abc\t" = 8 columns; with width 8 it fits exactly, no wrap
        assert_eq!(fold_c("abc\t", 8, false), "abc\t");
        // "abcd\t" would be 9 columns (tab at col 4 → col 8, but that's 9... wait)
        // 'a','b','c','d' = 4 cols, then tab at col 4 → col 8. That's 8 cols total. Fits.
        assert_eq!(fold_c("abcd\t", 8, false), "abcd\t");
        // 9 chars must wrap at 8
        assert_eq!(fold_c("abcdefghi", 8, false), "abcdefgh\ni");
    }
}
