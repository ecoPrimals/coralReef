// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright © 2026 ecoPrimals
//! AMD `EncodeOp` trait, [`AmdOpEncoder`], and unified `Op` → machine-code dispatch.

use super::gfx9::{patch_vop3_prefix_for_gfx9, patch_vopc_for_gfx9, vop2_opcode_for_gfx};
use crate::CompileError;
use crate::codegen::ir::*;
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
    /// Number of user data SGPRs (buffer VAs + NTID + NCTAID). Workgroup IDs
    /// follow immediately after in the SGPR file (s[user_sgpr_count], etc.).
    pub user_sgpr_count: u16,
    /// VGPR base where hardware-preloaded TID.x/y/z were saved in the prologue.
    /// S2R for SR_TID_X/Y/Z maps to v[tid_save_base+0/1/2].
    pub tid_save_base: u16,
}

impl<'a> AmdOpEncoder<'a> {
    pub fn new(
        labels: &'a FxHashMap<Label, usize>,
        ip: usize,
        scratch_vgpr_0: u16,
        scratch_vgpr_1: u16,
        gfx_major: u8,
        user_sgpr_count: u16,
        tid_save_base: u16,
    ) -> Self {
        Self {
            labels,
            ip,
            scratch_vgpr_0,
            scratch_vgpr_1,
            gfx_major,
            user_sgpr_count,
            tid_save_base,
        }
    }

    /// FLAT offset handling per GFX generation.
    ///
    /// FLAT (SEG=00) on GFX9 does not support offset, but GLOBAL (SEG=10)
    /// and SCRATCH (SEG=01) do. Since all coral-reef memory ops encode with
    /// SEG=GLOBAL, the offset is always passed through.
    pub fn flat_offset(&self, _offset: i16) -> i16 {
        _offset
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
    tid_save_base: u16,
) -> Result<Vec<u32>, CompileError> {
    let mut enc = AmdOpEncoder::new(
        labels,
        ip,
        scratch_vgpr_0,
        scratch_vgpr_1,
        gfx_major,
        user_sgpr_count,
        tid_save_base,
    );
    let mut words = op_encode_amd!(op, &mut enc)?;
    if gfx_major < 10 {
        patch_vop3_prefix_for_gfx9(&mut words);
        patch_vopc_for_gfx9(&mut words);
    }
    Ok(words)
}
