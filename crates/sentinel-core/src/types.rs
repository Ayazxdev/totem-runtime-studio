
//! Core domain types shared across all subsystems.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use uuid::Uuid;

// Task / Workflow

/// An agent task request from an orchestration framework or client.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentTask {
    pub id: Uuid,
    pub name: String,
    pub description: String,
    pub dependencies: Vec<Uuid>,
    pub actions: Vec<TaskAction>,
    pub context: ExecutionContext,
    pub capabilities_required: Vec<String>,
}

/// A single action within a task (e.g., a tool invocation).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskAction {
    pub id: Uuid,
    pub action_type: ActionType,
    pub tool_name: String,
    pub parameters: HashMap<String, serde_json::Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ActionType {
    ToolCall,
    AgentCommunication,
    ContextUpdate,
}


// Execution Context


/// Execution context for a task — the "memory" of the agent workflow.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ExecutionContext {
    pub messages: Vec<ContextMessage>,
    pub tool_results: Vec<ToolResult>,
    pub metadata: HashMap<String, serde_json::Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContextMessage {
    pub role: String,       // "user", "assistant", "system"
    pub content: String,
    pub timestamp: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolResult {
    pub tool_name: String,
    pub action_id: Uuid,
    pub output: serde_json::Value,
    pub latency_ms: u64,
    pub timestamp: DateTime<Utc>,
}


// Capability Token (CAPSEM)


/// A capability token granting permission to perform specific actions on resources.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CapabilityToken {
    pub id: Uuid,
    pub subject: String,              // agent/workflow ID
    pub resource: String,             // tool name, filesystem path, etc.
    pub actions: Vec<String>,         // "read", "write", "execute"
    pub constraints: TokenConstraints,
    pub issued_at: DateTime<Utc>,
    pub expires_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct TokenConstraints {
    pub max_invocations: Option<u32>,
    pub allowed_parameters: Option<Vec<String>>,
    pub rate_limit_per_minute: Option<u32>,
}


// Tool Definition


/// A registered tool callable by the runtime.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolDefinition {
    pub name: String,
    pub description: String,
    pub transport: ToolTransport,
    pub schema: serde_json::Value,  // JSON schema for input parameters
    pub attestation: Option<ToolAttestation>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ToolTransport {
    Local,
    Http { endpoint: String },
    /// GraphQL mutation/query transport
    GraphQl { endpoint: String, operation: Option<String> },
    Grpc { endpoint: String },
    Mcp { server_name: String },
    A2a { agent_id: String },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolAttestation {
    pub public_key: Vec<u8>,
    pub signature: Vec<u8>,
    pub algorithm: String,  // "ed25519", "ml-dsa" (post-quantum)
}


// Audit Event


/// Immutable audit event — every execution decision is logged.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuditEvent {
    pub id: Uuid,
    pub timestamp: DateTime<Utc>,
    pub event_type: AuditEventType,
    pub task_id: Option<Uuid>,
    pub action_id: Option<Uuid>,
    pub subject: String,      // who initiated the action
    pub details: serde_json::Value,
    /// Hash of the previous event in the chain (tamper-evidence).
    pub previous_hash: Option<String>,
    /// Hash of this event (BLAKE3).
    pub event_hash: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AuditEventType {
    TaskReceived,
    TaskScheduled,
    ToolInvoked,
    CapabilityVerified,
    CapabilityDenied,
    ContextCompressed,
    BranchPruned,
    RecoveryAttempted,
    TaskCompleted,
    SecurityViolation,
}


// Execution Result


/// The final result of executing a task.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExecutionResult {
    pub task_id: Uuid,
    pub status: ExecutionStatus,
    pub output: serde_json::Value,
    pub metrics: ExecutionMetrics,
    pub audit_trail_id: Uuid,  // first event in the audit chain for this execution
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ExecutionStatus {
    Success,
    PartialSuccess,
    Failed,
    Pruned,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ExecutionMetrics {
    pub total_latency_ms: u64,
    pub parallel_branches_executed: u32,
    pub branches_pruned: u32,
    pub total_token_count: u64,
    pub estimated_cost_usd: f64,
    pub tools_invoked: u32,
    pub capability_checks: u32,
    pub capability_denials: u32,
    pub recovery_attempts: u32,
}
