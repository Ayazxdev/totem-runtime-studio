//! MCP tool execution.

use crate::client::McpClient;
use sentinel_core::error::SentinelResult;
use serde_json::Value;
use std::collections::HashMap;

impl McpClient {
    /// Execute an MCP tool.
    pub async fn execute_tool(
        &self,
        tool_name: &str,
        arguments: &HashMap<String, Value>,
    ) -> SentinelResult<Value> {
        tracing::debug!("Executing MCP tool: {}", tool_name);

        let params = serde_json::json!({
            "name": tool_name,
            "arguments": arguments,
        });

        self.call("tools/call", params).await
    }
}
