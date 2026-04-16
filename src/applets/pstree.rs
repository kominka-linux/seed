use std::collections::{BTreeMap, BTreeSet};
use std::ffi::CString;
use std::io::Write;

use crate::common::applet::finish;
use crate::common::args::{ParsedArg, Parser};
use crate::common::error::AppletError;
use crate::common::io::stdout;
use crate::common::process::{ProcessInfo, list_processes};

const APPLET: &str = "pstree";

#[derive(Clone, Debug, Eq, PartialEq)]
enum Root {
    Pid(i32),
    User(u32),
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
struct Options {
    show_pids: bool,
    root: Option<Root>,
}

pub fn main(args: &[std::ffi::OsString]) -> i32 {
    finish(run(args))
}

fn run(args: &[std::ffi::OsString]) -> Result<(), Vec<AppletError>> {
    let options = parse_args(args)?;
    let processes = list_processes().map_err(|message| vec![AppletError::new(APPLET, message)])?;
    let process_map = processes
        .iter()
        .cloned()
        .map(|process| (process.pid, process))
        .collect::<BTreeMap<_, _>>();
    let children = build_children(&processes);
    let roots = select_roots(&options, &processes, &process_map)?;
    let mut out = stdout();
    let mut first = true;
    let renderer = TreeRenderer {
        children: &children,
        process_map: &process_map,
        show_pids: options.show_pids,
    };

    for pid in roots {
        let Some(process) = process_map.get(&pid) else {
            continue;
        };
        if !first {
            writeln!(out).map_err(|err| vec![AppletError::from_io(APPLET, "writing stdout", None, err)])?;
        }
        first = false;
        let mut visited = BTreeSet::new();
        renderer.render(&mut out, process, "", true, true, &mut visited)?;
    }

    out.flush()
        .map_err(|err| vec![AppletError::from_io(APPLET, "writing stdout", None, err)])?;
    Ok(())
}

fn parse_args(args: &[std::ffi::OsString]) -> Result<Options, Vec<AppletError>> {
    let mut options = Options::default();
    let mut operands = Vec::new();

    let mut parser = Parser::new(APPLET, args);
    while let Some(arg) = parser.next_arg()? {
        match arg {
            ParsedArg::Short('p') => options.show_pids = true,
            ParsedArg::Short(flag) => return Err(vec![AppletError::invalid_option(APPLET, flag)]),
            ParsedArg::Long(name) => {
                return Err(vec![AppletError::unrecognized_option(
                    APPLET,
                    &format!("--{name}"),
                )])
            }
            ParsedArg::Value(arg) => operands.push(arg.into_string().map_err(|arg| {
                vec![AppletError::new(
                    APPLET,
                    format!("argument is invalid unicode: {:?}", arg),
                )]
            })?),
        }
    }

    match operands.as_slice() {
        [] => {}
        [target] => {
            options.root = Some(if let Ok(pid) = target.parse::<i32>() {
                Root::Pid(pid)
            } else {
                Root::User(lookup_user(target)?)
            });
        }
        _ => return Err(vec![AppletError::new(APPLET, "extra operand")]),
    }

    Ok(options)
}

fn lookup_user(name: &str) -> Result<u32, Vec<AppletError>> {
    let c_name = CString::new(name).map_err(|_| vec![AppletError::new(APPLET, "user contains NUL byte")])?;
    // SAFETY: `c_name` is a valid NUL-terminated username.
    let passwd = unsafe { libc::getpwnam(c_name.as_ptr()) };
    if passwd.is_null() {
        return Err(vec![AppletError::new(APPLET, format!("unknown user '{name}'"))]);
    }
    // SAFETY: `passwd` is valid for the duration of this call.
    Ok(unsafe { (*passwd).pw_uid })
}

fn build_children(processes: &[ProcessInfo]) -> BTreeMap<i32, Vec<i32>> {
    let mut children = BTreeMap::<i32, Vec<i32>>::new();
    for process in processes {
        children.entry(process.ppid).or_default().push(process.pid);
    }
    for pids in children.values_mut() {
        pids.sort_unstable();
    }
    children
}

fn select_roots(
    options: &Options,
    processes: &[ProcessInfo],
    process_map: &BTreeMap<i32, ProcessInfo>,
) -> Result<Vec<i32>, Vec<AppletError>> {
    match options.root {
        Some(Root::Pid(pid)) => {
            if process_map.contains_key(&pid) {
                Ok(vec![pid])
            } else {
                Err(vec![AppletError::new(APPLET, format!("no such pid '{pid}'"))])
            }
        }
        Some(Root::User(uid)) => {
            let mut roots = processes
                .iter()
                .filter(|process| process.uid == uid)
                .filter(|process| {
                    process_map
                        .get(&process.ppid)
                        .is_none_or(|parent| parent.uid != uid)
                })
                .map(|process| process.pid)
                .collect::<Vec<_>>();
            roots.sort_unstable();
            if roots.is_empty() {
                Err(vec![AppletError::new(APPLET, "no matching processes")])
            } else {
                Ok(roots)
            }
        }
        None => Ok(vec![1]),
    }
}

struct TreeRenderer<'a> {
    children: &'a BTreeMap<i32, Vec<i32>>,
    process_map: &'a BTreeMap<i32, ProcessInfo>,
    show_pids: bool,
}

impl TreeRenderer<'_> {
    fn render(
        &self,
        out: &mut impl Write,
        process: &ProcessInfo,
        prefix: &str,
        is_root: bool,
        is_last: bool,
        visited: &mut BTreeSet<i32>,
    ) -> Result<(), Vec<AppletError>> {
        let label = if self.show_pids {
            format!("{}({})", process.name, process.pid)
        } else {
            process.name.clone()
        };
        if is_root {
            writeln!(out, "{label}")
        } else {
            let branch = if is_last { "`- " } else { "|- " };
            writeln!(out, "{prefix}{branch}{label}")
        }
        .map_err(|err| vec![AppletError::from_io(APPLET, "writing stdout", None, err)])?;

        if !visited.insert(process.pid) {
            return Ok(());
        }

        let Some(child_pids) = self.children.get(&process.pid) else {
            return Ok(());
        };
        let next_prefix = if prefix.is_empty() {
            String::new()
        } else if is_last {
            format!("{prefix}   ")
        } else {
            format!("{prefix}|  ")
        };

        for (index, child_pid) in child_pids.iter().enumerate() {
            let Some(child) = self.process_map.get(child_pid) else {
                continue;
            };
            self.render(
                out,
                child,
                &next_prefix,
                false,
                index + 1 == child_pids.len(),
                visited,
            )?;
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::{Options, Root, build_children, parse_args};
    use crate::common::process::ProcessInfo;

    fn args(values: &[&str]) -> Vec<std::ffi::OsString> {
        values.iter().map(std::ffi::OsString::from).collect()
    }

    #[test]
    fn parses_pid_root() {
        assert_eq!(
            parse_args(&args(&["-p", "12"])).unwrap(),
            Options {
                show_pids: true,
                root: Some(Root::Pid(12)),
            }
        );
    }

    #[test]
    fn builds_parent_child_index() {
        let children = build_children(&[
            ProcessInfo {
                pid: 10,
                ppid: 1,
                uid: 0,
                tty: "??".into(),
                cpu_time_ns: 0,
                name: "a".into(),
                command: "a".into(),
            },
            ProcessInfo {
                pid: 11,
                ppid: 10,
                uid: 0,
                tty: "??".into(),
                cpu_time_ns: 0,
                name: "b".into(),
                command: "b".into(),
            },
        ]);
        assert_eq!(children.get(&1), Some(&vec![10]));
        assert_eq!(children.get(&10), Some(&vec![11]));
    }
}
