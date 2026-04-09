use std::fs;
use std::io::{self, Read, Write};
use std::os::unix::fs::MetadataExt;
use std::path::Path;
use std::process::Command;

use crate::common::error::AppletError;

const APPLET: &str = "find";

#[derive(Clone, Debug, Default)]
struct Options {
    maxdepth: Option<usize>,
    xdev: bool,
}

#[derive(Clone, Debug)]
enum Predicate {
    Name(String),
    Type(FileTypeMatch),
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
enum Action {
    Print,
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
struct Query {
    predicates: Vec<Predicate>,
    actions: Vec<Action>,
}

pub fn main(args: &[String]) -> i32 {
    match run(args) {
        Ok(code) => code,
        Err(errors) => {
            for error in errors {
                error.print();
            }
            1
        }
    }
}

fn run(args: &[String]) -> Result<i32, Vec<AppletError>> {
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

    if let Err(error) = finalize_exec_plus(&mut query, &mut exec_plus_failed, &mut errors) {
        errors.push(error);
    }

    if errors.is_empty() {
        Ok(if exec_plus_failed { 1 } else { 0 })
    } else {
        Err(errors)
    }
}

fn parse_args(args: &[String]) -> Result<(Options, Vec<String>, Query), Vec<AppletError>> {
    let mut options = Options::default();
    let mut index = 0;
    while index < args.len() {
        match args[index].as_str() {
            "-xdev" => {
                options.xdev = true;
                index += 1;
            }
            "-maxdepth" => {
                let Some(value) = args.get(index + 1) else {
                    return Err(vec![AppletError::option_requires_arg(APPLET, "maxdepth")]);
                };
                let Some(depth) = value.parse::<usize>().ok() else {
                    return Err(vec![AppletError::new(
                        APPLET,
                        format!("bad maxdepth '{value}'"),
                    )]);
                };
                options.maxdepth = Some(depth);
                index += 2;
            }
            "-H" | "-L" | "-follow" => {
                index += 1;
            }
            _ => break,
        }
    }

    let path_start = index;
    while index < args.len() && !is_expression_token(&args[index]) {
        index += 1;
    }
    let paths = if path_start == index {
        vec![String::from(".")]
    } else {
        args[path_start..index].to_vec()
    };

    let query = parse_query(&args[index..], &mut options)?;
    Ok((options, paths, query))
}

fn is_expression_token(token: &str) -> bool {
    matches!(token, "!" | "(" | ")") || token.starts_with('-')
}

fn parse_query(tokens: &[String], options: &mut Options) -> Result<Query, Vec<AppletError>> {
    let mut predicates = Vec::new();
    let mut actions = Vec::new();
    let mut index = 0;
    let mut implicit_print = true;

    while index < tokens.len() {
        match tokens[index].as_str() {
            "-xdev" => {
                options.xdev = true;
                index += 1;
            }
            "-maxdepth" => {
                let Some(value) = tokens.get(index + 1) else {
                    return Err(vec![AppletError::option_requires_arg(APPLET, "maxdepth")]);
                };
                let Some(depth) = value.parse::<usize>().ok() else {
                    return Err(vec![AppletError::new(
                        APPLET,
                        format!("bad maxdepth '{value}'"),
                    )]);
                };
                options.maxdepth = Some(depth);
                index += 2;
            }
            "-name" => {
                let Some(pattern) = tokens.get(index + 1) else {
                    return Err(vec![AppletError::option_requires_arg(APPLET, "name")]);
                };
                predicates.push(Predicate::Name(pattern.clone()));
                index += 2;
            }
            "-type" => {
                let Some(kind) = tokens.get(index + 1) else {
                    return Err(vec![AppletError::option_requires_arg(APPLET, "type")]);
                };
                predicates.push(Predicate::Type(parse_type(kind)?));
                index += 2;
            }
            "-print" => {
                actions.push(Action::Print);
                implicit_print = false;
                index += 1;
            }
            "-exec" => {
                let (argv, next, plus) = parse_exec_arguments(tokens, index + 1)?;
                actions.push(if plus {
                    Action::ExecPlus {
                        argv,
                        paths: Vec::new(),
                    }
                } else {
                    Action::Exec { argv }
                });
                implicit_print = false;
                index = next;
            }
            "-ok" => {
                let (argv, next, plus) = parse_exec_arguments(tokens, index + 1)?;
                if plus {
                    return Err(vec![AppletError::new(APPLET, "-ok does not support '+'")]);
                }
                actions.push(Action::Ok { argv });
                implicit_print = false;
                index = next;
            }
            token => {
                return Err(vec![AppletError::new(
                    APPLET,
                    format!("unsupported expression '{token}'"),
                )]);
            }
        }
    }

    if actions.is_empty() || implicit_print {
        actions.push(Action::Print);
    }

    Ok(Query {
        predicates,
        actions,
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

fn parse_exec_arguments(
    tokens: &[String],
    mut index: usize,
) -> Result<(Vec<String>, usize, bool), Vec<AppletError>> {
    let start = index;
    while index < tokens.len() {
        if tokens[index] == ";" || tokens[index] == "+" {
            if start == index {
                return Err(vec![AppletError::new(APPLET, "missing command for -exec")]);
            }
            return Ok((
                tokens[start..index].to_vec(),
                index + 1,
                tokens[index] == "+",
            ));
        }
        index += 1;
    }
    Err(vec![AppletError::new(
        APPLET,
        "missing terminator for -exec",
    )])
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
    if matches_query(display, metadata, &query.predicates) {
        run_actions(display, query, stdout, stderr, exec_plus_failed)?;
    }

    if !metadata.is_dir() {
        return Ok(());
    }
    if options.maxdepth.is_some_and(|maxdepth| depth >= maxdepth) {
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

    Ok(())
}

fn matches_query(display: &str, metadata: &fs::Metadata, predicates: &[Predicate]) -> bool {
    predicates.iter().all(|predicate| match predicate {
        Predicate::Name(pattern) => {
            glob_match(pattern.as_bytes(), basename_for_match(display).as_bytes())
        }
        Predicate::Type(kind) => file_type_matches(metadata, *kind),
    })
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
    let file_mask = u32::from(libc::S_IFMT);
    match kind {
        FileTypeMatch::Regular => file_type.is_file(),
        FileTypeMatch::Directory => file_type.is_dir(),
        FileTypeMatch::Symlink => file_type.is_symlink(),
        FileTypeMatch::Block => mode & file_mask == u32::from(libc::S_IFBLK),
        FileTypeMatch::Character => mode & file_mask == u32::from(libc::S_IFCHR),
        FileTypeMatch::Socket => mode & file_mask == u32::from(libc::S_IFSOCK),
        FileTypeMatch::Fifo => mode & file_mask == u32::from(libc::S_IFIFO),
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

fn run_actions(
    display: &str,
    query: &mut Query,
    stdout: &mut impl Write,
    stderr: &mut impl Write,
    exec_plus_failed: &mut bool,
) -> Result<(), AppletError> {
    for action in &mut query.actions {
        match action {
            Action::Print => {
                writeln!(stdout, "{display}")
                    .map_err(|err| AppletError::from_io(APPLET, "writing stdout", None, err))?;
            }
            Action::Exec { argv } => {
                let status = run_command(argv, &[display.to_owned()])
                    .map_err(|err| AppletError::from_io(APPLET, "executing", Some(display), err))?;
                let _ = status;
            }
            Action::ExecPlus { paths, .. } => paths.push(display.to_owned()),
            Action::Ok { argv } => {
                write_prompt(stderr, argv, display)?;
                if read_confirmation()
                    .map_err(|err| AppletError::from_io(APPLET, "reading stdin", None, err))?
                {
                    let status = run_command(argv, &[display.to_owned()]).map_err(|err| {
                        AppletError::from_io(APPLET, "executing", Some(display), err)
                    })?;
                    let _ = status;
                }
            }
        }
    }
    let _ = exec_plus_failed;
    Ok(())
}

fn finalize_exec_plus(
    query: &mut Query,
    exec_plus_failed: &mut bool,
    errors: &mut Vec<AppletError>,
) -> Result<(), AppletError> {
    for action in &mut query.actions {
        if let Action::ExecPlus { argv, paths } = action {
            if paths.is_empty() {
                continue;
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

#[cfg(test)]
mod tests {
    use super::{basename_for_match, glob_match};

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
}
