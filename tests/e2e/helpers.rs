use std::collections::HashMap;
use std::io::Write;
use std::path::PathBuf;

use sha2::{Digest, Sha256};
use tempfile::TempDir;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

// ── TestEnv ──────────────────────────────────────────────────────────────────

/// Self-contained test environment with fake HOME, store, project, and mock
/// registry server. All file-system operations are isolated in temp dirs.
pub struct TestEnv {
    pub home_dir: TempDir,
    pub store_dir: TempDir,
    pub project_dir: TempDir,
    pub mock_server: MockServer,
}

impl TestEnv {
    /// Create a fresh test environment.
    ///
    /// Pre-creates `~/.claude/`, `~/.gemini/`, and `~/.codex/` directories so
    /// that the CLI adapters report `is_installed() == true`.
    pub async fn new() -> Self {
        let home_dir = TempDir::new().expect("failed to create home temp dir");
        let store_dir = TempDir::new().expect("failed to create store temp dir");
        let project_dir = TempDir::new().expect("failed to create project temp dir");
        let mock_server = MockServer::start().await;

        // Pre-create adapter directories so adapters detect as installed.
        for subdir in &[".claude", ".gemini", ".codex"] {
            std::fs::create_dir_all(home_dir.path().join(subdir))
                .expect("failed to create adapter directory");
        }

        Self {
            home_dir,
            store_dir,
            project_dir,
            mock_server,
        }
    }

    /// Build an `assert_cmd::Command` for the `weave` binary, pre-configured
    /// with environment overrides that isolate all state in temp dirs.
    pub fn weave_cmd(&self) -> assert_cmd::Command {
        let mut cmd = assert_cmd::Command::new(env!("CARGO_BIN_EXE_weave"));
        cmd.env("HOME", self.home_dir.path())
            .env("WEAVE_TEST_STORE_DIR", self.store_dir.path())
            .env("WEAVE_REGISTRY_URL", self.mock_server.uri())
            .current_dir(self.project_dir.path());
        cmd
    }

    /// Path to the profile TOML for the given profile name.
    pub fn profile_toml(&self, name: &str) -> PathBuf {
        self.store_dir
            .path()
            .join("profiles")
            .join(format!("{name}.toml"))
    }

    /// Path to the lockfile for the given profile name.
    pub fn lockfile_path(&self, name: &str) -> PathBuf {
        self.store_dir
            .path()
            .join("locks")
            .join(format!("{name}.lock"))
    }

    /// Path to `~/.claude/` in the fake HOME.
    pub fn claude_dir(&self) -> PathBuf {
        self.home_dir.path().join(".claude")
    }

    /// Path to `~/.claude.json` (Claude Code user-scope MCP config).
    pub fn claude_json(&self) -> PathBuf {
        self.home_dir.path().join(".claude.json")
    }
}

// ── FixturePack ──────────────────────────────────────────────────────────────

/// Builder for a valid pack archive (tar.gz) with computed SHA256.
pub struct FixturePack {
    pub name: String,
    pub version: String,
    pub description: String,
    pub servers: Vec<(String, String, Vec<String>)>,
    pub prompt: Option<String>,
    pub commands: Vec<(String, String)>,
    pub dependencies: Vec<(String, String)>,
    pub archive_bytes: Vec<u8>,
    pub sha256: String,
}

impl FixturePack {
    /// Create a new fixture with sensible defaults.
    pub fn new(name: &str, version: &str) -> Self {
        Self {
            name: name.to_string(),
            version: version.to_string(),
            description: format!("A test pack: {name}"),
            servers: Vec::new(),
            prompt: None,
            commands: Vec::new(),
            dependencies: Vec::new(),
            archive_bytes: Vec::new(),
            sha256: String::new(),
        }
    }

    /// Add an MCP server definition.
    pub fn with_server(mut self, name: &str, command: &str, args: &[&str]) -> Self {
        self.servers.push((
            name.to_string(),
            command.to_string(),
            args.iter().map(|s| s.to_string()).collect(),
        ));
        self
    }

    /// Set the system prompt content.
    pub fn with_prompt(mut self, content: &str) -> Self {
        self.prompt = Some(content.to_string());
        self
    }

    /// Add a slash command.
    pub fn with_command(mut self, name: &str, content: &str) -> Self {
        self.commands.push((name.to_string(), content.to_string()));
        self
    }

    /// Add a dependency on another pack.
    pub fn with_dependency(mut self, name: &str, version_req: &str) -> Self {
        self.dependencies
            .push((name.to_string(), version_req.to_string()));
        self
    }

    /// Build the tar.gz archive and compute its SHA256.
    pub fn build(mut self) -> Self {
        let mut files: Vec<(String, Vec<u8>)> = Vec::new();

        // Generate pack.toml
        let pack_toml = self.generate_pack_toml();
        files.push(("pack.toml".to_string(), pack_toml.into_bytes()));

        // System prompt
        if let Some(ref prompt) = self.prompt {
            files.push(("prompts/system.md".to_string(), prompt.clone().into_bytes()));
        }

        // Slash commands
        for (name, content) in &self.commands {
            files.push((format!("commands/{name}.md"), content.clone().into_bytes()));
        }

        // Build tar
        let mut tar_bytes = Vec::new();
        {
            let mut builder = tar::Builder::new(&mut tar_bytes);
            for (path, content) in &files {
                let mut header = tar::Header::new_gnu();
                header.set_size(content.len() as u64);
                header.set_mode(0o644);
                header.set_cksum();
                builder
                    .append_data(&mut header, path, content.as_slice())
                    .expect("failed to append tar entry");
            }
            builder.finish().expect("failed to finish tar archive");
        }

        // Compress with gzip
        let mut gz_bytes = Vec::new();
        {
            let mut encoder =
                flate2::write::GzEncoder::new(&mut gz_bytes, flate2::Compression::default());
            encoder.write_all(&tar_bytes).expect("failed to write gzip");
            encoder.finish().expect("failed to finish gzip");
        }

        // Compute SHA256
        let mut hasher = Sha256::new();
        hasher.update(&gz_bytes);
        let sha256 = format!("{:x}", hasher.finalize());

        self.archive_bytes = gz_bytes;
        self.sha256 = sha256;
        self
    }

    /// Generate a valid `pack.toml` string matching the Pack struct format.
    fn generate_pack_toml(&self) -> String {
        let mut toml = String::new();

        toml.push_str("[pack]\n");
        toml.push_str(&format!("name = \"{}\"\n", self.name));
        toml.push_str(&format!("version = \"{}\"\n", self.version));
        toml.push_str(&format!("description = \"{}\"\n", self.description));
        toml.push_str("authors = [\"test-author\"]\n");

        if !self.servers.is_empty() {
            for (name, command, args) in &self.servers {
                toml.push_str("\n[[servers]]\n");
                toml.push_str(&format!("name = \"{name}\"\n"));
                toml.push_str(&format!("command = \"{command}\"\n"));
                let args_str: Vec<String> = args.iter().map(|a| format!("\"{a}\"")).collect();
                toml.push_str(&format!("args = [{}]\n", args_str.join(", ")));
            }
        }

        if !self.dependencies.is_empty() {
            toml.push_str("\n[dependencies]\n");
            for (name, ver_req) in &self.dependencies {
                toml.push_str(&format!("{name} = \"{ver_req}\"\n"));
            }
        }

        toml
    }
}

// ── Mock registry helpers ────────────────────────────────────────────────────

/// Mount a mock registry index and pack archives on the given `MockServer`.
///
/// The registry index is served at `GET /` (since `WEAVE_REGISTRY_URL` points
/// to the server root, and `GitHubRegistry` fetches that URL directly).
/// Each pack archive is served at `GET /packs/<name>-<version>.tar.gz`.
pub async fn mount_registry(server: &MockServer, packs: &[&FixturePack]) {
    // Build the RegistryIndex JSON.
    // Format: a flat JSON object with pack names as keys, each containing
    // PackMetadata { name, description, authors, license, repository, versions }.
    let mut index: HashMap<String, serde_json::Value> = HashMap::new();

    for pack in packs {
        let download_url = format!(
            "{}/packs/{}-{}.tar.gz",
            server.uri(),
            pack.name,
            pack.version
        );

        let deps: HashMap<String, String> = pack.dependencies.iter().cloned().collect();

        let release = serde_json::json!({
            "version": pack.version,
            "url": download_url,
            "sha256": pack.sha256,
            "dependencies": deps,
        });

        let metadata = serde_json::json!({
            "name": pack.name,
            "description": pack.description,
            "authors": ["test-author"],
            "versions": [release],
        });

        index.insert(pack.name.clone(), metadata);
    }

    let index_json = serde_json::to_string(&index).expect("failed to serialize index");

    // Mount index at GET /
    Mock::given(method("GET"))
        .and(path("/"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_string(index_json)
                .insert_header("content-type", "application/json"),
        )
        .mount(server)
        .await;

    // Mount each pack archive at its download path.
    for pack in packs {
        let archive_path = format!("/packs/{}-{}.tar.gz", pack.name, pack.version);
        Mock::given(method("GET"))
            .and(path(&archive_path))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_bytes(pack.archive_bytes.clone())
                    .insert_header("content-type", "application/octet-stream"),
            )
            .mount(server)
            .await;
    }
}

/// Mount a mock registry where multiple versions of the same pack are available.
///
/// Unlike `mount_registry` (which creates one version per pack entry), this
/// groups packs by name and puts all versions into a single `PackMetadata.versions`
/// array, which is what the resolver needs to find newer versions during update.
pub async fn mount_registry_multi_version(server: &MockServer, packs: &[&FixturePack]) {
    // Group packs by name, preserving insertion order of first occurrence.
    let mut index: HashMap<String, serde_json::Value> = HashMap::new();
    let mut versions_map: HashMap<String, Vec<serde_json::Value>> = HashMap::new();
    let mut meta_map: HashMap<String, (String, String)> = HashMap::new();

    for pack in packs {
        let download_url = format!(
            "{}/packs/{}-{}.tar.gz",
            server.uri(),
            pack.name,
            pack.version
        );

        let deps: HashMap<String, String> = pack.dependencies.iter().cloned().collect();

        let release = serde_json::json!({
            "version": pack.version,
            "url": download_url,
            "sha256": pack.sha256,
            "dependencies": deps,
        });

        versions_map
            .entry(pack.name.clone())
            .or_default()
            .push(release);

        meta_map
            .entry(pack.name.clone())
            .or_insert_with(|| (pack.name.clone(), pack.description.clone()));
    }

    for (name, versions) in &versions_map {
        let (ref pack_name, ref desc) = meta_map[name];
        let metadata = serde_json::json!({
            "name": pack_name,
            "description": desc,
            "authors": ["test-author"],
            "versions": versions,
        });
        index.insert(name.clone(), metadata);
    }

    let index_json = serde_json::to_string(&index).expect("failed to serialize index");

    Mock::given(method("GET"))
        .and(path("/"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_string(index_json)
                .insert_header("content-type", "application/json"),
        )
        .mount(server)
        .await;

    // Mount each pack archive at its download path.
    for pack in packs {
        let archive_path = format!("/packs/{}-{}.tar.gz", pack.name, pack.version);
        Mock::given(method("GET"))
            .and(path(&archive_path))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_bytes(pack.archive_bytes.clone())
                    .insert_header("content-type", "application/octet-stream"),
            )
            .mount(server)
            .await;
    }
}
