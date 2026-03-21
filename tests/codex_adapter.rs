/// Integration tests for the Codex CLI adapter.
///
/// These tests use `TempDir` for the simulated Codex home directory and never
/// write to the real `~/.codex/`. Tests that exercise prompts or settings also
/// create temporary pack entries in a redirected store configured via
/// `WEAVE_TEST_STORE_DIR`, managed by a `StoreFixture` drop guard, so the real
/// `~/.packweave/packs/` is never touched.
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::OnceLock;

use packweave::adapters::codex_cli::CodexAdapter;
use packweave::adapters::CliAdapter;
use packweave::core::pack::{Pack, PackSource, PackTargets, ResolvedPack};
use packweave::core::store::Store;
use tempfile::TempDir;

// ── Helpers ───────────────────────────────────────────────────────────────────

fn make_adapter(home: &TempDir) -> CodexAdapter {
    let no_project = home.path().join("no-project");
    std::fs::create_dir_all(&no_project).unwrap();
    CodexAdapter::with_home_and_project(home.path().to_path_buf(), no_project)
}

fn setup_codex_home(home: &TempDir) {
    std::fs::create_dir_all(home.path().join(".codex")).unwrap();
}

fn read_file(path: &std::path::Path) -> String {
    std::fs::read_to_string(path).expect("file should exist")
}

fn read_json(path: &std::path::Path) -> serde_json::Value {
    let content = read_file(path);
    serde_json::from_str(&content).expect("file should be valid JSON")
}

// ── Pack builder ──────────────────────────────────────────────────────────────

fn bare_pack(name: &str) -> ResolvedPack {
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

fn pack_not_targeting_codex(name: &str) -> ResolvedPack {
    let mut p = bare_pack(name);
    p.pack.targets.codex_cli = false;
    p
}

// ── StoreFixture: isolated pack store ─────────────────────────────────────────

fn shared_store_root() -> &'static TempDir {
    static STORE: OnceLock<TempDir> = OnceLock::new();
    STORE.get_or_init(|| {
        let dir = TempDir::new().expect("shared store TempDir");
        std::env::set_var("WEAVE_TEST_STORE_DIR", dir.path());
        dir
    })
}

struct StoreFixture {
    pack_dir: PathBuf,
}

impl StoreFixture {
    fn create(name: &str, prompt: Option<&str>, settings: Option<&str>) -> Self {
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

        if let Some(content) = settings {
            std::fs::create_dir_all(pack_dir.join("settings")).unwrap();
            std::fs::write(pack_dir.join("settings/codex.json"), content).unwrap();
        }

        Self { pack_dir }
    }
}

impl Drop for StoreFixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.pack_dir);
    }
}

// ── apply: prompts ────────────────────────────────────────────────────────────

#[test]
fn apply_prompts_appends_to_agents_md() {
    let home = TempDir::new().unwrap();
    setup_codex_home(&home);
    let adapter = make_adapter(&home);
    let _fixture = StoreFixture::create("prompt-pack", Some("Be concise and accurate."), None);

    adapter.apply(&bare_pack("prompt-pack")).unwrap();

    let agents_md = home.path().join(".codex/AGENTS.md");
    assert!(agents_md.exists());
    let content = read_file(&agents_md);
    assert!(content.contains("Be concise and accurate."));
    assert!(content.contains("<!-- packweave:begin:prompt-pack -->"));
    assert!(content.contains("<!-- packweave:end:prompt-pack -->"));
}

#[test]
fn apply_prompts_idempotent() {
    let home = TempDir::new().unwrap();
    setup_codex_home(&home);
    let adapter = make_adapter(&home);
    let _fixture = StoreFixture::create("idem-prompt", Some("Idempotent content."), None);

    adapter.apply(&bare_pack("idem-prompt")).unwrap();
    adapter.apply(&bare_pack("idem-prompt")).unwrap();

    let content = read_file(&home.path().join(".codex/AGENTS.md"));
    assert_eq!(
        content
            .matches("<!-- packweave:begin:idem-prompt -->")
            .count(),
        1,
        "prompt block should appear exactly once"
    );
}

#[test]
fn apply_prompts_appends_to_existing_agents_md() {
    let home = TempDir::new().unwrap();
    setup_codex_home(&home);
    let agents_md = home.path().join(".codex/AGENTS.md");
    std::fs::write(
        &agents_md,
        "# My existing instructions\n\nExisting content.\n",
    )
    .unwrap();
    let adapter = make_adapter(&home);
    let _fixture = StoreFixture::create("append-pack", Some("New pack content."), None);

    adapter.apply(&bare_pack("append-pack")).unwrap();

    let content = read_file(&agents_md);
    assert!(content.contains("# My existing instructions"));
    assert!(content.contains("New pack content."));
}

#[test]
fn remove_prompts_cleans_section() {
    let home = TempDir::new().unwrap();
    setup_codex_home(&home);
    let adapter = make_adapter(&home);
    let _fixture = StoreFixture::create("rm-prompt", Some("Remove me."), None);

    adapter.apply(&bare_pack("rm-prompt")).unwrap();
    adapter.remove("rm-prompt").unwrap();

    let agents_md = home.path().join(".codex/AGENTS.md");
    if agents_md.exists() {
        let content = read_file(&agents_md);
        assert!(!content.contains("Remove me."));
        assert!(!content.contains("<!-- packweave:begin:rm-prompt -->"));
    }
}

#[test]
fn remove_prompts_is_surgical() {
    let home = TempDir::new().unwrap();
    setup_codex_home(&home);
    let adapter = make_adapter(&home);
    let _fx_a = StoreFixture::create("keep-pack", Some("Keep this."), None);
    let _fx_b = StoreFixture::create("remove-pack", Some("Remove this."), None);

    adapter.apply(&bare_pack("keep-pack")).unwrap();
    adapter.apply(&bare_pack("remove-pack")).unwrap();
    adapter.remove("remove-pack").unwrap();

    let content = read_file(&home.path().join(".codex/AGENTS.md"));
    assert!(content.contains("Keep this."));
    assert!(!content.contains("Remove this."));
}

// ── apply: settings ───────────────────────────────────────────────────────────

#[test]
fn apply_settings_merges_keys() {
    let home = TempDir::new().unwrap();
    setup_codex_home(&home);
    let adapter = make_adapter(&home);
    let _fixture = StoreFixture::create("settings-pack", None, Some(r#"{"model": "o3-mini"}"#));

    adapter.apply(&bare_pack("settings-pack")).unwrap();

    let config = read_json(&home.path().join(".codex/config.json"));
    assert_eq!(config["model"], "o3-mini");
}

#[test]
fn apply_settings_preserves_existing_keys() {
    let home = TempDir::new().unwrap();
    setup_codex_home(&home);
    std::fs::write(
        home.path().join(".codex/config.json"),
        r#"{"approvalMode": "auto"}"#,
    )
    .unwrap();
    let adapter = make_adapter(&home);
    let _fixture = StoreFixture::create("new-key", None, Some(r#"{"model": "o3"}"#));

    adapter.apply(&bare_pack("new-key")).unwrap();

    let config = read_json(&home.path().join(".codex/config.json"));
    assert_eq!(config["approvalMode"], "auto");
    assert_eq!(config["model"], "o3");
}

#[test]
fn apply_settings_is_idempotent() {
    let home = TempDir::new().unwrap();
    setup_codex_home(&home);
    let adapter = make_adapter(&home);
    let _fixture = StoreFixture::create("idem-settings", None, Some(r#"{"model": "o3-mini"}"#));

    adapter.apply(&bare_pack("idem-settings")).unwrap();
    adapter.apply(&bare_pack("idem-settings")).unwrap();

    let config = read_json(&home.path().join(".codex/config.json"));
    assert_eq!(config["model"], "o3-mini");
}

#[test]
fn remove_settings_restores_original() {
    let home = TempDir::new().unwrap();
    setup_codex_home(&home);
    let adapter = make_adapter(&home);
    let _fixture = StoreFixture::create("restore-settings", None, Some(r#"{"model": "o3-mini"}"#));

    adapter.apply(&bare_pack("restore-settings")).unwrap();
    adapter.remove("restore-settings").unwrap();

    let config_path = home.path().join(".codex/config.json");
    if config_path.exists() {
        let config = read_json(&config_path);
        assert!(config.get("model").is_none() || config["model"].is_null());
    }
}

#[test]
fn apply_settings_rejects_non_object_config_file() {
    let home = TempDir::new().unwrap();
    setup_codex_home(&home);
    std::fs::write(home.path().join(".codex/config.json"), r#"[]"#).unwrap();
    let adapter = make_adapter(&home);
    let _fixture = StoreFixture::create("bad-config", None, Some(r#"{"model": "o3"}"#));

    let result = adapter.apply(&bare_pack("bad-config"));
    assert!(result.is_err(), "should reject non-object config.json");
}

// ── apply: target filtering ───────────────────────────────────────────────────

#[test]
fn apply_skips_pack_not_targeting_codex() {
    let home = TempDir::new().unwrap();
    setup_codex_home(&home);
    let adapter = make_adapter(&home);

    adapter.apply(&pack_not_targeting_codex("skip-me")).unwrap();

    assert!(
        !home.path().join(".codex/.packweave_manifest.json").exists(),
        "manifest should not be written for non-Codex packs"
    );
}

// ── diagnose ──────────────────────────────────────────────────────────────────

#[test]
fn diagnose_returns_no_issues_on_clean_state() {
    let home = TempDir::new().unwrap();
    setup_codex_home(&home);
    let adapter = make_adapter(&home);

    let issues = adapter.diagnose().unwrap();
    assert!(issues.is_empty());
}

#[test]
fn diagnose_reports_missing_prompt_block() {
    let home = TempDir::new().unwrap();
    setup_codex_home(&home);
    let adapter = make_adapter(&home);
    let _fixture = StoreFixture::create("diagnose-pack", Some("Check me."), None);

    adapter.apply(&bare_pack("diagnose-pack")).unwrap();

    // Manually delete AGENTS.md to simulate drift.
    let _ = std::fs::remove_file(home.path().join(".codex/AGENTS.md"));

    let issues = adapter.diagnose().unwrap();
    assert!(
        issues.iter().any(|i| i.message.contains("diagnose-pack")),
        "diagnose should report missing prompt block"
    );
}
