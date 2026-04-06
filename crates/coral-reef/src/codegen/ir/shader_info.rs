// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright © 2025-2026 ecoPrimals
// Derived from Collabora, Ltd. (2022)
//! Shader metadata: stage info structs, `ShaderInfo`, and `Shader`.

use std::fmt;

use super::*;
use crate::{CompileError, FmaPolicy};

// ---------------------------------------------------------------------------
// Stage info types
// ---------------------------------------------------------------------------

/// Transform feedback info for vertex/geometry shader output.
#[derive(Debug, Default)]
pub struct TransformFeedbackInfo {
    /// Number of transform feedback buffers used.
    pub buffer_count: u8,
    /// Stride per buffer (in bytes).
    pub strides: [u32; 4],
    /// Output stream for each varying.
    pub stream_ids: Vec<u8>,
}

#[derive(Debug)]
pub struct ComputeShaderInfo {
    pub local_size: [u16; 3],
    pub shared_mem_size: u16,
}

#[derive(Debug)]
pub struct VertexShaderInfo {
    pub isbe_space_sharing_enable: bool,
}

#[derive(Debug)]
pub struct FragmentShaderInfo {
    pub uses_kill: bool,
    pub does_interlock: bool,
    pub post_depth_coverage: bool,
    pub early_fragment_tests: bool,
    pub uses_sample_shading: bool,
}

#[derive(Debug)]
pub struct GeometryShaderInfo {
    pub passthrough_enable: bool,
    pub stream_out_mask: u8,
    pub threads_per_input_primitive: u8,
    pub output_topology: OutputTopology,
    pub max_output_vertex_count: u16,
}

impl Default for GeometryShaderInfo {
    fn default() -> Self {
        Self {
            passthrough_enable: false,
            stream_out_mask: 0,
            threads_per_input_primitive: 0,
            output_topology: OutputTopology::LineStrip,
            max_output_vertex_count: 0,
        }
    }
}

#[repr(u8)]
#[derive(Clone, Copy, Debug)]
pub enum TessellationDomain {
    Isoline = 0,
    Triangle = 1,
    Quad = 2,
}

#[repr(u8)]
#[derive(Clone, Copy, Debug)]
pub enum TessellationSpacing {
    Integer = 0,
    FractionalOdd = 1,
    FractionalEven = 2,
}

#[derive(Debug)]
pub struct TessellationCommonShaderInfo {
    pub spacing: Option<TessellationSpacing>,
    pub counter_clockwise: bool,
    pub point_mode: bool,
}

#[derive(Debug)]
pub struct TessellationInitShaderInfo {
    pub per_patch_attribute_count: u8,
    pub threads_per_patch: u8,
    pub common: TessellationCommonShaderInfo,
}

#[derive(Debug)]
pub struct TessellationShaderInfo {
    pub domain: TessellationDomain,
    pub common: TessellationCommonShaderInfo,
}

#[derive(Debug)]
pub enum ShaderStageInfo {
    Compute(ComputeShaderInfo),
    Vertex(VertexShaderInfo),
    Fragment(FragmentShaderInfo),
    Geometry(GeometryShaderInfo),
    TessellationInit(TessellationInitShaderInfo),
    Tessellation(TessellationShaderInfo),
}

// ---------------------------------------------------------------------------
// ShaderInfo aggregate
// ---------------------------------------------------------------------------

#[derive(Debug)]
pub struct ShaderInfo {
    pub max_warps_per_sm: u32,
    pub gpr_count: u8,
    pub control_barrier_count: u8,
    pub instr_count: u32,
    pub static_cycle_count: u64,
    pub spills_to_mem: u32,
    pub fills_from_mem: u32,
    pub spills_to_reg: u32,
    pub fills_from_reg: u32,
    pub shared_local_mem_size: u32,
    pub max_crs_depth: u32,
    pub uses_global_mem: bool,
    pub writes_global_mem: bool,
    pub uses_fp64: bool,
    pub stage: ShaderStageInfo,
    pub io: ShaderIoInfo,
}

// ---------------------------------------------------------------------------
// Shader struct and ISBE analysis
// ---------------------------------------------------------------------------

pub struct Shader<'a> {
    pub sm: &'a dyn ShaderModel,
    pub info: ShaderInfo,
    pub functions: Vec<Function>,
    pub fma_policy: FmaPolicy,
}

#[derive(Debug, Copy, Clone, PartialEq, Eq)]
struct IsbeSpaceSharingStateTracker {
    has_attribute_store: bool,
    has_attribute_load: bool,
    can_overlap_io: bool,
}

impl Default for IsbeSpaceSharingStateTracker {
    fn default() -> Self {
        Self::new()
    }
}

impl IsbeSpaceSharingStateTracker {
    pub const fn new() -> Self {
        Self {
            has_attribute_store: false,
            has_attribute_load: false,
            can_overlap_io: true,
        }
    }

    pub fn visit_instr(&mut self, instr: &Instr) {
        self.has_attribute_store |= matches!(instr.op, Op::ASt(_));

        if matches!(instr.op, Op::ALd(_) | Op::Isberd(_)) {
            self.has_attribute_load = true;

            if self.has_attribute_store {
                self.can_overlap_io = false;
            }
        }
    }

    fn merge(&mut self, other: &Self) {
        self.has_attribute_store |= other.has_attribute_store;
        self.can_overlap_io &= other.can_overlap_io;

        if other.has_attribute_store && self.has_attribute_load {
            self.can_overlap_io = false;
        }
    }
}

fn isbe_transfer(
    _block_idx: usize,
    _block: &super::BasicBlock,
    sim_out: &mut IsbeSpaceSharingStateTracker,
    sim_in: &IsbeSpaceSharingStateTracker,
) -> bool {
    if sim_out == sim_in {
        false
    } else {
        *sim_out = *sim_in;
        true
    }
}

fn isbe_join(
    sim_in: &mut IsbeSpaceSharingStateTracker,
    pred_sim_out: &IsbeSpaceSharingStateTracker,
) {
    sim_in.merge(pred_sim_out);
}

fn can_isbe_space_sharing_be_enabled(f: &Function) -> bool {
    let mut state_in = Vec::new();
    for block in &f.blocks {
        let mut sim = IsbeSpaceSharingStateTracker::new();

        for instr in &block.instrs {
            sim.visit_instr(instr);
        }

        if !sim.can_overlap_io {
            return false;
        }

        state_in.push(sim);
    }

    let mut state_out: Vec<IsbeSpaceSharingStateTracker> = (0..f.blocks.len())
        .map(|_| IsbeSpaceSharingStateTracker::new())
        .collect();

    {
        use coral_reef_stubs::dataflow::ForwardDataflow;
        let mut df = ForwardDataflow {
            cfg: &f.blocks,
            block_in: &mut state_in[..],
            block_out: &mut state_out[..],
            transfer: isbe_transfer,
            join: isbe_join,
        };
        df.solve();
    }

    for state in state_in {
        if !state.can_overlap_io {
            return false;
        }
    }

    true
}

impl Shader<'_> {
    pub fn for_each_instr(&self, f: &mut impl FnMut(&Instr)) {
        for func in &self.functions {
            for b in &func.blocks {
                for i in &b.instrs {
                    f(i);
                }
            }
        }
    }

    pub fn map_instrs(
        &mut self,
        mut map: impl FnMut(Instr, &mut SSAValueAllocator) -> MappedInstrs,
    ) {
        for f in &mut self.functions {
            f.map_instrs(&mut map);
        }
    }

    /// Remove all annotations, presumably before encoding the shader.
    pub fn remove_annotations(&mut self) {
        self.map_instrs(|instr: Instr, _| -> MappedInstrs {
            if matches!(instr.op, Op::Annotate(_)) {
                MappedInstrs::None
            } else {
                MappedInstrs::One(instr)
            }
        });
    }

    /// Gather shader metadata (instruction counts, GPR usage, stage info).
    ///
    /// # Errors
    ///
    /// Returns `CompileError::InvalidInput` if vertex shader has no functions.
    pub fn gather_info(&mut self) -> Result<(), CompileError> {
        let mut instr_count = 0;
        let mut uses_global_mem = false;
        let mut writes_global_mem = false;
        let mut uses_fp64 = false;

        self.for_each_instr(&mut |instr| {
            instr_count += 1;

            if !uses_global_mem {
                uses_global_mem = instr.uses_global_mem();
            }

            if !writes_global_mem {
                writes_global_mem = instr.writes_global_mem();
            }

            if !uses_fp64 {
                uses_fp64 = instr.op.is_fp64();
            }
        });

        self.info.instr_count = instr_count;
        self.info.uses_global_mem = uses_global_mem;
        self.info.writes_global_mem = writes_global_mem;
        self.info.uses_fp64 = uses_fp64;

        self.info.max_warps_per_sm = max_warps_per_sm(
            self.sm,
            self.info.gpr_count as u32 + self.sm.hw_reserved_gpr_count(),
        );

        if self.sm.sm() >= 50 {
            if let ShaderStageInfo::Vertex(vertex_info) = &mut self.info.stage {
                let func = self.functions.first().ok_or_else(|| {
                    CompileError::InvalidInput(
                        "vertex shader must have exactly one function".into(),
                    )
                })?;
                vertex_info.isbe_space_sharing_enable = can_isbe_space_sharing_be_enabled(func);
            }
        }
        Ok(())
    }
}

impl fmt::Display for Shader<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for func in &self.functions {
            write!(f, "{func}")?;
        }
        Ok(())
    }
}
