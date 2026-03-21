// Public API surface is larger than what main.rs uses directly.
// These items are part of the library's intended interface.
#![allow(dead_code)]

mod adapters;
mod cli;
mod core;
mod error;
mod util;

use clap::{Parser, Subcommand};

/// weave — a pack manager for AI CLI tools.
///
/// Weave together MCP servers, prompts, commands, and settings
/// into shareable, versioned packs across Claude Code, Gemini CLI, and more.
#[derive(Parser)]
#[command(name = "weave", version, about, long_about = None)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Install a pack from the registry
    Install {
        /// Pack name (e.g., "webdev", "databases")
        name: String,

        /// Version requirement (e.g., "^1.0", "=2.3.1"). Defaults to latest.
        #[arg(short, long)]
        version: Option<String>,
    },

    /// List installed packs
    List,

    /// Remove an installed pack
    Remove {
        /// Pack name to remove
        name: String,
    },
}

fn main() {
    env_logger::init();

    let cli = Cli::parse();

    let result = match cli.command {
        Commands::Install { name, version } => cli::install::run(&name, version.as_deref()),
        Commands::List => cli::list::run(),
        Commands::Remove { name } => cli::remove::run(&name),
    };

    if let Err(err) = result {
        eprintln!("error: {err:#}");
        std::process::exit(1);
    }
}
