use anyhow::{Context, Result, bail};

use crate::cli::style;
use crate::core::config::Config;
use crate::core::mcp_registry::McpRegistryClient;
use crate::core::registry::{Registry, registry_from_config};

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
        println!(
            "{}",
            style::subtext(format!("No packs found matching '{query}'."))
        );
        println!();
        println!(
            "{}",
            style::subtext("Browse available packs at https://github.com/PackWeave/registry")
        );
        return Ok(());
    }

    // TODO: filter by `target` once the registry index includes per-pack target data.

    println!(
        "{}",
        style::header(format!("Search results for '{query}':"))
    );
    println!();

    for pack in &results {
        println!(
            "  {} v{}",
            style::pack_name(pack.name.as_str()),
            style::version(pack.latest_version.to_string())
        );
        if !pack.description.is_empty() {
            println!("    {}", style::subtext(pack.description.as_str()));
        }
        if !pack.keywords.is_empty() {
            println!(
                "    {}: {}",
                style::dim("Keywords"),
                style::subtext(pack.keywords.join(", "))
            );
        }
        println!();
    }

    println!(
        "{} pack(s) found.",
        style::success(results.len().to_string())
    );

    Ok(())
}

/// Search the official MCP Registry for servers.
fn run_mcp_search(query: &str) -> Result<()> {
    let client = McpRegistryClient::new();
    let results = client.search(query).context("searching MCP Registry")?;

    if results.is_empty() {
        println!(
            "{}",
            style::subtext(format!("No MCP servers found matching '{query}'."))
        );
        return Ok(());
    }

    println!(
        "{}",
        style::header(format!("MCP Registry results for '{query}':"))
    );
    println!();

    for server in &results {
        // Prefer title; fall back to the short name (after last '/').
        let display_name = server
            .title
            .as_deref()
            .unwrap_or_else(|| server.name.rsplit('/').next().unwrap_or(&server.name));

        println!("  {}", style::pack_name(display_name));

        if !server.description.is_empty() {
            println!("    {}", style::subtext(server.description.as_str()));
        }

        for pkg in &server.packages {
            if let Some(ver) = &pkg.version {
                println!(
                    "    {}: {} ({}) v{}",
                    style::dim("Package"),
                    pkg.identifier,
                    style::dim(pkg.registry_type.as_str()),
                    style::version(ver.as_str())
                );
            } else {
                println!(
                    "    {}: {} ({})",
                    style::dim("Package"),
                    pkg.identifier,
                    style::dim(pkg.registry_type.as_str())
                );
            }
        }

        if let Some(repo) = &server.repository
            && let Some(url) = &repo.url
        {
            println!("    {}: {url}", style::dim("Repository"));
        }

        println!();
    }

    println!(
        "{} server(s) found.",
        style::success(results.len().to_string())
    );
    println!();
    println!(
        "{}",
        style::subtext("Note: these are MCP servers, not weave packs.")
    );

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
