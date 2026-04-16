use std::path::Path;

use crate::common::applet::{AppletResult, finish};
use crate::common::args::argv_to_strings;
use crate::common::error::AppletError;
use crate::common::modules::finit_module;

const APPLET: &str = "insmod";

pub fn main(args: &[std::ffi::OsString]) -> i32 {
    finish(run(args))
}

fn run(args: &[std::ffi::OsString]) -> AppletResult {
    let args = argv_to_strings(APPLET, args)?;
    let (path, params) = parse_args(&args)?;
    run_linux(&path, &params)
}

fn parse_args(args: &[String]) -> Result<(String, Vec<String>), Vec<AppletError>> {
    let Some(path) = args.first() else {
        return Err(vec![AppletError::new(APPLET, "missing operand")]);
    };
    Ok((path.clone(), args[1..].to_vec()))
}

fn run_linux(path: &str, params: &[String]) -> AppletResult {
    let params = params
        .iter()
        .cloned()
        .map(std::ffi::OsString::from)
        .collect::<Vec<_>>();
    finit_module(Path::new(path), &params)
        .map_err(|err| vec![AppletError::new(APPLET, err.to_string())])
}

#[cfg(test)]
mod tests {
    use super::parse_args;

    fn args(values: &[&str]) -> Vec<String> {
        values.iter().map(|value| value.to_string()).collect()
    }

    #[test]
    fn parses_path_and_params() {
        let (path, params) =
            parse_args(&args(&["module.ko", "key=value", "other=1"])).expect("parse insmod");
        assert_eq!(path, "module.ko");
        assert_eq!(
            params,
            vec![String::from("key=value"), String::from("other=1")]
        );
    }

    #[test]
    fn missing_operand_errors() {
        assert!(parse_args(&[]).is_err());
    }
}
