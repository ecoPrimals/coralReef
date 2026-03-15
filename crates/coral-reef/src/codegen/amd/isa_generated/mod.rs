// SPDX-License-Identifier: AGPL-3.0-only
// Copyright © 2026 ecoPrimals
//! AUTO-GENERATED from AMD RDNA2 ISA XML specification.
//!
//! Source: specs/amd/amdgpu_isa_rdna2.xml (MIT license, AMD GPUOpen)
//! Generator: tools/amd-isa-gen (pure Rust, sovereign toolchain)
//!
//! DO NOT EDIT BY HAND. Regenerate with:
//!   cargo run -p amd-isa-gen

#[allow(
    dead_code,
    missing_docs,
    reason = "generated ISA tables from amd-isa-gen"
)]
pub mod isa_types;

#[allow(
    dead_code,
    missing_docs,
    unused_imports,
    reason = "generated ISA tables from amd-isa-gen"
)]
pub mod ds;
#[allow(
    dead_code,
    missing_docs,
    unused_imports,
    reason = "generated ISA tables from amd-isa-gen"
)]
pub mod flat;
#[allow(
    dead_code,
    missing_docs,
    unused_imports,
    reason = "generated ISA tables from amd-isa-gen"
)]
pub mod flat_glbl;
#[allow(
    dead_code,
    missing_docs,
    unused_imports,
    reason = "generated ISA tables from amd-isa-gen"
)]
pub mod flat_scratch;
#[allow(
    dead_code,
    missing_docs,
    unused_imports,
    reason = "generated ISA tables from amd-isa-gen"
)]
pub mod mimg;
#[allow(
    dead_code,
    missing_docs,
    unused_imports,
    reason = "generated ISA tables from amd-isa-gen"
)]
pub mod mtbuf;
#[allow(
    dead_code,
    missing_docs,
    unused_imports,
    reason = "generated ISA tables from amd-isa-gen"
)]
pub mod mubuf;
#[allow(
    dead_code,
    missing_docs,
    unused_imports,
    reason = "generated ISA tables from amd-isa-gen"
)]
pub mod smem;
#[allow(
    dead_code,
    missing_docs,
    unused_imports,
    reason = "generated ISA tables from amd-isa-gen"
)]
pub mod sop1;
#[allow(
    dead_code,
    missing_docs,
    unused_imports,
    reason = "generated ISA tables from amd-isa-gen"
)]
pub mod sop2;
#[allow(
    dead_code,
    missing_docs,
    unused_imports,
    reason = "generated ISA tables from amd-isa-gen"
)]
pub mod sopc;
#[allow(
    dead_code,
    missing_docs,
    unused_imports,
    reason = "generated ISA tables from amd-isa-gen"
)]
pub mod sopk;
#[allow(
    dead_code,
    missing_docs,
    unused_imports,
    reason = "generated ISA tables from amd-isa-gen"
)]
pub mod sopp;
#[allow(
    dead_code,
    missing_docs,
    unused_imports,
    reason = "generated ISA tables from amd-isa-gen"
)]
pub mod vop1;
#[allow(
    dead_code,
    missing_docs,
    unused_imports,
    reason = "generated ISA tables from amd-isa-gen"
)]
pub mod vop2;
#[allow(
    dead_code,
    missing_docs,
    unused_imports,
    reason = "generated ISA tables from amd-isa-gen"
)]
pub mod vop3;
#[allow(
    dead_code,
    missing_docs,
    unused_imports,
    reason = "generated ISA tables from amd-isa-gen"
)]
pub mod vop3p;
#[allow(
    dead_code,
    missing_docs,
    unused_imports,
    reason = "generated ISA tables from amd-isa-gen"
)]
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

#[cfg(test)]
mod tests {
    use super::*;

    /// Exercise encoding_bits dispatch for all encoding names.
    #[test]
    fn test_encoding_bits_all_encodings() {
        let encodings = [
            ("ENC_DS", 64),
            ("ENC_FLAT", 64),
            ("ENC_FLAT_GLBL", 64),
            ("ENC_FLAT_SCRATCH", 64),
            ("ENC_MIMG", 64),
            ("ENC_MTBUF", 64),
            ("ENC_MUBUF", 64),
            ("ENC_SMEM", 64),
            ("ENC_SOP1", 32),
            ("ENC_SOP2", 32),
            ("ENC_SOPC", 32),
            ("ENC_SOPK", 32),
            ("ENC_SOPP", 32),
            ("ENC_VOP1", 32),
            ("ENC_VOP2", 32),
            ("ENC_VOP3", 64),
            ("ENC_VOP3P", 64),
            ("ENC_VOPC", 32),
        ];
        for (name, expected) in encodings {
            assert_eq!(encoding_bits(name), Some(expected), "encoding_bits({name})");
        }
        assert!(encoding_bits("INVALID").is_none());
    }

    /// Exercise ds/table.rs lookup.
    #[test]
    fn test_ds_lookup() {
        let e = ds::lookup(ds::DS_ADD_U32).expect("DS_ADD_U32");
        assert_eq!(e.name, "DS_ADD_U32");
        assert_eq!(e.opcode, 0);
    }

    /// Exercise flat.rs lookup.
    #[test]
    fn test_flat_lookup() {
        let e = flat::lookup(flat::FLAT_LOAD_UBYTE).expect("FLAT_LOAD_UBYTE");
        assert_eq!(e.name, "FLAT_LOAD_UBYTE");
        assert_eq!(e.opcode, 8);
    }

    /// Exercise flat_glbl.rs lookup.
    #[test]
    fn test_flat_glbl_lookup() {
        let e = flat_glbl::lookup(flat_glbl::GLOBAL_LOAD_UBYTE).expect("GLOBAL_LOAD_UBYTE");
        assert_eq!(e.name, "GLOBAL_LOAD_UBYTE");
        assert_eq!(e.opcode, 8);
    }

    /// Exercise flat_scratch.rs lookup.
    #[test]
    fn test_flat_scratch_lookup() {
        let e = flat_scratch::lookup(flat_scratch::SCRATCH_LOAD_UBYTE).expect("SCRATCH_LOAD_UBYTE");
        assert_eq!(e.name, "SCRATCH_LOAD_UBYTE");
        assert_eq!(e.opcode, 8);
    }

    /// Exercise mimg/table.rs lookup.
    #[test]
    fn test_mimg_lookup() {
        let e = mimg::lookup(mimg::IMAGE_LOAD).expect("IMAGE_LOAD");
        assert_eq!(e.name, "IMAGE_LOAD");
        assert_eq!(e.opcode, 0);
    }

    /// Exercise mtbuf.rs lookup.
    #[test]
    fn test_mtbuf_lookup() {
        let e = mtbuf::lookup(mtbuf::TBUFFER_LOAD_FORMAT_X).expect("TBUFFER_LOAD_FORMAT_X");
        assert_eq!(e.name, "TBUFFER_LOAD_FORMAT_X");
        assert_eq!(e.opcode, 0);
    }

    /// Exercise mubuf.rs lookup.
    #[test]
    fn test_mubuf_lookup() {
        let e = mubuf::lookup(mubuf::BUFFER_LOAD_FORMAT_X).expect("BUFFER_LOAD_FORMAT_X");
        assert_eq!(e.name, "BUFFER_LOAD_FORMAT_X");
        assert_eq!(e.opcode, 0);
    }

    /// Exercise smem.rs lookup.
    #[test]
    fn test_smem_lookup() {
        let e = smem::lookup(smem::S_LOAD_DWORD).expect("S_LOAD_DWORD");
        assert_eq!(e.name, "S_LOAD_DWORD");
        assert_eq!(e.opcode, 0);
    }

    /// Exercise sop1.rs lookup.
    #[test]
    fn test_sop1_lookup() {
        let e = sop1::lookup(sop1::S_MOV_B32).expect("S_MOV_B32");
        assert_eq!(e.name, "S_MOV_B32");
        assert_eq!(e.opcode, 3);
    }

    /// Exercise sop2.rs lookup.
    #[test]
    fn test_sop2_lookup() {
        let e = sop2::lookup(sop2::S_ADD_U32).expect("S_ADD_U32");
        assert_eq!(e.name, "S_ADD_U32");
        assert_eq!(e.opcode, 0);
    }

    /// Exercise sopc.rs lookup.
    #[test]
    fn test_sopc_lookup() {
        let e = sopc::lookup(sopc::S_CMP_EQ_I32).expect("S_CMP_EQ_I32");
        assert_eq!(e.name, "S_CMP_EQ_I32");
        assert_eq!(e.opcode, 0);
    }

    /// Exercise sopk.rs lookup.
    #[test]
    fn test_sopk_lookup() {
        let e = sopk::lookup(sopk::S_MOVK_I32).expect("S_MOVK_I32");
        assert_eq!(e.name, "S_MOVK_I32");
        assert_eq!(e.opcode, 0);
    }

    /// Exercise sopp.rs lookup.
    #[test]
    fn test_sopp_lookup() {
        let e = sopp::lookup(sopp::S_NOP).expect("S_NOP");
        assert_eq!(e.name, "S_NOP");
        assert_eq!(e.opcode, 0);
    }

    /// Exercise vop1.rs lookup.
    #[test]
    fn test_vop1_lookup() {
        let e = vop1::lookup(vop1::V_NOP).expect("V_NOP");
        assert_eq!(e.name, "V_NOP");
        assert_eq!(e.opcode, 0);
    }

    /// Exercise vop2.rs lookup.
    #[test]
    fn test_vop2_lookup() {
        let e = vop2::lookup(vop2::V_CNDMASK_B32).expect("V_CNDMASK_B32");
        assert_eq!(e.name, "V_CNDMASK_B32");
        assert_eq!(e.opcode, 1);
    }

    /// Exercise vop3 lookup — table_cmp_f (float comparisons).
    #[test]
    fn test_vop3_lookup_cmp_f() {
        let e = vop3::lookup(vop3::V_CMP_F_F32).expect("V_CMP_F_F32");
        assert_eq!(e.name, "V_CMP_F_F32");
        assert_eq!(e.opcode, 0);
    }

    /// Exercise vop3 lookup — table_cmp_i (integer comparisons).
    #[test]
    fn test_vop3_lookup_cmp_i() {
        let e = vop3::lookup(vop3::V_CMP_LT_I32).expect("V_CMP_LT_I32");
        assert_eq!(e.name, "V_CMP_LT_I32");
        assert_eq!(e.opcode, 129);
    }

    /// Exercise vop3 lookup — table_arith.
    #[test]
    fn test_vop3_lookup_arith() {
        let e = vop3::lookup(vop3::V_CNDMASK_B32).expect("V_CNDMASK_B32");
        assert_eq!(e.name, "V_CNDMASK_B32");
        assert_eq!(e.opcode, 257);
    }

    /// Exercise vop3 lookup — table_logic.
    #[test]
    fn test_vop3_lookup_logic() {
        let e = vop3::lookup(vop3::V_AND_B32).expect("V_AND_B32");
        assert_eq!(e.name, "V_AND_B32");
        assert_eq!(e.opcode, 283);
    }

    /// Exercise vop3 lookup — table_math.
    #[test]
    fn test_vop3_lookup_math() {
        let e = vop3::lookup(vop3::V_SQRT_F32).expect("V_SQRT_F32");
        assert_eq!(e.name, "V_SQRT_F32");
        assert_eq!(e.opcode, 435);
    }

    /// Exercise vop3p.rs lookup.
    #[test]
    fn test_vop3p_lookup() {
        let e = vop3p::lookup(vop3p::V_PK_MAD_I16).expect("V_PK_MAD_I16");
        assert_eq!(e.name, "V_PK_MAD_I16");
        assert_eq!(e.opcode, 0);
    }

    /// Exercise vopc lookup — table_a (F32/F64 comparisons).
    #[test]
    fn test_vopc_lookup_table_a() {
        let e = vopc::lookup(vopc::V_CMP_F_F32).expect("V_CMP_F_F32");
        assert_eq!(e.name, "V_CMP_F_F32");
        assert_eq!(e.opcode, 0);
    }

    /// Exercise vopc lookup — table_b (I/U/F16 comparisons).
    #[test]
    fn test_vopc_lookup_table_b() {
        let e = vopc::lookup(vopc::V_CMP_LT_I32).expect("V_CMP_LT_I32");
        assert_eq!(e.name, "V_CMP_LT_I32");
        assert_eq!(e.opcode, 129);
    }

    /// Sanity check TOTAL_INSTRUCTIONS.
    #[test]
    fn test_total_instructions_nonzero() {
        let total = TOTAL_INSTRUCTIONS;
        assert!(total > 0);
    }
}
