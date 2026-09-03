//! recovery — Recovery Manager.
//!
//! Retry-with-backoff, checkpoint/restore, recovery audit events.

pub mod checkpoint;
pub mod retry;

use async_trait::async_trait;
use audit::logger::AuditLogger;
use sentinel_core::{
    error::SentinelResult,
    pipeline::PipelineStage,
    types::{AgentTask, AuditEventType, ExecutionContext},
};
use std::sync::Arc;

pub struct RecoveryStage {
    audit: Arc<AuditLogger>,
    checkpoint_store: Arc<checkpoint::CheckpointStore>,
}

impl RecoveryStage {
    pub fn new(audit: Arc<AuditLogger>) -> Self {
        Self {
            audit,
            checkpoint_store: Arc::new(checkpoint::CheckpointStore::default()),
        }
    }
}

#[async_trait]
impl PipelineStage for RecoveryStage {
    fn name(&self) -> &str {
        "Recovery"
    }

    async fn process(
        &self,
        task: &AgentTask,
        context: &mut ExecutionContext,
    ) -> SentinelResult<()> {
        // Save a checkpoint of the post-execution context
        match checkpoint::Checkpoint::capture(task.id, context) {
            Ok(cp) => {
                tracing::debug!("Recovery: checkpoint {} saved for task {}", cp.id, task.id);
                self.checkpoint_store.save(cp);
            }
            Err(e) => {
                tracing::warn!("Recovery: checkpoint capture failed (non-fatal): {e}");
            }
        }

        // Read recovery attempt counter (set by external retry logic if present)
        let attempts = context.metadata
            .get("recovery_attempts")
            .and_then(|v| v.as_u64())
            .unwrap_or(0) as u32;

        if attempts > 0 {
            tracing::info!("Recovery: task {} completed after {} recovery attempt(s)", task.id, attempts);
            self.audit
                .log_event(
                    AuditEventType::RecoveryAttempted,
                    Some(task.id),
                    None,
                    task.name.clone(),
                    serde_json::json!({
                        "attempts": attempts,
                        "checkpoints_saved": self.checkpoint_store.count(),
                    }),
                )
                .await?;
        }

        Ok(())
    }
}
