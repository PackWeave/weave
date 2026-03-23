use predicates::prelude::*;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

use super::helpers::*;
use assert_cmd::prelude::*;

#[tokio::test]
async fn auth_status_not_authenticated() {
    let env = TestEnv::new().await;
    env.weave_cmd()
        .args(["auth", "status"])
        .assert()
        .success()
        .stdout(predicate::str::contains("Not authenticated").from_utf8());
}

#[tokio::test]
async fn auth_login_stores_token() {
    let env = TestEnv::new().await;
    env.weave_cmd()
        .args(["auth", "login", "--token", "ghp_test123456"])
        .assert()
        .success()
        .stdout(predicate::str::contains("Token stored").from_utf8());
}

#[tokio::test]
async fn auth_login_then_status_shows_authenticated() {
    let env = TestEnv::new().await;

    env.weave_cmd()
        .args(["auth", "login", "--token", "ghp_test123456"])
        .assert()
        .success();

    env.weave_cmd()
        .args(["auth", "status"])
        .assert()
        .success()
        .stdout(predicate::str::contains("Authenticated").from_utf8())
        .stdout(predicate::str::contains("ghp_****").from_utf8());
}

#[tokio::test]
async fn auth_logout_removes_credentials() {
    let env = TestEnv::new().await;

    // Login first.
    env.weave_cmd()
        .args(["auth", "login", "--token", "ghp_test123456"])
        .assert()
        .success();

    // Logout.
    env.weave_cmd()
        .args(["auth", "logout"])
        .assert()
        .success()
        .stdout(predicate::str::contains("Logged out").from_utf8());

    // Status should show not authenticated.
    env.weave_cmd()
        .args(["auth", "status"])
        .assert()
        .success()
        .stdout(predicate::str::contains("Not authenticated").from_utf8());
}

#[tokio::test]
async fn auth_env_var_overrides_file() {
    let env = TestEnv::new().await;

    // Login with a file-based token.
    env.weave_cmd()
        .args(["auth", "login", "--token", "file-token-1234"])
        .assert()
        .success();

    // Set env var override — should take precedence.
    env.weave_cmd()
        .env("WEAVE_TOKEN", "env-token-5678")
        .args(["auth", "status"])
        .assert()
        .success()
        .stdout(predicate::str::contains("WEAVE_TOKEN").from_utf8())
        .stdout(predicate::str::contains("env-****").from_utf8());
}

#[tokio::test]
async fn auth_login_empty_token_fails() {
    let env = TestEnv::new().await;
    env.weave_cmd()
        .args(["auth", "login", "--token", ""])
        .assert()
        .failure();
}

#[tokio::test]
async fn auth_login_via_stdin() {
    let env = TestEnv::new().await;
    env.weave_cmd()
        .args(["auth", "login"])
        .write_stdin("test-stdin-token\n")
        .assert()
        .success()
        .stdout(predicate::str::contains("Token stored").from_utf8());
}

#[tokio::test]
async fn auth_login_overwrite() {
    let env = TestEnv::new().await;

    // Login with token A.
    env.weave_cmd()
        .args(["auth", "login", "--token", "tokenA-first1234"])
        .assert()
        .success();

    // Login with token B (overwrites A).
    env.weave_cmd()
        .args(["auth", "login", "--token", "tokenB-second5678"])
        .assert()
        .success();

    // Status should show token B's masked prefix, not A's.
    env.weave_cmd()
        .args(["auth", "status"])
        .assert()
        .success()
        .stdout(predicate::str::contains("toke****").from_utf8());
}

#[tokio::test]
async fn auth_logout_when_not_authenticated() {
    let env = TestEnv::new().await;

    // Logout without any prior login should succeed (not error).
    env.weave_cmd()
        .args(["auth", "logout"])
        .assert()
        .success()
        .stdout(predicate::str::contains("Logged out").from_utf8());
}

#[tokio::test]
async fn auth_status_empty_weave_token_env() {
    let env = TestEnv::new().await;

    // Store a file-based token first.
    env.weave_cmd()
        .args(["auth", "login", "--token", "file-token-abcd"])
        .assert()
        .success();

    // Set WEAVE_TOKEN="" — empty env var should be ignored, file token used instead.
    env.weave_cmd()
        .env("WEAVE_TOKEN", "")
        .args(["auth", "status"])
        .assert()
        .success()
        .stdout(predicate::str::contains("Authenticated").from_utf8())
        // Source should be the file path, NOT the WEAVE_TOKEN env var.
        .stdout(predicate::str::contains("WEAVE_TOKEN").from_utf8().not())
        .stdout(predicate::str::contains("file****").from_utf8());
}

/// Verify that auth login + search works end-to-end: the token is stored,
/// resolved, and the search command succeeds.
///
/// Note: the token is NOT sent to the mock server because localhost is not
/// in the trusted host allowlist (api.github.com, raw.githubusercontent.com).
/// The allowlist behavior is covered by unit tests in registry.rs. This E2E
/// test verifies the CLI flow: login → token stored → search uses registry.
#[tokio::test]
async fn auth_login_then_search_works() {
    let env = TestEnv::new().await;

    // Login with a token.
    env.weave_cmd()
        .args(["auth", "login", "--token", "test-registry-token"])
        .assert()
        .success();

    // Mount a mock registry (no auth requirement — localhost isn't trusted).
    let pack = FixturePack::new("test-pack", "1.0.0");
    mount_registry(&env.mock_server, &[&pack]).await;

    // Search should work — token is resolved but not sent to localhost.
    env.weave_cmd()
        .args(["search", "test"])
        .assert()
        .success()
        .stdout(predicate::str::contains("test-pack").from_utf8());
}

/// Verify that tap servers never receive an Authorization header, even when
/// the user is authenticated. Both servers run on localhost (not in trusted
/// host allowlist), so neither receives the token — but this test verifies
/// the tap receives no auth header even when one is configured.
#[tokio::test]
async fn auth_token_not_sent_to_taps() {
    let env = TestEnv::new().await;

    // Login with a token.
    env.weave_cmd()
        .args(["auth", "login", "--token", "secret-token-value"])
        .assert()
        .success();

    // Start a second MockServer to act as the tap registry.
    let tap_server = MockServer::start().await;

    // Mount the official registry index (no auth requirement).
    let index_json = serde_json::json!({});
    Mock::given(method("GET"))
        .and(path("/index.json"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_string(serde_json::to_string(&index_json).unwrap())
                .insert_header("content-type", "application/json"),
        )
        .mount(&env.mock_server)
        .await;

    // Mount the tap registry index.
    let tap_index_json = serde_json::json!({
        "tap-pack": {
            "name": "tap-pack",
            "description": "A tap pack",
            "latest_version": "1.0.0"
        }
    });
    Mock::given(method("GET"))
        .and(path("/index.json"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_string(serde_json::to_string(&tap_index_json).unwrap())
                .insert_header("content-type", "application/json"),
        )
        .mount(&tap_server)
        .await;

    // Write config.toml pointing to both registries.
    let config_content = format!(
        "registry_url = \"{}\"\nactive_profile = \"default\"\n\n[[taps]]\nname = \"test/tap\"\nurl = \"{}\"\n",
        env.mock_server.uri(),
        tap_server.uri()
    );
    std::fs::write(env.config_toml(), config_content).unwrap();

    // Run search to trigger requests to both servers.
    env.weave_cmd().args(["search", "test"]).assert().success();

    // Verify the tap server did NOT receive an Authorization header.
    let tap_requests = tap_server.received_requests().await.unwrap();
    for req in &tap_requests {
        assert!(
            !req.headers
                .iter()
                .any(|(name, _)| name.as_str().eq_ignore_ascii_case("authorization")),
            "tap server should NOT receive Authorization header, but got one in request to {}",
            req.url
        );
    }
}
