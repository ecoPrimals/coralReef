// SPDX-License-Identifier: AGPL-3.0-or-later
//! Core [`CFG`] operations: accessors, mutation, iterators.

use std::cell::RefCell;
use std::ops::{Index, IndexMut};

use super::types::{CFG, NodeId};

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
            successors: crate::fxhash::FxHashMap::default(),
            predecessors: crate::fxhash::FxHashMap::default(),
            dom_analysis: RefCell::new(None),
        }
    }
}
