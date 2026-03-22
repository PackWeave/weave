use super::helpers::*;
use assert_cmd::prelude::*;

#[tokio::test]
async fn install_single_pack() {
    let env = TestEnv::new().await;
    let pack = FixturePack::new("test-pack", "1.0.0")
        .with_server("echo-server", "echo", &["hello"])
        .build();

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
