//! evaluation — Runtime Evaluation metrics.
//!
//! per-run metrics (latency, token count, cost estimate).
//! all counters (capability_checks, denials, recovery) read from context.metadata.

pub mod metrics;

use async_trait::async_trait;
use sentinel_core::{
    error::SentinelResult,
    pipeline::PipelineStage,
    types::{AgentTask, ExecutionContext},
};

pub struct EvaluationStage;

impl EvaluationStage {
    pub fn new() -> Self {
        Self
    }
}

impl Default for EvaluationStage {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl PipelineStage for EvaluationStage {
    fn name(&self) -> &str {
        "Evaluation"
    }

    async fn process(
        &self,
        task: &AgentTask,
        context: &mut ExecutionContext,
    ) -> SentinelResult<()> {
        tracing::debug!("Evaluation: collecting metrics for task {}", task.id);

        let metrics = metrics::compute_metrics(task, context);

        context.metadata.insert("latency_ms".to_string(),         serde_json::json!(metrics.total_latency_ms));
        context.metadata.insert("token_count".to_string(),        serde_json::json!(metrics.total_token_count));
        context.metadata.insert("estimated_cost_usd".to_string(), serde_json::json!(metrics.estimated_cost_usd));

        Ok(())
    }
}

