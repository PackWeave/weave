use predicates::prelude::*;

use super::helpers::*;
use assert_cmd::prelude::*;

#[tokio::test]
async fn use_prints_active_profile() {
    let env = TestEnv::new().await;

    env.weave_cmd()
        .args(["use"])
        .assert()
        .success()
        .stdout(predicate::str::contains("default").from_utf8());
}

#[tokio::test]
async fn use_switch_profile() {
    let env = TestEnv::new().await;
    let pack =
        FixturePack::new("test-pack", "1.0.0").with_server("echo-server", "echo", &["hello"]);

    mount_registry(&env.mock_server, &[&pack]).await;

    // Create a profile and add a pack to it
    env.weave_cmd()
        .args(["profile", "create", "work"])
        .assert()
        .success();

    env.weave_cmd()
        .args(["profile", "add", "test-pack", "--profile", "work"])
        .assert()
        .success();

    // Switch to the profile
    env.weave_cmd()
        .args(["use", "work"])
        .assert()
        .success()
        .stdout(predicate::str::contains("Switched to profile 'work'").from_utf8());

    // Verify the pack was applied to the adapter
    let claude_json = env.claude_json();
    assert!(
        claude_json.exists(),
        "~/.claude.json should exist after switching to profile with a pack"
    );
    let content = std::fs::read_to_string(&claude_json).unwrap();
    assert!(
        content.contains("echo-server"),
        "echo-server should be in ~/.claude.json after switching to profile"
    );
}

#[tokio::test]
async fn use_nonexistent_profile_fails() {
    let env = TestEnv::new().await;

    env.weave_cmd()
        .args(["use", "no-such-profile"])
        .assert()
        .failure();
}

#[tokio::test]
async fn use_already_active_noop() {
    let env = TestEnv::new().await;

    env.weave_cmd()
        .args(["use", "default"])
        .assert()
        .success()
        .stdout(predicate::str::contains("Already on profile").from_utf8());
}

#[tokio::test]
async fn use_switch_removes_old_packs() {
    let env = TestEnv::new().await;
    let pack = FixturePack::new("old-pack", "1.0.0").with_server("old-server", "echo", &["old"]);

    mount_registry(&env.mock_server, &[&pack]).await;

    // Install a pack on the default profile
    env.weave_cmd()
        .args(["install", "old-pack"])
        .assert()
        .success();

    // Verify the server is present
    let content = std::fs::read_to_string(env.claude_json()).unwrap();
    assert!(
        content.contains("old-server"),
        "old-server should be in ~/.claude.json after install"
    );

    // Create an empty profile and switch to it
    env.weave_cmd()
        .args(["profile", "create", "empty"])
        .assert()
        .success();

    env.weave_cmd().args(["use", "empty"]).assert().success();

    // Verify the old pack's server was removed
    let content = std::fs::read_to_string(env.claude_json()).unwrap();
    let json: serde_json::Value = serde_json::from_str(&content).unwrap();
    let servers = json["mcpServers"].as_object();
    assert!(
        servers.map_or(true, |m| !m.contains_key("old-server")),
        "old-server should be removed from ~/.claude.json after switching to empty profile"
    );
}

#[tokio::test]
async fn use_switch_applies_new_packs() {
    let env = TestEnv::new().await;
    let pack_a = FixturePack::new("pack-a", "1.0.0").with_server("server-a", "echo", &["a"]);
    let pack_b = FixturePack::new("pack-b", "1.0.0").with_server("server-b", "echo", &["b"]);

    mount_registry(&env.mock_server, &[&pack_a, &pack_b]).await;

    // Install pack-a on the default profile
    env.weave_cmd()
        .args(["install", "pack-a"])
        .assert()
        .success();

    // Create profile-b and add pack-b to it
    env.weave_cmd()
        .args(["profile", "create", "profile-b"])
        .assert()
        .success();

    env.weave_cmd()
        .args(["profile", "add", "pack-b", "--profile", "profile-b"])
        .assert()
        .success();

    // Switch from default (pack-a) to profile-b (pack-b)
    env.weave_cmd()
        .args(["use", "profile-b"])
        .assert()
        .success();

    // Verify server-a was removed and server-b was applied
    let content = std::fs::read_to_string(env.claude_json()).unwrap();
    let json: serde_json::Value = serde_json::from_str(&content).unwrap();
    let servers = json["mcpServers"]
        .as_object()
        .expect("mcpServers should exist");

    assert!(
        !servers.contains_key("server-a"),
        "server-a should be removed after switching away from default profile"
    );
    assert!(
        servers.contains_key("server-b"),
        "server-b should be applied after switching to profile-b"
    );
}
