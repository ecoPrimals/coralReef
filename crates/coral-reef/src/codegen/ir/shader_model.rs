// SPDX-License-Identifier: AGPL-3.0-only
// Copyright © 2025-2026 ecoPrimals
// Derived from Collabora, Ltd. (2022)
//! Shader model trait and NVIDIA shader-model adapter.
//!
//! [`ShaderModel`] is the vendor-agnostic trait that all GPU backends
//! implement.  [`ShaderModelInfo`] is the NVIDIA adapter that dispatches to
//! the correct generation (SM20/SM32/SM50/SM70).

use std::cmp::{max, min};

use super::{LegalizeBuilder, Op, RegFile, Shader};
use crate::CompileError;
use crate::codegen::nv::sm20::ShaderModel20;
use crate::codegen::nv::sm32::ShaderModel32;
use crate::codegen::nv::sm50::ShaderModel50;
use crate::codegen::nv::sm70::ShaderModel70;

pub trait ShaderModel {
    fn sm(&self) -> u8;

    /// Whether this is an NVIDIA shader model. Default: true (legacy compat).
    fn is_nvidia(&self) -> bool {
        !self.is_amd()
    }

    /// Whether this is an AMD shader model. Default: false.
    fn is_amd(&self) -> bool {
        false
    }

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

    /// Legalize a single IR operation for the target architecture.
    ///
    /// # Errors
    ///
    /// Returns [`CompileError`] if the operation cannot be legalized
    /// for this architecture (unsupported opcode, invalid operand, etc.).
    fn legalize_op(&self, b: &mut LegalizeBuilder, op: &mut Op) -> Result<(), CompileError>;

    /// Encode a fully lowered shader into the target's binary instruction words.
    ///
    /// # Errors
    ///
    /// Returns [`CompileError`] if encoding fails (unresolved labels,
    /// unsupported instructions, register overflow, etc.).
    fn encode_shader(&self, s: &Shader<'_>) -> Result<Vec<u32>, CompileError>;

    /// Maximum concurrent warps/waves per streaming multiprocessor or compute unit.
    ///
    /// NVIDIA: warps per SM.  AMD: waves per CU.  Used by the scheduler
    /// to compute occupancy cliffs.
    fn max_warps(&self) -> u32;

    /// Threads per warp (NVIDIA) or lanes per wave (AMD).
    ///
    /// NVIDIA: always 32. AMD RDNA2: 32 (wave32) or 64 (wave64).
    fn wave_size(&self) -> u32 {
        32
    }

    /// Total 32-bit register file entries per SM/CU.
    ///
    /// NVIDIA: 65536 registers per SM (all architectures SM70+).
    /// AMD RDNA2: 1024 VGPRs per SIMD × 2 SIMDs = 2048 (in wave32 units).
    fn total_reg_file(&self) -> u32 {
        65_536
    }
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
    /// # Panics
    ///
    /// Panics if `sm < 20`. All NVIDIA shader models are SM 2.0+.
    pub fn new(sm: u8, warps_per_sm: u8) -> Self {
        assert!(sm >= 20, "NVIDIA shader model must be >= SM 2.0, got {sm}");
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
        } else {
            let $x = ShaderModel20::new($self.sm);
            $y
        }
    };
}

/// Like sm_match! but wraps the result in `Ok(...)`.
///
/// Constructor invariant (`sm >= 20`) guarantees all branches are reachable.
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
        } else {
            let $x = ShaderModel20::new($self.sm);
            Ok($y)
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
/// on the number of GPRs per thread.
///
/// Uses the shader model's `total_reg_file()` and `wave_size()` for
/// vendor-agnostic computation. Falls back to NVIDIA defaults for
/// `ShaderModelInfo`.
pub fn gpr_limit_from_local_size(local_size: &[u16; 3]) -> u32 {
    gpr_limit_from_local_size_sm(local_size, 65_536, 32)
}

/// Vendor-agnostic variant using SM parameters.
pub fn gpr_limit_from_local_size_sm(local_size: &[u16; 3], total_regs: u32, wave_size: u32) -> u32 {
    let local_size = local_size[0] * local_size[1] * local_size[2];
    let local_size = local_size.next_multiple_of(4 * wave_size as u16) as u32;

    let out = total_regs / local_size;
    let out = prev_multiple_of(out, 8);
    min(out, 255)
}

/// Compute max concurrent warps/waves from GPR usage.
///
/// Uses the shader model's `total_reg_file()` and `wave_size()` for
/// vendor-agnostic computation.
pub fn max_warps_per_sm(sm: &dyn ShaderModel, gprs: u32) -> u32 {
    let total_regs = sm.total_reg_file();
    let wave_size = sm.wave_size();
    let gprs = max(gprs, 1);
    let gprs = gprs.next_multiple_of(8);
    let max_warps = prev_multiple_of((total_regs / wave_size) / gprs, 4);
    min(max_warps, sm.max_warps())
}
