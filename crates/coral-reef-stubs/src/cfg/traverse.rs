// SPDX-License-Identifier: AGPL-3.0-or-later
//! CFG traversals (reverse post-order, etc.).

use super::types::{CFG, NodeId};

impl<T> CFG<T> {
    /// Reverse post-order traversal from entry, useful for dataflow analysis.
    #[must_use]
    pub fn reverse_post_order(&self) -> Vec<NodeId> {
        let n = self.blocks.len();
        if n == 0 {
            return Vec::new();
        }
        let mut visited = vec![false; n];
        let mut order = Vec::with_capacity(n);
        self.rpo_dfs(0, &mut visited, &mut order);
        order.reverse();
        order
    }

    fn rpo_dfs(&self, node: NodeId, visited: &mut [bool], order: &mut Vec<NodeId>) {
        if node >= visited.len() || visited[node] {
            return;
        }
        visited[node] = true;
        for &succ in self.successors(node) {
            self.rpo_dfs(succ, visited, order);
        }
        order.push(node);
    }
}
