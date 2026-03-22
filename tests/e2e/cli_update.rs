use super::helpers::*;
use assert_cmd::prelude::*;
use predicates::prelude::*;

#[tokio::test]
async fn update_to_newer_version() {
    let env = TestEnv::new().await;

    let pack_v1 = FixturePack::new("test-pack", "1.0.0")
        .with_server("echo-server", "echo", &["hello"])
        .build();

    let pack_v2 = FixturePack::new("test-pack", "1.1.0")
        .with_server("echo-server", "echo", &["hello"])
        .build();

    mount_registry_multi_version(&env.mock_server, &[&pack_v1, &pack_v2]).await;

    // Install v1.0.0 specifically
    env.weave_cmd()
        .args(["install", "test-pack", "--version", "=1.0.0"])
        .assert()
        .success();

    // Verify v1.0.0 is installed
    let profile_content = std::fs::read_to_string(env.profile_toml("default")).unwrap();
    assert!(
        profile_content.contains("1.0.0"),
        "profile should contain v1.0.0 after install"
    );

    // Update to latest
    env.weave_cmd()
        .args(["update", "test-pack"])
        .assert()
        .success();

    // Verify v1.1.0 is now in the profile
    let profile_content = std::fs::read_to_string(env.profile_toml("default")).unwrap();
    assert!(
        profile_content.contains("1.1.0"),
        "profile should contain v1.1.0 after update"
    );
}

#[tokio::test]
async fn update_already_latest() {
    let env = TestEnv::new().await;

    let pack = FixturePack::new("test-pack", "1.0.0")
        .with_server("echo-server", "echo", &["hello"])
        .build();

    mount_registry(&env.mock_server, &[&pack]).await;

    // Install latest (and only) version
    env.weave_cmd()
        .args(["install", "test-pack"])
        .assert()
        .success();

    // Update should indicate already up to date
    env.weave_cmd()
        .args(["update", "test-pack"])
        .assert()
        .success()
        .stdout(predicate::str::contains("already up to date"));
}
