use anyhow::{Context, Result};
use log::warn;
use serde::Serialize;

use crate::adapters;
use crate::adapters::{AdapterId, CliAdapter, DiagnosticIssue};
use crate::cli::style;
use crate::core::config::Config;
use crate::core::pack::{PackSource, PackTargets};
use crate::core::profile::Profile;
use crate::core::store::Store;

// ── Structured output types ─────────────────────────────────────────────────

/// Top-level diagnostic report.
#[derive(Debug, Serialize)]
pub struct DiagnoseReport {
    pub profile: String,
    pub pack_count: usize,
    pub packs: Vec<PackReport>,
    pub issue_count: usize,
}

/// Per-pack diagnostic status across all adapters.
#[derive(Debug, Serialize)]
pub struct PackReport {
    pub name: String,
    pub version: String,
    pub adapters: Vec<AdapterStatus>,
}

/// Status of a single pack within a single adapter.
#[derive(Debug, Serialize)]
pub struct AdapterStatus {
    pub adapter: String,
    pub status: PackHealth,
    /// Non-empty only when status is `Drifted`.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub issues: Vec<DiagnosticIssue>,
}

/// Health status of a pack in an adapter.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
// Use snake_case (not lowercase) so the multi-word `NotTargeted` variant
// serializes as "not_targeted" rather than "nottargeted".
#[serde(rename_all = "snake_case")]
pub enum PackHealth {
    Ok,
    Drifted,
    Missing,
    /// The adapter's CLI is not installed on this system.
    Skipped,
    /// The pack does not target this adapter.
    NotTargeted,
}

// ── Core logic ───────────────────────────────────────────────────────────────

/// Returns `true` if the pack targets the given adapter.
fn pack_targets_adapter(targets: &PackTargets, id: AdapterId) -> bool {
    match id {
        AdapterId::ClaudeCode => targets.claude_code,
        AdapterId::GeminiCli => targets.gemini_cli,
        AdapterId::CodexCli => targets.codex_cli,
    }
}

/// Build the full diagnostic report.
///
/// This is separated from `run()` so it can be tested with mock adapters.
///
/// `pack_targets` resolves a pack's target flags given its name and version.
/// In production this loads the pack from the store; in tests it can return
/// deterministic values without filesystem I/O.
pub fn build_report(
    profile_name: &str,
    profile: &Profile,
    adapters: &[Box<dyn CliAdapter>],
    pack_targets: &dyn Fn(&str, &semver::Version, Option<&PackSource>) -> PackTargets,
) -> Result<DiagnoseReport> {
    // Collect per-adapter issues once.
    struct AdapterDiag {
        id: AdapterId,
        name: String,
        installed: bool,
        issues: Vec<DiagnosticIssue>,
        tracked: std::collections::HashSet<String>,
    }

    let mut adapter_diags: Vec<AdapterDiag> = Vec::new();
    for adapter in adapters {
        let installed = adapter.is_installed();
        let (issues, tracked) = if installed {
            let issues = adapter
                .diagnose()
                .with_context(|| format!("diagnosing {}", adapter.name()))?;
            let tracked = adapter
                .tracked_packs()
                .with_context(|| format!("listing tracked packs for {}", adapter.name()))?;
            (issues, tracked)
        } else {
            (Vec::new(), std::collections::HashSet::new())
        };
        adapter_diags.push(AdapterDiag {
            id: adapter.id(),
            name: adapter.name().to_string(),
            installed,
            issues,
            tracked,
        });
    }

    let mut packs = Vec::new();
    let mut total_issues = 0;

    for installed_pack in &profile.packs {
        let targets = pack_targets(
            &installed_pack.name,
            &installed_pack.version,
            Some(&installed_pack.source),
        );

        let mut adapter_statuses = Vec::new();

        for diag in &adapter_diags {
            if !diag.installed {
                adapter_statuses.push(AdapterStatus {
                    adapter: diag.name.clone(),
                    status: PackHealth::Skipped,
                    issues: Vec::new(),
                });
                continue;
            }

            // If the pack does not target this adapter, skip it.
            if !pack_targets_adapter(&targets, diag.id) {
                adapter_statuses.push(AdapterStatus {
                    adapter: diag.name.clone(),
                    status: PackHealth::NotTargeted,
                    issues: Vec::new(),
                });
                continue;
            }

            // Collect issues that belong to this pack via the structured field.
            let pack_issues: Vec<DiagnosticIssue> = diag
                .issues
                .iter()
                .filter(|issue| issue.pack.as_deref() == Some(&installed_pack.name))
                .cloned()
                .collect();

            let status = if !diag.tracked.contains(&installed_pack.name) {
                // Pack is in the profile but this adapter has no record of it.
                PackHealth::Missing
            } else if pack_issues.is_empty() {
                PackHealth::Ok
            } else {
                PackHealth::Drifted
            };

            total_issues += pack_issues.len();

            adapter_statuses.push(AdapterStatus {
                adapter: diag.name.clone(),
                status,
                issues: pack_issues,
            });
        }

        packs.push(PackReport {
            name: installed_pack.name.clone(),
            version: installed_pack.version.to_string(),
            adapters: adapter_statuses,
        });
    }

    Ok(DiagnoseReport {
        profile: profile_name.to_string(),
        pack_count: profile.packs.len(),
        packs,
        issue_count: total_issues,
    })
}

// ── Formatting helpers (testable, pure functions) ───────────────────────────

/// Render the report as human-readable text.
///
/// Uses [`style`] helpers for color output — when colors are disabled (test
/// captures, piped output, `NO_COLOR`) the styled wrappers produce plain text,
/// so a single code path serves both cases.
pub fn format_human(report: &DiagnoseReport) -> String {
    let mut out = String::new();

    out.push_str(&format!(
        "{}: {}\n",
        style::header("Profile"),
        style::emphasis(report.profile.as_str())
    ));
    out.push_str(&format!(
        "{}: {} installed\n",
        style::header("Packs"),
        style::success(report.pack_count.to_string())
    ));

    if report.packs.is_empty() {
        out.push_str(&format!("\n  {}\n", style::subtext("(no packs installed)")));
    }

    for pack in &report.packs {
        out.push_str(&format!(
            "\n  {} v{}\n",
            style::pack_name(pack.name.as_str()),
            style::version(pack.version.as_str())
        ));
        for adapter_status in &pack.adapters {
            let status_str = match &adapter_status.status {
                PackHealth::Ok => format!("{}", style::success("ok")),
                PackHealth::Skipped => {
                    format!("{}", style::dim("skipped (not installed)"))
                }
                PackHealth::Missing => {
                    format!("{}", style::subtext("missing (not tracked by adapter)"))
                }
                PackHealth::NotTargeted => {
                    format!("{}", style::dim("skipped (not targeted)"))
                }
                PackHealth::Drifted => {
                    let details: Vec<&str> = adapter_status
                        .issues
                        .iter()
                        .map(|i| i.message.as_str())
                        .collect();
                    format!(
                        "{}",
                        style::subtext(format!("drifted ({})", details.join("; ")))
                    )
                }
            };
            out.push_str(&format!(
                "    {}: {}\n",
                style::target(adapter_status.adapter.as_str()),
                status_str
            ));
        }
    }

    out.push('\n');

    if report.issue_count == 0 {
        out.push_str(&format!("{}\n", style::success("No issues found.")));
    } else {
        out.push_str(&format!(
            "{} issue(s) found. Run `weave sync` to fix.\n",
            style::subtext(report.issue_count.to_string())
        ));
    }

    out
}

/// Render the report as JSON.
pub fn format_json(report: &DiagnoseReport) -> Result<String> {
    serde_json::to_string_pretty(report).context("serializing diagnose report to JSON")
}

// ── CLI entry point ─────────────────────────────────────────────────────────

/// Run the diagnose command.
pub fn run(json: bool) -> Result<()> {
    let config = Config::load().context("loading weave config")?;
    let profile = Profile::load(&config.active_profile).context("loading active profile")?;
    let adapters = adapters::all_adapters();

    let report = build_report(
        &config.active_profile,
        &profile,
        &adapters,
        &|name, version, source| match Store::load_pack(name, version, source) {
            Ok(pack) => pack.targets,
            Err(e) => {
                warn!(
                    "could not load pack '{name}' v{version} from store to check targets: {e}; assuming all targets"
                );
                PackTargets::default()
            }
        },
    )?;

    if json {
        let output = format_json(&report)?;
        println!("{output}");
    } else {
        let output = format_human(&report);
        print!("{output}");
    }

    Ok(())
}

// ── Tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::adapters::{DiagnosticIssue, Severity};
    use crate::core::pack::PackSource;
    use crate::core::profile::InstalledPack;

    fn make_profile(packs: &[(&str, &str)]) -> Profile {
        Profile {
            name: "test".into(),
            packs: packs
                .iter()
                .map(|(name, ver)| InstalledPack {
                    name: name.to_string(),
                    version: ver.parse().unwrap(),
                    source: PackSource::Registry {
                        registry_url: "https://example.com".into(),
                    },
                })
                .collect(),
        }
    }

    /// A mock adapter for testing.
    struct MockAdapter {
        adapter_id: AdapterId,
        adapter_name: String,
        installed: bool,
        issues: Vec<DiagnosticIssue>,
        tracked: std::collections::HashSet<String>,
    }

    impl CliAdapter for MockAdapter {
        fn id(&self) -> AdapterId {
            self.adapter_id
        }
        fn name(&self) -> &str {
            &self.adapter_name
        }
        fn is_installed(&self) -> bool {
            self.installed
        }
        fn config_dir(&self) -> std::path::PathBuf {
            std::path::PathBuf::from("/mock")
        }
        fn apply(
            &self,
            _pack: &crate::core::pack::ResolvedPack,
            _options: &crate::adapters::ApplyOptions,
        ) -> crate::error::Result<()> {
            Ok(())
        }
        fn remove(&self, _pack_name: &str) -> crate::error::Result<Vec<String>> {
            Ok(vec![])
        }
        fn diagnose(&self) -> crate::error::Result<Vec<DiagnosticIssue>> {
            Ok(self.issues.clone())
        }
        fn tracked_packs(&self) -> crate::error::Result<std::collections::HashSet<String>> {
            Ok(self.tracked.clone())
        }
    }

    fn mock_adapter(
        id: AdapterId,
        name: &str,
        installed: bool,
        tracked: &[&str],
        issues: Vec<DiagnosticIssue>,
    ) -> Box<dyn CliAdapter> {
        Box::new(MockAdapter {
            adapter_id: id,
            adapter_name: name.to_string(),
            installed,
            issues,
            tracked: tracked.iter().map(|s| s.to_string()).collect(),
        })
    }

    /// Default target lookup: all CLIs targeted.
    fn all_targets(
        _name: &str,
        _version: &semver::Version,
        _source: Option<&PackSource>,
    ) -> PackTargets {
        PackTargets::default()
    }

    #[test]
    fn report_all_ok() {
        let profile = make_profile(&[("webdev", "1.0.0"), ("datatools", "2.1.0")]);
        let adapters: Vec<Box<dyn CliAdapter>> = vec![
            mock_adapter(
                AdapterId::ClaudeCode,
                "Claude Code",
                true,
                &["webdev", "datatools"],
                vec![],
            ),
            mock_adapter(
                AdapterId::GeminiCli,
                "Gemini CLI",
                true,
                &["webdev", "datatools"],
                vec![],
            ),
        ];

        let report = build_report("default", &profile, &adapters, &all_targets).unwrap();
        assert_eq!(report.issue_count, 0);
        assert_eq!(report.pack_count, 2);
        assert_eq!(report.packs[0].adapters[0].status, PackHealth::Ok);
        assert_eq!(report.packs[1].adapters[1].status, PackHealth::Ok);
    }

    #[test]
    fn report_drifted() {
        let profile = make_profile(&[("webdev", "1.0.0")]);
        let adapters: Vec<Box<dyn CliAdapter>> = vec![mock_adapter(
            AdapterId::GeminiCli,
            "Gemini CLI",
            true,
            &["webdev"],
            vec![DiagnosticIssue {
                severity: Severity::Warning,
                message: "server 'puppeteer' (from pack 'webdev') tracked but missing".into(),
                suggestion: Some("run `weave install webdev` to re-apply".into()),
                pack: Some("webdev".into()),
            }],
        )];

        let report = build_report("default", &profile, &adapters, &all_targets).unwrap();
        assert_eq!(report.issue_count, 1);
        assert_eq!(report.packs[0].adapters[0].status, PackHealth::Drifted);
    }

    #[test]
    fn report_missing_pack() {
        let profile = make_profile(&[("webdev", "1.0.0")]);
        let adapters: Vec<Box<dyn CliAdapter>> = vec![mock_adapter(
            AdapterId::CodexCli,
            "Codex CLI",
            true,
            &[],
            vec![],
        )];

        let report = build_report("default", &profile, &adapters, &all_targets).unwrap();
        assert_eq!(report.packs[0].adapters[0].status, PackHealth::Missing);
    }

    #[test]
    fn report_skipped_uninstalled_adapter() {
        let profile = make_profile(&[("webdev", "1.0.0")]);
        let adapters: Vec<Box<dyn CliAdapter>> = vec![mock_adapter(
            AdapterId::GeminiCli,
            "Gemini CLI",
            false,
            &[],
            vec![],
        )];

        let report = build_report("default", &profile, &adapters, &all_targets).unwrap();
        assert_eq!(report.packs[0].adapters[0].status, PackHealth::Skipped);
    }

    #[test]
    fn pack_targets_adapter_mapping() {
        let all_true = PackTargets::default();
        assert!(pack_targets_adapter(&all_true, AdapterId::ClaudeCode));
        assert!(pack_targets_adapter(&all_true, AdapterId::GeminiCli));
        assert!(pack_targets_adapter(&all_true, AdapterId::CodexCli));

        let gemini_only = PackTargets {
            claude_code: false,
            gemini_cli: true,
            codex_cli: false,
        };
        assert!(!pack_targets_adapter(&gemini_only, AdapterId::ClaudeCode));
        assert!(pack_targets_adapter(&gemini_only, AdapterId::GeminiCli));
        assert!(!pack_targets_adapter(&gemini_only, AdapterId::CodexCli));

        let claude_only = PackTargets {
            claude_code: true,
            gemini_cli: false,
            codex_cli: false,
        };
        assert!(pack_targets_adapter(&claude_only, AdapterId::ClaudeCode));
        assert!(!pack_targets_adapter(&claude_only, AdapterId::GeminiCli));
        assert!(!pack_targets_adapter(&claude_only, AdapterId::CodexCli));

        let codex_only = PackTargets {
            claude_code: false,
            gemini_cli: false,
            codex_cli: true,
        };
        assert!(!pack_targets_adapter(&codex_only, AdapterId::ClaudeCode));
        assert!(!pack_targets_adapter(&codex_only, AdapterId::GeminiCli));
        assert!(pack_targets_adapter(&codex_only, AdapterId::CodexCli));
    }

    #[test]
    fn report_empty_profile() {
        let profile = make_profile(&[]);
        let adapters: Vec<Box<dyn CliAdapter>> = vec![mock_adapter(
            AdapterId::ClaudeCode,
            "Claude Code",
            true,
            &[],
            vec![],
        )];

        let report = build_report("default", &profile, &adapters, &all_targets).unwrap();
        assert_eq!(report.pack_count, 0);
        assert_eq!(report.issue_count, 0);
        assert!(report.packs.is_empty());
    }

    #[test]
    fn human_format_no_issues() {
        let report = DiagnoseReport {
            profile: "default".into(),
            pack_count: 1,
            packs: vec![PackReport {
                name: "webdev".into(),
                version: "1.0.0".into(),
                adapters: vec![AdapterStatus {
                    adapter: "Claude Code".into(),
                    status: PackHealth::Ok,
                    issues: vec![],
                }],
            }],
            issue_count: 0,
        };

        let text = format_human(&report);
        assert!(text.contains("Profile: default"));
        assert!(text.contains("Packs: 1 installed"));
        assert!(text.contains("webdev v1.0.0"));
        assert!(text.contains("Claude Code: ok"));
        assert!(text.contains("No issues found."));
    }

    #[test]
    fn human_format_with_drift() {
        let report = DiagnoseReport {
            profile: "default".into(),
            pack_count: 1,
            packs: vec![PackReport {
                name: "webdev".into(),
                version: "1.0.0".into(),
                adapters: vec![AdapterStatus {
                    adapter: "Gemini CLI".into(),
                    status: PackHealth::Drifted,
                    issues: vec![DiagnosticIssue {
                        severity: Severity::Warning,
                        message: "server 'puppeteer' missing".into(),
                        suggestion: None,
                        pack: Some("webdev".into()),
                    }],
                }],
            }],
            issue_count: 1,
        };

        let text = format_human(&report);
        assert!(text.contains("drifted"));
        assert!(text.contains("1 issue(s) found. Run `weave sync` to fix."));
    }

    #[test]
    fn json_format_roundtrip() {
        let report = DiagnoseReport {
            profile: "default".into(),
            pack_count: 1,
            packs: vec![PackReport {
                name: "webdev".into(),
                version: "1.0.0".into(),
                adapters: vec![AdapterStatus {
                    adapter: "Claude Code".into(),
                    status: PackHealth::Ok,
                    issues: vec![],
                }],
            }],
            issue_count: 0,
        };

        let json_str = format_json(&report).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&json_str).unwrap();
        assert_eq!(parsed["profile"], "default");
        assert_eq!(parsed["pack_count"], 1);
        assert_eq!(parsed["issue_count"], 0);
        assert_eq!(parsed["packs"][0]["name"], "webdev");
        assert_eq!(parsed["packs"][0]["adapters"][0]["status"], "ok");
    }

    #[test]
    fn report_not_targeted() {
        let profile = make_profile(&[("webdev", "1.0.0")]);
        let adapters: Vec<Box<dyn CliAdapter>> = vec![
            mock_adapter(
                AdapterId::ClaudeCode,
                "Claude Code",
                true,
                &["webdev"],
                vec![],
            ),
            mock_adapter(
                AdapterId::GeminiCli,
                "Gemini CLI",
                true,
                &["webdev"],
                vec![],
            ),
            mock_adapter(AdapterId::CodexCli, "Codex CLI", true, &["webdev"], vec![]),
        ];

        // Only target Claude Code — Gemini CLI and Codex CLI should be NotTargeted.
        let claude_only =
            |_name: &str, _version: &semver::Version, _source: Option<&PackSource>| PackTargets {
                claude_code: true,
                gemini_cli: false,
                codex_cli: false,
            };

        let report = build_report("default", &profile, &adapters, &claude_only).unwrap();
        assert_eq!(report.packs.len(), 1);

        let statuses = &report.packs[0].adapters;
        assert_eq!(statuses[0].adapter, "Claude Code");
        assert_eq!(statuses[0].status, PackHealth::Ok);

        assert_eq!(statuses[1].adapter, "Gemini CLI");
        assert_eq!(statuses[1].status, PackHealth::NotTargeted);

        assert_eq!(statuses[2].adapter, "Codex CLI");
        assert_eq!(statuses[2].status, PackHealth::NotTargeted);

        // NotTargeted adapters should not contribute to issue count.
        assert_eq!(report.issue_count, 0);
    }

    #[test]
    fn human_format_empty_profile() {
        let report = DiagnoseReport {
            profile: "default".into(),
            pack_count: 0,
            packs: vec![],
            issue_count: 0,
        };

        let text = format_human(&report);
        assert!(text.contains("Packs: 0 installed"));
        assert!(text.contains("(no packs installed)"));
        assert!(text.contains("No issues found."));
    }
}
