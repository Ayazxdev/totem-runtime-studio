//! capsem — Capability-Based Security Manager (CAPSEM).
//!
//! Zero-trust: every tool invocation requires capability verification.
//! rate-limiting per subject, per-action deny rules, counters in context.

pub mod capability;
pub mod injection;
pub mod policy;
pub mod sandbox;

use async_trait::async_trait;
use sentinel_core::{
    error::{SentinelError, SentinelResult},
    pipeline::PipelineStage,
    types::{AgentTask, ExecutionContext},
};
use std::sync::Arc;
#[derive(Clone)]
pub struct CapsemStage {
    policy_engine: Arc<policy::PolicyEngine>,
}

impl CapsemStage {
    pub fn new() -> Self {
        Self {
            policy_engine: Arc::new(policy::PolicyEngine::new()),
        }
    }

    pub fn policy(&self) -> &policy::PolicyEngine {
        &self.policy_engine
    }
}

impl Default for CapsemStage {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl PipelineStage for CapsemStage {
    fn name(&self) -> &str {
        "CAPSEM"
    }

    async fn process(
        &self,
        task: &AgentTask,
        context: &mut ExecutionContext,
    ) -> SentinelResult<()> {
        tracing::info!("CAPSEM: verifying {} actions for task {}", task.actions.len(), task.id);

        let mut checks: u32 = context.metadata
            .get("capability_checks")
            .and_then(|v| v.as_u64())
            .unwrap_or(0) as u32;
        let mut denials: u32 = context.metadata
            .get("capability_denials")
            .and_then(|v| v.as_u64())
            .unwrap_or(0) as u32;

        let mut first_denial: Option<SentinelError> = None;

        for action in &task.actions {
            // 1. Prompt injection scan (pre-capability check)
            if let Err(e) = injection::scan_parameters(&action.tool_name, &action.parameters) {
                tracing::warn!("CAPSEM: prompt injection blocked tool '{}'", action.tool_name);
                context.metadata.insert(
                    "security_violation".to_string(),
                    serde_json::json!({
                        "tool": action.tool_name,
                        "reason": e.to_string()
                    }),
                );
                denials += 1;
                checks += 1;
                if first_denial.is_none() {
                    first_denial = Some(e);
                }
                continue;
            }

            // 2. Capability policy check
            checks += 1;
            match self.policy_engine.verify_action(
                &task.id.to_string(),
                &action.tool_name,
                &action.action_type,
            ) {
                Ok(()) => {}
                Err(e) => {
                    denials += 1;
                    if first_denial.is_none() {
                        first_denial = Some(e);
                    }
                }
            }
        }

        // Write counters back into context so pipeline can surface them
        context.metadata.insert("capability_checks".to_string(), serde_json::json!(checks));
        context.metadata.insert("capability_denials".to_string(), serde_json::json!(denials));
        // Propagate the first denial (fail-fast on denied actions)
        if let Some(err) = first_denial {
            return Err(err);
        }

        Ok(())
    }
}
