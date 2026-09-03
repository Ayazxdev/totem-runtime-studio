//! A2A HTTP client — discovers remote agents and delegates tasks.
//!
//! Implements the client side of the Google Agent2Agent Protocol v1.0.
//! Supports:
//!   - Agent Card discovery (`GET /.well-known/agent.json`)
//!   - Task delegation (`POST /tasks/send`)
//!   - Task status polling (`GET /tasks/{id}`)

use crate::agent_card::AgentCard;
use sentinel_core::error::{SentinelError, SentinelResult};
use serde::{Deserialize, Serialize};
use std::time::Duration;
use uuid::Uuid;

/// Result of delegating a task to a remote A2A agent.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct A2aTaskResult {
    pub task_id: Uuid,
    pub status: String,
    pub output: serde_json::Value,
    pub agent_id: String,
}

pub struct A2aClient {
    endpoint: String,
    http: reqwest::Client,
}

impl A2aClient {
    pub fn new(endpoint: impl Into<String>) -> Self {
        Self {
            endpoint: endpoint.into().trim_end_matches('/').to_string(),
            http: reqwest::Client::builder()
                .timeout(Duration::from_secs(30))
                .build()
                .expect("reqwest client"),
        }
    }

    /// Fetch the remote agent's Agent Card from `/.well-known/agent.json`.
    pub async fn discover(&self) -> SentinelResult<AgentCard> {
        let url = format!("{}/.well-known/agent.json", self.endpoint);
        let resp = self
            .http
            .get(&url)
            .send()
            .await
            .map_err(|e| SentinelError::A2a(format!("Agent Card discovery failed at {url}: {e}")))?;

        if !resp.status().is_success() {
            return Err(SentinelError::A2a(format!(
                "Agent Card endpoint returned HTTP {}",
                resp.status()
            )));
        }

        resp.json::<AgentCard>()
            .await
            .map_err(|e| SentinelError::A2a(format!("Failed to parse Agent Card: {e}")))
    }

    /// Send a task to the remote agent.
    /// Returns immediately with the task ID; poll `get_task_status` for result.
    pub async fn send_task(
        &self,
        tool_name: &str,
        parameters: &serde_json::Value,
    ) -> SentinelResult<A2aTaskResult> {
        let task_id = Uuid::new_v4();
        let url = format!("{}/tasks/send", self.endpoint);

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

        let resp = self
            .http
            .post(&url)
            .json(&body)
            .send()
            .await
            .map_err(|e| SentinelError::A2a(format!("Task send to {url} failed: {e}")))?;

        let status_code = resp.status();
        let json: serde_json::Value = resp
            .json()
            .await
            .map_err(|e| SentinelError::A2a(format!("Failed to parse task response: {e}")))?;

        if !status_code.is_success() {
            return Err(SentinelError::A2a(format!(
                "Agent returned HTTP {}: {:?}",
                status_code, json
            )));
        }

        Ok(A2aTaskResult {
            task_id,
            status: json
                .get("status")
                .and_then(|s| s.as_str())
                .unwrap_or("unknown")
                .to_string(),
            output: json.get("result").cloned().unwrap_or(json.clone()),
            agent_id: self.endpoint.clone(),
        })
    }

    /// Poll task status from `GET /tasks/{id}`.
    pub async fn get_task_status(&self, task_id: Uuid) -> SentinelResult<A2aTaskResult> {
        let url = format!("{}/tasks/{}", self.endpoint, task_id);

        let resp = self
            .http
            .get(&url)
            .send()
            .await
            .map_err(|e| SentinelError::A2a(format!("Task status poll failed: {e}")))?;

        let json: serde_json::Value = resp
            .json()
            .await
            .map_err(|e| SentinelError::A2a(format!("Failed to parse status response: {e}")))?;

        Ok(A2aTaskResult {
            task_id,
            status: json
                .get("status")
                .and_then(|s| s.as_str())
                .unwrap_or("unknown")
                .to_string(),
            output: json.get("result").cloned().unwrap_or(json.clone()),
            agent_id: self.endpoint.clone(),
        })
    }

    /// Send a task and wait for completion (poll up to `max_polls` times).
    pub async fn send_and_wait(
        &self,
        tool_name: &str,
        parameters: &serde_json::Value,
        max_polls: u32,
    ) -> SentinelResult<A2aTaskResult> {
        let task = self.send_task(tool_name, parameters).await?;

        if task.status == "completed" || task.status == "success" {
            return Ok(task);
        }

        for attempt in 0..max_polls {
            tokio::time::sleep(Duration::from_millis(500 * (attempt as u64 + 1))).await;
            let status = self.get_task_status(task.task_id).await?;
            if status.status == "completed" || status.status == "success" || status.status == "failed" {
                return Ok(status);
            }
            tracing::debug!(
                "A2A task {} still pending (attempt {}/{}): {}",
                task.task_id, attempt + 1, max_polls, status.status
            );
        }

        Err(SentinelError::A2a(format!(
            "Task {} did not complete after {} polls",
            task.task_id, max_polls
        )))
    }
}
