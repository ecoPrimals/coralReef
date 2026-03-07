// SPDX-License-Identifier: AGPL-3.0-only
// Copyright © 2026 ecoPrimals
//! AMD shader model — implements `ShaderModel` for RDNA2 (GFX1030).
//!
//! Maps the vendor-agnostic `ShaderModel` trait onto AMD's architecture:
//!
//! | ShaderModel concept | AMD equivalent |
//! |---------------------|---------------|
//! | sm() | GFX version (e.g. 103 for GFX1030) |
//! | GPR | VGPR |
//! | UGPR | SGPR |
//! | Pred | Exec mask bits / SCC |
//! | warp (32 threads) | wave (32 or 64 lanes) |

#[allow(clippy::wildcard_imports)]
use super::super::ir::*;
use super::super::legalize::{LegalizeBuildHelpers, LegalizeBuilder};
use super::encoding::Rdna2Encoder;
use super::isa;
use crate::CompileError;

use coral_reef_stubs::fxhash::FxHashMap;

/// AMD RDNA2 shader model — direct `ShaderModel` impl (no intermediary vtable).
///
/// Version encoding: GFX major*10 + minor, e.g. GFX1030 → 103.
/// RDNA2 supports wave32 (default for compute) and wave64.
pub struct ShaderModelRdna2 {
    gfx_version: u8,
    wave_size: u8,
}

impl ShaderModelRdna2 {
    #[must_use]
    pub fn new(gfx_version: u8) -> Self {
        Self {
            gfx_version,
            wave_size: 32,
        }
    }

    #[must_use]
    pub fn with_wave_size(mut self, wave_size: u8) -> Self {
        self.wave_size = wave_size;
        self
    }
}

impl ShaderModel for ShaderModelRdna2 {
    fn sm(&self) -> u8 {
        self.gfx_version
    }

    fn is_amd(&self) -> bool {
        true
    }

    fn reg_count(&self, file: RegFile) -> u32 {
        match file {
            RegFile::GPR => 256,
            RegFile::UGPR => 106,
            RegFile::Pred => 1,
            RegFile::UPred => 1,
            RegFile::Carry => 1,
            RegFile::Bar => 16,
            RegFile::Mem => 0,
        }
    }

    fn hw_reserved_gpr_count(&self) -> u32 {
        0
    }

    fn crs_size(&self, _max_crs_depth: u32) -> u32 {
        0
    }

    fn op_can_be_uniform(&self, _op: &Op) -> bool {
        false
    }

    fn exec_latency(&self, _op: &Op) -> u32 {
        1
    }

    fn raw_latency(&self, _write: &Op, _dst_idx: usize, _read: &Op, _src_idx: usize) -> u32 {
        5
    }

    fn war_latency(&self, _read: &Op, _src_idx: usize, _write: &Op, _dst_idx: usize) -> u32 {
        1
    }

    fn waw_latency(
        &self,
        _a: &Op,
        _a_dst_idx: usize,
        _a_has_pred: bool,
        _b: &Op,
        _b_dst_idx: usize,
    ) -> u32 {
        5
    }

    fn paw_latency(&self, _write: &Op, _dst_idx: usize) -> u32 {
        5
    }

    fn worst_latency(&self, _write: &Op, _dst_idx: usize) -> u32 {
        200
    }

    fn latency_upper_bound(&self) -> u32 {
        200
    }

    fn max_instr_delay(&self) -> u8 {
        0
    }

    fn legalize_op(&self, b: &mut LegalizeBuilder, op: &mut Op) -> Result<(), CompileError> {
        legalize_rdna2_op(b, op)
    }

    fn encode_shader(&self, s: &Shader<'_>) -> Result<Vec<u32>, CompileError> {
        encode_rdna2_shader(self, s)
    }

    fn max_warps(&self) -> u32 {
        // RDNA2 CU: up to 32 waves/SIMD × 2 SIMDs = 64 waves/CU (wave32)
        // Conservative: 32 waves per SIMD
        32
    }

    fn wave_size(&self) -> u32 {
        u32::from(self.wave_size)
    }

    fn total_reg_file(&self) -> u32 {
        // RDNA2: 1024 VGPRs per SIMD in wave32 mode (each 32-bit)
        // 2 SIMDs per CU → 2048 VGPRs total per CU
        1024 * 2
    }
}

/// AMD-specific legalization for RDNA2.
///
/// Adapts IR ops to RDNA2 constraints:
/// - At most one SGPR or constant source per VALU instruction (VOP2/VOP1)
/// - VOP3 allows up to 3 sources from any register file
/// - f64 ops always use VOP3 encoding
fn legalize_rdna2_op(b: &mut LegalizeBuilder, op: &mut Op) -> Result<(), CompileError> {
    let gpr = RegFile::GPR;
    match op {
        Op::FAdd(op) => {
            let [src0, src1] = &mut op.srcs;
            super::super::legalize::swap_srcs_if_not_reg(src0, src1, gpr);
            b.copy_alu_src_if_not_reg(src0, gpr, SrcType::F32);
        }
        Op::FMul(op) => {
            let [src0, src1] = &mut op.srcs;
            super::super::legalize::swap_srcs_if_not_reg(src0, src1, gpr);
            b.copy_alu_src_if_not_reg(src0, gpr, SrcType::F32);
        }
        Op::FFma(op) => {
            b.copy_alu_src_if_not_reg(&mut op.srcs[0], gpr, SrcType::F32);
        }
        Op::DAdd(op) => {
            b.copy_alu_src_if_not_reg(&mut op.srcs[0], gpr, SrcType::F64);
        }
        Op::DFma(op) => {
            b.copy_alu_src_if_not_reg(&mut op.srcs[0], gpr, SrcType::F64);
        }
        Op::DMul(op) => {
            b.copy_alu_src_if_not_reg(&mut op.srcs[0], gpr, SrcType::F64);
        }
        Op::IAdd3(op) => {
            b.copy_alu_src_if_not_reg(&mut op.srcs[0], gpr, SrcType::ALU);
        }
        Op::IMad(op) => {
            b.copy_alu_src_if_not_reg(&mut op.srcs[0], gpr, SrcType::ALU);
        }
        Op::Mov(_) | Op::Copy(_) | Op::Swap(_) | Op::ParCopy(_) => {}
        Op::Ld(_) | Op::St(_) | Op::Atom(_) => {}
        Op::Bra(_) | Op::Exit(_) | Op::Nop(_) => {}
        Op::Sel(op) => {
            b.copy_alu_src_if_not_reg(&mut op.srcs[0], gpr, SrcType::ALU);
        }
        Op::FSetP(op) => {
            let [src0, src1] = &mut op.srcs;
            super::super::legalize::swap_srcs_if_not_reg(src0, src1, gpr);
            b.copy_alu_src_if_not_reg(src0, gpr, SrcType::F32);
        }
        Op::DSetP(op) => {
            b.copy_alu_src_if_not_reg(&mut op.srcs[0], gpr, SrcType::F64);
        }
        Op::ISetP(op) => {
            let [src0, src1] = &mut op.srcs;
            super::super::legalize::swap_srcs_if_not_reg(src0, src1, gpr);
            b.copy_alu_src_if_not_reg(src0, gpr, SrcType::ALU);
        }
        Op::F2F(op) => {
            b.copy_alu_src_if_not_reg(&mut op.src, gpr, SrcType::F32);
        }
        Op::F2I(op) => {
            b.copy_alu_src_if_not_reg(&mut op.src, gpr, SrcType::F32);
        }
        Op::I2F(op) => {
            b.copy_alu_src_if_not_reg(&mut op.src, gpr, SrcType::ALU);
        }
        Op::I2I(op) => {
            b.copy_alu_src_if_not_reg(&mut op.src, gpr, SrcType::ALU);
        }
        Op::Lop2(op) => {
            b.copy_alu_src_if_not_reg(&mut op.srcs[0], gpr, SrcType::ALU);
        }
        Op::Lop3(op) => {
            b.copy_alu_src_if_not_reg(&mut op.srcs[0], gpr, SrcType::ALU);
        }
        Op::Shl(op) => {
            b.copy_alu_src_if_not_reg(&mut op.src, gpr, SrcType::ALU);
        }
        Op::Shr(op) => {
            b.copy_alu_src_if_not_reg(&mut op.src, gpr, SrcType::ALU);
        }
        Op::Bar(_) | Op::S2R(_) | Op::CS2R(_) => {}
        Op::Undef(_)
        | Op::PhiSrcs(_)
        | Op::PhiDsts(_)
        | Op::Pin(_)
        | Op::Unpin(_)
        | Op::RegOut(_)
        | Op::SrcBar(_)
        | Op::Annotate(_) => {}
        _ => {}
    }
    Ok(())
}

/// Encode an AMD RDNA2 shader to instruction words.
///
/// Compute shaders have no header (unlike NVIDIA's SPH).
fn encode_rdna2_shader(_sm: &ShaderModelRdna2, s: &Shader<'_>) -> Result<Vec<u32>, CompileError> {
    if s.functions.is_empty() {
        return Err(CompileError::InvalidInput("empty shader".into()));
    }
    let func = &s.functions[0];

    let mut ip = 0_usize;
    let mut labels: FxHashMap<Label, usize> = FxHashMap::default();
    for b in &func.blocks {
        labels.insert(b.label, ip);
        for instr in &b.instrs {
            if let Op::Nop(nop) = &instr.op {
                if let Some(label) = nop.label {
                    labels.insert(label, ip);
                }
            }
            ip += estimate_instr_size(&instr.op);
        }
    }

    let mut encoded = Vec::new();
    for b in &func.blocks {
        for instr in &b.instrs {
            let words = encode_rdna2_op(&instr.op, &instr.pred, &labels, encoded.len())?;
            encoded.extend_from_slice(&words);
        }
    }

    if encoded.is_empty() || !ends_with_endpgm(&encoded) {
        encoded.extend_from_slice(&super::encoding::encode_s_endpgm());
    }

    Ok(encoded)
}

fn ends_with_endpgm(words: &[u32]) -> bool {
    words.last() == Some(&0xBF81_0000)
}

fn estimate_instr_size(op: &Op) -> usize {
    match op {
        Op::DAdd(_)
        | Op::DFma(_)
        | Op::DMul(_)
        | Op::DMnMx(_)
        | Op::DSetP(_)
        | Op::F64Sqrt(_)
        | Op::F64Rcp(_) => 2,
        Op::FAdd(_) | Op::FMul(_) | Op::Mov(_) => 1,
        Op::FFma(_) => 2,
        Op::Bra(_) | Op::Exit(_) | Op::Nop(_) | Op::Bar(_) => 1,
        Op::Ld(_) | Op::St(_) | Op::Atom(_) => 2,
        Op::Undef(_)
        | Op::PhiSrcs(_)
        | Op::PhiDsts(_)
        | Op::Pin(_)
        | Op::Unpin(_)
        | Op::RegOut(_)
        | Op::SrcBar(_)
        | Op::Annotate(_)
        | Op::Copy(_)
        | Op::Swap(_)
        | Op::ParCopy(_) => 0,
        _ => 1,
    }
}

fn encode_rdna2_op(
    op: &Op,
    _pred: &Pred,
    labels: &FxHashMap<Label, usize>,
    ip: usize,
) -> Result<Vec<u32>, CompileError> {
    match op {
        Op::Exit(_) => Ok(super::encoding::encode_s_endpgm()),
        Op::Nop(_) => Ok(super::encoding::encode_s_nop(0)),
        Op::Bar(_) => Ok(super::encoding::encode_s_barrier()),

        Op::FAdd(op) => {
            encode_vop2_from_srcs(isa::vop2::V_ADD_F32, &op.dst, &op.srcs[0], &op.srcs[1])
        }
        Op::FMul(op) => {
            encode_vop2_from_srcs(isa::vop2::V_MUL_F32, &op.dst, &op.srcs[0], &op.srcs[1])
        }
        Op::FFma(op) => encode_vop3_from_srcs(
            isa::vop3::V_FMA_F32,
            &op.dst,
            &op.srcs[0],
            &op.srcs[1],
            &op.srcs[2],
        ),
        Op::DAdd(op) => encode_vop3_from_srcs(
            isa::vop3::V_ADD_F64,
            &op.dst,
            &op.srcs[0],
            &op.srcs[1],
            &Src::ZERO,
        ),
        Op::DMul(op) => encode_vop3_from_srcs(
            isa::vop3::V_MUL_F64,
            &op.dst,
            &op.srcs[0],
            &op.srcs[1],
            &Src::ZERO,
        ),
        Op::DFma(op) => encode_vop3_from_srcs(
            isa::vop3::V_FMA_F64,
            &op.dst,
            &op.srcs[0],
            &op.srcs[1],
            &op.srcs[2],
        ),
        Op::F64Sqrt(op) => {
            let dst_reg = dst_to_vgpr_index(&op.dst)?;
            let src_enc = src_to_encoding(&Src::from(op.src.reference.clone()))?;
            Ok(Rdna2Encoder::encode_vop3(
                isa::vop3::V_SQRT_F64,
                super::reg::AmdRegRef::vgpr_pair(dst_reg),
                src_enc,
                0,
                0,
            ))
        }
        Op::F64Rcp(op) => {
            let dst_reg = dst_to_vgpr_index(&op.dst)?;
            let src_enc = src_to_encoding(&Src::from(op.src.reference.clone()))?;
            Ok(Rdna2Encoder::encode_vop3(
                isa::vop3::V_RCP_F64,
                super::reg::AmdRegRef::vgpr_pair(dst_reg),
                src_enc,
                0,
                0,
            ))
        }
        Op::Mov(op) => {
            let dst_reg = dst_to_vgpr_index(&op.dst)?;
            let src_enc = src_to_encoding(&op.src)?;
            Ok(Rdna2Encoder::encode_vop1(
                isa::vop1::V_MOV_B32,
                super::reg::AmdRegRef::vgpr(dst_reg),
                src_enc,
            ))
        }

        // ---- Memory operations (FLAT encoding) ----
        Op::Ld(op) => encode_flat_load_op(op),
        Op::St(op) => encode_flat_store_op(op),
        Op::Atom(op) => encode_flat_atomic_op(op),

        // ---- Control flow ----
        Op::Bra(op) => encode_branch(op, labels, ip),

        // ---- Comparisons (VOPC → VCC) ----
        Op::FSetP(op) => encode_fsetp(op),
        Op::ISetP(op) => encode_isetp(op),
        Op::DSetP(op) => encode_dsetp(op),

        // ---- Select (conditional move) ----
        Op::Sel(op) => encode_sel(op),

        // ---- Integer bitwise (VOP2) ----
        Op::Lop2(op) => encode_lop2(op),

        // ---- Shifts (VOP2) ----
        Op::Shl(op) => encode_shl(op),
        Op::Shr(op) => encode_shr(op),

        // ---- Integer add / multiply-add ----
        Op::IAdd3(op) => {
            encode_vop2_from_srcs(isa::vop2::V_ADD_NC_U32, &op.dst, &op.srcs[0], &op.srcs[1])
        }
        Op::IMad(op) => encode_vop3_from_srcs(
            isa::vop3::V_MAD_U32_U24,
            &op.dst,
            &op.srcs[0],
            &op.srcs[1],
            &op.srcs[2],
        ),

        // ---- System registers ----
        Op::S2R(op) => encode_s2r(op),
        Op::CS2R(op) => encode_cs2r(op),

        // ---- Conversion ops ----
        Op::F2F(op) => encode_f2f(op),
        Op::F2I(op) => encode_f2i(op),
        Op::I2F(op) => encode_i2f(op),
        Op::I2I(op) => encode_i2i(op),

        Op::Undef(_)
        | Op::PhiSrcs(_)
        | Op::PhiDsts(_)
        | Op::Pin(_)
        | Op::Unpin(_)
        | Op::RegOut(_)
        | Op::SrcBar(_)
        | Op::Annotate(_)
        | Op::Copy(_)
        | Op::Swap(_)
        | Op::ParCopy(_) => Ok(Vec::new()),

        other => Err(CompileError::NotImplemented(
            format!(
                "AMD encoding not implemented for {:?}",
                std::mem::discriminant(other)
            )
            .into(),
        )),
    }
}

// ---- FLAT memory encoding helpers ----

fn encode_flat_load_op(op: &OpLd) -> Result<Vec<u32>, CompileError> {
    let dst_reg = dst_to_vgpr_index(&op.dst)?;
    let addr_reg = src_to_vgpr_index(&op.addr)?;
    let flat_opcode = mem_type_to_flat_load(op.access.mem_type)?;
    let offset = i16::try_from(op.offset).unwrap_or(0);
    Ok(Rdna2Encoder::encode_flat_load(
        flat_opcode,
        addr_reg,
        dst_reg,
        offset,
    ))
}

fn encode_flat_store_op(op: &OpSt) -> Result<Vec<u32>, CompileError> {
    let addr_reg = src_to_vgpr_index(&op.addr)?;
    let data_reg = src_to_vgpr_index(&op.data)?;
    let flat_opcode = mem_type_to_flat_store(op.access.mem_type)?;
    let offset = i16::try_from(op.offset).unwrap_or(0);
    Ok(Rdna2Encoder::encode_flat_store(
        flat_opcode,
        addr_reg,
        data_reg,
        offset,
    ))
}

fn encode_flat_atomic_op(op: &OpAtom) -> Result<Vec<u32>, CompileError> {
    let dst_reg = dst_to_vgpr_index(&op.dst)?;
    let addr_reg = src_to_vgpr_index(&op.addr)?;
    let data_reg = src_to_vgpr_index(&op.data)?;
    let flat_opcode = atom_op_to_flat(op.atom_op)?;
    let offset = i16::try_from(op.addr_offset).unwrap_or(0);
    Ok(Rdna2Encoder::encode_flat_atomic(
        flat_opcode,
        addr_reg,
        data_reg,
        dst_reg,
        offset,
    ))
}

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

// ---- Branch encoding ----

fn encode_branch(
    op: &OpBra,
    labels: &FxHashMap<Label, usize>,
    ip: usize,
) -> Result<Vec<u32>, CompileError> {
    let target_ip = labels
        .get(&op.target)
        .copied()
        .ok_or_else(|| CompileError::InvalidInput("branch target label not found".into()))?;
    let next_ip = ip + 1; // SOPP is 1 word
    let offset = i32::try_from(target_ip)
        .unwrap_or(0)
        .wrapping_sub(i32::try_from(next_ip).unwrap_or(0));
    let offset_i16 = i16::try_from(offset).unwrap_or(0);

    if matches!(op.cond.reference, SrcRef::True) {
        Ok(Rdna2Encoder::encode_s_branch(offset_i16))
    } else {
        Ok(Rdna2Encoder::encode_s_cbranch_vccnz(offset_i16))
    }
}

// ---- Comparison encoding ----

fn encode_fsetp(op: &OpFSetP) -> Result<Vec<u32>, CompileError> {
    let src0_enc = src_to_encoding(&op.srcs[0])?;
    let src1_vgpr = src_to_vgpr_index(&op.srcs[1])?;
    let vopc_opcode = float_cmp_to_vopc_f32(op.cmp_op);
    Ok(Rdna2Encoder::encode_vopc(vopc_opcode, src0_enc, src1_vgpr))
}

fn encode_dsetp(op: &OpDSetP) -> Result<Vec<u32>, CompileError> {
    let src0_enc = src_to_encoding(&op.srcs[0])?;
    let src1_enc = src_to_encoding(&op.srcs[1])?;
    let vop3_opcode = float_cmp_to_vop3_f64(op.cmp_op);
    let dst = AmdRegRef::vgpr(0); // VCC implicit for comparison
    Ok(Rdna2Encoder::encode_vop3(
        vop3_opcode,
        dst,
        src0_enc,
        src1_enc,
        0,
    ))
}

fn encode_isetp(op: &OpISetP) -> Result<Vec<u32>, CompileError> {
    let src0_enc = src_to_encoding(&op.srcs[0])?;
    let src1_vgpr = src_to_vgpr_index(&op.srcs[1])?;
    let vopc_opcode = int_cmp_to_vopc(op.cmp_op, op.cmp_type);
    Ok(Rdna2Encoder::encode_vopc(vopc_opcode, src0_enc, src1_vgpr))
}

fn float_cmp_to_vopc_f32(cmp: FloatCmpOp) -> u16 {
    match cmp {
        FloatCmpOp::OrdEq => isa::vopc::V_CMP_EQ_F32,
        FloatCmpOp::OrdNe => isa::vopc::V_CMP_NEQ_F32,
        FloatCmpOp::OrdLt => isa::vopc::V_CMP_LT_F32,
        FloatCmpOp::OrdLe => isa::vopc::V_CMP_LE_F32,
        FloatCmpOp::OrdGt => isa::vopc::V_CMP_GT_F32,
        FloatCmpOp::OrdGe => isa::vopc::V_CMP_GE_F32,
        FloatCmpOp::UnordEq => isa::vopc::V_CMP_NLG_F32,
        FloatCmpOp::UnordNe => isa::vopc::V_CMP_NLE_F32,
        FloatCmpOp::UnordLt => isa::vopc::V_CMP_NGE_F32,
        FloatCmpOp::UnordLe => isa::vopc::V_CMP_NGT_F32,
        FloatCmpOp::UnordGt => isa::vopc::V_CMP_NLE_F32,
        FloatCmpOp::UnordGe => isa::vopc::V_CMP_NLT_F32,
        FloatCmpOp::IsNum => isa::vopc::V_CMP_O_F32,
        FloatCmpOp::IsNan => isa::vopc::V_CMP_U_F32,
    }
}

fn float_cmp_to_vop3_f64(cmp: FloatCmpOp) -> u16 {
    match cmp {
        FloatCmpOp::OrdEq => isa::vop3::V_CMP_EQ_F64,
        FloatCmpOp::OrdNe => isa::vop3::V_CMP_NEQ_F64,
        FloatCmpOp::OrdLt => isa::vop3::V_CMP_LT_F64,
        FloatCmpOp::OrdLe => isa::vop3::V_CMP_LE_F64,
        FloatCmpOp::OrdGt => isa::vop3::V_CMP_GT_F64,
        FloatCmpOp::OrdGe => isa::vop3::V_CMP_GE_F64,
        FloatCmpOp::UnordEq
        | FloatCmpOp::UnordNe
        | FloatCmpOp::UnordLt
        | FloatCmpOp::UnordLe
        | FloatCmpOp::UnordGt
        | FloatCmpOp::UnordGe => isa::vop3::V_CMP_U_F64,
        FloatCmpOp::IsNum => isa::vop3::V_CMP_O_F64,
        FloatCmpOp::IsNan => isa::vop3::V_CMP_U_F64,
    }
}

fn int_cmp_to_vopc(cmp: IntCmpOp, cmp_type: IntCmpType) -> u16 {
    if cmp_type.is_signed() {
        match cmp {
            IntCmpOp::False => isa::vopc::V_CMP_F_I32,
            IntCmpOp::True => isa::vopc::V_CMP_T_I32,
            IntCmpOp::Eq => isa::vopc::V_CMP_EQ_I32,
            IntCmpOp::Ne => isa::vopc::V_CMP_NE_I32,
            IntCmpOp::Lt => isa::vopc::V_CMP_LT_I32,
            IntCmpOp::Le => isa::vopc::V_CMP_LE_I32,
            IntCmpOp::Gt => isa::vopc::V_CMP_GT_I32,
            IntCmpOp::Ge => isa::vopc::V_CMP_GE_I32,
        }
    } else {
        match cmp {
            IntCmpOp::False => isa::vopc::V_CMP_F_U32,
            IntCmpOp::True => isa::vopc::V_CMP_T_U32,
            IntCmpOp::Eq => isa::vopc::V_CMP_EQ_U32,
            IntCmpOp::Ne => isa::vopc::V_CMP_NE_U32,
            IntCmpOp::Lt => isa::vopc::V_CMP_LT_U32,
            IntCmpOp::Le => isa::vopc::V_CMP_LE_U32,
            IntCmpOp::Gt => isa::vopc::V_CMP_GT_U32,
            IntCmpOp::Ge => isa::vopc::V_CMP_GE_U32,
        }
    }
}

// ---- Select (v_cndmask_b32: result = VCC ? src1 : src0) ----

fn encode_sel(op: &OpSel) -> Result<Vec<u32>, CompileError> {
    let dst_reg = dst_to_vgpr_index(&op.dst)?;
    let src0_enc = src_to_encoding(&op.srcs[0])?;
    let src1_vgpr = src_to_vgpr_index(&op.srcs[1])?;
    Ok(Rdna2Encoder::encode_vop2(
        isa::vop2::V_CNDMASK_B32,
        super::reg::AmdRegRef::vgpr(dst_reg),
        src0_enc,
        super::reg::AmdRegRef::vgpr(src1_vgpr),
    ))
}

// ---- Bitwise logic (VOP2) ----

fn encode_lop2(op: &OpLop2) -> Result<Vec<u32>, CompileError> {
    let vop2_opcode = match op.op {
        LogicOp2::And => isa::vop2::V_AND_B32,
        LogicOp2::Or => isa::vop2::V_OR_B32,
        LogicOp2::Xor => isa::vop2::V_XOR_B32,
        LogicOp2::PassB => {
            let dst_reg = dst_to_vgpr_index(&op.dst)?;
            let src_enc = src_to_encoding(&op.srcs[1])?;
            return Ok(Rdna2Encoder::encode_vop1(
                isa::vop1::V_MOV_B32,
                super::reg::AmdRegRef::vgpr(dst_reg),
                src_enc,
            ));
        }
    };
    encode_vop2_from_srcs(vop2_opcode, &op.dst, &op.srcs[0], &op.srcs[1])
}

// ---- Shifts (VOP2 reversed-operand forms) ----

fn encode_shl(op: &OpShl) -> Result<Vec<u32>, CompileError> {
    let dst_reg = dst_to_vgpr_index(&op.dst)?;
    let shift_enc = src_to_encoding(&op.shift)?;
    let src_vgpr = src_to_vgpr_index(&op.src)?;
    Ok(Rdna2Encoder::encode_vop2(
        isa::vop2::V_LSHLREV_B32,
        super::reg::AmdRegRef::vgpr(dst_reg),
        shift_enc,
        super::reg::AmdRegRef::vgpr(src_vgpr),
    ))
}

fn encode_shr(op: &OpShr) -> Result<Vec<u32>, CompileError> {
    let dst_reg = dst_to_vgpr_index(&op.dst)?;
    let shift_enc = src_to_encoding(&op.shift)?;
    let src_vgpr = src_to_vgpr_index(&op.src)?;
    let opcode = if op.signed {
        isa::vop2::V_ASHRREV_I32
    } else {
        isa::vop2::V_LSHRREV_B32
    };
    Ok(Rdna2Encoder::encode_vop2(
        opcode,
        super::reg::AmdRegRef::vgpr(dst_reg),
        shift_enc,
        super::reg::AmdRegRef::vgpr(src_vgpr),
    ))
}

// ---- System registers (S2R / CS2R) ----
// RDNA2 thread IDs are pre-loaded into VGPRs by the hardware dispatch.
// v0 = thread_id_x, v1 = thread_id_y, v2 = thread_id_z
// Workgroup IDs come from SGPRs (s0/s1/s2 if the shader descriptor sets it up).
// For now, we map NVIDIA SR indices to v_mov_b32 from the hardware registers.

fn encode_s2r(op: &OpS2R) -> Result<Vec<u32>, CompileError> {
    let dst_reg = dst_to_vgpr_index(&op.dst)?;
    let src_vgpr = amd_sys_reg_vgpr(op.idx)?;
    Ok(Rdna2Encoder::encode_vop1(
        isa::vop1::V_MOV_B32,
        super::reg::AmdRegRef::vgpr(dst_reg),
        256 + src_vgpr,
    ))
}

fn encode_cs2r(op: &OpCS2R) -> Result<Vec<u32>, CompileError> {
    let dst_reg = dst_to_vgpr_index(&op.dst)?;
    let src_vgpr = amd_sys_reg_vgpr(op.idx)?;
    Ok(Rdna2Encoder::encode_vop1(
        isa::vop1::V_MOV_B32,
        super::reg::AmdRegRef::vgpr(dst_reg),
        256 + src_vgpr,
    ))
}

/// Map NVIDIA system register indices to AMD RDNA2 hardware VGPR locations.
///
/// RDNA2 compute dispatch pre-loads:
/// - v0 = thread_id_x, v1 = thread_id_y, v2 = thread_id_z
/// - Workgroup ID comes from SGPR user data (s0/s1/s2 by convention).
fn amd_sys_reg_vgpr(nv_idx: u8) -> Result<u16, CompileError> {
    Ok(match nv_idx {
        0x21 => 0, // SR_TID_X → v0
        0x22 => 1, // SR_TID_Y → v1
        0x23 => 2, // SR_TID_Z → v2
        0x25 => 3, // SR_CTAID_X → v3 (mapped from SGPR user data)
        0x26 => 4, // SR_CTAID_Y → v4
        0x27 => 5, // SR_CTAID_Z → v5
        0x00 => 0, // SR_LANEID → v0 (lane within wave)
        other => {
            return Err(CompileError::NotImplemented(
                format!("AMD sys reg mapping for NVIDIA SR index 0x{other:02x}").into(),
            ));
        }
    })
}

// ---- Conversion ops ----

fn encode_f2f(op: &OpF2F) -> Result<Vec<u32>, CompileError> {
    let dst_reg = dst_to_vgpr_index(&op.dst)?;
    let src_enc = src_to_encoding(&op.src)?;
    Ok(Rdna2Encoder::encode_vop1(
        isa::vop1::V_MOV_B32,
        super::reg::AmdRegRef::vgpr(dst_reg),
        src_enc,
    ))
}

fn encode_f2i(op: &OpF2I) -> Result<Vec<u32>, CompileError> {
    let dst_reg = dst_to_vgpr_index(&op.dst)?;
    let src_enc = src_to_encoding(&op.src)?;
    Ok(Rdna2Encoder::encode_vop1(
        isa::vop1::V_CVT_I32_F32,
        super::reg::AmdRegRef::vgpr(dst_reg),
        src_enc,
    ))
}

fn encode_i2f(op: &OpI2F) -> Result<Vec<u32>, CompileError> {
    let dst_reg = dst_to_vgpr_index(&op.dst)?;
    let src_enc = src_to_encoding(&op.src)?;
    Ok(Rdna2Encoder::encode_vop1(
        isa::vop1::V_CVT_F32_I32,
        super::reg::AmdRegRef::vgpr(dst_reg),
        src_enc,
    ))
}

fn encode_i2i(op: &OpI2I) -> Result<Vec<u32>, CompileError> {
    let dst_reg = dst_to_vgpr_index(&op.dst)?;
    let src_enc = src_to_encoding(&op.src)?;
    Ok(Rdna2Encoder::encode_vop1(
        isa::vop1::V_MOV_B32,
        super::reg::AmdRegRef::vgpr(dst_reg),
        src_enc,
    ))
}

use super::reg::AmdRegRef;

fn encode_vop2_from_srcs(
    opcode: u16,
    dst: &Dst,
    src0: &Src,
    src1: &Src,
) -> Result<Vec<u32>, CompileError> {
    let dst_reg = dst_to_vgpr_index(dst)?;
    let src0_enc = src_to_encoding(src0)?;
    let src1_idx = src_to_vgpr_index(src1)?;
    Ok(Rdna2Encoder::encode_vop2(
        opcode,
        super::reg::AmdRegRef::vgpr(dst_reg),
        src0_enc,
        super::reg::AmdRegRef::vgpr(src1_idx),
    ))
}

fn encode_vop3_from_srcs(
    opcode: u16,
    dst: &Dst,
    src0: &Src,
    src1: &Src,
    src2: &Src,
) -> Result<Vec<u32>, CompileError> {
    let dst_reg = dst_to_vgpr_index(dst)?;
    let src0_enc = src_to_encoding(src0)?;
    let src1_enc = src_to_encoding(src1)?;
    let src2_enc = src_to_encoding(src2)?;
    Ok(Rdna2Encoder::encode_vop3(
        opcode,
        super::reg::AmdRegRef::vgpr(dst_reg),
        src0_enc,
        src1_enc,
        src2_enc,
    ))
}

fn dst_to_vgpr_index(dst: &Dst) -> Result<u16, CompileError> {
    match dst {
        Dst::None => Err(CompileError::InvalidInput("destination is None".into())),
        Dst::Reg(reg) => Ok(reg.base_idx().try_into().unwrap_or(0)),
        Dst::SSA(_) => Err(CompileError::InvalidInput(
            "SSA destination in encoder (not yet register-allocated)".into(),
        )),
    }
}

fn src_to_vgpr_index(src: &Src) -> Result<u16, CompileError> {
    match &src.reference {
        SrcRef::Reg(reg) => Ok(reg.base_idx().try_into().unwrap_or(0)),
        SrcRef::Zero => Ok(0),
        _ => Err(CompileError::InvalidInput(
            "VOP2 VSRC1 must be a VGPR register".into(),
        )),
    }
}

fn src_to_encoding(src: &Src) -> Result<u16, CompileError> {
    match &src.reference {
        SrcRef::Reg(reg) => {
            let idx = reg.base_idx();
            match reg.file() {
                RegFile::GPR => Ok(256 + u16::try_from(idx).unwrap_or(0)),
                RegFile::UGPR => Ok(u16::try_from(idx).unwrap_or(0)),
                _ => Ok(u16::try_from(idx).unwrap_or(0)),
            }
        }
        SrcRef::Zero => Ok(128),
        SrcRef::Imm32(val) => {
            if *val == 0 {
                Ok(128)
            } else if *val == 1 {
                Ok(129)
            } else {
                Ok(255)
            }
        }
        SrcRef::SSA(_) => Err(CompileError::InvalidInput(
            "SSA source in encoder (not yet register-allocated)".into(),
        )),
        _ => Ok(128),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shader_model_rdna2_version() {
        let sm = ShaderModelRdna2::new(103);
        assert_eq!(sm.sm(), 103);
    }

    #[test]
    fn shader_model_rdna2_reg_counts() {
        let sm = ShaderModelRdna2::new(103);
        assert_eq!(sm.reg_count(RegFile::GPR), 256);
        assert_eq!(sm.reg_count(RegFile::UGPR), 106);
    }

    #[test]
    fn shader_model_rdna2_latencies() {
        let sm = ShaderModelRdna2::new(103);
        assert_eq!(sm.exec_latency(&Op::Nop(OpNop { label: None })), 1);
        assert!(sm.latency_upper_bound() >= 200);
    }

    #[test]
    fn shader_model_rdna2_no_crs() {
        let sm = ShaderModelRdna2::new(103);
        assert_eq!(sm.crs_size(10), 0);
    }

    #[test]
    fn shader_model_rdna2_max_warps() {
        let sm = ShaderModelRdna2::new(103);
        assert_eq!(sm.max_warps(), 32);
    }

    #[test]
    fn shader_model_rdna2_wave_size() {
        let sm = ShaderModelRdna2::new(103).with_wave_size(64);
        assert_eq!(sm.wave_size, 64);
    }

    #[test]
    fn shader_model_rdna2_nvidia_compat_returns_false() {
        let sm = ShaderModelRdna2::new(103);
        assert!(!sm.is_volta());
        assert!(!sm.is_turing());
        assert!(!sm.is_ampere());
        assert!(!sm.is_ada());
    }
}
