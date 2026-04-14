use std::ffi::CString;
use std::fs;
use std::io::{Read, Write};
use std::mem::MaybeUninit;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use crate::common::args::{ArgCursor, ArgToken};
use crate::common::io::{open_input, stdout};

const APPLET: &str = "sed";
const MATCH_SLOTS: usize = 10;

#[derive(Clone, Debug, Default, Eq, PartialEq)]
struct Options {
    quiet: bool,
    extended: bool,
    in_place: Option<String>,
}

#[derive(Debug, Eq, PartialEq)]
struct ParsedArgs {
    options: Options,
    script: String,
    files: Vec<String>,
}

#[derive(Debug)]
struct Program {
    commands: Vec<Command>,
}

#[derive(Debug)]
struct Command {
    address1: Option<Address>,
    address2: Option<Address>,
    negate: bool,
    kind: CommandKind,
}

#[derive(Debug)]
enum CommandKind {
    Substitute(SubstituteCommand),
    Print,
    Delete,
    Quit(Option<i32>),
    Append(String),
    Insert(String),
    Change(String),
    Number,
}

#[derive(Debug)]
struct SubstituteCommand {
    pattern: Regex,
    replacement: String,
    global: bool,
    print: bool,
    occurrence: Option<usize>,
}

#[derive(Debug)]
enum Address {
    Line(usize),
    Last,
    Regex(Regex),
}

#[derive(Debug)]
struct Regex {
    raw: libc::regex_t,
}

#[derive(Clone, Debug)]
struct InputFile {
    path: Option<String>,
    lines: Vec<Line>,
}

#[derive(Clone, Debug)]
struct Line {
    text: String,
    had_newline: bool,
}

#[derive(Clone, Copy, Debug, Default)]
struct RuntimeState {
    range_active: bool,
}

#[derive(Debug)]
struct PatternSpace {
    text: String,
    had_newline: bool,
}

#[derive(Clone, Copy, Debug)]
struct MatchSpan {
    start: usize,
    end: usize,
}

#[derive(Clone, Debug)]
struct RegexMatch {
    full: MatchSpan,
    groups: [Option<MatchSpan>; MATCH_SLOTS],
}

#[derive(Debug, Default)]
struct LineOutput {
    before: Vec<String>,
    after: Vec<String>,
    print_now: Vec<String>,
    suppress_default_print: bool,
    stop_commands: bool,
    quit_code: Option<i32>,
}

#[derive(Clone, Copy, Debug)]
struct AddressDecision {
    selected: bool,
    started_range: bool,
}

pub fn main(args: &[String]) -> i32 {
    match run(args) {
        Ok(code) => code,
        Err(message) => {
            eprintln!("{APPLET}: {message}");
            1
        }
    }
}

fn run(args: &[String]) -> Result<i32, String> {
    let parsed = parse_args(args)?;
    let program = parse_program(&parsed.script, parsed.options.extended)?;
    let inputs = load_inputs(&parsed.files, parsed.options.in_place.is_some())?;
    execute(program, inputs, parsed.options)
}

fn parse_args(args: &[String]) -> Result<ParsedArgs, String> {
    let mut options = Options::default();
    let mut expressions = Vec::new();
    let mut script_files = Vec::new();
    let mut operands = Vec::new();
    let mut cursor = ArgCursor::new(args);

    while let Some(arg) = cursor.next_arg() {
        match arg {
            ArgToken::ShortFlags(flags) => {
                let mut chars = flags.chars();
                while let Some(flag) = chars.next() {
                    let attached = chars.as_str();
                    match flag {
                        'n' => options.quiet = true,
                        'E' | 'r' => options.extended = true,
                        'e' => {
                            let script = cursor
                                .next_value_or_attached(attached, APPLET, "e")
                                .map_err(|_| String::from("option requires an argument -- 'e'"))?;
                            expressions.push(script.to_owned());
                            break;
                        }
                        'f' => {
                            let path = cursor
                                .next_value_or_attached(attached, APPLET, "f")
                                .map_err(|_| String::from("option requires an argument -- 'f'"))?;
                            script_files.push(path.to_owned());
                            break;
                        }
                        'i' => {
                            options.in_place = Some(attached.to_owned());
                            break;
                        }
                        _ => return Err(format!("invalid option -- '{flag}'")),
                    }
                }
            }
            ArgToken::Operand(arg) => operands.push(arg.to_owned()),
        }
    }

    if expressions.is_empty() && script_files.is_empty() {
        let Some(script) = operands.first() else {
            return Err(String::from("missing script"));
        };
        expressions.push(script.clone());
        operands.remove(0);
    }

    for path in script_files {
        let text = fs::read_to_string(&path).map_err(|err| format!("reading {path}: {err}"))?;
        expressions.push(text);
    }

    Ok(ParsedArgs {
        options,
        script: expressions.join("\n"),
        files: operands,
    })
}

fn parse_program(script: &str, extended: bool) -> Result<Program, String> {
    let mut parser = ScriptParser::new(script, extended);
    let mut commands = Vec::new();

    while parser.skip_separators_and_comments() {
        commands.push(parser.parse_command()?);
    }

    Ok(Program { commands })
}

struct ScriptParser<'a> {
    script: &'a str,
    pos: usize,
    extended: bool,
}

impl<'a> ScriptParser<'a> {
    fn new(script: &'a str, extended: bool) -> Self {
        Self {
            script,
            pos: 0,
            extended,
        }
    }

    fn skip_separators_and_comments(&mut self) -> bool {
        loop {
            while matches!(self.peek_byte(), Some(b';' | b'\n' | b' ' | b'\t' | b'\r')) {
                self.pos += 1;
            }
            if self.peek_byte() == Some(b'#') {
                self.consume_rest_of_line();
                continue;
            }
            return self.pos < self.script.len();
        }
    }

    fn parse_command(&mut self) -> Result<Command, String> {
        let address1 = self.parse_optional_address()?;
        let mut address2 = None;
        if self.peek_byte() == Some(b',') {
            self.pos += 1;
            address2 = Some(
                self.parse_optional_address()?
                    .ok_or_else(|| String::from("missing second address"))?,
            );
        }
        self.skip_spaces();
        let negate = if self.peek_byte() == Some(b'!') {
            self.pos += 1;
            self.skip_spaces();
            true
        } else {
            false
        };
        let command = self
            .next_byte()
            .ok_or_else(|| String::from("missing command"))?;
        let kind = match command {
            b's' => CommandKind::Substitute(self.parse_substitute()?),
            b'p' => CommandKind::Print,
            b'd' => CommandKind::Delete,
            b'q' => CommandKind::Quit(self.parse_optional_number()?),
            b'a' => CommandKind::Append(self.parse_text_argument()),
            b'i' => CommandKind::Insert(self.parse_text_argument()),
            b'c' => CommandKind::Change(self.parse_text_argument()),
            b'=' => CommandKind::Number,
            other => return Err(format!("unsupported sed command '{}'", other as char)),
        };
        Ok(Command {
            address1,
            address2,
            negate,
            kind,
        })
    }

    fn parse_optional_address(&mut self) -> Result<Option<Address>, String> {
        self.skip_spaces();
        match self.peek_byte() {
            Some(b'$') => {
                self.pos += 1;
                Ok(Some(Address::Last))
            }
            Some(b'/') => {
                let pattern = self.parse_regex(b'/')?;
                Ok(Some(Address::Regex(pattern)))
            }
            Some(byte) if byte.is_ascii_digit() => Ok(Some(Address::Line(self.parse_number()?))),
            _ => Ok(None),
        }
    }

    fn parse_regex(&mut self, delimiter: u8) -> Result<Regex, String> {
        if self.next_byte() != Some(delimiter) {
            return Err(String::from("missing regex delimiter"));
        }
        let raw = self.parse_delimited(delimiter)?;
        Regex::compile(&raw, self.extended)
    }

    fn parse_substitute(&mut self) -> Result<SubstituteCommand, String> {
        let delimiter = self
            .next_byte()
            .ok_or_else(|| String::from("missing substitute delimiter"))?;
        let pattern = Regex::compile(&self.parse_delimited(delimiter)?, self.extended)?;
        let replacement = self.parse_delimited(delimiter)?;
        let mut global = false;
        let mut print = false;
        let mut occurrence = None;
        while let Some(byte) = self.peek_byte() {
            match byte {
                b'g' => {
                    global = true;
                    self.pos += 1;
                }
                b'p' => {
                    print = true;
                    self.pos += 1;
                }
                b'1'..=b'9' => {
                    occurrence = Some(self.parse_number()?);
                }
                b';' | b'\n' => break,
                b' ' | b'\t' | b'\r' => {
                    self.pos += 1;
                }
                _ => return Err(String::from("unsupported substitute flag")),
            }
        }
        Ok(SubstituteCommand {
            pattern,
            replacement,
            global,
            print,
            occurrence,
        })
    }

    fn parse_optional_number(&mut self) -> Result<Option<i32>, String> {
        self.skip_spaces();
        if !matches!(self.peek_byte(), Some(b'0'..=b'9')) {
            return Ok(None);
        }
        let value = self.parse_number()?;
        let converted = i32::try_from(value).map_err(|_| String::from("quit code too large"))?;
        Ok(Some(converted))
    }

    fn parse_text_argument(&mut self) -> String {
        self.skip_spaces();
        if self.peek_byte() == Some(b'\\') {
            self.pos += 1;
            if self.peek_byte() == Some(b'\n') {
                self.pos += 1;
            }
        }
        self.consume_rest_of_line()
    }

    fn parse_number(&mut self) -> Result<usize, String> {
        let start = self.pos;
        while matches!(self.peek_byte(), Some(b'0'..=b'9')) {
            self.pos += 1;
        }
        self.script[start..self.pos]
            .parse::<usize>()
            .map_err(|_| String::from("invalid number"))
    }

    fn parse_delimited(&mut self, delimiter: u8) -> Result<String, String> {
        let mut result = String::new();
        let mut escaped = false;
        while let Some(byte) = self.next_byte() {
            if escaped {
                if byte != delimiter {
                    result.push('\\');
                }
                result.push(byte as char);
                escaped = false;
                continue;
            }
            if byte == b'\\' {
                escaped = true;
                continue;
            }
            if byte == delimiter {
                return Ok(result);
            }
            if byte == b'\n' {
                return Err(String::from("unterminated command"));
            }
            result.push(byte as char);
        }
        Err(String::from("unterminated command"))
    }

    fn consume_rest_of_line(&mut self) -> String {
        let start = self.pos;
        while !matches!(self.peek_byte(), None | Some(b'\n')) {
            self.pos += 1;
        }
        self.script[start..self.pos].to_owned()
    }

    fn skip_spaces(&mut self) {
        while matches!(self.peek_byte(), Some(b' ' | b'\t' | b'\r')) {
            self.pos += 1;
        }
    }

    fn peek_byte(&self) -> Option<u8> {
        self.script.as_bytes().get(self.pos).copied()
    }

    fn next_byte(&mut self) -> Option<u8> {
        let byte = self.peek_byte()?;
        self.pos += 1;
        Some(byte)
    }
}

fn load_inputs(files: &[String], in_place: bool) -> Result<Vec<InputFile>, String> {
    if files.is_empty() {
        if in_place {
            return Err(String::from("no input files for -i"));
        }
        return Ok(vec![InputFile {
            path: None,
            lines: split_lines(&read_to_string("-")?),
        }]);
    }

    files.iter().map(|path| load_input_file(path)).collect()
}

fn load_input_file(path: &str) -> Result<InputFile, String> {
    Ok(InputFile {
        path: Some(path.to_owned()),
        lines: split_lines(&read_to_string(path)?),
    })
}

fn read_to_string(path: &str) -> Result<String, String> {
    let mut reader = open_input(path).map_err(|err| format!("reading {path}: {err}"))?;
    let mut content = String::new();
    reader
        .read_to_string(&mut content)
        .map_err(|err| format!("reading {path}: {err}"))?;
    Ok(content)
}

fn split_lines(content: &str) -> Vec<Line> {
    content
        .split_inclusive('\n')
        .map(|segment| {
            if let Some(stripped) = segment.strip_suffix('\n') {
                Line {
                    text: stripped.to_owned(),
                    had_newline: true,
                }
            } else {
                Line {
                    text: segment.to_owned(),
                    had_newline: false,
                }
            }
        })
        .collect()
}

fn execute(program: Program, inputs: Vec<InputFile>, options: Options) -> Result<i32, String> {
    let total_lines = inputs.iter().map(|input| input.lines.len()).sum::<usize>();
    let mut runtime = vec![RuntimeState::default(); program.commands.len()];
    let mut outputs = vec![String::new(); inputs.len()];
    let mut line_number = 0usize;
    let mut quit_code = None;

    'files: for (file_index, input) in inputs.iter().enumerate() {
        for line in &input.lines {
            line_number += 1;
            let mut pattern = PatternSpace {
                text: line.text.clone(),
                had_newline: line.had_newline,
            };
            let mut line_output = LineOutput::default();
            let is_last = line_number == total_lines;

            for (command_index, command) in program.commands.iter().enumerate() {
                let decision = select_address(
                    command,
                    &mut runtime[command_index],
                    line_number,
                    is_last,
                    &pattern.text,
                )?;
                if !decision.selected {
                    continue;
                }

                apply_command(command, decision, line_number, &mut pattern, &mut line_output)?;
                if line_output.stop_commands {
                    break;
                }
            }

            for text in &line_output.before {
                write_text(&mut outputs[file_index], text);
            }
            for text in &line_output.print_now {
                outputs[file_index].push_str(text);
            }
            if !line_output.suppress_default_print && !options.quiet {
                write_pattern(&mut outputs[file_index], &pattern);
            }
            for text in &line_output.after {
                write_text(&mut outputs[file_index], text);
            }

            if let Some(code) = line_output.quit_code {
                quit_code = Some(code);
                break 'files;
            }
        }
    }

    if let Some(suffix) = options.in_place {
        for (input, output) in inputs.iter().zip(outputs.iter()) {
            let path = input
                .path
                .as_deref()
                .ok_or_else(|| String::from("cannot use -i on standard input"))?;
            write_in_place(path, output, &suffix)?;
        }
        Ok(quit_code.unwrap_or(0))
    } else {
        let mut handle = stdout();
        for output in &outputs {
            handle
                .write_all(output.as_bytes())
                .map_err(|err| format!("writing stdout: {err}"))?;
        }
        handle
            .flush()
            .map_err(|err| format!("flushing stdout: {err}"))?;
        Ok(quit_code.unwrap_or(0))
    }
}

fn apply_command(
    command: &Command,
    decision: AddressDecision,
    line_number: usize,
    pattern: &mut PatternSpace,
    output: &mut LineOutput,
) -> Result<(), String> {
    match &command.kind {
        CommandKind::Substitute(substitute) => {
            let replaced = apply_substitute(substitute, pattern)?;
            if replaced && substitute.print {
                output.print_now.push(render_pattern(pattern));
            }
        }
        CommandKind::Print => output.print_now.push(render_pattern(pattern)),
        CommandKind::Delete => {
            output.suppress_default_print = true;
            output.stop_commands = true;
        }
        CommandKind::Quit(code) => {
            output.quit_code = Some(code.unwrap_or(0));
            output.stop_commands = true;
        }
        CommandKind::Append(text) => output.after.push(text.clone()),
        CommandKind::Insert(text) => output.before.push(text.clone()),
        CommandKind::Change(text) => {
            if decision.started_range || command.address2.is_none() {
                output.before.push(text.clone());
            }
            output.suppress_default_print = true;
            output.stop_commands = true;
        }
        CommandKind::Number => output.before.push(line_number.to_string()),
    }
    Ok(())
}

fn select_address(
    command: &Command,
    runtime: &mut RuntimeState,
    line_number: usize,
    is_last: bool,
    pattern: &str,
) -> Result<AddressDecision, String> {
    let mut selected = false;
    let mut started_range = false;

    match (&command.address1, &command.address2) {
        (None, None) => selected = true,
        (Some(address), None) => selected = address.matches(line_number, is_last, pattern)?,
        (Some(first), Some(second)) => {
            if runtime.range_active {
                selected = true;
                if second.matches(line_number, is_last, pattern)? {
                    runtime.range_active = false;
                }
            } else if first.matches(line_number, is_last, pattern)? {
                selected = true;
                started_range = true;
                runtime.range_active = !second.matches(line_number, is_last, pattern)?;
            }
        }
        (None, Some(_)) => return Err(String::from("invalid address range")),
    }

    if command.negate {
        selected = !selected;
    }

    Ok(AddressDecision {
        selected,
        started_range,
    })
}

fn apply_substitute(command: &SubstituteCommand, pattern: &mut PatternSpace) -> Result<bool, String> {
    let mut cursor = 0usize;
    let mut matches_seen = 0usize;
    let mut replaced_any = false;
    let mut output = String::new();

    loop {
        let matched = command.pattern.find(&pattern.text, cursor)?;
        let Some(found) = matched else {
            output.push_str(&pattern.text[cursor..]);
            break;
        };

        matches_seen += 1;
        let should_replace = if let Some(target) = command.occurrence {
            matches_seen == target
        } else if command.global {
            true
        } else {
            !replaced_any
        };

        if !should_replace {
            output.push_str(&pattern.text[cursor..found.full.end]);
            cursor = advance_past_match(&pattern.text, found.full, cursor);
            continue;
        }

        output.push_str(&pattern.text[cursor..found.full.start]);
        output.push_str(&expand_replacement(
            &command.replacement,
            &pattern.text,
            &found,
        ));
        replaced_any = true;
        cursor = advance_past_match(&pattern.text, found.full, found.full.end);

        if command.occurrence.is_some() || (!command.global && command.occurrence.is_none()) {
            output.push_str(&pattern.text[cursor..]);
            break;
        }
    }

    if replaced_any {
        pattern.text = output;
    }
    Ok(replaced_any)
}

fn advance_past_match(text: &str, span: MatchSpan, fallback: usize) -> usize {
    if span.start != span.end {
        span.end
    } else if fallback < text.len() {
        fallback + text[fallback..].chars().next().map(|ch| ch.len_utf8()).unwrap_or(1)
    } else {
        fallback
    }
}

fn expand_replacement(replacement: &str, text: &str, matched: &RegexMatch) -> String {
    let mut result = String::new();
    let mut chars = replacement.chars().peekable();
    while let Some(ch) = chars.next() {
        match ch {
            '&' => result.push_str(&text[matched.full.start..matched.full.end]),
            '\\' => match chars.next() {
                Some(next @ '1'..='9') => {
                    let index = usize::from(next as u8 - b'0');
                    if let Some(span) = matched.groups[index] {
                        result.push_str(&text[span.start..span.end]);
                    }
                }
                Some(next) => result.push(next),
                None => result.push('\\'),
            },
            _ => result.push(ch),
        }
    }
    result
}

fn render_pattern(pattern: &PatternSpace) -> String {
    let mut output = pattern.text.clone();
    if pattern.had_newline {
        output.push('\n');
    }
    output
}

fn write_pattern(output: &mut String, pattern: &PatternSpace) {
    output.push_str(&pattern.text);
    if pattern.had_newline {
        output.push('\n');
    }
}

fn write_text(output: &mut String, text: &str) {
    output.push_str(text);
    output.push('\n');
}

fn write_in_place(path: &str, content: &str, backup_suffix: &str) -> Result<(), String> {
    let path_ref = Path::new(path);
    let metadata = fs::metadata(path_ref).map_err(|err| format!("reading {path}: {err}"))?;
    if !backup_suffix.is_empty() {
        let backup_path = format!("{path}{backup_suffix}");
        fs::copy(path_ref, &backup_path).map_err(|err| format!("writing {backup_path}: {err}"))?;
    }

    let temp_path = temporary_output_path(path_ref);
    fs::write(&temp_path, content).map_err(|err| format!("writing {}: {err}", temp_path.display()))?;
    let mut permissions = fs::metadata(&temp_path)
        .map_err(|err| format!("reading {}: {err}", temp_path.display()))?
        .permissions();
    permissions.set_mode(metadata.permissions().mode());
    fs::set_permissions(&temp_path, permissions)
        .map_err(|err| format!("setting permissions on {}: {err}", temp_path.display()))?;
    fs::rename(&temp_path, path_ref).map_err(|err| format!("replacing {path}: {err}"))?;
    Ok(())
}

fn temporary_output_path(path: &Path) -> PathBuf {
    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    let pid = std::process::id();
    let file_name = path.file_name().and_then(|name| name.to_str()).unwrap_or("sed");
    let mut temp = path.to_path_buf();
    temp.set_file_name(format!(".{file_name}.seed.{pid}.{stamp}"));
    temp
}

impl Address {
    fn matches(&self, line_number: usize, is_last: bool, pattern: &str) -> Result<bool, String> {
        match self {
            Address::Line(expected) => Ok(line_number == *expected),
            Address::Last => Ok(is_last),
            Address::Regex(regex) => Ok(regex.find(pattern, 0)?.is_some()),
        }
    }
}

impl Regex {
    fn compile(pattern: &str, extended: bool) -> Result<Self, String> {
        let c_pattern = CString::new(pattern)
            .map_err(|_| format!("pattern contains NUL byte: {pattern:?}"))?;
        let mut raw = MaybeUninit::<libc::regex_t>::zeroed();
        let mut flags = 0;
        if extended {
            flags |= libc::REG_EXTENDED;
        }
        // SAFETY: `c_pattern` is NUL-terminated and `raw` points to valid storage.
        let rc = unsafe { libc::regcomp(raw.as_mut_ptr(), c_pattern.as_ptr(), flags) };
        if rc != 0 {
            return Err(format!("invalid regular expression: {pattern}"));
        }
        Ok(Self {
            // SAFETY: successful regcomp initialized `raw`.
            raw: unsafe { raw.assume_init() },
        })
    }

    fn find(&self, text: &str, start: usize) -> Result<Option<RegexMatch>, String> {
        let haystack = &text[start..];
        let c_text = CString::new(haystack)
            .map_err(|_| String::from("input contains NUL byte, unsupported by sed"))?;
        let mut matches = [libc::regmatch_t {
            rm_so: -1,
            rm_eo: -1,
        }; MATCH_SLOTS];
        let mut flags = 0;
        if start > 0 {
            flags |= libc::REG_NOTBOL;
        }
        // SAFETY: compiled regex and match buffer are valid, text is NUL-terminated.
        let rc = unsafe {
            libc::regexec(
                &self.raw as *const libc::regex_t,
                c_text.as_ptr(),
                matches.len(),
                matches.as_mut_ptr(),
                flags,
            )
        };
        if rc == libc::REG_NOMATCH {
            return Ok(None);
        }
        if rc != 0 {
            return Err(String::from("regex match failed"));
        }
        if matches[0].rm_so < 0 || matches[0].rm_eo < 0 {
            return Ok(None);
        }

        let mut groups = [None; MATCH_SLOTS];
        for (index, slot) in matches.iter().enumerate() {
            if slot.rm_so >= 0 && slot.rm_eo >= 0 {
                groups[index] = Some(MatchSpan {
                    start: start + slot.rm_so as usize,
                    end: start + slot.rm_eo as usize,
                });
            }
        }
        Ok(Some(RegexMatch {
            full: groups[0].expect("match span"),
            groups,
        }))
    }
}

impl Drop for Regex {
    fn drop(&mut self) {
        // SAFETY: `raw` was initialized by regcomp and may be freed once here.
        unsafe {
            libc::regfree(&mut self.raw as *mut libc::regex_t);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{
        Address, CommandKind, Options, ParsedArgs, Regex, apply_substitute, parse_args, parse_program,
        split_lines,
    };

    fn args(values: &[&str]) -> Vec<String> {
        values.iter().map(|value| value.to_string()).collect()
    }

    #[test]
    fn parse_args_uses_operand_as_script() {
        let parsed = parse_args(&args(&["s/a/b/", "file"])).expect("parse args");
        assert_eq!(
            parsed,
            ParsedArgs {
                options: Options::default(),
                script: String::from("s/a/b/"),
                files: vec![String::from("file")],
            }
        );
    }

    #[test]
    fn parse_program_supports_ranges_and_negation() {
        let program = parse_program("1,/two/!p", false).expect("parse program");
        assert_eq!(program.commands.len(), 1);
        assert!(matches!(program.commands[0].address1, Some(Address::Line(1))));
        assert!(program.commands[0].address2.is_some());
        assert!(program.commands[0].negate);
    }

    #[test]
    fn parse_program_supports_insert_and_change_text() {
        let program = parse_program("1i before\n2c after", false).expect("parse program");
        assert!(matches!(program.commands[0].kind, CommandKind::Insert(_)));
        assert!(matches!(program.commands[1].kind, CommandKind::Change(_)));
    }

    #[test]
    fn substitute_supports_backrefs_and_global() {
        let command = match parse_program(r"s/\([a-z]\)\([0-9]\)/[\2-\1]/g", false)
            .expect("parse")
            .commands
            .remove(0)
            .kind
        {
            CommandKind::Substitute(command) => command,
            _ => panic!("expected substitute"),
        };
        let mut pattern = super::PatternSpace {
            text: String::from("a1 b2"),
            had_newline: true,
        };
        assert!(apply_substitute(&command, &mut pattern).expect("substitute"));
        assert_eq!(pattern.text, "[1-a] [2-b]");
    }

    #[test]
    fn split_lines_preserves_missing_trailing_newline() {
        let lines = split_lines("one\ntwo");
        assert_eq!(lines.len(), 2);
        assert!(lines[0].had_newline);
        assert!(!lines[1].had_newline);
    }

    #[test]
    fn regex_matches_basic_and_extended() {
        let basic = Regex::compile(r"\([a-z]\)", false).expect("basic regex");
        assert!(basic.find("abc", 0).expect("find").is_some());
        let extended = Regex::compile(r"([a-z]+)", true).expect("extended regex");
        assert!(extended.find("abc", 0).expect("find").is_some());
    }
}
