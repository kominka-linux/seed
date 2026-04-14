use std::fs::{self, OpenOptions};
use std::io::{Read, Seek, SeekFrom, Write};

use crate::common::applet::finish;
use crate::common::error::AppletError;

const APPLET: &str = "mkswap";
const SWAP_MAGIC: &[u8; 10] = b"SWAPSPACE2";
const HEADER_OFFSET: u64 = 1024;
const HEADER_SIZE: usize = 129 * 4;
const SWAP_UUID_OFFSET: usize = 12;
const SWAP_LABEL_OFFSET: usize = 28;

#[derive(Clone, Debug, Default, Eq, PartialEq)]
struct Options {
    label: Option<String>,
    path: String,
    size_kib: Option<u64>,
}

pub fn main(args: &[String]) -> i32 {
    finish(run(args))
}

fn run(args: &[String]) -> Result<(), Vec<AppletError>> {
    let options = parse_args(args)?;
    write_swap_header(&options)
}

fn parse_args(args: &[String]) -> Result<Options, Vec<AppletError>> {
    let mut label = None;
    let mut operands = Vec::new();
    let mut index = 0;

    while index < args.len() {
        let arg = &args[index];
        index += 1;
        if arg == "-L" {
            let Some(value) = args.get(index) else {
                return Err(vec![AppletError::option_requires_arg(APPLET, "L")]);
            };
            index += 1;
            label = Some(value.clone());
            continue;
        }
        if arg.starts_with('-') {
            return Err(vec![AppletError::invalid_option(
                APPLET,
                arg.chars().nth(1).unwrap_or('-'),
            )]);
        }
        operands.push(arg.clone());
    }

    let [path, size] = match operands.as_slice() {
        [path] => [path.clone(), String::new()],
        [path, size] => [path.clone(), size.clone()],
        [] => return Err(vec![AppletError::new(APPLET, "missing operand")]),
        _ => return Err(vec![AppletError::new(APPLET, "extra operand")]),
    };

    let size_kib = if size.is_empty() {
        None
    } else {
        Some(size.parse::<u64>().map_err(|_| {
            vec![AppletError::new(
                APPLET,
                format!("invalid size '{size}'"),
            )]
        })?)
    };

    Ok(Options {
        label,
        path,
        size_kib,
    })
}

fn write_swap_header(options: &Options) -> Result<(), Vec<AppletError>> {
    if let Some(label) = &options.label
        && label.len() > 16
    {
        return Err(vec![AppletError::new(APPLET, "label may not exceed 16 bytes")]);
    }

    let mut file = OpenOptions::new()
        .read(true)
        .write(true)
        .open(&options.path)
        .map_err(|err| vec![AppletError::from_io(APPLET, "opening", Some(&options.path), err)])?;

    if let Some(size_kib) = options.size_kib {
        file.set_len(size_kib.saturating_mul(1024))
            .map_err(|err| vec![AppletError::from_io(APPLET, "resizing", Some(&options.path), err)])?;
    }

    let page_size = page_size()?;
    let size = file
        .metadata()
        .map_err(|err| vec![AppletError::from_io(APPLET, "reading", Some(&options.path), err)])?
        .len();
    if size < (page_size as u64) * 10 {
        return Err(vec![AppletError::new(APPLET, "swap area needs to be at least 10 pages")]);
    }

    let last_page = size
        .checked_div(page_size as u64)
        .and_then(|pages| pages.checked_sub(1))
        .ok_or_else(|| vec![AppletError::new(APPLET, "swap area is too small")])?;

    let zeroes = vec![0_u8; HEADER_OFFSET as usize];
    let mut header = vec![0_u8; HEADER_SIZE];
    header[0..4].copy_from_slice(&1_u32.to_ne_bytes());
    header[4..8].copy_from_slice(&(last_page as u32).to_ne_bytes());
    header[8..12].copy_from_slice(&0_u32.to_ne_bytes());
    fill_random(&mut header[SWAP_UUID_OFFSET..SWAP_UUID_OFFSET + 16])?;
    if let Some(label) = &options.label {
        header[SWAP_LABEL_OFFSET..SWAP_LABEL_OFFSET + label.len()].copy_from_slice(label.as_bytes());
    }
    let magic_offset = page_size - SWAP_MAGIC.len();

    file.seek(SeekFrom::Start(0))
        .and_then(|_| file.write_all(&zeroes))
        .and_then(|_| file.seek(SeekFrom::Start(HEADER_OFFSET)))
        .and_then(|_| file.write_all(&header))
        .and_then(|_| file.seek(SeekFrom::Start(magic_offset as u64)))
        .and_then(|_| file.write_all(SWAP_MAGIC))
        .and_then(|_| file.flush())
        .and_then(|_| file.sync_all())
        .map_err(|err| vec![AppletError::from_io(APPLET, "writing", Some(&options.path), err)])?;
    Ok(())
}

fn fill_random(buffer: &mut [u8]) -> Result<(), Vec<AppletError>> {
    match fs::File::open("/dev/urandom").and_then(|mut file| file.read_exact(buffer)) {
        Ok(()) => Ok(()),
        Err(err) => Err(vec![AppletError::from_io(
            APPLET,
            "reading",
            Some("/dev/urandom"),
            err,
        )]),
    }
}

fn page_size() -> Result<usize, Vec<AppletError>> {
    // SAFETY: sysconf reads process-global configuration and has no side effects.
    let value = unsafe { libc::sysconf(libc::_SC_PAGESIZE) };
    if value <= 0 {
        Err(vec![AppletError::new(APPLET, "reading page size failed")])
    } else {
        Ok(value as usize)
    }
}

#[cfg(test)]
mod tests {
    use super::{SWAP_MAGIC, parse_args, write_swap_header};
    use std::fs;
    use std::io::{Read, Seek, SeekFrom};

    fn args(values: &[&str]) -> Vec<String> {
        values.iter().map(|value| value.to_string()).collect()
    }

    #[test]
    fn parses_label_and_size() {
        let options = parse_args(&args(&["-L", "swap0", "file", "8192"])).unwrap();
        assert_eq!(options.label.as_deref(), Some("swap0"));
        assert_eq!(options.path, "file");
        assert_eq!(options.size_kib, Some(8192));
    }

    #[test]
    fn writes_magic_signature() {
        let path = std::env::temp_dir().join(format!("seed-mkswap-{}", std::process::id()));
        fs::write(&path, vec![0_u8; super::page_size().unwrap() * 10]).unwrap();
        write_swap_header(&super::Options {
            label: Some(String::from("swap0")),
            path: path.to_string_lossy().into_owned(),
            size_kib: None,
        })
        .unwrap();
        let mut file = fs::File::open(&path).unwrap();
        let page_size = super::page_size().unwrap();
        file.seek(SeekFrom::Start((page_size - SWAP_MAGIC.len()) as u64)).unwrap();
        let mut trailer = [0_u8; 10];
        file.read_exact(&mut trailer).unwrap();
        assert_eq!(&trailer, SWAP_MAGIC);
        let _ = fs::remove_file(path);
    }
}
