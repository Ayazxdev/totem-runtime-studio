//! Salience scoring for context compression.
//!
//! heuristic salience scorer with per-role and recency weighting.
//! Integration point: replace `score_message_salience` body with an ML model call
//! (e.g., LLMLingua-2 distillation, DACS scoping) for production use.

use sentinel_core::types::ContextMessage;

/// Score a message's importance for retention during compression (0.0–1.0).
///
/// Scoring factors:
///   1. Role weight: system > assistant > user (system prompts are most critical)
///   2. Content length: very short messages are often low-value filler
///   3. Keyword signal: messages containing structured output markers score higher
///   4. Recency bonus: applied externally by the caller using message index
pub fn score_message_salience(message: &ContextMessage) -> f64 {
    let mut score = 1.0_f64;

    // Factor 1: role-based weight
    score *= match message.role.as_str() {
        "system"    => 1.4,  // system prompts are critical — never drop if possible
        "assistant" => 1.1,  // assistant turns carry reasoning
        "user"      => 0.9,  // user turns can sometimes be summarised
        _           => 1.0,
    };

    // Factor 2: content length signal
    let len = message.content.len();
    if len < 20 {
        score *= 0.5; // very short = likely noise
    } else if len < 80 {
        score *= 0.8;
    } else if len > 2000 {
        score *= 1.2; // long structured content is usually worth keeping
    }

    // Factor 3: keyword signal — structured output markers
    let lower = message.content.to_lowercase();
    for keyword in &["result:", "output:", "error:", "```", "tool_call", "function", "json"] {
        if lower.contains(keyword) {
            score *= 1.15;
            break;
        }
    }

    // Clamp to [0.0, 1.0]
    score.clamp(0.0, 1.0)
}

/// Score a tool result's importance (0.0–1.0).
/// Tool results with larger outputs or error indicators are more salient.
pub fn score_tool_result_salience(output: &serde_json::Value) -> f64 {
    let s = output.to_string();
    let mut score = 0.7_f64;

    if s.contains("error") || s.contains("failed") || s.contains("denied") {
        score = 1.0; // errors are always important
    } else if s.len() > 500 {
        score = 0.85;
    } else if s.len() < 20 {
        score = 0.4;
    }

    score
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;

    fn msg(role: &str, content: &str) -> ContextMessage {
        ContextMessage {
            role: role.to_string(),
            content: content.to_string(),
            timestamp: Utc::now(),
        }
    }

    #[test]
    fn test_system_scores_highest() {
        let sys = msg("system", "You are a helpful assistant that executes tools.");
        let usr = msg("user", "Run the echo tool.");
        assert!(score_message_salience(&sys) > score_message_salience(&usr));
    }

    #[test]
    fn test_short_message_scores_low() {
        let short = msg("user", "ok");
        let long  = msg("user", "Please execute the echo tool with message 'hello world' and return the result to me.");
        assert!(score_message_salience(&short) < score_message_salience(&long));
    }

    #[test]
    fn test_structured_output_scores_higher() {
        let plain = msg("assistant", "I ran the tool.");
        let struc = msg("assistant", "```json\n{\"result\": \"hello\"}\n```");
        assert!(score_message_salience(&struc) >= score_message_salience(&plain));
    }
}
