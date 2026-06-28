use std::{env::current_dir, ffi::OsString, fmt::Display, fs, io::Write, path::PathBuf, process::ExitCode, sync::{Arc, Mutex}, thread::spawn, time::Duration};
use colored::Colorize;
use clap::{Parser, Subcommand, ValueEnum};
use indicatif::MultiProgress;
use tar::Archive;

mod operations;
mod style;

#[derive(Parser)]
#[command(version, about, long_about = None)]
struct Cli {
    #[command(subcommand)]
    command: Option<Subcommands>
}

#[derive(Subcommand)]
enum Subcommands {
    /// List files in archive
    List {
        /// Files
        files: Vec<OsString>
    },

    /// Detect archive format
    Detect {
        /// Files
        files: Vec<OsString>
    },

    /// Extract files from archive
    Extract {
        /// Archive to extract
        file: OsString,

        /// Output directory (defaults to current working directory)
        #[arg(short = 'o')]
        dst: Option<OsString>,

        /// List out entries being processed
        #[arg(short, long, default_value_t = false)]
        verbose: bool,

        /// Mode
        #[arg(short, long, default_value_t = Mode::Direct)]
        mode: Mode,

        /// No progress bar
        #[arg(short, long = "no-progress", default_value_t = false)]
        nopb: bool,

        /// fsync the file before extraction
        #[arg(long, default_value_t = false)]
        sync_before: bool,

        /// Specify the entries to extract
        filters: Vec<OsString>
    },

    /// Create an archive from files
    Create {

    }
}

#[derive(Clone, Copy, Debug)]
enum Format {
    Tar,
    Gzip,
    Xz,
    Zstd
}
impl Display for Format {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match *self {
            Format::Tar => write!(f, "Tar Archive"),
            Format::Gzip => write!(f, "GZip Data"),
            Format::Xz => write!(f, "XZ Data"),
            Format::Zstd => write!(f, "Zstandard Data"),
        }
    }
}

#[derive(Copy, Clone, PartialEq, Eq, PartialOrd, Ord, ValueEnum)]
enum Mode {
    Memory,
    Processor,
    Storage,
    StorageKeep,
    Sync,
    Direct
}

impl Display for Mode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Mode::Memory => write!(f, "memory"),
            Mode::Processor => write!(f, "processor"),
            Mode::Storage => write!(f, "storage"),
            Mode::StorageKeep => write!(f, "storage-keep"),
            Mode::Sync => write!(f, "sync"),
            Mode::Direct => write!(f, "direct")
        }
    }
}

fn main() -> Result<ExitCode, Box<dyn std::error::Error>> {
    let cli = Cli::parse();

    match &cli.command {
        Some(Subcommands::List { files }) => {
            for file in files {
                let fm = operations::determine_format(file)?;
                let mut arq = operations::prepare_archive(file, fm)?;
                // let list = operations::list_archive(arq)?;
                let list = arq.entries()?;
                for e in list {
                    println!("{}", e?.path()?.to_string_lossy());
                }
            }
            Ok(ExitCode::SUCCESS)
        },
        Some(Subcommands::Detect { files }) => {
            let mut suc = true;
            for file in files {
                let fm = operations::determine_format(file);
                if let Ok(fm) = fm {
                    println!("{}: {}", file.to_string_lossy(), fm);
                }
                else {
                    println!("{}, Unknown format", file.to_string_lossy());
                    suc = false;
                }
            }
            Ok(if suc { ExitCode::SUCCESS } else { ExitCode::FAILURE })
        }
        Some(Subcommands::Extract { file, dst, verbose, mode, nopb, sync_before, filters}) => {
            let cwd = current_dir()?;
            let dst = PathBuf::from(if let Some(p) = dst {
                p
            }
            else {
                cwd.as_os_str()
            });
            let name = String::from(PathBuf::from(file).file_name().unwrap().to_string_lossy());
            let name2 = name.clone();
            let verbose = *verbose;
            let filters = filters.clone();
            let fm = operations::determine_format(file)?;
            let mpb = Arc::new(MultiProgress::new());
            let mainpb = mpb.add(style::themed_spinner().with_message("Preparing..."));
            mainpb.enable_steady_tick(Duration::from_millis(100));
            let pb = Arc::new(Mutex::new(None));

            if *sync_before {
                let f = fs::File::open(file)?;
                f.sync_all()?;
            }
            if !*nopb
            {
                match *mode {
                    Mode::Direct => {
                        let f = fs::File::open(file)?;
                        let pb = mpb.add(style::themed_progressbar_bytes(f.metadata()?.len()));
                        let wr = pb.wrap_read(f);
                        let mut arq = operations::prepare_archive_from_read(wr, fm)?;
                        let fname = PathBuf::from(file).file_name().map(|f| f.to_owned());
                        if let Some(name) = fname {
                            mainpb.set_message(format!("Extracting {}...", name.to_string_lossy().yellow().bold()));
                        }
                        else {
                            mainpb.set_message("Extracting...");
                        }
                        arq.unpack(dst)?;
                    }
                    Mode::Processor => {
                        let arq = operations::prepare_archive(file, fm)?;
                        let (ampb, apb) = (mpb.clone(), pb.clone());
                        let _chandler = spawn(move || operations::count_archive_and_add_pb(arq, ampb, apb, name));
                        let arq = operations::prepare_archive(file, fm)?;
                        mainpb.set_message("Extracting...");
                        let ehandler = spawn(move || operations::extract_archive_with_progress(arq, dst, mpb.clone(), name2, mainpb, pb.clone(), verbose, filters));
                        ehandler.join().unwrap();
                    }
                    Mode::Memory => {
                        let mut buf: Vec<u8> = Vec::new();
                        operations::read_compressed_to_buf(file, &mut buf, fm)?;
                        let buf2 = buf.clone();
                        let (ampb, apb) = (mpb.clone(), pb.clone());
                        let _chandler = spawn(move || operations::count_archive_and_add_pb_vec(buf, ampb, apb, name));
                        mainpb.set_message("Extracting...");
                        let ehandler = spawn(move || operations::extract_archive_with_progress_vec(buf2, dst, mpb.clone(), name2, mainpb, pb.clone(), verbose, filters));
                        ehandler.join().unwrap();
                    },
                    Mode::Storage => {
                        if let Format::Tar = fm {
                            mpb.suspend(|| eprintln!("Storage mode is redundant with an uncompressed tar archive"));
                            let (ampb, apb) = (mpb.clone(), pb.clone());
                            let arq = operations::prepare_archive(file, fm)?;
                            let _chandler = spawn(move || operations::count_archive_and_add_pb(arq, ampb, apb, name));
                            let arq = operations::prepare_archive(file, fm)?;
                            mainpb.set_message("Extracting...");
                            let ehandler = spawn(move || operations::extract_archive_with_progress(arq, dst, mpb.clone(), name2, mainpb, pb.clone(), verbose, filters));
                            ehandler.join().unwrap();
                        }
                        else {
                            let fpath = format!(".dtar.{}.tar", file.to_string_lossy());
                            let mut buf = fs::File::create(&fpath)?;
                            operations::read_compressed_to_buf(file, &mut buf, fm)?;
                            buf.flush()?;
                            let buf = fs::File::open(&fpath)?;
                            let buf2 = fs::File::open(&fpath)?;
                            let (ampb, apb) = (mpb.clone(), pb.clone());
                            let arq = Archive::new(buf);
                            let _chandler = spawn(move || operations::count_archive_and_add_pb(arq, ampb, apb, name));
                            let arq = Archive::new(buf2);
                            mainpb.set_message("Extracting...");
                            let ehandler = spawn(move || operations::extract_archive_with_progress(arq, dst, mpb.clone(), name2, mainpb, pb.clone(), verbose, filters));
                            ehandler.join().unwrap();
                            fs::remove_file(&fpath)?;
                        }
                    },
                    Mode::StorageKeep => {
                        if let Format::Tar = fm {
                            mpb.suspend(|| eprintln!("Storage mode is redundant with an uncompressed tar archive"));
                            let (ampb, apb) = (mpb.clone(), pb.clone());
                            let arq = operations::prepare_archive(file, fm)?;
                            let _chandler = spawn(move || operations::count_archive_and_add_pb(arq, ampb, apb, name));
                            let arq = operations::prepare_archive(file, fm)?;
                            mainpb.set_message("Extracting...");
                            let ehandler = spawn(move || operations::extract_archive_with_progress(arq, dst, mpb.clone(), name2, mainpb, pb.clone(), verbose, filters));
                            ehandler.join().unwrap();
                        }
                        else {
                            let fpath = format!(".dtar.{}.tar", file.to_string_lossy());
                            let mut buf = fs::File::create(&fpath)?;
                            operations::read_compressed_to_buf(file, &mut buf, fm)?;
                            buf.flush()?;
                            let buf = fs::File::open(&fpath)?;
                            let buf2 = fs::File::open(&fpath)?;
                            let (ampb, apb) = (mpb.clone(), pb.clone());
                            let arq = Archive::new(buf);
                            let _chandler = spawn(move || operations::count_archive_and_add_pb(arq, ampb, apb, name));
                            let arq = Archive::new(buf2);
                            mainpb.set_message("Extracting...");
                            let ehandler = spawn(move || operations::extract_archive_with_progress(arq, dst, mpb.clone(), name2, mainpb, pb.clone(), verbose, filters));
                            ehandler.join().unwrap();
                            fs::rename(&fpath, format!("{}.tar", file.to_string_lossy()))?;
                        }
                    },
                    Mode::Sync => {
                        let arq = operations::prepare_archive(file, fm)?;
                        operations::count_archive_and_add_pb(arq, mpb.clone(), pb.clone(), name.clone());
                        let arq = operations::prepare_archive(file, fm)?;
                        operations::extract_archive_with_progress(arq, dst, mpb, name, mainpb, pb, verbose, filters);
                    }
                    #[allow(unreachable_patterns)]
                    _ => todo!("Mode not implemented")
                }
            }
            else {
                let arq = operations::prepare_archive(file, fm)?;
                mainpb.set_message("Extracting...");
                operations::extract_archive_with_progress(arq, dst, mpb.clone(), name2, mainpb, pb.clone(), verbose, filters);
            }
            Ok(ExitCode::SUCCESS)
        }
        Some(Subcommands::Create {  }) => todo!(),
        _ => Ok(ExitCode::FAILURE)
    }
}