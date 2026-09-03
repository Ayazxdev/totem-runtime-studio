//! HTTP/REST tool transport.

use sentinel_core::error::{SentinelError, SentinelResult};
use std::collections::HashMap;

pub async fn invoke_http_tool(
    endpoint: &str,
    parameters: &HashMap<String, serde_json::Value>,
) -> SentinelResult<serde_json::Value> {
    tracing::debug!("HTTP tool invocation: POST {} with {:?}", endpoint, parameters);

    let client = reqwest::Client::new();
    let response = client
        .post(endpoint)
        .json(parameters)
        .send()
        .await
        .map_err(|e| SentinelError::ToolInvocationFailed {
            tool: endpoint.to_string(),
            reason: format!("HTTP request failed: {}", e),
        })?;

    if !response.status().is_success() {
        return Err(SentinelError::ToolInvocationFailed {
            tool: endpoint.to_string(),
            reason: format!("HTTP status {}", response.status()),
        });
    }

    let result = response
        .json::<serde_json::Value>()
        .await
        .map_err(|e| SentinelError::ToolInvocationFailed {
            tool: endpoint.to_string(),
            reason: format!("JSON decode failed: {}", e),
        })?;

    Ok(result)
}
