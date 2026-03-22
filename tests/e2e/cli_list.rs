use predicates::prelude::*;

use super::helpers::*;
use assert_cmd::prelude::*;

#[tokio::test]
async fn list_empty() {
    let env = TestEnv::new().await;

    env.weave_cmd()
        .args(["list"])
        .assert()
        .success()
        .stdout(predicate::str::contains("No packs").from_utf8());
}

#[tokio::test]
async fn list_after_install() {
    let env = TestEnv::new().await;
    let pack = FixturePack::new("test-pack", "1.0.0")
        .with_server("echo-server", "echo", &["hello"])
        .build();

    mount_registry(&env.mock_server, &[&pack]).await;

    // Install the pack
    env.weave_cmd()
        .args(["install", "test-pack"])
        .assert()
        .success();

    // List should show the pack name
    env.weave_cmd()
        .args(["list"])
        .assert()
        .success()
        .stdout(predicate::str::contains("test-pack").from_utf8());
}
