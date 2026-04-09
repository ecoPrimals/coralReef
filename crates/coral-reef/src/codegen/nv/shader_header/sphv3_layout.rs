// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright © 2025-2026 ecoPrimals
// Derived from Collabora, Ltd. (2023)

//! SPH v3/v4 layout: word sizes and bit-field ranges for Type 1 and Type 2 chunks.

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
