// SPDX-License-Identifier: AGPL-3.0-or-later
//! GPU architecture targets — vendor-agnostic.
//!
//! [`GpuTarget`] is the top-level enum discriminating between GPU vendors.
//! Each vendor has its own architecture enum ([`NvArch`], [`AmdArch`],
//! [`IntelArch`]) that describes specific hardware generations.
//!
//! [`GpuArch`] is a convenience alias for [`NvArch`] to ease the
//! transition from the original NVIDIA-only codebase.

mod amd;
mod intel;
mod nv;

pub use amd::AmdArch;
pub use intel::IntelArch;
pub use nv::{GpuArch, NvArch};

#[cfg(test)]
mod tests;

// ---------------------------------------------------------------------------
// Universal compile target — GPU, CPU, NPU
// ---------------------------------------------------------------------------

/// A universal compilation target encompassing GPU, CPU, and NPU hardware.
///
/// `CompileTarget::Gpu` wraps the existing [`GpuTarget`] for full backward
/// compatibility. `Cpu` and `Npu` variants are stubs that return
/// `CompileError::NotImplemented` from all compile paths — they exist to
/// establish the type-level abstraction for future backends.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum CompileTarget {
    /// GPU target — NVIDIA, AMD, or Intel.
    Gpu(GpuTarget),
    /// CPU target — for shader validation on host hardware.
    Cpu(CpuArch),
    /// Neural processing unit — dataflow/event-driven accelerators.
    Npu(NpuTarget),
}

impl Default for CompileTarget {
    fn default() -> Self {
        Self::Gpu(GpuTarget::default())
    }
}

impl CompileTarget {
    /// Unwrap as a [`GpuTarget`], or `None` if this is a CPU/NPU target.
    #[must_use]
    pub const fn as_gpu(&self) -> Option<GpuTarget> {
        match self {
            Self::Gpu(t) => Some(*t),
            _ => None,
        }
    }

    /// Whether this target is a GPU.
    #[must_use]
    pub const fn is_gpu(&self) -> bool {
        matches!(self, Self::Gpu(_))
    }

    /// Whether this target is a CPU (host validation).
    #[must_use]
    pub const fn is_cpu(&self) -> bool {
        matches!(self, Self::Cpu(_))
    }

    /// Whether this target is an NPU (neural/dataflow accelerator).
    #[must_use]
    pub const fn is_npu(&self) -> bool {
        matches!(self, Self::Npu(_))
    }

    /// The execution model for this target class.
    #[must_use]
    pub const fn execution_model(&self) -> &'static str {
        match self {
            Self::Gpu(_) => "simt",
            Self::Cpu(_) => "sequential",
            Self::Npu(_) => "dataflow",
        }
    }

    /// Human-readable target description.
    #[must_use]
    pub const fn target_class(&self) -> &'static str {
        match self {
            Self::Gpu(_) => "gpu",
            Self::Cpu(_) => "cpu",
            Self::Npu(_) => "npu",
        }
    }
}

impl std::fmt::Display for CompileTarget {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Gpu(gpu) => write!(f, "{gpu}"),
            Self::Cpu(cpu) => write!(f, "{cpu}"),
            Self::Npu(npu) => write!(f, "{npu}"),
        }
    }
}

impl From<GpuTarget> for CompileTarget {
    fn from(gpu: GpuTarget) -> Self {
        Self::Gpu(gpu)
    }
}

impl From<NvArch> for CompileTarget {
    fn from(arch: NvArch) -> Self {
        Self::Gpu(GpuTarget::Nvidia(arch))
    }
}

impl From<AmdArch> for CompileTarget {
    fn from(arch: AmdArch) -> Self {
        Self::Gpu(GpuTarget::Amd(arch))
    }
}

impl From<CpuArch> for CompileTarget {
    fn from(cpu: CpuArch) -> Self {
        Self::Cpu(cpu)
    }
}

impl From<NpuTarget> for CompileTarget {
    fn from(npu: NpuTarget) -> Self {
        Self::Npu(npu)
    }
}

/// CPU architecture for host-side shader validation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum CpuArch {
    /// x86-64 (AMD64).
    X86_64,
    /// ARM 64-bit (`AArch64`).
    Aarch64,
}

impl std::fmt::Display for CpuArch {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::X86_64 => write!(f, "x86_64"),
            Self::Aarch64 => write!(f, "aarch64"),
        }
    }
}

/// Neural processing unit target — dataflow and event-driven accelerators.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum NpuTarget {
    /// `BrainChip` Akida — neuromorphic event-driven inference.
    Akida,
    /// Generic dataflow NPU (vendor-agnostic placeholder).
    GenericDataflow,
}

impl std::fmt::Display for NpuTarget {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Akida => write!(f, "akida"),
            Self::GenericDataflow => write!(f, "npu_dataflow"),
        }
    }
}

// ---------------------------------------------------------------------------
// GPU-specific target (backward-compatible)
// ---------------------------------------------------------------------------

/// A GPU compilation target, discriminated by vendor.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum GpuTarget {
    /// NVIDIA GPU architecture (SM35+).
    Nvidia(NvArch),
    /// AMD GPU architecture (RDNA2/3/4 backend).
    Amd(AmdArch),
    /// Intel GPU architecture (planned — register addresses TBD).
    Intel(IntelArch),
}

impl Default for GpuTarget {
    fn default() -> Self {
        Self::Nvidia(NvArch::default())
    }
}

impl GpuTarget {
    /// The vendor name for this target.
    #[must_use]
    pub const fn vendor(&self) -> &'static str {
        match self {
            Self::Nvidia(_) => "nvidia",
            Self::Amd(_) => "amd",
            Self::Intel(_) => "intel",
        }
    }

    /// Architecture identifier within the vendor (e.g. `"sm86"`, `"rdna2"`).
    #[must_use]
    pub const fn arch_name(&self) -> &'static str {
        match self {
            Self::Nvidia(nv) => nv.short_name(),
            Self::Amd(amd) => amd.short_name(),
            Self::Intel(intel) => intel.short_name(),
        }
    }

    /// Unwrap as [`NvArch`], or `None` if this is a different vendor.
    #[must_use]
    pub const fn as_nvidia(&self) -> Option<NvArch> {
        match self {
            Self::Nvidia(arch) => Some(*arch),
            _ => None,
        }
    }

    /// Unwrap as [`AmdArch`], or `None` if this is a different vendor.
    #[must_use]
    pub const fn as_amd(&self) -> Option<AmdArch> {
        match self {
            Self::Amd(arch) => Some(*arch),
            _ => None,
        }
    }

    /// Unwrap as [`IntelArch`], or `None` if this is a different vendor.
    #[must_use]
    pub const fn as_intel(&self) -> Option<IntelArch> {
        match self {
            Self::Intel(arch) => Some(*arch),
            _ => None,
        }
    }

    /// Whether this target has native f64 instructions.
    #[must_use]
    pub const fn has_native_f64(&self) -> bool {
        match self {
            Self::Nvidia(_nv) => true,
            Self::Amd(amd) => amd.has_native_f64(),
            Self::Intel(_) => false,
        }
    }

    /// Whether this target has fast f64 throughput (1:2 vs f32).
    #[must_use]
    pub const fn has_fast_fp64(&self) -> bool {
        match self {
            Self::Nvidia(nv) => nv.has_fast_fp64(),
            Self::Amd(_) | Self::Intel(_) => false,
        }
    }

    /// Native f64 rate relative to f32 (denominator: 1/N of f32 rate).
    #[must_use]
    pub const fn f64_rate_divisor(&self) -> u32 {
        match self {
            Self::Nvidia(nv) => nv.f64_rate_divisor(),
            Self::Amd(amd) => amd.f64_rate_divisor(),
            Self::Intel(_) => 0,
        }
    }
}

impl std::fmt::Display for GpuTarget {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Nvidia(arch) => write!(f, "{arch}"),
            Self::Amd(arch) => write!(f, "{arch}"),
            Self::Intel(arch) => write!(f, "{arch}"),
        }
    }
}

impl From<NvArch> for GpuTarget {
    fn from(arch: NvArch) -> Self {
        Self::Nvidia(arch)
    }
}

impl From<AmdArch> for GpuTarget {
    fn from(arch: AmdArch) -> Self {
        Self::Amd(arch)
    }
}

impl From<IntelArch> for GpuTarget {
    fn from(arch: IntelArch) -> Self {
        Self::Intel(arch)
    }
}
