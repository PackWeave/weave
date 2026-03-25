//! Core publish logic — file collection, version checking, and GitHub PR creation.
//!
//! This module handles the business logic of publishing a pack to the registry.
//! The CLI handler in `cli/publish.rs` is a thin wrapper around these functions.
//!
//! ## Security properties
//!
//! - `WEAVE_GITHUB_API_URL` override is restricted to HTTPS (no plaintext HTTP)
//! - Branch cleanup on failure prevents stale partial uploads
//! - File iteration is deterministic (BTreeMap, sorted keys)
//! - `parse_github_registry_url` uses proper domain matching (not substring)

use std::collections::BTreeMap;
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
/// Returns a sorted map of relative path (with forward slashes) → file bytes.
pub fn collect_pack_files(dir: &Path) -> Result<BTreeMap<String, Vec<u8>>> {
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

    let mut files = BTreeMap::new();

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
    files: &mut BTreeMap<String, Vec<u8>>,
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

        // Skip symlinks — prevents reading files outside the pack directory
        // (e.g., a symlink to ~/.ssh/id_rsa would leak sensitive content).
        let file_type = entry
            .file_type()
            .map_err(|e| WeaveError::io(format!("reading file type in {prefix}"), e))?;
        if file_type.is_symlink() {
            log::debug!("skipping symlink: {prefix}/{name}");
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
/// Uses proper domain matching (not substring) to reject `evil-github.com`.
pub fn parse_github_registry_url(url: &str) -> Result<(String, String)> {
    // Require HTTPS
    if !url.starts_with("https://") {
        return Err(WeaveError::Registry(format!(
            "registry URL '{url}' must use HTTPS"
        )));
    }

    let segments: Vec<&str> = url.strip_prefix("https://").unwrap().split('/').collect();

    // Expected: ["{host}", "{owner}", "{repo}", "{branch}", ...]
    if segments.len() < 3 {
        return Err(WeaveError::Registry(format!(
            "cannot parse registry URL '{url}' — expected https://raw.githubusercontent.com/{{owner}}/{{repo}}/{{branch}}"
        )));
    }

    // Proper domain matching (same logic as is_github_registry in credentials.rs)
    let host_with_port = segments[0];
    let host = host_with_port.split(':').next().unwrap_or(host_with_port);
    const GITHUB_DOMAINS: [&str; 2] = ["github.com", "githubusercontent.com"];
    let is_github = GITHUB_DOMAINS.iter().any(|domain| {
        host == *domain
            || (host.ends_with(domain)
                && host.as_bytes().get(host.len() - domain.len() - 1) == Some(&b'.'))
    });

    if !is_github {
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

/// Resolve the GitHub API base URL.
///
/// Defaults to `https://api.github.com`. Overridable via `WEAVE_GITHUB_API_URL`
/// for E2E tests. The override must use HTTPS (except for localhost, which
/// allows HTTP for test mock servers).
fn resolve_api_base() -> Result<String> {
    if let Ok(url) = std::env::var("WEAVE_GITHUB_API_URL") {
        // Allow http for localhost/127.0.0.1 (test mock servers)
        let is_localhost = url.starts_with("http://127.0.0.1:")
            || url.starts_with("http://127.0.0.1/")
            || url == "http://127.0.0.1"
            || url.starts_with("http://localhost:")
            || url.starts_with("http://localhost/")
            || url == "http://localhost";
        if url.starts_with("https://") || is_localhost {
            return Ok(url);
        }
        return Err(WeaveError::Registry(
            "WEAVE_GITHUB_API_URL must use HTTPS".into(),
        ));
    }
    Ok("https://api.github.com".to_string())
}

/// Create a PR on the registry GitHub repo with the pack files.
///
/// Uses the GitHub REST API to:
/// 1. Get the main branch SHA
/// 2. Create a `publish/{name}-{version}` branch
/// 3. Commit each file to `src/{name}/` on the branch (deterministic order)
/// 4. Open a PR from the branch to main
///
/// On failure after branch creation, attempts to clean up the branch.
pub fn create_registry_pr(
    owner: &str,
    repo: &str,
    pack: &Pack,
    files: &BTreeMap<String, Vec<u8>>,
    token: &str,
) -> Result<PublishResult> {
    let api_base = resolve_api_base()?;
    let client = reqwest::blocking::Client::new();
    let name = &pack.name;
    let version = &pack.version;
    let branch_name = format!("publish/{name}-{version}");

    // Step 1: Get main branch SHA
    let main_ref: serde_json::Value = github_api(
        &client,
        reqwest::Method::GET,
        &format!("{api_base}/repos/{owner}/{repo}/git/ref/heads/main"),
        token,
        None,
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
    github_api(
        &client,
        reqwest::Method::POST,
        &format!("{api_base}/repos/{owner}/{repo}/git/refs"),
        token,
        Some(&branch_body),
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

    // Step 3: Create files on the branch (deterministic order via BTreeMap).
    // On failure, attempt to clean up the branch.
    let ctx = PublishContext {
        client: &client,
        api_base: &api_base,
        owner,
        repo,
        name,
        version,
        branch_name: &branch_name,
        token,
    };
    let file_result = upload_files(&ctx, files);
    if let Err(e) = file_result {
        log::warn!("file upload failed, cleaning up branch: {e}");
        let _ = github_api(
            &client,
            reqwest::Method::DELETE,
            &format!("{api_base}/repos/{owner}/{repo}/git/refs/heads/{branch_name}"),
            token,
            None,
        );
        return Err(e);
    }

    // Step 4: Create PR
    // Truncate description to prevent GitHub API rejection (64KB limit).
    let desc = if pack.description.len() > 1000 {
        // Find a safe UTF-8 boundary at or before byte 1000.
        let end = pack.description[..1000]
            .char_indices()
            .last()
            .map(|(i, c)| i + c.len_utf8())
            .unwrap_or(1000);
        format!("{}...", &pack.description[..end])
    } else {
        pack.description.clone()
    };
    let pr_body = serde_json::json!({
        "title": format!("publish: {name}@{version}"),
        "head": branch_name,
        "base": "main",
        "body": format!(
            "Automated publish via `weave publish`.\n\n**Pack:** {name}\n**Version:** {version}\n**Description:** {desc}"
        ),
    });

    let pr_response: serde_json::Value = github_api(
        &client,
        reqwest::Method::POST,
        &format!("{api_base}/repos/{owner}/{repo}/pulls"),
        token,
        Some(&pr_body),
    )
    .map_err(|e| publish_err(name, version, &format!("creating PR: {e}")))?;

    let pr_url = pr_response["html_url"]
        .as_str()
        .ok_or_else(|| publish_err(name, version, "PR created but response missing html_url"))?
        .to_string();

    Ok(PublishResult {
        name: name.clone(),
        version: version.to_string(),
        pr_url,
    })
}

/// Upload all pack files to the branch. Returns the error if any file fails.
fn upload_files(ctx: &PublishContext<'_>, files: &BTreeMap<String, Vec<u8>>) -> Result<()> {
    for (rel_path, content) in files {
        let encoded = base64::engine::general_purpose::STANDARD.encode(content);
        let registry_path = format!("src/{}/{rel_path}", ctx.name);
        let file_body = serde_json::json!({
            "message": format!("publish: add {rel_path} for {}@{}", ctx.name, ctx.version),
            "content": encoded,
            "branch": ctx.branch_name,
        });

        github_api(
            ctx.client,
            reqwest::Method::PUT,
            &format!(
                "{}/repos/{}/{}/contents/{registry_path}",
                ctx.api_base, ctx.owner, ctx.repo
            ),
            ctx.token,
            Some(&file_body),
        )
        .map_err(|e| publish_err(ctx.name, ctx.version, &format!("uploading {rel_path}: {e}")))?;
    }
    Ok(())
}

/// Context for a publish operation — avoids passing 9+ args through functions.
struct PublishContext<'a> {
    client: &'a reqwest::blocking::Client,
    api_base: &'a str,
    owner: &'a str,
    repo: &'a str,
    name: &'a str,
    version: &'a semver::Version,
    branch_name: &'a str,
    token: &'a str,
}

/// Helper to construct a `PublishFailed` error.
fn publish_err(name: &str, version: &semver::Version, reason: &str) -> WeaveError {
    WeaveError::PublishFailed {
        name: name.to_string(),
        version: version.to_string(),
        reason: reason.to_string(),
    }
}

/// Unified GitHub API helper — handles GET, POST, PUT, DELETE with Bearer auth.
fn github_api(
    client: &reqwest::blocking::Client,
    method: reqwest::Method,
    url: &str,
    token: &str,
    body: Option<&serde_json::Value>,
) -> std::result::Result<serde_json::Value, String> {
    let mut request = client
        .request(method.clone(), url)
        .header("Authorization", format!("Bearer {token}"))
        .header("User-Agent", format!("weave/{}", env!("CARGO_PKG_VERSION")))
        .header("Accept", "application/vnd.github+json");

    if let Some(body) = body {
        request = request.json(body);
    }

    let response = request.send().map_err(|e| format!("network error: {e}"))?;

    let status = response.status();

    // DELETE returns 204 No Content on success — return empty object.
    if status == reqwest::StatusCode::NO_CONTENT {
        return Ok(serde_json::json!({}));
    }

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

    // ── URL parsing ──────────────────────────────────────────────────────

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
    fn parse_github_registry_url_rejects_http() {
        let result = parse_github_registry_url("http://raw.githubusercontent.com/owner/repo/main");
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("HTTPS"));
    }

    #[test]
    fn parse_github_registry_url_rejects_evil_subdomain() {
        let result = parse_github_registry_url("https://evil-github.com/owner/repo/main");
        assert!(result.is_err());
    }

    #[test]
    fn parse_github_registry_url_accepts_github_com() {
        let (owner, repo) =
            parse_github_registry_url("https://github.com/my-org/my-registry/main").unwrap();
        assert_eq!(owner, "my-org");
        assert_eq!(repo, "my-registry");
    }

    // ── File collection ──────────────────────────────────────────────────

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
        std::fs::create_dir_all(tmp.path().join("skills")).unwrap();
        std::fs::write(tmp.path().join("skills/review.md"), "skill").unwrap();
        std::fs::create_dir_all(tmp.path().join("settings")).unwrap();
        std::fs::write(tmp.path().join("settings/claude.json"), "{}").unwrap();

        let files = collect_pack_files(tmp.path()).unwrap();
        assert!(files.contains_key("pack.toml"));
        assert!(files.contains_key("prompts/system.md"));
        assert!(files.contains_key("commands/run.md"));
        assert!(files.contains_key("skills/review.md"));
        assert!(files.contains_key("settings/claude.json"));
    }

    #[test]
    fn collect_pack_files_includes_readme() {
        let tmp = tempfile::TempDir::new().unwrap();
        std::fs::write(
            tmp.path().join("pack.toml"),
            "[pack]\nname=\"test\"\nversion=\"0.1.0\"\ndescription=\"test\"\nauthors=[\"t\"]",
        )
        .unwrap();
        std::fs::write(tmp.path().join("README.md"), "# My Pack").unwrap();

        let files = collect_pack_files(tmp.path()).unwrap();
        assert!(files.contains_key("README.md"));
    }

    #[test]
    fn collect_pack_files_includes_nested_dirs() {
        let tmp = tempfile::TempDir::new().unwrap();
        std::fs::write(
            tmp.path().join("pack.toml"),
            "[pack]\nname=\"test\"\nversion=\"0.1.0\"\ndescription=\"test\"\nauthors=[\"t\"]",
        )
        .unwrap();
        std::fs::create_dir_all(tmp.path().join("prompts/sub")).unwrap();
        std::fs::write(tmp.path().join("prompts/sub/deep.md"), "nested").unwrap();

        let files = collect_pack_files(tmp.path()).unwrap();
        assert!(files.contains_key("prompts/sub/deep.md"));
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
    fn collect_pack_files_rejects_large_file() {
        let tmp = tempfile::TempDir::new().unwrap();
        std::fs::write(
            tmp.path().join("pack.toml"),
            "[pack]\nname=\"test\"\nversion=\"0.1.0\"\ndescription=\"test\"\nauthors=[\"t\"]",
        )
        .unwrap();
        std::fs::create_dir_all(tmp.path().join("prompts")).unwrap();
        // Create a file just over 1MB
        let large_content = vec![b'x'; 1_048_577];
        std::fs::write(tmp.path().join("prompts/huge.md"), &large_content).unwrap();

        let result = collect_pack_files(tmp.path());
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("too large"));
    }

    // ── Version checking ─────────────────────────────────────────────────

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
            schema_version: crate::core::registry::CURRENT_REGISTRY_SCHEMA_VERSION,
            name: "my-pack".into(),
            description: "test".into(),
            authors: vec![],
            license: None,
            repository: None,
            keywords: vec![],
            versions: vec![PackRelease {
                version: semver::Version::new(1, 0, 0),
                files: std::collections::HashMap::new(),
                dependencies: std::collections::HashMap::new(),
                checksum: None,
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
            schema_version: crate::core::registry::CURRENT_REGISTRY_SCHEMA_VERSION,
            name: "my-pack".into(),
            description: "test".into(),
            authors: vec![],
            license: None,
            repository: None,
            keywords: vec![],
            versions: vec![PackRelease {
                version: semver::Version::new(1, 0, 0),
                files: std::collections::HashMap::new(),
                dependencies: std::collections::HashMap::new(),
                checksum: None,
            }],
        });
        let result =
            check_version_not_published(&registry, "my-pack", &semver::Version::new(1, 1, 0));
        assert!(result.is_ok());
    }

    // ── API base resolution ──────────────────────────────────────────────

    #[test]
    fn resolve_api_base_default() {
        // When WEAVE_GITHUB_API_URL is not set, should return default.
        // (This test may see the env var if set externally, so just verify it starts with https.)
        let base = resolve_api_base().unwrap();
        assert!(
            base.starts_with("https://")
                || base.starts_with("http://127.0.0.1")
                || base.starts_with("http://localhost"),
            "API base should be HTTPS or localhost: {base}"
        );
    }
}
