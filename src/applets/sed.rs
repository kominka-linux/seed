use std::ffi::CString;
use std::fs;
use std::collections::HashMap;
use std::io::{Read, Write};
use std::mem::MaybeUninit;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use crate::common::args::{ParsedArg, Parser};
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
    labels: HashMap<String, usize>,
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
    PrintFirst,
    Delete,
    DeleteFirst,
    Quit(Option<i32>),
    Append(String),
    Insert(String),
    Change(String),
    Number,
    Next,
    AppendNext,
    Label(String),
    Branch(Option<String>),
    Test(Option<String>),
    TestNot(Option<String>),
    Hold,
    HoldAppend,
    Get,
    GetAppend,
    Exchange,
    Transliterate(TransliterateCommand),
    ReadFile(String),
    WriteFile(String),
    Block(Vec<Command>),
}

#[derive(Debug)]
struct SubstituteCommand {
    pattern: Regex,
    replacement: String,
    global: bool,
    print: bool,
    occurrence: Option<usize>,
    write_path: Option<String>,
}

#[derive(Debug)]
struct TransliterateCommand {
    source: Vec<char>,
    dest: Vec<char>,
}

#[derive(Debug)]
enum Address {
    Line(usize),
    Relative(usize),
    Last,
    Regex(Regex),
}

#[derive(Debug)]
struct Regex {
    raw: libc::regex_t,
    literal: Option<LiteralRegex>,
}

#[derive(Debug)]
struct LiteralRegex {
    bytes: Vec<u8>,
    ignore_case: bool,
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
    range_remaining: Option<usize>,
    numeric_range_started: bool,
}

#[derive(Clone, Debug)]
struct PatternSpace {
    text: String,
    had_newline: bool,
}

impl Default for PatternSpace {
    fn default() -> Self {
        Self {
            text: String::new(),
            had_newline: true,
        }
    }
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
    after_raw: Vec<String>,
    print_now: Vec<String>,
    suppress_default_print: bool,
}

#[derive(Clone, Copy, Debug)]
struct AddressDecision {
    selected: bool,
    started_range: bool,
}

pub fn main(args: &[std::ffi::OsString]) -> i32 {
    match run(args) {
        Ok(code) => code,
        Err(message) => {
            eprintln!("{APPLET}: {message}");
            1
        }
    }
}

fn run(args: &[std::ffi::OsString]) -> Result<i32, String> {
    let parsed = parse_args(args)?;
    let program = parse_program(&parsed.script, parsed.options.extended)?;
    let inputs = load_inputs(&parsed.files, parsed.options.in_place.is_some())?;
    execute(program, inputs, parsed.options)
}

fn parse_args(args: &[std::ffi::OsString]) -> Result<ParsedArgs, String> {
    let mut options = Options::default();
    let mut expressions = Vec::new();
    let mut script_files = Vec::new();
    let mut operands = Vec::new();
    let mut parser = Parser::new(APPLET, args);

    while let Some(arg) = parser.next_arg().map_err(|err| err[0].to_string())? {
        match arg {
            ParsedArg::Short('n') => options.quiet = true,
            ParsedArg::Short('E') | ParsedArg::Short('r') => options.extended = true,
            ParsedArg::Short('e') => expressions.push(
                parser
                    .value_str("e")
                    .map_err(|err| err[0].to_string())?,
            ),
            ParsedArg::Short('f') => script_files.push(
                parser
                    .value_str("f")
                    .map_err(|err| err[0].to_string())?,
            ),
            ParsedArg::Short('i') => {
                options.in_place = Some(
                    parser
                        .optional_value_str()
                        .map_err(|err| err[0].to_string())?
                        .unwrap_or_default(),
                );
            }
            ParsedArg::Short(flag) => return Err(format!("invalid option -- '{flag}'")),
            ParsedArg::Long(name) => match name.as_str() {
                "quiet" | "silent" => options.quiet = true,
                "regexp-extended" => options.extended = true,
                "expression" => expressions.push(
                    parser
                        .value_str("expression")
                        .map_err(|err| err[0].to_string())?,
                ),
                "file" => {
                    script_files.push(
                        parser
                            .value_str("file")
                            .map_err(|err| err[0].to_string())?,
                    )
                }
                "in-place" => {
                    options.in_place = Some(
                        parser
                            .optional_value_str()
                            .map_err(|err| err[0].to_string())?
                            .unwrap_or_default(),
                    );
                }
                _ => return Err(format!("unrecognized option '--{name}'")),
            },
            ParsedArg::Value(arg) => operands.push(
                arg.into_string()
                    .map_err(|arg| format!("argument is invalid unicode: {:?}", arg))?,
            ),
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

    let mut labels = HashMap::new();
    for (index, command) in commands.iter().enumerate() {
        if let CommandKind::Label(label) = &command.kind
            && labels.insert(label.clone(), index).is_some()
        {
            return Err(format!("duplicate label: {label}"));
        }
    }

    validate_branch_targets(&commands, &labels)?;

    Ok(Program { commands, labels })
}

fn validate_branch_targets(commands: &[Command], labels: &HashMap<String, usize>) -> Result<(), String> {
    for command in commands {
        validate_branch_target_command(command, labels)?;
    }
    Ok(())
}

fn validate_branch_target_command(command: &Command, labels: &HashMap<String, usize>) -> Result<(), String> {
    match &command.kind {
        CommandKind::Branch(Some(label)) | CommandKind::Test(Some(label)) | CommandKind::TestNot(Some(label)) => {
            if !labels.contains_key(label) {
                return Err(format!("undefined label: {label}"));
            }
        }
        CommandKind::Block(commands) => validate_branch_targets(commands, labels)?,
        _ => {}
    }
    Ok(())
}

struct ScriptParser<'a> {
    script: &'a str,
    pos: usize,
    extended: bool,
    last_regex_text: Option<String>,
}

impl<'a> ScriptParser<'a> {
    fn new(script: &'a str, extended: bool) -> Self {
        Self {
            script,
            pos: 0,
            extended,
            last_regex_text: None,
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
            b'P' => CommandKind::PrintFirst,
            b'd' => CommandKind::Delete,
            b'D' => CommandKind::DeleteFirst,
            b'q' => CommandKind::Quit(self.parse_optional_number()?),
            b'a' => CommandKind::Append(self.parse_text_argument()),
            b'i' => CommandKind::Insert(self.parse_text_argument()),
            b'c' => CommandKind::Change(self.parse_text_argument()),
            b'=' => CommandKind::Number,
            b'n' => CommandKind::Next,
            b'N' => CommandKind::AppendNext,
            b':' => CommandKind::Label(self.parse_label_argument()),
            b'b' => CommandKind::Branch(self.parse_branch_target()),
            b't' => CommandKind::Test(self.parse_branch_target()),
            b'T' => CommandKind::TestNot(self.parse_branch_target()),
            b'h' => CommandKind::Hold,
            b'H' => CommandKind::HoldAppend,
            b'g' => CommandKind::Get,
            b'G' => CommandKind::GetAppend,
            b'x' => CommandKind::Exchange,
            b'y' => CommandKind::Transliterate(self.parse_transliterate()?),
            b'r' => CommandKind::ReadFile(self.parse_filename_argument()?),
            b'w' => CommandKind::WriteFile(self.parse_filename_argument()?),
            b'{' => CommandKind::Block(self.parse_block()?),
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
            Some(b'+') => {
                self.pos += 1;
                Ok(Some(Address::Relative(self.parse_number()?)))
            }
            Some(b'$') => {
                self.pos += 1;
                Ok(Some(Address::Last))
            }
            Some(b'/') => {
                let pattern = self.parse_address_regex(b'/')?;
                Ok(Some(Address::Regex(pattern)))
            }
            Some(byte) if byte.is_ascii_digit() => Ok(Some(Address::Line(self.parse_number()?))),
            _ => Ok(None),
        }
    }

    fn parse_address_regex(&mut self, delimiter: u8) -> Result<Regex, String> {
        if self.next_byte() != Some(delimiter) {
            return Err(String::from("missing regex delimiter"));
        }
        let raw = self.parse_regex_source(delimiter)?;
        Regex::compile(&raw, self.extended, false)
    }

    fn parse_substitute(&mut self) -> Result<SubstituteCommand, String> {
        let delimiter = self
            .next_byte()
            .ok_or_else(|| String::from("missing substitute delimiter"))?;
        let pattern_text = self.parse_regex_source(delimiter)?;
        let replacement = self.parse_replacement(delimiter)?;
        let mut global = false;
        let mut print = false;
        let mut occurrence = None;
        let mut ignore_case = false;
        let mut write_path = None;
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
                b'I' => {
                    ignore_case = true;
                    self.pos += 1;
                }
                b'w' => {
                    self.pos += 1;
                    write_path = Some(self.parse_filename_argument()?);
                    break;
                }
                b'1'..=b'9' => {
                    occurrence = Some(self.parse_number()?);
                }
                b';' | b'\n' | b'}' => break,
                b' ' | b'\t' | b'\r' => {
                    self.pos += 1;
                }
                _ => return Err(String::from("unsupported substitute flag")),
            }
        }
        let pattern = Regex::compile(&pattern_text, self.extended, ignore_case)?;
        Ok(SubstituteCommand {
            pattern,
            replacement,
            global,
            print,
            occurrence,
            write_path,
        })
    }

    fn parse_regex_source(&mut self, delimiter: u8) -> Result<String, String> {
        let raw = self.parse_regex_delimited(delimiter)?;
        if raw.is_empty() {
            self.last_regex_text
                .clone()
                .ok_or_else(|| String::from("no previous regular expression"))
        } else {
            self.last_regex_text = Some(raw.clone());
            Ok(raw)
        }
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
        if self.peek_byte() == Some(b'\\')
            && self.script.as_bytes().get(self.pos + 1) == Some(&b'\n')
        {
            self.pos += 2;
        }
        decode_text_argument(&self.consume_rest_of_line())
    }

    fn parse_label_argument(&mut self) -> String {
        self.consume_to_separator().trim().to_owned()
    }

    fn parse_branch_target(&mut self) -> Option<String> {
        let label = self.consume_to_separator();
        let trimmed = label.trim();
        (!trimmed.is_empty()).then(|| trimmed.to_owned())
    }

    fn parse_transliterate(&mut self) -> Result<TransliterateCommand, String> {
        let delimiter = self
            .next_byte()
            .ok_or_else(|| String::from("missing transliterate delimiter"))?;
        let source = self.parse_delimited(delimiter)?;
        let dest = self.parse_delimited(delimiter)?;
        let source_chars = source.chars().collect::<Vec<_>>();
        let dest_chars = dest.chars().collect::<Vec<_>>();
        if source_chars.len() != dest_chars.len() {
            return Err(String::from("strings for y command are different lengths"));
        }
        Ok(TransliterateCommand {
            source: source_chars,
            dest: dest_chars,
        })
    }

    fn parse_filename_argument(&mut self) -> Result<String, String> {
        let value = self.consume_to_separator();
        let trimmed = value.trim_start();
        if trimmed.is_empty() {
            return Err(String::from("empty filename"));
        }
        Ok(trimmed.to_owned())
    }

    fn parse_block(&mut self) -> Result<Vec<Command>, String> {
        let mut commands = Vec::new();
        loop {
            while matches!(self.peek_byte(), Some(b';' | b'\n' | b' ' | b'\t' | b'\r')) {
                self.pos += 1;
            }
            if self.peek_byte() == Some(b'}') {
                self.pos += 1;
                return Ok(commands);
            }
            if self.pos >= self.script.len() {
                return Err(String::from("unterminated { block"));
            }
            commands.push(self.parse_command()?);
        }
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

    fn parse_regex_delimited(&mut self, delimiter: u8) -> Result<String, String> {
        let mut result = String::new();
        let mut escaped = false;
        let mut in_class = false;
        let mut class_at_start = false;
        while let Some(byte) = self.next_byte() {
            if escaped {
                if byte != delimiter {
                    result.push('\\');
                }
                result.push(byte as char);
                escaped = false;
                if in_class {
                    class_at_start = false;
                }
                continue;
            }
            if byte == b'\\' {
                escaped = true;
                continue;
            }
            if in_class {
                result.push(byte as char);
                if byte == b']' && !class_at_start {
                    in_class = false;
                } else {
                    class_at_start = false;
                }
                continue;
            }
            if byte == b'[' {
                in_class = true;
                class_at_start = true;
                result.push('[');
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

    fn parse_replacement(&mut self, delimiter: u8) -> Result<String, String> {
        let mut result = String::new();
        let mut escaped = false;
        while let Some(byte) = self.next_byte() {
            if escaped {
                match byte {
                    b'n' => result.push('\n'),
                    b't' => result.push('\t'),
                    b'r' => result.push('\r'),
                    _ if byte == delimiter => {
                        if byte == b'&' {
                            result.push('\\');
                        }
                        result.push(byte as char);
                    }
                    _ => {
                        result.push('\\');
                        result.push(byte as char);
                    }
                }
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

    fn consume_to_separator(&mut self) -> String {
        self.skip_spaces();
        let start = self.pos;
        while !matches!(self.peek_byte(), None | Some(b'\n' | b';' | b'}')) {
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
    let mut runtime = vec![RuntimeState::default(); program.commands.len()];
    let mut outputs = vec![String::new(); inputs.len()];
    let mut line_number = 0usize;
    let mut quit_code = None;
    let mut hold = PatternSpace::default();
    let global_total_lines = inputs.iter().map(|input| input.lines.len()).sum::<usize>();

    'files: for (file_index, input) in inputs.iter().enumerate() {
        if options.in_place.is_some() {
            runtime.fill(RuntimeState::default());
            hold = PatternSpace::default();
            line_number = 0;
        }
        let total_lines = if options.in_place.is_some() {
            input.lines.len()
        } else {
            global_total_lines
        };
        let mut index = 0usize;
        while index < input.lines.len() {
            let line = &input.lines[index];
            line_number += 1;
            let mut current_line_number = line_number;
            let mut pattern = PatternSpace {
                text: line.text.clone(),
                had_newline: line.had_newline,
            };
            let mut line_output = LineOutput::default();
            let mut last_substitution = false;
            let mut command_index = 0usize;
            let mut cycle_flushed = false;

            while command_index < program.commands.len() {
                let command = &program.commands[command_index];
                let decision = select_address(
                    command,
                    &mut runtime[command_index],
                    current_line_number,
                    current_line_number == total_lines,
                    &pattern.text,
                )?;
                if !decision.selected {
                    command_index += 1;
                    continue;
                }

                let mut context = CommandContext {
                    program: &program,
                    pattern: &mut pattern,
                    hold: &mut hold,
                    output: &mut line_output,
                    last_substitution: &mut last_substitution,
                    input,
                    input_index: &mut index,
                    line_number: &mut line_number,
                };
                match apply_command(
                    command,
                    decision,
                    *context.line_number,
                    &mut context,
                )? {
                    CommandFlow::Continue => command_index += 1,
                    CommandFlow::Jump(target) => command_index = target,
                    CommandFlow::NextLine => {
                        flush_cycle_output(
                            &mut outputs[file_index],
                            &line_output,
                            &pattern,
                            options.quiet,
                        );
                        if index + 1 >= input.lines.len() {
                            cycle_flushed = true;
                            index = input.lines.len();
                            break;
                        }
                        index += 1;
                        line_number += 1;
                        current_line_number = line_number;
                        let next = &input.lines[index];
                        pattern = PatternSpace {
                            text: next.text.clone(),
                            had_newline: next.had_newline,
                        };
                        line_output = LineOutput::default();
                        last_substitution = false;
                        command_index += 1;
                    }
                    CommandFlow::AppendNextLine => {
                        if index + 1 >= input.lines.len() {
                            cycle_flushed = true;
                            flush_cycle_output(
                                &mut outputs[file_index],
                                &line_output,
                                &pattern,
                                options.quiet,
                            );
                            index = input.lines.len();
                            break;
                        }
                        index += 1;
                        line_number += 1;
                        current_line_number = line_number;
                        let next = &input.lines[index];
                        pattern.text.push('\n');
                        pattern.text.push_str(&next.text);
                        pattern.had_newline = next.had_newline;
                        command_index += 1;
                    }
                    CommandFlow::RestartCycle => {
                        last_substitution = false;
                        command_index = 0;
                    }
                    CommandFlow::EndCycle => break,
                    CommandFlow::Quit(code) => {
                        quit_code = Some(code);
                        break;
                    }
                }
            }

            if !cycle_flushed {
                flush_cycle_output(&mut outputs[file_index], &line_output, &pattern, options.quiet);
            }
            if quit_code.is_some() {
                break 'files;
            }
            index += 1;
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
        let mut combined = String::new();
        for output in &outputs {
            write_chunk(&mut combined, output);
        }
        let mut handle = stdout();
        handle
            .write_all(combined.as_bytes())
            .map_err(|err| format!("writing stdout: {err}"))?;
        handle
            .flush()
            .map_err(|err| format!("flushing stdout: {err}"))?;
        Ok(quit_code.unwrap_or(0))
    }
}

enum CommandFlow {
    Continue,
    Jump(usize),
    NextLine,
    AppendNextLine,
    RestartCycle,
    EndCycle,
    Quit(i32),
}

struct CommandContext<'a> {
    program: &'a Program,
    pattern: &'a mut PatternSpace,
    hold: &'a mut PatternSpace,
    output: &'a mut LineOutput,
    last_substitution: &'a mut bool,
    input: &'a InputFile,
    input_index: &'a mut usize,
    line_number: &'a mut usize,
}

fn apply_command(
    command: &Command,
    decision: AddressDecision,
    line_number: usize,
    context: &mut CommandContext<'_>,
) -> Result<CommandFlow, String> {
    match &command.kind {
        CommandKind::Substitute(substitute) => {
            let replaced = apply_substitute(substitute, context.pattern)?;
            if replaced && substitute.print {
                context.output.print_now.push(render_pattern(context.pattern));
            }
            *context.last_substitution |= replaced;
            if replaced && let Some(path) = &substitute.write_path {
                write_pattern_to_file(path, context.pattern)?;
            }
        }
        CommandKind::Print => context.output.print_now.push(render_pattern(context.pattern)),
        CommandKind::PrintFirst => context
            .output
            .print_now
            .push(render_first_pattern_line(context.pattern)),
        CommandKind::Delete => {
            context.output.suppress_default_print = true;
            return Ok(CommandFlow::EndCycle);
        }
        CommandKind::DeleteFirst => {
            if let Some((_, rest)) = context.pattern.text.split_once('\n') {
                context.pattern.text = rest.to_owned();
                context.output.suppress_default_print = true;
                return Ok(CommandFlow::RestartCycle);
            }
            context.output.suppress_default_print = true;
            return Ok(CommandFlow::EndCycle);
        }
        CommandKind::Quit(code) => return Ok(CommandFlow::Quit(code.unwrap_or(0))),
        CommandKind::Append(text) => context.output.after.push(text.clone()),
        CommandKind::Insert(text) => context.output.before.push(text.clone()),
        CommandKind::Change(text) => {
            if decision.started_range || command.address2.is_none() {
                context.output.before.push(text.clone());
            }
            context.output.suppress_default_print = true;
            return Ok(CommandFlow::EndCycle);
        }
        CommandKind::Number => context.output.before.push(line_number.to_string()),
        CommandKind::Next => return Ok(CommandFlow::NextLine),
        CommandKind::AppendNext => return Ok(CommandFlow::AppendNextLine),
        CommandKind::Label(_) => {}
        CommandKind::Branch(label) => {
            return Ok(CommandFlow::Jump(branch_target(context.program, label)?));
        }
        CommandKind::Test(label) => {
            let changed = *context.last_substitution;
            *context.last_substitution = false;
            if changed {
                return Ok(CommandFlow::Jump(branch_target(context.program, label)?));
            }
        }
        CommandKind::TestNot(label) => {
            let changed = *context.last_substitution;
            *context.last_substitution = false;
            if !changed {
                return Ok(CommandFlow::Jump(branch_target(context.program, label)?));
            }
        }
        CommandKind::Hold => *context.hold = context.pattern.clone(),
        CommandKind::HoldAppend => {
            if context.hold.text.is_empty() {
                *context.hold = context.pattern.clone();
            } else {
                context.hold.text.push('\n');
                context.hold.text.push_str(&context.pattern.text);
                context.hold.had_newline = context.pattern.had_newline;
            }
        }
        CommandKind::Get => *context.pattern = context.hold.clone(),
        CommandKind::GetAppend => {
            context.pattern.text.push('\n');
            context.pattern.text.push_str(&context.hold.text);
            context.pattern.had_newline = context.hold.had_newline;
        }
        CommandKind::Exchange => std::mem::swap(context.pattern, context.hold),
        CommandKind::Transliterate(map) => apply_transliterate(map, context.pattern),
        CommandKind::ReadFile(path) => {
            if let Ok(contents) = fs::read_to_string(path) {
                context.output.after_raw.push(contents);
            }
        }
        CommandKind::WriteFile(path) => write_pattern_to_file(path, context.pattern)?,
        CommandKind::Block(commands) => {
            let mut nested_index = 0usize;
            while nested_index < commands.len() {
                let nested = &commands[nested_index];
                let decision = select_nested_address(nested, context.pattern)?;
                if !decision.selected {
                    nested_index += 1;
                    continue;
                }
                match apply_command(nested, decision, line_number, context)? {
                    CommandFlow::Continue => nested_index += 1,
                    CommandFlow::AppendNextLine => {
                        if !append_next_line(context) {
                            return Ok(CommandFlow::EndCycle);
                        }
                        nested_index += 1;
                    }
                    other => return Ok(other),
                }
            }
        }
    }
    Ok(CommandFlow::Continue)
}

fn append_next_line(context: &mut CommandContext<'_>) -> bool {
    if *context.input_index + 1 >= context.input.lines.len() {
        return false;
    }

    *context.input_index += 1;
    *context.line_number += 1;
    let next = &context.input.lines[*context.input_index];
    context.pattern.text.push('\n');
    context.pattern.text.push_str(&next.text);
    context.pattern.had_newline = next.had_newline;
    true
}

fn select_nested_address(
    command: &Command,
    pattern: &PatternSpace,
) -> Result<AddressDecision, String> {
    let selected = match (&command.address1, &command.address2) {
        (None, None) => true,
        (Some(address), None) => address.matches(0, false, &pattern.text)?,
        _ => return Err(String::from("range addresses in nested blocks are unsupported")),
    };
    Ok(AddressDecision {
        selected: if command.negate { !selected } else { selected },
        started_range: false,
    })
}

fn branch_target(program: &Program, label: &Option<String>) -> Result<usize, String> {
    match label {
        Some(label) => program
            .labels
            .get(label)
            .copied()
            .ok_or_else(|| format!("undefined label: {label}")),
        None => Ok(program.commands.len()),
    }
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
                if let Some(remaining) = runtime.range_remaining.as_mut() {
                    if *remaining == 0 {
                        runtime.range_active = false;
                        runtime.range_remaining = None;
                    } else {
                        *remaining -= 1;
                        if *remaining == 0 {
                            runtime.range_active = false;
                            runtime.range_remaining = None;
                        }
                    }
                } else if second.matches(line_number, is_last, pattern)? {
                    runtime.range_active = false;
                }
            } else {
                let numeric_lower_bound = matches!(
                    first,
                    Address::Line(start) if !runtime.numeric_range_started && line_number >= *start
                );
                let first_matches = if numeric_lower_bound {
                    true
                } else {
                    first.matches(line_number, is_last, pattern)?
                };
                if !first_matches {
                    // no-op
                } else {
                    if matches!(first, Address::Line(_)) {
                        runtime.numeric_range_started = true;
                    }
                    selected = true;
                    started_range = !numeric_lower_bound
                        || matches!(first, Address::Line(start) if line_number == *start);
                    match second {
                        Address::Relative(count) => {
                            runtime.range_remaining = Some(*count);
                            runtime.range_active = *count > 0;
                        }
                        Address::Line(end) => {
                            runtime.range_remaining = None;
                            runtime.range_active = line_number < *end;
                        }
                        _ => {
                            runtime.range_remaining = None;
                            runtime.range_active = !second.matches(line_number, is_last, pattern)?;
                        }
                    }
                }
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
    if pattern.text.contains('\0') && command.pattern.literal.is_some() {
        let mut output = String::new();
        let mut replaced_any = false;
        for (index, segment) in pattern.text.split('\0').enumerate() {
            if index > 0 {
                output.push('\0');
            }
            let (segment_output, segment_replaced) = apply_substitute_text(command, segment)?;
            output.push_str(&segment_output);
            replaced_any |= segment_replaced;
        }
        if replaced_any {
            pattern.text = output;
        }
        return Ok(replaced_any);
    }

    let (output, replaced_any) = apply_substitute_text(command, &pattern.text)?;
    if replaced_any {
        pattern.text = output;
    }
    Ok(replaced_any)
}

fn apply_substitute_text(command: &SubstituteCommand, text: &str) -> Result<(String, bool), String> {
    let mut line_offset = 0usize;
    let mut matches_seen = 0usize;
    let mut replaced_any = false;
    let mut prev_match_empty = true;
    let mut tried_at_eol = false;
    let mut output = String::new();

    while let Some(found) = command.pattern.find(text, line_offset)? {
        let start = found.full.start - line_offset;
        let end = found.full.end - line_offset;
        matches_seen += 1;
        let should_replace = if let Some(target) = command.occurrence {
            matches_seen == target
        } else if command.global {
            true
        } else {
            !replaced_any
        };

        if !should_replace {
            output.push_str(&text[line_offset..line_offset + end]);
            line_offset += end;
            if start == end && line_offset < text.len() {
                let next = text[line_offset..]
                    .chars()
                    .next()
                    .expect("char after empty match");
                output.push(next);
                line_offset += next.len_utf8();
            }
        } else {
            output.push_str(&text[line_offset..found.full.start]);
            if prev_match_empty || start != 0 || start != end {
                output.push_str(&expand_replacement(
                    &command.replacement,
                    text,
                    &found,
                ));
                replaced_any = true;
            }

            prev_match_empty = start == end;
            if prev_match_empty {
                if found.full.end == text.len() {
                    tried_at_eol = true;
                    line_offset = text.len();
                } else {
                    let next = text[found.full.end..]
                        .chars()
                        .next()
                        .expect("char after empty match");
                    output.push(next);
                    line_offset = found.full.end + next.len_utf8();
                }
            } else {
                line_offset = found.full.end;
            }

            if command.occurrence.is_some() || (!command.global && command.occurrence.is_none()) {
                break;
            }
        }

        if line_offset == text.len() {
            if tried_at_eol {
                break;
            }
            tried_at_eol = true;
        }
    }

    output.push_str(&text[line_offset..]);
    Ok((output, replaced_any))
}

fn apply_transliterate(command: &TransliterateCommand, pattern: &mut PatternSpace) {
    let mut output = String::with_capacity(pattern.text.len());
    for ch in pattern.text.chars() {
        if let Some(index) = command.source.iter().position(|candidate| *candidate == ch) {
            output.push(command.dest[index]);
        } else {
            output.push(ch);
        }
    }
    pattern.text = output;
}

fn expand_replacement(replacement: &str, text: &str, matched: &RegexMatch) -> String {
    let mut result = String::new();
    let mut chars = replacement.chars().peekable();
    while let Some(ch) = chars.next() {
        match ch {
            '&' => result.push_str(&text[matched.full.start..matched.full.end]),
            '\\' => match chars.next() {
                Some('0') => result.push_str(&text[matched.full.start..matched.full.end]),
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

fn render_first_pattern_line(pattern: &PatternSpace) -> String {
    let head = pattern.text.split('\n').next().unwrap_or("");
    let mut output = String::from(head);
    output.push('\n');
    output
}

fn flush_cycle_output(buffer: &mut String, output: &LineOutput, pattern: &PatternSpace, quiet: bool) {
    for text in &output.before {
        write_text(buffer, text);
    }
    for text in &output.print_now {
        write_chunk(buffer, text);
    }
    if !output.suppress_default_print && !quiet {
        write_pattern(buffer, pattern);
    }
    for text in &output.after {
        write_text(buffer, text);
    }
    for text in &output.after_raw {
        write_chunk(buffer, text);
    }
}

fn write_pattern(output: &mut String, pattern: &PatternSpace) {
    begin_output_chunk(output);
    output.push_str(&pattern.text);
    if pattern.had_newline {
        output.push('\n');
    }
}

fn write_text(output: &mut String, text: &str) {
    begin_output_chunk(output);
    output.push_str(text);
    output.push('\n');
}

fn write_chunk(output: &mut String, chunk: &str) {
    if chunk.is_empty() {
        return;
    }
    begin_output_chunk(output);
    output.push_str(chunk);
}

fn begin_output_chunk(output: &mut String) {
    if !output.is_empty() && !output.ends_with('\n') {
        output.push('\n');
    }
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

fn write_pattern_to_file(path: &str, pattern: &PatternSpace) -> Result<(), String> {
    let mut file = fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
        .map_err(|err| format!("opening {path}: {err}"))?;
    file.write_all(render_pattern(pattern).as_bytes())
        .map_err(|err| format!("writing {path}: {err}"))
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
            Address::Relative(expected) => Ok(line_number == *expected),
            Address::Last => Ok(is_last),
            Address::Regex(regex) => Ok(regex.find(pattern, 0)?.is_some()),
        }
    }
}

impl Regex {
    fn compile(pattern: &str, extended: bool, ignore_case: bool) -> Result<Self, String> {
        let c_pattern = CString::new(pattern)
            .map_err(|_| format!("pattern contains NUL byte: {pattern:?}"))?;
        let mut raw = MaybeUninit::<libc::regex_t>::zeroed();
        let mut flags = 0;
        if extended {
            flags |= libc::REG_EXTENDED;
        }
        if ignore_case {
            flags |= libc::REG_ICASE;
        }
        // SAFETY: `c_pattern` is NUL-terminated and `raw` points to valid storage.
        let rc = unsafe { libc::regcomp(raw.as_mut_ptr(), c_pattern.as_ptr(), flags) };
        if rc != 0 {
            return Err(format!("invalid regular expression: {pattern}"));
        }
        Ok(Self {
            // SAFETY: successful regcomp initialized `raw`.
            raw: unsafe { raw.assume_init() },
            literal: literal_regex(pattern, ignore_case),
        })
    }

    fn find(&self, text: &str, start: usize) -> Result<Option<RegexMatch>, String> {
        if let Some(literal) = &self.literal {
            return Ok(find_literal_match(literal, text, start));
        }

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

fn literal_regex(pattern: &str, ignore_case: bool) -> Option<LiteralRegex> {
    let is_literal = !pattern.chars().any(|ch| {
        matches!(
            ch,
            '.' | '^' | '$' | '*' | '[' | ']' | '\\' | '(' | ')' | '+' | '?' | '{' | '}' | '|'
        )
    });
    is_literal.then(|| LiteralRegex {
        bytes: pattern.as_bytes().to_vec(),
        ignore_case,
    })
}

fn find_literal_match(literal: &LiteralRegex, text: &str, start: usize) -> Option<RegexMatch> {
    let haystack = text.as_bytes();
    let needle = literal.bytes.as_slice();
    let match_start = if needle.is_empty() {
        Some(start.min(haystack.len()))
    } else if literal.ignore_case {
        haystack[start..]
            .windows(needle.len())
            .position(|window| {
                window.len() == needle.len()
                    && window
                        .iter()
                        .zip(needle.iter())
                        .all(|(left, right)| left.eq_ignore_ascii_case(right))
            })
            .map(|offset| start + offset)
    } else {
        haystack[start..]
            .windows(needle.len())
            .position(|window| window == needle)
            .map(|offset| start + offset)
    }?;
    let match_end = match_start + needle.len();
    let mut groups = [None; MATCH_SLOTS];
    groups[0] = Some(MatchSpan {
        start: match_start,
        end: match_end,
    });
    Some(RegexMatch {
        full: groups[0].expect("literal match span"),
        groups,
    })
}

impl Drop for Regex {
    fn drop(&mut self) {
        // SAFETY: `raw` was initialized by regcomp and may be freed once here.
        unsafe {
            libc::regfree(&mut self.raw as *mut libc::regex_t);
        }
    }
}

fn decode_text_argument(text: &str) -> String {
    let mut output = String::new();
    let chars = text.chars().collect::<Vec<_>>();
    let mut index = 0usize;
    while index < chars.len() {
        if chars[index] != '\\' {
            output.push(chars[index]);
            index += 1;
            continue;
        }

        let mut slash_count = 0usize;
        while index + slash_count < chars.len() && chars[index + slash_count] == '\\' {
            slash_count += 1;
        }
        if index + slash_count >= chars.len() {
            output.push_str(&"\\".repeat(slash_count / 2));
            if slash_count % 2 == 1 {
                output.push('\\');
            }
            break;
        }

        let next = chars[index + slash_count];
        match next {
            'n' | 'r' => {
                let escaped = slash_count % 2 == 1;
                let literal_slashes = slash_count / 2;
                output.push_str(&"\\".repeat(literal_slashes));
                if escaped {
                    output.push(match next {
                        'n' => '\n',
                        'r' => '\r',
                        _ => unreachable!(),
                    });
                } else {
                    output.push(next);
                }
            }
            _ => {
                output.push_str(&"\\".repeat(slash_count / 2));
                output.push(next);
            }
        }
        index += slash_count + 1;
    }
    output
}

#[cfg(test)]
mod tests {
    use std::ffi::OsString;
    use std::fs;

    use super::{
        Address, CommandKind, Options, ParsedArgs, Regex, apply_substitute, parse_args, parse_program,
        split_lines,
    };

    fn args(values: &[&str]) -> Vec<OsString> {
        values.iter().map(OsString::from).collect()
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
    fn parse_args_supports_gnu_long_options() {
        let script_path = std::env::temp_dir().join(format!(
            "seed-sed-{}-{}.sed",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("clock")
                .as_nanos()
        ));
        fs::write(&script_path, "s/b/c/\n").expect("write script");
        let parsed = parse_args(&args(&[
            "--quiet",
            "--regexp-extended",
            "--expression=s/a/b/",
            "--file",
            script_path.to_str().expect("utf-8 path"),
            "--in-place=.bak",
            "file",
        ]))
        .expect("parse args");
        let _ = fs::remove_file(&script_path);
        assert!(parsed.options.quiet);
        assert!(parsed.options.extended);
        assert_eq!(parsed.options.in_place.as_deref(), Some(".bak"));
        assert_eq!(parsed.script, "s/a/b/\ns/b/c/\n");
        assert_eq!(parsed.files, vec![String::from("file")]);
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
        let basic = Regex::compile(r"\([a-z]\)", false, false).expect("basic regex");
        assert!(basic.find("abc", 0).expect("find").is_some());
        let extended = Regex::compile(r"([a-z]+)", true, false).expect("extended regex");
        assert!(extended.find("abc", 0).expect("find").is_some());
    }

    #[test]
    fn parse_program_supports_branches_and_hold_space() {
        let program = parse_program(":loop\nh\nt done\nb loop\n:done\nx", false).expect("parse");
        assert!(matches!(program.commands[0].kind, CommandKind::Label(_)));
        assert!(matches!(program.commands[1].kind, CommandKind::Hold));
        assert!(matches!(program.commands[2].kind, CommandKind::Test(_)));
        assert!(matches!(program.commands[3].kind, CommandKind::Branch(_)));
        assert!(matches!(program.commands[5].kind, CommandKind::Exchange));
    }

    #[test]
    fn parse_program_reuses_previous_regex() {
        let program = parse_program(r"/foo/s//bar/", false).expect("parse");
        match &program.commands[0].kind {
            CommandKind::Substitute(command) => {
                let mut pattern = super::PatternSpace {
                    text: String::from("foo"),
                    had_newline: true,
                };
                assert!(apply_substitute(command, &mut pattern).expect("substitute"));
                assert_eq!(pattern.text, "bar");
            }
            _ => panic!("expected substitute"),
        }
    }

    #[test]
    fn parse_program_supports_relative_ranges_and_blocks() {
        let program = parse_program("/a/,+1{p;N}", false).expect("parse");
        assert!(matches!(program.commands[0].address2, Some(Address::Relative(1))));
        assert!(matches!(program.commands[0].kind, CommandKind::Block(_)));
    }

    #[test]
    fn parse_program_rejects_undefined_labels() {
        let error = parse_program("b walrus", false).expect_err("undefined label should fail");
        assert_eq!(error, "undefined label: walrus");
    }
}
