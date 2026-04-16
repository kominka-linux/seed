use std::fs;
use std::io::{self, BufRead, BufReader, Read, Write};
use std::path::Path;

use crate::common::args::{ArgCursor, ArgToken};
use crate::common::error::AppletError;
use crate::common::io::open_input;

const APPLET: &str = "grep";
const STDIN_LABEL: &str = "(standard input)";

#[derive(Clone, Copy, Debug, Default)]
struct Options {
    fixed: bool,
    extended: bool,
    ignore_case: bool,
    quiet: bool,
    silent: bool,
    line_regexp: bool,
    invert: bool,
    files_with_match: bool,
    files_without_match: bool,
    word: bool,
    only_matching: bool,
    count: bool,
    line_number: bool,
    recursive: bool,
    label_mode: LabelMode,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
enum LabelMode {
    #[default]
    Auto,
    Always,
    Never,
}

#[derive(Clone, Debug)]
struct PatternSet {
    matchers: Vec<Matcher>,
}

#[derive(Clone, Debug)]
enum Matcher {
    Fixed(Vec<u8>),
    Regex(Regex),
}

#[derive(Clone, Debug)]
struct Regex {
    anchored_start: bool,
    anchored_end: bool,
    pieces: Vec<Piece>,
}

#[derive(Clone, Debug)]
struct Piece {
    atom: Atom,
    quantifier: Quantifier,
}

#[derive(Clone, Copy, Debug)]
enum Quantifier {
    One,
    ZeroOrMore,
    OneOrMore,
    ZeroOrOne,
    Exact(usize),
}

#[derive(Clone, Debug)]
enum Atom {
    Literal(u8),
    Any,
    Class(CharClass),
    Group(Vec<Piece>),
}

#[derive(Clone, Debug)]
struct CharClass {
    negated: bool,
    items: Vec<ClassItem>,
}

#[derive(Clone, Debug)]
enum ClassItem {
    Byte(u8),
    Range(u8, u8),
    Xdigit,
}

#[derive(Clone, Debug)]
struct InputTarget {
    label: String,
    path: String,
}

pub fn main(args: &[String]) -> i32 {
    run_main(args, false, false)
}

pub fn main_extended(args: &[String]) -> i32 {
    run_main(args, true, false)
}

pub fn main_fixed(args: &[String]) -> i32 {
    run_main(args, false, true)
}

fn run_main(args: &[String], default_extended: bool, default_fixed: bool) -> i32 {
    match run_with_mode(args, default_extended, default_fixed) {
        Ok(code) => code,
        Err(message) => {
            eprintln!("{APPLET}: {message}");
            2
        }
    }
}

fn run_with_mode(
    args: &[String],
    default_extended: bool,
    default_fixed: bool,
) -> Result<i32, String> {
    let parsed = parse_args(args, default_extended, default_fixed)?;
    let inputs = if parsed.inputs.is_empty() {
        vec![InputTarget {
            label: STDIN_LABEL.to_owned(),
            path: "-".to_owned(),
        }]
    } else if parsed.options.recursive {
        collect_recursive_inputs(&parsed.inputs)?
    } else {
        parsed
            .inputs
            .iter()
            .map(|path| InputTarget {
                label: if path == "-" {
                    STDIN_LABEL.to_owned()
                } else {
                    path.clone()
                },
                path: path.clone(),
            })
            .collect()
    };

    let include_label = match parsed.options.label_mode {
        LabelMode::Always => true,
        LabelMode::Never => false,
        LabelMode::Auto => inputs.len() > 1 || parsed.options.recursive,
    };
    let mut stdout = io::stdout().lock();
    let mut matched_any = false;
    let mut had_error = false;
    let mut printed_nonmatching = false;
    let mut printed_matching = false;

    for input in &inputs {
        let reader = match open_input(&input.path) {
            Ok(reader) => reader,
            Err(err) => {
                had_error = true;
                if !parsed.options.silent {
                    eprintln!("{APPLET}: {}: {err}", input.path);
                }
                continue;
            }
        };

        let result = process_input(
            BufReader::new(reader),
            input,
            include_label,
            &parsed.patterns,
            parsed.options,
            &mut stdout,
        )?;

        if parsed.options.files_without_match {
            if !result.matched_any {
                printed_nonmatching = true;
                writeln!(stdout, "{}", input.label)
                    .map_err(|err| format!("writing stdout: {err}"))?;
            }
        } else if parsed.options.files_with_match {
            if result.matched_any {
                printed_matching = true;
                writeln!(stdout, "{}", input.label)
                    .map_err(|err| format!("writing stdout: {err}"))?;
            }
        } else if parsed.options.count {
            write_prefix(&mut stdout, include_label, &input.label, None)?;
            writeln!(stdout, "{}", result.selected_lines)
                .map_err(|err| format!("writing stdout: {err}"))?;
        }

        if result.matched_any {
            matched_any = true;
        }

        if parsed.options.quiet && result.matched_any {
            break;
        }
    }

    stdout
        .flush()
        .map_err(|err| format!("flushing stdout: {err}"))?;

    if parsed.options.quiet && matched_any {
        return Ok(0);
    }
    if had_error {
        return Ok(2);
    }
    if parsed.options.files_without_match {
        return Ok(if printed_nonmatching { 0 } else { 1 });
    }
    if parsed.options.files_with_match {
        return Ok(if printed_matching { 0 } else { 1 });
    }
    Ok(if matched_any { 0 } else { 1 })
}

struct ParsedArgs {
    options: Options,
    patterns: PatternSet,
    inputs: Vec<String>,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
struct ProcessResult {
    matched_any: bool,
    selected_lines: usize,
}

fn parse_args(
    args: &[String],
    default_extended: bool,
    default_fixed: bool,
) -> Result<ParsedArgs, String> {
    let mut options = Options {
        extended: default_extended,
        fixed: default_fixed,
        ..Options::default()
    };
    let mut inline_patterns = Vec::new();
    let mut pattern_files = Vec::new();
    let mut positionals = Vec::new();
    let mut cursor = ArgCursor::new(args);

    while let Some(arg) = cursor.next_token() {
        if cursor.parsing_flags() && arg.starts_with("--") && arg.len() > 2 {
            if let Some(pattern) = arg.strip_prefix("--regexp=") {
                inline_patterns.push(pattern.to_owned());
                continue;
            }
            if let Some(path) = arg.strip_prefix("--file=") {
                pattern_files.push(path.to_owned());
                continue;
            }
            match arg {
                "--extended-regexp" => options.extended = true,
                "--fixed-strings" => options.fixed = true,
                "--ignore-case" => options.ignore_case = true,
                "--quiet" => options.quiet = true,
                "--silent" => options.silent = true,
                "--line-regexp" => options.line_regexp = true,
                "--invert-match" => options.invert = true,
                "--files-with-matches" => options.files_with_match = true,
                "--files-without-match" => options.files_without_match = true,
                "--word-regexp" => options.word = true,
                "--only-matching" => options.only_matching = true,
                "--recursive" => options.recursive = true,
                "--text" => {}
                "--line-number" => options.line_number = true,
                "--with-filename" => options.label_mode = LabelMode::Always,
                "--no-filename" => options.label_mode = LabelMode::Never,
                "--count" => options.count = true,
                "--regexp" => {
                    let pattern = cursor
                        .next_value(APPLET, "regexp")
                        .map_err(|_| AppletError::option_requires_arg_message("regexp"))?;
                    inline_patterns.push(pattern.to_owned());
                }
                "--file" => {
                    let path = cursor
                        .next_value(APPLET, "file")
                        .map_err(|_| AppletError::option_requires_arg_message("file"))?;
                    pattern_files.push(path.to_owned());
                }
                _ => return Err(AppletError::unrecognized_option_message(arg)),
            }
            continue;
        }

        match if cursor.parsing_flags() && arg.starts_with('-') && arg.len() > 1 {
            ArgToken::ShortFlags(&arg[1..])
        } else {
            ArgToken::Operand(arg)
        } {
            ArgToken::ShortFlags(flags) => {
                let mut chars = flags.chars();
                while let Some(flag) = chars.next() {
                    let attached = chars.as_str();
                    match flag {
                        'E' => options.extended = true,
                        'F' => options.fixed = true,
                        'i' => options.ignore_case = true,
                        'q' => options.quiet = true,
                        's' => options.silent = true,
                        'x' => options.line_regexp = true,
                        'v' => options.invert = true,
                        'l' => options.files_with_match = true,
                        'L' => options.files_without_match = true,
                        'w' => options.word = true,
                        'o' => options.only_matching = true,
                        'r' => options.recursive = true,
                        'a' => {}
                        'n' => options.line_number = true,
                        'H' => options.label_mode = LabelMode::Always,
                        'h' => options.label_mode = LabelMode::Never,
                        'c' => options.count = true,
                        'e' => {
                            let pattern = cursor
                                .next_value_or_attached(attached, APPLET, "e")
                                .map_err(|_| AppletError::option_requires_arg_message("e"))?;
                            inline_patterns.push(pattern.to_owned());
                            break;
                        }
                        'f' => {
                            let path = cursor
                                .next_value_or_attached(attached, APPLET, "f")
                                .map_err(|_| AppletError::option_requires_arg_message("f"))?;
                            pattern_files.push(path.to_owned());
                            break;
                        }
                        _ => return Err(AppletError::invalid_option_message(flag)),
                    }
                }
            }
            ArgToken::Operand(arg) => positionals.push(arg.to_owned()),
        }
    }

    if inline_patterns.is_empty() && pattern_files.is_empty() {
        let Some(pattern) = positionals.first() else {
            return Err("missing pattern".to_owned());
        };
        inline_patterns.push(pattern.clone());
        positionals.remove(0);
    }

    let mut raw_patterns = Vec::new();
    for pattern in &inline_patterns {
        push_inline_patterns(&mut raw_patterns, pattern.as_bytes());
    }
    for file in &pattern_files {
        let bytes =
            read_input(file).map_err(|err| format!("reading patterns from {file}: {err}"))?;
        raw_patterns.extend(split_pattern_lines(&bytes));
    }

    let matchers = if options.fixed {
        raw_patterns
            .into_iter()
            .map(|pattern| Matcher::Fixed(normalize_case(pattern, options.ignore_case)))
            .collect()
    } else {
        let mut matchers = Vec::new();
        for pattern in raw_patterns {
            matchers.push(Matcher::Regex(compile_regex(
                &pattern,
                options.ignore_case,
                options.extended,
            )?));
        }
        matchers
    };

    Ok(ParsedArgs {
        options,
        patterns: PatternSet { matchers },
        inputs: positionals,
    })
}

fn split_pattern_lines(bytes: &[u8]) -> Vec<Vec<u8>> {
    let mut patterns = Vec::new();
    let mut start = 0;
    for (index, byte) in bytes.iter().enumerate() {
        if *byte == b'\n' {
            patterns.push(bytes[start..index].to_vec());
            start = index + 1;
        }
    }
    if start < bytes.len() {
        patterns.push(bytes[start..].to_vec());
    }
    patterns
}

fn push_inline_patterns(raw_patterns: &mut Vec<Vec<u8>>, bytes: &[u8]) {
    if bytes.is_empty() {
        raw_patterns.push(Vec::new());
    } else {
        raw_patterns.extend(split_pattern_lines(bytes));
    }
}

fn collect_recursive_inputs(paths: &[String]) -> Result<Vec<InputTarget>, String> {
    let mut inputs = Vec::new();
    for path in paths {
        collect_path(Path::new(path), true, &mut inputs)?;
    }
    Ok(inputs)
}

fn collect_path(path: &Path, top_level: bool, inputs: &mut Vec<InputTarget>) -> Result<(), String> {
    let symlink_meta =
        fs::symlink_metadata(path).map_err(|err| format!("{}: {err}", path.display()))?;
    let file_type = symlink_meta.file_type();
    if file_type.is_symlink() {
        let target_meta = fs::metadata(path).map_err(|err| format!("{}: {err}", path.display()))?;
        if target_meta.is_dir() {
            if top_level {
                for entry in
                    fs::read_dir(path).map_err(|err| format!("{}: {err}", path.display()))?
                {
                    let entry = entry.map_err(|err| format!("{}: {err}", path.display()))?;
                    collect_path(&entry.path(), false, inputs)?;
                }
            }
            return Ok(());
        }
    }
    if symlink_meta.is_dir() {
        for entry in fs::read_dir(path).map_err(|err| format!("{}: {err}", path.display()))? {
            let entry = entry.map_err(|err| format!("{}: {err}", path.display()))?;
            collect_path(&entry.path(), false, inputs)?;
        }
        return Ok(());
    }
    inputs.push(InputTarget {
        label: path.display().to_string(),
        path: path.display().to_string(),
    });
    Ok(())
}

fn read_input(path: &str) -> io::Result<Vec<u8>> {
    let mut input = open_input(path)?;
    let mut bytes = Vec::new();
    input.read_to_end(&mut bytes)?;
    Ok(bytes)
}

fn process_input(
    mut reader: impl BufRead,
    input: &InputTarget,
    include_label: bool,
    patterns: &PatternSet,
    options: Options,
    stdout: &mut impl Write,
) -> Result<ProcessResult, String> {
    let mut result = ProcessResult::default();
    let mut line = Vec::new();
    let mut line_index = 0usize;

    loop {
        line.clear();
        let read = reader
            .read_until(b'\n', &mut line)
            .map_err(|err| format!("reading {}: {err}", input.path))?;
        if read == 0 {
            break;
        }
        line_index += 1;
        if line.ends_with(b"\n") {
            line.pop();
        }

        let matches = match_line(&line, patterns, options);
        let is_selected = if options.invert {
            matches.is_empty()
        } else {
            !matches.is_empty()
        };
        if !is_selected {
            continue;
        }
        result.matched_any = true;
        result.selected_lines += 1;
        if options.quiet || options.files_with_match || options.files_without_match {
            break;
        }
        if options.count {
            continue;
        }

        if options.only_matching && !options.invert {
            for (start, end) in matches {
                if start == end {
                    break;
                }
                write_prefix(
                    stdout,
                    include_label,
                    &input.label,
                    options.line_number.then_some(line_index),
                )?;
                stdout
                    .write_all(&line[start..end])
                    .and_then(|_| stdout.write_all(b"\n"))
                    .map_err(|err| format!("writing stdout: {err}"))?;
            }
        } else {
            write_prefix(
                stdout,
                include_label,
                &input.label,
                options.line_number.then_some(line_index),
            )?;
            stdout
                .write_all(&line)
                .and_then(|_| stdout.write_all(b"\n"))
                .map_err(|err| format!("writing stdout: {err}"))?;
        }
    }

    Ok(result)
}

fn write_prefix(
    stdout: &mut impl Write,
    include_label: bool,
    label: &str,
    line_number: Option<usize>,
) -> Result<(), String> {
    if include_label {
        write!(stdout, "{label}:").map_err(|err| format!("writing stdout: {err}"))?;
    }
    if let Some(line_number) = line_number {
        write!(stdout, "{line_number}:").map_err(|err| format!("writing stdout: {err}"))?;
    }
    Ok(())
}

fn match_line(line: &[u8], patterns: &PatternSet, options: Options) -> Vec<(usize, usize)> {
    if patterns.matchers.is_empty() {
        return Vec::new();
    }

    for matcher in &patterns.matchers {
        let mut matches = match matcher {
            Matcher::Fixed(pattern) => fixed_matches(line, pattern, options),
            Matcher::Regex(regex) => regex_matches(line, regex, options),
        };
        if !matches.is_empty() {
            if options.line_regexp {
                matches.retain(|(start, end)| *start == 0 && *end == line.len());
            }
            if options.word {
                matches
                    .retain(|(start, end)| *start < *end && is_word_boundary(line, *start, *end));
            }
            if !matches.is_empty() {
                return matches;
            }
        }
    }

    Vec::new()
}

fn fixed_matches(line: &[u8], pattern: &[u8], options: Options) -> Vec<(usize, usize)> {
    let haystack = normalize_case(line.to_vec(), options.ignore_case);
    if options.line_regexp {
        return if haystack == *pattern {
            vec![(0, line.len())]
        } else {
            Vec::new()
        };
    }
    if pattern.is_empty() {
        return vec![(0, 0)];
    }
    let mut matches = Vec::new();
    let mut index = 0;
    while index + pattern.len() <= haystack.len() {
        if haystack[index..index + pattern.len()] == *pattern {
            matches.push((index, index + pattern.len()));
            if options.only_matching {
                index += pattern.len().max(1);
            } else {
                index += 1;
            }
        } else {
            index += 1;
        }
    }
    matches
}

fn regex_matches(line: &[u8], regex: &Regex, options: Options) -> Vec<(usize, usize)> {
    let haystack = normalize_case(line.to_vec(), options.ignore_case);
    if options.only_matching {
        let mut matches = Vec::new();
        let mut start = 0;
        while start <= haystack.len() {
            let Some((match_start, match_end)) = first_regex_match(&haystack, regex, start) else {
                break;
            };
            matches.push((match_start, match_end));
            if match_start == match_end {
                break;
            }
            start = match_end;
        }
        return matches;
    }

    let mut matches = Vec::new();
    let limit = if regex.anchored_start {
        1
    } else {
        haystack.len() + 1
    };
    for start in 0..limit {
        if let Some(end) = match_sequence(&regex.pieces, &haystack, 0, start) {
            if regex.anchored_end && end != haystack.len() {
                continue;
            }
            matches.push((start, end));
            if start == end {
                break;
            }
        }
    }
    matches
}

fn first_regex_match(line: &[u8], regex: &Regex, start_offset: usize) -> Option<(usize, usize)> {
    let limit = if regex.anchored_start {
        1
    } else {
        line.len() + 1
    };
    for start in start_offset..limit {
        if let Some(end) = match_sequence(&regex.pieces, line, 0, start) {
            if regex.anchored_end && end != line.len() {
                continue;
            }
            return Some((start, end));
        }
    }
    None
}

fn match_sequence(
    pieces: &[Piece],
    line: &[u8],
    piece_index: usize,
    offset: usize,
) -> Option<usize> {
    if piece_index == pieces.len() {
        return Some(offset);
    }
    let piece = &pieces[piece_index];
    let mut positions = vec![offset];

    match piece.quantifier {
        Quantifier::One => {
            positions = match_atom(&piece.atom, line, offset);
        }
        Quantifier::ZeroOrMore => {
            positions = repeat_atom(&piece.atom, line, offset, 0, None);
        }
        Quantifier::OneOrMore => {
            positions = repeat_atom(&piece.atom, line, offset, 1, None);
        }
        Quantifier::ZeroOrOne => {
            positions.extend(match_atom(&piece.atom, line, offset));
        }
        Quantifier::Exact(count) => {
            positions = repeat_atom(&piece.atom, line, offset, count, Some(count));
        }
    }

    while let Some(position) = positions.pop() {
        if let Some(end) = match_sequence(pieces, line, piece_index + 1, position) {
            return Some(end);
        }
    }
    None
}

fn repeat_atom(
    atom: &Atom,
    line: &[u8],
    offset: usize,
    min: usize,
    max: Option<usize>,
) -> Vec<usize> {
    let mut frontier = vec![offset];
    let mut repeats = 0_usize;
    let mut positions = if min == 0 { vec![offset] } else { Vec::new() };

    while max.is_none_or(|limit| repeats < limit) {
        let mut next_frontier = Vec::new();
        for position in frontier {
            for end in match_atom(atom, line, position) {
                if end != position {
                    next_frontier.push(end);
                }
            }
        }
        if next_frontier.is_empty() {
            break;
        }
        repeats += 1;
        if repeats >= min {
            positions.extend(next_frontier.iter().copied());
        }
        frontier = next_frontier;
    }
    positions.sort_unstable();
    positions.dedup();
    positions
}

fn match_atom(atom: &Atom, line: &[u8], offset: usize) -> Vec<usize> {
    match atom {
        Atom::Literal(expected) => {
            if offset < line.len() && *expected == line[offset] {
                vec![offset + 1]
            } else {
                Vec::new()
            }
        }
        Atom::Any => {
            if offset < line.len() {
                vec![offset + 1]
            } else {
                Vec::new()
            }
        }
        Atom::Class(class) => {
            let matched = class.items.iter().any(|item| match item {
                ClassItem::Byte(expected) => offset < line.len() && *expected == line[offset],
                ClassItem::Range(start, end) => {
                    offset < line.len() && *start <= line[offset] && line[offset] <= *end
                }
                ClassItem::Xdigit => offset < line.len() && line[offset].is_ascii_hexdigit(),
            });
            let matched = if class.negated { !matched } else { matched };
            if matched && offset < line.len() {
                vec![offset + 1]
            } else {
                Vec::new()
            }
        }
        Atom::Group(pieces) => match_sequence_all(pieces, line, 0, offset),
    }
}

fn match_sequence_all(
    pieces: &[Piece],
    line: &[u8],
    piece_index: usize,
    offset: usize,
) -> Vec<usize> {
    if piece_index == pieces.len() {
        return vec![offset];
    }
    let piece = &pieces[piece_index];
    let positions = match piece.quantifier {
        Quantifier::One => match_atom(&piece.atom, line, offset),
        Quantifier::ZeroOrMore => repeat_atom(&piece.atom, line, offset, 0, None),
        Quantifier::OneOrMore => repeat_atom(&piece.atom, line, offset, 1, None),
        Quantifier::ZeroOrOne => {
            let mut positions = vec![offset];
            positions.extend(match_atom(&piece.atom, line, offset));
            positions
        }
        Quantifier::Exact(count) => repeat_atom(&piece.atom, line, offset, count, Some(count)),
    };

    let mut ends = Vec::new();
    for position in positions {
        ends.extend(match_sequence_all(pieces, line, piece_index + 1, position));
    }
    ends.sort_unstable();
    ends.dedup();
    ends
}

fn is_word_boundary(line: &[u8], start: usize, end: usize) -> bool {
    let left = start == 0 || !is_word_byte(line[start - 1]);
    let right = end == line.len() || !is_word_byte(line[end]);
    left && right
}

fn is_word_byte(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || byte == b'_'
}

fn normalize_case(mut bytes: Vec<u8>, ignore_case: bool) -> Vec<u8> {
    if ignore_case {
        for byte in &mut bytes {
            *byte = byte.to_ascii_lowercase();
        }
    }
    bytes
}

fn compile_regex(pattern: &[u8], ignore_case: bool, extended: bool) -> Result<Regex, String> {
    let pattern = normalize_case(pattern.to_vec(), ignore_case);
    let mut index = 0;
    let mut anchored_start = false;
    let mut anchored_end = false;

    if pattern.first() == Some(&b'^') {
        anchored_start = true;
        index += 1;
    }

    let pieces = parse_pieces(&pattern, &mut index, extended, false, &mut anchored_end)?;

    Ok(Regex {
        anchored_start,
        anchored_end,
        pieces,
    })
}

fn parse_pieces(
    pattern: &[u8],
    index: &mut usize,
    extended: bool,
    in_group: bool,
    anchored_end: &mut bool,
) -> Result<Vec<Piece>, String> {
    let mut pieces = Vec::new();

    while *index < pattern.len() {
        if in_group && pattern[*index] == b')' {
            *index += 1;
            return Ok(pieces);
        }
        if pattern[*index] == b'$' && *index + 1 == pattern.len() {
            *anchored_end = true;
            *index += 1;
            break;
        }

        let atom = if pattern[*index] == b'.' {
            *index += 1;
            Atom::Any
        } else if pattern[*index] == b'[' {
            let (class, next) = parse_char_class(pattern, *index + 1)?;
            *index = next;
            Atom::Class(class)
        } else if extended && pattern[*index] == b'(' {
            *index += 1;
            Atom::Group(parse_pieces(pattern, index, extended, true, anchored_end)?)
        } else if pattern[*index] == b'\\' {
            *index += 1;
            if *index >= pattern.len() {
                return Err("invalid escape".to_owned());
            }
            let byte = pattern[*index];
            *index += 1;
            Atom::Literal(byte)
        } else {
            let byte = pattern[*index];
            *index += 1;
            Atom::Literal(byte)
        };

        let quantifier = parse_quantifier(pattern, index, extended)?;
        pieces.push(Piece { atom, quantifier });
    }

    if in_group {
        return Err("unterminated group".to_owned());
    }
    Ok(pieces)
}

fn parse_quantifier(
    pattern: &[u8],
    index: &mut usize,
    extended: bool,
) -> Result<Quantifier, String> {
    if *index >= pattern.len() {
        return Ok(Quantifier::One);
    }
    let quantifier = match pattern[*index] {
        b'*' => {
            *index += 1;
            Quantifier::ZeroOrMore
        }
        b'+' if extended => {
            *index += 1;
            Quantifier::OneOrMore
        }
        b'?' if extended => {
            *index += 1;
            Quantifier::ZeroOrOne
        }
        b'{' if extended => {
            let start = *index + 1;
            let mut end = start;
            while end < pattern.len() && pattern[end].is_ascii_digit() {
                end += 1;
            }
            if start == end || end >= pattern.len() || pattern[end] != b'}' {
                return Err("invalid repetition".to_owned());
            }
            let count = std::str::from_utf8(&pattern[start..end])
                .ok()
                .and_then(|value| value.parse::<usize>().ok())
                .ok_or_else(|| "invalid repetition".to_owned())?;
            *index = end + 1;
            Quantifier::Exact(count)
        }
        _ => Quantifier::One,
    };
    Ok(quantifier)
}

fn parse_char_class(pattern: &[u8], mut index: usize) -> Result<(CharClass, usize), String> {
    let mut negated = false;
    if index < pattern.len() && pattern[index] == b'^' {
        negated = true;
        index += 1;
    }
    let mut items = Vec::new();
    while index < pattern.len() {
        if pattern[index] == b']' {
            return Ok((CharClass { negated, items }, index + 1));
        }
        if pattern[index] == b'['
            && let Some((item, next)) = parse_posix_class(pattern, index)
        {
            items.push(item);
            index = next;
            continue;
        }
        let start = if pattern[index] == b'\\' && index + 1 < pattern.len() {
            index += 1;
            pattern[index]
        } else {
            pattern[index]
        };
        if index + 2 < pattern.len() && pattern[index + 1] == b'-' && pattern[index + 2] != b']' {
            let end = pattern[index + 2];
            items.push(ClassItem::Range(start, end));
            index += 3;
        } else {
            items.push(ClassItem::Byte(start));
            index += 1;
        }
    }
    Err("unterminated character class".to_owned())
}

fn parse_posix_class(pattern: &[u8], index: usize) -> Option<(ClassItem, usize)> {
    if pattern.get(index..index + 2) != Some(&b"[:"[..]) {
        return None;
    }
    let mut end = index + 2;
    while end + 1 < pattern.len() {
        if pattern[end] == b':' && pattern[end + 1] == b']' {
            break;
        }
        end += 1;
    }
    if end + 1 >= pattern.len() {
        return None;
    }
    match std::str::from_utf8(&pattern[index + 2..end]).ok()? {
        "xdigit" => Some((ClassItem::Xdigit, end + 2)),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::{LabelMode, parse_args, split_pattern_lines};

    fn args(values: &[&str]) -> Vec<String> {
        values.iter().map(|value| value.to_string()).collect()
    }

    #[test]
    fn parses_attached_pattern_option() {
        let parsed = parse_args(&args(&["-efoo", "input"]), false, false).expect("parse grep");
        assert_eq!(parsed.inputs, vec!["input"]);
        assert_eq!(parsed.patterns.matchers.len(), 1);
    }

    #[test]
    fn preserves_empty_pattern_argument() {
        let parsed = parse_args(&args(&["", "input"]), false, false).expect("parse grep");
        assert_eq!(parsed.inputs, vec!["input"]);
        assert_eq!(parsed.patterns.matchers.len(), 1);
    }

    #[test]
    fn preserves_blank_pattern_lines() {
        assert_eq!(split_pattern_lines(b"\n"), vec![Vec::<u8>::new()]);
        assert_eq!(
            split_pattern_lines(b"foo\n\nbar\n"),
            vec![b"foo".to_vec(), Vec::new(), b"bar".to_vec()]
        );
    }

    #[test]
    fn parses_common_gnu_long_options() {
        let parsed = parse_args(
            &args(&[
                "--line-number",
                "--count",
                "--with-filename",
                "--regexp=foo",
                "input",
            ]),
            false,
            false,
        )
        .expect("parse grep");

        assert!(parsed.options.line_number);
        assert!(parsed.options.count);
        assert_eq!(parsed.options.label_mode, LabelMode::Always);
        assert_eq!(parsed.inputs, vec!["input"]);
        assert_eq!(parsed.patterns.matchers.len(), 1);
    }
}
