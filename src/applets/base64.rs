// Implements base32 and base64 encode/decode.

use std::io::{self, Read, Write};

use crate::common::applet::{AppletResult, finish};
use crate::common::error::AppletError;
use crate::common::io::{open_input, stdout};

pub fn main_base64(args: &[String]) -> i32 { finish(run(args, Variant::B64)) }
pub fn main_base32(args: &[String]) -> i32 { finish(run(args, Variant::B32)) }

enum Variant { B64, B32 }

impl Variant {
    fn name(&self) -> &'static str {
        match self { Variant::B64 => "base64", Variant::B32 => "base32" }
    }
}

fn run(args: &[String], variant: Variant) -> AppletResult {
    let applet = variant.name();
    let mut decode = false;
    let mut ignore_garbage = false;
    let mut wrap: usize = 76;
    let mut paths: Vec<&str> = Vec::new();
    let mut i = 0;

    while i < args.len() {
        let arg = &args[i];
        match arg.as_str() {
            "--" => {
                for arg in &args[i + 1..] { paths.push(arg); }
                break;
            }
            "-d" | "--decode" => decode = true,
            "-i" | "--ignore-garbage" => ignore_garbage = true,
            "-w" | "--wrap" => {
                i += 1;
                let val = args.get(i)
                    .ok_or_else(|| vec![AppletError::option_requires_arg(applet, "-w")])?;
                wrap = val.parse()
                    .map_err(|_| vec![AppletError::new(applet, format!("invalid wrap size '{val}'"))])?;
            }
            a if a.starts_with("--wrap=") => {
                let val = &a[7..];
                wrap = val.parse()
                    .map_err(|_| vec![AppletError::new(applet, format!("invalid wrap size '{val}'"))])?;
            }
            a if a.starts_with("-w") && a.len() > 2 => {
                let val = &a[2..];
                wrap = val.parse()
                    .map_err(|_| vec![AppletError::new(applet, format!("invalid wrap size '{val}'"))])?;
            }
            a if a.starts_with('-') && a.len() > 1 => {
                return Err(vec![AppletError::invalid_option(applet, a.chars().nth(1).unwrap())]);
            }
            a => paths.push(a),
        }
        i += 1;
    }

    let paths: Vec<&str> = if paths.is_empty() { vec!["-"] } else { paths };
    let mut out = stdout();
    let mut errors = Vec::new();

    for &path in &paths {
        match open_input(path) {
            Err(e) => errors.push(AppletError::from_io(applet, "opening", Some(path), e)),
            Ok(mut f) => {
                let mut data = Vec::new();
                if let Err(e) = f.read_to_end(&mut data) {
                    errors.push(AppletError::from_io(applet, "reading", Some(path), e));
                    continue;
                }
                if decode {
                    let filtered = match filter_alphabet(&data, &variant, ignore_garbage) {
                        Ok(v) => v,
                        Err(msg) => { errors.push(AppletError::new(applet, msg)); continue; }
                    };
                    match decode_data(&filtered, &variant) {
                        Ok(decoded) => {
                            if let Err(e) = out.write_all(&decoded) {
                                return Err(vec![AppletError::from_io(applet, "writing", None, e)]);
                            }
                        }
                        Err(msg) => errors.push(AppletError::new(applet, msg)),
                    }
                } else {
                    let encoded = encode_data(&data, &variant);
                    if let Err(e) = write_wrapped(&mut out, &encoded, wrap) {
                        return Err(vec![AppletError::from_io(applet, "writing", None, e)]);
                    }
                }
            }
        }
    }

    if errors.is_empty() { Ok(()) } else { Err(errors) }
}

fn encode_data(data: &[u8], variant: &Variant) -> String {
    match variant { Variant::B64 => b64_encode(data), Variant::B32 => b32_encode(data) }
}

fn decode_data(filtered: &[u8], variant: &Variant) -> Result<Vec<u8>, String> {
    match variant { Variant::B64 => b64_decode(filtered), Variant::B32 => b32_decode(filtered) }
}

fn filter_alphabet(data: &[u8], variant: &Variant, ignore_garbage: bool) -> Result<Vec<u8>, String> {
    let mut out = Vec::new();
    for &b in data {
        if b.is_ascii_whitespace() { continue; }
        let valid = b == b'=' || match variant {
            Variant::B64 => b64_val(b).is_some(),
            Variant::B32 => b32_val(b).is_some(),
        };
        if valid {
            out.push(b);
        } else if !ignore_garbage {
            return Err("invalid input".into());
        }
    }
    Ok(out)
}

fn write_wrapped<W: Write>(out: &mut W, encoded: &str, wrap: usize) -> io::Result<()> {
    if wrap == 0 || encoded.is_empty() {
        return writeln!(out, "{encoded}");
    }
    for chunk in encoded.as_bytes().chunks(wrap) {
        out.write_all(chunk)?;
        out.write_all(b"\n")?;
    }
    Ok(())
}

// Base64

const B64_ALPHA: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

fn b64_encode(data: &[u8]) -> String {
    let mut out = Vec::with_capacity((data.len() + 2) / 3 * 4);
    for chunk in data.chunks(3) {
        let a = chunk[0];
        let b = chunk.get(1).copied().unwrap_or(0);
        let c = chunk.get(2).copied().unwrap_or(0);
        let n = chunk.len();
        out.push(B64_ALPHA[((a >> 2) & 0x3f) as usize]);
        out.push(B64_ALPHA[(((a & 3) << 4) | (b >> 4)) as usize]);
        out.push(if n >= 2 { B64_ALPHA[(((b & 0xf) << 2) | (c >> 6)) as usize] } else { b'=' });
        out.push(if n == 3 { B64_ALPHA[(c & 0x3f) as usize] } else { b'=' });
    }
    String::from_utf8(out).unwrap()
}

fn b64_val(c: u8) -> Option<u8> {
    match c {
        b'A'..=b'Z' => Some(c - b'A'),
        b'a'..=b'z' => Some(c - b'a' + 26),
        b'0'..=b'9' => Some(c - b'0' + 52),
        b'+' => Some(62),
        b'/' => Some(63),
        _ => None,
    }
}

fn b64_decode(data: &[u8]) -> Result<Vec<u8>, String> {
    let mut out = Vec::new();
    let mut i = 0;
    while i < data.len() {
        if data.len() - i < 2 { return Err("invalid base64 data".into()); }
        let a = b64_val(data[i]).ok_or("invalid base64 data")?;
        let b = b64_val(data[i + 1]).ok_or("invalid base64 data")?;
        out.push((a << 2) | (b >> 4));

        let c_byte = data.get(i + 2).copied().unwrap_or(b'=');
        let d_byte = data.get(i + 3).copied().unwrap_or(b'=');
        if c_byte != b'=' {
            let c = b64_val(c_byte).ok_or("invalid base64 data")?;
            out.push(((b & 0xf) << 4) | (c >> 2));
            if d_byte != b'=' {
                let d = b64_val(d_byte).ok_or("invalid base64 data")?;
                out.push(((c & 3) << 6) | d);
            }
        }
        i += 4;
    }
    Ok(out)
}

// Base32
//
// Alphabet: ABCDEFGHIJKLMNOPQRSTUVWXYZ234567 (RFC 4648)
// 5 bytes map to 8 base32 characters; padding fills groups to 8 chars.

const B32_ALPHA: &[u8; 32] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZ234567";

fn b32_encode(data: &[u8]) -> String {
    let mut out = Vec::new();
    for chunk in data.chunks(5) {
        let a = chunk[0];
        let b = chunk.get(1).copied().unwrap_or(0);
        let c = chunk.get(2).copied().unwrap_or(0);
        let d = chunk.get(3).copied().unwrap_or(0);
        let e = chunk.get(4).copied().unwrap_or(0);
        let n = chunk.len();

        out.push(B32_ALPHA[(a >> 3) as usize]);
        out.push(B32_ALPHA[(((a & 7) << 2) | (b >> 6)) as usize]);
        out.push(if n >= 2 { B32_ALPHA[((b >> 1) & 0x1f) as usize] } else { b'=' });
        out.push(if n >= 2 { B32_ALPHA[(((b & 1) << 4) | (c >> 4)) as usize] } else { b'=' });
        out.push(if n >= 3 { B32_ALPHA[(((c & 0xf) << 1) | (d >> 7)) as usize] } else { b'=' });
        out.push(if n >= 4 { B32_ALPHA[((d >> 2) & 0x1f) as usize] } else { b'=' });
        out.push(if n >= 4 { B32_ALPHA[(((d & 3) << 3) | (e >> 5)) as usize] } else { b'=' });
        out.push(if n == 5 { B32_ALPHA[(e & 0x1f) as usize] } else { b'=' });
    }
    String::from_utf8(out).unwrap()
}

fn b32_val(c: u8) -> Option<u8> {
    match c {
        b'A'..=b'Z' => Some(c - b'A'),
        b'a'..=b'z' => Some(c - b'a'),  // case-insensitive
        b'2'..=b'7' => Some(c - b'2' + 26),
        _ => None,
    }
}

fn b32_decode(data: &[u8]) -> Result<Vec<u8>, String> {
    let mut out = Vec::new();
    let mut i = 0;
    while i < data.len() {
        let get = |j: usize| data.get(i + j).copied().unwrap_or(b'=');
        let (c0, c1) = (get(0), get(1));
        if c0 == b'=' { break; }
        let v0 = b32_val(c0).ok_or("invalid base32 data")?;
        let v1 = b32_val(c1).ok_or("invalid base32 data")?;
        out.push((v0 << 3) | (v1 >> 2));  // byte 0: bits 39-32

        let (c2, c3) = (get(2), get(3));
        if c2 == b'=' { i += 8; continue; }
        let v2 = b32_val(c2).ok_or("invalid base32 data")?;
        let v3 = b32_val(c3).ok_or("invalid base32 data")?;
        out.push(((v1 & 3) << 6) | (v2 << 1) | (v3 >> 4));  // byte 1: bits 31-24

        let c4 = get(4);
        if c4 == b'=' { i += 8; continue; }
        let v4 = b32_val(c4).ok_or("invalid base32 data")?;
        out.push(((v3 & 0xf) << 4) | (v4 >> 1));  // byte 2: bits 23-16

        let (c5, c6) = (get(5), get(6));
        if c5 == b'=' { i += 8; continue; }
        let v5 = b32_val(c5).ok_or("invalid base32 data")?;
        let v6 = b32_val(c6).ok_or("invalid base32 data")?;
        out.push(((v4 & 1) << 7) | (v5 << 2) | (v6 >> 3));  // byte 3: bits 15-8

        let c7 = get(7);
        if c7 == b'=' { i += 8; continue; }
        let v7 = b32_val(c7).ok_or("invalid base32 data")?;
        out.push(((v6 & 7) << 5) | v7);  // byte 4: bits 7-0

        i += 8;
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(v: &[&str]) -> Vec<String> {
        v.iter().map(|s| s.to_string()).collect()
    }

    // RFC 4648 §10 base64 test vectors
    #[test]
    fn b64_encode_rfc_vectors() {
        assert_eq!(b64_encode(b""), "");
        assert_eq!(b64_encode(b"f"), "Zg==");
        assert_eq!(b64_encode(b"fo"), "Zm8=");
        assert_eq!(b64_encode(b"foo"), "Zm9v");
        assert_eq!(b64_encode(b"foob"), "Zm9vYg==");
        assert_eq!(b64_encode(b"fooba"), "Zm9vYmE=");
        assert_eq!(b64_encode(b"foobar"), "Zm9vYmFy");
    }

    #[test]
    fn b64_decode_rfc_vectors() {
        assert_eq!(b64_decode(b"").unwrap(), b"");
        assert_eq!(b64_decode(b"Zg==").unwrap(), b"f");
        assert_eq!(b64_decode(b"Zm8=").unwrap(), b"fo");
        assert_eq!(b64_decode(b"Zm9v").unwrap(), b"foo");
        assert_eq!(b64_decode(b"Zm9vYmFy").unwrap(), b"foobar");
    }

    #[test]
    fn b64_roundtrip_binary() {
        let data: Vec<u8> = (0u8..=255).collect();
        let encoded = b64_encode(&data);
        assert_eq!(b64_decode(encoded.as_bytes()).unwrap(), data);
    }

    // RFC 4648 §10 base32 test vectors
    #[test]
    fn b32_encode_rfc_vectors() {
        assert_eq!(b32_encode(b""), "");
        assert_eq!(b32_encode(b"f"), "MY======");
        assert_eq!(b32_encode(b"fo"), "MZXQ====");
        assert_eq!(b32_encode(b"foo"), "MZXW6===");
        assert_eq!(b32_encode(b"foob"), "MZXW6YQ=");
        assert_eq!(b32_encode(b"fooba"), "MZXW6YTB");
        assert_eq!(b32_encode(b"foobar"), "MZXW6YTBOI======");
    }

    #[test]
    fn b32_decode_rfc_vectors() {
        assert_eq!(b32_decode(b"").unwrap(), b"");
        assert_eq!(b32_decode(b"MY======").unwrap(), b"f");
        assert_eq!(b32_decode(b"MZXQ====").unwrap(), b"fo");
        assert_eq!(b32_decode(b"MZXW6===").unwrap(), b"foo");
        assert_eq!(b32_decode(b"MZXW6YQ=").unwrap(), b"foob");
        assert_eq!(b32_decode(b"MZXW6YTB").unwrap(), b"fooba");
        assert_eq!(b32_decode(b"MZXW6YTBOI======").unwrap(), b"foobar");
    }

    #[test]
    fn b32_decode_case_insensitive() {
        assert_eq!(b32_decode(b"my======").unwrap(), b"f");
        assert_eq!(b32_decode(b"mzxw6ytb").unwrap(), b"fooba");
    }

    #[test]
    fn b32_roundtrip_binary() {
        // Test all byte values 0–127 in a chunk
        let data: Vec<u8> = (0u8..128).collect();
        let encoded = b32_encode(&data);
        assert_eq!(b32_decode(encoded.as_bytes()).unwrap(), data);
    }

    #[test]
    fn filter_strips_whitespace() {
        let input = b"Zm9v\nYmFy";
        let filtered = filter_alphabet(input, &Variant::B64, false).unwrap();
        assert_eq!(b64_decode(&filtered).unwrap(), b"foobar");
    }

    #[test]
    fn filter_rejects_garbage_without_flag() {
        let input = b"Zm9v!YmFy";
        assert!(filter_alphabet(input, &Variant::B64, false).is_err());
    }

    #[test]
    fn filter_ignores_garbage_with_flag() {
        let input = b"Zm9v!YmFy";
        let filtered = filter_alphabet(input, &Variant::B64, true).unwrap();
        assert_eq!(b64_decode(&filtered).unwrap(), b"foobar");
    }

    #[test]
    fn invalid_option_fails() {
        assert!(run(&args(&["-z"]), Variant::B64).is_err());
        assert!(run(&args(&["-z"]), Variant::B32).is_err());
    }

    #[test]
    fn wrap_option() {
        // -w 4 wraps every 4 chars
        let data = b"foobar";
        let encoded = b64_encode(data);  // "Zm9vYmFy"
        assert_eq!(encoded.len(), 8);
        // Verify wrap logic: 8-char encoded with wrap=4 → two lines
        let mut out = Vec::new();
        write_wrapped(&mut out, &encoded, 4).unwrap();
        let s = String::from_utf8(out).unwrap();
        assert_eq!(s, "Zm9v\nYmFy\n");
    }

    #[test]
    fn wrap_zero_no_wrapping() {
        let encoded = "A".repeat(200);
        let mut out = Vec::new();
        write_wrapped(&mut out, &encoded, 0).unwrap();
        let s = String::from_utf8(out).unwrap();
        assert_eq!(s, format!("{encoded}\n"));
    }

    #[test]
    fn empty_input_writes_newline() {
        let mut out = Vec::new();
        write_wrapped(&mut out, "", 76).unwrap();
        assert_eq!(out, b"\n");
    }
}
