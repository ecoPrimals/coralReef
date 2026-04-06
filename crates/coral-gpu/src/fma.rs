// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright © 2026 ecoPrimals

use coral_reef::{AmdArch, FmaPolicy, GpuTarget, NvArch};

/// FMA (fused multiply-add) hardware capability for a GPU target.
///
/// Reports whether the hardware supports FMA, and what precision behavior
/// to expect. Springs use this to decide between `FmaPolicy::Fused` (fast)
/// and `FmaPolicy::Separate` (bit-exact CPU parity).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct FmaCapability {
    /// Hardware supports f32 FMA.
    pub f32_fma: bool,
    /// Hardware supports f64 FMA (DFMA).
    pub f64_fma: bool,
    /// Recommended FMA policy for numerical precision.
    pub recommended_policy: FmaPolicy,
    /// FMA throughput relative to separate mul+add (1.0 = same speed).
    /// Values > 1.0 mean FMA is faster than separate operations.
    pub f32_fma_throughput_ratio: f32,
}

impl FmaCapability {
    /// Query FMA capabilities for a given GPU target.
    ///
    /// Derives from architecture specifications — does not require
    /// a live device connection.
    #[must_use]
    pub const fn for_target(target: GpuTarget) -> Self {
        match target {
            GpuTarget::Nvidia(nv) => Self::nvidia(nv),
            GpuTarget::Amd(amd) => Self::amd(amd),
            _ => Self {
                f32_fma: true,
                f64_fma: false,
                recommended_policy: FmaPolicy::Auto,
                f32_fma_throughput_ratio: 1.0,
            },
        }
    }

    const fn nvidia(nv: NvArch) -> Self {
        Self {
            f32_fma: true,
            f64_fma: nv.has_dfma(),
            recommended_policy: FmaPolicy::Auto,
            // NVIDIA FMA is on the same pipeline as separate mul+add
            f32_fma_throughput_ratio: 2.0,
        }
    }

    const fn amd(amd: AmdArch) -> Self {
        Self {
            f32_fma: true,
            f64_fma: amd.has_native_f64(),
            recommended_policy: FmaPolicy::Auto,
            // AMD RDNA: v_fma_f32 is VOP3 (1 cycle), same as v_mul + v_add
            f32_fma_throughput_ratio: 2.0,
        }
    }
}
