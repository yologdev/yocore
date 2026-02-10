# Memory System

Yocore's memory system extracts and organizes knowledge from AI coding sessions, making it searchable and accessible across sessions.

## Memory Types

| Type | Description | Example |
|------|-------------|---------|
| `decision` | Architectural or design decisions | "Use JWT for auth instead of sessions" |
| `fact` | Discovered facts about the codebase | "The database uses WAL mode for concurrency" |
| `preference` | User or project preferences | "Always use bun instead of npm" |
| `context` | Background context and constraints | "The API must support backward compatibility" |
| `task` | Work items and TODOs | "Add pagination to the search endpoint" |

## Memory Fields

| Field | Type | Description |
|-------|------|-------------|
| `title` | string | Short descriptive title |
| `content` | string | Full memory content |
| `context` | string | Additional context about when/why |
| `tags` | string[] | Categorization tags (e.g., `["security", "auth"]`) |
| `confidence` | float | AI confidence score (0.0 - 1.0) |
| `is_validated` | boolean | Whether manually validated |
| `state` | string | Ranking state (see lifecycle below) |
| `access_count` | integer | How often this memory has been accessed |

## Memory Lifecycle

Memories go through a ranking lifecycle based on access patterns:

```
new → low → high → removed
                ↓
              (still accessible, just lower priority)
```

| State | Description |
|-------|-------------|
| `new` | Just extracted, not yet ranked |
| `low` | Low importance (infrequently accessed) |
| `high` | High importance (frequently accessed, validated) |
| `removed` | Demoted below threshold (still in DB, excluded from search) |

The background ranking job (every 6 hours by default) promotes memories that are frequently accessed and demotes stale ones.

## Search

Yocore supports two search modes:

### Keyword Search (FTS5)

SQLite full-text search across memory titles and content. Fast and exact.

### Hybrid Search

Combines keyword search (FTS5) with semantic search (vector embeddings). Uses Reciprocal Rank Fusion (RRF) to merge results. More relevant for natural language queries.

The local **all-MiniLM-L6-v2** model (384 dimensions) generates embeddings. It's loaded lazily on first use.

### Search via API

```bash
# Hybrid search
curl -X POST http://localhost:19420/api/memories/search \
  -H "Content-Type: application/json" \
  -d '{"query": "database migration strategy", "project_id": "<id>"}'
```

### Search via MCP

```json
{
  "name": "yolog_search_memories",
  "arguments": {
    "query": "database migration",
    "memory_types": ["decision"],
    "tags": ["database"]
  }
}
```

## Tags

Memories can have multiple tags stored as a JSON array. Filter by tags via the API or MCP:

```bash
# List all tags for a project
curl http://localhost:19420/api/projects/<id>/memory-tags
```

## Duplicate Detection

When `ai.features.duplicate_cleanup.enabled = true`, yocore periodically scans for semantically similar memories and removes duplicates. The similarity threshold (default: 0.75) controls how strict the detection is.

## Embedding Backfill

Memories extracted before the embedding model was loaded may lack embeddings. The background embedding refresh job (every 12 hours) backfills these. You can also trigger it manually:

```bash
curl -X POST http://localhost:19420/api/embeddings/backfill
```
