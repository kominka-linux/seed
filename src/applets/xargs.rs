use std::fs::File;
use std::io::{self, BufRead, BufReader, Read, Write};
use std::process::{Child, Command, ExitStatus};

use crate::common::args::{ArgCursor, ArgToken};
use crate::common::applet::{AppletResult, finish};
use crate::common::error::AppletError;

const APPLET: &str = "xargs";

#[derive(Clone, Debug, Eq, PartialEq)]
struct Options {
    null_delim: bool,
    max_args: usize,
    max_lines: usize,
    replace: Option<String>,
    no_run_if_empty: bool,
    verbose: bool,
    interactive: bool,
    max_procs: usize,
    delimiter: Option<u8>,
}

impl Default for Options {
    fn default() -> Self {
        Self {
            null_delim: false,
            max_args: 0,
            max_lines: 0,
            replace: None,
            no_run_if_empty: false,
            verbose: false,
            interactive: false,
            max_procs: 1,
            delimiter: None,
        }
    }
}

pub fn main(args: &[std::ffi::OsString]) -> i32 {
    finish(run(args))
}

fn run(args: &[std::ffi::OsString]) -> AppletResult {
    let (options, cmd_args) = parse_args(args)?;

    let (cmd, initial_args): (&str, &[String]) = if cmd_args.is_empty() {
        ("echo", &[])
    } else {
        (&cmd_args[0], &cmd_args[1..])
    };

    let stdin = io::stdin();
    let tokens = if options.null_delim {
        read_by_delimiter(stdin.lock(), b'\0')
    } else if let Some(d) = options.delimiter {
        read_by_delimiter(stdin.lock(), d)
    } else {
        read_whitespace_delimited(stdin.lock())
    };

    let invocations = build_invocations(&options, initial_args, tokens);
    if invocations.is_empty() {
        if options.no_run_if_empty {
            return Ok(());
        }
        return execute_invocations(cmd, vec![Vec::new()], &options);
    }

    execute_invocations(cmd, invocations, &options)
}

fn parse_args(args: &[std::ffi::OsString]) -> Result<(Options, Vec<String>), Vec<AppletError>> {
    let mut cursor = ArgCursor::new(args);
    let mut options = Options::default();
    let mut cmd_args = Vec::new();

    while let Some(arg) = cursor.next_arg(APPLET)? {
        match arg {
            ArgToken::Operand(value) => {
                cmd_args.push(value.to_string());
                for remaining in cursor.remaining() {
                    cmd_args
                        .push(crate::common::args::os_to_string(APPLET, remaining.as_os_str())?);
                }
                break;
            }
            ArgToken::LongOption("null", None) => options.null_delim = true,
            ArgToken::LongOption("no-run-if-empty", None) => options.no_run_if_empty = true,
            ArgToken::LongOption("verbose", None) => options.verbose = true,
            ArgToken::LongOption("interactive", None) => options.interactive = true,
            ArgToken::LongOption("max-args", attached) => {
                let value = cursor.next_value_or_maybe_attached(attached, APPLET, "n")?;
                options.max_args = parse_usize(APPLET, "-n", value)?;
            }
            ArgToken::LongOption("max-lines", attached) => {
                let value = cursor.next_value_or_maybe_attached(attached, APPLET, "L")?;
                options.max_lines = parse_usize(APPLET, "-L", value)?;
            }
            ArgToken::LongOption("max-procs", attached) => {
                let value = cursor.next_value_or_maybe_attached(attached, APPLET, "P")?;
                options.max_procs = parse_usize(APPLET, "-P", value)?;
            }
            ArgToken::LongOption("replace", attached) => {
                let value = cursor.next_value_or_maybe_attached(attached, APPLET, "I")?;
                options.replace = Some(value.to_string());
            }
            ArgToken::LongOption("delimiter", attached) => {
                let value = cursor.next_value_or_maybe_attached(attached, APPLET, "d")?;
                options.delimiter = Some(parse_delimiter(APPLET, value)?);
            }
            ArgToken::LongOption(name, _) => {
                return Err(vec![AppletError::unrecognized_option(
                    APPLET,
                    &format!("--{name}"),
                )]);
            }
            ArgToken::ShortFlags(flags) => {
                let (flag, attached) = flags.split_at(1);

                match flag {
                    "0" if attached.is_empty() => options.null_delim = true,
                    "r" if attached.is_empty() => options.no_run_if_empty = true,
                    "t" if attached.is_empty() => options.verbose = true,
                    "p" if attached.is_empty() => options.interactive = true,
                    "n" => {
                        let value = if attached.is_empty() {
                            cursor.next_value(APPLET, "n")?
                        } else {
                            attached
                        };
                        options.max_args = parse_usize(APPLET, "-n", value)?;
                    }
                    "L" => {
                        let value = if attached.is_empty() {
                            cursor.next_value(APPLET, "L")?
                        } else {
                            attached
                        };
                        options.max_lines = parse_usize(APPLET, "-L", value)?;
                    }
                    "P" => {
                        let value = if attached.is_empty() {
                            cursor.next_value(APPLET, "P")?
                        } else {
                            attached
                        };
                        options.max_procs = parse_usize(APPLET, "-P", value)?;
                    }
                    "I" => {
                        let value = if attached.is_empty() {
                            cursor.next_value(APPLET, "I")?
                        } else {
                            attached
                        };
                        options.replace = Some(value.to_string());
                    }
                    "d" => {
                        let value = if attached.is_empty() {
                            cursor.next_value(APPLET, "d")?
                        } else {
                            attached
                        };
                        options.delimiter = Some(parse_delimiter(APPLET, value)?);
                    }
                    _ => {
                        return Err(vec![AppletError::invalid_option(
                            APPLET,
                            flag.chars().next().unwrap_or('-'),
                        )]);
                    }
                }
            }
        }
    }

    Ok((options, cmd_args))
}

fn build_invocations(
    options: &Options,
    initial_args: &[String],
    tokens: Vec<String>,
) -> Vec<Vec<String>> {
    if tokens.is_empty() {
        return Vec::new();
    }

    if let Some(placeholder) = &options.replace {
        return tokens
            .into_iter()
            .map(|token| {
                initial_args
                    .iter()
                    .map(|arg| arg.replace(placeholder, &token))
                    .collect()
            })
            .collect();
    }

    let batch_size = if options.max_args > 0 {
        options.max_args
    } else if options.max_lines > 0 {
        options.max_lines
    } else {
        usize::MAX
    };

    tokens
        .chunks(batch_size)
        .map(|chunk| {
            let mut args = initial_args.to_vec();
            args.extend(chunk.iter().cloned());
            args
        })
        .collect()
}

fn execute_invocations(
    cmd: &str,
    invocations: Vec<Vec<String>>,
    options: &Options,
) -> AppletResult {
    let mut prompt_input = if options.interactive {
        Some(open_prompt_input()?)
    } else {
        None
    };

    let concurrency = if options.max_procs == 0 {
        invocations.len().max(1)
    } else {
        options.max_procs.max(1)
    };

    if concurrency == 1 {
        execute_sequential(cmd, invocations, options, &mut prompt_input)
    } else {
        execute_parallel(cmd, invocations, options, &mut prompt_input, concurrency)
    }
}

fn execute_sequential(
    cmd: &str,
    invocations: Vec<Vec<String>>,
    options: &Options,
    prompt_input: &mut Option<Box<dyn BufRead>>,
) -> AppletResult {
    let mut last_status = None;
    for args in invocations {
        if !should_run_invocation(cmd, &args, options, prompt_input)? {
            continue;
        }
        let status = invoke(cmd, &args, options.verbose)?;
        last_status = Some(status);
        if status.code() == Some(255) {
            break;
        }
    }

    if let Some(status) = last_status {
        exit_status(status)
    } else {
        Ok(())
    }
}

fn execute_parallel(
    cmd: &str,
    invocations: Vec<Vec<String>>,
    options: &Options,
    prompt_input: &mut Option<Box<dyn BufRead>>,
    concurrency: usize,
) -> AppletResult {
    let mut children = Vec::new();
    let mut last_status = None;
    let mut stop_spawning = false;

    for args in invocations {
        if !should_run_invocation(cmd, &args, options, prompt_input)? {
            continue;
        }

        while children.len() >= concurrency {
            let status = wait_for_one(&mut children)?;
            if status.code() == Some(255) {
                stop_spawning = true;
            }
            last_status = Some(status);
            if stop_spawning {
                break;
            }
        }
        if stop_spawning {
            break;
        }

        children.push(spawn(cmd, &args, options.verbose)?);
    }

    while !children.is_empty() {
        let status = wait_for_one(&mut children)?;
        last_status = Some(status);
    }

    if let Some(status) = last_status {
        exit_status(status)
    } else {
        Ok(())
    }
}

fn should_run_invocation(
    cmd: &str,
    args: &[String],
    options: &Options,
    prompt_input: &mut Option<Box<dyn BufRead>>,
) -> Result<bool, Vec<AppletError>> {
    if !options.interactive {
        return Ok(true);
    }

    let mut stderr = io::stderr().lock();
    prompt_invocation(
        &command_line_for_display(cmd, args),
        prompt_input
            .as_deref_mut()
            .ok_or_else(|| vec![AppletError::new(APPLET, "interactive prompt unavailable")])?,
        &mut stderr,
    )
    .map_err(|err| vec![err])
}

fn prompt_invocation<R: BufRead + ?Sized, W: Write>(
    command_line: &str,
    reader: &mut R,
    writer: &mut W,
) -> Result<bool, AppletError> {
    write!(writer, "{command_line} ?...")
        .map_err(|err| AppletError::from_io(APPLET, "writing stderr", None, err))?;
    writer
        .flush()
        .map_err(|err| AppletError::from_io(APPLET, "writing stderr", None, err))?;

    let mut response = String::new();
    let read = reader
        .read_line(&mut response)
        .map_err(|err| AppletError::from_io(APPLET, "reading prompt response", None, err))?;
    if read == 0 {
        return Ok(false);
    }

    Ok(matches!(
        response.trim_start().chars().next(),
        Some('y' | 'Y')
    ))
}

fn open_prompt_input() -> Result<Box<dyn BufRead>, Vec<AppletError>> {
    match File::open("/dev/tty") {
        Ok(file) => Ok(Box::new(BufReader::new(file))),
        Err(_) => File::open("/dev/stdin")
            .map(|file| Box::new(BufReader::new(file)) as Box<dyn BufRead>)
            .map_err(|err| {
                vec![AppletError::from_io(
                    APPLET,
                    "opening",
                    Some("/dev/tty"),
                    err,
                )]
            }),
    }
}

fn invoke(cmd: &str, args: &[String], verbose: bool) -> Result<ExitStatus, Vec<AppletError>> {
    if verbose {
        eprintln!("{}", command_line_for_display(cmd, args));
    }

    Command::new(cmd)
        .args(args)
        .status()
        .map_err(|err| vec![AppletError::new(APPLET, format!("{cmd}: {err}"))])
}

fn spawn(cmd: &str, args: &[String], verbose: bool) -> Result<Child, Vec<AppletError>> {
    if verbose {
        eprintln!("{}", command_line_for_display(cmd, args));
    }

    Command::new(cmd)
        .args(args)
        .spawn()
        .map_err(|err| vec![AppletError::new(APPLET, format!("{cmd}: {err}"))])
}

fn wait_for_one(children: &mut Vec<Child>) -> Result<ExitStatus, Vec<AppletError>> {
    for index in 0..children.len() {
        match children[index].try_wait() {
            Ok(Some(status)) => {
                let mut child = children.swap_remove(index);
                child.wait().map_err(|err| {
                    vec![AppletError::new(
                        APPLET,
                        format!("waiting for child process failed: {err}"),
                    )]
                })?;
                return Ok(status);
            }
            Ok(None) => {}
            Err(err) => {
                return Err(vec![AppletError::new(
                    APPLET,
                    format!("waiting for child process failed: {err}"),
                )]);
            }
        }
    }

    let mut child = children.swap_remove(0);
    child.wait().map_err(|err| {
        vec![AppletError::new(
            APPLET,
            format!("waiting for child process failed: {err}"),
        )]
    })
}

fn command_line_for_display(cmd: &str, args: &[String]) -> String {
    std::iter::once(cmd)
        .chain(args.iter().map(String::as_str))
        .collect::<Vec<_>>()
        .join(" ")
}

fn exit_status(status: ExitStatus) -> AppletResult {
    if status.success() {
        Ok(())
    } else {
        Err(vec![])
    }
}

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
        if !in_single && !in_double && !current.is_empty() {
            tokens.push(std::mem::take(&mut current));
        }
    }
    if !current.is_empty() {
        tokens.push(current);
    }
    tokens
}

fn read_by_delimiter<R: Read>(mut reader: R, delim: u8) -> Vec<String> {
    let mut data = Vec::new();
    let _ = reader.read_to_end(&mut data);
    data.split(|&byte| byte == delim)
        .filter_map(|chunk| {
            if chunk.is_empty() {
                None
            } else {
                String::from_utf8(chunk.to_vec()).ok()
            }
        })
        .collect()
}

fn parse_delimiter(applet: &'static str, s: &str) -> Result<u8, Vec<AppletError>> {
    if s.len() == 1 {
        return Ok(s.as_bytes()[0]);
    }
    if s.starts_with('\\') && s.len() == 2 {
        return Ok(match s.as_bytes()[1] {
            b'n' => b'\n',
            b't' => b'\t',
            b'0' => b'\0',
            b'\\' => b'\\',
            other => other,
        });
    }
    if let Some(hex) = s.strip_prefix("\\x")
        && let Ok(number) = u8::from_str_radix(hex, 16)
    {
        return Ok(number);
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
    use super::{
        Options, build_invocations, parse_args, prompt_invocation, read_whitespace_delimited, run,
    };
    use std::io::Cursor;

    fn args(values: &[&str]) -> Vec<std::ffi::OsString> {
        values.iter().map(std::ffi::OsString::from).collect()
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
    fn parses_parallel_and_prompt_options() {
        let (options, command) =
            parse_args(&args(&["-p", "-P", "4", "echo", "hi"])).expect("parse xargs");
        assert_eq!(
            options,
            Options {
                interactive: true,
                max_procs: 4,
                ..Options::default()
            }
        );
        assert_eq!(command, vec![String::from("echo"), String::from("hi")]);
    }

    #[test]
    fn parses_long_options_with_attached_values() {
        let (options, command) = parse_args(&args(&[
            "--interactive",
            "--max-procs=4",
            "--replace={}",
            "echo",
            "{}",
        ]))
        .expect("parse xargs");
        assert_eq!(
            options,
            Options {
                interactive: true,
                max_procs: 4,
                replace: Some(String::from("{}")),
                ..Options::default()
            }
        );
        assert_eq!(command, vec![String::from("echo"), String::from("{}")]);
    }

    #[test]
    fn builds_parallel_batches() {
        let options = Options {
            max_args: 2,
            ..Options::default()
        };
        let invocations = build_invocations(
            &options,
            &[String::from("echo")],
            vec![String::from("a"), String::from("b"), String::from("c")],
        );
        assert_eq!(
            invocations,
            vec![
                vec![String::from("echo"), String::from("a"), String::from("b")],
                vec![String::from("echo"), String::from("c")],
            ]
        );
    }

    #[test]
    fn prompt_accepts_yes() {
        let mut input = Cursor::new("yes\n");
        let mut output = Vec::new();
        assert!(prompt_invocation("echo hi", &mut input, &mut output).expect("prompt"));
        assert_eq!(String::from_utf8(output).expect("utf8"), "echo hi ?...");
    }

    #[test]
    fn prompt_rejects_non_yes() {
        let mut input = Cursor::new("no\n");
        let mut output = Vec::new();
        assert!(!prompt_invocation("echo hi", &mut input, &mut output).expect("prompt"));
    }
}
