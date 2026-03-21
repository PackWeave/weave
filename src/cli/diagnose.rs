use anyhow::{Context, Result};

use crate::adapters;
use crate::adapters::Severity;
use crate::core::config::Config;

/// Check for project-scope config staleness and other adapter issues.
///
/// Detects cases where a project-scope config directory (`.claude/` or `.gemini/`)
/// was created after a pack was installed, meaning the pack's project-scope config
/// was never applied. Re-run `weave install <pack>` to fix.
pub fn run() -> Result<()> {
    let config = Config::load().context("loading weave config")?;

    println!(
        "Running diagnostics (profile '{}')...",
        config.active_profile
    );
    println!();

    let adapters = adapters::all_adapters();
    let mut total_issues = 0;

    for adapter in &adapters {
        let issues = adapter
            .diagnose()
            .with_context(|| format!("diagnosing {}", adapter.name()))?;

        if issues.is_empty() {
            println!("  {} — OK", adapter.name());
        } else {
            println!("  {} — {} issue(s) found:", adapter.name(), issues.len());
            for issue in &issues {
                let label = match issue.severity {
                    Severity::Warning => "warning",
                    Severity::Error => "error",
                };
                println!("    [{label}] {}", issue.message);
                if let Some(suggestion) = &issue.suggestion {
                    println!("             {suggestion}");
                }
            }
            total_issues += issues.len();
        }
        println!();
    }

    if total_issues == 0 {
        println!("No issues found.");
    } else {
        println!("{total_issues} issue(s) found. See suggestions above to fix them.");
    }

    Ok(())
}
