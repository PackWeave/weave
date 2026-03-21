use anyhow::{Context, Result};

use crate::core::config::Config;
use crate::core::profile::Profile;

/// List all installed packs in the active profile.
pub fn run() -> Result<()> {
    let config = Config::load().context("loading weave config")?;
    let profile = Profile::load(&config.active_profile).context("loading active profile")?;

    if profile.packs.is_empty() {
        println!("No packs installed in profile '{}'.", profile.name);
        println!();
        println!("Install one with: weave install <pack-name>");
        return Ok(());
    }

    println!("Installed packs (profile '{}'):", profile.name);
    println!();

    for pack in &profile.packs {
        println!("  {} @ {}", pack.name, pack.version);
    }

    println!();
    println!("{} pack(s) installed.", profile.packs.len());

    Ok(())
}
