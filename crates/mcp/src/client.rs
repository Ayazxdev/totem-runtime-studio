//! MCP JSON-RPC client.
//!
//! basic JSON-RPC over HTTP/SSE.

use sentinel_core::error::{SentinelError, SentinelResult};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone)]
pub struct McpClient {
    endpoint: String,
}

impl McpClient {
    pub fn new(endpoint: String) -> Self {
        Self { endpoint }
    }

    /// Send a JSON-RPC request to the MCP server.
    pub async fn call(
        &self,
        method: &str,
        params: serde_json::Value,
    ) -> SentinelResult<serde_json::Value> {
        let request = JsonRpcRequest {
            jsonrpc: "2.0".to_string(),
            id: uuid::Uuid::new_v4().to_string(),
            method: method.to_string(),
            params,
        };

        tracing::debug!("MCP call: {} to {}", method, self.endpoint);

        let client = reqwest::Client::new();
        let response = client
            .post(&self.endpoint)
            .json(&request)
            .send()
            .await
            .map_err(|e| SentinelError::Mcp(format!("HTTP request failed: {}", e)))?;

        let rpc_response: JsonRpcResponse = response
            .json()
            .await
            .map_err(|e| SentinelError::Mcp(format!("JSON decode failed: {}", e)))?;

        if let Some(error) = rpc_response.error {
            return Err(SentinelError::Mcp(format!(
                "MCP error {}: {}",
                error.code, error.message
            )));
        }

        rpc_response
            .result
            .ok_or_else(|| SentinelError::Mcp("No result in MCP response".to_string()))
    }
}

#[derive(Debug, Serialize)]
struct JsonRpcRequest {
    jsonrpc: String,
    id: String,
    method: String,
    params: serde_json::Value,
}

#[derive(Debug, Deserialize)]
struct JsonRpcResponse {
    #[allow(dead_code)]
    jsonrpc: String,
    #[allow(dead_code)]
    id: String,
    result: Option<serde_json::Value>,
    error: Option<JsonRpcError>,
}

#[derive(Debug, Deserialize)]
struct JsonRpcError {
    code: i32,
    message: String,
}
