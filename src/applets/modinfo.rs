use std::path::{Path, PathBuf};

use crate::common::applet::{AppletResult, finish};
use crate::common::args::{ParsedArg, Parser};
use crate::common::error::AppletError;
use crate::common::modules::{ModuleIndex, module_tree_dir, read_module_metadata};

const APPLET: &str = "modinfo";

#[derive(Clone, Debug, Default, Eq, PartialEq)]
struct Options {
    separator: char,
    fields: Vec<String>,
    modules: Vec<String>,
}

pub fn main(args: &[std::ffi::OsString]) -> i32 {
    finish(run(args))
}

fn run(args: &[std::ffi::OsString]) -> AppletResult {
    let options = parse_args(args)?;
    let root = module_tree_dir(None).map_err(|err| vec![map_module_error(err)])?;
    let index = ModuleIndex::scan(&root)
        .map_err(|err| vec![AppletError::from_io(APPLET, "reading", Some(&root.to_string_lossy()), err)])?;

    for module in &options.modules {
        let (path, metadata) = resolve_module(&index, module)?;
        print_module(&options, &path, &metadata);
    }
    Ok(())
}

fn parse_args(args: &[std::ffi::OsString]) -> Result<Options, Vec<AppletError>> {
    let mut options = Options {
        separator: '\n',
        ..Options::default()
    };
    let mut parser = Parser::new(APPLET, args);
    while let Some(arg) = parser.next_arg()? {
        match arg {
            ParsedArg::Short('0') => options.separator = '\0',
            ParsedArg::Short('a') => options.fields.push(String::from("author")),
            ParsedArg::Short('d') => options.fields.push(String::from("description")),
            ParsedArg::Short('l') => options.fields.push(String::from("license")),
            ParsedArg::Short('p') => options.fields.push(String::from("parm")),
            ParsedArg::Short('n') => options.fields.push(String::from("filename")),
            ParsedArg::Short('F') => options.fields.push(parser.value_str("F")?),
            ParsedArg::Short(flag) => return Err(vec![AppletError::invalid_option(APPLET, flag)]),
            ParsedArg::Long(name) => {
                return Err(vec![AppletError::unrecognized_option(
                    APPLET,
                    &format!("--{name}"),
                )])
            }
            ParsedArg::Value(arg) => options.modules.push(arg.into_string().map_err(|arg| {
                vec![AppletError::new(
                    APPLET,
                    format!("argument is invalid unicode: {:?}", arg),
                )]
            })?),
        }
    }

    if options.modules.is_empty() {
        return Err(vec![AppletError::new(APPLET, "missing operand")]);
    }
    Ok(options)
}

fn resolve_module(index: &ModuleIndex, module: &str) -> Result<(PathBuf, crate::common::modules::ModuleMetadata), Vec<AppletError>> {
    let path = if Path::new(module).exists() {
        PathBuf::from(module)
    } else if let Some(entry) = index.get(module) {
        entry.path.clone()
    } else {
        return Err(vec![AppletError::new(
            APPLET,
            format!("module '{module}' not found"),
        )]);
    };

    let metadata = read_module_metadata(&path)
        .map_err(|err| vec![AppletError::from_io(APPLET, "reading", Some(&path.to_string_lossy()), err)])?;
    Ok((path, metadata))
}

fn print_module(options: &Options, path: &Path, metadata: &crate::common::modules::ModuleMetadata) {
    let selected = if options.fields.is_empty() {
        None
    } else {
        Some(options.fields.clone())
    };

    match selected {
        Some(fields) => {
            for field in fields {
                if field == "filename" {
                    print_value(options, "filename", &path.to_string_lossy());
                    continue;
                }
                for value in metadata.values(&field) {
                    print_value(options, &field, value);
                }
            }
        }
        None => {
            print_value(options, "filename", &path.to_string_lossy());
            for (field, value) in metadata.fields() {
                print_value(options, field, value);
            }
        }
    }
}

fn print_value(options: &Options, field: &str, value: &str) {
    if options.fields.len() <= 1 {
        print!("{value}{}", options.separator);
    } else {
        print!("{field}: {value}{}", options.separator);
    }
}

fn map_module_error(error: AppletError) -> AppletError {
    AppletError::new(APPLET, error.to_string())
}

#[cfg(test)]
mod tests {
    use super::{Options, parse_args};

    #[test]
    fn parses_shortcuts_and_field_option() {
        assert_eq!(
            parse_args(&[
                "-0".into(),
                "-n".into(),
                "-F".into(),
                "description".into(),
                "loop".into(),
            ])
            .expect("modinfo args"),
            Options {
                separator: '\0',
                fields: vec![String::from("filename"), String::from("description")],
                modules: vec![String::from("loop")],
            }
        );
    }
}
