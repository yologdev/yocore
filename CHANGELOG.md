# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/),
and this project adheres to [Semantic Versioning](https://semver.org/).

## [Unreleased]

### Added
- Instance nickname support in mDNS TXT records and `/health` endpoint
- Pre-commit hook for `cargo fmt` check

## [0.1.0] - 2026-02-02

### Added
- Session watching with incremental JSONL parsing (Claude Code format)
- SQLite storage with dual read/write connections and WAL mode
- Full-text search via FTS5 with auto-sync triggers
- REST API with ~57 endpoints for sessions, projects, memories, skills, search, and config
- MCP server with 5 tools for AI assistant integration
- AI features: title generation, memory extraction, skill discovery, marker detection, ranking
- Local embedding model (all-MiniLM-L6-v2, 384-dim) for hybrid keyword + semantic search
- mDNS/Bonjour service discovery for LAN auto-discovery
- Lifeboat pattern for session context preservation across context compaction
- Background scheduler for memory ranking, duplicate cleanup, embedding refresh
- npm distribution (`@yologdev/core`) with cross-platform binary selection
- Multi-platform builds: macOS (x64, ARM64), Linux (x64), Windows (x64)
