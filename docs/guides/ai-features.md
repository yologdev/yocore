# AI Features

Yocore includes AI-powered features that automatically enrich your coding sessions with titles, memories, skills, and markers.

## Prerequisites

1. Have [Claude Code](https://claude.ai/code) CLI installed and authenticated (yocore spawns it as a subprocess)
2. Enable AI in config:

```toml
[ai]
enabled = true
provider = "claude_code"
```

No API key is needed in yocore's config — it spawns the Claude Code CLI which handles its own authentication.

## Features

### Title Generation

Automatically generates descriptive titles for coding sessions based on their content.

- **Trigger**: Auto-triggered when a new session is parsed
- **Config**: `ai.title_generation = true`
- **SSE events**: `ai:title:start`, `ai:title:complete`, `ai:title:error`

### Memory Extraction

Extracts structured memories from sessions — decisions, facts, preferences, context, and tasks.

- **Trigger**: Auto-triggered after session parsing
- **Config**: `ai.memory_extraction = true`
- **SSE events**: `ai:memory:start`, `ai:memory:complete`, `ai:memory:error`
- **Memory types**: `decision`, `fact`, `preference`, `context`, `task`

See [Memory System](memory-system.md) for details on memory types and lifecycle.

### Skills Discovery

Discovers reusable patterns and workflows from coding sessions.

- **Trigger**: Auto-triggered after session parsing
- **Config**: `ai.skills_discovery = true`
- **SSE events**: `ai:skill:start`, `ai:skill:complete`, `ai:skill:error`

### Marker Detection

Identifies significant moments in sessions: breakthroughs, shipped features, decisions, bugs found, and stuck points.

- **Trigger**: Auto-triggered after session parsing
- **Config**: `ai.marker_detection = true`
- **Marker types**: `breakthrough`, `ship`, `decision`, `bug`, `stuck`
- **SSE events**: `ai:markers:start`, `ai:markers:complete`, `ai:markers:error`

## Background Scheduler

When AI is enabled, yocore runs periodic background tasks:

| Task | Default Interval | Config Section | Description |
|------|-----------------|----------------|-------------|
| Memory ranking | 6 hours | `[scheduler.ranking]` | Promotes accessed memories, demotes stale ones |
| Embedding refresh | 12 hours | `[scheduler.embedding_refresh]` | Backfills missing vector embeddings |
| Duplicate cleanup | 24 hours | `[scheduler.duplicate_cleanup]` | Removes similar memories (opt-in, `enabled = false` by default) |
| Skill cleanup | 24 hours | `[scheduler.skill_cleanup]` | Removes similar skills (opt-in, `enabled = false` by default) |

## Concurrency

AI tasks run via a semaphore-controlled task queue to prevent overwhelming the AI provider. Multiple sessions can trigger AI tasks simultaneously, but they're processed with controlled concurrency.

## Manual Triggers

You can manually trigger AI tasks via the API:

```bash
# Generate title
curl -X POST http://localhost:19420/api/ai/sessions/<id>/title

# Extract memories
curl -X POST http://localhost:19420/api/ai/sessions/<id>/memories

# Extract skills
curl -X POST http://localhost:19420/api/ai/sessions/<id>/skills

# Detect markers
curl -X POST http://localhost:19420/api/ai/sessions/<id>/markers

# Rank memories for a project
curl -X POST http://localhost:19420/api/projects/<id>/rank-memories
```

## Monitoring

Subscribe to SSE events to monitor AI task progress:

```bash
curl -N http://localhost:19420/api/events
```

Check AI CLI status:

```bash
curl http://localhost:19420/api/ai/cli/status
```
