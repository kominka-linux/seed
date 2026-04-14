use std::collections::HashMap;
use std::ffi::CString;
use std::fs;
use std::io::{self, BufRead, BufReader, Write};
use std::mem::MaybeUninit;

use crate::common::applet::{AppletResult, finish};
use crate::common::error::AppletError;

const APPLET: &str = "awk";
const MATCH_SLOTS: usize = 1;

pub fn main(args: &[String]) -> i32 {
    finish(run(args))
}

fn run(args: &[String]) -> AppletResult {
    let options = parse_options(args)?;
    let program = parse_program(&options.program)?;

    let mut env = Environment::new(program.functions.clone());
    for (name, value) in options.assignments {
        env.set_var(&name, Value::String(value));
    }
    if let Some(fs) = options.field_separator {
        env.set_var("FS", Value::String(fs));
    }

    let mut out = io::stdout().lock();
    execute_rules(&program.begin_rules, &mut env, &mut out)?;

    if program.main_rules.is_empty() && program.end_rules.is_empty() {
        return Ok(());
    }

    let paths = if options.files.is_empty() {
        vec![None]
    } else {
        options.files.iter().map(|path| Some(path.as_str())).collect()
    };

    for path in paths {
        env.start_file();
        let reader: Box<dyn BufRead> = match path {
            Some(path) => match fs::File::open(path) {
                Ok(file) => Box::new(BufReader::new(file)),
                Err(err) => {
                    return Err(vec![AppletError::from_io(APPLET, "opening", Some(path), err)]);
                }
            },
            None => Box::new(BufReader::new(io::stdin().lock())),
        };
        process_reader(reader, &program.main_rules, &mut env, &mut out)?;
    }

    execute_rules(&program.end_rules, &mut env, &mut out)?;
    out.flush()
        .map_err(|err| vec![AppletError::new(APPLET, format!("writing stdout: {err}"))])?;
    Ok(())
}

fn process_reader<R: BufRead>(
    mut reader: R,
    rules: &[Rule],
    env: &mut Environment,
    out: &mut dyn Write,
) -> AppletResult {
    let mut line = String::new();
    loop {
        line.clear();
        let read = reader
            .read_line(&mut line)
            .map_err(|err| vec![AppletError::new(APPLET, format!("reading input: {err}"))])?;
        if read == 0 {
            break;
        }
        if line.ends_with('\n') {
            line.pop();
            if line.ends_with('\r') {
                line.pop();
            }
        }
        env.set_record(&line)?;
        execute_rules(rules, env, out)?;
    }
    Ok(())
}

fn execute_rules(rules: &[Rule], env: &mut Environment, out: &mut dyn Write) -> AppletResult {
    for rule in rules {
        execute_block(&rule.action, env, out)?;
    }
    Ok(())
}

fn execute_block(block: &[Stmt], env: &mut Environment, out: &mut dyn Write) -> AppletResult {
    for stmt in block {
        if execute_stmt(stmt, env, out)?.is_some() {
            return Ok(());
        }
    }
    Ok(())
}

fn execute_stmt(
    stmt: &Stmt,
    env: &mut Environment,
    out: &mut dyn Write,
) -> Result<Option<Value>, Vec<AppletError>> {
    match stmt {
        Stmt::Print(exprs) => {
            let mut line = String::new();
            if exprs.is_empty() {
                line.push_str(&env.record_text());
            } else {
                let ofs = env.get_var("OFS").as_string();
                for (index, expr) in exprs.iter().enumerate() {
                    if index > 0 {
                        line.push_str(&ofs);
                    }
                    line.push_str(&eval_expr(expr, env, out)?.as_string());
                }
            }
            line.push_str(&env.get_var("ORS").as_string());
            out.write_all(line.as_bytes())
                .map_err(|err| vec![AppletError::new(APPLET, format!("writing stdout: {err}"))])?;
            Ok(None)
        }
        Stmt::Printf(format, exprs) => {
            let rendered = render_printf(&eval_expr(format, env, out)?.as_string(), exprs, env, out)?;
            out.write_all(rendered.as_bytes())
                .map_err(|err| vec![AppletError::new(APPLET, format!("writing stdout: {err}"))])?;
            Ok(None)
        }
        Stmt::Assign(name, expr) => {
            let value = eval_expr(expr, env, out)?;
            env.set_var(name, value);
            Ok(None)
        }
        Stmt::If(condition, then_branch) => {
            if eval_expr(condition, env, out)?.is_truthy() {
                return execute_stmt(then_branch, env, out);
            }
            Ok(None)
        }
        Stmt::Return(expr) => Ok(Some(if let Some(expr) = expr {
            eval_expr(expr, env, out)?
        } else {
            Value::String(String::new())
        })),
        Stmt::Block(stmts) => {
            for stmt in stmts {
                if let Some(value) = execute_stmt(stmt, env, out)? {
                    return Ok(Some(value));
                }
            }
            Ok(None)
        }
        Stmt::Expr(expr) => {
            let _ = eval_expr(expr, env, out)?;
            Ok(None)
        }
    }
}

fn eval_expr(expr: &Expr, env: &mut Environment, out: &mut dyn Write) -> Result<Value, Vec<AppletError>> {
    match expr {
        Expr::Number(value) => Ok(Value::Number(*value)),
        Expr::String(value) => Ok(Value::String(value.clone())),
        Expr::Var(name) => Ok(env.get_var(name)),
        Expr::Field(index) => {
            let field = eval_expr(index, env, out)?.as_number() as isize;
            Ok(Value::String(env.get_field(field)))
        }
        Expr::UnaryMinus(expr) => Ok(Value::Number(-eval_expr(expr, env, out)?.as_number())),
        Expr::UnaryPlus(expr) => Ok(Value::Number(eval_expr(expr, env, out)?.as_number())),
        Expr::Conditional(condition, yes, no) => {
            if eval_expr(condition, env, out)?.is_truthy() {
                eval_expr(yes, env, out)
            } else {
                eval_expr(no, env, out)
            }
        }
        Expr::Binary(op, left, right) => {
            let left = eval_expr(left, env, out)?;
            let right = eval_expr(right, env, out)?;
            match op {
                BinaryOp::Add => Ok(Value::Number(left.as_number() + right.as_number())),
                BinaryOp::Sub => Ok(Value::Number(left.as_number() - right.as_number())),
                BinaryOp::Mul => Ok(Value::Number(left.as_number() * right.as_number())),
                BinaryOp::Div => Ok(Value::Number(left.as_number() / right.as_number())),
                BinaryOp::Mod => Ok(Value::Number(left.as_number() % right.as_number())),
                BinaryOp::Eq => Ok(Value::Number(if compare_values(&left, &right) == 0 {
                    1.0
                } else {
                    0.0
                })),
                BinaryOp::Ne => Ok(Value::Number(if compare_values(&left, &right) != 0 {
                    1.0
                } else {
                    0.0
                })),
                BinaryOp::Lt => Ok(Value::Number(if compare_values(&left, &right) < 0 {
                    1.0
                } else {
                    0.0
                })),
                BinaryOp::Le => Ok(Value::Number(if compare_values(&left, &right) <= 0 {
                    1.0
                } else {
                    0.0
                })),
                BinaryOp::Gt => Ok(Value::Number(if compare_values(&left, &right) > 0 {
                    1.0
                } else {
                    0.0
                })),
                BinaryOp::Ge => Ok(Value::Number(if compare_values(&left, &right) >= 0 {
                    1.0
                } else {
                    0.0
                })),
                BinaryOp::Concat => {
                    let mut text = left.as_string();
                    text.push_str(&right.as_string());
                    Ok(Value::String(text))
                }
            }
        }
        Expr::Call(name, args) => match name.as_str() {
            "length" => {
                let text = if let Some(arg) = args.first() {
                    eval_expr(arg, env, out)?.as_string()
                } else {
                    env.record_text()
                };
                Ok(Value::Number(text.chars().count() as f64))
            }
            "or" => {
                if args.len() != 2 {
                    return Err(vec![AppletError::new(APPLET, "or() expects 2 arguments")]);
                }
                let left = eval_expr(&args[0], env, out)?.as_number() as u64;
                let right = eval_expr(&args[1], env, out)?.as_number() as u64;
                Ok(Value::Number((left | right) as f64))
            }
            _ => env.call_user_function(name, args, out),
        },
    }
}

fn render_printf(
    format: &str,
    exprs: &[Expr],
    env: &mut Environment,
    out: &mut dyn Write,
) -> Result<String, Vec<AppletError>> {
    let mut output = String::new();
    let mut chars = format.chars().peekable();
    let mut arg_index = 0usize;
    while let Some(ch) = chars.next() {
        if ch != '%' {
            output.push(ch);
            continue;
        }
        if chars.peek() == Some(&'%') {
            chars.next();
            output.push('%');
            continue;
        }

        let mut precision = None;
        if chars.peek() == Some(&'.') {
            chars.next();
            let mut digits = String::new();
            while let Some(next) = chars.peek() {
                if !next.is_ascii_digit() {
                    break;
                }
                digits.push(*next);
                chars.next();
            }
            precision = Some(
                digits
                    .parse::<usize>()
                    .map_err(|_| vec![AppletError::new(APPLET, "invalid printf precision")])?,
            );
        }

        let spec = chars
            .next()
            .ok_or_else(|| vec![AppletError::new(APPLET, "unterminated printf format")])?;
        let value = if let Some(expr) = exprs.get(arg_index) {
            arg_index += 1;
            eval_expr(expr, env, out)?
        } else {
            Value::String(String::new())
        };
        match spec {
            'f' => output.push_str(&format!("{:.*}", precision.unwrap_or(6), value.as_number())),
            'd' | 'i' => output.push_str(&format!("{:.0}", value.as_number().trunc())),
            's' => output.push_str(&value.as_string()),
            other => {
                return Err(vec![AppletError::new(
                    APPLET,
                    format!("unsupported printf format: %{other}"),
                )]);
            }
        }
    }
    Ok(output)
}

fn compare_values(left: &Value, right: &Value) -> i32 {
    match (left.numeric_string(), right.numeric_string()) {
        (Some(left), Some(right)) => match left.partial_cmp(&right) {
            Some(std::cmp::Ordering::Less) => -1,
            Some(std::cmp::Ordering::Equal) => 0,
            Some(std::cmp::Ordering::Greater) => 1,
            None => 0,
        },
        _ => left.as_string().cmp(&right.as_string()) as i32,
    }
}

#[derive(Clone, Debug)]
enum Value {
    String(String),
    Number(f64),
}

impl Value {
    fn as_string(&self) -> String {
        match self {
            Self::String(value) => value.clone(),
            Self::Number(value) => format_number(*value),
        }
    }

    fn as_number(&self) -> f64 {
        match self {
            Self::String(value) => parse_input_number(value),
            Self::Number(value) => *value,
        }
    }

    fn numeric_string(&self) -> Option<f64> {
        match self {
            Self::Number(value) => Some(*value),
            Self::String(value) => parse_numeric_string(value),
        }
    }

    fn is_truthy(&self) -> bool {
        match self {
            Self::Number(value) => *value != 0.0,
            Self::String(value) => !value.is_empty() && parse_input_number(value) != 0.0,
        }
    }
}

fn format_number(value: f64) -> String {
    if value.is_finite() && value.fract() == 0.0 {
        format!("{value:.0}")
    } else {
        format!("{value}")
    }
}

fn parse_numeric_string(text: &str) -> Option<f64> {
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return None;
    }
    trimmed.parse::<f64>().ok()
}

fn parse_input_number(text: &str) -> f64 {
    parse_numeric_string(text).unwrap_or(0.0)
}

#[derive(Debug)]
struct Environment {
    vars: HashMap<String, Value>,
    functions: HashMap<String, Function>,
    frames: Vec<HashMap<String, Value>>,
    record: String,
    fields: Vec<String>,
    nr: usize,
    fnr: usize,
}

impl Environment {
    fn new(functions: HashMap<String, Function>) -> Self {
        let mut vars = HashMap::new();
        vars.insert("FS".to_owned(), Value::String(" ".to_owned()));
        vars.insert("OFS".to_owned(), Value::String(" ".to_owned()));
        vars.insert("ORS".to_owned(), Value::String("\n".to_owned()));
        Self {
            vars,
            functions,
            frames: Vec::new(),
            record: String::new(),
            fields: Vec::new(),
            nr: 0,
            fnr: 0,
        }
    }

    fn start_file(&mut self) {
        self.fnr = 0;
    }

    fn set_record(&mut self, line: &str) -> Result<(), Vec<AppletError>> {
        self.nr += 1;
        self.fnr += 1;
        self.record = line.to_owned();
        self.fields = split_fields(line, &self.get_var("FS").as_string())?;
        Ok(())
    }

    fn get_var(&self, name: &str) -> Value {
        for frame in self.frames.iter().rev() {
            if let Some(value) = frame.get(name) {
                return value.clone();
            }
        }
        match name {
            "NR" => Value::Number(self.nr as f64),
            "FNR" => Value::Number(self.fnr as f64),
            "NF" => Value::Number(self.fields.len() as f64),
            _ => self
                .vars
                .get(name)
                .cloned()
                .unwrap_or_else(|| Value::String(String::new())),
        }
    }

    fn set_var(&mut self, name: &str, value: Value) {
        for frame in self.frames.iter_mut().rev() {
            if frame.contains_key(name) {
                frame.insert(name.to_owned(), value);
                return;
            }
        }
        self.vars.insert(name.to_owned(), value);
    }

    fn record_text(&self) -> String {
        self.record.clone()
    }

    fn get_field(&self, index: isize) -> String {
        match index {
            0 => self.record.clone(),
            index if index < 0 => String::new(),
            index => self
                .fields
                .get(index as usize - 1)
                .cloned()
                .unwrap_or_default(),
        }
    }

    fn call_user_function(
        &mut self,
        name: &str,
        args: &[Expr],
        out: &mut dyn Write,
    ) -> Result<Value, Vec<AppletError>> {
        let function = self
            .functions
            .get(name)
            .cloned()
            .ok_or_else(|| vec![AppletError::new(APPLET, format!("undefined function: {name}"))])?;
        let mut frame = HashMap::new();
        for (index, param) in function.params.iter().enumerate() {
            let value = if let Some(arg) = args.get(index) {
                eval_expr(arg, self, out)?
            } else {
                Value::String(String::new())
            };
            frame.insert(param.clone(), value);
        }
        self.frames.push(frame);
        let mut return_value = Value::String(String::new());
        for stmt in &function.body {
            if let Some(value) = execute_stmt(stmt, self, out)? {
                return_value = value;
                break;
            }
        }
        self.frames.pop();
        Ok(return_value)
    }
}

fn split_fields(record: &str, field_separator: &str) -> Result<Vec<String>, Vec<AppletError>> {
    if record.is_empty() {
        return Ok(Vec::new());
    }
    if field_separator == " " {
        return Ok(record
            .split_whitespace()
            .map(str::to_owned)
            .collect::<Vec<_>>());
    }

    let regex = Regex::compile(field_separator)
        .map_err(|message| vec![AppletError::new(APPLET, message)])?;
    let mut fields = Vec::new();
    let mut start = 0usize;
    while let Some((match_start, match_end)) = regex.find(record, start)? {
        fields.push(record[start..match_start].to_owned());
        start = match_end;
    }
    fields.push(record[start..].to_owned());
    Ok(fields)
}

#[derive(Debug)]
struct Regex {
    raw: libc::regex_t,
}

impl Regex {
    fn compile(pattern: &str) -> Result<Self, String> {
        let c_pattern = CString::new(pattern)
            .map_err(|_| format!("field separator contains NUL byte: {pattern:?}"))?;
        let mut raw = MaybeUninit::<libc::regex_t>::zeroed();
        let rc = unsafe { libc::regcomp(raw.as_mut_ptr(), c_pattern.as_ptr(), libc::REG_EXTENDED) };
        if rc != 0 {
            return Err(format!("invalid regular expression: {pattern}"));
        }
        Ok(Self {
            raw: unsafe { raw.assume_init() },
        })
    }

    fn find(&self, text: &str, start: usize) -> Result<Option<(usize, usize)>, Vec<AppletError>> {
        let haystack = &text[start..];
        let c_text = CString::new(haystack)
            .map_err(|_| vec![AppletError::new(APPLET, "input contains NUL byte, unsupported by awk")])?;
        let mut matches = [libc::regmatch_t {
            rm_so: -1,
            rm_eo: -1,
        }; MATCH_SLOTS];
        let rc = unsafe {
            libc::regexec(
                &self.raw as *const libc::regex_t,
                c_text.as_ptr(),
                matches.len(),
                matches.as_mut_ptr(),
                if start > 0 { libc::REG_NOTBOL } else { 0 },
            )
        };
        if rc == libc::REG_NOMATCH {
            return Ok(None);
        }
        if rc != 0 || matches[0].rm_so < 0 || matches[0].rm_eo < 0 {
            return Err(vec![AppletError::new(APPLET, "regex match failed")]);
        }
        Ok(Some((
            start + matches[0].rm_so as usize,
            start + matches[0].rm_eo as usize,
        )))
    }
}

impl Drop for Regex {
    fn drop(&mut self) {
        unsafe {
            libc::regfree(&mut self.raw as *mut libc::regex_t);
        }
    }
}

#[derive(Debug)]
struct Options {
    field_separator: Option<String>,
    assignments: Vec<(String, String)>,
    program: String,
    files: Vec<String>,
}

fn parse_options(args: &[String]) -> Result<Options, Vec<AppletError>> {
    let mut field_separator = None;
    let mut assignments = Vec::new();
    let mut program_parts = Vec::new();
    let mut files = Vec::new();
    let mut saw_program = false;
    let mut index = 0usize;

    while index < args.len() {
        let arg = &args[index];
        if !saw_program && arg == "--" {
            index += 1;
            break;
        }
        if !saw_program && arg == "-F" {
            index += 1;
            let Some(value) = args.get(index) else {
                return Err(vec![AppletError::option_requires_arg(APPLET, "F")]);
            };
            field_separator = Some(unescape_string(value)?);
            index += 1;
            continue;
        }
        if !saw_program && let Some(value) = arg.strip_prefix("-F") {
            field_separator = Some(unescape_string(value)?);
            index += 1;
            continue;
        }
        if !saw_program && arg == "-v" {
            index += 1;
            let Some(value) = args.get(index) else {
                return Err(vec![AppletError::option_requires_arg(APPLET, "v")]);
            };
            assignments.push(parse_assignment(value)?);
            index += 1;
            continue;
        }
        if !saw_program && arg == "-f" {
            index += 1;
            let Some(path) = args.get(index) else {
                return Err(vec![AppletError::option_requires_arg(APPLET, "f")]);
            };
            let program = fs::read_to_string(path)
                .map_err(|err| vec![AppletError::from_io(APPLET, "reading", Some(path), err)])?;
            program_parts.push(program);
            saw_program = true;
            index += 1;
            continue;
        }
        if !saw_program && arg == "-e" {
            index += 1;
            let Some(program) = args.get(index) else {
                return Err(vec![AppletError::option_requires_arg(APPLET, "e")]);
            };
            program_parts.push(program.clone());
            saw_program = true;
            index += 1;
            continue;
        }
        if !saw_program && arg.starts_with('-') {
            return Err(vec![AppletError::invalid_option(
                APPLET,
                arg.chars().nth(1).unwrap_or('-'),
            )]);
        }
        if !saw_program {
            program_parts.push(arg.clone());
            saw_program = true;
        } else {
            files.push(arg.clone());
        }
        index += 1;
    }

    while index < args.len() {
        files.push(args[index].clone());
        index += 1;
    }

    if program_parts.is_empty() {
        return Err(vec![AppletError::new(APPLET, "missing program")]);
    }

    Ok(Options {
        field_separator,
        assignments,
        program: program_parts.join("\n"),
        files,
    })
}

fn parse_assignment(text: &str) -> Result<(String, String), Vec<AppletError>> {
    let Some((name, value)) = text.split_once('=') else {
        return Err(vec![AppletError::new(
            APPLET,
            format!("invalid assignment: {text}"),
        )]);
    };
    if name.is_empty() {
        return Err(vec![AppletError::new(
            APPLET,
            format!("invalid assignment: {text}"),
        )]);
    }
    Ok((name.to_owned(), value.to_owned()))
}

#[derive(Debug)]
struct Program {
    begin_rules: Vec<Rule>,
    main_rules: Vec<Rule>,
    end_rules: Vec<Rule>,
    functions: HashMap<String, Function>,
}

#[derive(Debug)]
struct Rule {
    action: Vec<Stmt>,
}

#[derive(Debug, Clone)]
enum Stmt {
    Print(Vec<Expr>),
    Printf(Expr, Vec<Expr>),
    Assign(String, Expr),
    If(Expr, Box<Stmt>),
    Return(Option<Expr>),
    Block(Vec<Stmt>),
    Expr(Expr),
}

#[derive(Debug, Clone)]
enum Expr {
    Number(f64),
    String(String),
    Var(String),
    Field(Box<Expr>),
    UnaryMinus(Box<Expr>),
    UnaryPlus(Box<Expr>),
    Conditional(Box<Expr>, Box<Expr>, Box<Expr>),
    Binary(BinaryOp, Box<Expr>, Box<Expr>),
    Call(String, Vec<Expr>),
}

#[derive(Debug, Clone)]
struct Function {
    params: Vec<String>,
    body: Vec<Stmt>,
}

#[derive(Debug, Clone, Copy)]
enum BinaryOp {
    Add,
    Sub,
    Mul,
    Div,
    Mod,
    Eq,
    Ne,
    Lt,
    Le,
    Gt,
    Ge,
    Concat,
}

#[derive(Debug, Clone)]
enum Token {
    Begin,
    End,
    Function,
    Return,
    If,
    Print,
    Printf,
    Ident(String),
    Number(f64),
    String(String),
    LBrace,
    RBrace,
    LParen,
    RParen,
    Comma,
    Semicolon,
    Question,
    Colon,
    Dollar,
    Assign,
    Eq,
    Ne,
    Lt,
    Le,
    Gt,
    Ge,
    Plus,
    Minus,
    Star,
    Slash,
    Percent,
    Eof,
}

struct Parser<'a> {
    lexer: Lexer<'a>,
    current: Token,
    current_had_space: bool,
}

fn parse_program(source: &str) -> Result<Program, Vec<AppletError>> {
    let mut parser = Parser::new(source)?;
    let mut begin_rules = Vec::new();
    let mut main_rules = Vec::new();
    let mut end_rules = Vec::new();
    let mut functions = HashMap::new();

    while !matches!(parser.current, Token::Eof) {
        parser.skip_separators()?;
        match &parser.current {
            Token::Function => {
                let function = parser.parse_function()?;
                functions.insert(function.0, function.1);
            }
            Token::Begin => {
                parser.bump()?;
                begin_rules.push(Rule {
                    action: parser.parse_block()?,
                });
            }
            Token::End => {
                parser.bump()?;
                end_rules.push(Rule {
                    action: parser.parse_block()?,
                });
            }
            Token::LBrace => {
                main_rules.push(Rule {
                    action: parser.parse_block()?,
                });
            }
            Token::Eof => break,
            _ => {
                return Err(vec![AppletError::new(
                    APPLET,
                    "unsupported awk rule syntax",
                )]);
            }
        }
        parser.skip_separators()?;
    }

    Ok(Program {
        begin_rules,
        main_rules,
        end_rules,
        functions,
    })
}

impl<'a> Parser<'a> {
    fn new(source: &'a str) -> Result<Self, Vec<AppletError>> {
        let mut lexer = Lexer::new(source);
        let (current, current_had_space) = lexer.next_token()?;
        Ok(Self {
            lexer,
            current,
            current_had_space,
        })
    }

    fn bump(&mut self) -> Result<(), Vec<AppletError>> {
        let (current, had_space) = self.lexer.next_token()?;
        self.current = current;
        self.current_had_space = had_space;
        Ok(())
    }

    fn skip_separators(&mut self) -> Result<(), Vec<AppletError>> {
        while matches!(self.current, Token::Semicolon) {
            self.bump()?;
        }
        Ok(())
    }

    fn parse_block(&mut self) -> Result<Vec<Stmt>, Vec<AppletError>> {
        self.expect(Token::LBrace)?;
        let mut statements = Vec::new();
        self.skip_separators()?;
        while !matches!(self.current, Token::RBrace | Token::Eof) {
            statements.push(self.parse_stmt()?);
            self.skip_separators()?;
        }
        self.expect(Token::RBrace)?;
        Ok(statements)
    }

    fn parse_stmt(&mut self) -> Result<Stmt, Vec<AppletError>> {
        match &self.current {
            Token::Print => {
                self.bump()?;
                let mut exprs = Vec::new();
                if !matches!(self.current, Token::Semicolon | Token::RBrace | Token::Eof) {
                    loop {
                        exprs.push(self.parse_expr()?);
                        if !matches!(self.current, Token::Comma) {
                            break;
                        }
                        self.bump()?;
                    }
                }
                Ok(Stmt::Print(exprs))
            }
            Token::Printf => {
                self.bump()?;
                let format = self.parse_expr()?;
                let mut exprs = Vec::new();
                while matches!(self.current, Token::Comma) {
                    self.bump()?;
                    exprs.push(self.parse_expr()?);
                }
                Ok(Stmt::Printf(format, exprs))
            }
            Token::If => {
                self.bump()?;
                self.expect(Token::LParen)?;
                let condition = self.parse_expr()?;
                self.expect(Token::RParen)?;
                let then_branch = self.parse_stmt()?;
                Ok(Stmt::If(condition, Box::new(then_branch)))
            }
            Token::Return => {
                self.bump()?;
                if matches!(self.current, Token::Semicolon | Token::RBrace | Token::Eof) {
                    Ok(Stmt::Return(None))
                } else {
                    Ok(Stmt::Return(Some(self.parse_expr()?)))
                }
            }
            Token::LBrace => Ok(Stmt::Block(self.parse_block()?)),
            Token::Ident(name) => {
                let name = name.clone();
                self.bump()?;
                if matches!(self.current, Token::Assign) {
                    self.bump()?;
                    Ok(Stmt::Assign(name, self.parse_expr()?))
                } else {
                    let expr = self.parse_expr_with_prefix(Expr::Var(name))?;
                    Ok(Stmt::Expr(expr))
                }
            }
            _ => Ok(Stmt::Expr(self.parse_expr()?)),
        }
    }

    fn parse_expr(&mut self) -> Result<Expr, Vec<AppletError>> {
        self.parse_conditional()
    }

    fn parse_expr_with_prefix(&mut self, prefix: Expr) -> Result<Expr, Vec<AppletError>> {
        let prefix = match prefix {
            Expr::Var(name) if matches!(self.current, Token::LParen) && !self.current_had_space => {
                self.bump()?;
                let mut args = Vec::new();
                if !matches!(self.current, Token::RParen) {
                    loop {
                        args.push(self.parse_expr()?);
                        if !matches!(self.current, Token::Comma) {
                            break;
                        }
                        self.bump()?;
                    }
                }
                self.expect(Token::RParen)?;
                Expr::Call(name, args)
            }
            other => other,
        };
        let left = self.parse_concat_tail(prefix)?;
        let left = self.parse_comparison_tail(left)?;
        self.parse_conditional_tail(left)
    }

    fn parse_conditional(&mut self) -> Result<Expr, Vec<AppletError>> {
        let left = self.parse_concat()?;
        let left = self.parse_comparison_tail(left)?;
        self.parse_conditional_tail(left)
    }

    fn parse_conditional_tail(&mut self, left: Expr) -> Result<Expr, Vec<AppletError>> {
        if !matches!(self.current, Token::Question) {
            return Ok(left);
        }
        self.bump()?;
        let yes = self.parse_expr()?;
        self.expect(Token::Colon)?;
        let no = self.parse_expr()?;
        Ok(Expr::Conditional(Box::new(left), Box::new(yes), Box::new(no)))
    }

    fn parse_comparison_tail(&mut self, mut left: Expr) -> Result<Expr, Vec<AppletError>> {
        loop {
            let op = match self.current {
                Token::Eq => BinaryOp::Eq,
                Token::Ne => BinaryOp::Ne,
                Token::Lt => BinaryOp::Lt,
                Token::Le => BinaryOp::Le,
                Token::Gt => BinaryOp::Gt,
                Token::Ge => BinaryOp::Ge,
                _ => break,
            };
            self.bump()?;
            let right = self.parse_concat()?;
            left = Expr::Binary(op, Box::new(left), Box::new(right));
        }
        Ok(left)
    }

    fn parse_concat(&mut self) -> Result<Expr, Vec<AppletError>> {
        let left = self.parse_additive()?;
        self.parse_concat_tail(left)
    }

    fn parse_concat_tail(&mut self, mut left: Expr) -> Result<Expr, Vec<AppletError>> {
        while token_starts_expr(&self.current) {
            let right = self.parse_additive()?;
            left = Expr::Binary(BinaryOp::Concat, Box::new(left), Box::new(right));
        }
        Ok(left)
    }

    fn parse_additive(&mut self) -> Result<Expr, Vec<AppletError>> {
        let mut left = self.parse_multiplicative()?;
        loop {
            let op = match self.current {
                Token::Plus => BinaryOp::Add,
                Token::Minus => BinaryOp::Sub,
                _ => break,
            };
            self.bump()?;
            let right = self.parse_multiplicative()?;
            left = Expr::Binary(op, Box::new(left), Box::new(right));
        }
        Ok(left)
    }

    fn parse_multiplicative(&mut self) -> Result<Expr, Vec<AppletError>> {
        let mut left = self.parse_unary()?;
        loop {
            let op = match self.current {
                Token::Star => BinaryOp::Mul,
                Token::Slash => BinaryOp::Div,
                Token::Percent => BinaryOp::Mod,
                _ => break,
            };
            self.bump()?;
            let right = self.parse_unary()?;
            left = Expr::Binary(op, Box::new(left), Box::new(right));
        }
        Ok(left)
    }

    fn parse_unary(&mut self) -> Result<Expr, Vec<AppletError>> {
        match self.current {
            Token::Minus => {
                self.bump()?;
                Ok(Expr::UnaryMinus(Box::new(self.parse_unary()?)))
            }
            Token::Plus => {
                self.bump()?;
                Ok(Expr::UnaryPlus(Box::new(self.parse_unary()?)))
            }
            Token::Dollar => {
                self.bump()?;
                Ok(Expr::Field(Box::new(self.parse_unary()?)))
            }
            _ => self.parse_primary(),
        }
    }

    fn parse_primary(&mut self) -> Result<Expr, Vec<AppletError>> {
        match &self.current {
            Token::Number(value) => {
                let value = *value;
                self.bump()?;
                Ok(Expr::Number(value))
            }
            Token::String(value) => {
                let value = value.clone();
                self.bump()?;
                Ok(Expr::String(value))
            }
            Token::Ident(name) => {
                let name = name.clone();
                self.bump()?;
                if matches!(self.current, Token::LParen) && !self.current_had_space {
                    self.bump()?;
                    let mut args = Vec::new();
                    if !matches!(self.current, Token::RParen) {
                        loop {
                            args.push(self.parse_expr()?);
                            if !matches!(self.current, Token::Comma) {
                                break;
                            }
                            self.bump()?;
                        }
                    }
                    self.expect(Token::RParen)?;
                    Ok(Expr::Call(name, args))
                } else {
                    Ok(Expr::Var(name))
                }
            }
            Token::LParen => {
                self.bump()?;
                let expr = self.parse_expr()?;
                self.expect(Token::RParen)?;
                Ok(expr)
            }
            _ => Err(vec![AppletError::new(
                APPLET,
                "unexpected token in expression",
            )]),
        }
    }

    fn parse_function(&mut self) -> Result<(String, Function), Vec<AppletError>> {
        self.expect(Token::Function)?;
        let Token::Ident(name) = &self.current else {
            return Err(vec![AppletError::new(APPLET, "expected function name")]);
        };
        let name = name.clone();
        self.bump()?;
        self.expect(Token::LParen)?;
        let mut params = Vec::new();
        if !matches!(self.current, Token::RParen) {
            loop {
                let Token::Ident(param) = &self.current else {
                    return Err(vec![AppletError::new(APPLET, "expected function parameter")]);
                };
                params.push(param.clone());
                self.bump()?;
                if !matches!(self.current, Token::Comma) {
                    break;
                }
                self.bump()?;
            }
        }
        self.expect(Token::RParen)?;
        let body = self.parse_block()?;
        Ok((name, Function { params, body }))
    }

    fn expect(&mut self, expected: Token) -> Result<(), Vec<AppletError>> {
        if std::mem::discriminant(&self.current) != std::mem::discriminant(&expected) {
            return Err(vec![AppletError::new(APPLET, "unexpected token")]);
        }
        self.bump()
    }
}

fn token_starts_expr(token: &Token) -> bool {
    matches!(
        token,
        Token::Number(_)
            | Token::String(_)
            | Token::Ident(_)
            | Token::LParen
            | Token::Dollar
            | Token::Plus
            | Token::Minus
    )
}

struct Lexer<'a> {
    source: &'a str,
    index: usize,
}

impl<'a> Lexer<'a> {
    fn new(source: &'a str) -> Self {
        Self { source, index: 0 }
    }

    fn next_token(&mut self) -> Result<(Token, bool), Vec<AppletError>> {
        let had_space = self.skip_space();
        let Some(ch) = self.peek() else {
            return Ok((Token::Eof, had_space));
        };
        if ch == '\n' || ch == ';' {
            self.index += 1;
            return Ok((Token::Semicolon, had_space));
        }
        if ch == '#' {
            self.skip_comment();
            return self.next_token();
        }
        match ch {
            '{' => {
                self.index += 1;
                Ok((Token::LBrace, had_space))
            }
            '}' => {
                self.index += 1;
                Ok((Token::RBrace, had_space))
            }
            '(' => {
                self.index += 1;
                Ok((Token::LParen, had_space))
            }
            ')' => {
                self.index += 1;
                Ok((Token::RParen, had_space))
            }
            ',' => {
                self.index += 1;
                Ok((Token::Comma, had_space))
            }
            '?' => {
                self.index += 1;
                Ok((Token::Question, had_space))
            }
            ':' => {
                self.index += 1;
                Ok((Token::Colon, had_space))
            }
            '$' => {
                self.index += 1;
                Ok((Token::Dollar, had_space))
            }
            '+' => {
                self.index += 1;
                Ok((Token::Plus, had_space))
            }
            '-' => {
                self.index += 1;
                Ok((Token::Minus, had_space))
            }
            '*' => {
                self.index += 1;
                Ok((Token::Star, had_space))
            }
            '/' => {
                self.index += 1;
                Ok((Token::Slash, had_space))
            }
            '%' => {
                self.index += 1;
                Ok((Token::Percent, had_space))
            }
            '=' => {
                self.index += 1;
                if self.peek() == Some('=') {
                    self.index += 1;
                    Ok((Token::Eq, had_space))
                } else {
                    Ok((Token::Assign, had_space))
                }
            }
            '!' => {
                self.index += 1;
                if self.peek() == Some('=') {
                    self.index += 1;
                    Ok((Token::Ne, had_space))
                } else {
                    Err(vec![AppletError::new(APPLET, "unsupported operator")])
                }
            }
            '<' => {
                self.index += 1;
                if self.peek() == Some('=') {
                    self.index += 1;
                    Ok((Token::Le, had_space))
                } else {
                    Ok((Token::Lt, had_space))
                }
            }
            '>' => {
                self.index += 1;
                if self.peek() == Some('=') {
                    self.index += 1;
                    Ok((Token::Ge, had_space))
                } else {
                    Ok((Token::Gt, had_space))
                }
            }
            '"' => self.read_string().map(|token| (Token::String(token), had_space)),
            ch if ch.is_ascii_digit() => self.read_number().map(|token| (Token::Number(token), had_space)),
            ch if is_ident_start(ch) => {
                let ident = self.read_identifier();
                Ok(match ident.as_str() {
                    "BEGIN" => (Token::Begin, had_space),
                    "END" => (Token::End, had_space),
                    "function" | "func" => (Token::Function, had_space),
                    "return" => (Token::Return, had_space),
                    "if" => (Token::If, had_space),
                    "print" => (Token::Print, had_space),
                    "printf" => (Token::Printf, had_space),
                    _ => (Token::Ident(ident), had_space),
                })
            }
            _ => Err(vec![AppletError::new(APPLET, "unsupported awk syntax")]),
        }
    }

    fn skip_space(&mut self) -> bool {
        let start = self.index;
        while matches!(self.peek(), Some(' ' | '\t' | '\r')) {
            self.index += 1;
        }
        self.index != start
    }

    fn skip_comment(&mut self) {
        while let Some(ch) = self.peek() {
            self.index += 1;
            if ch == '\n' {
                break;
            }
        }
    }

    fn read_string(&mut self) -> Result<String, Vec<AppletError>> {
        self.index += 1;
        let mut value = String::new();
        while let Some(ch) = self.peek() {
            self.index += ch.len_utf8();
            match ch {
                '"' => return Ok(value),
                '\\' => {
                    let escaped = self
                        .next_char()
                        .ok_or_else(|| vec![AppletError::new(APPLET, "unterminated string")])?;
                    value.push(parse_escape_char(&mut self.index, escaped, self.source)?);
                }
                _ => value.push(ch),
            }
        }
        Err(vec![AppletError::new(APPLET, "unterminated string")])
    }

    fn read_number(&mut self) -> Result<f64, Vec<AppletError>> {
        let start = self.index;
        if self.source[self.index..].starts_with("0x") || self.source[self.index..].starts_with("0X") {
            self.index += 2;
            while matches!(self.peek(), Some(ch) if ch.is_ascii_hexdigit()) {
                self.index += 1;
            }
            let value = u64::from_str_radix(&self.source[start + 2..self.index], 16)
                .map_err(|_| vec![AppletError::new(APPLET, "invalid number")])?;
            return Ok(value as f64);
        }
        if self.peek() == Some('0')
            && matches!(self.peek_n(1), Some(ch) if ch.is_ascii_digit())
            && !matches!(self.peek_n(1), Some('8' | '9'))
        {
            self.index += 1;
            while matches!(self.peek(), Some(ch) if ch.is_ascii_digit()) {
                self.index += 1;
            }
            let value = u64::from_str_radix(&self.source[start + 1..self.index], 8)
                .map_err(|_| vec![AppletError::new(APPLET, "invalid number")])?;
            return Ok(value as f64);
        }
        while matches!(self.peek(), Some(ch) if ch.is_ascii_digit() || ch == '.') {
            self.index += 1;
        }
        self.source[start..self.index]
            .parse::<f64>()
            .map_err(|_| vec![AppletError::new(APPLET, "invalid number")])
    }

    fn read_identifier(&mut self) -> String {
        let start = self.index;
        while let Some(ch) = self.peek() {
            if !is_ident_continue(ch) {
                break;
            }
            self.index += ch.len_utf8();
        }
        self.source[start..self.index].to_owned()
    }

    fn peek(&self) -> Option<char> {
        self.source[self.index..].chars().next()
    }

    fn peek_n(&self, offset: usize) -> Option<char> {
        self.source[self.index..].chars().nth(offset)
    }

    fn next_char(&mut self) -> Option<char> {
        let ch = self.peek()?;
        self.index += ch.len_utf8();
        Some(ch)
    }
}

fn is_ident_start(ch: char) -> bool {
    ch == '_' || ch.is_ascii_alphabetic()
}

fn is_ident_continue(ch: char) -> bool {
    is_ident_start(ch) || ch.is_ascii_digit()
}

fn unescape_string(text: &str) -> Result<String, Vec<AppletError>> {
    let mut result = String::new();
    let mut chars = text.chars();
    while let Some(ch) = chars.next() {
        if ch != '\\' {
            result.push(ch);
            continue;
        }
        let Some(next) = chars.next() else {
            result.push('\\');
            break;
        };
        result.push(match next {
            'n' => '\n',
            't' => '\t',
            'r' => '\r',
            '\\' => '\\',
            '\'' => '\'',
            '"' => '"',
            'x' => {
                let hi = chars
                    .next()
                    .ok_or_else(|| vec![AppletError::new(APPLET, "invalid escape")])?;
                let lo = chars
                    .next()
                    .ok_or_else(|| vec![AppletError::new(APPLET, "invalid escape")])?;
                let hex = format!("{hi}{lo}");
                let value = u8::from_str_radix(&hex, 16)
                    .map_err(|_| vec![AppletError::new(APPLET, "invalid escape")])?;
                value as char
            }
            other => other,
        });
    }
    Ok(result)
}

fn parse_escape_char(index: &mut usize, escaped: char, source: &str) -> Result<char, Vec<AppletError>> {
    Ok(match escaped {
        'n' => '\n',
        't' => '\t',
        'r' => '\r',
        '\\' => '\\',
        '"' => '"',
        'x' => {
            let chars = source[*index..].chars().take(2).collect::<Vec<_>>();
            if chars.len() != 2 {
                return Err(vec![AppletError::new(APPLET, "invalid hex escape")]);
            }
            *index += chars[0].len_utf8() + chars[1].len_utf8();
            let hex = chars.iter().collect::<String>();
            u8::from_str_radix(&hex, 16)
                .map_err(|_| vec![AppletError::new(APPLET, "invalid hex escape")])? as char
        }
        other => other,
    })
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;
    use std::io;

    use super::{Environment, Expr, Parser, Value, eval_expr, parse_program, split_fields};

    #[test]
    fn split_fields_preserves_empty_matches() {
        let fields = split_fields("z##abc##zz", "[#]").expect("fields");
        assert_eq!(fields, ["z", "", "abc", "", "zz"]);
    }

    #[test]
    fn split_fields_default_space_collapses_runs() {
        let fields = split_fields("  one   two ", " ").expect("fields");
        assert_eq!(fields, ["one", "two"]);
    }

    #[test]
    fn parser_handles_begin_and_main_blocks() {
        let program = parse_program("BEGIN { print 1 }\n{ print $1 }\nEND { print 2 }").expect("program");
        assert_eq!(program.begin_rules.len(), 1);
        assert_eq!(program.main_rules.len(), 1);
        assert_eq!(program.end_rules.len(), 1);
    }

    #[test]
    fn concatenation_expression_is_parsed() {
        let mut parser = Parser::new("v (a)").expect("parser");
        let expr = parser.parse_expr().expect("expr");
        let mut env = Environment::new(HashMap::new());
        env.set_var("v", Value::Number(1.0));
        env.set_var("a", Value::Number(2.0));
        assert_eq!(
            eval_expr(&expr, &mut env, &mut io::sink())
                .expect("value")
                .as_string(),
            "12"
        );
    }

    #[test]
    fn field_access_uses_nf_expression() {
        let mut env = Environment::new(HashMap::new());
        env.set_record("alpha beta").expect("record");
        let expr = Expr::Field(Box::new(Expr::Var("NF".to_owned())));
        assert_eq!(
            eval_expr(&expr, &mut env, &mut io::sink())
                .expect("value")
                .as_string(),
            "beta"
        );
    }
}
