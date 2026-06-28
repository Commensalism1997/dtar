use std::{env::current_dir, ffi::OsString, fmt::Display, fs, path::PathBuf, process::ExitCode, time::Duration};
use colored::Colorize;
use clap::{Parser, Subcommand};
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
                            println!("{} {}", "├─".white(), p);
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
                let pb = mpb.add(style::themed_progressbar_bytes(f.metadata()?.len()));
                let wr = pb.wrap_read(f);
                let arq = operations::prepare_archive_from_read(wr, fm)?;
                let fname = PathBuf::from(file).file_name().map(|f| f.to_owned());
                if let Some(name) = fname {
                    mainpb.set_message(format!("Extracting {}...", name.to_string_lossy().yellow().bold()));
                }
                else {
                    mainpb.set_message("Extracting...");
                }
                operations::extract_archive_with_pb(arq, dst, &mpb, name, &mainpb, &pb, verbose, filters);
            }
            else {
                let arq = operations::prepare_archive(file, fm)?;
                mainpb.set_message("Extracting...");
                operations::extract_archive_no_pb(arq, dst, &mpb, &mainpb, verbose, filters);
            }
            Ok(ExitCode::SUCCESS)
        }
        Some(Subcommands::Create {  }) => todo!(),
        _ => Ok(ExitCode::FAILURE)
    }
}