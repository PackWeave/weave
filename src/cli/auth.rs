use anyhow::{Context, Result};

use crate::cli::style;
use crate::core::config::Config;
use crate::core::credentials;

/// Authenticate with the registry by storing a token.
///
/// If `token` is `None`, reads from stdin (interactive prompt).
pub fn login(token: Option<&str>) -> Result<()> {
    let config = Config::load().context("loading weave config")?;

    let token = match token {
        Some(t) => t.to_string(),
        None => {
            eprintln!("Paste your GitHub personal access token:");
            let mut buf = String::new();
            std::io::stdin()
                .read_line(&mut buf)
                .context("reading token from stdin")?;
            buf.trim().to_string()
        }
    };

    if token.is_empty() {
        anyhow::bail!("token cannot be empty");
    }

    // Best-effort validation against GitHub API.
    match credentials::validate_github_token(&token) {
        Some(username) => {
            println!(
                "{} Authenticated as {}",
                style::success("✓"),
                style::emphasis(&username)
            );
        }
        None => {
            println!(
                "{} Token could not be verified with GitHub (may still be valid for other registries)",
                style::dim("⚠")
            );
        }
    }

    credentials::store_token(&config, &token)?;

    let path = credentials::credentials_path(&config)?;
    println!(
        "Token stored at {}",
        style::subtext(path.display().to_string())
    );

    Ok(())
}

/// Remove stored credentials.
pub fn logout() -> Result<()> {
    let config = Config::load().context("loading weave config")?;
    credentials::remove_token(&config)?;
    println!("Logged out. Credentials removed.");
    Ok(())
}

/// Show current authentication state.
pub fn status() -> Result<()> {
    let config = Config::load().context("loading weave config")?;
    let token = credentials::resolve_token(&config)?;

    match token {
        Some(ref t) => {
            let source = if std::env::var("WEAVE_TOKEN").is_ok() {
                "environment variable WEAVE_TOKEN".to_string()
            } else {
                let path = credentials::credentials_path(&config)?;
                path.display().to_string()
            };

            let masked = if t.len() > 4 {
                format!("{}****", &t[..4])
            } else {
                "****".to_string()
            };

            println!("{} Authenticated", style::success("✓"));
            println!("  Source: {}", style::subtext(&source));
            println!("  Token:  {}", style::subtext(&masked));
        }
        None => {
            println!("Not authenticated.");
            println!(
                "  Run '{}' to authenticate.",
                style::emphasis("weave auth login")
            );
        }
    }

    Ok(())
}
