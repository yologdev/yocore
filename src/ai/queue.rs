//! AI Task Queue
//!
//! Limits concurrent AI operations to prevent resource exhaustion.
//! Uses a semaphore-based queue with configurable concurrency limit.

use std::sync::Arc;
use tokio::sync::{OwnedSemaphorePermit, Semaphore};

/// Task queue for limiting concurrent AI operations
#[derive(Clone)]
pub struct AiTaskQueue {
    semaphore: Arc<Semaphore>,
    max_concurrent: usize,
}

impl AiTaskQueue {
    /// Create a new task queue with the specified concurrency limit
    pub fn new(max_concurrent: usize) -> Self {
        Self {
            semaphore: Arc::new(Semaphore::new(max_concurrent)),
            max_concurrent,
        }
    }

    /// Acquire a permit to run an AI task
    ///
    /// This will block if the maximum number of concurrent tasks is reached.
    /// The permit is automatically released when dropped.
    pub async fn acquire(&self) -> Result<OwnedSemaphorePermit, String> {
        self.semaphore
            .clone()
            .acquire_owned()
            .await
            .map_err(|e| format!("Failed to acquire AI task permit: {}", e))
    }

    /// Get the number of available permits
    pub fn available_permits(&self) -> usize {
        self.semaphore.available_permits()
    }

    /// Get the maximum concurrent tasks
    pub fn max_concurrent(&self) -> usize {
        self.max_concurrent
    }
}

impl Default for AiTaskQueue {
    fn default() -> Self {
        // Default to 3 concurrent AI tasks
        Self::new(3)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_queue_limits_concurrency() {
        let queue = AiTaskQueue::new(2);

        // Acquire two permits
        let _permit1 = queue.acquire().await.unwrap();
        let _permit2 = queue.acquire().await.unwrap();

        // Should have no available permits
        assert_eq!(queue.available_permits(), 0);

        // Drop one permit
        drop(_permit1);

        // Should have one available permit
        assert_eq!(queue.available_permits(), 1);
    }
}
