# Packs

This document defines the canonical pack shape and quality bar for community contributions.

-----

## Minimal example

```toml
[pack]
name = "webdev"
version = "0.1.0"
description = "Web development essentials"
authors = ["yourname"]
license = "MIT"
repository = "https://github.com/yourname/webdev-pack"
keywords = ["web", "browser", "git"]
min_tool_version = "0.1.0"

[[servers]]
name = "filesystem"
command = "npx"
args = ["-y", "@modelcontextprotocol/server-filesystem"]
transport = "stdio"
tools = ["read_file", "write_file", "list_directory"]

[servers.env.FILESYSTEM_ROOT]
required = true
secret = false
description = "Root directory for the filesystem server"
```

### HTTP transport (remote servers)

```toml
[[servers]]
name = "remote-api"
transport = "http"
url = "https://api.example.com/mcp"

[servers.headers]
Authorization = "Bearer ${API_KEY}"
```

### Dependencies

```toml
[dependencies]
core-tools = "^1.0"
git-pack = ">=2.0, <3.0"
```

-----

## File layout

```
pack/
  pack.toml
  commands/
    review.md
  prompts/
    claude.md
    gemini.md
    codex.md
  settings/
    claude.json
    gemini.json
    codex.json
```

-----

## Quality bar for community packs

- Must include a clear description and keywords.
- Must not include secrets or credential values.
- Should declare `tools` lists when possible to enable conflict detection.
- Should include CLI-specific prompts if the guidance differs by CLI.

-----

## Extensions

`extensions.<cli>` blocks are optional and pass-through. Adapters ignore unknown keys so packs remain forward compatible.

Hooks are declared under `extensions.<cli>.hooks` and require the user to pass `--allow-hooks` when installing. Only Claude Code supports hooks currently.

```toml
[extensions.claude_code.hooks]
PreToolUse = [{ matcher = "Bash", command = "echo pre-check" }]
```
