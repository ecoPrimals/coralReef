// SPDX-License-Identifier: AGPL-3.0-or-later
//! Compiler precision thresholds and heuristic limits.
//!
//! Named constants for compiler precision, inspired by ecosystem numerical
//! validation tolerance patterns. Each constant documents what it controls, its provenance, and the
//! actual numeric value. Using named constants instead of magic numbers makes
//! precision requirements explicit and self-documenting across the codebase.

// ---------------------------------------------------------------------------
// f64 lowering precision
// ---------------------------------------------------------------------------

/// ULP tolerance for DF64 (double-float) operations.
///
/// DF64 uses Dekker/Knuth pair arithmetic with ~48-bit effective mantissa.
/// Operations should stay within this many ULPs of the true f64 result.
///
/// Provenance: ecosystem numerical validation `tol::ANALYTICAL` (≈1e-10), df64
/// reference, molecular dynamics primal requirements.
pub const DF64_ULP_TOLERANCE: u32 = 4;

/// Maximum ULP error for f64 transcendental lowering (exp2, log2, sin, cos).
///
/// Polynomial and Newton-Raphson lowering targets this bound. exp2/log2 ≤2,
/// sin/cos ≤4 per ecosystem `df64_transcendentals.wgsl` and numerical
/// validation service.
///
/// Provenance: ecosystem `df64_transcendentals.wgsl`, numerical validation service.
pub const F64_TRANSCENDENTAL_ULP: u32 = 4;

/// Maximum ULP error for f64 sqrt/rcp Newton-Raphson lowering.
///
/// MUFU.RSQ64H/RCP64H seeds + 2-iteration refinement targets ≤1 ULP.
///
/// Provenance: ecosystem DF64 requirements, numerical validation `tol::ANALYTICAL`.
pub const F64_SQRT_RCP_ULP: u32 = 1;

// ---------------------------------------------------------------------------
// f32 transcendental accuracy
// ---------------------------------------------------------------------------

/// Relative error tolerance for f32 power/log/exp polyfill implementations.
///
/// Used when validating or tuning f32 transcendental workarounds (e.g.
/// `power_f32`, `log_f32_safe`, `exp_f32_safe` in the ecosystem health preamble).
///
/// Provenance: ecosystem health monitoring f32 transcendental workaround, IEEE 754 f32
/// precision (~7 decimal digits).
pub const F32_TRANSCENDENTAL_REL_TOL: f32 = 1e-5;

/// Maximum ULP for f32 transcendental polyfills.
///
/// Target accuracy for software f32 pow/log/exp when hardware is unreliable.
///
/// Provenance: ecosystem health monitoring, WGSL precision requirements.
pub const F32_TRANSCENDENTAL_ULP: u32 = 8;

// ---------------------------------------------------------------------------
// Register allocation limits
// ---------------------------------------------------------------------------

/// Maximum GPRs per thread for NVIDIA SM70 (Volta).
///
/// Volta has 65536 GPRs per SM, 64 warps max. Practical limit for occupancy.
///
/// Provenance: NVIDIA Volta Tuning Guide, `ShaderModelInfo`.
pub const REG_LIMIT_NV_SM70: u32 = 255;

/// Maximum GPRs per thread for NVIDIA SM75 (Turing).
pub const REG_LIMIT_NV_SM75: u32 = 255;

/// Maximum GPRs per thread for NVIDIA SM86+ (Ampere).
pub const REG_LIMIT_NV_SM86: u32 = 255;

/// Maximum VGPRs for AMD RDNA2 compute.
///
/// Provenance: AMD RDNA2 ISA, coral-reef `ShaderModelRdna2`.
pub const REG_LIMIT_AMD_RDNA2: u32 = 256;

// ---------------------------------------------------------------------------
// Instruction scheduling heuristic thresholds
// ---------------------------------------------------------------------------

/// Target number of free GPRs before switching to pressure-aware scheduling.
///
/// When we have fewer than this many free GPRs, the scheduler prioritizes
/// register pressure over instruction-level parallelism.
///
/// Provenance: NAK/Valve scheduler, coral-reef `opt_instr_sched_prepass`.
pub const SCHED_TARGET_FREE_GPRS: i32 = 4;

/// Reserved GPRs for scheduling (non-spill path).
///
/// Headroom for spill code generation and edge cases.
///
/// Provenance: NAK scheduler, coral-reef `opt_instr_sched_prepass`.
pub const SCHED_SW_RESERVED_GPRS: i32 = 1;

/// Reserved GPRs when spilling is allowed.
///
/// Extra headroom when we may need to emit spill/restore sequences.
///
/// Provenance: NAK scheduler, coral-reef `opt_instr_sched_prepass`.
pub const SCHED_SW_RESERVED_GPRS_SPILL: i32 = 2;

// ---------------------------------------------------------------------------
// Binary size limits
// ---------------------------------------------------------------------------

/// Maximum shader binary size (bytes) for NVIDIA.
///
/// Kernel and hardware limits on pushbuf/code size. Exceeding triggers
/// validation failure.
///
/// Provenance: nouveau kernel, NVIDIA compute limits.
pub const BINARY_SIZE_LIMIT_NV: usize = 1024 * 1024;

/// Maximum shader binary size (bytes) for AMD.
///
/// Provenance: amdgpu kernel, AMD RDNA2 limits.
pub const BINARY_SIZE_LIMIT_AMD: usize = 1024 * 1024;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn df64_ulp_within_reasonable_bound() {
        let ulp = DF64_ULP_TOLERANCE;
        assert!(ulp <= 16, "DF64 ULP should be tight");
        assert!(ulp > 0);
    }

    #[test]
    fn transcendental_ulp_within_reasonable_bound() {
        let ulp = F64_TRANSCENDENTAL_ULP;
        assert!(ulp <= 16);
        assert!(ulp > 0);
    }

    #[test]
    fn sqrt_rcp_is_tightest() {
        let sqrt_ulp = F64_SQRT_RCP_ULP;
        let df64_ulp = DF64_ULP_TOLERANCE;
        assert!(
            sqrt_ulp <= df64_ulp,
            "sqrt/rcp should be at least as tight as general DF64"
        );
    }

    #[test]
    fn f32_rel_tol_is_meaningful() {
        let tol = F32_TRANSCENDENTAL_REL_TOL;
        assert!(tol > 0.0);
        assert!(tol < 0.01, "f32 rel tolerance should be sub-percent");
    }

    #[test]
    fn register_limits_match_hardware_specs() {
        assert_eq!(REG_LIMIT_NV_SM70, 255);
        assert_eq!(REG_LIMIT_NV_SM75, 255);
        assert_eq!(REG_LIMIT_NV_SM86, 255);
        assert_eq!(REG_LIMIT_AMD_RDNA2, 256);
    }

    #[test]
    fn scheduler_thresholds_are_positive() {
        let target_free = SCHED_TARGET_FREE_GPRS;
        let reserved = SCHED_SW_RESERVED_GPRS;
        let reserved_spill = SCHED_SW_RESERVED_GPRS_SPILL;
        assert!(target_free > 0);
        assert!(reserved > 0);
        assert!(reserved_spill >= reserved);
    }

    #[test]
    fn binary_size_limits_are_at_least_1mb() {
        let nv_limit = BINARY_SIZE_LIMIT_NV;
        let amd_limit = BINARY_SIZE_LIMIT_AMD;
        assert!(nv_limit >= 1024 * 1024);
        assert!(amd_limit >= 1024 * 1024);
    }
}
