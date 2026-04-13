use std::collections::HashMap;
use std::io::Write;

const APPLET: &str = "getopt";

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
    let Some((optstring, params)) = args.split_first() else {
        return Err(String::from("missing operand"));
    };

    let spec = parse_optstring(optstring)?;
    let (options, positionals, ok) = parse_params(&spec, params);

    let mut out = std::io::stdout().lock();
    for token in options {
        write!(out, " {token}").map_err(|err| format!("writing stdout: {err}"))?;
    }
    write!(out, " --").map_err(|err| format!("writing stdout: {err}"))?;
    for token in positionals {
        write!(out, " {token}").map_err(|err| format!("writing stdout: {err}"))?;
    }
    writeln!(out).map_err(|err| format!("writing stdout: {err}"))?;

    Ok(if ok { 0 } else { 1 })
}

fn parse_optstring(optstring: &str) -> Result<HashMap<char, bool>, String> {
    let mut spec = HashMap::new();
    let chars: Vec<char> = optstring.chars().collect();
    let mut index = 0;
    while index < chars.len() {
        let ch = chars[index];
        if ch == ':' {
            return Err(String::from("invalid optstring"));
        }
        let requires_arg = chars.get(index + 1) == Some(&':');
        spec.insert(ch, requires_arg);
        index += 1 + usize::from(requires_arg);
    }
    Ok(spec)
}

fn parse_params(spec: &HashMap<char, bool>, params: &[String]) -> (Vec<String>, Vec<String>, bool) {
    let mut options = Vec::new();
    let mut positionals = Vec::new();
    let mut index = 0;

    while index < params.len() {
        let arg = &params[index];
        if arg == "--" {
            positionals.extend(params[index + 1..].iter().cloned());
            break;
        }
        if !arg.starts_with('-') || arg == "-" {
            positionals.push(arg.clone());
            index += 1;
            continue;
        }

        let mut chars = arg[1..].char_indices().peekable();
        while let Some((offset, flag)) = chars.next() {
            let Some(&requires_arg) = spec.get(&flag) else {
                eprintln!("{APPLET}: illegal option -- {flag}");
                return (options, positionals, false);
            };
            options.push(format!("-{flag}"));
            if requires_arg {
                let value_start = offset + 2;
                if value_start < arg.len() {
                    options.push(arg[value_start..].to_string());
                } else if let Some(value) = params.get(index + 1) {
                    options.push(value.clone());
                    index += 1;
                } else {
                    eprintln!("{APPLET}: option requires an argument -- {flag}");
                    return (options, positionals, false);
                }
                break;
            }
            if chars.peek().is_none() {}
        }
        index += 1;
    }

    (options, positionals, true)
}

#[cfg(test)]
mod tests {
    use super::{parse_optstring, parse_params};

    fn args(values: &[&str]) -> Vec<String> {
        values.iter().map(|value| value.to_string()).collect()
    }

    #[test]
    fn parses_short_options_and_positionals() {
        let spec = parse_optstring("ab:c").expect("spec");
        let (options, positionals, ok) = parse_params(&spec, &args(&["-a", "-b", "foo", "bar"]));
        assert!(ok);
        assert_eq!(
            options,
            vec![String::from("-a"), String::from("-b"), String::from("foo")]
        );
        assert_eq!(positionals, vec![String::from("bar")]);
    }

    #[test]
    fn parses_attached_argument() {
        let spec = parse_optstring("ab:c").expect("spec");
        let (options, positionals, ok) = parse_params(&spec, &args(&["-abfoo", "--", "bar"]));
        assert!(ok);
        assert_eq!(
            options,
            vec![String::from("-a"), String::from("-b"), String::from("foo")]
        );
        assert_eq!(positionals, vec![String::from("bar")]);
    }
}
