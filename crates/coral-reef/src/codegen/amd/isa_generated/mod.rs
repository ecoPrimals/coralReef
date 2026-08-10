// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright © 2026 ecoPrimals
//! AUTO-GENERATED from AMD RDNA2 ISA XML specification.
//!
//! Source: specs/amd/amdgpu_isa_rdna2.xml (MIT license, AMD GPUOpen)
//! Generator: tools/amd-isa-gen (pure Rust, sovereign toolchain)
//!
//! DO NOT EDIT BY HAND. Regenerate with:
//!   cargo run -p amd-isa-gen

#[expect(dead_code, reason = "generated ISA tables from amd-isa-gen")]
pub mod isa_types;

#[expect(
    dead_code,
    unused_imports,
    reason = "generated ISA tables from amd-isa-gen"
)]
pub mod ds;
#[expect(dead_code, reason = "generated ISA tables from amd-isa-gen")]
pub mod flat;
#[expect(dead_code, reason = "generated ISA tables from amd-isa-gen")]
pub mod flat_glbl;
#[expect(dead_code, reason = "generated ISA tables from amd-isa-gen")]
pub mod flat_scratch;
#[expect(
    dead_code,
    unused_imports,
    reason = "generated ISA tables from amd-isa-gen"
)]
pub mod mimg;
#[expect(dead_code, reason = "generated ISA tables from amd-isa-gen")]
pub mod mtbuf;
#[expect(dead_code, reason = "generated ISA tables from amd-isa-gen")]
pub mod mubuf;
#[expect(dead_code, reason = "generated ISA tables from amd-isa-gen")]
pub mod smem;
#[expect(dead_code, reason = "generated ISA tables from amd-isa-gen")]
pub mod sop1;
#[expect(dead_code, reason = "generated ISA tables from amd-isa-gen")]
pub mod sop2;
#[expect(dead_code, reason = "generated ISA tables from amd-isa-gen")]
pub mod sopc;
#[expect(dead_code, reason = "generated ISA tables from amd-isa-gen")]
pub mod sopk;
#[expect(dead_code, reason = "generated ISA tables from amd-isa-gen")]
pub mod sopp;
#[expect(dead_code, reason = "generated ISA tables from amd-isa-gen")]
pub mod vop1;
#[expect(dead_code, reason = "generated ISA tables from amd-isa-gen")]
pub mod vop2;
#[expect(dead_code, reason = "generated ISA tables from amd-isa-gen")]
pub mod vop3;
#[expect(dead_code, reason = "generated ISA tables from amd-isa-gen")]
pub mod vop3p;
#[expect(dead_code, reason = "generated ISA tables from amd-isa-gen")]
pub mod vopc;

/// Total instruction count across all compute-relevant encodings: 1446
pub const TOTAL_INSTRUCTIONS: usize = 1446;

/// Look up encoding width in bits by name.
#[must_use]
pub fn encoding_bits(name: &str) -> Option<u32> {
    match name {
        "ENC_DS" => Some(64),
        "ENC_FLAT" => Some(64),
        "ENC_FLAT_GLBL" => Some(64),
        "ENC_FLAT_SCRATCH" => Some(64),
        "ENC_MIMG" => Some(64),
        "ENC_MTBUF" => Some(64),
        "ENC_MUBUF" => Some(64),
        "ENC_SMEM" => Some(64),
        "ENC_SOP1" => Some(32),
        "ENC_SOP2" => Some(32),
        "ENC_SOPC" => Some(32),
        "ENC_SOPK" => Some(32),
        "ENC_SOPP" => Some(32),
        "ENC_VOP1" => Some(32),
        "ENC_VOP2" => Some(32),
        "ENC_VOP3" => Some(64),
        "ENC_VOP3P" => Some(64),
        "ENC_VOPC" => Some(32),
        _ => None,
    }
}
