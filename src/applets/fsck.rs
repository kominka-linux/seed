use std::path::Path;
use std::process::{Child, Command};

use crate::common::applet::finish_code;
use crate::common::fstab::{self, FstabEntry};
use crate::common::error::AppletError;

const APPLET: &str = "fsck";

#[derive(Clone, Debug, Default, Eq, PartialEq)]
struct Options {
    check_all: bool,
    no_execute: bool,
    parallel: bool,
    skip_root: bool,
    no_title: bool,
    verbose: bool,
    type_filter: Option<Vec<String>>,
    helper_args: Vec<String>,
    devices: Vec<String>,
}

pub fn main(args: &[String]) -> i32 {
    finish_code(run(args))
}

fn run(args: &[String]) -> Result<i32, Vec<AppletError>> {
    let options = parse_args(args)?;
    let requests = build_requests(&options)?;
    if requests.is_empty() {
        return Ok(0);
    }

    if options.no_execute {
        for request in &requests {
            eprintln!("{}", request.display_command());
        }
        return Ok(0);
    }

    if options.parallel {
        return run_requests_in_parallel(&options, requests);
    }

    let mut exit_code = 0;
    for request in requests {
        if options.verbose {
            eprintln!("{}", request.display_command());
        }
        let status = request.command().status().map_err(|err| {
            vec![AppletError::from_io(APPLET, "executing", Some(&request.program), err)]
        })?;
        exit_code |= status.code().unwrap_or(1);
    }

    let _ = options.no_title;
    Ok(exit_code)
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct CheckRequest {
    program: String,
    args: Vec<String>,
    passno: u32,
}

impl CheckRequest {
    fn command(&self) -> Command {
        let mut command = Command::new(&self.program);
        command.args(&self.args);
        command
    }

    fn display_command(&self) -> String {
        std::iter::once(self.program.as_str())
            .chain(self.args.iter().map(String::as_str))
            .collect::<Vec<_>>()
            .join(" ")
    }
}

fn parse_args(args: &[String]) -> Result<Options, Vec<AppletError>> {
    let mut options = Options::default();
    let mut index = 0;
    let mut parsing_flags = true;

    while index < args.len() {
        let arg = &args[index];
        index += 1;
        if parsing_flags && arg == "--" {
            parsing_flags = false;
            continue;
        }
        if parsing_flags && arg.starts_with('-') && arg.len() > 1 {
            match arg.as_str() {
                "-A" => options.check_all = true,
                "-N" => options.no_execute = true,
                "-P" => options.parallel = true,
                "-R" => options.skip_root = true,
                "-T" => options.no_title = true,
                "-V" => options.verbose = true,
                "-t" => {
                    let Some(value) = args.get(index) else {
                        return Err(vec![AppletError::option_requires_arg(APPLET, "t")]);
                    };
                    index += 1;
                    options.type_filter = Some(
                        value
                            .split(',')
                            .filter(|item| !item.is_empty())
                            .map(str::to_string)
                            .collect(),
                    );
                }
                _ => options.helper_args.push(arg.clone()),
            }
            continue;
        }

        options.devices.push(arg.clone());
    }

    if options.check_all && !options.devices.is_empty() {
        return Err(vec![AppletError::new(
            APPLET,
            "block devices may not be specified with -A",
        )]);
    }

    Ok(options)
}

fn build_requests(options: &Options) -> Result<Vec<CheckRequest>, Vec<AppletError>> {
    if options.check_all {
        let mut entries = fstab::read_entries()
            .map_err(|err| vec![AppletError::from_io(APPLET, "reading", Some("/etc/fstab"), err)])?;
        entries.retain(|entry| entry.passno > 0);
        entries.retain(|entry| entry.vfstype != "swap");
        entries.retain(|entry| !fstab::option_present(&entry.mntops, "noauto"));
        if options.skip_root {
            entries.retain(|entry| entry.file != "/");
        }
        if let Some(filter) = options.type_filter.as_ref() {
            entries.retain(|entry| fstab::matches_type_filter(Some(filter), &entry.vfstype));
        }
        entries.sort_by_key(|entry| (entry.passno, entry.file.clone()));
        return entries
            .into_iter()
            .map(|entry| request_for_entry(&entry, &options.helper_args))
            .collect();
    }

    options
        .devices
        .iter()
        .map(|device| request_for_device(device, options.type_filter.as_deref(), &options.helper_args))
        .collect()
}

fn request_for_device(
    device: &str,
    type_filter: Option<&[String]>,
    helper_args: &[String],
) -> Result<CheckRequest, Vec<AppletError>> {
    let fstab_entry = fstab::read_entries()
        .ok()
        .and_then(|entries| {
            entries
                .into_iter()
                .find(|entry| entry.spec == device || entry.file == device)
        });
    let (filesystem_type, passno) = if let Some(filter) = type_filter {
        if filter.len() == 1 && !filter[0].starts_with("no") {
            (filter[0].clone(), fstab_entry.as_ref().map_or(0, |entry| entry.passno))
        } else {
            (
                fstab_entry
                    .as_ref()
                    .map(|entry| entry.vfstype.clone())
                    .unwrap_or_else(|| String::from("auto")),
                fstab_entry.as_ref().map_or(0, |entry| entry.passno),
            )
        }
    } else {
        (
            fstab_entry
                .as_ref()
                .map(|entry| entry.vfstype.clone())
                .unwrap_or_else(|| String::from("auto")),
            fstab_entry.as_ref().map_or(0, |entry| entry.passno),
        )
    };
    request_for_spec(device, &filesystem_type, passno, helper_args)
}

fn request_for_entry(entry: &FstabEntry, helper_args: &[String]) -> Result<CheckRequest, Vec<AppletError>> {
    request_for_spec(&entry.spec, &entry.vfstype, entry.passno, helper_args)
}

fn request_for_spec(
    device: &str,
    filesystem_type: &str,
    passno: u32,
    helper_args: &[String],
) -> Result<CheckRequest, Vec<AppletError>> {
    let program = checker_program(filesystem_type).ok_or_else(|| {
        vec![AppletError::new(
            APPLET,
            format!("no checker available for filesystem type '{filesystem_type}'"),
        )]
    })?;
    let mut args = helper_args.to_vec();
    args.push(device.to_string());
    Ok(CheckRequest {
        program,
        args,
        passno,
    })
}

fn run_requests_in_parallel(
    options: &Options,
    requests: Vec<CheckRequest>,
) -> Result<i32, Vec<AppletError>> {
    let mut exit_code = 0;
    let mut children = Vec::<Child>::new();
    let mut current_pass = None::<u32>;

    for request in requests {
        if options.verbose {
            eprintln!("{}", request.display_command());
        }
        if current_pass.is_some_and(|pass| pass != request.passno) {
            exit_code |= wait_for_children(children)?;
            children = Vec::new();
        }
        current_pass = Some(request.passno);
        let child = request.command().spawn().map_err(|err| {
            vec![AppletError::from_io(APPLET, "executing", Some(&request.program), err)]
        })?;
        children.push(child);
    }

    exit_code |= wait_for_children(children)?;
    Ok(exit_code)
}

fn wait_for_children(children: Vec<Child>) -> Result<i32, Vec<AppletError>> {
    let mut exit_code = 0;
    for mut child in children {
        let status = child
            .wait()
            .map_err(|err| vec![AppletError::from_io(APPLET, "waiting", None, err)])?;
        exit_code |= status.code().unwrap_or(1);
    }
    Ok(exit_code)
}

fn checker_program(filesystem_type: &str) -> Option<String> {
    if filesystem_type == "auto" {
        return Some(String::from("fsck"));
    }

    let candidates = [
        format!("fsck.{filesystem_type}"),
        format!("/sbin/fsck.{filesystem_type}"),
        format!("/usr/sbin/fsck.{filesystem_type}"),
    ];
    candidates.into_iter().find(|candidate| {
        if candidate.contains('/') {
            Path::new(candidate).exists()
        } else {
            true
        }
    })
}

#[cfg(test)]
mod tests {
    use super::{Options, build_requests, parse_args};
    use crate::common::test_env;
    use std::fs;

    fn args(values: &[&str]) -> Vec<String> {
        values.iter().map(|value| value.to_string()).collect()
    }

    #[test]
    fn parses_known_flags() {
        assert_eq!(
            parse_args(&args(&["-A", "-N", "-V", "-t", "ext4,xfs", "-f"])).unwrap(),
            Options {
                check_all: true,
                no_execute: true,
                verbose: true,
                type_filter: Some(vec!["ext4".to_string(), "xfs".to_string()]),
                helper_args: vec!["-f".to_string()],
                ..Options::default()
            }
        );
    }

    #[test]
    fn builds_requests_from_fstab() {
        let _guard = test_env::lock();
        let path = std::env::temp_dir().join(format!("seed-fsck-{}.fstab", std::process::id()));
        fs::write(
            &path,
            "/dev/root / ext4 defaults 1 1\n/dev/data /data xfs defaults 1 2\n/dev/swap none swap defaults 0 0\n",
        )
        .unwrap();
        unsafe {
            std::env::set_var("SEED_FSTAB", &path);
        }
        let requests = build_requests(&Options {
            check_all: true,
            type_filter: Some(vec!["ext4".to_string()]),
            ..Options::default()
        })
        .unwrap();
        unsafe {
            std::env::remove_var("SEED_FSTAB");
        }
        assert_eq!(requests.len(), 1);
        assert!(requests[0].program.contains("fsck.ext4"));
        assert_eq!(requests[0].passno, 1);
        let _ = fs::remove_file(path);
    }
}
