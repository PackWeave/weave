use super::helpers::*;
use assert_cmd::prelude::*;
use predicates::prelude::*;

#[tokio::test]
async fn search_finds_matching_pack() {
    let env = TestEnv::new().await;
    let pack =
        FixturePack::new("awesome-pack", "1.0.0").with_server("echo-server", "echo", &["hello"]);

    mount_registry(&env.mock_server, &[&pack]).await;

    env.weave_cmd()
        .args(["search", "awesome"])
        .assert()
        .success()
        .stdout(predicate::str::contains("awesome-pack"));
}

#[tokio::test]
async fn search_no_results() {
    let env = TestEnv::new().await;
    let pack =
        FixturePack::new("some-pack", "1.0.0").with_server("echo-server", "echo", &["hello"]);

    mount_registry(&env.mock_server, &[&pack]).await;

    env.weave_cmd()
        .args(["search", "zzz-nonexistent"])
        .assert()
        .success()
        .stdout(predicate::str::contains("No packs found"));
}

#[tokio::test]
async fn search_invalid_target() {
    let env = TestEnv::new().await;
    let pack =
        FixturePack::new("some-pack", "1.0.0").with_server("echo-server", "echo", &["hello"]);

    mount_registry(&env.mock_server, &[&pack]).await;

    env.weave_cmd()
        .args(["search", "foo", "--target", "invalid_cli"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("unknown target"));
}
