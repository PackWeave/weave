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
    pub versions: Vec<PackRelease>,
}

/// A specific release of a pack.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PackRelease {
    pub version: semver::Version,
    pub url: String,
    pub sha256: String,
    #[serde(default)]
    pub size_bytes: Option<u64>,
}

/// The registry trait. All registry implementations must be Send + Sync.
pub trait Registry: Send + Sync {
    /// Search for packs matching a query string.
    fn search(&self, query: &str) -> Result<Vec<PackSummary>>;

    /// Fetch full metadata for a pack by name.
    fn fetch_metadata(&self, name: &str) -> Result<PackMetadata>;

    /// Fetch a specific version of a pack.
    fn fetch_version(&self, name: &str, version: &semver::Version) -> Result<PackRelease>;
}

/// The registry index format: a JSON file mapping pack names to their metadata.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RegistryIndex {
    #[serde(flatten)]
    pub packs: HashMap<String, PackMetadata>,
}

/// GitHub-backed registry implementation.
/// Reads a JSON index from the PackWeave/registry GitHub repo.
pub struct GitHubRegistry {
    index_url: String,
    cached_index: std::sync::Mutex<Option<RegistryIndex>>,
}

impl GitHubRegistry {
    pub fn new(index_url: &str) -> Self {
        Self {
            index_url: index_url.to_string(),
            cached_index: std::sync::Mutex::new(None),
        }
    }

    fn load_index(&self) -> Result<RegistryIndex> {
        // Check cache first
        {
            let cache = self.cached_index.lock().expect("mutex not poisoned");
            if let Some(ref index) = *cache {
                return Ok(index.clone());
            }
        }

        let response = reqwest::blocking::get(&self.index_url)
            .map_err(|e| WeaveError::Registry(format!("failed to fetch registry index: {e}")))?;

        if !response.status().is_success() {
            return Err(WeaveError::Registry(format!(
                "registry returned HTTP {}",
                response.status()
            )));
        }

        let index: RegistryIndex = response
            .json()
            .map_err(|e| WeaveError::Registry(format!("failed to parse registry index: {e}")))?;

        // Cache the index
        {
            let mut cache = self.cached_index.lock().expect("mutex not poisoned");
            *cache = Some(index.clone());
        }

        Ok(index)
    }
}

impl Registry for GitHubRegistry {
    fn search(&self, query: &str) -> Result<Vec<PackSummary>> {
        let index = self.load_index()?;
        let query_lower = query.to_lowercase();

        let mut results: Vec<PackSummary> = index
            .packs
            .iter()
            .filter(|(name, meta)| {
                name.contains(&query_lower)
                    || meta.description.to_lowercase().contains(&query_lower)
                    || meta
                        .keywords()
                        .iter()
                        .any(|k| k.to_lowercase().contains(&query_lower))
            })
            .map(|(name, meta)| PackSummary {
                name: name.clone(),
                description: meta.description.clone(),
                latest_version: meta.latest_version(),
                keywords: meta.keywords(),
            })
            .collect();

        results.sort_by(|a, b| a.name.cmp(&b.name));
        Ok(results)
    }

    fn fetch_metadata(&self, name: &str) -> Result<PackMetadata> {
        let index = self.load_index()?;
        index
            .packs
            .get(name)
            .cloned()
            .ok_or_else(|| WeaveError::PackNotFound {
                name: name.to_string(),
            })
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
    /// Get the latest (highest) version.
    pub fn latest_version(&self) -> semver::Version {
        self.versions
            .iter()
            .map(|v| &v.version)
            .max()
            .cloned()
            .unwrap_or_else(|| semver::Version::new(0, 0, 0))
    }

    /// Get keywords from the metadata (not stored at top level in index, extracted from description for now).
    pub fn keywords(&self) -> Vec<String> {
        // In v1, keywords come from the pack manifests stored in the index
        Vec::new()
    }
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
            .iter()
            .filter(|(name, _)| name.contains(&query_lower))
            .map(|(name, meta)| PackSummary {
                name: name.clone(),
                description: meta.description.clone(),
                latest_version: meta.latest_version(),
                keywords: Vec::new(),
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
            versions: vec![
                PackRelease {
                    version: semver::Version::new(1, 0, 0),
                    url: "https://example.com/webdev-1.0.0.tar.gz".into(),
                    sha256: "abc123".into(),
                    size_bytes: Some(1024),
                },
                PackRelease {
                    version: semver::Version::new(1, 1, 0),
                    url: "https://example.com/webdev-1.1.0.tar.gz".into(),
                    sha256: "def456".into(),
                    size_bytes: Some(2048),
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
        assert_eq!(release.sha256, "abc123");

        let err = registry.fetch_version("webdev", &semver::Version::new(9, 9, 9));
        assert!(err.is_err());
    }

    #[test]
    fn latest_version() {
        let meta = sample_metadata();
        assert_eq!(meta.latest_version(), semver::Version::new(1, 1, 0));
    }
}
