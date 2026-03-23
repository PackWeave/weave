use std::path::PathBuf;

use anyhow::{Context, Result, bail};

use crate::cli::style;
use crate::core::config::Config;
use crate::core::credentials;
use crate::core::pack::Pack;
use crate::core::publish;
use crate::core::registry::{Registry, registry_from_config};

/// Publish a pack to the registry by creating a PR.
pub fn run(path: Option<&str>) -> Result<()> {
    // 1. Determine pack directory.
    let pack_dir = match path {
        Some(p) => {
            let pb = PathBuf::from(p);
            pb.canonicalize()
                .with_context(|| format!("resolving path '{p}'"))?
        }
        None => std::env::current_dir().context("determining current directory")?,
    };

    // 2. Load and validate the pack.
    println!(
        "  Loading pack from {}...",
        style::subtext(pack_dir.display().to_string())
    );
    let pack = Pack::load(&pack_dir).context("loading pack")?;
    println!(
        "  Found {}@{}",
        style::pack_name(&pack.name),
        style::version(pack.version.to_string())
    );

    // 3. Require authentication.
    let config = Config::load().context("loading weave config")?;
    let token = credentials::resolve_token(&config)?.ok_or_else(|| {
        anyhow::anyhow!("not authenticated — run `weave auth login` before publishing")
    })?;

    // 4. Check for duplicate version (works against any registry, including mocks).
    let registry = registry_from_config(&config);
    println!(
        "  Checking registry for existing {}@{}...",
        style::pack_name(&pack.name),
        style::version(pack.version.to_string())
    );
    publish::check_version_not_published(&registry, &pack.name, &pack.version)?;

    // 5. Verify this is a GitHub-backed registry (required for PR creation).
    if !credentials::is_github_registry(&config.registry_url) {
        bail!("publish is only supported for GitHub-backed registries");
    }

    // 6. Collect files.
    let files = publish::collect_pack_files(&pack_dir).context("collecting pack files")?;
    println!("  Collected {} file(s)", files.len());

    // 7. Publish via the Registry trait (routes through CompositeRegistry → GitHubRegistry).
    println!(
        "  Publishing {}@{}...",
        style::pack_name(&pack.name),
        style::version(pack.version.to_string())
    );
    let result = registry.publish(&pack, &files, &token.token)?;

    // 8. Success output.
    println!();
    println!(
        "{} {}@{} published",
        style::success("✓"),
        style::pack_name(&result.name),
        style::version(&result.version)
    );
    println!("  PR: {}", style::subtext(&result.pr_url));
    println!("  The pack will be available after the PR is reviewed and merged.");

    Ok(())
}
