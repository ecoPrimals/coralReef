// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright © 2025-2026 ecoPrimals
// Derived from Collabora, Ltd. (2023)

use super::*;

use bitview::BitViewable;

#[test]
fn test_shader_program_header_vertex() {
    let sph = ShaderProgramHeader::new(ShaderType::Vertex, 75);
    assert_eq!(
        sph.get_field(SPHV3_T1_SHADER_TYPE),
        u64::from(SPHV3_T1_SHADER_TYPE_VERTEX)
    );
    assert_eq!(
        sph.get_field(SPHV3_T1_SPH_TYPE),
        u64::from(SPHV3_T1_SPH_TYPE_TYPE_01_VTG)
    );
}

#[test]
fn test_shader_program_header_fragment() {
    let sph = ShaderProgramHeader::new(ShaderType::Fragment, 75);
    assert_eq!(
        sph.get_field(SPHV3_T1_SHADER_TYPE),
        u64::from(SPHV3_T1_SHADER_TYPE_PIXEL)
    );
    assert_eq!(
        sph.get_field(SPHV3_T1_SPH_TYPE),
        u64::from(SPHV3_T1_SPH_TYPE_TYPE_02_PS)
    );
}

#[test]
fn test_shader_program_header_geometry() {
    let sph = ShaderProgramHeader::new(ShaderType::Geometry, 75);
    assert_eq!(
        sph.get_field(SPHV3_T1_SHADER_TYPE),
        u64::from(SPHV3_T1_SHADER_TYPE_GEOMETRY)
    );
}

#[test]
fn test_shader_program_header_tessellation() {
    let sph = ShaderProgramHeader::new(ShaderType::Tessellation, 75);
    assert_eq!(
        sph.get_field(SPHV3_T1_SHADER_TYPE),
        u64::from(SPHV3_T1_SHADER_TYPE_TESSELLATION)
    );
}

#[test]
fn test_shader_program_header_tessellation_init() {
    let sph = ShaderProgramHeader::new(ShaderType::TessellationInit, 75);
    assert_eq!(
        sph.get_field(SPHV3_T1_SHADER_TYPE),
        u64::from(SPHV3_T1_SHADER_TYPE_TESSELLATION_INIT)
    );
}

#[test]
fn test_set_multiple_render_target_enable_roundtrip() {
    let mut sph = ShaderProgramHeader::new(ShaderType::Fragment, 75);
    sph.set_multiple_render_target_enable(true);
    assert_eq!(sph.get_field(SPHV3_T1_MRT_ENABLE), 1);
    sph.set_multiple_render_target_enable(false);
    assert_eq!(sph.get_field(SPHV3_T1_MRT_ENABLE), 0);
}

#[test]
fn test_set_kills_pixels_roundtrip() {
    let mut sph = ShaderProgramHeader::new(ShaderType::Fragment, 75);
    sph.set_kills_pixels(true);
    assert_eq!(sph.get_field(SPHV3_T1_KILLS_PIXELS), 1);
    sph.set_kills_pixels(false);
    assert_eq!(sph.get_field(SPHV3_T1_KILLS_PIXELS), 0);
}

#[test]
fn test_set_does_global_store_roundtrip() {
    let mut sph = ShaderProgramHeader::new(ShaderType::Fragment, 75);
    sph.set_does_global_store(true);
    assert_eq!(sph.get_field(SPHV3_T1_DOES_GLOBAL_STORE), 1);
}

#[test]
fn test_set_sass_version_roundtrip() {
    let mut sph = ShaderProgramHeader::new(ShaderType::Vertex, 75);
    sph.set_sass_version(5);
    assert_eq!(sph.get_field(SPHV3_T1_SASS_VERSION), 5);
}

#[test]
fn test_set_omap_sample_mask_fragment() {
    let mut sph = ShaderProgramHeader::new(ShaderType::Fragment, 75);
    sph.set_omap_sample_mask(true);
    assert_eq!(sph.get_field(SPHV3_T2_OMAP_SAMPLE_MASK), 1);
}

#[test]
fn test_set_omap_depth_fragment() {
    let mut sph = ShaderProgramHeader::new(ShaderType::Fragment, 75);
    sph.set_omap_depth(true);
    assert_eq!(sph.get_field(SPHV3_T2_OMAP_DEPTH), 1);
}

#[test]
fn test_sph_version_sm73() {
    let sph = ShaderProgramHeader::new(ShaderType::Vertex, 73);
    assert_eq!(sph.get_field(SPHV3_T1_VERSION), 4);
}

#[test]
fn test_sph_version_sm70() {
    let sph = ShaderProgramHeader::new(ShaderType::Vertex, 70);
    assert_eq!(sph.get_field(SPHV3_T1_VERSION), 3);
}

#[test]
fn test_output_topology_roundtrip() {
    let mut sph = ShaderProgramHeader::new(ShaderType::Geometry, 75);
    sph.set_output_topology(OutputTopology::PointList);
    assert_eq!(sph.get_field(SPHV3_T1_OUTPUT_TOPOLOGY), 0);
    sph.set_output_topology(OutputTopology::LineStrip);
    assert_eq!(sph.get_field(SPHV3_T1_OUTPUT_TOPOLOGY), 1);
    sph.set_output_topology(OutputTopology::TriangleStrip);
    assert_eq!(sph.get_field(SPHV3_T1_OUTPUT_TOPOLOGY), 2);
}

#[test]
fn test_pixel_imap_into_u8() {
    assert_eq!(u8::from(PixelImap::Unused), 0);
    assert_eq!(u8::from(PixelImap::Constant), 1);
    assert_eq!(u8::from(PixelImap::Perspective), 2);
    assert_eq!(u8::from(PixelImap::ScreenLinear), 3);
}

#[test]
fn test_set_does_load_or_store() {
    let mut sph = ShaderProgramHeader::new(ShaderType::Vertex, 75);
    sph.set_does_load_or_store(true);
    assert_eq!(sph.get_field(SPHV3_T1_DOES_LOAD_OR_STORE), 1);
    sph.set_does_load_or_store(false);
    assert_eq!(sph.get_field(SPHV3_T1_DOES_LOAD_OR_STORE), 0);
}

#[test]
fn test_set_does_fp64() {
    let mut sph = ShaderProgramHeader::new(ShaderType::Vertex, 75);
    sph.set_does_fp64(true);
    assert_eq!(sph.get_field(SPHV3_T1_DOES_FP64), 1);
}

#[test]
fn test_set_stream_out_mask() {
    let mut sph = ShaderProgramHeader::new(ShaderType::Geometry, 75);
    sph.set_stream_out_mask(0xf);
    assert_eq!(sph.get_field(SPHV3_T1_STREAM_OUT_MASK), 0xf);
}

#[test]
fn test_set_threads_per_input_primitive() {
    let mut sph = ShaderProgramHeader::new(ShaderType::Geometry, 75);
    sph.set_threads_per_input_primitive(4);
    assert_eq!(sph.get_field(SPHV3_T1_THREADS_PER_INPUT_PRIMITIVE), 4);
}

#[test]
fn test_set_max_output_vertex_count() {
    let mut sph = ShaderProgramHeader::new(ShaderType::Geometry, 75);
    sph.set_max_output_vertex_count(256);
    assert_eq!(sph.get_field(SPHV3_T1_MAX_OUTPUT_VERTEX_COUNT), 256);
}

#[test]
fn test_set_store_req_start_end() {
    let mut sph = ShaderProgramHeader::new(ShaderType::Vertex, 75);
    sph.set_store_req_start(1);
    sph.set_store_req_end(5);
    assert_eq!(sph.get_field(SPHV3_T1_STORE_REQ_START), 1);
    assert_eq!(sph.get_field(SPHV3_T1_STORE_REQ_END), 5);
}

#[test]
fn test_set_shader_local_memory_size() {
    let mut sph = ShaderProgramHeader::new(ShaderType::Vertex, 75);
    sph.set_shader_local_memory_size(256);
    assert_eq!(sph.get_field(SPHV3_T1_SHADER_LOCAL_MEMORY_LOW_SIZE), 256);
}

#[test]
fn test_set_shader_local_memory_crs_size() {
    let mut sph = ShaderProgramHeader::new(ShaderType::Vertex, 75);
    sph.set_shader_local_memory_crs_size(64);
    assert_eq!(sph.get_field(SPHV3_T1_SHADER_LOCAL_MEMORY_CRS_SIZE), 64);
}

#[test]
fn test_set_imap_vector_ps() {
    let mut sph = ShaderProgramHeader::new(ShaderType::Fragment, 75);
    sph.set_imap_vector_ps(0, PixelImap::Perspective);
    sph.set_imap_vector_ps(1, PixelImap::ScreenLinear);
    assert_eq!(
        sph.imap_g_ps().get_field(0..2),
        u64::from(u8::from(PixelImap::Perspective))
    );
    assert_eq!(
        sph.imap_g_ps().get_field(2..4),
        u64::from(u8::from(PixelImap::ScreenLinear))
    );
}

#[test]
fn test_set_imap_system_values_ab() {
    let mut sph = ShaderProgramHeader::new(ShaderType::Vertex, 75);
    sph.set_imap_system_values_ab(0x1234_5678);
    assert_eq!(sph.imap_system_values_ab().get_field(0..32), 0x1234_5678);
}

#[test]
fn test_set_omap_targets() {
    let mut sph = ShaderProgramHeader::new(ShaderType::Fragment, 75);
    sph.set_omap_targets(0x0000_000f);
    assert_eq!(sph.get_field(SPHV3_T2_OMAP_SAMPLE_MASK), 0);
    assert_eq!(sph.omap_target().get_field(0..32), 0x0000_000f);
}

#[test]
fn test_set_does_interlock() {
    let mut sph = ShaderProgramHeader::new(ShaderType::Fragment, 75);
    sph.set_does_interlock(true);
    assert_eq!(sph.get_field(SPHV3_T2_DOES_INTERLOCK), 1);
}

#[test]
fn test_set_uses_underestimate() {
    let mut sph = ShaderProgramHeader::new(ShaderType::Fragment, 75);
    sph.set_uses_underestimate(true);
    assert_eq!(sph.get_field(SPHV3_T2_USES_UNDERESTIMATE), 1);
}

#[test]
fn test_encode_header_compute_returns_zeros() {
    use crate::codegen::ir::{
        ComputeShaderInfo, ShaderInfo, ShaderIoInfo, ShaderModelInfo, ShaderStageInfo,
    };
    let sm = ShaderModelInfo::new(70, 64);
    let info = ShaderInfo {
        max_warps_per_sm: 0,
        gpr_count: 0,
        control_barrier_count: 0,
        instr_count: 0,
        static_cycle_count: 0,
        spills_to_mem: 0,
        fills_from_mem: 0,
        spills_to_reg: 0,
        fills_from_reg: 0,
        shared_local_mem_size: 0,
        max_crs_depth: 0,
        uses_global_mem: false,
        writes_global_mem: false,
        uses_fp64: false,
        stage: ShaderStageInfo::Compute(ComputeShaderInfo {
            local_size: [1, 1, 1],
            shared_mem_size: 0,
        }),
        io: ShaderIoInfo::None,
    };
    let header = encode_header(&sm, &info, None);
    assert!(header.iter().all(|&w| w == 0));
}

#[test]
fn test_encode_header_vertex_populates_sph_words() {
    use crate::codegen::ir::{
        ShaderInfo, ShaderIoInfo, ShaderModelInfo, ShaderStageInfo, SysValInfo, VertexShaderInfo,
        VtgIoInfo,
    };
    let sm = ShaderModelInfo::new(75, 64);
    let vtg = VtgIoInfo {
        sysvals_in: SysValInfo { ab: 0x0f, c: 0x0c },
        sysvals_in_d: 0x05,
        sysvals_out: SysValInfo { ab: 0x20, c: 0x01 },
        sysvals_out_d: 0x02,
        attr_in: [0x0a, 0x0b, 0, 0],
        attr_out: [0x0c, 0, 0, 0],
        store_req_start: 0,
        store_req_end: 3,
        clip_enable: 0,
        cull_enable: 0,
        xfb: None,
    };
    let info = ShaderInfo {
        max_warps_per_sm: 0,
        gpr_count: 0,
        control_barrier_count: 0,
        instr_count: 0,
        static_cycle_count: 0,
        spills_to_mem: 0,
        fills_from_mem: 0,
        spills_to_reg: 0,
        fills_from_reg: 0,
        shared_local_mem_size: 256,
        max_crs_depth: 0,
        uses_global_mem: true,
        writes_global_mem: false,
        uses_fp64: true,
        stage: ShaderStageInfo::Vertex(VertexShaderInfo {
            isbe_space_sharing_enable: true,
        }),
        io: ShaderIoInfo::Vtg(vtg),
    };
    let header = encode_header(&sm, &info, None);
    let nonzero = header.iter().filter(|&&w| w != 0).count();
    assert!(
        nonzero > 4,
        "vertex encode_header should set multiple SPH words from VTG I/O"
    );
}
