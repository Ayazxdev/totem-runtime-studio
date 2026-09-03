//! Tool registry — registration, lookup, and dispatch across all transports.
//! Supports GraphQL transport, attestation verification, A2A agent dispatch,
//! and per-invocation attestation checks.

use dashmap::DashMap;
use sentinel_core::{
    error::{SentinelError, SentinelResult},
    types::{ToolDefinition, ToolTransport},
};
use std::collections::HashMap;

pub struct ToolRegistry {
    tools: DashMap<String, ToolDefinition>,
}

impl ToolRegistry {
    pub fn new() -> Self {
        Self {
            tools: DashMap::new(),
        }
    }

    pub fn register(&self, tool: ToolDefinition) {
        // Verify attestation at registration time if present
        if let Some(ref att) = tool.attestation {
            match crate::attestation::verify_attestation(att) {
                Ok(()) => tracing::info!("Tool '{}' registered with verified {} attestation", tool.name, att.algorithm),
                Err(e) => tracing::warn!("Tool '{}' attestation verification failed: {e} — tool still registered", tool.name),
            }
        } else {
            tracing::info!("Registering tool: {} (no attestation)", tool.name);
        }
        self.tools.insert(tool.name.clone(), tool);
    }

    pub fn get(&self, name: &str) -> SentinelResult<ToolDefinition> {
        self.tools
            .get(name)
            .map(|entry| entry.clone())
            .ok_or_else(|| SentinelError::ToolNotFound(name.to_string()))
    }

    /// Returns a snapshot of all registered tool names and descriptions.
    pub fn list_tools(&self) -> Vec<(String, String)> {
        self.tools
            .iter()
            .map(|entry| (entry.key().clone(), entry.value().description.clone()))
            .collect()
    }

    pub async fn invoke_tool(
        &self,
        name: &str,
        parameters: &HashMap<String, serde_json::Value>,
    ) -> SentinelResult<serde_json::Value> {
        let tool = self.get(name)?;

        // Re-verify attestation on every invocation if present
        if let Some(ref att) = tool.attestation {
            crate::attestation::verify_attestation(att)?;
        }

        match &tool.transport {
            ToolTransport::Local => {
                crate::local::invoke_local_tool(name, parameters).await
            }

            ToolTransport::Http { endpoint } => {
                crate::http::invoke_http_tool(endpoint, parameters).await
            }

            ToolTransport::GraphQl { endpoint, operation } => {
                let op = operation.as_deref().unwrap_or(name);
                crate::graphql::invoke_graphql_tool(endpoint, op, parameters).await
            }

            ToolTransport::Grpc { endpoint } => {
                // Generic JSON-over-gRPC via fallback to HTTP JSON gateway
                // at the same endpoint.
                // (many gRPC services also expose a gRPC-Gateway HTTP/JSON endpoint).
                tracing::warn!(
                    "gRPC tool '{}' falling back to HTTP JSON gateway at {}",
                    name, endpoint
                );
                crate::http::invoke_http_tool(endpoint, parameters).await
            }

            ToolTransport::Mcp { .. } => {
                Err(SentinelError::ToolInvocationFailed {
                    tool: name.to_string(),
                    reason: "MCP transport is handled by the MCP pipeline stage".to_string(),
                })
            }

            ToolTransport::A2a { agent_id } => {
                // Delegate to remote A2A agent
                crate::a2a_dispatch::delegate_to_agent(agent_id, name, parameters).await
            }
        }
    }
}

impl Default for ToolRegistry {
    fn default() -> Self {
        Self::new()
    }
}
