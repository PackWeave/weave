use super::helpers::*;
use assert_cmd::prelude::*;

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
