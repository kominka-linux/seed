use std::io::{self, BufRead, BufReader, Write};

use crate::common::applet::{AppletResult, finish};
use crate::common::error::AppletError;
use crate::common::io::{open_input, stdout};

const APPLET: &str = "uniq";

pub fn main(args: &[String]) -> i32 {
    finish(run(args))
}

#[derive(Debug, Default, Clone, Copy)]
struct Options {
    count: bool,
    duplicates_only: bool,
    all_duplicates: bool,
    unique_only: bool,
    ignore_case: bool,
    skip_fields: usize,
    skip_chars: usize,
    check_chars: Option<usize>,
}

fn run(args: &[String]) -> AppletResult {
    let mut opts = Options::default();
    let mut positionals: Vec<&str> = Vec::new();
    let mut i = 0;

    while i < args.len() {
        let arg = &args[i];
        match arg.as_str() {
            "--" => {
                i += 1;
                while i < args.len() {
                    positionals.push(&args[i]);
                    i += 1;
                }
                break;
            }
            "-c" | "--count" => opts.count = true,
            "-d" | "--repeated" => opts.duplicates_only = true,
            "-D" | "--all-repeated" => opts.all_duplicates = true,
            "-u" | "--unique" => opts.unique_only = true,
            "-i" | "--ignore-case" => opts.ignore_case = true,
            "-f" | "--skip-fields" => {
                i += 1;
                if i >= args.len() {
                    return Err(vec![AppletError::option_requires_arg(APPLET, "f")]);
                }
                opts.skip_fields = parse_usize(APPLET, &args[i])?;
            }
            "-s" | "--skip-chars" => {
                i += 1;
                if i >= args.len() {
                    return Err(vec![AppletError::option_requires_arg(APPLET, "s")]);
                }
                opts.skip_chars = parse_usize(APPLET, &args[i])?;
            }
            "-w" | "--check-chars" => {
                i += 1;
                if i >= args.len() {
                    return Err(vec![AppletError::option_requires_arg(APPLET, "w")]);
                }
                opts.check_chars = Some(parse_usize(APPLET, &args[i])?);
            }
            a if a.starts_with('-') && a.len() > 1 => {
                return Err(vec![AppletError::invalid_option(
                    APPLET,
                    a.chars().nth(1).unwrap_or('-'),
                )]);
            }
            _ => positionals.push(arg),
        }
        i += 1;
    }

    let input_path = positionals.first().copied().unwrap_or("-");
    let output_path = positionals.get(1).copied();

    let input = open_input(input_path)
        .map_err(|e| vec![AppletError::from_io(APPLET, "opening", Some(input_path), e)])?;

    let mut out_box: Box<dyn Write> = if let Some(path) = output_path {
        Box::new(std::fs::File::create(path).map_err(|e| {
            vec![AppletError::from_io(
                APPLET,
                "opening output",
                Some(path),
                e,
            )]
        })?)
    } else {
        Box::new(stdout())
    };

    process(BufReader::new(input), &mut out_box, opts, input_path)
}

fn process<R: BufRead, W: Write>(
    mut reader: R,
    out: &mut W,
    opts: Options,
    path: &str,
) -> AppletResult {
    // Each "group" collects all lines that compare equal.
    let mut group: Vec<String> = Vec::new();
    let mut group_key = String::new();
    let mut line = String::new();

    let flush = |group: &[String], out: &mut dyn Write| -> io::Result<()> {
        if group.is_empty() {
            return Ok(());
        }
        let count = group.len();
        let should_print = if opts.duplicates_only {
            count > 1
        } else if opts.unique_only {
            count == 1
        } else {
            true
        };
        if !should_print {
            return Ok(());
        }
        let print_count = opts.all_duplicates && count > 1;
        let lines_to_print = if print_count { count } else { 1 };
        for l in group.iter().take(lines_to_print) {
            if opts.count {
                write!(out, "{count:>7} ")?;
            }
            out.write_all(l.as_bytes())?;
        }
        Ok(())
    };

    loop {
        line.clear();
        let n = reader
            .read_line(&mut line)
            .map_err(|e| vec![AppletError::from_io(APPLET, "reading", Some(path), e)])?;

        if n == 0 {
            flush(&group, out)
                .map_err(|e| vec![AppletError::from_io(APPLET, "writing", None, e)])?;
            break;
        }

        let key = make_key(&line, &opts);

        if group.is_empty() {
            group_key = key;
            group.push(line.clone());
        } else if key == group_key {
            group.push(line.clone());
        } else {
            flush(&group, out)
                .map_err(|e| vec![AppletError::from_io(APPLET, "writing", None, e)])?;
            group.clear();
            group_key = key;
            group.push(line.clone());
        }
    }

    Ok(())
}

fn make_key(line: &str, opts: &Options) -> String {
    // Strip line ending for comparison
    let s = line.trim_end_matches('\n').trim_end_matches('\r');

    // Skip fields (whitespace-delimited)
    let after_fields = if opts.skip_fields > 0 {
        skip_fields(s, opts.skip_fields)
    } else {
        s
    };

    // Skip characters
    let after_skip = if opts.skip_chars > 0 {
        let n = opts.skip_chars.min(after_fields.len());
        &after_fields[n..]
    } else {
        after_fields
    };

    // Limit comparison width
    let checked = if let Some(w) = opts.check_chars {
        let end = after_skip
            .char_indices()
            .nth(w)
            .map(|(i, _)| i)
            .unwrap_or(after_skip.len());
        &after_skip[..end]
    } else {
        after_skip
    };

    if opts.ignore_case {
        checked.to_lowercase()
    } else {
        checked.to_owned()
    }
}

fn skip_fields(s: &str, n: usize) -> &str {
    let mut remaining = s;
    for _ in 0..n {
        // Skip leading whitespace
        let trimmed = remaining.trim_start_matches([' ', '\t']);
        // Skip the field (non-whitespace)
        let after = trimmed.trim_start_matches(|c| c != ' ' && c != '\t');
        remaining = after;
        if remaining.is_empty() {
            break;
        }
    }
    remaining.trim_start_matches([' ', '\t'])
}

fn parse_usize(applet: &'static str, s: &str) -> Result<usize, Vec<AppletError>> {
    s.parse::<usize>()
        .map_err(|_| vec![AppletError::new(applet, format!("invalid count: '{s}'"))])
}

#[cfg(test)]
mod tests {
    use std::io::{BufReader, Cursor};

    use super::{Options, make_key, process, skip_fields};

    #[test]
    fn basic_key() {
        let opts = Options::default();
        assert_eq!(make_key("hello\n", &opts), "hello");
    }

    #[test]
    fn ignore_case_key() {
        let opts = Options {
            ignore_case: true,
            ..Default::default()
        };
        assert_eq!(make_key("Hello\n", &opts), "hello");
    }

    #[test]
    fn skip_fields_works() {
        assert_eq!(skip_fields("a b c", 1), "b c");
        assert_eq!(skip_fields("a b c", 2), "c");
        assert_eq!(skip_fields("a  b  c", 1), "b  c");
    }

    fn process_to_string(input: &str, opts: Options) -> String {
        let mut out = Vec::new();
        process(BufReader::new(Cursor::new(input)), &mut out, opts, "<test>").unwrap();
        String::from_utf8(out).unwrap()
    }

    #[test]
    fn count_mode_reports_group_size() {
        let output = process_to_string(
            "alpha\nalpha\nbeta\n",
            Options {
                count: true,
                ..Default::default()
            },
        );
        assert_eq!(output, "      2 alpha\n      1 beta\n");
    }

    #[test]
    fn duplicate_modes_filter_groups() {
        let input = "alpha\nalpha\nbeta\ngamma\ngamma\n";
        let duplicates = process_to_string(
            input,
            Options {
                duplicates_only: true,
                ..Default::default()
            },
        );
        assert_eq!(duplicates, "alpha\ngamma\n");

        let all_duplicates = process_to_string(
            input,
            Options {
                all_duplicates: true,
                ..Default::default()
            },
        );
        assert_eq!(all_duplicates, "alpha\nalpha\nbeta\ngamma\ngamma\n");

        let unique_only = process_to_string(
            input,
            Options {
                unique_only: true,
                ..Default::default()
            },
        );
        assert_eq!(unique_only, "beta\n");
    }

    #[test]
    fn comparison_options_change_grouping() {
        let ignore_case = process_to_string(
            "Alpha\nalpha\nBeta\n",
            Options {
                ignore_case: true,
                ..Default::default()
            },
        );
        assert_eq!(ignore_case, "Alpha\nBeta\n");

        let skip_and_width = process_to_string(
            "1 key-one\n2 key-two\n3 other\n",
            Options {
                skip_fields: 1,
                check_chars: Some(3),
                ..Default::default()
            },
        );
        assert_eq!(skip_and_width, "1 key-one\n3 other\n");
    }
}
