use std::path::PathBuf;
use std::process::ExitCode;

use clap::{Parser, Subcommand};
use openblrq_manifest::Manifest;

#[derive(Parser)]
#[command(
    name = "obq",
    version,
    about = "Build and publish OpenBLRQ experiences"
)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Check openblrq.toml in the given directory.
    Check {
        #[arg(default_value = ".")]
        dir: PathBuf,
    },
    /// Print the BLAKE3 hash of a file, in the form the manifest uses.
    Hash { file: PathBuf },
    /// Upload a bundle for review. Not available until the platform opens to creators.
    Publish {
        #[arg(default_value = ".")]
        dir: PathBuf,
    },
}

fn main() -> ExitCode {
    match Cli::parse().command {
        Command::Check { dir } => {
            let path = dir.join("openblrq.toml");
            let text = match std::fs::read_to_string(&path) {
                Ok(text) => text,
                Err(err) => {
                    eprintln!("can't read {}: {err}", path.display());
                    return ExitCode::FAILURE;
                }
            };
            match Manifest::parse(&text) {
                Ok(m) => {
                    println!("{} {} looks fine", m.experience.id, m.experience.version);
                    ExitCode::SUCCESS
                }
                Err(err) => {
                    eprintln!("{}: {err}", path.display());
                    ExitCode::FAILURE
                }
            }
        }
        Command::Hash { file } => match std::fs::read(&file) {
            Ok(bytes) => {
                println!("blake3:{}", blake3::hash(&bytes).to_hex());
                ExitCode::SUCCESS
            }
            Err(err) => {
                eprintln!("can't read {}: {err}", file.display());
                ExitCode::FAILURE
            }
        },
        Command::Publish { .. } => {
            eprintln!("Publishing isn't open yet. Follow the creator docs for the announcement.");
            ExitCode::FAILURE
        }
    }
}
