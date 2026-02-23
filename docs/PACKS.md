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
namespace = "fs"

[servers.env.FILESYSTEM_ROOT]
required = true
secret = false
description = "Root directory for the filesystem server"
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
- Should define namespaces if tools may conflict.
- Should declare tool lists when possible to enable conflict checks.
- Should include CLI-specific prompts if the guidance differs by CLI.

-----

## Extensions

`extensions.<cli>` blocks are optional and pass-through. Adapters ignore unknown keys so packs remain forward compatible. Hooks under `extensions.<cli>.hooks` are deferred until v0.3 and will require explicit opt-in.
