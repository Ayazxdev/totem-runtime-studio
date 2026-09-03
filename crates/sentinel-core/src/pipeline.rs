//! Pipeline abstraction — every subsystem is a PipelineStage.
//!
//! Stage order: Scheduler → Context → CAPSEM → MCP → ToolManager → A2A → Audit → Evaluation → Recovery
//! ExecutionMetrics read from context.metadata (populated by EvaluationStage).

use crate::{
    error::SentinelResult,
    types::{AgentTask, ExecutionContext, ExecutionResult, ExecutionMetrics, ExecutionStatus},
};
use async_trait::async_trait;
use std::time::Instant;

/// A stage in the execution pipeline.
#[async_trait]
pub trait PipelineStage: Send + Sync {
    fn name(&self) -> &str;

    async fn process(
        &self,
        task: &AgentTask,
        context: &mut ExecutionContext,
    ) -> SentinelResult<()>;
}

/// The full execution pipeline — wires all stages together.
pub struct ExecutionPipeline {
    stages: Vec<Box<dyn PipelineStage>>,
}

impl ExecutionPipeline {
    pub fn new() -> Self {
        Self { stages: Vec::new() }
    }

    pub fn add_stage(&mut self, stage: Box<dyn PipelineStage>) {
        self.stages.push(stage);
    }

    /// Execute a task through all pipeline stages in order.
    /// Collects real metrics written into context.metadata by EvaluationStage.
    pub async fn execute(&self, task: AgentTask) -> SentinelResult<ExecutionResult> {
        let pipeline_start = Instant::now();
        let mut context = task.context.clone();

        for stage in &self.stages {
            tracing::debug!("Pipeline → {}", stage.name());
            stage.process(&task, &mut context).await?;
        }

        let wall_clock_ms = pipeline_start.elapsed().as_millis() as u64;

        // Read metrics written by EvaluationStage into context.metadata
        let metrics = build_metrics_from_context(&context, wall_clock_ms);

        Ok(ExecutionResult {
            task_id: task.id,
            status: ExecutionStatus::Success,
            output: serde_json::json!({
                "message": "Task executed successfully",
                "tool_results": context.tool_results.iter().map(|r| serde_json::json!({
                    "tool": r.tool_name,
                    "output": r.output,
                    "latency_ms": r.latency_ms
                })).collect::<Vec<_>>()
            }),
            metrics,
            audit_trail_id: uuid::Uuid::new_v4(),
        })
    }
}

/// Pull all metrics fields from context.metadata (written by EvaluationStage).
fn build_metrics_from_context(context: &ExecutionContext, wall_clock_ms: u64) -> ExecutionMetrics {
    let get_u64 = |key: &str| -> u64 {
        context.metadata.get(key)
            .and_then(|v| v.as_u64())
            .unwrap_or(0)
    };
    let get_f64 = |key: &str| -> f64 {
        context.metadata.get(key)
            .and_then(|v| v.as_f64())
            .unwrap_or(0.0)
    };
    let get_u32 = |key: &str| -> u32 {
        context.metadata.get(key)
            .and_then(|v| v.as_u64())
            .unwrap_or(0) as u32
    };

    // Prefer EvaluationStage latency; fall back to wall_clock from pipeline itself
    let eval_latency = get_u64("latency_ms");
    let final_latency = if eval_latency > 0 { eval_latency } else { wall_clock_ms };

    ExecutionMetrics {
        total_latency_ms: final_latency,
        parallel_branches_executed: get_u32("parallel_branches"),
        branches_pruned: get_u32("branches_pruned"),
        total_token_count: get_u64("token_count"),
        estimated_cost_usd: get_f64("estimated_cost_usd"),
        tools_invoked: context.tool_results.len() as u32,
        capability_checks: get_u32("capability_checks"),
        capability_denials: get_u32("capability_denials"),
        recovery_attempts: get_u32("recovery_attempts"),
    }
}

impl Default for ExecutionPipeline {
    fn default() -> Self {
        Self::new()
    }
}
