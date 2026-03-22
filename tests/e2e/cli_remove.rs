use super::helpers::*;
use assert_cmd::prelude::*;
use tempfile::TempDir;

#[tokio::test]
async fn remove_installed_pack() {
    let env = TestEnv::new().await;
    let pack =
        FixturePack::new("test-pack", "1.0.0").with_server("echo-server", "echo", &["hello"]);

    mount_registry(&env.mock_server, &[&pack]).await;

    // Install
    env.weave_cmd()
        .args(["install", "test-pack"])
        .assert()
        .success();

    // Remove
    env.weave_cmd()
        .args(["remove", "test-pack"])
        .assert()
        .success();

    // Verify profile no longer contains the pack
    let profile_content = std::fs::read_to_string(env.profile_toml("default")).unwrap();
    assert!(
        !profile_content.contains("test-pack"),
        "profile should not contain test-pack after removal"
    );

    // Verify the MCP server was actually removed from ~/.claude.json
    let claude_json = env.claude_json();
    if claude_json.exists() {
        let content: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(&claude_json).unwrap()).unwrap();
        let servers = content["mcpServers"].as_object();
        assert!(
            servers.map_or(true, |m| !m.contains_key("echo-server")),
            "echo-server should be removed from ~/.claude.json after pack removal"
        );
    }
}

#[tokio::test]
async fn remove_not_installed() {
    let env = TestEnv::new().await;

    // Don't install anything, try to remove
    env.weave_cmd()
        .args(["remove", "nonexistent-pack"])
        .assert()
        .failure();
}

#[tokio::test]
async fn remove_preserves_other_packs() {
    let env = TestEnv::new().await;
    let pack_a = FixturePack::new("pack-a", "1.0.0").with_server("server-a", "echo", &["a"]);
    let pack_b = FixturePack::new("pack-b", "1.0.0").with_server("server-b", "echo", &["b"]);

    mount_registry(&env.mock_server, &[&pack_a, &pack_b]).await;

    // Install both packs
    env.weave_cmd()
        .args(["install", "pack-a"])
        .assert()
        .success();

    env.weave_cmd()
        .args(["install", "pack-b"])
        .assert()
        .success();

    // Remove pack-a only
    env.weave_cmd()
        .args(["remove", "pack-a"])
        .assert()
        .success();

    // Verify pack-b is still in the profile
    let profile_content = std::fs::read_to_string(env.profile_toml("default")).unwrap();
    assert!(
        !profile_content.contains("pack-a"),
        "profile should not contain pack-a after removal"
    );
    assert!(
        profile_content.contains("pack-b"),
        "profile should still contain pack-b after removing pack-a"
    );
}

/// Regression test: `weave remove` must clean up project-scope state (`.mcp.json`)
/// even when invoked from a different directory than where `weave install` ran.
///
/// This covers the bug where a user installs a pack from a project directory
/// (which has `.claude/`, triggering project-scope install into `.mcp.json`),
/// then removes the pack from a different directory — leaving the server stranded
/// in `.mcp.json` and still visible in the CLI's MCP server list.
#[cfg(not(target_os = "windows"))]
#[tokio::test]
async fn remove_cleans_project_scope_from_different_directory() {
    let env = TestEnv::new().await;
    let pack =
        FixturePack::new("test-pack", "1.0.0").with_server("echo-server", "echo", &["hello"]);

    mount_registry(&env.mock_server, &[&pack]).await;

    // Install with --project from the project dir — this writes to both ~/.claude.json
    // (user scope) and <project>/.mcp.json (project scope).
    env.weave_cmd()
        .args(["install", "test-pack", "--project"])
        .assert()
        .success();

    // Verify project-scope .mcp.json was written.
    let mcp_json_path = env.project_dir.path().join(".mcp.json");
    assert!(
        mcp_json_path.exists(),
        ".mcp.json should exist after project-scope install"
    );
    let mcp: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(&mcp_json_path).unwrap()).unwrap();
    assert!(
        mcp["mcpServers"]["echo-server"].is_object(),
        "echo-server should be present in .mcp.json after install"
    );

    // Remove from a completely different directory (simulates the real-world failure mode).
    let other_dir = TempDir::new().expect("failed to create other temp dir");
    let mut cmd = assert_cmd::Command::new(env!("CARGO_BIN_EXE_weave"));
    cmd.env("HOME", env.home_dir.path())
        .env("WEAVE_TEST_STORE_DIR", env.store_dir.path())
        .env("WEAVE_REGISTRY_URL", env.mock_server.uri())
        .current_dir(other_dir.path()) // <-- different directory
        .args(["remove", "test-pack"])
        .assert()
        .success();

    // The project-scope .mcp.json should be deleted entirely when the last
    // server is removed (no empty stub left behind).
    assert!(
        !mcp_json_path.exists(),
        ".mcp.json should be deleted when the last project-scope server is removed"
    );
}
