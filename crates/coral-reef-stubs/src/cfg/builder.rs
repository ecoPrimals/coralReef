// SPDX-License-Identifier: AGPL-3.0-or-later
//! Incremental [`CFG`] construction via [`CFGBuilder`].

use std::cell::RefCell;

use crate::fxhash::FxHashMap;

use super::types::{CFG, Edge, NodeId};

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
