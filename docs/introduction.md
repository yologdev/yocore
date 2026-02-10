# Yocore

Headless service for watching, parsing, storing, and serving AI coding sessions.

Yocore is the core engine behind [Yolog](https://github.com/yologdev/yolog) — a platform that archives and visualizes AI pair programming sessions from Claude Code, Cursor, and other AI coding assistants.

## Features

- **Session Watching** — Automatically watches folders for new AI coding sessions
- **Multi-Parser Support** — Parses Claude Code, OpenClaw, and other AI assistant formats
- **Full-Text Search** — SQLite FTS5-powered search across all sessions and memories
- **Memory System** — Extract and organize decisions, facts, preferences, and tasks
- **HTTP API** — RESTful API with 50+ endpoints for sessions, projects, memories, skills, search, and config
- **MCP Server** — Model Context Protocol integration for AI assistants
- **LAN Discovery** — Automatic instance discovery via mDNS/Bonjour on the local network
- **Lifeboat Pattern** — Session context preservation across context compaction

## Ecosystem

| Component | Description |
|-----------|-------------|
| **yocore** (this) | Headless service — watches files, stores data, serves API |
| [Yolog Desktop](https://github.com/yologdev/yolog) | GUI companion app — session replay, memory browser, dashboard |
| [yoskill](https://github.com/yologdev/yoskill) | Claude Code skill pack — slash commands for memory system |

## Quick Links

- [Installation](getting-started/installation.md) — Get yocore running in 2 minutes
- [Configuration](reference/configuration.md) — All config options and defaults
- [HTTP API Reference](reference/http-api.md) — Complete REST endpoint documentation
- [Long-Term Memory](getting-started/long-term-memory.md) — AI-powered memory with yoskill
- [Memory System Deep Dive](guides/memory-system.md) — How ranking, search, and dedup work
- [Architecture](architecture/overview.md) — How yocore works internally
