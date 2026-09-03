//! Local (in-process) tool execution.
//!
//! Local tool set — echo, add, multiply, hash, sleep (latency simulation).

use sentinel_core::error::{SentinelError, SentinelResult};
use std::collections::HashMap;

pub async fn invoke_local_tool(
    name: &str,
    parameters: &HashMap<String, serde_json::Value>,
) -> SentinelResult<serde_json::Value> {
    tracing::debug!("Invoking local tool: {} with {:?}", name, parameters);

    match name {
        "echo" => {
            let message = parameters
                .get("message")
                .and_then(|v| v.as_str())
                .unwrap_or("(no message)");
            Ok(serde_json::json!({ "echo": message }))
        }

        "add" => {
            let a = parameters.get("a").and_then(|v| v.as_f64()).unwrap_or(0.0);
            let b = parameters.get("b").and_then(|v| v.as_f64()).unwrap_or(0.0);
            Ok(serde_json::json!({ "sum": a + b, "a": a, "b": b }))
        }

        "multiply" => {
            let a = parameters.get("a").and_then(|v| v.as_f64()).unwrap_or(0.0);
            let b = parameters.get("b").and_then(|v| v.as_f64()).unwrap_or(0.0);
            Ok(serde_json::json!({ "product": a * b, "a": a, "b": b }))
        }

        "hash" => {
            // BLAKE3 hash of the input string — demonstrates crypto in tools
            let input = parameters
                .get("input")
                .and_then(|v| v.as_str())
                .unwrap_or("");
            let digest = blake3::hash(input.as_bytes());
            Ok(serde_json::json!({
                "input": input,
                "algorithm": "blake3",
                "digest": hex::encode(digest.as_bytes())
            }))
        }

        "sleep" => {
            // Latency simulation tool — useful for parallel execution benchmarks
            let ms = parameters
                .get("ms")
                .and_then(|v| v.as_u64())
                .unwrap_or(100)
                .min(5000); // cap at 5 seconds
            tokio::time::sleep(tokio::time::Duration::from_millis(ms)).await;
            Ok(serde_json::json!({ "slept_ms": ms }))
        }

        "env_info" => {
            // Safe read-only environment info (no secrets)
            Ok(serde_json::json!({
                "os": std::env::consts::OS,
                "arch": std::env::consts::ARCH,
                "family": std::env::consts::FAMILY,
            }))
        }

        _ => Err(SentinelError::ToolNotFound(format!(
            "Local tool '{}' not registered",
            name
        ))),
    }
}
