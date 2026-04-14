use std::fs;
use std::path::Path;

use crate::common::applet::{AppletResult, finish};
use crate::common::error::AppletError;
use crate::common::modules::{ModuleIndex, module_tree_dir};

const APPLET: &str = "depmod";

#[derive(Clone, Debug, Default, Eq, PartialEq)]
struct Options {
    dry_run: bool,
    release: Option<String>,
}

pub fn main(args: &[String]) -> i32 {
    finish(run(args))
}

fn run(args: &[String]) -> AppletResult {
    let options = parse_args(args)?;
    let root = module_tree_dir(options.release.as_deref()).map_err(|err| vec![map_module_error(err)])?;
    let index = ModuleIndex::scan(&root)
        .map_err(|err| vec![AppletError::from_io(APPLET, "reading", Some(&root.to_string_lossy()), err)])?;
    let deps = render_modules_dep(&index);
    let aliases = render_modules_alias(&index);

    if options.dry_run {
        print!("{deps}");
        return Ok(());
    }

    write_output(&root.join("modules.dep"), &deps)?;
    write_output(&root.join("modules.dep.bb"), &deps)?;
    write_output(&root.join("modules.alias"), &aliases)?;
    Ok(())
}

fn parse_args(args: &[String]) -> Result<Options, Vec<AppletError>> {
    let mut options = Options::default();
    for arg in args {
        match arg.as_str() {
            "-n" => options.dry_run = true,
            _ if arg.starts_with('-') => {
                return Err(vec![AppletError::invalid_option(
                    APPLET,
                    arg.chars().nth(1).unwrap_or('-'),
                )]);
            }
            _ if options.release.is_none() => options.release = Some(arg.clone()),
            _ => return Err(vec![AppletError::new(APPLET, "extra operand")]),
        }
    }
    Ok(options)
}

fn render_modules_dep(index: &ModuleIndex) -> String {
    let mut lines = index
        .entries
        .iter()
        .map(|entry| {
            let deps = entry
                .metadata
                .depends()
                .into_iter()
                .filter_map(|dependency| index.get(&dependency).map(|entry| entry.relative_path.clone()))
                .collect::<Vec<_>>()
                .join(" ");
            if deps.is_empty() {
                format!("{}:\n", entry.relative_path)
            } else {
                format!("{}: {deps}\n", entry.relative_path)
            }
        })
        .collect::<Vec<_>>();
    lines.sort();
    lines.concat()
}

fn render_modules_alias(index: &ModuleIndex) -> String {
    let mut lines = index
        .entries
        .iter()
        .flat_map(|entry| {
            entry.metadata.aliases().into_iter().map(|alias| {
                format!("alias {alias} {}\n", entry.name)
            })
        })
        .collect::<Vec<_>>();
    lines.sort();
    lines.concat()
}

fn write_output(path: &Path, content: &str) -> AppletResult {
    fs::write(path, content)
        .map_err(|err| vec![AppletError::from_io(APPLET, "writing", Some(&path.to_string_lossy()), err)])
}

fn map_module_error(error: AppletError) -> AppletError {
    AppletError::new(APPLET, error.to_string())
}

#[cfg(test)]
mod tests {
    use super::{Options, parse_args, render_modules_alias, render_modules_dep};
    use crate::common::modules::{ModuleEntry, ModuleIndex, ModuleMetadata};
    use std::collections::HashMap;
    use std::path::PathBuf;

    #[test]
    fn parses_dry_run_and_release() {
        assert_eq!(
            parse_args(&["-n".into(), "1.0".into()]).expect("depmod args"),
            Options {
                dry_run: true,
                release: Some(String::from("1.0")),
            }
        );
    }

    #[test]
    fn renders_dependency_and_alias_indexes() {
        let entries = vec![
            ModuleEntry {
                name: String::from("helper"),
                relative_path: String::from("kernel/helper.ko"),
                path: PathBuf::from("/tmp/kernel/helper.ko"),
                metadata: ModuleMetadata::parse(b"license=GPL\0"),
            },
            ModuleEntry {
                name: String::from("driver"),
                relative_path: String::from("kernel/driver.ko"),
                path: PathBuf::from("/tmp/kernel/driver.ko"),
                metadata: ModuleMetadata::parse(b"depends=helper\0alias=test*\0"),
            },
        ];
        let by_name = HashMap::from([(String::from("helper"), 0usize), (String::from("driver"), 1usize)]);
        let index = ModuleIndex {
            entries,
            by_name,
            builtins: Default::default(),
        };
        assert_eq!(
            render_modules_dep(&index),
            "kernel/driver.ko: kernel/helper.ko\nkernel/helper.ko:\n"
        );
        assert_eq!(render_modules_alias(&index), "alias test* driver\n");
    }
}
