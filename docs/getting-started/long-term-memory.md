# Long-Term Memory

Yocore includes an optional AI-powered memory system that extracts and organizes knowledge from your coding sessions. This gives your AI assistant persistent context across sessions — decisions made, patterns discovered, preferences established, and tasks tracked.

## What It Adds

Without memory, yocore watches and stores session files. With memory enabled, it also:

- **Extracts knowledge** — Decisions, facts, preferences, and tasks are pulled from session transcripts
- **Builds project context** — Categorized memories form a searchable knowledge base per project
- **Enables hybrid search** — Full-text + semantic search across all extracted memories
- **Preserves context** — The lifeboat pattern saves session state before context compaction
- **Self-maintains** — Background ranking promotes useful memories and removes stale ones

## Why It's Optional

Yocore works fully without the memory layer. The core value — watching, parsing, storing, and serving AI sessions — requires no AI features. You can replay sessions, search transcripts, and share via LAN without any of this.

Enable memory when you want your AI assistant to learn from past sessions.

## Setup with yoskill (Recommended)

[yoskill](https://github.com/yologdev/yoskill) is a skill pack that gives your AI assistant convenient slash commands for the memory system. It works with Claude Code, OpenClaw, Cursor, Windsurf, Copilot, and Cline.

### Install yoskill

**Plugin install** (if your client supports it):

```
/plugin marketplace add yologdev/yoskill
/plugin install yo@yoskill
```

**Manual install** (Claude Code):

```bash
# Clone the skill
git clone https://github.com/yologdev/yoskill ~/.claude/skills/yo

# Copy hooks
cp ~/.claude/skills/yo/hooks/* ~/.claude/hooks/
```

### Initialize

```
/yo init
```

This auto-detects your AI client and configures:
- Required permissions (`Skill(yo)`, `Bash(curl:*)`)
- Memory integration snippet in your project config
- Hook registration for automatic context flow

You can also specify a client: `/yo init cursor`

### Available Commands

| Command | Description |
|---------|-------------|
| `/yo context` | Get session context + relevant memories (use at session start) |
| `/yo project` | Get project-wide context overview |
| `/yo memory-search <query>` | Hybrid full-text + semantic memory search |
| `/yo memory-search tag:<name>` | Filter memories by tag (e.g., `tag:bug`, `tag:security`) |
| `/yo project-search <query>` | Search raw session transcripts (BM25 FTS) |
| `/yo memories` | List memories extracted from current session |
| `/yo update <id> state=<value>` | Modify a memory's state or confidence |
| `/yo delete <id>` | Soft-delete a memory |
| `/yo tags` | List all available memory tags |
| `/yo status` | Check yocore connection status |

### Hooks

yoskill registers two hooks that automate the memory workflow:

**SessionStart** — Runs when a new session begins. Automatically loads relevant project context and memories, so your AI assistant starts with knowledge from past sessions.

**PreCompact** — Runs before context compaction. Saves the current session state (active task, recent decisions, open questions) as a "lifeboat" so nothing is lost when the context window is compressed.

Hook configuration in `.claude/settings.local.json`:

```json
{
  "hooks": {
    "SessionStart": [
      {
        "hooks": [
          { "type": "command", "command": ".claude/hooks/session-start.sh" }
        ]
      }
    ],
    "PreCompact": [
      {
        "hooks": [
          { "type": "command", "command": ".claude/hooks/pre-compact.sh" }
        ]
      }
    ]
  }
}
```

### Enable AI Features in Yocore

For memory extraction to work, enable AI in your yocore config:

```toml
[ai]
provider = "claude_code"
memory_extraction = true
title_generation = true
skills_discovery = true
```

## Alternative: Raw MCP

If your AI client doesn't support skills/plugins but does support MCP (Model Context Protocol), you can connect to yocore directly via MCP.

### When to Use MCP

- Your client only supports MCP, not skills
- You want programmatic access to the memory API
- You're building a custom integration

### Setup

Add to your MCP configuration:

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

### MCP Tools

| Tool | Description |
|------|-------------|
| `yolog_search_memories` | Hybrid keyword + semantic search |
| `yolog_get_project_context` | Project overview with categorized memories |
| `yolog_get_memories_by_type` | Filter by type (decision, fact, etc.) |
| `yolog_get_memories_by_tag` | Filter by tag |
| `yolog_get_recent_memories` | Recent session memories |
| `yolog_get_session_context` | Session state with lifeboat |
| `yolog_save_lifeboat` | Save session state before compaction |

See [MCP Tools Reference](../reference/mcp-tools.md) for parameters and examples.

## Learn More

- [Memory System Deep Dive](../guides/memory-system.md) — How extraction, ranking, search, and dedup work internally
- [AI Features](../guides/ai-features.md) — All AI-powered features and configuration
- [Configuration Reference](../reference/configuration.md) — Full `[ai]` config options
