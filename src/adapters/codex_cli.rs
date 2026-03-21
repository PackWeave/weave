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
    /// The TOML fragment we merged in (from settings/codex.toml or settings/codex.json).
    /// Stored as JSON for uniform serialization inside the JSON sidecar.
    applied: serde_json::Value,
    /// The pre-apply values for each top-level key in `applied`
    /// (Value::Null means the key was absent before installation).
    original: serde_json::Value,
}

/// Sidecar manifest tracking what weave wrote to Codex CLI config.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
struct CodexManifest {
    #[serde(default)]
    servers: HashMap<String, String>, // server_name -> pack_name
    #[serde(default)]
    prompt_blocks: Vec<String>, // pack names with prompt content
    #[serde(default)]
    settings: HashMap<String, SettingsRecord>, // pack_name -> settings record
    #[serde(default)]
    skills: HashMap<String, String>, // filename -> pack_name
}

pub struct CodexAdapter {
    home: Option<PathBuf>,
    /// Current working directory, used to detect project-scope config.
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

    /// `~/.codex/`
    fn codex_dir(&self) -> Result<PathBuf> {
        Ok(self.home()?.join(".codex"))
    }

    /// `~/.codex/config.toml` — user-scope MCP servers + settings
    fn config_toml_path(&self) -> Result<PathBuf> {
        Ok(self.codex_dir()?.join("config.toml"))
    }

    /// `~/.codex/AGENTS.md` — user-scope prompts
    fn agents_md_path(&self) -> Result<PathBuf> {
        Ok(self.codex_dir()?.join("AGENTS.md"))
    }

    /// `~/.codex/.packweave_manifest.json` — user-scope ownership tracking
    fn manifest_path(&self) -> Result<PathBuf> {
        Ok(self.codex_dir()?.join(".packweave_manifest.json"))
    }

    /// `~/.codex/skills/` — user-scope skills directory
    fn skills_dir(&self) -> Result<PathBuf> {
        Ok(self.codex_dir()?.join("skills"))
    }

    // ── Project-scope paths ───────────────────────────────────────────────────

    /// `.codex/` in the current project root
    fn project_codex_dir(&self) -> PathBuf {
        self.project_root.join(".codex")
    }

    /// `.codex/config.toml` — project-scope MCP servers + settings
    fn project_config_toml_path(&self) -> PathBuf {
        self.project_codex_dir().join("config.toml")
    }

    /// `.codex/.packweave_manifest.json` — project-scope ownership tracking
    fn project_manifest_path(&self) -> PathBuf {
        self.project_codex_dir().join(".packweave_manifest.json")
    }

    /// Returns true if the project has a `.codex/` directory, indicating
    /// that project-scope config should be maintained.
    fn has_project_scope(&self) -> bool {
        self.project_codex_dir().exists()
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
        let content =
            // CodexManifest only contains String/HashMap/Vec fields — cannot fail.
            serde_json::to_string_pretty(manifest).expect("manifest serialization cannot fail");
        util::write_file(&path, &content)
    }

    fn load_project_manifest(&self) -> Result<CodexManifest> {
        let path = self.project_manifest_path();
        if !path.exists() {
            return Ok(CodexManifest::default());
        }
        let content = util::read_file(&path)?;
        serde_json::from_str(&content).map_err(|e| WeaveError::Json { path, source: e })
    }

    fn save_project_manifest(&self, manifest: &CodexManifest) -> Result<()> {
        let path = self.project_manifest_path();
        let content =
            // CodexManifest only contains String/HashMap/Vec fields — cannot fail.
            serde_json::to_string_pretty(manifest).expect("manifest serialization cannot fail");
        util::write_file(&path, &content)
    }

    // ── TOML config helpers ───────────────────────────────────────────────────

    /// Read a `config.toml`, returning an empty table if the file doesn't exist.
    fn read_config_toml(path: &std::path::Path) -> Result<toml::Value> {
        if !path.exists() {
            return Ok(toml::Value::Table(Default::default()));
        }
        let content = util::read_file(path)?;
        toml::from_str::<toml::Value>(&content).map_err(|e| WeaveError::Toml {
            path: path.to_path_buf(),
            source: Box::new(e),
        })
    }

    /// Write a TOML value to a file.
    fn write_config_toml(path: &std::path::Path, config: &toml::Value) -> Result<()> {
        // toml::Value only holds valid TOML data constructed by weave — serialization cannot fail.
        let content = toml::to_string(config).expect("toml::Value serialization cannot fail");
        util::write_file(path, &content)
    }

    // ── Shared helpers ────────────────────────────────────────────────────────

    /// Merge pack servers into the TOML file at `path`, recording ownership
    /// in `servers_map`. Used for both user-scope and project-scope.
    fn apply_servers_to_file(
        &self,
        path: &std::path::Path,
        pack: &ResolvedPack,
        servers_map: &mut HashMap<String, String>,
    ) -> Result<()> {
        let mut config = Self::read_config_toml(path)?;

        let config_table = config
            .as_table_mut()
            .ok_or_else(|| WeaveError::ApplyFailed {
                pack: pack.pack.name.clone(),
                cli: "Codex CLI".into(),
                reason: format!(
                    "{} is not a TOML table — cannot merge MCP servers into it",
                    path.display()
                ),
            })?;

        let mcp_entry = config_table
            .entry("mcp_servers")
            .or_insert_with(|| toml::Value::Table(Default::default()));

        let mcp_table = mcp_entry
            .as_table_mut()
            .ok_or_else(|| WeaveError::ApplyFailed {
                pack: pack.pack.name.clone(),
                cli: "Codex CLI".into(),
                reason: format!(
                    "'mcp_servers' in {} is not a TOML table — cannot merge servers into it",
                    path.display()
                ),
            })?;

        for server in &pack.pack.servers {
            if let Some(owner) = servers_map.get(&server.name) {
                if owner != &pack.pack.name {
                    return Err(WeaveError::ApplyFailed {
                        pack: pack.pack.name.clone(),
                        cli: "Codex CLI".into(),
                        reason: format!(
                            "server '{}' is already registered by pack '{}'; \
                             remove it first with `weave remove {}`",
                            server.name, owner, owner
                        ),
                    });
                }
            } else if mcp_table.contains_key(&server.name) {
                // Key exists in the file but is not tracked by weave — it was added
                // manually by the user. Overwriting it would violate the non-destructive
                // mutation principle.
                return Err(WeaveError::ApplyFailed {
                    pack: pack.pack.name.clone(),
                    cli: "Codex CLI".into(),
                    reason: format!(
                        "server '{}' already exists in {} and was not installed by weave; \
                         rename or remove it manually before installing this pack",
                        server.name,
                        path.display()
                    ),
                });
            }
            mcp_table.insert(
                server.name.clone(),
                build_codex_server_config(server).map_err(|reason| WeaveError::ApplyFailed {
                    pack: pack.pack.name.clone(),
                    cli: "Codex CLI".into(),
                    reason,
                })?,
            );
            servers_map.insert(server.name.clone(), pack.pack.name.clone());
        }

        Self::write_config_toml(path, &config)
    }

    /// Remove pack servers from the TOML file at `path`, updating `servers_map`.
    fn remove_servers_from_file(
        &self,
        path: &std::path::Path,
        pack_name: &str,
        servers_to_remove: &[String],
        servers_map: &mut HashMap<String, String>,
    ) -> Result<()> {
        if !path.exists() {
            // File already gone — clean up manifest so we don't leave orphan entries.
            for s in servers_to_remove {
                servers_map.remove(s);
            }
            return Ok(());
        }

        let mut config = Self::read_config_toml(path)?;
        let config_table = config
            .as_table_mut()
            .ok_or_else(|| WeaveError::RemoveFailed {
                pack: pack_name.to_owned(),
                cli: "Codex CLI".into(),
                reason: format!(
                    "{} is not a TOML table — cannot remove MCP servers from it",
                    path.display()
                ),
            })?;

        match config_table.get_mut("mcp_servers") {
            Some(v) => {
                let mcp = v.as_table_mut().ok_or_else(|| WeaveError::RemoveFailed {
                    pack: pack_name.to_owned(),
                    cli: "Codex CLI".into(),
                    reason: format!(
                        "{}: `mcp_servers` exists but is not a TOML table",
                        path.display()
                    ),
                })?;
                for server_name in servers_to_remove {
                    mcp.remove(server_name);
                    servers_map.remove(server_name);
                }
            }
            None => {
                // mcp_servers key absent — nothing to remove from the file; clean up manifest.
                for s in servers_to_remove {
                    servers_map.remove(s);
                }
                return Ok(());
            }
        }

        Self::write_config_toml(path, &config)
    }

    /// Merge settings fragment (top-level keys only) into the TOML file at `path`.
    fn apply_settings_to_file(
        &self,
        path: &std::path::Path,
        pack: &ResolvedPack,
        fragment: &toml::Value,
        settings_map: &mut HashMap<String, SettingsRecord>,
    ) -> Result<()> {
        let mut config = Self::read_config_toml(path)?;

        let frag_table = fragment.as_table().ok_or_else(|| WeaveError::ApplyFailed {
            pack: pack.pack.name.clone(),
            cli: "Codex CLI".into(),
            reason: "settings/codex.toml must be a TOML table, not a primitive or array".into(),
        })?;

        // Strip mcp_servers from the fragment — server writes must go through
        // apply_servers_to_file.
        let sanitised_pairs: Vec<(String, toml::Value)> = frag_table
            .iter()
            .filter(|(k, _)| {
                if k.as_str() == "mcp_servers" {
                    log::warn!(
                        "pack '{}' settings/codex.toml contains 'mcp_servers' — \
                         this key is managed by weave and has been ignored; \
                         declare servers in pack.toml instead",
                        pack.pack.name
                    );
                    false
                } else {
                    true
                }
            })
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect();

        if sanitised_pairs.is_empty() {
            return Ok(());
        }

        let config_table = config
            .as_table_mut()
            .ok_or_else(|| WeaveError::ApplyFailed {
                pack: pack.pack.name.clone(),
                cli: "Codex CLI".into(),
                reason: format!(
                    "{} is not a TOML table — cannot merge settings into it",
                    path.display()
                ),
            })?;

        // Snapshot original values for later removal. Keys absent in the current config
        // are stored as JSON null so we can detect "was not present" during removal.
        let mut snap_map = serde_json::Map::new();
        for (key, _) in &sanitised_pairs {
            let before = config_table
                .get(key)
                .map(toml_value_to_json)
                .unwrap_or(serde_json::Value::Null);
            snap_map.insert(key.clone(), before);
        }
        let original_json = serde_json::Value::Object(snap_map);

        // Merge the fragment (top-level keys only — no deep merge for TOML settings).
        for (key, val) in &sanitised_pairs {
            config_table.insert(key.clone(), val.clone());
        }

        Self::write_config_toml(path, &config)?;

        // Convert applied fragment to JSON for storage in the JSON sidecar.
        let applied_json = toml_table_to_json(&sanitised_pairs.iter().cloned().collect());

        settings_map.insert(
            pack.pack.name.clone(),
            SettingsRecord {
                applied: applied_json,
                original: original_json,
            },
        );

        Ok(())
    }

    /// Remove settings written by a pack from the TOML file at `path`.
    fn remove_settings_from_file(
        &self,
        path: &std::path::Path,
        pack_name: &str,
        settings_map: &mut HashMap<String, SettingsRecord>,
    ) -> Result<()> {
        let record = match settings_map.get(pack_name).cloned() {
            Some(r) => r,
            None => return Ok(()),
        };

        if !path.exists() {
            settings_map.remove(pack_name);
            return Ok(());
        }

        let mut config = Self::read_config_toml(path)?;

        let frag_obj = record
            .applied
            .as_object()
            .ok_or_else(|| WeaveError::RemoveFailed {
                pack: pack_name.to_owned(),
                cli: "Codex CLI".into(),
                reason: "manifest settings record 'applied' is not a JSON object — \
                     the manifest may be corrupt; edit ~/.codex/.packweave_manifest.json to fix it"
                    .into(),
            })?;
        let frag_obj = frag_obj.clone();

        let config_table = config
            .as_table_mut()
            .ok_or_else(|| WeaveError::RemoveFailed {
                pack: pack_name.to_owned(),
                cli: "Codex CLI".into(),
                reason: format!(
                    "{} is not a TOML table — cannot remove settings from it",
                    path.display()
                ),
            })?;

        let orig_obj = record.original.as_object().cloned().unwrap_or_default();

        for (key, applied_val) in &frag_obj {
            // Check whether the current value still matches what we wrote. If the user
            // modified it after install, leave it alone and warn — non-destructive mutations.
            let current = config_table.get(key);
            let current_json = current.map(toml_value_to_json);
            if current_json.as_ref() != Some(applied_val) {
                log::warn!(
                    "settings key '{key}' (from pack '{pack_name}') was modified after install; \
                     leaving it in place — remove manually if desired"
                );
                continue;
            }

            let pre_json = orig_obj
                .get(key)
                .cloned()
                .unwrap_or(serde_json::Value::Null);
            if pre_json.is_null() {
                config_table.remove(key);
            } else if let Some(toml_val) = json_value_to_toml(&pre_json) {
                config_table.insert(key.clone(), toml_val);
            } else {
                log::warn!(
                    "settings key '{key}' (from pack '{pack_name}') original value could not be \
                     restored; leaving it in place — remove manually if desired"
                );
            }
        }

        Self::write_config_toml(path, &config)?;
        settings_map.remove(pack_name);
        Ok(())
    }

    // ── User-scope apply/remove ───────────────────────────────────────────────

    /// Merge pack servers into `~/.codex/config.toml` (user scope).
    fn apply_servers(&self, pack: &ResolvedPack, manifest: &mut CodexManifest) -> Result<()> {
        if pack.pack.servers.is_empty() {
            return Ok(());
        }
        let path = self.config_toml_path()?;
        self.apply_servers_to_file(&path, pack, &mut manifest.servers)
    }

    fn remove_servers(&self, pack_name: &str, manifest: &mut CodexManifest) -> Result<()> {
        let servers_to_remove: Vec<String> = manifest
            .servers
            .iter()
            .filter(|(_, pn)| *pn == pack_name)
            .map(|(sn, _)| sn.clone())
            .collect();

        if servers_to_remove.is_empty() {
            return Ok(());
        }

        let path = self.config_toml_path()?;
        self.remove_servers_from_file(&path, pack_name, &servers_to_remove, &mut manifest.servers)
    }

    // ── Project-scope apply/remove ────────────────────────────────────────────

    /// Merge pack servers into `.codex/config.toml` (project scope).
    fn apply_project_servers(
        &self,
        pack: &ResolvedPack,
        manifest: &mut CodexManifest,
    ) -> Result<()> {
        if pack.pack.servers.is_empty() {
            return Ok(());
        }
        let path = self.project_config_toml_path();
        self.apply_servers_to_file(&path, pack, &mut manifest.servers)
    }

    fn remove_project_servers(&self, pack_name: &str, manifest: &mut CodexManifest) -> Result<()> {
        let servers_to_remove: Vec<String> = manifest
            .servers
            .iter()
            .filter(|(_, pn)| *pn == pack_name)
            .map(|(sn, _)| sn.clone())
            .collect();

        if servers_to_remove.is_empty() {
            return Ok(());
        }

        let path = self.project_config_toml_path();
        self.remove_servers_from_file(&path, pack_name, &servers_to_remove, &mut manifest.servers)
    }

    // ── Skills (like Claude Code commands) ────────────────────────────────────

    /// Copy skill files with namespaced filenames to `~/.codex/skills/`.
    /// Removes stale skills from a previous version of the same pack before adding the new set.
    fn apply_skills(&self, pack: &ResolvedPack, manifest: &mut CodexManifest) -> Result<()> {
        let skills_dir = Store::pack_dir(&pack.pack.name, &pack.pack.version)?.join("skills");

        // Remove stale skills from an older version of this pack.
        self.remove_skills(&pack.pack.name, manifest)?;

        if !skills_dir.exists() {
            return Ok(());
        }

        let dest_dir = self.skills_dir()?;
        util::ensure_dir(&dest_dir)?;

        let entries =
            std::fs::read_dir(&skills_dir).map_err(|e| WeaveError::io("reading pack skills", e))?;

        for entry in entries {
            let entry = entry.map_err(|e| WeaveError::io("reading skill entry", e))?;
            let file_name = entry.file_name().to_string_lossy().to_string();

            if !file_name.ends_with(".md") {
                continue;
            }

            let namespaced = format!("{}__{}", pack.pack.name, file_name);
            let dest_path = dest_dir.join(&namespaced);

            std::fs::copy(entry.path(), &dest_path)
                .map_err(|e| WeaveError::io(format!("copying skill {file_name}"), e))?;

            // Record in manifest immediately so a failure on a later entry doesn't
            // leave on-disk files that are invisible to remove()/diagnose().
            manifest.skills.insert(namespaced, pack.pack.name.clone());
            self.save_manifest(manifest)?;
        }

        Ok(())
    }

    /// Remove namespaced skill files from `~/.codex/skills/`.
    fn remove_skills(&self, pack_name: &str, manifest: &mut CodexManifest) -> Result<()> {
        let skills_to_remove: Vec<String> = manifest
            .skills
            .iter()
            .filter(|(_, pn)| *pn == pack_name)
            .map(|(fn_, _)| fn_.clone())
            .collect();

        let skills_dir = self.skills_dir()?;
        for skill_file in &skills_to_remove {
            let path = skills_dir.join(skill_file);
            util::remove_file_if_exists(&path)?;
            manifest.skills.remove(skill_file);
        }

        Ok(())
    }

    // ── Prompts (AGENTS.md) ───────────────────────────────────────────────────

    /// Append prompt content between tagged delimiters to `~/.codex/AGENTS.md`.
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
        let agents_md = self.agents_md_path()?;
        if !agents_md.exists() {
            manifest.prompt_blocks.retain(|n| n != pack_name);
            return Ok(());
        }

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

    // ── Settings (config.toml top-level keys) ─────────────────────────────────

    /// Merge settings fragment into `~/.codex/config.toml` (user scope).
    fn apply_settings(&self, pack: &ResolvedPack, manifest: &mut CodexManifest) -> Result<()> {
        // Try TOML first, fall back to JSON for compatibility.
        let (fragment, format) = load_settings_fragment(&pack.pack.name, &pack.pack.version)?;

        let fragment = match fragment {
            Some(f) => f,
            None => return Ok(()),
        };

        let path = self.config_toml_path()?;
        let toml_fragment = json_to_toml_value(&fragment, &pack.pack.name, format)?;
        self.apply_settings_to_file(&path, pack, &toml_fragment, &mut manifest.settings)
    }

    /// Remove settings written by a pack from `~/.codex/config.toml` (user scope).
    fn remove_settings(&self, pack_name: &str, manifest: &mut CodexManifest) -> Result<()> {
        let path = self.config_toml_path()?;
        self.remove_settings_from_file(&path, pack_name, &mut manifest.settings)
    }

    /// Merge settings fragment into `.codex/config.toml` (project scope).
    fn apply_project_settings(
        &self,
        pack: &ResolvedPack,
        manifest: &mut CodexManifest,
    ) -> Result<()> {
        let (fragment, format) = load_settings_fragment(&pack.pack.name, &pack.pack.version)?;

        let fragment = match fragment {
            Some(f) => f,
            None => return Ok(()),
        };

        let path = self.project_config_toml_path();
        let toml_fragment = json_to_toml_value(&fragment, &pack.pack.name, format)?;
        self.apply_settings_to_file(&path, pack, &toml_fragment, &mut manifest.settings)
    }

    /// Remove settings written by a pack from `.codex/config.toml` (project scope).
    fn remove_project_settings(&self, pack_name: &str, manifest: &mut CodexManifest) -> Result<()> {
        let path = self.project_config_toml_path();
        self.remove_settings_from_file(&path, pack_name, &mut manifest.settings)
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

        // User-scope — save manifest after each step so a failure mid-way leaves the
        // manifest consistent with whatever was actually written to disk.
        let mut manifest = self.load_manifest()?;
        self.apply_servers(pack, &mut manifest)?;
        self.save_manifest(&manifest)?;
        self.apply_skills(pack, &mut manifest)?;
        self.save_manifest(&manifest)?;
        self.apply_prompts(pack, &mut manifest)?;
        self.save_manifest(&manifest)?;
        self.apply_settings(pack, &mut manifest)?;
        self.save_manifest(&manifest)?;

        // Project-scope — only if a `.codex/` directory exists in cwd.
        if self.has_project_scope() {
            let mut project_manifest = self.load_project_manifest()?;
            self.apply_project_servers(pack, &mut project_manifest)?;
            self.save_project_manifest(&project_manifest)?;
            self.apply_project_settings(pack, &mut project_manifest)?;
            self.save_project_manifest(&project_manifest)?;
        }

        Ok(())
    }

    fn remove(&self, pack_name: &str) -> Result<()> {
        // User-scope — only touch the manifest if it already exists.
        let manifest_path = self.manifest_path()?;
        if manifest_path.exists() {
            let mut manifest = self.load_manifest()?;
            self.remove_servers(pack_name, &mut manifest)?;
            self.remove_skills(pack_name, &mut manifest)?;
            self.remove_prompts(pack_name, &mut manifest)?;
            self.remove_settings(pack_name, &mut manifest)?;
            self.save_manifest(&manifest)?;
        }

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

        // Check tracked servers exist in config.toml
        let config_path = self.config_toml_path()?;
        if config_path.exists() {
            if let Ok(config) = Self::read_config_toml(&config_path) {
                let mcp_servers = config
                    .as_table()
                    .and_then(|t| t.get("mcp_servers"))
                    .and_then(|v| v.as_table());
                for (server_name, pack_name) in &manifest.servers {
                    if mcp_servers.and_then(|m| m.get(server_name)).is_none() {
                        issues.push(DiagnosticIssue {
                            severity: Severity::Warning,
                            message: format!(
                                "server '{server_name}' (from pack '{pack_name}') tracked but missing from config.toml"
                            ),
                            suggestion: Some(format!("run `weave install {pack_name}` to re-apply")),
                        });
                    }
                }
            }
        }

        // Check tracked skill files exist on disk
        if let Ok(skills_dir) = self.skills_dir() {
            for (filename, pack_name) in &manifest.skills {
                if !skills_dir.join(filename).exists() {
                    issues.push(DiagnosticIssue {
                        severity: Severity::Warning,
                        message: format!(
                            "skill file '{filename}' (from pack '{pack_name}') is tracked but missing"
                        ),
                        suggestion: Some(format!("run `weave install {pack_name}` to re-apply")),
                    });
                }
            }
        }

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

        // Check tracked settings keys still exist in config.toml
        if !manifest.settings.is_empty() && config_path.exists() {
            if let Ok(config) = Self::read_config_toml(&config_path) {
                if let Some(config_table) = config.as_table() {
                    for (pack_name, record) in &manifest.settings {
                        if let Some(frag_obj) = record.applied.as_object() {
                            for key in frag_obj.keys() {
                                if config_table.get(key).is_none() {
                                    issues.push(DiagnosticIssue {
                                        severity: Severity::Warning,
                                        message: format!(
                                            "settings key '{key}' (from pack '{pack_name}') is tracked but missing from config.toml"
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

// ── TOML server config builder ────────────────────────────────────────────────

/// Build a Codex CLI MCP server config TOML value.
///
/// Returns `Err(reason)` if required fields are missing for the chosen transport.
fn build_codex_server_config(server: &McpServer) -> std::result::Result<toml::Value, String> {
    let mut table = toml::value::Table::new();

    match server.transport {
        Some(Transport::Http) => {
            // HTTP transport: requires `url`.
            let url = server.url.as_deref().ok_or_else(|| {
                format!(
                    "server '{}' uses HTTP transport but has no `url` field — \
                     add `url = \"https://...\"` to the server definition in pack.toml",
                    server.name
                )
            })?;
            table.insert("url".into(), toml::Value::String(url.to_owned()));
        }
        _ => {
            // Stdio (default): requires `command`.
            let command = server.command.as_deref().ok_or_else(|| {
                format!(
                    "server '{}' uses stdio transport but has no `command` field — \
                     add `command = \"...\"` to the server definition in pack.toml",
                    server.name
                )
            })?;
            table.insert("command".into(), toml::Value::String(command.to_owned()));

            if !server.args.is_empty() {
                table.insert(
                    "args".into(),
                    toml::Value::Array(
                        server
                            .args
                            .iter()
                            .map(|a| toml::Value::String(a.clone()))
                            .collect(),
                    ),
                );
            }
        }
    }

    table.insert("enabled".into(), toml::Value::Boolean(true));

    if !server.env.is_empty() {
        let mut env_table = toml::value::Table::new();
        for key in server.env.keys() {
            // Write "${KEY}" references so the config clearly signals which env
            // vars the user must populate. An actual secret value is never stored.
            env_table.insert(key.clone(), toml::Value::String(format!("${{{key}}}")));
        }
        table.insert("env".into(), toml::Value::Table(env_table));
    }

    Ok(toml::Value::Table(table))
}

// ── Settings helpers ──────────────────────────────────────────────────────────

/// Which file format the settings came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SettingsFormat {
    Toml,
    Json,
}

/// Load the settings fragment for a pack (TOML preferred, JSON as fallback).
/// Returns `(Some(json_value), format)` or `(None, _)` if no settings file exists.
fn load_settings_fragment(
    pack_name: &str,
    version: &semver::Version,
) -> Result<(Option<serde_json::Value>, SettingsFormat)> {
    // Prefer TOML
    if let Some(content) = Store::read_pack_file(pack_name, version, "settings/codex.toml")? {
        if content.trim().is_empty() {
            return Ok((None, SettingsFormat::Toml));
        }
        // Parse TOML and convert to JSON for uniform storage in the JSON sidecar.
        let toml_val: toml::Value =
            toml::from_str(&content).map_err(|e| WeaveError::ApplyFailed {
                pack: pack_name.to_string(),
                cli: "Codex CLI".into(),
                reason: format!("invalid settings/codex.toml: {e}"),
            })?;
        let json_val = toml_value_to_json(&toml_val);
        return Ok((Some(json_val), SettingsFormat::Toml));
    }

    // Fallback to JSON
    if let Some(content) = Store::read_pack_file(pack_name, version, "settings/codex.json")? {
        if content.trim().is_empty() {
            return Ok((None, SettingsFormat::Json));
        }
        let json_val: serde_json::Value =
            serde_json::from_str(&content).map_err(|e| WeaveError::ApplyFailed {
                pack: pack_name.to_string(),
                cli: "Codex CLI".into(),
                reason: format!("invalid settings/codex.json: {e}"),
            })?;
        return Ok((Some(json_val), SettingsFormat::Json));
    }

    Ok((None, SettingsFormat::Toml))
}

/// Convert a JSON value (settings fragment) to a TOML value for merging.
fn json_to_toml_value(
    json: &serde_json::Value,
    pack_name: &str,
    _format: SettingsFormat,
) -> Result<toml::Value> {
    json_value_to_toml(json).ok_or_else(|| WeaveError::ApplyFailed {
        pack: pack_name.to_string(),
        cli: "Codex CLI".into(),
        reason: "settings fragment must be a TOML-compatible object".into(),
    })
}

/// Convert a `serde_json::Value` to a `toml::Value`, returning `None` for incompatible types.
fn json_value_to_toml(val: &serde_json::Value) -> Option<toml::Value> {
    match val {
        serde_json::Value::Null => None,
        serde_json::Value::Bool(b) => Some(toml::Value::Boolean(*b)),
        serde_json::Value::Number(n) => {
            if let Some(i) = n.as_i64() {
                Some(toml::Value::Integer(i))
            } else {
                n.as_f64().map(toml::Value::Float)
            }
        }
        serde_json::Value::String(s) => Some(toml::Value::String(s.clone())),
        serde_json::Value::Array(arr) => {
            let items: Vec<toml::Value> = arr.iter().filter_map(json_value_to_toml).collect();
            Some(toml::Value::Array(items))
        }
        serde_json::Value::Object(obj) => {
            let mut table = toml::value::Table::new();
            for (k, v) in obj {
                if let Some(tv) = json_value_to_toml(v) {
                    table.insert(k.clone(), tv);
                }
            }
            Some(toml::Value::Table(table))
        }
    }
}

/// Convert a `toml::Value` to a `serde_json::Value` for storage in the JSON sidecar.
fn toml_value_to_json(val: &toml::Value) -> serde_json::Value {
    match val {
        toml::Value::String(s) => serde_json::Value::String(s.clone()),
        toml::Value::Integer(i) => serde_json::json!(*i),
        toml::Value::Float(f) => serde_json::json!(*f),
        toml::Value::Boolean(b) => serde_json::Value::Bool(*b),
        toml::Value::Array(arr) => {
            serde_json::Value::Array(arr.iter().map(toml_value_to_json).collect())
        }
        toml::Value::Table(table) => {
            let mut map = serde_json::Map::new();
            for (k, v) in table {
                map.insert(k.clone(), toml_value_to_json(v));
            }
            serde_json::Value::Object(map)
        }
        toml::Value::Datetime(dt) => serde_json::Value::String(dt.to_string()),
    }
}

/// Convert a `toml::value::Table` (as vec of pairs) to a JSON object.
fn toml_table_to_json(pairs: &toml::value::Table) -> serde_json::Value {
    let mut map = serde_json::Map::new();
    for (k, v) in pairs {
        map.insert(k.clone(), toml_value_to_json(v));
    }
    serde_json::Value::Object(map)
}

// ── which_exists ─────────────────────────────────────────────────────────────

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
