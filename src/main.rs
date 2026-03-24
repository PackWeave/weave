#[allow(dead_code)]
mod adapters;
mod cli;
#[allow(dead_code)]
mod core;
mod error;
#[allow(dead_code)]
mod util;

use clap::{ColorChoice, CommandFactory, FromArgMatches, Parser, Subcommand, builder::styling};

/// weave — a pack manager for AI CLI tools.
///
/// Weave together MCP servers, prompts, commands, and settings
/// into shareable, versioned packs across Claude Code, Gemini CLI, and more.
#[derive(Parser)]
#[command(
    name = "weave",
    version,
    about,
    long_about = None,
    styles = styling::Styles::styled()
        .header(styling::AnsiColor::Magenta.on_default().bold())
        .usage(styling::AnsiColor::Magenta.on_default().bold())
        .literal(styling::AnsiColor::Cyan.on_default().bold())
        .placeholder(styling::AnsiColor::Green.on_default())
)]
struct Cli {
    /// Control color output (auto, always, never)
    #[arg(long, global = true, default_value = "auto")]
    color: cli::style::ColorMode,

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

        /// Also install to .mcp.json in the current directory (project scope)
        #[arg(long)]
        project: bool,

        /// Apply hooks declared by the pack (shell commands that run at lifecycle events)
        #[arg(long)]
        allow_hooks: bool,

        /// Preview changes without writing
        #[arg(long)]
        dry_run: bool,
    },

    /// List installed packs
    List,

    /// Remove an installed pack
    Remove {
        /// Pack name to remove
        name: String,

        /// Preview changes without writing
        #[arg(long)]
        dry_run: bool,
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

    /// Reapply the active profile's lock file to all adapters
    Sync {
        /// Apply hooks declared by packs (shell commands that run at lifecycle events)
        #[arg(long)]
        allow_hooks: bool,

        /// Preview changes without writing
        #[arg(long)]
        dry_run: bool,
    },

    /// Check for config drift and project-scope staleness across all adapters
    Diagnose {
        /// Output results as JSON
        #[arg(long)]
        json: bool,
    },

    /// Manage named profiles
    Profile {
        #[command(subcommand)]
        action: ProfileAction,
    },

    /// Manage community taps (third-party pack registries)
    Tap {
        #[command(subcommand)]
        action: TapAction,
    },

    /// Publish a pack to the registry (creates a PR on the registry repo)
    Publish {
        /// Path to the pack directory (defaults to current directory)
        path: Option<String>,
    },

    /// Manage registry authentication (for pack publishing and rate limits)
    Auth {
        #[command(subcommand)]
        action: AuthAction,
    },

    /// Switch to a named profile, or print the active profile (no args)
    Use {
        /// Profile name to switch to. Omit to print the current profile.
        profile: Option<String>,

        /// Apply hooks declared by packs (shell commands that run at lifecycle events)
        #[arg(long)]
        allow_hooks: bool,

        /// Preview changes without writing
        #[arg(long)]
        dry_run: bool,
    },
}

#[derive(Subcommand)]
enum AuthAction {
    /// Authenticate with the registry using a GitHub personal access token.
    ///
    /// Required for `weave publish`. Also raises the GitHub API rate limit
    /// from 60 to 5,000 requests/hour for install, search, and update.
    ///
    /// Create a token at https://github.com/settings/tokens — no special
    /// scopes are needed for read-only operations (install, search, update).
    ///
    /// For CI/automation, set the WEAVE_TOKEN environment variable instead
    /// of running `weave auth login`.
    ///
    /// Security: prefer `weave auth login` (stdin prompt) or WEAVE_TOKEN
    /// over --token, which is visible in process listings (`ps aux`).
    Login {
        /// GitHub personal access token. Visible in process listings —
        /// prefer omitting this flag (stdin prompt) or WEAVE_TOKEN env var.
        #[arg(long)]
        token: Option<String>,
    },

    /// Remove stored credentials from ~/.packweave/credentials
    Logout,

    /// Show current authentication state (token source and masked value)
    Status,
}

#[derive(Subcommand)]
enum TapAction {
    /// Add a community tap (e.g. `weave tap add user/repo`)
    Add {
        /// Tap name in user/repo format
        name: String,
    },

    /// List registered community taps
    List,

    /// Remove a community tap
    Remove {
        /// Tap name in user/repo format
        name: String,
    },
}

#[derive(Subcommand)]
enum ProfileAction {
    /// Create a new empty profile
    Create {
        /// Profile name
        name: String,
    },

    /// Delete a profile (cannot delete the active or default profile)
    Delete {
        /// Profile name
        name: String,
    },

    /// List all profiles
    List,

    /// Add a pack reference to a profile
    Add {
        /// Pack name (e.g., "webdev")
        pack: String,

        /// Target profile name
        #[arg(short, long)]
        profile: String,
    },
}

/// Pre-parse `--color` from raw args so we can set the color mode before clap
/// renders help text or parse errors (which happen during `Cli::parse()`).
fn pre_parse_color_mode() -> cli::style::ColorMode {
    let args: Vec<String> = std::env::args().collect();
    for (i, arg) in args.iter().enumerate() {
        // --color=value
        if let Some(val) = arg.strip_prefix("--color=")
            && let Ok(mode) = val.parse()
        {
            return mode;
        }
        // --color value
        if arg == "--color"
            && let Some(val) = args.get(i + 1)
            && let Ok(mode) = val.parse()
        {
            return mode;
        }
    }
    cli::style::ColorMode::Auto
}

/// Helper: acquire the advisory file lock, converting the error to `anyhow::Error`.
fn lock() -> anyhow::Result<core::lock::WeaveFileLock> {
    Ok(core::lock::acquire()?)
}

fn main() {
    env_logger::init();

    // Set color mode before parse() so clap's help/error output respects it.
    let color_mode = pre_parse_color_mode();
    cli::style::set_color_mode(color_mode);

    let clap_color = match color_mode {
        cli::style::ColorMode::Auto => ColorChoice::Auto,
        cli::style::ColorMode::Always => ColorChoice::Always,
        cli::style::ColorMode::Never => ColorChoice::Never,
    };

    let cli = Cli::command().color(clap_color).get_matches();
    let cli = Cli::from_arg_matches(&cli).expect("clap arg mismatch");

    let result = match cli.command {
        // Read-only commands — no lock needed.
        Commands::Init { name } => cli::init::run(name.as_deref()),
        Commands::List => cli::list::run(),
        Commands::Search { query, target, mcp } => cli::search::run(&query, target.as_deref(), mcp),
        Commands::Diagnose { json } => cli::diagnose::run(json),

        // Mutating commands — acquire advisory lock.
        Commands::Install {
            name,
            version,
            force,
            project,
            allow_hooks,
            dry_run,
        } => (|| {
            let _lock = lock()?;
            cli::install::run(
                &name,
                version.as_deref(),
                force,
                project,
                allow_hooks,
                dry_run,
            )
        })(),
        Commands::Remove { name, dry_run } => (|| {
            let _lock = lock()?;
            cli::remove::run(&name, dry_run)
        })(),
        Commands::Update { name } => (|| {
            let _lock = lock()?;
            cli::update::run(name.as_deref())
        })(),
        Commands::Sync {
            allow_hooks,
            dry_run,
        } => (|| {
            let _lock = lock()?;
            cli::sync::run(allow_hooks, dry_run)
        })(),
        Commands::Publish { path } => (|| {
            let _lock = lock()?;
            cli::publish::run(path.as_deref())
        })(),
        Commands::Use {
            profile,
            allow_hooks,
            dry_run,
        } => (|| {
            let _lock = lock()?;
            cli::use_profile::run(profile.as_deref(), allow_hooks, dry_run)
        })(),
        Commands::Auth { action } => match action {
            AuthAction::Login { token } => (|| {
                let _lock = lock()?;
                cli::auth::login(token.as_deref())
            })(),
            AuthAction::Logout => (|| {
                let _lock = lock()?;
                cli::auth::logout()
            })(),
            AuthAction::Status => cli::auth::status(),
        },
        Commands::Tap { action } => match action {
            TapAction::Add { name } => (|| {
                let _lock = lock()?;
                cli::tap::add(&name)
            })(),
            TapAction::List => cli::tap::list(),
            TapAction::Remove { name } => (|| {
                let _lock = lock()?;
                cli::tap::remove(&name)
            })(),
        },
        Commands::Profile { action } => match action {
            ProfileAction::Create { name } => (|| {
                let _lock = lock()?;
                cli::profile::create(&name)
            })(),
            ProfileAction::Delete { name } => (|| {
                let _lock = lock()?;
                cli::profile::delete(&name)
            })(),
            ProfileAction::List => cli::profile::list(),
            ProfileAction::Add { pack, profile } => (|| {
                let _lock = lock()?;
                cli::profile::add_pack(&pack, &profile)
            })(),
        },
    };

    if let Err(err) = result {
        eprintln!("error: {err:#}");
        std::process::exit(1);
    }
}
