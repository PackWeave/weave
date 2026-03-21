/// Integration tests for the `weave init` command.
///
/// These tests use `TempDir` for all file system operations and never write
/// outside of temporary directories.
use std::path::Path;

use packweave::core::pack::Pack;
use tempfile::TempDir;

// ── Helpers ─────────────────────────────────────────────────────────────────

/// Run `weave init <name>` from within `cwd`.
fn run_init_in(cwd: &Path, args: &[&str]) -> std::process::Output {
    let binary = env!("CARGO_BIN_EXE_weave");
    let mut cmd = std::process::Command::new(binary);
    cmd.current_dir(cwd);
    cmd.arg("init");
    for arg in args {
        cmd.arg(arg);
    }
    cmd.output().expect("failed to execute weave")
}

// ── Tests ───────────────────────────────────────────────────────────────────

#[test]
fn init_creates_all_expected_files() {
    let tmp = TempDir::new().unwrap();
    let output = run_init_in(tmp.path(), &["my-pack"]);

    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let root = tmp.path().join("my-pack");
    assert!(root.join("pack.toml").is_file());
    assert!(root.join("README.md").is_file());
    assert!(root.join("prompts/system.md").is_file());
    assert!(root.join("settings").is_dir());
    assert!(root.join("commands").is_dir());
    assert!(root.join("skills").is_dir());

    // Verify pack.toml content
    let content = std::fs::read_to_string(root.join("pack.toml")).unwrap();
    assert!(content.contains("name = \"my-pack\""));
    assert!(content.contains("version = \"0.1.0\""));

    // Verify README.md content
    let readme = std::fs::read_to_string(root.join("README.md")).unwrap();
    assert!(readme.contains("# my-pack"));
}

#[test]
fn init_rejects_uppercase_name() {
    let tmp = TempDir::new().unwrap();
    let output = run_init_in(tmp.path(), &["MyPack"]);

    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("invalid"),
        "expected 'invalid' in: {stderr}"
    );

    // No directory should have been created
    assert!(!tmp.path().join("MyPack").exists());
}

#[test]
fn init_rejects_underscore_name() {
    let tmp = TempDir::new().unwrap();
    let output = run_init_in(tmp.path(), &["my_pack"]);

    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("invalid"),
        "expected 'invalid' in: {stderr}"
    );

    // No directory should have been created
    assert!(!tmp.path().join("my_pack").exists());
}

#[test]
fn init_fails_when_directory_already_exists() {
    let tmp = TempDir::new().unwrap();
    std::fs::create_dir(tmp.path().join("existing")).unwrap();

    let output = run_init_in(tmp.path(), &["existing"]);

    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("already exists"),
        "expected 'already exists' in: {stderr}"
    );
}

#[test]
fn init_no_args_uses_current_directory_name() {
    let tmp = TempDir::new().unwrap();
    // Create a subdirectory with a valid pack name to use as cwd
    let pack_dir = tmp.path().join("my-cool-pack");
    std::fs::create_dir(&pack_dir).unwrap();

    let output = run_init_in(&pack_dir, &[]);

    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    assert!(pack_dir.join("pack.toml").is_file());
    let content = std::fs::read_to_string(pack_dir.join("pack.toml")).unwrap();
    assert!(content.contains("name = \"my-cool-pack\""));
}

#[test]
fn init_no_args_fails_when_pack_toml_already_exists() {
    let tmp = TempDir::new().unwrap();
    let pack_dir = tmp.path().join("my-pack");
    std::fs::create_dir(&pack_dir).unwrap();
    std::fs::write(pack_dir.join("pack.toml"), "existing content").unwrap();

    let output = run_init_in(&pack_dir, &[]);

    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("pack.toml already exists"),
        "expected 'pack.toml already exists' in: {stderr}"
    );
}

#[test]
fn generated_pack_toml_parses_via_pack_from_toml() {
    let tmp = TempDir::new().unwrap();
    let output = run_init_in(tmp.path(), &["test-pack"]);
    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let pack_toml_path = tmp.path().join("test-pack/pack.toml");
    let content = std::fs::read_to_string(&pack_toml_path).unwrap();
    let pack = Pack::from_toml(&content, &pack_toml_path)
        .expect("generated pack.toml should parse without error");

    assert_eq!(pack.name, "test-pack");
    assert_eq!(pack.version, semver::Version::new(0, 1, 0));
    assert!(pack.targets.claude_code);
}

#[test]
fn init_no_args_fails_when_readme_already_exists() {
    let tmp = TempDir::new().unwrap();
    let pack_dir = tmp.path().join("my-pack");
    std::fs::create_dir(&pack_dir).unwrap();

    // Pre-create a README.md with known content
    let readme_path = pack_dir.join("README.md");
    std::fs::write(&readme_path, "do not overwrite me").unwrap();

    let output = run_init_in(&pack_dir, &[]);

    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("already exist"),
        "expected 'already exist' in: {stderr}"
    );

    // Verify the original file was not modified
    let content = std::fs::read_to_string(&readme_path).unwrap();
    assert_eq!(content, "do not overwrite me");
}

#[test]
fn init_fails_when_file_exists_with_target_name() {
    let tmp = TempDir::new().unwrap();
    // Create a file (not a directory) named "my-pack"
    std::fs::write(tmp.path().join("my-pack"), "I am a file").unwrap();

    let output = run_init_in(tmp.path(), &["my-pack"]);

    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("file named"),
        "expected 'file named' in: {stderr}"
    );
}
