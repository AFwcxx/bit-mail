use std::{error::Error, process::ExitCode};

use bit_mail::cli::Cli;
use clap::Parser;

type Result<T> = std::result::Result<T, Box<dyn Error + Send + Sync>>;

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("error: {error}");
            ExitCode::FAILURE
        }
    }
}

fn run() -> Result<()> {
    let _cli = Cli::parse();

    tracing_subscriber::fmt().with_target(false).try_init()?;
    tracing::info!("bit-mail implementation has not started yet; see docs/milestones/");

    Ok(())
}
