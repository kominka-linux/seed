use std::collections::HashSet;
use std::ffi::OsString;
use std::process::Command;

use crate::common::applet::{AppletResult, finish};
use crate::common::args::argv_to_strings;
use crate::common::error::AppletError;
use crate::common::modules::{
    ModprobeConfig, ModuleEntry, ModuleIndex, delete_module, finit_module, module_tree_dir,
};

const APPLET: &str = "modprobe";

#[derive(Clone, Debug, Default, Eq, PartialEq)]
struct Options {
    remove: bool,
    quiet: bool,
    dry_run: bool,
    module: Option<String>,
    params: Vec<String>,
}

#[derive(Clone, Copy)]
enum RequestKind {
    Explicit,
    Alias,
}

pub fn main(args: &[OsString]) -> i32 {
    finish(run(args))
}

fn run(args: &[OsString]) -> AppletResult {
    let args = argv_to_strings(APPLET, args)?;
    let options = parse_args(&args)?;
    let root = module_tree_dir(None).map_err(|err| vec![map_module_error(err)])?;
    let index = ModuleIndex::scan(&root).map_err(|err| {
        vec![AppletError::from_io(
            APPLET,
            "reading",
            Some(&root.to_string_lossy()),
            err,
        )]
    })?;
    let config =
        ModprobeConfig::load().map_err(|err| vec![AppletError::new(APPLET, err.to_string())])?;
    let module = options.module.as_deref().unwrap_or_default();

    if options.remove {
        remove_request(&index, &config, module, options.quiet, options.dry_run)
    } else {
        insert_request(
            &index,
            &config,
            module,
            &options.params,
            options.quiet,
            options.dry_run,
        )
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

fn insert_request(
    index: &ModuleIndex,
    config: &ModprobeConfig,
    request: &str,
    params: &[String],
    quiet: bool,
    dry_run: bool,
) -> AppletResult {
    let Some(entry) = resolve_request(
        index,
        config,
        request,
        RequestKind::Explicit,
        &mut HashSet::new(),
    )?
    else {
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
    dependency_order(index, config, entry, &mut seen, &mut order)?;

    for module in order {
        if dry_run {
            continue;
        }
        let mut module_params = config.module_options(&module.name);
        if module.name == entry.name {
            module_params.extend(params.iter().cloned());
        }
        if let Some(command) = config.install_command(&module.name) {
            run_module_command(command, &module.name, &module_params)?;
        } else {
            let module_params = module_params
                .into_iter()
                .map(OsString::from)
                .collect::<Vec<_>>();
            finit_module(&module.path, &module_params).map_err(wrap_module_error)?;
        }
    }
    Ok(())
}

fn remove_request(
    index: &ModuleIndex,
    config: &ModprobeConfig,
    request: &str,
    quiet: bool,
    dry_run: bool,
) -> AppletResult {
    let Some(entry) = resolve_request(
        index,
        config,
        request,
        RequestKind::Explicit,
        &mut HashSet::new(),
    )?
    else {
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
    dependency_order(index, config, entry, &mut seen, &mut order)?;
    order.reverse();

    for module in order {
        if dry_run {
            continue;
        }
        if let Some(command) = config.remove_command(&module.name) {
            run_module_command(command, &module.name, &[])?;
        } else {
            delete_module(&module.name, libc::O_NONBLOCK).map_err(wrap_module_error)?;
        }
    }
    Ok(())
}

fn resolve_request<'a>(
    index: &'a ModuleIndex,
    config: &ModprobeConfig,
    request: &str,
    kind: RequestKind,
    resolving_aliases: &mut HashSet<String>,
) -> Result<Option<&'a ModuleEntry>, Vec<AppletError>> {
    if index.is_builtin(request) {
        return Ok(None);
    }
    if let Some(entry) = index.get(request) {
        if matches!(kind, RequestKind::Alias) && config.is_blacklisted(&entry.name) {
            return Err(vec![AppletError::new(
                APPLET,
                format!("module '{}' is blacklisted", entry.name),
            )]);
        }
        return Ok(Some(entry));
    }

    if let Some(target) = config.resolve_config_alias(request) {
        let key = format!("cfg:{request}");
        if !resolving_aliases.insert(key) {
            return Err(vec![AppletError::new(
                APPLET,
                format!("alias loop detected for '{request}'"),
            )]);
        }
        return resolve_request(index, config, target, RequestKind::Alias, resolving_aliases);
    }

    let resolved = index.resolve_alias(request);
    if matches!(kind, RequestKind::Alias)
        && let Some(entry) = resolved
        && config.is_blacklisted(&entry.name)
    {
        return Err(vec![AppletError::new(
            APPLET,
            format!("module '{}' is blacklisted", entry.name),
        )]);
    }
    Ok(resolved)
}

fn dependency_order<'a>(
    index: &'a ModuleIndex,
    config: &ModprobeConfig,
    entry: &'a ModuleEntry,
    seen: &mut HashSet<String>,
    order: &mut Vec<&'a ModuleEntry>,
) -> AppletResult {
    if !seen.insert(entry.name.clone()) {
        return Ok(());
    }

    let softdep = config.softdep(&entry.name);
    for dependency in &softdep.pre {
        dependency_visit(index, config, dependency, seen, order)?;
    }
    for dependency in entry.metadata.depends() {
        dependency_visit(index, config, &dependency, seen, order)?;
    }
    order.push(entry);
    for dependency in &softdep.post {
        dependency_visit(index, config, dependency, seen, order)?;
    }
    Ok(())
}

fn dependency_visit<'a>(
    index: &'a ModuleIndex,
    config: &ModprobeConfig,
    request: &str,
    seen: &mut HashSet<String>,
    order: &mut Vec<&'a ModuleEntry>,
) -> AppletResult {
    let Some(entry) = resolve_request(
        index,
        config,
        request,
        RequestKind::Alias,
        &mut HashSet::new(),
    )?
    else {
        return Ok(());
    };
    dependency_order(index, config, entry, seen, order)
}

fn run_module_command(command: &str, module: &str, params: &[String]) -> AppletResult {
    let status = Command::new("sh")
        .arg("-c")
        .arg(command)
        .env("MODPROBE_MODULE", module)
        .env("CMDLINE_OPTS", params.join(" "))
        .status()
        .map_err(|err| vec![AppletError::from_io(APPLET, "running", None, err)])?;
    if status.success() {
        Ok(())
    } else {
        Err(vec![AppletError::new(
            APPLET,
            format!("command failed: {command}"),
        )])
    }
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
            parse_args(&["-rqn".into(), "loop".into()]).expect("modprobe args"),
            Options {
                remove: true,
                quiet: true,
                dry_run: true,
                module: Some(String::from("loop")),
                params: Vec::new(),
            }
        );
    }
}
