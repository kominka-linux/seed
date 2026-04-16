use std::fs::OpenOptions;
use std::io::{Read, Write};

use crate::common::applet::{AppletResult, finish};
use crate::common::args::argv_to_strings;
use crate::common::error::AppletError;
use crate::common::io::{BUFFER_SIZE, stdin, stdout};

const APPLET: &str = "tee";

#[derive(Clone, Copy, Debug, Default)]
struct Options {
    append: bool,
}

pub fn main(args: &[std::ffi::OsString]) -> i32 {
    finish(run(args))
}

fn run(args: &[std::ffi::OsString]) -> AppletResult {
    let args = argv_to_strings(APPLET, args)?;
    let (options, files) = parse_args(&args)?;
    let mut writers: Vec<Box<dyn Write>> = Vec::new();
    writers.push(Box::new(stdout()));

    let mut errors = Vec::new();
    for file in &files {
        match OpenOptions::new()
            .create(true)
            .write(true)
            .truncate(!options.append)
            .append(options.append)
            .open(file)
        {
            Ok(handle) => writers.push(Box::new(handle)),
            Err(err) => errors.push(AppletError::from_io(APPLET, "opening", Some(file), err)),
        }
    }
    if !errors.is_empty() {
        return Err(errors);
    }

    let mut input = stdin();
    let mut buf = [0_u8; BUFFER_SIZE];
    loop {
        let read = input
            .read(&mut buf)
            .map_err(|err| vec![AppletError::from_io(APPLET, "reading", None, err)])?;
        if read == 0 {
            break;
        }
        for writer in &mut writers {
            writer
                .write_all(&buf[..read])
                .map_err(|err| vec![AppletError::from_io(APPLET, "writing", None, err)])?;
        }
    }

    for writer in &mut writers {
        writer
            .flush()
            .map_err(|err| vec![AppletError::from_io(APPLET, "flushing", None, err)])?;
    }

    Ok(())
}

fn parse_args(args: &[String]) -> Result<(Options, Vec<String>), Vec<AppletError>> {
    let mut options = Options::default();
    let mut files = Vec::new();
    let mut parsing_flags = true;

    for arg in args {
        if parsing_flags && arg == "--" {
            parsing_flags = false;
            continue;
        }

        if parsing_flags && arg.starts_with('-') && arg.len() > 1 {
            for flag in arg[1..].chars() {
                match flag {
                    'a' => options.append = true,
                    'i' => {}
                    _ => return Err(vec![AppletError::invalid_option(APPLET, flag)]),
                }
            }
            continue;
        }

        files.push(arg.clone());
    }

    Ok((options, files))
}
