// SPDX-License-Identifier: AGPL-3.0-or-later
//! Vendor-agnostic compiler backend abstraction.
//!
//! The [`Backend`] trait defines the contract that vendor backends
//! implement.  Each backend takes an IR [`Shader`] (produced by a
//! [`Frontend`](crate::frontend::Frontend)) and produces a native GPU
//! binary.
//!
//! Backends are resolved via [`backend_for`], which maps a [`GpuTarget`]
//! to the appropriate implementation.

use crate::codegen::ir::Shader;
use crate::codegen::ir::ShaderStageInfo;
use crate::error::CompileError;
use crate::gpu_arch::GpuTarget;

/// Output of a successful compilation.
#[derive(Debug, Clone)]
pub struct CompiledBinary {
    /// The final binary (header + code as little-endian bytes).
    pub binary: Vec<u8>,
    /// Metadata from compilation (register count, etc.).
    pub info: CompilationInfo,
}

/// Summary statistics from a compilation pass.
#[derive(Debug, Clone, Default)]
pub struct CompilationInfo {
    /// Number of general-purpose registers used.
    pub gpr_count: u32,
    /// Number of instructions emitted.
    pub instr_count: u32,
    /// Shared memory used by the shader (bytes).
    pub shared_mem_bytes: u32,
    /// Number of barriers used.
    pub barrier_count: u32,
    /// Workgroup dimensions from `@workgroup_size(x, y, z)`.
    pub local_size: [u32; 3],
}

/// A vendor-specific compiler backend.
///
/// Backends take a parsed and (optionally) pre-optimized [`Shader`] and
/// produce a native GPU binary for the target architecture.
pub trait Backend {
    /// Returns `true` if `target` can be compiled by this backend.
    fn supports(&self, target: GpuTarget) -> bool;

    /// Compile a shader to a native binary.
    ///
    /// # Errors
    ///
    /// Returns [`CompileError`] if optimization, register allocation,
    /// or encoding fails.
    fn compile(&self, shader: &mut Shader<'_>) -> Result<CompiledBinary, CompileError>;
}

/// Extract compute-specific metadata from shader stage info.
fn compute_info(shader: &Shader<'_>) -> (u32, u32, [u32; 3]) {
    match &shader.info.stage {
        ShaderStageInfo::Compute(cs) => (
            u32::from(cs.shared_mem_size),
            u32::from(shader.info.control_barrier_count),
            [
                u32::from(cs.local_size[0]),
                u32::from(cs.local_size[1]),
                u32::from(cs.local_size[2]),
            ],
        ),
        _ => (0, u32::from(shader.info.control_barrier_count), [1, 1, 1]),
    }
}

/// NVIDIA backend — drives the codegen pipeline (SM70+).
pub struct NvidiaBackend;

impl Backend for NvidiaBackend {
    fn supports(&self, target: GpuTarget) -> bool {
        target.as_nvidia().is_some()
    }

    fn compile(&self, shader: &mut Shader<'_>) -> Result<CompiledBinary, CompileError> {
        let compiled = crate::codegen::pipeline::compile_shader(shader, false)?;

        // Compute shaders have no SPH — all dispatch metadata lives in the QMD.
        // Graphics shaders carry the SPH prepended to the instruction stream.
        let is_compute = matches!(shader.info.stage, ShaderStageInfo::Compute(_));
        let hdr_words = if is_compute { 0 } else { compiled.header.len() };
        let mut binary = Vec::with_capacity(hdr_words * 4 + compiled.code.len() * 4);
        for word in &compiled.header[..hdr_words] {
            binary.extend_from_slice(&word.to_le_bytes());
        }
        for word in &compiled.code {
            binary.extend_from_slice(&word.to_le_bytes());
        }

        let (shared_mem_bytes, barrier_count, local_size) = compute_info(shader);
        Ok(CompiledBinary {
            binary,
            info: CompilationInfo {
                gpr_count: u32::from(shader.info.gpr_count),
                instr_count: shader.info.instr_count,
                shared_mem_bytes,
                barrier_count,
                local_size,
            },
        })
    }
}

/// AMD backend — drives the RDNA2+ codegen pipeline.
pub struct AmdBackend;

impl Backend for AmdBackend {
    fn supports(&self, target: GpuTarget) -> bool {
        target.as_amd().is_some()
    }

    fn compile(&self, shader: &mut Shader<'_>) -> Result<CompiledBinary, CompileError> {
        let debug = std::env::var("CORAL_DEBUG_IR").is_ok();
        let compiled = crate::codegen::pipeline::compile_shader(shader, debug)?;

        let mut binary = Vec::with_capacity(compiled.code.len() * 4);
        for word in &compiled.code {
            binary.extend_from_slice(&word.to_le_bytes());
        }

        let (shared_mem_bytes, barrier_count, local_size) = compute_info(shader);
        let effective_gpr_count = u32::from(shader.info.gpr_count) + 2;
        Ok(CompiledBinary {
            binary,
            info: CompilationInfo {
                gpr_count: effective_gpr_count,
                instr_count: shader.info.instr_count,
                shared_mem_bytes,
                barrier_count,
                local_size,
            },
        })
    }
}

/// Resolve a backend for the given target.
///
/// # Errors
///
/// Returns [`CompileError::UnsupportedArch`] if no backend supports the target.
pub fn backend_for(target: GpuTarget) -> Result<Box<dyn Backend>, CompileError> {
    if target.as_nvidia().is_some() {
        return Ok(Box::new(NvidiaBackend));
    }
    if target.as_amd().is_some() {
        return Ok(Box::new(AmdBackend));
    }
    Err(CompileError::UnsupportedArch(target.to_string().into()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::gpu_arch::{AmdArch, IntelArch, NvArch};

    #[test]
    fn nvidia_backend_supports_nvidia() {
        let be = NvidiaBackend;
        assert!(be.supports(GpuTarget::Nvidia(NvArch::Sm70)));
        assert!(be.supports(GpuTarget::Nvidia(NvArch::Sm89)));
    }

    #[test]
    fn nvidia_backend_rejects_amd() {
        let be = NvidiaBackend;
        assert!(!be.supports(GpuTarget::Amd(AmdArch::Rdna2)));
    }

    #[test]
    fn nvidia_backend_rejects_intel() {
        let be = NvidiaBackend;
        assert!(!be.supports(GpuTarget::Intel(IntelArch::XeHpg)));
    }

    #[test]
    fn amd_backend_supports_amd() {
        let be = AmdBackend;
        assert!(be.supports(GpuTarget::Amd(AmdArch::Gcn5)));
        assert!(be.supports(GpuTarget::Amd(AmdArch::Rdna2)));
        assert!(be.supports(GpuTarget::Amd(AmdArch::Rdna3)));
    }

    #[test]
    fn amd_backend_rejects_nvidia() {
        let be = AmdBackend;
        assert!(!be.supports(GpuTarget::Nvidia(NvArch::Sm70)));
    }

    #[test]
    fn backend_for_nvidia_resolves() {
        let be = backend_for(GpuTarget::Nvidia(NvArch::Sm70));
        assert!(be.is_ok());
    }

    #[test]
    fn backend_for_amd_resolves() {
        let be = backend_for(GpuTarget::Amd(AmdArch::Rdna2));
        assert!(be.is_ok());
    }

    #[test]
    fn backend_for_gcn5_resolves() {
        let be = backend_for(GpuTarget::Amd(AmdArch::Gcn5));
        assert!(be.is_ok());
    }

    #[test]
    fn backend_for_intel_fails() {
        let be = backend_for(GpuTarget::Intel(IntelArch::XeHpg));
        assert!(be.is_err());
    }
}
