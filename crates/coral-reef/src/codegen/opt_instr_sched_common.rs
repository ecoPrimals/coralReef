// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright © 2025-2026 ecoPrimals
// Derived from Valve Corporation (2024)

use super::ir::*;
use coral_reef_stubs::cfg::CFG;
use std::cmp::Reverse;
use std::cmp::max;

pub mod graph {
    #[derive(Clone)]
    pub struct Edge<EdgeLabel> {
        pub label: EdgeLabel,
        pub head_idx: usize,
    }

    #[derive(Clone)]
    pub struct Node<NodeLabel, EdgeLabel> {
        pub label: NodeLabel,
        pub outgoing_edges: Vec<Edge<EdgeLabel>>,
    }

    #[derive(Clone)]
    pub struct Graph<NodeLabel, EdgeLabel> {
        pub nodes: Vec<Node<NodeLabel, EdgeLabel>>,
    }

    impl<NodeLabel, EdgeLabel> Graph<NodeLabel, EdgeLabel> {
        pub fn new(node_labels: impl Iterator<Item = NodeLabel>) -> Self {
            let nodes = node_labels
                .map(|label| Node {
                    label,
                    outgoing_edges: Vec::new(),
                })
                .collect();

            Self { nodes }
        }

        pub fn add_edge(&mut self, tail_idx: usize, head_idx: usize, label: EdgeLabel) {
            assert!(head_idx < self.nodes.len());
            self.nodes[tail_idx]
                .outgoing_edges
                .push(Edge { label, head_idx });
        }

        pub fn reverse(&mut self) {
            let old_edges: Vec<_> = self
                .nodes
                .iter_mut()
                .map(|node| std::mem::take(&mut node.outgoing_edges))
                .collect();

            for (tail_idx, edges) in old_edges.into_iter().enumerate() {
                for e in edges {
                    self.add_edge(e.head_idx, tail_idx, e.label);
                }
            }
        }
    }
}

#[derive(Eq, PartialEq)]
pub enum SideEffect {
    /// No side effect (ALU-like)
    None,

    /// Instruction reads or writes memory
    ///
    /// This will be serialized with respect to other
    /// SideEffect::Memory instructions
    Memory,

    /// This instcuction is a full code motion barrier
    ///
    /// No other instruction will be re-ordered with respect to this one
    Barrier,
}

pub fn side_effect_type(op: &Op) -> SideEffect {
    match op {
        // Float ALU
        Op::F2FP(_)
        | Op::FAdd(_)
        | Op::FFma(_)
        | Op::FMnMx(_)
        | Op::FMul(_)
        | Op::FSet(_)
        | Op::FSetP(_)
        | Op::HAdd2(_)
        | Op::HFma2(_)
        | Op::HMul2(_)
        | Op::HSet2(_)
        | Op::HSetP2(_)
        | Op::HMnMx2(_)
        | Op::FSwz(_)
        | Op::FSwzAdd(_) => SideEffect::None,

        // Multi-function unit
        Op::Rro(_) | Op::Transcendental(_) => SideEffect::None,

        // Double-precision float ALU
        Op::DAdd(_)
        | Op::DFma(_)
        | Op::F64Exp2(_)
        | Op::F64Log2(_)
        | Op::F64Rcp(_)
        | Op::F64Sin(_)
        | Op::F64Cos(_)
        | Op::F64Sqrt(_)
        | Op::DMnMx(_)
        | Op::DMul(_)
        | Op::DSetP(_) => SideEffect::None,

        // Integer ALU
        Op::BRev(_)
        | Op::Flo(_)
        | Op::PopC(_)
        | Op::IMad(_)
        | Op::IMul(_)
        | Op::BMsk(_)
        | Op::IAbs(_)
        | Op::IAdd2(_)
        | Op::IAdd2X(_)
        | Op::IAdd3(_)
        | Op::IAdd3X(_)
        | Op::IDp4(_)
        | Op::IMad64(_)
        | Op::IMnMx(_)
        | Op::ISetP(_)
        | Op::Lea(_)
        | Op::LeaX(_)
        | Op::Lop2(_)
        | Op::Lop3(_)
        | Op::SuClamp(_)
        | Op::SuBfm(_)
        | Op::SuEau(_)
        | Op::IMadSp(_)
        | Op::Shf(_)
        | Op::Shl(_)
        | Op::Shr(_)
        | Op::Bfe(_) => SideEffect::None,

        // Conversions
        Op::F2F(_) | Op::F2I(_) | Op::I2F(_) | Op::I2I(_) | Op::FRnd(_) => SideEffect::None,

        // Move ops
        Op::Mov(_) | Op::Prmt(_) | Op::Sel(_) | Op::Sgxt(_) => SideEffect::None,
        Op::Shfl(_) => SideEffect::None,

        // Predicate ops
        Op::PLop3(_) | Op::PSetP(_) => SideEffect::None,

        // Uniform ops
        Op::R2UR(_) | Op::Redux(_) => SideEffect::None,

        // Texture ops
        Op::Tex(_) | Op::Tld(_) | Op::Tld4(_) | Op::Tmml(_) | Op::Txd(_) | Op::Txq(_) => {
            SideEffect::Memory
        }

        // Surface ops
        Op::SuLd(_) | Op::SuSt(_) | Op::SuAtom(_) | Op::SuLdGa(_) | Op::SuStGa(_) => {
            SideEffect::Memory
        }

        // Memory ops
        Op::Ipa(_) | Op::Ldc(_) => SideEffect::None,
        Op::Ld(_)
        | Op::Ldsm(_)
        | Op::LdSharedLock(_)
        | Op::St(_)
        | Op::StSCheckUnlock(_)
        | Op::Atom(_)
        | Op::AL2P(_)
        | Op::ALd(_)
        | Op::ASt(_)
        | Op::CCtl(_)
        | Op::LdTram(_)
        | Op::MemBar(_) => SideEffect::Memory,

        // Matrix ops
        Op::Imma(_) | Op::Hmma(_) | Op::Movm(_) => SideEffect::None,

        // Control-flow ops
        Op::BClear(_)
        | Op::Break(_)
        | Op::BSSy(_)
        | Op::BSync(_)
        | Op::SSy(_)
        | Op::Sync(_)
        | Op::Brk(_)
        | Op::PBk(_)
        | Op::Cont(_)
        | Op::PCnt(_)
        | Op::Bra(_)
        | Op::Exit(_)
        | Op::WarpSync(_) => SideEffect::Barrier,

        // We don't model the barrier register yet, so serialize these
        Op::BMov(_) => SideEffect::Memory,

        // Geometry ops
        Op::Out(_) | Op::OutFinal(_) => SideEffect::Barrier,

        // Miscellaneous ops
        Op::Bar(_)
        | Op::TexDepBar(_)
        | Op::CS2R(_)
        | Op::Isberd(_)
        | Op::ViLd(_)
        | Op::Kill(_)
        | Op::S2R(_) => SideEffect::Barrier,
        Op::PixLd(_) | Op::Vote(_) | Op::Match(_) => SideEffect::None,
        Op::Nop(OpNop { label, .. }) => {
            if label.is_none() {
                SideEffect::None
            } else {
                SideEffect::Barrier
            }
        }

        // Virtual ops
        Op::Annotate(_) | Op::ParCopy(_) | Op::Swap(_) | Op::Copy(_) | Op::Undef(_) => {
            SideEffect::None
        }

        Op::SrcBar(_)
        | Op::Pin(_)
        | Op::Unpin(_)
        | Op::PhiSrcs(_)
        | Op::PhiDsts(_)
        | Op::RegOut(_) => SideEffect::Barrier,
    }
}

pub fn estimate_block_weight(cfg: &CFG<BasicBlock>, block_idx: usize) -> u64 {
    let loop_depth = cfg.loop_depth(block_idx) as f32;
    10_f32.powf((loop_depth + 1.0).log2()) as u64
}

/// Estimated cycle latencies for variable-latency instructions.
///
/// Source: ["Dissecting the NVidia Turing T4 GPU via
/// Microbenchmarking"](https://arxiv.org/pdf/1903.07486).
/// Memory latencies from L1 data cache measurements.
mod sched_latency {
    /// Constant buffer load (LDC) — uniform/CBuf path.
    pub const CBUF_LOAD: u32 = 4;
    /// Multi-function unit (MUFU, transcendentals).
    pub const MUFU: u32 = 15;
    /// Warp shuffle, conversions, integer bit ops.
    pub const ALU_EXTENDED: u32 = 15;
    /// Control-flow, barrier, system regs, pixel load.
    pub const CONTROL: u32 = 16;
    /// Tensor-core MMA (HMMA/IMMA).
    pub const TENSOR_CORE: u32 = 22;
    /// Texture, surface, global memory, membar.
    pub const MEMORY: u32 = 32;
    /// Double-precision add/mul/minmax.
    pub const F64_ALU: u32 = 48;
    /// Double-precision FMA / comparison.
    pub const F64_FMA: u32 = 54;
    /// Pre-SM70 integer multiply (multi-cycle).
    pub const LEGACY_IMUL: u32 = 86;
    /// f64 transcendental pseudo-ops (MUFU + chain of DFMA/DMul).
    pub const F64_TRANSCENDENTAL: u32 = 200;
}

/// Try to guess how many cycles a variable latency instruction will take.
///
/// See [`sched_latency`] for source documentation.
pub fn estimate_variable_latency(sm: &dyn ShaderModel, op: &Op) -> u32 {
    use sched_latency::*;

    if !sm.op_needs_scoreboard(op) {
        return 0;
    }

    match op {
        Op::Rro(_) | Op::Transcendental(_) => MUFU,

        Op::DFma(_) | Op::DSetP(_) => F64_FMA,
        Op::DAdd(_) | Op::DMnMx(_) | Op::DMul(_) => F64_ALU,

        Op::F64Exp2(_)
        | Op::F64Log2(_)
        | Op::F64Rcp(_)
        | Op::F64Sin(_)
        | Op::F64Cos(_)
        | Op::F64Sqrt(_) => F64_TRANSCENDENTAL,

        Op::BRev(_) | Op::Flo(_) | Op::PopC(_) => MUFU,
        Op::IMad(_) | Op::IMul(_) => {
            assert!(sm.sm() < 70);
            LEGACY_IMUL
        }

        Op::F2F(_) | Op::F2I(_) | Op::I2F(_) | Op::I2I(_) | Op::FRnd(_) => ALU_EXTENDED,

        Op::Shfl(_) => ALU_EXTENDED,

        Op::Redux(_) | Op::R2UR(_) => ALU_EXTENDED,

        Op::Tex(_) | Op::Tld(_) | Op::Tld4(_) | Op::Tmml(_) | Op::Txd(_) | Op::Txq(_) => MEMORY,

        Op::SuLd(_) | Op::SuSt(_) | Op::SuAtom(_) | Op::SuLdGa(_) | Op::SuStGa(_) => MEMORY,

        Op::Ldc(_) => CBUF_LOAD,

        Op::Ld(_)
        | Op::Ldsm(_)
        | Op::Movm(_)
        | Op::LdSharedLock(_)
        | Op::St(_)
        | Op::StSCheckUnlock(_)
        | Op::Atom(_)
        | Op::AL2P(_)
        | Op::ALd(_)
        | Op::ASt(_)
        | Op::Ipa(_)
        | Op::CCtl(_)
        | Op::LdTram(_)
        | Op::MemBar(_) => MEMORY,

        Op::WarpSync(_) => CONTROL,

        Op::BMov(_) => CONTROL,

        Op::Out(_) | Op::OutFinal(_) => 2,

        Op::Bar(_)
        | Op::TexDepBar(_)
        | Op::CS2R(_)
        | Op::Isberd(_)
        | Op::ViLd(_)
        | Op::Kill(_)
        | Op::PixLd(_)
        | Op::S2R(_)
        | Op::Match(_) => CONTROL,

        Op::Hmma(_) | Op::Imma(_) => TENSOR_CORE,

        _ => crate::codegen::ice!("Unknown variable latency op {op}"),
    }
}

#[derive(Default, Clone)]
pub struct NodeLabel {
    pub cycles_to_end: u32,
    pub use_count: u32,

    /// The first cycle that the instruction can begin executing
    pub ready_cycle: u32,

    pub exec_latency: u32,
}

pub struct EdgeLabel {
    pub latency: u32,
}

pub type DepGraph = graph::Graph<NodeLabel, EdgeLabel>;

pub fn calc_statistics(g: &mut DepGraph) -> Vec<usize> {
    let mut initial_ready_list = Vec::new();
    for i in (0..g.nodes.len()).rev() {
        let node = &g.nodes[i];
        let mut max_delay = 0;
        for edge in &node.outgoing_edges {
            assert!(edge.head_idx > i);
            max_delay = max(
                max_delay,
                g.nodes[edge.head_idx].label.cycles_to_end + edge.label.latency,
            );
        }
        let node = &mut g.nodes[i];
        node.label.cycles_to_end = max_delay;
        node.label.use_count = node
            .outgoing_edges
            .len()
            .try_into()
            .expect("outgoing edge count must fit in u32");
        if node.label.use_count == 0 {
            initial_ready_list.push(i);
        }
    }

    initial_ready_list
}

#[derive(Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct ReadyInstr {
    cycles_to_end: u32,

    // We use the original instruction order as a final tie-breaker, the idea
    // being that the original schedule is often not too bad. Since we're
    // iterating in reverse order, that means scheduling the largest instruciton
    // index first.
    pub index: usize,
}

impl ReadyInstr {
    pub fn new<E>(g: &graph::Graph<NodeLabel, E>, i: usize) -> Self {
        let label = &g.nodes[i].label;
        // NodeLabel::cycles_to_end is cycles from the beginning of the
        // instruction to the top of the block, but we want from the end of the
        // instruction to the top of the block
        let cycles_to_end = label.cycles_to_end + label.exec_latency;
        Self {
            cycles_to_end,
            index: i,
        }
    }
}

#[derive(Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct FutureReadyInstr {
    /// The first cycle that the instruction can end executing
    pub ready_cycle: Reverse<u32>,
    pub index: usize,
}

impl FutureReadyInstr {
    pub fn new<E>(g: &graph::Graph<NodeLabel, E>, i: usize) -> Self {
        let label = &g.nodes[i].label;
        // NodeLabel::ready_cycle is the earliest beginning cycle for the
        // instruction, but we need the earliest end cycle for the instruction.
        let ready_cycle = label.ready_cycle.saturating_sub(label.exec_latency);
        Self {
            ready_cycle: Reverse(ready_cycle),
            index: i,
        }
    }
}

#[cfg(test)]
pub fn save_graphviz(instrs: &[Box<Instr>], g: &DepGraph) -> std::io::Result<()> {
    use std::fs::File;
    use std::io::{BufWriter, Write};

    let path = std::env::var(crate::env_keys::CORAL_DEP_GRAPH_PATH).map_or_else(
        |_| {
            let base = std::env::var("XDG_RUNTIME_DIR")
                .map_or_else(|_| std::env::temp_dir(), std::path::PathBuf::from);
            base.join("instr_dep_graph.dot")
        },
        std::path::PathBuf::from,
    );
    let file = File::create(&path)?;
    let mut w = BufWriter::new(file);

    writeln!(w, "digraph {{")?;
    for (i, instr) in instrs.iter().enumerate() {
        let l = &g.nodes[i].label;
        writeln!(
            w,
            "    {i} [label=\"{}\\n{}, {}\"];",
            instr, l.cycles_to_end, l.use_count
        )?;
    }
    for (i, node) in g.nodes.iter().enumerate() {
        for j in &node.outgoing_edges {
            writeln!(
                w,
                "    {i} -> {} [label=\"{}\"];",
                j.head_idx, j.label.latency
            )?;
        }
    }
    writeln!(w, "}}")?;
    w.flush()?;
    Ok(())
}
