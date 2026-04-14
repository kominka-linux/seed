use std::ffi::CString;
use std::io::Write;

use crate::common::applet::{AppletResult, finish};
use crate::common::error::AppletError;
use crate::common::io::stdout;
use crate::common::unix::{lookup_group, lookup_user};

const APPLET: &str = "id";

pub fn main(args: &[String]) -> i32 {
    finish(run(args))
}

fn run(args: &[String]) -> AppletResult {
    let mut show_uid = false;
    let mut show_gid = false;
    let mut show_groups = false;
    let mut name_only = false;
    let mut real_only = false;
    let mut user_arg: Option<&str> = None;
    let mut parsing_flags = true;
    let mut i = 0;

    while i < args.len() {
        let arg = &args[i];
        if parsing_flags {
            match arg.as_str() {
                "--" => {
                    parsing_flags = false;
                }
                a if a.starts_with('-') && a.len() > 1 => {
                    for ch in a[1..].chars() {
                        match ch {
                            'u' => show_uid = true,
                            'g' => show_gid = true,
                            'G' => show_groups = true,
                            'n' => name_only = true,
                            'r' => real_only = true,
                            _ => return Err(vec![AppletError::invalid_option(APPLET, ch)]),
                        }
                    }
                }
                _ => {
                    if user_arg.is_some() {
                        return Err(vec![AppletError::new(APPLET, "extra operand")]);
                    }
                    user_arg = Some(arg);
                }
            }
        } else {
            if user_arg.is_some() {
                return Err(vec![AppletError::new(APPLET, "extra operand")]);
            }
            user_arg = Some(arg);
        }
        i += 1;
    }

    let mode_count = [show_uid, show_gid, show_groups]
        .iter()
        .filter(|&&x| x)
        .count();
    if mode_count > 1 {
        return Err(vec![AppletError::new(
            APPLET,
            "cannot print \"only\" of more than one choice",
        )]);
    }

    let mut out = stdout();

    if let Some(username) = user_arg {
        let info = lookup_user_by_name(username).ok_or_else(|| {
            vec![AppletError::new(
                APPLET,
                format!("{username}: no such user"),
            )]
        })?;

        if show_uid {
            let s = if name_only {
                info.name.clone()
            } else {
                info.uid.to_string()
            };
            writeln!(out, "{s}")
                .map_err(|e| vec![AppletError::from_io(APPLET, "writing", None, e)])?;
        } else if show_gid {
            let s = if name_only {
                lookup_group(info.gid)
            } else {
                info.gid.to_string()
            };
            writeln!(out, "{s}")
                .map_err(|e| vec![AppletError::from_io(APPLET, "writing", None, e)])?;
        } else if show_groups {
            let parts: Vec<String> = info
                .groups
                .iter()
                .map(|&g| {
                    if name_only {
                        lookup_group(g)
                    } else {
                        g.to_string()
                    }
                })
                .collect();
            writeln!(out, "{}", parts.join(" "))
                .map_err(|e| vec![AppletError::from_io(APPLET, "writing", None, e)])?;
        } else {
            let groups_str: Vec<String> = info
                .groups
                .iter()
                .map(|&g| format!("{}({})", g, lookup_group(g)))
                .collect();
            writeln!(
                out,
                "uid={}({}) gid={}({}) groups={}",
                info.uid,
                info.name,
                info.gid,
                lookup_group(info.gid),
                groups_str.join(",")
            )
            .map_err(|e| vec![AppletError::from_io(APPLET, "writing", None, e)])?;
        }
    } else {
        // SAFETY: getuid/geteuid/getgid/getegid always succeed.
        let ruid = unsafe { libc::getuid() };
        let rgid = unsafe { libc::getgid() };
        let euid = unsafe { libc::geteuid() };
        let egid = unsafe { libc::getegid() };
        let groups = get_supplementary_groups();

        if show_uid {
            let id = if real_only { ruid } else { euid };
            let s = if name_only {
                lookup_user(id)
            } else {
                id.to_string()
            };
            writeln!(out, "{s}")
                .map_err(|e| vec![AppletError::from_io(APPLET, "writing", None, e)])?;
        } else if show_gid {
            let id = if real_only { rgid } else { egid };
            let s = if name_only {
                lookup_group(id)
            } else {
                id.to_string()
            };
            writeln!(out, "{s}")
                .map_err(|e| vec![AppletError::from_io(APPLET, "writing", None, e)])?;
        } else if show_groups {
            let parts: Vec<String> = groups
                .iter()
                .map(|&g| {
                    if name_only {
                        lookup_group(g)
                    } else {
                        g.to_string()
                    }
                })
                .collect();
            writeln!(out, "{}", parts.join(" "))
                .map_err(|e| vec![AppletError::from_io(APPLET, "writing", None, e)])?;
        } else {
            let uid_name = lookup_user(ruid);
            let gid_name = lookup_group(rgid);
            let groups_str: Vec<String> = groups
                .iter()
                .map(|&g| format!("{}({})", g, lookup_group(g)))
                .collect();

            let mut line = format!("uid={}({}) gid={}({})", ruid, uid_name, rgid, gid_name);
            // Show effective IDs only if they differ from real
            if euid != ruid {
                line.push_str(&format!(" euid={}({})", euid, lookup_user(euid)));
            }
            if egid != rgid {
                line.push_str(&format!(" egid={}({})", egid, lookup_group(egid)));
            }
            line.push_str(&format!(" groups={}", groups_str.join(",")));
            writeln!(out, "{line}")
                .map_err(|e| vec![AppletError::from_io(APPLET, "writing", None, e)])?;
        }
    }

    Ok(())
}

struct UserInfo {
    uid: u32,
    gid: u32,
    name: String,
    groups: Vec<u32>,
}

fn lookup_user_by_name(username: &str) -> Option<UserInfo> {
    let c_name = CString::new(username).ok()?;
    // SAFETY: getpwnam returns null or a pointer valid until the next call.
    // We copy all fields before returning.
    let pw = unsafe { libc::getpwnam(c_name.as_ptr()) };
    if pw.is_null() {
        return None;
    }
    let (uid, gid, name) = unsafe {
        let uid = (*pw).pw_uid;
        let gid = (*pw).pw_gid;
        let name = std::ffi::CStr::from_ptr((*pw).pw_name)
            .to_string_lossy()
            .into_owned();
        (uid, gid, name)
    };
    let groups = get_groups_for_user(&c_name, gid);
    Some(UserInfo {
        uid,
        gid,
        name,
        groups,
    })
}

fn get_groups_for_user(c_name: &CString, primary_gid: u32) -> Vec<u32> {
    #[cfg(not(target_os = "macos"))]
    {
        let mut ngroups: libc::c_int = 64;
        let mut groups: Vec<libc::gid_t> = vec![0; 64];
        let result = unsafe {
            libc::getgrouplist(
                c_name.as_ptr(),
                primary_gid,
                groups.as_mut_ptr(),
                &mut ngroups,
            )
        };
        if result == -1 {
            groups.resize(ngroups.max(0) as usize, 0);
            unsafe {
                libc::getgrouplist(
                    c_name.as_ptr(),
                    primary_gid,
                    groups.as_mut_ptr(),
                    &mut ngroups,
                );
            }
        }
        groups.truncate(ngroups.max(0) as usize);
        groups.into_iter().map(|g| g as u32).collect()
    }
    #[cfg(target_os = "macos")]
    {
        let _ = c_name;
        vec![primary_gid]
    }
}

fn get_supplementary_groups() -> Vec<u32> {
    let mut buf: Vec<libc::gid_t> = vec![0; 64];
    // SAFETY: buf is valid and we pass its length.
    let n = unsafe { libc::getgroups(buf.len() as libc::c_int, buf.as_mut_ptr()) };
    if n < 0 {
        return vec![];
    }
    buf.truncate(n as usize);
    buf.into_iter().collect()
}

#[cfg(test)]
mod tests {
    use super::{get_supplementary_groups, run};

    fn args(v: &[&str]) -> Vec<String> {
        v.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn no_args_succeeds() {
        assert!(run(&args(&[])).is_ok());
    }

    #[test]
    fn show_uid_numeric() {
        assert!(run(&args(&["-u"])).is_ok());
    }

    #[test]
    fn show_gid_numeric() {
        assert!(run(&args(&["-g"])).is_ok());
    }

    #[test]
    fn show_groups_numeric() {
        assert!(run(&args(&["-G"])).is_ok());
    }

    #[test]
    fn show_uid_name() {
        assert!(run(&args(&["-un"])).is_ok());
    }

    #[test]
    fn real_uid() {
        assert!(run(&args(&["-ru"])).is_ok());
    }

    #[test]
    fn multiple_mode_flags_fail() {
        assert!(run(&args(&["-ug"])).is_err());
    }

    #[test]
    fn invalid_option_fails() {
        assert!(run(&args(&["-z"])).is_err());
    }

    #[test]
    fn unknown_user_fails() {
        assert!(run(&args(&["__no_such_user_xyz__"])).is_err());
    }

    #[test]
    fn supplementary_groups_nonempty() {
        // Current process belongs to at least one group.
        let gs = get_supplementary_groups();
        assert!(!gs.is_empty());
    }
}
