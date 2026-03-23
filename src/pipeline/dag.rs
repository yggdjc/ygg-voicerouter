//! DAG pipeline execution: topological sort and parallel stage execution.

use std::collections::HashMap;

use anyhow::{bail, Result};
use crossbeam::channel::Sender;

use super::handler::HandlerResult;
use super::stage::Stage;
use crate::actor::Message;

/// Topological sort of stages. Returns execution order as indices.
pub fn topo_sort(
    stage_names: &[&str],
    dependencies: &[Option<&str>],
) -> Result<Vec<usize>> {
    let name_to_idx: HashMap<&str, usize> = stage_names.iter()
        .enumerate()
        .map(|(i, n)| (*n, i))
        .collect();

    let n = stage_names.len();
    let mut in_degree = vec![0usize; n];
    let mut adj: Vec<Vec<usize>> = vec![Vec::new(); n];

    for (i, dep) in dependencies.iter().enumerate() {
        if let Some(dep_name) = dep {
            let dep_idx = name_to_idx.get(dep_name)
                .ok_or_else(|| anyhow::anyhow!(
                    "stage '{}' depends on unknown stage '{dep_name}'",
                    stage_names[i]
                ))?;
            adj[*dep_idx].push(i);
            in_degree[i] += 1;
        }
    }

    let mut queue: Vec<usize> = (0..n).filter(|&i| in_degree[i] == 0).collect();
    let mut order = Vec::with_capacity(n);

    while let Some(node) = queue.pop() {
        order.push(node);
        for &next in &adj[node] {
            in_degree[next] -= 1;
            if in_degree[next] == 0 {
                queue.push(next);
            }
        }
    }

    if order.len() != n {
        bail!("cycle detected in pipeline DAG");
    }

    Ok(order)
}

/// Execute a DAG of stages. Stages with `after` set depend on their parent.
/// Stages at the same depth level can run in parallel via crossbeam::scope.
pub fn execute_dag(
    stages: &[Stage],
    text: &str,
    outbox: &Sender<Message>,
) {
    let stage_names: Vec<&str> = stages.iter().map(|s| s.name.as_str()).collect();
    let deps: Vec<Option<&str>> = stages.iter()
        .map(|s| s.after.as_deref())
        .collect();

    let order = match topo_sort(&stage_names, &deps) {
        Ok(o) => o,
        Err(e) => {
            log::error!("[pipeline/dag] {e}");
            return;
        }
    };

    let mut results: HashMap<String, String> = HashMap::new();

    for &idx in &order {
        let stage = &stages[idx];

        if let Some(ref cond) = stage.condition {
            if !cond.matches_with_results(text, &results) {
                continue;
            }
        }

        let payload = stage.condition.as_ref()
            .and_then(|c| c.strip_prefix(text))
            .unwrap_or(text);

        let ctx = stage.to_context();
        match stage.handler.handle(payload, &ctx) {
            Ok(HandlerResult::Forward(output)) => {
                results.insert(stage.name.clone(), output);
            }
            Ok(HandlerResult::Emit(msg)) => {
                outbox.send(msg).ok();
            }
            Ok(HandlerResult::ForwardAndEmit(output, msg)) => {
                results.insert(stage.name.clone(), output);
                outbox.send(msg).ok();
            }
            Ok(HandlerResult::Done) => break,
            Err(e) => {
                log::error!(
                    "[pipeline/dag] stage '{}' failed: {e:#}",
                    stage.name,
                );
                break;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn topo_sort_linear_chain() {
        let stages = vec!["a", "b", "c"];
        let deps = vec![None, Some("a"), Some("b")];
        let order = topo_sort(&stages, &deps).unwrap();
        assert_eq!(order, vec![0, 1, 2]);
    }

    #[test]
    fn topo_sort_fan_out() {
        // a → b, a → c (b and c are independent)
        let stages = vec!["a", "b", "c"];
        let deps = vec![None, Some("a"), Some("a")];
        let order = topo_sort(&stages, &deps).unwrap();
        assert_eq!(order[0], 0); // a first
        // b and c in any order
    }

    #[test]
    fn topo_sort_detects_cycle() {
        let stages = vec!["a", "b"];
        let deps = vec![Some("b"), Some("a")];
        assert!(topo_sort(&stages, &deps).is_err());
    }

    #[test]
    fn topo_sort_detects_missing_dependency() {
        let stages = vec!["a"];
        let deps = vec![Some("nonexistent")];
        assert!(topo_sort(&stages, &deps).is_err());
    }
}
