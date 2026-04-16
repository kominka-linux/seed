use std::io::{self, Write};

use crate::common::applet::fail;
use crate::common::args::argv_to_strings;

pub fn main(args: &[std::ffi::OsString]) -> i32 {
    let args = match argv_to_strings("yes", args) {
        Ok(args) => args,
        Err(errors) => return fail(errors, 1),
    };
    let line = output_line(&args);
    let bytes = line.as_bytes();
    let stdout = io::stdout();
    let mut out = stdout.lock();
    loop {
        match out.write_all(bytes) {
            Ok(()) => {}
            Err(e) if e.kind() == io::ErrorKind::BrokenPipe => return 0,
            Err(e) => {
                eprintln!("yes: write error: {e}");
                return 1;
            }
        }
    }
}

fn output_line(args: &[String]) -> String {
    if args.is_empty() {
        "y\n".to_string()
    } else {
        format!("{}\n", args.join(" "))
    }
}

#[cfg(test)]
mod tests {
    use super::output_line;

    fn args(v: &[&str]) -> Vec<String> {
        v.iter().map(|value| value.to_string()).collect()
    }

    #[test]
    fn default_output_is_y() {
        assert_eq!(output_line(&[]), "y\n");
    }

    #[test]
    fn arguments_are_joined_with_spaces() {
        assert_eq!(output_line(&args(&["hello", "world"])), "hello world\n");
    }
}
