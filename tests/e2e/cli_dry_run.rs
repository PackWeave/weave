use super::helpers::*;
use assert_cmd::prelude::*;
use predicates::prelude::*;

// ── Install dry-run ─────────────────────────────────────────────────────────

#[tokio::test]
async fn install_dry_run_shows_preview_without_writing() {
    let env = TestEnv::new().await;
    let pack =
        FixturePack::new("test-pack", "1.0.0").with_server("echo-server", "echo", &["hello"]);

    mount_registry(&env.mock_server, &[&pack]).await;

    env.weave_cmd()
        .args(["install", "--dry-run", "test-pack"])
        .assert()
        .success()
        .stdout(predicate::str::contains("Dry run").from_utf8())
        .stdout(predicate::str::contains("test-pack").from_utf8());

    // Profile must NOT be written.
    let profile_path = env.profile_toml("default");
    let profile_content = std::fs::read_to_string(&profile_path).unwrap_or_default();
    assert!(
        !profile_content.contains("test-pack"),
        "dry-run must not record pack in profile"
    );

    // claude.json must NOT contain the server.
    let claude_json = env.claude_json();
    if claude_json.exists() {
        let content = std::fs::read_to_string(&claude_json).unwrap();
        assert!(
            !content.contains("echo-server"),
            "dry-run must not write server to claude.json"
        );
    }

    // Store must NOT contain the pack.
    let pack_store_dir = env.store_dir.path().join("packs/test-pack");
    assert!(
        !pack_store_dir.exists(),
        "dry-run must not write pack to store"
    );
}

#[tokio::test]
async fn install_dry_run_does_not_affect_lockfile() {
    let env = TestEnv::new().await;
    let pack =
        FixturePack::new("test-pack", "1.0.0").with_server("echo-server", "echo", &["hello"]);

    mount_registry(&env.mock_server, &[&pack]).await;

    env.weave_cmd()
        .args(["install", "--dry-run", "test-pack"])
        .assert()
        .success();

    let lockfile_path = env.lockfile_path("default");
    let lockfile_content = std::fs::read_to_string(&lockfile_path).unwrap_or_default();
    assert!(
        !lockfile_content.contains("test-pack"),
        "dry-run must not write pack to lockfile"
    );
}

// ── Remove dry-run ──────────────────────────────────────────────────────────

#[tokio::test]
async fn remove_dry_run_shows_preview_without_removing() {
    let env = TestEnv::new().await;
    let pack =
        FixturePack::new("test-pack", "1.0.0").with_server("echo-server", "echo", &["hello"]);

    mount_registry(&env.mock_server, &[&pack]).await;

    // Actually install first.
    env.weave_cmd()
        .args(["install", "test-pack"])
        .assert()
        .success();

    // Dry-run remove.
    env.weave_cmd()
        .args(["remove", "--dry-run", "test-pack"])
        .assert()
        .success()
        .stdout(predicate::str::contains("Dry run").from_utf8())
        .stdout(predicate::str::contains("test-pack").from_utf8());

    // Pack must still be installed.
    let profile_content = std::fs::read_to_string(env.profile_toml("default")).unwrap();
    assert!(
        profile_content.contains("test-pack"),
        "dry-run remove must not actually remove pack from profile"
    );

    // Server must still be in claude.json.
    let claude_json = env.claude_json();
    let content = std::fs::read_to_string(&claude_json).unwrap();
    assert!(
        content.contains("echo-server"),
        "dry-run remove must not remove server from claude.json"
    );
}

// ── Sync dry-run ────────────────────────────────────────────────────────────

#[tokio::test]
async fn sync_dry_run_shows_preview_without_applying() {
    let env = TestEnv::new().await;
    let pack =
        FixturePack::new("test-pack", "1.0.0").with_server("echo-server", "echo", &["hello"]);

    mount_registry(&env.mock_server, &[&pack]).await;

    // Install the pack first.
    env.weave_cmd()
        .args(["install", "test-pack"])
        .assert()
        .success();

    // Simulate drift: remove server from claude.json.
    let claude_json = env.claude_json();
    let content = std::fs::read_to_string(&claude_json).unwrap();
    let mut config: serde_json::Value = serde_json::from_str(&content).unwrap();
    if let Some(servers) = config.get_mut("mcpServers").and_then(|v| v.as_object_mut()) {
        servers.remove("echo-server");
    }
    std::fs::write(&claude_json, serde_json::to_string_pretty(&config).unwrap()).unwrap();

    // Dry-run sync.
    env.weave_cmd()
        .args(["sync", "--dry-run"])
        .assert()
        .success()
        .stdout(predicate::str::contains("Dry run").from_utf8())
        .stdout(predicate::str::contains("test-pack").from_utf8());

    // Server must still be missing (dry-run didn't re-apply).
    let content_after = std::fs::read_to_string(&claude_json).unwrap();
    assert!(
        !content_after.contains("echo-server"),
        "dry-run sync must not re-apply servers to claude.json"
    );
}

// ── Local install dry-run ───────────────────────────────────────────────────

#[cfg(not(target_os = "windows"))]
#[tokio::test]
async fn install_local_dry_run_does_not_write_to_store() {
    let env = TestEnv::new().await;

    let pack_dir = env.project_dir.path().join("my-local-pack");
    std::fs::create_dir_all(&pack_dir).unwrap();
    std::fs::write(
        pack_dir.join("pack.toml"),
        "[pack]\nname = \"my-local-pack\"\nversion = \"0.1.0\"\ndescription = \"local test\"\nauthors = [\"tester\"]\n",
    )
    .unwrap();

    env.weave_cmd()
        .args(["install", "--dry-run", "./my-local-pack"])
        .assert()
        .success()
        .stdout(predicate::str::contains("Dry run").from_utf8());

    // Store must NOT contain the pack.
    let pack_store_dir = env.store_dir.path().join("packs/my-local-pack");
    assert!(
        !pack_store_dir.exists(),
        "dry-run local install must not write pack to store"
    );

    // Profile must NOT contain the pack.
    let profile_content = std::fs::read_to_string(env.profile_toml("default")).unwrap_or_default();
    assert!(
        !profile_content.contains("my-local-pack"),
        "dry-run local install must not record pack in profile"
    );
}
