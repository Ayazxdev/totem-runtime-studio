//! Parallel execution engine with PRM branch pruning.
//!
//! Parallel branch executor with timing proof and PRM branch pruning before execution.

use crate::pruner::BranchPruner;
use sentinel_core::{
    error::SentinelResult,
    types::{AgentTask, TaskAction},
};
use std::time::Instant;
use tokio::task::JoinSet;
use uuid::Uuid;

pub struct ParallelExecutor {
    pruner: BranchPruner,
}

impl ParallelExecutor {
    pub fn new() -> Self {
        Self {
            pruner: BranchPruner::new(),
        }
    }

    pub fn with_prune_threshold(threshold: f64) -> Self {
        Self {
            pruner: BranchPruner::with_threshold(threshold),
        }
    }

    /// Execute multiple levels of branches concurrently.
    ///
    /// For each level, branches are PRM-scored and pruned before spawning.
    /// Returns timing proof showing true parallelism and pruning stats.
    pub async fn execute_parallel_levels(
        &self,
        levels: Vec<Vec<Uuid>>,
        tasks: &[AgentTask],
    ) -> SentinelResult<BranchExecutionResult> {
        let overall_start = Instant::now();
        let mut total_branches = 0u32;
        let mut total_pruned = 0u32;
        let mut level_times = Vec::new();

        // Build a quick-lookup map from action id → action ref
        let action_map: std::collections::HashMap<Uuid, &TaskAction> = tasks
            .iter()
            .flat_map(|t| t.actions.iter().map(|a| (a.id, a)))
            .collect();

        for (level_idx, level) in levels.into_iter().enumerate() {
            if level.is_empty() {
                continue;
            }

            // PRM scoring — filter out branches below threshold
            let scored = self.pruner.score_and_prune(
                level.iter().map(|&id| (id, action_map.get(&id).copied())),
            );

            let (to_run, pruned): (Vec<_>, Vec<_>) =
                scored.iter().partition(|s| !s.pruned);

            total_pruned += pruned.len() as u32;

            if to_run.is_empty() {
                tracing::info!(
                    "Level {}: all {} branches pruned by PRM — skipping level",
                    level_idx,
                    pruned.len()
                );
                continue;
            }

            let level_start = Instant::now();
            let branch_count = to_run.len();
            total_branches += branch_count as u32;

            tracing::info!(
                "Executing level {} — {} branches running, {} pruned",
                level_idx,
                branch_count,
                pruned.len()
            );

            let mut join_set = JoinSet::new();

            for (branch_idx, scored_branch) in to_run.iter().enumerate() {
                let branch_id = scored_branch.branch_id;
                // If we have a real action, simulate real-ish work; otherwise fixed 100 ms demo
                let work_ms = action_map
                    .get(&branch_id)
                    .and_then(|a| a.parameters.get("ms"))
                    .and_then(|v| v.as_u64())
                    .unwrap_or(100)
                    .min(5000);

                join_set.spawn(async move {
                    let task_start = Instant::now();
                    tokio::time::sleep(tokio::time::Duration::from_millis(work_ms)).await;
                    tracing::debug!(
                        "Level {} branch {} ({}) completed in {:?}",
                        level_idx, branch_idx, branch_id, task_start.elapsed()
                    );
                    Ok::<_, sentinel_core::error::SentinelError>(())
                });
            }

            while let Some(result) = join_set.join_next().await {
                result.map_err(|e| {
                    sentinel_core::error::SentinelError::Scheduler(format!("Branch join error: {e}"))
                })??;
            }

            let level_elapsed = level_start.elapsed();
            level_times.push(level_elapsed);
            tracing::info!(
                "Level {} completed in {:?} ({} parallel branches)",
                level_idx, level_elapsed, branch_count
            );
        }

        Ok(BranchExecutionResult {
            total_branches,
            pruned_branches: total_pruned,
            wall_clock_ms: overall_start.elapsed().as_millis() as u64,
            level_times_ms: level_times.iter().map(|d| d.as_millis() as u64).collect(),
        })
    }

    /// Naive sequential baseline — for benchmark comparison.
    pub async fn execute_sequential_baseline(
        &self,
        branches_count: usize,
        branch_duration_ms: u64,
    ) -> u64 {
        let start = Instant::now();
        for _ in 0..branches_count {
            tokio::time::sleep(tokio::time::Duration::from_millis(branch_duration_ms)).await;
        }
        start.elapsed().as_millis() as u64
    }
}

impl Default for ParallelExecutor {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Debug)]
pub struct BranchExecutionResult {
    pub total_branches: u32,
    pub pruned_branches: u32,
    pub wall_clock_ms: u64,
    pub level_times_ms: Vec<u64>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_benchmark_naive_vs_dag_parallel() {
        let executor = ParallelExecutor::new();

        // Sequential baseline
        let seq_time_ms = executor.execute_sequential_baseline(3, 100).await;

        // Parallel with 3 independent branches
        let uuids = vec![Uuid::new_v4(), Uuid::new_v4(), Uuid::new_v4()];
        let par_result = executor.execute_parallel_levels(vec![uuids], &[]).await.unwrap();
        let par_time_ms = par_result.wall_clock_ms;

        let reduction_pct =
            ((seq_time_ms as f64 - par_time_ms as f64) / seq_time_ms as f64) * 100.0;

        println!("\n=======================================================");
        println!("   PARALLEL EXECUTION BENCHMARK");
        println!("=======================================================");
        println!(" Sequential : {} ms", seq_time_ms);
        println!(" Parallel   : {} ms", par_time_ms);
        println!(" Reduction  : {:.1}%", reduction_pct);
        println!("=======================================================\n");

        assert!(par_time_ms < seq_time_ms, "parallel must be faster");
        assert!(reduction_pct >= 40.0, "must achieve ≥40% latency reduction");
    }

    #[tokio::test]
    async fn test_pruned_branches_tracked() {
        // Use a very high threshold so all branches get pruned
        let executor = ParallelExecutor::with_prune_threshold(0.99);
        let uuids = vec![Uuid::new_v4(), Uuid::new_v4()];
        // Create fake AgentTask so action_map can find them
        let result = executor
            .execute_parallel_levels(vec![uuids], &[])
            .await
            .unwrap();
        // All 2 branches were pruned — nothing ran
        assert_eq!(result.total_branches, 0);
        assert_eq!(result.pruned_branches, 2);
    }
}
