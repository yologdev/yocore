# Memory System Deep Dive

Yocore's memory system extracts, organizes, ranks, and serves knowledge from AI coding sessions. This page explains how each piece works internally.

## How It Works

```
Session file change
  → Incremental parse (only new bytes)
  → AI memory extraction (Claude Code CLI)
  → Dedup check (Jaccard similarity)
  → Store in SQLite + generate embeddings
  → Available via search, MCP, and yoskill
  → Background ranking promotes/demotes over time
```

Each step is automatic. Once AI features are enabled, memories flow from session transcripts into a searchable knowledge base without manual intervention.

## Memory Types

| Type | What it captures | Example |
|------|-----------------|---------|
| `decision` | Architectural or design choices with reasoning | "Use JWT for auth instead of sessions — stateless scales better" |
| `fact` | Discovered information about the codebase | "The database uses WAL mode for concurrent read/write" |
| `preference` | User or project conventions | "Always use bun instead of npm" |
| `context` | Background constraints and domain knowledge | "The API must maintain backward compatibility with v1 clients" |
| `task` | Work items and follow-ups | "Add pagination to the search endpoint" |

## Memory Extraction

When a session file grows, yocore parses the new content and (if AI is enabled) runs memory extraction.

### Quality Controls

The extraction pipeline prioritizes precision over recall:

| Control | Value | Purpose |
|---------|-------|---------|
| Min messages | 25 | Skip short sessions with insufficient context |
| Max input | 150,000 chars | Prevent token overflow |
| Min confidence | 0.70 | Only store memories the AI is confident about |
| Max per chunk | 10-15 | Force quality over quantity |

### What Gets Extracted

The AI is instructed to focus on genuinely actionable knowledge:

- Choices made with reasoning (why this approach over alternatives)
- Learned discoveries and how things work
- User preferences and workflow patterns
- Background context that constrains future work
- Action items and follow-ups

It explicitly skips routine operations, secrets, generic knowledge, and temporary workarounds.

### Extraction-Time Dedup

Before storing a new memory, yocore checks it against existing memories using [Jaccard similarity](#duplicate-cleanup) with a threshold of 0.65. If a near-duplicate already exists, the new memory is discarded. This prevents the same insight from being stored multiple times across incremental parses.

## Hybrid Search

Yocore combines two search engines for best results:

### 1. FTS5 Keyword Search

SQLite full-text search across memory titles and content. Fast, exact, and great for specific terms:

```bash
curl -X POST http://localhost:19420/api/memories/search \
  -H "Content-Type: application/json" \
  -d '{"query": "database migration", "project_id": "<id>"}'
```

### 2. Semantic Search (Vector Embeddings)

The local **all-MiniLM-L6-v2** model generates 384-dimensional embeddings for each memory. Queries are embedded the same way, and results are ranked by cosine similarity.

This catches conceptually similar results even when the exact words differ — searching for "auth flow" finds memories about "login process" and "JWT token handling".

The embedding model is loaded lazily on first use (`OnceLock`), so there's no startup cost if you don't use search.

### Reciprocal Rank Fusion (RRF)

Both result sets are merged using RRF, which combines rankings rather than raw scores:

```
RRF_score(d) = 1/(k + rank_fts(d)) + 1/(k + rank_semantic(d))
```

This produces results that are both keyword-relevant and semantically meaningful.

### Search via yoskill

```
/yo memory-search database migration strategy
/yo memory-search tag:security
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

## Memory Ranking

A background job evaluates memories periodically (every 6 hours by default) and transitions their state based on a weighted score.

### Score Calculation

Each memory gets a score from 0.0 to 1.0:

| Factor | Weight | Calculation |
|--------|--------|-------------|
| Access frequency | 35% | `min(access_count / 10, 1.0)` |
| AI confidence | 25% | Original extraction confidence (0.0–1.0) |
| Recency | 25% | Linear decay: `max(1.0 - days_since_access / 90, 0.0)` |
| Validation | 15% | 1.0 if manually validated, 0.0 otherwise |

A memory accessed 5 times with 0.8 confidence, accessed yesterday, and validated would score:

```
0.35 * min(5/10, 1.0) + 0.25 * 0.8 + 0.25 * (1 - 1/90) + 0.15 * 1.0
= 0.35 * 0.5 + 0.25 * 0.8 + 0.25 * 0.989 + 0.15 * 1.0
= 0.175 + 0.2 + 0.247 + 0.15
= 0.772
```

### State Transitions

```
         ┌────────────────────────────┐
         │                            v
new ──→ high ──→ low          removed
 │                                ^
 │                                │
 └──→ low ──→ high                │
       │                          │
       └──────────────────────────┘
```

| Transition | Condition |
|-----------|-----------|
| new → high | Score >= 0.7 **and** accessed >= 3 times |
| new → low | Score < 0.4 after 14 days |
| new → removed | Score < 0.3 after 30 days with 0 accesses |
| low → high | Score >= 0.6 **and** accessed >= 5 times (comeback) |
| high → low | Score < 0.4, stale for 90+ days, **not** validated |

**Validated memories are protected from demotion.** Once you mark a memory as validated (via `/yo update <id> state=validated` or the API), it stays in its current state regardless of access patterns.

### Why This Works

The ranking system is deliberately conservative:

- **New memories get time** — 14 days before any demotion, 30 before removal
- **Access patterns matter most** (35%) — memories that get retrieved are inherently valuable
- **Comebacks are possible** — a low-ranked memory can climb back to high if it starts being used
- **Manual override** — validated memories are immune to automated demotion
- **Soft removal** — removed memories stay in the database, just excluded from search results

## Duplicate Cleanup

Over time, similar memories may accumulate (e.g., from working on the same feature across multiple sessions). A background task periodically scans for and removes near-duplicates.

### How It Works

1. Memories are tokenized using a hybrid approach:
   - **Latin text**: Lowercase, split into words (min 2 chars), simple suffix stemming (`-ing`, `-ed`, `-s`, `-es`, `-ly`)
   - **CJK text**: Character bigrams (sliding window of 2)
   - **Mixed text**: Both approaches combined

2. Similarity is calculated as weighted Jaccard similarity:
   - Title similarity (60%) + Content similarity (40%)
   - Jaccard = |intersection| / |union| of token sets

3. Two thresholds operate at different stages:

| Stage | Threshold | Purpose |
|-------|-----------|---------|
| Extraction-time | 0.65 | Prevent storing duplicates at insert |
| Background cleanup | 0.75 | Stricter retroactive scan (fewer false positives) |

4. When duplicates are found, the **older memory is kept** and the newer one is soft-removed. This preserves the established memory.

## Project Context

Project context aggregates all active memories for a project into a structured overview, grouped by type:

- Decisions made and their reasoning
- Known facts about the codebase
- Established preferences and conventions
- Active tasks and follow-ups

Access via:
- `/yo project` — Formatted overview for AI assistants
- `yolog_get_project_context` MCP tool — Structured JSON
- `GET /api/projects/<id>/context` — Raw API

## Session Context & Lifeboat

The lifeboat pattern preserves session state across context compaction.

### What Gets Saved

Before the AI assistant's context window is compressed, the PreCompact hook saves:
- **Active task** — What you're currently working on
- **Recent decisions** — Choices made in this session
- **Open questions** — Unresolved items that need follow-up

### How It Flows

1. You're working in a session; the context window fills up
2. The AI client triggers context compaction
3. **PreCompact hook** fires → saves lifeboat to `session_context` table
4. Context is compressed
5. **SessionStart hook** fires (or `/yo context`) → restores lifeboat + relevant memories
6. The AI assistant continues with full context of what was happening

This makes context compaction nearly seamless — no more "what were we doing?" after a compaction event.

## Full-Text Session Search

Beyond memory search, yocore indexes all session messages in an FTS5 table. This lets you search across raw transcripts:

```
/yo project-search "when did we add the pagination endpoint"
```

This searches the actual conversation text (not just extracted memories), useful for finding specific discussions, error messages, or code snippets from past sessions.

## Real-Time Events (SSE)

The desktop app and other clients can subscribe to real-time events via Server-Sent Events:

```
GET /api/events
```

Memory-related events include:

| Event | Description |
|-------|-------------|
| `ai_memory_start` | Memory extraction began for a session |
| `ai_memory_complete` | Extraction finished (includes count) |
| `ai_memory_error` | Extraction failed |
| `ranking_start` | Ranking job started for a project |
| `ranking_complete` | Ranking finished (promoted/demoted/removed counts) |
| `heartbeat` | Connection keepalive |

## Background Scheduler

Four periodic tasks maintain the memory system:

| Task | Default Interval | What It Does |
|------|-----------------|--------------|
| Ranking | 6 hours | Evaluate and transition memory states |
| Duplicate cleanup | 24 hours | Find and soft-remove near-duplicate memories |
| Embedding refresh | 12 hours | Backfill embeddings for memories missing them |
| Skill cleanup | 24 hours | Deduplicate extracted skills |

Each task:
- Runs in its own tokio task with independent timers
- Has a per-project timeout (60–120 seconds)
- Is feature-flag gated (requires `ai.enabled` + specific feature flag)
- Emits SSE events for progress tracking

Configure intervals and thresholds in your config:

```toml
[ai.features]
memory_extraction = true

[scheduler.ranking]
enabled = true
interval_hours = 6
batch_size = 500

[scheduler.duplicate_cleanup]
enabled = true
interval_hours = 24
similarity_threshold = 0.75
batch_size = 500

[scheduler.embedding_refresh]
enabled = true
interval_hours = 12

[scheduler.skill_cleanup]
enabled = true
interval_hours = 24
```

## Using with yoskill

For day-to-day use, [yoskill](https://github.com/yologdev/yoskill) commands are the primary interface:

| Command | When to Use |
|---------|-------------|
| `/yo context` | At session start — loads project context + relevant memories |
| `/yo memory-search <query>` | Before implementing features — check past decisions |
| `/yo memory-search tag:<name>` | Filter by tag (e.g., `tag:bug`, `tag:architecture`) |
| `/yo project` | Before working in unfamiliar areas — get project overview |
| `/yo project-search <query>` | Find past discussions in raw transcripts |
| `/yo memories` | Review what was extracted from the current session |
| `/yo update <id> state=high` | Promote an important memory |
| `/yo delete <id>` | Remove an incorrect memory |
| `/yo tags` | Browse available memory tags |
| `/yo status` | Verify yocore is running and connected |

The SessionStart and PreCompact hooks handle context flow automatically — you don't need to manually save or restore state. See [Long-Term Memory setup](../getting-started/long-term-memory.md) for installation.
