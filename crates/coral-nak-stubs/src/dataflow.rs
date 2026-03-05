// SPDX-License-Identifier: AGPL-3.0-only
//! Forward and backward dataflow analysis — replacement for `compiler::dataflow`.
//!
//! NAK uses these for liveness analysis and dependency tracking.
//! Implements a worklist-based fixed-point algorithm.

use crate::cfg::CFG;

/// NAK-style forward dataflow runner.
///
/// Construct with `cfg`, `block_in`, `block_out`, `transfer`, and `join` closures,
/// then call `.solve()` to run to fixed point.
pub struct ForwardDataflow<'a, T, S, F, J> {
    /// Control-flow graph to analyze.
    pub cfg: &'a CFG<T>,
    /// Per-block input state.
    pub block_in: &'a mut [S],
    /// Per-block output state.
    pub block_out: &'a mut [S],
    /// Transfer function: `(block_idx, block, out, in) -> changed`.
    pub transfer: F,
    /// Join function: merges predecessor output into successor input.
    pub join: J,
}

impl<T, S, F, J> ForwardDataflow<'_, T, S, F, J>
where
    S: Clone + Default,
    F: FnMut(usize, &T, &mut S, &S) -> bool,
    J: FnMut(&mut S, &S),
{
    /// Run forward dataflow to fixed point.
    ///
    /// # Panics
    ///
    /// Panics if a node in the reverse post-order is not present in the CFG.
    pub fn solve(&mut self) {
        let order = self.cfg.reverse_post_order();
        loop {
            let mut changed = false;
            for &node in &order {
                let block = self.cfg.block(node).expect("node in CFG");
                let preds = self.cfg.predecessors(node);
                let mut input = if let Some(&first) = preds.first() {
                    self.block_out[first].clone()
                } else {
                    S::default()
                };
                for &pred in preds.iter().skip(1) {
                    (self.join)(&mut input, &self.block_out[pred]);
                }
                self.block_in[node] = input;
                changed |=
                    (self.transfer)(node, block, &mut self.block_out[node], &self.block_in[node]);
            }
            if !changed {
                break;
            }
        }
    }
}

/// NAK-style backward dataflow runner (same type for in/out).
pub struct BackwardDataflow<'a, T, S, F, J> {
    /// Control-flow graph to analyze.
    pub cfg: &'a CFG<T>,
    /// Per-block input state.
    pub block_in: &'a mut [S],
    /// Per-block output state.
    pub block_out: &'a mut [S],
    /// Transfer function: `(block_idx, block, in, out) -> changed`.
    pub transfer: F,
    /// Join function: merges successor input into predecessor output.
    pub join: J,
}

impl<T, S, F, J> BackwardDataflow<'_, T, S, F, J>
where
    S: Clone + Default,
    F: FnMut(usize, &T, &mut S, &S) -> bool,
    J: FnMut(&mut S, &S),
{
    /// Run backward dataflow to fixed point.
    ///
    /// # Panics
    ///
    /// Panics if a node in the reverse post-order is not present in the CFG.
    pub fn solve(&mut self) {
        let mut order = self.cfg.reverse_post_order();
        order.reverse();
        loop {
            let mut changed = false;
            for &node in &order {
                let block = self.cfg.block(node).expect("node in CFG");
                let succs = self.cfg.successors(node);
                let mut output = if let Some(&first) = succs.first() {
                    self.block_in[first].clone()
                } else {
                    S::default()
                };
                for &succ in succs.iter().skip(1) {
                    (self.join)(&mut output, &self.block_in[succ]);
                }
                self.block_out[node] = output;
                changed |=
                    (self.transfer)(node, block, &mut self.block_in[node], &self.block_out[node]);
            }
            if !changed {
                break;
            }
        }
    }
}

/// Backward dataflow with different types for `block_in` and `block_out`.
pub struct BackwardDataflowBi<'a, T, SIn, SOut, F, J> {
    /// Control-flow graph to analyze.
    pub cfg: &'a CFG<T>,
    /// Per-block input state.
    pub block_in: &'a mut [SIn],
    /// Per-block output state.
    pub block_out: &'a mut [SOut],
    /// Transfer function: `(block_idx, block, in, out) -> changed`.
    pub transfer: F,
    /// Join function: merges successor input into predecessor output.
    pub join: J,
}

impl<T, SIn, SOut, F, J> BackwardDataflowBi<'_, T, SIn, SOut, F, J>
where
    SOut: Default,
    F: FnMut(usize, &T, &mut SIn, &SOut) -> bool,
    J: FnMut(&mut SOut, &SIn),
{
    /// Run backward bi-type dataflow to fixed point.
    ///
    /// # Panics
    ///
    /// Panics if a node in the reverse post-order is not present in the CFG.
    pub fn solve(&mut self) {
        let mut order = self.cfg.reverse_post_order();
        order.reverse();
        loop {
            let mut changed = false;
            for &node in &order {
                let block = self.cfg.block(node).expect("node in CFG");
                let succs = self.cfg.successors(node);
                let mut output = SOut::default();
                for &succ in succs {
                    (self.join)(&mut output, &self.block_in[succ]);
                }
                self.block_out[node] = output;
                changed |=
                    (self.transfer)(node, block, &mut self.block_in[node], &self.block_out[node]);
            }
            if !changed {
                break;
            }
        }
    }
}

/// Lattice element for dataflow analysis.
///
/// Types implementing this must form a bounded semilattice: `join` is
/// commutative, associative, and idempotent, with `bottom` as identity.
pub trait Lattice: Clone + PartialEq {
    /// Bottom element (identity for `join`).
    fn bottom() -> Self;

    /// Join two elements (least upper bound).
    #[must_use]
    fn join(&self, other: &Self) -> Self;
}

/// Forward dataflow analysis trait (for `solve_forward`).
///
/// Computes a fixed-point over the CFG by propagating state forward
/// from entry to exit.
pub trait ForwardDataflowAnalysis {
    /// State type (must form a lattice for convergence).
    type State: Lattice;
    /// Block type.
    type Block;

    /// Transfer function: given input state and a block, produce output state.
    fn transfer(&self, block: &Self::Block, input: &Self::State) -> Self::State;
}

/// Backward dataflow analysis trait (for `solve_backward`).
///
/// Computes a fixed-point by propagating state backward from exit to entry.
pub trait BackwardDataflowAnalysis {
    /// State type (must form a lattice for convergence).
    type State: Lattice;
    /// Block type.
    type Block;

    /// Transfer function: given output state and a block, produce input state.
    fn transfer(&self, block: &Self::Block, output: &Self::State) -> Self::State;
}

/// Run a forward dataflow analysis to fixed point.
///
/// Returns a vector of (in-state, out-state) for each block in the CFG.
///
/// # Panics
///
/// Panics if a node from the reverse-post-order is missing from the CFG
/// (indicates a malformed CFG).
pub fn solve_forward<A, T>(analysis: &A, cfg: &CFG<T>) -> Vec<(A::State, A::State)>
where
    A: ForwardDataflowAnalysis<Block = T>,
{
    let n = cfg.len();
    if n == 0 {
        return Vec::new();
    }

    let mut states: Vec<(A::State, A::State)> = (0..n)
        .map(|_| (A::State::bottom(), A::State::bottom()))
        .collect();

    let order = cfg.reverse_post_order();
    let mut changed = true;

    while changed {
        changed = false;
        for &node in &order {
            let block = cfg.block(node).expect("node in CFG");

            let mut input = A::State::bottom();
            for &pred in cfg.predecessors(node) {
                input = input.join(&states[pred].1);
            }

            let output = analysis.transfer(block, &input);
            if output == states[node].1 {
                states[node].0 = input;
            } else {
                states[node] = (input, output);
                changed = true;
            }
        }
    }

    states
}

/// Run a backward dataflow analysis to fixed point.
///
/// Returns a vector of (in-state, out-state) for each block in the CFG.
///
/// # Panics
///
/// Panics if a node from the reverse-post-order is missing from the CFG
/// (indicates a malformed CFG).
pub fn solve_backward<A, T>(analysis: &A, cfg: &CFG<T>) -> Vec<(A::State, A::State)>
where
    A: BackwardDataflowAnalysis<Block = T>,
{
    let n = cfg.len();
    if n == 0 {
        return Vec::new();
    }

    let mut states: Vec<(A::State, A::State)> = (0..n)
        .map(|_| (A::State::bottom(), A::State::bottom()))
        .collect();

    let mut order = cfg.reverse_post_order();
    order.reverse();

    let mut changed = true;

    while changed {
        changed = false;
        for &node in &order {
            let block = cfg.block(node).expect("node in CFG");

            let mut output = A::State::bottom();
            for &succ in cfg.successors(node) {
                output = output.join(&states[succ].0);
            }

            let input = analysis.transfer(block, &output);
            if input == states[node].0 {
                states[node].1 = output;
            } else {
                states[node] = (input, output);
                changed = true;
            }
        }
    }

    states
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cfg::CFGBuilder;

    #[derive(Clone, PartialEq, Debug)]
    struct ReachingConst(bool);

    impl Lattice for ReachingConst {
        fn bottom() -> Self {
            Self(false)
        }
        fn join(&self, other: &Self) -> Self {
            Self(self.0 || other.0)
        }
    }

    struct ForwardReach;

    impl ForwardDataflowAnalysis for ForwardReach {
        type State = ReachingConst;
        type Block = &'static str;

        fn transfer(&self, block: &&'static str, input: &ReachingConst) -> ReachingConst {
            if *block == "def" {
                ReachingConst(true)
            } else {
                input.clone()
            }
        }
    }

    #[test]
    fn test_forward_reaching() {
        let mut builder = CFGBuilder::new();
        let entry = builder.add_block("def");
        let mid = builder.add_block("use");
        let exit = builder.add_block("exit");
        builder.add_edge(entry, mid);
        builder.add_edge(mid, exit);
        let cfg = builder.build();

        let result = solve_forward(&ForwardReach, &cfg);
        assert!(result[exit].0.0);
    }

    struct BackwardLive;

    impl BackwardDataflowAnalysis for BackwardLive {
        type State = ReachingConst;
        type Block = &'static str;

        fn transfer(&self, block: &&'static str, output: &ReachingConst) -> ReachingConst {
            if *block == "use" {
                ReachingConst(true)
            } else {
                output.clone()
            }
        }
    }

    #[test]
    fn test_backward_liveness() {
        let mut builder = CFGBuilder::new();
        let entry = builder.add_block("entry");
        let mid = builder.add_block("use");
        let exit = builder.add_block("exit");
        builder.add_edge(entry, mid);
        builder.add_edge(mid, exit);
        let cfg = builder.build();

        let result = solve_backward(&BackwardLive, &cfg);
        assert!(result[entry].1.0);
    }

    #[test]
    fn test_empty_cfg() {
        let cfg: CFG<&str> = CFG::default();
        let result = solve_forward(&ForwardReach, &cfg);
        assert!(result.is_empty());

        let result = solve_backward(&BackwardLive, &cfg);
        assert!(result.is_empty());
    }

    #[test]
    fn test_forward_reaching_definitions_diamond() {
        let mut builder = CFGBuilder::new();
        let entry = builder.add_block("def");
        let left = builder.add_block("pass");
        let right = builder.add_block("pass");
        let merge = builder.add_block("use");
        builder.add_edge(entry, left);
        builder.add_edge(entry, right);
        builder.add_edge(left, merge);
        builder.add_edge(right, merge);
        let cfg = builder.build();

        let result = solve_forward(&ForwardReach, &cfg);
        assert!(
            result[merge].0.0,
            "definition should reach merge from both paths"
        );
    }

    #[test]
    fn test_backward_liveness_diamond() {
        let mut builder = CFGBuilder::new();
        let entry = builder.add_block("entry");
        let left = builder.add_block("pass");
        let right = builder.add_block("pass");
        let merge = builder.add_block("use");
        builder.add_edge(entry, left);
        builder.add_edge(entry, right);
        builder.add_edge(left, merge);
        builder.add_edge(right, merge);
        let cfg = builder.build();

        let result = solve_backward(&BackwardLive, &cfg);
        assert!(result[entry].1.0, "liveness should propagate back to entry");
        assert!(result[left].1.0);
        assert!(result[right].1.0);
    }

    #[test]
    fn test_convergence_fixed_point_forward() {
        use std::sync::atomic::{AtomicUsize, Ordering};
        static ITER_COUNT: AtomicUsize = AtomicUsize::new(0);

        struct CountingForward;

        impl ForwardDataflowAnalysis for CountingForward {
            type State = ReachingConst;
            type Block = &'static str;

            fn transfer(&self, block: &&'static str, input: &ReachingConst) -> ReachingConst {
                ITER_COUNT.fetch_add(1, Ordering::SeqCst);
                if *block == "def" {
                    ReachingConst(true)
                } else {
                    input.clone()
                }
            }
        }

        ITER_COUNT.store(0, Ordering::SeqCst);
        let mut builder = CFGBuilder::new();
        let a = builder.add_block("def");
        let b = builder.add_block("pass");
        let c = builder.add_block("pass");
        builder.add_edge(a, b);
        builder.add_edge(b, c);
        builder.add_edge(c, b);
        let cfg = builder.build();

        let _ = solve_forward(&CountingForward, &cfg);
        let count = ITER_COUNT.load(Ordering::SeqCst);
        assert!(count > 0, "should have run transfer");
        assert!(count < 100, "should converge (not infinite loop)");
    }

    #[test]
    fn test_convergence_fixed_point_backward() {
        use std::sync::atomic::{AtomicUsize, Ordering};
        static ITER_COUNT: AtomicUsize = AtomicUsize::new(0);

        struct CountingBackward;

        impl BackwardDataflowAnalysis for CountingBackward {
            type State = ReachingConst;
            type Block = &'static str;

            fn transfer(&self, block: &&'static str, output: &ReachingConst) -> ReachingConst {
                ITER_COUNT.fetch_add(1, Ordering::SeqCst);
                if *block == "use" {
                    ReachingConst(true)
                } else {
                    output.clone()
                }
            }
        }

        ITER_COUNT.store(0, Ordering::SeqCst);
        let mut builder = CFGBuilder::new();
        let a = builder.add_block("entry");
        let b = builder.add_block("use");
        let c = builder.add_block("pass");
        builder.add_edge(a, b);
        builder.add_edge(b, c);
        builder.add_edge(c, b);
        let cfg = builder.build();

        let _ = solve_backward(&CountingBackward, &cfg);
        let count = ITER_COUNT.load(Ordering::SeqCst);
        assert!(count > 0);
        assert!(count < 100);
    }
}
