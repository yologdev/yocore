# Configuration

Yocore uses a TOML config file at `~/.yolog/config.toml`. Generate a default config with `yocore --init`.

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

## `[ai]`

AI feature settings. Requires [Claude Code](https://claude.ai/code) CLI installed and authenticated.

| Key | Type | Default | Description |
|-----|------|---------|-------------|
| `enabled` | boolean | `false` | Master switch for all AI features |
| `provider` | string | *none* | AI provider (e.g., `"claude_code"`). Required for AI features to work. |

## `[ai.features]`

Individual AI feature toggles. Only active when `ai.enabled = true`.

| Key | Type | Default | Description |
|-----|------|---------|-------------|
| `title_generation` | boolean | `true` | Auto-generate session titles |
| `skills_discovery` | boolean | `true` | Discover reusable skills from sessions |
| `memory_extraction` | boolean | `true` | Extract memories (decisions, facts, etc.) |

## `[ai.features.ranking]`

Background memory ranking — promotes frequently-accessed memories and demotes stale ones.

| Key | Type | Default | Description |
|-----|------|---------|-------------|
| `enabled` | boolean | `true` | Enable periodic ranking |
| `interval_hours` | integer | `6` | Hours between ranking sweeps |
| `batch_size` | integer | `500` | Memories per batch |

## `[ai.features.duplicate_cleanup]`

Retroactive duplicate memory detection and removal.

| Key | Type | Default | Description |
|-----|------|---------|-------------|
| `enabled` | boolean | `false` | Enable duplicate cleanup (opt-in) |
| `interval_hours` | integer | `24` | Hours between cleanup sweeps |
| `similarity_threshold` | float | `0.75` | Cosine similarity threshold for duplicates |
| `batch_size` | integer | `500` | Memories per batch |

## `[ai.features.embedding_refresh]`

Backfill embeddings for memories that are missing them.

| Key | Type | Default | Description |
|-----|------|---------|-------------|
| `enabled` | boolean | `true` | Enable embedding refresh |
| `interval_hours` | integer | `12` | Hours between refresh sweeps |
| `batch_size` | integer | `100` | Memories per batch (lower — embeddings are CPU-intensive) |

## `[ai.features.skill_cleanup]`

Retroactive duplicate skill detection and removal.

| Key | Type | Default | Description |
|-----|------|---------|-------------|
| `enabled` | boolean | `false` | Enable skill cleanup (opt-in) |
| `interval_hours` | integer | `24` | Hours between cleanup sweeps |
| `similarity_threshold` | float | `0.80` | Cosine similarity threshold for duplicates |
| `batch_size` | integer | `500` | Skills per batch |

## Top-Level

| Key | Type | Default | Description |
|-----|------|---------|-------------|
| `data_dir` | string | `"~/.yolog"` | Data directory for database and other files |

## Full Example

```toml
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

[ai]
enabled = false
# provider = "claude_code"

[ai.features]
title_generation = true
skills_discovery = true
memory_extraction = true

[ai.features.ranking]
enabled = true
interval_hours = 6
batch_size = 500

[ai.features.duplicate_cleanup]
enabled = false
interval_hours = 24
similarity_threshold = 0.75
batch_size = 500

[ai.features.embedding_refresh]
enabled = true
interval_hours = 12
batch_size = 100

[ai.features.skill_cleanup]
enabled = false
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
