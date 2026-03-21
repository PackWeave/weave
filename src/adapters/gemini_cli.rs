use std::collections::HashMap;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::adapters::{CliAdapter, DiagnosticIssue, Severity};
use crate::core::pack::{McpServer, ResolvedPack};
use crate::core::store::Store;
use crate::error::{Result, WeaveError};
use crate::util;

/// Tracks the settings contribution of a single pack so it can be safely undone.
#[derive(Debug, Clone, Serialize, Deserialize)]
struct SettingsRecord {
    applied: serde_json::Value,
    original: serde_json::Value,
}

/// Sidecar manifest tracking what weave wrote to Gemini CLI config.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
struct GeminiManifest {
    #[serde(default)]
    servers: HashMap<String, String>, // server_name -> pack_name
    #[serde(default)]
    prompt_blocks: Vec<String>,
    #[serde(default)]
    settings: HashMap<String, SettingsRecord>, // pack_name -> settings record
}

pub struct GeminiCliAdapter {
    home: Option<PathBuf>,
    /// Current working directory, used to detect project-scope config.
    project_root: PathBuf,
}

impl Default for GeminiCliAdapter {
    fn default() -> Self {
        Self::new()
    }
}

impl GeminiCliAdapter {
    pub fn new() -> Self {
        Self {
            home: dirs::home_dir(),
            project_root: std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")),
        }
    }

    /// Override the home directory for testing without writing to real `~/.gemini/`.
    pub fn with_home(home: PathBuf) -> Self {
        Self {
            home: Some(home.clone()),
            project_root: home,
        }
    }

    /// Override both home and project root for testing.
    pub fn with_home_and_project(home: PathBuf, project_root: PathBuf) -> Self {
        Self {
            home: Some(home),
            project_root,
        }
    }

    fn home(&self) -> Result<&PathBuf> {
        self.home.as_ref().ok_or(WeaveError::NoHomeDir)
    }

    // ── User-scope paths ──────────────────────────────────────────────────────

    /// `~/.gemini/`
    fn gemini_dir(&self) -> Result<PathBuf> {
        Ok(self.home()?.join(".gemini"))
    }

    /// `~/.gemini/settings.json`
    fn settings_path(&self) -> Result<PathBuf> {
        Ok(self.gemini_dir()?.join("settings.json"))
    }

    /// `~/.gemini/.packweave_manifest.json`
    fn manifest_path(&self) -> Result<PathBuf> {
        Ok(self.gemini_dir()?.join(".packweave_manifest.json"))
    }

    // ── Project-scope paths ───────────────────────────────────────────────────

    /// `.gemini/` in the current project root
    fn project_gemini_dir(&self) -> PathBuf {
        self.project_root.join(".gemini")
    }

    /// `.gemini/settings.json` — project-scope settings + MCP servers
    fn project_settings_path(&self) -> PathBuf {
        self.project_gemini_dir().join("settings.json")
    }

    /// `.gemini/.packweave_manifest.json` — project-scope ownership tracking
    fn project_manifest_path(&self) -> PathBuf {
        self.project_gemini_dir().join(".packweave_manifest.json")
    }

    /// Returns true if the project has a `.gemini/` directory, indicating
    /// that project-scope config should be maintained.
    fn has_project_scope(&self) -> bool {
        self.project_gemini_dir().exists()
    }

    // ── Manifest helpers ──────────────────────────────────────────────────────

    fn load_manifest(&self) -> Result<GeminiManifest> {
        let path = self.manifest_path()?;
        if !path.exists() {
            return Ok(GeminiManifest::default());
        }
        let content = util::read_file(&path)?;
        serde_json::from_str(&content).map_err(|e| WeaveError::Json { path, source: e })
    }

    fn save_manifest(&self, manifest: &GeminiManifest) -> Result<()> {
        let path = self.manifest_path()?;
        let content =
            // GeminiManifest only contains String/HashMap/Vec fields — cannot fail.
            serde_json::to_string_pretty(manifest).expect("manifest serialization cannot fail");
        util::write_file(&path, &content)
    }

    fn load_project_manifest(&self) -> Result<GeminiManifest> {
        let path = self.project_manifest_path();
        if !path.exists() {
            return Ok(GeminiManifest::default());
        }
        let content = util::read_file(&path)?;
        serde_json::from_str(&content).map_err(|e| WeaveError::Json { path, source: e })
    }

    fn save_project_manifest(&self, manifest: &GeminiManifest) -> Result<()> {
        let path = self.project_manifest_path();
        let content =
            // GeminiManifest only contains String/HashMap/Vec fields — cannot fail.
            serde_json::to_string_pretty(manifest).expect("manifest serialization cannot fail");
        util::write_file(&path, &content)
    }

    // ── Shared helpers ────────────────────────────────────────────────────────

    /// Merge pack servers into the JSON file at `path`, recording ownership
    /// in `servers_map`. Used for both user-scope and project-scope.
    fn apply_servers_to_file(
        &self,
        path: &std::path::Path,
        pack: &ResolvedPack,
        servers_map: &mut HashMap<String, String>,
    ) -> Result<()> {
        let mut config: serde_json::Value = if path.exists() {
            let content = util::read_file(path)?;
            serde_json::from_str(&content).map_err(|e| WeaveError::Json {
                path: path.to_path_buf(),
                source: e,
            })?
        } else {
            serde_json::json!({})
        };

        let config_obj = config
            .as_object_mut()
            .ok_or_else(|| WeaveError::ApplyFailed {
                pack: pack.pack.name.clone(),
                cli: "Gemini CLI".into(),
                reason: format!(
                    "{} is not a JSON object — cannot merge MCP servers into it",
                    path.display()
                ),
            })?;

        let servers_entry = config_obj
            .entry("mcpServers")
            .or_insert_with(|| serde_json::json!({}));

        // Guard against a malformed file where `mcpServers` exists but is not an object
        // (e.g. `"mcpServers": []`). Indexing a non-object Value with a string key panics.
        let servers_obj = servers_entry
            .as_object_mut()
            .ok_or_else(|| WeaveError::ApplyFailed {
                pack: pack.pack.name.clone(),
                cli: "Gemini CLI".into(),
                reason: format!(
                    "'mcpServers' in {} is not a JSON object — cannot merge servers into it",
                    path.display()
                ),
            })?;

        for server in &pack.pack.servers {
            if let Some(owner) = servers_map.get(&server.name) {
                if owner != &pack.pack.name {
                    return Err(WeaveError::ApplyFailed {
                        pack: pack.pack.name.clone(),
                        cli: "Gemini CLI".into(),
                        reason: format!(
                            "server '{}' is already registered by pack '{}'; \
                             remove it first with `weave remove {}`",
                            server.name, owner, owner
                        ),
                    });
                }
            } else if servers_obj.contains_key(&server.name) {
                // Key exists in the file but is not tracked by weave — it was added
                // manually by the user. Overwriting it would violate the non-destructive
                // mutation principle.
                return Err(WeaveError::ApplyFailed {
                    pack: pack.pack.name.clone(),
                    cli: "Gemini CLI".into(),
                    reason: format!(
                        "server '{}' already exists in {} and was not installed by weave; \
                         rename or remove it manually before installing this pack",
                        server.name,
                        path.display()
                    ),
                });
            }
            servers_obj.insert(server.name.clone(), build_gemini_server_config(server));
            servers_map.insert(server.name.clone(), pack.pack.name.clone());
        }

        // JSON serialization of a valid serde_json::Value cannot fail.
        let content =
            serde_json::to_string_pretty(&config).expect("JSON serialization cannot fail");
        util::write_file(path, &content)
    }

    /// Remove pack servers from the JSON file at `path`, updating `servers_map`.
    fn remove_servers_from_file(
        &self,
        path: &std::path::Path,
        servers_to_remove: &[String],
        servers_map: &mut HashMap<String, String>,
    ) -> Result<()> {
        if !path.exists() {
            for s in servers_to_remove {
                servers_map.remove(s);
            }
            return Ok(());
        }

        let content = util::read_file(path)?;
        let mut config: serde_json::Value =
            serde_json::from_str(&content).map_err(|e| WeaveError::Json {
                path: path.to_path_buf(),
                source: e,
            })?;

        if let Some(mcp) = config.get_mut("mcpServers").and_then(|v| v.as_object_mut()) {
            for server_name in servers_to_remove {
                mcp.remove(server_name);
                servers_map.remove(server_name);
            }
        }

        // JSON serialization of a valid serde_json::Value cannot fail.
        let output = serde_json::to_string_pretty(&config).expect("JSON serialization cannot fail");
        util::write_file(path, &output)
    }

    /// Deep-merge settings fragment into the JSON file at `path`.
    fn apply_settings_to_file(
        &self,
        path: &std::path::Path,
        pack: &ResolvedPack,
        fragment: &serde_json::Value,
        settings_map: &mut HashMap<String, SettingsRecord>,
    ) -> Result<()> {
        let mut config: serde_json::Value = if path.exists() {
            let content = util::read_file(path)?;
            serde_json::from_str(&content).map_err(|e| WeaveError::Json {
                path: path.to_path_buf(),
                source: e,
            })?
        } else {
            serde_json::json!({})
        };

        // A non-object fragment would cause deep_merge's fallthrough arm to replace
        // the entire settings file. Reject it here with a clear error.
        if !fragment.is_object() {
            return Err(WeaveError::ApplyFailed {
                pack: pack.pack.name.clone(),
                cli: "Gemini CLI".into(),
                reason: "settings/gemini.json must be a JSON object, not a primitive or array"
                    .into(),
            });
        }

        // Strip mcpServers from the fragment — Gemini stores servers and settings in the
        // same file, so a pack could bypass ownership tracking by including mcpServers
        // in its settings fragment. Server writes must go through apply_servers_to_file.
        let mut sanitised = fragment.clone();
        if let Some(obj) = sanitised.as_object_mut() {
            if obj.remove("mcpServers").is_some() {
                log::warn!(
                    "pack '{}' settings/gemini.json contains 'mcpServers' — \
                     this key is managed by weave and has been ignored; \
                     declare servers in pack.toml instead",
                    pack.pack.name
                );
            }
        }
        let fragment = &sanitised;
        let frag_obj = fragment
            .as_object()
            .expect("sanitised is always an object — we just checked is_object() above");

        let mut snap = serde_json::Map::new();
        for key in frag_obj.keys() {
            let before = config.get(key).cloned().unwrap_or(serde_json::Value::Null);
            snap.insert(key.clone(), before);
        }
        let original = serde_json::Value::Object(snap);

        deep_merge(&mut config, fragment);

        // JSON serialization of a valid serde_json::Value cannot fail.
        let output = serde_json::to_string_pretty(&config).expect("JSON serialization cannot fail");
        util::write_file(path, &output)?;

        settings_map.insert(
            pack.pack.name.clone(),
            SettingsRecord {
                applied: fragment.clone(),
                original,
            },
        );

        Ok(())
    }

    /// Remove settings written by a pack from the JSON file at `path`.
    fn remove_settings_from_file(
        &self,
        path: &std::path::Path,
        pack_name: &str,
        settings_map: &mut HashMap<String, SettingsRecord>,
    ) -> Result<()> {
        // Peek at the record without removing it yet. We only remove it from the map
        // after a successful write — otherwise an early return or error would silently
        // drop ownership tracking, breaking future remove/diagnose calls.
        let record = match settings_map.get(pack_name).cloned() {
            Some(r) => r,
            None => return Ok(()),
        };

        if !path.exists() {
            // File is already gone — nothing to restore. Drop the record.
            settings_map.remove(pack_name);
            return Ok(());
        }

        let content = util::read_file(path)?;
        let mut config: serde_json::Value =
            serde_json::from_str(&content).map_err(|e| WeaveError::Json {
                path: path.to_path_buf(),
                source: e,
            })?;

        let frag_obj = match record.applied.as_object() {
            Some(o) => o,
            None => {
                settings_map.remove(pack_name);
                return Ok(());
            }
        };

        let config_obj = match config.as_object_mut() {
            Some(o) => o,
            None => {
                settings_map.remove(pack_name);
                return Ok(());
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
        util::write_file(path, &output)?;
        settings_map.remove(pack_name);
        Ok(())
    }

    // ── User-scope apply/remove ───────────────────────────────────────────────

    /// Merge pack servers into `~/.gemini/settings.json` (user scope).
    fn apply_servers(&self, pack: &ResolvedPack, manifest: &mut GeminiManifest) -> Result<()> {
        if pack.pack.servers.is_empty() {
            return Ok(());
        }
        let path = self.settings_path()?;
        self.apply_servers_to_file(&path, pack, &mut manifest.servers)
    }

    fn remove_servers(&self, pack_name: &str, manifest: &mut GeminiManifest) -> Result<()> {
        let servers_to_remove: Vec<String> = manifest
            .servers
            .iter()
            .filter(|(_, pn)| *pn == pack_name)
            .map(|(sn, _)| sn.clone())
            .collect();

        if servers_to_remove.is_empty() {
            return Ok(());
        }

        let path = self.settings_path()?;
        self.remove_servers_from_file(&path, &servers_to_remove, &mut manifest.servers)
    }

    // ── Project-scope apply/remove ────────────────────────────────────────────

    /// Merge pack servers into `.gemini/settings.json` (project scope).
    fn apply_project_servers(
        &self,
        pack: &ResolvedPack,
        manifest: &mut GeminiManifest,
    ) -> Result<()> {
        if pack.pack.servers.is_empty() {
            return Ok(());
        }
        let path = self.project_settings_path();
        self.apply_servers_to_file(&path, pack, &mut manifest.servers)
    }

    fn remove_project_servers(&self, pack_name: &str, manifest: &mut GeminiManifest) -> Result<()> {
        let servers_to_remove: Vec<String> = manifest
            .servers
            .iter()
            .filter(|(_, pn)| *pn == pack_name)
            .map(|(sn, _)| sn.clone())
            .collect();

        if servers_to_remove.is_empty() {
            return Ok(());
        }

        let path = self.project_settings_path();
        self.remove_servers_from_file(&path, &servers_to_remove, &mut manifest.servers)
    }

    /// Apply prompt content to Gemini's GEMINI.md (using same tagged delimiter pattern).
    fn apply_prompts(&self, pack: &ResolvedPack, manifest: &mut GeminiManifest) -> Result<()> {
        let prompt_content =
            Store::read_pack_file(&pack.pack.name, &pack.pack.version, "prompts/gemini.md")?.or(
                Store::read_pack_file(&pack.pack.name, &pack.pack.version, "prompts/system.md")?,
            );

        let prompt_content = match prompt_content {
            Some(c) if !c.trim().is_empty() => c,
            _ => return Ok(()),
        };

        let gemini_md = self.gemini_dir()?.join("GEMINI.md");
        let mut content = if gemini_md.exists() {
            util::read_file(&gemini_md)?
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

        util::write_file(&gemini_md, &content)?;

        if !manifest.prompt_blocks.contains(&pack.pack.name) {
            manifest.prompt_blocks.push(pack.pack.name.clone());
        }

        Ok(())
    }

    fn remove_prompts(&self, pack_name: &str, manifest: &mut GeminiManifest) -> Result<()> {
        let gemini_md = self.gemini_dir().map(|d| d.join("GEMINI.md"));
        let gemini_md = match gemini_md {
            Ok(p) if p.exists() => p,
            _ => {
                manifest.prompt_blocks.retain(|n| n != pack_name);
                return Ok(());
            }
        };

        let mut content = util::read_file(&gemini_md)?;
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
                util::write_file(&gemini_md, &content)?;
            }
        }

        manifest.prompt_blocks.retain(|n| n != pack_name);
        Ok(())
    }

    /// Deep-merge settings fragment into `~/.gemini/settings.json` (user scope).
    fn apply_settings(&self, pack: &ResolvedPack, manifest: &mut GeminiManifest) -> Result<()> {
        let settings_content =
            Store::read_pack_file(&pack.pack.name, &pack.pack.version, "settings/gemini.json")?;

        let settings_content = match settings_content {
            Some(c) if !c.trim().is_empty() => c,
            _ => return Ok(()),
        };

        let fragment: serde_json::Value =
            serde_json::from_str(&settings_content).map_err(|e| WeaveError::ApplyFailed {
                pack: pack.pack.name.clone(),
                cli: "Gemini CLI".into(),
                reason: format!("invalid settings/gemini.json: {e}"),
            })?;

        let path = self.settings_path()?;
        self.apply_settings_to_file(&path, pack, &fragment, &mut manifest.settings)
    }

    /// Remove settings written by a pack from `~/.gemini/settings.json` (user scope).
    fn remove_settings(&self, pack_name: &str, manifest: &mut GeminiManifest) -> Result<()> {
        let path = self.settings_path()?;
        self.remove_settings_from_file(&path, pack_name, &mut manifest.settings)
    }

    /// Deep-merge settings fragment into `.gemini/settings.json` (project scope).
    fn apply_project_settings(
        &self,
        pack: &ResolvedPack,
        manifest: &mut GeminiManifest,
    ) -> Result<()> {
        let settings_content =
            Store::read_pack_file(&pack.pack.name, &pack.pack.version, "settings/gemini.json")?;

        let settings_content = match settings_content {
            Some(c) if !c.trim().is_empty() => c,
            _ => return Ok(()),
        };

        let fragment: serde_json::Value =
            serde_json::from_str(&settings_content).map_err(|e| WeaveError::ApplyFailed {
                pack: pack.pack.name.clone(),
                cli: "Gemini CLI".into(),
                reason: format!("invalid settings/gemini.json: {e}"),
            })?;

        let path = self.project_settings_path();
        self.apply_settings_to_file(&path, pack, &fragment, &mut manifest.settings)
    }

    /// Remove settings written by a pack from `.gemini/settings.json` (project scope).
    fn remove_project_settings(
        &self,
        pack_name: &str,
        manifest: &mut GeminiManifest,
    ) -> Result<()> {
        let path = self.project_settings_path();
        self.remove_settings_from_file(&path, pack_name, &mut manifest.settings)
    }
}

impl CliAdapter for GeminiCliAdapter {
    fn name(&self) -> &str {
        "Gemini CLI"
    }

    fn is_installed(&self) -> bool {
        self.gemini_dir().map(|d| d.exists()).unwrap_or(false) || which_exists("gemini")
    }

    fn config_dir(&self) -> PathBuf {
        self.gemini_dir()
            .unwrap_or_else(|_| PathBuf::from(".gemini"))
    }

    fn apply(&self, pack: &ResolvedPack) -> Result<()> {
        if !pack.pack.targets.gemini_cli {
            return Ok(());
        }

        util::ensure_dir(&self.gemini_dir()?)?;

        // User-scope
        let mut manifest = self.load_manifest()?;
        self.apply_servers(pack, &mut manifest)?;
        self.apply_prompts(pack, &mut manifest)?;
        self.apply_settings(pack, &mut manifest)?;
        self.save_manifest(&manifest)?;

        // Project-scope — only if a `.gemini/` directory exists in cwd.
        if self.has_project_scope() {
            let mut project_manifest = self.load_project_manifest()?;
            self.apply_project_servers(pack, &mut project_manifest)?;
            self.apply_project_settings(pack, &mut project_manifest)?;
            self.save_project_manifest(&project_manifest)?;
        }

        Ok(())
    }

    fn remove(&self, pack_name: &str) -> Result<()> {
        // User-scope
        let mut manifest = self.load_manifest()?;
        self.remove_servers(pack_name, &mut manifest)?;
        self.remove_prompts(pack_name, &mut manifest)?;
        self.remove_settings(pack_name, &mut manifest)?;
        self.save_manifest(&manifest)?;

        // Project-scope — only if a project manifest exists in cwd.
        let project_manifest_path = self.project_manifest_path();
        if project_manifest_path.exists() {
            let mut project_manifest = self.load_project_manifest()?;
            self.remove_project_servers(pack_name, &mut project_manifest)?;
            self.remove_project_settings(pack_name, &mut project_manifest)?;
            self.save_project_manifest(&project_manifest)?;
        }

        Ok(())
    }

    fn diagnose(&self) -> Result<Vec<DiagnosticIssue>> {
        let mut issues = Vec::new();
        let manifest = self.load_manifest()?;

        // Check tracked servers exist in settings.json
        let settings_path = self.settings_path()?;
        if settings_path.exists() {
            let content = util::read_file(&settings_path)?;
            if let Ok(config) = serde_json::from_str::<serde_json::Value>(&content) {
                let mcp_servers = config.get("mcpServers").and_then(|v| v.as_object());
                for (server_name, pack_name) in &manifest.servers {
                    if mcp_servers.and_then(|m| m.get(server_name)).is_none() {
                        issues.push(DiagnosticIssue {
                            severity: Severity::Warning,
                            message: format!(
                                "server '{server_name}' (from pack '{pack_name}') tracked but missing from settings.json"
                            ),
                            suggestion: Some(format!("run `weave install {pack_name}` to re-apply")),
                        });
                    }
                }
            }
        }

        // Check tracked prompt blocks exist in GEMINI.md
        if !manifest.prompt_blocks.is_empty() {
            let gemini_md = self.gemini_dir()?.join("GEMINI.md");
            let content = if gemini_md.exists() {
                util::read_file(&gemini_md)?
            } else {
                String::new()
            };
            for pack_name in &manifest.prompt_blocks {
                let begin_tag = format!("<!-- packweave:begin:{pack_name} -->");
                if !content.contains(&begin_tag) {
                    issues.push(DiagnosticIssue {
                        severity: Severity::Warning,
                        message: format!(
                            "prompt block for '{pack_name}' is tracked but missing from GEMINI.md"
                        ),
                        suggestion: Some(format!("run `weave install {pack_name}` to re-apply")),
                    });
                }
            }
        }

        // Check tracked settings keys still exist in settings.json
        if !manifest.settings.is_empty() && settings_path.exists() {
            let content = util::read_file(&settings_path)?;
            if let Ok(config) = serde_json::from_str::<serde_json::Value>(&content) {
                for (pack_name, record) in &manifest.settings {
                    if let Some(frag_obj) = record.applied.as_object() {
                        for key in frag_obj.keys() {
                            if config.get(key).is_none() {
                                issues.push(DiagnosticIssue {
                                    severity: Severity::Warning,
                                    message: format!(
                                        "settings key '{key}' (from pack '{pack_name}') is tracked but missing from settings.json"
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

        Ok(issues)
    }
}

/// Build a Gemini CLI MCP server config JSON value.
fn build_gemini_server_config(server: &McpServer) -> serde_json::Value {
    let mut config = serde_json::Map::new();

    config.insert(
        "command".into(),
        serde_json::Value::String(server.command.clone()),
    );

    if !server.args.is_empty() {
        config.insert(
            "args".into(),
            serde_json::Value::Array(
                server
                    .args
                    .iter()
                    .map(|a| serde_json::Value::String(a.clone()))
                    .collect(),
            ),
        );
    }

    if !server.env.is_empty() {
        let mut env_map = serde_json::Map::new();
        for key in server.env.keys() {
            env_map.insert(
                key.clone(),
                serde_json::Value::String(format!("${{{key}}}")),
            );
        }
        config.insert("env".into(), serde_json::Value::Object(env_map));
    }

    serde_json::Value::Object(config)
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
    use crate::core::pack::{Pack, PackSource, PackTargets, Transport};
    use tempfile::TempDir;

    fn test_adapter(dir: &TempDir) -> GeminiCliAdapter {
        GeminiCliAdapter::with_home(dir.path().to_path_buf())
    }

    fn test_pack(name: &str) -> ResolvedPack {
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
                servers: vec![McpServer {
                    name: "test-server".into(),
                    package_type: None,
                    package: None,
                    command: "npx".into(),
                    args: vec!["-y".into(), "test-server".into()],
                    transport: Some(Transport::Stdio),
                    namespace: None,
                    tools: vec![],
                    env: HashMap::new(),
                }],
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
        std::fs::create_dir_all(dir.path().join(".gemini")).unwrap();
    }

    #[test]
    fn apply_and_remove_servers() {
        let dir = TempDir::new().unwrap();
        let adapter = test_adapter(&dir);
        setup_dir(&dir);

        let pack = test_pack("webdev");
        let mut manifest = GeminiManifest::default();

        adapter.apply_servers(&pack, &mut manifest).unwrap();

        let settings = dir.path().join(".gemini").join("settings.json");
        let content: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(&settings).unwrap()).unwrap();
        assert!(content["mcpServers"]["test-server"].is_object());

        adapter.remove_servers("webdev", &mut manifest).unwrap();
        let content: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(&settings).unwrap()).unwrap();
        assert!(content["mcpServers"].as_object().unwrap().is_empty());
    }

    #[test]
    fn apply_servers_rejects_non_object_config() {
        let dir = TempDir::new().unwrap();
        let adapter = test_adapter(&dir);
        setup_dir(&dir);

        std::fs::write(
            dir.path().join(".gemini").join("settings.json"),
            "[1, 2, 3]",
        )
        .unwrap();

        let pack = test_pack("webdev");
        let mut manifest = GeminiManifest::default();
        let result = adapter.apply_servers(&pack, &mut manifest);
        assert!(result.is_err(), "should fail on non-object settings.json");
    }

    #[test]
    fn apply_and_remove_prompts() {
        let dir = TempDir::new().unwrap();
        let adapter = test_adapter(&dir);
        setup_dir(&dir);

        let gemini_md = dir.path().join(".gemini").join("GEMINI.md");
        std::fs::write(&gemini_md, "# My instructions\n").unwrap();

        let pack_name = "webdev";
        let begin_tag = format!("<!-- packweave:begin:{pack_name} -->");
        let end_tag = format!("<!-- packweave:end:{pack_name} -->");

        let mut content = std::fs::read_to_string(&gemini_md).unwrap();
        content.push_str(&begin_tag);
        content.push('\n');
        content.push_str("You are a Gemini power user.");
        content.push('\n');
        content.push_str(&end_tag);
        content.push('\n');
        std::fs::write(&gemini_md, &content).unwrap();

        let mut manifest = GeminiManifest::default();
        manifest.prompt_blocks.push("webdev".into());
        adapter.remove_prompts("webdev", &mut manifest).unwrap();

        let final_content = std::fs::read_to_string(&gemini_md).unwrap();
        assert_eq!(final_content.trim(), "# My instructions");
        assert!(manifest.prompt_blocks.is_empty());
    }

    #[test]
    fn remove_prompts_is_surgical_with_multiple_blocks() {
        let dir = TempDir::new().unwrap();
        let adapter = test_adapter(&dir);
        setup_dir(&dir);

        let gemini_md = dir.path().join(".gemini").join("GEMINI.md");
        std::fs::write(
            &gemini_md,
            "# Docs\n\
             <!-- packweave:begin:pack-a -->\npack-a content\n<!-- packweave:end:pack-a -->\n\
             <!-- packweave:begin:pack-b -->\npack-b content\n<!-- packweave:end:pack-b -->\n",
        )
        .unwrap();

        let mut manifest = GeminiManifest::default();
        manifest.prompt_blocks.push("pack-a".into());
        manifest.prompt_blocks.push("pack-b".into());

        adapter.remove_prompts("pack-a", &mut manifest).unwrap();

        let content = std::fs::read_to_string(&gemini_md).unwrap();
        assert!(!content.contains("pack-a"), "pack-a block should be gone");
        assert!(
            content.contains("pack-b content"),
            "pack-b block should remain"
        );
        assert_eq!(manifest.prompt_blocks, vec!["pack-b"]);
    }

    #[test]
    fn apply_settings_and_remove_unchanged() {
        let dir = TempDir::new().unwrap();
        let adapter = test_adapter(&dir);
        setup_dir(&dir);

        let settings_path = dir.path().join(".gemini").join("settings.json");
        std::fs::write(&settings_path, r#"{"theme": "dark"}"#).unwrap();

        let fragment = serde_json::json!({"model": "gemini-2.0-flash"});
        let config_before: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(&settings_path).unwrap()).unwrap();

        let original = {
            let mut snap = serde_json::Map::new();
            for key in fragment.as_object().unwrap().keys() {
                let before = config_before
                    .get(key)
                    .cloned()
                    .unwrap_or(serde_json::Value::Null);
                snap.insert(key.clone(), before);
            }
            serde_json::Value::Object(snap)
        };

        let mut config = config_before;
        deep_merge(&mut config, &fragment);
        std::fs::write(
            &settings_path,
            serde_json::to_string_pretty(&config).unwrap(),
        )
        .unwrap();

        let mut manifest = GeminiManifest::default();
        manifest.settings.insert(
            "test-pack".into(),
            SettingsRecord {
                applied: fragment,
                original,
            },
        );

        adapter.remove_settings("test-pack", &mut manifest).unwrap();

        let restored: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(&settings_path).unwrap()).unwrap();
        assert_eq!(restored["theme"], "dark");
        assert!(
            restored.get("model").is_none(),
            "model key should be removed"
        );
    }
}
