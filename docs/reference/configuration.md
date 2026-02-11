# Configuration

Yocore uses a TOML config file at `~/.yolog/config.toml`. Generate a default config with `yocore --init`.

## Top-Level

| Key | Type | Default | Description |
|-----|------|---------|-------------|
| `storage` | string | `"db"` | Storage backend: `"db"` (SQLite, persistent) or `"ephemeral"` (in-memory, volatile) |
| `data_dir` | string | `"~/.yolog"` | Data directory for database and other files |

## `[server]`

HTTP server settings.

| Key | Type | Default | Description |
|-----|------|---------|-------------|
| `port` | integer | `19420` | Server port |
| `host` | string | `"127.0.0.1"` | Bind address. Use `"0.0.0.0"` for LAN access |
| `api_key` | string | *none* | Bearer token for API authentication. If set, all `/api/*` endpoints require `Authorization: Bearer <key>` |
| `mdns_enabled` | boolean | `true` | Enable mDNS/Bonjour LAN discovery. Auto-disabled when host is `127.0.0.1` |
| `instance_name` | string | *auto* | Custom display name for mDNS (e.g., `"Office Desktop"`). Default: `Yocore-{hostname}-{short_uuid}` |

## `[[watch]]`

Directories to watch for session files. This is an array — add multiple `[[watch]]` blocks for multiple paths.

| Key | Type | Default | Description |
|-----|------|---------|-------------|
| `path` | string | *required* | Directory path (supports `~` expansion) |
| `parser` | string | `"claude_code"` | Parser type: `claude_code`, `openclaw` |
| `enabled` | boolean | `true` | Whether this watch path is active |

> **Note:** `[[projects]]` is accepted as an alias for `[[watch]]` for backward compatibility.

## `[ephemeral]`

Settings for ephemeral (in-memory) storage mode. Only used when `storage = "ephemeral"`.

| Key | Type | Default | Description |
|-----|------|---------|-------------|
| `max_sessions` | integer | `100` | Maximum sessions to keep in memory. Oldest (LRU) sessions are evicted when exceeded |
| `max_messages_per_session` | integer | `5000` | Maximum messages stored per session |

## `[ai]`

AI feature settings. AI is active when `provider` is set and at least one feature toggle is `true`. Requires [Claude Code](https://claude.ai/code) CLI installed and authenticated.

| Key | Type | Default | Description |
|-----|------|---------|-------------|
| `provider` | string | *none* | AI provider (e.g., `"claude_code"`). Required for any AI feature to work |
| `title_generation` | boolean | `true` | Auto-generate session titles. Works with both `db` and `ephemeral` storage |
| `marker_detection` | boolean | `true` | Detect session markers. Requires `storage = "db"` |
| `memory_extraction` | boolean | `true` | Extract memories (decisions, facts, etc.). Requires `storage = "db"`. Activates ranking, duplicate_cleanup, and embedding_refresh scheduler tasks |
| `skills_discovery` | boolean | `true` | Discover reusable skills from sessions. Requires `storage = "db"`. Activates skill_cleanup scheduler task |

> **Note:** The legacy `[ai.features]` section and `ai.enabled` field are still accepted for backward compatibility but deprecated.

## `[scheduler]`

Background tasks that run periodically. Auto-activated by their parent AI features — no individual `enabled` flags needed. All scheduler tasks require `storage = "db"`.

### `[scheduler.ranking]`

Promotes frequently-accessed memories and demotes stale ones. Activated by `memory_extraction`.

| Key | Type | Default | Description |
|-----|------|---------|-------------|
| `interval_hours` | integer | `6` | Hours between ranking sweeps |
| `batch_size` | integer | `500` | Memories per batch |

### `[scheduler.duplicate_cleanup]`

Retroactive duplicate memory detection and removal. Activated by `memory_extraction`.

| Key | Type | Default | Description |
|-----|------|---------|-------------|
| `interval_hours` | integer | `24` | Hours between cleanup sweeps |
| `similarity_threshold` | float | `0.75` | Jaccard similarity threshold for duplicates |
| `batch_size` | integer | `500` | Memories per batch |

### `[scheduler.embedding_refresh]`

Backfill embeddings for memories that are missing them. Activated by `memory_extraction`.

| Key | Type | Default | Description |
|-----|------|---------|-------------|
| `interval_hours` | integer | `12` | Hours between refresh sweeps |
| `batch_size` | integer | `100` | Memories per batch (lower — embeddings are CPU-intensive) |

### `[scheduler.skill_cleanup]`

Retroactive duplicate skill detection and removal. Activated by `skills_discovery`.

| Key | Type | Default | Description |
|-----|------|---------|-------------|
| `interval_hours` | integer | `24` | Hours between cleanup sweeps |
| `similarity_threshold` | float | `0.80` | Jaccard similarity threshold for duplicates |
| `batch_size` | integer | `500` | Skills per batch |

## Storage Modes

### `storage = "db"` (default)

Full-featured mode with SQLite persistence. All data survives restarts. Supports search (FTS5), memories, skills, AI features, and scheduler tasks.

### `storage = "ephemeral"`

Lightweight mode with in-memory storage. No database files, no persistence — all data is lost on restart. Useful for quick local monitoring without disk overhead.

Available in ephemeral mode:
- File watching and session parsing
- SSE event streaming
- Project/session/message API endpoints (from memory)
- Message content API (reads from JSONL files on disk)
- Config API
- Title generation (if AI provider configured)

Not available (returns `501 Not Implemented`):
- Full-text search
- Memories, skills, markers
- Memory ranking, duplicate cleanup, embedding refresh
- MCP server
- mDNS discovery (no persistent UUID)

## Full Example

```toml
# Storage backend: "db" (default) or "ephemeral"
storage = "db"

[server]
port = 19420
host = "127.0.0.1"
# api_key = "your-secret-key"
# instance_name = "My Mac mini"

[[watch]]
path = "~/.claude/projects"
parser = "claude_code"
enabled = true

# [[watch]]
# path = "~/.openclaw/workspace"
# parser = "openclaw"
# enabled = true

# Ephemeral storage limits (only used when storage = "ephemeral")
# [ephemeral]
# max_sessions = 100
# max_messages_per_session = 5000

[ai]
# provider = "claude_code"
title_generation = true
marker_detection = true
memory_extraction = true
skills_discovery = true

[scheduler.ranking]
interval_hours = 6
batch_size = 500

[scheduler.duplicate_cleanup]
interval_hours = 24
similarity_threshold = 0.75
batch_size = 500

[scheduler.embedding_refresh]
interval_hours = 12
batch_size = 100

[scheduler.skill_cleanup]
interval_hours = 24
similarity_threshold = 0.80
batch_size = 500
```

## Config API

Configuration can also be read and modified at runtime via the REST API:

- `GET /api/config` — Read full config
- `PUT /api/config` — Update full config
- `GET /api/config/ai` — Read AI config
- `PUT /api/config/ai` — Update AI config
- `GET /api/config/watch` — List watch paths
- `POST /api/config/watch` — Add watch path
- `DELETE /api/config/watch/:index` — Remove watch path

Set `YOLOG_CONFIG_READONLY=true` to disable config writes via API.
