//! Sliding-window context compression with salience-weighted eviction.
//!
//! replaces naive proportional truncation with scored eviction.
//! Messages and tool results are evicted in ascending salience order,
//! preserving the most important context within the token budget.

use crate::scorer::{score_message_salience, score_tool_result_salience};
use sentinel_core::types::ExecutionContext;

/// Compress context to stay within `max_tokens`.
/// Uses salience-weighted eviction: lowest-scoring items dropped first.
pub fn compress_context(context: &mut ExecutionContext, max_tokens: usize) {
    let current = estimate_token_count(context);
    if current <= max_tokens {
        return; // nothing to do
    }

    tracing::info!(
        "Context compression: {} tokens → target {} tokens (salience-weighted eviction)",
        current, max_tokens
    );

    // Score and sort messages ascending by salience (lowest first = evict first)
    // Apply a recency bonus: newer messages get a slight boost
    let msg_count = context.messages.len();
    let mut scored_msgs: Vec<(usize, f64)> = context
        .messages
        .iter()
        .enumerate()
        .map(|(i, msg)| {
            let base = score_message_salience(msg);
            let recency_bonus = (i as f64 / msg_count.max(1) as f64) * 0.1;
            (i, base + recency_bonus)
        })
        .collect();
    scored_msgs.sort_by(|a, b| a.1.partial_cmp(&b.1).unwrap());

    // Score tool results
    let tr_count = context.tool_results.len();
    let mut scored_results: Vec<(usize, f64)> = context
        .tool_results
        .iter()
        .enumerate()
        .map(|(i, tr)| {
            let base = score_tool_result_salience(&tr.output);
            let recency_bonus = (i as f64 / tr_count.max(1) as f64) * 0.1;
            (i, base + recency_bonus)
        })
        .collect();
    scored_results.sort_by(|a, b| a.1.partial_cmp(&b.1).unwrap());

    // Alternate evicting messages and tool results (lowest score first)
    // until we're within budget
    let mut msg_ptr = 0usize;
    let mut tr_ptr  = 0usize;
    let mut to_drop_msgs: Vec<usize> = Vec::new();
    let mut to_drop_results: Vec<usize> = Vec::new();

    let mut remaining = current;

    while remaining > max_tokens {
        let can_drop_msg = msg_ptr < scored_msgs.len();
        let can_drop_tr  = tr_ptr  < scored_results.len();

        if !can_drop_msg && !can_drop_tr {
            break;
        }

        // Pick the lowest-score item across both lists
        let drop_msg = match (can_drop_msg, can_drop_tr) {
            (true, false) => true,
            (false, true) => false,
            (true, true)  => scored_msgs[msg_ptr].1 <= scored_results[tr_ptr].1,
            (false, false) => break,
        };

        if drop_msg {
            let idx = scored_msgs[msg_ptr].0;
            let tokens = context.messages[idx].content.len() / 4;
            to_drop_msgs.push(idx);
            remaining = remaining.saturating_sub(tokens.max(1));
            msg_ptr += 1;
        } else {
            let idx = scored_results[tr_ptr].0;
            let tokens = context.tool_results[idx].output.to_string().len() / 4;
            to_drop_results.push(idx);
            remaining = remaining.saturating_sub(tokens.max(1));
            tr_ptr += 1;
        }
    }

    // Remove in reverse index order so indices stay valid
    to_drop_msgs.sort_unstable();
    for &idx in to_drop_msgs.iter().rev() {
        context.messages.remove(idx);
    }
    to_drop_results.sort_unstable();
    for &idx in to_drop_results.iter().rev() {
        context.tool_results.remove(idx);
    }

    tracing::info!(
        "Context compression done: dropped {} messages, {} tool results → {} tokens remaining",
        to_drop_msgs.len(),
        to_drop_results.len(),
        estimate_token_count(context)
    );
}

/// Naive token count: ~4 chars per token (GPT-style).
pub fn estimate_token_count(context: &ExecutionContext) -> usize {
    let msg_chars: usize = context.messages.iter().map(|m| m.content.len()).sum();
    let tr_chars:  usize = context.tool_results.iter().map(|r| r.output.to_string().len()).sum();
    (msg_chars + tr_chars) / 4
}
