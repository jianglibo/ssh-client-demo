use std::io;
use pbr::{ProgressBar};

pub fn copy<R: ?Sized, W: ?Sized>(reader: &mut R, writer: &mut W, pb: &mut ProgressBar<io::Stdout>) -> io::Result<u64>
    where R: io::Read, W: io::Write
{
    let mut buf = [0u8; 8*1024];

    let mut written = 0;
    loop {
        let len = match reader.read(&mut buf) {
            Ok(0) => return Ok(written),
            Ok(len) => len,
            Err(ref e) if e.kind() == io::ErrorKind::Interrupted => continue,
            Err(e) => return Err(e),
        };
        pb.add(len as u64);
        writer.write_all(&buf[..len])?;
        written += len as u64;
    }
}