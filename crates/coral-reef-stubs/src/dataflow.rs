// SPDX-License-Identifier: AGPL-3.0-or-later
//! Forward and backward dataflow analysis — replacement for `compiler::dataflow`.
//!
//! Used for liveness analysis and dependency tracking.
//! Implements a worklist-based fixed-point algorithm.

use crate::cfg::CFG;

/// Forward dataflow runner.
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
                let mut input = S::default();
                for &pred in preds {
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

/// Backward dataflow runner (same type for in/out).
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
                let mut output = S::default();
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
#[path = "dataflow_tests.rs"]
mod tests;
