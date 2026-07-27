// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright © 2026 ecoPrimals

use super::super::reg::AmdRegRef;
use super::*;

#[test]
fn s_endpgm_encoding() {
    let words = encode_s_endpgm();
    assert_eq!(words.len(), 1);
    // s_endpgm = SOPP encoding prefix (0xBF800000) | opcode 1 << 16
    assert_eq!(words[0], 0xBF81_0000);
}

#[test]
fn s_barrier_encoding() {
    let words = encode_s_barrier();
    assert_eq!(words.len(), 1);
    // s_barrier = SOPP prefix | opcode 10 << 16
    assert_eq!(words[0], 0xBF8A_0000);
}

#[test]
fn s_nop_encoding() {
    let words = encode_s_nop(0);
    assert_eq!(words.len(), 1);
    // s_nop 0 = SOPP prefix | opcode 0 << 16 | 0
    assert_eq!(words[0], 0xBF80_0000);
}

#[test]
fn s_waitcnt_encoding() {
    let words = encode_s_waitcnt(0, 0, 0);
    assert_eq!(words.len(), 1);
    // s_waitcnt 0 = SOPP prefix | opcode 12 << 16
    let expected_prefix = 0xBF8C_0000u32;
    assert_eq!(words[0], expected_prefix);
}

#[test]
fn vop3_f64_fma_is_64bit() {
    let dst = AmdRegRef::vgpr_pair(0);
    let words = encode_v_fma_f64(dst, 256, 258, 260);
    assert_eq!(words.len(), 2, "VOP3 should be 2 words");
}

#[test]
fn vop3_encoding_opcode_field() {
    let dst = AmdRegRef::vgpr_pair(4);
    let words = Rdna2Encoder::encode_vop3(
        isa::vop3::V_ADD_F64,
        dst,
        256, // v0
        258, // v2
        0,
    );
    let prefix = (words[0] >> 26) & 0x3F;
    assert_eq!(prefix, 0b11_0101);
    let opcode = (words[0] >> 16) & 0x3FF;
    assert_eq!(opcode, u32::from(isa::vop3::V_ADD_F64));
    let vdst = words[0] & 0xFF;
    assert_eq!(vdst, 4);
}

#[test]
fn vop2_encoding_structure() {
    let dst = AmdRegRef::vgpr(0);
    let vsrc1 = AmdRegRef::vgpr(1);
    let words = Rdna2Encoder::encode_vop2(isa::vop2::V_ADD_F32, dst, 256, vsrc1);
    assert_eq!(words.len(), 1, "VOP2 should be 1 word");
    let opcode = (words[0] >> 25) & 0x3F;
    assert_eq!(opcode, u32::from(isa::vop2::V_ADD_F32));
}

#[test]
fn sop2_encoding_structure() {
    let dst = AmdRegRef::sgpr(0);
    let src0 = AmdRegRef::sgpr(1);
    let src1 = AmdRegRef::sgpr(2);
    let words = Rdna2Encoder::encode_sop2(isa::sop2::S_ADD_U32, dst, src0, src1);
    assert_eq!(words.len(), 1, "SOP2 should be 1 word");
    let prefix = (words[0] >> 30) & 0x3;
    assert_eq!(prefix, 0b10);
}

#[test]
fn vop1_encoding_structure() {
    let dst = AmdRegRef::vgpr(5);
    let words = Rdna2Encoder::encode_vop1(isa::vop1::V_MOV_B32, dst, 256);
    assert_eq!(words.len(), 1, "VOP1 should be 1 word");
    let prefix = (words[0] >> 25) & 0x7F;
    assert_eq!(prefix, 0b011_1111);
}

#[test]
fn vop3_negate_abs_modifiers() {
    let dst = AmdRegRef::vgpr_pair(0);
    let words = Rdna2Encoder::encode_vop3_mod(
        isa::vop3::V_FMA_F64,
        dst,
        256,
        258,
        260,
        [true, false, false],
        [false, true, false],
    );
    assert_eq!(words.len(), 2);
    let abs_bits = (words[0] >> 8) & 0x7;
    assert_eq!(abs_bits, 0b010);
    let neg_bits = (words[1] >> 29) & 0x7;
    assert_eq!(neg_bits, 0b001);
}

#[test]
fn literal_constant_appended() {
    let mut e = Rdna2Encoder::new_32();
    e.set_literal(0xDEAD_BEEF);
    assert_eq!(e.words().len(), 2);
    assert_eq!(e.words()[1], 0xDEAD_BEEF);
}

#[test]
fn minimal_compute_kernel() {
    let mut code = Vec::new();
    // v_mov_b32 v0, 42 (literal)
    let mut mov = Rdna2Encoder::encode_vop1(isa::vop1::V_MOV_B32, AmdRegRef::vgpr(0), 255);
    mov.push(42); // literal constant
    code.extend_from_slice(&mov);
    // s_endpgm
    code.extend_from_slice(&encode_s_endpgm());
    // 3 words: VOP1 + literal + SOPP
    assert_eq!(code.len(), 3);
}

// ---- LLVM cross-validation tests ----
// These expected values are produced by:
//   echo "<asm>" | llvm-mc --triple=amdgcn--amdpal --mcpu=gfx1030 --show-encoding
// and converted from little-endian byte arrays to u32 words.

#[test]
fn llvm_validated_s_endpgm() {
    // LLVM: [0x00,0x00,0x81,0xbf] = 0xBF810000
    assert_eq!(encode_s_endpgm(), vec![0xBF81_0000]);
}

#[test]
fn llvm_validated_s_barrier() {
    // LLVM: [0x00,0x00,0x8a,0xbf] = 0xBF8A0000
    assert_eq!(encode_s_barrier(), vec![0xBF8A_0000]);
}

#[test]
fn llvm_validated_s_nop_0() {
    // LLVM: [0x00,0x00,0x80,0xbf] = 0xBF800000
    assert_eq!(encode_s_nop(0), vec![0xBF80_0000]);
}

#[test]
fn llvm_validated_s_waitcnt_0() {
    // LLVM: [0x00,0x00,0x8c,0xbf] = 0xBF8C0000
    assert_eq!(encode_s_waitcnt(0, 0, 0), vec![0xBF8C_0000]);
}

#[test]
fn llvm_validated_v_add_f64() {
    // v_add_f64 v[0:1], v[2:3], v[4:5]
    // LLVM: [0x00,0x00,0x64,0xd5, 0x02,0x09,0x02,0x00]
    let words = encode_v_add_f64(AmdRegRef::vgpr_pair(0), 258, 260);
    assert_eq!(words, vec![0xD564_0000, 0x0002_0902]);
}

#[test]
fn llvm_validated_v_fma_f64() {
    // v_fma_f64 v[0:1], v[2:3], v[4:5], v[6:7]
    // LLVM: [0x00,0x00,0x4c,0xd5, 0x02,0x09,0x1a,0x04]
    let words = encode_v_fma_f64(AmdRegRef::vgpr_pair(0), 258, 260, 262);
    assert_eq!(words, vec![0xD54C_0000, 0x041A_0902]);
}

#[test]
fn llvm_validated_v_add_f32() {
    // v_add_f32 v0, v1, v2  (VOP2 with v1 as SRC0, v2 as VSRC1)
    // LLVM: [0x01,0x05,0x00,0x06] = 0x06000501
    // src0 = 256+1 = 257 (v1), vsrc1 = v2 (index 2)
    let words = Rdna2Encoder::encode_vop2(
        isa::vop2::V_ADD_F32,
        AmdRegRef::vgpr(0),
        257, // v1 encoded as 256+1
        AmdRegRef::vgpr(2),
    );
    assert_eq!(words, vec![0x0600_0501]);
}

#[test]
fn llvm_validated_s_add_u32() {
    // s_add_u32 s0, s1, s2
    // LLVM: [0x01,0x02,0x00,0x80] = 0x80000201
    let words = Rdna2Encoder::encode_sop2(
        isa::sop2::S_ADD_U32,
        AmdRegRef::sgpr(0),
        AmdRegRef::sgpr(1),
        AmdRegRef::sgpr(2),
    );
    assert_eq!(words, vec![0x8000_0201]);
}

#[test]
fn llvm_validated_v_mov_b32() {
    // v_mov_b32 v5, v0
    // LLVM: [0x00,0x03,0x0a,0x7e] = 0x7E0A0300
    // src0 = 256+0 = 256 (v0)
    let words = Rdna2Encoder::encode_vop1(
        isa::vop1::V_MOV_B32,
        AmdRegRef::vgpr(5),
        256, // v0
    );
    assert_eq!(words, vec![0x7E0A_0300]);
}

#[test]
fn generated_opcode_table_coverage() {
    use super::super::isa_generated;
    // Verify key opcodes from the generated tables match LLVM-validated values.
    assert_eq!(isa_generated::sopp::S_ENDPGM, 1);
    assert_eq!(isa_generated::sopp::S_BARRIER, 10);
    assert_eq!(isa_generated::sopp::S_WAITCNT, 12);
    assert_eq!(isa_generated::vop3::V_ADD_F64, 356);
    assert_eq!(isa_generated::vop3::V_FMA_F64, 332);
    assert_eq!(isa_generated::vop3::V_MUL_F64, 357);
    assert_eq!(isa_generated::vop1::V_MOV_B32, 1);
    assert_eq!(isa_generated::vop2::V_ADD_F32, 3);
    assert_eq!(isa_generated::sop2::S_ADD_U32, 0);
    assert_eq!(isa_generated::sop1::S_MOV_B32, 3);
}

#[test]
fn generated_table_lookup() {
    use super::super::isa_generated;
    let entry = isa_generated::sopp::lookup(1).expect("S_ENDPGM should exist");
    assert_eq!(entry.name, "S_ENDPGM");
    assert!(entry.is_terminator);
    assert!(!entry.is_branch);

    let branch = isa_generated::sopp::lookup(2).expect("S_BRANCH should exist");
    assert_eq!(branch.name, "S_BRANCH");
    assert!(branch.is_branch);
}

/// Exercise every generated `TABLE` + `lookup` (llvm-cov: isa_generated tables
/// are otherwise unused dead data).
#[test]
fn generated_isa_tables_lookup_all_encodings() {
    use super::super::isa;
    use super::super::isa_generated;

    assert_eq!(isa_generated::TOTAL_INSTRUCTIONS, 1446);
    assert_eq!(isa_generated::encoding_bits("ENC_DS"), Some(64));
    assert_eq!(isa_generated::encoding_bits("ENC_VOP3"), Some(64));
    assert_eq!(isa_generated::encoding_bits("ENC_VOP3P"), Some(64));
    assert!(isa_generated::encoding_bits("ENC_UNKNOWN").is_none());

    assert!(isa_generated::ds::lookup(0).is_some());
    assert!(isa_generated::flat::lookup(20).is_some());
    assert!(isa_generated::flat_glbl::lookup(8).is_some());
    assert!(isa_generated::flat_scratch::lookup(8).is_some());
    assert!(isa_generated::mimg::lookup(0).is_some());
    assert!(isa_generated::mtbuf::lookup(0).is_some());
    assert!(isa_generated::mubuf::lookup(0).is_some());
    assert!(isa_generated::smem::lookup(0).is_some());
    assert!(isa_generated::sop1::lookup(3).is_some());
    assert!(isa_generated::sop2::lookup(0).is_some());
    assert!(isa_generated::sopc::lookup(0).is_some());
    assert!(isa_generated::sopk::lookup(0).is_some());
    assert!(isa_generated::vop1::lookup(1).is_some());
    assert!(isa_generated::vop2::lookup(3).is_some());
    assert!(isa_generated::vop3::lookup(356).is_some());
    assert!(isa_generated::vop3p::lookup(0).is_some());
    assert!(isa_generated::vopc::lookup(0).is_some());

    let _vop3_full = isa_generated::vop3::table();
    assert!(!_vop3_full.is_empty());
    let _vopc_full = isa_generated::vopc::table();
    assert!(!_vopc_full.is_empty());

    assert_eq!(
        isa_generated::flat::lookup(20)
            .expect("FLAT_LOAD_DWORD")
            .name,
        "FLAT_LOAD_DWORD"
    );
    assert_eq!(
        isa_generated::ds::lookup(0).expect("DS_ADD_U32").name,
        "DS_ADD_U32"
    );
    assert_eq!(isa::flat::FLAT_LOAD_DWORD, 20);
}

#[test]
fn flat_load_encoding_structure() {
    let words = Rdna2Encoder::encode_flat_load(isa::flat_glbl::GLOBAL_LOAD_DWORD, 0, 5, 0);
    assert_eq!(words.len(), 2, "FLAT is 64-bit");
    let prefix = (words[0] >> 26) & 0x3F;
    assert_eq!(prefix, 0b11_0111, "FLAT encoding prefix");
    let opcode = (words[0] >> 18) & 0x7F;
    assert_eq!(opcode, u32::from(isa::flat_glbl::GLOBAL_LOAD_DWORD));
    let seg = (words[0] >> 14) & 3;
    assert_eq!(seg, 2, "SEG must be GLOBAL (10)");
}

#[test]
fn flat_store_encoding_structure() {
    let words = Rdna2Encoder::encode_flat_store(isa::flat_glbl::GLOBAL_STORE_DWORD, 0, 1, 0);
    assert_eq!(words.len(), 2);
    let opcode = (words[0] >> 18) & 0x7F;
    assert_eq!(opcode, u32::from(isa::flat_glbl::GLOBAL_STORE_DWORD));
    let seg = (words[0] >> 14) & 3;
    assert_eq!(seg, 2, "SEG must be GLOBAL (10)");
}

#[test]
fn flat_atomic_encoding_has_glc() {
    let words = Rdna2Encoder::encode_flat_atomic(isa::flat_glbl::GLOBAL_ATOMIC_ADD, 0, 1, 2, 0);
    assert_eq!(words.len(), 2);
    let glc = (words[0] >> 16) & 1;
    assert_eq!(glc, 1, "GLC must be set for atomic return");
    let seg = (words[0] >> 14) & 3;
    assert_eq!(seg, 2, "SEG must be GLOBAL (10)");
}

#[test]
fn vopc_encoding_structure() {
    let words = Rdna2Encoder::encode_vopc(isa::vopc::V_CMP_EQ_F32, 256, 1);
    assert_eq!(words.len(), 1, "VOPC is 32-bit");
    let prefix = (words[0] >> 25) & 0x7F;
    assert_eq!(prefix, 0b011_1110, "VOPC encoding prefix");
}

#[test]
fn s_branch_encoding() {
    let words = Rdna2Encoder::encode_s_branch(4);
    assert_eq!(words.len(), 1);
    let opcode = (words[0] >> 16) & 0x7F;
    assert_eq!(opcode, u32::from(isa::sopp::S_BRANCH));
    let simm16 = words[0] & 0xFFFF;
    assert_eq!(simm16, 4);
}

#[test]
fn s_cbranch_scc1_encoding() {
    let words = Rdna2Encoder::encode_s_cbranch_scc1(0);
    let opcode = (words[0] >> 16) & 0x7F;
    assert_eq!(opcode, u32::from(isa::sopp::S_CBRANCH_SCC1));
}

#[test]
fn s_cbranch_vccnz_encoding() {
    let words = Rdna2Encoder::encode_s_cbranch_vccnz(-2i16);
    let opcode = (words[0] >> 16) & 0x7F;
    assert_eq!(opcode, u32::from(isa::sopp::S_CBRANCH_VCCNZ));
    let simm16 = words[0] & 0xFFFF;
    assert_eq!(simm16, (-2i16 as u16) as u32);
}
