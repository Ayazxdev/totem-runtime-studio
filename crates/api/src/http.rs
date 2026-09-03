//! HTTP API (Axum).
//!
//! Endpoints:
//!   GET  /health                   — health check
//!   GET  /.well-known/agent.json   — A2A Agent Card (discovery)
//!   GET  /tools                    — list registered tools
//!   POST /execute                  — execute a task workflow
//!   GET  /audit                    — retrieve hash-chained audit trail

use crate::RuntimeContext;
use axum::{
    extract::State,
    http::StatusCode,
    response::{IntoResponse, Json, Response},
    routing::{get, post},
    Router,
};
use sentinel_core::types::{ActionType, AgentTask, ExecutionResult, TaskAction};
use serde::{Deserialize, Serialize};
use std::{collections::HashMap, sync::Arc};
use tower_http::trace::TraceLayer;
use uuid::Uuid;

pub fn app(ctx: Arc<RuntimeContext>) -> Router {
    Router::new()
        .route("/", get(health))
        .route("/health", get(health))
        .route("/.well-known/agent.json", get(agent_card))
        .route("/tools", get(list_tools))
        .route("/execute", post(execute))
        .route("/audit", get(get_audit_log))
        .with_state(ctx)
        .layer(TraceLayer::new_for_http())
}

async fn health() -> impl IntoResponse {
    Json(serde_json::json!({
        "status": "ok",
        "service": "Totem Sentinel Runtime",
        "phase": 7,
        "version": env!("CARGO_PKG_VERSION"),
    }))
}

/// A2A Agent Card — served at `/.well-known/agent.json` for discovery.
async fn agent_card(State(ctx): State<Arc<RuntimeContext>>) -> Json<a2a::AgentCard> {
    let mut card = a2a::AgentCard::totem_runtime();
    // Reflect actual bind address from context if available
    if let Some(addr) = ctx.bind_addr.as_deref() {
        card.endpoint = format!("http://{}", addr);
    }
    Json(card)
}

/// List all registered tools.
async fn list_tools(State(ctx): State<Arc<RuntimeContext>>) -> Json<serde_json::Value> {
    let tools = ctx.tool_registry_snapshot();
    Json(serde_json::json!({ "tools": tools }))
}

// Execute

#[derive(Debug, Deserialize)]
struct ExecuteRequest {
    name: String,
    #[serde(default)]
    description: String,
    #[serde(default)]
    actions: Vec<ActionRequest>,
}

#[derive(Debug, Deserialize)]
struct ActionRequest {
    tool_name: String,
    #[serde(default)]
    parameters: HashMap<String, serde_json::Value>,
    /// Override action type: "tool_call" (default), "agent_communication", "context_update"
    #[serde(default)]
    action_type: Option<String>,
}

async fn execute(
    State(ctx): State<Arc<RuntimeContext>>,
    Json(req): Json<ExecuteRequest>,
) -> Result<Json<ExecutionResult>, AppError> {
    tracing::info!("Received execute request: '{}'", req.name);

    let task = AgentTask {
        id: Uuid::new_v4(),
        name: req.name,
        description: req.description,
        dependencies: Vec::new(),
        actions: req
            .actions
            .into_iter()
            .map(|a| {
                let action_type = match a.action_type.as_deref() {
                    Some("agent_communication") => ActionType::AgentCommunication,
                    Some("context_update")      => ActionType::ContextUpdate,
                    _                           => ActionType::ToolCall,
                };
                TaskAction {
                    id: Uuid::new_v4(),
                    action_type,
                    tool_name: a.tool_name,
                    parameters: a.parameters,
                }
            })
            .collect(),
        context: Default::default(),
        capabilities_required: Vec::new(),
    };

    let result = ctx.pipeline.execute(task).await?;
    Ok(Json(result))
}

// Audit

#[derive(Debug, Serialize)]
struct AuditLogResponse {
    events: Vec<sentinel_core::types::AuditEvent>,
    chain_valid: bool,
    event_count: usize,
}

async fn get_audit_log(
    State(ctx): State<Arc<RuntimeContext>>,
) -> Result<Json<AuditLogResponse>, AppError> {
    let events = ctx.audit_logger.get_events()?;
    let chain_valid = ctx.audit_logger.verify_chain()?;
    let event_count = events.len();

    Ok(Json(AuditLogResponse {
        events,
        chain_valid,
        event_count,
    }))
}

// Error handling

struct AppError(sentinel_core::error::SentinelError);

impl IntoResponse for AppError {
    fn into_response(self) -> Response {
        use sentinel_core::error::SentinelError;
        let (status, message) = match &self.0 {
            SentinelError::CapabilityDenied { .. } => (StatusCode::FORBIDDEN, format!("{}", self.0)),
            SentinelError::ToolNotFound(_)         => (StatusCode::NOT_FOUND, format!("{}", self.0)),
            _                                      => (StatusCode::INTERNAL_SERVER_ERROR, format!("{}", self.0)),
        };
        let body = serde_json::json!({ "error": message, "status": status.as_u16() });
        (status, Json(body)).into_response()
    }
}

impl<E: Into<sentinel_core::error::SentinelError>> From<E> for AppError {
    fn from(err: E) -> Self {
        AppError(err.into())
    }
}
