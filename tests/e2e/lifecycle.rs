use super::helpers::*;
use assert_cmd::prelude::*;
use predicates::prelude::*;

#[tokio::test]
async fn golden_path() {
    let env = TestEnv::new().await;
    let pack =
        FixturePack::new("test-pack", "1.0.0").with_server("echo-server", "echo", &["hello"]);

    mount_registry(&env.mock_server, &[&pack]).await;

    // 1. List should be empty initially
    env.weave_cmd()
        .args(["list"])
        .assert()
        .success()
        .stdout(predicate::str::contains("test-pack").not());

    // 2. Install test-pack
    env.weave_cmd()
        .args(["install", "test-pack"])
        .assert()
        .success();

    // 3. List should show test-pack
    env.weave_cmd()
        .args(["list"])
        .assert()
        .success()
        .stdout(predicate::str::contains("test-pack"));

    // 4. Remove test-pack
    env.weave_cmd()
        .args(["remove", "test-pack"])
        .assert()
        .success();

    // 5. List should be empty again
    env.weave_cmd()
        .args(["list"])
        .assert()
        .success()
        .stdout(predicate::str::contains("test-pack").not());
}
