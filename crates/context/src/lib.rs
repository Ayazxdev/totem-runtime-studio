//! context — Context Management Layer.
//!
//! Sliding-window truncation, token counting, and salience-weighted eviction.
//! Messages and tool results scored by role, content length, keyword signals, and recency.
//!          content length, keyword signals, and recency; lowest-scoring items evicted first.

pub mod scorer;
pub mod window;

use async_trait::async_trait;
use sentinel_core::{
    error::SentinelResult,
    pipeline::PipelineStage,
    types::{AgentTask, ExecutionContext},
};

pub struct ContextStage {
    max_context_size: usize,
}

impl ContextStage {
    pub fn new(max_context_size: usize) -> Self {
        Self { max_context_size }
    }
}

impl Default for ContextStage {
    fn default() -> Self {
        Self::new(8000) // 8k token default limit
    }
}

#[async_trait]
impl PipelineStage for ContextStage {
    fn name(&self) -> &str {
        "Context"
    }

    async fn process(
        &self,
        task: &AgentTask,
        context: &mut ExecutionContext,
    ) -> SentinelResult<()> {
        tracing::debug!("Context: compressing context for task {}", task.id);
        // compression
        window::compress_context(context, self.max_context_size);
        Ok(())
    }
}
