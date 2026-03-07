// SPDX-License-Identifier: AGPL-3.0-only
// Copyright © 2025-2026 ecoPrimals
// Derived from Collabora, Ltd. (2023) — upstream NAK.
//! Compilation pipeline: optimization passes, legalization, RA, encoding.

use super::ir::Shader;
use super::nv::shader_header::{self, CURRENT_MAX_SHADER_HEADER_SIZE};

/// Output of the compilation pipeline: shader program header and code.
#[derive(Debug, Clone)]
pub struct CompiledShader {
    /// Shader Program Header (SPH) — metadata for vertex/geometry/fragment shaders.
    /// Compute shaders have a zeroed header.
    pub header: [u32; CURRENT_MAX_SHADER_HEADER_SIZE],
    /// Encoded instruction words.
    pub code: Vec<u32>,
}

/// Run the full optimization and encoding pipeline on a shader.
pub fn compile_shader(
    shader: &mut Shader<'_>,
    _debug: bool,
) -> Result<CompiledShader, crate::CompileError> {
    // Optimization passes
    shader.opt_copy_prop();
    shader.opt_dce();
    shader.opt_crs();
    shader.opt_lop();
    shader.opt_prmt();
    shader.opt_out();
    shader.opt_jump_thread();
    shader.opt_bar_prop();
    shader.opt_uniform_instrs();

    // Pre-RA scheduling
    shader.opt_instr_sched_prepass();

    // f64 transcendental software lowering
    shader.lower_f64_transcendentals();

    // Legalize for target arch
    shader.legalize()?;

    // Register allocation
    shader.assign_regs();

    // Post-RA lowering
    shader.lower_par_copies();
    shader.lower_copy_swap();

    // Dependency calculation
    shader.assign_deps_serial();

    // Remove annotations before post-RA scheduling (they're not hardware ops)
    shader.remove_annotations();

    // Post-RA scheduling
    shader.opt_instr_sched_postpass();

    // Gather info for header encoding (uses gpr_count from RA)
    shader.gather_info()?;

    // Encode to binary
    let code = shader.sm.encode_shader(shader)?;
    let header = shader_header::encode_header(shader.sm, &shader.info, None);

    Ok(CompiledShader { header, code })
}
