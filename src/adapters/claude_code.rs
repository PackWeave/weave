use std::collections::HashMap;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::adapters::{CliAdapter, DiagnosticIssue, Severity};
use crate::core::pack::{McpServer, ResolvedPack, Transport};
use crate::core::store::Store;
use crate::error::{Result, WeaveError};
use crate::util;

/// Tracks the settings contribution of a single pack so it can be safely undone.
#[derive(Debug, Clone, Serialize, Deserialize)]
struct SettingsRecord {
    /// The fragment we merged in (pack's settings/claude.json).
    applied: serde_json::Value,
    /// The pre-apply values for each top-level key in `applied`
    /// (Value::Null means the key was absent before installation).
    original: serde_json::Value,
}

/// Sidecar manifest tracking what weave wrote to Claude Code config.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
struct PackweaveManifest {
    #[serde(default)]
    servers: HashMap<String, String>, // server_name -> pack_name
    #[serde(default)]
    commands: HashMap<String, String>, // filename -> pack_name
    #[serde(default)]
    prompt_blocks: Vec<String>, // pack names with prompt content
    #[serde(default)]
    settings: HashMap<String, SettingsRecord>, // pack_name -> settings record
}

pub struct ClaudeCodeAdapter {
    home: Option<PathBuf>,
    /// Current working directory, used to detect project-scope config.
    project_root: PathBuf,
}

impl Default for ClaudeCodeAdapter {
    fn default() -> Self {
        Self::new()
    }
}

impl ClaudeCodeAdapter {
    pub fn new() -> Self {
        Self {
            home: dirs::home_dir(),
            project_root: std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")),
        }
    }

    #[cfg(test)]
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

    /// `~/.claude/`
    fn claude_dir(&self) -> Result<PathBuf> {
        Ok(self.home()?.join(".claude"))
    }

    /// `~/.claude.json` — user-scope MCP servers
    fn claude_json_path(&self) -> Result<PathBuf> {
        Ok(self.home()?.join(".claude.json"))
    }

    /// `~/.claude/.packweave_manifest.json` — user-scope ownership tracking
    fn manifest_path(&self) -> Result<PathBuf> {
        Ok(self.claude_dir()?.join(".packweave_manifest.json"))
    }

    /// `~/.claude/commands/` — slash commands (user-scope only)
    fn commands_dir(&self) -> Result<PathBuf> {
        Ok(self.claude_dir()?.join("commands"))
    }

    /// `~/.claude/CLAUDE.md` — global system prompt (user-scope only)
    fn claude_md_path(&self) -> Result<PathBuf> {
        Ok(self.claude_dir()?.join("CLAUDE.md"))
    }

    /// `~/.claude/settings.json` — user-scope settings
    fn settings_path(&self) -> Result<PathBuf> {
        Ok(self.claude_dir()?.join("settings.json"))
    }

    // ── Project-scope paths ───────────────────────────────────────────────────

    /// `.claude/` in the current project root
    fn project_claude_dir(&self) -> PathBuf {
        self.project_root.join(".claude")
    }

    /// `.mcp.json` — project-scope MCP servers
    fn project_mcp_json_path(&self) -> PathBuf {
        self.project_root.join(".mcp.json")
    }

    /// `.claude/.packweave_manifest.json` — project-scope ownership tracking
    fn project_manifest_path(&self) -> PathBuf {
        self.project_claude_dir().join(".packweave_manifest.json")
    }

    /// `.claude/settings.json` — project-scope settings
    fn project_settings_path(&self) -> PathBuf {
        self.project_claude_dir().join("settings.json")
    }

    /// Returns true if the project has a `.claude/` directory, indicating
    /// that project-scope config should be maintained.
    fn has_project_scope(&self) -> bool {
        self.project_claude_dir().exists()
    }

    // ── Manifest helpers ──────────────────────────────────────────────────────

    fn load_manifest(&self) -> Result<PackweaveManifest> {
        let path = self.manifest_path()?;
        if !path.exists() {
            return Ok(PackweaveManifest::default());
        }
        let content = util::read_file(&path)?;
        serde_json::from_str(&content).map_err(|e| WeaveError::Json { path, source: e })
    }

    fn save_manifest(&self, manifest: &PackweaveManifest) -> Result<()> {
        let path = self.manifest_path()?;
        let content =
            // PackweaveManifest only contains String/HashMap/Vec fields — cannot fail.
            serde_json::to_string_pretty(manifest).expect("manifest serialization cannot fail");
        util::write_file(&path, &content)
    }

    fn load_project_manifest(&self) -> Result<PackweaveManifest> {
        let path = self.project_manifest_path();
        if !path.exists() {
            return Ok(PackweaveManifest::default());
        }
        let content = util::read_file(&path)?;
        serde_json::from_str(&content).map_err(|e| WeaveError::Json { path, source: e })
    }

    fn save_project_manifest(&self, manifest: &PackweaveManifest) -> Result<()> {
        let path = self.project_manifest_path();
        let content =
            // PackweaveManifest only contains String/HashMap/Vec fields — cannot fail.
            serde_json::to_string_pretty(manifest).expect("manifest serialization cannot fail");
        util::write_file(&path, &content)
    }

    // ── Shared helpers (called with either user or project paths) ─────────────

    /// Merge pack servers into a JSON file at `path` (either `~/.claude.json`
    /// or `.mcp.json`), recording ownership in `servers_map`.
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
                cli: "Claude Code".into(),
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
                cli: "Claude Code".into(),
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
                        cli: "Claude Code".into(),
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
                    cli: "Claude Code".into(),
                    reason: format!(
                        "server '{}' already exists in {} and was not installed by weave; \
                         rename or remove it manually before installing this pack",
                        server.name,
                        path.display()
                    ),
                });
            }
            servers_obj.insert(server.name.clone(), build_claude_server_config(server));
            servers_map.insert(server.name.clone(), pack.pack.name.clone());
        }

        // JSON serialization of a valid serde_json::Value cannot fail.
        let content =
            serde_json::to_string_pretty(&config).expect("JSON serialization cannot fail");
        util::write_file(path, &content)
    }

    /// Remove pack servers from a JSON file at `path`, updating `servers_map`.
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

    /// Deep-merge settings fragment into the JSON file at `path`, recording
    /// the snapshot needed for safe removal in `settings_map`.
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
        let frag_obj = fragment
            .as_object()
            .ok_or_else(|| WeaveError::ApplyFailed {
                pack: pack.pack.name.clone(),
                cli: "Claude Code".into(),
                reason: "settings/claude.json must be a JSON object, not a primitive or array"
                    .into(),
            })?;

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

    /// Remove settings written by a pack from the JSON file at `path`, using
    /// the snapshot in `settings_map` to determine what to restore.
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

    /// Merge pack servers into `~/.claude.json` (user scope).
    fn apply_servers(&self, pack: &ResolvedPack, manifest: &mut PackweaveManifest) -> Result<()> {
        if pack.pack.servers.is_empty() {
            return Ok(());
        }
        let path = self.claude_json_path()?;
        self.apply_servers_to_file(&path, pack, &mut manifest.servers)
    }

    /// Remove pack servers from `~/.claude.json` (user scope).
    fn remove_servers(&self, pack_name: &str, manifest: &mut PackweaveManifest) -> Result<()> {
        let servers_to_remove: Vec<String> = manifest
            .servers
            .iter()
            .filter(|(_, pn)| *pn == pack_name)
            .map(|(sn, _)| sn.clone())
            .collect();

        if servers_to_remove.is_empty() {
            return Ok(());
        }

        let path = self.claude_json_path()?;
        self.remove_servers_from_file(&path, &servers_to_remove, &mut manifest.servers)
    }

    // ── Project-scope apply/remove ────────────────────────────────────────────

    /// Merge pack servers into `.mcp.json` (project scope).
    fn apply_project_servers(
        &self,
        pack: &ResolvedPack,
        manifest: &mut PackweaveManifest,
    ) -> Result<()> {
        if pack.pack.servers.is_empty() {
            return Ok(());
        }
        let path = self.project_mcp_json_path();
        self.apply_servers_to_file(&path, pack, &mut manifest.servers)
    }

    /// Remove pack servers from `.mcp.json` (project scope).
    fn remove_project_servers(
        &self,
        pack_name: &str,
        manifest: &mut PackweaveManifest,
    ) -> Result<()> {
        let servers_to_remove: Vec<String> = manifest
            .servers
            .iter()
            .filter(|(_, pn)| *pn == pack_name)
            .map(|(sn, _)| sn.clone())
            .collect();

        if servers_to_remove.is_empty() {
            return Ok(());
        }

        let path = self.project_mcp_json_path();
        self.remove_servers_from_file(&path, &servers_to_remove, &mut manifest.servers)
    }

    /// Copy slash command files with namespaced filenames.
    /// Removes stale commands from a previous version of the same pack before adding the new set.
    fn apply_commands(&self, pack: &ResolvedPack, manifest: &mut PackweaveManifest) -> Result<()> {
        let commands_dir = Store::pack_dir(&pack.pack.name, &pack.pack.version)?.join("commands");

        // Remove any commands previously installed for this pack so stale files
        // from an older version don't linger.
        self.remove_commands(&pack.pack.name, manifest)?;

        if !commands_dir.exists() {
            return Ok(());
        }

        let dest_dir = self.commands_dir()?;
        util::ensure_dir(&dest_dir)?;

        let entries = std::fs::read_dir(&commands_dir)
            .map_err(|e| WeaveError::io("reading pack commands", e))?;

        for entry in entries {
            let entry = entry.map_err(|e| WeaveError::io("reading command entry", e))?;
            let file_name = entry.file_name().to_string_lossy().to_string();

            if !file_name.ends_with(".md") {
                continue;
            }

            let namespaced = format!("{}__{}", pack.pack.name, file_name);
            let dest_path = dest_dir.join(&namespaced);

            std::fs::copy(entry.path(), &dest_path)
                .map_err(|e| WeaveError::io(format!("copying command {file_name}"), e))?;

            manifest.commands.insert(namespaced, pack.pack.name.clone());
        }

        Ok(())
    }

    /// Remove namespaced command files.
    fn remove_commands(&self, pack_name: &str, manifest: &mut PackweaveManifest) -> Result<()> {
        let commands_to_remove: Vec<String> = manifest
            .commands
            .iter()
            .filter(|(_, pn)| *pn == pack_name)
            .map(|(cn, _)| cn.clone())
            .collect();

        let commands_dir = self.commands_dir()?;
        for cmd_file in &commands_to_remove {
            let path = commands_dir.join(cmd_file);
            util::remove_file_if_exists(&path)?;
            manifest.commands.remove(cmd_file);
        }

        Ok(())
    }

    /// Append prompt content between tagged delimiters to CLAUDE.md.
    fn apply_prompts(&self, pack: &ResolvedPack, manifest: &mut PackweaveManifest) -> Result<()> {
        // Try CLI-specific prompt first, fall back to system.md
        let prompt_content =
            Store::read_pack_file(&pack.pack.name, &pack.pack.version, "prompts/claude.md")?.or(
                Store::read_pack_file(&pack.pack.name, &pack.pack.version, "prompts/system.md")?,
            );

        let prompt_content = match prompt_content {
            Some(c) if !c.trim().is_empty() => c,
            _ => return Ok(()),
        };

        let claude_md = self.claude_md_path()?;
        let mut content = if claude_md.exists() {
            util::read_file(&claude_md)?
        } else {
            String::new()
        };

        let begin_tag = format!("<!-- packweave:begin:{} -->", pack.pack.name);
        let end_tag = format!("<!-- packweave:end:{} -->", pack.pack.name);

        // Remove existing block if present (idempotency).
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

        // Append new block
        if !content.is_empty() && !content.ends_with('\n') {
            content.push('\n');
        }
        content.push_str(&begin_tag);
        content.push('\n');
        content.push_str(prompt_content.trim());
        content.push('\n');
        content.push_str(&end_tag);
        content.push('\n');

        util::write_file(&claude_md, &content)?;

        if !manifest.prompt_blocks.contains(&pack.pack.name) {
            manifest.prompt_blocks.push(pack.pack.name.clone());
        }

        Ok(())
    }

    /// Remove tagged prompt block from CLAUDE.md.
    fn remove_prompts(&self, pack_name: &str, manifest: &mut PackweaveManifest) -> Result<()> {
        let claude_md = self.claude_md_path()?;
        if !claude_md.exists() {
            manifest.prompt_blocks.retain(|n| n != pack_name);
            return Ok(());
        }

        let mut content = util::read_file(&claude_md)?;
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
                util::write_file(&claude_md, &content)?;
            }
        }

        manifest.prompt_blocks.retain(|n| n != pack_name);
        Ok(())
    }

    /// Deep-merge settings fragment into `~/.claude/settings.json` (user scope).
    fn apply_settings(&self, pack: &ResolvedPack, manifest: &mut PackweaveManifest) -> Result<()> {
        let settings_content =
            Store::read_pack_file(&pack.pack.name, &pack.pack.version, "settings/claude.json")?;

        let settings_content = match settings_content {
            Some(c) if !c.trim().is_empty() => c,
            _ => return Ok(()),
        };

        let fragment: serde_json::Value =
            serde_json::from_str(&settings_content).map_err(|e| WeaveError::ApplyFailed {
                pack: pack.pack.name.clone(),
                cli: "Claude Code".into(),
                reason: format!("invalid settings/claude.json: {e}"),
            })?;

        let path = self.settings_path()?;
        self.apply_settings_to_file(&path, pack, &fragment, &mut manifest.settings)
    }

    /// Remove settings written by a pack from `~/.claude/settings.json` (user scope).
    fn remove_settings(&self, pack_name: &str, manifest: &mut PackweaveManifest) -> Result<()> {
        let path = self.settings_path()?;
        self.remove_settings_from_file(&path, pack_name, &mut manifest.settings)
    }

    /// Deep-merge settings fragment into `.claude/settings.json` (project scope).
    fn apply_project_settings(
        &self,
        pack: &ResolvedPack,
        manifest: &mut PackweaveManifest,
    ) -> Result<()> {
        let settings_content =
            Store::read_pack_file(&pack.pack.name, &pack.pack.version, "settings/claude.json")?;

        let settings_content = match settings_content {
            Some(c) if !c.trim().is_empty() => c,
            _ => return Ok(()),
        };

        let fragment: serde_json::Value =
            serde_json::from_str(&settings_content).map_err(|e| WeaveError::ApplyFailed {
                pack: pack.pack.name.clone(),
                cli: "Claude Code".into(),
                reason: format!("invalid settings/claude.json: {e}"),
            })?;

        let path = self.project_settings_path();
        self.apply_settings_to_file(&path, pack, &fragment, &mut manifest.settings)
    }

    /// Remove settings written by a pack from `.claude/settings.json` (project scope).
    fn remove_project_settings(
        &self,
        pack_name: &str,
        manifest: &mut PackweaveManifest,
    ) -> Result<()> {
        let path = self.project_settings_path();
        self.remove_settings_from_file(&path, pack_name, &mut manifest.settings)
    }
}

impl CliAdapter for ClaudeCodeAdapter {
    fn name(&self) -> &str {
        "Claude Code"
    }

    fn is_installed(&self) -> bool {
        // Check if ~/.claude/ exists or the `claude` binary is on PATH
        self.claude_dir().map(|d| d.exists()).unwrap_or(false) || which_exists("claude")
    }

    fn config_dir(&self) -> PathBuf {
        self.claude_dir()
            .unwrap_or_else(|_| PathBuf::from(".claude"))
    }

    fn apply(&self, pack: &ResolvedPack) -> Result<()> {
        if !pack.pack.targets.claude_code {
            return Ok(());
        }

        util::ensure_dir(&self.claude_dir()?)?;

        // User-scope
        let mut manifest = self.load_manifest()?;
        self.apply_servers(pack, &mut manifest)?;
        self.apply_commands(pack, &mut manifest)?;
        self.apply_prompts(pack, &mut manifest)?;
        self.apply_settings(pack, &mut manifest)?;
        self.save_manifest(&manifest)?;

        // Project-scope — only if a `.claude/` directory exists in cwd,
        // indicating the user is working in a Claude Code project.
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
        self.remove_commands(pack_name, &mut manifest)?;
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

        // Check tracked servers exist in claude.json
        let claude_json = self.claude_json_path()?;
        if claude_json.exists() {
            let content = util::read_file(&claude_json)?;
            if let Ok(config) = serde_json::from_str::<serde_json::Value>(&content) {
                let mcp_servers = config.get("mcpServers").and_then(|v| v.as_object());
                for (server_name, pack_name) in &manifest.servers {
                    if mcp_servers.and_then(|m| m.get(server_name)).is_none() {
                        issues.push(DiagnosticIssue {
                            severity: Severity::Warning,
                            message: format!(
                                "server '{server_name}' (from pack '{pack_name}') is tracked but missing from claude.json"
                            ),
                            suggestion: Some(format!("run `weave install {pack_name}` to re-apply")),
                        });
                    }
                }
            }
        }

        // Check tracked command files exist on disk
        let commands_dir = self.commands_dir()?;
        for (filename, pack_name) in &manifest.commands {
            if !commands_dir.join(filename).exists() {
                issues.push(DiagnosticIssue {
                    severity: Severity::Warning,
                    message: format!(
                        "command file '{filename}' (from pack '{pack_name}') is tracked but missing"
                    ),
                    suggestion: Some(format!("run `weave install {pack_name}` to re-apply")),
                });
            }
        }

        // Check tracked prompt blocks exist in CLAUDE.md
        if !manifest.prompt_blocks.is_empty() {
            let claude_md = self.claude_md_path()?;
            let content = if claude_md.exists() {
                util::read_file(&claude_md)?
            } else {
                String::new()
            };
            for pack_name in &manifest.prompt_blocks {
                let begin_tag = format!("<!-- packweave:begin:{pack_name} -->");
                if !content.contains(&begin_tag) {
                    issues.push(DiagnosticIssue {
                        severity: Severity::Warning,
                        message: format!(
                            "prompt block for '{pack_name}' is tracked but missing from CLAUDE.md"
                        ),
                        suggestion: Some(format!("run `weave install {pack_name}` to re-apply")),
                    });
                }
            }
        }

        // Check tracked settings keys still exist in settings.json
        if !manifest.settings.is_empty() {
            let settings_path = self.settings_path()?;
            if settings_path.exists() {
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
        }

        Ok(issues)
    }
}

/// Build a Claude Code MCP server config JSON value.
fn build_claude_server_config(server: &McpServer) -> serde_json::Value {
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

    if let Some(Transport::Http) = server.transport {
        config.insert("type".into(), serde_json::Value::String("http".into()));
    }

    if !server.env.is_empty() {
        let mut env_map = serde_json::Map::new();
        for key in server.env.keys() {
            // Write "${KEY}" references so the config clearly signals which env
            // vars the user must populate. An empty string would silently
            // override any value the user already has in their environment.
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
    use crate::core::pack::{Pack, PackSource, PackTargets};
    use tempfile::TempDir;

    fn test_adapter(dir: &TempDir) -> ClaudeCodeAdapter {
        let no_project = dir.path().join("no-project");
        std::fs::create_dir_all(&no_project).unwrap();
        ClaudeCodeAdapter::with_home_and_project(dir.path().to_path_buf(), no_project)
    }

    fn test_pack(name: &str) -> ResolvedPack {
        ResolvedPack {
            pack: Pack {
                name: name.to_string(),
                version: semver::Version::new(1, 0, 0),
                description: "Test pack".into(),
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
        std::fs::create_dir_all(dir.path().join(".claude")).unwrap();
    }

    #[test]
    fn apply_and_remove_servers() {
        let dir = TempDir::new().unwrap();
        let adapter = test_adapter(&dir);
        setup_dir(&dir);

        let pack = test_pack("webdev");
        let mut manifest = PackweaveManifest::default();

        adapter.apply_servers(&pack, &mut manifest).unwrap();

        // Verify server was written
        let claude_json = dir.path().join(".claude.json");
        let content: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(&claude_json).unwrap()).unwrap();
        assert!(content["mcpServers"]["test-server"].is_object());
        assert_eq!(manifest.servers["test-server"], "webdev");

        // Remove
        adapter.remove_servers("webdev", &mut manifest).unwrap();
        let content: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(&claude_json).unwrap()).unwrap();
        assert!(content["mcpServers"].as_object().unwrap().is_empty());
        assert!(manifest.servers.is_empty());
    }

    #[test]
    fn apply_servers_rejects_non_object_config() {
        let dir = TempDir::new().unwrap();
        let adapter = test_adapter(&dir);
        setup_dir(&dir);

        // Write a non-object JSON file
        std::fs::write(dir.path().join(".claude.json"), "[1, 2, 3]").unwrap();

        let pack = test_pack("webdev");
        let mut manifest = PackweaveManifest::default();
        let result = adapter.apply_servers(&pack, &mut manifest);
        assert!(result.is_err(), "should fail on non-object claude.json");
    }

    #[test]
    fn apply_and_remove_prompts() {
        let dir = TempDir::new().unwrap();
        let adapter = test_adapter(&dir);
        setup_dir(&dir);

        let claude_md = dir.path().join(".claude").join("CLAUDE.md");
        std::fs::write(&claude_md, "# My instructions\n").unwrap();

        let pack_name = "webdev";
        let begin_tag = format!("<!-- packweave:begin:{pack_name} -->");
        let end_tag = format!("<!-- packweave:end:{pack_name} -->");

        // Simulate what apply_prompts writes
        let mut content = std::fs::read_to_string(&claude_md).unwrap();
        content.push_str(&begin_tag);
        content.push('\n');
        content.push_str("You are a web developer.");
        content.push('\n');
        content.push_str(&end_tag);
        content.push('\n');
        std::fs::write(&claude_md, &content).unwrap();

        let mut manifest = PackweaveManifest::default();
        manifest.prompt_blocks.push("webdev".into());
        adapter.remove_prompts("webdev", &mut manifest).unwrap();

        let final_content = std::fs::read_to_string(&claude_md).unwrap();
        assert_eq!(final_content.trim(), "# My instructions");
        assert!(manifest.prompt_blocks.is_empty());
    }

    #[test]
    fn remove_prompts_is_surgical_with_multiple_blocks() {
        let dir = TempDir::new().unwrap();
        let adapter = test_adapter(&dir);
        setup_dir(&dir);

        let claude_md = dir.path().join(".claude").join("CLAUDE.md");
        std::fs::write(
            &claude_md,
            "# Docs\n\
             <!-- packweave:begin:pack-a -->\npack-a content\n<!-- packweave:end:pack-a -->\n\
             <!-- packweave:begin:pack-b -->\npack-b content\n<!-- packweave:end:pack-b -->\n",
        )
        .unwrap();

        let mut manifest = PackweaveManifest::default();
        manifest.prompt_blocks.push("pack-a".into());
        manifest.prompt_blocks.push("pack-b".into());

        // Remove only pack-a
        adapter.remove_prompts("pack-a", &mut manifest).unwrap();

        let content = std::fs::read_to_string(&claude_md).unwrap();
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

        let settings_path = dir.path().join(".claude").join("settings.json");
        // Pre-existing user settings
        std::fs::write(&settings_path, r#"{"theme": "dark"}"#).unwrap();

        // Manually build the SettingsRecord as apply_settings would
        let fragment = serde_json::json!({"permissions": {"allow": ["bash"]}});
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

        let mut manifest = PackweaveManifest::default();
        manifest.settings.insert(
            "test-pack".into(),
            SettingsRecord {
                applied: fragment,
                original,
            },
        );

        // Verify merge happened
        let after: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(&settings_path).unwrap()).unwrap();
        assert_eq!(after["theme"], "dark");
        assert_eq!(after["permissions"]["allow"][0], "bash");

        // Remove settings
        adapter.remove_settings("test-pack", &mut manifest).unwrap();

        let restored: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(&settings_path).unwrap()).unwrap();
        assert_eq!(restored["theme"], "dark");
        assert!(
            restored.get("permissions").is_none(),
            "permissions key should be removed since user didn't modify it"
        );
        assert!(manifest.settings.is_empty());
    }

    #[test]
    fn apply_settings_preserves_user_modified_key() {
        let dir = TempDir::new().unwrap();
        let adapter = test_adapter(&dir);
        setup_dir(&dir);

        let settings_path = dir.path().join(".claude").join("settings.json");
        let fragment = serde_json::json!({"permissions": {"allow": ["bash"]}});

        // Simulate current state: user modified the key after weave applied it
        let user_modified = serde_json::json!({"permissions": {"allow": ["bash", "curl"]}});
        std::fs::write(
            &settings_path,
            serde_json::to_string_pretty(&user_modified).unwrap(),
        )
        .unwrap();

        let mut manifest = PackweaveManifest::default();
        manifest.settings.insert(
            "test-pack".into(),
            SettingsRecord {
                applied: fragment,
                original: serde_json::json!({"permissions": null}),
            },
        );

        // remove_settings should leave the key untouched because current != expected
        adapter.remove_settings("test-pack", &mut manifest).unwrap();

        let after: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(&settings_path).unwrap()).unwrap();
        assert_eq!(after["permissions"]["allow"][1], "curl");
    }

    #[test]
    fn deep_merge_objects() {
        let mut target = serde_json::json!({"a": 1, "b": {"c": 2}});
        let source = serde_json::json!({"b": {"d": 3}, "e": 4});
        deep_merge(&mut target, &source);

        assert_eq!(target["a"], 1);
        assert_eq!(target["b"]["c"], 2);
        assert_eq!(target["b"]["d"], 3);
        assert_eq!(target["e"], 4);
    }

    #[test]
    fn apply_servers_writes_env_vars_as_references() {
        let dir = TempDir::new().unwrap();
        let adapter = test_adapter(&dir);
        setup_dir(&dir);

        let mut env = HashMap::new();
        env.insert(
            "MY_API_KEY".to_string(),
            crate::core::pack::EnvVar {
                required: true,
                secret: true,
                description: Some("API key for the service".into()),
            },
        );
        env.insert(
            "ANOTHER_VAR".to_string(),
            crate::core::pack::EnvVar {
                required: false,
                secret: false,
                description: None,
            },
        );

        let pack = ResolvedPack {
            pack: crate::core::pack::Pack {
                name: "env-pack".into(),
                version: semver::Version::new(1, 0, 0),
                description: "Pack with env vars".into(),
                authors: vec![],
                license: None,
                repository: None,
                keywords: vec![],
                min_tool_version: None,
                servers: vec![McpServer {
                    name: "env-server".into(),
                    package_type: None,
                    package: None,
                    command: "npx".into(),
                    args: vec!["-y".into(), "env-server".into()],
                    transport: None,
                    namespace: None,
                    tools: vec![],
                    env,
                }],
                dependencies: HashMap::new(),
                extensions: Default::default(),
                targets: crate::core::pack::PackTargets::default(),
            },
            source: crate::core::pack::PackSource::Registry {
                registry_url: "https://example.com".into(),
            },
        };

        let mut manifest = PackweaveManifest::default();
        adapter.apply_servers(&pack, &mut manifest).unwrap();

        let claude_json = dir.path().join(".claude.json");
        let content: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(&claude_json).unwrap()).unwrap();

        let env_obj = &content["mcpServers"]["env-server"]["env"];
        assert!(env_obj.is_object(), "env key should be present");
        assert_eq!(
            env_obj["MY_API_KEY"], "${MY_API_KEY}",
            "env var should be written as a reference"
        );
        assert_eq!(
            env_obj["ANOTHER_VAR"], "${ANOTHER_VAR}",
            "env var should be written as a reference"
        );
    }

    #[test]
    fn apply_servers_omits_env_key_when_server_has_no_env_vars() {
        let dir = TempDir::new().unwrap();
        let adapter = test_adapter(&dir);
        setup_dir(&dir);

        let pack = test_pack("no-env-pack");
        let mut manifest = PackweaveManifest::default();
        adapter.apply_servers(&pack, &mut manifest).unwrap();

        let claude_json = dir.path().join(".claude.json");
        let content: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(&claude_json).unwrap()).unwrap();

        // test_pack builds a server with env: HashMap::new() — no env key should appear
        assert!(
            content["mcpServers"]["test-server"].is_object(),
            "test-server must have been written to mcpServers"
        );
        assert!(
            content["mcpServers"]["test-server"].get("env").is_none(),
            "env key should not be present when server has no env vars"
        );
    }

    #[test]
    fn idempotent_prompt_apply() {
        let content = "# Docs\n<!-- packweave:begin:test -->\nHello\n<!-- packweave:end:test -->\n";
        let begin_tag = "<!-- packweave:begin:test -->";
        let end_tag = "<!-- packweave:end:test -->";

        let mut result = content.to_string();

        if let Some(start) = result.find(begin_tag) {
            if let Some(end_offset) = result[start..].find(end_tag) {
                let end_pos = start + end_offset;
                let end = end_pos + end_tag.len();
                let end = if result[end..].starts_with('\n') {
                    end + 1
                } else {
                    end
                };
                result.replace_range(start..end, "");
            }
        }

        if !result.ends_with('\n') {
            result.push('\n');
        }
        result.push_str(begin_tag);
        result.push('\n');
        result.push_str("Hello");
        result.push('\n');
        result.push_str(end_tag);
        result.push('\n');

        assert_eq!(
            result.matches(begin_tag).count(),
            1,
            "should have exactly one begin tag"
        );
    }
}
