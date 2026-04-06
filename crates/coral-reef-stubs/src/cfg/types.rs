// SPDX-License-Identifier: AGPL-3.0-or-later
//! Core CFG types.

use std::cell::RefCell;

use super::dom;

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
    pub(crate) successors: crate::fxhash::FxHashMap<NodeId, Vec<NodeId>>,
    pub(crate) predecessors: crate::fxhash::FxHashMap<NodeId, Vec<NodeId>>,
    pub(crate) dom_analysis: RefCell<Option<dom::DomAnalysis>>,
}
