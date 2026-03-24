// SPDX-License-Identifier: AGPL-3.0-only
//! Control-flow graph — replacement for `compiler::cfg`.
//!
//! Provides a directed graph over basic blocks with predecessor/successor
//! tracking, used for dominance analysis, loop detection, and
//! instruction scheduling.
//!
//! Dominator analysis lives in the `dom` submodule and is computed lazily
//! on first access via any dominance/loop query method.

pub(crate) mod dom;

use std::cell::RefCell;
use std::ops::{Index, IndexMut};

use crate::fxhash::FxHashMap;

/// A node index in the CFG.
pub type NodeId = usize;

/// Edge in the control-flow graph.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Edge {
    /// Source node.
    pub from: NodeId,
    /// Destination node.
    pub to: NodeId,
}

/// Control-flow graph over basic blocks of type `T`.
///
/// Supports O(1) predecessor/successor lookup per node and iterates
/// blocks in insertion order by default.
#[derive(Debug)]
pub struct CFG<T> {
    pub(crate) blocks: Vec<T>,
    pub(crate) successors: FxHashMap<NodeId, Vec<NodeId>>,
    pub(crate) predecessors: FxHashMap<NodeId, Vec<NodeId>>,
    pub(crate) dom_analysis: RefCell<Option<dom::DomAnalysis>>,
}

impl<T> CFG<T> {
    /// Number of blocks in the CFG.
    #[must_use]
    pub fn len(&self) -> usize {
        self.blocks.len()
    }

    /// Whether the CFG has no blocks.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.blocks.is_empty()
    }

    /// Get a block by index.
    #[must_use]
    pub fn block(&self, id: NodeId) -> Option<&T> {
        self.blocks.get(id)
    }

    /// Get a mutable block by index.
    pub fn block_mut(&mut self, id: NodeId) -> Option<&mut T> {
        self.blocks.get_mut(id)
    }

    /// Successors of a node.
    #[must_use]
    pub fn successors(&self, id: NodeId) -> &[NodeId] {
        self.successors
            .get(&id)
            .map_or(&[], std::vec::Vec::as_slice)
    }

    /// Predecessors of a node.
    #[must_use]
    pub fn predecessors(&self, id: NodeId) -> &[NodeId] {
        self.predecessors
            .get(&id)
            .map_or(&[], std::vec::Vec::as_slice)
    }

    /// Iterate over blocks.
    pub fn iter(&self) -> std::slice::Iter<'_, T> {
        self.blocks.iter()
    }

    /// Iterate mutably over blocks.
    pub fn iter_mut(&mut self) -> std::slice::IterMut<'_, T> {
        self.blocks.iter_mut()
    }

    /// Entry node (first block). Returns `None` if empty.
    #[must_use]
    pub fn entry(&self) -> Option<NodeId> {
        if self.blocks.is_empty() {
            None
        } else {
            Some(0)
        }
    }

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

    /// Push a new block onto the CFG, returning its node ID.
    pub fn push(&mut self, block: T) -> NodeId {
        let id = self.blocks.len();
        self.blocks.push(block);
        id
    }

    /// Set edge from `from` to `to`.
    pub fn add_edge(&mut self, from: NodeId, to: NodeId) {
        self.successors.entry(from).or_default().push(to);
        self.predecessors.entry(to).or_default().push(from);
    }

    /// Get blocks as slice.
    #[must_use]
    pub fn blocks(&self) -> &[T] {
        &self.blocks
    }

    /// Predecessor node indices (alias for `predecessors`).
    #[must_use]
    pub fn pred_indices(&self, id: NodeId) -> &[NodeId] {
        self.predecessors(id)
    }

    /// Successor node indices (alias for `successors`).
    #[must_use]
    pub fn succ_indices(&self, id: NodeId) -> &[NodeId] {
        self.successors(id)
    }

    /// Mutable reference to the blocks vec.
    pub const fn blocks_mut(&mut self) -> &mut Vec<T> {
        &mut self.blocks
    }

    /// Remove all edges incident to a given block and invalidate
    /// dominator analysis. Callers must also clear the block's content.
    pub fn disconnect_block(&mut self, id: NodeId) {
        // Remove id from all successors' predecessor lists
        if let Some(succs) = self.successors.remove(&id) {
            for s in succs {
                if let Some(preds) = self.predecessors.get_mut(&s) {
                    preds.retain(|&p| p != id);
                }
            }
        }
        // Remove id from all predecessors' successor lists
        if let Some(preds) = self.predecessors.remove(&id) {
            for p in preds {
                if let Some(succs) = self.successors.get_mut(&p) {
                    succs.retain(|&s| s != id);
                }
            }
        }
        // Cached dominator analysis is now stale
        *self.dom_analysis.borrow_mut() = None;
    }

    /// Drain blocks from the CFG.
    pub fn drain(&mut self) -> std::vec::Drain<'_, T> {
        self.blocks.drain(..)
    }
}

impl<T> Index<usize> for CFG<T> {
    type Output = T;
    fn index(&self, idx: usize) -> &T {
        &self.blocks[idx]
    }
}

impl<T> IndexMut<usize> for CFG<T> {
    fn index_mut(&mut self, idx: usize) -> &mut T {
        &mut self.blocks[idx]
    }
}

impl<'a, T> IntoIterator for &'a CFG<T> {
    type Item = &'a T;
    type IntoIter = std::slice::Iter<'a, T>;

    fn into_iter(self) -> Self::IntoIter {
        self.blocks.iter()
    }
}

impl<'a, T> IntoIterator for &'a mut CFG<T> {
    type Item = &'a mut T;
    type IntoIter = std::slice::IterMut<'a, T>;

    fn into_iter(self) -> Self::IntoIter {
        self.blocks.iter_mut()
    }
}

impl<T> Default for CFG<T> {
    fn default() -> Self {
        Self {
            blocks: Vec::new(),
            successors: FxHashMap::default(),
            predecessors: FxHashMap::default(),
            dom_analysis: RefCell::new(None),
        }
    }
}

/// Incremental builder for a [`CFG`].
#[derive(Debug)]
pub struct CFGBuilder<T> {
    blocks: Vec<T>,
    edges: Vec<Edge>,
}

impl<T> CFGBuilder<T> {
    /// Create a new builder.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            blocks: Vec::new(),
            edges: Vec::new(),
        }
    }

    /// Add a block, returning its [`NodeId`].
    pub fn add_block(&mut self, block: T) -> NodeId {
        let id = self.blocks.len();
        self.blocks.push(block);
        id
    }

    /// Add a node (alias for `add_block`). For Label-keyed CFGs, pass the block;
    /// the node id is the insertion index.
    pub fn add_node(&mut self, block: T) -> NodeId {
        self.add_block(block)
    }

    /// Get the built CFG (alias for build).
    #[must_use]
    pub fn as_cfg(self) -> CFG<T> {
        self.build()
    }

    /// Add a directed edge from `from` to `to`.
    pub fn add_edge(&mut self, from: NodeId, to: NodeId) {
        self.edges.push(Edge { from, to });
    }

    /// Number of blocks.
    #[must_use]
    pub fn block_count(&self) -> usize {
        self.blocks.len()
    }

    /// Number of edges.
    #[must_use]
    pub fn edge_count(&self) -> usize {
        self.edges.len()
    }

    /// Debug-print edges.
    #[must_use]
    pub fn edges_debug(&self) -> Vec<(usize, usize)> {
        self.edges.iter().map(|e| (e.from, e.to)).collect()
    }

    /// Build the CFG, consuming the builder.
    #[must_use]
    pub fn build(self) -> CFG<T> {
        let mut successors: FxHashMap<NodeId, Vec<NodeId>> = FxHashMap::default();
        let mut predecessors: FxHashMap<NodeId, Vec<NodeId>> = FxHashMap::default();

        for edge in &self.edges {
            successors.entry(edge.from).or_default().push(edge.to);
            predecessors.entry(edge.to).or_default().push(edge.from);
        }

        CFG {
            blocks: self.blocks,
            successors,
            predecessors,
            dom_analysis: RefCell::new(None),
        }
    }
}

impl<T> Default for CFGBuilder<T> {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use std::collections::VecDeque;

    use super::*;

    #[test]
    fn test_empty_cfg() {
        let cfg: CFG<&str> = CFG::default();
        assert!(cfg.is_empty());
        assert_eq!(cfg.len(), 0);
        assert_eq!(cfg.entry(), None);
    }

    #[test]
    fn test_build_linear_cfg() {
        let mut builder = CFGBuilder::new();
        let a = builder.add_block("entry");
        let b = builder.add_block("body");
        let c = builder.add_block("exit");
        builder.add_edge(a, b);
        builder.add_edge(b, c);
        let cfg = builder.build();

        assert_eq!(cfg.len(), 3);
        assert_eq!(cfg.entry(), Some(0));
        assert_eq!(cfg.block(a), Some(&"entry"));
        assert_eq!(cfg.successors(a), &[b]);
        assert_eq!(cfg.predecessors(b), &[a]);
        assert_eq!(cfg.predecessors(c), &[b]);
    }

    #[test]
    fn test_diamond_cfg() {
        let mut builder = CFGBuilder::new();
        let entry = builder.add_block("entry");
        let left = builder.add_block("left");
        let right = builder.add_block("right");
        let merge = builder.add_block("merge");
        builder.add_edge(entry, left);
        builder.add_edge(entry, right);
        builder.add_edge(left, merge);
        builder.add_edge(right, merge);
        let cfg = builder.build();

        assert_eq!(cfg.successors(entry).len(), 2);
        assert_eq!(cfg.predecessors(merge).len(), 2);
    }

    #[test]
    fn test_reverse_post_order() {
        let mut builder = CFGBuilder::new();
        let a = builder.add_block("a");
        let b = builder.add_block("b");
        let c = builder.add_block("c");
        builder.add_edge(a, b);
        builder.add_edge(a, c);
        builder.add_edge(b, c);
        let cfg = builder.build();

        let rpo = cfg.reverse_post_order();
        assert_eq!(rpo.len(), 3);
        assert_eq!(rpo[0], a);
        let b_pos = rpo
            .iter()
            .position(|&n| n == b)
            .expect("reverse post-order should contain b");
        let c_pos = rpo
            .iter()
            .position(|&n| n == c)
            .expect("reverse post-order should contain c");
        assert!(b_pos < c_pos);
    }

    #[test]
    fn test_loop_cfg() {
        let mut builder = CFGBuilder::new();
        let entry = builder.add_block("entry");
        let header = builder.add_block("header");
        let body = builder.add_block("body");
        let exit = builder.add_block("exit");
        builder.add_edge(entry, header);
        builder.add_edge(header, body);
        builder.add_edge(header, exit);
        builder.add_edge(body, header);
        let cfg = builder.build();

        assert_eq!(cfg.successors(header).len(), 2);
        assert!(cfg.predecessors(header).contains(&body));
    }

    #[test]
    fn test_iter() {
        let mut builder = CFGBuilder::new();
        builder.add_block("a");
        builder.add_block("b");
        let cfg = builder.build();

        let blocks: Vec<_> = cfg.iter().collect();
        assert_eq!(blocks.len(), 2);
        assert_eq!(blocks[0], &"a");
        assert_eq!(blocks[1], &"b");
    }

    #[test]
    fn test_dominator_linear() {
        let mut builder = CFGBuilder::new();
        let a = builder.add_block("entry");
        let b = builder.add_block("body");
        let c = builder.add_block("exit");
        builder.add_edge(a, b);
        builder.add_edge(b, c);
        let cfg = builder.build();

        assert_eq!(cfg.dom_parent_index(a), None);
        assert_eq!(cfg.dom_parent_index(b), Some(a));
        assert_eq!(cfg.dom_parent_index(c), Some(b));

        assert!(cfg.dominates(a, b));
        assert!(cfg.dominates(a, c));
        assert!(cfg.dominates(b, c));
        assert!(!cfg.dominates(b, a));
        assert!(!cfg.dominates(c, a));

        assert!(cfg.is_dominated_by(b, a));
        assert!(cfg.is_dominated_by(c, b));

        assert_eq!(cfg.idom(b), Some(a));
        assert_eq!(cfg.idom(c), Some(b));

        assert!(!cfg.has_loop());
        assert_eq!(cfg.loop_depth(a), 0);
        assert_eq!(cfg.loop_depth(b), 0);
        assert_eq!(cfg.loop_depth(c), 0);
    }

    #[test]
    fn test_dominator_diamond() {
        let mut builder = CFGBuilder::new();
        let entry = builder.add_block("entry");
        let left = builder.add_block("left");
        let right = builder.add_block("right");
        let merge = builder.add_block("merge");
        builder.add_edge(entry, left);
        builder.add_edge(entry, right);
        builder.add_edge(left, merge);
        builder.add_edge(right, merge);
        let cfg = builder.build();

        assert_eq!(cfg.dom_parent_index(entry), None);
        assert_eq!(cfg.dom_parent_index(left), Some(entry));
        assert_eq!(cfg.dom_parent_index(right), Some(entry));
        assert_eq!(cfg.dom_parent_index(merge), Some(entry));

        assert!(cfg.dominates(entry, merge));
        assert!(cfg.dominates(entry, left));
        assert!(cfg.dominates(entry, right));
        assert!(!cfg.dominates(left, right));
        assert!(!cfg.dominates(right, left));

        assert!(!cfg.has_loop());
    }

    #[test]
    fn test_dominator_loop() {
        let mut builder = CFGBuilder::new();
        let entry = builder.add_block("entry");
        let header = builder.add_block("header");
        let body = builder.add_block("body");
        let exit = builder.add_block("exit");
        builder.add_edge(entry, header);
        builder.add_edge(header, body);
        builder.add_edge(header, exit);
        builder.add_edge(body, header);
        let cfg = builder.build();

        assert!(cfg.has_loop());
        assert!(cfg.is_loop_header(header));
        assert_eq!(cfg.loop_depth(header), 1);
        assert_eq!(cfg.loop_depth(body), 1);
        assert_eq!(cfg.loop_depth(entry), 0);
        assert_eq!(cfg.loop_depth(exit), 0);

        assert_eq!(cfg.dom_parent_index(entry), None);
        assert_eq!(cfg.dom_parent_index(header), Some(entry));
        assert_eq!(cfg.dom_parent_index(body), Some(header));
        assert_eq!(cfg.dom_parent_index(exit), Some(header));

        assert!(cfg.dominates(entry, header));
        assert!(cfg.dominates(header, body));
        assert!(!cfg.dominates(body, header));
    }

    #[test]
    fn test_dom_dfs_pre_index() {
        let mut builder = CFGBuilder::new();
        let a = builder.add_block("a");
        let b = builder.add_block("b");
        let c = builder.add_block("c");
        builder.add_edge(a, b);
        builder.add_edge(a, c);
        builder.add_edge(b, c);
        let cfg = builder.build();

        let rpo = cfg.reverse_post_order();
        assert_eq!(rpo[0], a);

        let pre_a = cfg.dom_dfs_pre_index(a);
        let pre_b = cfg.dom_dfs_pre_index(b);
        let pre_c = cfg.dom_dfs_pre_index(c);

        assert_eq!(pre_a, 0);
        assert!(pre_b > 0);
        assert!(pre_c > 0);
        assert!(pre_b != pre_c);
    }

    #[test]
    fn test_dominator_nested_loops() {
        let mut builder = CFGBuilder::new();
        let entry = builder.add_block("entry");
        let outer_header = builder.add_block("outer_header");
        let outer_body = builder.add_block("outer_body");
        let inner_header = builder.add_block("inner_header");
        let inner_body = builder.add_block("inner_body");
        let exit = builder.add_block("exit");

        builder.add_edge(entry, outer_header);
        builder.add_edge(outer_header, outer_body);
        builder.add_edge(outer_header, exit);
        builder.add_edge(outer_body, inner_header);
        builder.add_edge(inner_header, inner_body);
        builder.add_edge(inner_body, inner_header);
        builder.add_edge(outer_body, outer_header);

        let cfg = builder.build();

        assert!(cfg.has_loop());
        assert!(cfg.is_loop_header(inner_header));
        assert!(cfg.is_loop_header(outer_header));
        assert!(cfg.loop_depth(inner_header) >= 1);
        assert!(cfg.loop_depth(inner_body) >= 1);
        assert!(cfg.loop_depth(outer_body) >= 1);
        assert!(cfg.loop_depth(outer_header) >= 1);
        assert_eq!(cfg.loop_depth(entry), 0);
        assert_eq!(cfg.loop_depth(exit), 0);

        assert!(cfg.dominates(entry, outer_header));
        assert!(cfg.dominates(outer_header, inner_header));
        assert!(cfg.dominates(inner_header, inner_body));
    }

    #[test]
    fn test_irreducible_control_flow() {
        let mut builder = CFGBuilder::new();
        let a = builder.add_block("a");
        let b = builder.add_block("b");
        let c = builder.add_block("c");
        builder.add_edge(a, b);
        builder.add_edge(b, c);
        builder.add_edge(c, b);
        builder.add_edge(c, a);

        let cfg = builder.build();
        assert!(cfg.has_loop());
        assert!(cfg.is_loop_header(b));
        assert!(cfg.predecessors(b).contains(&c));
        assert!(cfg.predecessors(a).contains(&c));
    }

    #[test]
    fn test_complex_diamond_with_merge() {
        let mut builder = CFGBuilder::new();
        let entry = builder.add_block("entry");
        let left1 = builder.add_block("left1");
        let right1 = builder.add_block("right1");
        let left2 = builder.add_block("left2");
        let right2 = builder.add_block("right2");
        let merge = builder.add_block("merge");
        let exit = builder.add_block("exit");

        builder.add_edge(entry, left1);
        builder.add_edge(entry, right1);
        builder.add_edge(left1, left2);
        builder.add_edge(right1, right2);
        builder.add_edge(left2, merge);
        builder.add_edge(right2, merge);
        builder.add_edge(merge, exit);

        let cfg = builder.build();

        assert_eq!(cfg.predecessors(merge).len(), 2);
        assert!(cfg.predecessors(merge).contains(&left2));
        assert!(cfg.predecessors(merge).contains(&right2));

        assert_eq!(cfg.dom_parent_index(entry), None);
        assert_eq!(cfg.dom_parent_index(merge), Some(entry));
        assert!(cfg.dominates(entry, merge));
        assert!(cfg.dominates(entry, left1));
        assert!(cfg.dominates(entry, right1));
        assert!(cfg.dominates(entry, left2));
        assert!(cfg.dominates(entry, right2));
        assert!(cfg.dominates(entry, exit));

        assert!(!cfg.has_loop());
    }

    #[test]
    fn test_cfg_push_and_add_edge_direct() {
        let mut cfg: CFG<&str> = CFG::default();
        let a = cfg.push("a");
        let b = cfg.push("b");
        cfg.add_edge(a, b);
        assert_eq!(cfg.successors(a), &[b]);
        assert_eq!(cfg.predecessors(b), &[a]);
    }

    #[test]
    fn test_cfg_block_mut() {
        let mut builder = CFGBuilder::new();
        let a = builder.add_block("original");
        builder.add_block("b");
        let mut cfg = builder.build();
        *cfg.block_mut(a)
            .expect("block a was inserted and must exist") = "modified";
        assert_eq!(cfg.block(a), Some(&"modified"));
    }

    #[test]
    fn test_cfg_drain() {
        let mut builder = CFGBuilder::new();
        builder.add_block("a");
        builder.add_block("b");
        let mut cfg = builder.build();
        assert_eq!(cfg.drain().count(), 2);
        assert!(cfg.is_empty());
    }

    #[test]
    fn test_cfg_index_trait() {
        let mut builder = CFGBuilder::new();
        builder.add_block("x");
        builder.add_block("y");
        let cfg = builder.build();
        assert_eq!(cfg[0], "x");
        assert_eq!(cfg[1], "y");
    }

    #[test]
    fn test_cfg_index_mut_trait() {
        let mut builder = CFGBuilder::new();
        builder.add_block(1);
        builder.add_block(2);
        let mut cfg = builder.build();
        cfg[0] = 10;
        assert_eq!(cfg[0], 10);
    }

    #[test]
    fn test_cfg_into_iter() {
        let mut builder = CFGBuilder::new();
        builder.add_block("a");
        builder.add_block("b");
        let cfg = builder.build();
        assert_eq!((&cfg).into_iter().count(), 2);
    }

    #[test]
    fn test_cfgbuilder_edges_debug() {
        let mut builder = CFGBuilder::new();
        let a = builder.add_block("a");
        let b = builder.add_block("b");
        builder.add_edge(a, b);
        let edges = builder.edges_debug();
        assert_eq!(edges, vec![(a, b)]);
    }

    #[test]
    fn test_empty_cfg_reverse_post_order() {
        let cfg: CFG<&str> = CFG::default();
        let rpo = cfg.reverse_post_order();
        assert!(rpo.is_empty());
    }

    #[test]
    fn test_single_block_cfg_dominance() {
        let mut builder = CFGBuilder::new();
        let solo = builder.add_block("solo");
        let cfg = builder.build();

        assert_eq!(cfg.len(), 1);
        assert_eq!(cfg.dom_parent_index(solo), None);
        assert!(cfg.dominates(solo, solo));
        assert!(!cfg.dominates(solo + 1, solo));
        assert!(!cfg.dominates(solo, solo + 1));
        assert!(!cfg.has_loop());
        assert_eq!(cfg.loop_depth(solo), 0);
        assert_eq!(cfg.loop_header_index(solo), None);
    }

    #[test]
    fn test_dominates_out_of_range_ids() {
        let mut builder = CFGBuilder::new();
        let a = builder.add_block("a");
        let b = builder.add_block("b");
        builder.add_edge(a, b);
        let cfg = builder.build();

        assert!(!cfg.dominates(99, 0));
        assert!(!cfg.dominates(0, 99));
    }

    #[test]
    fn test_loop_header_index_returns_header_for_body() {
        let mut builder = CFGBuilder::new();
        let entry = builder.add_block("entry");
        let header = builder.add_block("header");
        let body = builder.add_block("body");
        let exit = builder.add_block("exit");
        builder.add_edge(entry, header);
        builder.add_edge(header, body);
        builder.add_edge(header, exit);
        builder.add_edge(body, header);
        let cfg = builder.build();

        assert_eq!(cfg.loop_header_index(body), Some(header));
    }

    #[test]
    fn empty_cfg_dom_queries_are_safe() {
        let cfg: CFG<()> = CFG::default();
        assert!(!cfg.has_loop());
        assert_eq!(cfg.loop_depth(0), 0);
        assert_eq!(cfg.loop_header_index(0), None);
        assert_eq!(cfg.dom_dfs_pre_index(0), 0);
        assert!(!cfg.dominates(0, 0));
        assert_eq!(cfg.dom_parent_index(0), None);
    }

    #[test]
    fn large_linear_chain_dominators_and_rpo() {
        const N: usize = 48;
        let mut builder = CFGBuilder::new();
        let mut ids = Vec::with_capacity(N);
        for i in 0..N {
            ids.push(builder.add_block(i));
        }
        for i in 0..N - 1 {
            builder.add_edge(ids[i], ids[i + 1]);
        }
        let cfg = builder.build();
        assert_eq!(cfg.len(), N);
        let rpo = cfg.reverse_post_order();
        assert_eq!(rpo.len(), N);
        assert_eq!(rpo[0], ids[0]);
        for i in 1..N {
            assert_eq!(cfg.dom_parent_index(ids[i]), Some(ids[i - 1]));
            assert!(cfg.dominates(ids[0], ids[i]));
        }
    }

    #[test]
    fn triangle_cfg_successors_and_preds() {
        let mut builder = CFGBuilder::new();
        let a = builder.add_block("a");
        let b = builder.add_block("b");
        let c = builder.add_block("c");
        builder.add_edge(a, b);
        builder.add_edge(a, c);
        builder.add_edge(b, c);
        let cfg = builder.build();
        assert_eq!(cfg.successors(a).len(), 2);
        assert_eq!(cfg.predecessors(c).len(), 2);
        assert_eq!(cfg.predecessors(b), &[a]);
        assert!(cfg.dominates(a, c));
        assert!(!cfg.dominates(b, c));
    }

    fn bfs_from_entry(cfg: &CFG<impl Sized>, entry: NodeId) -> Vec<NodeId> {
        let mut out = Vec::new();
        let mut q = VecDeque::new();
        let mut seen = vec![false; cfg.len()];
        q.push_back(entry);
        while let Some(n) = q.pop_front() {
            if n >= seen.len() || seen[n] {
                continue;
            }
            seen[n] = true;
            out.push(n);
            for &s in cfg.successors(n) {
                if s < seen.len() && !seen[s] {
                    q.push_back(s);
                }
            }
        }
        out
    }

    #[test]
    fn bfs_diamond_reaches_merge_before_exit() {
        let mut builder = CFGBuilder::new();
        let entry = builder.add_block(0u8);
        let left = builder.add_block(1u8);
        let right = builder.add_block(2u8);
        let merge = builder.add_block(3u8);
        builder.add_edge(entry, left);
        builder.add_edge(entry, right);
        builder.add_edge(left, merge);
        builder.add_edge(right, merge);
        let cfg = builder.build();
        let order = bfs_from_entry(&cfg, entry);
        assert_eq!(order[0], entry);
        assert_eq!(order.len(), 4);
        let merge_pos = order
            .iter()
            .position(|&n| n == merge)
            .expect("BFS should visit merge");
        let left_pos = order
            .iter()
            .position(|&n| n == left)
            .expect("BFS should visit left");
        let right_pos = order
            .iter()
            .position(|&n| n == right)
            .expect("BFS should visit right");
        assert!(merge_pos > left_pos);
        assert!(merge_pos > right_pos);
    }

    #[test]
    fn test_pred_indices_succ_indices_aliases() {
        let mut builder = CFGBuilder::new();
        let a = builder.add_block("a");
        let b = builder.add_block("b");
        builder.add_edge(a, b);
        let cfg = builder.build();
        assert_eq!(cfg.pred_indices(b), cfg.predecessors(b));
        assert_eq!(cfg.succ_indices(a), cfg.successors(a));
    }

    #[test]
    fn test_cfg_iter_mut() {
        let mut builder = CFGBuilder::new();
        builder.add_block(1);
        builder.add_block(2);
        let mut cfg = builder.build();
        for x in cfg.iter_mut() {
            *x *= 10;
        }
        assert_eq!(cfg[0], 10);
        assert_eq!(cfg[1], 20);
    }

    #[test]
    fn test_cfg_blocks_mut_alias() {
        let mut builder = CFGBuilder::new();
        builder.add_block(1u8);
        let mut cfg = builder.build();
        cfg.blocks_mut()[0] = 9;
        assert_eq!(cfg[0], 9);
    }

    #[test]
    fn test_cfgbuilder_add_node_as_cfg_counts() {
        let mut b = CFGBuilder::<()>::new();
        let _ = b.add_node(());
        let _ = b.add_node(());
        b.add_edge(0, 1);
        assert_eq!(b.block_count(), 2);
        assert_eq!(b.edge_count(), 1);
        let cfg = b.as_cfg();
        assert_eq!(cfg.len(), 2);
    }

    #[test]
    fn disconnect_block_removes_incident_edges_and_clears_dom_cache() {
        let mut cfg: CFG<&str> = CFG::default();
        let a = cfg.push("a");
        let b = cfg.push("b");
        let c = cfg.push("c");
        cfg.add_edge(a, b);
        cfg.add_edge(b, c);
        let _ = cfg.dom_parent_index(b);
        cfg.disconnect_block(b);
        assert_eq!(cfg.successors(a), [] as [usize; 0]);
        assert_eq!(cfg.predecessors(c), [] as [usize; 0]);
        assert_eq!(cfg.successors(b), [] as [usize; 0]);
        assert_eq!(cfg.predecessors(b), [] as [usize; 0]);
        assert_eq!(cfg.dom_parent_index(a), None);
    }

    #[test]
    fn nested_loop_dominators_and_loop_depth() {
        let mut builder = CFGBuilder::new();
        let entry = builder.add_block("e");
        let h1 = builder.add_block("h1");
        let x = builder.add_block("x");
        let h2 = builder.add_block("h2");
        let y = builder.add_block("y");
        let exit = builder.add_block("exit");
        builder.add_edge(entry, h1);
        builder.add_edge(h1, x);
        builder.add_edge(h1, exit);
        builder.add_edge(x, h2);
        builder.add_edge(h2, y);
        builder.add_edge(h2, h1);
        builder.add_edge(y, h2);
        let cfg = builder.build();
        assert!(cfg.has_loop());
        assert!(cfg.is_loop_header(h1));
        assert!(cfg.is_loop_header(h2));
        assert!(cfg.loop_depth(h2) >= 1);
        assert!(cfg.loop_depth(y) >= 1);
        assert_eq!(cfg.loop_depth(entry), 0);
        assert_eq!(cfg.loop_depth(exit), 0);
        assert!(cfg.dominates(entry, h1));
        assert!(cfg.dominates(h1, h2));
    }
}
