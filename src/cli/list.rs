use anyhow::{Context, Result};

use crate::adapters::claude_code::ClaudeCodeAdapter;
use crate::cli::style;
use crate::core::config::Config;
use crate::core::pack::PackTargets;
use crate::core::profile::Profile;
use crate::core::store::Store;

/// List all installed packs in the active profile.
pub fn run() -> Result<()> {
    let config = Config::load().context("loading weave config")?;
    let profile = Profile::load(&config.active_profile).context("loading active profile")?;

    if profile.packs.is_empty() {
        println!(
            "{}",
            style::subtext(format!("No packs installed (profile: {}).", profile.name))
        );
        println!();
        println!(
            "{}",
            style::subtext("Run `weave install <pack>` to get started.")
        );
        return Ok(());
    }

    // Load project_dirs from the Claude Code user manifest for scope display.
    // This is best-effort — if it fails we simply omit scope lines.
    let project_dirs = ClaudeCodeAdapter::new().load_project_dirs_public();

    println!(
        "{}",
        style::header(format!("Installed packs (profile: {}):", profile.name))
    );
    println!();

    for installed in &profile.packs {
        // Try to load the full manifest from the store for rich details.
        match Store::load_pack(&installed.name, &installed.version, Some(&installed.source)) {
            Ok(pack) => {
                let hooks_badge = if pack.has_hooks() { " [hooks]" } else { "" };
                println!(
                    "  {} v{}{}",
                    style::pack_name(&installed.name),
                    style::version(installed.version.to_string()),
                    hooks_badge
                );
                println!("    {}", style::subtext(&pack.description));
                println!(
                    "    {}: {}",
                    style::dim("Targets"),
                    style::target(format_targets(&pack.targets))
                );
                if !pack.servers.is_empty() {
                    let names: Vec<&str> = pack.servers.iter().map(|s| s.name.as_str()).collect();
                    println!(
                        "    {}: {}",
                        style::dim("Servers"),
                        style::subtext(names.join(", "))
                    );
                }

                // Show scope only for packs targeting Claude Code.
                if pack.targets.claude_code {
                    if let Some(ref dirs) = project_dirs {
                        if let Some(paths) = dirs.get(&installed.name) {
                            if paths.is_empty() {
                                println!("    {}: user", style::dim("Scope"));
                            } else {
                                for path in paths {
                                    println!(
                                        "    {}: {}",
                                        style::dim("Scope"),
                                        style::subtext(format!("user + project ({path})"))
                                    );
                                }
                            }
                        } else {
                            println!("    {}: user", style::dim("Scope"));
                        }
                    }
                }
            }
            Err(e) => {
                eprintln!(
                    "  warning: could not load manifest for {} v{}: {e}",
                    style::pack_name(&installed.name),
                    style::version(installed.version.to_string())
                );
                println!(
                    "  {} v{}",
                    style::pack_name(&installed.name),
                    style::version(installed.version.to_string())
                );
            }
        }

        println!();
    }

    println!(
        "{} pack(s) installed.",
        style::success(profile.packs.len().to_string())
    );

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
