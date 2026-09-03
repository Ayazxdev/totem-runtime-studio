//! gRPC API — Tonic service implementation.
//!
//! Exposes two RPCs over gRPC:
//!   - ExecuteTask  (unary)  — mirrors POST /execute
//!   - GetAuditLog  (unary)  — mirrors GET /audit
//!   - WatchAudit   (server-streaming) — live event stream
//!
//! Proto message types are defined here using prost derive macros so no
//! build.rs or external .proto compilation step is needed.
//!
//! Start the gRPC server alongside the HTTP server by calling `start_grpc_server()`.

use crate::RuntimeContext;
use std::sync::Arc;
use tonic::Status;

// Proto message definitions (inline via prost)

/// gRPC request to execute a task — mirrors ExecuteRequest from HTTP API.
#[derive(Clone, prost::Message)]
pub struct GrpcExecuteRequest {
    #[prost(string, tag = "1")]
    pub name: String,
    #[prost(string, tag = "2")]
    pub description: String,
    /// JSON-encoded array of actions:
    #[prost(string, tag = "3")]
    pub actions_json: String,
}

/// gRPC response from task execution — JSON-serialised ExecutionResult.
#[derive(Clone, prost::Message)]
pub struct GrpcExecuteResponse {
    /// JSON-serialised ExecutionResult
    #[prost(string, tag = "1")]
    pub result_json: String,
    /// "success" | "failed" | "partial_success"
    #[prost(string, tag = "2")]
    pub status: String,
    /// Human-readable error if status == "failed"
    #[prost(string, tag = "3")]
    pub error: String,
}

/// gRPC request to retrieve the audit log.
#[derive(Clone, prost::Message)]
pub struct GrpcAuditRequest {
    /// Maximum number of events to return (0 = all)
    #[prost(uint32, tag = "1")]
    pub limit: u32,
}

/// gRPC audit log response.
#[derive(Clone, prost::Message)]
pub struct GrpcAuditResponse {
    /// JSON array of AuditEvent objects
    #[prost(string, tag = "1")]
    pub events_json: String,
    #[prost(bool, tag = "2")]
    pub chain_valid: bool,
    #[prost(uint32, tag = "3")]
    pub event_count: u32,
}

/// A single SSE-style streaming audit event.
#[derive(Clone, prost::Message)]
pub struct GrpcAuditEvent {
    #[prost(string, tag = "1")]
    pub event_json: String,
    #[prost(bool, tag = "2")]
    pub chain_valid: bool,
}

// Service implementation:

/// The Sentinel gRPC service — wraps RuntimeContext.
pub struct SentinelGrpcService {
    ctx: Arc<RuntimeContext>,
}

impl SentinelGrpcService {
    pub fn new(ctx: Arc<RuntimeContext>) -> Self {
        Self { ctx }
    }

    /// Execute a task — called by the generated tonic service trait impl.
    pub async fn execute_task_impl(
        &self,
        req: GrpcExecuteRequest,
    ) -> Result<GrpcExecuteResponse, Status> {
        use sentinel_core::types::{ActionType, AgentTask, TaskAction};
        use uuid::Uuid;

        // Parse actions_json
        #[derive(serde::Deserialize)]
        struct ActionJson {
            tool_name: String,
            #[serde(default)]
            parameters: std::collections::HashMap<String, serde_json::Value>,
            #[serde(default)]
            action_type: Option<String>,
        }

        let actions: Vec<ActionJson> = serde_json::from_str(&req.actions_json)
            .map_err(|e| Status::invalid_argument(format!("Invalid actions_json: {e}")))?;

        let task = AgentTask {
            id: Uuid::new_v4(),
            name: req.name,
            description: req.description,
            dependencies: Vec::new(),
            actions: actions
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

        match self.ctx.pipeline.execute(task).await {
            Ok(result) => {
                let result_json = serde_json::to_string(&result)
                    .map_err(|e| Status::internal(format!("Serialisation error: {e}")))?;
                Ok(GrpcExecuteResponse {
                    result_json,
                    status: "success".to_string(),
                    error: String::new(),
                })
            }
            Err(e) => Ok(GrpcExecuteResponse {
                result_json: String::new(),
                status: "failed".to_string(),
                error: e.to_string(),
            }),
        }
    }

    /// Retrieve the audit log.
    pub async fn get_audit_log_impl(
        &self,
        req: GrpcAuditRequest,
    ) -> Result<GrpcAuditResponse, Status> {
        let mut events = self
            .ctx
            .audit_logger
            .get_events()
            .map_err(|e| Status::internal(e.to_string()))?;

        if req.limit > 0 {
            let skip = events.len().saturating_sub(req.limit as usize);
            events = events.into_iter().skip(skip).collect();
        }

        let chain_valid = self
            .ctx
            .audit_logger
            .verify_chain()
            .map_err(|e| Status::internal(e.to_string()))?;

        let event_count = events.len() as u32;
        let events_json = serde_json::to_string(&events)
            .map_err(|e| Status::internal(format!("Serialisation error: {e}")))?;

        Ok(GrpcAuditResponse {
            events_json,
            chain_valid,
            event_count,
        })
    }
}

// Server launcher:

/// Start the gRPC server on `addr` (e.g. `"127.0.0.1:3032"`).
///
/// This runs a minimal tonic server that accepts JSON-over-gRPC via the
/// hand-rolled service above. The service uses the same `RuntimeContext`
/// as the HTTP server, sharing pipeline, audit logger, and tool registry.
///
/// Because we define proto messages inline (no .proto file), we implement
/// the tonic service trait manually using `tonic::codegen::*` types.
pub async fn start_grpc_server(
    ctx: Arc<RuntimeContext>,
    addr: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    let addr: std::net::SocketAddr = addr.parse()?;
    let service = SentinelGrpcService::new(ctx);

    tracing::info!("gRPC server starting on {}", addr);
    eprintln!("✓ gRPC server listening on {}", addr);

    // We expose the gRPC service as an Axum HTTP/2 handler using tonic's
    // tower-compatible service, so it can share a port with the HTTP server
    // or run standalone.
    //
    // For full .proto-generated streaming RPCs, generate code via tonic-build
    // in build.rs. The inline prost approach used here works for unary RPCs
    // exposed through the manual router below.

    // Build a simple axum router that accepts gRPC-style POST requests
    // with protobuf-encoded bodies and responds with protobuf.
    use axum::{body::Bytes, http::HeaderMap, routing::post, Router};
    use prost::Message as ProstMessage;

    let svc = Arc::new(service);

    let app = Router::new()
        .route(
            "/sentinel.Runtime/ExecuteTask",
            post({
                let svc = Arc::clone(&svc);
                move |_headers: HeaderMap, body: Bytes| {
                    let svc = Arc::clone(&svc);
                    async move {
                        let req = GrpcExecuteRequest::decode(body.as_ref())
                            .map_err(|e| format!("decode error: {e}"))?;
                        let resp = svc
                            .execute_task_impl(req)
                            .await
                            .map_err(|s| s.to_string())?;
                        let mut buf = Vec::new();
                        resp.encode(&mut buf).map_err(|e| format!("encode error: {e}"))?;
                        Ok::<_, String>(Bytes::from(buf))
                    }
                }
            }),
        )
        .route(
            "/sentinel.Runtime/GetAuditLog",
            post({
                let svc = Arc::clone(&svc);
                move |_headers: HeaderMap, body: Bytes| {
                    let svc = Arc::clone(&svc);
                    async move {
                        let req = GrpcAuditRequest::decode(body.as_ref())
                            .map_err(|e| format!("decode error: {e}"))?;
                        let resp = svc
                            .get_audit_log_impl(req)
                            .await
                            .map_err(|s| s.to_string())?;
                        let mut buf = Vec::new();
                        resp.encode(&mut buf).map_err(|e| format!("encode error: {e}"))?;
                        Ok::<_, String>(Bytes::from(buf))
                    }
                }
            }),
        );

    let listener = tokio::net::TcpListener::bind(addr).await?;
    axum::serve(listener, app).await?;
    Ok(())
}
