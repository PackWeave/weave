/// Integration tests for the Claude Code adapter.
///
/// These tests use `TempDir` for the simulated Claude home directory and never
/// write to the real `~/.claude/` or `~/.claude.json`. Tests that exercise
/// prompts, commands, or settings also create temporary pack entries in a
/// redirected store configured via `WEAVE_TEST_STORE_DIR`, managed by a
/// `StoreFixture` drop guard, so the real `~/.packweave/packs/` is never touched.
use std::collections::HashMap;
use std::path::PathBuf;

use packweave::adapters::claude_code::ClaudeCodeAdapter;
use packweave::adapters::{ApplyOptions, CliAdapter};
use packweave::core::pack::{McpServer, Pack, PackSource, PackTargets, ResolvedPack, Transport};
use packweave::core::store::Store;
use tempfile::TempDir;

// ── Helpers ───────────────────────────────────────────────────────────────────

fn make_adapter(home: &TempDir) -> ClaudeCodeAdapter {
    // Use a separate project root that has no `.claude/` directory so that
    // project-scope operations are not activated and do not interfere with
    // user-scope tests.
    let no_project = home.path().join("no-project");
    std::fs::create_dir_all(&no_project).unwrap();
    ClaudeCodeAdapter::with_home_and_project(home.path().to_path_buf(), no_project)
}

/// Create an adapter with project-scope enabled (opt-in via `project_install: true`).
fn make_adapter_with_project(home: &TempDir, project: &TempDir) -> ClaudeCodeAdapter {
    ClaudeCodeAdapter::with_home_project_scope(
        home.path().to_path_buf(),
        project.path().to_path_buf(),
    )
}

/// Create `~/.claude/` inside the temp home so user-scope writes succeed.
fn setup_claude_home(home: &TempDir) {
    std::fs::create_dir_all(home.path().join(".claude")).unwrap();
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
        tools: vec![],
        env: HashMap::new(),
    }
}

/// A pack that targets no CLIs — useful for verifying that apply() is a no-op.
fn pack_not_targeting_claude(name: &str) -> ResolvedPack {
    ResolvedPack {
        pack: Pack {
            name: name.to_string(),
            version: semver::Version::new(1, 0, 0),
            description: "Non-claude pack".into(),
            authors: vec![],
            license: None,
            repository: None,
            keywords: vec![],
            min_tool_version: None,
            servers: vec![simple_server("some-server")],
            dependencies: HashMap::new(),
            extensions: Default::default(),
            targets: PackTargets {
                claude_code: false,
                gemini_cli: true,
                codex_cli: true,
            },
        },
        source: PackSource::Registry {
            registry_url: "https://example.com".into(),
        },
    }
}

// ── StoreFixture: writes pack files into an isolated TempDir store ────────────
//
// Setting WEAVE_TEST_STORE_DIR redirects Store::root() (via util::packweave_dir())
// to a TempDir so fixture data never lands in the real ~/.packweave/.
//
// A single shared TempDir is initialised once for the whole test process via
// OnceLock, which means:
//   - WEAVE_TEST_STORE_DIR is set exactly once and never races with other tests
//   - Multiple StoreFixtures in the same test work without mutex re-entrancy
//   - Each fixture is responsible only for its own pack subdirectory

use std::sync::OnceLock;

fn shared_store_root() -> &'static TempDir {
    static STORE: OnceLock<TempDir> = OnceLock::new();
    STORE.get_or_init(|| {
        let dir = TempDir::new().expect("shared store TempDir");
        // SAFETY: serial test (serial_test crate)
        unsafe { std::env::set_var("WEAVE_TEST_STORE_DIR", dir.path()) };
        dir
    })
}

struct StoreFixture {
    pack_dir: PathBuf,
}

impl StoreFixture {
    /// Create a pack directory in the shared isolated store.
    ///
    /// - `prompt` — optional content written to `prompts/claude.md`
    /// - `settings` — optional content written to `settings/claude.json`
    /// - `commands` — optional list of `(filename, content)` pairs written to `commands/`
    fn create(
        name: &str,
        prompt: Option<&str>,
        settings: Option<&str>,
        commands: Option<&[(&str, &str)]>,
    ) -> Self {
        let store = shared_store_root();

        let version = semver::Version::new(1, 0, 0);
        let pack_dir =
            Store::pack_dir(name, &version, None).expect("store root must be determinable");

        // Safety: verify pack_dir is inside the shared test store — guards
        // against accidental writes to the real ~/.packweave/ store.
        assert!(
            pack_dir.starts_with(store.path()),
            "pack_dir {pack_dir:?} is not inside the isolated store root {:?}",
            store.path()
        );

        std::fs::create_dir_all(&pack_dir).unwrap();

        // Write a minimal pack.toml so the store recognises the entry.
        std::fs::write(
            pack_dir.join("pack.toml"),
            format!("[pack]\nname = \"{name}\"\nversion = \"1.0.0\"\ndescription = \"fixture\"\n"),
        )
        .unwrap();

        if let Some(content) = prompt {
            std::fs::create_dir_all(pack_dir.join("prompts")).unwrap();
            std::fs::write(pack_dir.join("prompts/claude.md"), content).unwrap();
        }

        if let Some(content) = settings {
            std::fs::create_dir_all(pack_dir.join("settings")).unwrap();
            std::fs::write(pack_dir.join("settings/claude.json"), content).unwrap();
        }

        if let Some(cmds) = commands {
            std::fs::create_dir_all(pack_dir.join("commands")).unwrap();
            for (filename, content) in cmds {
                std::fs::write(pack_dir.join("commands").join(filename), content).unwrap();
            }
        }

        Self { pack_dir }
    }
}

impl Drop for StoreFixture {
    fn drop(&mut self) {
        // Remove only this pack's subdirectory — other tests sharing the same
        // store root may still be running.
        let _ = std::fs::remove_dir_all(&self.pack_dir);
    }
}

// ── apply: servers ────────────────────────────────────────────────────────────

#[test]
fn apply_servers_adds_to_claude_json() {
    let home = TempDir::new().unwrap();
    setup_claude_home(&home);
    let adapter = make_adapter(&home);

    let pack = pack_with_servers("mcp-pack", vec![simple_server("my-server")]);
    adapter.apply(&pack, &ApplyOptions::default()).unwrap();

    let claude_json = home.path().join(".claude.json");
    assert!(claude_json.exists(), ".claude.json should be created");
    let config = read_json(&claude_json);
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
fn apply_servers_writes_args() {
    let home = TempDir::new().unwrap();
    setup_claude_home(&home);
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
        tools: vec![],
        env: HashMap::new(),
    };
    let pack = pack_with_servers("arg-pack", vec![server]);
    adapter.apply(&pack, &ApplyOptions::default()).unwrap();

    let config = read_json(&home.path().join(".claude.json"));
    assert_eq!(
        config["mcpServers"]["arg-server"]["args"],
        serde_json::json!(["--flag", "value"])
    );
}

#[test]
fn apply_servers_idempotent() {
    let home = TempDir::new().unwrap();
    setup_claude_home(&home);
    let adapter = make_adapter(&home);

    let pack = pack_with_servers("idem-pack", vec![simple_server("idem-server")]);
    adapter.apply(&pack, &ApplyOptions::default()).unwrap();
    adapter.apply(&pack, &ApplyOptions::default()).unwrap(); // second apply must not error

    let config = read_json(&home.path().join(".claude.json"));
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
fn apply_preserves_existing_user_servers() {
    let home = TempDir::new().unwrap();
    setup_claude_home(&home);
    let adapter = make_adapter(&home);

    // Pre-populate .claude.json with a user-managed server.
    let claude_json = home.path().join(".claude.json");
    std::fs::write(
        &claude_json,
        r#"{"mcpServers": {"user-server": {"command": "user-cmd"}}}"#,
    )
    .unwrap();

    let pack = pack_with_servers("new-pack", vec![simple_server("new-server")]);
    adapter.apply(&pack, &ApplyOptions::default()).unwrap();

    let config = read_json(&claude_json);
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
    setup_claude_home(&home);
    let adapter = make_adapter(&home);

    // Pre-populate .claude.json with a server name the pack also uses.
    std::fs::write(
        home.path().join(".claude.json"),
        r#"{"mcpServers": {"clash-server": {"command": "user-cmd"}}}"#,
    )
    .unwrap();

    let pack = pack_with_servers("clash-pack", vec![simple_server("clash-server")]);
    let result = adapter.apply(&pack, &ApplyOptions::default());
    assert!(result.is_err(), "should fail when a user server collides");
    let msg = result.unwrap_err().to_string();
    assert!(
        msg.contains("clash-server"),
        "error should name the conflicting server"
    );
}

#[test]
fn apply_skips_pack_not_targeting_claude() {
    let home = TempDir::new().unwrap();
    setup_claude_home(&home);
    let adapter = make_adapter(&home);

    let pack = pack_not_targeting_claude("other-cli-pack");
    adapter.apply(&pack, &ApplyOptions::default()).unwrap();

    // .claude.json should not be created since pack doesn't target claude.
    let claude_json = home.path().join(".claude.json");
    assert!(
        !claude_json.exists(),
        ".claude.json should not be created for non-claude pack"
    );
}

// ── remove: servers ───────────────────────────────────────────────────────────

#[test]
fn remove_servers_cleans_up_claude_json() {
    let home = TempDir::new().unwrap();
    setup_claude_home(&home);
    let adapter = make_adapter(&home);

    let pack = pack_with_servers("rm-pack", vec![simple_server("rm-server")]);
    adapter.apply(&pack, &ApplyOptions::default()).unwrap();
    adapter.remove("rm-pack").unwrap();

    let config = read_json(&home.path().join(".claude.json"));
    let mcp = config["mcpServers"].as_object().unwrap();
    assert!(mcp.get("rm-server").is_none(), "server should be removed");
}

#[test]
fn remove_is_surgical_leaves_other_servers() {
    let home = TempDir::new().unwrap();
    setup_claude_home(&home);
    let adapter = make_adapter(&home);

    let pack_a = pack_with_servers("pack-a", vec![simple_server("server-a")]);
    let pack_b = pack_with_servers("pack-b", vec![simple_server("server-b")]);
    adapter.apply(&pack_a, &ApplyOptions::default()).unwrap();
    adapter.apply(&pack_b, &ApplyOptions::default()).unwrap();

    adapter.remove("pack-a").unwrap();

    let config = read_json(&home.path().join(".claude.json"));
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
fn remove_unknown_pack_is_a_no_op() {
    let home = TempDir::new().unwrap();
    setup_claude_home(&home);
    let adapter = make_adapter(&home);

    // Should not error even if the pack was never installed.
    adapter.remove("nonexistent-pack").unwrap();
}

// ── apply: manifest tracking ──────────────────────────────────────────────────

#[test]
fn apply_writes_manifest() {
    let home = TempDir::new().unwrap();
    setup_claude_home(&home);
    let adapter = make_adapter(&home);

    let pack = pack_with_servers("tracked-pack", vec![simple_server("tracked-server")]);
    adapter.apply(&pack, &ApplyOptions::default()).unwrap();

    let manifest_path = home.path().join(".claude/.packweave_manifest.json");
    assert!(manifest_path.exists(), "manifest file should be created");
    let manifest = read_json(&manifest_path);
    assert_eq!(
        manifest["servers"]["tracked-server"], "tracked-pack",
        "manifest should track server ownership"
    );
}

// ── apply: project scope ──────────────────────────────────────────────────────

#[test]
fn apply_servers_project_scope() {
    let home = TempDir::new().unwrap();
    let project = TempDir::new().unwrap();
    setup_claude_home(&home);
    let adapter = make_adapter_with_project(&home, &project);

    let pack = pack_with_servers("proj-pack", vec![simple_server("proj-server")]);
    adapter.apply(&pack, &ApplyOptions::default()).unwrap();

    // Should write to project .mcp.json.
    let proj_mcp = project.path().join(".mcp.json");
    assert!(proj_mcp.exists(), "project .mcp.json should be created");
    let config = read_json(&proj_mcp);
    assert!(
        config["mcpServers"]["proj-server"].is_object(),
        "project-scope server entry should be written"
    );

    // Should also write to user scope.
    let user_claude_json = home.path().join(".claude.json");
    assert!(
        user_claude_json.exists(),
        "user-scope .claude.json should be created"
    );
}

#[test]
fn apply_does_not_write_project_scope_without_flag() {
    let home = TempDir::new().unwrap();
    let project = TempDir::new().unwrap();
    setup_claude_home(&home);
    // Create .claude/ so the old auto-detection would have fired — but the new
    // code must NOT write project scope without project_install: true.
    std::fs::create_dir_all(project.path().join(".claude")).unwrap();
    let adapter = ClaudeCodeAdapter::with_home_and_project(
        home.path().to_path_buf(),
        project.path().to_path_buf(),
    );

    let pack = pack_with_servers("no-proj-pack", vec![simple_server("no-proj-server")]);
    adapter.apply(&pack, &ApplyOptions::default()).unwrap();

    let proj_mcp = project.path().join(".mcp.json");
    assert!(
        !proj_mcp.exists(),
        "project .mcp.json must not be created without --project flag (project_install: false)"
    );
}

// ── remove: project scope ─────────────────────────────────────────────────────

#[test]
fn remove_servers_project_scope() {
    let home = TempDir::new().unwrap();
    let project = TempDir::new().unwrap();
    setup_claude_home(&home);
    let adapter = make_adapter_with_project(&home, &project);

    let pack = pack_with_servers("rm-proj-pack", vec![simple_server("rm-proj-server")]);
    adapter.apply(&pack, &ApplyOptions::default()).unwrap();
    adapter.remove("rm-proj-pack").unwrap();

    // When the last project-scope server is removed, the .mcp.json file
    // should be deleted entirely rather than leaving an empty stub.
    let proj_mcp = project.path().join(".mcp.json");
    assert!(
        !proj_mcp.exists(),
        ".mcp.json should be deleted when the last project-scope server is removed"
    );
}

#[test]
fn apply_project_scope_is_idempotent() {
    let home = TempDir::new().unwrap();
    let project = TempDir::new().unwrap();
    setup_claude_home(&home);
    let adapter = make_adapter_with_project(&home, &project);

    let pack = pack_with_servers("proj-idem", vec![simple_server("proj-idem-server")]);
    adapter.apply(&pack, &ApplyOptions::default()).unwrap();
    adapter.apply(&pack, &ApplyOptions::default()).unwrap();

    let proj_mcp = project.path().join(".mcp.json");
    let config = read_json(&proj_mcp);
    let mcp = config["mcpServers"].as_object().unwrap();
    let count = mcp
        .keys()
        .filter(|k| k.as_str() == "proj-idem-server")
        .count();
    assert_eq!(count, 1, "project server should appear exactly once");
}

// ── apply: commands ───────────────────────────────────────────────────────────

#[test]
fn apply_commands_writes_files() {
    let home = TempDir::new().unwrap();
    setup_claude_home(&home);
    let adapter = make_adapter(&home);

    let _fixture = StoreFixture::create(
        "cmd-pack",
        None,
        None,
        Some(&[("deploy.md", "# Deploy\nRun the deploy command.")]),
    );

    let pack = pack_with_servers("cmd-pack", vec![]);
    adapter.apply(&pack, &ApplyOptions::default()).unwrap();

    let commands_dir = home.path().join(".claude/commands");
    assert!(
        commands_dir.exists(),
        "commands directory should be created"
    );

    // The adapter namespaces command files as <pack_name>__<filename>
    let cmd_file = commands_dir.join("cmd-pack__deploy.md");
    assert!(cmd_file.exists(), "command file should be written");
    let content = std::fs::read_to_string(&cmd_file).unwrap();
    assert!(
        content.contains("Run the deploy command."),
        "command file should contain expected content"
    );
}

#[test]
fn apply_commands_multiple_files() {
    let home = TempDir::new().unwrap();
    setup_claude_home(&home);
    let adapter = make_adapter(&home);

    let _fixture = StoreFixture::create(
        "multi-cmd-pack",
        None,
        None,
        Some(&[
            ("build.md", "# Build\nBuild the project."),
            ("test.md", "# Test\nRun tests."),
        ]),
    );

    let pack = pack_with_servers("multi-cmd-pack", vec![]);
    adapter.apply(&pack, &ApplyOptions::default()).unwrap();

    let commands_dir = home.path().join(".claude/commands");
    assert!(commands_dir.join("multi-cmd-pack__build.md").exists());
    assert!(commands_dir.join("multi-cmd-pack__test.md").exists());
}

// ── remove: commands ──────────────────────────────────────────────────────────

#[test]
fn remove_commands_cleans_up() {
    let home = TempDir::new().unwrap();
    setup_claude_home(&home);
    let adapter = make_adapter(&home);

    let _fixture = StoreFixture::create(
        "rm-cmd-pack",
        None,
        None,
        Some(&[("action.md", "# Action\nDo something.")]),
    );

    let pack = pack_with_servers("rm-cmd-pack", vec![]);
    adapter.apply(&pack, &ApplyOptions::default()).unwrap();

    let cmd_file = home.path().join(".claude/commands/rm-cmd-pack__action.md");
    assert!(cmd_file.exists(), "command file should exist after apply");

    adapter.remove("rm-cmd-pack").unwrap();
    assert!(
        !cmd_file.exists(),
        "command file should be removed after remove"
    );
}

#[test]
fn remove_commands_is_surgical() {
    let home = TempDir::new().unwrap();
    setup_claude_home(&home);
    let adapter = make_adapter(&home);

    let _fx_a = StoreFixture::create("cmd-a", None, None, Some(&[("alpha.md", "# Alpha")]));
    let _fx_b = StoreFixture::create("cmd-b", None, None, Some(&[("beta.md", "# Beta")]));

    adapter
        .apply(
            &pack_with_servers("cmd-a", vec![]),
            &ApplyOptions::default(),
        )
        .unwrap();
    adapter
        .apply(
            &pack_with_servers("cmd-b", vec![]),
            &ApplyOptions::default(),
        )
        .unwrap();
    adapter.remove("cmd-a").unwrap();

    let commands_dir = home.path().join(".claude/commands");
    assert!(
        !commands_dir.join("cmd-a__alpha.md").exists(),
        "cmd-a command should be removed"
    );
    assert!(
        commands_dir.join("cmd-b__beta.md").exists(),
        "cmd-b command should remain"
    );
}

// ── apply: prompts ────────────────────────────────────────────────────────────

#[test]
fn apply_prompts_appends_to_claude_md() {
    let home = TempDir::new().unwrap();
    setup_claude_home(&home);
    let adapter = make_adapter(&home);

    let _fixture = StoreFixture::create(
        "prompt-pack",
        Some("You are a helpful Claude assistant."),
        None,
        None,
    );

    let pack = pack_with_servers("prompt-pack", vec![]);
    adapter.apply(&pack, &ApplyOptions::default()).unwrap();

    let claude_md = home.path().join(".claude/CLAUDE.md");
    assert!(claude_md.exists(), "CLAUDE.md should be created");
    let content = std::fs::read_to_string(&claude_md).unwrap();
    assert!(content.contains("<!-- packweave:begin:prompt-pack -->"));
    assert!(content.contains("You are a helpful Claude assistant."));
    assert!(content.contains("<!-- packweave:end:prompt-pack -->"));
}

#[test]
fn apply_prompts_idempotent() {
    let home = TempDir::new().unwrap();
    setup_claude_home(&home);
    let adapter = make_adapter(&home);

    let _fixture = StoreFixture::create("idem-prompt", Some("Idempotent content."), None, None);

    let pack = pack_with_servers("idem-prompt", vec![]);
    adapter.apply(&pack, &ApplyOptions::default()).unwrap();
    adapter.apply(&pack, &ApplyOptions::default()).unwrap();

    let claude_md = home.path().join(".claude/CLAUDE.md");
    let content = std::fs::read_to_string(&claude_md).unwrap();
    let count = content
        .matches("<!-- packweave:begin:idem-prompt -->")
        .count();
    assert_eq!(count, 1, "begin tag should appear exactly once");
}

#[test]
fn apply_prompt_appends_to_existing_claude_md() {
    let home = TempDir::new().unwrap();
    setup_claude_home(&home);
    let adapter = make_adapter(&home);

    let claude_md_path = home.path().join(".claude/CLAUDE.md");
    std::fs::write(&claude_md_path, "# User instructions\n").unwrap();

    let _fixture = StoreFixture::create("append-pack", Some("Pack content here."), None, None);
    let pack = pack_with_servers("append-pack", vec![]);
    adapter.apply(&pack, &ApplyOptions::default()).unwrap();

    let content = std::fs::read_to_string(&claude_md_path).unwrap();
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
fn remove_prompts_cleans_section() {
    let home = TempDir::new().unwrap();
    setup_claude_home(&home);
    let adapter = make_adapter(&home);

    let _fixture = StoreFixture::create("rm-prompt-pack", Some("Remove me."), None, None);

    let pack = pack_with_servers("rm-prompt-pack", vec![]);
    adapter.apply(&pack, &ApplyOptions::default()).unwrap();
    adapter.remove("rm-prompt-pack").unwrap();

    let claude_md = home.path().join(".claude/CLAUDE.md");
    let content = std::fs::read_to_string(&claude_md).unwrap();
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
    setup_claude_home(&home);
    let adapter = make_adapter(&home);

    let _fx_a = StoreFixture::create("prune-a", Some("Content A."), None, None);
    let _fx_b = StoreFixture::create("prune-b", Some("Content B."), None, None);

    adapter
        .apply(
            &pack_with_servers("prune-a", vec![]),
            &ApplyOptions::default(),
        )
        .unwrap();
    adapter
        .apply(
            &pack_with_servers("prune-b", vec![]),
            &ApplyOptions::default(),
        )
        .unwrap();
    adapter.remove("prune-a").unwrap();

    let claude_md = home.path().join(".claude/CLAUDE.md");
    let content = std::fs::read_to_string(&claude_md).unwrap();
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
fn apply_settings_merges_keys() {
    let home = TempDir::new().unwrap();
    setup_claude_home(&home);
    let adapter = make_adapter(&home);

    let _fixture =
        StoreFixture::create("settings-pack", None, Some(r#"{"theme": "monokai"}"#), None);

    let pack = pack_with_servers("settings-pack", vec![]);
    adapter.apply(&pack, &ApplyOptions::default()).unwrap();

    let settings_path = home.path().join(".claude/settings.json");
    assert!(settings_path.exists(), "settings.json should be created");
    let config = read_json(&settings_path);
    assert_eq!(config["theme"], "monokai");
}

#[test]
fn apply_settings_preserves_existing_keys() {
    let home = TempDir::new().unwrap();
    setup_claude_home(&home);
    let adapter = make_adapter(&home);

    let settings_path = home.path().join(".claude/settings.json");
    std::fs::write(&settings_path, r#"{"verbose": true}"#).unwrap();

    let _fixture = StoreFixture::create(
        "settings-merge",
        None,
        Some(r#"{"theme": "dracula"}"#),
        None,
    );

    let pack = pack_with_servers("settings-merge", vec![]);
    adapter.apply(&pack, &ApplyOptions::default()).unwrap();

    let config = read_json(&settings_path);
    assert_eq!(config["verbose"], true, "pre-existing key must survive");
    assert_eq!(config["theme"], "dracula", "new key must be written");
}

#[test]
fn apply_settings_is_idempotent() {
    let home = TempDir::new().unwrap();
    setup_claude_home(&home);
    let adapter = make_adapter(&home);

    let _fixture = StoreFixture::create(
        "settings-idem",
        None,
        Some(r#"{"model": "claude-3-5-sonnet"}"#),
        None,
    );

    let pack = pack_with_servers("settings-idem", vec![]);
    adapter.apply(&pack, &ApplyOptions::default()).unwrap();
    adapter.apply(&pack, &ApplyOptions::default()).unwrap();

    let config = read_json(&home.path().join(".claude/settings.json"));
    assert_eq!(config["model"], "claude-3-5-sonnet");
}

// ── remove: settings ──────────────────────────────────────────────────────────

#[test]
fn remove_settings_restores_original() {
    let home = TempDir::new().unwrap();
    setup_claude_home(&home);
    let adapter = make_adapter(&home);

    let settings_path = home.path().join(".claude/settings.json");
    std::fs::write(&settings_path, r#"{"theme": "dark"}"#).unwrap();

    let _fixture = StoreFixture::create(
        "settings-restore",
        None,
        Some(r#"{"theme": "monokai"}"#),
        None,
    );

    let pack = pack_with_servers("settings-restore", vec![]);
    adapter.apply(&pack, &ApplyOptions::default()).unwrap();

    let config = read_json(&settings_path);
    assert_eq!(config["theme"], "monokai", "theme should be changed");

    adapter.remove("settings-restore").unwrap();

    let config = read_json(&settings_path);
    assert_eq!(config["theme"], "dark", "original theme should be restored");
}

#[test]
fn remove_deletes_settings_key_added_by_pack() {
    let home = TempDir::new().unwrap();
    setup_claude_home(&home);
    let adapter = make_adapter(&home);

    let _fixture = StoreFixture::create(
        "settings-delete",
        None,
        Some(r#"{"newKey": "newVal"}"#),
        None,
    );

    let pack = pack_with_servers("settings-delete", vec![]);
    adapter.apply(&pack, &ApplyOptions::default()).unwrap();
    adapter.remove("settings-delete").unwrap();

    let settings_path = home.path().join(".claude/settings.json");
    let config = read_json(&settings_path);
    assert!(
        config.get("newKey").is_none() || config["newKey"].is_null(),
        "key added by pack should be removed"
    );
}

// ── diagnose ──────────────────────────────────────────────────────────────────

#[test]
fn diagnose_returns_no_issues_on_clean_state() {
    let home = TempDir::new().unwrap();
    setup_claude_home(&home);
    let adapter = make_adapter(&home);

    let pack = pack_with_servers("diag-pack", vec![simple_server("diag-server")]);
    adapter.apply(&pack, &ApplyOptions::default()).unwrap();

    let issues = adapter.diagnose().unwrap();
    assert!(
        issues.is_empty(),
        "clean state should have no diagnostic issues"
    );
}

#[test]
fn diagnose_reports_server_missing_from_claude_json() {
    let home = TempDir::new().unwrap();
    setup_claude_home(&home);
    let adapter = make_adapter(&home);

    let pack = pack_with_servers("diag-missing", vec![simple_server("missing-server")]);
    adapter.apply(&pack, &ApplyOptions::default()).unwrap();

    // Manually wipe .claude.json without updating the manifest.
    std::fs::write(home.path().join(".claude.json"), "{}").unwrap();

    let issues = adapter.diagnose().unwrap();
    assert!(
        !issues.is_empty(),
        "should report issue when server is missing from .claude.json"
    );
    let msg = &issues[0].message;
    assert!(
        msg.contains("missing-server"),
        "issue should name the missing server"
    );
}

#[test]
fn diagnose_reports_prompt_block_missing_from_claude_md() {
    let home = TempDir::new().unwrap();
    setup_claude_home(&home);
    let adapter = make_adapter(&home);

    let _fixture = StoreFixture::create("diag-prompt", Some("Diagnose me."), None, None);

    let pack = pack_with_servers("diag-prompt", vec![]);
    adapter.apply(&pack, &ApplyOptions::default()).unwrap();

    // Wipe CLAUDE.md so the tracked block is missing.
    let claude_md = home.path().join(".claude/CLAUDE.md");
    std::fs::write(&claude_md, "").unwrap();

    let issues = adapter.diagnose().unwrap();
    assert!(
        !issues.is_empty(),
        "should report issue when prompt block is missing"
    );
    let any_prompt_issue = issues.iter().any(|i| i.message.contains("diag-prompt"));
    assert!(any_prompt_issue, "issue should name the pack");
}

#[test]
fn diagnose_reports_command_file_missing() {
    let home = TempDir::new().unwrap();
    setup_claude_home(&home);
    let adapter = make_adapter(&home);

    let _fixture = StoreFixture::create("diag-cmd", None, None, Some(&[("check.md", "# Check")]));

    let pack = pack_with_servers("diag-cmd", vec![]);
    adapter.apply(&pack, &ApplyOptions::default()).unwrap();

    // Manually delete the command file without updating the manifest.
    let cmd_file = home.path().join(".claude/commands/diag-cmd__check.md");
    std::fs::remove_file(&cmd_file).unwrap();

    let issues = adapter.diagnose().unwrap();
    assert!(
        !issues.is_empty(),
        "should report issue when command file is missing"
    );
    let any_cmd_issue = issues
        .iter()
        .any(|i| i.message.contains("diag-cmd__check.md"));
    assert!(any_cmd_issue, "issue should name the missing file");
}

// ── error cases ───────────────────────────────────────────────────────────────

#[test]
fn apply_two_packs_conflict_on_same_server_name() {
    let home = TempDir::new().unwrap();
    setup_claude_home(&home);
    let adapter = make_adapter(&home);

    let pack_a = pack_with_servers("conflict-a", vec![simple_server("shared-server")]);
    let pack_b = pack_with_servers("conflict-b", vec![simple_server("shared-server")]);

    adapter.apply(&pack_a, &ApplyOptions::default()).unwrap();
    let result = adapter.apply(&pack_b, &ApplyOptions::default());
    assert!(
        result.is_err(),
        "second pack should fail on conflicting server name"
    );
    let msg = result.unwrap_err().to_string();
    assert!(
        msg.contains("shared-server"),
        "error should name the conflicting server"
    );
}

#[test]
fn apply_rejects_malformed_claude_json() {
    let home = TempDir::new().unwrap();
    setup_claude_home(&home);
    let adapter = make_adapter(&home);

    std::fs::write(home.path().join(".claude.json"), "[1, 2, 3]").unwrap();

    let pack = pack_with_servers("bad-pack", vec![simple_server("any-server")]);
    let result = adapter.apply(&pack, &ApplyOptions::default());
    assert!(result.is_err(), "should fail on non-object .claude.json");
}

#[test]
fn apply_rejects_non_object_mcp_servers() {
    let home = TempDir::new().unwrap();
    setup_claude_home(&home);
    let adapter = make_adapter(&home);

    std::fs::write(
        home.path().join(".claude.json"),
        r#"{"mcpServers": "not-an-object"}"#,
    )
    .unwrap();

    let pack = pack_with_servers("bad-pack", vec![simple_server("any-server")]);
    let result = adapter.apply(&pack, &ApplyOptions::default());
    assert!(
        result.is_err(),
        "should fail when mcpServers is not an object"
    );
}

#[test]
fn apply_http_server_writes_url() {
    let home = TempDir::new().unwrap();
    setup_claude_home(&home);
    let adapter = make_adapter(&home);

    let server = McpServer {
        name: "http-server".into(),
        package_type: None,
        package: None,
        command: None,
        args: vec![],
        url: Some("https://example.com/mcp".into()),
        headers: Some(
            [("Authorization".to_string(), "Bearer ${TOKEN}".to_string())]
                .into_iter()
                .collect(),
        ),
        transport: Some(Transport::Http),
        tools: vec![],
        env: std::collections::HashMap::new(),
    };
    let pack = pack_with_servers("http-pack", vec![server]);
    adapter.apply(&pack, &ApplyOptions::default()).unwrap();

    let config = read_json(&home.path().join(".claude.json"));
    let entry = &config["mcpServers"]["http-server"];
    assert_eq!(entry["type"], "http", "type must be 'http'");
    assert_eq!(
        entry["url"], "https://example.com/mcp",
        "url must be written"
    );
    assert_eq!(
        entry["headers"]["Authorization"], "Bearer ${TOKEN}",
        "headers must be written"
    );
    assert!(
        entry["command"].is_null(),
        "command must not appear in HTTP server config"
    );
}

#[test]
fn apply_http_server_without_url_returns_error() {
    let home = TempDir::new().unwrap();
    setup_claude_home(&home);
    let adapter = make_adapter(&home);

    let server = McpServer {
        name: "no-url-server".into(),
        package_type: None,
        package: None,
        command: None,
        args: vec![],
        url: None, // missing url
        headers: None,
        transport: Some(Transport::Http),
        tools: vec![],
        env: std::collections::HashMap::new(),
    };
    let pack = pack_with_servers("bad-http-pack", vec![server]);
    let result = adapter.apply(&pack, &ApplyOptions::default());
    assert!(result.is_err(), "should fail when HTTP server has no url");
    let msg = result.unwrap_err().to_string();
    assert!(
        msg.contains("url"),
        "error message should mention the missing url field"
    );
}

#[test]
fn remove_http_server_cleans_up() {
    let home = TempDir::new().unwrap();
    setup_claude_home(&home);
    let adapter = make_adapter(&home);

    let server = McpServer {
        name: "http-removable".into(),
        package_type: None,
        package: None,
        command: None,
        args: vec![],
        url: Some("https://example.com/mcp".into()),
        headers: Some(
            [("Authorization".to_string(), "Bearer ${TOKEN}".to_string())]
                .into_iter()
                .collect(),
        ),
        transport: Some(Transport::Http),
        tools: vec![],
        env: std::collections::HashMap::new(),
    };
    let pack = pack_with_servers("http-remove-pack", vec![server]);
    adapter.apply(&pack, &ApplyOptions::default()).unwrap();

    // Verify it was written
    let config = read_json(&home.path().join(".claude.json"));
    assert!(
        config["mcpServers"]["http-removable"].is_object(),
        "HTTP server should be present after apply"
    );

    // Remove
    adapter.remove("http-remove-pack").unwrap();

    let config_after = read_json(&home.path().join(".claude.json"));
    assert!(
        config_after["mcpServers"]
            .as_object()
            .map(|m| !m.contains_key("http-removable"))
            .unwrap_or(true),
        "HTTP server should be removed after remove()"
    );
}

#[test]
fn apply_persists_manifest_after_each_step_even_if_later_step_fails() {
    let home = TempDir::new().unwrap();
    setup_claude_home(&home);
    let adapter = make_adapter(&home);

    // Create a pack with servers + malformed settings (will fail at apply_settings).
    let _fixture = StoreFixture::create("mid-fail-pack", None, Some("NOT VALID JSON {{{"), None);

    let pack = pack_with_servers("mid-fail-pack", vec![simple_server("my-server")]);
    let result = adapter.apply(&pack, &ApplyOptions::default());

    // apply() should fail because of the invalid settings JSON.
    assert!(result.is_err(), "apply should fail on invalid settings");

    // But the manifest should still record the server that was successfully applied
    // before the settings step failed.
    let manifest_path = home.path().join(".claude/.packweave_manifest.json");
    assert!(
        manifest_path.exists(),
        "manifest should exist after partial apply"
    );
    let manifest = read_json(&manifest_path);
    let servers = manifest["servers"]
        .as_object()
        .expect("servers should be an object");
    assert!(
        servers.contains_key("my-server"),
        "manifest should record the server written before the failure"
    );
    assert_eq!(
        servers["my-server"].as_str().unwrap(),
        "mid-fail-pack",
        "server should be owned by the failing pack"
    );
}

/// Issue #118: when `apply_project_settings` fails mid-apply (after
/// `apply_project_servers` already wrote `.mcp.json`), the user-scope manifest
/// must still contain the project root in `project_dirs`. This ensures that a
/// subsequent `weave remove` from any working directory can clean up the
/// project-scope state.
#[test]
fn mid_apply_failure_records_project_root_and_remove_cleans_up() {
    let home = TempDir::new().unwrap();
    let project = TempDir::new().unwrap();
    setup_claude_home(&home);

    // The pack has a valid server and valid settings in the store, so user-scope
    // apply succeeds. The failure is injected at project scope by placing a
    // malformed settings.json in the project's .claude/ directory. When
    // apply_project_settings tries to deep-merge into that file it will fail
    // parsing the existing content.
    let _fixture = StoreFixture::create("mid-proj-fail", None, Some(r#"{"theme": "dark"}"#), None);

    // Pre-create the project-scope settings file with invalid JSON so that
    // apply_project_settings fails when it tries to read the existing content.
    let proj_claude_dir = project.path().join(".claude");
    std::fs::create_dir_all(&proj_claude_dir).unwrap();
    std::fs::write(proj_claude_dir.join("settings.json"), "NOT VALID JSON {{{").unwrap();

    let adapter = make_adapter_with_project(&home, &project);
    let pack = pack_with_servers("mid-proj-fail", vec![simple_server("mid-proj-srv")]);

    let result = adapter.apply(&pack, &ApplyOptions::default());
    assert!(
        result.is_err(),
        "apply should fail due to malformed project-scope settings.json"
    );

    // ── Assert 1: project_dirs in user-scope manifest contains the project root ──
    let manifest_path = home.path().join(".claude/.packweave_manifest.json");
    assert!(
        manifest_path.exists(),
        "user-scope manifest must exist after partial apply"
    );
    let manifest = read_json(&manifest_path);

    let project_root_abs = project
        .path()
        .canonicalize()
        .unwrap()
        .to_string_lossy()
        .to_string();
    let project_dirs = manifest["project_dirs"]["mid-proj-fail"]
        .as_array()
        .expect("project_dirs should contain an entry for the pack");
    let recorded_roots: Vec<&str> = project_dirs.iter().filter_map(|v| v.as_str()).collect();
    assert!(
        recorded_roots.contains(&project_root_abs.as_str()),
        "project root should be recorded before the failing step; got {recorded_roots:?}"
    );

    // ── Assert 2: .mcp.json was written (apply_project_servers ran) ──
    let proj_mcp = project.path().join(".mcp.json");
    assert!(
        proj_mcp.exists(),
        ".mcp.json should exist because apply_project_servers ran before the failure"
    );
    let mcp_config = read_json(&proj_mcp);
    assert!(
        mcp_config["mcpServers"]["mid-proj-srv"].is_object(),
        "project-scope server entry should be present in .mcp.json"
    );

    // ── Assert 3: remove from a DIFFERENT cwd still cleans up ──
    // Create an adapter rooted in a completely different directory (simulating
    // `weave remove` invoked from outside the project).
    let other_cwd = TempDir::new().unwrap();
    let remove_adapter = ClaudeCodeAdapter::with_home_and_project(
        home.path().to_path_buf(),
        other_cwd.path().to_path_buf(),
    );
    remove_adapter
        .remove("mid-proj-fail")
        .expect("remove should succeed from a different working directory");

    // .mcp.json should be cleaned up (deleted, since it was the only server).
    assert!(
        !proj_mcp.exists(),
        ".mcp.json should be deleted after remove cleans up the project scope"
    );

    // project_dirs should no longer contain the pack entry.
    let manifest_after = read_json(&manifest_path);
    assert!(
        manifest_after
            .get("project_dirs")
            .and_then(|d| d.get("mid-proj-fail"))
            .is_none(),
        "project_dirs entry should be removed after successful cleanup"
    );
}

// ── Hooks tests ──────────────────────────────────────────────────────────────

use packweave::core::pack::PackExtensions;

fn pack_with_hooks(name: &str) -> ResolvedPack {
    let hooks_json = serde_json::json!({
        "hooks": {
            "PreToolUse": [
                { "matcher": "Bash", "command": "echo pre-bash" }
            ],
            "PostToolUse": [
                { "command": "echo post-all" }
            ]
        }
    });
    ResolvedPack {
        pack: Pack {
            name: name.to_string(),
            version: semver::Version::new(1, 0, 0),
            description: "Pack with hooks".into(),
            authors: vec![],
            license: None,
            repository: None,
            keywords: vec![],
            min_tool_version: None,
            servers: vec![],
            dependencies: HashMap::new(),
            extensions: PackExtensions {
                claude_code: Some(hooks_json),
                gemini_cli: None,
                codex_cli: None,
            },
            targets: PackTargets::default(),
        },
        source: PackSource::Registry {
            registry_url: "https://example.com".into(),
        },
    }
}

#[test]
fn apply_hooks_when_allowed() {
    let home = TempDir::new().unwrap();
    let adapter = make_adapter(&home);
    setup_claude_home(&home);

    let pack = pack_with_hooks("hooks-pack");
    let options = ApplyOptions { allow_hooks: true };
    adapter.apply(&pack, &options).unwrap();

    // Verify hooks were written to settings.json
    let settings_path = home.path().join(".claude").join("settings.json");
    let settings = read_json(&settings_path);
    let hooks = settings.get("hooks").expect("hooks key should exist");
    let pre = hooks
        .get("PreToolUse")
        .expect("PreToolUse should exist")
        .as_array()
        .unwrap();
    assert_eq!(pre.len(), 1);
    assert_eq!(pre[0]["matcher"], "Bash");
    assert_eq!(pre[0]["hooks"][0]["command"], "echo pre-bash");
    assert_eq!(pre[0]["hooks"][0]["type"], "command");

    let post = hooks
        .get("PostToolUse")
        .expect("PostToolUse should exist")
        .as_array()
        .unwrap();
    assert_eq!(post.len(), 1);
    assert_eq!(post[0]["hooks"][0]["command"], "echo post-all");
}

#[test]
fn apply_skips_hooks_when_not_allowed() {
    let home = TempDir::new().unwrap();
    let adapter = make_adapter(&home);
    setup_claude_home(&home);

    let pack = pack_with_hooks("hooks-pack");
    let options = ApplyOptions { allow_hooks: false };
    adapter.apply(&pack, &options).unwrap();

    // settings.json should not exist (no servers/settings/hooks were written)
    let settings_path = home.path().join(".claude").join("settings.json");
    assert!(
        !settings_path.exists(),
        "settings.json should not be created when hooks are not allowed and there are no other settings"
    );
}

#[test]
fn remove_cleans_up_hooks() {
    let home = TempDir::new().unwrap();
    let adapter = make_adapter(&home);
    setup_claude_home(&home);

    let pack = pack_with_hooks("hooks-pack");
    let options = ApplyOptions { allow_hooks: true };
    adapter.apply(&pack, &options).unwrap();

    // Verify hooks were written
    let settings_path = home.path().join(".claude").join("settings.json");
    assert!(settings_path.exists());
    let settings = read_json(&settings_path);
    assert!(settings.get("hooks").is_some());

    // Remove the pack
    adapter.remove("hooks-pack").unwrap();

    // settings.json should have hooks removed
    if settings_path.exists() {
        let settings_after = read_json(&settings_path);
        assert!(
            settings_after.get("hooks").is_none(),
            "hooks key should be removed after pack removal"
        );
    }
}

#[test]
fn has_hooks_detects_extension_hooks() {
    let pack = pack_with_hooks("hooks-pack");
    assert!(pack.pack.has_hooks(), "pack with hooks should return true");

    let no_hooks = pack_with_servers("no-hooks", vec![simple_server("srv")]);
    assert!(
        !no_hooks.pack.has_hooks(),
        "pack without hooks should return false"
    );
}
