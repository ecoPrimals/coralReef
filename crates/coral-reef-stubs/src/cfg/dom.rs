// SPDX-License-Identifier: AGPL-3.0-only
//! Dominator tree, loop detection, and related CFG analysis.
//!
//! Implements the Cooper-Harvey-Kennedy iterative dominator algorithm with
//! lazy computation and caching via `DomAnalysis`.

use std::collections::HashSet;

use super::{CFG, NodeId};

/// Cached dominator and loop analysis, computed lazily.
#[derive(Debug)]
pub struct DomAnalysis {
    pub dom_parent: Vec<Option<NodeId>>,
    pub dom_dfs_pre: Vec<usize>,
    pub loop_depth: Vec<usize>,
    pub has_loop: bool,
}

impl<T> CFG<T> {
    /// Whether a block is a loop header (has a back-edge predecessor).
    #[must_use]
    pub fn is_loop_header(&self, id: NodeId) -> bool {
        let analysis = self.ensure_dom_analysis();
        analysis.loop_depth.get(id).copied().unwrap_or(0) > 0
            && self.predecessors(id).iter().any(|&p| self.dominates(id, p))
    }

    /// The loop header index for a block, if it's in a loop.
    #[must_use]
    pub fn loop_header_index(&self, id: NodeId) -> Option<NodeId> {
        if self.loop_depth(id) == 0 {
            return None;
        }
        let mut node = id;
        let analysis = self.ensure_dom_analysis();
        while let Some(parent) = analysis.dom_parent.get(node).and_then(|&p| p) {
            if self
                .predecessors(parent)
                .iter()
                .any(|&p| self.dominates(parent, p))
            {
                return Some(parent);
            }
            node = parent;
        }
        None
    }

    /// Loop depth of a node (0 = not in a loop).
    #[must_use]
    pub fn loop_depth(&self, id: NodeId) -> usize {
        let analysis = self.ensure_dom_analysis();
        analysis.loop_depth.get(id).copied().unwrap_or(0)
    }

    /// Dominator tree parent index (immediate dominator).
    #[must_use]
    pub fn dom_parent_index(&self, id: NodeId) -> Option<NodeId> {
        let analysis = self.ensure_dom_analysis();
        analysis
            .dom_parent
            .get(id)
            .and_then(|&p| p)
            .filter(|&p| p != id)
    }

    /// Whether node `a` dominates node `b`.
    #[must_use]
    pub fn dominates(&self, a: NodeId, b: NodeId) -> bool {
        let analysis = self.ensure_dom_analysis();
        if a >= analysis.dom_parent.len() || b >= analysis.dom_parent.len() {
            return false;
        }
        let mut finger = b;
        while finger != a {
            match analysis.dom_parent.get(finger).and_then(|&p| p) {
                Some(parent) if parent != finger => finger = parent,
                _ => return false,
            }
        }
        true
    }

    /// DFS pre-order index in dominator tree.
    #[must_use]
    pub fn dom_dfs_pre_index(&self, id: NodeId) -> usize {
        let analysis = self.ensure_dom_analysis();
        analysis.dom_dfs_pre.get(id).copied().unwrap_or(id)
    }

    /// Whether the CFG contains a loop.
    #[must_use]
    pub fn has_loop(&self) -> bool {
        let analysis = self.ensure_dom_analysis();
        analysis.has_loop
    }

    /// Whether node `a` is dominated by node `b` (i.e. `b` dominates `a`).
    #[must_use]
    pub fn is_dominated_by(&self, a: NodeId, b: NodeId) -> bool {
        self.dominates(b, a)
    }

    /// Immediate dominator of a node. Same as `dom_parent_index`.
    #[must_use]
    pub fn idom(&self, id: NodeId) -> Option<NodeId> {
        self.dom_parent_index(id)
    }

    /// Ensures dominator analysis is computed; returns a reference to it.
    fn ensure_dom_analysis(&self) -> std::cell::Ref<'_, DomAnalysis> {
        if self.dom_analysis.borrow().is_none() {
            let analysis = self.compute_dom_analysis();
            *self.dom_analysis.borrow_mut() = Some(analysis);
        }
        std::cell::Ref::map(self.dom_analysis.borrow(), |opt| {
            opt.as_ref().expect("analysis was just computed")
        })
    }

    /// Computes dominator tree (Cooper-Harvey-Kennedy), loop detection, and DFS indices.
    fn compute_dom_analysis(&self) -> DomAnalysis {
        let n = self.blocks.len();
        let rpo = self.reverse_post_order();
        if rpo.is_empty() {
            return DomAnalysis {
                dom_parent: Vec::new(),
                dom_dfs_pre: Vec::new(),
                loop_depth: vec![],
                has_loop: false,
            };
        }

        let dom_parent = self.compute_dominators(&rpo, n);
        let has_loop = self.detect_back_edges(&dom_parent, n);
        let loop_depth = self.compute_loop_depth(&dom_parent, n, has_loop);
        let dom_dfs_pre = Self::compute_dom_dfs_pre(&dom_parent, rpo[0], n);

        DomAnalysis {
            dom_parent,
            dom_dfs_pre,
            loop_depth,
            has_loop,
        }
    }

    /// Cooper-Harvey-Kennedy iterative dominator tree algorithm.
    fn compute_dominators(&self, rpo: &[NodeId], n: usize) -> Vec<Option<NodeId>> {
        let entry = rpo[0];
        let mut rpo_number: Vec<usize> = vec![0; n];
        for (i, &node) in rpo.iter().enumerate() {
            if node < n {
                rpo_number[node] = i;
            }
        }

        let mut doms: Vec<Option<NodeId>> = vec![None; n];
        doms[entry] = Some(entry);

        let mut changed = true;
        let mut iterations = 0usize;
        while changed {
            iterations += 1;
            assert!(
                iterations <= 1000,
                "Cooper-Harvey-Kennedy dominator computation did not converge"
            );
            changed = false;
            for &b in rpo.iter().skip(1) {
                if b >= n {
                    continue;
                }
                let mut new_idom = None;
                for &p in self.predecessors(b) {
                    if doms.get(p).and_then(|&x| x).is_some() {
                        new_idom = Some(
                            new_idom.map_or(p, |prev| Self::intersect(&doms, &rpo_number, p, prev)),
                        );
                    }
                }
                let new_idom = new_idom.unwrap_or(entry);
                if doms.get(b) != Some(&Some(new_idom)) {
                    if b < doms.len() {
                        doms[b] = Some(new_idom);
                    }
                    changed = true;
                }
            }
        }

        (0..n).map(|i| doms.get(i).copied().flatten()).collect()
    }

    fn intersect(doms: &[Option<NodeId>], rpo_number: &[usize], b1: NodeId, b2: NodeId) -> NodeId {
        let mut finger1 = b1;
        let mut finger2 = b2;
        while finger1 != finger2 {
            while rpo_number.get(finger1).copied().unwrap_or(0)
                > rpo_number.get(finger2).copied().unwrap_or(0)
            {
                finger1 = doms.get(finger1).and_then(|&p| p).unwrap_or(finger1);
            }
            while rpo_number.get(finger2).copied().unwrap_or(0)
                > rpo_number.get(finger1).copied().unwrap_or(0)
            {
                finger2 = doms.get(finger2).and_then(|&p| p).unwrap_or(finger2);
            }
        }
        finger1
    }

    fn dom_path_reaches(dom_parent: &[Option<NodeId>], from: NodeId, target: NodeId) -> bool {
        let mut finger = from;
        while finger != target {
            match dom_parent.get(finger).and_then(|&p| p) {
                Some(p) if p != finger => finger = p,
                _ => return false,
            }
        }
        true
    }

    fn detect_back_edges(&self, dom_parent: &[Option<NodeId>], n: usize) -> bool {
        for (from, succs) in &self.successors {
            for &to in succs {
                if to < n
                    && *from < n
                    && dom_parent.get(to).and_then(|&x| x).is_some()
                    && Self::dom_path_reaches(dom_parent, *from, to)
                {
                    return true;
                }
            }
        }
        false
    }

    fn compute_loop_depth(
        &self,
        dom_parent: &[Option<NodeId>],
        n: usize,
        has_loop: bool,
    ) -> Vec<usize> {
        let mut loop_depth = vec![0usize; n];
        if !has_loop {
            return loop_depth;
        }
        let mut processed_headers: HashSet<NodeId> = HashSet::new();
        for (from, succs) in &self.successors {
            for &to in succs {
                if to >= n
                    || *from >= n
                    || processed_headers.contains(&to)
                    || !Self::dom_path_reaches(dom_parent, *from, to)
                {
                    continue;
                }
                processed_headers.insert(to);
                let mut in_loop = HashSet::new();
                in_loop.insert(to);
                let mut stack = vec![*from];
                while let Some(node) = stack.pop() {
                    if in_loop.insert(node) {
                        for &p in self.predecessors(node) {
                            stack.push(p);
                        }
                    }
                }
                for &node in &in_loop {
                    if node < loop_depth.len() {
                        loop_depth[node] = loop_depth[node].saturating_add(1);
                    }
                }
            }
        }
        loop_depth
    }

    fn compute_dom_dfs_pre(dom_parent: &[Option<NodeId>], entry: NodeId, n: usize) -> Vec<usize> {
        let mut dom_dfs_pre = vec![0usize; n];
        let mut counter = 0;
        let mut stack = vec![entry];
        while let Some(node) = stack.pop() {
            if node >= n {
                continue;
            }
            dom_dfs_pre[node] = counter;
            counter += 1;
            for i in (0..n).rev() {
                if i != node && dom_parent.get(i).copied().flatten() == Some(node) {
                    stack.push(i);
                }
            }
        }
        dom_dfs_pre
    }
}
