use super::helpers::*;
use assert_cmd::prelude::*;
use predicates::prelude::*;

#[tokio::test]
async fn diagnose_healthy_config() {
    let env = TestEnv::new().await;
    let pack =
        FixturePack::new("test-pack", "1.0.0").with_server("echo-server", "echo", &["hello"]);

    mount_registry(&env.mock_server, &[&pack]).await;

    // Install the pack
    env.weave_cmd()
        .args(["install", "test-pack"])
        .assert()
        .success();

    // Run diagnose — should report ok and no issues
    env.weave_cmd()
        .args(["diagnose"])
        .assert()
        .success()
        .stdout(predicate::str::contains("ok").from_utf8())
        .stdout(predicate::str::contains("No issues found").from_utf8());
}

#[tokio::test]
async fn diagnose_detects_drift() {
    let env = TestEnv::new().await;
    let pack =
        FixturePack::new("test-pack", "1.0.0").with_server("echo-server", "echo", &["hello"]);

    mount_registry(&env.mock_server, &[&pack]).await;

    // Install the pack
    env.weave_cmd()
        .args(["install", "test-pack"])
        .assert()
        .success();

    // Simulate drift: remove the server entry from claude.json
    let claude_json = env.claude_json();
    let content = std::fs::read_to_string(&claude_json).unwrap();
    let mut config: serde_json::Value = serde_json::from_str(&content).unwrap();
    if let Some(servers) = config.get_mut("mcpServers").and_then(|v| v.as_object_mut()) {
        servers.remove("echo-server");
    }
    std::fs::write(&claude_json, serde_json::to_string_pretty(&config).unwrap()).unwrap();

    // Run diagnose — should detect drift
    env.weave_cmd()
        .args(["diagnose"])
        .assert()
        .success()
        .stdout(predicate::str::contains("drifted").from_utf8());
}

#[tokio::test]
async fn diagnose_json_output() {
    let env = TestEnv::new().await;
    let pack =
        FixturePack::new("test-pack", "1.0.0").with_server("echo-server", "echo", &["hello"]);

    mount_registry(&env.mock_server, &[&pack]).await;

    // Install the pack
    env.weave_cmd()
        .args(["install", "test-pack"])
        .assert()
        .success();

    // Run diagnose --json
    let output = env
        .weave_cmd()
        .args(["diagnose", "--json"])
        .output()
        .expect("failed to run weave diagnose --json");

    assert!(output.status.success(), "diagnose --json should succeed");

    let stdout = String::from_utf8(output.stdout).unwrap();
    let json: serde_json::Value =
        serde_json::from_str(&stdout).expect("diagnose --json output should be valid JSON");

    // Verify expected top-level fields
    assert!(
        json.get("profile").is_some(),
        "JSON should have 'profile' field"
    );
    assert!(
        json.get("pack_count").is_some(),
        "JSON should have 'pack_count' field"
    );
    assert!(
        json.get("packs").is_some(),
        "JSON should have 'packs' field"
    );
    assert!(
        json.get("issue_count").is_some(),
        "JSON should have 'issue_count' field"
    );

    // Verify values
    assert_eq!(json["pack_count"], 1);
    assert_eq!(json["issue_count"], 0);
    assert!(json["packs"].is_array());
    assert_eq!(json["packs"].as_array().unwrap().len(), 1);
    assert_eq!(json["packs"][0]["name"], "test-pack");
}

#[tokio::test]
async fn diagnose_json_detects_drift() {
    let env = TestEnv::new().await;
    let pack =
        FixturePack::new("test-pack", "1.0.0").with_server("echo-server", "echo", &["hello"]);

    mount_registry(&env.mock_server, &[&pack]).await;

    // Install the pack
    env.weave_cmd()
        .args(["install", "test-pack"])
        .assert()
        .success();

    // Simulate drift: remove the server entry from claude.json
    let claude_json = env.claude_json();
    let content = std::fs::read_to_string(&claude_json).unwrap();
    let mut config: serde_json::Value = serde_json::from_str(&content).unwrap();
    if let Some(servers) = config.get_mut("mcpServers").and_then(|v| v.as_object_mut()) {
        servers.remove("echo-server");
    }
    std::fs::write(&claude_json, serde_json::to_string_pretty(&config).unwrap()).unwrap();

    // Run diagnose --json
    let output = env
        .weave_cmd()
        .args(["diagnose", "--json"])
        .output()
        .expect("failed to run weave diagnose --json");

    assert!(output.status.success(), "diagnose --json should succeed");

    let stdout = String::from_utf8(output.stdout).unwrap();
    let json: serde_json::Value =
        serde_json::from_str(&stdout).expect("diagnose --json output should be valid JSON");

    // Should report at least one issue
    assert!(
        json["issue_count"].as_u64().unwrap() > 0,
        "issue_count should be > 0 when drift is detected"
    );

    // Find the Claude Code adapter status for the pack
    let pack_report = &json["packs"][0];
    let adapters = pack_report["adapters"].as_array().unwrap();
    let claude_status = adapters
        .iter()
        .find(|a| a["adapter"] == "Claude Code")
        .expect("should have Claude Code adapter status");

    assert_eq!(
        claude_status["status"], "drifted",
        "Claude Code adapter status should be 'drifted'"
    );

    // Verify issues array exists and has entries with severity and message
    let issues = claude_status["issues"].as_array().unwrap();
    assert!(!issues.is_empty(), "issues array should not be empty");
    assert!(
        issues[0].get("severity").is_some(),
        "issue should have 'severity' field"
    );
    assert!(
        issues[0].get("message").is_some(),
        "issue should have 'message' field"
    );
}

#[tokio::test]
async fn diagnose_empty_profile() {
    let env = TestEnv::new().await;

    // Run diagnose with no packs installed
    env.weave_cmd()
        .args(["diagnose"])
        .assert()
        .success()
        .stdout(predicate::str::contains("no packs installed").from_utf8())
        .stdout(predicate::str::contains("No issues found").from_utf8());
}

#[tokio::test]
async fn diagnose_skips_uninstalled_adapters() {
    let env = TestEnv::new().await;
    let pack =
        FixturePack::new("test-pack", "1.0.0").with_server("echo-server", "echo", &["hello"]);

    mount_registry(&env.mock_server, &[&pack]).await;

    // Install the pack (this populates the profile)
    env.weave_cmd()
        .args(["install", "test-pack"])
        .assert()
        .success();

    // Remove one adapter directory to simulate it not being installed.
    // Gemini CLI lives at ~/.gemini/ — remove it so the adapter reports is_installed() == false.
    let gemini_dir = env.home_dir.path().join(".gemini");
    std::fs::remove_dir_all(&gemini_dir).unwrap();

    // Run diagnose --json to get structured output we can inspect
    let output = env
        .weave_cmd()
        .args(["diagnose", "--json"])
        .output()
        .expect("failed to run weave diagnose --json");

    assert!(output.status.success(), "diagnose --json should succeed");

    let stdout = String::from_utf8(output.stdout).unwrap();
    let json: serde_json::Value =
        serde_json::from_str(&stdout).expect("diagnose --json output should be valid JSON");

    // Find the Gemini CLI adapter status — it should be "skipped"
    let pack_report = &json["packs"][0];
    let adapters = pack_report["adapters"].as_array().unwrap();
    let gemini_status = adapters
        .iter()
        .find(|a| a["adapter"] == "Gemini CLI")
        .expect("should have Gemini CLI adapter status");

    assert_eq!(
        gemini_status["status"], "skipped",
        "Gemini CLI adapter should be 'skipped' when not installed"
    );
}
