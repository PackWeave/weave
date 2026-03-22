use serde::Deserialize;

use crate::error::WeaveError;

/// Response envelope from the MCP Registry search API.
#[derive(Debug, Deserialize)]
pub struct McpRegistryResponse {
    pub servers: Vec<McpRegistryEntry>,
    #[serde(default)]
    pub metadata: McpRegistryMetadata,
}

/// A single entry in the search results.
#[derive(Debug, Deserialize)]
pub struct McpRegistryEntry {
    pub server: McpRegistryServer,
}

/// An MCP server record returned by the registry.
#[derive(Debug, Deserialize)]
pub struct McpRegistryServer {
    pub name: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub version: String,
    pub title: Option<String>,
    pub repository: Option<McpRegistryRepo>,
    #[serde(default)]
    pub packages: Vec<McpRegistryPackage>,
}

/// Repository metadata for an MCP server.
#[derive(Debug, Deserialize)]
pub struct McpRegistryRepo {
    #[serde(default)]
    pub url: Option<String>,
    #[serde(default)]
    pub source: Option<String>,
}

/// Package distribution information for an MCP server.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct McpRegistryPackage {
    pub registry_type: String,
    pub identifier: String,
    #[serde(default)]
    pub version: Option<String>,
}

/// Pagination metadata from the registry.
#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct McpRegistryMetadata {
    pub next_cursor: Option<String>,
    pub count: Option<u32>,
}

/// Client for the official MCP Registry at registry.modelcontextprotocol.io.
pub struct McpRegistryClient {
    base_url: String,
}

impl Default for McpRegistryClient {
    fn default() -> Self {
        Self::new()
    }
}

impl McpRegistryClient {
    pub fn new() -> Self {
        Self {
            base_url: "https://registry.modelcontextprotocol.io".to_string(),
        }
    }

    #[cfg(test)]
    pub fn with_base_url(base_url: &str) -> Self {
        Self {
            base_url: base_url.to_string(),
        }
    }

    /// Search the MCP Registry for servers matching the given query.
    pub fn search(&self, query: &str) -> crate::error::Result<Vec<McpRegistryServer>> {
        let url = format!("{}/v0.1/servers", self.base_url);
        let client = reqwest::blocking::Client::new();
        let resp = client
            .get(&url)
            .query(&[("search", query), ("version", "latest"), ("limit", "20")])
            .header("User-Agent", format!("weave/{}", env!("CARGO_PKG_VERSION")))
            .send()
            .map_err(|e| {
                WeaveError::McpRegistry(format!(
                    "request failed: {e} — check your network connection"
                ))
            })?;

        if !resp.status().is_success() {
            return Err(WeaveError::McpRegistry(format!(
                "HTTP {} from MCP Registry — this may be temporary, try again in a moment",
                resp.status()
            )));
        }

        let body: McpRegistryResponse = resp.json().map_err(|e| {
            WeaveError::McpRegistry(format!(
                "failed to parse response: {e} — try again later or report this issue"
            ))
        })?;

        Ok(body.servers.into_iter().map(|e| e.server).collect())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn deserialize_full_response() {
        let json = r#"{
            "servers": [
                {
                    "server": {
                        "name": "org/my-server",
                        "description": "A great MCP server",
                        "version": "1.0.0",
                        "title": "My Server",
                        "repository": {
                            "url": "https://github.com/org/my-server",
                            "source": "github"
                        },
                        "packages": [
                            {
                                "registryType": "npm",
                                "identifier": "@org/my-server",
                                "version": "1.0.0"
                            }
                        ]
                    }
                }
            ],
            "metadata": {
                "nextCursor": "abc123",
                "count": 1
            }
        }"#;

        let resp: McpRegistryResponse = serde_json::from_str(json).unwrap();
        assert_eq!(resp.servers.len(), 1);

        let server = &resp.servers[0].server;
        assert_eq!(server.name, "org/my-server");
        assert_eq!(server.description, "A great MCP server");
        assert_eq!(server.title.as_deref(), Some("My Server"));
        assert_eq!(server.version, "1.0.0");

        let repo = server.repository.as_ref().unwrap();
        assert_eq!(
            repo.url.as_deref(),
            Some("https://github.com/org/my-server")
        );
        assert_eq!(repo.source.as_deref(), Some("github"));

        assert_eq!(server.packages.len(), 1);
        assert_eq!(server.packages[0].registry_type, "npm");
        assert_eq!(server.packages[0].identifier, "@org/my-server");
        assert_eq!(server.packages[0].version.as_deref(), Some("1.0.0"));

        assert_eq!(resp.metadata.next_cursor.as_deref(), Some("abc123"));
        assert_eq!(resp.metadata.count, Some(1));
    }

    #[test]
    fn deserialize_real_api_response() {
        // Captured from https://registry.modelcontextprotocol.io/v0.1/servers?search=filesystem&version=latest&limit=20
        // This test catches schema drift between the live API and our structs.
        let json =
            std::fs::read_to_string("tests/fixtures_mcp_resp.json").expect("fixture missing");
        let result: Result<McpRegistryResponse, _> = serde_json::from_str(&json);
        match &result {
            Err(e) => panic!("Failed to parse real MCP registry response: {e}"),
            Ok(r) => assert!(!r.servers.is_empty(), "expected at least one server"),
        }
    }

    #[test]
    fn deserialize_minimal_response() {
        let json = r#"{
            "servers": [
                {
                    "server": {
                        "name": "simple-server"
                    }
                }
            ]
        }"#;

        let resp: McpRegistryResponse = serde_json::from_str(json).unwrap();
        assert_eq!(resp.servers.len(), 1);

        let server = &resp.servers[0].server;
        assert_eq!(server.name, "simple-server");
        assert_eq!(server.description, "");
        assert_eq!(server.version, "");
        assert!(server.title.is_none());
        assert!(server.repository.is_none());
        assert!(server.packages.is_empty());
        assert!(resp.metadata.next_cursor.is_none());
        assert!(resp.metadata.count.is_none());
    }
}
