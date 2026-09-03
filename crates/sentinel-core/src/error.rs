//! Unified error type for the Sentinel Runtime.

use thiserror::Error;

#[derive(Debug, Error)]
pub enum SentinelError {
    // Scheduling
    #[error("Scheduler error: {0}")]
    Scheduler(String),

    #[error("Cyclic dependency detected in task graph")]
    CyclicDependency,

    // Security / CAPSEM
    #[error("Capability verification failed for action '{action}' on resource '{resource}': {reason}")]
    CapabilityDenied { action: String, resource: String, reason: String },

    #[error("Invalid capability token: {0}")]
    InvalidToken(String),

    #[error("Sandbox violation: {0}")]
    SandboxViolation(String),

    // Tool Manager─────
    #[error("Tool not found: {0}")]
    ToolNotFound(String),

    #[error("Tool invocation failed for '{tool}': {reason}")]
    ToolInvocationFailed { tool: String, reason: String },

    #[error("Tool attestation failed: {0}")]
    ToolAttestationFailed(String),

    // MCP───────
    #[error("MCP error: {0}")]
    Mcp(String),

    // Context───
    #[error("Context management error: {0}")]
    Context(String),

    // Audit─────
    #[error("Audit logger error: {0}")]
    Audit(String),

    #[error("Audit chain integrity violation: {0}")]
    AuditChainViolation(String),

    // Recovery──
    #[error("Recovery failed after {attempts} attempts: {reason}")]
    RecoveryFailed { attempts: u32, reason: String },

    // A2A (stub)
    #[error("A2A error: {0}")]
    A2a(String),

    // Generic───
    #[error("Internal runtime error: {0}")]
    Internal(String),

    #[error(transparent)]
    Io(#[from] std::io::Error),

    #[error(transparent)]
    Json(#[from] serde_json::Error),
}

pub type SentinelResult<T> = Result<T, SentinelError>;
