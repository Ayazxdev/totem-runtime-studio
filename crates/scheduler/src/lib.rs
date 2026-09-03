//! scheduler — Runtime Scheduler + Parallel Execution Engine + PRM Branch Pruning.
//!
//! Parallel branch execution with timing proof and PRM-scored branch pruning.

pub mod dag;
pub mod executor;
pub mod pruner;

use async_trait::async_trait;
use sentinel_core::{
    error::SentinelResult,
    pipeline::PipelineStage,
    types::{AgentTask, ExecutionContext},
};

pub struct SchedulerStage {
    executor: executor::ParallelExecutor,
}

impl SchedulerStage {
    pub fn new() -> Self {
        Self {
            executor: executor::ParallelExecutor::new(),
        }
    }
}

impl Default for SchedulerStage {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl PipelineStage for SchedulerStage {
    fn name(&self) -> &str {
        "Scheduler"
    }

    async fn process(
        &self,
        task: &AgentTask,
        context: &mut ExecutionContext,
    ) -> SentinelResult<()> {
        tracing::info!(
            "Scheduling task: {} ({}) with {} actions",
            task.name, task.id, task.actions.len()
        );

        if task.actions.is_empty() {
            context.metadata.insert("parallel_branches".to_string(), serde_json::json!(0));
            context.metadata.insert("branches_pruned".to_string(), serde_json::json!(0));
            context.metadata.insert("parallel_execution_time_ms".to_string(), serde_json::json!(0));
            return Ok(());
        }

        // All actions at level 0 — they are independent within a single task
        let levels = vec![task.actions.iter().map(|a| a.id).collect()];

        let result = self
            .executor
            .execute_parallel_levels(levels, std::slice::from_ref(task))
            .await?;

        tracing::info!(
            "Scheduler: {} branches executed, {} pruned, {}ms wall clock",
            result.total_branches,
            result.pruned_branches,
            result.wall_clock_ms
        );

        context.metadata.insert(
            "parallel_execution_time_ms".to_string(),
            serde_json::json!(result.wall_clock_ms),
        );
        context.metadata.insert(
            "parallel_branches".to_string(),
            serde_json::json!(result.total_branches),
        );
        context.metadata.insert(
            "branches_pruned".to_string(),
            serde_json::json!(result.pruned_branches),
        );

        Ok(())
    }
}
