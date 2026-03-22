use anyhow::{Context, Result};

use crate::core::config::Config;

/// Register a community tap by `user/repo` name.
pub fn add(name: &str) -> Result<()> {
    let mut config = Config::load().context("loading weave config")?;
    config.add_tap(name)?;
    config.save().context("saving weave config")?;
    println!("Tap '{name}' added (https://raw.githubusercontent.com/{name}/main)");
    Ok(())
}

/// List all registered taps.
pub fn list() -> Result<()> {
    let config = Config::load().context("loading weave config")?;
    let taps = config.list_taps();

    if taps.is_empty() {
        println!("No community taps registered.");
        println!();
        println!("Add one with: weave tap add <user/repo>");
        return Ok(());
    }

    println!("Registered taps:");
    println!();
    for tap in taps {
        println!("  {} ({})", tap.name, tap.url);
    }
    println!();
    println!("{} tap(s) registered.", taps.len());

    Ok(())
}

/// Deregister a community tap by `user/repo` name.
pub fn remove(name: &str) -> Result<()> {
    let mut config = Config::load().context("loading weave config")?;
    config.remove_tap(name)?;
    config.save().context("saving weave config")?;
    println!("Tap '{name}' removed.");
    Ok(())
}
