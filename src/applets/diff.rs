use std::collections::BTreeSet;
use std::fs;
use std::io::{self, Read, Write};
use std::path::Path;

use crate::common::error::AppletError;
use crate::common::io::open_input;

const APPLET: &str = "diff";

#[derive(Clone, Copy, Debug, Default)]
struct Options {
    unified: bool,
    brief: bool,
    ignore_space_change: bool,
    ignore_blank_lines: bool,
    recursive: bool,
    treat_missing_as_empty: bool,
}

#[derive(Clone, Debug)]
struct Line {
    bytes: Vec<u8>,
    has_newline: bool,
}

#[derive(Clone, Copy, Debug)]
enum Op {
    Equal(usize),
    Delete(usize),
    Insert(usize),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Outcome {
    Same,
    Different,
}

pub fn main(args: &[String]) -> i32 {
    match run(args) {
        Ok(code) => code,
        Err(message) => {
            eprintln!("{APPLET}: {message}");
            2
        }
    }
}

fn run(args: &[String]) -> Result<i32, String> {
    let (options, paths) = parse_args(args)?;
    if paths.len() != 2 {
        return Err("missing operand".to_owned());
    }
    if paths[0] == "-" && paths[1] == "-" {
        return Ok(0);
    }

    let mut stdout = io::stdout().lock();
    let outcome = compare_target(
        Path::new(&paths[0]),
        Path::new(&paths[1]),
        &paths[0],
        &paths[1],
        options,
        &mut stdout,
    )?;
    Ok(if outcome == Outcome::Same { 0 } else { 1 })
}

fn parse_args(args: &[String]) -> Result<(Options, Vec<String>), String> {
    let mut options = Options::default();
    let mut paths = Vec::new();
    let mut parsing_flags = true;

    for arg in args {
        if parsing_flags && arg == "--" {
            parsing_flags = false;
            continue;
        }
        if parsing_flags && arg.starts_with('-') && arg.len() > 1 {
            for flag in arg[1..].chars() {
                match flag {
                    'u' => options.unified = true,
                    'q' => options.brief = true,
                    'b' => options.ignore_space_change = true,
                    'B' => options.ignore_blank_lines = true,
                    'r' => options.recursive = true,
                    'N' => options.treat_missing_as_empty = true,
                    _ => return Err(AppletError::invalid_option_message(flag)),
                }
            }
            continue;
        }
        paths.push(arg.clone());
    }

    if !options.unified && !options.brief {
        options.unified = true;
    }

    Ok((options, paths))
}

fn compare_target(
    left_path: &Path,
    right_path: &Path,
    left_display: &str,
    right_display: &str,
    options: Options,
    stdout: &mut impl Write,
) -> Result<Outcome, String> {
    if left_display == "-" || right_display == "-" {
        return compare_files(left_display, right_display, options, stdout);
    }

    let left_meta =
        fs::symlink_metadata(left_path).map_err(|err| format!("reading {left_display}: {err}"))?;
    let right_meta = fs::symlink_metadata(right_path)
        .map_err(|err| format!("reading {right_display}: {err}"))?;

    if left_meta.is_dir() && right_meta.is_dir() {
        if options.recursive {
            compare_directories(
                left_path,
                right_path,
                left_display,
                right_display,
                options,
                stdout,
            )
        } else {
            compare_files(left_display, right_display, options, stdout)
        }
    } else if left_meta.is_dir() && options.recursive {
        compare_directory_with_file(
            left_path,
            right_path,
            left_display,
            right_display,
            options,
            stdout,
            true,
        )
    } else if right_meta.is_dir() && options.recursive {
        compare_directory_with_file(
            right_path,
            left_path,
            right_display,
            left_display,
            options,
            stdout,
            false,
        )
    } else if is_regular_or_dir(&left_meta) && is_regular_or_dir(&right_meta) {
        compare_files(left_display, right_display, options, stdout)
    } else {
        emit_skipped(
            left_display,
            left_meta.file_type(),
            right_display,
            right_meta.file_type(),
            stdout,
        )
    }
}

fn compare_directory_with_file(
    dir_path: &Path,
    file_path: &Path,
    dir_display: &str,
    file_display: &str,
    options: Options,
    stdout: &mut impl Write,
    left_is_dir: bool,
) -> Result<Outcome, String> {
    let Some(name) = file_path.file_name().and_then(|name| name.to_str()) else {
        return Err(format!("reading {file_display}: invalid file name"));
    };
    let child_path = dir_path.join(name);
    let child_display = join_display(dir_display, name);
    if !child_path.exists() {
        return if left_is_dir {
            writeln!(stdout, "Only in {dir_display}: {name}")
                .map_err(|err| format!("writing stdout: {err}"))?;
            Ok(Outcome::Different)
        } else {
            writeln!(stdout, "Only in {dir_display}: {name}")
                .map_err(|err| format!("writing stdout: {err}"))?;
            Ok(Outcome::Different)
        };
    }

    if left_is_dir {
        compare_target(
            &child_path,
            file_path,
            &child_display,
            file_display,
            options,
            stdout,
        )
    } else {
        compare_target(
            file_path,
            &child_path,
            file_display,
            &child_display,
            options,
            stdout,
        )
    }
}

fn compare_directories(
    left_path: &Path,
    right_path: &Path,
    left_display: &str,
    right_display: &str,
    options: Options,
    stdout: &mut impl Write,
) -> Result<Outcome, String> {
    let left_names = read_dir_names(left_path, left_display)?;
    let right_names = read_dir_names(right_path, right_display)?;
    let mut names = BTreeSet::new();
    names.extend(left_names.iter().cloned());
    names.extend(right_names.iter().cloned());

    let mut outcome = Outcome::Same;
    for name in names {
        let left_child = left_path.join(&name);
        let right_child = right_path.join(&name);
        let left_exists = left_names.contains(&name);
        let right_exists = right_names.contains(&name);
        let left_child_display = join_display(left_display, &name);
        let right_child_display = join_display(right_display, &name);

        let child_outcome = match (left_exists, right_exists) {
            (true, true) => compare_existing_children(
                &left_child,
                &right_child,
                &left_child_display,
                &right_child_display,
                options,
                stdout,
            )?,
            (true, false) => compare_missing_child(
                &left_child,
                &left_child_display,
                left_display,
                &name,
                options,
                stdout,
            )?,
            (false, true) => compare_missing_child(
                &right_child,
                &right_child_display,
                right_display,
                &name,
                options,
                stdout,
            )?,
            (false, false) => Outcome::Same,
        };
        if child_outcome == Outcome::Different {
            outcome = Outcome::Different;
        }
    }

    Ok(outcome)
}

fn compare_existing_children(
    left_path: &Path,
    right_path: &Path,
    left_display: &str,
    right_display: &str,
    options: Options,
    stdout: &mut impl Write,
) -> Result<Outcome, String> {
    let left_meta =
        fs::symlink_metadata(left_path).map_err(|err| format!("reading {left_display}: {err}"))?;
    let right_meta = fs::symlink_metadata(right_path)
        .map_err(|err| format!("reading {right_display}: {err}"))?;

    if left_meta.is_dir() && right_meta.is_dir() {
        compare_directories(
            left_path,
            right_path,
            left_display,
            right_display,
            options,
            stdout,
        )
    } else if left_meta.is_dir() && !is_regular_or_dir(&right_meta) {
        let (parent, name) = split_display(right_display);
        writeln!(stdout, "Only in {parent}: {name}")
            .map_err(|err| format!("writing stdout: {err}"))?;
        Ok(Outcome::Different)
    } else if right_meta.is_dir() && !is_regular_or_dir(&left_meta) {
        let (parent, name) = split_display(left_display);
        writeln!(stdout, "Only in {parent}: {name}")
            .map_err(|err| format!("writing stdout: {err}"))?;
        Ok(Outcome::Different)
    } else if is_regular_or_dir(&left_meta) && is_regular_or_dir(&right_meta) {
        compare_target(
            left_path,
            right_path,
            left_display,
            right_display,
            options,
            stdout,
        )
    } else {
        emit_skipped(
            left_display,
            left_meta.file_type(),
            right_display,
            right_meta.file_type(),
            stdout,
        )
    }
}

fn compare_missing_child(
    existing_path: &Path,
    existing_display: &str,
    existing_parent_display: &str,
    name: &str,
    options: Options,
    stdout: &mut impl Write,
) -> Result<Outcome, String> {
    let metadata = fs::symlink_metadata(existing_path)
        .map_err(|err| format!("reading {existing_display}: {err}"))?;
    if options.treat_missing_as_empty && !is_regular_or_dir(&metadata) {
        writeln!(
            stdout,
            "File {existing_display} is not a regular file or directory and was skipped"
        )
        .map_err(|err| format!("writing stdout: {err}"))?;
    } else {
        writeln!(stdout, "Only in {existing_parent_display}: {name}")
            .map_err(|err| format!("writing stdout: {err}"))?;
    }
    Ok(Outcome::Different)
}

fn emit_skipped(
    left_display: &str,
    left_type: fs::FileType,
    right_display: &str,
    right_type: fs::FileType,
    stdout: &mut impl Write,
) -> Result<Outcome, String> {
    if !is_regular_or_dir_type(left_type) {
        writeln!(
            stdout,
            "File {left_display} is not a regular file or directory and was skipped"
        )
        .map_err(|err| format!("writing stdout: {err}"))?;
    }
    if !is_regular_or_dir_type(right_type) {
        writeln!(
            stdout,
            "File {right_display} is not a regular file or directory and was skipped"
        )
        .map_err(|err| format!("writing stdout: {err}"))?;
    }
    Ok(Outcome::Different)
}

fn read_dir_names(path: &Path, display: &str) -> Result<BTreeSet<String>, String> {
    let mut names = BTreeSet::new();
    for entry in fs::read_dir(path).map_err(|err| format!("reading {display}: {err}"))? {
        let entry = entry.map_err(|err| format!("reading {display}: {err}"))?;
        names.insert(entry.file_name().to_string_lossy().into_owned());
    }
    Ok(names)
}

fn join_display(base: &str, name: &str) -> String {
    if base.ends_with('/') {
        format!("{base}{name}")
    } else {
        format!("{base}/{name}")
    }
}

fn split_display(display: &str) -> (&str, &str) {
    match display.rsplit_once('/') {
        Some((parent, name)) => (parent, name),
        None => (".", display),
    }
}

fn is_regular_or_dir(metadata: &fs::Metadata) -> bool {
    is_regular_or_dir_type(metadata.file_type())
}

fn is_regular_or_dir_type(file_type: fs::FileType) -> bool {
    file_type.is_file() || file_type.is_dir()
}

fn compare_files(
    left_display: &str,
    right_display: &str,
    options: Options,
    stdout: &mut impl Write,
) -> Result<Outcome, String> {
    let left = read_lines(left_display).map_err(|err| format!("reading {left_display}: {err}"))?;
    let right =
        read_lines(right_display).map_err(|err| format!("reading {right_display}: {err}"))?;
    let mut ops = diff_ops(&left, &right, options.ignore_space_change);
    if options.ignore_blank_lines {
        ops = filter_blank_only_hunks(ops, &left, &right);
    }

    if ops.iter().all(|op| matches!(op, Op::Equal(_))) {
        return Ok(Outcome::Same);
    }

    if options.brief {
        writeln!(stdout, "Files {left_display} and {right_display} differ")
            .map_err(|err| format!("writing stdout: {err}"))?;
        return Ok(Outcome::Different);
    }

    if options.unified {
        write_unified_diff(stdout, left_display, right_display, &left, &right, &ops)?;
    }
    Ok(Outcome::Different)
}

fn write_unified_diff(
    stdout: &mut impl Write,
    left_display: &str,
    right_display: &str,
    left: &[Line],
    right: &[Line],
    ops: &[Op],
) -> Result<(), String> {
    writeln!(stdout, "--- {left_display}").map_err(|err| format!("writing stdout: {err}"))?;
    writeln!(stdout, "+++ {right_display}").map_err(|err| format!("writing stdout: {err}"))?;
    let old_start = if left.is_empty() { 0 } else { 1 };
    let new_start = if right.is_empty() { 0 } else { 1 };
    writeln!(
        stdout,
        "@@ -{} +{} @@",
        hunk_range(old_start, left.len()),
        hunk_range(new_start, right.len())
    )
    .map_err(|err| format!("writing stdout: {err}"))?;
    for op in ops {
        match *op {
            Op::Equal(index) => write_diff_line(stdout, b' ', &left[index])?,
            Op::Delete(index) => write_diff_line(stdout, b'-', &left[index])?,
            Op::Insert(index) => write_diff_line(stdout, b'+', &right[index])?,
        }
    }
    Ok(())
}

fn hunk_range(start: usize, count: usize) -> String {
    if count == 1 {
        start.to_string()
    } else {
        format!("{start},{count}")
    }
}

fn read_lines(path: &str) -> io::Result<Vec<Line>> {
    let mut input = open_input(path)?;
    let mut bytes = Vec::new();
    input.read_to_end(&mut bytes)?;
    Ok(split_lines(&bytes))
}

fn split_lines(bytes: &[u8]) -> Vec<Line> {
    let mut lines = Vec::new();
    let mut start = 0;
    while start < bytes.len() {
        let end = bytes[start..]
            .iter()
            .position(|byte| *byte == b'\n')
            .map(|offset| start + offset);
        match end {
            Some(end) => {
                lines.push(Line {
                    bytes: bytes[start..end].to_vec(),
                    has_newline: true,
                });
                start = end + 1;
            }
            None => {
                lines.push(Line {
                    bytes: bytes[start..].to_vec(),
                    has_newline: false,
                });
                break;
            }
        }
    }
    lines
}

fn diff_ops(left: &[Line], right: &[Line], ignore_space_change: bool) -> Vec<Op> {
    let mut dp = vec![vec![0_usize; right.len() + 1]; left.len() + 1];
    for i in (0..left.len()).rev() {
        for j in (0..right.len()).rev() {
            if lines_equal(&left[i], &right[j], ignore_space_change) {
                dp[i][j] = dp[i + 1][j + 1] + 1;
            } else {
                dp[i][j] = dp[i + 1][j].max(dp[i][j + 1]);
            }
        }
    }

    let mut ops = Vec::new();
    let mut i = 0;
    let mut j = 0;
    while i < left.len() && j < right.len() {
        if lines_equal(&left[i], &right[j], ignore_space_change) {
            ops.push(Op::Equal(i));
            i += 1;
            j += 1;
        } else if dp[i + 1][j] >= dp[i][j + 1] {
            ops.push(Op::Delete(i));
            i += 1;
        } else {
            ops.push(Op::Insert(j));
            j += 1;
        }
    }
    while i < left.len() {
        ops.push(Op::Delete(i));
        i += 1;
    }
    while j < right.len() {
        ops.push(Op::Insert(j));
        j += 1;
    }
    ops
}

fn lines_equal(left: &Line, right: &Line, ignore_space_change: bool) -> bool {
    if ignore_space_change {
        normalize_space(&left.bytes) == normalize_space(&right.bytes)
    } else {
        left.bytes == right.bytes
    }
}

fn normalize_space(bytes: &[u8]) -> Vec<u8> {
    let mut normalized = Vec::new();
    let mut in_space = false;
    for &byte in bytes {
        if byte.is_ascii_whitespace() {
            in_space = true;
        } else {
            if in_space && !normalized.is_empty() {
                normalized.push(b' ');
            }
            normalized.push(byte);
            in_space = false;
        }
    }
    normalized
}

fn filter_blank_only_hunks(ops: Vec<Op>, left: &[Line], right: &[Line]) -> Vec<Op> {
    let mut filtered = Vec::new();
    let mut block = Vec::new();

    for op in ops {
        if matches!(op, Op::Equal(_)) {
            flush_block(&mut filtered, &mut block, left, right);
            filtered.push(op);
        } else {
            block.push(op);
        }
    }
    flush_block(&mut filtered, &mut block, left, right);
    filtered
}

fn flush_block(filtered: &mut Vec<Op>, block: &mut Vec<Op>, left: &[Line], right: &[Line]) {
    if block.is_empty() {
        return;
    }
    let blank_only = block.iter().all(|op| match *op {
        Op::Delete(index) => is_blank(&left[index]),
        Op::Insert(index) => is_blank(&right[index]),
        Op::Equal(_) => true,
    });
    if !blank_only {
        filtered.append(block);
    } else {
        block.clear();
    }
}

fn is_blank(line: &Line) -> bool {
    line.bytes.iter().all(|byte| byte.is_ascii_whitespace())
}

fn write_diff_line(stdout: &mut impl Write, prefix: u8, line: &Line) -> Result<(), String> {
    stdout
        .write_all(&[prefix])
        .and_then(|_| stdout.write_all(&line.bytes))
        .and_then(|_| stdout.write_all(b"\n"))
        .map_err(|err| format!("writing stdout: {err}"))?;
    if !line.has_newline {
        stdout
            .write_all(b"\\ No newline at end of file\n")
            .map_err(|err| format!("writing stdout: {err}"))?;
    }
    Ok(())
}
