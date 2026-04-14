use std::collections::HashSet;

use crate::common::applet::{AppletResult, finish};
use crate::common::error::AppletError;
use crate::common::modules::{ModuleEntry, ModuleIndex, delete_module, finit_module, module_tree_dir};

const APPLET: &str = "modprobe";

#[derive(Clone, Debug, Default, Eq, PartialEq)]
struct Options {
    remove: bool,
    quiet: bool,
    dry_run: bool,
    module: Option<String>,
    params: Vec<String>,
}

pub fn main(args: &[String]) -> i32 {
    finish(run(args))
}

fn run(args: &[String]) -> AppletResult {
    let options = parse_args(args)?;
    let root = module_tree_dir(None).map_err(|err| vec![map_module_error(err)])?;
    let index = ModuleIndex::scan(&root)
        .map_err(|err| vec![AppletError::from_io(APPLET, "reading", Some(&root.to_string_lossy()), err)])?;
    let module = options.module.as_deref().unwrap_or_default();

    if options.remove {
        remove_module(&index, module, options.quiet, options.dry_run)
    } else {
        insert_module_request(&index, module, &options.params, options.quiet, options.dry_run)
    }
}

fn parse_args(args: &[String]) -> Result<Options, Vec<AppletError>> {
    let mut options = Options::default();
    let mut parsing_flags = true;

    for arg in args {
        if parsing_flags && arg == "--" {
            parsing_flags = false;
            continue;
        }
        if parsing_flags && arg.starts_with('-') && arg.len() > 1 {
            for flag in arg[1..].chars() {
                match flag {
                    'r' => options.remove = true,
                    'q' => options.quiet = true,
                    'n' => options.dry_run = true,
                    _ => return Err(vec![AppletError::invalid_option(APPLET, flag)]),
                }
            }
            continue;
        }
        if options.module.is_none() {
            options.module = Some(arg.clone());
        } else {
            options.params.push(arg.clone());
        }
    }

    if options.module.is_none() {
        return Err(vec![AppletError::new(APPLET, "missing operand")]);
    }
    Ok(options)
}

fn insert_module_request(
    index: &ModuleIndex,
    request: &str,
    params: &[String],
    quiet: bool,
    dry_run: bool,
) -> AppletResult {
    if index.is_builtin(request) {
        return Ok(());
    }
    let Some(entry) = index.resolve_alias(request) else {
        return if quiet {
            Ok(())
        } else {
            Err(vec![AppletError::new(
                APPLET,
                format!("module '{request}' not found"),
            )])
        };
    };

    let mut order = Vec::new();
    let mut seen = HashSet::new();
    dependency_order(index, entry, &mut seen, &mut order)?;

    for module in order {
        if dry_run {
            continue;
        }
        let module_params = if module.name == entry.name { params } else { &[] };
        finit_module(&module.path, module_params).map_err(wrap_module_error)?;
    }
    Ok(())
}

fn remove_module(index: &ModuleIndex, request: &str, quiet: bool, dry_run: bool) -> AppletResult {
    if index.is_builtin(request) {
        return Ok(());
    }
    let Some(entry) = index.get(request) else {
        return if quiet {
            Ok(())
        } else {
            Err(vec![AppletError::new(
                APPLET,
                format!("module '{request}' not found"),
            )])
        };
    };

    let mut order = Vec::new();
    let mut seen = HashSet::new();
    dependency_order(index, entry, &mut seen, &mut order)?;
    order.reverse();

    for module in order {
        if dry_run {
            continue;
        }
        delete_module(&module.name, libc::O_NONBLOCK).map_err(wrap_module_error)?;
    }
    Ok(())
}

fn dependency_order<'a>(
    index: &'a ModuleIndex,
    entry: &'a ModuleEntry,
    seen: &mut HashSet<String>,
    order: &mut Vec<&'a ModuleEntry>,
) -> AppletResult {
    if !seen.insert(entry.name.clone()) {
        return Ok(());
    }
    for dependency in entry.metadata.depends() {
        if index.is_builtin(&dependency) {
            continue;
        }
        let dep_entry = index.get(&dependency).ok_or_else(|| {
            vec![AppletError::new(
                APPLET,
                format!("dependency '{dependency}' for '{}' not found", entry.name),
            )]
        })?;
        dependency_order(index, dep_entry, seen, order)?;
    }
    order.push(entry);
    Ok(())
}

fn wrap_module_error(error: AppletError) -> Vec<AppletError> {
    vec![AppletError::new(APPLET, error.to_string())]
}

fn map_module_error(error: AppletError) -> AppletError {
    AppletError::new(APPLET, error.to_string())
}

#[cfg(test)]
mod tests {
    use super::{Options, parse_args};

    #[test]
    fn parses_load_and_remove_flags() {
        assert_eq!(
            parse_args(&["-rq".into(), "loop".into()]).expect("modprobe args"),
            Options {
                remove: true,
                quiet: true,
                dry_run: false,
                module: Some(String::from("loop")),
                params: Vec::new(),
            }
        );
    }
}
