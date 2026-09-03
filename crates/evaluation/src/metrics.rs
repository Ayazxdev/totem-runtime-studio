//! Runtime metrics computation.

use sentinel_core::types::{AgentTask, ExecutionContext, ExecutionMetrics};

pub fn compute_metrics(_task: &AgentTask, context: &ExecutionContext) -> ExecutionMetrics {
    let get_u64 = |key: &str| -> u64 {
        context.metadata.get(key).and_then(|v| v.as_u64()).unwrap_or(0)
    };
    let get_u32 = |key: &str| -> u32 {
        get_u64(key) as u32
    };

    // Token count: estimate from message and tool result sizes (~4 chars per token)
    let msg_chars: usize  = context.messages.iter().map(|m| m.content.len()).sum();
    let tool_chars: usize = context.tool_results.iter().map(|r| r.output.to_string().len()).sum();
    let total_token_count = ((msg_chars + tool_chars) / 4) as u64;

    // Cost: $0.01 per 1k tokens (GPT-3.5-equivalent estimate)
    let estimated_cost_usd = total_token_count as f64 * 0.00001;

    // Latency: sum of individual tool invocation times
    let total_latency_ms: u64 = context.tool_results.iter().map(|r| r.latency_ms).sum();

    ExecutionMetrics {
        total_latency_ms,
        parallel_branches_executed: get_u32("parallel_branches"),
        branches_pruned: get_u32("branches_pruned"),
        total_token_count,
        estimated_cost_usd,
        tools_invoked: context.tool_results.len() as u32,
        capability_checks:  get_u32("capability_checks"),
        capability_denials: get_u32("capability_denials"),
        recovery_attempts:  get_u32("recovery_attempts"),
    }
}

