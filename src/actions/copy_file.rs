use super::super::data_shape::{FileItem, RemoteFileItem};
use log::*;
use ssh2;
use std::ffi::OsStr;
use std::{io, fs};
use std::io::prelude::{Read, Write};
use std::path::Path;
use std::time::Instant;
use sha1::{Sha1, Digest};

pub fn write_stream_to_file<T: AsRef<OsStr>>(
    from: &mut impl std::io::Read,
    to_file: T,
) -> Result<(u64, String), failure::Error> {
    let mut u8_buf = [0; 1024];
    let mut length = 0_u64;
    let mut hasher = Sha1::new();
    let mut wf = fs::OpenOptions::new()
        .create(true)
        .write(true)
        .open(to_file.as_ref())?;
    loop {
        match from.read(&mut u8_buf[..]) {
            Ok(n) if n > 0 => {
                length += n as u64;
                wf.write_all(&u8_buf[..n])?;
                hasher.input(&u8_buf[..n]);
            }
            _ => break,
        }
    }
    Ok((length, format!("{:X}", hasher.result())))
}

pub fn write_str_to_file(content: impl AsRef<str>, to_file: impl AsRef<OsStr>) -> Result<(), failure::Error> {
    let mut wf = fs::OpenOptions::new()
        .create(true)
        .write(true)
        .open(to_file.as_ref())?;
        wf.write_all(content.as_ref().as_bytes());
        Ok(())
}

pub fn hash_file_sha1(file_name: impl AsRef<Path>) -> Option<String> {
    // let start = Instant::now();
    let file_r = fs::File::open(file_name.as_ref());
    match file_r {
        Ok(mut file) => {
            let mut hasher = Sha1::new();
            let n_r = io::copy(&mut file, &mut hasher);
            match n_r {
                Ok(n) => {
                    let hash = hasher.result();
                    // println!("Bytes processed: {}", n);
                    let r = format!("{:x}", hash);
                    // println!("r: {:?}, elapsed: {}",r, start.elapsed().as_millis());
                    Some(r)
                }
                Err(err) => {
                    error!("hash_file_sha1 copy stream failed: {:?}, {:?}", file_name.as_ref(), err);
                    None
                }
            }
        }
        Err(err) => {
            error!("hash_file_sha1 failed: {:?}, {:?}", file_name.as_ref(), err);
            None
        }
    }
}

// one possible implementation of walking a directory only visiting files
// https://doc.rust-lang.org/std/fs/fn.read_dir.html
pub fn visit_dirs(dir: &Path, cb: &Fn(&fs::DirEntry)) -> io::Result<()> {
    if dir.is_dir() {
        for entry in fs::read_dir(dir)? {
            let entry = entry?;
            let path = entry.path();
            if path.is_dir() {
                visit_dirs(&path, cb)?;
            } else {
                cb(&entry);
            }
        }
    }
    Ok(())
}

// https://stackoverflow.com/questions/32300132/why-cant-i-store-a-value-and-a-reference-to-that-value-in-the-same-struct

pub fn copy_a_file<'a>(session: &mut ssh2::Session, remote_path: &'a str, local_path: &'a str) -> Result<(), failure::Error> {
    let ri = RemoteFileItem::new(remote_path);
    let fi = FileItem::standalone(local_path, &ri);
    let r = copy_a_file_item(session, fi);

    if let Some(err) = r.get_fail_reason() {
        bail!(err.clone())
    } else {
        Ok(())
    }
}

pub fn copy_a_file_item<'a>(
    session: &mut ssh2::Session,
    mut file_item: FileItem<'a>,
) -> FileItem<'a> {
    let sftp = session.sftp().expect("should got sfpt instance.");
    match sftp.open(Path::new(file_item.remote_item.get_path())) {
        Ok(mut file) => {
            // if let Some(lp) = file_item.get_path() {
                let lp = file_item.get_path();
                match write_stream_to_file(&mut file, lp) {
                    Ok((length, sha1)) => {
                        file_item.set_len(length);
                        file_item.set_sha1(sha1);
                    }
                    Err(err) => {
                        file_item.set_fail_reason(format!("{:?}", err));
                    }
                }
            // } else {
            //     file_item
            //         .set_fail_reason("missing local path.");
            // }
        }
        Err(err) => {
            file_item.set_fail_reason(format!("{:?}", err));
        }
    }
    file_item
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn t_visit() {
        let mut count = 0_u64;
        visit_dirs(Path::new("e:\\"), &|entry| {
            // count += 1;
            println!("{:?}", entry);
        }).expect("success");
    }
}

// fn hash_file_2(file_name: impl AsRef<str>) -> Result<String, failure::Error> {
//     let start = Instant::now();

//     let mut hasher = DefaultHasher::new();

//     let mut file = fs::File::open(file_name.as_ref())?;
//     let mut buffer = [0; 1024];
//     let mut total = 0_usize;
//     loop {
//         let n = file.read(&mut buffer[..])?;
//         if n == 0 {
//             break
//         } else {
//             hasher.write(&buffer[..n]);
//             total += n;
//         }
//     }
//     let hash = hasher.finish();
//     println!("Bytes processed: {}", total);
//     let r = format!("{:x}", hash);
//     println!("r: {:?}, elapsed: {}",r, start.elapsed().as_millis());
//     Ok(r)
// }

// fn hash_file_1(file_name: impl AsRef<str>) -> Result<String, failure::Error> {
//     let start = Instant::now();
//     let mut file = fs::File::open(file_name.as_ref())?;
//     let mut hasher = Sha224::new();
//     let mut buffer = [0; 1024];
//     let mut total = 0_usize;
//     loop {
//         let n = file.read(&mut buffer[..])?;
//         if n == 0 {
//             break
//         } else {
//             hasher.input(&buffer[..n]);
//             total += n;
//         }
//     }
//     let hash = hasher.result();
//     println!("Bytes processed: {}", total);
//     let r = format!("{:x}", hash);
//     println!("r: {:?}, elapsed: {}",r, start.elapsed().as_millis());
//     Ok(r)
// }


// fn hash_file(file_name: impl AsRef<str>) -> Result<String, failure::Error> {
//     let start = Instant::now();
//     let mut file = fs::File::open(file_name.as_ref())?;
//     let mut hasher = Sha224::new();
//     let n = io::copy(&mut file, &mut hasher)?;
//     let hash = hasher.result();
//     println!("Bytes processed: {}", n);
//     let r = format!("{:x}", hash);
//     println!("r: {:?}, elapsed: {}",r, start.elapsed().as_millis());
//     Ok(r)
// }