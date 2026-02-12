# Yocore

Headless service for watching, parsing, storing, and serving AI coding sessions.

Yocore is the core engine behind [Yolog](https://github.com/yologdev/support) — a platform that archives and visualizes AI pair programming sessions from Claude Code (more parsers coming soon).

## Features

- **Session Watching** — Automatically watches folders for new AI coding sessions
- **Multi-Parser Support** — Parses Claude Code, OpenClaw, and other AI assistant formats
- **Ephemeral Mode** — Lightweight in-memory storage with no database overhead
- **Full-Text Search** — SQLite FTS5-powered search across all sessions and memories
- **Yo Memory** — Extract and organize decisions, facts, preferences, and tasks
- **HTTP API** — RESTful API with 50+ endpoints for sessions, projects, memories, skills, search, and config
- **MCP Server** — Model Context Protocol integration for AI assistants
- **LAN Discovery** — Automatic instance discovery via mDNS/Bonjour on the local network
- **Lifeboat Pattern** — Session context preservation across context compaction

## Ecosystem

| Component | Description |
|-----------|-------------|
| **yocore** (this) | Headless service — watches files, stores data, serves API |
| [yolog](https://github.com/yologdev/support) | GUI companion app — session replay, memory browser, dashboard |
| [yoskill](https://github.com/yologdev/yoskill) | Claude Code skill pack — slash commands for memory system |

## Quick Links

- [Installation](getting-started/installation.md) — Get yocore running in 2 minutes
- [Configuration](reference/configuration.md) — All config options and defaults
- [HTTP API Reference](reference/http-api.md) — Complete REST endpoint documentation
- [Yo Memory](getting-started/yo-memory.md) — AI-powered memory with yoskill
- [Yo Memory System](guides/memory-system.md) — How ranking, search, and dedup work
- [Architecture](architecture/overview.md) — How yocore works internally
