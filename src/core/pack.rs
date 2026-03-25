use std::collections::HashMap;
use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::error::{Result, WeaveError};

/// Current schema version for `pack.toml` files.
pub const CURRENT_PACK_SCHEMA_VERSION: u32 = 1;

/// Serde default for pack manifests that predate schema versioning — always returns 1
/// (the original schema), not `CURRENT_PACK_SCHEMA_VERSION`. Files that omit
/// the field were written before versioning existed and are implicitly version 1.
fn default_schema_version() -> u32 {
    1
}

/// The in-memory representation of a parsed `pack.toml`.
/// A `Pack` that exists is always structurally valid.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Pack {
    pub name: String,
    pub version: semver::Version,
    pub description: String,
    pub authors: Vec<String>,
    #[serde(default)]
    pub license: Option<String>,
    #[serde(default)]
    pub repository: Option<String>,
    #[serde(default)]
    pub keywords: Vec<String>,
    #[serde(default)]
    pub min_tool_version: Option<semver::Version>,
    #[serde(default)]
    pub servers: Vec<McpServer>,
    #[serde(default)]
    pub dependencies: HashMap<String, semver::VersionReq>,
    #[serde(default)]
    pub extensions: PackExtensions,
    #[serde(default)]
    pub targets: PackTargets,
}

/// An MCP server definition within a pack.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpServer {
    pub name: String,
    #[serde(rename = "type", default)]
    pub package_type: Option<String>,
    #[serde(default)]
    pub package: Option<String>,
    /// The executable to run. Required for stdio transport; unused for http.
    #[serde(default)]
    pub command: Option<String>,
    #[serde(default)]
    pub args: Vec<String>,
    /// The endpoint URL. Required for http transport; unused for stdio.
    #[serde(default)]
    pub url: Option<String>,
    /// Optional HTTP headers (e.g. `Authorization`). Only used for http transport.
    #[serde(default)]
    pub headers: Option<HashMap<String, String>>,
    #[serde(default)]
    pub transport: Option<Transport>,
    #[serde(default)]
    pub tools: Vec<String>,
    #[serde(default)]
    pub env: HashMap<String, EnvVar>,
}

/// Transport type for an MCP server.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum Transport {
    Stdio,
    Http,
}

/// Environment variable metadata. Never stores the actual secret value.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EnvVar {
    #[serde(default)]
    pub required: bool,
    #[serde(default)]
    pub secret: bool,
    #[serde(default)]
    pub description: Option<String>,
}

/// A single hook action within a hook event.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HookEntry {
    /// Pattern to match against (e.g. tool name like "Bash").
    /// If omitted, the hook matches all events.
    #[serde(default)]
    pub matcher: Option<String>,
    /// Hook type. Currently only "command" is supported.
    #[serde(rename = "type", default = "default_hook_type")]
    pub hook_type: String,
    /// Shell command to execute.
    pub command: String,
}

fn default_hook_type() -> String {
    "command".to_string()
}

/// CLI-specific extension configuration.
/// Adapters ignore keys they don't understand (forward compatibility).
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct PackExtensions {
    #[serde(default)]
    pub claude_code: Option<serde_json::Value>,
    #[serde(default)]
    pub gemini_cli: Option<serde_json::Value>,
    #[serde(default)]
    pub codex_cli: Option<serde_json::Value>,
}

/// Which CLIs this pack targets. Defaults to all true.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PackTargets {
    #[serde(default = "default_true")]
    pub claude_code: bool,
    #[serde(default = "default_true")]
    pub gemini_cli: bool,
    #[serde(default = "default_true")]
    pub codex_cli: bool,
}

fn default_true() -> bool {
    true
}

impl Default for PackTargets {
    fn default() -> Self {
        Self {
            claude_code: true,
            gemini_cli: true,
            codex_cli: true,
        }
    }
}

/// A pack with resolved (exact, pinned) version.
#[derive(Debug, Clone)]
pub struct ResolvedPack {
    pub pack: Pack,
    pub source: PackSource,
}

/// Where a pack was sourced from.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum PackSource {
    Registry { registry_url: String },
    Local { path: String },
    Git { url: String, rev: Option<String> },
}

/// Canonical nested format: metadata under a `[pack]` section.
#[derive(Debug, Deserialize)]
struct PackManifest {
    /// Pack manifest schema version. Defaults to 1 for files that predate versioning.
    #[serde(default = "default_schema_version")]
    schema_version: u32,
    pack: PackMetadataToml,
    #[serde(default)]
    servers: Option<Vec<McpServer>>,
    #[serde(default)]
    dependencies: Option<HashMap<String, semver::VersionReq>>,
    #[serde(default)]
    extensions: Option<PackExtensions>,
    #[serde(default)]
    targets: Option<PackTargets>,
}

#[derive(Debug, Deserialize)]
struct PackMetadataToml {
    name: String,
    version: semver::Version,
    description: String,
    #[serde(default)]
    authors: Vec<String>,
    #[serde(default)]
    license: Option<String>,
    #[serde(default)]
    repository: Option<String>,
    #[serde(default)]
    keywords: Vec<String>,
    #[serde(default)]
    min_tool_version: Option<semver::Version>,
}

impl Pack {
    /// Parse and validate a pack manifest from a TOML string.
    ///
    /// Expects the canonical nested format with a `[pack]` section header.
    pub fn from_toml(content: &str, path: &Path) -> Result<Self> {
        let manifest: PackManifest = toml::from_str(content).map_err(|e| WeaveError::Toml {
            path: path.to_path_buf(),
            source: Box::new(e),
        })?;
        if manifest.schema_version > CURRENT_PACK_SCHEMA_VERSION {
            return Err(WeaveError::SchemaVersionTooNew {
                file_kind: "pack manifest",
                path: path.to_path_buf(),
                found: manifest.schema_version,
                supported: CURRENT_PACK_SCHEMA_VERSION,
                current_version: env!("CARGO_PKG_VERSION"),
            });
        }
        let pack = Pack {
            name: manifest.pack.name,
            version: manifest.pack.version,
            description: manifest.pack.description,
            authors: manifest.pack.authors,
            license: manifest.pack.license,
            repository: manifest.pack.repository,
            keywords: manifest.pack.keywords,
            min_tool_version: manifest.pack.min_tool_version,
            servers: manifest.servers.unwrap_or_default(),
            dependencies: manifest.dependencies.unwrap_or_default(),
            extensions: manifest.extensions.unwrap_or_default(),
            targets: manifest.targets.unwrap_or_default(),
        };
        pack.validate(path)?;
        Ok(pack)
    }

    /// Load a pack from a directory containing `pack.toml`.
    pub fn load(dir: &Path) -> Result<Self> {
        let manifest_path = dir.join("pack.toml");
        let content = crate::util::read_file(&manifest_path)?;
        Self::from_toml(&content, &manifest_path)
    }

    /// Returns true if any CLI extension declares hooks.
    pub fn has_hooks(&self) -> bool {
        self.hooks_for_cli("claude_code").is_some()
            || self.hooks_for_cli("gemini_cli").is_some()
            || self.hooks_for_cli("codex_cli").is_some()
    }

    /// Extract hooks from a CLI extension value, if present.
    ///
    /// Looks for `extensions.<cli>.hooks` in the pack manifest. Returns a
    /// map of event name to list of hook entries, or `None` if no hooks are declared.
    pub fn hooks_for_cli(
        &self,
        cli: &str,
    ) -> Option<std::collections::BTreeMap<String, Vec<HookEntry>>> {
        let ext_value = match cli {
            "claude_code" => self.extensions.claude_code.as_ref()?,
            "gemini_cli" => self.extensions.gemini_cli.as_ref()?,
            "codex_cli" => self.extensions.codex_cli.as_ref()?,
            _ => return None,
        };
        let hooks_value = ext_value.get("hooks")?;
        match serde_json::from_value(hooks_value.clone()) {
            Ok(hooks) => Some(hooks),
            Err(e) => {
                log::warn!(
                    "pack '{}': malformed extensions.{}.hooks — {}",
                    self.name,
                    cli,
                    e
                );
                None
            }
        }
    }

    fn validate(&self, path: &Path) -> Result<()> {
        if self.name.is_empty() {
            return Err(WeaveError::InvalidManifest {
                path: path.to_path_buf(),
                reason: "pack name cannot be empty".into(),
            });
        }

        // Name must be lowercase alphanumeric + hyphens
        if !self
            .name
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
        {
            return Err(WeaveError::InvalidManifest {
                path: path.to_path_buf(),
                reason: format!(
                    "pack name '{}' must contain only lowercase letters, numbers, and hyphens",
                    self.name
                ),
            });
        }

        if self.description.is_empty() {
            return Err(WeaveError::InvalidManifest {
                path: path.to_path_buf(),
                reason: "pack description cannot be empty".into(),
            });
        }

        // Validate server names are unique and transport requirements are met
        let mut seen_servers = std::collections::HashSet::new();
        for server in &self.servers {
            if !seen_servers.insert(&server.name) {
                return Err(WeaveError::InvalidManifest {
                    path: path.to_path_buf(),
                    reason: format!("duplicate server name '{}'", server.name),
                });
            }

            match server.transport.as_ref() {
                Some(Transport::Http) => {
                    if server.url.is_none() {
                        return Err(WeaveError::InvalidManifest {
                            path: path.to_path_buf(),
                            reason: format!(
                                "server '{}' uses HTTP transport but has no `url` field",
                                server.name
                            ),
                        });
                    }
                }
                _ => {
                    // Stdio (default): command is required
                    if server.command.is_none() {
                        return Err(WeaveError::InvalidManifest {
                            path: path.to_path_buf(),
                            reason: format!(
                                "server '{}' uses stdio transport but has no `command` field",
                                server.name
                            ),
                        });
                    }
                }
            }

            // Validate HTTP headers for plaintext secrets.
            if let Some(headers) = &server.headers {
                validate_server_headers(&server.name, headers, path)?;
            }
        }

        Ok(())
    }
}

/// Returns true if the value is an environment variable reference (`${...}`).
fn is_env_var_reference(value: &str) -> bool {
    let trimmed = value.trim();
    trimmed.starts_with("${") && trimmed.ends_with('}') && trimmed.len() > 3
}

/// Returns true if the value contains at least one `${VAR}` environment
/// variable reference (possibly among other text, e.g. `Bearer ${TOKEN}`).
fn contains_env_var_reference(value: &str) -> bool {
    if let Some(start) = value.find("${") {
        let rest = &value[start + 2..];
        if let Some(end) = rest.find('}') {
            // Ensure there is at least one character between `${` and `}`
            return end > 0;
        }
    }
    false
}

/// Header names that are known to carry secrets and must use `${VAR}` references.
const SECRET_HEADER_NAMES: &[&str] = &[
    "authorization",
    "proxy-authorization",
    "x-api-key",
    "x-auth-token",
];

/// Header names that are safe to carry static (non-secret) values.
const SAFE_STATIC_HEADERS: &[&str] = &[
    "accept",
    "accept-encoding",
    "accept-language",
    "cache-control",
    "content-type",
    "user-agent",
    "x-api-version",
    "x-request-id",
];

/// Returns true if the value looks like it contains a plaintext secret:
/// - Starts with `Bearer` or `Basic` (case-insensitive) followed by a token
/// - Looks like a long random string (potential API key)
fn looks_like_secret(value: &str) -> bool {
    let trimmed = value.trim();

    // Bearer or Basic auth tokens with actual token content (case-insensitive).
    // Accept any whitespace separator between the scheme and token.
    let lower = trimmed.to_ascii_lowercase();
    if let Some(rest) = lower.strip_prefix("bearer")
        && rest.starts_with(char::is_whitespace)
    {
        let token = trimmed[6..].trim();
        return !token.is_empty() && !contains_env_var_reference(token);
    }
    if let Some(rest) = lower.strip_prefix("basic")
        && rest.starts_with(char::is_whitespace)
    {
        let token = trimmed[5..].trim();
        return !token.is_empty() && !contains_env_var_reference(token);
    }

    // Long high-entropy strings that look like API keys (32+ chars, mostly
    // alphanumeric with common key punctuation like hyphens and underscores).
    if trimmed.len() >= 32 {
        let alnum_or_key_chars = trimmed
            .chars()
            .filter(|c| c.is_ascii_alphanumeric() || *c == '-' || *c == '_' || *c == '.')
            .count();
        let ratio = alnum_or_key_chars as f64 / trimmed.len() as f64;
        if ratio > 0.85 {
            return true;
        }
    }

    false
}

/// Validate HTTP headers on a server definition. Rejects plaintext secrets
/// and enforces `${VAR}` env-var references for known secret headers.
fn validate_server_headers(
    server_name: &str,
    headers: &HashMap<String, String>,
    path: &Path,
) -> Result<()> {
    for (name, value) in headers {
        let lower_name = name.to_ascii_lowercase();

        // Values that are entirely an env var reference are always allowed.
        if is_env_var_reference(value) {
            continue;
        }

        // Known secret headers must use env var references.  We also accept
        // composite values that contain an env var (e.g. `Bearer ${TOKEN}`).
        if SECRET_HEADER_NAMES.contains(&lower_name.as_str()) {
            if contains_env_var_reference(value) {
                continue;
            }
            return Err(WeaveError::InvalidManifest {
                path: path.to_path_buf(),
                reason: format!(
                    "server '{}': header '{}' typically carries a secret — use an \
                     environment variable reference like `${{MY_TOKEN}}` instead of a \
                     plaintext value",
                    server_name, name,
                ),
            });
        }

        // Safe static headers are always allowed with literal values.
        if SAFE_STATIC_HEADERS.contains(&lower_name.as_str()) {
            continue;
        }

        // For unknown headers, check if the value looks like a secret.
        if looks_like_secret(value) {
            return Err(WeaveError::InvalidManifest {
                path: path.to_path_buf(),
                reason: format!(
                    "server '{}': header '{}' value looks like a plaintext secret \
                     (Bearer/Basic token or API key) — use an environment variable \
                     reference like `${{MY_SECRET}}` instead",
                    server_name, name,
                ),
            });
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn parse_minimal_pack() {
        let toml = r#"
[pack]
name = "test-pack"
version = "1.0.0"
description = "A test pack"
authors = ["tester"]
"#;
        let pack = Pack::from_toml(toml, &PathBuf::from("test.toml")).unwrap();
        assert_eq!(pack.name, "test-pack");
        assert_eq!(pack.version, semver::Version::new(1, 0, 0));
        assert!(pack.targets.claude_code);
        assert!(pack.targets.gemini_cli);
        assert!(pack.servers.is_empty());
    }

    #[test]
    fn parse_pack_with_servers() {
        let toml = r#"
[pack]
name = "webdev"
version = "0.1.0"
description = "Web development essentials"
authors = ["dev"]

[[servers]]
name = "filesystem"
command = "npx"
args = ["-y", "@modelcontextprotocol/server-filesystem"]
transport = "stdio"

[servers.env.FS_ROOT]
required = true
secret = false
description = "Root directory"
"#;
        let pack = Pack::from_toml(toml, &PathBuf::from("test.toml")).unwrap();
        assert_eq!(pack.servers.len(), 1);
        assert_eq!(pack.servers[0].name, "filesystem");
        assert!(pack.servers[0].env["FS_ROOT"].required);
    }

    #[test]
    fn reject_invalid_name() {
        let toml = r#"
[pack]
name = "Invalid_Name"
version = "1.0.0"
description = "Bad name"
"#;
        let result = Pack::from_toml(toml, &PathBuf::from("test.toml"));
        assert!(result.is_err());
    }

    #[test]
    fn reject_empty_description() {
        let toml = r#"
[pack]
name = "test"
version = "1.0.0"
description = ""
"#;
        let result = Pack::from_toml(toml, &PathBuf::from("test.toml"));
        assert!(result.is_err());
    }

    #[test]
    fn reject_duplicate_servers() {
        let toml = r#"
[pack]
name = "test"
version = "1.0.0"
description = "Test"

[[servers]]
name = "dup"
command = "a"

[[servers]]
name = "dup"
command = "b"
"#;
        let result = Pack::from_toml(toml, &PathBuf::from("test.toml"));
        assert!(result.is_err());
    }

    #[test]
    fn targets_default_to_true() {
        let toml = r#"
[pack]
name = "test"
version = "1.0.0"
description = "Test"
"#;
        let pack = Pack::from_toml(toml, &PathBuf::from("test.toml")).unwrap();
        assert!(pack.targets.claude_code);
        assert!(pack.targets.gemini_cli);
        assert!(pack.targets.codex_cli);
    }

    #[test]
    fn reject_stdio_server_without_command() {
        let toml = r#"
[pack]
name = "test"
version = "1.0.0"
description = "Test"

[[servers]]
name = "my-server"
transport = "stdio"
"#;
        let result = Pack::from_toml(toml, &PathBuf::from("test.toml"));
        assert!(result.is_err());
        let msg = result.unwrap_err().to_string();
        assert!(msg.contains("stdio"), "expected 'stdio' in error: {msg}");
    }

    #[test]
    fn reject_http_server_without_url() {
        let toml = r#"
[pack]
name = "test"
version = "1.0.0"
description = "Test"

[[servers]]
name = "my-http-server"
transport = "http"
"#;
        let result = Pack::from_toml(toml, &PathBuf::from("test.toml"));
        assert!(result.is_err());
        let msg = result.unwrap_err().to_string();
        assert!(msg.contains("http"), "expected 'http' in error: {msg}");
    }

    #[test]
    fn accept_http_server_with_url() {
        let toml = r#"
[pack]
name = "test"
version = "1.0.0"
description = "Test"

[[servers]]
name = "my-http-server"
transport = "http"
url = "https://example.com/mcp"
"#;
        let result = Pack::from_toml(toml, &PathBuf::from("test.toml"));
        assert!(result.is_ok());
    }

    #[test]
    fn has_hooks_returns_false_without_hooks() {
        let toml = r#"
[pack]
name = "test"
version = "1.0.0"
description = "Test"
"#;
        let pack = Pack::from_toml(toml, &PathBuf::from("test.toml")).unwrap();
        assert!(!pack.has_hooks());
    }

    #[test]
    fn has_hooks_returns_true_with_claude_code_hooks() {
        let toml = r#"
[pack]
name = "test"
version = "1.0.0"
description = "Test"

[extensions.claude_code]
hooks = { PreToolUse = [{ matcher = "Bash", command = "echo hello" }] }
"#;
        let pack = Pack::from_toml(toml, &PathBuf::from("test.toml")).unwrap();
        assert!(pack.has_hooks());
    }

    #[test]
    fn hooks_for_cli_parses_entries() {
        let toml = r#"
[pack]
name = "test"
version = "1.0.0"
description = "Test"

[extensions.claude_code.hooks]
PreToolUse = [
    { matcher = "Bash", command = "echo pre" },
    { command = "echo all" },
]
PostToolUse = [
    { matcher = "Write", command = "echo post" },
]
"#;
        let pack = Pack::from_toml(toml, &PathBuf::from("test.toml")).unwrap();
        let hooks = pack.hooks_for_cli("claude_code").unwrap();
        assert_eq!(hooks.len(), 2);
        assert_eq!(hooks["PreToolUse"].len(), 2);
        assert_eq!(hooks["PreToolUse"][0].matcher.as_deref(), Some("Bash"));
        assert_eq!(hooks["PreToolUse"][0].command, "echo pre");
        assert_eq!(hooks["PreToolUse"][0].hook_type, "command");
        assert!(hooks["PreToolUse"][1].matcher.is_none());
        assert_eq!(hooks["PostToolUse"].len(), 1);
    }

    #[test]
    fn hooks_for_cli_returns_none_for_unsupported_cli() {
        let toml = r#"
[pack]
name = "test"
version = "1.0.0"
description = "Test"

[extensions.claude_code.hooks]
PreToolUse = [{ command = "echo hello" }]
"#;
        let pack = Pack::from_toml(toml, &PathBuf::from("test.toml")).unwrap();
        assert!(pack.hooks_for_cli("gemini_cli").is_none());
        assert!(pack.hooks_for_cli("codex_cli").is_none());
    }

    #[test]
    fn parse_http_server_with_headers() {
        let toml = r#"
[pack]
name = "test"
version = "1.0.0"
description = "Test"

[[servers]]
name = "remote-api"
transport = "http"
url = "https://api.example.com/mcp"

[servers.headers]
Authorization = "${API_KEY}"
X-Custom = "static-value"
"#;
        let pack = Pack::from_toml(toml, &PathBuf::from("test.toml")).unwrap();
        assert_eq!(pack.servers.len(), 1);
        let server = &pack.servers[0];
        assert_eq!(server.transport, Some(Transport::Http));
        assert_eq!(server.url.as_deref(), Some("https://api.example.com/mcp"));
        let headers = server.headers.as_ref().expect("headers should be present");
        assert_eq!(headers["Authorization"], "${API_KEY}");
        assert_eq!(headers["X-Custom"], "static-value");
        assert!(server.command.is_none());
    }

    // ── Header validation tests ──────────────────────────────────────

    #[test]
    fn allow_env_var_reference_for_authorization() {
        let toml = r#"
[pack]
name = "test"
version = "1.0.0"
description = "Test"

[[servers]]
name = "api"
transport = "http"
url = "https://example.com/mcp"

[servers.headers]
Authorization = "${MY_TOKEN}"
"#;
        let result = Pack::from_toml(toml, &PathBuf::from("test.toml"));
        assert!(result.is_ok(), "env var ref for Authorization should pass");
    }

    #[test]
    fn reject_plaintext_authorization_header() {
        let toml = r#"
[pack]
name = "test"
version = "1.0.0"
description = "Test"

[[servers]]
name = "api"
transport = "http"
url = "https://example.com/mcp"

[servers.headers]
Authorization = "sk-1234567890abcdef"
"#;
        let result = Pack::from_toml(toml, &PathBuf::from("test.toml"));
        assert!(result.is_err());
        let msg = result.unwrap_err().to_string();
        assert!(msg.contains("secret"), "expected 'secret' in error: {msg}");
    }

    #[test]
    fn reject_plaintext_x_api_key_header() {
        let toml = r#"
[pack]
name = "test"
version = "1.0.0"
description = "Test"

[[servers]]
name = "api"
transport = "http"
url = "https://example.com/mcp"

[servers.headers]
X-API-Key = "my-secret-key"
"#;
        let result = Pack::from_toml(toml, &PathBuf::from("test.toml"));
        assert!(result.is_err());
        let msg = result.unwrap_err().to_string();
        assert!(msg.contains("secret"), "expected 'secret' in error: {msg}");
    }

    #[test]
    fn allow_safe_static_headers() {
        let toml = r#"
[pack]
name = "test"
version = "1.0.0"
description = "Test"

[[servers]]
name = "api"
transport = "http"
url = "https://example.com/mcp"

[servers.headers]
Content-Type = "application/json"
Accept = "application/json"
X-API-Version = "2024-01-01"
"#;
        let result = Pack::from_toml(toml, &PathBuf::from("test.toml"));
        assert!(result.is_ok(), "safe static headers should pass");
    }

    #[test]
    fn reject_bearer_token_in_custom_header() {
        let toml = r#"
[pack]
name = "test"
version = "1.0.0"
description = "Test"

[[servers]]
name = "api"
transport = "http"
url = "https://example.com/mcp"

[servers.headers]
X-Forwarded-Auth = "Bearer eyJhbGciOiJIUzI1NiIsInR5cCI6IkpXVCJ9.payload.signature"
"#;
        let result = Pack::from_toml(toml, &PathBuf::from("test.toml"));
        assert!(result.is_err());
        let msg = result.unwrap_err().to_string();
        assert!(
            msg.contains("plaintext secret"),
            "expected 'plaintext secret' in error: {msg}"
        );
    }

    #[test]
    fn reject_basic_auth_in_custom_header() {
        let toml = r#"
[pack]
name = "test"
version = "1.0.0"
description = "Test"

[[servers]]
name = "api"
transport = "http"
url = "https://example.com/mcp"

[servers.headers]
X-Proxy-Auth = "Basic dXNlcjpwYXNzd29yZA=="
"#;
        let result = Pack::from_toml(toml, &PathBuf::from("test.toml"));
        assert!(result.is_err());
        let msg = result.unwrap_err().to_string();
        assert!(
            msg.contains("plaintext secret"),
            "expected 'plaintext secret' in error: {msg}"
        );
    }

    #[test]
    fn reject_long_random_string_in_custom_header() {
        let toml = r#"
[pack]
name = "test"
version = "1.0.0"
description = "Test"

[[servers]]
name = "api"
transport = "http"
url = "https://example.com/mcp"

[servers.headers]
X-Secret = "aK9xZmQ2NzhhYjNjMTRlOGY5YjJkNWUwMWE4ZjRiNzMw"
"#;
        let result = Pack::from_toml(toml, &PathBuf::from("test.toml"));
        assert!(result.is_err());
        let msg = result.unwrap_err().to_string();
        assert!(
            msg.contains("plaintext secret"),
            "expected 'plaintext secret' in error: {msg}"
        );
    }

    #[test]
    fn allow_env_var_in_bearer_prefix() {
        let toml = r#"
[pack]
name = "test"
version = "1.0.0"
description = "Test"

[[servers]]
name = "api"
transport = "http"
url = "https://example.com/mcp"

[servers.headers]
X-Forwarded-Auth = "Bearer ${MY_TOKEN}"
"#;
        let result = Pack::from_toml(toml, &PathBuf::from("test.toml"));
        // The value starts with "Bearer " but the token part is an env var ref — allowed.
        assert!(
            result.is_ok(),
            "Bearer with env var reference should pass: {:?}",
            result.err()
        );
    }

    #[test]
    fn allow_short_non_secret_custom_header() {
        let toml = r#"
[pack]
name = "test"
version = "1.0.0"
description = "Test"

[[servers]]
name = "api"
transport = "http"
url = "https://example.com/mcp"

[servers.headers]
X-Trace-Id = "my-trace"
X-Region = "us-east-1"
"#;
        let result = Pack::from_toml(toml, &PathBuf::from("test.toml"));
        assert!(
            result.is_ok(),
            "short non-secret custom headers should pass: {:?}",
            result.err()
        );
    }

    #[test]
    fn allow_env_var_for_x_api_key() {
        let toml = r#"
[pack]
name = "test"
version = "1.0.0"
description = "Test"

[[servers]]
name = "api"
transport = "http"
url = "https://example.com/mcp"

[servers.headers]
X-API-Key = "${API_KEY}"
"#;
        let result = Pack::from_toml(toml, &PathBuf::from("test.toml"));
        assert!(
            result.is_ok(),
            "env var ref for X-API-Key should pass: {:?}",
            result.err()
        );
    }

    #[test]
    fn is_env_var_reference_works() {
        assert!(is_env_var_reference("${FOO}"));
        assert!(is_env_var_reference("${MY_API_KEY}"));
        assert!(is_env_var_reference("  ${SPACED}  "));
        assert!(!is_env_var_reference("$FOO"));
        assert!(!is_env_var_reference("plain-value"));
        assert!(!is_env_var_reference("${}"));
    }

    #[test]
    fn looks_like_secret_detects_patterns() {
        assert!(looks_like_secret("Bearer eyJhbGciOiJIUzI1NiJ9.payload.sig"));
        assert!(looks_like_secret("Basic dXNlcjpwYXNzd29yZA=="));
        assert!(looks_like_secret(
            "aK9xZmQ2NzhhYjNjMTRlOGY5YjJkNWUwMWE4ZjRiNzMw"
        ));
        assert!(!looks_like_secret("application/json"));
        assert!(!looks_like_secret("us-east-1"));
        assert!(!looks_like_secret("Bearer ${TOKEN}"));
        assert!(!looks_like_secret("Basic ${CREDS}"));

        // Case-insensitive auth scheme detection
        assert!(looks_like_secret("bearer token123"));
        assert!(looks_like_secret("BEARER token123"));
        assert!(looks_like_secret("basic dXNlcjpwYXNz"));
        assert!(looks_like_secret("BASIC dXNlcjpwYXNz"));
        assert!(looks_like_secret("BeArEr mixed-case-token"));

        // Extra whitespace between scheme and token
        assert!(looks_like_secret("Bearer   token123"));
        assert!(looks_like_secret("Basic\tbase64data"));
    }

    #[test]
    fn contains_env_var_reference_works() {
        assert!(contains_env_var_reference("${FOO}"));
        assert!(contains_env_var_reference("Bearer ${TOKEN}"));
        assert!(contains_env_var_reference("Basic ${CREDS}"));
        assert!(contains_env_var_reference("prefix ${VAR} suffix"));
        assert!(!contains_env_var_reference("plain-value"));
        assert!(!contains_env_var_reference("${}"));
        assert!(!contains_env_var_reference("$FOO"));
    }

    #[test]
    fn x_custom_header_not_in_safe_list() {
        // x-custom was removed from the safe list; a secret value should be detected.
        let toml = r#"
[pack]
name = "test"
version = "1.0.0"
description = "Test"

[[servers]]
name = "api"
transport = "http"
url = "https://example.com/mcp"

[servers.headers]
X-Custom = "Bearer some-plaintext-token"
"#;
        let result = Pack::from_toml(toml, &PathBuf::from("test.toml"));
        assert!(
            result.is_err(),
            "X-Custom with secret value should be rejected"
        );
    }

    #[test]
    fn allow_bearer_env_var_in_authorization_header() {
        // Authorization: Bearer ${TOKEN} should be allowed (composite env var).
        let toml = r#"
[pack]
name = "test"
version = "1.0.0"
description = "Test"

[[servers]]
name = "api"
transport = "http"
url = "https://example.com/mcp"

[servers.headers]
Authorization = "Bearer ${TOKEN}"
"#;
        let result = Pack::from_toml(toml, &PathBuf::from("test.toml"));
        assert!(
            result.is_ok(),
            "Authorization with Bearer ${{TOKEN}} should pass: {:?}",
            result.err()
        );
    }

    // ── Schema versioning tests ──────────────────────────────────────

    #[test]
    fn parse_pack_with_explicit_schema_version_1() {
        let toml = r#"
schema_version = 1

[pack]
name = "test"
version = "1.0.0"
description = "Test"
"#;
        let result = Pack::from_toml(toml, &PathBuf::from("test.toml"));
        assert!(result.is_ok(), "explicit schema_version = 1 should work");
    }

    #[test]
    fn reject_pack_with_future_schema_version() {
        let toml = r#"
schema_version = 99

[pack]
name = "test"
version = "1.0.0"
description = "Test"
"#;
        let result = Pack::from_toml(toml, &PathBuf::from("test.toml"));
        assert!(result.is_err());
        let msg = result.unwrap_err().to_string();
        assert!(
            msg.contains("schema version 99"),
            "expected 'schema version 99' in error: {msg}"
        );
        assert!(
            msg.contains("please upgrade"),
            "expected 'please upgrade' in error: {msg}"
        );
    }

    #[test]
    fn parse_pack_without_schema_version_defaults_to_1() {
        let toml = r#"
[pack]
name = "test"
version = "1.0.0"
description = "Test"
"#;
        let result = Pack::from_toml(toml, &PathBuf::from("test.toml"));
        assert!(
            result.is_ok(),
            "missing schema_version should default to 1 and succeed"
        );
    }
}
