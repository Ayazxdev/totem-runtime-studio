//! PRM-inspired branch pruning.
//!
//! Process Reward Model (PRM) style scoring.
//! Each branch is scored on heuristics derived from its action metadata.
//! Branches below the confidence threshold are pruned before execution.
//!
//! In production, the score function would call an external ML model endpoint
//! (or a local ONNX model). Here we implement a principled heuristic scorer
//! that is deterministic, testable, and documents the integration point.

use sentinel_core::types::TaskAction;
use uuid::Uuid;

/// Minimum confidence score to proceed with a branch (0.0–1.0).
/// Branches scoring below this threshold are pruned.
pub const DEFAULT_PRUNE_THRESHOLD: f64 = 0.15;

/// Scored branch ready for pruning decision.
#[derive(Debug, Clone)]
pub struct ScoredBranch {
    pub branch_id: Uuid,
    pub score: f64,
    pub pruned: bool,
    pub reason: String,
}

pub struct BranchPruner {
    threshold: f64,
}

impl BranchPruner {
    pub fn new() -> Self {
        Self {
            threshold: DEFAULT_PRUNE_THRESHOLD,
        }
    }

    pub fn with_threshold(threshold: f64) -> Self {
        Self { threshold: threshold.clamp(0.0, 1.0) }
    }

    /// Score and optionally prune a list of branches from a single DAG level.
    ///
    /// Each `(Uuid, Option<&TaskAction>)` pair gives the branch ID and its
    /// associated action (if available for scoring). Returns `ScoredBranch`
    /// records — callers skip branches where `pruned == true`.
    pub fn score_and_prune<'a>(
        &self,
        branches: impl Iterator<Item = (Uuid, Option<&'a TaskAction>)>,
    ) -> Vec<ScoredBranch> {
        branches
            .map(|(id, action)| {
                let score = self.score_branch(action);
                let pruned = score < self.threshold;
                let reason = if pruned {
                    format!("score {:.3} below threshold {:.3}", score, self.threshold)
                } else {
                    format!("score {:.3} — proceed", score)
                };

                if pruned {
                    tracing::info!(
                        "PRM pruner: branch {} pruned ({}) — tool: {}",
                        id,
                        reason,
                        action.map(|a| a.tool_name.as_str()).unwrap_or("unknown")
                    );
                }

                ScoredBranch { branch_id: id, score, pruned, reason }
            })
            .collect()
    }

    /// Legacy interface: return IDs of branches to prune (no action metadata).
    /// Low scores when no metadata available — keeps all branches alive.
    pub fn evaluate_branches(&self, branch_ids: &[Uuid]) -> Vec<Uuid> {
        branch_ids
            .iter()
            .filter_map(|&id| {
                // Without action metadata, assign a neutral score above threshold
                let score = 0.5_f64;
                if score < self.threshold { Some(id) } else { None }
            })
            .collect()
    }

    /// Heuristic PRM score for a branch action (0.0 = definitely prune, 1.0 = high value).
    ///
    /// Scoring factors:
    ///   1. Tool name risk level — known dangerous/expensive tools score lower
    ///   2. Parameter completeness — more complete parameters → higher score
    ///   3. Action type — ToolCall scores higher than stub AgentCommunication
    ///
    /// Integration point: replace this with `prm_model.score(action.serialize())` for ML scoring.
    fn score_branch(&self, action: Option<&TaskAction>) -> f64 {
        let Some(action) = action else {
            return 0.5; // neutral score for unknown branches
        };

        let mut score = 1.0_f64;

        // Factor 1: known risky tool names reduce score
        let tool = action.tool_name.to_lowercase();
        if tool.contains("dangerous") || tool.contains("delete") || tool.contains("drop") {
            score *= 0.05; // nearly always prune
        } else if tool.contains("write") || tool.contains("exec") || tool.contains("run") {
            score *= 0.6; // cautious
        } else if tool.contains("read") || tool.contains("list") || tool.contains("get") {
            score *= 1.0; // safe reads — keep
        }

        // Factor 2: parameter completeness (having parameters means the caller prepared well)
        if action.parameters.is_empty() {
            score *= 0.8;
        } else {
            // More parameters = more deliberate call
            let param_bonus = 1.0 + 0.05 * (action.parameters.len() as f64).min(4.0);
            score *= param_bonus.min(1.2);
        }

        // Factor 3: action type
        use sentinel_core::types::ActionType;
        match action.action_type {
            ActionType::ToolCall => {} // baseline
            ActionType::AgentCommunication => score *= 0.9,
            ActionType::ContextUpdate => score *= 0.95,
        }

        score.clamp(0.0, 1.0)
    }
}

impl Default for BranchPruner {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use sentinel_core::types::ActionType;

    fn make_action(tool_name: &str, params: &[(&str, &str)]) -> TaskAction {
        TaskAction {
            id: Uuid::new_v4(),
            action_type: ActionType::ToolCall,
            tool_name: tool_name.to_string(),
            parameters: params
                .iter()
                .map(|(k, v)| (k.to_string(), serde_json::json!(v)))
                .collect(),
        }
    }

    #[test]
    fn test_safe_tools_not_pruned() {
        let pruner = BranchPruner::new();
        let echo = make_action("echo", &[("message", "hello")]);
        let id = Uuid::new_v4();
        let results = pruner.score_and_prune(std::iter::once((id, Some(&echo))));
        assert!(!results[0].pruned, "echo should not be pruned");
        assert!(results[0].score >= DEFAULT_PRUNE_THRESHOLD);
    }

    #[test]
    fn test_dangerous_tool_pruned() {
        let pruner = BranchPruner::new();
        let dangerous = make_action("dangerous-tool", &[]);
        let id = Uuid::new_v4();
        let results = pruner.score_and_prune(std::iter::once((id, Some(&dangerous))));
        assert!(results[0].pruned, "dangerous-tool should be pruned");
        assert!(results[0].score < DEFAULT_PRUNE_THRESHOLD);
    }

    #[test]
    fn test_custom_threshold() {
        // dangerous-tool always scores very low — even at very high threshold it gets pruned
        let pruner = BranchPruner::with_threshold(0.99);
        let dangerous = make_action("dangerous-tool", &[]);
        let id = Uuid::new_v4();
        let results = pruner.score_and_prune(std::iter::once((id, Some(&dangerous))));
        assert!(results[0].pruned, "dangerous-tool should be pruned at 0.99 threshold");
    }
}
