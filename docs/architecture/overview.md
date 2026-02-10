# Architecture Overview

## Data Flow

```
File change → notify debouncer → tokio channel → spawned parse task
  → incremental/full parse → SQLite + FTS5 triggers
  → broadcast WatcherEvent via SSE
  → auto-trigger AI tasks (title, memory, skills extraction)
  → AI results stored with embeddings
```

## Modules

```
src/
├── lib.rs          Core orchestrator — owns all subsystems
├── main.rs         CLI entry point (clap)
├── config.rs       TOML config loading and defaults
├── error.rs        Error types
├── watcher/        File system watcher (notify crate)
├── parser/         Trait-based JSONL parsing
├── db/             SQLite with dual read/write connections
├── api/            Axum REST server (~50 routes)
├── mcp/            Stdio JSON-RPC MCP server
├── ai/             AI features (title, memory, skills, markers, ranking)
├── embeddings/     Local all-MiniLM-L6-v2 model (candle)
├── handlers/       Shared business logic (API + MCP)
├── mdns.rs         mDNS/Bonjour service discovery
└── scheduler/      Background periodic tasks
```

### Core (`lib.rs`)

The `Core` struct owns all subsystems: config, database, file watcher, event broadcasting, and AI task queue. It coordinates startup: watcher → scheduler → API server.

### Watcher

Uses the `notify` crate for file system events. Detects changes to session files and delegates to `storage.rs` for incremental parsing. Each file event spawns an independent tokio task to prevent starvation.

### Parser

Trait-based (`SessionParser`) JSONL parsing. Currently implements Claude Code parser. Returns `ParseResult` with events, metadata, and stats.

### Database

Dual-connection SQLite with WAL mode. Write connection used by watcher/AI; read connection used by API (never blocked). Schema includes FTS5 tables with auto-sync triggers.

### API

Axum REST server with ~50 routes. Auth via optional Bearer token. SSE endpoint broadcasts `WatcherEvent` and `AiEvent`.

### MCP

Stdio JSON-RPC server implementing the Model Context Protocol. 5 tools for AI assistants to query memories, context, and skills.

### AI

AI feature modules: title generation, memory extraction, skill discovery, marker detection, ranking. Uses Claude Code CLI as subprocess. `AiTaskQueue` (semaphore) limits concurrency.

### Embeddings

Local all-MiniLM-L6-v2 model (384 dimensions) via the `candle` crate. Lazy-loaded on first use (`OnceLock`). Powers hybrid search combining FTS5 keyword matching with cosine similarity.

### Scheduler

Background tasks: memory ranking, duplicate cleanup, embedding refresh, skill cleanup. Staggered intervals, feature-flag gated.

## Key Design Patterns

### Dual SQLite Connections

Separate read and write connections prevent API queries from blocking during file parses and AI writes. WAL (Write-Ahead Logging) mode enables concurrent reads while a write is in progress.

```
Writer (watcher, AI)  ──→  SQLite (WAL)  ←──  Reader (API)
```

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
