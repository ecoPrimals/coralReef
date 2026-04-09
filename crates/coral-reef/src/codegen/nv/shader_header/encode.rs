// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright © 2025-2026 ecoPrimals
// Derived from Collabora, Ltd. (2023)

//! High-level `encode_header` from compiler `ShaderInfo`.

use crate::codegen::ir::{ShaderInfo, ShaderIoInfo, ShaderModel, ShaderStageInfo};

use super::program_header::ShaderProgramHeader;
use super::sphv3_layout::CURRENT_MAX_SHADER_HEADER_SIZE;
use super::types::{FragmentShaderKey, ShaderType};

/// Encodes the shader program header words for graphics stages from compiler IR.
pub fn encode_header(
    sm: &dyn ShaderModel,
    shader_info: &ShaderInfo,
    fs_key: Option<&FragmentShaderKey>,
) -> [u32; CURRENT_MAX_SHADER_HEADER_SIZE] {
    if let ShaderStageInfo::Compute(_) = shader_info.stage {
        return [0_u32; CURRENT_MAX_SHADER_HEADER_SIZE];
    }

    let mut sph = ShaderProgramHeader::new(ShaderType::from(&shader_info.stage), sm.sm());

    let shared_local_mem_size = shader_info.shared_local_mem_size.next_multiple_of(16);
    sph.set_sass_version(1);
    sph.set_does_load_or_store(
        shader_info.uses_global_mem || (sm.is_kepler() && shared_local_mem_size > 0),
    );
    sph.set_does_global_store(shader_info.writes_global_mem);
    sph.set_does_fp64(shader_info.uses_fp64);

    sph.set_shader_local_memory_size(shared_local_mem_size.into());
    let crs_size = sm.crs_size(shader_info.max_crs_depth);
    sph.set_shader_local_memory_crs_size(crs_size);

    match &shader_info.io {
        ShaderIoInfo::Vtg(io) => {
            sph.set_imap_system_values_ab(io.sysvals_in.ab);
            sph.set_imap_system_values_c(io.sysvals_in.c);
            sph.set_imap_system_values_d_vtg(io.sysvals_in_d);

            for (index, value) in io.attr_in.iter().enumerate() {
                sph.set_imap_vector_vtg(index, *value);
            }

            for (index, value) in io.attr_out.iter().enumerate() {
                sph.set_omap_vector(index, *value);
            }

            sph.set_store_req_start(io.store_req_start);
            sph.set_store_req_end(io.store_req_end);

            sph.set_omap_system_values_ab(io.sysvals_out.ab);
            sph.set_omap_system_values_c(io.sysvals_out.c);
            sph.set_omap_system_values_d_vtg(io.sysvals_out_d);
        }
        ShaderIoInfo::Fragment(io) => {
            sph.set_imap_system_values_ab(io.sysvals_in.ab);
            sph.set_imap_system_values_c(io.sysvals_in.c);

            for (index, imap) in io.sysvals_in_d.iter().enumerate() {
                sph.set_imap_system_values_d_ps(index, *imap);
            }

            for (index, imap) in io.attr_in.iter().enumerate() {
                sph.set_imap_vector_ps(index, *imap);
            }

            let uses_underestimate = fs_key.is_some_and(|key| key.uses_underestimate);

            // This isn't so much a "Do we write multiple render targets?" bit
            // as a "Should color0 be broadcast to all render targets?" bit. In
            // other words, it's the gl_FragCoord behavior, not gl_FragData.
            //
            // For now, we always set it to true because Vulkan requires
            // explicit fragment output locations.
            sph.set_multiple_render_target_enable(true);

            sph.set_omap_sample_mask(io.writes_sample_mask);
            sph.set_omap_depth(io.writes_depth);
            sph.set_omap_targets(io.writes_color);
            sph.set_uses_underestimate(uses_underestimate);

            for (index, value) in io.barycentric_attr_in.iter().enumerate() {
                sph.set_pervertex_imap_vector(index, *value);
            }
        }
        ShaderIoInfo::None => {}
    }

    match &shader_info.stage {
        ShaderStageInfo::Vertex(stage) => {
            sph.set_isbe_space_sharing_enable(stage.isbe_space_sharing_enable);
        }
        ShaderStageInfo::Fragment(stage) => {
            let zs_self_dep = fs_key.is_some_and(|key| key.zs_self_dep);
            sph.set_kills_pixels(stage.uses_kill || zs_self_dep);
            sph.set_does_interlock(stage.does_interlock);
        }
        ShaderStageInfo::Geometry(stage) => {
            sph.set_gs_passthrough_enable(stage.passthrough_enable);
            sph.set_stream_out_mask(stage.stream_out_mask);
            sph.set_threads_per_input_primitive(stage.threads_per_input_primitive);
            sph.set_output_topology(stage.output_topology);
            sph.set_max_output_vertex_count(stage.max_output_vertex_count);
        }
        ShaderStageInfo::TessellationInit(stage) => {
            sph.set_per_patch_attribute_count(stage.per_patch_attribute_count);
            sph.set_threads_per_input_primitive(stage.threads_per_patch);
        }
        ShaderStageInfo::Compute(_) => {
            crate::codegen::ice!("Compute shaders don't have a SPH!")
        }
        ShaderStageInfo::Tessellation(_) => {}
    }

    sph.data
}
