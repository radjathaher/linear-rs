use std::process::ExitCode;

use anyhow::Result;
use clap::{Parser, Subcommand};

/// Developer helper tasks for the linear-rs workspace.
#[derive(Parser)]
#[command(author, version, about)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Regenerate GraphQL schema artifacts.
    Codegen,
}

fn main() -> ExitCode {
    if let Err(err) = run() {
        eprintln!("{err:?}");
        ExitCode::FAILURE
    } else {
        ExitCode::SUCCESS
    }
}

fn run() -> Result<()> {
    let cli = Cli::parse();
    match cli.command {
        Commands::Codegen => linear_codegen::run()?,
    }
    Ok(())
}
