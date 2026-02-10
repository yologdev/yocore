# @yologdev/core

[![GitHub Release](https://img.shields.io/github/v/release/yologdev/yocore)](https://github.com/yologdev/yocore/releases)
[![npm](https://img.shields.io/npm/v/@yologdev/core)](https://www.npmjs.com/package/@yologdev/core)
[![License: MIT](https://img.shields.io/badge/License-MIT-blue.svg)](https://github.com/yologdev/yocore/blob/main/LICENSE)

Headless service for watching, parsing, storing, and serving AI coding sessions.

Yocore is the core engine behind [Yolog](https://github.com/yologdev/yolog) — a platform that archives and visualizes AI pair programming sessions from Claude Code, Cursor, and other AI coding assistants.

## Install

```bash
npm install -g @yologdev/core
```

Pre-built binaries are included for:

| Platform | Architecture |
|----------|-------------|
| macOS | Apple Silicon (ARM64) |
| macOS | Intel (x64) |
| Linux | x64 |
| Windows | x64 |

## Quick Start

```bash
# Initialize default config
yocore --init

# Start the HTTP server (default: 127.0.0.1:19420)
yocore

# Start in MCP mode (for Claude Code integration)
yocore --mcp

# Custom port
yocore --port 8080
```

## Features

- **Session Watching** — Automatically watches folders for new AI coding sessions
- **Multi-Parser Support** — Parses Claude Code, OpenClaw, and other AI assistant formats
- **Full-Text Search** — SQLite FTS5-powered search across all sessions and memories
- **Memory System** — Extract and organize decisions, facts, preferences, and tasks
- **HTTP API** — RESTful API for session replay, search, and memory management
- **MCP Server** — Model Context Protocol integration for AI assistants
- **LAN Discovery** — Automatic instance discovery via mDNS/Bonjour
- **Lifeboat Pattern** — Session context preservation across context compaction

## Configuration

Config file at `~/.yolog/config.toml`:

```toml
[server]
port = 19420
host = "127.0.0.1"
# api_key = "optional-secret"
# instance_name = "My Workstation"

[database]
path = "~/.local/share/yocore/yocore.db"

[[watch]]
path = "~/.claude/projects"
parser = "claude_code"

[ai]
enabled = false
```

## MCP Tools

When running in MCP mode (`yocore --mcp`):

| Tool | Description |
|------|-------------|
| `yolog_search_memories` | Hybrid keyword + semantic search |
| `yolog_get_project_context` | Get project overview with categorized memories |
| `yolog_get_memories_by_type` | Filter memories by type |
| `yolog_get_memories_by_tag` | Filter memories by tag |
| `yolog_get_recent_memories` | Get memories from recent sessions |
| `yolog_get_session_context` | Get session state with lifeboat pattern |
| `yolog_save_lifeboat` | Save session state before context compaction |

## Links

- [GitHub](https://github.com/yologdev/yocore) — Source code, issues, and full documentation
- [Yolog Desktop](https://github.com/yologdev/yolog) — GUI companion app
- [yoskill](https://github.com/yologdev/yoskill) — Claude Code skill pack
- [Changelog](https://github.com/yologdev/yocore/blob/main/CHANGELOG.md)

## License

MIT
