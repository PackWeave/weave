use anyhow::{Context, Result};

use crate::adapters::{self, ApplyOptions};
use crate::core::config::Config;
use crate::core::install;
use crate::core::lockfile::LockFile;
use crate::core::profile::Profile;
use crate::core::registry::registry_from_config;

/// Install a pack by name (or local path), optionally with a version requirement.
/// When `force` is true, tool-conflict warnings are suppressed.
/// When `project` is true, also installs to `.mcp.json` in the current directory.
/// When `allow_hooks` is true, hooks declared in the pack manifest are applied.
pub fn run(
    pack_name: &str,
    version: Option<&str>,
    force: bool,
    project: bool,
    allow_hooks: bool,
) -> Result<()> {
    // Guard: --project from the home directory would write to ~/.mcp.json, which
    // Claude Code reads globally. Refuse early with a clear error.
    if project {
        let cwd = std::env::current_dir()
            .unwrap_or_else(|_| std::path::PathBuf::from("."))
            .canonicalize()
            .unwrap_or_else(|_| std::env::current_dir().unwrap_or_default());
        if let Some(home) = dirs::home_dir().and_then(|h| h.canonicalize().ok()) {
            anyhow::ensure!(
                cwd != home,
                "cannot install to project scope from the home directory (~)\n\
                 hint: run `weave install` from a project directory, or omit --project \
                 to install to user scope only"
            );
        }
    }

    // Local path install — bypasses the registry entirely.
    if install::is_local_path(pack_name) {
        return run_local(pack_name, force, project, allow_hooks);
    }

    // Normalise name: strip a leading '@' so `weave install @webdev` works like
    // `weave install webdev` (consistent with how packs are validated/stored).
    let pack_name = pack_name.strip_prefix('@').unwrap_or(pack_name);

    let config = Config::load().context("loading weave config")?;
    let registry = registry_from_config(&config);

    let version_req = match version {
        Some(v) => Some(
            semver::VersionReq::parse(v)
                .with_context(|| format!("invalid version requirement '{v}'"))?,
        ),
        None => None,
    };

    let mut profile = Profile::load(&config.active_profile).context("loading active profile")?;
    let mut lockfile = LockFile::load(&config.active_profile).context("loading lock file")?;
    let adapters = adapters::installed_adapters_with_scope(project);
    let apply_options = ApplyOptions { allow_hooks };

    let mut ctx = install::InstallContext {
        config: &config,
        registry: &registry,
        profile: &mut profile,
        lockfile: &mut lockfile,
        adapters: &adapters,
    };

    let result = install::install_from_registry(
        pack_name,
        version_req.as_ref(),
        force,
        &apply_options,
        &mut ctx,
    )?;

    // Format output
    for name in &result.already_satisfied {
        println!("  {name} is already installed and up to date");
    }

    for pack_result in &result.installed {
        println!(
            "  Installing {}@{}...",
            pack_result.name, pack_result.version
        );
        for conflict in &pack_result.tool_conflicts {
            eprintln!("  warning: {conflict}");
        }
        if pack_result.has_hooks && !allow_hooks {
            eprintln!(
                "  note: pack '{}' declares hooks (shell commands that run at lifecycle events)",
                pack_result.name
            );
            eprintln!("  pass --allow-hooks to apply them");
        }
        for adapter in &pack_result.applied_adapters {
            println!("    Applied to {adapter}");
        }
        for err in &pack_result.adapter_errors {
            eprintln!("  warning: {err}");
        }
        for env_var in &pack_result.missing_env_vars {
            eprintln!(
                "  warning: pack '{}' requires {} to be set",
                env_var.pack_name, env_var.key
            );
            if let Some(desc) = &env_var.description {
                eprintln!("  {}: {desc}", env_var.key);
            }
            eprintln!("  set it with: export {}=<value>", env_var.key);
        }
    }

    if !result.installed.is_empty() {
        println!("Done.");
    }

    Ok(())
}

/// Install a pack from a local directory path (bypasses the registry).
fn run_local(raw_path: &str, force: bool, project: bool, allow_hooks: bool) -> Result<()> {
    let path = install::expand_home(raw_path);
    let path = path
        .canonicalize()
        .with_context(|| format!("resolving path '{raw_path}'"))?;

    anyhow::ensure!(
        path.is_dir(),
        "'{raw_path}' is not a directory — local installs require a path to a pack directory containing pack.toml"
    );

    let config = Config::load().context("loading weave config")?;
    let mut profile = Profile::load(&config.active_profile).context("loading active profile")?;
    let mut lockfile = LockFile::load(&config.active_profile).context("loading lock file")?;
    let adapters = adapters::installed_adapters_with_scope(project);

    // No registry needed for local installs, but InstallContext requires one.
    let registry = registry_from_config(&config);
    let apply_options = ApplyOptions { allow_hooks };
    let mut ctx = install::InstallContext {
        config: &config,
        registry: &registry,
        profile: &mut profile,
        lockfile: &mut lockfile,
        adapters: &adapters,
    };

    let result = install::install_local(&path, force, &apply_options, &mut ctx)?;

    println!("  Installing {}@{} (local)...", result.name, result.version);

    // Warn about declared dependencies.
    if !result.unresolved_dependencies.is_empty() {
        eprintln!(
            "  warning: '{}' declares dependencies: {}",
            result.name,
            result.unresolved_dependencies.join(", ")
        );
        eprintln!("  Install them separately: weave install <pack-name>");
    }

    for conflict in &result.tool_conflicts {
        eprintln!("  warning: {conflict}");
    }

    // Warn about hooks that require opt-in.
    if result.has_hooks && !allow_hooks {
        eprintln!(
            "  note: pack '{}' declares hooks (shell commands that run at lifecycle events)",
            result.name
        );
        eprintln!("  pass --allow-hooks to apply them");
    }

    for adapter in &result.applied_adapters {
        println!("    Applied to {adapter}");
    }
    for err in &result.adapter_errors {
        eprintln!("  warning: failed to apply to {err}");
    }

    for env_var in &result.missing_env_vars {
        eprintln!(
            "  warning: pack '{}' requires {} to be set",
            env_var.pack_name, env_var.key
        );
        if let Some(desc) = &env_var.description {
            eprintln!("  {}: {desc}", env_var.key);
        }
        eprintln!("  set it with: export {}=<value>", env_var.key);
    }

    println!("Installed {}@{} (local)", result.name, result.version);
    Ok(())
}
