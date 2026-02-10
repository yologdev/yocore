# Claude Code Integration

Yocore integrates with Claude Code via the Model Context Protocol (MCP), giving Claude access to your project's memory system.

## Setup

### 1. Start Yocore in MCP Mode

```bash
yocore --mcp
```

### 2. Add to Claude Code MCP Config

Add yocore to `~/.claude/claude_desktop_config.json`:

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

### 3. Install yoskill (Optional)

[yoskill](https://github.com/yologdev/yoskill) provides convenient slash commands:

```bash
git clone https://github.com/yologdev/yoskill ~/.claude/skills/yoskill
```

## Available Skills

| Skill | Description |
|-------|-------------|
| `/yo context` | Get session context + relevant memories |
| `/yo project` | Get project-wide context |
| `/yo search <query>` | Search memories by keyword |
| `/yo search tag:<name>` | Filter by tag (e.g., `tag:bug`) |

## MCP Tools

When connected via MCP, Claude has access to these tools:

| Tool | Description |
|------|-------------|
| `yolog_search_memories` | Hybrid keyword + semantic search across memories |
| `yolog_get_project_context` | Get project overview with categorized memories |
| `yolog_get_recent_memories` | Get memories from recent sessions |
| `yolog_get_session_context` | Get session state with lifeboat pattern |
| `yolog_save_lifeboat` | Save session state before context compaction |

See [MCP Tools Reference](../reference/mcp-tools.md) for detailed parameter documentation.
