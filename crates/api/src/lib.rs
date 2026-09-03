//! api — HTTP + gRPC API surface.
//!
//! A2A Agent Card endpoint, /tools listing, bind_addr plumbing,
//! full tool set registration, audit event injection from all stages.

pub mod grpc;
pub mod http;

use audit::logger::AuditLogger;
use sentinel_core::{
    pipeline::ExecutionPipeline,
    types::ToolTransport,
};
use std::sync::Arc;

/// Shared runtime context — one instance for the server's lifetime.
pub struct RuntimeContext {
    pub pipeline: Arc<ExecutionPipeline>,
    pub audit_logger: Arc<AuditLogger>,
    pub capsem: Arc<capsem::CapsemStage>,
    pub mcp: Arc<mcp::McpStage>,
    /// The address the HTTP server is bound to (for Agent Card).
    pub bind_addr: Option<String>,
    /// Snapshot accessor — tool-manager registry stored separately for /tools endpoint.
    tool_mgr: Arc<tool_manager::ToolManagerStage>,
}

impl RuntimeContext {
    pub fn new() -> Self {
        Self::with_bind(None)
    }

    pub fn with_bind(bind_addr: Option<String>) -> Self {
        let audit_logger = Arc::new(AuditLogger::new());
        let tool_mgr    = Arc::new(tool_manager::ToolManagerStage::new());
        let capsem      = Arc::new(capsem::CapsemStage::new());
        let mcp_stage   = Arc::new(mcp::McpStage::new());

        //Local tools 
        let reg = tool_mgr.registry();

        reg.register(sentinel_core::types::ToolDefinition {
            name: "echo".to_string(),
            description: "Echo — returns the input message unchanged".to_string(),
            transport: ToolTransport::Local,
            schema: serde_json::json!({
                "type": "object",
                "properties": { "message": { "type": "string" } },
                "required": ["message"]
            }),
            attestation: None,
        });

        reg.register(sentinel_core::types::ToolDefinition {
            name: "add".to_string(),
            description: "Add two numbers".to_string(),
            transport: ToolTransport::Local,
            schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "a": { "type": "number" },
                    "b": { "type": "number" }
                },
                "required": ["a", "b"]
            }),
            attestation: None,
        });

        reg.register(sentinel_core::types::ToolDefinition {
            name: "multiply".to_string(),
            description: "Multiply two numbers".to_string(),
            transport: ToolTransport::Local,
            schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "a": { "type": "number" },
                    "b": { "type": "number" }
                },
                "required": ["a", "b"]
            }),
            attestation: None,
        });

        reg.register(sentinel_core::types::ToolDefinition {
            name: "hash".to_string(),
            description: "BLAKE3 hash of input string".to_string(),
            transport: ToolTransport::Local,
            schema: serde_json::json!({
                "type": "object",
                "properties": { "input": { "type": "string" } },
                "required": ["input"]
            }),
            attestation: None,
        });

        reg.register(sentinel_core::types::ToolDefinition {
            name: "sleep".to_string(),
            description: "Sleep for N milliseconds (latency simulation / parallel benchmark)".to_string(),
            transport: ToolTransport::Local,
            schema: serde_json::json!({
                "type": "object",
                "properties": { "ms": { "type": "integer", "minimum": 0, "maximum": 5000 } }
            }),
            attestation: None,
        });

        reg.register(sentinel_core::types::ToolDefinition {
            name: "env_info".to_string(),
            description: "Return safe read-only OS/arch information".to_string(),
            transport: ToolTransport::Local,
            schema: serde_json::json!({ "type": "object", "properties": {} }),
            attestation: None,
        });

        //Restricted tool (CAPSEM demo) 
        reg.register(sentinel_core::types::ToolDefinition {
            name: "dangerous-tool".to_string(),
            description: "Restricted — access denied by CAPSEM policy".to_string(),
            transport: ToolTransport::Local,
            schema: serde_json::json!({ "type": "object" }),
            attestation: None,
        });

        //MCP tools
        for (name, desc) in [
            ("read_file",  "Read a file from the filesystem (via MCP)"),
            ("list_files", "List files in a directory (via MCP)"),
            ("write_file", "Write content to a file (via MCP)"),
        ] {
            reg.register(sentinel_core::types::ToolDefinition {
                name: name.to_string(),
                description: desc.to_string(),
                transport: ToolTransport::Mcp { server_name: "filesystem".to_string() },
                schema: serde_json::json!({ "type": "object" }),
                attestation: None,
            });
        }

        //Security policy ───────────────────────────────────────────────
        capsem.policy().revoke_resource("dangerous-tool");

        //MCP server ─
        mcp_stage
            .register_server("filesystem".to_string(), "http://127.0.0.1:3031".to_string())
            .ok();

        //Pipeline ───
        let mut pipeline = ExecutionPipeline::new();
        pipeline.add_stage(Box::new(scheduler::SchedulerStage::new()));
        pipeline.add_stage(Box::new(context::ContextStage::default()));
        pipeline.add_stage(Box::new(capsem.as_ref().clone()));
        pipeline.add_stage(Box::new(mcp_stage.as_ref().clone()));
        // Share the same registry Arc so pipeline stage sees all registrations
        pipeline.add_stage(Box::new(
            tool_manager::ToolManagerStage::with_registry(tool_mgr.registry_arc())
        ));
        pipeline.add_stage(Box::new(a2a::A2aStage));
        pipeline.add_stage(Box::new(audit::AuditStage::new(audit_logger.clone())));
        pipeline.add_stage(Box::new(evaluation::EvaluationStage::new()));
        pipeline.add_stage(Box::new(recovery::RecoveryStage::new(audit_logger.clone())));

        Self {
            pipeline: Arc::new(pipeline),
            audit_logger,
            capsem,
            mcp: mcp_stage,
            bind_addr,
            tool_mgr,
        }
    }

    /// Snapshot of registered tool names + descriptions for /tools endpoint.
    pub fn tool_registry_snapshot(&self) -> Vec<serde_json::Value> {
        self.tool_mgr
            .registry()
            .list_tools()
            .into_iter()
            .map(|(name, desc)| serde_json::json!({ "name": name, "description": desc }))
            .collect()
    }
}

impl Default for RuntimeContext {
    fn default() -> Self {
        Self::new()
    }
}
