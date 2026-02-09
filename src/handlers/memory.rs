//! Memory handling logic

use crate::db::Database;
use std::sync::Arc;

/// Memory handler for business logic
pub struct MemoryHandler {
    #[allow(dead_code)]
    db: Arc<Database>,
}

impl MemoryHandler {
    pub fn new(db: Arc<Database>) -> Self {
        MemoryHandler { db }
    }

    // TODO: Add memory handling methods
    // - extract_memories
    // - search_memories
    // - update_memory_state
}
