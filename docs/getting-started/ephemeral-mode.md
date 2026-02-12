# Ephemeral Mode (No Database)

Lightweight mode that keeps everything in memory. No SQLite, no disk writes, no persistence — data is lost on restart. Ideal for quick local monitoring or testing.

## 1. Create a Config

```toml
storage = "ephemeral"

[server]
port = 19420
host = "127.0.0.1"

[[watch]]
path = "~/.claude/projects"
parser = "claude_code"

# Optional: enable AI title generation
[ai]
provider = "claude_code"

[ephemeral]
max_sessions = 100              # LRU eviction after this many sessions
max_messages_per_session = 50   # Keep last 50 messages per session in memory
```

## 2. Start Yocore

```bash
yocore --config /path/to/ephemeral.toml
```

Yocore will:
- Watch configured directories for session files
- Parse them incrementally (same as DB mode)
- Store sessions and messages in memory (`EphemeralIndex`)
- Serve everything via the HTTP API and SSE
- Auto-generate session titles if AI is enabled (at 49+ messages)

## 3. Connect with Yolog

The [Yolog](https://github.com/yologdev/support) desktop app connects the same way — it detects ephemeral mode via the `/health` endpoint (`"storage": "ephemeral"`) and hides DB-only features automatically.

## What You Get

| Feature | Available |
|---------|-----------|
| Session watching & parsing | Yes |
| HTTP API for sessions and messages | Yes |
| SSE real-time events | Yes |
| AI title generation | Yes (if `[ai]` configured) |
| Message content from JSONL files | Yes (reads from disk) |
| Full-text search | No — requires `storage = "db"` |
| Memories, skills, markers | No — requires `storage = "db"` |
| MCP server | No — requires `storage = "db"` |
| LAN discovery (mDNS) | No — no persistent instance UUID |

## Memory Management

- **Message windowing**: Full parses keep only the last N messages in RAM (default 50). New messages from incremental parses are appended without limit. Older messages are still readable from JSONL files via byte offsets.
- **LRU eviction**: When session count exceeds `max_sessions`, the least recently accessed session is evicted from memory.
- **No persistence**: All data is lost when yocore stops. Start with `storage = "db"` if you need persistence.

## Next Step

Want full-text search, memories, and AI-powered features? Switch to [Minimal Setup](minimal-setup.md) with `storage = "db"` (the default).
