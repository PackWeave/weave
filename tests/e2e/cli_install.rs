use predicates::prelude::*;

use super::helpers::*;
use assert_cmd::prelude::*;

// ── Local pack install ────────────────────────────────────────────────────────

#[cfg(not(target_os = "windows"))]
#[tokio::test]
async fn install_local_pack() {
    let env = TestEnv::new().await;
    // No registry mounts needed — local install bypasses the registry.

    // Create a minimal pack directory under project_dir.
    let pack_dir = env.project_dir.path().join("my-local-pack");
    std::fs::create_dir_all(&pack_dir).unwrap();
    std::fs::write(
        pack_dir.join("pack.toml"),
        "[pack]\nname = \"my-local-pack\"\nversion = \"0.1.0\"\ndescription = \"local test\"\nauthors = [\"tester\"]\n",
    )
    .unwrap();
    std::fs::create_dir_all(pack_dir.join("prompts")).unwrap();
    std::fs::write(pack_dir.join("prompts/system.md"), "Hello from local pack.").unwrap();

    env.weave_cmd()
        .args(["install", "./my-local-pack"])
        .assert()
        .success()
        .stdout(predicate::str::contains("Installed my-local-pack@0.1.0 (local)").from_utf8());

    // Pack files should be in the store.
    let stored_toml = env
        .store_dir
        .path()
        .join("packs/my-local-pack/0.1.0/pack.toml");
    assert!(stored_toml.exists(), "pack.toml should be written to store");

    // Profile should record the pack.
    let profile_content = std::fs::read_to_string(env.profile_toml("default")).unwrap();
    assert!(profile_content.contains("my-local-pack"));

    // Lockfile should record the pack with a local source.
    let lockfile_content = std::fs::read_to_string(env.lockfile_path("default")).unwrap();
    assert!(lockfile_content.contains("my-local-pack"));
    assert!(lockfile_content.contains("local"));
}

#[cfg(not(target_os = "windows"))]
#[tokio::test]
async fn install_local_pack_prompt_applied() {
    let env = TestEnv::new().await;

    let pack_dir = env.project_dir.path().join("prompt-pack");
    std::fs::create_dir_all(pack_dir.join("prompts")).unwrap();
    std::fs::write(
        pack_dir.join("pack.toml"),
        "[pack]\nname = \"prompt-pack\"\nversion = \"0.1.0\"\ndescription = \"test\"\nauthors = [\"tester\"]\n",
    )
    .unwrap();
    std::fs::write(
        pack_dir.join("prompts/system.md"),
        "## Unique local prompt marker",
    )
    .unwrap();

    env.weave_cmd()
        .args(["install", "./prompt-pack"])
        .assert()
        .success();

    // Prompt content should appear in CLAUDE.md.
    let claude_md = env.claude_dir().join("CLAUDE.md");
    let content = std::fs::read_to_string(&claude_md).unwrap_or_default();
    assert!(
        content.contains("Unique local prompt marker"),
        "CLAUDE.md should contain prompt content from local pack"
    );
}

#[cfg(not(target_os = "windows"))]
#[tokio::test]
async fn install_local_pack_already_installed() {
    let env = TestEnv::new().await;

    let pack_dir = env.project_dir.path().join("my-pack");
    std::fs::create_dir_all(&pack_dir).unwrap();
    std::fs::write(
        pack_dir.join("pack.toml"),
        "[pack]\nname = \"my-pack\"\nversion = \"0.1.0\"\ndescription = \"test\"\nauthors = [\"tester\"]\n",
    )
    .unwrap();

    env.weave_cmd()
        .args(["install", "./my-pack"])
        .assert()
        .success();

    // Second install at the same version should report already installed.
    env.weave_cmd()
        .args(["install", "./my-pack"])
        .assert()
        .success()
        .stdout(predicate::str::contains("already installed").from_utf8());
}

#[tokio::test]
async fn install_single_pack() {
    let env = TestEnv::new().await;
    let pack =
        FixturePack::new("test-pack", "1.0.0").with_server("echo-server", "echo", &["hello"]);

    mount_registry(&env.mock_server, &[&pack]).await;

    env.weave_cmd()
        .args(["install", "test-pack"])
        .assert()
        .success();

    // Verify profile was written
    let profile_path = env.profile_toml("default");
    assert!(
        profile_path.exists(),
        "profile file should exist after install"
    );
    let profile_content = std::fs::read_to_string(&profile_path).unwrap();
    assert!(profile_content.contains("test-pack"));
}

#[tokio::test]
async fn install_already_installed() {
    let env = TestEnv::new().await;
    let pack =
        FixturePack::new("test-pack", "1.0.0").with_server("echo-server", "echo", &["hello"]);

    mount_registry(&env.mock_server, &[&pack]).await;

    // First install
    env.weave_cmd()
        .args(["install", "test-pack"])
        .assert()
        .success();

    // Second install should mention "already"
    env.weave_cmd()
        .args(["install", "test-pack"])
        .assert()
        .success()
        .stdout(predicate::str::contains("already").from_utf8());
}

#[tokio::test]
async fn install_nonexistent_pack() {
    let env = TestEnv::new().await;

    // Don't mount any packs — registry is empty
    mount_registry(&env.mock_server, &[]).await;

    env.weave_cmd()
        .args(["install", "nonexistent-pack"])
        .assert()
        .failure();
}

#[tokio::test]
async fn install_with_at_prefix() {
    let env = TestEnv::new().await;
    let pack =
        FixturePack::new("test-pack", "1.0.0").with_server("echo-server", "echo", &["hello"]);

    mount_registry(&env.mock_server, &[&pack]).await;

    // Install using @test-pack syntax
    env.weave_cmd()
        .args(["install", "@test-pack"])
        .assert()
        .success();

    // Verify profile contains the pack
    let profile_content = std::fs::read_to_string(env.profile_toml("default")).unwrap();
    assert!(profile_content.contains("test-pack"));
}

#[tokio::test]
async fn install_idempotent() {
    let env = TestEnv::new().await;
    let pack =
        FixturePack::new("test-pack", "1.0.0").with_server("echo-server", "echo", &["hello"]);

    mount_registry(&env.mock_server, &[&pack]).await;

    // Install twice
    env.weave_cmd()
        .args(["install", "test-pack"])
        .assert()
        .success();

    env.weave_cmd()
        .args(["install", "test-pack"])
        .assert()
        .success();

    // Verify profile has only one entry for the pack
    let profile_content = std::fs::read_to_string(env.profile_toml("default")).unwrap();
    let count = profile_content.matches("test-pack").count();
    assert_eq!(
        count, 1,
        "profile should contain exactly one entry for test-pack, found {count}"
    );
}

#[tokio::test]
async fn install_writes_lockfile() {
    let env = TestEnv::new().await;
    let pack =
        FixturePack::new("test-pack", "1.0.0").with_server("echo-server", "echo", &["hello"]);

    mount_registry(&env.mock_server, &[&pack]).await;

    env.weave_cmd()
        .args(["install", "test-pack"])
        .assert()
        .success();

    // Verify lockfile exists and contains the pack name
    let lockfile_path = env.lockfile_path("default");
    assert!(
        lockfile_path.exists(),
        "lockfile should exist after install"
    );
    let lockfile_content = std::fs::read_to_string(&lockfile_path).unwrap();
    assert!(
        lockfile_content.contains("test-pack"),
        "lockfile should contain the pack name"
    );
}

#[tokio::test]
async fn install_writes_claude_config() {
    let env = TestEnv::new().await;
    let pack =
        FixturePack::new("test-pack", "1.0.0").with_server("my-mcp-server", "node", &["server.js"]);

    mount_registry(&env.mock_server, &[&pack]).await;

    env.weave_cmd()
        .args(["install", "test-pack"])
        .assert()
        .success();

    // Verify ~/.claude.json was created and contains the server name
    let claude_json = env.claude_json();
    assert!(
        claude_json.exists(),
        "~/.claude.json should exist after installing a pack with a server"
    );
    let claude_content = std::fs::read_to_string(&claude_json).unwrap();
    assert!(
        claude_content.contains("my-mcp-server"),
        "~/.claude.json should contain the server name"
    );
}
