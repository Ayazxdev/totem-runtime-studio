//! GraphQL tool transport.
//!
//! Executes tools exposed as GraphQL mutations/queries over HTTP.
//! Uses introspection to discover available operations.

use sentinel_core::error::{SentinelError, SentinelResult};
use std::collections::HashMap;

/// Invoke a tool exposed as a GraphQL mutation.
///
/// # Arguments
/// - `endpoint`  — GraphQL HTTP endpoint (e.g. `http://tools-server/graphql`)
/// - `operation` — GraphQL operation name (mutation or query name)
/// - `parameters` — key-value arguments mapped to GraphQL variables
pub async fn invoke_graphql_tool(
    endpoint: &str,
    operation: &str,
    parameters: &HashMap<String, serde_json::Value>,
) -> SentinelResult<serde_json::Value> {
    // Build a generic mutation that passes all parameters as variables
    // The operation name is used as the mutation/query name.
    let variables = serde_json::Value::Object(
        parameters.iter().map(|(k, v)| (k.clone(), v.clone())).collect(),
    );

    // GraphQL request body: { query: "mutation OpName($args: ...) {...}", variables: {...} }
    // For a generic transport we use an introspection-free envelope.
    let body = serde_json::json!({
        "operationName": operation,
        "query": format!("mutation {} {{ {}(input: $input) {{ result error }} }}", operation, operation),
        "variables": { "input": variables }
    });

    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(30))
        .build()
        .map_err(|e| SentinelError::ToolInvocationFailed {
            tool: operation.to_string(),
            reason: format!("Failed to build HTTP client: {e}"),
        })?;

    let response = client
        .post(endpoint)
        .header("Content-Type", "application/json")
        .json(&body)
        .send()
        .await
        .map_err(|e| SentinelError::ToolInvocationFailed {
            tool: operation.to_string(),
            reason: format!("GraphQL request failed: {e}"),
        })?;

    let status = response.status();
    let json: serde_json::Value = response.json().await.map_err(|e| {
        SentinelError::ToolInvocationFailed {
            tool: operation.to_string(),
            reason: format!("Failed to parse GraphQL response: {e}"),
        }
    })?;

    if !status.is_success() {
        return Err(SentinelError::ToolInvocationFailed {
            tool: operation.to_string(),
            reason: format!("GraphQL endpoint returned HTTP {}: {:?}", status, json),
        });
    }

    // Check for GraphQL-level errors
    if let Some(errors) = json.get("errors") {
        return Err(SentinelError::ToolInvocationFailed {
            tool: operation.to_string(),
            reason: format!("GraphQL errors: {errors}"),
        });
    }

    // Return the `data` field, or the whole response if no data key
    Ok(json.get("data").cloned().unwrap_or(json))
}
