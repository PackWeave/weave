use crate::core::pack::Pack;

/// A tool-name conflict between an incoming pack's server and an already-installed
/// pack's server.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ToolConflict {
    pub tool_name: String,
    pub incoming_server: String,
    pub incoming_pack: String,
    pub installed_server: String,
    pub installed_pack: String,
}

/// Check for tool-name conflicts between an incoming pack and already-installed packs.
///
/// A conflict occurs when:
/// - The incoming pack has a server S1 exporting tool T
/// - An installed pack has a server S2 also exporting tool T
/// - S1 != S2 (different servers with overlapping tool names)
///
/// Empty `tools` lists mean "tools unknown" and never produce conflicts.
/// Same-server-name conflicts are handled by adapters, not here.
pub fn check_tool_conflicts(incoming: &Pack, installed: &[Pack]) -> Vec<ToolConflict> {
    let mut conflicts = Vec::new();

    for incoming_server in &incoming.servers {
        // Empty tools list means "tools unknown" — skip.
        if incoming_server.tools.is_empty() {
            continue;
        }

        for installed_pack in installed {
            // Skip the same pack — when upgrading, the old version is still in
            // the installed list and would produce false "self-conflict" warnings.
            if installed_pack.name == incoming.name {
                continue;
            }

            // TODO: filter by PackTargets overlap — packs targeting disjoint CLIs
            // cannot actually conflict at runtime. This requires adding an
            // `overlaps()` method to PackTargets.

            for installed_server in &installed_pack.servers {
                // Empty tools list on the installed side — skip.
                if installed_server.tools.is_empty() {
                    continue;
                }

                // Same server name is an adapter-level conflict, not a tool conflict.
                if incoming_server.name == installed_server.name {
                    continue;
                }

                for tool in &incoming_server.tools {
                    if installed_server.tools.contains(tool) {
                        conflicts.push(ToolConflict {
                            tool_name: tool.clone(),
                            incoming_server: incoming_server.name.clone(),
                            incoming_pack: incoming.name.clone(),
                            installed_server: installed_server.name.clone(),
                            installed_pack: installed_pack.name.clone(),
                        });
                    }
                }
            }
        }
    }

    conflicts
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::pack::{McpServer, Pack};
    use std::collections::HashMap;

    /// Helper to build a minimal valid Pack for testing.
    fn make_pack(name: &str, servers: Vec<McpServer>) -> Pack {
        Pack {
            name: name.to_string(),
            version: semver::Version::new(1, 0, 0),
            description: "test pack".to_string(),
            authors: vec!["tester".to_string()],
            license: None,
            repository: None,
            keywords: vec![],
            min_tool_version: None,
            servers,
            dependencies: HashMap::new(),
            extensions: Default::default(),
            targets: Default::default(),
        }
    }

    /// Helper to build an McpServer with a name, tools list, and stdio command.
    fn make_server(name: &str, tools: Vec<&str>) -> McpServer {
        McpServer {
            name: name.to_string(),
            package_type: None,
            package: None,
            command: Some("test-cmd".to_string()),
            args: vec![],
            url: None,
            headers: None,
            transport: None,
            tools: tools.into_iter().map(String::from).collect(),
            env: HashMap::new(),
        }
    }

    #[test]
    fn no_conflict_when_no_installed_packs() {
        let incoming = make_pack(
            "new-pack",
            vec![make_server("s1", vec!["tool-a", "tool-b"])],
        );
        let installed: Vec<Pack> = vec![];

        let conflicts = check_tool_conflicts(&incoming, &installed);
        assert!(conflicts.is_empty());
    }

    #[test]
    fn no_conflict_when_tools_do_not_overlap() {
        let incoming = make_pack("new-pack", vec![make_server("s1", vec!["tool-a"])]);
        let installed = vec![make_pack(
            "existing-pack",
            vec![make_server("s2", vec!["tool-b"])],
        )];

        let conflicts = check_tool_conflicts(&incoming, &installed);
        assert!(conflicts.is_empty());
    }

    #[test]
    fn detects_tool_conflict_between_different_servers() {
        let incoming = make_pack(
            "devtools",
            vec![make_server(
                "playwright",
                vec!["browser_navigate", "screenshot"],
            )],
        );
        let installed = vec![make_pack(
            "webdev",
            vec![make_server("puppeteer", vec!["browser_navigate", "click"])],
        )];

        let conflicts = check_tool_conflicts(&incoming, &installed);
        assert_eq!(conflicts.len(), 1);
        assert_eq!(conflicts[0].tool_name, "browser_navigate");
        assert_eq!(conflicts[0].incoming_server, "playwright");
        assert_eq!(conflicts[0].incoming_pack, "devtools");
        assert_eq!(conflicts[0].installed_server, "puppeteer");
        assert_eq!(conflicts[0].installed_pack, "webdev");
    }

    #[test]
    fn empty_tools_list_produces_no_conflict() {
        // Incoming has empty tools — "tools unknown"
        let incoming = make_pack("new-pack", vec![make_server("s1", vec![])]);
        let installed = vec![make_pack(
            "existing-pack",
            vec![make_server("s2", vec!["tool-a"])],
        )];

        let conflicts = check_tool_conflicts(&incoming, &installed);
        assert!(conflicts.is_empty());

        // Installed has empty tools — "tools unknown"
        let incoming2 = make_pack("new-pack", vec![make_server("s1", vec!["tool-a"])]);
        let installed2 = vec![make_pack("existing-pack", vec![make_server("s2", vec![])])];

        let conflicts2 = check_tool_conflicts(&incoming2, &installed2);
        assert!(conflicts2.is_empty());
    }

    #[test]
    fn same_server_name_different_packs_no_tool_conflict() {
        // Two packs with the same server name — that's an adapter-level conflict,
        // not a tool conflict. The tool conflict checker should skip it.
        let incoming = make_pack(
            "new-pack",
            vec![make_server("shared-server", vec!["tool-a"])],
        );
        let installed = vec![make_pack(
            "existing-pack",
            vec![make_server("shared-server", vec!["tool-a"])],
        )];

        let conflicts = check_tool_conflicts(&incoming, &installed);
        assert!(conflicts.is_empty());
    }

    #[test]
    fn multiple_conflicts_across_multiple_packs() {
        let incoming = make_pack(
            "new-pack",
            vec![make_server("s1", vec!["tool-a", "tool-b", "tool-c"])],
        );
        let installed = vec![
            make_pack("pack-a", vec![make_server("s2", vec!["tool-a"])]),
            make_pack("pack-b", vec![make_server("s3", vec!["tool-b", "tool-c"])]),
        ];

        let conflicts = check_tool_conflicts(&incoming, &installed);
        assert_eq!(conflicts.len(), 3);

        let tool_names: Vec<&str> = conflicts.iter().map(|c| c.tool_name.as_str()).collect();
        assert!(tool_names.contains(&"tool-a"));
        assert!(tool_names.contains(&"tool-b"));
        assert!(tool_names.contains(&"tool-c"));
    }

    #[test]
    fn both_sides_empty_tools_no_conflict() {
        let incoming = make_pack("new-pack", vec![make_server("s1", vec![])]);
        let installed = vec![make_pack("existing-pack", vec![make_server("s2", vec![])])];

        let conflicts = check_tool_conflicts(&incoming, &installed);
        assert!(conflicts.is_empty());
    }

    #[test]
    fn same_pack_name_skips_self_conflict() {
        // When upgrading a pack, the old version is still in the installed list.
        // The checker must skip it to avoid false "self-conflict" warnings.
        let incoming = make_pack("my-pack", vec![make_server("s1", vec!["tool-a", "tool-b"])]);
        let installed = vec![make_pack(
            "my-pack",
            vec![make_server("s2", vec!["tool-a", "tool-b"])],
        )];

        let conflicts = check_tool_conflicts(&incoming, &installed);
        assert!(
            conflicts.is_empty(),
            "expected no self-conflicts, got {conflicts:?}"
        );
    }
}
