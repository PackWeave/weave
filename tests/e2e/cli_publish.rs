use predicates::prelude::*;

use super::helpers::*;
use assert_cmd::prelude::*;

#[tokio::test]
async fn publish_requires_auth() {
    let env = TestEnv::new().await;

    // Create a valid pack in the project directory.
    std::fs::write(
        env.project_dir.path().join("pack.toml"),
        "[pack]\nname = \"test-pack\"\nversion = \"0.1.0\"\ndescription = \"test\"\nauthors = [\"tester\"]\n",
    )
    .unwrap();

    // Publish without auth should fail.
    env.weave_cmd()
        .args(["publish"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("not authenticated").from_utf8());
}

#[tokio::test]
async fn publish_validates_pack_toml() {
    let env = TestEnv::new().await;

    // No pack.toml in the project directory.
    env.weave_cmd()
        .args(["publish"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("pack.toml").from_utf8());
}

#[tokio::test]
async fn publish_detects_duplicate_version() {
    let env = TestEnv::new().await;

    // Create a valid pack.
    std::fs::write(
        env.project_dir.path().join("pack.toml"),
        "[pack]\nname = \"my-pack\"\nversion = \"1.0.0\"\ndescription = \"test\"\nauthors = [\"tester\"]\n",
    )
    .unwrap();

    // Store an auth token.
    env.weave_cmd()
        .args(["auth", "login", "--token", "ghp_testtoken1234"])
        .assert()
        .success();

    // Mount a mock registry with my-pack@1.0.0 already published.
    let pack = FixturePack::new("my-pack", "1.0.0");
    mount_registry(&env.mock_server, &[&pack]).await;

    // Publish should detect the duplicate.
    env.weave_cmd()
        .args(["publish"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("already published").from_utf8());
}

#[tokio::test]
async fn publish_accepts_explicit_path() {
    let env = TestEnv::new().await;

    // Create a pack in a subdirectory.
    let pack_dir = env.project_dir.path().join("my-pack");
    std::fs::create_dir_all(&pack_dir).unwrap();
    std::fs::write(
        pack_dir.join("pack.toml"),
        "[pack]\nname = \"my-pack\"\nversion = \"0.1.0\"\ndescription = \"test\"\nauthors = [\"tester\"]\n",
    )
    .unwrap();

    // Publish with explicit path (will fail at auth, but that proves path was accepted).
    env.weave_cmd()
        .args(["publish", pack_dir.to_str().unwrap()])
        .assert()
        .failure()
        .stderr(predicate::str::contains("not authenticated").from_utf8());
}
