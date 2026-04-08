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

pub fn stdout() -> BufWriter<io::StdoutLock<'static>> {
    let stdout = Box::leak(Box::new(io::stdout()));
    BufWriter::with_capacity(BUFFER_SIZE, stdout.lock())
}
