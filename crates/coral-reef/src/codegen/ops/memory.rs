// SPDX-License-Identifier: AGPL-3.0-only
// Copyright © 2026 ecoPrimals
//! Memory operation encoding — Ld, St, Atom, Copy(CBuf), Ldc, MemBar.
//!
//! Implements `EncodeOp<AmdOpEncoder>` for all memory-class operations.
//!
//! | Op | AMD instruction | Encoding |
//! |-----------|------------------------------|----------|
//! | Ld | FLAT_LOAD_* | FLAT |
//! | St | FLAT_STORE_* | FLAT |
//! | Atom | FLAT_ATOMIC_* | FLAT |
//! | Copy(CBuf)| V_MOV_B32 (user SGPR) | VOP1 |
//! | Ldc | V_MOV_B32 (user SGPR) | VOP1 |
//! | MemBar | S_WAITCNT | SOPP |

use super::{AmdOpEncoder, EncodeOp, dst_to_vgpr_index, src_to_vgpr_index};
use crate::CompileError;
use crate::codegen::amd::encoding::{self, Rdna2Encoder};
use crate::codegen::amd::isa;
use crate::codegen::amd::reg::AmdRegRef;
#[allow(
    clippy::wildcard_imports,
    reason = "op module re-exports are intentional for codegen"
)]
use crate::codegen::ir::*;

// ---- Ld (FLAT load) ----

impl EncodeOp<AmdOpEncoder<'_>> for OpLd {
    fn encode(&self, _e: &mut AmdOpEncoder<'_>) -> Result<Vec<u32>, CompileError> {
        let dst_reg = dst_to_vgpr_index(&self.dst)?;
        let addr_reg = src_to_vgpr_index(&self.addr)?;
        let flat_opcode = mem_type_to_flat_load(self.access.mem_type)?;
        let offset = checked_flat_offset(self.offset)?;
        Ok(Rdna2Encoder::encode_flat_load(
            flat_opcode,
            addr_reg,
            dst_reg,
            offset,
        ))
    }
}

// ---- St (FLAT store) ----

impl EncodeOp<AmdOpEncoder<'_>> for OpSt {
    fn encode(&self, _e: &mut AmdOpEncoder<'_>) -> Result<Vec<u32>, CompileError> {
        let addr_reg = src_to_vgpr_index(&self.addr)?;
        let data_reg = src_to_vgpr_index(&self.data)?;
        let flat_opcode = mem_type_to_flat_store(self.access.mem_type)?;
        let offset = checked_flat_offset(self.offset)?;
        Ok(Rdna2Encoder::encode_flat_store(
            flat_opcode,
            addr_reg,
            data_reg,
            offset,
        ))
    }
}

// ---- Atom (FLAT atomic) ----

impl EncodeOp<AmdOpEncoder<'_>> for OpAtom {
    fn encode(&self, _e: &mut AmdOpEncoder<'_>) -> Result<Vec<u32>, CompileError> {
        let dst_reg = dst_to_vgpr_index(&self.dst)?;
        let addr_reg = src_to_vgpr_index(&self.addr)?;
        let data_reg = src_to_vgpr_index(&self.data)?;
        let flat_opcode = atom_op_to_flat(self.atom_op)?;
        let offset = checked_flat_offset(self.addr_offset)?;
        Ok(Rdna2Encoder::encode_flat_atomic(
            flat_opcode,
            addr_reg,
            data_reg,
            dst_reg,
            offset,
        ))
    }
}

fn checked_flat_offset(offset: i32) -> Result<i16, CompileError> {
    i16::try_from(offset).map_err(|_| {
        CompileError::InvalidInput(
            format!("FLAT offset {offset} exceeds i16 range (-32768..32767)").into(),
        )
    })
}

// ---- Copy with CBuf source (user SGPR materialization) ----
//
// When naga_translate generates `OpCopy { src: SrcRef::CBuf(...) }` for
// storage buffer address loads, the AMD backend materializes the constant
// buffer value from user SGPRs (populated by COMPUTE_USER_DATA in PM4).

impl EncodeOp<AmdOpEncoder<'_>> for OpCopy {
    fn encode(&self, _e: &mut AmdOpEncoder<'_>) -> Result<Vec<u32>, CompileError> {
        let dst_reg = dst_to_vgpr_index(&self.dst)?;
        let src_enc = super::src_to_encoding(&self.src)?;
        let mut words =
            Rdna2Encoder::encode_vop1(isa::vop1::V_MOV_B32, AmdRegRef::vgpr(dst_reg), src_enc.src0);
        src_enc.extend_with_literal(&mut words);
        Ok(words)
    }
}

// ---- Ldc (constant buffer load via user SGPR) ----
//
// On AMD, constant buffer data is pre-loaded into user SGPRs by the
// hardware from COMPUTE_USER_DATA registers. The Ldc op materializes
// a value from user SGPRs into a VGPR via V_MOV_B32.
//
// The SGPR index is derived from the constant buffer binding and byte
// offset (see `cbuf_to_user_sgpr_encoding` in mod.rs).

impl EncodeOp<AmdOpEncoder<'_>> for OpLdc {
    fn encode(&self, _e: &mut AmdOpEncoder<'_>) -> Result<Vec<u32>, CompileError> {
        let dst_reg = dst_to_vgpr_index(&self.dst)?;

        let SrcRef::CBuf(cb) = &self.cb.reference else {
            return Err(CompileError::InvalidInput(
                "Ldc source is not a constant buffer reference".into(),
            ));
        };

        let sgpr_enc = super::cbuf_to_user_sgpr_encoding(&cb.buf, cb.offset)?;

        Ok(Rdna2Encoder::encode_vop1(
            isa::vop1::V_MOV_B32,
            AmdRegRef::vgpr(dst_reg),
            sgpr_enc,
        ))
    }
}

// ---- MemBar (S_WAITCNT) ----
//
// AMD doesn't have a direct memory barrier instruction like NVIDIA's MEMBAR.
// Instead, we emit S_WAITCNT with appropriate counters zeroed to enforce
// ordering. The scope determines how aggressively we wait:
//   - CTA scope: wait on LGKM_CNT (LDS/GDS/SMEM) only
//   - GPU scope: wait on VM_CNT + LGKM_CNT (global memory + scalar)
//   - System scope: wait on all counters (VM + EXP + LGKM)

impl EncodeOp<AmdOpEncoder<'_>> for OpMemBar {
    fn encode(&self, _e: &mut AmdOpEncoder<'_>) -> Result<Vec<u32>, CompileError> {
        let (vm_cnt, exp_cnt, lgkm_cnt) = match self.scope {
            MemScope::CTA => (0x0F, 0x07, 0), // wait only on LGKM
            MemScope::GPU => (0, 0x07, 0),    // wait on VM + LGKM
            MemScope::System => (0, 0, 0),    // wait on everything
        };
        Ok(encoding::encode_s_waitcnt(vm_cnt, exp_cnt, lgkm_cnt))
    }
}

// ---- Lookup tables ----

fn mem_type_to_flat_load(mt: MemType) -> Result<u16, CompileError> {
    Ok(match mt {
        MemType::U8 => isa::flat::FLAT_LOAD_UBYTE,
        MemType::I8 => isa::flat::FLAT_LOAD_SBYTE,
        MemType::U16 => isa::flat::FLAT_LOAD_USHORT,
        MemType::I16 => isa::flat::FLAT_LOAD_SSHORT,
        MemType::B32 => isa::flat::FLAT_LOAD_DWORD,
        MemType::B64 => isa::flat::FLAT_LOAD_DWORDX2,
        MemType::B128 => isa::flat::FLAT_LOAD_DWORDX4,
    })
}

fn mem_type_to_flat_store(mt: MemType) -> Result<u16, CompileError> {
    Ok(match mt {
        MemType::U8 | MemType::I8 => isa::flat::FLAT_STORE_BYTE,
        MemType::U16 | MemType::I16 => isa::flat::FLAT_STORE_SHORT,
        MemType::B32 => isa::flat::FLAT_STORE_DWORD,
        MemType::B64 => isa::flat::FLAT_STORE_DWORDX2,
        MemType::B128 => isa::flat::FLAT_STORE_DWORDX4,
    })
}

fn atom_op_to_flat(op: AtomOp) -> Result<u16, CompileError> {
    Ok(match op {
        AtomOp::Add => isa::flat::FLAT_ATOMIC_ADD,
        AtomOp::Min => isa::flat::FLAT_ATOMIC_SMIN,
        AtomOp::Max => isa::flat::FLAT_ATOMIC_SMAX,
        AtomOp::Inc => isa::flat::FLAT_ATOMIC_INC,
        AtomOp::Dec => isa::flat::FLAT_ATOMIC_DEC,
        AtomOp::And => isa::flat::FLAT_ATOMIC_AND,
        AtomOp::Or => isa::flat::FLAT_ATOMIC_OR,
        AtomOp::Xor => isa::flat::FLAT_ATOMIC_XOR,
        AtomOp::Exch => isa::flat::FLAT_ATOMIC_SWAP,
        AtomOp::CmpExch(_) => isa::flat::FLAT_ATOMIC_CMPSWAP,
    })
}
