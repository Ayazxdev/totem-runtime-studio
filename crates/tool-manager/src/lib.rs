//! tool-manager — Tool registration, verification, invocation.
//!
//! Registry, local tools, HTTP/REST, gRPC, GraphQL, tool versioning, and cryptographic attestation.

pub mod a2a_dispatch;
pub mod attestation;
pub mod graphql;
pub mod http;
pub mod local;
pub mod registry;

use async_trait::async_trait;
use sentinel_core::{
    error::SentinelResult,
    pipeline::PipelineStage,
    types::{AgentTask, ExecutionContext},
};
use std::sync::Arc;

pub struct ToolManagerStage {
    registry: Arc<registry::ToolRegistry>,
}

impl ToolManagerStage {
    pub fn new() -> Self {
        Self {
            registry: Arc::new(registry::ToolRegistry::new()),
        }
    }

    /// Create a stage sharing an existing registry (allows composition root to share one registry).
    pub fn with_registry(registry: Arc<registry::ToolRegistry>) -> Self {
        Self { registry }
    }

    pub fn registry(&self) -> &registry::ToolRegistry {
        &self.registry
    }

    pub fn registry_arc(&self) -> Arc<registry::ToolRegistry> {
        Arc::clone(&self.registry)
    }
}

impl Default for ToolManagerStage {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl PipelineStage for ToolManagerStage {
    fn name(&self) -> &str {
        "ToolManager"
    }

    async fn process(
        &self,
        task: &AgentTask,
        context: &mut ExecutionContext,
    ) -> SentinelResult<()> {
        tracing::info!("ToolManager: processing task {}", task.id);

        for action in &task.actions {
            // MCP tools are handled by the MCP stage — skip them here
            if is_mcp_tool(&action.tool_name) {
                tracing::debug!("Skipping MCP tool: {} (handled by MCP stage)", action.tool_name);
                continue;
            }

            let start = std::time::Instant::now();
            let result = self.registry.invoke_tool(&action.tool_name, &action.parameters).await?;
            let latency_ms = start.elapsed().as_millis() as u64;

            context.tool_results.push(sentinel_core::types::ToolResult {
                tool_name: action.tool_name.clone(),
                action_id: action.id,
                output: result,
                latency_ms,
                timestamp: chrono::Utc::now(),
            });
        }
        Ok(())
    }
}

/// Returns true if the tool is served by the MCP stage.
fn is_mcp_tool(tool_name: &str) -> bool {
    matches!(tool_name, "read_file" | "list_files" | "write_file")
}
