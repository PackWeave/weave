use std::collections::HashMap;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::adapters::{CliAdapter, DiagnosticIssue, Severity};
use crate::core::pack::{McpServer, ResolvedPack, Transport};
use crate::core::store::Store;
use crate::error::{Result, WeaveError};
use crate::util;

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
    settings_keys: HashMap<String, Vec<String>>, // pack_name -> keys written
}

pub struct ClaudeCodeAdapter {
    home: Option<PathBuf>,
}

impl ClaudeCodeAdapter {
    pub fn new() -> Self {
        Self {
            home: dirs::home_dir(),
        }
    }

    #[cfg(test)]
    pub fn with_home(home: PathBuf) -> Self {
        Self { home: Some(home) }
    }

    fn home(&self) -> Result<&PathBuf> {
        self.home.as_ref().ok_or(WeaveError::NoHomeDir)
    }

    /// `~/.claude/`
    fn claude_dir(&self) -> Result<PathBuf> {
        Ok(self.home()?.join(".claude"))
    }

    /// `~/.claude.json` — user-scope MCP servers
    fn claude_json_path(&self) -> Result<PathBuf> {
        Ok(self.home()?.join(".claude.json"))
    }

    /// `~/.claude/.packweave_manifest.json` — ownership tracking
    fn manifest_path(&self) -> Result<PathBuf> {
        Ok(self.claude_dir()?.join(".packweave_manifest.json"))
    }

    /// `~/.claude/commands/` — slash commands
    fn commands_dir(&self) -> Result<PathBuf> {
        Ok(self.claude_dir()?.join("commands"))
    }

    /// `~/.claude/CLAUDE.md` — global system prompt
    fn claude_md_path(&self) -> Result<PathBuf> {
        Ok(self.claude_dir()?.join("CLAUDE.md"))
    }

    /// `~/.claude/settings.json` — user-scope settings
    fn settings_path(&self) -> Result<PathBuf> {
        Ok(self.claude_dir()?.join("settings.json"))
    }

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
            serde_json::to_string_pretty(manifest).expect("manifest serialization cannot fail");
        util::write_file(&path, &content)
    }

    /// Merge pack servers into `~/.claude.json`.
    fn apply_servers(&self, pack: &ResolvedPack, manifest: &mut PackweaveManifest) -> Result<()> {
        if pack.pack.servers.is_empty() {
            return Ok(());
        }

        let path = self.claude_json_path()?;
        let mut config: serde_json::Value = if path.exists() {
            let content = util::read_file(&path)?;
            serde_json::from_str(&content).map_err(|e| WeaveError::Json {
                path: path.clone(),
                source: e,
            })?
        } else {
            serde_json::json!({})
        };

        let servers_map = config
            .as_object_mut()
            .expect("claude.json is always an object")
            .entry("mcpServers")
            .or_insert_with(|| serde_json::json!({}));

        for server in &pack.pack.servers {
            let server_config = build_claude_server_config(server);
            servers_map[&server.name] = server_config;
            manifest
                .servers
                .insert(server.name.clone(), pack.pack.name.clone());
        }

        let content =
            serde_json::to_string_pretty(&config).expect("JSON serialization cannot fail");
        util::write_file(&path, &content)
    }

    /// Remove pack servers from `~/.claude.json`.
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
        if !path.exists() {
            return Ok(());
        }

        let content = util::read_file(&path)?;
        let mut config: serde_json::Value =
            serde_json::from_str(&content).map_err(|e| WeaveError::Json {
                path: path.clone(),
                source: e,
            })?;

        if let Some(mcp) = config.get_mut("mcpServers").and_then(|v| v.as_object_mut()) {
            for server_name in &servers_to_remove {
                mcp.remove(server_name);
                manifest.servers.remove(server_name);
            }
        }

        let output = serde_json::to_string_pretty(&config).expect("JSON serialization cannot fail");
        util::write_file(&path, &output)
    }

    /// Copy slash command files with namespaced filenames.
    fn apply_commands(&self, pack: &ResolvedPack, manifest: &mut PackweaveManifest) -> Result<()> {
        let commands_dir = Store::pack_dir(&pack.pack.name, &pack.pack.version)?.join("commands");

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

        // Remove existing block if present (idempotency)
        if let (Some(start), Some(end_pos)) = (content.find(&begin_tag), content.find(&end_tag)) {
            let end = end_pos + end_tag.len();
            // Also remove trailing newline if present
            let end = if content[end..].starts_with('\n') {
                end + 1
            } else {
                end
            };
            content.replace_range(start..end, "");
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

        if let (Some(start), Some(end_pos)) = (content.find(&begin_tag), content.find(&end_tag)) {
            let end = end_pos + end_tag.len();
            let end = if content[end..].starts_with('\n') {
                end + 1
            } else {
                end
            };
            content.replace_range(start..end, "");
            util::write_file(&claude_md, &content)?;
        }

        manifest.prompt_blocks.retain(|n| n != pack_name);
        Ok(())
    }

    /// Deep-merge settings fragment into settings.json.
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
        let mut config: serde_json::Value = if path.exists() {
            let content = util::read_file(&path)?;
            serde_json::from_str(&content).map_err(|e| WeaveError::Json {
                path: path.clone(),
                source: e,
            })?
        } else {
            serde_json::json!({})
        };

        // Track which keys we write
        let keys: Vec<String> = if let Some(obj) = fragment.as_object() {
            obj.keys().cloned().collect()
        } else {
            Vec::new()
        };

        deep_merge(&mut config, &fragment);

        let output = serde_json::to_string_pretty(&config).expect("JSON serialization cannot fail");
        util::write_file(&path, &output)?;

        manifest.settings_keys.insert(pack.pack.name.clone(), keys);

        Ok(())
    }

    /// Remove settings keys written by a pack.
    fn remove_settings(&self, pack_name: &str, manifest: &mut PackweaveManifest) -> Result<()> {
        let keys = match manifest.settings_keys.remove(pack_name) {
            Some(k) => k,
            None => return Ok(()),
        };

        let path = self.settings_path()?;
        if !path.exists() {
            return Ok(());
        }

        let content = util::read_file(&path)?;
        let mut config: serde_json::Value =
            serde_json::from_str(&content).map_err(|e| WeaveError::Json {
                path: path.clone(),
                source: e,
            })?;

        if let Some(obj) = config.as_object_mut() {
            for key in &keys {
                obj.remove(key);
            }
        }

        let output = serde_json::to_string_pretty(&config).expect("JSON serialization cannot fail");
        util::write_file(&path, &output)
    }
}

impl CliAdapter for ClaudeCodeAdapter {
    fn name(&self) -> &str {
        "Claude Code"
    }

    fn is_installed(&self) -> bool {
        // Check if the claude CLI exists or if ~/.claude/ exists
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

        let mut manifest = self.load_manifest()?;

        self.apply_servers(pack, &mut manifest)?;
        self.apply_commands(pack, &mut manifest)?;
        self.apply_prompts(pack, &mut manifest)?;
        self.apply_settings(pack, &mut manifest)?;

        self.save_manifest(&manifest)?;
        Ok(())
    }

    fn remove(&self, pack_name: &str) -> Result<()> {
        let mut manifest = self.load_manifest()?;

        self.remove_servers(pack_name, &mut manifest)?;
        self.remove_commands(pack_name, &mut manifest)?;
        self.remove_prompts(pack_name, &mut manifest)?;
        self.remove_settings(pack_name, &mut manifest)?;

        self.save_manifest(&manifest)?;
        Ok(())
    }

    fn diagnose(&self) -> Result<Vec<DiagnosticIssue>> {
        let mut issues = Vec::new();

        let manifest = self.load_manifest()?;

        // Check that tracked servers still exist in claude.json
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
                            suggestion: Some(format!(
                                "run `weave install {pack_name}` to re-apply"
                            )),
                        });
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
            // Write env var references, never actual values
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

/// Check if a command exists on PATH.
fn which_exists(cmd: &str) -> bool {
    std::process::Command::new("which")
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
        ClaudeCodeAdapter::with_home(dir.path().to_path_buf())
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

    #[test]
    fn apply_and_remove_servers() {
        let dir = TempDir::new().unwrap();
        let adapter = test_adapter(&dir);

        // Create .claude dir
        std::fs::create_dir_all(dir.path().join(".claude")).unwrap();

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
    fn apply_and_remove_prompts() {
        let dir = TempDir::new().unwrap();
        let adapter = test_adapter(&dir);
        std::fs::create_dir_all(dir.path().join(".claude")).unwrap();

        // Write initial CLAUDE.md content
        let claude_md = dir.path().join(".claude").join("CLAUDE.md");
        std::fs::write(&claude_md, "# My instructions\n").unwrap();

        // We can't test apply_prompts directly without the store,
        // but we can test the prompt tag insertion/removal logic
        let pack_name = "webdev";
        let begin_tag = format!("<!-- packweave:begin:{pack_name} -->");
        let end_tag = format!("<!-- packweave:end:{pack_name} -->");

        let mut content = std::fs::read_to_string(&claude_md).unwrap();
        content.push_str(&begin_tag);
        content.push('\n');
        content.push_str("You are a web developer.");
        content.push('\n');
        content.push_str(&end_tag);
        content.push('\n');
        std::fs::write(&claude_md, &content).unwrap();

        // Now remove
        let mut manifest = PackweaveManifest::default();
        manifest.prompt_blocks.push("webdev".into());
        adapter.remove_prompts("webdev", &mut manifest).unwrap();

        let final_content = std::fs::read_to_string(&claude_md).unwrap();
        assert_eq!(final_content.trim(), "# My instructions");
        assert!(manifest.prompt_blocks.is_empty());
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
    fn idempotent_prompt_apply() {
        // Verify that re-applying doesn't duplicate blocks
        let content = "# Docs\n<!-- packweave:begin:test -->\nHello\n<!-- packweave:end:test -->\n";
        let begin_tag = "<!-- packweave:begin:test -->";
        let end_tag = "<!-- packweave:end:test -->";

        let mut result = content.to_string();

        // Simulate re-apply: remove old block
        if let (Some(start), Some(end_pos)) = (result.find(begin_tag), result.find(end_tag)) {
            let end = end_pos + end_tag.len();
            let end = if result[end..].starts_with('\n') {
                end + 1
            } else {
                end
            };
            result.replace_range(start..end, "");
        }

        // Add new block
        if !result.ends_with('\n') {
            result.push('\n');
        }
        result.push_str(begin_tag);
        result.push('\n');
        result.push_str("Hello");
        result.push('\n');
        result.push_str(end_tag);
        result.push('\n');

        // Should have exactly one block
        assert_eq!(
            result.matches(begin_tag).count(),
            1,
            "should have exactly one begin tag"
        );
    }
}
