// Implements md5sum, sha1sum, sha256sum, sha512sum.
// All hash algorithms are implemented from scratch without external crates.

use std::io::{self, BufRead, BufReader, Read, Write};

use crate::common::applet::{AppletResult, finish};
use crate::common::error::AppletError;
use crate::common::io::{open_input, stdout};

pub fn main_md5sum(args: &[String]) -> i32 { finish(run(args, Algo::Md5)) }
pub fn main_sha1sum(args: &[String]) -> i32 { finish(run(args, Algo::Sha1)) }
pub fn main_sha256sum(args: &[String]) -> i32 { finish(run(args, Algo::Sha256)) }
pub fn main_sha512sum(args: &[String]) -> i32 { finish(run(args, Algo::Sha512)) }

enum Algo { Md5, Sha1, Sha256, Sha512 }

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
    let mut binary = false;   // -b: binary mode (only affects display marker on non-Unix)
    let mut warn = false;     // -w: warn about malformed check lines
    let mut status = false;   // --status: no output, just exit code
    let mut quiet = false;    // --quiet: don't print OK lines
    let mut paths: Vec<&str> = Vec::new();
    let mut i = 0;

    while i < args.len() {
        let arg = &args[i];
        match arg.as_str() {
            "--" => {
                i += 1;
                while i < args.len() { paths.push(&args[i]); i += 1; }
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

        let check_paths: Vec<&str> = if paths.is_empty() { vec!["-"] } else { paths.clone() };

        for check_path in check_paths {
            match open_input(check_path) {
                Err(e) => errors.push(AppletError::from_io(applet, "opening", Some(check_path), e)),
                Ok(f) => {
                    let reader = BufReader::new(f);
                    for line in reader.lines() {
                        let line = match line {
                            Ok(l) => l,
                            Err(e) => {
                                errors.push(AppletError::from_io(applet, "reading", Some(check_path), e));
                                break;
                            }
                        };
                        let line = line.trim_end_matches('\r');
                        // Format: "<hash>  <file>" or "<hash> *<file>"
                        let (expected, file) = match parse_check_line(line) {
                            Some(x) => x,
                            None => {
                                if warn && !line.is_empty() && !line.starts_with('#') {
                                    eprintln!("{applet}: {check_path}: improperly formatted checksum line");
                                    malformed += 1;
                                }
                                continue;
                            }
                        };
                        match hash_file(file, &algo) {
                            Err(e) => {
                                if !status {
                                    writeln!(out, "{file}: FAILED open or read")
                                        .map_err(|e| vec![AppletError::from_io(applet, "writing", None, e)])?;
                                }
                                errors.push(AppletError::from_io(applet, "reading", Some(file), e));
                                bad_count += 1;
                            }
                            Ok(actual) => {
                                if actual == expected {
                                    ok_count += 1;
                                    if !status && !quiet {
                                        writeln!(out, "{file}: OK")
                                            .map_err(|e| vec![AppletError::from_io(applet, "writing", None, e)])?;
                                    }
                                } else {
                                    bad_count += 1;
                                    if !status {
                                        writeln!(out, "{file}: FAILED")
                                            .map_err(|e| vec![AppletError::from_io(applet, "writing", None, e)])?;
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
                eprintln!("{applet}: WARNING: {bad_count} computed checksum{} did NOT match",
                    if bad_count == 1 { "" } else { "s" });
            }
            return Err(vec![]);
        }
    } else {
        // Hash mode: hash each file and print
        let hash_paths: Vec<&str> = if paths.is_empty() { vec!["-"] } else { paths.clone() };

        for path in hash_paths {
            match open_input(path) {
                Err(e) => errors.push(AppletError::from_io(applet, "opening", Some(path), e)),
                Ok(mut f) => {
                    match hash_reader(&mut f, &algo) {
                        Err(e) => errors.push(AppletError::from_io(applet, "reading", Some(path), e)),
                        Ok(digest) => {
                            let label = if path == "-" { "-" } else { path };
                            writeln!(out, "{digest}  {label}")
                                .map_err(|e| vec![AppletError::from_io(applet, "writing", None, e)])?;
                        }
                    }
                }
            }
        }
    }

    if errors.is_empty() { Ok(()) } else { Err(errors) }
}

fn parse_check_line(line: &str) -> Option<(&str, &str)> {
    // Expect: "<hash>  <file>" or "<hash> *<file>" (GNU format)
    let space = line.find("  ").or_else(|| line.find(" *"))?;
    let hash = &line[..space];
    let rest = &line[space + 2..];
    let file = rest.trim_start_matches('*');
    if hash.is_empty() || file.is_empty() { return None; }
    // Validate hash is hex
    if !hash.chars().all(|c| c.is_ascii_hexdigit()) { return None; }
    Some((hash, file))
}

fn hash_file(path: &str, algo: &Algo) -> io::Result<String> {
    let mut f = if path == "-" {
        open_input("-").map_err(|e| io::Error::other(e.to_string()))?
    } else {
        open_input(path).map_err(|e| io::Error::other(e.to_string()))?
    };
    hash_reader(&mut f, algo)
}

fn hash_reader(r: &mut dyn Read, algo: &Algo) -> io::Result<String> {
    let mut data = Vec::new();
    r.read_to_end(&mut data)?;
    Ok(match algo {
        Algo::Md5 => hex(&md5(&data)),
        Algo::Sha1 => hex(&sha1(&data)),
        Algo::Sha256 => hex(&sha256(&data)),
        Algo::Sha512 => hex(&sha512(&data)),
    })
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

// ── MD5 (RFC 1321) ──────────────────────────────────────────────────────────

fn md5(data: &[u8]) -> [u8; 16] {
    // Per-round shift amounts
    const S: [u32; 64] = [
        7,12,17,22, 7,12,17,22, 7,12,17,22, 7,12,17,22,
        5, 9,14,20, 5, 9,14,20, 5, 9,14,20, 5, 9,14,20,
        4,11,16,23, 4,11,16,23, 4,11,16,23, 4,11,16,23,
        6,10,15,21, 6,10,15,21, 6,10,15,21, 6,10,15,21,
    ];
    // Precomputed table: K[i] = floor(2^32 * |sin(i+1)|)
    const K: [u32; 64] = [
        0xd76aa478,0xe8c7b756,0x242070db,0xc1bdceee,0xf57c0faf,0x4787c62a,0xa8304613,0xfd469501,
        0x698098d8,0x8b44f7af,0xffff5bb1,0x895cd7be,0x6b901122,0xfd987193,0xa679438e,0x49b40821,
        0xf61e2562,0xc040b340,0x265e5a51,0xe9b6c7aa,0xd62f105d,0x02441453,0xd8a1e681,0xe7d3fbc8,
        0x21e1cde6,0xc33707d6,0xf4d50d87,0x455a14ed,0xa9e3e905,0xfcefa3f8,0x676f02d9,0x8d2a4c8a,
        0xfffa3942,0x8771f681,0x6d9d6122,0xfde5380c,0xa4beea44,0x4bdecfa9,0xf6bb4b60,0xbebfbc70,
        0x289b7ec6,0xeaa127fa,0xd4ef3085,0x04881d05,0xd9d4d039,0xe6db99e5,0x1fa27cf8,0xc4ac5665,
        0xf4292244,0x432aff97,0xab9423a7,0xfc93a039,0x655b59c3,0x8f0ccc92,0xffeff47d,0x85845dd1,
        0x6fa87e4f,0xfe2ce6e0,0xa3014314,0x4e0811a1,0xf7537e82,0xbd3af235,0x2ad7d2bb,0xeb86d391,
    ];

    let bit_len = (data.len() as u64).wrapping_mul(8);
    let mut msg = data.to_vec();
    msg.push(0x80);
    while msg.len() % 64 != 56 { msg.push(0); }
    msg.extend_from_slice(&bit_len.to_le_bytes());

    let mut a0: u32 = 0x67452301;
    let mut b0: u32 = 0xefcdab89;
    let mut c0: u32 = 0x98badcfe;
    let mut d0: u32 = 0x10325476;

    for chunk in msg.chunks_exact(64) {
        let mut m = [0u32; 16];
        for (i, w) in m.iter_mut().enumerate() {
            *w = u32::from_le_bytes(chunk[i*4..i*4+4].try_into().unwrap());
        }
        let (mut a, mut b, mut c, mut d) = (a0, b0, c0, d0);
        for i in 0usize..64 {
            let (f, g) = match i {
                0..=15  => ((b & c) | (!b & d), i),
                16..=31 => ((d & b) | (!d & c), (5*i+1) % 16),
                32..=47 => (b ^ c ^ d, (3*i+5) % 16),
                _       => (c ^ (b | !d), (7*i) % 16),
            };
            let temp = d;
            d = c;
            c = b;
            b = b.wrapping_add(
                (a.wrapping_add(f).wrapping_add(K[i]).wrapping_add(m[g]))
                    .rotate_left(S[i])
            );
            a = temp;
        }
        a0 = a0.wrapping_add(a);
        b0 = b0.wrapping_add(b);
        c0 = c0.wrapping_add(c);
        d0 = d0.wrapping_add(d);
    }

    let mut out = [0u8; 16];
    out[0..4].copy_from_slice(&a0.to_le_bytes());
    out[4..8].copy_from_slice(&b0.to_le_bytes());
    out[8..12].copy_from_slice(&c0.to_le_bytes());
    out[12..16].copy_from_slice(&d0.to_le_bytes());
    out
}

// ── SHA-1 (FIPS 180-4) ──────────────────────────────────────────────────────

fn sha1(data: &[u8]) -> [u8; 20] {
    let bit_len = (data.len() as u64).wrapping_mul(8);
    let mut msg = data.to_vec();
    msg.push(0x80);
    while msg.len() % 64 != 56 { msg.push(0); }
    msg.extend_from_slice(&bit_len.to_be_bytes());

    let mut h = [0x67452301u32, 0xefcdab89, 0x98badcfe, 0x10325476, 0xc3d2e1f0];

    for chunk in msg.chunks_exact(64) {
        let mut w = [0u32; 80];
        for (i, word) in w.iter_mut().take(16).enumerate() {
            *word = u32::from_be_bytes(chunk[i*4..i*4+4].try_into().unwrap());
        }
        for i in 16..80 {
            w[i] = (w[i-3] ^ w[i-8] ^ w[i-14] ^ w[i-16]).rotate_left(1);
        }
        let (mut a, mut b, mut c, mut d, mut e) = (h[0], h[1], h[2], h[3], h[4]);
        for i in 0usize..80 {
            let (f, k) = match i {
                0..=19  => ((b & c) | (!b & d), 0x5a827999u32),
                20..=39 => (b ^ c ^ d,          0x6ed9eba1),
                40..=59 => ((b & c) | (b & d) | (c & d), 0x8f1bbcdc),
                _       => (b ^ c ^ d,          0xca62c1d6),
            };
            let temp = a.rotate_left(5)
                .wrapping_add(f)
                .wrapping_add(e)
                .wrapping_add(k)
                .wrapping_add(w[i]);
            e = d; d = c; c = b.rotate_left(30); b = a; a = temp;
        }
        h[0] = h[0].wrapping_add(a);
        h[1] = h[1].wrapping_add(b);
        h[2] = h[2].wrapping_add(c);
        h[3] = h[3].wrapping_add(d);
        h[4] = h[4].wrapping_add(e);
    }

    let mut out = [0u8; 20];
    for (i, &word) in h.iter().enumerate() {
        out[i*4..i*4+4].copy_from_slice(&word.to_be_bytes());
    }
    out
}

// ── SHA-256 (FIPS 180-4) ────────────────────────────────────────────────────

const SHA256_K: [u32; 64] = [
    0x428a2f98,0x71374491,0xb5c0fbcf,0xe9b5dba5,0x3956c25b,0x59f111f1,0x923f82a4,0xab1c5ed5,
    0xd807aa98,0x12835b01,0x243185be,0x550c7dc3,0x72be5d74,0x80deb1fe,0x9bdc06a7,0xc19bf174,
    0xe49b69c1,0xefbe4786,0x0fc19dc6,0x240ca1cc,0x2de92c6f,0x4a7484aa,0x5cb0a9dc,0x76f988da,
    0x983e5152,0xa831c66d,0xb00327c8,0xbf597fc7,0xc6e00bf3,0xd5a79147,0x06ca6351,0x14292967,
    0x27b70a85,0x2e1b2138,0x4d2c6dfc,0x53380d13,0x650a7354,0x766a0abb,0x81c2c92e,0x92722c85,
    0xa2bfe8a1,0xa81a664b,0xc24b8b70,0xc76c51a3,0xd192e819,0xd6990624,0xf40e3585,0x106aa070,
    0x19a4c116,0x1e376c08,0x2748774c,0x34b0bcb5,0x391c0cb3,0x4ed8aa4a,0x5b9cca4f,0x682e6ff3,
    0x748f82ee,0x78a5636f,0x84c87814,0x8cc70208,0x90befffa,0xa4506ceb,0xbef9a3f7,0xc67178f2,
];

fn sha256(data: &[u8]) -> [u8; 32] {
    let bit_len = (data.len() as u64).wrapping_mul(8);
    let mut msg = data.to_vec();
    msg.push(0x80);
    while msg.len() % 64 != 56 { msg.push(0); }
    msg.extend_from_slice(&bit_len.to_be_bytes());

    let mut h = [
        0x6a09e667u32, 0xbb67ae85, 0x3c6ef372, 0xa54ff53a,
        0x510e527f, 0x9b05688c, 0x1f83d9ab, 0x5be0cd19,
    ];

    for chunk in msg.chunks_exact(64) {
        let mut w = [0u32; 64];
        for (i, word) in w.iter_mut().take(16).enumerate() {
            *word = u32::from_be_bytes(chunk[i*4..i*4+4].try_into().unwrap());
        }
        for i in 16..64 {
            let s0 = w[i-15].rotate_right(7) ^ w[i-15].rotate_right(18) ^ (w[i-15] >> 3);
            let s1 = w[i-2].rotate_right(17) ^ w[i-2].rotate_right(19) ^ (w[i-2] >> 10);
            w[i] = w[i-16].wrapping_add(s0).wrapping_add(w[i-7]).wrapping_add(s1);
        }
        let (mut a, mut b, mut c, mut d, mut e, mut f, mut g, mut hh) =
            (h[0], h[1], h[2], h[3], h[4], h[5], h[6], h[7]);
        for i in 0..64 {
            let s1 = e.rotate_right(6) ^ e.rotate_right(11) ^ e.rotate_right(25);
            let ch = (e & f) ^ (!e & g);
            let temp1 = hh.wrapping_add(s1).wrapping_add(ch).wrapping_add(SHA256_K[i]).wrapping_add(w[i]);
            let s0 = a.rotate_right(2) ^ a.rotate_right(13) ^ a.rotate_right(22);
            let maj = (a & b) ^ (a & c) ^ (b & c);
            let temp2 = s0.wrapping_add(maj);
            hh = g; g = f; f = e;
            e = d.wrapping_add(temp1);
            d = c; c = b; b = a;
            a = temp1.wrapping_add(temp2);
        }
        h[0] = h[0].wrapping_add(a); h[1] = h[1].wrapping_add(b);
        h[2] = h[2].wrapping_add(c); h[3] = h[3].wrapping_add(d);
        h[4] = h[4].wrapping_add(e); h[5] = h[5].wrapping_add(f);
        h[6] = h[6].wrapping_add(g); h[7] = h[7].wrapping_add(hh);
    }

    let mut out = [0u8; 32];
    for (i, &word) in h.iter().enumerate() {
        out[i*4..i*4+4].copy_from_slice(&word.to_be_bytes());
    }
    out
}

// ── SHA-512 (FIPS 180-4) ────────────────────────────────────────────────────

const SHA512_K: [u64; 80] = [
    0x428a2f98d728ae22,0x7137449123ef65cd,0xb5c0fbcfec4d3b2f,0xe9b5dba58189dbbc,
    0x3956c25bf348b538,0x59f111f1b605d019,0x923f82a4af194f9b,0xab1c5ed5da6d8118,
    0xd807aa98a3030242,0x12835b0145706fbe,0x243185be4ee4b28c,0x550c7dc3d5ffb4e2,
    0x72be5d74f27b896f,0x80deb1fe3b1696b1,0x9bdc06a725c71235,0xc19bf174cf692694,
    0xe49b69c19ef14ad2,0xefbe4786384f25e3,0x0fc19dc68b8cd5b5,0x240ca1cc77ac9c65,
    0x2de92c6f592b0275,0x4a7484aa6ea6e483,0x5cb0a9dcbd41fbd4,0x76f988da831153b5,
    0x983e5152ee66dfab,0xa831c66d2db43210,0xb00327c898fb213f,0xbf597fc7beef0ee4,
    0xc6e00bf33da88fc2,0xd5a79147930aa725,0x06ca6351e003826f,0x142929670a0e6e70,
    0x27b70a8546d22ffc,0x2e1b21385c26c926,0x4d2c6dfc5ac42aed,0x53380d139d95b3df,
    0x650a73548baf63de,0x766a0abb3c77b2a8,0x81c2c92e47edaee6,0x92722c851482353b,
    0xa2bfe8a14cf10364,0xa81a664bbc423001,0xc24b8b70d0f89791,0xc76c51a30654be30,
    0xd192e819d6ef5218,0xd69906245565a910,0xf40e35855771202a,0x106aa07032bbd1b8,
    0x19a4c116b8d2d0c8,0x1e376c085141ab53,0x2748774cdf8eeb99,0x34b0bcb5e19b48a8,
    0x391c0cb3c5c95a63,0x4ed8aa4ae3418acb,0x5b9cca4f7763e373,0x682e6ff3d6b2b8a3,
    0x748f82ee5defb2fc,0x78a5636f43172f60,0x84c87814a1f0ab72,0x8cc702081a6439ec,
    0x90befffa23631e28,0xa4506cebde82bde9,0xbef9a3f7b2c67915,0xc67178f2e372532b,
    0xca273eceea26619c,0xd186b8c721c0c207,0xeada7dd6cde0eb1e,0xf57d4f7fee6ed178,
    0x06f067aa72176fba,0x0a637dc5a2c898a6,0x113f9804bef90dae,0x1b710b35131c471b,
    0x28db77f523047d84,0x32caab7b40c72493,0x3c9ebe0a15c9bebc,0x431d67c49c100d4c,
    0x4cc5d4becb3e42b6,0x597f299cfc657e2a,0x5fcb6fab3ad6faec,0x6c44198c4a475817,
];

fn sha512(data: &[u8]) -> [u8; 64] {
    let bit_len = (data.len() as u128).wrapping_mul(8);
    let mut msg = data.to_vec();
    msg.push(0x80);
    while msg.len() % 128 != 112 { msg.push(0); }
    msg.extend_from_slice(&bit_len.to_be_bytes());

    let mut h: [u64; 8] = [
        0x6a09e667f3bcc908, 0xbb67ae8584caa73b, 0x3c6ef372fe94f82b, 0xa54ff53a5f1d36f1,
        0x510e527fade682d1, 0x9b05688c2b3e6c1f, 0x1f83d9abfb41bd6b, 0x5be0cd19137e2179,
    ];

    for chunk in msg.chunks_exact(128) {
        let mut w = [0u64; 80];
        for (i, word) in w.iter_mut().take(16).enumerate() {
            *word = u64::from_be_bytes(chunk[i*8..i*8+8].try_into().unwrap());
        }
        for i in 16..80 {
            let s0 = w[i-15].rotate_right(1) ^ w[i-15].rotate_right(8) ^ (w[i-15] >> 7);
            let s1 = w[i-2].rotate_right(19) ^ w[i-2].rotate_right(61) ^ (w[i-2] >> 6);
            w[i] = w[i-16].wrapping_add(s0).wrapping_add(w[i-7]).wrapping_add(s1);
        }
        let (mut a, mut b, mut c, mut d, mut e, mut f, mut g, mut hh) =
            (h[0], h[1], h[2], h[3], h[4], h[5], h[6], h[7]);
        for i in 0..80 {
            let s1 = e.rotate_right(14) ^ e.rotate_right(18) ^ e.rotate_right(41);
            let ch = (e & f) ^ (!e & g);
            let temp1 = hh.wrapping_add(s1).wrapping_add(ch).wrapping_add(SHA512_K[i]).wrapping_add(w[i]);
            let s0 = a.rotate_right(28) ^ a.rotate_right(34) ^ a.rotate_right(39);
            let maj = (a & b) ^ (a & c) ^ (b & c);
            let temp2 = s0.wrapping_add(maj);
            hh = g; g = f; f = e;
            e = d.wrapping_add(temp1);
            d = c; c = b; b = a;
            a = temp1.wrapping_add(temp2);
        }
        h[0] = h[0].wrapping_add(a); h[1] = h[1].wrapping_add(b);
        h[2] = h[2].wrapping_add(c); h[3] = h[3].wrapping_add(d);
        h[4] = h[4].wrapping_add(e); h[5] = h[5].wrapping_add(f);
        h[6] = h[6].wrapping_add(g); h[7] = h[7].wrapping_add(hh);
    }

    let mut out = [0u8; 64];
    for (i, &word) in h.iter().enumerate() {
        out[i*8..i*8+8].copy_from_slice(&word.to_be_bytes());
    }
    out
}

#[cfg(test)]
mod tests {
    use super::{hex, md5, sha1, sha256, sha512, run};

    fn args(v: &[&str]) -> Vec<String> {
        v.iter().map(|s| s.to_string()).collect()
    }

    // Known-answer tests against RFC/NIST test vectors

    #[test]
    fn md5_empty() {
        assert_eq!(hex(&md5(b"")), "d41d8cd98f00b204e9800998ecf8427e");
    }

    #[test]
    fn md5_abc() {
        assert_eq!(hex(&md5(b"abc")), "900150983cd24fb0d6963f7d28e17f72");
    }

    #[test]
    fn md5_long() {
        assert_eq!(
            hex(&md5(b"abcdbcdecdefdefgefghfghighijhijkijkljklmklmnlmnomnopnopq")),
            "8215ef0796a20bcaaae116d3876c664a"
        );
    }

    #[test]
    fn sha1_empty() {
        assert_eq!(hex(&sha1(b"")), "da39a3ee5e6b4b0d3255bfef95601890afd80709");
    }

    #[test]
    fn sha1_abc() {
        assert_eq!(hex(&sha1(b"abc")), "a9993e364706816aba3e25717850c26c9cd0d89d");
    }

    #[test]
    fn sha256_empty() {
        assert_eq!(
            hex(&sha256(b"")),
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
        );
    }

    #[test]
    fn sha256_abc() {
        assert_eq!(
            hex(&sha256(b"abc")),
            "ba7816bf8f01cfea414140de5dae2ec73b00361bbef0469348423f656b305a7c"
            // Note: correct SHA-256 of "abc"
        );
    }

    #[test]
    fn sha512_empty() {
        assert_eq!(
            hex(&sha512(b"")),
            "cf83e1357eefb8bdf1542850d66d8007d620e4050b5715dc83f4a921d36ce9ce47d0d13c5d85f2b0ff8318d2877eec2f63b931bd47417a81a538327af927da3e"
        );
    }

    #[test]
    fn sha512_abc() {
        assert_eq!(
            hex(&sha512(b"abc")),
            "ddaf35a193617abacc417349ae20413112e6fa4e89a97ea20a9eeee64b55d39a2192992a274fc1a836ba3c23a3feebbd454d4423643ce80e2a9ac94fa54ca49f"
        );
    }

    #[test]
    fn invalid_option_fails() {
        assert!(run(&args(&["-z"]), super::Algo::Md5).is_err());
    }

    #[test]
    fn nonexistent_file_fails() {
        assert!(run(&args(&["__no_such_file_xyz__"]), super::Algo::Md5).is_err());
    }
}
