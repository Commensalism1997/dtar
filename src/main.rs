use std::{env::current_dir, ffi::OsString, fmt::Display, fs::{self, File}, io::BufWriter, path::PathBuf, process::ExitCode, time::Duration};
use colored::Colorize;
use clap::{Parser, Subcommand, ValueEnum};
use indicatif::MultiProgress;

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
        files: Vec<OsString>,

        /// Output a clean list; don't indent, color, and list file names
        #[arg(short, long, default_value_t = false)]
        clean: bool,
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
        /// Archive to create
        file: OsString,

        /// Create an archive out of contents of a directory. If this is specified, PATHS are ignored. 
        #[arg(short, long)]
        content: Option<OsString>,

        /// Specify a format (by default guesses from file extension).
        #[arg(short, long)]
        format: Option<Format>,

        /// Specify compression level. gzip and xz use 0-9, bzip2 uses 1-9, zstd uses 1-22.
        #[arg(short, long)]
        level: Option<i32>,

        /// Overwrite the file if it exists.
        #[arg(short = 'y', long)]
        overwrite: bool,

        /// Abort if file already exists. Overrides --overwrite.
        #[arg(short = 'n', long = "no-clobber")]
        noclobber: bool,

        /// Don't show the progress bar. Might be faster by foregoing counting the total bytes.
        #[arg(short = 'b', long = "no-progress")]
        noprogress: bool,

        /// Paths to include in the archive
        paths: Vec<OsString>
    }
}

#[derive(Clone, Copy, Debug, ValueEnum)]
enum Format {
    Tar,
    Gzip,
    Bzip2,
    Xz,
    Zstd
}
impl Display for Format {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match *self {
            Format::Tar => write!(f, "Tar Archive"),
            Format::Gzip => write!(f, "GZip Data"),
            Format::Bzip2 => write!(f, "BZip2 Data"),
            Format::Xz => write!(f, "XZ Data"),
            Format::Zstd => write!(f, "Zstandard Data")
        }
    }
}

fn main() -> Result<ExitCode, Box<dyn std::error::Error>> {
    let cli = Cli::parse();

    match &cli.command {
        Some(Subcommands::List { files, clean }) => {
            let clean = *clean;
            for file in files {
                let fm = operations::determine_format(file)?;
                let mut arq = operations::prepare_archive(file, fm)?;
                // let list = operations::list_archive(arq)?;
                let list = arq.entries()?;
                
                if !clean {
                    println!("{}", file.to_string_lossy().cyan().bold());
                    for e in list {
                        let _e = e?;
                        let _p = _e.path()?;
                        let p = _p.to_string_lossy();
                        if let Some((dir, file)) = p.rsplit_once('/') {
                            if !file.is_empty() {
                                println!("{} {}{}", "├─".white(), format!("{}/", dir).white(), file.yellow().bold());
                            }
                            else {
                                if let Some((parentpath, dirname)) = dir.rsplit_once('/') {
                                    println!("{} {}{}", "├─".white(), format!("{}/", parentpath).white(), format!("{}/", dirname).blue().bold());
                                }
                            }
                        }
                        else {
                            println!("{} {}", "├─".white(), p.yellow().bold());
                        }
                    }
                    println!("\x1b[1A\r{}", "└".white());
                }
                else {
                    for e in list {
                        println!("{}", e?.path()?.to_string_lossy());
                    }
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
        Some(Subcommands::Extract { file, dst, verbose, nopb, sync_before, filters}) => {
            let cwd = current_dir()?;
            let dst = PathBuf::from(if let Some(p) = dst {
                p
            }
            else {
                cwd.as_os_str()
            });
            let name = String::from(PathBuf::from(file).file_name().unwrap().to_string_lossy());
            let verbose = *verbose;
            let filters = filters.clone();
            let fm = operations::determine_format(file)?;
            let mpb = MultiProgress::new();
            let mainpb = mpb.add(style::themed_spinner().with_message("Preparing..."));
            mainpb.enable_steady_tick(Duration::from_millis(100));

            if *sync_before {
                let f = fs::File::open(file)?;
                f.sync_all()?;
            }
            if !*nopb
            {
                let f = fs::File::open(file)?;
                let pb = mpb.add(style::themed_progressbar_bytes_blue(f.metadata()?.len()));
                let wr = pb.wrap_read(f);
                let arq = operations::prepare_archive_from_read(wr, fm)?;
                let fname = PathBuf::from(file).file_name().map(|f| f.to_owned());
                if let Some(name) = fname {
                    mainpb.set_message(format!("Extracting {}...", name.to_string_lossy().yellow().bold()));
                }
                else {
                    mainpb.set_message("Extracting...");
                }
                operations::extract_archive_with_pb(arq, dst, &mpb, name, &mainpb, &pb, verbose, filters)?;
            }
            else {
                let arq = operations::prepare_archive(file, fm)?;
                mainpb.set_message("Extracting...");
                operations::extract_archive_no_pb(arq, dst, &mpb, &mainpb, verbose, filters)?;
            }
            Ok(ExitCode::SUCCESS)
        }
        Some(Subcommands::Create { file, paths, content, format, level, overwrite, noclobber, noprogress }) => {
            if fs::exists(file)? {
                if *noclobber {
                    println!("File exists, aborting");
                    return Ok(ExitCode::FAILURE)
                }
                if !*overwrite {
                    let q = dialoguer::Confirm::new().with_prompt(format!("Overwrite existing {}?", file.to_string_lossy().cyan().bold())).default(false).interact_opt()?;
                    if let Some(res) = q {
                        if !res {
                            return Ok(ExitCode::FAILURE)
                        }
                    }
                    else {
                        return Ok(ExitCode::FAILURE)
                    }
                }
            }
            let writer = BufWriter::with_capacity(1024*1024, File::create(file)?);
            let _p = PathBuf::from(file);
            let ext = _p.extension();
            let fm: Format = if let Some(qr) = format {
                *qr
            }
            else {
                if let Some(e) = ext {
                    let e: String = e.to_string_lossy().into();
                    match e.as_str() {
                        "tar" => Format::Tar,
                        "gz" => Format::Gzip,
                        "tgz" => Format::Gzip,
                        "gzip" => Format::Gzip,
                        "tgzip" => Format::Gzip,
                        "bz2" => Format::Bzip2,
                        "tbz2" => Format::Bzip2,
                        "bzip2" => Format::Bzip2,
                        "tbzip2" => Format::Bzip2,
                        "xz" => Format::Xz,
                        "txz" => Format::Xz,
                        "lzma" => Format::Xz,
                        "tlzma" => Format::Xz,
                        "lzma2" => Format::Xz,
                        "tlzma2" => Format::Xz,
                        "zst" => Format::Zstd,
                        "tzst" => Format::Zstd,
                        "zstd" => Format::Zstd,
                        "tzstd" => Format::Zstd,
                        _ => Format::Tar
                    }
                }
                else {
                    Format::Tar
                }
            };

            operations::create_archive_progress(writer, paths, content.clone(), fm, *level, !*noprogress)?;
            Ok(ExitCode::SUCCESS)
        }
        _ => Ok(ExitCode::FAILURE)
    }
}
