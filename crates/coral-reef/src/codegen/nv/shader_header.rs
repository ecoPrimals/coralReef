// SPDX-License-Identifier: AGPL-3.0-only
// Copyright © 2025-2026 ecoPrimals
// Derived from Collabora, Ltd. (2023)

use crate::codegen::ir::{ShaderInfo, ShaderIoInfo, ShaderModel, ShaderStageInfo};

use std::ops::Range;

/// Fragment shader variant key — controls SPH encoding for FS-specific behavior.
#[derive(Debug, Default, Clone, Copy)]
pub struct FragmentShaderKey {
    /// Whether the FS uses conservative rasterization underestimate mode.
    pub uses_underestimate: bool,
    /// Whether there is a depth/stencil self-dependency.
    pub zs_self_dep: bool,
}

pub const _SPHV3_SHADER_HEADER_SIZE: usize = 20;
pub const SPHV4_SHADER_HEADER_SIZE: usize = 32;
pub const CURRENT_MAX_SHADER_HEADER_SIZE: usize = SPHV4_SHADER_HEADER_SIZE;

// SPH v3 Type 1 (SPHV3_T1) bit-field ranges and enum values
pub const SPHV3_T1_SPH_TYPE: std::ops::Range<usize> = 0..4;
pub const SPHV3_T1_SPH_TYPE_TYPE_01_VTG: u32 = 1;
pub const SPHV3_T1_SPH_TYPE_TYPE_02_PS: u32 = 2;
pub const SPHV3_T1_VERSION: std::ops::Range<usize> = 4..8;
pub const SPHV3_T1_SHADER_TYPE: std::ops::Range<usize> = 8..12;
pub const SPHV3_T1_SHADER_TYPE_VERTEX: u32 = 0;
pub const SPHV3_T1_SHADER_TYPE_TESSELLATION_INIT: u32 = 1;
pub const SPHV3_T1_SHADER_TYPE_TESSELLATION: u32 = 2;
pub const SPHV3_T1_SHADER_TYPE_GEOMETRY: u32 = 3;
pub const SPHV3_T1_SHADER_TYPE_PIXEL: u32 = 4;
pub const SPHV3_T1_MRT_ENABLE: std::ops::Range<usize> = 12..13;
pub const SPHV3_T1_KILLS_PIXELS: std::ops::Range<usize> = 13..14;
pub const SPHV3_T1_DOES_GLOBAL_STORE: std::ops::Range<usize> = 14..15;
pub const SPHV3_T1_SASS_VERSION: std::ops::Range<usize> = 16..24;
pub const SPHV3_T1_DOES_LOAD_OR_STORE: std::ops::Range<usize> = 24..25;
pub const SPHV3_T1_DOES_FP64: std::ops::Range<usize> = 25..26;
pub const SPHV3_T1_STREAM_OUT_MASK: std::ops::Range<usize> = 26..30;
pub const SPHV3_T1_SHADER_LOCAL_MEMORY_LOW_SIZE: std::ops::Range<usize> = 96..120;
pub const SPHV3_T1_SHADER_LOCAL_MEMORY_HIGH_SIZE: std::ops::Range<usize> = 120..144;
pub const SPHV3_T1_PER_PATCH_ATTRIBUTE_COUNT: std::ops::Range<usize> = 144..152;
pub const SPHV3_T1_RESERVED_COMMON_B: std::ops::Range<usize> = 152..156;
pub const SPHV3_T1_THREADS_PER_INPUT_PRIMITIVE: std::ops::Range<usize> = 156..160;
pub const SPHV3_T1_SHADER_LOCAL_MEMORY_CRS_SIZE: std::ops::Range<usize> = 336..360;
pub const SPHV3_T1_OUTPUT_TOPOLOGY: std::ops::Range<usize> = 360..363;
pub const SPHV3_T1_OUTPUT_TOPOLOGY_POINTLIST: u32 = 0;
pub const SPHV3_T1_OUTPUT_TOPOLOGY_LINESTRIP: u32 = 1;
pub const SPHV3_T1_OUTPUT_TOPOLOGY_TRIANGLESTRIP: u32 = 2;
pub const SPHV3_T1_MAX_OUTPUT_VERTEX_COUNT: std::ops::Range<usize> = 363..376;
pub const SPHV3_T1_STORE_REQ_START: std::ops::Range<usize> = 376..384;
pub const SPHV3_T1_STORE_REQ_END: std::ops::Range<usize> = 384..392;
pub const SPHV3_T1_GS_PASSTHROUGH_ENABLE: std::ops::Range<usize> = 24..25;
pub const SPHV3_T1_ISBE_SPACE_SHARING_ENABLE: std::ops::Range<usize> = 25..26;
/// Kepler+ per-patch attribute count high nibble (bits 148..152).
pub const SPHV3_T1_PER_PATCH_ATTRIBUTE_COUNT_HIGH: std::ops::Range<usize> = 148..152;

// SPH v3 Type 2 (SPHV3_T2) bit-field ranges
pub const SPHV3_T2_OMAP_SAMPLE_MASK: std::ops::Range<usize> = 608..609;
pub const SPHV3_T2_OMAP_DEPTH: std::ops::Range<usize> = 609..610;
pub const SPHV3_T2_DOES_INTERLOCK: std::ops::Range<usize> = 610..611;
pub const SPHV3_T2_USES_UNDERESTIMATE: std::ops::Range<usize> = 611..612;

use bitview::*;
type SubSPHView<'a> = BitMutSubsetView<'a>;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ShaderType {
    Vertex,
    TessellationInit,
    Tessellation,
    Geometry,
    Fragment,
}

impl From<&ShaderStageInfo> for ShaderType {
    fn from(value: &ShaderStageInfo) -> Self {
        match value {
            ShaderStageInfo::Vertex(_) => Self::Vertex,
            ShaderStageInfo::Fragment(_) => Self::Fragment,
            ShaderStageInfo::Geometry(_) => Self::Geometry,
            ShaderStageInfo::TessellationInit(_) => Self::TessellationInit,
            ShaderStageInfo::Tessellation(_) => Self::Tessellation,
            ShaderStageInfo::Compute(_) => {
                crate::codegen::ice!("Invalid ShaderStageInfo {value:?}")
            }
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OutputTopology {
    PointList,
    LineStrip,
    TriangleStrip,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PixelImap {
    Unused,
    Constant,
    Perspective,
    ScreenLinear,
}

impl From<PixelImap> for u8 {
    fn from(value: PixelImap) -> Self {
        match value {
            PixelImap::Unused => 0,
            PixelImap::Constant => 1,
            PixelImap::Perspective => 2,
            PixelImap::ScreenLinear => 3,
        }
    }
}

#[derive(Debug)]
pub struct ShaderProgramHeader {
    pub data: [u32; CURRENT_MAX_SHADER_HEADER_SIZE],
    shader_type: ShaderType,
}

impl BitViewable for ShaderProgramHeader {
    fn bits(&self) -> usize {
        self.data.bits()
    }

    fn get_bit_range_u64(&self, range: Range<usize>) -> u64 {
        self.data.get_bit_range_u64(range)
    }
}

impl BitMutViewable for ShaderProgramHeader {
    fn set_bit_range_u64(&mut self, range: Range<usize>, val: u64) {
        self.data.set_bit_range_u64(range, val);
    }
}

impl ShaderProgramHeader {
    pub fn new(shader_type: ShaderType, sm: u8) -> Self {
        let mut res = Self {
            data: [0; CURRENT_MAX_SHADER_HEADER_SIZE],
            shader_type,
        };

        let sph_type = if shader_type == ShaderType::Fragment {
            SPHV3_T1_SPH_TYPE_TYPE_02_PS
        } else {
            SPHV3_T1_SPH_TYPE_TYPE_01_VTG
        };

        let sph_version = if sm >= 73 { 4 } else { 3 };
        res.set_sph_type(sph_type, sph_version);
        res.set_shader_type(shader_type);

        res
    }

    #[inline]
    fn imap_system_values_ab(&mut self) -> SubSPHView<'_> {
        new_subset(&mut self.data, 160, 32)
    }

    #[inline]
    fn imap_g_vtg(&mut self) -> SubSPHView<'_> {
        assert!(self.shader_type != ShaderType::Fragment);

        new_subset(&mut self.data, 192, 128)
    }

    #[inline]
    fn imap_g_ps(&mut self) -> SubSPHView<'_> {
        assert!(self.shader_type == ShaderType::Fragment);

        new_subset(&mut self.data, 192, 256)
    }

    #[inline]
    fn imap_system_values_c(&mut self) -> SubSPHView<'_> {
        if self.shader_type == ShaderType::Fragment {
            new_subset(&mut self.data, 464, 16)
        } else {
            new_subset(&mut self.data, 336, 16)
        }
    }

    #[inline]
    fn imap_system_values_d_vtg(&mut self) -> SubSPHView<'_> {
        assert!(self.shader_type != ShaderType::Fragment);
        new_subset(&mut self.data, 392, 8)
    }

    #[inline]
    fn omap_system_values_ab(&mut self) -> SubSPHView<'_> {
        assert!(self.shader_type != ShaderType::Fragment);
        new_subset(&mut self.data, 400, 32)
    }

    #[inline]
    fn omap_g(&mut self) -> SubSPHView<'_> {
        assert!(self.shader_type != ShaderType::Fragment);

        new_subset(&mut self.data, 432, 128)
    }

    #[inline]
    fn omap_system_values_c(&mut self) -> SubSPHView<'_> {
        assert!(self.shader_type != ShaderType::Fragment);
        new_subset(&mut self.data, 576, 16)
    }

    #[inline]
    fn imap_system_values_d_ps(&mut self) -> SubSPHView<'_> {
        assert!(self.shader_type == ShaderType::Fragment);
        new_subset(&mut self.data, 560, 16)
    }

    #[inline]
    fn omap_target(&mut self) -> SubSPHView<'_> {
        assert!(self.shader_type == ShaderType::Fragment);

        new_subset(&mut self.data, 576, 32)
    }

    #[inline]
    fn omap_system_values_d_vtg(&mut self) -> SubSPHView<'_> {
        assert!(self.shader_type != ShaderType::Fragment);
        new_subset(&mut self.data, 632, 8)
    }

    #[inline]
    fn set_sph_type(&mut self, sph_type: u32, sph_version: u8) {
        self.set_field(SPHV3_T1_SPH_TYPE, sph_type);
        self.set_field(SPHV3_T1_VERSION, sph_version);
    }

    #[inline]
    fn set_shader_type(&mut self, shader_type: ShaderType) {
        self.set_field(
            SPHV3_T1_SHADER_TYPE,
            match shader_type {
                ShaderType::Vertex => SPHV3_T1_SHADER_TYPE_VERTEX,
                ShaderType::TessellationInit => SPHV3_T1_SHADER_TYPE_TESSELLATION_INIT,
                ShaderType::Tessellation => SPHV3_T1_SHADER_TYPE_TESSELLATION,
                ShaderType::Geometry => SPHV3_T1_SHADER_TYPE_GEOMETRY,
                ShaderType::Fragment => SPHV3_T1_SHADER_TYPE_PIXEL,
            },
        );
    }

    #[inline]
    pub fn set_multiple_render_target_enable(&mut self, mrt_enable: bool) {
        self.set_field(SPHV3_T1_MRT_ENABLE, mrt_enable);
    }

    #[inline]
    pub fn set_kills_pixels(&mut self, kills_pixels: bool) {
        self.set_field(SPHV3_T1_KILLS_PIXELS, kills_pixels);
    }

    #[inline]
    pub fn set_does_global_store(&mut self, does_global_store: bool) {
        self.set_field(SPHV3_T1_DOES_GLOBAL_STORE, does_global_store);
    }

    #[inline]
    pub fn set_sass_version(&mut self, sass_version: u8) {
        self.set_field(SPHV3_T1_SASS_VERSION, sass_version);
    }

    #[inline]
    pub fn set_gs_passthrough_enable(&mut self, gs_passthrough_enable: bool) {
        assert!(self.shader_type == ShaderType::Geometry);
        self.set_field(SPHV3_T1_GS_PASSTHROUGH_ENABLE, gs_passthrough_enable);
    }

    #[inline]
    pub fn set_isbe_space_sharing_enable(&mut self, isbe_space_sharing_enable: bool) {
        assert!(self.shader_type == ShaderType::Vertex);
        self.set_field(
            SPHV3_T1_ISBE_SPACE_SHARING_ENABLE,
            isbe_space_sharing_enable,
        );
    }

    #[inline]
    pub fn set_does_load_or_store(&mut self, does_load_or_store: bool) {
        self.set_field(SPHV3_T1_DOES_LOAD_OR_STORE, does_load_or_store);
    }

    #[inline]
    pub fn set_does_fp64(&mut self, does_fp64: bool) {
        self.set_field(SPHV3_T1_DOES_FP64, does_fp64);
    }

    #[inline]
    pub fn set_stream_out_mask(&mut self, stream_out_mask: u8) {
        self.set_field(SPHV3_T1_STREAM_OUT_MASK, stream_out_mask);
    }

    #[inline]
    pub fn set_shader_local_memory_size(&mut self, shader_local_memory_size: u64) {
        assert!(shader_local_memory_size <= 0xffff_ffff_ffff);
        assert!(shader_local_memory_size % 0x10 == 0);

        let low = (shader_local_memory_size & 0xff_ffff) as u32;
        let high = ((shader_local_memory_size >> 32) & 0xff_ffff) as u32;

        self.set_field(SPHV3_T1_SHADER_LOCAL_MEMORY_LOW_SIZE, low);
        self.set_field(SPHV3_T1_SHADER_LOCAL_MEMORY_HIGH_SIZE, high);
    }

    #[inline]
    pub fn set_per_patch_attribute_count(&mut self, per_patch_attribute_count: u8) {
        assert!(self.shader_type == ShaderType::TessellationInit);

        self.set_field(
            SPHV3_T1_PER_PATCH_ATTRIBUTE_COUNT,
            per_patch_attribute_count,
        );

        // This is Kepler+
        self.set_field(SPHV3_T1_RESERVED_COMMON_B, per_patch_attribute_count & 0xf);
        self.set_field(
            SPHV3_T1_PER_PATCH_ATTRIBUTE_COUNT_HIGH,
            per_patch_attribute_count >> 4,
        );
    }

    #[inline]
    pub fn set_threads_per_input_primitive(&mut self, threads_per_input_primitive: u8) {
        self.set_field(
            SPHV3_T1_THREADS_PER_INPUT_PRIMITIVE,
            threads_per_input_primitive,
        );
    }

    #[inline]
    pub fn set_shader_local_memory_crs_size(&mut self, shader_local_memory_crs_size: u32) {
        assert!(shader_local_memory_crs_size <= 0xff_ffff);
        self.set_field(
            SPHV3_T1_SHADER_LOCAL_MEMORY_CRS_SIZE,
            shader_local_memory_crs_size,
        );
    }

    #[inline]
    pub fn set_output_topology(&mut self, output_topology: OutputTopology) {
        self.set_field(
            SPHV3_T1_OUTPUT_TOPOLOGY,
            match output_topology {
                OutputTopology::PointList => SPHV3_T1_OUTPUT_TOPOLOGY_POINTLIST,
                OutputTopology::LineStrip => SPHV3_T1_OUTPUT_TOPOLOGY_LINESTRIP,
                OutputTopology::TriangleStrip => SPHV3_T1_OUTPUT_TOPOLOGY_TRIANGLESTRIP,
            },
        );
    }

    #[inline]
    pub fn set_max_output_vertex_count(&mut self, max_output_vertex_count: u16) {
        assert!(max_output_vertex_count <= 0xfff);
        self.set_field(SPHV3_T1_MAX_OUTPUT_VERTEX_COUNT, max_output_vertex_count);
    }

    #[inline]
    pub fn set_store_req_start(&mut self, store_req_start: u8) {
        self.set_field(SPHV3_T1_STORE_REQ_START, store_req_start);
    }

    #[inline]
    pub fn set_store_req_end(&mut self, store_req_end: u8) {
        self.set_field(SPHV3_T1_STORE_REQ_END, store_req_end);
    }

    pub fn set_imap_system_values_ab(&mut self, val: u32) {
        self.imap_system_values_ab().set_field(0..32, val);
    }

    pub fn set_imap_system_values_c(&mut self, val: u16) {
        self.imap_system_values_c().set_field(0..16, val);
    }

    pub fn set_imap_system_values_d_vtg(&mut self, val: u8) {
        assert!(self.shader_type != ShaderType::Fragment);
        self.imap_system_values_d_vtg().set_field(0..8, val);
    }

    #[inline]
    pub fn set_imap_vector_ps(&mut self, index: usize, value: PixelImap) {
        assert!(index < 128);
        assert!(self.shader_type == ShaderType::Fragment);

        self.imap_g_ps()
            .set_field(index * 2..(index + 1) * 2, u8::from(value));
    }

    #[inline]
    pub fn set_imap_system_values_d_ps(&mut self, index: usize, value: PixelImap) {
        assert!(index < 8);
        assert!(self.shader_type == ShaderType::Fragment);

        self.imap_system_values_d_ps()
            .set_field(index * 2..(index + 1) * 2, u8::from(value));
    }

    #[inline]
    pub fn set_imap_vector_vtg(&mut self, index: usize, value: u32) {
        assert!(index < 4);
        assert!(self.shader_type != ShaderType::Fragment);

        self.imap_g_vtg()
            .set_field(index * 32..(index + 1) * 32, value);
    }

    #[inline]
    pub fn set_omap_system_values_ab(&mut self, val: u32) {
        self.omap_system_values_ab().set_field(0..32, val);
    }

    #[inline]
    pub fn set_omap_system_values_c(&mut self, val: u16) {
        self.omap_system_values_c().set_field(0..16, val);
    }

    pub fn set_omap_system_values_d_vtg(&mut self, val: u8) {
        assert!(self.shader_type != ShaderType::Fragment);
        self.omap_system_values_d_vtg().set_field(0..8, val);
    }

    #[inline]
    pub fn set_omap_vector(&mut self, index: usize, value: u32) {
        assert!(index < 4);
        assert!(self.shader_type != ShaderType::Fragment);

        self.omap_g().set_field(index * 32..(index + 1) * 32, value);
    }

    #[inline]
    pub fn set_omap_targets(&mut self, value: u32) {
        self.omap_target().set_field(0..32, value);
    }

    #[inline]
    pub fn set_omap_sample_mask(&mut self, sample_mask: bool) {
        assert!(self.shader_type == ShaderType::Fragment);
        self.set_field(SPHV3_T2_OMAP_SAMPLE_MASK, sample_mask);
    }

    #[inline]
    pub fn set_omap_depth(&mut self, depth: bool) {
        assert!(self.shader_type == ShaderType::Fragment);
        self.set_field(SPHV3_T2_OMAP_DEPTH, depth);
    }

    #[inline]
    pub fn set_does_interlock(&mut self, does_interlock: bool) {
        assert!(self.shader_type == ShaderType::Fragment);
        self.set_field(SPHV3_T2_DOES_INTERLOCK, does_interlock);
    }

    #[inline]
    pub fn set_uses_underestimate(&mut self, uses_underestimate: bool) {
        assert!(self.shader_type == ShaderType::Fragment);
        self.set_field(SPHV3_T2_USES_UNDERESTIMATE, uses_underestimate);
    }

    #[inline]
    fn pervertex_imap_vector_ps(&mut self) -> SubSPHView<'_> {
        assert!(self.shader_type == ShaderType::Fragment);

        new_subset(&mut self.data, 672, 128)
    }

    #[inline]
    pub fn set_pervertex_imap_vector(&mut self, index: usize, value: u32) {
        assert!(index < 4);
        assert!(self.shader_type == ShaderType::Fragment);

        self.pervertex_imap_vector_ps()
            .set_field(index * 32..(index + 1) * 32, value);
    }
}

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

#[cfg(test)]
mod tests {
    use super::*;

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
        use crate::codegen::ir::{ComputeShaderInfo, ShaderInfo, ShaderModelInfo};
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
            ShaderInfo, ShaderIoInfo, ShaderModelInfo, ShaderStageInfo, SysValInfo,
            VertexShaderInfo, VtgIoInfo,
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
}
