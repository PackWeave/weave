#[allow(dead_code)]
mod adapters;
mod cli;
#[allow(dead_code)]
mod core;
mod error;
#[allow(dead_code)]
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

        /// Suppress tool-conflict warnings
        #[arg(long)]
        force: bool,
    },

    /// List installed packs
    List,

    /// Remove an installed pack
    Remove {
        /// Pack name to remove
        name: String,
    },

    /// Search for packs in the registry
    Search {
        /// Search query
        query: String,

        /// Filter results by target CLI (reserved, not yet active; e.g., "claude_code", "gemini_cli", "codex_cli")
        #[arg(short, long)]
        target: Option<String>,

        /// Search the official MCP Registry for servers instead of weave packs
        #[arg(long)]
        mcp: bool,
    },

    /// Initialize a new pack directory
    Init {
        /// Pack name (creates a subdirectory). Omit to initialize the current directory.
        name: Option<String>,
    },

    /// Update one or all installed packs to the latest compatible version
    Update {
        /// Pack name to update (e.g., "webdev", "webdev@latest"). Omit to update all.
        name: Option<String>,
    },

    /// Check for config drift and project-scope staleness across all adapters
    Diagnose,
}

fn main() {
    env_logger::init();

    let cli = Cli::parse();

    let result = match cli.command {
        Commands::Init { name } => cli::init::run(name.as_deref()),
        Commands::Install {
            name,
            version,
            force,
        } => cli::install::run(&name, version.as_deref(), force),
        Commands::List => cli::list::run(),
        Commands::Remove { name } => cli::remove::run(&name),
        Commands::Search { query, target, mcp } => cli::search::run(&query, target.as_deref(), mcp),
        Commands::Update { name } => cli::update::run(name.as_deref()),
        Commands::Diagnose => cli::diagnose::run(),
    };

    if let Err(err) = result {
        eprintln!("error: {err:#}");
        std::process::exit(1);
    }
}
