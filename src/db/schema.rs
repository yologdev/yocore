//! SQLite schema for Yolog Core
//!
//! Manages projects, sessions, memories, and skills.
//! This schema is compatible with the Desktop app schema.

use rusqlite::{Connection, Result};

/// Initialize the database with required tables
pub fn init_db(conn: &Connection) -> Result<()> {
    // Enable foreign key enforcement for CASCADE deletes
    conn.execute("PRAGMA foreign_keys = ON", [])?;

    // Projects table
    conn.execute(
        "CREATE TABLE IF NOT EXISTS projects (
            id TEXT PRIMARY KEY,
            name TEXT NOT NULL,
            folder_path TEXT NOT NULL UNIQUE,
            description TEXT,
            repo_url TEXT,
            language TEXT,
            framework TEXT,
            auto_sync BOOLEAN NOT NULL DEFAULT 1,
            longest_streak INTEGER NOT NULL DEFAULT 0,
            created_at TEXT NOT NULL,
            updated_at TEXT NOT NULL
        )",
        [],
    )?;

    // Sessions table - indexed metadata from JSONL files
    conn.execute(
        "CREATE TABLE IF NOT EXISTS sessions (
            id TEXT PRIMARY KEY,
            project_id TEXT NOT NULL,
            file_path TEXT NOT NULL UNIQUE,
            title TEXT,
            ai_tool TEXT NOT NULL,
            message_count INTEGER NOT NULL DEFAULT 0,
            duration_ms INTEGER,
            has_code BOOLEAN NOT NULL DEFAULT 0,
            has_errors BOOLEAN NOT NULL DEFAULT 0,
            file_size INTEGER,
            file_modified TEXT,
            archived_file_path TEXT,
            archived_at TEXT,
            title_edited BOOLEAN NOT NULL DEFAULT 0,
            title_ai_generated BOOLEAN NOT NULL DEFAULT 0,
            memories_extracted_at TEXT,
            memories_extracted_count INTEGER DEFAULT 0,
            skills_extracted_at TEXT,
            skills_extracted_count INTEGER DEFAULT 0,
            import_status TEXT DEFAULT 'success' CHECK (import_status IN ('success', 'failed')),
            import_error TEXT,
            is_hidden BOOLEAN NOT NULL DEFAULT 0,
            created_at TEXT NOT NULL,
            indexed_at TEXT NOT NULL,
            FOREIGN KEY (project_id) REFERENCES projects(id) ON DELETE CASCADE
        )",
        [],
    )?;

    // Session messages index - for fast preview without parsing full file
    conn.execute(
        "CREATE TABLE IF NOT EXISTS session_messages (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            session_id TEXT NOT NULL,
            sequence_num INTEGER NOT NULL,
            role TEXT NOT NULL,
            content_preview TEXT,
            search_content TEXT,
            has_code BOOLEAN NOT NULL DEFAULT 0,
            has_error BOOLEAN NOT NULL DEFAULT 0,
            has_file_changes BOOLEAN NOT NULL DEFAULT 0,
            tool_name TEXT,
            tool_type TEXT,
            tool_summary TEXT,
            byte_offset INTEGER NOT NULL DEFAULT 0,
            byte_length INTEGER NOT NULL DEFAULT 0,
            input_tokens INTEGER,
            output_tokens INTEGER,
            cache_read_tokens INTEGER,
            cache_creation_tokens INTEGER,
            model TEXT,
            timestamp TEXT NOT NULL,
            FOREIGN KEY (session_id) REFERENCES sessions(id) ON DELETE CASCADE,
            UNIQUE(session_id, sequence_num)
        )",
        [],
    )?;

    // Memories table for AI Memory Layer
    conn.execute(
        "CREATE TABLE IF NOT EXISTS memories (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            project_id TEXT NOT NULL,
            session_id TEXT NOT NULL,
            memory_type TEXT NOT NULL CHECK (
                memory_type IN ('decision', 'fact', 'preference', 'context', 'task')
            ),
            title TEXT NOT NULL,
            content TEXT NOT NULL,
            context TEXT,
            tags TEXT NOT NULL DEFAULT '[]',
            confidence REAL NOT NULL DEFAULT 0.5,
            is_validated BOOLEAN NOT NULL DEFAULT 0,
            extracted_at TEXT NOT NULL,
            file_reference TEXT,
            state TEXT NOT NULL DEFAULT 'new' CHECK (state IN ('new', 'low', 'high', 'removed')),
            access_count INTEGER NOT NULL DEFAULT 0,
            last_accessed_at TEXT,
            FOREIGN KEY (project_id) REFERENCES projects(id) ON DELETE CASCADE,
            FOREIGN KEY (session_id) REFERENCES sessions(id) ON DELETE CASCADE
        )",
        [],
    )?;

    // Memory embeddings table for vector search
    conn.execute(
        "CREATE TABLE IF NOT EXISTS memory_embeddings (
            memory_id INTEGER PRIMARY KEY,
            embedding BLOB NOT NULL,
            FOREIGN KEY (memory_id) REFERENCES memories(id) ON DELETE CASCADE
        )",
        [],
    )?;

    // Memory settings table (singleton)
    conn.execute(
        "CREATE TABLE IF NOT EXISTS memory_settings (
            id INTEGER PRIMARY KEY CHECK (id = 1),
            model_downloaded BOOLEAN NOT NULL DEFAULT 0,
            model_path TEXT,
            last_extraction TEXT,
            total_memories_extracted INTEGER NOT NULL DEFAULT 0
        )",
        [],
    )?;

    // Initialize memory settings if not exists
    conn.execute(
        "INSERT OR IGNORE INTO memory_settings (id, model_downloaded, total_memories_extracted)
         VALUES (1, 0, 0)",
        [],
    )?;

    // Skills table
    conn.execute(
        "CREATE TABLE IF NOT EXISTS skills (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            project_id TEXT NOT NULL,
            session_id TEXT NOT NULL,
            name TEXT NOT NULL,
            description TEXT NOT NULL,
            steps TEXT NOT NULL DEFAULT '[]',
            confidence REAL NOT NULL DEFAULT 0.5,
            extracted_at TEXT NOT NULL,
            FOREIGN KEY (project_id) REFERENCES projects(id) ON DELETE CASCADE,
            FOREIGN KEY (session_id) REFERENCES sessions(id) ON DELETE CASCADE
        )",
        [],
    )?;

    // Skill embeddings table
    conn.execute(
        "CREATE TABLE IF NOT EXISTS skill_embeddings (
            skill_id INTEGER PRIMARY KEY,
            embedding BLOB NOT NULL,
            FOREIGN KEY (skill_id) REFERENCES skills(id) ON DELETE CASCADE
        )",
        [],
    )?;

    // Skill-session linking table
    conn.execute(
        "CREATE TABLE IF NOT EXISTS skill_sessions (
            skill_id INTEGER NOT NULL,
            session_id TEXT NOT NULL,
            added_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
            PRIMARY KEY (skill_id, session_id),
            FOREIGN KEY (skill_id) REFERENCES skills(id) ON DELETE CASCADE,
            FOREIGN KEY (session_id) REFERENCES sessions(id) ON DELETE CASCADE
        )",
        [],
    )?;

    // Session markers table
    conn.execute(
        "CREATE TABLE IF NOT EXISTS session_markers (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            session_id TEXT NOT NULL,
            event_index INTEGER NOT NULL,
            marker_type TEXT NOT NULL CHECK (
                marker_type IN ('breakthrough', 'ship', 'decision', 'bug', 'stuck')
            ),
            label TEXT NOT NULL,
            description TEXT,
            created_at TEXT NOT NULL,
            FOREIGN KEY (session_id) REFERENCES sessions(id) ON DELETE CASCADE
        )",
        [],
    )?;

    // Session context table for lifeboat pattern
    conn.execute(
        "CREATE TABLE IF NOT EXISTS session_context (
            session_id TEXT PRIMARY KEY,
            project_id TEXT NOT NULL,
            active_task TEXT,
            recent_decisions TEXT NOT NULL DEFAULT '[]',
            open_questions TEXT NOT NULL DEFAULT '[]',
            resume_context TEXT,
            source TEXT NOT NULL DEFAULT 'startup',
            created_at TEXT NOT NULL,
            updated_at TEXT NOT NULL,
            FOREIGN KEY (project_id) REFERENCES projects(id) ON DELETE CASCADE
        )",
        [],
    )?;

    // Migrations for existing databases
    run_migrations(conn)?;

    // Create indexes
    create_indexes(conn)?;

    // Initialize FTS5 tables
    init_fts_tables(conn)?;

    Ok(())
}

/// Run schema migrations for existing databases
fn run_migrations(conn: &Connection) -> Result<()> {
    // Add title_ai_generated column if missing (existing DBs won't have it)
    let has_column: bool = conn
        .prepare("SELECT COUNT(*) FROM pragma_table_info('sessions') WHERE name = 'title_ai_generated'")?
        .query_row([], |row| row.get::<_, i64>(0))
        .map(|count| count > 0)?;

    if !has_column {
        conn.execute(
            "ALTER TABLE sessions ADD COLUMN title_ai_generated BOOLEAN NOT NULL DEFAULT 0",
            [],
        )?;
    }

    // Yolo mode: all projects always sync (auto_sync = 1)
    conn.execute(
        "UPDATE projects SET auto_sync = 1 WHERE auto_sync = 0",
        [],
    )?;

    Ok(())
}

/// Create database indexes for query performance
fn create_indexes(conn: &Connection) -> Result<()> {
    // Project indexes
    conn.execute(
        "CREATE INDEX IF NOT EXISTS idx_sessions_project ON sessions(project_id)",
        [],
    )?;

    conn.execute(
        "CREATE INDEX IF NOT EXISTS idx_sessions_created ON sessions(created_at DESC)",
        [],
    )?;

    // Message indexes
    conn.execute(
        "CREATE INDEX IF NOT EXISTS idx_messages_session ON session_messages(session_id, sequence_num)",
        [],
    )?;

    conn.execute(
        "CREATE INDEX IF NOT EXISTS idx_messages_timestamp ON session_messages(session_id, timestamp)",
        [],
    )?;

    conn.execute(
        "CREATE INDEX IF NOT EXISTS idx_messages_has_error ON session_messages(session_id, has_error) WHERE has_error = 1",
        [],
    )?;

    conn.execute(
        "CREATE INDEX IF NOT EXISTS idx_messages_model ON session_messages(session_id, model) WHERE model IS NOT NULL",
        [],
    )?;

    conn.execute(
        "CREATE INDEX IF NOT EXISTS idx_messages_role_tool ON session_messages(session_id, role, tool_type)",
        [],
    )?;

    // Memory indexes
    conn.execute(
        "CREATE INDEX IF NOT EXISTS idx_memories_project ON memories(project_id)",
        [],
    )?;

    conn.execute(
        "CREATE INDEX IF NOT EXISTS idx_memories_session ON memories(session_id)",
        [],
    )?;

    conn.execute(
        "CREATE INDEX IF NOT EXISTS idx_memories_type ON memories(memory_type)",
        [],
    )?;

    conn.execute(
        "CREATE INDEX IF NOT EXISTS idx_memories_confidence ON memories(confidence DESC)",
        [],
    )?;

    conn.execute(
        "CREATE INDEX IF NOT EXISTS idx_memories_state ON memories(project_id, state, confidence DESC)",
        [],
    )?;

    // Skill indexes
    conn.execute(
        "CREATE INDEX IF NOT EXISTS idx_skills_project ON skills(project_id)",
        [],
    )?;

    conn.execute(
        "CREATE INDEX IF NOT EXISTS idx_skills_session ON skills(session_id)",
        [],
    )?;

    conn.execute(
        "CREATE INDEX IF NOT EXISTS idx_skills_confidence ON skills(confidence DESC)",
        [],
    )?;

    conn.execute(
        "CREATE INDEX IF NOT EXISTS idx_skill_sessions_skill ON skill_sessions(skill_id)",
        [],
    )?;

    // Session context index
    conn.execute(
        "CREATE INDEX IF NOT EXISTS idx_session_context_project ON session_context(project_id, updated_at DESC)",
        [],
    )?;

    // Session markers index
    conn.execute(
        "CREATE INDEX IF NOT EXISTS idx_markers_session ON session_markers(session_id, event_index)",
        [],
    )?;

    Ok(())
}

/// Initialize FTS5 virtual tables for full-text search
fn init_fts_tables(conn: &Connection) -> Result<()> {
    // Check if FTS5 tables already exist
    let messages_fts_exists: bool = conn
        .prepare(
            "SELECT name FROM sqlite_master WHERE type='table' AND name='session_messages_fts'",
        )
        .and_then(|mut stmt| stmt.query_row([], |_| Ok(true)))
        .unwrap_or(false);

    if !messages_fts_exists {
        // FTS5 for session messages
        conn.execute(
            "CREATE VIRTUAL TABLE session_messages_fts USING fts5(
                search_content,
                content='session_messages', content_rowid='id',
                tokenize='porter unicode61'
            )",
            [],
        )?;

        // Triggers for session_messages_fts
        conn.execute(
            "CREATE TRIGGER session_messages_fts_ai AFTER INSERT ON session_messages BEGIN
                INSERT INTO session_messages_fts(rowid, search_content)
                VALUES (new.id, new.search_content);
            END",
            [],
        )?;

        conn.execute(
            "CREATE TRIGGER session_messages_fts_ad AFTER DELETE ON session_messages BEGIN
                INSERT INTO session_messages_fts(session_messages_fts, rowid, search_content)
                VALUES ('delete', old.id, old.search_content);
            END",
            [],
        )?;

        conn.execute(
            "CREATE TRIGGER session_messages_fts_au AFTER UPDATE ON session_messages BEGIN
                INSERT INTO session_messages_fts(session_messages_fts, rowid, search_content)
                VALUES ('delete', old.id, old.search_content);
                INSERT INTO session_messages_fts(rowid, search_content)
                VALUES (new.id, new.search_content);
            END",
            [],
        )?;
    }

    // Check if memories FTS exists
    let memories_fts_exists: bool = conn
        .prepare("SELECT name FROM sqlite_master WHERE type='table' AND name='memories_fts'")
        .and_then(|mut stmt| stmt.query_row([], |_| Ok(true)))
        .unwrap_or(false);

    if !memories_fts_exists {
        // FTS5 for memories
        conn.execute(
            "CREATE VIRTUAL TABLE memories_fts USING fts5(
                title, content, context, tags,
                content='memories', content_rowid='id',
                tokenize='porter unicode61'
            )",
            [],
        )?;

        // Triggers for memories_fts
        conn.execute(
            "CREATE TRIGGER memories_ai AFTER INSERT ON memories BEGIN
                INSERT INTO memories_fts(rowid, title, content, context, tags)
                VALUES (new.id, new.title, new.content, new.context, new.tags);
            END",
            [],
        )?;

        conn.execute(
            "CREATE TRIGGER memories_ad AFTER DELETE ON memories BEGIN
                INSERT INTO memories_fts(memories_fts, rowid, title, content, context, tags)
                VALUES ('delete', old.id, old.title, old.content, old.context, old.tags);
            END",
            [],
        )?;

        conn.execute(
            "CREATE TRIGGER memories_au AFTER UPDATE ON memories BEGIN
                INSERT INTO memories_fts(memories_fts, rowid, title, content, context, tags)
                VALUES ('delete', old.id, old.title, old.content, old.context, old.tags);
                INSERT INTO memories_fts(rowid, title, content, context, tags)
                VALUES (new.id, new.title, new.content, new.context, new.tags);
            END",
            [],
        )?;
    }

    // Check if skills FTS exists
    let skills_fts_exists: bool = conn
        .prepare("SELECT name FROM sqlite_master WHERE type='table' AND name='skills_fts'")
        .and_then(|mut stmt| stmt.query_row([], |_| Ok(true)))
        .unwrap_or(false);

    if !skills_fts_exists {
        // FTS5 for skills
        conn.execute(
            "CREATE VIRTUAL TABLE skills_fts USING fts5(
                name, description, steps,
                content='skills', content_rowid='id',
                tokenize='porter unicode61'
            )",
            [],
        )?;

        // Triggers for skills_fts
        conn.execute(
            "CREATE TRIGGER skills_ai AFTER INSERT ON skills BEGIN
                INSERT INTO skills_fts(rowid, name, description, steps)
                VALUES (new.id, new.name, new.description, new.steps);
            END",
            [],
        )?;

        conn.execute(
            "CREATE TRIGGER skills_ad AFTER DELETE ON skills BEGIN
                INSERT INTO skills_fts(skills_fts, rowid, name, description, steps)
                VALUES ('delete', old.id, old.name, old.description, old.steps);
            END",
            [],
        )?;

        conn.execute(
            "CREATE TRIGGER skills_au AFTER UPDATE ON skills BEGIN
                INSERT INTO skills_fts(skills_fts, rowid, name, description, steps)
                VALUES ('delete', old.id, old.name, old.description, old.steps);
                INSERT INTO skills_fts(rowid, name, description, steps)
                VALUES (new.id, new.name, new.description, new.steps);
            END",
            [],
        )?;
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_init_db() {
        let conn = Connection::open_in_memory().unwrap();
        assert!(init_db(&conn).is_ok());

        // Verify tables exist
        let table_count: i32 = conn
            .query_row(
                "SELECT COUNT(*) FROM sqlite_master WHERE type='table'",
                [],
                |row| row.get(0),
            )
            .unwrap();

        assert!(
            table_count >= 10,
            "Expected at least 10 tables, got {}",
            table_count
        );
    }
}
