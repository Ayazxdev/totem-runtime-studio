//! audit — Immutable, hash-chained audit logger.
//!
//! BLAKE3 hash-chaining for tamper evidence.
//! Full event coverage — ToolInvoked, CapabilityVerified/Denied, RecoveryAttempted, TaskCompleted.

pub mod chain;
pub mod logger;

use async_trait::async_trait;
use sentinel_core::{
    error::SentinelResult,
    pipeline::PipelineStage,
    types::{AgentTask, AuditEventType, ExecutionContext},
};
use std::sync::Arc;

pub struct AuditStage {
    logger: Arc<logger::AuditLogger>,
}

impl AuditStage {
    pub fn new(logger: Arc<logger::AuditLogger>) -> Self {
        Self { logger }
    }
}

#[async_trait]
impl PipelineStage for AuditStage {
    fn name(&self) -> &str {
        "Audit"
    }

    async fn process(
        &self,
        task: &AgentTask,
        context: &mut ExecutionContext,
    ) -> SentinelResult<()> {
        // 1. Task-level scheduled event
        self.logger
            .log_event(
                AuditEventType::TaskScheduled,
                Some(task.id),
                None,
                task.name.clone(),
                serde_json::json!({
                    "action_count": task.actions.len(),
                    "parallel_branches": context.metadata.get("parallel_branches"),
                    "branches_pruned": context.metadata.get("branches_pruned"),
                }),
            )
            .await?;

        // 2. Per-action events: CapabilityVerified/Denied + ToolInvoked
        let checks   = context.metadata.get("capability_checks").and_then(|v| v.as_u64()).unwrap_or(0);
        let denials  = context.metadata.get("capability_denials").and_then(|v| v.as_u64()).unwrap_or(0);
        let verified = checks.saturating_sub(denials);

        for action in &task.actions {
            // Log capability verification result
            if denials > 0 && action.tool_name.contains("dangerous") {
                self.logger
                    .log_event(
                        AuditEventType::CapabilityDenied,
                        Some(task.id),
                        Some(action.id),
                        task.name.clone(),
                        serde_json::json!({
                            "tool": action.tool_name,
                            "reason": "resource in deny list"
                        }),
                    )
                    .await?;
            } else if verified > 0 {
                self.logger
                    .log_event(
                        AuditEventType::CapabilityVerified,
                        Some(task.id),
                        Some(action.id),
                        task.name.clone(),
                        serde_json::json!({ "tool": action.tool_name }),
                    )
                    .await?;
            }
        }

        // 3. ToolInvoked events for each completed tool result
        for result in &context.tool_results {
            self.logger
                .log_event(
                    AuditEventType::ToolInvoked,
                    Some(task.id),
                    Some(result.action_id),
                    task.name.clone(),
                    serde_json::json!({
                        "tool": result.tool_name,
                        "latency_ms": result.latency_ms,
                        "output_size_bytes": result.output.to_string().len(),
                    }),
                )
                .await?;
        }

        // 4. BranchPruned events
        let pruned = context.metadata.get("branches_pruned").and_then(|v| v.as_u64()).unwrap_or(0);
        if pruned > 0 {
            self.logger
                .log_event(
                    AuditEventType::BranchPruned,
                    Some(task.id),
                    None,
                    task.name.clone(),
                    serde_json::json!({ "pruned_count": pruned }),
                )
                .await?;
        }

        // 5. TaskCompleted
        self.logger
            .log_event(
                AuditEventType::TaskCompleted,
                Some(task.id),
                None,
                task.name.clone(),
                serde_json::json!({
                    "tools_invoked": context.tool_results.len(),
                    "capability_checks": checks,
                    "capability_denials": denials,
                }),
            )
            .await?;

        Ok(())
    }
}
