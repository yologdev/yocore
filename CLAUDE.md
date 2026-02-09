# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Build & Development Commands

```bash
cargo build                    # Debug build
cargo build --release          # Optimized release build (LTO enabled)
cargo test                     # Run all tests
cargo test <test_name>         # Run a single test
cargo fmt                      # Format code (CI enforces this)
cargo fmt -- --check           # Check formatting without changing files
cargo clippy -- -D warnings    # Lint (CI enforces zero warnings)
```

Run the server:
```bash
cargo run                      # HTTP server on 127.0.0.1:19420
cargo run -- --mcp             # MCP stdio server (JSON-RPC)
cargo run -- --port 8080       # Custom port
cargo run -- --verbose         # Enable debug logging
cargo run -- --init            # Create default config and exit
```

## Architecture Overview

**yocore** is a headless service that watches AI session files (Claude Code JSONL), parses them incrementally, stores structured data in SQLite, and exposes it via REST API and MCP server.

### Core (`lib.rs`) — Central Orchestrator

`Core` struct owns all subsystems: config, database, file watcher, event broadcasting, and AI task queue. It coordinates startup: watcher → scheduler → API server.

### Data Flow

```
File change → notify debouncer → tokio channel → spawned parse task
  → incremental/full parse → SQLite + FTS5 triggers
  → broadcast WatcherEvent via SSE
  → auto-trigger AI tasks (title, memory, skills extraction)
  → AI results stored with embeddings
```

### Module Relationships

- **`watcher/`** — File system watcher using `notify`. Detects changes, delegates to `storage.rs` for incremental parsing. Each file event spawns an independent tokio task to prevent starvation.
- **`parser/`** — Trait-based (`SessionParser`) JSONL parsing. Currently implements Claude Code parser. Returns `ParseResult` with events, metadata, and stats.
- **`db/`** — Dual-connection SQLite (read + write) with WAL mode. Write connection used by watcher/AI; read connection used by API (never blocked). Schema in `schema.rs` includes FTS5 tables with auto-sync triggers. Migrations handled in `run_migrations()`.
- **`api/`** — Axum REST server (~40 routes). Auth via optional Bearer token. SSE endpoint broadcasts `WatcherEvent` and `AiEvent`.
- **`mcp/`** — Stdio JSON-RPC server implementing Model Context Protocol. 7 tools for AI assistants to query memories, context, and skills.
- **`ai/`** — AI feature modules (title generation, memory extraction, skill discovery, marker detection, ranking). Uses Claude Code CLI as subprocess. `AiTaskQueue` (semaphore) limits concurrency.
- **`embeddings/`** — Local all-MiniLM-L6-v2 model (384-dim) via candle. Lazy-loaded on first use (`OnceLock`). Powers hybrid search (FTS5 keyword + cosine similarity).
- **`scheduler/`** — Background tasks: memory ranking, duplicate cleanup, embedding refresh, skill cleanup. Staggered intervals, feature-flag gated.
- **`handlers/`** — Shared business logic used by both API routes and MCP handlers.

### Key Design Patterns

- **Dual SQLite connections**: Separate read/write prevents API queries from blocking during file parses. WAL mode enables concurrent reads.
- **Incremental parsing**: Tracks `file_size` + `max_sequence` per session. Only parses new bytes on file growth; full re-parse on truncation.
- **Lifeboat pattern**: Saves session context (`active_task`, `recent_decisions`, `open_questions`) before context compaction for seamless resume. Stored in `session_context` table.
- **Event broadcasting**: `tokio::sync::broadcast` channels for both watcher and AI events, consumed by SSE clients.

### Configuration

TOML config at `~/.yolog/config.toml`. Key env vars: `YOLOG_DATA_DIR`, `YOLOG_SERVER_PORT`, `YOLOG_SERVER_HOST`, `YOLOG_SERVER_API_KEY`, `ANTHROPIC_API_KEY`. Config is also editable via REST API (`/api/config`).

### Database Schema

10+ tables in SQLite: `projects`, `sessions`, `session_messages`, `memories`, `memory_embeddings`, `skills`, `skill_embeddings`, `skill_sessions`, `session_markers`, `session_context`. Three FTS5 tables (`session_messages_fts`, `memories_fts`, `skills_fts`) auto-synced via triggers.
