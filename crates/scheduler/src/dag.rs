//! DAG construction and dependency resolution.
//!
//! Full topological sort and parallel branch identification.

use sentinel_core::{error::{SentinelError, SentinelResult}, types::AgentTask};
use std::collections::{HashMap, HashSet};
use uuid::Uuid;

/// A directed acyclic graph of tasks.
pub struct TaskDAG {
    pub nodes: HashMap<Uuid, AgentTask>,
    pub edges: HashMap<Uuid, Vec<Uuid>>, // task_id -> dependencies
}

impl TaskDAG {
    pub fn new() -> Self {
        Self {
            nodes: HashMap::new(),
            edges: HashMap::new(),
        }
    }

    pub fn add_task(&mut self, task: AgentTask) {
        let task_id = task.id;
        let deps = task.dependencies.clone();
        self.nodes.insert(task_id, task);
        self.edges.insert(task_id, deps);
    }

    /// Returns tasks in topological order (dependencies first) with cycle detection.
    pub fn topological_sort(&self) -> SentinelResult<Vec<Uuid>> {
        let mut visited = HashSet::new();
        let mut rec_stack = HashSet::new();
        let mut result = Vec::new();

        for node_id in self.nodes.keys() {
            if !visited.contains(node_id) {
                self.visit_topo(*node_id, &mut visited, &mut rec_stack, &mut result)?;
            }
        }

        Ok(result)
    }

    fn visit_topo(
        &self,
        node: Uuid,
        visited: &mut HashSet<Uuid>,
        rec_stack: &mut HashSet<Uuid>,
        result: &mut Vec<Uuid>,
    ) -> SentinelResult<()> {
        visited.insert(node);
        rec_stack.insert(node);

        if let Some(deps) = self.edges.get(&node) {
            for dep in deps {
                if rec_stack.contains(dep) {
                    return Err(SentinelError::CyclicDependency);
                }
                if !visited.contains(dep) {
                    self.visit_topo(*dep, visited, rec_stack, result)?;
                }
            }
        }

        rec_stack.remove(&node);
        result.push(node);
        Ok(())
    }

    /// Identify independent branches that can execute concurrently using topological order.
    pub fn identify_parallel_branches(&self) -> Vec<Vec<Uuid>> {
        // Group tasks into levels: tasks at the same level have no dependencies on each other
        let mut levels: Vec<Vec<Uuid>> = Vec::new();
        let mut visited = HashSet::new();

        for task_id in self.nodes.keys() {
            if !visited.contains(task_id) {
                self.assign_level(*task_id, &mut levels, &mut visited);
            }
        }

        levels
    }

    fn assign_level(&self, task_id: Uuid, levels: &mut Vec<Vec<Uuid>>, visited: &mut HashSet<Uuid>) {
        if visited.contains(&task_id) {
            return;
        }

        let mut max_dep_level = 0;
        if let Some(deps) = self.edges.get(&task_id) {
            for dep in deps {
                self.assign_level(*dep, levels, visited);
                // Find which level this dependency is at
                for (level_idx, level) in levels.iter().enumerate() {
                    if level.contains(dep) {
                        max_dep_level = max_dep_level.max(level_idx + 1);
                        break;
                    }
                }
            }
        }

        // Ensure enough levels exist
        while levels.len() <= max_dep_level {
            levels.push(Vec::new());
        }

        levels[max_dep_level].push(task_id);
        visited.insert(task_id);
    }
}

impl Default for TaskDAG {
    fn default() -> Self {
        Self::new()
    }
}
