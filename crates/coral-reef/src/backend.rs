// SPDX-License-Identifier: AGPL-3.0-only
//! Vendor-specific compiler backend abstraction.
//!
//! The [`Backend`] trait defines the contract that vendor backends
//! implement.  Each backend takes an IR [`Shader`] (produced by a
//! [`Frontend`](crate::frontend::Frontend)) and produces a native GPU
//! binary.
//!
//! The default backend, [`NvidiaBackend`], drives the codegen
//! pipeline (optimize → legalize → register-allocate → encode).
//! AMD and Intel backends will implement the same trait.

use crate::codegen::ir::Shader;
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
}

/// A vendor-specific compiler backend.
///
/// Backends take a parsed and (optionally) pre-optimized [`Shader`] and
/// produce a native GPU binary for the target architecture.
pub trait Backend {
    /// The GPU targets this backend supports.
    ///
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

/// NVIDIA backend — drives the codegen pipeline (SM70+).
pub struct NvidiaBackend;

impl Backend for NvidiaBackend {
    fn supports(&self, target: GpuTarget) -> bool {
        target.as_nvidia().is_some()
    }

    fn compile(&self, shader: &mut Shader<'_>) -> Result<CompiledBinary, CompileError> {
        let compiled = crate::codegen::pipeline::compile_shader(shader, false)?;

        let mut binary = Vec::with_capacity(compiled.header.len() * 4 + compiled.code.len() * 4);
        for word in &compiled.header {
            binary.extend_from_slice(&word.to_le_bytes());
        }
        for word in &compiled.code {
            binary.extend_from_slice(&word.to_le_bytes());
        }

        Ok(CompiledBinary {
            binary,
            info: CompilationInfo {
                gpr_count: u32::from(shader.info.gpr_count),
                instr_count: shader.info.instr_count,
            },
        })
    }
}

/// Resolve a backend for the given target.
///
/// Currently only NVIDIA is supported; future AMD/Intel backends will
/// be registered here.
///
/// # Errors
///
/// Returns [`CompileError::UnsupportedArch`] if no backend supports the target.
pub fn backend_for(target: GpuTarget) -> Result<Box<dyn Backend>, CompileError> {
    if target.as_nvidia().is_some() {
        return Ok(Box::new(NvidiaBackend));
    }
    Err(CompileError::UnsupportedArch(target.to_string()))
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
        assert!(!be.supports(GpuTarget::Amd(AmdArch::Rdna3)));
    }

    #[test]
    fn nvidia_backend_rejects_intel() {
        let be = NvidiaBackend;
        assert!(!be.supports(GpuTarget::Intel(IntelArch::XeHpg)));
    }

    #[test]
    fn backend_for_nvidia_resolves() {
        let be = backend_for(GpuTarget::Nvidia(NvArch::Sm70));
        assert!(be.is_ok());
    }

    #[test]
    fn backend_for_amd_fails() {
        let be = backend_for(GpuTarget::Amd(AmdArch::Rdna3));
        assert!(be.is_err());
    }
}
