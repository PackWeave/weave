/// Integration tests for the Codex CLI adapter.
///
/// These tests use `TempDir` for the simulated Codex home directory and never
/// write to the real `~/.codex/`. Tests that exercise prompts or settings also
/// create temporary pack entries under the isolated test store via `StoreFixture`.
use std::collections::HashMap;
use std::path::PathBuf;

use packweave::adapters::codex_cli::CodexAdapter;
use packweave::adapters::CliAdapter;
use packweave::core::pack::{
    EnvVar, McpServer, Pack, PackSource, PackTargets, ResolvedPack, Transport,
};
use packweave::core::store::Store;
use tempfile::TempDir;

// ── Helpers ───────────────────────────────────────────────────────────────────

fn make_adapter(home: &TempDir) -> CodexAdapter {
    // Use a sub-path as project root so no `.codex/` dir exists there by default.
    let no_project = home.path().join("no-project");
    std::fs::create_dir_all(&no_project).unwrap();
    CodexAdapter::with_home_and_project(home.path().to_path_buf(), no_project)
}

fn make_adapter_with_project(home: &TempDir, project: &TempDir) -> CodexAdapter {
    CodexAdapter::with_home_and_project(home.path().to_path_buf(), project.path().to_path_buf())
}

/// Create `~/.codex/` inside the temp home.
fn setup_codex_home(home: &TempDir) {
    std::fs::create_dir_all(home.path().join(".codex")).unwrap();
}

/// Create `.codex/` inside the temp project root.
fn setup_project_codex_dir(project: &TempDir) {
    std::fs::create_dir_all(project.path().join(".codex")).unwrap();
}

fn read_toml(path: &std::path::Path) -> toml::Value {
    let content = std::fs::read_to_string(path).expect("file should exist");
    toml::from_str(&content).expect("file should be valid TOML")
}

fn read_json(path: &std::path::Path) -> serde_json::Value {
    let content = std::fs::read_to_string(path).expect("file should exist");
    serde_json::from_str(&content).expect("file should be valid JSON")
}

// ── Pack builders ─────────────────────────────────────────────────────────────

fn pack_with_servers(name: &str, servers: Vec<McpServer>) -> ResolvedPack {
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
            servers,
            dependencies: HashMap::new(),
            extensions: Default::default(),
            targets: PackTargets::default(),
        },
        source: PackSource::Registry {
            registry_url: "https://example.com".into(),
        },
    }
}

fn simple_server(name: &str) -> McpServer {
    McpServer {
        name: name.to_string(),
        package_type: None,
        package: None,
        command: Some("npx".into()),
        args: vec!["-y".into(), name.to_string()],
        url: None,
        headers: None,
        transport: Some(Transport::Stdio),
        namespace: None,
        tools: vec![],
        env: HashMap::new(),
    }
}

fn server_with_env(name: &str, env_keys: &[&str]) -> McpServer {
    let mut env = HashMap::new();
    for key in env_keys {
        env.insert(
            key.to_string(),
            EnvVar {
                required: true,
                secret: true,
                description: None,
            },
        );
    }
    McpServer {
        name: name.to_string(),
        package_type: None,
        package: None,
        command: Some("npx".into()),
        args: vec!["-y".into(), name.to_string()],
        url: None,
        headers: None,
        transport: Some(Transport::Stdio),
        namespace: None,
        tools: vec![],
        env,
    }
}

fn pack_not_targeting_codex(name: &str) -> ResolvedPack {
    ResolvedPack {
        pack: Pack {
            name: name.to_string(),
            version: semver::Version::new(1, 0, 0),
            description: "Non-codex pack".into(),
            authors: vec![],
            license: None,
            repository: None,
            keywords: vec![],
            min_tool_version: None,
            servers: vec![simple_server("some-server")],
            dependencies: HashMap::new(),
            extensions: Default::default(),
            targets: PackTargets {
                claude_code: true,
                gemini_cli: true,
                codex_cli: false,
            },
        },
        source: PackSource::Registry {
            registry_url: "https://example.com".into(),
        },
    }
}

// ── StoreFixture ──────────────────────────────────────────────────────────────

use std::sync::OnceLock;

fn shared_store_root() -> &'static TempDir {
    static STORE: OnceLock<TempDir> = OnceLock::new();
    STORE.get_or_init(|| {
        let dir = TempDir::new().expect("shared store TempDir");
        // Unconditionally set so Store::pack_dir() always resolves under this TempDir,
        // regardless of any pre-existing env var from the test runner.
        std::env::set_var("WEAVE_TEST_STORE_DIR", dir.path());
        dir
    })
}

struct StoreFixture {
    pack_dir: PathBuf,
}

impl StoreFixture {
    fn create(name: &str, prompt: Option<&str>, settings_toml: Option<&str>) -> Self {
        let store = shared_store_root();

        let version = semver::Version::new(1, 0, 0);
        let pack_dir = Store::pack_dir(name, &version).expect("store root must be determinable");

        assert!(
            pack_dir.starts_with(store.path()),
            "pack_dir {pack_dir:?} is not inside the isolated store root {:?}",
            store.path()
        );

        std::fs::create_dir_all(&pack_dir).unwrap();

        std::fs::write(
            pack_dir.join("pack.toml"),
            format!("[pack]\nname = \"{name}\"\nversion = \"1.0.0\"\ndescription = \"fixture\"\n"),
        )
        .unwrap();

        if let Some(content) = prompt {
            std::fs::create_dir_all(pack_dir.join("prompts")).unwrap();
            std::fs::write(pack_dir.join("prompts/codex.md"), content).unwrap();
        }

        if let Some(content) = settings_toml {
            std::fs::create_dir_all(pack_dir.join("settings")).unwrap();
            std::fs::write(pack_dir.join("settings/codex.toml"), content).unwrap();
        }

        Self { pack_dir }
    }

    fn create_with_skills(name: &str, skills: &[(&str, &str)]) -> Self {
        let store = shared_store_root();

        let version = semver::Version::new(1, 0, 0);
        let pack_dir = Store::pack_dir(name, &version).expect("store root must be determinable");

        assert!(
            pack_dir.starts_with(store.path()),
            "pack_dir {pack_dir:?} is not inside the isolated store root {:?}",
            store.path()
        );

        std::fs::create_dir_all(&pack_dir).unwrap();
        std::fs::write(
            pack_dir.join("pack.toml"),
            format!("[pack]\nname = \"{name}\"\nversion = \"1.0.0\"\ndescription = \"fixture\"\n"),
        )
        .unwrap();

        if !skills.is_empty() {
            let skills_dir = pack_dir.join("skills");
            std::fs::create_dir_all(&skills_dir).unwrap();
            for (filename, content) in skills {
                std::fs::write(skills_dir.join(filename), content).unwrap();
            }
        }

        Self { pack_dir }
    }
}

impl Drop for StoreFixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.pack_dir);
    }
}

// ── Helper to get the resolved pack for fixture packs ─────────────────────────

fn pack_for_fixture(name: &str) -> ResolvedPack {
    ResolvedPack {
        pack: Pack {
            name: name.to_string(),
            version: semver::Version::new(1, 0, 0),
            description: "Fixture pack".into(),
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

// ── apply: MCP servers ────────────────────────────────────────────────────────

#[test]
fn apply_writes_server_to_config_toml() {
    let home = TempDir::new().unwrap();
    setup_codex_home(&home);
    let adapter = make_adapter(&home);

    let pack = pack_with_servers("mcp-pack", vec![simple_server("my-server")]);
    adapter.apply(&pack).unwrap();

    let config_path = home.path().join(".codex/config.toml");
    assert!(config_path.exists(), "config.toml should be created");
    let config = read_toml(&config_path);
    assert!(
        config["mcp_servers"]["my-server"].is_table(),
        "server entry should be written"
    );
    assert_eq!(
        config["mcp_servers"]["my-server"]["command"]
            .as_str()
            .unwrap(),
        "npx",
        "command should be set"
    );
}

#[test]
fn apply_writes_server_args() {
    let home = TempDir::new().unwrap();
    setup_codex_home(&home);
    let adapter = make_adapter(&home);

    let server = McpServer {
        name: "arg-server".into(),
        package_type: None,
        package: None,
        command: Some("node".into()),
        args: vec!["--flag".into(), "value".into()],
        url: None,
        headers: None,
        transport: Some(Transport::Stdio),
        namespace: None,
        tools: vec![],
        env: HashMap::new(),
    };
    let pack = pack_with_servers("arg-pack", vec![server]);
    adapter.apply(&pack).unwrap();

    let config_path = home.path().join(".codex/config.toml");
    let config = read_toml(&config_path);
    let args = config["mcp_servers"]["arg-server"]["args"]
        .as_array()
        .unwrap();
    assert_eq!(args[0].as_str().unwrap(), "--flag");
    assert_eq!(args[1].as_str().unwrap(), "value");
}

#[test]
fn apply_sets_enabled_true() {
    let home = TempDir::new().unwrap();
    setup_codex_home(&home);
    let adapter = make_adapter(&home);

    let pack = pack_with_servers("enabled-pack", vec![simple_server("enabled-server")]);
    adapter.apply(&pack).unwrap();

    let config_path = home.path().join(".codex/config.toml");
    let config = read_toml(&config_path);
    assert_eq!(
        config["mcp_servers"]["enabled-server"]["enabled"]
            .as_bool()
            .unwrap(),
        true,
        "enabled should be set to true"
    );
}

#[test]
fn apply_writes_env_vars_as_references() {
    let home = TempDir::new().unwrap();
    setup_codex_home(&home);
    let adapter = make_adapter(&home);

    let pack = pack_with_servers(
        "env-pack",
        vec![server_with_env("env-server", &["API_KEY", "TOKEN"])],
    );
    adapter.apply(&pack).unwrap();

    let config_path = home.path().join(".codex/config.toml");
    let config = read_toml(&config_path);
    let env = &config["mcp_servers"]["env-server"]["env"];
    assert!(env.is_table(), "env key should be present");
    assert_eq!(
        env["API_KEY"].as_str().unwrap(),
        "${API_KEY}",
        "env var should be written as a reference"
    );
    assert_eq!(
        env["TOKEN"].as_str().unwrap(),
        "${TOKEN}",
        "env var should be written as a reference"
    );
}

#[test]
fn apply_omits_env_when_server_has_no_env_vars() {
    let home = TempDir::new().unwrap();
    setup_codex_home(&home);
    let adapter = make_adapter(&home);

    let pack = pack_with_servers("no-env-pack", vec![simple_server("no-env-server")]);
    adapter.apply(&pack).unwrap();

    let config_path = home.path().join(".codex/config.toml");
    let config = read_toml(&config_path);
    assert!(
        config["mcp_servers"]["no-env-server"].is_table(),
        "server must be written"
    );
    assert!(
        config["mcp_servers"]["no-env-server"]
            .as_table()
            .unwrap()
            .get("env")
            .is_none(),
        "env key must not be written when server has no env vars"
    );
}

#[test]
fn apply_writes_url_for_http_transport() {
    let home = TempDir::new().unwrap();
    setup_codex_home(&home);
    let adapter = make_adapter(&home);

    let server = McpServer {
        name: "http-server".into(),
        package_type: None,
        package: None,
        command: None,
        args: vec![],
        url: Some("https://example.com/mcp".into()),
        headers: None,
        transport: Some(Transport::Http),
        namespace: None,
        tools: vec![],
        env: HashMap::new(),
    };
    let pack = pack_with_servers("http-pack", vec![server]);
    adapter.apply(&pack).unwrap();

    let config_path = home.path().join(".codex/config.toml");
    let config = read_toml(&config_path);
    assert_eq!(
        config["mcp_servers"]["http-server"]["url"]
            .as_str()
            .unwrap(),
        "https://example.com/mcp",
        "HTTP server should use url field"
    );
    // command key should not be present for HTTP transport
    assert!(
        config["mcp_servers"]["http-server"]
            .as_table()
            .unwrap()
            .get("command")
            .is_none(),
        "command key should not be present for HTTP transport"
    );
}

#[test]
fn apply_preserves_existing_user_servers() {
    let home = TempDir::new().unwrap();
    setup_codex_home(&home);
    let adapter = make_adapter(&home);

    // Pre-populate config.toml with a user-managed server.
    let config_path = home.path().join(".codex/config.toml");
    std::fs::write(
        &config_path,
        "[mcp_servers.user-server]\ncommand = \"user-cmd\"\nenabled = true\n",
    )
    .unwrap();

    let pack = pack_with_servers("new-pack", vec![simple_server("new-server")]);
    adapter.apply(&pack).unwrap();

    let config = read_toml(&config_path);
    assert!(
        config["mcp_servers"]["user-server"].is_table(),
        "user server must not be removed"
    );
    assert!(
        config["mcp_servers"]["new-server"].is_table(),
        "pack server must be added"
    );
}

#[test]
fn apply_rejects_collision_with_user_server() {
    let home = TempDir::new().unwrap();
    setup_codex_home(&home);
    let adapter = make_adapter(&home);

    let config_path = home.path().join(".codex/config.toml");
    std::fs::write(
        &config_path,
        "[mcp_servers.clash-server]\ncommand = \"user-cmd\"\nenabled = true\n",
    )
    .unwrap();

    let pack = pack_with_servers("clash-pack", vec![simple_server("clash-server")]);
    let result = adapter.apply(&pack);
    assert!(result.is_err(), "should fail when a user server collides");
    let msg = result.unwrap_err().to_string();
    assert!(
        msg.contains("clash-server"),
        "error should name the conflicting server"
    );
}

#[test]
fn apply_skips_pack_not_targeting_codex() {
    let home = TempDir::new().unwrap();
    setup_codex_home(&home);
    let adapter = make_adapter(&home);

    let pack = pack_not_targeting_codex("other-cli-pack");
    adapter.apply(&pack).unwrap();

    let config_path = home.path().join(".codex/config.toml");
    assert!(
        !config_path.exists(),
        "config.toml should not be created for non-codex pack"
    );
}

#[test]
fn apply_writes_multiple_servers() {
    let home = TempDir::new().unwrap();
    setup_codex_home(&home);
    let adapter = make_adapter(&home);

    let pack = pack_with_servers(
        "multi-pack",
        vec![simple_server("server-a"), simple_server("server-b")],
    );
    adapter.apply(&pack).unwrap();

    let config_path = home.path().join(".codex/config.toml");
    let config = read_toml(&config_path);
    assert!(config["mcp_servers"]["server-a"].is_table());
    assert!(config["mcp_servers"]["server-b"].is_table());
}

// ── apply: manifest tracking ──────────────────────────────────────────────────

#[test]
fn apply_writes_manifest() {
    let home = TempDir::new().unwrap();
    setup_codex_home(&home);
    let adapter = make_adapter(&home);

    let pack = pack_with_servers("tracked-pack", vec![simple_server("tracked-server")]);
    adapter.apply(&pack).unwrap();

    let manifest_path = home.path().join(".codex/.packweave_manifest.json");
    assert!(manifest_path.exists(), "manifest file should be created");
    let manifest = read_json(&manifest_path);
    assert_eq!(
        manifest["servers"]["tracked-server"], "tracked-pack",
        "manifest should track server ownership"
    );
}

// ── remove: servers ───────────────────────────────────────────────────────────

#[test]
fn remove_deletes_server_from_config_toml() {
    let home = TempDir::new().unwrap();
    setup_codex_home(&home);
    let adapter = make_adapter(&home);

    let pack = pack_with_servers("rm-pack", vec![simple_server("rm-server")]);
    adapter.apply(&pack).unwrap();
    adapter.remove("rm-pack").unwrap();

    let config_path = home.path().join(".codex/config.toml");
    let config = read_toml(&config_path);
    assert!(
        config["mcp_servers"]
            .as_table()
            .map(|t| !t.contains_key("rm-server"))
            .unwrap_or(true),
        "server should be removed"
    );
}

#[test]
fn remove_is_surgical_leaves_other_servers() {
    let home = TempDir::new().unwrap();
    setup_codex_home(&home);
    let adapter = make_adapter(&home);

    let pack_a = pack_with_servers("pack-a", vec![simple_server("server-a")]);
    let pack_b = pack_with_servers("pack-b", vec![simple_server("server-b")]);
    adapter.apply(&pack_a).unwrap();
    adapter.apply(&pack_b).unwrap();

    adapter.remove("pack-a").unwrap();

    let config_path = home.path().join(".codex/config.toml");
    let config = read_toml(&config_path);
    assert!(
        config["mcp_servers"]
            .as_table()
            .map(|t| !t.contains_key("server-a"))
            .unwrap_or(true),
        "pack-a server should be removed"
    );
    assert!(
        config["mcp_servers"]["server-b"].is_table(),
        "pack-b server should remain"
    );
}

#[test]
fn remove_unknown_pack_is_a_no_op() {
    let home = TempDir::new().unwrap();
    setup_codex_home(&home);
    let adapter = make_adapter(&home);

    // Should not error even if the pack was never installed.
    adapter.remove("nonexistent-pack").unwrap();
}

#[test]
fn remove_clears_manifest_entry() {
    let home = TempDir::new().unwrap();
    setup_codex_home(&home);
    let adapter = make_adapter(&home);

    let pack = pack_with_servers("clear-pack", vec![simple_server("clear-server")]);
    adapter.apply(&pack).unwrap();
    adapter.remove("clear-pack").unwrap();

    let manifest_path = home.path().join(".codex/.packweave_manifest.json");
    let manifest = read_json(&manifest_path);
    assert!(
        manifest["servers"]
            .as_object()
            .map(|m| !m.contains_key("clear-server"))
            .unwrap_or(true),
        "manifest should not track removed server"
    );
}

// ── idempotency ───────────────────────────────────────────────────────────────

#[test]
fn apply_twice_is_idempotent_for_servers() {
    let home = TempDir::new().unwrap();
    setup_codex_home(&home);
    let adapter = make_adapter(&home);

    let pack = pack_with_servers("idem-pack", vec![simple_server("idem-server")]);
    adapter.apply(&pack).unwrap();
    adapter.apply(&pack).unwrap(); // second apply must not error

    let config_path = home.path().join(".codex/config.toml");
    let config = read_toml(&config_path);

    let mcp = config["mcp_servers"].as_table().unwrap();
    assert!(
        mcp.contains_key("idem-server"),
        "server should still be present after two applies"
    );
    assert_eq!(
        mcp.iter()
            .filter(|(k, _)| k.as_str() == "idem-server")
            .count(),
        1,
        "server should appear exactly once"
    );
}

// ── project-scope: servers ────────────────────────────────────────────────────

#[test]
fn apply_writes_server_to_project_config_toml_when_project_scope_active() {
    let home = TempDir::new().unwrap();
    let project = TempDir::new().unwrap();
    setup_codex_home(&home);
    setup_project_codex_dir(&project);

    let adapter = make_adapter_with_project(&home, &project);

    let pack = pack_with_servers("proj-pack", vec![simple_server("proj-server")]);
    adapter.apply(&pack).unwrap();

    // User-scope config.toml should have the server
    let user_config = home.path().join(".codex/config.toml");
    assert!(user_config.exists(), "user config.toml should exist");
    let user_toml = read_toml(&user_config);
    assert!(user_toml["mcp_servers"]["proj-server"].is_table());

    // Project-scope config.toml should also have the server
    let proj_config = project.path().join(".codex/config.toml");
    assert!(proj_config.exists(), "project config.toml should exist");
    let proj_toml = read_toml(&proj_config);
    assert!(proj_toml["mcp_servers"]["proj-server"].is_table());
}

#[test]
fn apply_does_not_create_project_config_when_no_project_scope() {
    let home = TempDir::new().unwrap();
    setup_codex_home(&home);
    let adapter = make_adapter(&home);

    let pack = pack_with_servers("noproj-pack", vec![simple_server("noproj-server")]);
    adapter.apply(&pack).unwrap();

    // project root has no .codex/ dir so project config.toml should not be created
    let project_no_codex = home.path().join("no-project/.codex/config.toml");
    assert!(
        !project_no_codex.exists(),
        "project config.toml should not be created when project scope is inactive"
    );
}

#[test]
fn remove_removes_from_project_config_toml() {
    let home = TempDir::new().unwrap();
    let project = TempDir::new().unwrap();
    setup_codex_home(&home);
    setup_project_codex_dir(&project);

    let adapter = make_adapter_with_project(&home, &project);

    let pack = pack_with_servers("rm-proj-pack", vec![simple_server("rm-proj-server")]);
    adapter.apply(&pack).unwrap();
    adapter.remove("rm-proj-pack").unwrap();

    let proj_config = project.path().join(".codex/config.toml");
    if proj_config.exists() {
        let config = read_toml(&proj_config);
        assert!(
            config["mcp_servers"]
                .as_table()
                .map(|t| !t.contains_key("rm-proj-server"))
                .unwrap_or(true),
            "server should be removed from project config.toml"
        );
    }
}

// ── skills ────────────────────────────────────────────────────────────────────

#[test]
fn apply_installs_skill_files() {
    let home = TempDir::new().unwrap();
    setup_codex_home(&home);
    let adapter = make_adapter(&home);

    let _fixture = StoreFixture::create_with_skills(
        "skill-pack",
        &[
            ("my-skill.md", "# My Skill\nDo something useful."),
            ("another.md", "# Another\nAnother skill."),
        ],
    );

    let pack = pack_for_fixture("skill-pack");
    adapter.apply(&pack).unwrap();

    let skills_dir = home.path().join(".codex/skills");
    assert!(skills_dir.exists(), "skills dir should be created");
    assert!(
        skills_dir.join("skill-pack__my-skill.md").exists(),
        "namespaced skill file should exist"
    );
    assert!(
        skills_dir.join("skill-pack__another.md").exists(),
        "second skill file should exist"
    );
}

#[test]
fn apply_skill_content_is_preserved() {
    let home = TempDir::new().unwrap();
    setup_codex_home(&home);
    let adapter = make_adapter(&home);

    let _fixture = StoreFixture::create_with_skills(
        "content-pack",
        &[("tool.md", "# My Tool\nThis is the skill content.")],
    );

    let pack = pack_for_fixture("content-pack");
    adapter.apply(&pack).unwrap();

    let skill_path = home.path().join(".codex/skills/content-pack__tool.md");
    let content = std::fs::read_to_string(&skill_path).unwrap();
    assert!(
        content.contains("This is the skill content."),
        "skill content should be preserved"
    );
}

#[test]
fn remove_deletes_skill_files() {
    let home = TempDir::new().unwrap();
    setup_codex_home(&home);
    let adapter = make_adapter(&home);

    let _fixture =
        StoreFixture::create_with_skills("del-skill-pack", &[("task.md", "# Task\nDo this.")]);

    let pack = pack_for_fixture("del-skill-pack");
    adapter.apply(&pack).unwrap();

    let skill_path = home.path().join(".codex/skills/del-skill-pack__task.md");
    assert!(skill_path.exists(), "skill file should exist after apply");

    adapter.remove("del-skill-pack").unwrap();
    assert!(
        !skill_path.exists(),
        "skill file should be removed after remove"
    );
}

#[test]
fn apply_skills_tracks_manifest() {
    let home = TempDir::new().unwrap();
    setup_codex_home(&home);
    let adapter = make_adapter(&home);

    let _fixture = StoreFixture::create_with_skills("track-skill-pack", &[("task.md", "# Task")]);

    let pack = pack_for_fixture("track-skill-pack");
    adapter.apply(&pack).unwrap();

    let manifest_path = home.path().join(".codex/.packweave_manifest.json");
    let manifest = read_json(&manifest_path);
    assert_eq!(
        manifest["skills"]["track-skill-pack__task.md"], "track-skill-pack",
        "skill should be tracked in manifest"
    );
}

// ── prompts (AGENTS.md) ───────────────────────────────────────────────────────

#[test]
fn apply_appends_prompt_to_agents_md() {
    let home = TempDir::new().unwrap();
    setup_codex_home(&home);
    let adapter = make_adapter(&home);

    let _fixture = StoreFixture::create("prompt-pack", Some("Be helpful and concise."), None);

    let pack = pack_for_fixture("prompt-pack");
    adapter.apply(&pack).unwrap();

    let agents_md = home.path().join(".codex/AGENTS.md");
    assert!(agents_md.exists(), "AGENTS.md should be created");
    let content = std::fs::read_to_string(&agents_md).unwrap();
    assert!(content.contains("<!-- packweave:begin:prompt-pack -->"));
    assert!(content.contains("Be helpful and concise."));
    assert!(content.contains("<!-- packweave:end:prompt-pack -->"));
}

#[test]
fn apply_prompt_is_idempotent() {
    let home = TempDir::new().unwrap();
    setup_codex_home(&home);
    let adapter = make_adapter(&home);

    let _fixture = StoreFixture::create("idem-prompt-pack", Some("Idempotent prompt."), None);
    let pack = pack_for_fixture("idem-prompt-pack");

    adapter.apply(&pack).unwrap();
    adapter.apply(&pack).unwrap();

    let agents_md = home.path().join(".codex/AGENTS.md");
    let content = std::fs::read_to_string(&agents_md).unwrap();
    let count = content
        .matches("<!-- packweave:begin:idem-prompt-pack -->")
        .count();
    assert_eq!(count, 1, "prompt block should appear exactly once");
}

#[test]
fn remove_deletes_prompt_block_from_agents_md() {
    let home = TempDir::new().unwrap();
    setup_codex_home(&home);
    let adapter = make_adapter(&home);

    let _fixture = StoreFixture::create("rm-prompt-pack", Some("Remove me."), None);
    let pack = pack_for_fixture("rm-prompt-pack");

    adapter.apply(&pack).unwrap();
    adapter.remove("rm-prompt-pack").unwrap();

    let agents_md = home.path().join(".codex/AGENTS.md");
    if agents_md.exists() {
        let content = std::fs::read_to_string(&agents_md).unwrap();
        assert!(
            !content.contains("<!-- packweave:begin:rm-prompt-pack -->"),
            "prompt block should be removed"
        );
    }
}

#[test]
fn remove_prompt_leaves_other_blocks() {
    let home = TempDir::new().unwrap();
    setup_codex_home(&home);
    let adapter = make_adapter(&home);

    let _fix_a = StoreFixture::create("prompt-a", Some("Prompt A content."), None);
    let _fix_b = StoreFixture::create("prompt-b", Some("Prompt B content."), None);

    adapter.apply(&pack_for_fixture("prompt-a")).unwrap();
    adapter.apply(&pack_for_fixture("prompt-b")).unwrap();
    adapter.remove("prompt-a").unwrap();

    let agents_md = home.path().join(".codex/AGENTS.md");
    let content = std::fs::read_to_string(&agents_md).unwrap();
    assert!(!content.contains("<!-- packweave:begin:prompt-a -->"));
    assert!(content.contains("<!-- packweave:begin:prompt-b -->"));
}

// ── settings (config.toml top-level keys) ─────────────────────────────────────

#[test]
fn apply_merges_settings_into_config_toml() {
    let home = TempDir::new().unwrap();
    setup_codex_home(&home);
    let adapter = make_adapter(&home);

    let _fixture = StoreFixture::create(
        "settings-pack",
        None,
        Some("model = \"o3-mini\"\napproval_policy = \"auto-edit\"\n"),
    );

    let pack = pack_for_fixture("settings-pack");
    adapter.apply(&pack).unwrap();

    let config_path = home.path().join(".codex/config.toml");
    let config = read_toml(&config_path);
    assert_eq!(
        config["model"].as_str().unwrap(),
        "o3-mini",
        "model setting should be merged"
    );
    assert_eq!(
        config["approval_policy"].as_str().unwrap(),
        "auto-edit",
        "approval_policy setting should be merged"
    );
}

#[test]
fn remove_restores_settings_from_config_toml() {
    let home = TempDir::new().unwrap();
    setup_codex_home(&home);
    let adapter = make_adapter(&home);

    let _fixture = StoreFixture::create("rm-settings-pack", None, Some("model = \"o3\"\n"));

    let pack = pack_for_fixture("rm-settings-pack");
    adapter.apply(&pack).unwrap();
    adapter.remove("rm-settings-pack").unwrap();

    let config_path = home.path().join(".codex/config.toml");
    if config_path.exists() {
        let config = read_toml(&config_path);
        assert!(
            config
                .as_table()
                .map(|t| !t.contains_key("model"))
                .unwrap_or(true),
            "model key should be removed when pack is removed"
        );
    }
}

#[test]
fn settings_mcp_servers_key_is_ignored() {
    let home = TempDir::new().unwrap();
    setup_codex_home(&home);
    let adapter = make_adapter(&home);

    // The settings file includes mcp_servers which should be silently ignored.
    // Note: model = "o3" must appear before [mcp_servers.bad] to be a top-level key.
    let _fixture = StoreFixture::create(
        "bad-settings-pack",
        None,
        Some("model = \"o3\"\n\n[mcp_servers.bad]\ncommand = \"evil\"\n"),
    );

    let pack = pack_for_fixture("bad-settings-pack");
    adapter.apply(&pack).unwrap();

    let config_path = home.path().join(".codex/config.toml");
    let config = read_toml(&config_path);
    // The mcp_servers key from settings should NOT have been applied.
    let mcp_has_bad = config
        .as_table()
        .and_then(|t| t.get("mcp_servers"))
        .and_then(|v| v.as_table())
        .map(|t| t.contains_key("bad"))
        .unwrap_or(false);
    assert!(!mcp_has_bad, "mcp_servers from settings should be ignored");
    // But model should be applied normally.
    assert_eq!(config["model"].as_str().unwrap(), "o3");
}

// ── diagnose ──────────────────────────────────────────────────────────────────

#[test]
fn diagnose_returns_no_issues_for_clean_state() {
    let home = TempDir::new().unwrap();
    setup_codex_home(&home);
    let adapter = make_adapter(&home);

    let pack = pack_with_servers("clean-pack", vec![simple_server("clean-server")]);
    adapter.apply(&pack).unwrap();

    let issues = adapter.diagnose().unwrap();
    assert!(issues.is_empty(), "should be no issues for a clean state");
}

#[test]
fn diagnose_reports_missing_server() {
    let home = TempDir::new().unwrap();
    setup_codex_home(&home);
    let adapter = make_adapter(&home);

    let pack = pack_with_servers("diag-pack", vec![simple_server("diag-server")]);
    adapter.apply(&pack).unwrap();

    // Manually remove the server from config.toml to simulate drift.
    let config_path = home.path().join(".codex/config.toml");
    std::fs::write(&config_path, "# empty\n").unwrap();

    let issues = adapter.diagnose().unwrap();
    assert!(!issues.is_empty(), "should report issue for missing server");
    assert!(
        issues.iter().any(|i| i.message.contains("diag-server")),
        "issue should mention the missing server"
    );
}

#[test]
fn diagnose_reports_missing_skill_file() {
    let home = TempDir::new().unwrap();
    setup_codex_home(&home);
    let adapter = make_adapter(&home);

    let _fixture = StoreFixture::create_with_skills("diag-skill-pack", &[("check.md", "# Check")]);

    let pack = pack_for_fixture("diag-skill-pack");
    adapter.apply(&pack).unwrap();

    // Manually remove the skill file to simulate drift.
    let skill_path = home.path().join(".codex/skills/diag-skill-pack__check.md");
    std::fs::remove_file(&skill_path).unwrap();

    let issues = adapter.diagnose().unwrap();
    assert!(
        !issues.is_empty(),
        "should report issue for missing skill file"
    );
    assert!(
        issues
            .iter()
            .any(|i| i.message.contains("diag-skill-pack__check.md")),
        "issue should mention the missing skill file"
    );
}
