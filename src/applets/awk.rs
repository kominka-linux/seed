use std::collections::HashMap;
use std::ffi::CString;
use std::fs;
use std::io::{self, BufRead, BufReader, Read, Write};
use std::mem::MaybeUninit;

use crate::common::applet::{AppletCodeResult, finish_code};
use crate::common::args::argv_to_strings;
use crate::common::error::AppletError;

const APPLET: &str = "awk";
const MATCH_SLOTS: usize = 1;

pub fn main(args: &[std::ffi::OsString]) -> i32 {
    finish_code(run(args))
}

fn run(args: &[std::ffi::OsString]) -> AppletCodeResult {
    let args = argv_to_strings(APPLET, args)?;
    let options = parse_options(&args)?;
    let program = parse_program(&options.program)?;

    let mut env = Environment::new(program.functions.clone());
    env.set_var("ARGC", Value::Number((options.files.len() + 1) as f64));
    for (name, value) in options.assignments {
        env.set_var(&name, Value::String(value));
    }
    if let Some(fs) = options.field_separator {
        env.set_var("FS", Value::String(fs));
    }

    let mut out = io::stdout().lock();
    let mut exit_code = match execute_rules(&program.begin_rules, &mut env, &mut out)? {
        ExecFlow::Exit(code) => Some(code),
        _ => None,
    };

    if exit_code.is_none() && !program.main_rules.is_empty() {
        let paths = if options.files.is_empty() {
            vec![None]
        } else {
            options
                .files
                .iter()
                .map(|path| Some(path.as_str()))
                .collect()
        };

        for path in paths {
            let reader: Box<dyn BufRead> = match path {
                Some(path) => match fs::File::open(path) {
                    Ok(file) => Box::new(BufReader::new(file)),
                    Err(err) => {
                        return Err(vec![AppletError::from_io(
                            APPLET,
                            "opening",
                            Some(path),
                            err,
                        )]);
                    }
                },
                None => Box::new(BufReader::new(io::stdin().lock())),
            };
            let lines = collect_input_lines(reader)?;
            env.start_file(lines);
            if let Some(code) = process_reader(&program.main_rules, &mut env, &mut out)? {
                exit_code = Some(code);
                break;
            }
        }
    }

    if let ExecFlow::Exit(code) = execute_rules(&program.end_rules, &mut env, &mut out)? {
        exit_code = Some(code);
    }
    out.flush()
        .map_err(|err| vec![AppletError::new(APPLET, format!("writing stdout: {err}"))])?;
    Ok(exit_code.unwrap_or(0))
}

fn collect_input_lines<R: BufRead>(mut reader: R) -> Result<Vec<String>, Vec<AppletError>> {
    let mut lines = Vec::new();
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
        lines.push(line.clone());
    }
    Ok(lines)
}

fn process_reader(
    rules: &[Rule],
    env: &mut Environment,
    out: &mut dyn Write,
) -> Result<Option<i32>, Vec<AppletError>> {
    while let Some(line) = env.next_input_record() {
        env.set_record(&line)?;
        match execute_rules(rules, env, out)? {
            ExecFlow::Exit(code) => return Ok(Some(code)),
            ExecFlow::NextRecord => continue,
            ExecFlow::NextFile => break,
            _ => {}
        }
    }
    Ok(None)
}

fn execute_rules(
    rules: &[Rule],
    env: &mut Environment,
    out: &mut dyn Write,
) -> Result<ExecFlow, Vec<AppletError>> {
    for rule in rules {
        if let Some(pattern) = &rule.pattern
            && !eval_expr(pattern, env, out)?.is_truthy()
        {
            continue;
        }
        let flow = if rule.default_print {
            print_current_record(env, out)?
        } else {
            execute_block(&rule.action, env, out)?
        };
        if !matches!(flow, ExecFlow::Next) {
            return Ok(flow);
        }
    }
    Ok(ExecFlow::Next)
}

fn execute_block(
    block: &[Stmt],
    env: &mut Environment,
    out: &mut dyn Write,
) -> Result<ExecFlow, Vec<AppletError>> {
    for stmt in block {
        let flow = execute_stmt(stmt, env, out)?;
        if !matches!(flow, ExecFlow::Next) {
            return Ok(flow);
        }
    }
    Ok(ExecFlow::Next)
}

fn execute_stmt(
    stmt: &Stmt,
    env: &mut Environment,
    out: &mut dyn Write,
) -> Result<ExecFlow, Vec<AppletError>> {
    match stmt {
        Stmt::Print(exprs) => {
            print_exprs(exprs, env, out)?;
            Ok(ExecFlow::Next)
        }
        Stmt::Printf(format, exprs) => {
            let rendered =
                render_printf(&eval_expr(format, env, out)?.as_string(), exprs, env, out)?;
            out.write_all(rendered.as_bytes())
                .map_err(|err| vec![AppletError::new(APPLET, format!("writing stdout: {err}"))])?;
            Ok(ExecFlow::Next)
        }
        Stmt::Assign(target, op, expr) => {
            let value = eval_expr(expr, env, out)?;
            assign_with_op(target, *op, value, env, out)?;
            Ok(ExecFlow::Next)
        }
        Stmt::If(condition, then_branch, else_branch) => {
            if eval_expr(condition, env, out)?.is_truthy() {
                return execute_stmt(then_branch, env, out);
            }
            if let Some(else_branch) = else_branch {
                return execute_stmt(else_branch, env, out);
            }
            Ok(ExecFlow::Next)
        }
        Stmt::While(condition, body) => {
            while eval_expr(condition, env, out)?.is_truthy() {
                match execute_stmt(body, env, out)? {
                    ExecFlow::Next | ExecFlow::Continue => {}
                    ExecFlow::Break => break,
                    flow @ (
                        ExecFlow::Return(_)
                        | ExecFlow::Exit(_)
                        | ExecFlow::NextRecord
                        | ExecFlow::NextFile
                    ) => {
                        return Ok(flow);
                    }
                }
            }
            Ok(ExecFlow::Next)
        }
        Stmt::DoWhile(body, condition) => {
            loop {
                match execute_stmt(body, env, out)? {
                    ExecFlow::Next | ExecFlow::Continue => {}
                    ExecFlow::Break => break,
                    flow @ (
                        ExecFlow::Return(_)
                        | ExecFlow::Exit(_)
                        | ExecFlow::NextRecord
                        | ExecFlow::NextFile
                    ) => return Ok(flow),
                }
                if !eval_expr(condition, env, out)?.is_truthy() {
                    break;
                }
            }
            Ok(ExecFlow::Next)
        }
        Stmt::ForLoop(init, condition, step, body) => {
            if let Some(init) = init {
                match execute_stmt(init, env, out)? {
                    ExecFlow::Next => {}
                    flow => return Ok(flow),
                }
            }
            loop {
                if let Some(condition) = condition
                    && !eval_expr(condition, env, out)?.is_truthy()
                {
                    break;
                }
                match execute_stmt(body, env, out)? {
                    ExecFlow::Next | ExecFlow::Continue => {}
                    ExecFlow::Break => break,
                    flow @ (
                        ExecFlow::Return(_)
                        | ExecFlow::Exit(_)
                        | ExecFlow::NextRecord
                        | ExecFlow::NextFile
                    ) => {
                        return Ok(flow);
                    }
                }
                if let Some(step) = step {
                    let _ = eval_expr(step, env, out)?;
                }
            }
            Ok(ExecFlow::Next)
        }
        Stmt::ForIn(var, array, body) => {
            for key in env.array_keys(array) {
                env.set_var(var, Value::String(key));
                match execute_stmt(body, env, out)? {
                    ExecFlow::Next | ExecFlow::Continue => {}
                    ExecFlow::Break => break,
                    flow @ (
                        ExecFlow::Return(_)
                        | ExecFlow::Exit(_)
                        | ExecFlow::NextRecord
                        | ExecFlow::NextFile
                    ) => {
                        return Ok(flow);
                    }
                }
            }
            Ok(ExecFlow::Next)
        }
        Stmt::Delete(array, index) => {
            let key = eval_expr(index, env, out)?.as_string();
            env.delete_array_element(array, &key);
            Ok(ExecFlow::Next)
        }
        Stmt::Return(expr) => Ok(ExecFlow::Return(if let Some(expr) = expr {
            eval_expr(expr, env, out)?
        } else {
            Value::String(String::new())
        })),
        Stmt::Break => Ok(ExecFlow::Break),
        Stmt::Continue => Ok(ExecFlow::Continue),
        Stmt::Exit(expr) => Ok(ExecFlow::Exit(
            expr.as_ref()
                .map(|value| eval_expr(value, env, out).map(|value| value.as_number() as i32))
                .transpose()?
                .unwrap_or(0),
        )),
        Stmt::Next => Ok(ExecFlow::NextRecord),
        Stmt::NextFile => {
            env.skip_current_file();
            Ok(ExecFlow::NextFile)
        }
        Stmt::Getline(target, source) => {
            let _ = execute_getline(target.as_deref(), source.as_ref(), env, out)?;
            Ok(ExecFlow::Next)
        }
        Stmt::Block(stmts) => execute_block(stmts, env, out),
        Stmt::Expr(expr) => {
            let _ = eval_expr(expr, env, out)?;
            Ok(ExecFlow::Next)
        }
    }
}

fn print_exprs(
    exprs: &[Expr],
    env: &mut Environment,
    out: &mut dyn Write,
) -> Result<(), Vec<AppletError>> {
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
    Ok(())
}

fn print_current_record(
    env: &mut Environment,
    out: &mut dyn Write,
) -> Result<ExecFlow, Vec<AppletError>> {
    print_exprs(&[], env, out)?;
    Ok(ExecFlow::Next)
}

fn eval_expr(
    expr: &Expr,
    env: &mut Environment,
    out: &mut dyn Write,
) -> Result<Value, Vec<AppletError>> {
    match expr {
        Expr::Number(value) => Ok(Value::Number(*value)),
        Expr::String(value) => Ok(Value::String(value.clone())),
        Expr::Regex(pattern) => {
            let regex = Regex::compile_with_fallback(pattern, true)
                .map_err(|message| vec![AppletError::new(APPLET, message)])?;
            Ok(Value::Number(
                if regex.find(&env.record_text(), 0)?.is_some() {
                    1.0
                } else {
                    0.0
                },
            ))
        }
        Expr::Getline(target, source) => Ok(Value::Number(
            execute_getline(target.as_deref(), source.as_deref(), env, out)? as f64,
        )),
        Expr::Var(name) => {
            if name == "length" && !env.has_user_var(name) && !env.has_array(name) {
                return Ok(Value::Number(env.record_text().chars().count() as f64));
            }
            Ok(env.get_var(name))
        }
        Expr::ArrayGet(name, index) => {
            let key = eval_expr(index, env, out)?.as_string();
            Ok(env.get_array_element(name, &key))
        }
        Expr::Field(index) => {
            let field = eval_expr(index, env, out)?.as_number() as isize;
            if field < 0 {
                return Err(vec![AppletError::new(APPLET, "access to negative field")]);
            }
            Ok(Value::String(env.get_field(field)))
        }
        Expr::UnaryNot(expr) => Ok(Value::Number(if eval_expr(expr, env, out)?.is_truthy() {
            0.0
        } else {
            1.0
        })),
        Expr::UnaryMinus(expr) => Ok(Value::Number(-eval_expr(expr, env, out)?.as_number())),
        Expr::UnaryPlus(expr) => Ok(Value::Number(eval_expr(expr, env, out)?.as_number())),
        Expr::PreInc(target) => update_lvalue(target, 1.0, true, env, out),
        Expr::PreDec(target) => update_lvalue(target, -1.0, true, env, out),
        Expr::PostInc(target) => update_lvalue(target, 1.0, false, env, out),
        Expr::PostDec(target) => update_lvalue(target, -1.0, false, env, out),
        Expr::Conditional(condition, yes, no) => {
            if eval_expr(condition, env, out)?.is_truthy() {
                eval_expr(yes, env, out)
            } else {
                eval_expr(no, env, out)
            }
        }
        Expr::Binary(op, left, right) => match op {
            BinaryOp::Or => {
                let left = eval_expr(left, env, out)?;
                if left.is_truthy() {
                    Ok(Value::Number(1.0))
                } else {
                    Ok(Value::Number(if eval_expr(right, env, out)?.is_truthy() {
                        1.0
                    } else {
                        0.0
                    }))
                }
            }
            BinaryOp::And => {
                let left = eval_expr(left, env, out)?;
                if !left.is_truthy() {
                    Ok(Value::Number(0.0))
                } else {
                    Ok(Value::Number(if eval_expr(right, env, out)?.is_truthy() {
                        1.0
                    } else {
                        0.0
                    }))
                }
            }
            BinaryOp::Match | BinaryOp::NotMatch => {
                let left = eval_expr(left, env, out)?.as_string();
                let pattern = regex_pattern_from_expr(right, env, out)?;
                let regex = Regex::compile_with_fallback(&pattern, true)
                    .map_err(|message| vec![AppletError::new(APPLET, message)])?;
                let matched = regex.find(&left, 0)?.is_some();
                Ok(Value::Number(if matches!(op, BinaryOp::Match) == matched {
                    1.0
                } else {
                    0.0
                }))
            }
            _ => {
                let left = eval_expr(left, env, out)?;
                let right = eval_expr(right, env, out)?;
                match op {
                    BinaryOp::Pow => Ok(Value::Number(left.as_number().powf(right.as_number()))),
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
                    BinaryOp::Or | BinaryOp::And | BinaryOp::Match | BinaryOp::NotMatch => {
                        unreachable!()
                    }
                }
            }
        },
        Expr::Call(name, args) => match name.as_str() {
            "length" => {
                if let Some(Expr::Var(name)) = args.first()
                    && env.has_array(name)
                {
                    return Ok(Value::Number(env.array_len(name) as f64));
                }
                let text = if let Some(arg) = args.first() {
                    eval_expr(arg, env, out)?.as_string()
                } else {
                    env.record_text()
                };
                Ok(Value::Number(text.chars().count() as f64))
            }
            "int" => {
                let value = if let Some(arg) = args.first() {
                    eval_expr(arg, env, out)?.as_number()
                } else {
                    0.0
                };
                Ok(Value::Number(value.trunc()))
            }
            "index" => {
                if args.len() != 2 {
                    return Err(vec![AppletError::new(
                        APPLET,
                        "index() expects 2 arguments",
                    )]);
                }
                let haystack = eval_expr(&args[0], env, out)?.as_string();
                let needle = eval_expr(&args[1], env, out)?.as_string();
                let index = haystack.find(&needle).map_or(0, |i| i + 1);
                Ok(Value::Number(index as f64))
            }
            "substr" => {
                if args.len() < 2 || args.len() > 3 {
                    return Err(vec![AppletError::new(
                        APPLET,
                        "substr() expects 2 or 3 arguments",
                    )]);
                }
                let text = eval_expr(&args[0], env, out)?.as_string();
                let start = eval_expr(&args[1], env, out)?.as_number().max(1.0) as usize;
                let len = if let Some(arg) = args.get(2) {
                    Some(eval_expr(arg, env, out)?.as_number().max(0.0) as usize)
                } else {
                    None
                };
                let chars = text.chars().collect::<Vec<_>>();
                if start == 0 || start > chars.len() + 1 {
                    return Ok(Value::String(String::new()));
                }
                let start_index = start - 1;
                let end_index = len.map_or(chars.len(), |len| (start_index + len).min(chars.len()));
                Ok(Value::String(
                    chars[start_index..end_index].iter().collect(),
                ))
            }
            "sprintf" => {
                if args.is_empty() {
                    return Err(vec![AppletError::new(
                        APPLET,
                        "sprintf() expects a format string",
                    )]);
                }
                let rendered = render_printf(
                    &eval_expr(&args[0], env, out)?.as_string(),
                    &args[1..],
                    env,
                    out,
                )?;
                Ok(Value::String(rendered))
            }
            "close" => {
                if let Some(arg) = args.first() {
                    let path = eval_expr(arg, env, out)?.as_string();
                    env.close_path(&path);
                }
                Ok(Value::Number(0.0))
            }
            "sub" => execute_substitution(args, env, out, false),
            "gsub" => execute_substitution(args, env, out, true),
            "match" => {
                if args.len() != 2 {
                    return Err(vec![AppletError::new(
                        APPLET,
                        "match() expects 2 arguments",
                    )]);
                }
                let text = eval_expr(&args[0], env, out)?.as_string();
                let pattern = regex_pattern_from_expr(&args[1], env, out)?;
                let regex = Regex::compile_with_fallback(&pattern, true)
                    .map_err(|message| vec![AppletError::new(APPLET, message)])?;
                if let Some((start, end)) = regex.find(&text, 0)? {
                    env.set_match_info(start + 1, end - start);
                    Ok(Value::Number((start + 1) as f64))
                } else {
                    env.set_match_info(0, 0);
                    Ok(Value::Number(0.0))
                }
            }
            "split" => {
                if args.len() < 2 || args.len() > 3 {
                    return Err(vec![AppletError::new(
                        APPLET,
                        "split() expects 2 or 3 arguments",
                    )]);
                }
                let text = eval_expr(&args[0], env, out)?.as_string();
                let Some(Expr::Var(array_name)) = args.get(1) else {
                    return Err(vec![AppletError::new(
                        APPLET,
                        "split() expects an array variable",
                    )]);
                };
                let separator = if let Some(arg) = args.get(2) {
                    regex_pattern_from_expr(arg, env, out)?
                } else {
                    env.get_var("FS").as_string()
                };
                let fields = split_fields(&text, &separator)?;
                env.clear_array(array_name);
                for (index, field) in fields.iter().enumerate() {
                    env.set_array_element(
                        array_name,
                        (index + 1).to_string(),
                        Value::String(field.clone()),
                    );
                }
                Ok(Value::Number(fields.len() as f64))
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

#[derive(Debug)]
enum ResolvedLValue {
    Var(String),
    Array(String, String),
}

fn assign_lvalue(
    target: &LValue,
    value: Value,
    env: &mut Environment,
    out: &mut dyn Write,
) -> Result<(), Vec<AppletError>> {
    match resolve_lvalue(target, env, out)? {
        ResolvedLValue::Var(name) => env.set_var(&name, value),
        ResolvedLValue::Array(name, key) => env.set_array_element(&name, key, value),
    }
    Ok(())
}

fn assign_with_op(
    target: &LValue,
    op: AssignOp,
    value: Value,
    env: &mut Environment,
    out: &mut dyn Write,
) -> Result<(), Vec<AppletError>> {
    if matches!(op, AssignOp::Set) {
        return assign_lvalue(target, value, env, out);
    }
    let current = get_lvalue_value(target, env, out)?;
    let next = match op {
        AssignOp::Set => value,
        AssignOp::Add => Value::Number(current.as_number() + value.as_number()),
        AssignOp::Sub => Value::Number(current.as_number() - value.as_number()),
        AssignOp::Mul => Value::Number(current.as_number() * value.as_number()),
        AssignOp::Div => Value::Number(current.as_number() / value.as_number()),
        AssignOp::Mod => Value::Number(current.as_number() % value.as_number()),
        AssignOp::Pow => Value::Number(current.as_number().powf(value.as_number())),
    };
    assign_lvalue(target, next, env, out)
}

fn update_lvalue(
    target: &LValue,
    delta: f64,
    prefix: bool,
    env: &mut Environment,
    out: &mut dyn Write,
) -> Result<Value, Vec<AppletError>> {
    match resolve_lvalue(target, env, out)? {
        ResolvedLValue::Var(name) => {
            let old = env.get_var(&name).as_number();
            let new = old + delta;
            env.set_var(&name, Value::Number(new));
            Ok(Value::Number(if prefix { new } else { old }))
        }
        ResolvedLValue::Array(name, key) => {
            let old = env.get_array_element(&name, &key).as_number();
            let new = old + delta;
            env.set_array_element(&name, key, Value::Number(new));
            Ok(Value::Number(if prefix { new } else { old }))
        }
    }
}

fn get_lvalue_value(
    target: &LValue,
    env: &mut Environment,
    out: &mut dyn Write,
) -> Result<Value, Vec<AppletError>> {
    Ok(match resolve_lvalue(target, env, out)? {
        ResolvedLValue::Var(name) => env.get_var(&name),
        ResolvedLValue::Array(name, key) => env.get_array_element(&name, &key),
    })
}

fn resolve_lvalue(
    target: &LValue,
    env: &mut Environment,
    out: &mut dyn Write,
) -> Result<ResolvedLValue, Vec<AppletError>> {
    Ok(match target {
        LValue::Var(name) => ResolvedLValue::Var(name.clone()),
        LValue::Array(name, index) => {
            ResolvedLValue::Array(name.clone(), eval_expr(index, env, out)?.as_string())
        }
    })
}

fn execute_getline(
    target: Option<&str>,
    source: Option<&Expr>,
    env: &mut Environment,
    out: &mut dyn Write,
) -> Result<i32, Vec<AppletError>> {
    let line = if let Some(source) = source {
        let path = eval_expr(source, env, out)?.as_string();
        match env.get_line_from_path(&path) {
            Ok(line) => line,
            Err(_) => return Ok(-1),
        }
    } else {
        env.set_errno(0);
        env.next_input_record()
    };

    match line {
        Some(line) => {
            if let Some(target) = target {
                if source.is_none() {
                    env.increment_record_counters();
                }
                env.set_var(target, Value::String(line));
            } else {
                env.set_record(&line)?;
            }
            Ok(1)
        }
        None => Ok(0),
    }
}

fn execute_substitution(
    args: &[Expr],
    env: &mut Environment,
    out: &mut dyn Write,
    global: bool,
) -> Result<Value, Vec<AppletError>> {
    if args.len() < 2 || args.len() > 3 {
        return Err(vec![AppletError::new(
            APPLET,
            "gsub() expects 2 or 3 arguments",
        )]);
    }
    let pattern = regex_pattern_from_expr(&args[0], env, out)?;
    let replacement = eval_expr(&args[1], env, out)?.as_string();
    let mut text = if let Some(target) = args.get(2) {
        eval_expr(target, env, out)?.as_string()
    } else {
        env.record_text()
    };
    let regex = Regex::compile_with_fallback(&pattern, true)
        .map_err(|message| vec![AppletError::new(APPLET, message)])?;
    let count = regex.replace_all(&mut text, &replacement, global)?;
    if let Some(target) = args.get(2) {
        let Some(lvalue) = expr_to_lvalue(target) else {
            return Err(vec![AppletError::new(
                APPLET,
                "expected substitution target",
            )]);
        };
        assign_lvalue(&lvalue, Value::String(text), env, out)?;
    } else {
        env.set_record(&text)?;
    }
    Ok(Value::Number(count as f64))
}

fn regex_pattern_from_expr(
    expr: &Expr,
    env: &mut Environment,
    out: &mut dyn Write,
) -> Result<String, Vec<AppletError>> {
    match expr {
        Expr::Regex(pattern) => Ok(pattern.clone()),
        _ => Ok(eval_expr(expr, env, out)?.as_string()),
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

#[derive(Debug)]
enum ExecFlow {
    Next,
    NextRecord,
    NextFile,
    Return(Value),
    Break,
    Continue,
    Exit(i32),
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
    arrays: HashMap<String, ArrayValue>,
    functions: HashMap<String, Function>,
    frames: Vec<HashMap<String, Value>>,
    readers: HashMap<String, BufReader<fs::File>>,
    current_input_lines: Vec<String>,
    current_input_index: usize,
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
            arrays: HashMap::new(),
            functions,
            frames: Vec::new(),
            readers: HashMap::new(),
            current_input_lines: Vec::new(),
            current_input_index: 0,
            record: String::new(),
            fields: Vec::new(),
            nr: 0,
            fnr: 0,
        }
    }

    fn start_file(&mut self, lines: Vec<String>) {
        self.fnr = 0;
        self.current_input_lines = lines;
        self.current_input_index = 0;
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
        let function = self.functions.get(name).cloned().ok_or_else(|| {
            vec![AppletError::new(
                APPLET,
                format!("undefined function: {name}"),
            )]
        })?;
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
            match execute_stmt(stmt, self, out)? {
                ExecFlow::Next | ExecFlow::Break | ExecFlow::Continue => {}
                ExecFlow::NextRecord => {
                    return Err(vec![AppletError::new(
                        APPLET,
                        "next from function not yet supported",
                    )]);
                }
                ExecFlow::NextFile => {
                    return Err(vec![AppletError::new(
                        APPLET,
                        "nextfile from function not yet supported",
                    )]);
                }
                ExecFlow::Return(value) => {
                    return_value = value;
                    break;
                }
                ExecFlow::Exit(code) => {
                    return Err(vec![AppletError::new(
                        APPLET,
                        format!("exit from function not yet supported: {code}"),
                    )]);
                }
            }
        }
        self.frames.pop();
        Ok(return_value)
    }

    fn array_len(&self, name: &str) -> usize {
        self.arrays.get(name).map_or(0, ArrayValue::len)
    }

    fn has_array(&self, name: &str) -> bool {
        self.arrays.contains_key(name)
    }

    fn get_array_element(&self, name: &str, key: &str) -> Value {
        self.arrays
            .get(name)
            .and_then(|array| array.get(key))
            .unwrap_or_else(|| Value::String(String::new()))
    }

    fn set_array_element(&mut self, name: &str, key: String, value: Value) {
        self.arrays
            .entry(name.to_owned())
            .or_default()
            .set(key, value);
    }

    fn clear_array(&mut self, name: &str) {
        self.arrays.remove(name);
    }

    fn delete_array_element(&mut self, name: &str, key: &str) {
        if let Some(array) = self.arrays.get_mut(name) {
            array.remove(key);
        }
    }

    fn array_keys(&self, name: &str) -> Vec<String> {
        self.arrays
            .get(name)
            .map_or_else(Vec::new, ArrayValue::keys)
    }

    fn get_line_from_path(&mut self, path: &str) -> Result<Option<String>, Vec<AppletError>> {
        if !self.readers.contains_key(path) {
            let file = fs::File::open(path).map_err(|err| {
                self.set_errno(err.raw_os_error().unwrap_or(1));
                vec![AppletError::from_io(APPLET, "opening", Some(path), err)]
            })?;
            self.readers.insert(path.to_owned(), BufReader::new(file));
        }
        let reader = self.readers.get_mut(path).expect("reader inserted");
        let mut line = String::new();
        let read = reader
            .read_line(&mut line)
            .map_err(|err| vec![AppletError::new(APPLET, format!("reading {path}: {err}"))])?;
        self.set_errno(0);
        if read == 0 {
            return Ok(None);
        }
        if line.ends_with('\n') {
            line.pop();
            if line.ends_with('\r') {
                line.pop();
            }
        }
        Ok(Some(line))
    }

    fn close_path(&mut self, path: &str) {
        self.readers.remove(path);
    }

    fn next_input_record(&mut self) -> Option<String> {
        let line = self
            .current_input_lines
            .get(self.current_input_index)
            .cloned()?;
        self.current_input_index += 1;
        Some(line)
    }

    fn increment_record_counters(&mut self) {
        self.nr += 1;
        self.fnr += 1;
    }

    fn skip_current_file(&mut self) {
        self.current_input_index = self.current_input_lines.len();
    }

    fn set_errno(&mut self, code: i32) {
        self.set_var("ERRNO", Value::Number(code as f64));
    }

    fn set_match_info(&mut self, start: usize, len: usize) {
        self.set_var("RSTART", Value::Number(start as f64));
        self.set_var("RLENGTH", Value::Number(len as f64));
    }

    fn has_user_var(&self, name: &str) -> bool {
        self.frames
            .iter()
            .rev()
            .any(|frame| frame.contains_key(name))
            || self.vars.contains_key(name)
    }
}

#[derive(Debug, Clone, Default)]
struct ArrayValue {
    order: Vec<String>,
    values: HashMap<String, Value>,
}

impl ArrayValue {
    fn len(&self) -> usize {
        self.values.len()
    }

    fn get(&self, key: &str) -> Option<Value> {
        self.values.get(key).cloned()
    }

    fn set(&mut self, key: String, value: Value) {
        if !self.values.contains_key(&key) {
            self.order.push(key.clone());
        }
        self.values.insert(key, value);
    }

    fn remove(&mut self, key: &str) {
        self.values.remove(key);
        self.order.retain(|entry| entry != key);
    }

    fn keys(&self) -> Vec<String> {
        self.order
            .iter()
            .filter(|key| self.values.contains_key(*key))
            .cloned()
            .collect()
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
    // Single-character separators are always literal (POSIX: matches FS behavior).
    let chars: Vec<char> = field_separator.chars().collect();
    if chars.len() == 1 {
        return Ok(record.split(chars[0]).map(str::to_owned).collect());
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
        Self::compile_inner(pattern, libc::REG_EXTENDED)
    }

    fn compile_with_fallback(pattern: &str, fallback_basic: bool) -> Result<Self, String> {
        match Self::compile_inner(pattern, libc::REG_EXTENDED) {
            Ok(regex) => Ok(regex),
            Err(err) if fallback_basic => Self::compile_inner(pattern, 0).or(Err(err)),
            Err(err) => Err(err),
        }
    }

    fn compile_inner(pattern: &str, flags: libc::c_int) -> Result<Self, String> {
        let c_pattern = CString::new(pattern)
            .map_err(|_| format!("field separator contains NUL byte: {pattern:?}"))?;
        let mut raw = MaybeUninit::<libc::regex_t>::zeroed();
        let rc = unsafe { libc::regcomp(raw.as_mut_ptr(), c_pattern.as_ptr(), flags) };
        if rc != 0 {
            return Err(format!("invalid regular expression: {pattern}"));
        }
        Ok(Self {
            raw: unsafe { raw.assume_init() },
        })
    }

    fn find(&self, text: &str, start: usize) -> Result<Option<(usize, usize)>, Vec<AppletError>> {
        let haystack = &text[start..];
        let c_text = CString::new(haystack).map_err(|_| {
            vec![AppletError::new(
                APPLET,
                "input contains NUL byte, unsupported by awk",
            )]
        })?;
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

    fn replace_all(
        &self,
        text: &mut String,
        replacement: &str,
        global: bool,
    ) -> Result<usize, Vec<AppletError>> {
        let original = text.clone();
        let mut result = String::new();
        let mut start = 0usize;
        let mut count = 0usize;
        while let Some((match_start, match_end)) = self.find(&original, start)? {
            count += 1;
            result.push_str(&original[start..match_start]);
            result.push_str(&expand_replacement(
                replacement,
                &original[match_start..match_end],
            ));
            if match_end == match_start {
                if let Some(next) = original[match_end..].chars().next() {
                    result.push(next);
                    start = match_end + next.len_utf8();
                } else {
                    start = match_end;
                    break;
                }
            } else {
                start = match_end;
            }
            if !global {
                break;
            }
        }
        if count == 0 {
            return Ok(0);
        }
        result.push_str(&original[start..]);
        *text = result;
        Ok(count)
    }
}

fn expand_replacement(replacement: &str, matched: &str) -> String {
    let mut result = String::new();
    let mut escaped = false;
    for ch in replacement.chars() {
        if escaped {
            result.push(ch);
            escaped = false;
            continue;
        }
        match ch {
            '\\' => escaped = true,
            '&' => result.push_str(matched),
            other => result.push(other),
        }
    }
    if escaped {
        result.push('\\');
    }
    result
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
            let program = if path == "-" {
                let mut stdin = String::new();
                io::stdin().read_to_string(&mut stdin).map_err(|err| {
                    vec![AppletError::new(APPLET, format!("reading stdin: {err}"))]
                })?;
                stdin
            } else {
                fs::read_to_string(path)
                    .map_err(|err| vec![AppletError::from_io(APPLET, "reading", Some(path), err)])?
            };
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
    pattern: Option<Expr>,
    default_print: bool,
    action: Vec<Stmt>,
}

#[derive(Debug, Clone)]
enum Stmt {
    Print(Vec<Expr>),
    Printf(Expr, Vec<Expr>),
    Assign(LValue, AssignOp, Expr),
    If(Expr, Box<Stmt>, Option<Box<Stmt>>),
    While(Expr, Box<Stmt>),
    DoWhile(Box<Stmt>, Expr),
    ForLoop(Option<Box<Stmt>>, Option<Expr>, Option<Expr>, Box<Stmt>),
    ForIn(String, String, Box<Stmt>),
    Delete(String, Expr),
    Return(Option<Expr>),
    Break,
    Continue,
    Exit(Option<Expr>),
    Next,
    NextFile,
    Getline(Option<String>, Option<Expr>),
    Block(Vec<Stmt>),
    Expr(Expr),
}

#[derive(Debug, Clone)]
enum Expr {
    Number(f64),
    String(String),
    Regex(String),
    Getline(Option<String>, Option<Box<Expr>>),
    Var(String),
    ArrayGet(String, Box<Expr>),
    Field(Box<Expr>),
    UnaryNot(Box<Expr>),
    UnaryMinus(Box<Expr>),
    UnaryPlus(Box<Expr>),
    PreInc(LValue),
    PreDec(LValue),
    PostInc(LValue),
    PostDec(LValue),
    Conditional(Box<Expr>, Box<Expr>, Box<Expr>),
    Binary(BinaryOp, Box<Expr>, Box<Expr>),
    Call(String, Vec<Expr>),
}

#[derive(Debug, Clone)]
enum LValue {
    Var(String),
    Array(String, Box<Expr>),
}

#[derive(Debug, Clone)]
struct Function {
    params: Vec<String>,
    body: Vec<Stmt>,
}

#[derive(Debug, Clone, Copy)]
enum BinaryOp {
    Or,
    And,
    Pow,
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
    Match,
    NotMatch,
    Concat,
}

#[derive(Debug, Clone, Copy)]
enum AssignOp {
    Set,
    Add,
    Sub,
    Mul,
    Div,
    Mod,
    Pow,
}

#[derive(Debug, Clone)]
enum Token {
    Begin,
    End,
    Function,
    Return,
    If,
    While,
    Do,
    Else,
    Break,
    Continue,
    Exit,
    Next,
    Nextfile,
    Getline,
    For,
    In,
    Delete,
    Print,
    Printf,
    Ident(String),
    Number(f64),
    String(String),
    LBrace,
    RBrace,
    LParen,
    RParen,
    LBracket,
    RBracket,
    Comma,
    Semicolon,
    Question,
    Colon,
    Dollar,
    Assign,
    AddAssign,
    SubAssign,
    MulAssign,
    DivAssign,
    ModAssign,
    PowAssign,
    Eq,
    Ne,
    Match,
    NotMatch,
    AndAnd,
    OrOr,
    Lt,
    Le,
    Gt,
    Ge,
    Caret,
    PlusPlus,
    MinusMinus,
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
                    pattern: None,
                    default_print: false,
                    action: parser.parse_block()?,
                });
            }
            Token::End => {
                parser.bump()?;
                end_rules.push(Rule {
                    pattern: None,
                    default_print: false,
                    action: parser.parse_block()?,
                });
            }
            Token::LBrace => {
                main_rules.push(Rule {
                    pattern: None,
                    default_print: false,
                    action: parser.parse_block()?,
                });
            }
            Token::Eof => break,
            _ => {
                let pattern = parser.parse_expr()?;
                let default_print = !matches!(parser.current, Token::LBrace);
                let action = if default_print {
                    Vec::new()
                } else {
                    parser.parse_block()?
                };
                main_rules.push(Rule {
                    pattern: Some(pattern),
                    default_print,
                    action,
                });
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
                if matches!(self.current, Token::LParen) {
                    self.bump()?;
                    if matches!(self.current, Token::RParen) {
                        return Err(vec![AppletError::new(APPLET, "empty sequence")]);
                    }
                    exprs.push(self.parse_expr()?);
                    self.expect(Token::RParen)?;
                    while matches!(self.current, Token::Comma) {
                        self.bump()?;
                        exprs.push(self.parse_expr()?);
                    }
                } else if !matches!(self.current, Token::Semicolon | Token::RBrace | Token::Eof) {
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
                self.skip_separators()?;
                let then_branch = self.parse_stmt()?;
                self.skip_separators()?;
                let else_branch = if matches!(self.current, Token::Else) {
                    self.bump()?;
                    self.skip_separators()?;
                    Some(Box::new(self.parse_stmt()?))
                } else {
                    None
                };
                Ok(Stmt::If(condition, Box::new(then_branch), else_branch))
            }
            Token::While => {
                self.bump()?;
                self.expect(Token::LParen)?;
                let condition = self.parse_expr()?;
                self.expect(Token::RParen)?;
                self.skip_separators()?;
                Ok(Stmt::While(condition, Box::new(self.parse_stmt()?)))
            }
            Token::Do => {
                self.bump()?;
                self.skip_separators()?;
                let body = self.parse_stmt()?;
                self.skip_separators()?;
                self.expect(Token::While)?;
                self.expect(Token::LParen)?;
                let condition = self.parse_expr()?;
                self.expect(Token::RParen)?;
                Ok(Stmt::DoWhile(Box::new(body), condition))
            }
            Token::Break => {
                self.bump()?;
                Ok(Stmt::Break)
            }
            Token::Continue => {
                self.bump()?;
                Ok(Stmt::Continue)
            }
            Token::Next => {
                self.bump()?;
                Ok(Stmt::Next)
            }
            Token::Nextfile => {
                self.bump()?;
                Ok(Stmt::NextFile)
            }
            Token::Exit => {
                self.bump()?;
                if matches!(self.current, Token::Semicolon | Token::RBrace | Token::Eof) {
                    Ok(Stmt::Exit(None))
                } else {
                    Ok(Stmt::Exit(Some(self.parse_expr()?)))
                }
            }
            Token::Getline => {
                self.bump()?;
                let (target, source) = self.parse_getline_parts()?;
                Ok(Stmt::Getline(target, source))
            }
            Token::For => {
                self.bump()?;
                self.expect(Token::LParen)?;
                if let Token::Ident(var) = &self.current {
                    let var = var.clone();
                    self.bump()?;
                    if matches!(self.current, Token::In) {
                        self.bump()?;
                        let Token::Ident(array) = &self.current else {
                            return Err(vec![AppletError::new(APPLET, "unexpected token")]);
                        };
                        let array = array.clone();
                        self.bump()?;
                        self.expect(Token::RParen)?;
                        self.skip_separators()?;
                        return Ok(Stmt::ForIn(var, array, Box::new(self.parse_stmt()?)));
                    }
                    let init_stmt = self.parse_stmt_with_ident(var)?;
                    self.expect(Token::Semicolon)?;
                    let condition = if matches!(self.current, Token::Semicolon) {
                        None
                    } else {
                        Some(self.parse_expr()?)
                    };
                    self.expect(Token::Semicolon)?;
                    let step = if matches!(self.current, Token::RParen) {
                        None
                    } else {
                        Some(self.parse_expr()?)
                    };
                    self.expect(Token::RParen)?;
                    self.skip_separators()?;
                    return Ok(Stmt::ForLoop(
                        Some(Box::new(init_stmt)),
                        condition,
                        step,
                        Box::new(self.parse_stmt()?),
                    ));
                }
                let init = if matches!(self.current, Token::Semicolon) {
                    None
                } else {
                    Some(Box::new(Stmt::Expr(self.parse_expr()?)))
                };
                self.expect(Token::Semicolon)?;
                let condition = if matches!(self.current, Token::Semicolon) {
                    None
                } else {
                    Some(self.parse_expr()?)
                };
                self.expect(Token::Semicolon)?;
                let step = if matches!(self.current, Token::RParen) {
                    None
                } else {
                    Some(self.parse_expr()?)
                };
                self.expect(Token::RParen)?;
                self.skip_separators()?;
                Ok(Stmt::ForLoop(
                    init,
                    condition,
                    step,
                    Box::new(self.parse_stmt()?),
                ))
            }
            Token::Delete => {
                self.bump()?;
                let Token::Ident(name) = &self.current else {
                    return Err(vec![AppletError::new(APPLET, "too few arguments")]);
                };
                let name = name.clone();
                self.bump()?;
                self.expect(Token::LBracket)?;
                let index = self.parse_expr()?;
                self.expect(Token::RBracket)?;
                Ok(Stmt::Delete(name, index))
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
                self.parse_stmt_with_ident(name)
            }
            _ => Ok(Stmt::Expr(self.parse_expr()?)),
        }
    }

    fn parse_expr(&mut self) -> Result<Expr, Vec<AppletError>> {
        self.parse_conditional()
    }

    fn parse_stmt_with_ident(&mut self, name: String) -> Result<Stmt, Vec<AppletError>> {
        let prefix = self.parse_postfix_expr(Expr::Var(name))?;
        if let Some(assign_op) = assign_token_to_op(&self.current) {
            let Some(target) = expr_to_lvalue(&prefix) else {
                return Err(vec![AppletError::new(APPLET, "unexpected token")]);
            };
            self.bump()?;
            Ok(Stmt::Assign(target, assign_op, self.parse_expr()?))
        } else {
            let expr = self.parse_expr_with_prefix(prefix)?;
            Ok(Stmt::Expr(expr))
        }
    }

    fn parse_expr_with_prefix(&mut self, prefix: Expr) -> Result<Expr, Vec<AppletError>> {
        let left = self.parse_concat_tail(prefix)?;
        let left = self.parse_comparison_tail(left)?;
        let left = self.parse_logical_and_tail(left)?;
        let left = self.parse_logical_or_tail(left)?;
        self.parse_conditional_tail(left)
    }

    fn parse_conditional(&mut self) -> Result<Expr, Vec<AppletError>> {
        let left = self.parse_logical_or()?;
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
        Ok(Expr::Conditional(
            Box::new(left),
            Box::new(yes),
            Box::new(no),
        ))
    }

    fn parse_logical_or(&mut self) -> Result<Expr, Vec<AppletError>> {
        let left = self.parse_logical_and()?;
        self.parse_logical_or_tail(left)
    }

    fn parse_logical_or_tail(&mut self, mut left: Expr) -> Result<Expr, Vec<AppletError>> {
        while matches!(self.current, Token::OrOr) {
            self.bump()?;
            let right = self.parse_logical_and()?;
            left = Expr::Binary(BinaryOp::Or, Box::new(left), Box::new(right));
        }
        Ok(left)
    }

    fn parse_logical_and(&mut self) -> Result<Expr, Vec<AppletError>> {
        let left = self.parse_concat()?;
        let left = self.parse_comparison_tail(left)?;
        self.parse_logical_and_tail(left)
    }

    fn parse_logical_and_tail(&mut self, mut left: Expr) -> Result<Expr, Vec<AppletError>> {
        while matches!(self.current, Token::AndAnd) {
            self.bump()?;
            let right = self.parse_concat()?;
            let right = self.parse_comparison_tail(right)?;
            left = Expr::Binary(BinaryOp::And, Box::new(left), Box::new(right));
        }
        Ok(left)
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
                Token::Match => BinaryOp::Match,
                Token::NotMatch => BinaryOp::NotMatch,
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
        let mut left = self.parse_power()?;
        loop {
            let op = match self.current {
                Token::Star => BinaryOp::Mul,
                Token::Slash => BinaryOp::Div,
                Token::Percent => BinaryOp::Mod,
                _ => break,
            };
            self.bump()?;
            let right = self.parse_power()?;
            left = Expr::Binary(op, Box::new(left), Box::new(right));
        }
        Ok(left)
    }

    fn parse_power(&mut self) -> Result<Expr, Vec<AppletError>> {
        let left = self.parse_unary()?;
        if matches!(self.current, Token::Caret) {
            self.bump()?;
            let right = self.parse_power()?;
            Ok(Expr::Binary(BinaryOp::Pow, Box::new(left), Box::new(right)))
        } else {
            Ok(left)
        }
    }

    fn parse_unary(&mut self) -> Result<Expr, Vec<AppletError>> {
        match self.current {
            Token::Ne => {
                self.bump()?;
                Ok(Expr::UnaryNot(Box::new(self.parse_unary()?)))
            }
            Token::PlusPlus => {
                self.bump()?;
                let expr = self.parse_unary()?;
                let Some(target) = expr_to_lvalue(&expr) else {
                    return Err(vec![AppletError::new(APPLET, "unexpected token")]);
                };
                Ok(Expr::PreInc(target))
            }
            Token::MinusMinus => {
                self.bump()?;
                let expr = self.parse_unary()?;
                let Some(target) = expr_to_lvalue(&expr) else {
                    return Err(vec![AppletError::new(APPLET, "unexpected token")]);
                };
                Ok(Expr::PreDec(target))
            }
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
            _ => self.parse_postfix(),
        }
    }

    fn parse_postfix(&mut self) -> Result<Expr, Vec<AppletError>> {
        let primary = self.parse_primary()?;
        self.parse_postfix_expr(primary)
    }

    fn parse_postfix_expr(&mut self, mut expr: Expr) -> Result<Expr, Vec<AppletError>> {
        loop {
            match (&expr, &self.current) {
                (Expr::Var(name), Token::LParen) if !self.current_had_space => {
                    let name = name.clone();
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
                    expr = Expr::Call(name, args);
                }
                (Expr::Var(name), Token::LBracket) => {
                    let name = name.clone();
                    self.bump()?;
                    let index = self.parse_expr()?;
                    self.expect(Token::RBracket)?;
                    expr = Expr::ArrayGet(name, Box::new(index));
                }
                (_, Token::PlusPlus) => {
                    let Some(target) = expr_to_lvalue(&expr) else {
                        return Err(vec![AppletError::new(APPLET, "unexpected token")]);
                    };
                    self.bump()?;
                    expr = Expr::PostInc(target);
                }
                (_, Token::MinusMinus) => {
                    let Some(target) = expr_to_lvalue(&expr) else {
                        return Err(vec![AppletError::new(APPLET, "unexpected token")]);
                    };
                    self.bump()?;
                    expr = Expr::PostDec(target);
                }
                _ => break,
            }
        }
        Ok(expr)
    }

    fn parse_primary(&mut self) -> Result<Expr, Vec<AppletError>> {
        let expr = match &self.current {
            Token::Number(value) => {
                let value = *value;
                self.bump()?;
                Expr::Number(value)
            }
            Token::String(value) => {
                let value = value.clone();
                self.bump()?;
                Expr::String(value)
            }
            Token::Slash => {
                let pattern = self.lexer.read_regex_literal()?;
                let (current, had_space) = self.lexer.next_token()?;
                self.current = current;
                self.current_had_space = had_space;
                Expr::Regex(pattern)
            }
            Token::Ident(name) => {
                let name = name.clone();
                self.bump()?;
                Expr::Var(name)
            }
            Token::LParen => {
                self.bump()?;
                let expr = self.parse_expr()?;
                self.expect(Token::RParen)?;
                expr
            }
            Token::Getline => {
                self.bump()?;
                let (target, source) = self.parse_getline_parts()?;
                Expr::Getline(target, source.map(Box::new))
            }
            _ => {
                return Err(vec![AppletError::new(
                    APPLET,
                    "unexpected token in expression",
                )]);
            }
        };
        Ok(expr)
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
                    return Err(vec![AppletError::new(
                        APPLET,
                        "expected function parameter",
                    )]);
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

    fn parse_getline_parts(&mut self) -> Result<(Option<String>, Option<Expr>), Vec<AppletError>> {
        let target = match &self.current {
            Token::Ident(name) => {
                let name = name.clone();
                self.bump()?;
                Some(name)
            }
            _ => None,
        };
        let source = if matches!(self.current, Token::Lt) {
            self.bump()?;
            Some(self.parse_expr()?)
        } else {
            None
        };
        Ok((target, source))
    }
}

fn token_starts_expr(token: &Token) -> bool {
    matches!(
        token,
        Token::Number(_)
            | Token::String(_)
            | Token::Ident(_)
            | Token::LParen
            | Token::Slash
            | Token::Dollar
            | Token::Getline
            | Token::Ne
            | Token::Plus
            | Token::Minus
            | Token::PlusPlus
            | Token::MinusMinus
    )
}

fn expr_to_lvalue(expr: &Expr) -> Option<LValue> {
    match expr {
        Expr::Var(name) => Some(LValue::Var(name.clone())),
        Expr::ArrayGet(name, index) => Some(LValue::Array(name.clone(), index.clone())),
        _ => None,
    }
}

fn assign_token_to_op(token: &Token) -> Option<AssignOp> {
    Some(match token {
        Token::Assign => AssignOp::Set,
        Token::AddAssign => AssignOp::Add,
        Token::SubAssign => AssignOp::Sub,
        Token::MulAssign => AssignOp::Mul,
        Token::DivAssign => AssignOp::Div,
        Token::ModAssign => AssignOp::Mod,
        Token::PowAssign => AssignOp::Pow,
        _ => return None,
    })
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
            '[' => {
                self.index += 1;
                Ok((Token::LBracket, had_space))
            }
            ']' => {
                self.index += 1;
                Ok((Token::RBracket, had_space))
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
            '^' => {
                self.index += 1;
                if self.peek() == Some('=') {
                    self.index += 1;
                    Ok((Token::PowAssign, had_space))
                } else {
                    Ok((Token::Caret, had_space))
                }
            }
            '+' => {
                self.index += 1;
                if self.peek() == Some('+') {
                    self.index += 1;
                    Ok((Token::PlusPlus, had_space))
                } else if self.peek() == Some('=') {
                    self.index += 1;
                    Ok((Token::AddAssign, had_space))
                } else {
                    Ok((Token::Plus, had_space))
                }
            }
            '-' => {
                self.index += 1;
                if self.peek() == Some('-') {
                    self.index += 1;
                    Ok((Token::MinusMinus, had_space))
                } else if self.peek() == Some('=') {
                    self.index += 1;
                    Ok((Token::SubAssign, had_space))
                } else {
                    Ok((Token::Minus, had_space))
                }
            }
            '*' => {
                self.index += 1;
                if self.peek() == Some('=') {
                    self.index += 1;
                    Ok((Token::MulAssign, had_space))
                } else {
                    Ok((Token::Star, had_space))
                }
            }
            '/' => {
                self.index += 1;
                if self.peek() == Some('=') {
                    self.index += 1;
                    Ok((Token::DivAssign, had_space))
                } else {
                    Ok((Token::Slash, had_space))
                }
            }
            '%' => {
                self.index += 1;
                if self.peek() == Some('=') {
                    self.index += 1;
                    Ok((Token::ModAssign, had_space))
                } else {
                    Ok((Token::Percent, had_space))
                }
            }
            '&' => {
                self.index += 1;
                if self.peek() == Some('&') {
                    self.index += 1;
                    Ok((Token::AndAnd, had_space))
                } else {
                    Err(vec![AppletError::new(APPLET, "unsupported operator")])
                }
            }
            '|' => {
                self.index += 1;
                if self.peek() == Some('|') {
                    self.index += 1;
                    Ok((Token::OrOr, had_space))
                } else {
                    Err(vec![AppletError::new(APPLET, "unsupported operator")])
                }
            }
            '~' => {
                self.index += 1;
                Ok((Token::Match, had_space))
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
                } else if self.peek() == Some('~') {
                    self.index += 1;
                    Ok((Token::NotMatch, had_space))
                } else {
                    Ok((Token::Ne, had_space))
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
            '"' => self
                .read_string()
                .map(|token| (Token::String(token), had_space)),
            ch if ch.is_ascii_digit() => self
                .read_number()
                .map(|token| (Token::Number(token), had_space)),
            ch if is_ident_start(ch) => {
                let ident = self.read_identifier();
                Ok(match ident.as_str() {
                    "BEGIN" => (Token::Begin, had_space),
                    "END" => (Token::End, had_space),
                    "function" | "func" => (Token::Function, had_space),
                    "return" => (Token::Return, had_space),
                    "if" => (Token::If, had_space),
                    "while" => (Token::While, had_space),
                    "do" => (Token::Do, had_space),
                    "else" => (Token::Else, had_space),
                    "break" => (Token::Break, had_space),
                    "continue" => (Token::Continue, had_space),
                    "exit" => (Token::Exit, had_space),
                    "next" => (Token::Next, had_space),
                    "nextfile" => (Token::Nextfile, had_space),
                    "getline" => (Token::Getline, had_space),
                    "for" => (Token::For, had_space),
                    "in" => (Token::In, had_space),
                    "delete" => (Token::Delete, had_space),
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
        loop {
            match self.peek() {
                Some(' ' | '\t' | '\r') => self.index += 1,
                // Backslash-newline is a line continuation, treated as whitespace.
                // autoconf config.status generates awk programs with this pattern
                // for long substitution variable values split across 148-char lines.
                Some('\\') if self.peek_n(1) == Some('\n') => self.index += 2,
                _ => break,
            }
        }
        self.index != start
    }

    fn skip_comment(&mut self) {
        while let Some(ch) = self.peek() {
            if ch == '\n' {
                break; // leave \n for tokenizer to emit Token::Semicolon
            }
            self.index += 1;
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

    fn read_regex_literal(&mut self) -> Result<String, Vec<AppletError>> {
        let mut value = String::new();
        while let Some(ch) = self.peek() {
            self.index += ch.len_utf8();
            match ch {
                '/' => return Ok(value),
                '\\' => {
                    let escaped = self
                        .next_char()
                        .ok_or_else(|| vec![AppletError::new(APPLET, "unterminated regex")])?;
                    value.push('\\');
                    value.push(escaped);
                }
                _ => value.push(ch),
            }
        }
        Err(vec![AppletError::new(APPLET, "unterminated regex")])
    }

    fn read_number(&mut self) -> Result<f64, Vec<AppletError>> {
        let start = self.index;
        if self.source[self.index..].starts_with("0x")
            || self.source[self.index..].starts_with("0X")
        {
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

fn parse_escape_char(
    index: &mut usize,
    escaped: char,
    source: &str,
) -> Result<char, Vec<AppletError>> {
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
                .map_err(|_| vec![AppletError::new(APPLET, "invalid hex escape")])?
                as char
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
        let program =
            parse_program("BEGIN { print 1 }\n{ print $1 }\nEND { print 2 }").expect("program");
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

    #[test]
    fn backslash_newline_continuation_in_program() {
        // autoconf config.status generates awk programs with backslash-newline
        // line continuations when substitution variable values exceed 148 chars,
        // producing strings like: S["DEFS"]="first148chars"\<newline>"rest"
        let program = parse_program("BEGIN {\n  x = \"part1\"\\\n\"part2\"\n}").expect("program");
        assert_eq!(program.begin_rules.len(), 1);
    }
}
