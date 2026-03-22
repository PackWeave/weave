use anyhow::{Context, Result};

use crate::adapters::claude_code::ClaudeCodeAdapter;
use crate::core::config::Config;
use crate::core::pack::PackTargets;
use crate::core::profile::Profile;
use crate::core::store::Store;

/// List all installed packs in the active profile.
pub fn run() -> Result<()> {
    let config = Config::load().context("loading weave config")?;
    let profile = Profile::load(&config.active_profile).context("loading active profile")?;

    if profile.packs.is_empty() {
        println!("No packs installed (profile: {}).", profile.name);
        println!();
        println!("Run `weave install <pack>` to get started.");
        return Ok(());
    }

    // Load project_dirs from the Claude Code user manifest for scope display.
    // This is best-effort — if it fails we simply omit scope lines.
    let project_dirs = ClaudeCodeAdapter::new().load_project_dirs_public();

    println!("Installed packs (profile: {}):", profile.name);
    println!();

    for installed in &profile.packs {
        // Try to load the full manifest from the store for rich details.
        match Store::load_pack(&installed.name, &installed.version) {
            Ok(pack) => {
                println!("  {} v{}", installed.name, installed.version);
                println!("    {}", pack.description);
                println!("    Targets: {}", format_targets(&pack.targets));
                if !pack.servers.is_empty() {
                    let names: Vec<&str> = pack.servers.iter().map(|s| s.name.as_str()).collect();
                    println!("    Servers: {}", names.join(", "));
                }
            }
            Err(e) => {
                eprintln!(
                    "  warning: could not load manifest for {} v{}: {e}",
                    installed.name, installed.version
                );
                println!("  {} v{}", installed.name, installed.version);
            }
        }

        // Show scope if Claude Code adapter is available and has tracking data.
        if let Some(ref dirs) = project_dirs {
            if let Some(paths) = dirs.get(&installed.name) {
                if paths.is_empty() {
                    println!("    Scope: user");
                } else {
                    for path in paths {
                        println!("    Scope: user + project ({path})");
                    }
                }
            } else {
                println!("    Scope: user");
            }
        }

        println!();
    }

    println!("{} pack(s) installed.", profile.packs.len());

    Ok(())
}

/// Format target CLIs as a comma-separated string.
fn format_targets(targets: &PackTargets) -> String {
    let mut names = Vec::new();
    if targets.claude_code {
        names.push("Claude Code");
    }
    if targets.gemini_cli {
        names.push("Gemini CLI");
    }
    if targets.codex_cli {
        names.push("Codex CLI");
    }
    if names.is_empty() {
        return "none".to_string();
    }
    names.join(", ")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn format_targets_all() {
        let targets = PackTargets {
            claude_code: true,
            gemini_cli: true,
            codex_cli: true,
        };
        assert_eq!(
            format_targets(&targets),
            "Claude Code, Gemini CLI, Codex CLI"
        );
    }

    #[test]
    fn format_targets_subset() {
        let targets = PackTargets {
            claude_code: true,
            gemini_cli: false,
            codex_cli: true,
        };
        assert_eq!(format_targets(&targets), "Claude Code, Codex CLI");
    }

    #[test]
    fn format_targets_none() {
        let targets = PackTargets {
            claude_code: false,
            gemini_cli: false,
            codex_cli: false,
        };
        assert_eq!(format_targets(&targets), "none");
    }
}
