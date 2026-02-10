# HTTP API Reference

Base URL: `http://127.0.0.1:19420`

## Authentication

If `api_key` is configured, all `/api/*` endpoints require a Bearer token:

```
Authorization: Bearer <api_key>
```

The `/health` endpoint is always public.

---

## Health

### `GET /health`

Health check. No authentication required.

```bash
curl http://localhost:19420/health
```

```json
{
  "status": "ok",
  "version": "0.2.0",
  "instance_uuid": "84c11d21-d95a-48f1-ac17-b4c5d9e97c44",
  "instance_name": "Office Desktop"
}
```

---

## Projects

### `GET /api/projects`

List all projects.

```bash
curl http://localhost:19420/api/projects
```

### `POST /api/projects`

Create a new project.

```bash
curl -X POST http://localhost:19420/api/projects \
  -H "Content-Type: application/json" \
  -d '{"name": "my-project", "folder_path": "/path/to/project"}'
```

### `GET /api/projects/resolve`

Resolve a project by folder path.

```bash
curl "http://localhost:19420/api/projects/resolve?folder_path=/path/to/project"
```

### `GET /api/projects/:id`

Get a single project by ID.

### `PATCH /api/projects/:id`

Update a project.

```bash
curl -X PATCH http://localhost:19420/api/projects/<id> \
  -H "Content-Type: application/json" \
  -d '{"name": "new-name"}'
```

### `DELETE /api/projects/:id`

Delete a project and all its sessions, memories, and skills.

### `GET /api/projects/:id/analytics`

Get project analytics (session counts, message stats, memory distribution).

---

## Sessions

### `GET /api/sessions`

List sessions with optional filters.

| Parameter | Type | Description |
|-----------|------|-------------|
| `project_id` | string | Filter by project |
| `limit` | integer | Max results (default: 50) |
| `offset` | integer | Pagination offset |

```bash
curl "http://localhost:19420/api/sessions?project_id=<id>&limit=10"
```

### `GET /api/sessions/:id`

Get a single session by ID.

### `PATCH /api/sessions/:id`

Update session fields (e.g., title).

```bash
curl -X PATCH http://localhost:19420/api/sessions/<id> \
  -H "Content-Type: application/json" \
  -d '{"title": "New Title"}'
```

### `DELETE /api/sessions/:id`

Delete a session and its messages.

### `GET /api/sessions/:id/messages`

Get all messages for a session.

| Parameter | Type | Description |
|-----------|------|-------------|
| `limit` | integer | Max results |
| `offset` | integer | Pagination offset |

### `GET /api/sessions/:id/messages/:seq/content`

Get full content for a specific message by sequence number.

### `GET /api/sessions/:id/markers`

Get session markers (breakthrough, ship, decision, bug, stuck).

### `GET /api/sessions/:id/search`

Search within a single session's messages.

| Parameter | Type | Description |
|-----------|------|-------------|
| `q` | string | Search query |

### `GET /api/sessions/:id/bytes`

Read raw session file bytes.

### `POST /api/sessions/:id/messages/append`

Append messages to a session.

### `POST /api/sessions/:id/agent-summary`

Update the agent summary for a session.

### `GET /api/sessions/limit`

Get session limit information.

---

## Search

### `POST /api/search`

Global search across sessions and messages using hybrid keyword + semantic search.

```bash
curl -X POST http://localhost:19420/api/search \
  -H "Content-Type: application/json" \
  -d '{"query": "authentication bug", "project_id": "<id>"}'
```

---

## Memories

### `GET /api/memories`

List memories with optional filters.

| Parameter | Type | Description |
|-----------|------|-------------|
| `project_id` | string | Filter by project |
| `memory_type` | string | Filter by type: `decision`, `fact`, `preference`, `context`, `task` |
| `state` | string | Filter by state: `new`, `low`, `high`, `removed` |
| `limit` | integer | Max results |
| `offset` | integer | Pagination offset |

### `POST /api/memories/search`

Search memories using hybrid keyword + semantic search.

```bash
curl -X POST http://localhost:19420/api/memories/search \
  -H "Content-Type: application/json" \
  -d '{"query": "database schema", "project_id": "<id>", "limit": 10}'
```

### `GET /api/memories/:id`

Get a single memory by ID.

### `PATCH /api/memories/:id`

Update a memory (title, content, tags, state, etc.).

### `DELETE /api/memories/:id`

Delete a memory.

### `GET /api/projects/:id/memory-stats`

Get memory statistics for a project (counts by type, state, confidence distribution).

### `GET /api/projects/:id/memory-tags`

Get all unique tags used in a project's memories.

---

## Markers

### `DELETE /api/markers/:id`

Delete a session marker.

---

## AI Features

### `POST /api/ai/sessions/:id/title`

Trigger AI title generation for a session.

### `POST /api/ai/sessions/:id/memories`

Trigger AI memory extraction for a session.

### `POST /api/ai/sessions/:id/skills`

Trigger AI skill extraction for a session.

### `POST /api/ai/sessions/:id/markers`

Trigger AI marker detection for a session.

### `GET /api/ai/cli/status`

Check AI CLI availability status.

### `GET /api/ai/pending-sessions`

Get sessions awaiting AI processing.

### `GET /api/ai/export/capabilities`

Get AI export capabilities.

### `POST /api/ai/export/generate`

Generate an AI export.

### `POST /api/ai/export/chunk`

Process an AI export chunk.

### `POST /api/ai/export/merge`

Merge AI export chunks.

---

## Memory Ranking

### `POST /api/projects/:id/rank-memories`

Trigger memory ranking for a project. Promotes frequently-accessed memories and demotes stale ones.

### `GET /api/projects/:id/ranking-stats`

Get ranking statistics for a project.

---

## Skills

### `GET /api/projects/:id/skills`

List skills for a project.

| Parameter | Type | Description |
|-----------|------|-------------|
| `limit` | integer | Max results |
| `offset` | integer | Pagination offset |
| `sort` | string | Sort field |

### `GET /api/projects/:id/skills/stats`

Get skill statistics for a project.

### `DELETE /api/skills/:id`

Delete a skill.

---

## Embeddings

### `POST /api/embeddings/backfill`

Backfill embeddings for memories that are missing them. Uses the local all-MiniLM-L6-v2 model (384 dimensions).

---

## Configuration

### `GET /api/config`

Get the full configuration.

### `PUT /api/config`

Update the full configuration. Disabled when `YOLOG_CONFIG_READONLY=true`.

### `GET /api/config/ai`

Get AI configuration.

### `PUT /api/config/ai`

Update AI configuration.

### `GET /api/config/watch`

List watch paths.

### `POST /api/config/watch`

Add a new watch path.

```bash
curl -X POST http://localhost:19420/api/config/watch \
  -H "Content-Type: application/json" \
  -d '{"path": "~/.openclaw/workspace", "parser": "openclaw"}'
```

### `DELETE /api/config/watch/:index`

Remove a watch path by index.

---

## Context API

Used by LLM skills and hooks for memory access.

### `GET /api/context/project`

Get project context with categorized memories.

| Parameter | Type | Description |
|-----------|------|-------------|
| `project_path` | string | Project directory path |

### `POST /api/context/session`

Get session context including lifeboat data.

### `GET /api/context/recent-memories`

Get memories from recent sessions.

### `POST /api/context/lifeboat`

Save session state (lifeboat pattern) before context compaction.

### `POST /api/context/search`

Search memories via the context API.

---

## Server-Sent Events

### `GET /api/events`

Subscribe to real-time events via SSE. See [SSE Events](sse-events.md) for event types and payloads.

```bash
curl -N http://localhost:19420/api/events
```
