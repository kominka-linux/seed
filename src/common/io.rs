use std::fs::File;
use std::io::{self, BufWriter, Read, Write};

pub const BUFFER_SIZE: usize = 64 * 1024;

pub fn copy_stream<R: Read, W: Write>(reader: &mut R, writer: &mut W) -> io::Result<u64> {
    let mut buf = [0_u8; BUFFER_SIZE];
    let mut total = 0_u64;

    loop {
        let read = reader.read(&mut buf)?;
        if read == 0 {
            return Ok(total);
        }
        writer.write_all(&buf[..read])?;
        total += read as u64;
    }
}

pub fn stdout() -> BufWriter<io::Stdout> {
    BufWriter::with_capacity(BUFFER_SIZE, io::stdout())
}

pub fn stdin() -> io::Stdin {
    io::stdin()
}

pub enum Input {
    Stdin(io::Stdin),
    File(File),
}

impl Read for Input {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        match self {
            Input::Stdin(reader) => reader.read(buf),
            Input::File(reader) => reader.read(buf),
        }
    }
}

pub fn open_input(path: &str) -> io::Result<Input> {
    if path == "-" {
        Ok(Input::Stdin(stdin()))
    } else {
        File::open(path).map(Input::File)
    }
}
