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

## Claude Code Integration

For the best experience with Claude Code, use [yo-skills](https://github.com/yologdev/yo-skills) - a skill pack that provides easy access to Yocore's memory system.

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

3. Install yo-skills for convenient slash commands:
   ```bash
   git clone https://github.com/yologdev/yo-skills ~/.claude/skills/yo-skills
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
- [yo-skills](https://github.com/yologdev/yo-skills) - Claude Code skill pack
