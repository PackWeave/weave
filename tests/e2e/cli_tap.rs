use predicates::prelude::*;
use wiremock::MockServer;

use super::helpers::*;
use assert_cmd::prelude::*;

#[tokio::test]
async fn tap_add_registers_tap() {
    let env = TestEnv::new().await;
    env.weave_cmd()
        .args(["tap", "add", "user/repo"])
        .assert()
        .success()
        .stdout(predicate::str::contains("added").from_utf8());
}

#[tokio::test]
async fn tap_add_duplicate_fails() {
    let env = TestEnv::new().await;
    env.weave_cmd()
        .args(["tap", "add", "user/repo"])
        .assert()
        .success();

    env.weave_cmd()
        .args(["tap", "add", "user/repo"])
        .assert()
        .failure();
}

#[tokio::test]
async fn tap_list_empty() {
    let env = TestEnv::new().await;
    env.weave_cmd()
        .args(["tap", "list"])
        .assert()
        .success()
        .stdout(predicate::str::contains("No community taps").from_utf8());
}

#[tokio::test]
async fn tap_list_shows_registered() {
    let env = TestEnv::new().await;
    env.weave_cmd()
        .args(["tap", "add", "acme/packs"])
        .assert()
        .success();

    env.weave_cmd()
        .args(["tap", "list"])
        .assert()
        .success()
        .stdout(predicate::str::contains("acme/packs").from_utf8());
}

#[tokio::test]
async fn tap_remove_deregisters() {
    let env = TestEnv::new().await;
    env.weave_cmd()
        .args(["tap", "add", "acme/packs"])
        .assert()
        .success();

    env.weave_cmd()
        .args(["tap", "remove", "acme/packs"])
        .assert()
        .success()
        .stdout(predicate::str::contains("removed").from_utf8());

    // Verify no longer listed
    env.weave_cmd()
        .args(["tap", "list"])
        .assert()
        .success()
        .stdout(predicate::str::contains("No community taps").from_utf8());
}

#[tokio::test]
async fn tap_remove_nonexistent_fails() {
    let env = TestEnv::new().await;
    env.weave_cmd()
        .args(["tap", "remove", "no/such-tap"])
        .assert()
        .failure();
}

#[tokio::test]
async fn tap_install_from_tap() {
    let env = TestEnv::new().await;

    // Start a second MockServer to act as the tap registry.
    let tap_server = wiremock::MockServer::start().await;

    // Mount a pack on the tap server only.
    let tap_pack =
        FixturePack::new("tap-only-pack", "1.0.0").with_server("tap-server", "echo", &["hello"]);
    mount_registry(&tap_server, &[&tap_pack]).await;

    // Mount an empty index on the main server (no tap-only-pack).
    mount_registry(&env.mock_server, &[]).await;

    // Write config.toml pointing to both registries.
    let config_content = format!(
        "registry_url = \"{}\"\nactive_profile = \"default\"\n\n[[taps]]\nname = \"test/tap\"\nurl = \"{}\"\n",
        env.mock_server.uri(),
        tap_server.uri()
    );
    std::fs::write(env.config_toml(), config_content).unwrap();

    // Install should find the pack via the tap.
    env.weave_cmd()
        .args(["install", "tap-only-pack"])
        .assert()
        .success();

    // Verify the pack is in the profile TOML.
    let profile_content = std::fs::read_to_string(env.profile_toml("default")).unwrap();
    assert!(
        profile_content.contains("tap-only-pack"),
        "profile should contain tap-only-pack after install from tap"
    );
}
