// SPDX-License-Identifier: AGPL-3.0-only
//! Stub for `nvidia_headers` — NVIDIA hardware class definitions.
//!
//! These define the memory-mapped registers and methods for NVIDIA GPU
//! classes (compute dispatch, DMA copy, shader program headers, etc.).

#![allow(non_upper_case_globals)]
// Hardware register names must match NVIDIA spec verbatim.
#![allow(non_snake_case, missing_docs)]

/// NVIDIA hardware class definitions.
pub mod classes {
    /// Kepler A — Shader Program Header.
    pub mod cla097 {
        /// SPH definitions.
        pub mod sph {
            // Stub — SPH field definitions will be extracted from nvidia_headers
        }
    }

    /// Kepler Compute A (class a0c0).
    pub mod cla0c0 {
        /// Method definitions.
        pub mod mthd {}

        /// QMD v0.6 definitions for Kepler.
        pub mod qmd {
            #![allow(non_upper_case_globals)]
            use std::ops::Range;

            pub const QMDV00_06_QMD_MAJOR_VERSION: Range<usize> = 0..4;
            pub const QMDV00_06_QMD_VERSION: Range<usize> = 4..8;
            pub const QMDV00_06_API_VISIBLE_CALL_LIMIT: Range<usize> = 8..9;
            pub const QMDV00_06_API_VISIBLE_CALL_LIMIT_NO_CHECK: u64 = 0;
            pub const QMDV00_06_SAMPLER_INDEX: Range<usize> = 9..12;
            pub const QMDV00_06_SAMPLER_INDEX_INDEPENDENTLY: u64 = 0;
            pub const QMDV00_06_SASS_VERSION: Range<usize> = 16..24;
            pub const QMDV00_06_MAX_BIT: usize = 2047;
            pub const QMDV00_06_CTA_RASTER_WIDTH: Range<usize> = 224..256;
            pub const QMDV00_06_CTA_RASTER_HEIGHT: Range<usize> = 256..272;
            pub const QMDV00_06_CTA_RASTER_DEPTH: Range<usize> = 272..288;
            pub const QMDV00_06_CTA_THREAD_DIMENSION0: Range<usize> = 544..560;
            pub const QMDV00_06_CTA_THREAD_DIMENSION1: Range<usize> = 560..576;
            pub const QMDV00_06_CTA_THREAD_DIMENSION2: Range<usize> = 576..592;
            pub const QMDV00_06_BARRIER_COUNT: Range<usize> = 592..597;
            pub const QMDV00_06_REGISTER_COUNT: Range<usize> = 608..616;
            pub const QMDV00_06_SHADER_LOCAL_MEMORY_CRS_SIZE: Range<usize> = 1024..1048;
            pub const QMDV00_06_PROGRAM_OFFSET: Range<usize> = 832..864;
            pub const QMDV00_06_SHARED_MEMORY_SIZE: Range<usize> = 640..658;
            pub const QMDV00_06_L1_CONFIGURATION: Range<usize> = 658..661;
            pub const QMDV00_06_L1_CONFIGURATION_DIRECTLY_ADDRESSABLE_MEMORY_SIZE_16KB: u64 = 1;
            pub const QMDV00_06_L1_CONFIGURATION_DIRECTLY_ADDRESSABLE_MEMORY_SIZE_32KB: u64 = 2;
            pub const QMDV00_06_L1_CONFIGURATION_DIRECTLY_ADDRESSABLE_MEMORY_SIZE_48KB: u64 = 3;
            pub const QMDV00_06_SHADER_LOCAL_MEMORY_LOW_SIZE: Range<usize> = 1472..1496;
            pub const QMDV00_06_SHADER_LOCAL_MEMORY_HIGH_SIZE: Range<usize> = 1496..1520;

            #[must_use]
            pub const fn QMDV00_06_CONSTANT_BUFFER_ADDR_LOWER(idx: usize) -> Range<usize> {
                let base = 1536 + idx * 64;
                base..(base + 32)
            }
            #[must_use]
            pub const fn QMDV00_06_CONSTANT_BUFFER_ADDR_UPPER(idx: usize) -> Range<usize> {
                let base = 1536 + idx * 64 + 32;
                base..(base + 8)
            }
            #[must_use]
            pub const fn QMDV00_06_CONSTANT_BUFFER_SIZE(idx: usize) -> Range<usize> {
                let base = 1536 + idx * 64 + 40;
                base..(base + 17)
            }
            #[must_use]
            pub const fn QMDV00_06_CONSTANT_BUFFER_VALID(idx: usize) -> Range<usize> {
                let base = 1536 + idx * 64 + 57;
                base..(base + 1)
            }
        }
    }

    /// Pascal Compute (class c0c0).
    pub mod clc0c0 {
        /// Method definitions.
        pub mod mthd {}

        /// QMD v2.1 definitions for Pascal.
        pub mod qmd {
            #![allow(non_upper_case_globals)]
            use std::ops::Range;

            pub const QMDV02_01_QMD_MAJOR_VERSION: Range<usize> = 0..4;
            pub const QMDV02_01_QMD_VERSION: Range<usize> = 4..8;
            pub const QMDV02_01_API_VISIBLE_CALL_LIMIT: Range<usize> = 8..9;
            pub const QMDV02_01_API_VISIBLE_CALL_LIMIT_NO_CHECK: u64 = 0;
            pub const QMDV02_01_SAMPLER_INDEX: Range<usize> = 9..12;
            pub const QMDV02_01_SAMPLER_INDEX_INDEPENDENTLY: u64 = 0;
            pub const QMDV02_01_SM_GLOBAL_CACHING_ENABLE: Range<usize> = 16..17;
            pub const QMDV02_01_MAX_BIT: usize = 2047;
            pub const QMDV02_01_CTA_RASTER_WIDTH: Range<usize> = 224..256;
            pub const QMDV02_01_CTA_RASTER_HEIGHT: Range<usize> = 256..272;
            pub const QMDV02_01_CTA_RASTER_DEPTH: Range<usize> = 272..288;
            pub const QMDV02_01_CTA_THREAD_DIMENSION0: Range<usize> = 544..560;
            pub const QMDV02_01_CTA_THREAD_DIMENSION1: Range<usize> = 560..576;
            pub const QMDV02_01_CTA_THREAD_DIMENSION2: Range<usize> = 576..592;
            pub const QMDV02_01_BARRIER_COUNT: Range<usize> = 592..597;
            pub const QMDV02_01_REGISTER_COUNT: Range<usize> = 608..616;
            pub const QMDV02_01_SHADER_LOCAL_MEMORY_CRS_SIZE: Range<usize> = 1024..1048;
            pub const QMDV02_01_PROGRAM_OFFSET: Range<usize> = 832..864;
            pub const QMDV02_01_SHARED_MEMORY_SIZE: Range<usize> = 640..658;
            pub const QMDV02_01_SHADER_LOCAL_MEMORY_LOW_SIZE: Range<usize> = 1472..1496;
            pub const QMDV02_01_SHADER_LOCAL_MEMORY_HIGH_SIZE: Range<usize> = 1496..1520;

            #[must_use]
            pub const fn QMDV02_01_CONSTANT_BUFFER_ADDR_LOWER(idx: usize) -> Range<usize> {
                let base = 1536 + idx * 64;
                base..(base + 32)
            }
            #[must_use]
            pub const fn QMDV02_01_CONSTANT_BUFFER_ADDR_UPPER(idx: usize) -> Range<usize> {
                let base = 1536 + idx * 64 + 32;
                base..(base + 8)
            }
            #[must_use]
            pub const fn QMDV02_01_CONSTANT_BUFFER_SIZE_SHIFTED4(idx: usize) -> Range<usize> {
                let base = 1536 + idx * 64 + 40;
                base..(base + 17)
            }
            #[must_use]
            pub const fn QMDV02_01_CONSTANT_BUFFER_VALID(idx: usize) -> Range<usize> {
                let base = 1536 + idx * 64 + 57;
                base..(base + 1)
            }
        }
    }

    /// Volta Compute A (class c3c0).
    pub mod clc3c0 {
        /// Class constant.
        pub const VOLTA_COMPUTE_A: u32 = 0xC3C0;
        /// Method definitions.
        pub mod mthd {}

        /// QMD v2.2 definitions for Volta.
        pub mod qmd {
            #![allow(non_upper_case_globals)]
            use std::ops::Range;

            pub const QMDV02_02_QMD_MAJOR_VERSION: Range<usize> = 0..4;
            pub const QMDV02_02_QMD_VERSION: Range<usize> = 4..8;
            pub const QMDV02_02_API_VISIBLE_CALL_LIMIT: Range<usize> = 8..9;
            pub const QMDV02_02_API_VISIBLE_CALL_LIMIT_NO_CHECK: u64 = 0;
            pub const QMDV02_02_SAMPLER_INDEX: Range<usize> = 9..12;
            pub const QMDV02_02_SAMPLER_INDEX_INDEPENDENTLY: u64 = 0;
            pub const QMDV02_02_SM_GLOBAL_CACHING_ENABLE: Range<usize> = 16..17;
            pub const QMDV02_02_MAX_BIT: usize = 2047;
            pub const QMDV02_02_CTA_RASTER_WIDTH: Range<usize> = 224..256;
            pub const QMDV02_02_CTA_RASTER_HEIGHT: Range<usize> = 256..272;
            pub const QMDV02_02_CTA_RASTER_DEPTH: Range<usize> = 272..288;
            pub const QMDV02_02_CTA_THREAD_DIMENSION0: Range<usize> = 544..560;
            pub const QMDV02_02_CTA_THREAD_DIMENSION1: Range<usize> = 560..576;
            pub const QMDV02_02_CTA_THREAD_DIMENSION2: Range<usize> = 576..592;
            pub const QMDV02_02_BARRIER_COUNT: Range<usize> = 592..597;
            pub const QMDV02_02_REGISTER_COUNT_V: Range<usize> = 608..616;
            pub const QMDV02_02_SHARED_MEMORY_SIZE: Range<usize> = 640..658;
            pub const QMDV02_02_MIN_SM_CONFIG_SHARED_MEM_SIZE: Range<usize> = 672..680;
            pub const QMDV02_02_MAX_SM_CONFIG_SHARED_MEM_SIZE: Range<usize> = 680..688;
            pub const QMDV02_02_TARGET_SM_CONFIG_SHARED_MEM_SIZE: Range<usize> = 688..696;
            pub const QMDV02_02_SHADER_LOCAL_MEMORY_CRS_SIZE: Range<usize> = 1024..1048;
            pub const QMDV02_02_PROGRAM_ADDRESS_LOWER: Range<usize> = 832..864;
            pub const QMDV02_02_PROGRAM_ADDRESS_UPPER: Range<usize> = 864..896;
            pub const QMDV02_02_SHADER_LOCAL_MEMORY_LOW_SIZE: Range<usize> = 1472..1496;
            pub const QMDV02_02_SHADER_LOCAL_MEMORY_HIGH_SIZE: Range<usize> = 1496..1520;

            #[must_use]
            pub const fn QMDV02_02_CONSTANT_BUFFER_ADDR_LOWER(idx: usize) -> Range<usize> {
                let base = 1536 + idx * 64;
                base..(base + 32)
            }
            #[must_use]
            pub const fn QMDV02_02_CONSTANT_BUFFER_ADDR_UPPER(idx: usize) -> Range<usize> {
                let base = 1536 + idx * 64 + 32;
                base..(base + 8)
            }
            #[must_use]
            pub const fn QMDV02_02_CONSTANT_BUFFER_SIZE_SHIFTED4(idx: usize) -> Range<usize> {
                let base = 1536 + idx * 64 + 40;
                base..(base + 17)
            }
            #[must_use]
            pub const fn QMDV02_02_CONSTANT_BUFFER_VALID(idx: usize) -> Range<usize> {
                let base = 1536 + idx * 64 + 57;
                base..(base + 1)
            }
        }
    }

    /// Ampere Compute A (class c6c0).
    pub mod clc6c0 {
        /// Class constant.
        pub const AMPERE_COMPUTE_A: u32 = 0xC6C0;
        /// Method definitions.
        pub mod mthd {}

        /// QMD v3.0 definitions for Ampere.
        pub mod qmd {
            #![allow(non_upper_case_globals)]
            use std::ops::Range;

            pub const QMDV03_00_QMD_MAJOR_VERSION: Range<usize> = 0..4;
            pub const QMDV03_00_QMD_VERSION: Range<usize> = 4..8;
            pub const QMDV03_00_API_VISIBLE_CALL_LIMIT: Range<usize> = 8..9;
            pub const QMDV03_00_API_VISIBLE_CALL_LIMIT_NO_CHECK: u64 = 0;
            pub const QMDV03_00_SAMPLER_INDEX: Range<usize> = 9..12;
            pub const QMDV03_00_SAMPLER_INDEX_INDEPENDENTLY: u64 = 0;
            pub const QMDV03_00_SM_GLOBAL_CACHING_ENABLE: Range<usize> = 16..17;
            pub const QMDV03_00_MAX_BIT: usize = 2047;
            pub const QMDV03_00_CTA_RASTER_WIDTH: Range<usize> = 224..256;
            pub const QMDV03_00_CTA_RASTER_HEIGHT: Range<usize> = 256..272;
            pub const QMDV03_00_CTA_RASTER_DEPTH: Range<usize> = 272..288;
            pub const QMDV03_00_CTA_THREAD_DIMENSION0: Range<usize> = 544..560;
            pub const QMDV03_00_CTA_THREAD_DIMENSION1: Range<usize> = 560..576;
            pub const QMDV03_00_CTA_THREAD_DIMENSION2: Range<usize> = 576..592;
            pub const QMDV03_00_BARRIER_COUNT: Range<usize> = 592..597;
            pub const QMDV03_00_REGISTER_COUNT_V: Range<usize> = 608..616;
            pub const QMDV03_00_SHARED_MEMORY_SIZE: Range<usize> = 640..658;
            pub const QMDV03_00_MIN_SM_CONFIG_SHARED_MEM_SIZE: Range<usize> = 672..680;
            pub const QMDV03_00_MAX_SM_CONFIG_SHARED_MEM_SIZE: Range<usize> = 680..688;
            pub const QMDV03_00_TARGET_SM_CONFIG_SHARED_MEM_SIZE: Range<usize> = 688..696;
            pub const QMDV03_00_PROGRAM_ADDRESS_LOWER: Range<usize> = 832..864;
            pub const QMDV03_00_PROGRAM_ADDRESS_UPPER: Range<usize> = 864..896;
            pub const QMDV03_00_SHADER_LOCAL_MEMORY_LOW_SIZE: Range<usize> = 1472..1496;
            pub const QMDV03_00_SHADER_LOCAL_MEMORY_HIGH_SIZE: Range<usize> = 1496..1520;

            #[must_use]
            pub const fn QMDV03_00_CONSTANT_BUFFER_ADDR_LOWER(idx: usize) -> Range<usize> {
                let base = 1536 + idx * 64;
                base..(base + 32)
            }
            #[must_use]
            pub const fn QMDV03_00_CONSTANT_BUFFER_ADDR_UPPER(idx: usize) -> Range<usize> {
                let base = 1536 + idx * 64 + 32;
                base..(base + 8)
            }
            #[must_use]
            pub const fn QMDV03_00_CONSTANT_BUFFER_SIZE_SHIFTED4(idx: usize) -> Range<usize> {
                let base = 1536 + idx * 64 + 40;
                base..(base + 17)
            }
            #[must_use]
            pub const fn QMDV03_00_CONSTANT_BUFFER_VALID(idx: usize) -> Range<usize> {
                let base = 1536 + idx * 64 + 57;
                base..(base + 1)
            }
        }
    }

    /// Hopper Compute A.
    pub mod clcbc0 {
        /// Class constant.
        pub const HOPPER_COMPUTE_A: u32 = 0xCBC0;
        /// QMD v4.0 definitions for Hopper.
        pub mod qmd {
            #![allow(non_upper_case_globals)]
            use std::ops::Range;

            pub const QMDV04_00_GRID_WIDTH: Range<usize> = 0..32;
            pub const QMDV04_00_GRID_HEIGHT: Range<usize> = 32..48;
            pub const QMDV04_00_GRID_DEPTH: Range<usize> = 48..64;
            pub const QMDV04_00_QMD_MINOR_VERSION: Range<usize> = 64..68;
            pub const QMDV04_00_QMD_MAJOR_VERSION: Range<usize> = 68..72;
            pub const QMDV04_00_QMD_VERSION: Range<usize> = 64..68;
            pub const QMDV04_00_API_VISIBLE_CALL_LIMIT: Range<usize> = 72..73;
            pub const QMDV04_00_API_VISIBLE_CALL_LIMIT_NO_CHECK: u64 = 0;
            pub const QMDV04_00_SAMPLER_INDEX: Range<usize> = 73..76;
            pub const QMDV04_00_SAMPLER_INDEX_INDEPENDENTLY: u64 = 0;
            pub const QMDV04_00_MAX_BIT: usize = 3071;
            pub const QMDV04_00_CTA_THREAD_DIMENSION0: Range<usize> = 544..560;
            pub const QMDV04_00_CTA_THREAD_DIMENSION1: Range<usize> = 560..576;
            pub const QMDV04_00_CTA_THREAD_DIMENSION2: Range<usize> = 576..592;
            pub const QMDV04_00_BARRIER_COUNT: Range<usize> = 592..597;
            pub const QMDV04_00_REGISTER_COUNT: Range<usize> = 608..616;
            pub const QMDV04_00_SHARED_MEMORY_SIZE: Range<usize> = 640..658;
            pub const QMDV04_00_MIN_SM_CONFIG_SHARED_MEM_SIZE: Range<usize> = 672..680;
            pub const QMDV04_00_MAX_SM_CONFIG_SHARED_MEM_SIZE: Range<usize> = 680..688;
            pub const QMDV04_00_TARGET_SM_CONFIG_SHARED_MEM_SIZE: Range<usize> = 688..696;
            pub const QMDV04_00_PROGRAM_ADDRESS_LOWER: Range<usize> = 832..864;
            pub const QMDV04_00_PROGRAM_ADDRESS_UPPER: Range<usize> = 864..896;
            pub const QMDV04_00_SHADER_LOCAL_MEMORY_LOW_SIZE: Range<usize> = 1472..1496;
            pub const QMDV04_00_SHADER_LOCAL_MEMORY_HIGH_SIZE: Range<usize> = 1496..1520;

            /// Indexed CBUF field accessors for Hopper QMD v4.0.
            #[must_use]
            pub const fn QMDV04_00_CONSTANT_BUFFER_ADDR_LOWER_SHIFTED6(idx: usize) -> Range<usize> {
                let base = 2048 + idx * 64;
                base..(base + 26)
            }
            #[must_use]
            pub const fn QMDV04_00_CONSTANT_BUFFER_ADDR_UPPER_SHIFTED6(idx: usize) -> Range<usize> {
                let base = 2048 + idx * 64 + 26;
                base..(base + 17)
            }
            #[must_use]
            pub const fn QMDV04_00_CONSTANT_BUFFER_SIZE_SHIFTED4(idx: usize) -> Range<usize> {
                let base = 2048 + idx * 64 + 43;
                base..(base + 17)
            }
            #[must_use]
            pub const fn QMDV04_00_CONSTANT_BUFFER_VALID(idx: usize) -> Range<usize> {
                let base = 2048 + idx * 64 + 60;
                base..(base + 1)
            }
        }
    }

    /// Blackwell Compute.
    pub mod clcdc0 {
        /// QMD v5.0 definitions for Blackwell.
        pub mod qmd {
            #![allow(non_upper_case_globals)]
            use std::ops::Range;

            pub const QMDV05_00_GRID_WIDTH: Range<usize> = 0..32;
            pub const QMDV05_00_GRID_HEIGHT: Range<usize> = 32..48;
            pub const QMDV05_00_GRID_DEPTH: Range<usize> = 48..64;
            pub const QMDV05_00_QMD_MINOR_VERSION: Range<usize> = 64..68;
            pub const QMDV05_00_QMD_MAJOR_VERSION: Range<usize> = 68..72;
            pub const QMDV05_00_QMD_VERSION: Range<usize> = 64..68;
            pub const QMDV05_00_QMD_TYPE: Range<usize> = 76..80;
            pub const QMDV05_00_QMD_GROUP_ID: Range<usize> = 80..86;
            pub const QMDV05_00_API_VISIBLE_CALL_LIMIT: Range<usize> = 72..73;
            pub const QMDV05_00_API_VISIBLE_CALL_LIMIT_NO_CHECK: u64 = 0;
            pub const QMDV05_00_SAMPLER_INDEX: Range<usize> = 73..76;
            pub const QMDV05_00_SAMPLER_INDEX_INDEPENDENTLY: u64 = 0;
            pub const QMDV05_00_MAX_BIT: usize = 3071;
            pub const QMDV05_00_CTA_THREAD_DIMENSION0: Range<usize> = 544..560;
            pub const QMDV05_00_CTA_THREAD_DIMENSION1: Range<usize> = 560..576;
            pub const QMDV05_00_CTA_THREAD_DIMENSION2: Range<usize> = 576..592;
            pub const QMDV05_00_BARRIER_COUNT: Range<usize> = 592..597;
            pub const QMDV05_00_REGISTER_COUNT: Range<usize> = 608..616;
            pub const QMDV05_00_SHARED_MEMORY_SIZE_SHIFTED7: Range<usize> = 640..651;
            pub const QMDV05_00_MIN_SM_CONFIG_SHARED_MEM_SIZE: Range<usize> = 672..680;
            pub const QMDV05_00_MAX_SM_CONFIG_SHARED_MEM_SIZE: Range<usize> = 680..688;
            pub const QMDV05_00_TARGET_SM_CONFIG_SHARED_MEM_SIZE: Range<usize> = 688..696;
            pub const QMDV05_00_PROGRAM_ADDRESS_LOWER_SHIFTED4: Range<usize> = 832..860;
            pub const QMDV05_00_PROGRAM_ADDRESS_UPPER_SHIFTED4: Range<usize> = 860..892;
            pub const QMDV05_00_SHADER_LOCAL_MEMORY_LOW_SIZE_SHIFTED4: Range<usize> = 1472..1492;
            pub const QMDV05_00_SHADER_LOCAL_MEMORY_HIGH_SIZE_SHIFTED4: Range<usize> = 1492..1516;

            /// Indexed CBUF field accessors for Blackwell QMD v5.0.
            #[must_use]
            pub const fn QMDV05_00_CONSTANT_BUFFER_ADDR_LOWER_SHIFTED6(idx: usize) -> Range<usize> {
                let base = 2048 + idx * 64;
                base..(base + 26)
            }
            #[must_use]
            pub const fn QMDV05_00_CONSTANT_BUFFER_ADDR_UPPER_SHIFTED6(idx: usize) -> Range<usize> {
                let base = 2048 + idx * 64 + 26;
                base..(base + 17)
            }
            #[must_use]
            pub const fn QMDV05_00_CONSTANT_BUFFER_SIZE_SHIFTED4(idx: usize) -> Range<usize> {
                let base = 2048 + idx * 64 + 43;
                base..(base + 17)
            }
            #[must_use]
            pub const fn QMDV05_00_CONSTANT_BUFFER_VALID(idx: usize) -> Range<usize> {
                let base = 2048 + idx * 64 + 60;
                base..(base + 1)
            }
        }
    }

    /// DMA Copy (Fermi).
    pub mod cl90b5 {
        /// Method definitions.
        pub mod mthd {}
    }

    /// Maxwell Compute B.
    pub mod clb1c0 {
        /// Class constant.
        pub const MAXWELL_COMPUTE_B: u32 = 0xB1C0;
        /// Method definitions.
        pub mod mthd {}
    }
}
