pub mod claude_code;
pub mod codex_cli;
pub mod gemini_cli;

use std::collections::HashSet;
use std::path::PathBuf;

use serde::Serialize;

use crate::core::pack::ResolvedPack;
use crate::error::Result;

/// A diagnostic issue found by an adapter.
#[derive(Debug, Clone, Serialize)]
pub struct DiagnosticIssue {
    pub severity: Severity,
    pub message: String,
    pub suggestion: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Severity {
    Warning,
    Error,
}

/// The central adapter trait. Every supported CLI implements this.
/// The core calls these methods; it never touches CLI config files directly.
pub trait CliAdapter: Send + Sync {
    /// Human-readable name, e.g. "Claude Code"
    fn name(&self) -> &str;

    /// Whether this CLI appears to be installed on the system.
    fn is_installed(&self) -> bool;

    /// Root config directory for this CLI.
    fn config_dir(&self) -> PathBuf;

    /// Apply a pack's contributions to this CLI's config.
    /// Must be idempotent — calling twice has the same effect as calling once.
    fn apply(&self, pack: &ResolvedPack) -> Result<()>;

    /// Remove all contributions made by a pack.
    /// Must leave user's manual edits untouched.
    fn remove(&self, pack_name: &str) -> Result<()>;

    /// Verify the CLI's current config is consistent with installed packs.
    /// Returns a list of issues for `weave diagnose`.
    fn diagnose(&self) -> Result<Vec<DiagnosticIssue>>;

    /// Returns the set of pack names this adapter is currently tracking
    /// (i.e., has contributions for in its sidecar manifest).
    fn tracked_packs(&self) -> Result<HashSet<String>>;
}

/// Returns all available adapters.
pub fn all_adapters() -> Vec<Box<dyn CliAdapter>> {
    vec![
        Box::new(claude_code::ClaudeCodeAdapter::new()),
        Box::new(gemini_cli::GeminiCliAdapter::new()),
        Box::new(codex_cli::CodexAdapter::new()),
    ]
}

/// Returns only adapters for CLIs that are installed.
pub fn installed_adapters() -> Vec<Box<dyn CliAdapter>> {
    all_adapters()
        .into_iter()
        .filter(|a| a.is_installed())
        .collect()
}
