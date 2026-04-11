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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parser::StepDef;

    fn step(id: &str, depends_on: &[&str]) -> StepDef {
        StepDef {
            id: id.to_string(),
            harness: "claude".to_string(),
            model: None,
            prompt: format!("run {id}"),
            depends_on: depends_on.iter().map(|dep| dep.to_string()).collect(),
            timeout: None,
            condition: None,
            retry: None,
            on_failure: None,
        }
    }

    #[test]
    fn dag_reports_roots_and_ready_steps() {
        let dag = WorkflowDag::from_steps(&[
            step("plan", &[]),
            step("impl", &["plan"]),
            step("review", &["plan"]),
            step("ship", &["impl", "review"]),
        ])
        .unwrap();

        let mut roots = dag.roots();
        roots.sort();
        assert_eq!(roots, vec!["plan".to_string()]);

        let mut initial_ready = dag.ready_steps(&[]);
        initial_ready.sort();
        assert_eq!(initial_ready, vec!["plan".to_string()]);

        let mut after_plan = dag.ready_steps(&["plan".to_string()]);
        after_plan.sort();
        assert_eq!(after_plan, vec!["impl".to_string(), "review".to_string()]);
    }

    #[test]
    fn dag_rejects_unknown_dependency() {
        let err = WorkflowDag::from_steps(&[step("impl", &["missing"])])
            .err()
            .unwrap()
            .to_string();
        assert!(err.contains("unknown step 'missing'"));
    }

    #[test]
    fn dag_rejects_cycles() {
        let err = WorkflowDag::from_steps(&[step("a", &["b"]), step("b", &["a"])])
            .err()
            .unwrap()
            .to_string();
        assert!(err.contains("cycle") || err.contains("Cycle"));
    }

    #[test]
    fn topo_order_places_dependencies_before_dependents() {
        let dag = WorkflowDag::from_steps(&[
            step("plan", &[]),
            step("impl", &["plan"]),
            step("review", &["impl"]),
        ])
        .unwrap();

        let order = dag.topo_order().unwrap();
        let plan_idx = order.iter().position(|id| id == "plan").unwrap();
        let impl_idx = order.iter().position(|id| id == "impl").unwrap();
        let review_idx = order.iter().position(|id| id == "review").unwrap();

        assert!(plan_idx < impl_idx);
        assert!(impl_idx < review_idx);
    }
}
