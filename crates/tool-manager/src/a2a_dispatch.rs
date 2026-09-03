//! A2A agent dispatch for ToolTransport::A2a.
//!
//! Delegates tool invocation to a remote A2A agent via the Agent2Agent HTTP protocol:
//!   1. POST  {agent_id}/tasks/send        — submit the task
//!   2. GET   {agent_id}/tasks/{task_id}   — poll until terminal state
//!   3. Return the artifact output or error
//!
//! Polling uses exponential backoff: 200ms, 400ms, 800ms … capped at 5s.

use sentinel_core::error::{SentinelError, SentinelResult};
use std::collections::HashMap;
use std::time::Duration;

/// Terminal states per A2A v1.0 spec.
const TERMINAL_STATES: &[&str] = &["completed", "failed", "cancelled"];

/// Maximum number of poll attempts before giving up.
const DEFAULT_MAX_POLLS: u32 = 12; // up to ~20s total with backoff

/// Delegate a tool call to a remote A2A agent and wait for completion.
///
/// # Arguments
/// - `agent_id`  — base URL of the remote agent (e.g. `http://agent-b:4000`)
/// - `tool_name` — tool to invoke on the remote agent
/// - `parameters` — input parameters as key-value map
pub async fn delegate_to_agent(
    agent_id: &str,
    tool_name: &str,
    parameters: &HashMap<String, serde_json::Value>,
) -> SentinelResult<serde_json::Value> {
    let base = agent_id.trim_end_matches('/');
    let client = build_client(tool_name)?;

    // Step 1: Send task 
    let task_id = send_task(&client, base, tool_name, parameters).await?;
    tracing::info!("A2A dispatch: task {} submitted to agent '{}'", task_id, agent_id);

    // Step 2: Poll until terminal state 
    let result = poll_until_done(&client, base, tool_name, &task_id, DEFAULT_MAX_POLLS).await?;

    tracing::info!(
        "A2A dispatch: task {} completed — tool '{}' on agent '{}'",
        task_id, tool_name, agent_id
    );
    Ok(result)
}

// Internal helpers 

fn build_client(tool_name: &str) -> SentinelResult<reqwest::Client> {
    reqwest::Client::builder()
        .timeout(Duration::from_secs(30))
        .build()
        .map_err(|e| SentinelError::ToolInvocationFailed {
            tool: tool_name.to_string(),
            reason: format!("Failed to build HTTP client: {e}"),
        })
}

/// POST /tasks/send — returns the server-assigned task_id.
async fn send_task(
    client: &reqwest::Client,
    base: &str,
    tool_name: &str,
    parameters: &HashMap<String, serde_json::Value>,
) -> SentinelResult<String> {
    let endpoint = format!("{base}/tasks/send");
    let task_id = uuid::Uuid::new_v4().to_string();

    let body = serde_json::json!({
        "id": task_id,
        "message": {
            "role": "user",
            "parts": [{
                "type": "tool_call",
                "tool_name": tool_name,
                "parameters": parameters
            }]
        },
        "metadata": {
            "sender": "urn:totem:sentinel-runtime:v1",
            "sent_at": chrono::Utc::now().to_rfc3339()
        }
    });

    let resp = client
        .post(&endpoint)
        .json(&body)
        .send()
        .await
        .map_err(|e| SentinelError::ToolInvocationFailed {
            tool: tool_name.to_string(),
            reason: format!("A2A send_task to '{endpoint}' failed: {e}"),
        })?;

    let status = resp.status();
    let json: serde_json::Value = resp.json().await.map_err(|e| {
        SentinelError::ToolInvocationFailed {
            tool: tool_name.to_string(),
            reason: format!("Failed to parse send_task response: {e}"),
        }
    })?;

    if !status.is_success() {
        return Err(SentinelError::ToolInvocationFailed {
            tool: tool_name.to_string(),
            reason: format!("Agent returned HTTP {status} on send_task: {json}"),
        });
    }

    // Server may return the task id in `id` or `task_id`
    let server_id = json.get("id")
        .or_else(|| json.get("task_id"))
        .and_then(|v| v.as_str())
        .unwrap_or(&task_id)
        .to_string();

    Ok(server_id)
}

/// GET /tasks/{task_id} — poll until the task reaches a terminal state.
/// Returns the artifact output on completion or an error on failure/cancellation.
async fn poll_until_done(
    client: &reqwest::Client,
    base: &str,
    tool_name: &str,
    task_id: &str,
    max_polls: u32,
) -> SentinelResult<serde_json::Value> {
    let poll_url = format!("{base}/tasks/{task_id}");
    let mut backoff_ms: u64 = 200;

    for attempt in 1..=max_polls {
        tokio::time::sleep(Duration::from_millis(backoff_ms)).await;
        // Exponential backoff capped at 5 seconds
        backoff_ms = (backoff_ms * 2).min(5_000);

        let resp = client
            .get(&poll_url)
            .send()
            .await
            .map_err(|e| SentinelError::ToolInvocationFailed {
                tool: tool_name.to_string(),
                reason: format!("A2A poll attempt {attempt} failed: {e}"),
            })?;

        let json: serde_json::Value = resp.json().await.map_err(|e| {
            SentinelError::ToolInvocationFailed {
                tool: tool_name.to_string(),
                reason: format!("Failed to parse poll response: {e}"),
            }
        })?;

        let state = json.get("state")
            .or_else(|| json.get("status"))
            .and_then(|v| v.as_str())
            .unwrap_or("unknown");

        tracing::debug!(
            "A2A poll {}/{}: task {} state={}",
            attempt, max_polls, task_id, state
        );

        if !TERMINAL_STATES.contains(&state) {
            continue; // still working
        }

        // Terminal: check for failure
        if state == "failed" || state == "cancelled" {
            let reason = json.get("message")
                .and_then(|v| v.as_str())
                .unwrap_or("unknown reason");
            return Err(SentinelError::ToolInvocationFailed {
                tool: tool_name.to_string(),
                reason: format!("A2A task {task_id} ended with state '{state}': {reason}"),
            });
        }

        // Completed — extract artifacts
        return extract_output(&json, tool_name, task_id);
    }

    Err(SentinelError::ToolInvocationFailed {
        tool: tool_name.to_string(),
        reason: format!(
            "A2A task {task_id} did not reach terminal state after {max_polls} polls"
        ),
    })
}

/// Extract the usable output from a completed A2A task response.
fn extract_output(
    json: &serde_json::Value,
    tool_name: &str,
    task_id: &str,
) -> SentinelResult<serde_json::Value> {
    // Try `artifacts[0].parts[0].data` first (canonical A2A structure)
    if let Some(artifacts) = json.get("artifacts").and_then(|a| a.as_array()) {
        if let Some(first) = artifacts.first() {
            if let Some(parts) = first.get("parts").and_then(|p| p.as_array()) {
                if let Some(part) = parts.first() {
                    if let Some(data) = part.get("data") {
                        return Ok(data.clone());
                    }
                    if let Some(text) = part.get("text") {
                        return Ok(text.clone());
                    }
                }
            }
        }
    }

    // Fallback: `result` field
    if let Some(result) = json.get("result") {
        return Ok(result.clone());
    }

    // Last resort: return full response with provenance metadata
    tracing::warn!(
        "A2A task {}: no 'artifacts' or 'result' field found — returning raw response",
        task_id
    );
    Ok(serde_json::json!({
        "task_id": task_id,
        "tool": tool_name,
        "raw": json
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn test_extract_output_from_artifacts() {
        let resp = json!({
            "state": "completed",
            "artifacts": [{
                "name": "echo_result",
                "parts": [{"type": "data", "data": {"echo": "hello"}}]
            }]
        });
        let out = extract_output(&resp, "echo", "test-id").unwrap();
        assert_eq!(out, json!({"echo": "hello"}));
    }

    #[test]
    fn test_extract_output_fallback_to_result() {
        let resp = json!({
            "state": "completed",
            "result": {"sum": 42}
        });
        let out = extract_output(&resp, "add", "test-id").unwrap();
        assert_eq!(out, json!({"sum": 42}));
    }

    #[test]
    fn test_extract_output_raw_fallback() {
        let resp = json!({ "state": "completed", "other": "data" });
        let out = extract_output(&resp, "unknown", "test-id").unwrap();
        assert!(out.get("task_id").is_some());
    }
}
