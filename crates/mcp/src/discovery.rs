//! MCP tool and resource discovery.

use crate::client::McpClient;
use sentinel_core::error::SentinelResult;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpTool {
    pub name: String,
    pub description: String,
    pub input_schema: serde_json::Value,
}

impl McpClient {
    /// Discover available tools from the MCP server.
    pub async fn discover_tools(&self) -> SentinelResult<Vec<McpTool>> {
        let result = self.call("tools/list", serde_json::json!({})).await?;
        
        let tools: Vec<McpTool> = serde_json::from_value(result)
            .unwrap_or_else(|_| Vec::new());

        tracing::info!("Discovered {} MCP tools", tools.len());
        Ok(tools)
    }
}
