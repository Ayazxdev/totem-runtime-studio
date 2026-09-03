//! Agent Card — the A2A identity/capability advertisement document.
//!
//! Per the Google Agent2Agent Protocol v1.0 / Linux Foundation spec.
//! An Agent Card is a JSON document served at `/.well-known/agent.json` that describes
//! the agent's identity, capabilities, and how to reach it.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// An Agent Card document per A2A v1.0 spec.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentCard {
    /// Unique agent identifier (URL or URN).
    pub id: String,
    /// Human-readable agent name.
    pub name: String,
    /// Short description of what the agent does.
    pub description: String,
    /// Agent version string.
    pub version: String,
    /// HTTP endpoint where this agent accepts tasks (`POST /tasks/send`).
    pub endpoint: String,
    /// Tools/capabilities this agent exposes.
    pub capabilities: Vec<AgentCapability>,
    /// Supported input/output content types.
    pub supported_content_types: Vec<String>,
    /// When this card was issued.
    pub issued_at: DateTime<Utc>,
}

/// A capability advertised by the agent.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentCapability {
    /// Unique capability identifier.
    pub id: Uuid,
    /// Name of the capability (e.g. "code_generation", "web_search").
    pub name: String,
    /// Human-readable description.
    pub description: String,
    /// JSON Schema for input parameters.
    pub input_schema: serde_json::Value,
    /// JSON Schema for output.
    pub output_schema: serde_json::Value,
}

impl AgentCard {
    /// Build the Totem Runtime Studio's own Agent Card.
    pub fn totem_runtime() -> Self {
        Self {
            id: "urn:totem:sentinel-runtime:v1".to_string(),
            name: "Totem Sentinel Runtime".to_string(),
            description: "Secure, local-first Rust execution runtime for autonomous AI agent workflows. Supports CAPSEM zero-trust security, parallel DAG execution, MCP tool integration, and hash-chained audit logging.".to_string(),
            version: env!("CARGO_PKG_VERSION").to_string(),
            endpoint: "http://127.0.0.1:3030".to_string(),
            capabilities: vec![
                AgentCapability {
                    id: Uuid::new_v4(),
                    name: "tool_execution".to_string(),
                    description: "Execute registered tools (local, HTTP, MCP, A2A transports)".to_string(),
                    input_schema: serde_json::json!({
                        "type": "object",
                        "properties": {
                            "tool_name": { "type": "string" },
                            "parameters": { "type": "object" }
                        },
                        "required": ["tool_name"]
                    }),
                    output_schema: serde_json::json!({ "type": "object" }),
                },
                AgentCapability {
                    id: Uuid::new_v4(),
                    name: "parallel_dag_scheduling".to_string(),
                    description: "Decompose multi-action tasks into a DAG and execute independent branches concurrently".to_string(),
                    input_schema: serde_json::json!({
                        "type": "array",
                        "items": { "$ref": "#/definitions/TaskAction" }
                    }),
                    output_schema: serde_json::json!({ "type": "object" }),
                },
                AgentCapability {
                    id: Uuid::new_v4(),
                    name: "audit_trail".to_string(),
                    description: "Immutable BLAKE3 hash-chained audit log for all executions".to_string(),
                    input_schema: serde_json::json!({ "type": "null" }),
                    output_schema: serde_json::json!({
                        "type": "object",
                        "properties": {
                            "events": { "type": "array" },
                            "chain_valid": { "type": "boolean" }
                        }
                    }),
                },
            ],
            supported_content_types: vec![
                "application/json".to_string(),
            ],
            issued_at: Utc::now(),
        }
    }
}
