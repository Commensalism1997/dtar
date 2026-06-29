use std::{error::Error, ffi::{OsStr, OsString}, fs::{self, File}, io::{self, BufReader, Read, Write}, os::unix::fs::MetadataExt, path::{Path, PathBuf}, time::Duration};
use flate2::{Compression, bufread::GzDecoder, write::GzEncoder};
use indicatif::{HumanCount, MultiProgress, ProgressBar, ProgressStyle};
use xz2::{bufread::XzDecoder, write::XzEncoder};
use tar::{Archive, Builder};
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
    let r = BufReader::with_capacity(1024*1024, reader);
    match format {
        Format::Gzip => {
            let dec = GzDecoder::new(r);
            let arq = Archive::new(Box::new(dec) as Box<dyn Read + Send>);
            Ok(arq)
        }
        Format::Xz => {
            let dec = XzDecoder::new(r);
            let arq = Archive::new(Box::new(dec) as Box<dyn Read + Send>);
            Ok(arq)
        }
        Format::Tar => {
            let arq = Archive::new(Box::new(r) as Box<dyn Read + Send>);
            Ok(arq)
        }
        Format::Zstd => {
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
pub fn extract_archive_with_pb(mut arq: Archive<impl Read>, dst: PathBuf, mpb: &MultiProgress, name: String, mainpb: &ProgressBar, pb: &ProgressBar, verbose: bool, filters: Vec<OsString>) -> Result<(), Box<dyn std::error::Error>> {
    pb.set_message(format!("Extracting {}...", name.cyan().bold()));

    if dst.symlink_metadata().is_err() {
        fs::create_dir_all(&dst).unwrap();
    }

    let dst = &dst.canonicalize().unwrap_or(dst.to_path_buf());

    let mut directories = Vec::new();
    for entry in arq.entries()? {
        let mut file = entry?;
        let fpath = file.path()?;
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
                mpb.println(format!("Extracting {}...", name.to_string_lossy().yellow().bold()))?;
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
    Ok(())
}

pub fn create_archive_progress(writer: impl Write, paths: &Vec<impl AsRef<Path>>, content: Option<impl AsRef<Path>>, fm: Format, level: i32, totalpb: bool) -> Result<(), Box<dyn std::error::Error>> {
    let mpb = MultiProgress::new();
    let procpb = mpb.add(crate::style::themed_spinner().with_message("Preparing..."));
    procpb.enable_steady_tick(Duration::from_millis(100));
    let mut total = 0u64;
    if let Some(ref cont) = content {
        let p = cont.as_ref();
        total += fs_extra::dir::get_size(p.canonicalize()?)?;
    }
    else {
        if totalpb {
            for p in paths {
                let p = p.as_ref();
                if p.is_dir() {
                    total += fs_extra::dir::get_size(p.canonicalize()?)?;
                }
                else if p.is_file() {
                    total += fs::metadata(p.canonicalize()?)?.size();
                }
            }
        }
    }
    let readpb = if totalpb {
        mpb.add(crate::style::themed_progressbar_bytes_green(total))
    }
    else {
        mpb.add(crate::style::themed_spinner())
    };
    let writepb = mpb.add(crate::style::themed_spinner());
    if !totalpb {
        readpb.set_style(ProgressStyle::with_template("{spinner} {bytes:.yellow} read")?);
    }
    writepb.set_style(ProgressStyle::with_template("{spinner} {bytes:.magenta} written")?);
    let wwr = writepb.wrap_write(writer);
    let rwr = match fm {
        Format::Tar => {
            readpb.wrap_write(Box::new(wwr) as Box<dyn Write>)
        }
        Format::Gzip => {
            let writer = GzEncoder::new(wwr, Compression::new(level as u32));
            readpb.wrap_write(Box::new(writer) as Box<dyn Write>)
        }
        Format::Xz => {
            let writer = XzEncoder::new(wwr, level as u32);
            readpb.wrap_write(Box::new(writer) as Box<dyn Write>)
        }
        Format::Zstd => {
            let writer = zstd::Encoder::new(wwr, level)?;
            readpb.wrap_write(Box::new(writer) as Box<dyn Write>)
        }
    };
    let mut tar = Builder::new(rwr);
    tar.follow_symlinks(false);
    if let Some(ref cont) = content {
        let p = cont.as_ref();
        if let Some(fname) = p.file_name() {
            procpb.set_message(format!("Adding contents of {}...", fname.to_string_lossy().green().bold()));
        }
        else {
            procpb.set_message("Adding contents...");
        }
        tar.append_dir_all("", p)?;
    }
    else {
        for p in paths {
            let p = p.as_ref();
            if let Some(fname) = p.file_name() {
                procpb.set_message(format!("Adding {}...", fname.to_string_lossy().green().bold()));
            }
            else {
                procpb.set_message("Adding...");
            }
            if p.is_dir() {
                tar.append_dir_all(p, p)?;
            }
            else if p.is_file() {
                tar.append_path(p)?;
            }
        }
    }
    tar.finish()?;
    readpb.finish_and_clear();
    writepb.finish_and_clear();
    procpb.finish_with_message("Done");
    Ok(())
}

pub fn extract_archive_no_pb(mut arq: Archive<impl Read>, dst: PathBuf, mpb: &MultiProgress, mainpb: &ProgressBar, verbose: bool, filters: Vec<OsString>) -> Result<(), Box<dyn std::error::Error>> {
    let mut sofar: u64 = 0;
    if dst.symlink_metadata().is_err() {
        fs::create_dir_all(&dst).unwrap();
    }

    let dst = &dst.canonicalize().unwrap_or(dst.to_path_buf());

    let mut directories = Vec::new();
    for entry in arq.entries()? {
        let mut file = entry?;
        let fpath = file.path()?;
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
                mpb.println(format!("Extracting {}...", name.to_string_lossy().yellow().bold()))?;
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
    Ok(())
}