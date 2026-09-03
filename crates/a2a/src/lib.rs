//! a2a — Agent2Agent (A2A) Protocol integration.
//!
//! Agent Cards, HTTP discovery, task delegation, capability advertisement.
//! Implements the client side of Google Agent2Agent Protocol v1.0 / Linux Foundation spec.

pub mod agent_card;
pub mod client;
pub mod stub;

pub use agent_card::AgentCard;
pub use client::A2aClient;
pub use stub::{TaskStore, A2aTask, TaskState, TaskStatusUpdate, result_to_artifact};

use async_trait::async_trait;
use sentinel_core::{
    error::SentinelResult,
    pipeline::PipelineStage,
    types::{ActionType, AgentTask, ExecutionContext},
};

/// Pipeline stage for A2A inter-agent communication.
/// For actions with `ActionType::AgentCommunication`, delegates to the remote agent.
pub struct A2aStage;

#[async_trait]
impl PipelineStage for A2aStage {
    fn name(&self) -> &str {
        "A2A"
    }

    async fn process(
        &self,
        task: &AgentTask,
        context: &mut ExecutionContext,
    ) -> SentinelResult<()> {
        for action in &task.actions {
            // Only handle AgentCommunication actions here; ToolCalls go to ToolManager
            if !matches!(action.action_type, ActionType::AgentCommunication) {
                continue;
            }

            // Expect an `agent_endpoint` parameter for delegation
            let endpoint = action
                .parameters
                .get("agent_endpoint")
                .and_then(|v| v.as_str())
                .unwrap_or("");

            if endpoint.is_empty() {
                tracing::warn!(
                    "A2A: action {} has AgentCommunication type but no agent_endpoint parameter — skipping",
                    action.id
                );
                continue;
            }

            tracing::info!("A2A: delegating tool '{}' to agent at {}", action.tool_name, endpoint);

            let client = A2aClient::new(endpoint);
            let params = serde_json::Value::Object(
                action.parameters.iter().map(|(k, v)| (k.clone(), v.clone())).collect(),
            );

            match client.send_and_wait(&action.tool_name, &params, 6).await {
                Ok(result) => {
                    tracing::info!(
                        "A2A: delegation succeeded — task {} status: {}",
                        result.task_id,
                        result.status
                    );
                    context.tool_results.push(sentinel_core::types::ToolResult {
                        tool_name: action.tool_name.clone(),
                        action_id: action.id,
                        output: result.output,
                        latency_ms: 0,
                        timestamp: chrono::Utc::now(),
                    });
                }
                Err(e) => {
                    tracing::warn!("A2A: delegation to {} failed: {}", endpoint, e);
                    // Don't propagate — log and continue; other stages handle fallback
                    context.metadata.insert(
                        format!("a2a_error_{}", action.id),
                        serde_json::json!({ "error": e.to_string(), "endpoint": endpoint }),
                    );
                }
            }
        }
        Ok(())
    }
}
