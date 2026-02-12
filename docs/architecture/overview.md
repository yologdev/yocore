# Architecture Overview

## Data Flow

```
File change → notify debouncer → tokio channel → spawned parse task
  → incremental/full parse → SQLite + FTS5 triggers OR EphemeralIndex (in-memory)
  → broadcast WatcherEvent via SSE
  → auto-trigger AI tasks (title in both modes; memory/skills/markers in DB mode only)
  → AI results stored with embeddings (DB) or in-memory (ephemeral)
```

## Modules

```
src/
├── lib.rs          Core orchestrator — owns all subsystems
├── main.rs         CLI entry point (clap)
├── config.rs       TOML config loading and defaults
├── error.rs        Error types
├── watcher/        File system watcher (notify crate) + SessionStore dispatch
├── parser/         Trait-based JSONL parsing
├── db/             SQLite with dual read/write connections
├── ephemeral/      In-memory storage (EphemeralIndex) — alternative to SQLite
├── api/            Axum REST server (~50 routes)
├── mcp/            Stdio JSON-RPC MCP server
├── ai/             AI features (title, memory, skills, markers, ranking)
├── embeddings/     Local all-MiniLM-L6-v2 model (candle)
├── handlers/       Shared business logic (API + MCP)
├── mdns.rs         mDNS/Bonjour service discovery
└── scheduler/      Background periodic tasks (DB mode only)
```

### Core (`lib.rs`)

The `Core` struct owns all subsystems: config, database, file watcher, event broadcasting, and AI task queue. It coordinates startup: watcher → scheduler → API server.

### Watcher

Uses the `notify` crate for file system events. Detects changes to session files and delegates to `store.rs` (`SessionStore` enum) for incremental parsing. `SessionStore` dispatches to DB or `EphemeralIndex` based on config. Each file event spawns an independent tokio task to prevent starvation.

### Parser

Trait-based (`SessionParser`) JSONL parsing. Currently implements Claude Code parser. Returns `ParseResult` with events, metadata, and stats.

### Database

Dual-connection SQLite with WAL mode. Write connection used by watcher/AI; read connection used by API (never blocked). Schema includes FTS5 tables with auto-sync triggers. Only used when `storage = "db"`.

### Ephemeral Storage

In-memory alternative to SQLite (`EphemeralIndex`). Uses `RwLock<HashMap>` for projects, sessions, and messages. Message windowing keeps last N messages from full parse (default 50); incremental appends are uncapped. LRU session eviction when `max_sessions` exceeded. No persistence — all data lost on restart.

### API

Axum REST server with ~50 routes. Auth via optional Bearer token. SSE endpoint broadcasts `WatcherEvent` and `AiEvent`. Each route handles both DB and ephemeral modes with per-handler branching (DB-only features return empty results in ephemeral mode).

### MCP

Stdio JSON-RPC server implementing the Model Context Protocol. 5 tools for AI assistants to query memories, context, and skills.

### AI

AI feature modules: title generation, memory extraction, skill discovery, marker detection, ranking. Uses Claude Code CLI as subprocess. `AiTaskQueue` (semaphore) limits concurrency. Title generation works in both storage modes; other features require DB.

### Embeddings

Local all-MiniLM-L6-v2 model (384 dimensions) via the `candle` crate. Lazy-loaded on first use (`OnceLock`). Powers hybrid search combining FTS5 keyword matching with cosine similarity.

### Scheduler

Background tasks: memory ranking, duplicate cleanup, embedding refresh, skill cleanup. Staggered intervals, feature-flag gated. DB mode only.

## Key Design Patterns

### Dual Storage Backends

`SessionStore` enum dispatches to SQLite or `EphemeralIndex` based on the `storage` config value. Core initializes one or the other at startup — never both. API routes handle both modes with per-handler branching.

```
Config: storage = "db"         → SQLite (full-featured, persistent)
Config: storage = "ephemeral"  → EphemeralIndex (in-memory, volatile)
```

### Dual SQLite Connections (DB mode)

Separate read and write connections prevent API queries from blocking during file parses and AI writes. WAL (Write-Ahead Logging) mode enables concurrent reads while a write is in progress.

```
Writer (watcher, AI)  ──→  SQLite (WAL)  ←──  Reader (API)
```

### Message Windowing (Ephemeral mode)

Full parses keep only the last N messages in memory (default 50) to bound memory usage. Incremental appends (small deltas) are uncapped. Older messages are still readable from the JSONL file via byte offsets.

### Incremental Parsing

Tracks `file_size` and `max_sequence` per session. When a file grows, only new bytes are parsed. On truncation (rare), a full re-parse is triggered.

### Lifeboat Pattern

Before context compaction in AI assistants, session context (active task, recent decisions, open questions) is saved to the `session_context` table. This allows seamless resume in new sessions.

### Event Broadcasting

`tokio::sync::broadcast` channels for both watcher and AI events. Multiple SSE clients can subscribe simultaneously. Events are typed enums converted to JSON for SSE.

## Technology Stack

| Component | Technology |
|-----------|-----------|
| Language | Rust |
| HTTP framework | Axum |
| Database | SQLite (rusqlite) |
| Full-text search | FTS5 |
| Embeddings | candle (all-MiniLM-L6-v2) |
| File watching | notify |
| mDNS | mdns-sd |
| CLI | clap |
| Async runtime | tokio |
| Serialization | serde + serde_json |
