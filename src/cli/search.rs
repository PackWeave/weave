use anyhow::{Context, Result};

use crate::core::config::Config;
use crate::core::registry::GitHubRegistry;
use crate::core::registry::Registry;

/// Search for packs in the registry.
pub fn run(query: &str) -> Result<()> {
    let config = Config::load().context("loading weave config")?;
    let registry = GitHubRegistry::new(&config.registry_url);

    let results = registry.search(query).context("searching registry")?;

    if results.is_empty() {
        println!("No packs found matching '{query}'.");
        println!();
        println!("Browse all packs at https://github.com/PackWeave/registry");
        return Ok(());
    }

    println!("Found {} pack(s) matching '{query}':", results.len());
    println!();

    for pack in &results {
        println!("  {} @ {}", pack.name, pack.latest_version);
        if !pack.description.is_empty() {
            println!("    {}", pack.description);
        }
    }

    Ok(())
}
