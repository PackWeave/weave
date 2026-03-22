use anyhow::{Context, Result};

use crate::cli::style;
use crate::core::config::Config;

/// Register a community tap by `user/repo` name.
pub fn add(name: &str) -> Result<()> {
    let mut config = Config::load().context("loading weave config")?;
    config.add_tap(name)?;
    config.save().context("saving weave config")?;
    println!(
        "{} '{}' added (https://raw.githubusercontent.com/{name}/main)",
        style::success("Tap"),
        style::emphasis(name)
    );
    Ok(())
}

/// List all registered taps.
pub fn list() -> Result<()> {
    let config = Config::load().context("loading weave config")?;
    let taps = config.list_taps();

    if taps.is_empty() {
        println!("{}", style::subtext("No community taps registered."));
        println!();
        println!(
            "{}",
            style::subtext("Add one with: weave tap add <user/repo>")
        );
        return Ok(());
    }

    println!("{}", style::header("Registered taps:"));
    println!();
    for tap in taps {
        println!(
            "  {} ({})",
            style::emphasis(&tap.name),
            style::subtext(&tap.url)
        );
    }
    println!();
    println!(
        "{} tap(s) registered.",
        style::success(taps.len().to_string())
    );

    Ok(())
}

/// Deregister a community tap by `user/repo` name.
pub fn remove(name: &str) -> Result<()> {
    let mut config = Config::load().context("loading weave config")?;
    config.remove_tap(name)?;
    config.save().context("saving weave config")?;
    println!("Tap '{}' removed.", style::emphasis(name));
    Ok(())
}
