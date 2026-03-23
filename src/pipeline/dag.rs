//! DAG pipeline execution: topological sort and parallel stage execution.

use std::collections::HashMap;

use anyhow::{bail, Result};

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
