// SPDX-License-Identifier: AGPL-3.0-only
// Copyright © 2026 ecoPrimals
//! Unified ops encoder — vendor-agnostic operation encoding via `EncodeOp<E>`.
//!
//! This module organizes GPU instruction encoding by **operation category**
//! rather than by vendor. Each op implements `EncodeOp<E>` for each encoder
//! type (`AmdOpEncoder`, and eventually `SM70Encoder`), keeping all vendor
//! implementations together per operation.
//!
//! ## Architecture
//!
//! ```text
//!   Op enum ──→ op_encode! macro ──→ category module (e.g. memory.rs)
//!                                        ├─ impl EncodeOp<AmdOpEncoder>
//!                                        └─ impl EncodeOp<SM70Encoder>  (future)
//! ```

pub mod alu_float;
pub mod alu_int;
pub mod control;
pub mod convert;
pub mod memory;
pub mod system;

use super::amd::encoding::Rdna2Encoder;
use super::amd::isa;
use super::amd::reg::AmdRegRef;
use super::ir::*;
use crate::CompileError;

use coral_reef_stubs::fxhash::FxHashMap;

/// Vendor-agnostic operation encoding trait.
///
/// Each concrete IR op (e.g. `OpFAdd`, `OpLd`, `OpLdc`) implements this
/// for each target encoder type. `E` is the encoder — `AmdOpEncoder` for
/// RDNA2, `SM70Encoder` for NVIDIA (future).
pub trait EncodeOp<E> {
    fn encode(&self, e: &mut E) -> Result<Vec<u32>, CompileError>;
}

/// AMD RDNA2 op encoder — carries state needed during instruction encoding.
///
/// Wraps the low-level `Rdna2Encoder` with additional context (label map,
/// current IP) that individual op encoders need for branch resolution and
/// instruction sizing.
pub struct AmdOpEncoder<'a> {
    pub labels: &'a FxHashMap<Label, usize>,
    pub ip: usize,
    pub scratch_vgpr_0: u16,
    pub scratch_vgpr_1: u16,
    /// GFX major version (9 = GCN5/Vega, 10 = RDNA2, 11 = RDNA3, 12 = RDNA4).
    /// Controls encoding differences such as FLAT offset availability.
    pub gfx_major: u8,
    /// Number of user data SGPRs (2 per buffer VA). Workgroup IDs follow
    /// immediately after in the SGPR file (s[user_sgpr_count], etc.).
    pub user_sgpr_count: u16,
}

impl<'a> AmdOpEncoder<'a> {
    pub fn new(
        labels: &'a FxHashMap<Label, usize>,
        ip: usize,
        scratch_vgpr_0: u16,
        scratch_vgpr_1: u16,
        gfx_major: u8,
        user_sgpr_count: u16,
    ) -> Self {
        Self {
            labels,
            ip,
            scratch_vgpr_0,
            scratch_vgpr_1,
            gfx_major,
            user_sgpr_count,
        }
    }

    /// GFX9 (GCN5/Vega) does not support FLAT offset; clamp to 0.
    pub fn flat_offset(&self, offset: i16) -> i16 {
        if self.gfx_major < 10 { 0 } else { offset }
    }

    /// Remap an RDNA2 VOP2 opcode to the correct hardware opcode for this GFX version.
    pub fn vop2(&self, rdna2_op: u16) -> u16 {
        vop2_opcode_for_gfx(rdna2_op, self.gfx_major)
    }
}

/// Dispatch an `Op` to its `EncodeOp<E>` implementation for AMD.
///
/// This macro maps each `Op` variant to the appropriate category module's
/// encoding. Virtual ops (SSA-only, no machine code) emit nothing.
macro_rules! op_encode_amd {
    ($op:expr, $enc:expr) => {
        match $op {
            // ---- Memory ops (ops/memory.rs) ----
            Op::Ld(op) => EncodeOp::<AmdOpEncoder<'_>>::encode(op.as_ref(), $enc),
            Op::St(op) => EncodeOp::<AmdOpEncoder<'_>>::encode(op.as_ref(), $enc),
            Op::Atom(op) => EncodeOp::<AmdOpEncoder<'_>>::encode(op.as_ref(), $enc),
            Op::Ldc(op) => EncodeOp::<AmdOpEncoder<'_>>::encode(op.as_ref(), $enc),
            Op::MemBar(op) => EncodeOp::<AmdOpEncoder<'_>>::encode(op.as_ref(), $enc),

            // ---- Float ALU ops (ops/alu_float.rs) ----
            Op::FAdd(op) => EncodeOp::<AmdOpEncoder<'_>>::encode(op.as_ref(), $enc),
            Op::FMul(op) => EncodeOp::<AmdOpEncoder<'_>>::encode(op.as_ref(), $enc),
            Op::FFma(op) => EncodeOp::<AmdOpEncoder<'_>>::encode(op.as_ref(), $enc),
            Op::FMnMx(op) => EncodeOp::<AmdOpEncoder<'_>>::encode(op.as_ref(), $enc),
            Op::DAdd(op) => EncodeOp::<AmdOpEncoder<'_>>::encode(op.as_ref(), $enc),
            Op::DMul(op) => EncodeOp::<AmdOpEncoder<'_>>::encode(op.as_ref(), $enc),
            Op::DFma(op) => EncodeOp::<AmdOpEncoder<'_>>::encode(op.as_ref(), $enc),
            Op::DMnMx(op) => EncodeOp::<AmdOpEncoder<'_>>::encode(op.as_ref(), $enc),
            Op::F64Sqrt(op) => EncodeOp::<AmdOpEncoder<'_>>::encode(op.as_ref(), $enc),
            Op::F64Rcp(op) => EncodeOp::<AmdOpEncoder<'_>>::encode(op.as_ref(), $enc),
            Op::F64Exp2(op) => EncodeOp::<AmdOpEncoder<'_>>::encode(op.as_ref(), $enc),
            Op::F64Log2(op) => EncodeOp::<AmdOpEncoder<'_>>::encode(op.as_ref(), $enc),
            Op::F64Sin(op) => EncodeOp::<AmdOpEncoder<'_>>::encode(op.as_ref(), $enc),
            Op::F64Cos(op) => EncodeOp::<AmdOpEncoder<'_>>::encode(op.as_ref(), $enc),
            Op::Transcendental(op) => EncodeOp::<AmdOpEncoder<'_>>::encode(op.as_ref(), $enc),
            Op::FSetP(op) => EncodeOp::<AmdOpEncoder<'_>>::encode(op.as_ref(), $enc),
            Op::DSetP(op) => EncodeOp::<AmdOpEncoder<'_>>::encode(op.as_ref(), $enc),
            Op::ISetP(op) => EncodeOp::<AmdOpEncoder<'_>>::encode(op.as_ref(), $enc),

            // ---- Integer ALU ops (ops/alu_int.rs) ----
            Op::IAdd3(op) => EncodeOp::<AmdOpEncoder<'_>>::encode(op.as_ref(), $enc),
            Op::IMad(op) => EncodeOp::<AmdOpEncoder<'_>>::encode(op.as_ref(), $enc),
            Op::IMnMx(op) => EncodeOp::<AmdOpEncoder<'_>>::encode(op.as_ref(), $enc),
            Op::Lop2(op) => EncodeOp::<AmdOpEncoder<'_>>::encode(op.as_ref(), $enc),
            Op::Lop3(op) => EncodeOp::<AmdOpEncoder<'_>>::encode(op.as_ref(), $enc),
            Op::Shl(op) => EncodeOp::<AmdOpEncoder<'_>>::encode(op.as_ref(), $enc),
            Op::Shr(op) => EncodeOp::<AmdOpEncoder<'_>>::encode(op.as_ref(), $enc),
            Op::Shf(op) => EncodeOp::<AmdOpEncoder<'_>>::encode(op.as_ref(), $enc),
            Op::Sel(op) => EncodeOp::<AmdOpEncoder<'_>>::encode(op.as_ref(), $enc),
            Op::PopC(op) => EncodeOp::<AmdOpEncoder<'_>>::encode(op.as_ref(), $enc),
            Op::BRev(op) => EncodeOp::<AmdOpEncoder<'_>>::encode(op.as_ref(), $enc),
            Op::Flo(op) => EncodeOp::<AmdOpEncoder<'_>>::encode(op.as_ref(), $enc),
            Op::Bfe(op) => EncodeOp::<AmdOpEncoder<'_>>::encode(op.as_ref(), $enc),
            Op::BMsk(op) => EncodeOp::<AmdOpEncoder<'_>>::encode(op.as_ref(), $enc),

            // ---- Control flow ops (ops/control.rs) ----
            Op::Exit(op) => EncodeOp::<AmdOpEncoder<'_>>::encode(op, $enc),
            Op::Nop(op) => EncodeOp::<AmdOpEncoder<'_>>::encode(op, $enc),
            Op::Bar(op) => EncodeOp::<AmdOpEncoder<'_>>::encode(op.as_ref(), $enc),
            Op::Bra(op) => EncodeOp::<AmdOpEncoder<'_>>::encode(op.as_ref(), $enc),

            // ---- Rounding ops (ops/alu_float.rs) ----
            Op::FRnd(op) => EncodeOp::<AmdOpEncoder<'_>>::encode(op.as_ref(), $enc),

            // ---- Conversion ops (ops/convert.rs) ----
            Op::F2F(op) => EncodeOp::<AmdOpEncoder<'_>>::encode(op.as_ref(), $enc),
            Op::F2I(op) => EncodeOp::<AmdOpEncoder<'_>>::encode(op.as_ref(), $enc),
            Op::I2F(op) => EncodeOp::<AmdOpEncoder<'_>>::encode(op.as_ref(), $enc),
            Op::I2I(op) => EncodeOp::<AmdOpEncoder<'_>>::encode(op.as_ref(), $enc),

            // ---- System / move ops (ops/system.rs) ----
            Op::Mov(op) => EncodeOp::<AmdOpEncoder<'_>>::encode(op.as_ref(), $enc),
            Op::S2R(op) => EncodeOp::<AmdOpEncoder<'_>>::encode(op.as_ref(), $enc),
            Op::CS2R(op) => EncodeOp::<AmdOpEncoder<'_>>::encode(op.as_ref(), $enc),

            // ---- Copy: CBuf sources need AMD user SGPR materialization ----
            Op::Copy(op) => {
                if matches!(op.src.reference, SrcRef::CBuf(_)) {
                    EncodeOp::<AmdOpEncoder<'_>>::encode(op.as_ref(), $enc)
                } else {
                    Ok(Vec::new())
                }
            }

            // ---- Virtual ops (no machine code) ----
            Op::Undef(_)
            | Op::PhiSrcs(_)
            | Op::PhiDsts(_)
            | Op::Pin(_)
            | Op::Unpin(_)
            | Op::RegOut(_)
            | Op::SrcBar(_)
            | Op::Annotate(_)
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
    };
}

/// Encode a single AMD op using the unified dispatch.
pub fn encode_amd_op(
    op: &Op,
    _pred: &Pred,
    labels: &FxHashMap<Label, usize>,
    ip: usize,
    scratch_vgpr_0: u16,
    scratch_vgpr_1: u16,
    gfx_major: u8,
    user_sgpr_count: u16,
) -> Result<Vec<u32>, CompileError> {
    let mut enc = AmdOpEncoder::new(
        labels,
        ip,
        scratch_vgpr_0,
        scratch_vgpr_1,
        gfx_major,
        user_sgpr_count,
    );
    let mut words = op_encode_amd!(op, &mut enc)?;
    if gfx_major < 10 {
        patch_vop3_prefix_for_gfx9(&mut words);
    }
    Ok(words)
}

/// Patch VOP3 words from RDNA2 to GFX9.
///
/// Both architectures share the same VOP3a word-0 layout:
///   [31:26]=prefix  [25:16]=OP(10)  [15]=CLAMP  [10:8]=ABS  [7:0]=VDST
///
/// Only two things differ:
///   1. Prefix: 110101 (RDNA2) → 110100 (GFX9)
///   2. VOP3-only opcode values (≥320) are remapped between architectures
///
/// VOP2-promoted VOP3 opcodes (<64) are already translated before encoding
/// via `vop3_promoted_opcode_for_gfx`, so we leave those unchanged.
fn patch_vop3_prefix_for_gfx9(words: &mut [u32]) {
    for word in words.iter_mut() {
        if (*word >> 26) & 0x3F == 0b11_0101 {
            let op_rdna2 = ((*word >> 16) & 0x3FF) as u16;
            let rest = *word & 0xFFFF;
            let op_gfx9 = if op_rdna2 >= 320 {
                vop3_only_opcode_for_gfx9(op_rdna2)
            } else {
                op_rdna2
            };
            *word = (0b11_0100u32 << 26) | ((op_gfx9 as u32 & 0x3FF) << 16) | rest;
        }
    }
}

/// Translate VOP3-only opcodes from RDNA2 to GFX9.
///
/// Group A (MAD/BFE/BFI/FMA, RDNA2 320-351) shifts uniformly by +128.
/// Group B (F64 arith / MUL_HI, RDNA2 352+) requires per-instruction mapping
/// because the instruction ordering changed between architectures.
/// LLVM `llvm-mc --mcpu=gfx906` was used to derive every entry.
fn vop3_only_opcode_for_gfx9(rdna2_op: u16) -> u16 {
    match rdna2_op {
        // Group A: uniform +128 offset (verified via LLVM for MAD/FMA/BFE/BFI)
        320..=351 => rdna2_op + 128,
        // Group B: per-instruction (LLVM-validated)
        356 => 640, // V_ADD_F64
        357 => 641, // V_MUL_F64
        358 => 642, // V_MIN_F64
        359 => 643, // V_MAX_F64
        362 => 646, // V_MUL_HI_U32
        364 => 647, // V_MUL_HI_I32
        _ => rdna2_op,
    }
}

// ---- GFX9 VOP2 opcode remap ----
//
// RDNA (GFX10+) reshuffled VOP2 opcodes relative to GCN5 (GFX9).
// V_ADD_NC_U32 (no carry) doesn't exist on GFX9; we substitute
// V_ADD_CO_U32 which generates carry to VCC (harmless when unread).

fn vop2_opcode_for_gfx(rdna2_op: u16, gfx_major: u8) -> u16 {
    if gfx_major >= 10 {
        return rdna2_op;
    }
    match rdna2_op {
        1 => 0,   // V_CNDMASK_B32
        3 => 1,   // V_ADD_F32
        4 => 2,   // V_SUB_F32
        5 => 3,   // V_SUBREV_F32
        8 => 5,   // V_MUL_F32
        9 => 6,   // V_MUL_I32_I24
        11 => 8,  // V_MUL_U32_U24
        15 => 10, // V_MIN_F32
        16 => 11, // V_MAX_F32
        17 => 12, // V_MIN_I32
        18 => 13, // V_MAX_I32
        19 => 14, // V_MIN_U32
        20 => 15, // V_MAX_U32
        22 => 16, // V_LSHRREV_B32
        24 => 17, // V_ASHRREV_I32
        26 => 18, // V_LSHLREV_B32
        27 => 19, // V_AND_B32
        28 => 20, // V_OR_B32
        29 => 21, // V_XOR_B32
        37 => 25, // V_ADD_NC_U32 → V_ADD_CO_U32
        38 => 26, // V_SUB_NC_U32 → V_SUB_CO_U32
        40 => 28, // V_ADD_CO_CI_U32 → V_ADDC_CO_U32
        _ => rdna2_op,
    }
}

/// Remap a VOP3 opcode that was promoted from VOP2.
/// On RDNA2: VOP3_opcode = VOP2_opcode + 256.
/// On GFX9:  VOP3_opcode = GFX9_VOP2_opcode + 256.
fn vop3_promoted_opcode_for_gfx(rdna2_vop3: u16, gfx_major: u8) -> u16 {
    if gfx_major >= 10 {
        return rdna2_vop3;
    }
    if rdna2_vop3 >= 256 && rdna2_vop3 < 512 {
        let rdna2_vop2 = rdna2_vop3 - 256;
        vop2_opcode_for_gfx(rdna2_vop2, gfx_major) + 256
    } else {
        rdna2_vop3
    }
}

// ---- Shared encoding helpers used across category modules ----

pub fn materialize_if_literal(scratch_vgpr: u16, enc: &SrcEncoding) -> (Vec<u32>, SrcEncoding) {
    if let Some(literal_val) = enc.literal {
        let mut mov =
            Rdna2Encoder::encode_vop1(isa::vop1::V_MOV_B32, AmdRegRef::vgpr(scratch_vgpr), 255);
        mov.push(literal_val);
        (mov, SrcEncoding::inline(256 + scratch_vgpr))
    } else {
        (Vec::new(), SrcEncoding::inline(enc.src0))
    }
}

pub fn dst_to_vgpr_index(dst: &Dst) -> Result<u16, CompileError> {
    match dst {
        Dst::None => Err(CompileError::InvalidInput("destination is None".into())),
        Dst::Reg(reg) => u16::try_from(reg.base_idx()).map_err(|_| {
            CompileError::InvalidInput(
                format!("VGPR dst index {} exceeds u16", reg.base_idx()).into(),
            )
        }),
        Dst::SSA(_) => Err(CompileError::InvalidInput(
            "SSA destination in encoder (not yet register-allocated)".into(),
        )),
    }
}

pub fn src_to_vgpr_index(src: &Src) -> Result<u16, CompileError> {
    match &src.reference {
        SrcRef::Reg(reg) => u16::try_from(reg.base_idx()).map_err(|_| {
            CompileError::InvalidInput(
                format!("VGPR src index {} exceeds u16", reg.base_idx()).into(),
            )
        }),
        SrcRef::Zero => Ok(0),
        _ => Err(CompileError::InvalidInput(
            "VOP2 VSRC1 must be a VGPR register".into(),
        )),
    }
}

/// Result of encoding a source operand — SRC0 field value + optional literal.
///
/// On RDNA2, the SRC0 field uses inline constants for common values (0..64,
/// -1..-16, common floats). Values outside that range require SRC0=255
/// followed by a literal DWORD in the instruction stream.
pub struct SrcEncoding {
    /// The 9-bit SRC0 field value (SGPR, VGPR, inline constant, or 255 for literal).
    pub src0: u16,
    /// Literal DWORD to append after the instruction word, if SRC0=255.
    pub literal: Option<u32>,
}

impl SrcEncoding {
    pub const fn inline(src0: u16) -> Self {
        Self {
            src0,
            literal: None,
        }
    }
    pub const fn literal(val: u32) -> Self {
        Self {
            src0: 255,
            literal: Some(val),
        }
    }
    /// Append any literal DWORD to the encoded instruction words.
    pub fn extend_with_literal(&self, words: &mut Vec<u32>) {
        if let Some(lit) = self.literal {
            words.push(lit);
        }
    }
}

pub fn src_to_encoding(src: &Src) -> Result<SrcEncoding, CompileError> {
    match &src.reference {
        SrcRef::Reg(reg) => {
            let idx = u16::try_from(reg.base_idx()).map_err(|_| {
                CompileError::InvalidInput(
                    format!("register index {} exceeds u16", reg.base_idx()).into(),
                )
            })?;
            match reg.file() {
                RegFile::GPR => Ok(SrcEncoding::inline(256 + idx)),
                RegFile::UGPR => Ok(SrcEncoding::inline(idx)),
                _ => Ok(SrcEncoding::inline(idx)),
            }
        }
        SrcRef::Zero => Ok(SrcEncoding::inline(128)),
        SrcRef::Imm32(val) => Ok(imm32_to_src_encoding(*val)),
        SrcRef::SSA(_) => Err(CompileError::InvalidInput(
            "SSA source in encoder (not yet register-allocated)".into(),
        )),
        SrcRef::CBuf(cb) => cbuf_to_user_sgpr_encoding(&cb.buf, cb.offset).map(SrcEncoding::inline),
        _ => Ok(SrcEncoding::inline(128)),
    }
}

/// Encode a u32 immediate as an RDNA2 inline constant or literal.
///
/// Inline constant map (no extra DWORD):
///   128     → 0
///   129-192 → 1..64
///   193-208 → -1..-16  (as u32: 0xFFFF_FFFF .. 0xFFFF_FFF0)
/// Everything else requires a literal (SRC0=255 + trailing DWORD).
fn imm32_to_src_encoding(val: u32) -> SrcEncoding {
    match val {
        0 => SrcEncoding::inline(128),
        1..=64 => SrcEncoding::inline(128 + val as u16),
        // -1..-16 in two's complement
        0xFFFF_FFF0..=0xFFFF_FFFF => {
            let neg = val.wrapping_neg(); // 1..16
            SrcEncoding::inline(192 + neg as u16)
        }
        _ => SrcEncoding::literal(val),
    }
}

/// Map a constant buffer reference to an AMD user SGPR encoding value.
///
/// On AMD, constant buffer data is passed via COMPUTE_USER_DATA registers
/// which populate SGPRs s[0..N]. The naga translation lays out storage
/// buffer addresses as: `CBuf::Binding(group)[binding * 8 + component]`.
///
/// Returns the SGPR register index (0..105) suitable for VOP1/VOP2 src fields.
pub fn cbuf_to_user_sgpr_encoding(buf: &CBuf, byte_offset: u16) -> Result<u16, CompileError> {
    let CBuf::Binding(_buf_idx) = buf else {
        return Err(CompileError::NotImplemented(
            "bindless constant buffer access on AMD".into(),
        ));
    };
    // Within a binding group, offsets are laid out sequentially.
    // byte_offset / 4 gives the DWORD (SGPR) index.
    let sgpr_idx = byte_offset / 4;
    Ok(sgpr_idx)
}

/// Encode a VOP2 instruction with automatic operand legalization.
///
/// RDNA2 VOP2 requires VSRC1 to be a VGPR. If `src1` is not a VGPR:
/// 1. Swap operands (valid for commutative ops like add/mul/min/max).
/// 2. Fall back to VOP3 encoding (opcode + 256) which allows any
///    9-bit source in all three operand slots.
pub fn encode_vop2_from_srcs(
    opcode: u16,
    dst: &Dst,
    src0: &Src,
    src1: &Src,
    enc: &AmdOpEncoder<'_>,
) -> Result<Vec<u32>, CompileError> {
    let dst_reg = dst_to_vgpr_index(dst)?;
    let hw_op = vop2_opcode_for_gfx(opcode, enc.gfx_major);

    let src1_is_vgpr = src_to_vgpr_index(src1).is_ok();
    let src0_is_vgpr = src_to_vgpr_index(src0).is_ok();

    if src1_is_vgpr {
        let src0_enc = src_to_encoding(src0)?;
        let src1_idx = src_to_vgpr_index(src1)?;
        let mut words = Rdna2Encoder::encode_vop2(
            hw_op,
            AmdRegRef::vgpr(dst_reg),
            src0_enc.src0,
            AmdRegRef::vgpr(src1_idx),
        );
        src0_enc.extend_with_literal(&mut words);
        Ok(words)
    } else if src0_is_vgpr {
        let src1_enc = src_to_encoding(src1)?;
        let src0_idx = src_to_vgpr_index(src0)?;
        let mut words = Rdna2Encoder::encode_vop2(
            hw_op,
            AmdRegRef::vgpr(dst_reg),
            src1_enc.src0,
            AmdRegRef::vgpr(src0_idx),
        );
        src1_enc.extend_with_literal(&mut words);
        Ok(words)
    } else {
        let vop3_opcode = vop2_opcode_for_gfx(opcode, enc.gfx_major) + 256;
        let src0_enc = src_to_encoding(src0)?;
        let src1_enc = src_to_encoding(src1)?;
        let (mut prefix0, mat0) = materialize_if_literal(enc.scratch_vgpr_0, &src0_enc);
        let (prefix1, mat1) = materialize_if_literal(enc.scratch_vgpr_1, &src1_enc);
        prefix0.extend(prefix1);
        let words = Rdna2Encoder::encode_vop3(
            vop3_opcode,
            AmdRegRef::vgpr(dst_reg),
            mat0.src0,
            mat1.src0,
            0,
        );
        prefix0.extend(words);
        Ok(prefix0)
    }
}

/// Encode a VOPC comparison with automatic operand legalization.
///
/// RDNA2 VOPC requires VSRC1 to be a VGPR. If `src1` is not a VGPR but
/// `src0` is, swap operands (comparison direction is preserved by the caller
/// selecting the appropriate opcode).
pub fn encode_vopc_legalized(
    opcode: u16,
    src0: &Src,
    src1: &Src,
    enc: &AmdOpEncoder<'_>,
) -> Result<Vec<u32>, CompileError> {
    let src1_is_vgpr = src_to_vgpr_index(src1).is_ok();
    let src0_is_vgpr = src_to_vgpr_index(src0).is_ok();

    if src1_is_vgpr {
        let src0_enc = src_to_encoding(src0)?;
        let src1_idx = src_to_vgpr_index(src1)?;
        let mut words = Rdna2Encoder::encode_vopc(opcode, src0_enc.src0, src1_idx);
        src0_enc.extend_with_literal(&mut words);
        Ok(words)
    } else if src0_is_vgpr {
        let src1_enc = src_to_encoding(src1)?;
        let src0_idx = src_to_vgpr_index(src0)?;
        let mut words = Rdna2Encoder::encode_vopc(opcode, src1_enc.src0, src0_idx);
        src1_enc.extend_with_literal(&mut words);
        Ok(words)
    } else {
        let src0_enc = src_to_encoding(src0)?;
        let src1_enc = src_to_encoding(src1)?;
        let (mut prefix0, mat0) = materialize_if_literal(enc.scratch_vgpr_0, &src0_enc);
        let (prefix1, mat1) = materialize_if_literal(enc.scratch_vgpr_1, &src1_enc);
        prefix0.extend(prefix1);
        let vop3_opcode = vop3_promoted_opcode_for_gfx(opcode + 256, enc.gfx_major);
        let words =
            Rdna2Encoder::encode_vop3(vop3_opcode, AmdRegRef::vgpr(0), mat0.src0, mat1.src0, 0);
        prefix0.extend(words);
        Ok(prefix0)
    }
}

pub fn encode_vop3_from_srcs(
    opcode: u16,
    dst: &Dst,
    src0: &Src,
    src1: &Src,
    src2: &Src,
    enc: &AmdOpEncoder<'_>,
) -> Result<Vec<u32>, CompileError> {
    let dst_reg = dst_to_vgpr_index(dst)?;
    let src0_enc = src_to_encoding(src0)?;
    let src1_enc = src_to_encoding(src1)?;
    let src2_enc = src_to_encoding(src2)?;
    let literal_count = [&src0_enc, &src1_enc, &src2_enc]
        .iter()
        .filter(|e| e.literal.is_some())
        .count();
    if literal_count > 2 {
        return Err(CompileError::NotImplemented(
            "VOP3: third literal would require additional scratch VGPR".into(),
        ));
    }
    let mut next_scratch = enc.scratch_vgpr_0;
    let (mut prefix0, mat0) = if src0_enc.literal.is_some() {
        let (p, m) = materialize_if_literal(next_scratch, &src0_enc);
        next_scratch = enc.scratch_vgpr_1;
        (p, m)
    } else {
        (Vec::new(), SrcEncoding::inline(src0_enc.src0))
    };
    let (prefix1, mat1) = if src1_enc.literal.is_some() {
        let (p, m) = materialize_if_literal(next_scratch, &src1_enc);
        next_scratch = enc.scratch_vgpr_1;
        (p, m)
    } else {
        (Vec::new(), SrcEncoding::inline(src1_enc.src0))
    };
    let (prefix2, mat2) = if src2_enc.literal.is_some() {
        materialize_if_literal(next_scratch, &src2_enc)
    } else {
        (Vec::new(), SrcEncoding::inline(src2_enc.src0))
    };
    prefix0.extend(prefix1);
    prefix0.extend(prefix2);
    let words = Rdna2Encoder::encode_vop3(
        opcode,
        AmdRegRef::vgpr(dst_reg),
        mat0.src0,
        mat1.src0,
        mat2.src0,
    );
    prefix0.extend(words);
    Ok(prefix0)
}
