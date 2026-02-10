# Yocore

[![CI](https://github.com/yologdev/yocore/actions/workflows/ci.yml/badge.svg)](https://github.com/yologdev/yocore/actions/workflows/ci.yml)
[![License: MIT](https://img.shields.io/badge/License-MIT-blue.svg)](LICENSE)
[![GitHub Release](https://img.shields.io/github/v/release/yologdev/yocore)](https://github.com/yologdev/yocore/releases)
[![Docs](https://img.shields.io/badge/docs-mdBook-blue)](https://yologdev.github.io/yocore/)

Headless service for watching, parsing, storing, and serving AI coding sessions.

Yocore is the core engine behind [Yolog](https://github.com/yologdev/yolog) - a platform that archives and visualizes AI pair programming sessions from Claude Code, Cursor, and other AI coding assistants.

## Features

- **Session Watching**: Automatically watches folders for new AI coding sessions
- **Multi-Parser Support**: Parses Claude Code, OpenClaw, and other AI assistant formats
- **Full-Text Search**: SQLite FTS5-powered search across all sessions and memories
- **Memory System**: Extract and organize decisions, facts, preferences, and tasks
- **HTTP API**: RESTful API for session replay, search, and memory management
- **MCP Server**: Model Context Protocol integration for AI assistants
- **LAN Discovery**: Automatic instance discovery via mDNS/Bonjour on the local network
- **Lifeboat Pattern**: Session context preservation across context compaction

## Installation

### npm (Recommended)

```bash
npm install -g @yologdev/core
```

### Homebrew (macOS)

```bash
brew install yocore
```

### Binary Download

Download the latest release from [GitHub Releases](https://github.com/yologdev/yocore/releases).

### Build from Source

```bash
git clone https://github.com/yologdev/yocore.git
cd yocore
cargo build --release
```

## Usage

### Start the Server

```bash
# Start with default config
yocore

# Start with custom config
yocore --config /path/to/config.toml

# Start in MCP server mode (for Claude Code integration)
yocore --mcp
```

### Configuration

Create a config file at `~/.yolog/config.toml`:

```toml
[server]
port = 19420
host = "127.0.0.1"
# api_key = "optional-secret"    # Enable for remote access
# mdns_enabled = true            # mDNS discovery (default: true)
# instance_name = "My Workstation"  # Custom display name for LAN discovery

[database]
path = "~/.local/share/yocore/yocore.db"

# Watch multiple paths with different parsers
[[watch]]
path = "~/.claude/projects"
parser = "claude_code"

[[watch]]
path = "~/.openclaw/workspace"
parser = "openclaw"

[ai]
enabled = false
# provider = "anthropic"
# ANTHROPIC_API_KEY from env

[ai.features]
title_generation = false
skills_discovery = false
memory_extraction = false
```

### MCP Tools

When running in MCP mode (`yocore --mcp`), the following tools are available:

| Tool | Description |
|------|-------------|
| `yolog_search_memories` | Hybrid keyword + semantic search |
| `yolog_get_project_context` | Get project overview with categorized memories |
| `yolog_get_memories_by_type` | Filter memories by type (decision, fact, etc.) |
| `yolog_get_memories_by_tag` | Filter memories by tag |
| `yolog_get_recent_memories` | Get memories from recent sessions |
| `yolog_get_session_context` | Get session state with lifeboat pattern |
| `yolog_save_lifeboat` | Save session state before context compaction |

## LAN Discovery

Yocore can announce itself on your local network via mDNS (Bonjour/Zeroconf), so the [Yolog desktop app](https://github.com/yologdev/yolog) can automatically find all running instances without manual configuration.

### Enable LAN Discovery

By default, yocore binds to `127.0.0.1` (localhost only) and mDNS is disabled. To enable LAN discovery, change the host to `0.0.0.0`:

```toml
[server]
host = "0.0.0.0"
port = 19420
api_key = "your-secret-key"  # Recommended when exposing to the network
```

That's it — yocore will automatically announce itself as `_yocore._tcp` on the local network. The Yolog desktop app will discover it within seconds.

### What Gets Advertised

The mDNS announcement includes metadata so the desktop app can display instance information before connecting:

| Property | Example | Description |
|----------|---------|-------------|
| Instance name | `Yocore-macbook-a1b2c3d4` | Auto-generated or custom display name |
| Port | `19420` | HTTP API port |
| `version` | `0.1.0` | Yocore version |
| `uuid` | `84c11d21-...` | Persistent instance ID (stable across restarts) |
| `hostname` | `macbook.local` | Machine hostname |
| `name` | `Office Desktop` | Custom friendly name (if configured) |
| `api_key_required` | `true` / `false` | Whether auth is needed |
| `projects` | `3` | Number of tracked projects |

### Custom Instance Name

By default, instances are named `Yocore-{hostname}-{short-uuid}`. You can set a friendly name:

```toml
[server]
host = "0.0.0.0"
instance_name = "Office Desktop"
```

### Disable mDNS

If you want to expose yocore on the network but without mDNS announcement:

```toml
[server]
host = "0.0.0.0"
mdns_enabled = false
```

### Verify Discovery

On macOS, you can verify mDNS is working:

```bash
dns-sd -B _yocore._tcp
```

### Security

When exposing yocore to the network, always set an API key:

```toml
[server]
host = "0.0.0.0"
api_key = "your-secret-key"
```

All `/api/*` endpoints will require the key as a Bearer token. The `/health` endpoint remains public (used for discovery verification).

## Claude Code Integration

For the best experience with Claude Code, use [yoskill](https://github.com/yologdev/yoskill) - a skill pack that provides easy access to Yocore's memory system.

### Setup

1. Start Yocore in MCP mode:
   ```bash
   yocore --mcp
   ```

2. Add to your Claude Code MCP config (`~/.claude/claude_desktop_config.json`):
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

3. Install yoskill for convenient slash commands:
   ```bash
   git clone https://github.com/yologdev/yoskill ~/.claude/skills/yoskill
   ```

### Available Skills

| Skill | Description |
|-------|-------------|
| `/yo context` | Get session context + relevant memories |
| `/yo project` | Get project-wide context |
| `/yo search <query>` | Search memories by keyword |
| `/yo search tag:<name>` | Filter by tag (e.g., `tag:bug`) |

## License

MIT License - see [LICENSE](LICENSE) for details.

## Links

- [Yolog Desktop](https://github.com/yologdev/yolog) - GUI companion app
- [yoskill](https://github.com/yologdev/yoskill) - Claude Code skill pack
