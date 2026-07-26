// SPDX-License-Identifier: AGPL-3.0-or-later
//! Tests for the forward and backward dataflow analysis module.

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

#[test]
fn test_forward_dataflow_closure_api() {
    let mut builder = CFGBuilder::new();
    let a = builder.add_block("a");
    let b = builder.add_block("b");
    let c = builder.add_block("c");
    builder.add_edge(a, b);
    builder.add_edge(b, c);
    let cfg = builder.build();

    let n = cfg.len();
    let mut block_in = vec![0u32; n];
    let mut block_out = vec![0u32; n];

    let mut transfer = |_idx: usize, block: &&str, out: &mut u32, inp: &u32| {
        let prev = *out;
        *out = if *block == "a" { 1 } else { *inp };
        *out != prev
    };
    let mut join = |dst: &mut u32, src: &u32| *dst = (*dst).max(*src);

    let mut fwd = ForwardDataflow {
        cfg: &cfg,
        block_in: &mut block_in,
        block_out: &mut block_out,
        transfer: &mut transfer,
        join: &mut join,
    };
    fwd.solve();

    assert_eq!(block_out[a], 1);
    assert_eq!(block_out[b], 1);
    assert_eq!(block_out[c], 1);
}

#[test]
fn test_backward_dataflow_closure_api() {
    let mut builder = CFGBuilder::new();
    let a = builder.add_block("a");
    let b = builder.add_block("b");
    let c = builder.add_block("c");
    builder.add_edge(a, b);
    builder.add_edge(b, c);
    let cfg = builder.build();

    let n = cfg.len();
    let mut block_in = vec![0u32; n];
    let mut block_out = vec![0u32; n];

    let mut transfer = |idx: usize, _block: &&str, inp: &mut u32, out: &u32| {
        let prev = *inp;
        *inp = if idx == 1 { 1 } else { *out };
        *inp != prev
    };
    let mut join = |dst: &mut u32, src: &u32| *dst = (*dst).max(*src);

    let mut bwd = BackwardDataflow {
        cfg: &cfg,
        block_in: &mut block_in,
        block_out: &mut block_out,
        transfer: &mut transfer,
        join: &mut join,
    };
    bwd.solve();

    assert_eq!(block_in[1], 1);
}

#[test]
fn test_backward_dataflow_bi_closure_api() {
    #[derive(Default)]
    struct State(u32);

    impl Clone for State {
        fn clone(&self) -> Self {
            Self(self.0)
        }
    }

    let mut builder = CFGBuilder::new();
    let a = builder.add_block("a");
    let b = builder.add_block("b");
    builder.add_edge(a, b);
    let cfg = builder.build();

    let n = cfg.len();
    let mut block_in: Vec<State> = (0..n).map(|_| State::default()).collect();
    let mut block_out: Vec<State> = (0..n).map(|_| State::default()).collect();

    let mut transfer = |_idx: usize, _block: &&str, inp: &mut State, out: &State| {
        let prev = inp.0;
        inp.0 = out.0 + 1;
        inp.0 != prev
    };
    let mut join = |dst: &mut State, src: &State| dst.0 = dst.0.max(src.0);

    let mut bi = BackwardDataflowBi {
        cfg: &cfg,
        block_in: &mut block_in,
        block_out: &mut block_out,
        transfer: &mut transfer,
        join: &mut join,
    };
    bi.solve();

    assert!(block_out[a].0 >= block_out[b].0);
}

#[test]
fn test_forward_dataflow_entry_no_predecessors() {
    let mut builder = CFGBuilder::new();
    let entry = builder.add_block("entry");
    let exit = builder.add_block("exit");
    builder.add_edge(entry, exit);
    let cfg = builder.build();

    let n = cfg.len();
    let mut block_in = vec![false; n];
    let mut block_out = vec![false; n];

    let mut transfer = |_idx: usize, block: &&str, out: &mut bool, inp: &bool| {
        let prev = *out;
        *out = *block == "entry" || *inp;
        *out != prev
    };
    let mut join = |dst: &mut bool, src: &bool| *dst = *dst || *src;

    let mut fwd = ForwardDataflow {
        cfg: &cfg,
        block_in: &mut block_in,
        block_out: &mut block_out,
        transfer: &mut transfer,
        join: &mut join,
    };
    fwd.solve();

    assert!(block_out[entry]);
    assert!(block_out[exit]);
}

#[test]
fn test_backward_dataflow_exit_no_successors() {
    let mut builder = CFGBuilder::new();
    let entry = builder.add_block("entry");
    let exit = builder.add_block("exit");
    builder.add_edge(entry, exit);
    let cfg = builder.build();

    let n = cfg.len();
    let mut block_in = vec![0u32; n];
    let mut block_out = vec![0u32; n];

    let mut transfer = |idx: usize, _block: &&str, inp: &mut u32, out: &u32| {
        let prev = *inp;
        *inp = if idx == 1 { 42 } else { *out };
        *inp != prev
    };
    let mut join = |dst: &mut u32, src: &u32| *dst = (*dst).max(*src);

    let mut bwd = BackwardDataflow {
        cfg: &cfg,
        block_in: &mut block_in,
        block_out: &mut block_out,
        transfer: &mut transfer,
        join: &mut join,
    };
    bwd.solve();

    assert_eq!(block_in[1], 42);
}

#[test]
fn test_solve_forward_no_change_path() {
    struct IdTransfer;

    impl ForwardDataflowAnalysis for IdTransfer {
        type State = ReachingConst;
        type Block = &'static str;

        fn transfer(&self, _block: &&'static str, input: &ReachingConst) -> ReachingConst {
            input.clone()
        }
    }

    let mut builder = CFGBuilder::new();
    let a = builder.add_block("pass");
    let b = builder.add_block("pass");
    builder.add_edge(a, b);
    let cfg = builder.build();

    let result = solve_forward(&IdTransfer, &cfg);
    assert_eq!(result[a].0, ReachingConst(false));
    assert_eq!(result[a].1, ReachingConst(false));
    assert_eq!(result[b].0, ReachingConst(false));
    assert_eq!(result[b].1, ReachingConst(false));
}

#[test]
fn test_solve_backward_no_change_path() {
    struct IdTransfer;

    impl BackwardDataflowAnalysis for IdTransfer {
        type State = ReachingConst;
        type Block = &'static str;

        fn transfer(&self, _block: &&'static str, output: &ReachingConst) -> ReachingConst {
            output.clone()
        }
    }

    let mut builder = CFGBuilder::new();
    let a = builder.add_block("pass");
    let b = builder.add_block("pass");
    builder.add_edge(a, b);
    let cfg = builder.build();

    let result = solve_backward(&IdTransfer, &cfg);
    assert_eq!(result[a].0, ReachingConst(false));
    assert_eq!(result[a].1, ReachingConst(false));
    assert_eq!(result[b].0, ReachingConst(false));
    assert_eq!(result[b].1, ReachingConst(false));
}

#[test]
fn test_forward_dataflow_multiple_predecessors_join() {
    let mut builder = CFGBuilder::new();
    let a = builder.add_block("def");
    let b = builder.add_block("pass");
    let c = builder.add_block("pass");
    let d = builder.add_block("use");
    builder.add_edge(a, b);
    builder.add_edge(a, c);
    builder.add_edge(b, d);
    builder.add_edge(c, d);
    let cfg = builder.build();

    let result = solve_forward(&ForwardReach, &cfg);
    assert!(result[d].0.0, "def should reach merge from both paths");
}

#[test]
fn test_backward_dataflow_single_block() {
    let mut builder = CFGBuilder::new();
    let a = builder.add_block("use");
    let cfg = builder.build();

    let result = solve_backward(&BackwardLive, &cfg);
    assert_eq!(result.len(), 1);
    assert!(result[a].0.0, "use block produces live input");
}

#[test]
fn test_forward_dataflow_single_block_def() {
    let mut builder = CFGBuilder::new();
    let a = builder.add_block("def");
    let cfg = builder.build();

    let result = solve_forward(&ForwardReach, &cfg);
    assert_eq!(result.len(), 1);
    assert!(!result[a].0.0);
    assert!(result[a].1.0);
}

#[test]
fn test_lattice_join_commutative() {
    let a = ReachingConst(true);
    let b = ReachingConst(false);
    assert_eq!(a.join(&b), ReachingConst(true));
    assert_eq!(b.join(&a), ReachingConst(true));
}

#[test]
fn test_lattice_bottom_identity() {
    let a = ReachingConst(true);
    let bottom = ReachingConst::bottom();
    assert_eq!(a.join(&bottom), a);
    assert_eq!(bottom.join(&a), a);
}
