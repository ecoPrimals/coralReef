// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright © 2025-2026 ecoPrimals
// Derived from Collabora, Ltd. (2023)

//! Mutable SPH word buffer and field setters (VTG vs fragment layouts).

use super::sphv3_layout::*;
use super::types::{OutputTopology, PixelImap, ShaderType};

use std::ops::Range;

use bitview::*;

type SubSPHView<'a> = BitMutSubsetView<'a>;

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

    /// Used by unit tests in the parent module.
    #[inline]
    pub(super) fn imap_system_values_ab(&mut self) -> SubSPHView<'_> {
        new_subset(&mut self.data, 160, 32)
    }

    #[inline]
    fn imap_g_vtg(&mut self) -> SubSPHView<'_> {
        assert!(self.shader_type != ShaderType::Fragment);

        new_subset(&mut self.data, 192, 128)
    }

    /// Used by unit tests in the parent module.
    #[inline]
    pub(super) fn imap_g_ps(&mut self) -> SubSPHView<'_> {
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

    /// Used by unit tests in the parent module.
    #[inline]
    pub(super) fn omap_target(&mut self) -> SubSPHView<'_> {
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
