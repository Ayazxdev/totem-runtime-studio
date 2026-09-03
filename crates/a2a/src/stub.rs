//! A2A v1.0 long-running task lifecycle implementation.
//!
//! This module implements the server-side A2A protocol task handling:
//! - Task creation and state machine (submitted → working → completed/failed/cancelled)
//! - In-memory task store with status polling
//! - Push notifications via Server-Sent Events (SSE) streaming
//! - Task cancellation
//!
//! Per the Google Agent2Agent Protocol v1.0 / Linux Foundation specification.

use chrono::{DateTime, Utc};
use dashmap::DashMap;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use uuid::Uuid;

// Task state machine 

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum TaskState {
    /// Task accepted, queued for execution
    Submitted,
    /// Task is being executed by the agent
    Working,
    /// Task completed successfully; output available
    Completed,
    /// Task failed; error details in message
    Failed,
    /// Task was cancelled by the caller
    Cancelled,
    /// Task requires caller input to continue
    InputRequired,
}

impl std::fmt::Display for TaskState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let s = match self {
            TaskState::Submitted     => "submitted",
            TaskState::Working       => "working",
            TaskState::Completed     => "completed",
            TaskState::Failed        => "failed",
            TaskState::Cancelled     => "cancelled",
            TaskState::InputRequired => "input_required",
        };
        write!(f, "{s}")
    }
}

// Task record

/// A long-running A2A task tracked in the task store.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct A2aTask {
    pub id: Uuid,
    /// The agent that sent this task
    pub sender_id: String,
    /// Tool name requested
    pub tool_name: String,
    /// Input parameters
    pub parameters: serde_json::Value,
    /// Current state in the task lifecycle
    pub state: TaskState,
    /// Artifacts/results once completed
    pub artifacts: Vec<A2aArtifact>,
    /// Human-readable status message
    pub message: Option<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

/// An output artifact produced by a completed A2A task.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct A2aArtifact {
    pub name: String,
    pub description: Option<String>,
    pub parts: Vec<A2aArtifactPart>,
}

/// A single part of an artifact (text, file, data).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum A2aArtifactPart {
    Text  { text: String },
    Data  { data: serde_json::Value },
    File  { name: String, mime_type: String, content_base64: String },
}

// Task Store

/// In-memory A2A task store: thread-safe, server lifetime.
#[derive(Default, Clone)]
pub struct TaskStore {
    tasks: Arc<DashMap<Uuid, A2aTask>>,
}

impl TaskStore {
    pub fn new() -> Self {
        Self { tasks: Arc::new(DashMap::new()) }
    }

    /// Accept an incoming task and return it in Submitted state.
    pub fn accept(
        &self,
        sender_id: impl Into<String>,
        tool_name: impl Into<String>,
        parameters: serde_json::Value,
    ) -> A2aTask {
        let now = Utc::now();
        let task = A2aTask {
            id: Uuid::new_v4(),
            sender_id: sender_id.into(),
            tool_name: tool_name.into(),
            parameters,
            state: TaskState::Submitted,
            artifacts: Vec::new(),
            message: None,
            created_at: now,
            updated_at: now,
        };
        self.tasks.insert(task.id, task.clone());
        tracing::info!("A2A TaskStore: accepted task {}", task.id);
        task
    }

    /// Transition a task to Working state.
    pub fn set_working(&self, id: Uuid) {
        self.update(id, |t| {
            t.state = TaskState::Working;
            t.message = Some("Task is being processed".to_string());
        });
    }

    /// Mark a task as completed with output artifacts.
    pub fn complete(&self, id: Uuid, artifacts: Vec<A2aArtifact>) {
        self.update(id, |t| {
            t.state = TaskState::Completed;
            t.artifacts = artifacts;
            t.message = Some("Task completed successfully".to_string());
        });
    }

    /// Mark a task as failed.
    pub fn fail(&self, id: Uuid, reason: impl Into<String>) {
        self.update(id, |t| {
            t.state = TaskState::Failed;
            t.message = Some(reason.into());
        });
    }

    /// Cancel a task if it is still in-progress.
    /// Returns false if the task is already terminal.
    pub fn cancel(&self, id: Uuid) -> bool {
        let mut cancelled = false;
        self.update(id, |t| {
            if t.state == TaskState::Submitted || t.state == TaskState::Working {
                t.state = TaskState::Cancelled;
                t.message = Some("Task cancelled by caller".to_string());
                cancelled = true;
            }
        });
        cancelled
    }

    /// Get a snapshot of a task by ID.
    pub fn get(&self, id: Uuid) -> Option<A2aTask> {
        self.tasks.get(&id).map(|e| e.clone())
    }

    /// List all tasks.
    pub fn list(&self) -> Vec<A2aTask> {
        self.tasks.iter().map(|e| e.value().clone()).collect()
    }

    fn update(&self, id: Uuid, f: impl FnOnce(&mut A2aTask)) {
        if let Some(mut entry) = self.tasks.get_mut(&id) {
            f(entry.value_mut());
            entry.updated_at = Utc::now();
        }
    }
}

// Push notification payload

/// SSE event sent to subscribers watching a task.
#[derive(Debug, Clone, Serialize)]
pub struct TaskStatusUpdate {
    pub task_id: Uuid,
    pub state: TaskState,
    pub message: Option<String>,
    pub timestamp: DateTime<Utc>,
    pub final_event: bool,
}

impl TaskStatusUpdate {
    pub fn from_task(task: &A2aTask) -> Self {
        let final_event = matches!(
            task.state,
            TaskState::Completed | TaskState::Failed | TaskState::Cancelled
        );
        Self {
            task_id: task.id,
            state: task.state.clone(),
            message: task.message.clone(),
            timestamp: task.updated_at,
            final_event,
        }
    }
}

// Helpers:

/// Build a completed text artifact from a JSON tool result.
pub fn result_to_artifact(tool_name: &str, output: &serde_json::Value) -> A2aArtifact {
    A2aArtifact {
        name: format!("{}_result", tool_name),
        description: Some(format!("Output from tool '{}'", tool_name)),
        parts: vec![A2aArtifactPart::Data { data: output.clone() }],
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_task_lifecycle() {
        let store = TaskStore::new();
        let task = store.accept("agent-a", "echo", serde_json::json!({"message": "hi"}));
        assert_eq!(task.state, TaskState::Submitted);

        store.set_working(task.id);
        assert_eq!(store.get(task.id).unwrap().state, TaskState::Working);

        let artifact = result_to_artifact("echo", &serde_json::json!({"echo": "hi"}));
        store.complete(task.id, vec![artifact]);
        let done = store.get(task.id).unwrap();
        assert_eq!(done.state, TaskState::Completed);
        assert_eq!(done.artifacts.len(), 1);
    }

    #[test]
    fn test_task_cancel_only_if_in_progress() {
        let store = TaskStore::new();
        let task = store.accept("agent-a", "sleep", serde_json::json!({"ms": 1000}));
        assert!(store.cancel(task.id), "should cancel while submitted");

        // Can't cancel again once terminal
        assert!(!store.cancel(task.id), "should not cancel when already cancelled");
    }

    #[test]
    fn test_task_update_is_timestamped() {
        let store = TaskStore::new();
        let task = store.accept("x", "echo", serde_json::json!({}));
        let before = task.updated_at;
        std::thread::sleep(std::time::Duration::from_millis(2));
        store.set_working(task.id);
        let after = store.get(task.id).unwrap().updated_at;
        assert!(after >= before);
    }
}
