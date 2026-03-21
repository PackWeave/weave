use std::collections::HashMap;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::adapters::{CliAdapter, DiagnosticIssue, Severity};
use crate::core::pack::{McpServer, ResolvedPack};
use crate::core::store::Store;
use crate::error::{Result, WeaveError};
use crate::util;

/// Sidecar manifest tracking what weave wrote to Gemini CLI config.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
struct GeminiManifest {
    #[serde(default)]
    servers: HashMap<String, String>, // server_name -> pack_name
    #[serde(default)]
    prompt_blocks: Vec<String>,
}

pub struct GeminiCliAdapter {
    home: Option<PathBuf>,
}

impl GeminiCliAdapter {
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
            serde_json::to_string_pretty(manifest).expect("manifest serialization cannot fail");
        util::write_file(&path, &content)
    }

    /// Merge pack servers into Gemini's settings.json.
    fn apply_servers(&self, pack: &ResolvedPack, manifest: &mut GeminiManifest) -> Result<()> {
        if pack.pack.servers.is_empty() {
            return Ok(());
        }

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

        let servers_map = config
            .as_object_mut()
            .expect("settings.json is always an object")
            .entry("mcpServers")
            .or_insert_with(|| serde_json::json!({}));

        for server in &pack.pack.servers {
            let server_config = build_gemini_server_config(server);
            servers_map[&server.name] = server_config;
            manifest
                .servers
                .insert(server.name.clone(), pack.pack.name.clone());
        }

        let content =
            serde_json::to_string_pretty(&config).expect("JSON serialization cannot fail");
        util::write_file(&path, &content)
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

        // Remove existing block (idempotency)
        if let (Some(start), Some(end_pos)) = (content.find(&begin_tag), content.find(&end_tag)) {
            let end = end_pos + end_tag.len();
            let end = if content[end..].starts_with('\n') {
                end + 1
            } else {
                end
            };
            content.replace_range(start..end, "");
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

        if let (Some(start), Some(end_pos)) = (content.find(&begin_tag), content.find(&end_tag)) {
            let end = end_pos + end_tag.len();
            let end = if content[end..].starts_with('\n') {
                end + 1
            } else {
                end
            };
            content.replace_range(start..end, "");
            util::write_file(&gemini_md, &content)?;
        }

        manifest.prompt_blocks.retain(|n| n != pack_name);
        Ok(())
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

        let mut manifest = self.load_manifest()?;

        self.apply_servers(pack, &mut manifest)?;
        self.apply_prompts(pack, &mut manifest)?;

        self.save_manifest(&manifest)?;
        Ok(())
    }

    fn remove(&self, pack_name: &str) -> Result<()> {
        let mut manifest = self.load_manifest()?;

        self.remove_servers(pack_name, &mut manifest)?;
        self.remove_prompts(pack_name, &mut manifest)?;

        self.save_manifest(&manifest)?;
        Ok(())
    }

    fn diagnose(&self) -> Result<Vec<DiagnosticIssue>> {
        let mut issues = Vec::new();
        let manifest = self.load_manifest()?;

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

    #[test]
    fn apply_and_remove_servers() {
        let dir = TempDir::new().unwrap();
        let adapter = test_adapter(&dir);
        std::fs::create_dir_all(dir.path().join(".gemini")).unwrap();

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
}
