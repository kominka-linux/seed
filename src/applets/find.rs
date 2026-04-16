use std::fs;
use std::io::{self, Read, Write};
use std::os::unix::fs::MetadataExt;
use std::path::Path;
use std::process::Command;

use crate::common::args::ArgCursor;
use crate::common::applet::finish_code;
use crate::common::error::AppletError;

const APPLET: &str = "find";

#[derive(Clone, Debug, Default)]
struct Options {
    mindepth: Option<usize>,
    maxdepth: Option<usize>,
    xdev: bool,
}

#[derive(Clone, Debug)]
enum Primary {
    Name(String),
    Path(String),
    Type(FileTypeMatch),
    Print,
    Prune,
    Delete,
    Exec {
        argv: Vec<String>,
    },
    ExecPlus {
        argv: Vec<String>,
        paths: Vec<String>,
    },
    Ok {
        argv: Vec<String>,
    },
}

#[derive(Clone, Debug)]
enum Expr {
    True,
    Primary(Primary),
    Not(Box<Expr>),
    And(Box<Expr>, Box<Expr>),
    Or(Box<Expr>, Box<Expr>),
}

#[derive(Clone, Copy, Debug)]
enum FileTypeMatch {
    Regular,
    Directory,
    Symlink,
    Block,
    Character,
    Socket,
    Fifo,
}

#[derive(Clone, Debug)]
struct Query {
    expr: Expr,
    has_action: bool,
    postorder: bool,
}

pub fn main(args: &[std::ffi::OsString]) -> i32 {
    finish_code(run(args))
}

fn run(args: &[std::ffi::OsString]) -> Result<i32, Vec<AppletError>> {
    let (options, paths, mut query) = parse_args(args)?;
    let mut stdout = io::stdout().lock();
    let mut stderr = io::stderr().lock();
    let mut errors = Vec::new();
    let mut exec_plus_failed = false;

    for path in &paths {
        let metadata = match fs::symlink_metadata(path) {
            Ok(metadata) => metadata,
            Err(err) => {
                errors.push(AppletError::from_io(APPLET, "reading", Some(path), err));
                continue;
            }
        };
        if let Err(error) = visit(
            Path::new(path),
            path,
            0,
            &metadata,
            metadata.dev(),
            &options,
            &mut query,
            &mut stdout,
            &mut stderr,
            &mut exec_plus_failed,
        ) {
            errors.push(error);
        }
    }

    if let Err(error) = finalize_exec_plus(&mut query.expr, &mut exec_plus_failed, &mut errors) {
        errors.push(error);
    }

    if errors.is_empty() {
        Ok(if exec_plus_failed { 1 } else { 0 })
    } else {
        Err(errors)
    }
}

fn parse_args(args: &[std::ffi::OsString]) -> Result<(Options, Vec<String>, Query), Vec<AppletError>> {
    let mut options = Options::default();
    let mut cursor = ArgCursor::new(args);
    let mut paths = Vec::new();
    let mut expression_tokens = Vec::new();
    let mut parsing_paths = true;

    while let Some(token) = cursor.next_token(APPLET)? {
        if parsing_paths {
            match token {
                "-mindepth" => options.mindepth = Some(parse_depth("mindepth", cursor.next_value(APPLET, "mindepth")?)?),
                "-xdev" => options.xdev = true,
                "-maxdepth" => options.maxdepth = Some(parse_depth("maxdepth", cursor.next_value(APPLET, "maxdepth")?)?),
                "-H" | "-L" | "-follow" => {}
                _ if is_expression_token(token) => {
                    parsing_paths = false;
                    expression_tokens.push(token.to_string());
                }
                _ => paths.push(token.to_string()),
            }
        } else {
            expression_tokens.push(token.to_string());
        }
    }

    let paths = if paths.is_empty() {
        vec![String::from(".")]
    } else {
        paths
    };

    let query = parse_query(&expression_tokens, &mut options)?;
    Ok((options, paths, query))
}

fn is_expression_token(token: &str) -> bool {
    matches!(token, "!" | "(" | ")") || token.starts_with('-')
}

fn parse_query(tokens: &[String], options: &mut Options) -> Result<Query, Vec<AppletError>> {
    let mut cursor = ArgCursor::new(tokens);
    let mut expression_tokens = Vec::new();

    while let Some(token) = cursor.next_token(APPLET)? {
        match token {
            "-mindepth" => options.mindepth = Some(parse_depth("mindepth", cursor.next_value(APPLET, "mindepth")?)?),
            "-xdev" => options.xdev = true,
            "-maxdepth" => options.maxdepth = Some(parse_depth("maxdepth", cursor.next_value(APPLET, "maxdepth")?)?),
            _ => expression_tokens.push(token.to_string()),
        }
    }

    if expression_tokens.is_empty() {
        return Ok(Query {
            expr: Expr::True,
            has_action: false,
            postorder: false,
        });
    }

    let mut parser = Parser::new(&expression_tokens);
    let expr = parser.parse_or()?;
    if let Some(token) = parser.peek() {
        return Err(vec![AppletError::new(
            APPLET,
            format!("unsupported expression '{token}'"),
        )]);
    }

    Ok(Query {
        expr,
        has_action: parser.has_action,
        postorder: parser.postorder,
    })
}

fn parse_depth(option: &str, value: &str) -> Result<usize, Vec<AppletError>> {
    value.parse::<usize>().map_err(|_| {
        vec![AppletError::new(
            APPLET,
            format!("bad {option} '{value}'"),
        )]
    })
}

fn parse_type(kind: &str) -> Result<FileTypeMatch, Vec<AppletError>> {
    let mut chars = kind.chars();
    let Some(ch) = chars.next() else {
        return Err(vec![AppletError::new(APPLET, "missing file type")]);
    };
    if chars.next().is_some() {
        return Err(vec![AppletError::new(
            APPLET,
            format!("bad file type '{kind}'"),
        )]);
    }
    let kind = match ch {
        'f' => FileTypeMatch::Regular,
        'd' => FileTypeMatch::Directory,
        'l' => FileTypeMatch::Symlink,
        'b' => FileTypeMatch::Block,
        'c' => FileTypeMatch::Character,
        's' => FileTypeMatch::Socket,
        'p' => FileTypeMatch::Fifo,
        _ => {
            return Err(vec![AppletError::new(
                APPLET,
                format!("bad file type '{kind}'"),
            )]);
        }
    };
    Ok(kind)
}

#[allow(clippy::too_many_arguments)]
fn visit(
    path: &Path,
    display: &str,
    depth: usize,
    metadata: &fs::Metadata,
    root_dev: u64,
    options: &Options,
    query: &mut Query,
    stdout: &mut impl Write,
    stderr: &mut impl Write,
    exec_plus_failed: &mut bool,
) -> Result<(), AppletError> {
    let can_eval = options.mindepth.is_none_or(|mindepth| depth >= mindepth);
    let mut should_prune = false;
    if can_eval && !query.postorder {
        let outcome = evaluate_expr(
            path,
            display,
            metadata,
            &mut query.expr,
            stdout,
            stderr,
            exec_plus_failed,
        )?;
        if outcome.matched && !query.has_action {
            writeln!(stdout, "{display}")
                .map_err(|err| AppletError::from_io(APPLET, "writing stdout", None, err))?;
        }
        should_prune = outcome.prune;
    }

    if !metadata.is_dir() || should_prune || options.maxdepth.is_some_and(|maxdepth| depth >= maxdepth)
    {
        if can_eval && query.postorder {
            let outcome = evaluate_expr(
                path,
                display,
                metadata,
                &mut query.expr,
                stdout,
                stderr,
                exec_plus_failed,
            )?;
            if outcome.matched && !query.has_action {
                writeln!(stdout, "{display}")
                    .map_err(|err| AppletError::from_io(APPLET, "writing stdout", None, err))?;
            }
        }
        return Ok(());
    }

    for entry in fs::read_dir(path)
        .map_err(|err| AppletError::from_io(APPLET, "reading", Some(display), err))?
    {
        let entry =
            entry.map_err(|err| AppletError::from_io(APPLET, "reading", Some(display), err))?;
        let child_path = entry.path();
        let child_display = join_display(display, &entry.file_name().to_string_lossy());
        let child_meta = fs::symlink_metadata(&child_path)
            .map_err(|err| AppletError::from_io(APPLET, "reading", Some(&child_display), err))?;

        if options.xdev && child_meta.dev() != root_dev && child_meta.is_dir() {
            continue;
        }

        visit(
            &child_path,
            &child_display,
            depth + 1,
            &child_meta,
            root_dev,
            options,
            query,
            stdout,
            stderr,
            exec_plus_failed,
        )?;
    }

    if can_eval && query.postorder {
        let outcome = evaluate_expr(
            path,
            display,
            metadata,
            &mut query.expr,
            stdout,
            stderr,
            exec_plus_failed,
        )?;
        if outcome.matched && !query.has_action {
            writeln!(stdout, "{display}")
                .map_err(|err| AppletError::from_io(APPLET, "writing stdout", None, err))?;
        }
    }

    Ok(())
}

struct Parser<'a> {
    tokens: &'a [String],
    index: usize,
    has_action: bool,
    postorder: bool,
}

impl<'a> Parser<'a> {
    fn new(tokens: &'a [String]) -> Self {
        Self {
            tokens,
            index: 0,
            has_action: false,
            postorder: false,
        }
    }

    fn peek(&self) -> Option<&'a str> {
        self.tokens.get(self.index).map(String::as_str)
    }

    fn next(&mut self) -> Option<&'a str> {
        let token = self.peek()?;
        self.index += 1;
        Some(token)
    }

    fn parse_or(&mut self) -> Result<Expr, Vec<AppletError>> {
        let mut expr = self.parse_and()?;
        while self.peek() == Some("-o") {
            self.index += 1;
            expr = Expr::Or(Box::new(expr), Box::new(self.parse_and()?));
        }
        Ok(expr)
    }

    fn parse_and(&mut self) -> Result<Expr, Vec<AppletError>> {
        let mut expr = self.parse_not()?;
        loop {
            match self.peek() {
                Some("-a") => {
                    self.index += 1;
                    expr = Expr::And(Box::new(expr), Box::new(self.parse_not()?));
                }
                Some(token) if starts_primary_expression(token) => {
                    expr = Expr::And(Box::new(expr), Box::new(self.parse_not()?));
                }
                _ => return Ok(expr),
            }
        }
    }

    fn parse_not(&mut self) -> Result<Expr, Vec<AppletError>> {
        if self.peek() == Some("!") {
            self.index += 1;
            return Ok(Expr::Not(Box::new(self.parse_not()?)));
        }
        self.parse_primary()
    }

    fn parse_primary(&mut self) -> Result<Expr, Vec<AppletError>> {
        match self.next() {
            Some("(") => {
                let expr = self.parse_or()?;
                if self.next() != Some(")") {
                    return Err(vec![AppletError::new(APPLET, "missing ')'")]);
                }
                Ok(expr)
            }
            Some("-name") => {
                let Some(pattern) = self.next() else {
                    return Err(vec![AppletError::option_requires_arg(APPLET, "name")]);
                };
                Ok(Expr::Primary(Primary::Name(pattern.to_owned())))
            }
            Some("-path") => {
                let Some(pattern) = self.next() else {
                    return Err(vec![AppletError::option_requires_arg(APPLET, "path")]);
                };
                Ok(Expr::Primary(Primary::Path(pattern.to_owned())))
            }
            Some("-type") => {
                let Some(kind) = self.next() else {
                    return Err(vec![AppletError::option_requires_arg(APPLET, "type")]);
                };
                Ok(Expr::Primary(Primary::Type(parse_type(kind)?)))
            }
            Some("-print") => {
                self.has_action = true;
                Ok(Expr::Primary(Primary::Print))
            }
            Some("-prune") => {
                self.has_action = true;
                Ok(Expr::Primary(Primary::Prune))
            }
            Some("-delete") => {
                self.has_action = true;
                self.postorder = true;
                Ok(Expr::Primary(Primary::Delete))
            }
            Some("-exec") => {
                self.has_action = true;
                let (argv, plus) = self.parse_exec_arguments()?;
                Ok(Expr::Primary(if plus {
                    Primary::ExecPlus {
                        argv,
                        paths: Vec::new(),
                    }
                } else {
                    Primary::Exec { argv }
                }))
            }
            Some("-ok") => {
                self.has_action = true;
                let (argv, plus) = self.parse_exec_arguments()?;
                if plus {
                    return Err(vec![AppletError::new(APPLET, "-ok does not support '+'")]);
                }
                Ok(Expr::Primary(Primary::Ok { argv }))
            }
            Some(token) => Err(vec![AppletError::new(
                APPLET,
                format!("unsupported expression '{token}'"),
            )]),
            None => Err(vec![AppletError::new(APPLET, "argument expected")]),
        }
    }

    fn parse_exec_arguments(&mut self) -> Result<(Vec<String>, bool), Vec<AppletError>> {
        let start = self.index;
        while let Some(token) = self.peek() {
            if token == ";" || token == "+" {
                if start == self.index {
                    return Err(vec![AppletError::new(APPLET, "missing command for -exec")]);
                }
                let argv = self.tokens[start..self.index].to_vec();
                let plus = token == "+";
                self.index += 1;
                return Ok((argv, plus));
            }
            self.index += 1;
        }
        Err(vec![AppletError::new(
            APPLET,
            "missing terminator for -exec",
        )])
    }
}

fn starts_primary_expression(token: &str) -> bool {
    matches!(
        token,
        "!"
            | "("
            | "-name"
            | "-path"
            | "-type"
            | "-print"
            | "-prune"
            | "-delete"
            | "-exec"
            | "-ok"
    )
}

fn basename_for_match(display: &str) -> String {
    if display.chars().all(|ch| ch == '/') {
        return String::from("/");
    }
    let trimmed = display.trim_end_matches('/');
    match trimmed.rsplit_once('/') {
        Some((_, name)) => name.to_owned(),
        None => trimmed.to_owned(),
    }
}

fn file_type_matches(metadata: &fs::Metadata, kind: FileTypeMatch) -> bool {
    let file_type = metadata.file_type();
    let mode = metadata.mode();
    #[cfg(target_os = "linux")]
    let file_mask: u32 = libc::S_IFMT;
    #[cfg(target_os = "macos")]
    let file_mask = u32::from(libc::S_IFMT);
    #[cfg(target_os = "linux")]
    let block_mask: u32 = libc::S_IFBLK;
    #[cfg(target_os = "macos")]
    let block_mask = u32::from(libc::S_IFBLK);
    #[cfg(target_os = "linux")]
    let char_mask: u32 = libc::S_IFCHR;
    #[cfg(target_os = "macos")]
    let char_mask = u32::from(libc::S_IFCHR);
    #[cfg(target_os = "linux")]
    let socket_mask: u32 = libc::S_IFSOCK;
    #[cfg(target_os = "macos")]
    let socket_mask = u32::from(libc::S_IFSOCK);
    #[cfg(target_os = "linux")]
    let fifo_mask: u32 = libc::S_IFIFO;
    #[cfg(target_os = "macos")]
    let fifo_mask = u32::from(libc::S_IFIFO);
    match kind {
        FileTypeMatch::Regular => file_type.is_file(),
        FileTypeMatch::Directory => file_type.is_dir(),
        FileTypeMatch::Symlink => file_type.is_symlink(),
        FileTypeMatch::Block => mode & file_mask == block_mask,
        FileTypeMatch::Character => mode & file_mask == char_mask,
        FileTypeMatch::Socket => mode & file_mask == socket_mask,
        FileTypeMatch::Fifo => mode & file_mask == fifo_mask,
    }
}

fn glob_match(pattern: &[u8], text: &[u8]) -> bool {
    if pattern.is_empty() {
        return text.is_empty();
    }
    match pattern[0] {
        b'*' => {
            glob_match(&pattern[1..], text) || (!text.is_empty() && glob_match(pattern, &text[1..]))
        }
        b'?' => !text.is_empty() && glob_match(&pattern[1..], &text[1..]),
        byte => !text.is_empty() && byte == text[0] && glob_match(&pattern[1..], &text[1..]),
    }
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
struct EvalOutcome {
    matched: bool,
    prune: bool,
}

fn evaluate_expr(
    path: &Path,
    display: &str,
    metadata: &fs::Metadata,
    expr: &mut Expr,
    stdout: &mut impl Write,
    stderr: &mut impl Write,
    exec_plus_failed: &mut bool,
) -> Result<EvalOutcome, AppletError> {
    match expr {
        Expr::True => Ok(EvalOutcome {
            matched: true,
            prune: false,
        }),
        Expr::Not(inner) => {
            let outcome = evaluate_expr(
                path,
                display,
                metadata,
                inner,
                stdout,
                stderr,
                exec_plus_failed,
            )?;
            Ok(EvalOutcome {
                matched: !outcome.matched,
                prune: outcome.prune,
            })
        }
        Expr::And(left, right) => {
            let left_outcome = evaluate_expr(
                path,
                display,
                metadata,
                left,
                stdout,
                stderr,
                exec_plus_failed,
            )?;
            if !left_outcome.matched {
                return Ok(left_outcome);
            }
            let right_outcome = evaluate_expr(
                path,
                display,
                metadata,
                right,
                stdout,
                stderr,
                exec_plus_failed,
            )?;
            Ok(EvalOutcome {
                matched: true,
                prune: left_outcome.prune || right_outcome.prune,
            })
        }
        Expr::Or(left, right) => {
            let left_outcome = evaluate_expr(
                path,
                display,
                metadata,
                left,
                stdout,
                stderr,
                exec_plus_failed,
            )?;
            if left_outcome.matched {
                return Ok(left_outcome);
            }
            let right_outcome = evaluate_expr(
                path,
                display,
                metadata,
                right,
                stdout,
                stderr,
                exec_plus_failed,
            )?;
            Ok(EvalOutcome {
                matched: right_outcome.matched,
                prune: left_outcome.prune || right_outcome.prune,
            })
        }
        Expr::Primary(primary) => evaluate_primary(
            path,
            display,
            metadata,
            primary,
            stdout,
            stderr,
            exec_plus_failed,
        ),
    }
}

fn evaluate_primary(
    path: &Path,
    display: &str,
    metadata: &fs::Metadata,
    primary: &mut Primary,
    stdout: &mut impl Write,
    stderr: &mut impl Write,
    exec_plus_failed: &mut bool,
) -> Result<EvalOutcome, AppletError> {
    match primary {
        Primary::Name(pattern) => Ok(EvalOutcome {
            matched: glob_match(pattern.as_bytes(), basename_for_match(display).as_bytes()),
            prune: false,
        }),
        Primary::Path(pattern) => Ok(EvalOutcome {
            matched: glob_match(pattern.as_bytes(), display.as_bytes()),
            prune: false,
        }),
        Primary::Type(kind) => Ok(EvalOutcome {
            matched: file_type_matches(metadata, *kind),
            prune: false,
        }),
        Primary::Print => {
            writeln!(stdout, "{display}")
                .map_err(|err| AppletError::from_io(APPLET, "writing stdout", None, err))?;
            Ok(EvalOutcome {
                matched: true,
                prune: false,
            })
        }
        Primary::Prune => Ok(EvalOutcome {
            matched: true,
            prune: metadata.is_dir(),
        }),
        Primary::Delete => {
            delete_path(path, display, metadata)?;
            Ok(EvalOutcome {
                matched: true,
                prune: false,
            })
        }
        Primary::Exec { argv } => {
            let status = run_command(argv, &[display.to_owned()])
                .map_err(|err| AppletError::from_io(APPLET, "executing", Some(display), err))?;
            Ok(EvalOutcome {
                matched: status.success(),
                prune: false,
            })
        }
        Primary::ExecPlus { paths, .. } => {
            paths.push(display.to_owned());
            let _ = exec_plus_failed;
            Ok(EvalOutcome {
                matched: true,
                prune: false,
            })
        }
        Primary::Ok { argv } => {
            write_prompt(stderr, argv, display)?;
            if !read_confirmation()
                .map_err(|err| AppletError::from_io(APPLET, "reading stdin", None, err))?
            {
                return Ok(EvalOutcome {
                    matched: false,
                    prune: false,
                });
            }
            let status = run_command(argv, &[display.to_owned()])
                .map_err(|err| AppletError::from_io(APPLET, "executing", Some(display), err))?;
            Ok(EvalOutcome {
                matched: status.success(),
                prune: false,
            })
        }
    }
}

fn finalize_exec_plus(
    expr: &mut Expr,
    exec_plus_failed: &mut bool,
    errors: &mut Vec<AppletError>,
) -> Result<(), AppletError> {
    match expr {
        Expr::True => {}
        Expr::Not(inner) => finalize_exec_plus(inner, exec_plus_failed, errors)?,
        Expr::And(left, right) | Expr::Or(left, right) => {
            finalize_exec_plus(left, exec_plus_failed, errors)?;
            finalize_exec_plus(right, exec_plus_failed, errors)?;
        }
        Expr::Primary(primary) => {
            if let Primary::ExecPlus { argv, paths } = primary {
                if paths.is_empty() {
                    return Ok(());
                }
                match run_command(argv, paths) {
                    Ok(status) => {
                        if !status.success() {
                            *exec_plus_failed = true;
                        }
                    }
                    Err(err) => errors.push(AppletError::from_io(APPLET, "executing", None, err)),
                }
            }
        }
    }
    Ok(())
}

fn write_prompt(
    stderr: &mut impl Write,
    argv: &[String],
    display: &str,
) -> Result<(), AppletError> {
    let mut first = true;
    for arg in argv {
        if !first {
            write!(stderr, " ")
                .map_err(|err| AppletError::from_io(APPLET, "writing stderr", None, err))?;
        }
        let value = if arg == "{}" { display } else { arg };
        write!(stderr, "{value}")
            .map_err(|err| AppletError::from_io(APPLET, "writing stderr", None, err))?;
        first = false;
    }
    write!(stderr, " ?")
        .map_err(|err| AppletError::from_io(APPLET, "writing stderr", None, err))?;
    stderr
        .flush()
        .map_err(|err| AppletError::from_io(APPLET, "writing stderr", None, err))
}

fn read_confirmation() -> io::Result<bool> {
    let mut byte = [0_u8; 1];
    let read = io::stdin().read(&mut byte)?;
    if read == 0 {
        return Ok(false);
    }
    Ok(matches!(byte[0], b'y' | b'Y'))
}

fn run_command(argv: &[String], paths: &[String]) -> io::Result<std::process::ExitStatus> {
    let Some(program) = argv.first() else {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "missing command for -exec",
        ));
    };
    let mut command = Command::new(program);
    let mut replaced = false;
    for arg in &argv[1..] {
        if arg == "{}" {
            command.args(paths);
            replaced = true;
        } else {
            command.arg(arg);
        }
    }
    if !replaced {
        command.args(paths);
    }
    command.status()
}

fn join_display(base: &str, name: &str) -> String {
    if base.ends_with('/') {
        format!("{base}{name}")
    } else {
        format!("{base}/{name}")
    }
}

fn delete_path(path: &Path, display: &str, metadata: &fs::Metadata) -> Result<(), AppletError> {
    let result = if metadata.is_dir() {
        fs::remove_dir(path)
    } else {
        fs::remove_file(path)
    };
    result.map_err(|err| AppletError::from_io(APPLET, "deleting", Some(display), err))
}

#[cfg(test)]
mod tests {
    use super::{Parser, basename_for_match, glob_match, parse_args};

    fn args(values: &[&str]) -> Vec<String> {
        values.iter().map(|value| value.to_string()).collect()
    }

    fn args_os(values: &[&str]) -> Vec<std::ffi::OsString> {
        values.iter().map(std::ffi::OsString::from).collect()
    }

    #[test]
    fn basename_matching_handles_rootish_inputs() {
        assert_eq!(basename_for_match("/"), "/");
        assert_eq!(basename_for_match("//"), "/");
        assert_eq!(basename_for_match(".///"), ".");
        assert_eq!(basename_for_match("./testfile"), "testfile");
    }

    #[test]
    fn basic_glob_matching_works() {
        assert!(glob_match(b"*", b"abc"));
        assert!(glob_match(b"te?t*", b"testfile"));
        assert!(!glob_match(b"foo", b"bar"));
    }

    #[test]
    fn parser_supports_parenthesized_or() {
        let tokens = args(&["(", "-name", "a", "-o", "-name", "b", ")", "-type", "f"]);
        let mut parser = Parser::new(&tokens);
        parser.parse_or().expect("parse expression");
        assert!(parser.peek().is_none());
    }

    #[test]
    fn parser_tracks_actions() {
        let tokens = args(&["-name", "a", "-exec", "true", "{}", ";"]);
        let mut parser = Parser::new(&tokens);
        parser.parse_or().expect("parse expression");
        assert!(parser.has_action);
    }

    #[test]
    fn parser_supports_path_prune_and_delete() {
        let parsed = parse_args(&args_os(&[
            ".",
            "-mindepth",
            "1",
            "-path",
            "./skip",
            "-prune",
            "-o",
            "-delete",
        ]))
        .expect("parse args");

        assert_eq!(parsed.0.mindepth, Some(1));
        assert!(parsed.2.has_action);
        assert!(parsed.2.postorder);
    }
}
