use std::fs;
use std::os::unix::fs::{MetadataExt, PermissionsExt};
use std::path::Path;

use crate::common::applet::{AppletResult, finish};
use crate::common::error::AppletError;

const APPLET: &str = "chmod";

#[derive(Clone, Debug)]
enum ModeSpec {
    Numeric(u32),
    Symbolic(Vec<SymbolicClause>),
}

#[derive(Clone, Copy, Debug)]
struct WhoMask(u8);

#[derive(Clone, Copy, Debug)]
enum Action {
    Add,
    Remove,
    Set,
}

#[derive(Clone, Copy, Debug)]
enum Permission {
    Read,
    Write,
    Execute,
    ConditionalExecute,
    SetId,
    Sticky,
}

#[derive(Clone, Debug)]
struct SymbolicClause {
    who: WhoMask,
    action: Action,
    permissions: Vec<Permission>,
}

pub fn main(args: &[String]) -> i32 {
    finish(run(args))
}

fn run(args: &[String]) -> AppletResult {
    let (recursive, mode, paths) = parse_args(args)?;
    let mut errors = Vec::new();

    for path in &paths {
        if let Err(err) = chmod_path(Path::new(path), &mode, recursive) {
            errors.push(AppletError::from_io(
                APPLET,
                "changing permissions on",
                Some(path),
                err,
            ));
        }
    }

    if errors.is_empty() {
        Ok(())
    } else {
        Err(errors)
    }
}

fn parse_args(args: &[String]) -> Result<(bool, ModeSpec, Vec<String>), Vec<AppletError>> {
    let mut recursive = false;
    let mut parsing_flags = true;
    let mut paths = Vec::new();

    for arg in args {
        if parsing_flags && arg == "--" {
            parsing_flags = false;
            continue;
        }

        if parsing_flags && arg.starts_with('-') && arg.len() > 1 {
            for flag in arg[1..].chars() {
                match flag {
                    'R' => recursive = true,
                    'c' | 'f' | 'v' => {}
                    _ => return Err(vec![AppletError::invalid_option(APPLET, flag)]),
                }
            }
            continue;
        }

        paths.push(arg.clone());
    }

    if paths.len() < 2 {
        return Err(vec![AppletError::new(APPLET, "missing operand")]);
    }

    let mode = parse_mode_spec(&paths[0])?;
    Ok((recursive, mode, paths[1..].to_vec()))
}

fn parse_mode_spec(input: &str) -> Result<ModeSpec, Vec<AppletError>> {
    if input.chars().all(|ch| matches!(ch, '0'..='7')) {
        return u32::from_str_radix(input, 8)
            .map(ModeSpec::Numeric)
            .map_err(|_| vec![AppletError::new(APPLET, format!("invalid mode '{input}'"))]);
    }
    parse_symbolic_mode(input).map(ModeSpec::Symbolic)
}

fn chmod_path(path: &Path, mode: &ModeSpec, recursive: bool) -> std::io::Result<()> {
    if recursive {
        let metadata = fs::symlink_metadata(path)?;
        if metadata.is_dir() && !metadata.file_type().is_symlink() {
            for entry in fs::read_dir(path)? {
                let entry = entry?;
                chmod_path(&entry.path(), mode, true)?;
            }
        }
    }
    apply_mode(path, mode)
}

fn apply_mode(path: &Path, mode: &ModeSpec) -> std::io::Result<()> {
    let metadata = fs::metadata(path)?;
    let current = metadata.mode() & 0o7777;
    let next = match mode {
        ModeSpec::Numeric(value) => *value,
        ModeSpec::Symbolic(clauses) => apply_symbolic_mode(clauses, current, metadata.is_dir())
            .map_err(std::io::Error::other)?,
    };
    fs::set_permissions(path, fs::Permissions::from_mode(next))
}

fn parse_symbolic_mode(spec: &str) -> Result<Vec<SymbolicClause>, Vec<AppletError>> {
    let mut clauses = Vec::new();
    for clause in spec.split(',') {
        let chars: Vec<char> = clause.chars().collect();
        let mut index = 0;
        let mut who = 0_u8;

        while index < chars.len() {
            match chars[index] {
                'u' => who |= 0b100,
                'g' => who |= 0b010,
                'o' => who |= 0b001,
                'a' => who |= 0b111,
                _ => break,
            }
            index += 1;
        }

        if who == 0 {
            who = 0b111;
        }
        if index >= chars.len() {
            return Err(vec![AppletError::new(
                APPLET,
                format!("invalid mode '{spec}'"),
            )]);
        }

        let action = match chars[index] {
            '+' => Action::Add,
            '-' => Action::Remove,
            '=' => Action::Set,
            _ => {
                return Err(vec![AppletError::new(
                    APPLET,
                    format!("invalid mode '{spec}'"),
                )]);
            }
        };
        index += 1;

        if index >= chars.len() {
            return Err(vec![AppletError::new(
                APPLET,
                format!("invalid mode '{spec}'"),
            )]);
        }

        let mut permissions = Vec::new();
        for perm in &chars[index..] {
            let permission = match perm {
                'r' => Permission::Read,
                'w' => Permission::Write,
                'x' => Permission::Execute,
                'X' => Permission::ConditionalExecute,
                's' => Permission::SetId,
                't' => Permission::Sticky,
                _ => {
                    return Err(vec![AppletError::new(
                        APPLET,
                        format!("invalid mode '{spec}'"),
                    )]);
                }
            };
            permissions.push(permission);
        }

        clauses.push(SymbolicClause {
            who: WhoMask(who),
            action,
            permissions,
        });
    }
    Ok(clauses)
}

fn apply_symbolic_mode(
    clauses: &[SymbolicClause],
    mut mode: u32,
    is_dir: bool,
) -> Result<u32, String> {
    for clause in clauses {
        let mut perm_mask = 0_u32;
        let mut special_mask = 0_u32;
        for permission in &clause.permissions {
            match permission {
                Permission::Read => perm_mask |= expand_bits(clause.who.0, 0o444),
                Permission::Write => perm_mask |= expand_bits(clause.who.0, 0o222),
                Permission::Execute => perm_mask |= expand_bits(clause.who.0, 0o111),
                Permission::ConditionalExecute => {
                    if is_dir || (mode & 0o111) != 0 {
                        perm_mask |= expand_bits(clause.who.0, 0o111);
                    }
                }
                Permission::SetId => {
                    if (clause.who.0 & 0b100) != 0 {
                        special_mask |= 0o4000;
                    }
                    if (clause.who.0 & 0b010) != 0 {
                        special_mask |= 0o2000;
                    }
                }
                Permission::Sticky => special_mask |= 0o1000,
            }
        }

        let clear_mask = expand_bits(clause.who.0, 0o777)
            | if (clause.who.0 & 0b100) != 0 {
                0o4000
            } else {
                0
            }
            | if (clause.who.0 & 0b010) != 0 {
                0o2000
            } else {
                0
            }
            | if special_mask & 0o1000 != 0 {
                0o1000
            } else {
                0
            };

        match clause.action {
            Action::Add => mode |= perm_mask | special_mask,
            Action::Remove => mode &= !(perm_mask | special_mask),
            Action::Set => {
                mode &= !clear_mask;
                mode |= perm_mask | special_mask;
            }
        }
    }

    Ok(mode)
}

fn expand_bits(who: u8, bits: u32) -> u32 {
    let mut mask = 0_u32;
    if (who & 0b100) != 0 {
        mask |= bits & 0o700;
    }
    if (who & 0b010) != 0 {
        mask |= bits & 0o070;
    }
    if (who & 0b001) != 0 {
        mask |= bits & 0o007;
    }
    mask
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::os::unix::fs::PermissionsExt;
    use std::path::Path;

    use super::{apply_symbolic_mode, parse_symbolic_mode};
    use crate::common::unix::temp_dir;

    #[test]
    fn symbolic_plus_updates_expected_classes() {
        let clauses = parse_symbolic_mode("u+x,g-w").expect("parse mode");
        let mode = apply_symbolic_mode(&clauses, 0o764, false).expect("apply mode");
        assert_eq!(mode, 0o744);
    }

    #[test]
    fn symbolic_x_respects_directory_semantics() {
        let clauses = parse_symbolic_mode("a+X").expect("parse mode");
        let file_mode = apply_symbolic_mode(&clauses, 0o644, false).expect("file mode");
        let dir_mode = apply_symbolic_mode(&clauses, 0o644, true).expect("dir mode");
        assert_eq!(file_mode, 0o644);
        assert_eq!(dir_mode, 0o755);
    }

    #[test]
    fn recursive_numeric_mode_descends_before_changing_directory() {
        let dir = temp_dir("chmod-recursive");
        let parent = dir.join("parent");
        let child = parent.join("child");
        fs::create_dir(&parent).expect("create parent dir");
        fs::write(&child, b"ok").expect("create child file");

        super::chmod_path(Path::new(&parent), &super::ModeSpec::Numeric(0o644), true)
            .expect("recursive chmod");

        let parent_mode = fs::metadata(&parent)
            .expect("parent metadata")
            .permissions()
            .mode()
            & 0o777;
        fs::set_permissions(&parent, fs::Permissions::from_mode(0o755))
            .expect("restore parent search bit");
        let child_mode = fs::metadata(&child)
            .expect("child metadata")
            .permissions()
            .mode()
            & 0o777;
        assert_eq!(parent_mode, 0o644);
        assert_eq!(child_mode, 0o644);
    }
}
