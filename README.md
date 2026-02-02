# Yocore

Headless service for watching, parsing, storing, and serving AI coding sessions.

Yocore is the core engine behind [Yolog](https://github.com/yologdev/yolog) - a platform that archives and visualizes AI pair programming sessions from Claude Code, Cursor, and other AI coding assistants.

## Features

- **Session Watching**: Automatically watches folders for new AI coding sessions
- **Multi-Parser Support**: Parses Claude Code, OpenClaw, and other AI assistant formats
- **Full-Text Search**: SQLite FTS5-powered search across all sessions and memories
- **Memory System**: Extract and organize decisions, facts, preferences, and tasks
- **HTTP API**: RESTful API for session replay, search, and memory management
- **MCP Server**: Model Context Protocol integration for AI assistants
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

Create a config file at `~/.config/yocore/config.toml`:

```toml
[server]
port = 19420
host = "127.0.0.1"
# api_key = "optional-secret"  # Enable for remote access

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

## License

MIT License - see [LICENSE](LICENSE) for details.

## Links

- [Yolog Desktop](https://github.com/yologdev/yolog) - GUI companion app
