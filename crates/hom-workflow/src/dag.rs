//! DAG construction from workflow step definitions.

use std::collections::HashMap;

use petgraph::algo::toposort;
use petgraph::graph::{DiGraph, NodeIndex};

use hom_core::{HomError, HomResult};

use crate::parser::StepDef;

/// A directed acyclic graph of workflow steps.
pub struct WorkflowDag {
    pub graph: DiGraph<String, ()>,
    pub node_map: HashMap<String, NodeIndex>,
}

impl WorkflowDag {
    /// Build a DAG from step definitions.
    ///
    /// Each step becomes a node; `depends_on` relationships become edges.
    pub fn from_steps(steps: &[StepDef]) -> HomResult<Self> {
        let mut graph = DiGraph::new();
        let mut node_map = HashMap::new();

        // Add nodes
        for step in steps {
            let idx = graph.add_node(step.id.clone());
            node_map.insert(step.id.clone(), idx);
        }

        // Add edges (dependency → dependent)
        for step in steps {
            let target = node_map[&step.id];
            for dep_id in &step.depends_on {
                let source = node_map.get(dep_id).ok_or_else(|| {
                    HomError::WorkflowParseError(format!(
                        "step '{}' depends on unknown step '{dep_id}'",
                        step.id
                    ))
                })?;
                graph.add_edge(*source, target, ());
            }
        }

        let dag = Self { graph, node_map };
        dag.validate()?;
        Ok(dag)
    }

    /// Validate that the graph is acyclic.
    fn validate(&self) -> HomResult<()> {
        toposort(&self.graph, None).map_err(|_| HomError::WorkflowCycleDetected)?;
        Ok(())
    }

    /// Get a topological ordering of step IDs.
    pub fn topo_order(&self) -> HomResult<Vec<String>> {
        let sorted = toposort(&self.graph, None).map_err(|_| HomError::WorkflowCycleDetected)?;

        Ok(sorted
            .into_iter()
            .map(|idx| self.graph[idx].clone())
            .collect())
    }

    /// Get the step IDs that have no dependencies (roots).
    pub fn roots(&self) -> Vec<String> {
        self.graph
            .node_indices()
            .filter(|&idx| {
                self.graph
                    .neighbors_directed(idx, petgraph::Direction::Incoming)
                    .count()
                    == 0
            })
            .map(|idx| self.graph[idx].clone())
            .collect()
    }

    /// Get steps that are ready to run (all dependencies completed).
    pub fn ready_steps(&self, completed: &[String]) -> Vec<String> {
        self.graph
            .node_indices()
            .filter(|&idx| {
                let step_id = &self.graph[idx];
                // Not already completed
                if completed.contains(step_id) {
                    return false;
                }
                // All incoming neighbors are completed
                self.graph
                    .neighbors_directed(idx, petgraph::Direction::Incoming)
                    .all(|dep_idx| completed.contains(&self.graph[dep_idx]))
            })
            .map(|idx| self.graph[idx].clone())
            .collect()
    }
}
