//! Core publish logic — file collection, version checking, and GitHub PR creation.
//!
//! This module handles the business logic of publishing a pack to the registry.
//! The CLI handler in `cli/publish.rs` is a thin wrapper around these functions.

use std::collections::HashMap;
use std::path::Path;

use base64::Engine;

use crate::core::pack::Pack;
use crate::core::registry::Registry;
use crate::error::{Result, WeaveError};

/// Result of a successful publish operation.
#[derive(Debug)]
pub struct PublishResult {
    pub name: String,
    pub version: String,
    pub pr_url: String,
}

/// Collect all publishable files from a pack directory.
///
/// Walks the directory, collecting files from the allowlisted set:
/// `pack.toml`, `README.md`, `prompts/`, `commands/`, `skills/`, `settings/`.
/// Skips hidden entries and non-pack directories.
///
/// Returns a map of relative path (with forward slashes) → file bytes.
pub fn collect_pack_files(dir: &Path) -> Result<HashMap<String, Vec<u8>>> {
    let pack_toml = dir.join("pack.toml");
    if !pack_toml.exists() {
        return Err(WeaveError::io(
            "collecting pack files",
            std::io::Error::new(
                std::io::ErrorKind::NotFound,
                "pack.toml not found — run `weave publish` from a pack directory",
            ),
        ));
    }

    let mut files = HashMap::new();

    // Allowlisted top-level files
    for name in &["pack.toml", "README.md"] {
        let path = dir.join(name);
        if path.is_file() {
            let content =
                std::fs::read(&path).map_err(|e| WeaveError::io(format!("reading {name}"), e))?;
            files.insert(name.to_string(), content);
        }
    }

    // Allowlisted directories (recursive)
    for dirname in &["prompts", "commands", "skills", "settings"] {
        let subdir = dir.join(dirname);
        if subdir.is_dir() {
            collect_dir_recursive(&subdir, dirname, &mut files)?;
        }
    }

    Ok(files)
}

/// Recursively collect files from a subdirectory.
fn collect_dir_recursive(
    dir: &Path,
    prefix: &str,
    files: &mut HashMap<String, Vec<u8>>,
) -> Result<()> {
    let entries = std::fs::read_dir(dir)
        .map_err(|e| WeaveError::io(format!("reading directory {prefix}"), e))?;

    for entry in entries {
        let entry = entry.map_err(|e| WeaveError::io(format!("reading entry in {prefix}"), e))?;
        let name = entry.file_name().to_string_lossy().to_string();

        // Skip hidden files
        if name.starts_with('.') {
            continue;
        }

        let path = entry.path();
        let rel_path = format!("{prefix}/{name}");

        if path.is_file() {
            // Guard: reject files over 1MB (GitHub Contents API limit)
            let meta = std::fs::metadata(&path)
                .map_err(|e| WeaveError::io(format!("reading metadata for {rel_path}"), e))?;
            if meta.len() > 1_048_576 {
                return Err(WeaveError::io(
                    format!("file {rel_path} is too large ({} bytes)", meta.len()),
                    std::io::Error::new(
                        std::io::ErrorKind::InvalidData,
                        "pack files must be under 1MB",
                    ),
                ));
            }
            let content = std::fs::read(&path)
                .map_err(|e| WeaveError::io(format!("reading {rel_path}"), e))?;
            files.insert(rel_path, content);
        } else if path.is_dir() {
            collect_dir_recursive(&path, &rel_path, files)?;
        }
    }

    Ok(())
}

/// Parse a GitHub raw.githubusercontent.com URL into (owner, repo).
///
/// Expected format: `https://raw.githubusercontent.com/{owner}/{repo}/{branch}`
pub fn parse_github_registry_url(url: &str) -> Result<(String, String)> {
    let segments: Vec<&str> = url.split("://").nth(1).unwrap_or("").split('/').collect();

    // Expected: ["raw.githubusercontent.com", "{owner}", "{repo}", "{branch}", ...]
    if segments.len() < 3 {
        return Err(WeaveError::Registry(format!(
            "cannot parse registry URL '{url}' — expected https://raw.githubusercontent.com/{{owner}}/{{repo}}/{{branch}}"
        )));
    }

    let host = segments[0];
    if !host.contains("githubusercontent.com") && !host.contains("github.com") {
        return Err(WeaveError::Registry(format!(
            "registry URL '{url}' is not a GitHub-backed registry — publish requires GitHub"
        )));
    }

    let owner = segments[1].to_string();
    let repo = segments[2].to_string();

    if owner.is_empty() || repo.is_empty() {
        return Err(WeaveError::Registry(format!(
            "cannot parse owner/repo from registry URL '{url}'"
        )));
    }

    Ok((owner, repo))
}

/// Check whether the given version already exists in the registry.
///
/// Returns `Ok(())` if the version is new (safe to publish).
/// Returns `Err(VersionAlreadyPublished)` if the version exists.
/// A `PackNotFound` error from the registry means this is a brand-new pack — that's fine.
pub fn check_version_not_published(
    registry: &dyn Registry,
    name: &str,
    version: &semver::Version,
) -> Result<()> {
    match registry.fetch_metadata(name) {
        Ok(meta) => {
            if meta.versions.iter().any(|v| &v.version == version) {
                return Err(WeaveError::VersionAlreadyPublished {
                    name: name.to_string(),
                    version: version.to_string(),
                });
            }
            Ok(())
        }
        Err(WeaveError::PackNotFound { .. }) => Ok(()),
        Err(e) => Err(e),
    }
}

/// Create a PR on the registry GitHub repo with the pack files.
///
/// Uses the GitHub REST API to:
/// 1. Get the main branch SHA
/// 2. Create a `publish/{name}-{version}` branch
/// 3. Commit each file to `src/{name}/` on the branch
/// 4. Open a PR from the branch to main
pub fn create_registry_pr(
    owner: &str,
    repo: &str,
    pack: &Pack,
    files: &HashMap<String, Vec<u8>>,
    token: &str,
) -> Result<PublishResult> {
    let api_base = std::env::var("WEAVE_GITHUB_API_URL")
        .unwrap_or_else(|_| "https://api.github.com".to_string());

    let client = reqwest::blocking::Client::new();
    let name = &pack.name;
    let version = &pack.version;
    let branch_name = format!("publish/{name}-{version}");

    // Step 1: Get main branch SHA
    let main_ref: serde_json::Value = github_get(
        &client,
        &format!("{api_base}/repos/{owner}/{repo}/git/ref/heads/main"),
        token,
    )
    .map_err(|e| publish_err(name, version, &format!("getting main branch: {e}")))?;

    let main_sha = main_ref["object"]["sha"]
        .as_str()
        .ok_or_else(|| publish_err(name, version, "unexpected response: missing main SHA"))?;

    // Step 2: Create branch
    let branch_body = serde_json::json!({
        "ref": format!("refs/heads/{branch_name}"),
        "sha": main_sha,
    });
    github_post(
        &client,
        &format!("{api_base}/repos/{owner}/{repo}/git/refs"),
        token,
        &branch_body,
    )
    .map_err(|e| {
        publish_err(
            name,
            version,
            &format!(
                "creating branch '{branch_name}': {e} — a branch for this version may already exist"
            ),
        )
    })?;

    // Step 3: Create files on the branch
    for (rel_path, content) in files {
        let encoded = base64::engine::general_purpose::STANDARD.encode(content);
        let registry_path = format!("src/{name}/{rel_path}");
        let file_body = serde_json::json!({
            "message": format!("publish: add {rel_path} for {name}@{version}"),
            "content": encoded,
            "branch": branch_name,
        });

        github_put(
            &client,
            &format!("{api_base}/repos/{owner}/{repo}/contents/{registry_path}"),
            token,
            &file_body,
        )
        .map_err(|e| publish_err(name, version, &format!("uploading {rel_path}: {e}")))?;
    }

    // Step 4: Create PR
    let pr_body = serde_json::json!({
        "title": format!("publish: {name}@{version}"),
        "head": branch_name,
        "base": "main",
        "body": format!(
            "Automated publish via `weave publish`.\n\n**Pack:** {name}\n**Version:** {version}\n**Description:** {}",
            pack.description
        ),
    });

    let pr_response: serde_json::Value = github_post(
        &client,
        &format!("{api_base}/repos/{owner}/{repo}/pulls"),
        token,
        &pr_body,
    )
    .map_err(|e| publish_err(name, version, &format!("creating PR: {e}")))?;

    let pr_url = pr_response["html_url"]
        .as_str()
        .unwrap_or("(unknown)")
        .to_string();

    Ok(PublishResult {
        name: name.clone(),
        version: version.to_string(),
        pr_url,
    })
}

/// Helper to construct a `PublishFailed` error.
fn publish_err(name: &str, version: &semver::Version, reason: &str) -> WeaveError {
    WeaveError::PublishFailed {
        name: name.to_string(),
        version: version.to_string(),
        reason: reason.to_string(),
    }
}

/// GitHub API GET with Bearer auth.
fn github_get(
    client: &reqwest::blocking::Client,
    url: &str,
    token: &str,
) -> std::result::Result<serde_json::Value, String> {
    let response = client
        .get(url)
        .header("Authorization", format!("Bearer {token}"))
        .header("User-Agent", format!("weave/{}", env!("CARGO_PKG_VERSION")))
        .header("Accept", "application/vnd.github+json")
        .send()
        .map_err(|e| format!("network error: {e}"))?;

    let status = response.status();
    if !status.is_success() {
        let body = response.text().unwrap_or_default();
        return Err(format!("HTTP {status}: {body}"));
    }

    response
        .json()
        .map_err(|e| format!("JSON parse error: {e}"))
}

/// GitHub API POST with Bearer auth.
fn github_post(
    client: &reqwest::blocking::Client,
    url: &str,
    token: &str,
    body: &serde_json::Value,
) -> std::result::Result<serde_json::Value, String> {
    let response = client
        .post(url)
        .header("Authorization", format!("Bearer {token}"))
        .header("User-Agent", format!("weave/{}", env!("CARGO_PKG_VERSION")))
        .header("Accept", "application/vnd.github+json")
        .json(body)
        .send()
        .map_err(|e| format!("network error: {e}"))?;

    let status = response.status();
    if !status.is_success() {
        let body = response.text().unwrap_or_default();
        return Err(format!("HTTP {status}: {body}"));
    }

    response
        .json()
        .map_err(|e| format!("JSON parse error: {e}"))
}

/// GitHub API PUT with Bearer auth.
fn github_put(
    client: &reqwest::blocking::Client,
    url: &str,
    token: &str,
    body: &serde_json::Value,
) -> std::result::Result<serde_json::Value, String> {
    let response = client
        .put(url)
        .header("Authorization", format!("Bearer {token}"))
        .header("User-Agent", format!("weave/{}", env!("CARGO_PKG_VERSION")))
        .header("Accept", "application/vnd.github+json")
        .json(body)
        .send()
        .map_err(|e| format!("network error: {e}"))?;

    let status = response.status();
    if !status.is_success() {
        let body = response.text().unwrap_or_default();
        return Err(format!("HTTP {status}: {body}"));
    }

    response
        .json()
        .map_err(|e| format!("JSON parse error: {e}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_github_registry_url_default() {
        let (owner, repo) =
            parse_github_registry_url("https://raw.githubusercontent.com/PackWeave/registry/main")
                .unwrap();
        assert_eq!(owner, "PackWeave");
        assert_eq!(repo, "registry");
    }

    #[test]
    fn parse_github_registry_url_custom_org() {
        let (owner, repo) =
            parse_github_registry_url("https://raw.githubusercontent.com/my-org/my-registry/main")
                .unwrap();
        assert_eq!(owner, "my-org");
        assert_eq!(repo, "my-registry");
    }

    #[test]
    fn parse_github_registry_url_non_github_fails() {
        let result = parse_github_registry_url("https://example.com/registry");
        assert!(result.is_err());
    }

    #[test]
    fn parse_github_registry_url_too_short_fails() {
        let result = parse_github_registry_url("https://raw.githubusercontent.com/only-one");
        assert!(result.is_err());
    }

    #[test]
    fn collect_pack_files_requires_pack_toml() {
        let tmp = tempfile::TempDir::new().unwrap();
        let result = collect_pack_files(tmp.path());
        assert!(result.is_err());
    }

    #[test]
    fn collect_pack_files_includes_expected_dirs() {
        let tmp = tempfile::TempDir::new().unwrap();
        std::fs::write(
            tmp.path().join("pack.toml"),
            "[pack]\nname=\"test\"\nversion=\"0.1.0\"\ndescription=\"test\"\nauthors=[\"t\"]",
        )
        .unwrap();
        std::fs::create_dir_all(tmp.path().join("prompts")).unwrap();
        std::fs::write(tmp.path().join("prompts/system.md"), "prompt").unwrap();
        std::fs::create_dir_all(tmp.path().join("commands")).unwrap();
        std::fs::write(tmp.path().join("commands/run.md"), "cmd").unwrap();

        let files = collect_pack_files(tmp.path()).unwrap();
        assert!(files.contains_key("pack.toml"));
        assert!(files.contains_key("prompts/system.md"));
        assert!(files.contains_key("commands/run.md"));
    }

    #[test]
    fn collect_pack_files_skips_hidden_and_unknown() {
        let tmp = tempfile::TempDir::new().unwrap();
        std::fs::write(
            tmp.path().join("pack.toml"),
            "[pack]\nname=\"test\"\nversion=\"0.1.0\"\ndescription=\"test\"\nauthors=[\"t\"]",
        )
        .unwrap();
        std::fs::create_dir_all(tmp.path().join(".git")).unwrap();
        std::fs::write(tmp.path().join(".git/config"), "gitconfig").unwrap();
        std::fs::write(tmp.path().join("random.txt"), "junk").unwrap();

        let files = collect_pack_files(tmp.path()).unwrap();
        assert!(!files.keys().any(|k| k.starts_with(".git")));
        assert!(!files.contains_key("random.txt"));
        assert!(files.contains_key("pack.toml"));
    }

    #[test]
    fn check_version_not_published_new_pack_ok() {
        use crate::core::registry::MockRegistry;
        let registry = MockRegistry::new();
        let result =
            check_version_not_published(&registry, "new-pack", &semver::Version::new(0, 1, 0));
        assert!(result.is_ok());
    }

    #[test]
    fn check_version_not_published_existing_version_fails() {
        use crate::core::registry::{MockRegistry, PackMetadata, PackRelease};
        let mut registry = MockRegistry::new();
        registry.add_pack(PackMetadata {
            name: "my-pack".into(),
            description: "test".into(),
            authors: vec![],
            license: None,
            repository: None,
            keywords: vec![],
            versions: vec![PackRelease {
                version: semver::Version::new(1, 0, 0),
                files: HashMap::new(),
                dependencies: HashMap::new(),
            }],
        });
        let result =
            check_version_not_published(&registry, "my-pack", &semver::Version::new(1, 0, 0));
        assert!(result.is_err());
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("already published")
        );
    }

    #[test]
    fn check_version_not_published_different_version_ok() {
        use crate::core::registry::{MockRegistry, PackMetadata, PackRelease};
        let mut registry = MockRegistry::new();
        registry.add_pack(PackMetadata {
            name: "my-pack".into(),
            description: "test".into(),
            authors: vec![],
            license: None,
            repository: None,
            keywords: vec![],
            versions: vec![PackRelease {
                version: semver::Version::new(1, 0, 0),
                files: HashMap::new(),
                dependencies: HashMap::new(),
            }],
        });
        let result =
            check_version_not_published(&registry, "my-pack", &semver::Version::new(1, 1, 0));
        assert!(result.is_ok());
    }
}
