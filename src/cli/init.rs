use std::path::{Path, PathBuf};

use anyhow::{bail, Context, Result};

use crate::cli::style;

/// Validate that a pack name matches `[a-z0-9-]+`.
fn validate_pack_name(name: &str) -> Result<()> {
    if name.is_empty() {
        bail!("pack name cannot be empty");
    }

    if !name
        .chars()
        .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
    {
        let suggestion = name
            .to_lowercase()
            .replace('_', "-")
            .chars()
            .filter(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || *c == '-')
            .collect::<String>();

        if suggestion.is_empty() {
            bail!(
                "pack name '{}' is invalid\n  \
                 Pack names must contain only lowercase letters, numbers, and hyphens.",
                name,
            );
        } else {
            bail!(
                "pack name '{}' is invalid\n  \
                 Pack names must contain only lowercase letters, numbers, and hyphens.\n  \
                 Try: weave init {}",
                name,
                suggestion
            );
        }
    }

    Ok(())
}

/// Generate the `pack.toml` content for a new pack.
fn pack_toml_content(name: &str) -> String {
    format!(
        r#"[pack]
name = "{name}"
version = "0.1.0"
description = "TODO: describe what this pack does"
authors = ["TODO: your name <you@example.com>"]
license = "MIT"

[targets]
claude_code = true
gemini_cli = false
codex_cli = false
"#
    )
}

/// Generate the `README.md` content for a new pack.
fn readme_content(name: &str) -> String {
    format!("# {name}\n\nA weave pack.\n\n## Description\n\nTODO: describe what this pack does.\n")
}

/// Files that `scaffold` creates. Used to check for conflicts before writing.
const SCAFFOLD_FILES: &[&str] = &["pack.toml", "README.md", "prompts/system.md"];

/// Check that none of the files scaffold would create already exist in `root_dir`.
///
/// This is important for the in-place (`weave init` with no args) case — the
/// subdirectory case is already guarded by checking that the directory does not
/// exist at all.
fn check_no_conflicts(root_dir: &Path) -> Result<()> {
    let mut conflicts: Vec<String> = Vec::new();
    for rel in SCAFFOLD_FILES {
        let path = root_dir.join(rel);
        if path.exists() {
            conflicts.push(rel.to_string());
        }
    }
    if !conflicts.is_empty() {
        bail!(
            "the following files already exist and would be overwritten:\n  {}\n  \
             Remove them first or use a different directory.",
            conflicts.join("\n  ")
        );
    }
    Ok(())
}

/// Scaffold all pack files into `root_dir`.
fn scaffold(root_dir: &Path, name: &str, check_existing: bool) -> Result<()> {
    if check_existing {
        check_no_conflicts(root_dir)?;
    }

    // Create the root directory if it doesn't already exist (for the <name> case,
    // it was just created; for the no-args case, it already exists).
    std::fs::create_dir_all(root_dir)
        .with_context(|| format!("creating directory '{}'", root_dir.display()))?;

    // pack.toml
    let pack_toml_path = root_dir.join("pack.toml");
    std::fs::write(&pack_toml_path, pack_toml_content(name))
        .with_context(|| format!("writing '{}'", pack_toml_path.display()))?;

    // README.md
    let readme_path = root_dir.join("README.md");
    std::fs::write(&readme_path, readme_content(name))
        .with_context(|| format!("writing '{}'", readme_path.display()))?;

    // prompts/system.md (empty)
    let prompts_dir = root_dir.join("prompts");
    std::fs::create_dir_all(&prompts_dir)
        .with_context(|| format!("creating '{}'", prompts_dir.display()))?;
    std::fs::write(prompts_dir.join("system.md"), "")
        .with_context(|| "writing 'prompts/system.md'")?;

    // settings/ (empty directory)
    let settings_dir = root_dir.join("settings");
    std::fs::create_dir_all(&settings_dir)
        .with_context(|| format!("creating '{}'", settings_dir.display()))?;

    // commands/ (empty directory)
    let commands_dir = root_dir.join("commands");
    std::fs::create_dir_all(&commands_dir)
        .with_context(|| format!("creating '{}'", commands_dir.display()))?;

    // skills/ (empty directory, for Codex CLI)
    let skills_dir = root_dir.join("skills");
    std::fs::create_dir_all(&skills_dir)
        .with_context(|| format!("creating '{}'", skills_dir.display()))?;

    Ok(())
}

/// Run `weave init [name]`.
///
/// If `name` is `Some`, creates a `<name>/` subdirectory in the current working
/// directory. If `None`, initializes the current directory using its name as the
/// pack name.
pub fn run(name: Option<&str>) -> Result<()> {
    let cwd = std::env::current_dir().context("determining current directory")?;

    let (root_dir, pack_name, check_existing): (PathBuf, String, bool) = match name {
        Some(n) => {
            // Validate before any I/O
            validate_pack_name(n)?;

            let dir = cwd.join(n);
            if dir.is_file() {
                bail!(
                    "a file named '{}' already exists\n  \
                     Choose a different name or remove the existing file.",
                    n
                );
            }
            if dir.is_dir() {
                bail!(
                    "directory '{}' already exists\n  \
                     Choose a different name or remove the existing directory.",
                    n
                );
            }
            // For the subdirectory case, the directory is freshly created so
            // there can be no file conflicts — skip the check.
            (dir, n.to_string(), false)
        }
        None => {
            // No-args: initialize current directory
            let pack_toml = cwd.join("pack.toml");
            if pack_toml.exists() {
                bail!(
                    "pack.toml already exists in the current directory\n  \
                     This directory is already initialized as a pack."
                );
            }

            let dir_name = cwd
                .file_name()
                .and_then(|n| n.to_str())
                .map(|s| s.to_string())
                .context(
                    "could not determine current directory name\n  \
                     Use `weave init <name>` to specify the pack name explicitly.",
                )?;

            validate_pack_name(&dir_name)?;

            // In-place init: the directory already exists and may contain
            // files that would be overwritten — scaffold must check.
            (cwd.clone(), dir_name, true)
        }
    };

    scaffold(&root_dir, &pack_name, check_existing)?;

    // Verify the generated pack.toml is valid by parsing it.
    let pack_toml_path = root_dir.join("pack.toml");
    let content = std::fs::read_to_string(&pack_toml_path)
        .with_context(|| format!("reading '{}'", pack_toml_path.display()))?;
    crate::core::pack::Pack::from_toml(&content, &pack_toml_path).with_context(|| {
        format!(
            "generated pack.toml at '{}' failed validation — this is a bug",
            pack_toml_path.display()
        )
    })?;

    println!(
        "{} pack '{}' in {}",
        style::success("Initialized"),
        style::pack_name(pack_name.as_str()),
        root_dir.display()
    );
    Ok(())
}
