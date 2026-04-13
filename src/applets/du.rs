use std::collections::HashSet;
use std::fs;
use std::io::{BufWriter, Write};
use std::os::unix::fs::MetadataExt;
use std::path::Path;

use crate::common::applet::{AppletResult, finish};
use crate::common::error::AppletError;
use crate::common::io::stdout;

const APPLET: &str = "du";

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Units {
    Kibibytes,
    Mebibytes,
    Human,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct Options {
    units: Units,
    summarize: bool,
    count_links: bool,
}

impl Default for Options {
    fn default() -> Self {
        Self {
            units: Units::Kibibytes,
            summarize: false,
            count_links: false,
        }
    }
}

pub fn main(args: &[String]) -> i32 {
    finish(run(args))
}

fn run(args: &[String]) -> AppletResult {
    let (options, paths) = parse_args(args)?;
    let paths = if paths.is_empty() {
        vec![String::from(".")]
    } else {
        paths
    };

    let mut out = stdout();
    let mut seen = HashSet::new();
    let mut errors = Vec::new();

    for path in &paths {
        if let Err(error) = visit(Path::new(path), path, 0, options, &mut seen, &mut out) {
            errors.push(error);
        }
    }

    out.flush()
        .map_err(|err| vec![AppletError::from_io(APPLET, "writing stdout", None, err)])?;

    if errors.is_empty() {
        Ok(())
    } else {
        Err(errors)
    }
}

fn parse_args(args: &[String]) -> Result<(Options, Vec<String>), Vec<AppletError>> {
    let mut options = Options::default();
    let mut paths = Vec::new();
    let mut parsing_flags = true;

    for arg in args {
        if parsing_flags && arg == "--" {
            parsing_flags = false;
            continue;
        }

        if parsing_flags && arg.starts_with('-') && arg.len() > 1 {
            for flag in arg[1..].chars() {
                match flag {
                    'h' => options.units = Units::Human,
                    'k' => options.units = Units::Kibibytes,
                    'm' => options.units = Units::Mebibytes,
                    's' => options.summarize = true,
                    'l' => options.count_links = true,
                    _ => return Err(vec![AppletError::invalid_option(APPLET, flag)]),
                }
            }
            continue;
        }

        paths.push(arg.clone());
    }

    Ok((options, paths))
}

fn visit(
    path: &Path,
    display: &str,
    depth: usize,
    options: Options,
    seen: &mut HashSet<(u64, u64)>,
    out: &mut BufWriter<std::io::Stdout>,
) -> Result<u64, AppletError> {
    let metadata = fs::symlink_metadata(path)
        .map_err(|err| AppletError::from_io(APPLET, "reading", Some(display), err))?;

    if !options.count_links && metadata.nlink() > 1 {
        let key = (metadata.dev(), metadata.ino());
        if !seen.insert(key) {
            return Ok(0);
        }
    }

    let mut blocks = metadata.blocks();
    let is_dir = metadata.is_dir() && !metadata.file_type().is_symlink();

    if is_dir {
        let mut children = fs::read_dir(path)
            .map_err(|err| AppletError::from_io(APPLET, "reading", Some(display), err))?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|err| AppletError::from_io(APPLET, "reading", Some(display), err))?;
        children.sort_by_key(|entry| entry.file_name());

        for child in children {
            let child_path = child.path();
            let child_display = child_path.display().to_string();
            blocks += visit(&child_path, &child_display, depth + 1, options, seen, out)?;
        }
    }

    let should_print = depth == 0 || is_dir;
    if should_print && (!options.summarize || depth == 0) {
        writeln!(out, "{}\t{}", format_blocks(blocks, options.units), display)
            .map_err(|err| AppletError::from_io(APPLET, "writing stdout", None, err))?;
    }

    Ok(blocks)
}

fn format_blocks(blocks_512: u64, units: Units) -> String {
    match units {
        Units::Kibibytes => blocks_512.div_ceil(2).to_string(),
        Units::Mebibytes => blocks_512.div_ceil(2048).to_string(),
        Units::Human => format_human(blocks_512 * 512),
    }
}

fn format_human(bytes: u64) -> String {
    const UNITS: [&str; 5] = ["B", "K", "M", "G", "T"];

    if bytes < 1024 {
        return format!("{bytes}B");
    }

    let mut value = bytes as f64;
    let mut unit = 0;
    while value >= 1024.0 && unit < UNITS.len() - 1 {
        value /= 1024.0;
        unit += 1;
    }

    format!("{value:.1}{}", UNITS[unit])
}

#[cfg(test)]
mod tests {
    use super::{Options, Units, format_blocks, parse_args};

    fn args(values: &[&str]) -> Vec<String> {
        values.iter().map(|value| value.to_string()).collect()
    }

    #[test]
    fn parses_supported_flags() {
        let (options, paths) = parse_args(&args(&["-slm", "dir"])).expect("parse du");
        assert_eq!(
            options,
            Options {
                units: Units::Mebibytes,
                summarize: true,
                count_links: true,
            }
        );
        assert_eq!(paths, vec!["dir"]);
    }

    #[test]
    fn formats_kibibytes() {
        assert_eq!(format_blocks(3, Units::Kibibytes), "2");
    }

    #[test]
    fn formats_human_mebibytes() {
        assert_eq!(format_blocks(2048, Units::Human), "1.0M");
    }
}
