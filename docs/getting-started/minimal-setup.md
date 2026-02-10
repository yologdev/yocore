# Minimal Setup (Archive & Replay)

The simplest way to use yocore: archive your AI coding sessions and replay them later. No AI features, no API keys — just watch and store.

## 1. Configure a Watch Path

After running `yocore --init`, edit `~/.yolog/config.toml` to watch the directories where your AI assistant stores sessions:

```toml
[[watch]]
path = "~/.claude/projects"
parser = "claude_code"
```

You can watch multiple directories with different parsers:

```toml
[[watch]]
path = "~/.claude/projects"
parser = "claude_code"

[[watch]]
path = "~/.openclaw/workspace"
parser = "openclaw"
```

## 2. Start Yocore

```bash
yocore
```

That's it. Yocore will:
- Watch the configured directories for new and updated session files
- Parse them incrementally (only new content, no re-parsing)
- Store structured data in a local SQLite database
- Serve everything via its HTTP API

## 3. Replay with Yolog

Install the [Yolog](https://github.com/yologdev/support) app and point it at your yocore instance. You get:

- **Session timeline** — Step through conversations message by message
- **Full-text search** — Find any discussion across all sessions
- **Project grouping** — Sessions organized by project directory
- **Export** — Copy or export sessions for sharing

## What You Get

With just this setup, yocore archives every AI coding session automatically. You can search across all your past conversations, replay any session, and never lose context from a previous coding session.

| Feature | Available |
|---------|-----------|
| Session watching & parsing | Yes |
| Full-text search across sessions | Yes |
| HTTP API for all session data | Yes |
| Session replay via Yolog app | Yes |
| LAN discovery (mDNS) | Yes |
| Memory extraction | No — requires [Long-Term Memory](long-term-memory.md) |
| Project context & knowledge base | No — requires [Long-Term Memory](long-term-memory.md) |
| Hybrid semantic search | No — requires [Long-Term Memory](long-term-memory.md) |

## Next Step

Want your AI assistant to learn from past sessions? Enable the [Long-Term Memory](long-term-memory.md) layer to add AI-powered memory extraction, ranking, and semantic search.
