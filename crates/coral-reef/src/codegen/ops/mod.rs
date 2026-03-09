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
use super::amd::reg::AmdRegRef;
#[allow(
    clippy::wildcard_imports,
    reason = "op module re-exports are intentional for codegen"
)]
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
}

impl<'a> AmdOpEncoder<'a> {
    pub fn new(labels: &'a FxHashMap<Label, usize>, ip: usize) -> Self {
        Self { labels, ip }
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
            Op::Bfe(op) => EncodeOp::<AmdOpEncoder<'_>>::encode(op.as_ref(), $enc),
            Op::BMsk(op) => EncodeOp::<AmdOpEncoder<'_>>::encode(op.as_ref(), $enc),

            // ---- Control flow ops (ops/control.rs) ----
            Op::Exit(op) => EncodeOp::<AmdOpEncoder<'_>>::encode(op, $enc),
            Op::Nop(op) => EncodeOp::<AmdOpEncoder<'_>>::encode(op, $enc),
            Op::Bar(op) => EncodeOp::<AmdOpEncoder<'_>>::encode(op.as_ref(), $enc),
            Op::Bra(op) => EncodeOp::<AmdOpEncoder<'_>>::encode(op.as_ref(), $enc),

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

/// Encode a single AMD RDNA2 op using the unified dispatch.
pub fn encode_amd_op(
    op: &Op,
    _pred: &Pred,
    labels: &FxHashMap<Label, usize>,
    ip: usize,
) -> Result<Vec<u32>, CompileError> {
    let mut enc = AmdOpEncoder::new(labels, ip);
    op_encode_amd!(op, &mut enc)
}

// ---- Shared encoding helpers used across category modules ----

pub(crate) fn dst_to_vgpr_index(dst: &Dst) -> Result<u16, CompileError> {
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

pub(crate) fn src_to_vgpr_index(src: &Src) -> Result<u16, CompileError> {
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
pub(crate) struct SrcEncoding {
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

pub(crate) fn src_to_encoding(src: &Src) -> Result<SrcEncoding, CompileError> {
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
pub(crate) fn cbuf_to_user_sgpr_encoding(
    buf: &CBuf,
    byte_offset: u16,
) -> Result<u16, CompileError> {
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
pub(crate) fn encode_vop2_from_srcs(
    opcode: u16,
    dst: &Dst,
    src0: &Src,
    src1: &Src,
) -> Result<Vec<u32>, CompileError> {
    let dst_reg = dst_to_vgpr_index(dst)?;

    let src1_is_vgpr = src_to_vgpr_index(src1).is_ok();
    let src0_is_vgpr = src_to_vgpr_index(src0).is_ok();

    if src1_is_vgpr {
        let src0_enc = src_to_encoding(src0)?;
        let src1_idx = src_to_vgpr_index(src1)?;
        let mut words = Rdna2Encoder::encode_vop2(
            opcode,
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
            opcode,
            AmdRegRef::vgpr(dst_reg),
            src1_enc.src0,
            AmdRegRef::vgpr(src0_idx),
        );
        src1_enc.extend_with_literal(&mut words);
        Ok(words)
    } else {
        let vop3_opcode = opcode + 256;
        let src0_enc = src_to_encoding(src0)?;
        let src1_enc = src_to_encoding(src1)?;
        if src0_enc.literal.is_some() || src1_enc.literal.is_some() {
            return Err(CompileError::NotImplemented(
                "VOP3 fallback does not support literal constants in both sources".into(),
            ));
        }
        Ok(Rdna2Encoder::encode_vop3(
            vop3_opcode,
            AmdRegRef::vgpr(dst_reg),
            src0_enc.src0,
            src1_enc.src0,
            0,
        ))
    }
}

/// Encode a VOPC comparison with automatic operand legalization.
///
/// RDNA2 VOPC requires VSRC1 to be a VGPR. If `src1` is not a VGPR but
/// `src0` is, swap operands (comparison direction is preserved by the caller
/// selecting the appropriate opcode).
pub(crate) fn encode_vopc_legalized(
    opcode: u16,
    src0: &Src,
    src1: &Src,
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
        if src0_enc.literal.is_some() || src1_enc.literal.is_some() {
            return Err(CompileError::NotImplemented(
                "VOPC fallback does not support literal constants in both sources".into(),
            ));
        }
        let vop3_opcode = opcode + 256;
        Ok(Rdna2Encoder::encode_vop3(
            vop3_opcode,
            AmdRegRef::vgpr(0),
            src0_enc.src0,
            src1_enc.src0,
            0,
        ))
    }
}

pub(crate) fn encode_vop3_from_srcs(
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
    if src0_enc.literal.is_some() || src1_enc.literal.is_some() || src2_enc.literal.is_some() {
        return Err(CompileError::NotImplemented(
            "VOP3 does not support literal constants; value must be materialized first".into(),
        ));
    }
    Ok(Rdna2Encoder::encode_vop3(
        opcode,
        AmdRegRef::vgpr(dst_reg),
        src0_enc.src0,
        src1_enc.src0,
        src2_enc.src0,
    ))
}
