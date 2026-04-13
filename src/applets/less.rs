use crate::common::applet::{AppletResult, finish};
use crate::common::error::AppletError;
use crate::common::io::{copy_stream, open_input, stdout};

const APPLET: &str = "less";

pub fn main(args: &[String]) -> i32 {
    finish(run(args))
}

fn run(args: &[String]) -> AppletResult {
    let inputs = parse_args(args)?;
    let mut out = stdout();
    let inputs: Vec<&str> = if inputs.is_empty() {
        vec!["-"]
    } else {
        inputs.iter().map(String::as_str).collect()
    };

    let mut errors = Vec::new();
    for path in inputs {
        match open_input(path) {
            Ok(mut input) => {
                if let Err(err) = copy_stream(&mut input, &mut out) {
                    errors.push(AppletError::from_io(APPLET, "reading", Some(path), err));
                }
            }
            Err(err) => errors.push(AppletError::from_io(APPLET, "opening", Some(path), err)),
        }
    }

    if let Err(err) = std::io::Write::flush(&mut out) {
        errors.push(AppletError::from_io(APPLET, "writing stdout", None, err));
    }

    if errors.is_empty() {
        Ok(())
    } else {
        Err(errors)
    }
}

fn parse_args(args: &[String]) -> Result<Vec<String>, Vec<AppletError>> {
    let mut files = Vec::new();
    let mut parsing_flags = true;

    for arg in args {
        if parsing_flags && arg == "--" {
            parsing_flags = false;
            continue;
        }
        if parsing_flags && arg.starts_with('-') && arg.len() > 1 {
            let flag = arg.chars().nth(1).expect("short option has flag");
            return Err(vec![AppletError::invalid_option(APPLET, flag)]);
        }
        files.push(arg.clone());
    }

    Ok(files)
}

#[cfg(test)]
mod tests {
    use super::parse_args;

    fn args(values: &[&str]) -> Vec<String> {
        values.iter().map(|value| value.to_string()).collect()
    }

    #[test]
    fn parses_paths() {
        assert_eq!(
            parse_args(&args(&["one", "two"])).expect("parse less"),
            vec![String::from("one"), String::from("two")]
        );
    }

    #[test]
    fn rejects_unknown_flags() {
        assert!(parse_args(&args(&["-x"])).is_err());
    }
}
