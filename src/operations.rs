use std::{error::Error, ffi::{OsStr, OsString}, fs::{self, File}, io::{self, BufReader, Read, Write}, path::PathBuf, sync::{Arc, Mutex}};
use flate2::bufread::GzDecoder;
use indicatif::{MultiProgress, ProgressBar};
use xz2::bufread::XzDecoder;
use tar::Archive;

use crate::Format;

pub fn determine_format(path: &OsStr) -> Result<Format, Box<dyn std::error::Error>> {
    let mut fd = File::open(path)?;
    let mut mn: [u8; 300] = [0; 300];
    fd.read_exact(&mut mn)?;
    match &mn[..2] {
        [0x1f, 0x8b] => Ok(Format::Gzip),
        _ => {
            match &mn[..6] {
                [0xfd, 0x37, 0x7a, 0x58, 0x5a, 0x00] => Ok(Format::Xz),
                _ => match &mn[257..262] {
                    [0x75, 0x73, 0x74, 0x61, 0x72] => Ok(Format::Tar),
                    _ => match &mn[..4] {
                        [0x28, 0xb5, 0x2f, 0xfd] => Ok(Format::Zstd),
                        _ => Err(FormatError {}.into())
                    }
                }
            }
        }
    }
}

#[derive(Debug)]
struct FormatError {}
impl Error for FormatError {}
impl std::fmt::Display for FormatError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Unknown file format")
    }
}

pub fn read_compressed_to_buf(path: &OsStr, buf: &mut impl Write, format: Format) -> io::Result<()> {
    match format {
        Format::Gzip => {
            let r = BufReader::new(File::open(path)?);
            let mut dec = GzDecoder::new(r);
            io::copy(&mut dec, buf)?;
            Ok(())
        }
        Format::Xz => {
            let r = BufReader::new(File::open(path)?);
            let mut dec = XzDecoder::new(r);
            io::copy(&mut dec, buf)?;
            Ok(())
        }
        Format::Tar => {
            let mut r: BufReader<File> = BufReader::new(File::open(path)?);
            io::copy(&mut r, buf)?;
            Ok(())
        },
        Format::Zstd => {
            let r: BufReader<File> = BufReader::new(File::open(path)?);
            let mut dec = zstd::Decoder::new(r)?;
            io::copy(&mut dec, buf)?;
            Ok(())
        }
        // _ => Err(Box::new(FormatError {}))
    }
}

pub fn prepare_archive(path: &OsStr, format: Format) -> Result<Archive<Box<dyn Read + Send>>, Box<dyn std::error::Error>> {
    match format {
        Format::Gzip => {
            let r = BufReader::new(File::open(path)?);
            let dec = GzDecoder::new(r);
            let arq = Archive::new(Box::new(dec) as Box<dyn Read + Send>);
            Ok(arq)
        }
        Format::Xz => {
            let r = BufReader::new(File::open(path)?);
            let dec = XzDecoder::new(r);
            let arq = Archive::new(Box::new(dec) as Box<dyn Read + Send>);
            Ok(arq)
        }
        Format::Tar => {
            let r = BufReader::new(File::open(path)?);
            let arq = Archive::new(Box::new(r) as Box<dyn Read + Send>);
            Ok(arq)
        }
        Format::Zstd => {
            let r = BufReader::new(File::open(path)?);
            let dec = zstd::Decoder::new(r)?;
            let arq = Archive::new(Box::new(dec) as Box<dyn Read + Send>);
            Ok(arq)
        }
        // _ => Err(Box::new(FormatError {}))
    }
}

pub fn _list_archive(mut arq: Archive<impl Read>) -> Result<Vec<OsString>, Box<dyn std::error::Error>> {
    let mut res: Vec<OsString> = Vec::new();
    let entries = arq.entries()?;
    for e in entries {
        let e = e?;
        let p = e.path()?;
        res.push(p.as_os_str().to_owned());
    }
    Ok(res)
}

pub fn count_archive_and_add_pb_vec(buf: Vec<u8>, mpb: Arc<MultiProgress>, pb: Arc<Mutex<Option<ProgressBar>>>, name: String) {
    let arq = Archive::new(buf.as_slice());
    count_archive_and_add_pb(arq, mpb, pb, name);
}

pub  fn count_archive_and_add_pb(mut arq: Archive<impl Read>, mpb: Arc<MultiProgress>, pb: Arc<Mutex<Option<ProgressBar>>>, name: String) {
    let count = arq.entries().unwrap().count();
    let mut pb = pb.lock().unwrap();
    *pb = Some(mpb.add(crate::style::themed_progressbar(count as u64).with_message(format!("Extracting {name}..."))));
}

#[allow(clippy::too_many_arguments)]
pub fn extract_archive_with_progress_vec(buf: Vec<u8>, dst: PathBuf, mpb: Arc<MultiProgress>, name: String, mainpb: ProgressBar, pbr: Arc<Mutex<Option<ProgressBar>>>, verbose: bool, filters: Vec<OsString>) {
    let arq = Archive::new(buf.as_slice());
    extract_archive_with_progress(arq, dst, mpb, name, mainpb, pbr, verbose, filters);
}

#[allow(clippy::too_many_arguments)]
pub fn extract_archive_with_progress(mut arq: Archive<impl Read>, dst: PathBuf, mpb: Arc<MultiProgress>, name: String, mainpb: ProgressBar, pbr: Arc<Mutex<Option<ProgressBar>>>, verbose: bool, filters: Vec<OsString>) {
    let mut sofar: u64 = 0;
    let mut skip: u64 = 0;
    let mut pb_exists = false;

    if let Some(ref pb) = *pbr.lock().unwrap() {
        pb_exists = true;
        pb.set_message(format!("Extracting {name}..."));
    }

    if dst.symlink_metadata().is_err() {
        fs::create_dir_all(&dst).unwrap();
    }

    let dst = &dst.canonicalize().unwrap_or(dst.to_path_buf());

    let mut directories = Vec::new();
    for entry in arq.entries().unwrap() {
        let mut file = entry.unwrap();
        let fpath = file.path().unwrap();
        let fname = fpath.file_name();
        if !filters.is_empty() {
            let mut pass = false;
            for filter in &filters {
                if fpath.starts_with(filter) {
                    pass = true;
                }
            }
            if !pass {
                skip += 1;
                if let Some(ref pb) = *pbr.lock().unwrap() {
                    pb_exists = true;
                    pb.set_length(pb.length().unwrap() - skip);
                    skip = 0;
                }
                continue;
            }
        }
        if let Some(name) = fname {
            if !pb_exists {
                mainpb.set_message(format!("({sofar}) Extracting {}...", name.to_string_lossy()));
            }
            else {
                mainpb.set_message(format!("Extracting {}...", name.to_string_lossy()));
            }
        }
        else {
            if !pb_exists {
                mainpb.set_message(format!("({sofar}) Extracting..."));
            }
            else {
                mainpb.set_message("Extracting...");
            }
        }
        if verbose
            && let Some(name) = fname {
                mpb.println(format!("Extracting {}...", name.to_string_lossy())).unwrap();
        }
        if file.header().entry_type() == tar::EntryType::Directory {
            directories.push(file);
        } else {
            file.unpack_in(dst).unwrap();
        }
        sofar += 1;
        if let Some(ref pb) = *pbr.lock().unwrap() {
            pb_exists = true;
            pb.set_position(sofar);
        }
    }

    directories.sort_by(|a, b| b.path_bytes().cmp(&a.path_bytes()));
    for mut dir in directories {
        dir.unpack_in(dst).unwrap();
        sofar += 1;
        if let Some(ref pb) = *pbr.lock().unwrap() {
            // pb_exists = true;
            pb.set_position(sofar);
        }
    }
    if let Some(ref pb) = *pbr.lock().unwrap() {
        // pb_exists = true;
        pb.finish_with_message(format!("Extracted {}", name));
    }
    mainpb.finish_with_message("Done");
}
