use predicates::prelude::*;

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
