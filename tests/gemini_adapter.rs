/// Integration tests for the Gemini CLI adapter.
///
/// These tests use `TempDir` for the simulated Gemini home directory and never
/// write to the real `~/.gemini/`.  Tests that exercise prompts or settings also
/// create temporary pack entries under `~/.packweave/packs/` (the real store)
/// and clean them up via a `StoreFixture` drop guard.
use std::collections::HashMap;
use std::path::PathBuf;

use tempfile::TempDir;
use weave::adapters::gemini_cli::GeminiCliAdapter;
use weave::adapters::CliAdapter;
use weave::core::pack::{McpServer, Pack, PackSource, PackTargets, ResolvedPack, Transport};
use weave::core::store::Store;

// ── Helpers ───────────────────────────────────────────────────────────────────

fn make_adapter(home: &TempDir) -> GeminiCliAdapter {
    // Use a separate project root that has no `.gemini/` directory so that
    // project-scope operations are not activated and do not interfere with
    // user-scope tests.  `home.path()` itself is used as the project root so
    // there is no `.gemini` immediately inside it (the `.gemini` dir lives
    // inside `home`'s content, not at `home` itself... actually we use a
    // dedicated sub-path to be explicit).
    let no_project = home.path().join("no-project");
    std::fs::create_dir_all(&no_project).unwrap();
    GeminiCliAdapter::with_home_and_project(home.path().to_path_buf(), no_project)
}

fn make_adapter_with_project(home: &TempDir, project: &TempDir) -> GeminiCliAdapter {
    GeminiCliAdapter::with_home_and_project(home.path().to_path_buf(), project.path().to_path_buf())
}

/// Create `~/.gemini/` inside the temp home.
fn setup_gemini_home(home: &TempDir) {
    std::fs::create_dir_all(home.path().join(".gemini")).unwrap();
}

/// Create `.gemini/` inside the temp project root.
fn setup_project_gemini_dir(project: &TempDir) {
    std::fs::create_dir_all(project.path().join(".gemini")).unwrap();
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
        command: "npx".into(),
        args: vec!["-y".into(), name.to_string()],
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
            weave::core::pack::EnvVar {
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
        command: "npx".into(),
        args: vec!["-y".into(), name.to_string()],
        transport: Some(Transport::Stdio),
        namespace: None,
        tools: vec![],
        env,
    }
}

/// A pack that targets no CLIs — useful for verifying that apply() is a no-op.
fn pack_not_targeting_gemini(name: &str) -> ResolvedPack {
    ResolvedPack {
        pack: Pack {
            name: name.to_string(),
            version: semver::Version::new(1, 0, 0),
            description: "Non-gemini pack".into(),
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
                gemini_cli: false,
                codex_cli: true,
            },
        },
        source: PackSource::Registry {
            registry_url: "https://example.com".into(),
        },
    }
}

// ── StoreFixture: writes pack files into the real store and cleans up on drop ─

struct StoreFixture {
    pack_dir: PathBuf,
}

impl StoreFixture {
    /// Create a pack directory in the real store with optional prompt and/or settings files.
    fn create(name: &str, prompt: Option<&str>, settings: Option<&str>) -> Self {
        let version = semver::Version::new(1, 0, 0);
        let pack_dir = Store::pack_dir(name, &version).expect("store root must be determinable");

        std::fs::create_dir_all(&pack_dir).unwrap();

        // Write a minimal pack.toml so the store recognises the entry.
        std::fs::write(
            pack_dir.join("pack.toml"),
            format!("[pack]\nname = \"{name}\"\nversion = \"1.0.0\"\ndescription = \"fixture\"\n"),
        )
        .unwrap();

        if let Some(content) = prompt {
            std::fs::create_dir_all(pack_dir.join("prompts")).unwrap();
            std::fs::write(pack_dir.join("prompts/gemini.md"), content).unwrap();
        }

        if let Some(content) = settings {
            std::fs::create_dir_all(pack_dir.join("settings")).unwrap();
            std::fs::write(pack_dir.join("settings/gemini.json"), content).unwrap();
        }

        Self { pack_dir }
    }
}

impl Drop for StoreFixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.pack_dir);
    }
}

// ── apply: servers ────────────────────────────────────────────────────────────

#[test]
fn apply_writes_server_to_settings_json() {
    let home = TempDir::new().unwrap();
    setup_gemini_home(&home);
    let adapter = make_adapter(&home);

    let pack = pack_with_servers("mcp-pack", vec![simple_server("my-server")]);
    adapter.apply(&pack).unwrap();

    let settings_path = home.path().join(".gemini/settings.json");
    assert!(settings_path.exists(), "settings.json should be created");
    let config = read_json(&settings_path);
    assert!(
        config["mcpServers"]["my-server"].is_object(),
        "server entry should be written"
    );
    assert_eq!(
        config["mcpServers"]["my-server"]["command"], "npx",
        "command should be set"
    );
}

#[test]
fn apply_writes_server_args() {
    let home = TempDir::new().unwrap();
    setup_gemini_home(&home);
    let adapter = make_adapter(&home);

    let server = McpServer {
        name: "arg-server".into(),
        package_type: None,
        package: None,
        command: "node".into(),
        args: vec!["--flag".into(), "value".into()],
        transport: Some(Transport::Stdio),
        namespace: None,
        tools: vec![],
        env: HashMap::new(),
    };
    let pack = pack_with_servers("arg-pack", vec![server]);
    adapter.apply(&pack).unwrap();

    let settings_path = home.path().join(".gemini/settings.json");
    let config = read_json(&settings_path);
    assert_eq!(
        config["mcpServers"]["arg-server"]["args"],
        serde_json::json!(["--flag", "value"])
    );
}

#[test]
fn apply_writes_env_vars_as_references() {
    let home = TempDir::new().unwrap();
    setup_gemini_home(&home);
    let adapter = make_adapter(&home);

    let pack = pack_with_servers(
        "env-pack",
        vec![server_with_env("env-server", &["API_KEY", "TOKEN"])],
    );
    adapter.apply(&pack).unwrap();

    let settings_path = home.path().join(".gemini/settings.json");
    let config = read_json(&settings_path);
    let env = &config["mcpServers"]["env-server"]["env"];
    assert_eq!(env["API_KEY"], "${API_KEY}");
    assert_eq!(env["TOKEN"], "${TOKEN}");
}

#[test]
fn apply_preserves_existing_user_servers() {
    let home = TempDir::new().unwrap();
    setup_gemini_home(&home);
    let adapter = make_adapter(&home);

    // Pre-populate settings.json with a user-managed server.
    let settings_path = home.path().join(".gemini/settings.json");
    std::fs::write(
        &settings_path,
        r#"{"mcpServers": {"user-server": {"command": "user-cmd"}}}"#,
    )
    .unwrap();

    let pack = pack_with_servers("new-pack", vec![simple_server("new-server")]);
    adapter.apply(&pack).unwrap();

    let config = read_json(&settings_path);
    assert!(
        config["mcpServers"]["user-server"].is_object(),
        "user server must not be removed"
    );
    assert!(
        config["mcpServers"]["new-server"].is_object(),
        "pack server must be added"
    );
}

#[test]
fn apply_rejects_collision_with_user_server() {
    let home = TempDir::new().unwrap();
    setup_gemini_home(&home);
    let adapter = make_adapter(&home);

    // Pre-populate settings.json with a server name that the pack also uses.
    let settings_path = home.path().join(".gemini/settings.json");
    std::fs::write(
        &settings_path,
        r#"{"mcpServers": {"clash-server": {"command": "user-cmd"}}}"#,
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
fn apply_rejects_malformed_settings_json() {
    let home = TempDir::new().unwrap();
    setup_gemini_home(&home);
    let adapter = make_adapter(&home);

    let settings_path = home.path().join(".gemini/settings.json");
    std::fs::write(&settings_path, "[1, 2, 3]").unwrap();

    let pack = pack_with_servers("bad-pack", vec![simple_server("any-server")]);
    let result = adapter.apply(&pack);
    assert!(result.is_err(), "should fail on non-object settings.json");
}

#[test]
fn apply_rejects_non_object_mcp_servers() {
    let home = TempDir::new().unwrap();
    setup_gemini_home(&home);
    let adapter = make_adapter(&home);

    let settings_path = home.path().join(".gemini/settings.json");
    std::fs::write(&settings_path, r#"{"mcpServers": "not-an-object"}"#).unwrap();

    let pack = pack_with_servers("bad-pack", vec![simple_server("any-server")]);
    let result = adapter.apply(&pack);
    assert!(
        result.is_err(),
        "should fail when mcpServers is not an object"
    );
}

#[test]
fn apply_skips_pack_not_targeting_gemini() {
    let home = TempDir::new().unwrap();
    setup_gemini_home(&home);
    let adapter = make_adapter(&home);

    let pack = pack_not_targeting_gemini("other-cli-pack");
    adapter.apply(&pack).unwrap();

    // settings.json should not be created since pack doesn't target gemini.
    let settings_path = home.path().join(".gemini/settings.json");
    assert!(
        !settings_path.exists(),
        "settings.json should not be created for non-gemini pack"
    );
}

// ── apply: multiple servers in one pack ───────────────────────────────────────

#[test]
fn apply_writes_multiple_servers() {
    let home = TempDir::new().unwrap();
    setup_gemini_home(&home);
    let adapter = make_adapter(&home);

    let pack = pack_with_servers(
        "multi-pack",
        vec![simple_server("server-a"), simple_server("server-b")],
    );
    adapter.apply(&pack).unwrap();

    let settings_path = home.path().join(".gemini/settings.json");
    let config = read_json(&settings_path);
    assert!(config["mcpServers"]["server-a"].is_object());
    assert!(config["mcpServers"]["server-b"].is_object());
}

// ── apply: manifest tracking ──────────────────────────────────────────────────

#[test]
fn apply_writes_manifest() {
    let home = TempDir::new().unwrap();
    setup_gemini_home(&home);
    let adapter = make_adapter(&home);

    let pack = pack_with_servers("tracked-pack", vec![simple_server("tracked-server")]);
    adapter.apply(&pack).unwrap();

    let manifest_path = home.path().join(".gemini/.packweave_manifest.json");
    assert!(manifest_path.exists(), "manifest file should be created");
    let manifest = read_json(&manifest_path);
    assert_eq!(
        manifest["servers"]["tracked-server"], "tracked-pack",
        "manifest should track server ownership"
    );
}

// ── remove: servers ───────────────────────────────────────────────────────────

#[test]
fn remove_deletes_server_from_settings_json() {
    let home = TempDir::new().unwrap();
    setup_gemini_home(&home);
    let adapter = make_adapter(&home);

    let pack = pack_with_servers("rm-pack", vec![simple_server("rm-server")]);
    adapter.apply(&pack).unwrap();
    adapter.remove("rm-pack").unwrap();

    let settings_path = home.path().join(".gemini/settings.json");
    let config = read_json(&settings_path);
    let mcp = config["mcpServers"].as_object().unwrap();
    assert!(mcp.get("rm-server").is_none(), "server should be removed");
}

#[test]
fn remove_is_surgical_leaves_other_servers() {
    let home = TempDir::new().unwrap();
    setup_gemini_home(&home);
    let adapter = make_adapter(&home);

    let pack_a = pack_with_servers("pack-a", vec![simple_server("server-a")]);
    let pack_b = pack_with_servers("pack-b", vec![simple_server("server-b")]);
    adapter.apply(&pack_a).unwrap();
    adapter.apply(&pack_b).unwrap();

    adapter.remove("pack-a").unwrap();

    let settings_path = home.path().join(".gemini/settings.json");
    let config = read_json(&settings_path);
    assert!(
        config["mcpServers"]["server-a"].is_null()
            || config["mcpServers"].get("server-a").is_none(),
        "pack-a server should be removed"
    );
    assert!(
        config["mcpServers"]["server-b"].is_object(),
        "pack-b server should remain"
    );
}

#[test]
fn remove_preserves_user_managed_keys_in_settings_json() {
    let home = TempDir::new().unwrap();
    setup_gemini_home(&home);
    let adapter = make_adapter(&home);

    // User has a pre-existing key in settings.json.
    let settings_path = home.path().join(".gemini/settings.json");
    std::fs::write(&settings_path, r#"{"theme": "dark"}"#).unwrap();

    let pack = pack_with_servers("my-pack", vec![simple_server("my-server")]);
    adapter.apply(&pack).unwrap();
    adapter.remove("my-pack").unwrap();

    let config = read_json(&settings_path);
    assert_eq!(config["theme"], "dark", "user key must survive remove");
}

#[test]
fn remove_unknown_pack_is_a_no_op() {
    let home = TempDir::new().unwrap();
    setup_gemini_home(&home);
    let adapter = make_adapter(&home);

    // Should not error even if the pack was never installed.
    adapter.remove("nonexistent-pack").unwrap();
}

#[test]
fn remove_clears_manifest_entry() {
    let home = TempDir::new().unwrap();
    setup_gemini_home(&home);
    let adapter = make_adapter(&home);

    let pack = pack_with_servers("clear-pack", vec![simple_server("clear-server")]);
    adapter.apply(&pack).unwrap();
    adapter.remove("clear-pack").unwrap();

    let manifest_path = home.path().join(".gemini/.packweave_manifest.json");
    let manifest = read_json(&manifest_path);
    assert!(
        manifest["servers"]
            .as_object()
            .map(|m| m.get("clear-server").is_none())
            .unwrap_or(true),
        "manifest should not track removed server"
    );
}

// ── idempotency ───────────────────────────────────────────────────────────────

#[test]
fn apply_twice_is_idempotent_for_servers() {
    let home = TempDir::new().unwrap();
    setup_gemini_home(&home);
    let adapter = make_adapter(&home);

    let pack = pack_with_servers("idem-pack", vec![simple_server("idem-server")]);
    adapter.apply(&pack).unwrap();
    adapter.apply(&pack).unwrap(); // second apply must not error

    let settings_path = home.path().join(".gemini/settings.json");
    let config = read_json(&settings_path);

    // Exactly one entry for the server.
    let mcp = config["mcpServers"].as_object().unwrap();
    assert_eq!(
        mcp.iter()
            .filter(|(k, _)| k.as_str() == "idem-server")
            .count(),
        1,
        "server should appear exactly once"
    );
}

#[test]
fn apply_twice_same_manifest_state() {
    let home = TempDir::new().unwrap();
    setup_gemini_home(&home);
    let adapter = make_adapter(&home);

    let pack = pack_with_servers("idem2-pack", vec![simple_server("idem2-server")]);
    adapter.apply(&pack).unwrap();

    let manifest_after_first = {
        let manifest_path = home.path().join(".gemini/.packweave_manifest.json");
        std::fs::read_to_string(&manifest_path).unwrap()
    };

    adapter.apply(&pack).unwrap();

    let manifest_after_second = {
        let manifest_path = home.path().join(".gemini/.packweave_manifest.json");
        std::fs::read_to_string(&manifest_path).unwrap()
    };

    // Parse both and compare as values (ignoring key order).
    let v1: serde_json::Value = serde_json::from_str(&manifest_after_first).unwrap();
    let v2: serde_json::Value = serde_json::from_str(&manifest_after_second).unwrap();
    assert_eq!(v1, v2, "manifest should be identical after two applies");
}

// ── apply: prompts ────────────────────────────────────────────────────────────

#[test]
fn apply_writes_prompt_block_to_gemini_md() {
    let home = TempDir::new().unwrap();
    setup_gemini_home(&home);
    let adapter = make_adapter(&home);

    let _fixture = StoreFixture::create(
        "prompt-pack",
        Some("You are a helpful Gemini assistant."),
        None,
    );

    let pack = pack_with_servers("prompt-pack", vec![]);
    adapter.apply(&pack).unwrap();

    let gemini_md = home.path().join(".gemini/GEMINI.md");
    assert!(gemini_md.exists(), "GEMINI.md should be created");
    let content = std::fs::read_to_string(&gemini_md).unwrap();
    assert!(content.contains("<!-- packweave:begin:prompt-pack -->"));
    assert!(content.contains("You are a helpful Gemini assistant."));
    assert!(content.contains("<!-- packweave:end:prompt-pack -->"));
}

#[test]
fn apply_prompt_is_idempotent() {
    let home = TempDir::new().unwrap();
    setup_gemini_home(&home);
    let adapter = make_adapter(&home);

    let _fixture = StoreFixture::create("idem-prompt", Some("Idempotent content."), None);

    let pack = pack_with_servers("idem-prompt", vec![]);
    adapter.apply(&pack).unwrap();
    adapter.apply(&pack).unwrap();

    let gemini_md = home.path().join(".gemini/GEMINI.md");
    let content = std::fs::read_to_string(&gemini_md).unwrap();
    let count = content
        .matches("<!-- packweave:begin:idem-prompt -->")
        .count();
    assert_eq!(count, 1, "begin tag should appear exactly once");
}

#[test]
fn apply_prompt_appends_to_existing_gemini_md() {
    let home = TempDir::new().unwrap();
    setup_gemini_home(&home);
    let adapter = make_adapter(&home);

    let gemini_md_path = home.path().join(".gemini/GEMINI.md");
    std::fs::write(&gemini_md_path, "# User instructions\n").unwrap();

    let _fixture = StoreFixture::create("append-pack", Some("Pack content here."), None);
    let pack = pack_with_servers("append-pack", vec![]);
    adapter.apply(&pack).unwrap();

    let content = std::fs::read_to_string(&gemini_md_path).unwrap();
    assert!(
        content.contains("# User instructions"),
        "original content must remain"
    );
    assert!(
        content.contains("Pack content here."),
        "pack content must be appended"
    );
}

// ── remove: prompts ───────────────────────────────────────────────────────────

#[test]
fn remove_strips_prompt_block_from_gemini_md() {
    let home = TempDir::new().unwrap();
    setup_gemini_home(&home);
    let adapter = make_adapter(&home);

    let _fixture = StoreFixture::create("rm-prompt-pack", Some("Remove me."), None);

    let pack = pack_with_servers("rm-prompt-pack", vec![]);
    adapter.apply(&pack).unwrap();
    adapter.remove("rm-prompt-pack").unwrap();

    let gemini_md = home.path().join(".gemini/GEMINI.md");
    let content = std::fs::read_to_string(&gemini_md).unwrap();
    assert!(
        !content.contains("<!-- packweave:begin:rm-prompt-pack -->"),
        "begin tag should be gone"
    );
    assert!(
        !content.contains("Remove me."),
        "prompt content should be gone"
    );
}

#[test]
fn remove_prompt_is_surgical_multiple_packs() {
    let home = TempDir::new().unwrap();
    setup_gemini_home(&home);
    let adapter = make_adapter(&home);

    let _fx_a = StoreFixture::create("prune-a", Some("Content A."), None);
    let _fx_b = StoreFixture::create("prune-b", Some("Content B."), None);

    adapter
        .apply(&pack_with_servers("prune-a", vec![]))
        .unwrap();
    adapter
        .apply(&pack_with_servers("prune-b", vec![]))
        .unwrap();
    adapter.remove("prune-a").unwrap();

    let gemini_md = home.path().join(".gemini/GEMINI.md");
    let content = std::fs::read_to_string(&gemini_md).unwrap();
    assert!(
        !content.contains("Content A."),
        "prune-a content should be gone"
    );
    assert!(
        content.contains("Content B."),
        "prune-b content must remain"
    );
}

// ── apply: settings ───────────────────────────────────────────────────────────

#[test]
fn apply_merges_settings_fragment_into_settings_json() {
    let home = TempDir::new().unwrap();
    setup_gemini_home(&home);
    let adapter = make_adapter(&home);

    let _fixture = StoreFixture::create(
        "settings-pack",
        None,
        Some(r#"{"model": "gemini-2.0-flash"}"#),
    );

    let pack = pack_with_servers("settings-pack", vec![]);
    adapter.apply(&pack).unwrap();

    let settings_path = home.path().join(".gemini/settings.json");
    let config = read_json(&settings_path);
    assert_eq!(config["model"], "gemini-2.0-flash");
}

#[test]
fn apply_settings_preserves_existing_keys() {
    let home = TempDir::new().unwrap();
    setup_gemini_home(&home);
    let adapter = make_adapter(&home);

    let settings_path = home.path().join(".gemini/settings.json");
    std::fs::write(&settings_path, r#"{"theme": "dark"}"#).unwrap();

    let _fixture = StoreFixture::create("settings-merge", None, Some(r#"{"model": "gemini-pro"}"#));

    let pack = pack_with_servers("settings-merge", vec![]);
    adapter.apply(&pack).unwrap();

    let config = read_json(&settings_path);
    assert_eq!(config["theme"], "dark", "pre-existing key must survive");
    assert_eq!(config["model"], "gemini-pro", "new key must be written");
}

#[test]
fn apply_settings_is_idempotent() {
    let home = TempDir::new().unwrap();
    setup_gemini_home(&home);
    let adapter = make_adapter(&home);

    let _fixture = StoreFixture::create("settings-idem", None, Some(r#"{"theme": "monokai"}"#));

    let pack = pack_with_servers("settings-idem", vec![]);
    adapter.apply(&pack).unwrap();
    adapter.apply(&pack).unwrap();

    let settings_path = home.path().join(".gemini/settings.json");
    let config = read_json(&settings_path);
    assert_eq!(config["theme"], "monokai");
}

// ── remove: settings ──────────────────────────────────────────────────────────

#[test]
fn remove_restores_settings_key_to_original_value() {
    let home = TempDir::new().unwrap();
    setup_gemini_home(&home);
    let adapter = make_adapter(&home);

    let settings_path = home.path().join(".gemini/settings.json");
    std::fs::write(&settings_path, r#"{"theme": "dark"}"#).unwrap();

    let _fixture = StoreFixture::create("settings-restore", None, Some(r#"{"theme": "monokai"}"#));

    let pack = pack_with_servers("settings-restore", vec![]);
    adapter.apply(&pack).unwrap();

    let config = read_json(&settings_path);
    assert_eq!(config["theme"], "monokai", "theme should be changed");

    adapter.remove("settings-restore").unwrap();

    let config = read_json(&settings_path);
    assert_eq!(config["theme"], "dark", "original theme should be restored");
}

#[test]
fn remove_deletes_settings_key_added_by_pack() {
    let home = TempDir::new().unwrap();
    setup_gemini_home(&home);
    let adapter = make_adapter(&home);

    let _fixture = StoreFixture::create("settings-delete", None, Some(r#"{"newKey": "newVal"}"#));

    let pack = pack_with_servers("settings-delete", vec![]);
    adapter.apply(&pack).unwrap();
    adapter.remove("settings-delete").unwrap();

    let settings_path = home.path().join(".gemini/settings.json");
    let config = read_json(&settings_path);
    assert!(
        config.get("newKey").is_none() || config["newKey"].is_null(),
        "key added by pack should be removed"
    );
}

// ── project scope: servers ────────────────────────────────────────────────────

#[test]
fn apply_writes_server_to_project_settings_json_when_project_dir_exists() {
    let home = TempDir::new().unwrap();
    let project = TempDir::new().unwrap();
    setup_gemini_home(&home);
    setup_project_gemini_dir(&project);
    let adapter = make_adapter_with_project(&home, &project);

    let pack = pack_with_servers("proj-pack", vec![simple_server("proj-server")]);
    adapter.apply(&pack).unwrap();

    // Should write to project .gemini/settings.json.
    let proj_settings = project.path().join(".gemini/settings.json");
    assert!(
        proj_settings.exists(),
        "project settings.json should be created"
    );
    let config = read_json(&proj_settings);
    assert!(config["mcpServers"]["proj-server"].is_object());

    // Should also write to user scope.
    let user_settings = home.path().join(".gemini/settings.json");
    assert!(
        user_settings.exists(),
        "user-scope settings.json should be created"
    );
}

#[test]
fn apply_does_not_write_project_scope_when_dir_absent() {
    let home = TempDir::new().unwrap();
    let project = TempDir::new().unwrap();
    // NOTE: we do NOT create project/.gemini/
    setup_gemini_home(&home);
    let adapter = make_adapter_with_project(&home, &project);

    let pack = pack_with_servers("no-proj-pack", vec![simple_server("no-proj-server")]);
    adapter.apply(&pack).unwrap();

    let proj_settings = project.path().join(".gemini/settings.json");
    assert!(
        !proj_settings.exists(),
        "project settings.json must not be created when .gemini/ is absent"
    );
}

#[test]
fn remove_cleans_up_project_scope_servers() {
    let home = TempDir::new().unwrap();
    let project = TempDir::new().unwrap();
    setup_gemini_home(&home);
    setup_project_gemini_dir(&project);
    let adapter = make_adapter_with_project(&home, &project);

    let pack = pack_with_servers("rm-proj-pack", vec![simple_server("rm-proj-server")]);
    adapter.apply(&pack).unwrap();
    adapter.remove("rm-proj-pack").unwrap();

    let proj_settings = project.path().join(".gemini/settings.json");
    let config = read_json(&proj_settings);
    let mcp = config["mcpServers"].as_object().unwrap();
    assert!(
        mcp.get("rm-proj-server").is_none(),
        "project-scope server should be removed"
    );
}

// ── project scope: idempotency ────────────────────────────────────────────────

#[test]
fn apply_project_scope_is_idempotent() {
    let home = TempDir::new().unwrap();
    let project = TempDir::new().unwrap();
    setup_gemini_home(&home);
    setup_project_gemini_dir(&project);
    let adapter = make_adapter_with_project(&home, &project);

    let pack = pack_with_servers("proj-idem", vec![simple_server("proj-idem-server")]);
    adapter.apply(&pack).unwrap();
    adapter.apply(&pack).unwrap();

    let proj_settings = project.path().join(".gemini/settings.json");
    let config = read_json(&proj_settings);
    let mcp = config["mcpServers"].as_object().unwrap();
    let count = mcp
        .keys()
        .filter(|k| k.as_str() == "proj-idem-server")
        .count();
    assert_eq!(count, 1, "project server should appear exactly once");
}

// ── diagnose ──────────────────────────────────────────────────────────────────

#[test]
fn diagnose_returns_no_issues_for_clean_state() {
    let home = TempDir::new().unwrap();
    setup_gemini_home(&home);
    let adapter = make_adapter(&home);

    let pack = pack_with_servers("diag-pack", vec![simple_server("diag-server")]);
    adapter.apply(&pack).unwrap();

    let issues = adapter.diagnose().unwrap();
    assert!(
        issues.is_empty(),
        "clean state should have no diagnostic issues"
    );
}

#[test]
fn diagnose_reports_server_missing_from_settings_json() {
    let home = TempDir::new().unwrap();
    setup_gemini_home(&home);
    let adapter = make_adapter(&home);

    let pack = pack_with_servers("diag-missing", vec![simple_server("missing-server")]);
    adapter.apply(&pack).unwrap();

    // Manually remove the server from settings.json without updating the manifest.
    let settings_path = home.path().join(".gemini/settings.json");
    std::fs::write(&settings_path, "{}").unwrap();

    let issues = adapter.diagnose().unwrap();
    assert!(
        !issues.is_empty(),
        "should report issue when server is missing from settings.json"
    );
    let msg = &issues[0].message;
    assert!(
        msg.contains("missing-server"),
        "issue should name the missing server"
    );
}

#[test]
fn diagnose_reports_prompt_block_missing_from_gemini_md() {
    let home = TempDir::new().unwrap();
    setup_gemini_home(&home);
    let adapter = make_adapter(&home);

    let _fixture = StoreFixture::create("diag-prompt", Some("Diagnose me."), None);

    let pack = pack_with_servers("diag-prompt", vec![]);
    adapter.apply(&pack).unwrap();

    // Wipe GEMINI.md so the tracked block is missing.
    let gemini_md = home.path().join(".gemini/GEMINI.md");
    std::fs::write(&gemini_md, "").unwrap();

    let issues = adapter.diagnose().unwrap();
    assert!(
        !issues.is_empty(),
        "should report issue when prompt block is missing"
    );
    let any_prompt_issue = issues.iter().any(|i| i.message.contains("diag-prompt"));
    assert!(any_prompt_issue, "issue should name the pack");
}

// ── error cases ───────────────────────────────────────────────────────────────

#[test]
fn apply_two_packs_conflict_on_same_server_name() {
    let home = TempDir::new().unwrap();
    setup_gemini_home(&home);
    let adapter = make_adapter(&home);

    let pack_a = pack_with_servers("conflict-a", vec![simple_server("shared-server")]);
    let pack_b = pack_with_servers("conflict-b", vec![simple_server("shared-server")]);

    adapter.apply(&pack_a).unwrap();
    let result = adapter.apply(&pack_b);
    assert!(
        result.is_err(),
        "second pack should fail on conflicting server"
    );
    let msg = result.unwrap_err().to_string();
    assert!(
        msg.contains("shared-server"),
        "error should name the conflict"
    );
    assert!(
        msg.contains("conflict-a"),
        "error should name the existing owner"
    );
}

#[test]
fn remove_after_missing_settings_file_is_graceful() {
    let home = TempDir::new().unwrap();
    setup_gemini_home(&home);
    let adapter = make_adapter(&home);

    let pack = pack_with_servers("ghost-pack", vec![simple_server("ghost-server")]);
    adapter.apply(&pack).unwrap();

    // Delete settings.json to simulate it being deleted externally.
    std::fs::remove_file(home.path().join(".gemini/settings.json")).unwrap();

    // remove should not panic or return an error — the server simply isn't there to remove.
    adapter.remove("ghost-pack").unwrap();
}
