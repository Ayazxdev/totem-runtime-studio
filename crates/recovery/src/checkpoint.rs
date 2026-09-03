//! Checkpoint/restore for transactional recovery.
//!
//! A Checkpoint captures the full ExecutionContext at a point in time,
//! tagged with a BLAKE3 hash for integrity verification.
//! On branch failure, the context can be restored to the last good checkpoint.

use chrono::{DateTime, Utc};
use sentinel_core::{
    error::{SentinelError, SentinelResult},
    types::ExecutionContext,
};
use uuid::Uuid;

#[derive(Debug, Clone)]
pub struct Checkpoint {
    pub id: Uuid,
    pub task_id: Uuid,
    pub created_at: DateTime<Utc>,
    pub context_snapshot: ExecutionContext,
    /// BLAKE3 hash of the serialised context (integrity check on restore).
    pub hash: String,
}

impl Checkpoint {
    /// Capture the current execution context as a checkpoint.
    pub fn capture(task_id: Uuid, context: &ExecutionContext) -> SentinelResult<Self> {
        let serialised = serde_json::to_string(context)
            .map_err(|e| SentinelError::Internal(format!("Checkpoint serialisation failed: {e}")))?;
        let hash = hex::encode(blake3::hash(serialised.as_bytes()).as_bytes());

        Ok(Self {
            id: Uuid::new_v4(),
            task_id,
            created_at: Utc::now(),
            context_snapshot: context.clone(),
            hash,
        })
    }

    /// Restore context from this checkpoint, verifying integrity first.
    pub fn restore(&self) -> SentinelResult<ExecutionContext> {
        // Recompute hash to verify the snapshot hasn't been tampered with in memory
        let serialised = serde_json::to_string(&self.context_snapshot)
            .map_err(|e| SentinelError::Internal(format!("Checkpoint re-serialisation failed: {e}")))?;
        let recomputed = hex::encode(blake3::hash(serialised.as_bytes()).as_bytes());

        if recomputed != self.hash {
            return Err(SentinelError::Internal(format!(
                "Checkpoint {} integrity check failed — snapshot may be corrupted",
                self.id
            )));
        }

        tracing::info!("Checkpoint {}: context restored for task {}", self.id, self.task_id);
        Ok(self.context_snapshot.clone())
    }
}

/// In-memory checkpoint store (one per task execution).
#[derive(Default)]
pub struct CheckpointStore {
    checkpoints: std::sync::Mutex<Vec<Checkpoint>>,
}

impl CheckpointStore {
    pub fn save(&self, checkpoint: Checkpoint) {
        if let Ok(mut store) = self.checkpoints.lock() {
            tracing::debug!(
                "Checkpoint saved: {} (task {})",
                checkpoint.id, checkpoint.task_id
            );
            store.push(checkpoint);
        }
    }

    pub fn latest(&self, task_id: Uuid) -> Option<Checkpoint> {
        self.checkpoints
            .lock()
            .ok()?
            .iter()
            .filter(|c| c.task_id == task_id)
            .last()
            .cloned()
    }

    pub fn count(&self) -> usize {
        self.checkpoints.lock().map(|s| s.len()).unwrap_or(0)
    }
}
