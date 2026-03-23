use std::collections::HashMap;

use serde::{Deserialize, Serialize};

use crate::error::{Result, WeaveError};

/// Summary of a pack in registry search results.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PackSummary {
    pub name: String,
    pub description: String,
    pub latest_version: semver::Version,
    #[serde(default)]
    pub keywords: Vec<String>,
}

/// Full metadata for a pack in the registry.
/// Deserialized from `packs/{name}.json` in the sparse index.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PackMetadata {
    pub name: String,
    pub description: String,
    #[serde(default)]
    pub authors: Vec<String>,
    #[serde(default)]
    pub license: Option<String>,
    #[serde(default)]
    pub repository: Option<String>,
    #[serde(default)]
    pub keywords: Vec<String>,
    pub versions: Vec<PackRelease>,
}

/// A specific release of a pack.
///
/// `files` is a flat map of relative path → file content (e.g. `"pack.toml"`,
/// `"prompts/system.md"`). The store writes these directly to disk — no tarball,
/// no SHA256 verification, no additional network download beyond the registry JSON.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PackRelease {
    pub version: semver::Version,
    /// Pack file contents keyed by relative path.
    #[serde(default)]
    pub files: HashMap<String, String>,
    /// Direct dependencies of this release, keyed by pack name with a semver requirement.
    #[serde(default)]
    pub dependencies: HashMap<String, semver::VersionReq>,
}

/// The registry trait. All registry implementations must be Send + Sync.
pub trait Registry: Send + Sync {
    /// Search for packs matching a query string.
    fn search(&self, query: &str) -> Result<Vec<PackSummary>>;

    /// Fetch full metadata for a pack by name.
    fn fetch_metadata(&self, name: &str) -> Result<PackMetadata>;

    /// Fetch a specific version of a pack.
    fn fetch_version(&self, name: &str, version: &semver::Version) -> Result<PackRelease>;

    /// Publish a pack archive to the registry. Deferred to Milestone 3 (v0.2).
    fn publish(&self, _archive: &std::path::Path, _token: &str) -> Result<()> {
        Err(crate::error::WeaveError::Registry(
            "publish is not yet supported — see https://github.com/PackWeave/weave for updates"
                .into(),
        ))
    }
}

/// Entry in the lightweight search index (`index.json`).
/// Contains only what is needed for `weave search` and `weave list` — no version arrays.
/// Clients fetch this once and cache it in-process.
#[derive(Debug, Clone, Deserialize)]
struct PackListing {
    #[allow(dead_code)]
    name: String,
    description: String,
    #[serde(default)]
    keywords: Vec<String>,
    latest_version: semver::Version,
}

/// The lightweight search index — a flat JSON object mapping pack names to their listing.
type SearchIndex = HashMap<String, PackListing>;

/// GitHub-backed registry implementation using a two-tier sparse index.
///
/// - `{base_url}/index.json` — lightweight catalog fetched once for search and listing
/// - `{base_url}/packs/{name}.json` — full metadata fetched on demand per pack
pub struct GitHubRegistry {
    base_url: String,
    token: Option<String>,
    cached_search_index: std::sync::Mutex<Option<SearchIndex>>,
    cached_packs: std::sync::Mutex<HashMap<String, PackMetadata>>,
}

impl GitHubRegistry {
    pub fn new(base_url: &str, token: Option<String>) -> Self {
        // Strip trailing slash and also normalise old-style URLs that already
        // include the `/index.json` suffix (e.g. configs written before the
        // sparse-index migration).  Without this, old installs would request
        // `.../index.json/index.json` and break silently.
        let base_url = base_url
            .trim_end_matches('/')
            .trim_end_matches("/index.json");
        Self {
            base_url: base_url.to_string(),
            token,
            cached_search_index: std::sync::Mutex::new(None),
            cached_packs: std::sync::Mutex::new(HashMap::new()),
        }
    }

    /// Fetch and cache the lightweight `index.json`.
    fn load_search_index(&self) -> Result<SearchIndex> {
        {
            let cache = self
                .cached_search_index
                .lock()
                .unwrap_or_else(|e| e.into_inner());
            if let Some(ref index) = *cache {
                return Ok(index.clone());
            }
        }

        let url = format!("{}/index.json", self.base_url);
        let index: SearchIndex =
            http_get_json(&url, "registry search index", self.token.as_deref())?;

        {
            let mut cache = self
                .cached_search_index
                .lock()
                .unwrap_or_else(|e| e.into_inner());
            *cache = Some(index.clone());
        }

        Ok(index)
    }

    /// Fetch and cache full metadata for a single pack from `packs/{name}.json`.
    fn load_pack_metadata(&self, name: &str) -> Result<PackMetadata> {
        {
            let cache = self.cached_packs.lock().unwrap_or_else(|e| e.into_inner());
            if let Some(meta) = cache.get(name) {
                return Ok(meta.clone());
            }
        }

        // Validate the name before interpolating it into the URL.  Pack names
        // must be [a-z0-9-]+ and this is already enforced for user-supplied
        // names by Pack::validate, but dependency names from registry responses
        // could in theory contain path-traversal segments like `../`.
        if name.is_empty()
            || !name
                .chars()
                .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
        {
            return Err(WeaveError::Registry(format!(
                "invalid pack name '{name}' — names must contain only lowercase letters, numbers, and hyphens"
            )));
        }

        let url = format!("{}/packs/{}.json", self.base_url, name);
        let meta: PackMetadata = http_get_json(
            &url,
            &format!("pack metadata for '{name}'"),
            self.token.as_deref(),
        )
        .map_err(|e| match e {
            WeaveError::RegistryHttp { status: 404, .. } => WeaveError::PackNotFound {
                name: name.to_string(),
            },
            other => other,
        })?;

        {
            let mut cache = self.cached_packs.lock().unwrap_or_else(|e| e.into_inner());
            cache.insert(name.to_string(), meta.clone());
        }

        Ok(meta)
    }
}

impl Registry for GitHubRegistry {
    fn search(&self, query: &str) -> Result<Vec<PackSummary>> {
        let index = self.load_search_index()?;
        let query_lower = query.to_lowercase();

        let mut results: Vec<PackSummary> = index
            .iter()
            .filter(|(name, listing)| {
                name.to_lowercase().contains(&query_lower)
                    || listing.description.to_lowercase().contains(&query_lower)
                    || listing
                        .keywords
                        .iter()
                        .any(|k| k.to_lowercase().contains(&query_lower))
            })
            .map(|(name, listing)| PackSummary {
                name: name.clone(),
                description: listing.description.clone(),
                latest_version: listing.latest_version.clone(),
                keywords: listing.keywords.clone(),
            })
            .collect();

        results.sort_by(|a, b| a.name.cmp(&b.name));
        Ok(results)
    }

    fn fetch_metadata(&self, name: &str) -> Result<PackMetadata> {
        self.load_pack_metadata(name)
    }

    fn fetch_version(&self, name: &str, version: &semver::Version) -> Result<PackRelease> {
        let metadata = self.fetch_metadata(name)?;
        metadata
            .versions
            .iter()
            .find(|v| &v.version == version)
            .cloned()
            .ok_or_else(|| WeaveError::VersionNotFound {
                name: name.to_string(),
                version: version.to_string(),
                available: metadata
                    .versions
                    .iter()
                    .map(|v| v.version.to_string())
                    .collect::<Vec<_>>()
                    .join(", "),
            })
    }
}

impl PackMetadata {
    /// Get the latest (highest) version, or an error if the pack has no releases.
    pub fn latest_version(&self) -> Result<semver::Version> {
        self.versions
            .iter()
            .map(|v| &v.version)
            .max()
            .cloned()
            .ok_or_else(|| WeaveError::NoReleases {
                name: self.name.clone(),
            })
    }
}

/// Perform a blocking HTTP GET and deserialize the JSON response body.
///
/// When `token` is `Some`, an `Authorization: Bearer` header is included in
/// the request. This raises the GitHub API rate limit from 60/hr to 5000/hr
/// and is required for accessing private registries.
fn http_get_json<T: serde::de::DeserializeOwned>(
    url: &str,
    label: &str,
    token: Option<&str>,
) -> Result<T> {
    let client = reqwest::blocking::Client::new();
    let mut request = client
        .get(url)
        .header("User-Agent", format!("weave/{}", env!("CARGO_PKG_VERSION")));
    // Only send the token to trusted GitHub hosts. If registry_url in config
    // points to a non-GitHub domain, the token must not be leaked to it.
    if let Some(token) = token {
        let host_with_port = url
            .split("://")
            .nth(1)
            .and_then(|rest| rest.split('/').next())
            .unwrap_or("");
        let host = host_with_port.split(':').next().unwrap_or(host_with_port);
        const TRUSTED_HOSTS: [&str; 2] = ["api.github.com", "raw.githubusercontent.com"];
        if TRUSTED_HOSTS.iter().any(|h| host.eq_ignore_ascii_case(h)) {
            request = request.header("Authorization", format!("Bearer {token}"));
        }
    }
    let response = request
        .send()
        .map_err(|e| WeaveError::Registry(format!("failed to fetch {label}: {e}")))?;

    let status = response.status().as_u16();
    if !response.status().is_success() {
        return Err(WeaveError::RegistryHttp {
            status,
            url: url.to_string(),
        });
    }

    response
        .json()
        .map_err(|e| WeaveError::Registry(format!("failed to parse {label}: {e}")))
}

/// Build a registry from a [`Config`](crate::core::config::Config): the official
/// registry plus any registered community taps, wrapped in a [`CompositeRegistry`].
///
/// If no taps are configured the composite still works correctly — it degrades to
/// a single-registry wrapper with negligible overhead.
pub fn registry_from_config(config: &crate::core::config::Config) -> CompositeRegistry {
    let token = crate::core::credentials::resolve_token(config)
        .unwrap_or_else(|e| {
            log::warn!("failed to resolve auth token: {e}");
            None
        })
        .map(|r| r.token);
    // Token is only sent to the official registry, never to community taps.
    // A malicious tap operator could otherwise harvest the user's GitHub PAT.
    let official = GitHubRegistry::new(&config.registry_url, token);
    let taps = config
        .taps
        .iter()
        .map(|t| GitHubRegistry::new(&t.url, None))
        .collect();
    CompositeRegistry::new(official, taps)
}

/// A composite registry that searches the official registry first, then community taps in order.
///
/// - `search()` merges results from all registries, deduplicating by pack name (official wins).
/// - `fetch_metadata()` tries the official registry first, then taps in registration order.
/// - `fetch_version()` follows the same priority as `fetch_metadata()`.
/// - `publish()` always delegates to the official (primary) registry.
pub struct CompositeRegistry {
    registries: Vec<Box<dyn Registry>>,
}

impl CompositeRegistry {
    /// Create a composite registry from the official registry and a list of tap registries.
    ///
    /// The official registry is always first; taps follow in the order they appear.
    pub fn new(official: GitHubRegistry, taps: Vec<GitHubRegistry>) -> Self {
        let mut registries: Vec<Box<dyn Registry>> = Vec::with_capacity(1 + taps.len());
        registries.push(Box::new(official));
        for tap in taps {
            registries.push(Box::new(tap));
        }
        Self { registries }
    }

    /// Create a composite registry from pre-boxed trait objects.
    ///
    /// The first registry in `registries` is treated as the "official" registry
    /// (index 0) and the rest are taps, searched in order. This mirrors the
    /// invariant established by [`CompositeRegistry::new`].
    ///
    /// Test-only: allows injecting mock `Registry` implementations directly so
    /// tests exercise the real `CompositeRegistry` fallthrough and error-handling
    /// logic instead of reimplementing it.
    #[cfg(test)]
    pub fn from_boxed(registries: Vec<Box<dyn Registry>>) -> Self {
        assert!(
            !registries.is_empty(),
            "at least one registry (official) is required"
        );
        Self { registries }
    }
}

impl Registry for CompositeRegistry {
    fn search(&self, query: &str) -> Result<Vec<PackSummary>> {
        let mut seen = std::collections::HashSet::new();
        let mut results = Vec::new();

        for (idx, registry) in self.registries.iter().enumerate() {
            match registry.search(query) {
                Ok(packs) => {
                    for pack in packs {
                        if seen.insert(pack.name.clone()) {
                            results.push(pack);
                        }
                    }
                }
                Err(e) => {
                    if idx == 0 {
                        // Official registry failure is fatal — don't silently return empty results.
                        return Err(e);
                    }
                    // Tap failures are logged but don't abort the search.
                    log::warn!("tap search failed: {e}");
                }
            }
        }

        results.sort_by(|a, b| a.name.cmp(&b.name));
        Ok(results)
    }

    fn fetch_metadata(&self, name: &str) -> Result<PackMetadata> {
        let mut last_not_found: Option<WeaveError> = None;
        for (idx, registry) in self.registries.iter().enumerate() {
            match registry.fetch_metadata(name) {
                Ok(meta) => return Ok(meta),
                Err(e) => match &e {
                    WeaveError::PackNotFound { .. } => {
                        last_not_found = Some(e);
                    }
                    _ => {
                        if idx == 0 {
                            return Err(e);
                        }
                        log::warn!("tap metadata fetch failed: {e}");
                    }
                },
            }
        }
        Err(last_not_found.unwrap_or_else(|| WeaveError::PackNotFound {
            name: name.to_string(),
        }))
    }

    fn fetch_version(&self, name: &str, version: &semver::Version) -> Result<PackRelease> {
        let mut last_not_found: Option<WeaveError> = None;
        for (idx, registry) in self.registries.iter().enumerate() {
            match registry.fetch_version(name, version) {
                Ok(release) => return Ok(release),
                Err(e) => match &e {
                    WeaveError::PackNotFound { .. } | WeaveError::VersionNotFound { .. } => {
                        last_not_found = Some(e);
                    }
                    _ => {
                        if idx == 0 {
                            return Err(e);
                        }
                        log::warn!("tap version fetch failed: {e}");
                    }
                },
            }
        }
        Err(last_not_found.unwrap_or_else(|| WeaveError::PackNotFound {
            name: name.to_string(),
        }))
    }

    fn publish(&self, archive: &std::path::Path, token: &str) -> Result<()> {
        // Publish always goes to the official (first) registry.
        if let Some(primary) = self.registries.first() {
            primary.publish(archive, token)
        } else {
            Err(WeaveError::Registry("no registries configured".to_string()))
        }
    }
}

/// A mock registry for testing. No network calls.
#[cfg(test)]
#[derive(Default)]
pub struct MockRegistry {
    pub packs: HashMap<String, PackMetadata>,
}

#[cfg(test)]
impl MockRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn add_pack(&mut self, metadata: PackMetadata) {
        self.packs.insert(metadata.name.clone(), metadata);
    }
}

#[cfg(test)]
impl Registry for MockRegistry {
    fn search(&self, query: &str) -> Result<Vec<PackSummary>> {
        let query_lower = query.to_lowercase();
        Ok(self
            .packs
            .values()
            .filter_map(|meta| {
                let name_lower = meta.name.to_lowercase();
                let desc_lower = meta.description.to_lowercase();
                if name_lower.contains(&query_lower) || desc_lower.contains(&query_lower) {
                    meta.latest_version().ok().map(|ver| PackSummary {
                        name: meta.name.clone(),
                        description: meta.description.clone(),
                        latest_version: ver,
                        keywords: meta.keywords.clone(),
                    })
                } else {
                    None
                }
            })
            .collect())
    }

    fn fetch_metadata(&self, name: &str) -> Result<PackMetadata> {
        self.packs
            .get(name)
            .cloned()
            .ok_or_else(|| WeaveError::PackNotFound {
                name: name.to_string(),
            })
    }

    fn fetch_version(&self, name: &str, version: &semver::Version) -> Result<PackRelease> {
        let meta = self.fetch_metadata(name)?;
        meta.versions
            .iter()
            .find(|v| &v.version == version)
            .cloned()
            .ok_or_else(|| WeaveError::VersionNotFound {
                name: name.to_string(),
                version: version.to_string(),
                available: meta
                    .versions
                    .iter()
                    .map(|v| v.version.to_string())
                    .collect::<Vec<_>>()
                    .join(", "),
            })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_metadata() -> PackMetadata {
        PackMetadata {
            name: "webdev".into(),
            description: "Web development tools".into(),
            authors: vec!["tester".into()],
            license: Some("MIT".into()),
            repository: None,
            keywords: vec!["web".into(), "dev".into()],
            versions: vec![
                PackRelease {
                    version: semver::Version::new(1, 0, 0),
                    files: HashMap::from([(
                        "pack.toml".to_string(),
                        "[pack]\nname = \"webdev\"\nversion = \"1.0.0\"\ndescription = \"Web development tools\"\n".to_string(),
                    )]),
                    dependencies: HashMap::new(),
                },
                PackRelease {
                    version: semver::Version::new(1, 1, 0),
                    files: HashMap::from([(
                        "pack.toml".to_string(),
                        "[pack]\nname = \"webdev\"\nversion = \"1.1.0\"\ndescription = \"Web development tools\"\n".to_string(),
                    )]),
                    dependencies: HashMap::new(),
                },
            ],
        }
    }

    #[test]
    fn mock_registry_search() {
        let mut registry = MockRegistry::new();
        registry.add_pack(sample_metadata());

        let results = registry.search("web").unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].name, "webdev");
    }

    #[test]
    fn mock_registry_fetch_version() {
        let mut registry = MockRegistry::new();
        registry.add_pack(sample_metadata());

        let release = registry
            .fetch_version("webdev", &semver::Version::new(1, 0, 0))
            .unwrap();
        assert!(release.files.contains_key("pack.toml"));

        let err = registry.fetch_version("webdev", &semver::Version::new(9, 9, 9));
        assert!(err.is_err());
    }

    #[test]
    fn latest_version() {
        let meta = sample_metadata();
        assert_eq!(
            meta.latest_version().unwrap(),
            semver::Version::new(1, 1, 0)
        );
    }

    #[test]
    fn new_strips_trailing_slash() {
        let r = GitHubRegistry::new("https://example.com/registry/", None);
        assert_eq!(r.base_url, "https://example.com/registry");
    }

    #[test]
    fn new_strips_old_index_json_suffix() {
        // Old configs stored the full URL including /index.json.
        let r = GitHubRegistry::new("https://example.com/registry/index.json", None);
        assert_eq!(r.base_url, "https://example.com/registry");
    }

    #[test]
    fn new_strips_index_json_with_trailing_slash() {
        let r = GitHubRegistry::new("https://example.com/registry/index.json/", None);
        assert_eq!(r.base_url, "https://example.com/registry");
    }

    // ── CompositeRegistry tests ────────────────────────────────────────────

    fn sample_metadata_named(name: &str, desc: &str) -> PackMetadata {
        PackMetadata {
            name: name.into(),
            description: desc.into(),
            authors: vec!["tester".into()],
            license: Some("MIT".into()),
            repository: None,
            keywords: vec![],
            versions: vec![PackRelease {
                version: semver::Version::new(1, 0, 0),
                files: HashMap::new(),
                dependencies: HashMap::new(),
            }],
        }
    }

    /// First registry is treated as official; rest are taps.
    fn composite_from_mocks(registries: Vec<MockRegistry>) -> CompositeRegistry {
        CompositeRegistry::from_boxed(
            registries
                .into_iter()
                .map(|r| Box::new(r) as Box<dyn Registry>)
                .collect(),
        )
    }

    /// A mock registry that always returns an error, for testing error-handling paths.
    struct FailingRegistry {
        error: String,
    }

    impl FailingRegistry {
        fn new(error: &str) -> Self {
            Self {
                error: error.to_string(),
            }
        }
    }

    impl Registry for FailingRegistry {
        fn search(&self, _query: &str) -> Result<Vec<PackSummary>> {
            Err(WeaveError::Registry(self.error.clone()))
        }

        fn fetch_metadata(&self, _name: &str) -> Result<PackMetadata> {
            Err(WeaveError::Registry(self.error.clone()))
        }

        fn fetch_version(&self, _name: &str, _version: &semver::Version) -> Result<PackRelease> {
            Err(WeaveError::Registry(self.error.clone()))
        }
    }

    #[test]
    fn composite_search_merges_deduplicates() {
        let mut official = MockRegistry::new();
        official.add_pack(sample_metadata_named("webdev", "official webdev"));

        let mut tap = MockRegistry::new();
        tap.add_pack(sample_metadata_named("webdev", "tap webdev")); // duplicate
        tap.add_pack(sample_metadata_named("tap-only", "from tap"));

        let composite = composite_from_mocks(vec![official, tap]);
        let results = composite.search("").unwrap();

        let names: Vec<&str> = results.iter().map(|p| p.name.as_str()).collect();
        assert!(names.contains(&"webdev"), "should include webdev");
        assert!(names.contains(&"tap-only"), "should include tap-only");
        // webdev should only appear once (official wins)
        assert_eq!(
            names.iter().filter(|n| **n == "webdev").count(),
            1,
            "webdev must not be duplicated"
        );
    }

    #[test]
    fn composite_search_official_description_wins() {
        let mut official = MockRegistry::new();
        official.add_pack(sample_metadata_named("webdev", "official"));

        let mut tap = MockRegistry::new();
        tap.add_pack(sample_metadata_named("webdev", "tap copy"));

        let composite = composite_from_mocks(vec![official, tap]);
        let results = composite.search("webdev").unwrap();

        assert_eq!(results.len(), 1);
        assert_eq!(results[0].description, "official");
    }

    #[test]
    fn composite_fetch_metadata_official_first() {
        let mut official = MockRegistry::new();
        official.add_pack(sample_metadata_named("webdev", "official"));

        let mut tap = MockRegistry::new();
        tap.add_pack(sample_metadata_named("webdev", "tap copy"));

        let composite = composite_from_mocks(vec![official, tap]);
        let meta = composite.fetch_metadata("webdev").unwrap();
        assert_eq!(meta.description, "official");
    }

    #[test]
    fn composite_fetch_metadata_falls_through_to_tap() {
        let official = MockRegistry::new(); // no packs

        let mut tap = MockRegistry::new();
        tap.add_pack(sample_metadata_named("tap-only", "from tap"));

        let composite = composite_from_mocks(vec![official, tap]);
        let meta = composite.fetch_metadata("tap-only").unwrap();
        assert_eq!(meta.description, "from tap");
    }

    #[test]
    fn composite_fetch_metadata_not_found() {
        let official = MockRegistry::new();
        let tap = MockRegistry::new();

        let composite = composite_from_mocks(vec![official, tap]);
        let err = composite.fetch_metadata("nonexistent");
        assert!(err.is_err());
    }

    #[test]
    fn composite_fetch_version_falls_through() {
        let official = MockRegistry::new();

        let mut tap = MockRegistry::new();
        tap.add_pack(sample_metadata_named("tap-pack", "from tap"));

        let composite = composite_from_mocks(vec![official, tap]);
        let release = composite
            .fetch_version("tap-pack", &semver::Version::new(1, 0, 0))
            .unwrap();
        assert_eq!(release.version, semver::Version::new(1, 0, 0));
    }

    // ── Error-handling path tests ────────────────────────────────────────

    #[test]
    fn composite_search_official_error_is_fatal() {
        let official = FailingRegistry::new("official down");
        let mut tap = MockRegistry::new();
        tap.add_pack(sample_metadata_named("tap-pack", "from tap"));

        let composite = CompositeRegistry::from_boxed(vec![Box::new(official), Box::new(tap)]);
        let err = composite.search("anything").unwrap_err();
        assert!(
            matches!(err, WeaveError::Registry(ref msg) if msg.contains("official down")),
            "official registry error should propagate: {err}"
        );
    }

    #[test]
    fn composite_search_tap_error_is_ignored() {
        let mut official = MockRegistry::new();
        official.add_pack(sample_metadata_named("webdev", "official"));

        let failing_tap = FailingRegistry::new("tap unreachable");

        let composite =
            CompositeRegistry::from_boxed(vec![Box::new(official), Box::new(failing_tap)]);
        // Search should succeed despite the tap error (warned but not fatal).
        // NOTE: We verify the operation succeeds despite tap failure but do not
        // currently assert that `log::warn!` was emitted. This is a known gap —
        // no log-capture test harness is wired up in this project yet.
        let results = composite.search("webdev").unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].name, "webdev");
    }

    #[test]
    fn composite_fetch_metadata_official_registry_error_is_fatal() {
        // A non-PackNotFound error from the official registry should be fatal.
        let official = FailingRegistry::new("connection refused");
        let mut tap = MockRegistry::new();
        tap.add_pack(sample_metadata_named("some-pack", "from tap"));

        let composite = CompositeRegistry::from_boxed(vec![Box::new(official), Box::new(tap)]);
        let err = composite.fetch_metadata("some-pack").unwrap_err();
        assert!(
            matches!(err, WeaveError::Registry(ref msg) if msg.contains("connection refused")),
            "official non-not-found error should propagate: {err}"
        );
    }

    #[test]
    fn composite_fetch_metadata_tap_registry_error_is_ignored() {
        // A non-PackNotFound error from a tap should be warned/ignored,
        // not propagated as fatal.
        let official = MockRegistry::new(); // pack not here
        let failing_tap = FailingRegistry::new("tap connection timeout");
        let mut good_tap = MockRegistry::new();
        good_tap.add_pack(sample_metadata_named("deep-pack", "from good tap"));

        let composite = CompositeRegistry::from_boxed(vec![
            Box::new(official),
            Box::new(failing_tap),
            Box::new(good_tap),
        ]);
        // Should skip the failing tap and find the pack in the good tap.
        // NOTE: We verify the operation succeeds despite tap failure but do not
        // currently assert that `log::warn!` was emitted. This is a known gap —
        // no log-capture test harness is wired up in this project yet.
        let meta = composite.fetch_metadata("deep-pack").unwrap();
        assert_eq!(meta.description, "from good tap");
    }

    #[test]
    fn composite_fetch_metadata_three_registries_skip_erroring_tap() {
        // 3-registry scenario: official (PackNotFound) -> tap1 (non-NotFound error,
        // should be warned/skipped) -> tap2 (has the pack, should succeed).
        let official = MockRegistry::new(); // returns PackNotFound
        let erroring_tap = FailingRegistry::new("tap1 connection reset");
        let mut good_tap = MockRegistry::new();
        good_tap.add_pack(sample_metadata_named("rare-pack", "from third registry"));

        let composite = CompositeRegistry::from_boxed(vec![
            Box::new(official),
            Box::new(erroring_tap),
            Box::new(good_tap),
        ]);

        // The erroring tap should be skipped (warned) and the pack found in tap2.
        let meta = composite.fetch_metadata("rare-pack").unwrap();
        assert_eq!(meta.name, "rare-pack");
        assert_eq!(meta.description, "from third registry");
    }

    #[test]
    fn composite_fetch_version_official_registry_error_is_fatal() {
        let official = FailingRegistry::new("server error");
        let mut tap = MockRegistry::new();
        tap.add_pack(sample_metadata_named("some-pack", "from tap"));

        let composite = CompositeRegistry::from_boxed(vec![Box::new(official), Box::new(tap)]);
        let err = composite
            .fetch_version("some-pack", &semver::Version::new(1, 0, 0))
            .unwrap_err();
        assert!(
            matches!(err, WeaveError::Registry(ref msg) if msg.contains("server error")),
            "official non-not-found error should propagate: {err}"
        );
    }

    #[test]
    fn composite_fetch_version_tap_registry_error_is_ignored() {
        let official = MockRegistry::new(); // pack not here
        let failing_tap = FailingRegistry::new("tap DNS failure");
        let mut good_tap = MockRegistry::new();
        good_tap.add_pack(sample_metadata_named("tap-pack", "from good tap"));

        let composite = CompositeRegistry::from_boxed(vec![
            Box::new(official),
            Box::new(failing_tap),
            Box::new(good_tap),
        ]);
        // NOTE: We verify the operation succeeds despite tap failure but do not
        // currently assert that `log::warn!` was emitted. This is a known gap —
        // no log-capture test harness is wired up in this project yet.
        let release = composite
            .fetch_version("tap-pack", &semver::Version::new(1, 0, 0))
            .unwrap();
        assert_eq!(release.version, semver::Version::new(1, 0, 0));
    }

    #[test]
    fn composite_publish_delegates_to_first_registry() {
        let official = MockRegistry::new();
        let composite = CompositeRegistry::from_boxed(vec![Box::new(official)]);
        // publish is not yet supported on any registry, so we just verify
        // it delegates to the first registry and returns its error.
        let err = composite
            .publish(std::path::Path::new("/fake"), "token")
            .unwrap_err();
        assert!(
            matches!(err, WeaveError::Registry(ref msg) if msg.contains("publish is not yet supported")),
            "publish should return Registry error: {err}"
        );
    }

    // This test exercises a defensive branch in `publish()` that is unreachable
    // through the production constructor (`CompositeRegistry::new`), which always
    // includes at least the official registry. After the `from_boxed()` precondition
    // was added, constructing an empty composite panics, so this test validates that
    // the precondition fires correctly.
    #[test]
    #[should_panic(expected = "at least one registry (official) is required")]
    fn composite_publish_no_registries() {
        let _composite = CompositeRegistry::from_boxed(vec![]);
    }

    #[test]
    fn latest_version_no_releases() {
        let meta = PackMetadata {
            name: "empty".into(),
            description: "no releases".into(),
            authors: vec![],
            license: None,
            repository: None,
            keywords: vec![],
            versions: vec![],
        };
        assert!(meta.latest_version().is_err());
    }

    // ── Host allowlist tests ─────────────────────────────────────────────

    /// Helper to extract the host from a URL the same way http_get_json does.
    fn extract_trusted_host(url: &str) -> &str {
        let host_with_port = url
            .split("://")
            .nth(1)
            .and_then(|rest| rest.split('/').next())
            .unwrap_or("");
        host_with_port.split(':').next().unwrap_or(host_with_port)
    }

    fn is_trusted(url: &str) -> bool {
        let host = extract_trusted_host(url);
        const TRUSTED_HOSTS: [&str; 2] = ["api.github.com", "raw.githubusercontent.com"];
        TRUSTED_HOSTS.iter().any(|h| host.eq_ignore_ascii_case(h))
    }

    #[test]
    fn trusted_host_matches_github_raw() {
        assert!(is_trusted(
            "https://raw.githubusercontent.com/PackWeave/registry/main/index.json"
        ));
    }

    #[test]
    fn trusted_host_matches_github_api() {
        assert!(is_trusted("https://api.github.com/user"));
    }

    #[test]
    fn trusted_host_matches_with_port() {
        assert!(is_trusted(
            "https://raw.githubusercontent.com:443/PackWeave/registry/main/index.json"
        ));
    }

    #[test]
    fn trusted_host_rejects_localhost() {
        assert!(!is_trusted("http://127.0.0.1:8080/index.json"));
    }

    #[test]
    fn trusted_host_rejects_evil_subdomain() {
        assert!(!is_trusted(
            "https://raw.githubusercontent.com.evil.com/index.json"
        ));
    }

    #[test]
    fn trusted_host_rejects_non_github() {
        assert!(!is_trusted("https://my-registry.example.com/index.json"));
    }

    #[test]
    fn trusted_host_case_insensitive() {
        assert!(is_trusted(
            "https://RAW.GITHUBUSERCONTENT.COM/PackWeave/registry/main/index.json"
        ));
    }
}
