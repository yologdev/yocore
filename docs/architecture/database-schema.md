# Database Schema

Yocore uses SQLite with WAL mode. The database is stored at the configured `data_dir` path (default: `~/.yolog/`).

## Tables

### `projects`

Project metadata.

| Column | Type | Description |
|--------|------|-------------|
| `id` | TEXT PK | Project ID |
| `name` | TEXT | Project name |
| `folder_path` | TEXT UNIQUE | Filesystem path |
| `description` | TEXT | Optional description |
| `repo_url` | TEXT | Git repository URL |
| `language` | TEXT | Primary language |
| `framework` | TEXT | Primary framework |
| `auto_sync` | BOOLEAN | Auto-sync enabled (default: true) |
| `longest_streak` | INTEGER | Longest coding streak |
| `created_at` | TEXT | ISO 8601 timestamp |
| `updated_at` | TEXT | ISO 8601 timestamp |

### `sessions`

Session metadata from parsed JSONL files.

| Column | Type | Description |
|--------|------|-------------|
| `id` | TEXT PK | Session ID |
| `project_id` | TEXT FK | Parent project |
| `file_path` | TEXT UNIQUE | JSONL file path |
| `title` | TEXT | Session title |
| `ai_tool` | TEXT | AI tool name (e.g., `claude_code`) |
| `message_count` | INTEGER | Number of messages |
| `duration_ms` | INTEGER | Session duration |
| `has_code` | BOOLEAN | Contains code |
| `has_errors` | BOOLEAN | Contains errors |
| `file_size` | INTEGER | File size (for incremental parsing) |
| `memories_extracted_at` | TEXT | Last memory extraction time |
| `skills_extracted_at` | TEXT | Last skill extraction time |
| `created_at` | TEXT | Session start time |
| `indexed_at` | TEXT | Last indexing time |

### `session_messages`

Message index for fast preview without parsing the full JSONL file.

| Column | Type | Description |
|--------|------|-------------|
| `id` | INTEGER PK | Auto-increment |
| `session_id` | TEXT FK | Parent session |
| `sequence_num` | INTEGER | Message order (unique per session) |
| `role` | TEXT | `human`, `assistant`, `tool` |
| `content_preview` | TEXT | Truncated content |
| `search_content` | TEXT | Full text for FTS |
| `has_code` | BOOLEAN | Contains code blocks |
| `has_error` | BOOLEAN | Contains error output |
| `tool_name` | TEXT | Tool name (for tool messages) |
| `byte_offset` | INTEGER | Position in JSONL file |
| `byte_length` | INTEGER | Length in JSONL file |
| `input_tokens` | INTEGER | Input token count |
| `output_tokens` | INTEGER | Output token count |
| `model` | TEXT | Model used |
| `timestamp` | TEXT | Message timestamp |

### `memories`

Extracted memories from AI sessions.

| Column | Type | Description |
|--------|------|-------------|
| `id` | INTEGER PK | Auto-increment |
| `project_id` | TEXT FK | Parent project |
| `session_id` | TEXT FK | Source session |
| `memory_type` | TEXT | `decision`, `fact`, `preference`, `context`, `task` |
| `title` | TEXT | Short title |
| `content` | TEXT | Full content |
| `context` | TEXT | Additional context |
| `tags` | TEXT | JSON array of tags |
| `confidence` | REAL | AI confidence (0.0 - 1.0) |
| `is_validated` | BOOLEAN | Manually validated |
| `state` | TEXT | `new`, `low`, `high`, `removed` |
| `access_count` | INTEGER | Access count (for ranking) |
| `extracted_at` | TEXT | Extraction timestamp |

### `memory_embeddings`

Vector embeddings for semantic search (384-dimensional, all-MiniLM-L6-v2).

| Column | Type | Description |
|--------|------|-------------|
| `memory_id` | INTEGER PK FK | Memory reference |
| `embedding` | BLOB | 384-dim float32 vector |

### `skills`

Discovered skills from coding sessions.

| Column | Type | Description |
|--------|------|-------------|
| `id` | INTEGER PK | Auto-increment |
| `project_id` | TEXT FK | Parent project |
| `session_id` | TEXT FK | Source session |
| `name` | TEXT | Skill name |
| `description` | TEXT | What it does |
| `steps` | TEXT | JSON array of steps |
| `confidence` | REAL | AI confidence |
| `extracted_at` | TEXT | Extraction timestamp |

### `skill_embeddings`

Vector embeddings for skills.

| Column | Type | Description |
|--------|------|-------------|
| `skill_id` | INTEGER PK FK | Skill reference |
| `embedding` | BLOB | 384-dim float32 vector |

### `skill_sessions`

Many-to-many link between skills and sessions.

| Column | Type | Description |
|--------|------|-------------|
| `skill_id` | INTEGER FK | Skill reference |
| `session_id` | TEXT FK | Session reference |
| `added_at` | TEXT | Link timestamp |

### `session_markers`

Significant moments in sessions.

| Column | Type | Description |
|--------|------|-------------|
| `id` | INTEGER PK | Auto-increment |
| `session_id` | TEXT FK | Parent session |
| `event_index` | INTEGER | Position in session |
| `marker_type` | TEXT | `breakthrough`, `ship`, `decision`, `bug`, `stuck` |
| `label` | TEXT | Short label |
| `description` | TEXT | Detailed description |
| `created_at` | TEXT | Timestamp |

### `session_context`

Lifeboat pattern — session state for context preservation.

| Column | Type | Description |
|--------|------|-------------|
| `session_id` | TEXT PK | Session reference |
| `project_id` | TEXT FK | Parent project |
| `active_task` | TEXT | Current task description |
| `recent_decisions` | TEXT | JSON array of decisions |
| `open_questions` | TEXT | JSON array of questions |
| `resume_context` | TEXT | Context for resume |
| `source` | TEXT | How context was created |
| `created_at` | TEXT | Timestamp |
| `updated_at` | TEXT | Timestamp |

### `instance_metadata`

Singleton table for persistent instance identity.

| Column | Type | Description |
|--------|------|-------------|
| `id` | INTEGER PK | Always 1 |
| `uuid` | TEXT | Persistent instance UUID |
| `instance_name` | TEXT | Custom display name |
| `created_at` | TEXT | Timestamp |

## FTS5 Tables

Three full-text search virtual tables auto-synced via triggers:

| FTS Table | Source Table | Indexed Columns |
|-----------|-------------|-----------------|
| `session_messages_fts` | `session_messages` | `search_content` |
| `memories_fts` | `memories` | `title`, `content` |
| `skills_fts` | `skills` | `name`, `description` |

Changes to source tables automatically propagate to FTS tables via INSERT/UPDATE/DELETE triggers.

## Key Indexes

- `idx_sessions_project` — Sessions by project
- `idx_sessions_created` — Sessions by creation time
- `idx_messages_session` — Messages by session
- `idx_messages_timestamp` — Messages by time
- `idx_memories_project` — Memories by project
- `idx_memories_state` — Memories by ranking state
- `idx_memories_confidence` — Memories by confidence score
- `idx_skills_project` — Skills by project
