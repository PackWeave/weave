use anyhow::{bail, Context, Result};

use crate::core::config::Config;
use crate::core::registry::GitHubRegistry;
use crate::core::registry::Registry;

/// Valid target CLI names for the `--target` filter.
const VALID_TARGETS: &[&str] = &["claude_code", "gemini_cli", "codex_cli"];

/// Search for packs in the registry.
///
/// The optional `target` filter validates early but currently includes all results
/// because the registry index does not yet expose per-pack target information.
/// Once the registry adds target fields to `PackSummary`, the filter will narrow
/// results to packs that support the requested CLI.
pub fn run(query: &str, target: Option<&str>) -> Result<()> {
    // Validate --target value early so the user gets immediate feedback.
    if let Some(t) = target {
        if !VALID_TARGETS.contains(&t) {
            bail!(
                "unknown target '{t}'. Valid targets: {}",
                VALID_TARGETS.join(", ")
            );
        }
    }

    let config = Config::load().context("loading weave config")?;
    let registry = GitHubRegistry::new(&config.registry_url);

    let results = registry.search(query).context("searching registry")?;

    if results.is_empty() {
        println!("No packs found matching '{query}'.");
        println!();
        println!("Browse available packs at https://github.com/PackWeave/registry");
        return Ok(());
    }

    // TODO: filter by `target` once the registry index includes per-pack target data.

    println!("Search results for '{query}':");
    println!();

    for pack in &results {
        println!("  {} v{}", pack.name, pack.latest_version);
        if !pack.description.is_empty() {
            println!("    {}", pack.description);
        }
        if !pack.keywords.is_empty() {
            println!("    Keywords: {}", pack.keywords.join(", "));
        }
        println!();
    }

    println!("{} pack(s) found.", results.len());

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn valid_targets_are_accepted() {
        for t in VALID_TARGETS {
            assert!(VALID_TARGETS.contains(t));
        }
    }

    #[test]
    fn invalid_target_is_rejected() {
        let target = "invalid_cli";
        assert!(!VALID_TARGETS.contains(&target));
    }
}
