use std::io::{self, BufRead};
use std::process::{Command, ExitStatus};

use crate::common::applet::{AppletResult, finish};
use crate::common::error::AppletError;

const APPLET: &str = "xargs";

pub fn main(args: &[String]) -> i32 {
    finish(run(args))
}

fn run(args: &[String]) -> AppletResult {
    let mut null_delim = false; // -0: NUL-delimited input
    let mut max_args: usize = 0; // -n N: max args per invocation (0 = unlimited)
    let mut max_lines: usize = 0; // -L N: max lines per invocation
    let mut replace: Option<String> = None; // -I STR: replace occurrences of STR
    let mut no_run_if_empty = false; // -r: don't run if no args
    let mut verbose = false; // -t: print command to stderr
    let mut interactive = false; // -p: prompt
    let mut max_procs: usize = 1; // -P N: max parallel processes (basic)
    let mut delimiter: Option<u8> = None; // -d DELIM
    let mut cmd_args: Vec<&str> = Vec::new();
    let mut i = 0;

    while i < args.len() {
        let arg = &args[i];
        match arg.as_str() {
            "--" => {
                i += 1;
                while i < args.len() {
                    cmd_args.push(&args[i]);
                    i += 1;
                }
                break;
            }
            "-0" | "--null" => null_delim = true,
            "-r" | "--no-run-if-empty" => no_run_if_empty = true,
            "-t" | "--verbose" => verbose = true,
            "-p" | "--interactive" => interactive = true,
            "-n" | "--max-args" => {
                i += 1;
                if i >= args.len() {
                    return Err(vec![AppletError::option_requires_arg(APPLET, "n")]);
                }
                max_args = parse_usize(APPLET, "-n", &args[i])?;
            }
            "-L" | "--max-lines" => {
                i += 1;
                if i >= args.len() {
                    return Err(vec![AppletError::option_requires_arg(APPLET, "L")]);
                }
                max_lines = parse_usize(APPLET, "-L", &args[i])?;
            }
            "-P" | "--max-procs" => {
                i += 1;
                if i >= args.len() {
                    return Err(vec![AppletError::option_requires_arg(APPLET, "P")]);
                }
                max_procs = parse_usize(APPLET, "-P", &args[i])?;
            }
            "-I" => {
                i += 1;
                if i >= args.len() {
                    return Err(vec![AppletError::option_requires_arg(APPLET, "I")]);
                }
                replace = Some(args[i].clone());
            }
            "-d" | "--delimiter" => {
                i += 1;
                if i >= args.len() {
                    return Err(vec![AppletError::option_requires_arg(APPLET, "d")]);
                }
                delimiter = Some(parse_delimiter(APPLET, &args[i])?);
            }
            a if a.starts_with("-I") => {
                replace = Some(a[2..].to_string());
            }
            a if a.starts_with("-n") && a.len() > 2 => {
                max_args = parse_usize(APPLET, "-n", &a[2..])?;
            }
            a if a.starts_with("-L") && a.len() > 2 => {
                max_lines = parse_usize(APPLET, "-L", &a[2..])?;
            }
            a if a.starts_with("-P") && a.len() > 2 => {
                max_procs = parse_usize(APPLET, "-P", &a[2..])?;
            }
            a if a.starts_with('-') && a.len() > 1 => {
                return Err(vec![AppletError::invalid_option(
                    APPLET,
                    a.chars().nth(1).unwrap_or('-'),
                )]);
            }
            _ => {
                // First non-flag is the start of the command
                cmd_args.push(arg);
                i += 1;
                while i < args.len() {
                    cmd_args.push(&args[i]);
                    i += 1;
                }
                break;
            }
        }
        i += 1;
    }

    let _ = (interactive, max_procs); // TODO: interactive prompting, parallel

    // Determine the command to run (default: echo)
    let (cmd, initial_args): (&str, &[&str]) = if cmd_args.is_empty() {
        ("echo", &[])
    } else {
        (cmd_args[0], &cmd_args[1..])
    };

    // Read tokens from stdin
    let stdin = io::stdin();
    let tokens = if null_delim {
        read_by_delimiter(stdin.lock(), b'\0')
    } else if let Some(d) = delimiter {
        read_by_delimiter(stdin.lock(), d)
    } else {
        read_whitespace_delimited(stdin.lock())
    };

    if tokens.is_empty() {
        if no_run_if_empty {
            return Ok(());
        }
        // Run with no extra args (just the command + initial_args)
        let status = invoke(cmd, initial_args, &[], verbose)?;
        return exit_status(status);
    }

    if let Some(placeholder) = &replace {
        // -I mode: each token replaces placeholder in cmd_args; one invocation per token
        let mut last_status: Option<ExitStatus> = None;
        for token in &tokens {
            let replaced: Vec<String> = initial_args
                .iter()
                .map(|a| a.replace(placeholder.as_str(), token))
                .collect();
            let replaced_refs: Vec<&str> = replaced.iter().map(String::as_str).collect();
            let status = invoke(cmd, &replaced_refs, &[], verbose)?;
            last_status = Some(status);
            if !status.success() && status.code() == Some(255) {
                break;
            }
        }
        if let Some(s) = last_status {
            return exit_status(s);
        }
        return Ok(());
    }

    // Normal mode: batch tokens, invoke when batch is full
    let batch_size = if max_args > 0 {
        max_args
    } else if max_lines > 0 {
        max_lines // approximate: treat each token as one "line" for simplicity
    } else {
        usize::MAX
    };

    let mut last_status: Option<ExitStatus> = None;
    for chunk in tokens.chunks(batch_size) {
        let chunk_refs: Vec<&str> = chunk.iter().map(String::as_str).collect();
        let status = invoke(cmd, initial_args, &chunk_refs, verbose)?;
        last_status = Some(status);
        if status.code() == Some(255) {
            break;
        }
    }
    if let Some(s) = last_status {
        return exit_status(s);
    }
    Ok(())
}

fn invoke(
    cmd: &str,
    prefix_args: &[&str],
    extra_args: &[&str],
    verbose: bool,
) -> Result<ExitStatus, Vec<AppletError>> {
    let mut all_args: Vec<&str> = Vec::with_capacity(prefix_args.len() + extra_args.len());
    all_args.extend_from_slice(prefix_args);
    all_args.extend_from_slice(extra_args);

    if verbose {
        let parts: Vec<&str> = std::iter::once(cmd)
            .chain(all_args.iter().copied())
            .collect();
        eprintln!("{}", parts.join(" "));
    }

    Command::new(cmd)
        .args(&all_args)
        .status()
        .map_err(|e| vec![AppletError::new(APPLET, format!("{cmd}: {e}"))])
}

fn exit_status(status: ExitStatus) -> AppletResult {
    if status.success() {
        Ok(())
    } else {
        // Return a non-zero exit but no printed error
        Err(vec![])
    }
}

// Read whitespace-separated tokens (the default), handling shell-style quotes.
fn read_whitespace_delimited<R: BufRead>(reader: R) -> Vec<String> {
    let mut tokens = Vec::new();
    let mut current = String::new();
    let mut in_single = false;
    let mut in_double = false;

    for line in reader.lines().map_while(Result::ok) {
        for ch in line.chars() {
            if in_single {
                if ch == '\'' {
                    in_single = false;
                } else {
                    current.push(ch);
                }
            } else if in_double {
                if ch == '"' {
                    in_double = false;
                } else {
                    current.push(ch);
                }
            } else if ch == '\'' {
                in_single = true;
            } else if ch == '"' {
                in_double = true;
            } else if ch.is_whitespace() {
                if !current.is_empty() {
                    tokens.push(std::mem::take(&mut current));
                }
            } else {
                current.push(ch);
            }
        }
        // Newline terminates a token only if we're not in a quoted string
        if !in_single && !in_double && !current.is_empty() {
            tokens.push(std::mem::take(&mut current));
        }
    }
    if !current.is_empty() {
        tokens.push(current);
    }
    tokens
}

fn read_by_delimiter<R: io::Read>(mut reader: R, delim: u8) -> Vec<String> {
    let mut data = Vec::new();
    let _ = reader.read_to_end(&mut data);
    data.split(|&b| b == delim)
        .filter_map(|s| {
            if s.is_empty() {
                None
            } else {
                String::from_utf8(s.to_vec()).ok()
            }
        })
        .collect()
}

fn parse_delimiter(applet: &'static str, s: &str) -> Result<u8, Vec<AppletError>> {
    if s.len() == 1 {
        return Ok(s.as_bytes()[0]);
    }
    if s.starts_with("\\") && s.len() == 2 {
        return Ok(match s.as_bytes()[1] {
            b'n' => b'\n',
            b't' => b'\t',
            b'0' => b'\0',
            b'\\' => b'\\',
            other => other,
        });
    }
    // Hex: \xNN
    if let Some(hex) = s.strip_prefix("\\x")
        && let Ok(n) = u8::from_str_radix(hex, 16)
    {
        return Ok(n);
    }
    Err(vec![AppletError::new(
        applet,
        format!("invalid delimiter: '{s}'"),
    )])
}

fn parse_usize(applet: &'static str, flag: &str, s: &str) -> Result<usize, Vec<AppletError>> {
    s.parse::<usize>().map_err(|_| {
        vec![AppletError::new(
            applet,
            format!("{flag}: invalid number: '{s}'"),
        )]
    })
}

#[cfg(test)]
mod tests {
    use super::{read_whitespace_delimited, run};
    use std::io::Cursor;

    fn args(v: &[&str]) -> Vec<String> {
        v.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn tokenize_simple() {
        let tokens = read_whitespace_delimited(Cursor::new("a b c\n"));
        assert_eq!(tokens, ["a", "b", "c"]);
    }

    #[test]
    fn tokenize_multiline() {
        let tokens = read_whitespace_delimited(Cursor::new("a\nb\nc\n"));
        assert_eq!(tokens, ["a", "b", "c"]);
    }

    #[test]
    fn tokenize_single_quotes() {
        let tokens = read_whitespace_delimited(Cursor::new("'hello world' foo\n"));
        assert_eq!(tokens, ["hello world", "foo"]);
    }

    #[test]
    fn tokenize_double_quotes() {
        let tokens = read_whitespace_delimited(Cursor::new("\"hello world\" foo\n"));
        assert_eq!(tokens, ["hello world", "foo"]);
    }

    #[test]
    fn tokenize_empty_input() {
        let tokens = read_whitespace_delimited(Cursor::new(""));
        assert!(tokens.is_empty());
    }

    #[test]
    fn invalid_option_fails() {
        assert!(run(&args(&["-z"])).is_err());
    }

    #[test]
    fn max_args_option_parsed() {
        // Just verify parsing doesn't crash; actual execution uses stdin.
        // We can't easily test execution without stdin injection, so test
        // that -n with invalid arg fails.
        assert!(run(&args(&["-n", "bad"])).is_err());
    }
}
