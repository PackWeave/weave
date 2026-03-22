use anyhow::{bail, Context, Result};

use crate::core::config::Config;
use crate::core::mcp_registry::McpRegistryClient;
use crate::core::registry::{registry_from_config, Registry};

/// Valid target CLI names for the `--target` filter.
const VALID_TARGETS: &[&str] = &["claude_code", "gemini_cli", "codex_cli"];

/// Search for packs in the registry, or MCP servers via `--mcp`.
///
/// The optional `target` filter validates early but currently includes all results
/// because the registry index does not yet expose per-pack target information.
/// Once the registry adds target fields to `PackSummary`, the filter will narrow
/// results to packs that support the requested CLI.
pub fn run(query: &str, target: Option<&str>, mcp: bool) -> Result<()> {
    if mcp {
        if target.is_some() {
            bail!("--mcp and --target cannot be used together");
        }
        return run_mcp_search(query);
    }

    // Validate --target value early so the user gets immediate feedback.
    if let Some(t) = target {
        if !VALID_TARGETS.contains(&t) {
            bail!(
                "unknown target '{t}'. Valid targets: {}",
                VALID_TARGETS.join(", ")
            );
        }
        eprintln!("note: --target filtering is not yet implemented; showing all results");
    }

    let config = Config::load().context("loading weave config")?;
    let registry = registry_from_config(&config);

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

/// Search the official MCP Registry for servers.
fn run_mcp_search(query: &str) -> Result<()> {
    let client = McpRegistryClient::new();
    let results = client.search(query).context("searching MCP Registry")?;

    if results.is_empty() {
        println!("No MCP servers found matching '{query}'.");
        return Ok(());
    }

    println!("MCP Registry results for '{query}':");
    println!();

    for server in &results {
        // Prefer title; fall back to the short name (after last '/').
        let display_name = server
            .title
            .as_deref()
            .unwrap_or_else(|| server.name.rsplit('/').next().unwrap_or(&server.name));

        println!("  {display_name}");

        if !server.description.is_empty() {
            println!("    {}", server.description);
        }

        for pkg in &server.packages {
            if let Some(ver) = &pkg.version {
                println!(
                    "    Package: {} ({}) v{}",
                    pkg.identifier, pkg.registry_type, ver
                );
            } else {
                println!("    Package: {} ({})", pkg.identifier, pkg.registry_type);
            }
        }

        if let Some(repo) = &server.repository {
            if let Some(url) = &repo.url {
                println!("    Repository: {url}");
            }
        }

        println!();
    }

    println!("{} server(s) found.", results.len());
    println!();
    println!("Note: these are MCP servers, not weave packs.");

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

    #[test]
    fn mcp_and_target_are_mutually_exclusive() {
        let result = run("query", Some("claude_code"), true);
        let err = result.unwrap_err();
        assert!(
            err.to_string()
                .contains("--mcp and --target cannot be used together"),
            "expected mutual exclusion error, got: {err}"
        );
    }
}
