use std::{error::Error, ffi::{OsStr, OsString}, fs::{self, File}, io::{self, BufReader, Read}, path::PathBuf};
use flate2::bufread::GzDecoder;
use indicatif::{HumanCount, MultiProgress, ProgressBar};
use xz2::bufread::XzDecoder;
use tar::Archive;
use colored::Colorize;

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

pub fn prepare_archive_from_read(reader: impl io::Read + Send + 'static, format: Format) -> Result<Archive<Box<dyn Read + Send>>, Box<dyn std::error::Error>> {
    match format {
        Format::Gzip => {
            let r = BufReader::new(reader);
            let dec = GzDecoder::new(r);
            let arq = Archive::new(Box::new(dec) as Box<dyn Read + Send>);
            Ok(arq)
        }
        Format::Xz => {
            let r = BufReader::new(reader);
            let dec = XzDecoder::new(r);
            let arq = Archive::new(Box::new(dec) as Box<dyn Read + Send>);
            Ok(arq)
        }
        Format::Tar => {
            let r = BufReader::new(reader);
            let arq = Archive::new(Box::new(r) as Box<dyn Read + Send>);
            Ok(arq)
        }
        Format::Zstd => {
            let r = BufReader::new(reader);
            let dec = zstd::Decoder::new(r)?;
            let arq = Archive::new(Box::new(dec) as Box<dyn Read + Send>);
            Ok(arq)
        }
        // _ => Err(Box::new(FormatError {}))
    }
}

pub fn prepare_archive(path: &OsStr, format: Format) -> Result<Archive<Box<dyn Read + Send>>, Box<dyn std::error::Error>> {
    let f = File::open(path)?;
    prepare_archive_from_read(f, format)
}

#[allow(clippy::too_many_arguments)]
pub fn extract_archive_with_pb(mut arq: Archive<impl Read>, dst: PathBuf, mpb: &MultiProgress, name: String, mainpb: &ProgressBar, pb: &ProgressBar, verbose: bool, filters: Vec<OsString>) {
    pb.set_message(format!("Extracting {}...", name.cyan().bold()));

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
                continue;
            }
        }
        if let Some(name) = fname {
            mainpb.set_message(format!("Extracting {}...", name.to_string_lossy().yellow().bold()));
        }
        else {
            mainpb.set_message("Extracting...");
        }
        if verbose
            && let Some(name) = fname {
                mpb.println(format!("Extracting {}...", name.to_string_lossy().yellow().bold())).unwrap();
        }
        if file.header().entry_type() == tar::EntryType::Directory {
            directories.push(file);
        } else {
            file.unpack_in(dst).unwrap();
        }
    }

    directories.sort_by(|a, b| b.path_bytes().cmp(&a.path_bytes()));
    for mut dir in directories {
        dir.unpack_in(dst).unwrap();
    }
    pb.finish_with_message(format!("Extracted {}", name.cyan().bold()));
    mainpb.finish_with_message("Done");
}

pub fn extract_archive_no_pb(mut arq: Archive<impl Read>, dst: PathBuf, mpb: &MultiProgress, mainpb: &ProgressBar, verbose: bool, filters: Vec<OsString>) {
    let mut sofar: u64 = 0;
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
                continue;
            }
        }
        if let Some(name) = fname {
            mainpb.set_message(format!("{} Extracting {}...", format!("({})", HumanCount(sofar)).white(), name.to_string_lossy().yellow().bold()));
        }
        else {
            mainpb.set_message(format!("{} Extracting...", format!("({})", HumanCount(sofar)).white()));
        }
        if verbose
            && let Some(name) = fname {
                mpb.println(format!("Extracting {}...", name.to_string_lossy().yellow().bold())).unwrap();
        }
        if file.header().entry_type() == tar::EntryType::Directory {
            directories.push(file);
        } else {
            file.unpack_in(dst).unwrap();
        }
        sofar += 1;
    }

    directories.sort_by(|a, b| b.path_bytes().cmp(&a.path_bytes()));
    for mut dir in directories {
        dir.unpack_in(dst).unwrap();
    }
    mainpb.finish_with_message("Done");
}