//! mcp — Model Context Protocol integration.
//!
//! JSON-RPC/SSE MCP client, tool discovery, tool execution.
//! Resource discovery, sampling, and prompts are out of scope for the MVP.

pub mod client;
pub mod discovery;
pub mod executor;

use async_trait::async_trait;
use sentinel_core::{
    error::SentinelResult,
    pipeline::PipelineStage,
    types::{AgentTask, ExecutionContext},
};
use std::sync::{Arc, Mutex};

pub struct McpStage {
    /// Cached MCP server clients per endpoint
    servers: Arc<Mutex<std::collections::HashMap<String, client::McpClient>>>,
}

impl Clone for McpStage {
    fn clone(&self) -> Self {
        Self {
            servers: Arc::clone(&self.servers),
        }
    }
}

impl McpStage {
    pub fn new() -> Self {
        Self {
            servers: Arc::new(Mutex::new(std::collections::HashMap::new())),
        }
    }

    /// Register an MCP server endpoint
    pub fn register_server(&self, name: String, endpoint: String) -> SentinelResult<()> {
        let client = client::McpClient::new(endpoint);
        self.servers
            .lock()
            .map_err(|e| sentinel_core::error::SentinelError::Mcp(format!("Lock error: {}", e)))?
            .insert(name, client);
        Ok(())
    }

    /// Get client for a specific server
    fn get_client(&self, server_name: &str) -> SentinelResult<client::McpClient> {
        self.servers
            .lock()
            .map_err(|e| sentinel_core::error::SentinelError::Mcp(format!("Lock error: {}", e)))?
            .get(server_name)
            .cloned()
            .ok_or_else(|| {
                sentinel_core::error::SentinelError::Mcp(format!("Server {} not found", server_name))
            })
    }
}

impl Default for McpStage {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl PipelineStage for McpStage {
    fn name(&self) -> &str {
        "MCP"
    }

    async fn process(
        &self,
        task: &AgentTask,
        context: &mut ExecutionContext,
    ) -> SentinelResult<()> {
        tracing::info!("MCP: processing task {}", task.id);

        // Execute MCP tools
        for action in &task.actions {
            // Check if this is an MCP tool
            if let Some(mcp_server) = get_mcp_server_for_tool(&action.tool_name) {
                tracing::debug!("Executing MCP tool: {} via server: {}", action.tool_name, mcp_server);

                let client = self.get_client(&mcp_server)?;
                let result = client
                    .execute_tool(&action.tool_name, &action.parameters)
                    .await?;

                context.tool_results.push(sentinel_core::types::ToolResult {
                    tool_name: action.tool_name.clone(),
                    action_id: action.id,
                    output: result,
                    latency_ms: 0,
                    timestamp: chrono::Utc::now(),
                });
            }
        }

        Ok(())
    }
}

/// Map tool names to their MCP server.
fn get_mcp_server_for_tool(tool_name: &str) -> Option<String> {
    match tool_name {
        "read_file" | "list_files" | "write_file" => Some("filesystem".to_string()),
        _ => None,
    }
}
