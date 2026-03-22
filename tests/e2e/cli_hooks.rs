use super::helpers::*;
use assert_cmd::prelude::*;
use predicates::prelude::*;

/// Create a local pack directory with hooks declared in the manifest.
fn create_hooks_pack(base_dir: &std::path::Path) -> std::path::PathBuf {
    let pack_dir = base_dir.join("hooks-pack");
    std::fs::create_dir_all(&pack_dir).unwrap();
    std::fs::write(
        pack_dir.join("pack.toml"),
        r#"[pack]
name = "hooks-test"
version = "0.1.0"
description = "Hooks test pack"
authors = ["test"]

[extensions.claude_code.hooks]
PreToolUse = [{ matcher = "Bash", command = "echo hook-test" }]
"#,
    )
    .unwrap();
    pack_dir
}

#[cfg(not(target_os = "windows"))]
#[tokio::test]
async fn install_hooks_skipped_without_flag() {
    let env = TestEnv::new().await;
    create_hooks_pack(env.project_dir.path());

    env.weave_cmd()
        .args(["install", "./hooks-pack"])
        .assert()
        .success()
        .stderr(
            predicate::str::contains("hooks")
                .and(predicate::str::contains("allow-hooks"))
                .from_utf8(),
        );

    // settings.json should either not exist or not contain hooks
    let content = std::fs::read_to_string(env.claude_settings_json()).unwrap_or_default();
    if !content.is_empty() {
        let json: serde_json::Value =
            serde_json::from_str(&content).unwrap_or(serde_json::json!({}));
        assert!(
            json.get("hooks").is_none(),
            "hooks should not be in settings.json without --allow-hooks"
        );
    }
}

#[cfg(not(target_os = "windows"))]
#[tokio::test]
async fn install_hooks_applied_with_flag() {
    let env = TestEnv::new().await;
    create_hooks_pack(env.project_dir.path());

    env.weave_cmd()
        .args(["install", "./hooks-pack", "--allow-hooks"])
        .assert()
        .success();

    let content = std::fs::read_to_string(env.claude_settings_json())
        .expect("settings.json should exist after --allow-hooks install");
    let json: serde_json::Value = serde_json::from_str(&content).unwrap();
    assert!(
        json.get("hooks").is_some(),
        "hooks key should exist in settings.json"
    );
    assert!(
        json["hooks"]["PreToolUse"].is_array(),
        "PreToolUse should be an array under hooks"
    );
}

#[cfg(not(target_os = "windows"))]
#[tokio::test]
async fn list_shows_hooks_badge() {
    let env = TestEnv::new().await;
    create_hooks_pack(env.project_dir.path());

    env.weave_cmd()
        .args(["install", "./hooks-pack"])
        .assert()
        .success();

    env.weave_cmd()
        .args(["list"])
        .assert()
        .success()
        .stdout(predicate::str::contains("[hooks]").from_utf8());
}

#[cfg(not(target_os = "windows"))]
#[tokio::test]
async fn remove_cleans_up_hooks() {
    let env = TestEnv::new().await;
    create_hooks_pack(env.project_dir.path());

    // Install with hooks
    env.weave_cmd()
        .args(["install", "./hooks-pack", "--allow-hooks"])
        .assert()
        .success();

    // Verify hooks are present
    let content = std::fs::read_to_string(env.claude_settings_json()).unwrap();
    let json: serde_json::Value = serde_json::from_str(&content).unwrap();
    assert!(
        json.get("hooks").is_some(),
        "hooks should exist before remove"
    );

    // Remove the pack
    env.weave_cmd()
        .args(["remove", "hooks-test"])
        .assert()
        .success();

    // Verify hooks are gone from settings.json
    let content = std::fs::read_to_string(env.claude_settings_json()).unwrap_or_default();
    if !content.is_empty() {
        let json: serde_json::Value =
            serde_json::from_str(&content).unwrap_or(serde_json::json!({}));
        // Either no hooks key, or the hooks object has no PreToolUse entries
        if let Some(hooks) = json.get("hooks") {
            let hooks_obj = hooks.as_object();
            assert!(
                hooks_obj.map_or(true, |h| h.is_empty()),
                "hooks should be cleaned up after remove"
            );
        }
    }
}

#[cfg(not(target_os = "windows"))]
#[tokio::test]
async fn sync_applies_hooks_with_flag() {
    let env = TestEnv::new().await;
    create_hooks_pack(env.project_dir.path());

    // Install without --allow-hooks (hooks skipped)
    env.weave_cmd()
        .args(["install", "./hooks-pack"])
        .assert()
        .success();

    // Verify hooks are NOT present
    let content = std::fs::read_to_string(env.claude_settings_json()).unwrap_or_default();
    if !content.is_empty() {
        let json: serde_json::Value =
            serde_json::from_str(&content).unwrap_or(serde_json::json!({}));
        assert!(
            json.get("hooks").is_none(),
            "hooks should not be present before sync --allow-hooks"
        );
    }

    // Sync with --allow-hooks should apply them
    env.weave_cmd()
        .args(["sync", "--allow-hooks"])
        .assert()
        .success();

    // Verify hooks are now present
    let content = std::fs::read_to_string(env.claude_settings_json())
        .expect("settings.json should exist after sync --allow-hooks");
    let json: serde_json::Value = serde_json::from_str(&content).unwrap();
    assert!(
        json.get("hooks").is_some(),
        "hooks should be in settings.json after sync --allow-hooks"
    );
}

#[cfg(not(target_os = "windows"))]
#[tokio::test]
async fn use_applies_hooks_with_flag() {
    let env = TestEnv::new().await;
    create_hooks_pack(env.project_dir.path());

    // Create a second profile and switch to it
    env.weave_cmd()
        .args(["profile", "create", "hooks-profile"])
        .assert()
        .success();

    env.weave_cmd()
        .args(["use", "hooks-profile"])
        .assert()
        .success();

    // Install hooks pack without --allow-hooks on hooks-profile
    env.weave_cmd()
        .args(["install", "./hooks-pack"])
        .assert()
        .success();

    // Switch away to default
    env.weave_cmd().args(["use", "default"]).assert().success();

    // Switch back with --allow-hooks
    env.weave_cmd()
        .args(["use", "hooks-profile", "--allow-hooks"])
        .assert()
        .success();

    // Verify hooks are now applied
    let content = std::fs::read_to_string(env.claude_settings_json())
        .expect("settings.json should exist after use --allow-hooks");
    let json: serde_json::Value = serde_json::from_str(&content).unwrap();
    assert!(
        json.get("hooks").is_some(),
        "hooks should be in settings.json after use --allow-hooks"
    );
}
