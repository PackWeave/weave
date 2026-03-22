use predicates::prelude::*;

use super::helpers::*;
use assert_cmd::prelude::*;

#[tokio::test]
async fn profile_list_shows_default() {
    let env = TestEnv::new().await;

    env.weave_cmd()
        .args(["profile", "list"])
        .assert()
        .success()
        .stdout(
            predicate::str::contains("default")
                .and(predicate::str::contains("active"))
                .from_utf8(),
        );
}

#[tokio::test]
async fn profile_create_and_list() {
    let env = TestEnv::new().await;

    env.weave_cmd()
        .args(["profile", "create", "testing"])
        .assert()
        .success()
        .stdout(predicate::str::contains("Created profile 'testing'").from_utf8());

    env.weave_cmd()
        .args(["profile", "list"])
        .assert()
        .success()
        .stdout(predicate::str::contains("testing").from_utf8());
}

#[tokio::test]
async fn profile_create_duplicate_fails() {
    let env = TestEnv::new().await;

    env.weave_cmd()
        .args(["profile", "create", "dup"])
        .assert()
        .success();

    env.weave_cmd()
        .args(["profile", "create", "dup"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("already exists").from_utf8());
}

#[tokio::test]
async fn profile_delete() {
    let env = TestEnv::new().await;

    env.weave_cmd()
        .args(["profile", "create", "temp"])
        .assert()
        .success();

    env.weave_cmd()
        .args(["profile", "delete", "temp"])
        .assert()
        .success()
        .stdout(predicate::str::contains("Deleted profile 'temp'").from_utf8());

    // Verify it no longer appears in list (beyond default)
    env.weave_cmd()
        .args(["profile", "list"])
        .assert()
        .success()
        .stdout(predicate::str::contains("temp").not().from_utf8());
}

#[tokio::test]
async fn profile_delete_active_fails() {
    let env = TestEnv::new().await;

    // Create and switch to a profile so it becomes active
    env.weave_cmd()
        .args(["profile", "create", "active-one"])
        .assert()
        .success();

    env.weave_cmd()
        .args(["use", "active-one"])
        .assert()
        .success();

    // Attempting to delete the active profile should fail
    env.weave_cmd()
        .args(["profile", "delete", "active-one"])
        .assert()
        .failure();
}

#[tokio::test]
async fn profile_delete_default_fails() {
    let env = TestEnv::new().await;

    env.weave_cmd()
        .args(["profile", "delete", "default"])
        .assert()
        .failure();
}

#[tokio::test]
async fn profile_add_pack() {
    let env = TestEnv::new().await;
    let pack = FixturePack::new("my-pack", "1.0.0").with_server("my-server", "echo", &["hello"]);

    mount_registry(&env.mock_server, &[&pack]).await;

    env.weave_cmd()
        .args(["profile", "create", "dev"])
        .assert()
        .success();

    env.weave_cmd()
        .args(["profile", "add", "my-pack", "--profile", "dev"])
        .assert()
        .success()
        .stdout(predicate::str::contains("Added my-pack@1.0.0").from_utf8());

    // Verify the profile file contains the pack
    let profile_content = std::fs::read_to_string(env.profile_toml("dev")).unwrap();
    assert!(
        profile_content.contains("my-pack"),
        "profile should contain my-pack after add"
    );
}
