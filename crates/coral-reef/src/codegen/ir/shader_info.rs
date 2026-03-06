// SPDX-License-Identifier: AGPL-3.0-only
// Copyright © 2025-2026 ecoPrimals
// Derived from Collabora, Ltd. (2022)
//! Shader metadata: stage info, IO info, shader model, shader struct.

#![allow(clippy::wildcard_imports)]

use super::*;
use crate::CompileError;

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

#[derive(Debug, Default)]
pub struct SysValInfo {
    pub ab: u32,
    pub c: u16,
}

#[derive(Debug)]
pub struct VtgIoInfo {
    pub sysvals_in: SysValInfo,
    pub sysvals_in_d: u8,
    pub sysvals_out: SysValInfo,
    pub sysvals_out_d: u8,
    pub attr_in: [u32; 4],
    pub attr_out: [u32; 4],
    pub store_req_start: u8,
    pub store_req_end: u8,
    pub clip_enable: u8,
    pub cull_enable: u8,
    pub xfb: Option<Box<TransformFeedbackInfo>>,
}

impl VtgIoInfo {
    fn mark_attrs(&mut self, addrs: Range<u16>, written: bool) -> Result<(), CompileError> {
        let sysvals = if written {
            &mut self.sysvals_out
        } else {
            &mut self.sysvals_in
        };

        let sysvals_d = if written {
            &mut self.sysvals_out_d
        } else {
            &mut self.sysvals_in_d
        };

        let attr = if written {
            &mut self.attr_out
        } else {
            &mut self.attr_in
        };

        let mut addrs = addrs;
        addrs.start &= !3;
        for addr in addrs.step_by(4) {
            if addr < 0x080 {
                sysvals.ab |= 1 << (addr / 4);
            } else if addr < 0x280 {
                let attr_idx = (addr - 0x080) as usize / 4;
                attr.set_bit(attr_idx, true);
            } else if addr < 0x2c0 {
                return Err(CompileError::NotImplemented(
                    "FF color I/O not supported".into(),
                ));
            } else if addr < 0x300 {
                sysvals.c |= 1 << ((addr - 0x2c0) / 4);
            } else if addr >= 0x3a0 && addr < 0x3c0 {
                *sysvals_d |= 1 << ((addr - 0x3a0) / 4);
            }
        }
        Ok(())
    }

    pub fn mark_attrs_read(&mut self, addrs: Range<u16>) -> Result<(), CompileError> {
        self.mark_attrs(addrs, false)
    }

    pub fn mark_attrs_written(&mut self, addrs: Range<u16>) -> Result<(), CompileError> {
        self.mark_attrs(addrs, true)
    }

    pub fn attr_written(&self, addr: u16) -> Result<bool, CompileError> {
        Ok(if addr < 0x080 {
            self.sysvals_out.ab & (1 << (addr / 4)) != 0
        } else if addr < 0x280 {
            let attr_idx = (addr - 0x080) as usize / 4;
            self.attr_out.get_bit(attr_idx)
        } else if addr < 0x2c0 {
            return Err(CompileError::NotImplemented(
                "FF color I/O not supported".into(),
            ));
        } else if addr < 0x300 {
            self.sysvals_out.c & (1 << ((addr - 0x2c0) / 4)) != 0
        } else if addr >= 0x3a0 && addr < 0x3c0 {
            self.sysvals_out_d & (1 << ((addr - 0x3a0) / 4)) != 0
        } else {
            return Err(CompileError::InvalidInput(format!(
                "unknown I/O address 0x{addr:03x}"
            )));
        })
    }

    pub fn mark_store_req(&mut self, addrs: Range<u16>) -> Result<(), CompileError> {
        let start: u8 = (addrs.start / 4)
            .try_into()
            .map_err(|_| CompileError::InvalidInput("store_req start index out of range".into()))?;
        let end: u8 = ((addrs.end - 1) / 4)
            .try_into()
            .map_err(|_| CompileError::InvalidInput("store_req end index out of range".into()))?;
        self.store_req_start = min(self.store_req_start, start);
        self.store_req_end = max(self.store_req_end, end);
        Ok(())
    }
}

#[derive(Debug)]
pub struct FragmentIoInfo {
    pub sysvals_in: SysValInfo,
    pub sysvals_in_d: [PixelImap; 8],
    pub attr_in: [PixelImap; 128],
    pub barycentric_attr_in: [u32; 4],

    pub reads_sample_mask: bool,
    pub writes_color: u32,
    pub writes_sample_mask: bool,
    pub writes_depth: bool,
}

impl FragmentIoInfo {
    pub fn mark_attr_read(&mut self, addr: u16, interp: PixelImap) -> Result<(), CompileError> {
        if addr < 0x080 {
            self.sysvals_in.ab |= 1 << (addr / 4);
        } else if addr < 0x280 {
            let attr_idx = (addr - 0x080) as usize / 4;
            self.attr_in[attr_idx] = interp;
        } else if addr < 0x2c0 {
            return Err(CompileError::NotImplemented(
                "FF color I/O not supported".into(),
            ));
        } else if addr < 0x300 {
            self.sysvals_in.c |= 1 << ((addr - 0x2c0) / 4);
        } else if addr >= 0x3a0 && addr < 0x3c0 {
            let attr_idx = (addr - 0x3a0) as usize / 4;
            self.sysvals_in_d[attr_idx] = interp;
        }
        Ok(())
    }

    pub fn mark_barycentric_attr_in(&mut self, addr: u16) -> Result<(), CompileError> {
        if !(addr >= 0x80 && addr < 0x280) {
            return Err(CompileError::InvalidInput(format!(
                "barycentric attr addr 0x{addr:03x} out of range 0x080..0x280"
            )));
        }
        let attr_idx = (addr - 0x080) as usize / 4;
        self.barycentric_attr_in.set_bit(attr_idx, true);
        Ok(())
    }
}

#[derive(Debug)]
pub enum ShaderIoInfo {
    None,
    Vtg(VtgIoInfo),
    Fragment(FragmentIoInfo),
}

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

pub trait ShaderModel {
    fn sm(&self) -> u8;

    fn is_fermi(&self) -> bool {
        self.sm() >= 20 && self.sm() < 30
    }

    fn is_kepler_a(&self) -> bool {
        self.sm() >= 30 && self.sm() < 32
    }

    fn is_kepler_b(&self) -> bool {
        // TK1 is SM 3.2 and desktop Kepler B is SM 3.3+
        self.sm() >= 32 && self.sm() < 40
    }

    fn is_kepler(&self) -> bool {
        self.is_kepler_a() || self.is_kepler_b()
    }

    // The following helpers are pulled from GetSpaVersion in the open-source
    // NVIDIA kernel driver sources

    fn is_maxwell(&self) -> bool {
        self.sm() >= 50 && self.sm() < 60
    }

    fn is_pascal(&self) -> bool {
        self.sm() >= 60 && self.sm() < 70
    }

    fn is_volta(&self) -> bool {
        self.sm() >= 70 && self.sm() < 73
    }

    fn is_turing(&self) -> bool {
        self.sm() >= 73 && self.sm() < 80
    }

    fn is_ampere(&self) -> bool {
        self.sm() >= 80 && self.sm() < 89
    }

    fn is_ada(&self) -> bool {
        self.sm() == 89
    }

    fn is_hopper(&self) -> bool {
        self.sm() >= 90 && self.sm() < 100
    }

    fn is_blackwell_a(&self) -> bool {
        self.sm() >= 100 && self.sm() < 110
    }

    fn is_blackwell_b(&self) -> bool {
        self.sm() >= 120 && self.sm() < 130
    }

    fn is_blackwell(&self) -> bool {
        self.is_blackwell_a() || self.is_blackwell_b()
    }

    fn reg_count(&self, file: RegFile) -> u32;
    fn hw_reserved_gpr_count(&self) -> u32;
    fn crs_size(&self, max_crs_depth: u32) -> u32;

    fn op_can_be_uniform(&self, op: &Op) -> bool;

    // Scheduling information
    fn op_needs_scoreboard(&self, op: &Op) -> bool {
        !op.no_scoreboard() && !op.has_fixed_latency(self.sm())
    }

    /// Latency before another non-NOP can execute
    fn exec_latency(&self, op: &Op) -> u32;

    /// Read-after-read latency
    fn raw_latency(&self, write: &Op, dst_idx: usize, read: &Op, src_idx: usize) -> u32;

    /// Write-after-read latency
    fn war_latency(&self, read: &Op, src_idx: usize, write: &Op, dst_idx: usize) -> u32;

    /// Write-after-write latency
    fn waw_latency(
        &self,
        a: &Op,
        a_dst_idx: usize,
        a_has_pred: bool,
        b: &Op,
        b_dst_idx: usize,
    ) -> u32;

    /// Predicate read-after-write latency
    fn paw_latency(&self, write: &Op, dst_idx: usize) -> u32;

    /// Worst-case access-after-write latency
    fn worst_latency(&self, write: &Op, dst_idx: usize) -> u32;

    /// Upper bound on latency
    ///
    /// Every '*_latency' function must return latencies that are
    /// bounded.  Ex: self.war_latency() <= self.latency_upper_bound().
    /// This is only used for compile-time optimization.  If unsure, be
    /// conservative.
    fn latency_upper_bound(&self) -> u32;

    /// Maximum encodable instruction delay
    fn max_instr_delay(&self) -> u8;

    fn legalize_op(&self, b: &mut LegalizeBuilder, op: &mut Op) -> Result<(), CompileError>;
    fn encode_shader(&self, s: &Shader<'_>) -> Result<Vec<u32>, CompileError>;

    /// Maximum concurrent warps/waves per streaming multiprocessor or compute unit.
    ///
    /// NVIDIA: warps per SM.  AMD: waves per CU.  Used by the scheduler
    /// to compute occupancy cliffs.
    fn max_warps(&self) -> u32;
}

/// NVIDIA shader model — delegates to generation-specific implementations.
///
/// This is a compatibility adapter that dispatches to the appropriate
/// NVIDIA generation (SM20/SM32/SM50/SM70) based on `sm` version.
/// New vendor backends should implement [`ShaderModel`] directly on
/// their own types (see `ShaderModelRdna2` for AMD).
pub struct ShaderModelInfo {
    sm: u8,
    warps_per_sm: u8,
}

impl ShaderModelInfo {
    pub fn new(sm: u8, warps_per_sm: u8) -> Self {
        Self { sm, warps_per_sm }
    }
}

macro_rules! sm_match {
    ($self: expr, |$x: ident| $y: expr) => {
        if $self.sm >= 70 {
            let $x = ShaderModel70::new($self.sm);
            $y
        } else if $self.sm >= 50 {
            let $x = ShaderModel50::new($self.sm);
            $y
        } else if $self.sm >= 32 {
            let $x = ShaderModel32::new($self.sm);
            $y
        } else if $self.sm >= 20 {
            let $x = ShaderModel20::new($self.sm);
            $y
        } else {
            panic!("Unsupported shader model");
        }
    };
}

/// Like sm_match! but returns Result for pipeline entry points that can fail.
macro_rules! sm_match_result {
    ($self: expr, |$x: ident| $y: expr) => {{
        if $self.sm >= 70 {
            let $x = ShaderModel70::new($self.sm);
            Ok($y)
        } else if $self.sm >= 50 {
            let $x = ShaderModel50::new($self.sm);
            Ok($y)
        } else if $self.sm >= 32 {
            let $x = ShaderModel32::new($self.sm);
            Ok($y)
        } else if $self.sm >= 20 {
            let $x = ShaderModel20::new($self.sm);
            Ok($y)
        } else {
            Err(CompileError::UnsupportedArch(format!("sm_{}", $self.sm)))
        }
    }};
}

impl ShaderModel for ShaderModelInfo {
    fn sm(&self) -> u8 {
        self.sm
    }

    fn reg_count(&self, file: RegFile) -> u32 {
        sm_match!(self, |sm| sm.reg_count(file))
    }
    fn hw_reserved_gpr_count(&self) -> u32 {
        sm_match!(self, |sm| sm.hw_reserved_gpr_count())
    }
    fn crs_size(&self, max_crs_depth: u32) -> u32 {
        sm_match!(self, |sm| sm.crs_size(max_crs_depth))
    }
    fn op_can_be_uniform(&self, op: &Op) -> bool {
        sm_match!(self, |sm| sm.op_can_be_uniform(op))
    }

    fn exec_latency(&self, op: &Op) -> u32 {
        sm_match!(self, |sm| sm.exec_latency(op))
    }

    fn raw_latency(&self, write: &Op, dst_idx: usize, read: &Op, src_idx: usize) -> u32 {
        sm_match!(self, |sm| sm.raw_latency(write, dst_idx, read, src_idx))
    }

    fn war_latency(&self, read: &Op, src_idx: usize, write: &Op, dst_idx: usize) -> u32 {
        sm_match!(self, |sm| sm.war_latency(read, src_idx, write, dst_idx))
    }

    fn waw_latency(
        &self,
        a: &Op,
        a_dst_idx: usize,
        a_has_pred: bool,
        b: &Op,
        b_dst_idx: usize,
    ) -> u32 {
        sm_match!(self, |sm| sm
            .waw_latency(a, a_dst_idx, a_has_pred, b, b_dst_idx))
    }

    fn paw_latency(&self, write: &Op, dst_idx: usize) -> u32 {
        sm_match!(self, |sm| sm.paw_latency(write, dst_idx))
    }
    fn worst_latency(&self, write: &Op, dst_idx: usize) -> u32 {
        sm_match!(self, |sm| sm.worst_latency(write, dst_idx))
    }
    fn latency_upper_bound(&self) -> u32 {
        sm_match!(self, |sm| sm.latency_upper_bound())
    }
    fn max_instr_delay(&self) -> u8 {
        sm_match!(self, |sm| sm.max_instr_delay())
    }
    fn legalize_op(&self, b: &mut LegalizeBuilder, op: &mut Op) -> Result<(), CompileError> {
        sm_match_result!(self, |sm| sm.legalize_op(b, op)?)
    }
    fn encode_shader(&self, s: &Shader<'_>) -> Result<Vec<u32>, CompileError> {
        sm_match_result!(self, |sm| sm.encode_shader(s)?)
    }
    fn max_warps(&self) -> u32 {
        self.warps_per_sm.into()
    }
}

pub const fn prev_multiple_of(x: u32, y: u32) -> u32 {
    (x / y) * y
}

/// For compute shaders, large values of local_size impose an additional limit
/// on the number of GPRs per thread
pub fn gpr_limit_from_local_size(local_size: &[u16; 3]) -> u32 {
    let local_size = local_size[0] * local_size[1] * local_size[2];
    // Warps are allocated in multiples of 4
    // Multiply that by 32 threads/warp
    let local_size = local_size.next_multiple_of(4 * 32) as u32;
    let total_regs: u32 = 65_536;

    let out = total_regs / local_size;
    // GPRs are allocated in multiples of 8
    let out = prev_multiple_of(out, 8);
    min(out, 255)
}

pub fn max_warps_per_sm(sm: &dyn ShaderModel, gprs: u32) -> u32 {
    let total_regs: u32 = 65_536;
    let gprs = max(gprs, 1);
    let gprs = gprs.next_multiple_of(8);
    let max_warps = prev_multiple_of((total_regs / 32) / gprs, 4);
    min(max_warps, sm.max_warps())
}

pub struct Shader<'a> {
    pub sm: &'a dyn ShaderModel,
    pub info: ShaderInfo,
    pub functions: Vec<Function>,
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
        // Track attribute store. (XXX: ISBEWR)
        self.has_attribute_store |= matches!(instr.op, Op::ASt(_));

        // Track attribute load.
        if matches!(instr.op, Op::ALd(_) | Op::Isberd(_)) {
            self.has_attribute_load = true;

            // If we have any attribute load after an attribute store,
            // we cannot overlap IO.
            if self.has_attribute_store {
                self.can_overlap_io = false;
            }
        }
    }

    fn merge(&mut self, other: &Self) {
        // Propagate details on attribute store and overlap IO.
        self.has_attribute_store |= other.has_attribute_store;
        self.can_overlap_io &= other.can_overlap_io;

        // If a previous block has any attribute store and we found an attribute load,
        // we cannot overlap IO.
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
            write!(f, "{}", func)?;
        }
        Ok(())
    }
}
