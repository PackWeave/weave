use std::collections::HashMap;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::adapters::{CliAdapter, DiagnosticIssue, Severity};
use crate::core::pack::ResolvedPack;
use crate::core::store::Store;
use crate::error::{Result, WeaveError};
use crate::util;

/// Tracks the settings contribution of a single pack so it can be safely undone.
#[derive(Debug, Clone, Serialize, Deserialize)]
struct SettingsRecord {
    /// The fragment we merged in (pack's settings/codex.json).
    applied: serde_json::Value,
    /// The pre-apply values for each top-level key in `applied`
    /// (Value::Null means the key was absent before installation).
    original: serde_json::Value,
}

/// Sidecar manifest tracking what weave wrote to Codex CLI config.
///
/// NOTE: Codex CLI (openai/codex) does NOT support MCP server configuration
/// in its config files — a search of the openai/codex repository returns zero
/// results for "mcpServer", "mcp_server", or "MCP". Therefore this adapter
/// only tracks prompts (AGENTS.md blocks) and settings (config.json keys).
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
struct CodexManifest {
    #[serde(default)]
    prompt_blocks: Vec<String>, // pack names with prompt content
    #[serde(default)]
    settings: HashMap<String, SettingsRecord>, // pack_name -> settings record
}

pub struct CodexAdapter {
    home: Option<PathBuf>,
    /// Current working directory, kept for structural parity with other adapters.
    #[allow(dead_code)]
    project_root: PathBuf,
}

impl Default for CodexAdapter {
    fn default() -> Self {
        Self::new()
    }
}

impl CodexAdapter {
    pub fn new() -> Self {
        Self {
            home: dirs::home_dir(),
            project_root: std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")),
        }
    }

    /// Override both home and project root for testing without writing to real `~/.codex/`.
    pub fn with_home_and_project(home: PathBuf, project_root: PathBuf) -> Self {
        Self {
            home: Some(home),
            project_root,
        }
    }

    fn home(&self) -> Result<&PathBuf> {
        self.home.as_ref().ok_or(WeaveError::NoHomeDir)
    }

    // ── Path helpers ──────────────────────────────────────────────────────────

    /// `~/.codex/`
    fn codex_dir(&self) -> Result<PathBuf> {
        Ok(self.home()?.join(".codex"))
    }

    /// `~/.codex/config.json`
    ///
    /// Codex supports both `config.yaml` and `config.json`. We use JSON to stay
    /// consistent with the rest of weave's config handling, and because
    /// `serde_json` is already a dependency (no need to add `serde_yaml`).
    fn config_path(&self) -> Result<PathBuf> {
        Ok(self.codex_dir()?.join("config.json"))
    }

    /// `~/.codex/AGENTS.md`
    ///
    /// Codex searches for `AGENTS.md` in `~/.codex/`, the repository root, and
    /// the current working directory, merging them hierarchically. We write to
    /// the global location so instructions apply across all projects.
    fn agents_md_path(&self) -> Result<PathBuf> {
        Ok(self.codex_dir()?.join("AGENTS.md"))
    }

    /// `~/.codex/.packweave_manifest.json`
    fn manifest_path(&self) -> Result<PathBuf> {
        Ok(self.codex_dir()?.join(".packweave_manifest.json"))
    }

    // ── Manifest helpers ──────────────────────────────────────────────────────

    fn load_manifest(&self) -> Result<CodexManifest> {
        let path = self.manifest_path()?;
        if !path.exists() {
            return Ok(CodexManifest::default());
        }
        let content = util::read_file(&path)?;
        serde_json::from_str(&content).map_err(|e| WeaveError::Json { path, source: e })
    }

    fn save_manifest(&self, manifest: &CodexManifest) -> Result<()> {
        let path = self.manifest_path()?;
        // CodexManifest only contains String/HashMap/Vec fields — cannot fail.
        let content =
            serde_json::to_string_pretty(manifest).expect("manifest serialization cannot fail");
        util::write_file(&path, &content)
    }

    // ── Prompt helpers ────────────────────────────────────────────────────────

    /// Append pack prompt content to `~/.codex/AGENTS.md` inside tagged delimiters.
    fn apply_prompts(&self, pack: &ResolvedPack, manifest: &mut CodexManifest) -> Result<()> {
        let prompt_content =
            Store::read_pack_file(&pack.pack.name, &pack.pack.version, "prompts/codex.md")?.or(
                Store::read_pack_file(&pack.pack.name, &pack.pack.version, "prompts/system.md")?,
            );

        let prompt_content = match prompt_content {
            Some(c) if !c.trim().is_empty() => c,
            _ => return Ok(()),
        };

        let agents_md = self.agents_md_path()?;
        let mut content = if agents_md.exists() {
            util::read_file(&agents_md)?
        } else {
            String::new()
        };

        let begin_tag = format!("<!-- packweave:begin:{} -->", pack.pack.name);
        let end_tag = format!("<!-- packweave:end:{} -->", pack.pack.name);

        // Remove existing block (idempotency).
        // Search for end_tag starting after begin_tag to avoid matching another
        // pack's end tag that might appear before this pack's begin tag.
        if let Some(start) = content.find(&begin_tag) {
            if let Some(end_offset) = content[start..].find(&end_tag) {
                let end_pos = start + end_offset;
                let end = end_pos + end_tag.len();
                let end = if content[end..].starts_with('\n') {
                    end + 1
                } else {
                    end
                };
                content.replace_range(start..end, "");
            }
        }

        if !content.is_empty() && !content.ends_with('\n') {
            content.push('\n');
        }
        content.push_str(&begin_tag);
        content.push('\n');
        content.push_str(prompt_content.trim());
        content.push('\n');
        content.push_str(&end_tag);
        content.push('\n');

        util::write_file(&agents_md, &content)?;

        if !manifest.prompt_blocks.contains(&pack.pack.name) {
            manifest.prompt_blocks.push(pack.pack.name.clone());
        }

        Ok(())
    }

    fn remove_prompts(&self, pack_name: &str, manifest: &mut CodexManifest) -> Result<()> {
        let agents_md = self.agents_md_path();
        let agents_md = match agents_md {
            Ok(p) if p.exists() => p,
            _ => {
                manifest.prompt_blocks.retain(|n| n != pack_name);
                return Ok(());
            }
        };

        let mut content = util::read_file(&agents_md)?;
        let begin_tag = format!("<!-- packweave:begin:{pack_name} -->");
        let end_tag = format!("<!-- packweave:end:{pack_name} -->");

        if let Some(start) = content.find(&begin_tag) {
            if let Some(end_offset) = content[start..].find(&end_tag) {
                let end_pos = start + end_offset;
                let end = end_pos + end_tag.len();
                let end = if content[end..].starts_with('\n') {
                    end + 1
                } else {
                    end
                };
                content.replace_range(start..end, "");
                util::write_file(&agents_md, &content)?;
            }
        }

        manifest.prompt_blocks.retain(|n| n != pack_name);
        Ok(())
    }

    // ── Settings helpers ──────────────────────────────────────────────────────

    /// Deep-merge settings fragment into `~/.codex/config.json`.
    fn apply_settings(&self, pack: &ResolvedPack, manifest: &mut CodexManifest) -> Result<()> {
        let settings_content =
            Store::read_pack_file(&pack.pack.name, &pack.pack.version, "settings/codex.json")?;

        let settings_content = match settings_content {
            Some(c) if !c.trim().is_empty() => c,
            _ => return Ok(()),
        };

        let fragment: serde_json::Value =
            serde_json::from_str(&settings_content).map_err(|e| WeaveError::ApplyFailed {
                pack: pack.pack.name.clone(),
                cli: "Codex CLI".into(),
                reason: format!("invalid settings/codex.json: {e}"),
            })?;

        if !fragment.is_object() {
            return Err(WeaveError::ApplyFailed {
                pack: pack.pack.name.clone(),
                cli: "Codex CLI".into(),
                reason: "settings/codex.json must be a JSON object, not a primitive or array"
                    .into(),
            });
        }

        let path = self.config_path()?;
        let mut config: serde_json::Value = if path.exists() {
            let content = util::read_file(&path)?;
            let value: serde_json::Value =
                serde_json::from_str(&content).map_err(|e| WeaveError::Json {
                    path: path.clone(),
                    source: e,
                })?;
            if !value.is_object() {
                return Err(WeaveError::ApplyFailed {
                    pack: pack.pack.name.clone(),
                    cli: "Codex CLI".into(),
                    reason: format!(
                        "config file at {} must be a JSON object — found array or primitive",
                        path.display()
                    ),
                });
            }
            value
        } else {
            serde_json::json!({})
        };

        let frag_obj = fragment
            .as_object()
            .expect("checked is_object() above — always Some here");

        let mut snap = serde_json::Map::new();
        for key in frag_obj.keys() {
            let before = config.get(key).cloned().unwrap_or(serde_json::Value::Null);
            snap.insert(key.clone(), before);
        }
        let original = serde_json::Value::Object(snap);

        deep_merge(&mut config, &fragment);

        // JSON serialization of a valid serde_json::Value cannot fail.
        let output = serde_json::to_string_pretty(&config).expect("JSON serialization cannot fail");
        util::write_file(&path, &output)?;

        manifest.settings.insert(
            pack.pack.name.clone(),
            SettingsRecord {
                applied: fragment,
                original,
            },
        );

        Ok(())
    }

    /// Remove settings written by a pack from `~/.codex/config.json`.
    fn remove_settings(&self, pack_name: &str, manifest: &mut CodexManifest) -> Result<()> {
        let record = match manifest.settings.get(pack_name).cloned() {
            Some(r) => r,
            None => return Ok(()),
        };

        let path = self.config_path()?;

        if !path.exists() {
            manifest.settings.remove(pack_name);
            return Ok(());
        }

        let content = util::read_file(&path)?;
        let mut config: serde_json::Value =
            serde_json::from_str(&content).map_err(|e| WeaveError::Json {
                path: path.clone(),
                source: e,
            })?;

        let frag_obj = match record.applied.as_object() {
            Some(o) => o,
            None => {
                return Err(WeaveError::ApplyFailed {
                    pack: pack_name.to_string(),
                    cli: "Codex CLI".into(),
                    reason: "settings manifest 'applied' fragment is not a JSON object".into(),
                });
            }
        };

        let config_obj = match config.as_object_mut() {
            Some(o) => o,
            None => {
                return Err(WeaveError::ApplyFailed {
                    pack: pack_name.to_string(),
                    cli: "Codex CLI".into(),
                    reason: format!(
                        "config file at {} is not a JSON object; cannot restore settings",
                        path.display()
                    ),
                });
            }
        };

        let orig_obj = record.original.as_object();

        for (key, applied_val) in frag_obj {
            let pre = orig_obj
                .and_then(|o| o.get(key))
                .cloned()
                .unwrap_or(serde_json::Value::Null);
            let mut expected = pre.clone();
            deep_merge(&mut expected, applied_val);

            let current = config_obj
                .get(key)
                .cloned()
                .unwrap_or(serde_json::Value::Null);

            if current == expected {
                if pre.is_null() {
                    config_obj.remove(key);
                } else {
                    config_obj.insert(key.clone(), pre);
                }
            } else {
                log::warn!(
                    "settings key '{key}' was modified after '{pack_name}' was installed; \
                     leaving it in place — remove manually if desired"
                );
            }
        }

        // JSON serialization of a valid serde_json::Value cannot fail.
        let output = serde_json::to_string_pretty(&config).expect("JSON serialization cannot fail");
        util::write_file(&path, &output)?;
        manifest.settings.remove(pack_name);
        Ok(())
    }
}

impl CliAdapter for CodexAdapter {
    fn name(&self) -> &str {
        "Codex CLI"
    }

    fn is_installed(&self) -> bool {
        self.codex_dir().map(|d| d.exists()).unwrap_or(false) || which_exists("codex")
    }

    fn config_dir(&self) -> PathBuf {
        self.codex_dir().unwrap_or_else(|_| PathBuf::from(".codex"))
    }

    fn apply(&self, pack: &ResolvedPack) -> Result<()> {
        if !pack.pack.targets.codex_cli {
            return Ok(());
        }

        util::ensure_dir(&self.codex_dir()?)?;

        let mut manifest = self.load_manifest()?;
        // Save after each successful step so partial failures leave tracked state
        // rather than untracked on-disk writes with no cleanup path.
        self.apply_prompts(pack, &mut manifest)?;
        self.save_manifest(&manifest)?;
        self.apply_settings(pack, &mut manifest)?;
        self.save_manifest(&manifest)?;

        Ok(())
    }

    fn remove(&self, pack_name: &str) -> Result<()> {
        let manifest_path = self.manifest_path()?;
        // If no manifest file exists, nothing was ever applied — skip entirely
        // to avoid creating ~/.codex/.packweave_manifest.json as a side-effect.
        if !manifest_path.exists() {
            return Ok(());
        }

        let mut manifest = self.load_manifest()?;
        self.remove_prompts(pack_name, &mut manifest)?;
        self.remove_settings(pack_name, &mut manifest)?;
        self.save_manifest(&manifest)?;

        Ok(())
    }

    fn diagnose(&self) -> Result<Vec<DiagnosticIssue>> {
        let mut issues = Vec::new();
        let manifest = self.load_manifest()?;

        // Check tracked prompt blocks exist in AGENTS.md
        if !manifest.prompt_blocks.is_empty() {
            let agents_md = self.agents_md_path()?;
            let content = if agents_md.exists() {
                util::read_file(&agents_md)?
            } else {
                String::new()
            };
            for pack_name in &manifest.prompt_blocks {
                let begin_tag = format!("<!-- packweave:begin:{pack_name} -->");
                if !content.contains(&begin_tag) {
                    issues.push(DiagnosticIssue {
                        severity: Severity::Warning,
                        message: format!(
                            "prompt block for '{pack_name}' is tracked but missing from AGENTS.md"
                        ),
                        suggestion: Some(format!("run `weave install {pack_name}` to re-apply")),
                    });
                }
            }
        }

        // Check tracked settings keys still exist in config.json
        if !manifest.settings.is_empty() {
            let config_path = self.config_path()?;
            if config_path.exists() {
                let content = util::read_file(&config_path)?;
                if let Ok(config) = serde_json::from_str::<serde_json::Value>(&content) {
                    for (pack_name, record) in &manifest.settings {
                        if let Some(frag_obj) = record.applied.as_object() {
                            for key in frag_obj.keys() {
                                if config.get(key).is_none() {
                                    issues.push(DiagnosticIssue {
                                        severity: Severity::Warning,
                                        message: format!(
                                            "settings key '{key}' (from pack '{pack_name}') is tracked but missing from config.json"
                                        ),
                                        suggestion: Some(format!(
                                            "run `weave install {pack_name}` to re-apply"
                                        )),
                                    });
                                }
                            }
                        }
                    }
                }
            }
        }

        Ok(issues)
    }
}

/// Deep-merge source into target. Arrays are replaced, objects are merged recursively.
fn deep_merge(target: &mut serde_json::Value, source: &serde_json::Value) {
    match (target, source) {
        (serde_json::Value::Object(target_map), serde_json::Value::Object(source_map)) => {
            for (key, source_val) in source_map {
                let target_val = target_map
                    .entry(key.clone())
                    .or_insert(serde_json::Value::Null);
                deep_merge(target_val, source_val);
            }
        }
        (target, source) => {
            *target = source.clone();
        }
    }
}

/// Check if a command exists on PATH in a cross-platform way.
fn which_exists(cmd: &str) -> bool {
    #[cfg(windows)]
    let check_cmd = "where";
    #[cfg(not(windows))]
    let check_cmd = "which";

    std::process::Command::new(check_cmd)
        .arg(cmd)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::pack::{Pack, PackSource, PackTargets};
    use tempfile::TempDir;

    fn test_adapter(dir: &TempDir) -> CodexAdapter {
        CodexAdapter::with_home_and_project(dir.path().to_path_buf(), dir.path().to_path_buf())
    }

    fn test_pack_no_servers(name: &str) -> ResolvedPack {
        ResolvedPack {
            pack: Pack {
                name: name.to_string(),
                version: semver::Version::new(1, 0, 0),
                description: "Test".into(),
                authors: vec![],
                license: None,
                repository: None,
                keywords: vec![],
                min_tool_version: None,
                servers: vec![],
                dependencies: HashMap::new(),
                extensions: Default::default(),
                targets: PackTargets::default(),
            },
            source: PackSource::Registry {
                registry_url: "https://example.com".into(),
            },
        }
    }

    fn setup_dir(dir: &TempDir) {
        std::fs::create_dir_all(dir.path().join(".codex")).unwrap();
    }

    #[test]
    fn apply_and_remove_prompt_block() {
        let dir = TempDir::new().unwrap();
        let adapter = test_adapter(&dir);
        setup_dir(&dir);

        let pack = test_pack_no_servers("test-pack");
        let begin_tag = "<!-- packweave:begin:test-pack -->";
        let end_tag = "<!-- packweave:end:test-pack -->";
        let prompt = "Do something useful.";

        let mut manifest = CodexManifest::default();

        // Manually write a prompt block to simulate apply_prompts
        let agents_md = dir.path().join(".codex").join("AGENTS.md");
        let content = format!("{begin_tag}\n{prompt}\n{end_tag}\n");
        std::fs::write(&agents_md, &content).unwrap();
        manifest.prompt_blocks.push(pack.pack.name.clone());

        // Verify block exists
        let read = std::fs::read_to_string(&agents_md).unwrap();
        assert!(read.contains(begin_tag));
        assert!(read.contains(prompt));

        // Remove
        adapter.remove_prompts("test-pack", &mut manifest).unwrap();
        let read = std::fs::read_to_string(&agents_md).unwrap();
        assert!(!read.contains(begin_tag));
        assert!(!read.contains(end_tag));
        assert!(manifest.prompt_blocks.is_empty());
    }

    #[test]
    fn apply_and_remove_settings() {
        let dir = TempDir::new().unwrap();
        let adapter = test_adapter(&dir);
        setup_dir(&dir);

        let pack = test_pack_no_servers("my-pack");
        let fragment = serde_json::json!({ "model": "o4-mini", "approvalMode": "suggest" });
        let mut manifest = CodexManifest::default();

        // Simulate apply_settings by directly calling the settings logic
        // (Store::read_pack_file isn't available in unit tests, so we exercise
        //  the config.json read/write path manually.)
        let config_path = dir.path().join(".codex").join("config.json");
        let mut config = serde_json::json!({});
        deep_merge(&mut config, &fragment);
        std::fs::write(&config_path, serde_json::to_string_pretty(&config).unwrap()).unwrap();
        manifest.settings.insert(
            pack.pack.name.clone(),
            SettingsRecord {
                applied: fragment.clone(),
                original: serde_json::json!({ "model": null, "approvalMode": null }),
            },
        );

        // Config should have the keys
        let read: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(&config_path).unwrap()).unwrap();
        assert_eq!(read["model"], "o4-mini");

        // Remove
        adapter.remove_settings("my-pack", &mut manifest).unwrap();
        let read: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(&config_path).unwrap()).unwrap();
        assert!(read.get("model").is_none());
        assert!(manifest.settings.is_empty());
    }

    #[test]
    fn diagnose_missing_prompt_block() {
        let dir = TempDir::new().unwrap();
        let adapter = test_adapter(&dir);
        setup_dir(&dir);

        // Write a manifest that claims a prompt block exists, but no AGENTS.md
        let manifest = CodexManifest {
            prompt_blocks: vec!["ghost-pack".into()],
            settings: HashMap::new(),
        };
        adapter.save_manifest(&manifest).unwrap();

        let issues = adapter.diagnose().unwrap();
        assert_eq!(issues.len(), 1);
        assert!(issues[0].message.contains("prompt block for 'ghost-pack'"));
    }

    #[test]
    fn targets_codex_cli_false_skips_apply() {
        let dir = TempDir::new().unwrap();
        let adapter = test_adapter(&dir);
        setup_dir(&dir);

        let mut pack = test_pack_no_servers("skip-pack");
        pack.pack.targets.codex_cli = false;

        // apply() should return Ok without writing anything
        adapter.apply(&pack).unwrap();
        let agents_md = dir.path().join(".codex").join("AGENTS.md");
        assert!(!agents_md.exists());
    }

    #[test]
    fn manifest_round_trips() {
        let dir = TempDir::new().unwrap();
        let adapter = test_adapter(&dir);
        setup_dir(&dir);

        let mut manifest = CodexManifest::default();
        manifest.prompt_blocks.push("some-pack".into());
        manifest.settings.insert(
            "some-pack".into(),
            SettingsRecord {
                applied: serde_json::json!({ "model": "gpt-4o" }),
                original: serde_json::json!({ "model": null }),
            },
        );

        adapter.save_manifest(&manifest).unwrap();
        let loaded = adapter.load_manifest().unwrap();
        assert_eq!(loaded.prompt_blocks, vec!["some-pack"]);
        assert!(loaded.settings.contains_key("some-pack"));
    }
}
