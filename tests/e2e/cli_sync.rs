use super::helpers::*;
use assert_cmd::prelude::*;
use predicates::prelude::*;

#[tokio::test]
async fn sync_empty_profile_noop() {
    let env = TestEnv::new().await;

    env.weave_cmd()
        .args(["sync"])
        .assert()
        .success()
        .stdout(predicate::str::contains("nothing to sync").from_utf8());
}

#[tokio::test]
async fn sync_reapplies_installed_pack() {
    let env = TestEnv::new().await;
    let pack =
        FixturePack::new("test-pack", "1.0.0").with_server("echo-server", "echo", &["hello"]);

    mount_registry(&env.mock_server, &[&pack]).await;

    // Install the pack
    env.weave_cmd()
        .args(["install", "test-pack"])
        .assert()
        .success();

    // Verify the server is in claude.json after install
    let claude_json = env.claude_json();
    assert!(
        claude_json.exists(),
        "claude.json should exist after install"
    );

    // Run sync
    env.weave_cmd()
        .args(["sync"])
        .assert()
        .success()
        .stdout(predicate::str::contains("test-pack@1.0.0").from_utf8())
        .stdout(predicate::str::contains("Sync complete").from_utf8());

    // Verify the server is still present after sync
    let content = std::fs::read_to_string(&claude_json).unwrap();
    assert!(
        content.contains("echo-server"),
        "echo-server should still be in claude.json after sync"
    );
}

#[tokio::test]
async fn sync_recovers_from_drift() {
    let env = TestEnv::new().await;
    let pack =
        FixturePack::new("test-pack", "1.0.0").with_server("echo-server", "echo", &["hello"]);

    mount_registry(&env.mock_server, &[&pack]).await;

    // Install the pack
    env.weave_cmd()
        .args(["install", "test-pack"])
        .assert()
        .success();

    // Simulate drift: remove the server entry from claude.json
    let claude_json = env.claude_json();
    let content = std::fs::read_to_string(&claude_json).unwrap();
    let mut config: serde_json::Value = serde_json::from_str(&content).unwrap();
    if let Some(servers) = config.get_mut("mcpServers").and_then(|v| v.as_object_mut()) {
        servers.remove("echo-server");
    }
    std::fs::write(&claude_json, serde_json::to_string_pretty(&config).unwrap()).unwrap();

    // Verify drift: server should be gone
    let content_after_drift = std::fs::read_to_string(&claude_json).unwrap();
    assert!(
        !content_after_drift.contains("echo-server"),
        "echo-server should be removed to simulate drift"
    );

    // Run sync to recover
    env.weave_cmd()
        .args(["sync"])
        .assert()
        .success()
        .stdout(predicate::str::contains("Sync complete").from_utf8());

    // Verify the server is restored
    let content_after_sync = std::fs::read_to_string(&claude_json).unwrap();
    assert!(
        content_after_sync.contains("echo-server"),
        "echo-server should be restored after sync"
    );
}

#[tokio::test]
async fn sync_idempotent() {
    let env = TestEnv::new().await;
    let pack =
        FixturePack::new("test-pack", "1.0.0").with_server("echo-server", "echo", &["hello"]);

    mount_registry(&env.mock_server, &[&pack]).await;

    // Install the pack
    env.weave_cmd()
        .args(["install", "test-pack"])
        .assert()
        .success();

    // First sync
    env.weave_cmd()
        .args(["sync"])
        .assert()
        .success()
        .stdout(predicate::str::contains("Sync complete").from_utf8());

    // Capture state after first sync
    let claude_json = env.claude_json();
    let content_after_first_sync = std::fs::read_to_string(&claude_json).unwrap();

    // Second sync
    env.weave_cmd()
        .args(["sync"])
        .assert()
        .success()
        .stdout(predicate::str::contains("Sync complete").from_utf8());

    // State should be identical after second sync
    let content_after_second_sync = std::fs::read_to_string(&claude_json).unwrap();
    assert_eq!(
        content_after_first_sync, content_after_second_sync,
        "claude.json should be identical after running sync twice"
    );
}
