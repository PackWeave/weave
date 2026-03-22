use super::helpers::*;
use assert_cmd::prelude::*;
use predicates::prelude::*;

#[cfg(not(target_os = "windows"))]
#[tokio::test]
async fn init_scaffolds_pack_directory() {
    let env = TestEnv::new().await;

    env.weave_cmd()
        .args(["init", "my-test-pack"])
        .assert()
        .success()
        .stdout(predicate::str::contains("Initialized").from_utf8());

    let pack_toml = env
        .project_dir
        .path()
        .join("my-test-pack")
        .join("pack.toml");
    assert!(pack_toml.exists(), "pack.toml should be created");

    let content = std::fs::read_to_string(&pack_toml).unwrap();
    assert!(
        content.contains("name = \"my-test-pack\""),
        "pack.toml should contain the pack name"
    );
}

#[cfg(not(target_os = "windows"))]
#[tokio::test]
async fn init_default_name() {
    let env = TestEnv::new().await;

    // Create a subdirectory with a valid pack name and run init from there.
    let subdir = env.project_dir.path().join("valid-pack-name");
    std::fs::create_dir_all(&subdir).unwrap();

    let mut cmd = assert_cmd::Command::new(env!("CARGO_BIN_EXE_weave"));
    cmd.env("HOME", env.home_dir.path())
        .env("WEAVE_TEST_STORE_DIR", env.store_dir.path())
        .env("WEAVE_REGISTRY_URL", env.mock_server.uri())
        .current_dir(&subdir)
        .args(["init"])
        .assert()
        .success()
        .stdout(predicate::str::contains("Initialized").from_utf8());

    let pack_toml = subdir.join("pack.toml");
    assert!(
        pack_toml.exists(),
        "pack.toml should be created in current dir"
    );

    let content = std::fs::read_to_string(&pack_toml).unwrap();
    assert!(
        content.contains("name = \"valid-pack-name\""),
        "pack.toml should use directory name as pack name"
    );
}
