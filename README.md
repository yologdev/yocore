<div align="center">

<picture>
  <img alt="Yocore" src="docs/images/banner.svg" width="100%" height="auto">
</picture>

<a href="https://yologdev.github.io/yocore/">Docs</a> · <a href="https://github.com/yologdev/yocore">GitHub</a> · <a href="https://github.com/yologdev/yocore/issues">Issues</a> · <a href="https://github.com/yologdev/yocore/releases">Releases</a>

[![][ci-shield]][ci-link]
[![][release-shield]][release-link]
[![][npm-shield]][npm-link]
[![][license-shield]][license-link]
[![][docs-shield]][docs-link]
[![][last-commit-shield]][last-commit-link]

<a href="https://github.com/yologdev/yolog">yolog</a> · <a href="https://github.com/yologdev/yoskill">yoskill</a>

</div>

---

## Overview

Yocore watches AI coding session files (Claude Code, OpenClaw), parses them incrementally, stores structured data in SQLite or in-memory, and exposes everything via REST API and MCP server. It powers [yolog](https://github.com/yologdev/yolog) — a desktop app for replaying, searching, and analyzing AI pair programming sessions.

```
Session Files (JSONL)
  → Watch & Parse (incremental, multi-parser)
  → Store (SQLite with FTS5 + embeddings | In-memory ephemeral)
  → AI Features (title, memory, skills, markers, export)
  → Serve (HTTP API · MCP · SSE events · mDNS discovery)
  → Desktop App / AI Assistants
```

## Features

**Watch & Parse**
- File system watcher with incremental parsing — only new bytes are processed
- Multi-parser support: Claude Code, OpenClaw, extensible trait-based design
- Each file event spawns an independent task to prevent starvation

**Store & Search**
- **DB mode**: SQLite with WAL, dual read/write connections, FTS5 full-text search
- **Ephemeral mode**: In-memory storage with LRU eviction, no database overhead
- Semantic embeddings via local all-MiniLM-L6-v2 (384-dim), hybrid keyword + vector search

**AI Features** (Claude Code CLI)
- Title generation — auto-name sessions from conversation content
- Memory extraction — decisions, facts, preferences, context, tasks
- Skills discovery — reusable workflow patterns and techniques
- Marker detection — breakthroughs, ship moments, bugs, key decisions
- Export generation — technical summaries and highlight reels with chunked processing

**Serve & Connect**
- HTTP API (~57 endpoints) for session replay, search, memory management
- MCP server (5 tools) for AI assistant integration
- SSE real-time events for file changes and AI task progress
- mDNS/Bonjour LAN discovery with custom instance names

---

## Quick Start

### 1. Install

```bash
npm install -g @yologdev/core
```

<details>
<summary>Other install methods</summary>

**Binary download:**

Download from [GitHub Releases](https://github.com/yologdev/yocore/releases).

**Build from source:**

```bash
git clone https://github.com/yologdev/yocore.git
cd yocore
cargo build --release
```

</details>

### 2. Initialize

```bash
yocore --init
```

Creates a default config at `~/.yolog/config.toml`.

### 3. Start

```bash
yocore
```

The server starts on `http://127.0.0.1:19420`. Verify with:

```bash
curl http://localhost:19420/health
```

---

## Configuration

<details>
<summary>Full config reference (~/.yolog/config.toml)</summary>

```toml
# Storage: "db" (default, persistent) or "ephemeral" (in-memory, volatile)
storage = "db"

[server]
port = 19420
host = "127.0.0.1"
# api_key = "optional-secret"       # Required for remote access
# mdns_enabled = true               # LAN discovery (default: true)
# instance_name = "My Workstation"  # Custom display name

# Watch multiple paths with different parsers
[[watch]]
path = "~/.claude/projects"
parser = "claude_code"

# [[watch]]
# path = "~/.openclaw/workspace"
# parser = "openclaw"

[ai]
provider = "claude_code"      # Requires Claude Code CLI installed
title_generation = true
memory_extraction = true
skills_discovery = true
marker_detection = true

# Ephemeral mode limits (only when storage = "ephemeral")
# [ephemeral]
# max_sessions = 100
# max_messages_per_session = 50

# Background scheduler (DB mode only)
# [scheduler.ranking]
# interval_hours = 6
# [scheduler.duplicate_cleanup]
# interval_hours = 24
# similarity_threshold = 0.75
# [scheduler.embedding_refresh]
# interval_hours = 12
# [scheduler.skill_cleanup]
# interval_hours = 24
# similarity_threshold = 0.80
```

**Environment variables:** `YOLOG_DATA_DIR`, `YOLOG_SERVER_PORT`, `YOLOG_SERVER_HOST`, `YOLOG_SERVER_API_KEY`

</details>

---

## Core Concepts

### Dual Storage

Yocore supports two storage backends, selected at startup via `storage = "db"` or `"ephemeral"`:

```
DB Mode (default)                    Ephemeral Mode
├── SQLite + WAL                     ├── In-memory HashMap
├── FTS5 full-text search            ├── LRU session eviction
├── Semantic embeddings              ├── Message windowing (last N)
├── All AI features                  ├── Title generation only
└── Background scheduler             └── No persistence
```

### Incremental Parsing

Sessions are parsed incrementally — yocore tracks `file_size` and `max_sequence` per session. When a file grows, only new bytes are parsed. On truncation, a full re-parse is triggered.

### Memory System

```
Session messages → AI extraction → Memories (decision, fact, preference, context, task)
  → Embedding generation → Hybrid search (FTS5 keyword + cosine similarity)
  → Background ranking (promote accessed, demote stale)
  → Duplicate cleanup (similarity threshold)
```

### Lifeboat Pattern

Before context compaction, yocore saves session state (`active_task`, `recent_decisions`, `open_questions`) to `session_context` for seamless resume in subsequent sessions.

---

## MCP & Claude Code Integration

### MCP Tools

Start in MCP mode with `yocore --mcp`. Available tools:

| Tool | Description |
|------|-------------|
| `yolog_search_memories` | Hybrid keyword + semantic search with type/tag filters |
| `yolog_get_project_context` | Project overview with categorized memories |
| `yolog_get_recent_memories` | Memories from recent sessions |
| `yolog_get_session_context` | Session state with lifeboat pattern |
| `yolog_save_lifeboat` | Save session state before context compaction |

### Claude Code Setup

1. Add to `~/.claude/claude_desktop_config.json`:
   ```json
   {
     "mcpServers": {
       "yolog": {
         "command": "yocore",
         "args": ["--mcp"]
       }
     }
   }
   ```

2. Install [yoskill](https://github.com/yologdev/yoskill) for `/yo` slash commands:
   ```bash
   git clone https://github.com/yologdev/yoskill ~/.claude/skills/yoskill
   ```

| Skill | Description |
|-------|-------------|
| `/yo context` | Session context + relevant memories |
| `/yo project` | Project-wide context |
| `/yo search <query>` | Search memories by keyword |
| `/yo search tag:<name>` | Filter by tag |

---

## LAN Discovery

Bind to `0.0.0.0` to enable automatic mDNS/Bonjour discovery on your local network:

```toml
[server]
host = "0.0.0.0"
api_key = "your-secret-key"       # Recommended when exposing to network
instance_name = "Office Desktop"  # Optional friendly name
```

Yocore announces itself as `_yocore._tcp` with TXT metadata (version, uuid, hostname, project count). The [yolog](https://github.com/yologdev/yolog) desktop app discovers instances automatically. Disable with `mdns_enabled = false`. All `/api/*` endpoints require the API key as a Bearer token; `/health` remains public.

---

## Architecture

```
yocore/
├── src/
│   ├── lib.rs              # Core orchestrator (config, db, watcher, AI queue)
│   ├── main.rs             # CLI entry point (HTTP server / MCP mode)
│   ├── config.rs           # TOML config with env var overrides
│   ├── watcher/            # File system watcher (notify + debouncer)
│   │   └── store.rs        # SessionStore enum → DB or EphemeralIndex
│   ├── parser/             # Trait-based JSONL parsing (Claude Code, OpenClaw)
│   ├── db/                 # SQLite: dual connections, WAL, FTS5, migrations
│   ├── ephemeral/          # In-memory storage: RwLock<HashMap>, LRU eviction
│   ├── api/                # Axum REST server (~57 routes, SSE, auth)
│   ├── mcp/                # MCP stdio JSON-RPC server (5 tools)
│   ├── ai/                 # AI features: title, memory, skills, markers, export
│   ├── embeddings/         # Local all-MiniLM-L6-v2 via candle (384-dim)
│   ├── scheduler/          # Background tasks: ranking, cleanup, backfill
│   ├── handlers/           # Shared logic for API + MCP
│   └── mdns.rs             # mDNS/Bonjour service announcement
├── docs/                   # mdBook documentation
├── npm/                    # npm package (@yologdev/core)
└── Cargo.toml
```

---

## License

MIT License - see [LICENSE](LICENSE) for details.

## Links

- [yolog](https://github.com/yologdev/yolog) — Desktop companion app
- [yoskill](https://github.com/yologdev/yoskill) — Claude Code skill pack
- [@yologdev/core](https://www.npmjs.com/package/@yologdev/core) — npm package
- [Documentation](https://yologdev.github.io/yocore/) — Full reference

<!-- Badge link definitions -->
[ci-shield]: https://img.shields.io/github/actions/workflow/status/yologdev/yocore/ci.yml?labelColor=black&style=flat-square&logo=github&label=CI
[ci-link]: https://github.com/yologdev/yocore/actions/workflows/ci.yml
[release-shield]: https://img.shields.io/github/v/release/yologdev/yocore?color=369eff&labelColor=black&style=flat-square
[release-link]: https://github.com/yologdev/yocore/releases
[npm-shield]: https://img.shields.io/npm/v/@yologdev/core?color=cb3837&labelColor=black&style=flat-square&logo=npm
[npm-link]: https://www.npmjs.com/package/@yologdev/core
[license-shield]: https://img.shields.io/badge/license-MIT-white?labelColor=black&style=flat-square
[license-link]: https://github.com/yologdev/yocore/blob/main/LICENSE
[docs-shield]: https://img.shields.io/badge/docs-mdBook-blue?labelColor=black&style=flat-square
[docs-link]: https://yologdev.github.io/yocore/
[last-commit-shield]: https://img.shields.io/github/last-commit/yologdev/yocore?color=c4f042&labelColor=black&style=flat-square
[last-commit-link]: https://github.com/yologdev/yocore/commits/main
