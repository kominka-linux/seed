// Implements md5sum, sha1sum, sha256sum.

use std::io::{self, BufRead, BufReader, Read, Write};

use md5::Digest as _;

use crate::common::applet::{AppletResult, finish};
use crate::common::error::AppletError;
use crate::common::io::{open_input, stdout};

pub fn main_md5sum(args: &[String]) -> i32 {
    finish(run(args, Algo::Md5))
}
pub fn main_sha1sum(args: &[String]) -> i32 {
    finish(run(args, Algo::Sha1))
}
pub fn main_sha256sum(args: &[String]) -> i32 {
    finish(run(args, Algo::Sha256))
}
pub fn main_sha512sum(args: &[String]) -> i32 {
    finish(run(args, Algo::Sha512))
}

enum Algo {
    Md5,
    Sha1,
    Sha256,
    Sha512,
}

impl Algo {
    fn name(&self) -> &'static str {
        match self {
            Algo::Md5 => "md5sum",
            Algo::Sha1 => "sha1sum",
            Algo::Sha256 => "sha256sum",
            Algo::Sha512 => "sha512sum",
        }
    }
}

fn run(args: &[String], algo: Algo) -> AppletResult {
    let applet = algo.name();
    let mut check_mode = false;
    let mut binary = false; // -b: binary mode (only affects display marker on non-Unix)
    let mut warn = false; // -w: warn about malformed check lines
    let mut status = false; // --status: no output, just exit code
    let mut quiet = false; // --quiet: don't print OK lines
    let mut paths: Vec<&str> = Vec::new();
    let mut i = 0;

    while i < args.len() {
        let arg = &args[i];
        match arg.as_str() {
            "--" => {
                i += 1;
                while i < args.len() {
                    paths.push(&args[i]);
                    i += 1;
                }
                break;
            }
            "-c" | "--check" => check_mode = true,
            "-b" | "--binary" => binary = true,
            "-t" | "--text" => binary = false,
            "-w" | "--warn" => warn = true,
            "--status" => status = true,
            "--quiet" => quiet = true,
            a if a.starts_with('-') && a.len() > 1 => {
                return Err(vec![AppletError::invalid_option(
                    applet,
                    a.chars().nth(1).unwrap_or('-'),
                )]);
            }
            _ => paths.push(arg),
        }
        i += 1;
    }

    let _ = binary; // binary/text mode doesn't affect output on Unix

    let mut out = stdout();
    let mut errors = Vec::new();

    if check_mode {
        let mut bad_count = 0usize;
        let mut ok_count = 0usize;
        let mut malformed = 0usize;

        let check_paths: Vec<&str> = if paths.is_empty() {
            vec!["-"]
        } else {
            paths.clone()
        };

        for check_path in check_paths {
            match open_input(check_path) {
                Err(e) => errors.push(AppletError::from_io(applet, "opening", Some(check_path), e)),
                Ok(f) => {
                    let reader = BufReader::new(f);
                    for line in reader.lines() {
                        let line = match line {
                            Ok(l) => l,
                            Err(e) => {
                                errors.push(AppletError::from_io(
                                    applet,
                                    "reading",
                                    Some(check_path),
                                    e,
                                ));
                                break;
                            }
                        };
                        let line = line.trim_end_matches('\r');
                        // Format: "<hash>  <file>" or "<hash> *<file>"
                        let (expected, file) = match parse_check_line(line) {
                            Some(x) => x,
                            None => {
                                if warn && !line.is_empty() && !line.starts_with('#') {
                                    eprintln!(
                                        "{applet}: {check_path}: improperly formatted checksum line"
                                    );
                                    malformed += 1;
                                }
                                continue;
                            }
                        };
                        match hash_file(file, &algo) {
                            Err(e) => {
                                if !status {
                                    writeln!(out, "{file}: FAILED open or read").map_err(|e| {
                                        vec![AppletError::from_io(applet, "writing", None, e)]
                                    })?;
                                }
                                errors.push(AppletError::from_io(applet, "reading", Some(file), e));
                                bad_count += 1;
                            }
                            Ok(actual) => {
                                if actual == expected {
                                    ok_count += 1;
                                    if !status && !quiet {
                                        writeln!(out, "{file}: OK").map_err(|e| {
                                            vec![AppletError::from_io(applet, "writing", None, e)]
                                        })?;
                                    }
                                } else {
                                    bad_count += 1;
                                    if !status {
                                        writeln!(out, "{file}: FAILED").map_err(|e| {
                                            vec![AppletError::from_io(applet, "writing", None, e)]
                                        })?;
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }

        let _ = (ok_count, malformed);

        if bad_count > 0 {
            if !status {
                eprintln!(
                    "{applet}: WARNING: {bad_count} computed checksum{} did NOT match",
                    if bad_count == 1 { "" } else { "s" }
                );
            }
            return Err(vec![]);
        }
    } else {
        // Hash mode: hash each file and print
        let hash_paths: Vec<&str> = if paths.is_empty() {
            vec!["-"]
        } else {
            paths.clone()
        };

        for path in hash_paths {
            match open_input(path) {
                Err(e) => errors.push(AppletError::from_io(applet, "opening", Some(path), e)),
                Ok(mut f) => match hash_reader(&mut f, &algo) {
                    Err(e) => errors.push(AppletError::from_io(applet, "reading", Some(path), e)),
                    Ok(digest) => {
                        let label = if path == "-" { "-" } else { path };
                        writeln!(out, "{digest}  {label}")
                            .map_err(|e| vec![AppletError::from_io(applet, "writing", None, e)])?;
                    }
                },
            }
        }
    }

    if errors.is_empty() {
        Ok(())
    } else {
        Err(errors)
    }
}

fn parse_check_line(line: &str) -> Option<(&str, &str)> {
    // Expect: "<hash>  <file>" or "<hash> *<file>" (GNU format)
    let space = line.find("  ").or_else(|| line.find(" *"))?;
    let hash = &line[..space];
    let rest = &line[space + 2..];
    let file = rest.trim_start_matches('*');
    if hash.is_empty() || file.is_empty() {
        return None;
    }
    // Validate hash is hex
    if !hash.chars().all(|c| c.is_ascii_hexdigit()) {
        return None;
    }
    Some((hash, file))
}

fn hash_file(path: &str, algo: &Algo) -> io::Result<String> {
    let mut f = open_input(path).map_err(|e| io::Error::other(e.to_string()))?;
    hash_reader(&mut f, algo)
}

fn hash_reader(r: &mut dyn Read, algo: &Algo) -> io::Result<String> {
    let mut data = Vec::new();
    r.read_to_end(&mut data)?;
    Ok(match algo {
        Algo::Md5 => hex(&md5::Md5::digest(&data)),
        Algo::Sha1 => hex(&sha1::Sha1::digest(&data)),
        Algo::Sha256 => hex(&sha2::Sha256::digest(&data)),
        Algo::Sha512 => hex(&sha2::Sha512::digest(&data)),
    })
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

#[cfg(test)]
mod tests {
    use super::{Algo, hash_reader, run};

    fn args(v: &[&str]) -> Vec<String> {
        v.iter().map(|s| s.to_string()).collect()
    }

    fn hex_hash(algo: Algo, data: &[u8]) -> String {
        let mut reader = data;
        hash_reader(&mut reader, &algo).unwrap()
    }

    // Known-answer tests against RFC/NIST test vectors

    #[test]
    fn md5_empty() {
        assert_eq!(hex_hash(Algo::Md5, b""), "d41d8cd98f00b204e9800998ecf8427e");
    }

    #[test]
    fn md5_abc() {
        assert_eq!(
            hex_hash(Algo::Md5, b"abc"),
            "900150983cd24fb0d6963f7d28e17f72"
        );
    }

    #[test]
    fn md5_long() {
        assert_eq!(
            hex_hash(
                Algo::Md5,
                b"abcdbcdecdefdefgefghfghighijhijkijkljklmklmnlmnomnopnopq"
            ),
            "8215ef0796a20bcaaae116d3876c664a"
        );
    }

    #[test]
    fn sha1_empty() {
        assert_eq!(
            hex_hash(Algo::Sha1, b""),
            "da39a3ee5e6b4b0d3255bfef95601890afd80709"
        );
    }

    #[test]
    fn sha1_abc() {
        assert_eq!(
            hex_hash(Algo::Sha1, b"abc"),
            "a9993e364706816aba3e25717850c26c9cd0d89d"
        );
    }

    #[test]
    fn sha256_empty() {
        assert_eq!(
            hex_hash(Algo::Sha256, b""),
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
        );
    }

    #[test]
    fn sha256_abc() {
        assert_eq!(
            hex_hash(Algo::Sha256, b"abc"),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
    }

    #[test]
    fn sha512_empty() {
        assert_eq!(
            hex_hash(Algo::Sha512, b""),
            "cf83e1357eefb8bdf1542850d66d8007d620e4050b5715dc83f4a921d36ce9ce\
             47d0d13c5d85f2b0ff8318d2877eec2f63b931bd47417a81a538327af927da3e"
        );
    }

    #[test]
    fn sha512_abc() {
        assert_eq!(
            hex_hash(Algo::Sha512, b"abc"),
            "ddaf35a193617abacc417349ae20413112e6fa4e89a97ea20a9eeee64b55d39a\
             2192992a274fc1a836ba3c23a3feebbd454d4423643ce80e2a9ac94fa54ca49f"
        );
    }

    #[test]
    fn invalid_option_fails() {
        assert!(run(&args(&["-z"]), Algo::Md5).is_err());
    }

    #[test]
    fn nonexistent_file_fails() {
        assert!(run(&args(&["__no_such_file_xyz__"]), Algo::Md5).is_err());
    }
}
