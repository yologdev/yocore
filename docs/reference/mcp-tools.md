# MCP Tools Reference

Yocore implements the [Model Context Protocol](https://modelcontextprotocol.io/) (MCP) for AI assistant integration. Start in MCP mode with `yocore --mcp`.

> **Tip:** For most users, [yoskill](https://github.com/yologdev/yoskill) provides a more convenient interface with slash commands and automatic hooks. MCP is useful for programmatic access or clients that don't support skills. See [Yo Memory](../getting-started/yo-memory.md) for setup.

Protocol version: `2024-11-05`

## Tools

### `yolog_search_memories`

Search and browse project memories. Supports hybrid keyword + semantic search, or browsing/filtering without a query.

**Parameters:**

| Name | Type | Required | Default | Description |
|------|------|----------|---------|-------------|
| `query` | string | no | | Search query. Omit to browse/filter |
| `project_path` | string | no | `.` | Project directory path |
| `memory_types` | string[] | no | | Filter by type: `decision`, `fact`, `preference`, `context`, `task` |
| `tags` | string[] | no | | Filter by tags (AND logic — memories must have ALL tags) |
| `limit` | integer | no | `10` | Maximum results |

**Example:**

```json
{
  "name": "yolog_search_memories",
  "arguments": {
    "query": "database schema decisions",
    "project_path": "/Users/me/my-project",
    "memory_types": ["decision", "fact"],
    "limit": 5
  }
}
```

---

### `yolog_get_project_context`

Get high-level project context with key decisions, facts, and preferences. Returns the top 5 memories per type.

**Parameters:**

| Name | Type | Required | Description |
|------|------|----------|-------------|
| `project_path` | string | **yes** | Project directory path |

**Example:**

```json
{
  "name": "yolog_get_project_context",
  "arguments": {
    "project_path": "/Users/me/my-project"
  }
}
```

---

### `yolog_get_recent_memories`

Get memories from the most recent coding sessions.

**Parameters:**

| Name | Type | Required | Default | Description |
|------|------|----------|---------|-------------|
| `project_path` | string | **yes** | | Project directory path |
| `sessions` | integer | no | `3` | Number of recent sessions to include |
| `limit` | integer | no | `10` | Maximum memories |

**Example:**

```json
{
  "name": "yolog_get_recent_memories",
  "arguments": {
    "project_path": "/Users/me/my-project",
    "sessions": 5,
    "limit": 20
  }
}
```

---

### `yolog_get_session_context`

Get session context including current task state, recent decisions, open questions, and relevant memories. Implements the lifeboat pattern for context preservation.

**Parameters:**

| Name | Type | Required | Description |
|------|------|----------|-------------|
| `session_id` | string | **yes** | Claude Code session ID (from `YOLOG_SESSION_ID` env var) |
| `project_path` | string | no | Project directory path |

**Example:**

```json
{
  "name": "yolog_get_session_context",
  "arguments": {
    "session_id": "abc123-session-id",
    "project_path": "/Users/me/my-project"
  }
}
```

**Response includes:**
- Current state (active task, resume context)
- Recent decisions and open questions
- Persistent knowledge (high-importance memories)
- This session's memories
- Recent memories from last 3 sessions

---

### `yolog_save_lifeboat`

Emergency save of session state before context compaction. Preserves the current work context so it can be recovered in the next session or after context is compacted.

**Parameters:**

| Name | Type | Required | Description |
|------|------|----------|-------------|
| `session_id` | string | **yes** | Claude Code session ID |
| `summary` | string | no | Brief summary of current work state. Auto-generated if omitted |

**Example:**

```json
{
  "name": "yolog_save_lifeboat",
  "arguments": {
    "session_id": "abc123-session-id",
    "summary": "Implementing auth middleware, blocked on JWT validation"
  }
}
```
