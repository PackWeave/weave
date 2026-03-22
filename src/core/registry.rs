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
    cached_search_index: std::sync::Mutex<Option<SearchIndex>>,
    cached_packs: std::sync::Mutex<HashMap<String, PackMetadata>>,
}

impl GitHubRegistry {
    pub fn new(base_url: &str) -> Self {
        // Strip trailing slash and also normalise old-style URLs that already
        // include the `/index.json` suffix (e.g. configs written before the
        // sparse-index migration).  Without this, old installs would request
        // `.../index.json/index.json` and break silently.
        let base_url = base_url
            .trim_end_matches('/')
            .trim_end_matches("/index.json");
        Self {
            base_url: base_url.to_string(),
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
        let index: SearchIndex = http_get_json(&url, "registry search index")?;

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
        let meta: PackMetadata = http_get_json(&url, &format!("pack metadata for '{name}'"))
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
fn http_get_json<T: serde::de::DeserializeOwned>(url: &str, label: &str) -> Result<T> {
    let client = reqwest::blocking::Client::new();
    let response = client
        .get(url)
        .header("User-Agent", format!("weave/{}", env!("CARGO_PKG_VERSION")))
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

/// A mock registry for testing. No network calls.
#[cfg(test)]
pub struct MockRegistry {
    pub packs: HashMap<String, PackMetadata>,
}

#[cfg(test)]
impl MockRegistry {
    pub fn new() -> Self {
        Self {
            packs: HashMap::new(),
        }
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
        let r = GitHubRegistry::new("https://example.com/registry/");
        assert_eq!(r.base_url, "https://example.com/registry");
    }

    #[test]
    fn new_strips_old_index_json_suffix() {
        // Old configs stored the full URL including /index.json.
        let r = GitHubRegistry::new("https://example.com/registry/index.json");
        assert_eq!(r.base_url, "https://example.com/registry");
    }

    #[test]
    fn new_strips_index_json_with_trailing_slash() {
        let r = GitHubRegistry::new("https://example.com/registry/index.json/");
        assert_eq!(r.base_url, "https://example.com/registry");
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
}
