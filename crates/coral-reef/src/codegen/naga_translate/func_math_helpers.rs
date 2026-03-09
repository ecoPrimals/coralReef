// SPDX-License-Identifier: AGPL-3.0-only
//! Math emission helpers for the naga translator.
//!
//! Extracted from `func_math.rs` to keep each file under 1000 LOC.
//!
//! - **f64 rounding**: round/floor/ceil via the 2^52 magic-number technique
//! - **f32 trig**: pre-scaled Sin/Cos transcendentals
//! - **f32 dot**: self-dot-product via FMul + FFma chain
//! - **SM-portable logic ops**: LOP3 (SM70+) / LOP2 (older)

#![allow(clippy::wildcard_imports)]
use super::super::ir::*;
use super::func::FuncTranslator;
use crate::error::CompileError;

impl<'a, 'b> FuncTranslator<'a, 'b> {
    /// f64 round-to-nearest-even using the 2^52 magic number technique.
    ///
    /// `result = (x + copysign(MAGIC, x)) - copysign(MAGIC, x)`
    ///
    /// where `MAGIC = 2^52`. Adding and subtracting this value forces
    /// the FPU to round to an integer while preserving the sign.
    pub(super) fn emit_f64_round(&mut self, x: SSARef) -> Result<SSARef, CompileError> {
        const MAGIC_HI: u32 = 0x4330_0000; // 2^52 high word

        let sign_bit = self.alloc_ssa(RegFile::GPR);
        self.emit_logic_and(sign_bit, x[1].into(), Src::new_imm_u32(0x8000_0000));

        let signed_hi = self.alloc_ssa(RegFile::GPR);
        self.emit_logic_or(signed_hi, Src::new_imm_u32(MAGIC_HI), sign_bit.into());

        let signed_magic = self.alloc_ssa_vec(RegFile::GPR, 2);
        self.push_instr(Instr::new(OpCopy {
            dst: signed_magic[0].into(),
            src: Src::ZERO,
        }));
        self.push_instr(Instr::new(OpCopy {
            dst: signed_magic[1].into(),
            src: signed_hi.into(),
        }));

        let tmp = self.alloc_ssa_vec(RegFile::GPR, 2);
        self.push_instr(Instr::new(OpDAdd {
            dst: tmp.clone().into(),
            srcs: [Src::from(x), Src::from(signed_magic.clone())],
            rnd_mode: FRndMode::NearestEven,
        }));
        let dst = self.alloc_ssa_vec(RegFile::GPR, 2);
        self.push_instr(Instr::new(OpDAdd {
            dst: dst.clone().into(),
            srcs: [Src::from(tmp), Src::from(signed_magic).fneg()],
            rnd_mode: FRndMode::NearestEven,
        }));
        Ok(dst)
    }

    /// f64 floor using the 2^52 magic number technique.
    ///
    /// `floor(x) = round(x)` adjusted downward for negative non-integers:
    /// if `round(x) > x`, subtract 1.0.
    pub(super) fn emit_f64_floor(&mut self, x: SSARef) -> Result<SSARef, CompileError> {
        let rounded = self.emit_f64_round(x.clone())?;
        let one_bits: u64 = 1.0f64.to_bits();
        let one = self.alloc_ssa_vec(RegFile::GPR, 2);
        self.push_instr(Instr::new(OpCopy {
            dst: one[0].into(),
            src: Src::new_imm_u32((one_bits & 0xFFFF_FFFF) as u32),
        }));
        self.push_instr(Instr::new(OpCopy {
            dst: one[1].into(),
            src: Src::new_imm_u32(((one_bits >> 32) & 0xFFFF_FFFF) as u32),
        }));

        let pred = self.alloc_ssa(RegFile::Pred);
        self.push_instr(Instr::new(OpDSetP {
            dst: pred.into(),
            set_op: PredSetOp::And,
            cmp_op: FloatCmpOp::OrdGt,
            srcs: [Src::from(rounded.clone()), Src::from(x)],
            accum: SrcRef::True.into(),
        }));
        let adjusted = self.alloc_ssa_vec(RegFile::GPR, 2);
        self.push_instr(Instr::new(OpDAdd {
            dst: adjusted.clone().into(),
            srcs: [Src::from(rounded.clone()), Src::from(one).fneg()],
            rnd_mode: FRndMode::NearestEven,
        }));
        let dst = self.alloc_ssa_vec(RegFile::GPR, 2);
        for c in 0..2usize {
            self.push_instr(Instr::new(OpSel {
                dst: dst[c].into(),
                cond: pred.into(),
                srcs: [adjusted[c].into(), rounded[c].into()],
            }));
        }
        Ok(dst)
    }

    /// f64 ceil using floor: `ceil(x) = -floor(-x)`.
    pub(super) fn emit_f64_ceil(&mut self, x: SSARef) -> Result<SSARef, CompileError> {
        let neg_x = self.alloc_ssa_vec(RegFile::GPR, 2);
        self.push_instr(Instr::new(OpDAdd {
            dst: neg_x.clone().into(),
            srcs: [Src::ZERO, Src::from(x).fneg()],
            rnd_mode: FRndMode::NearestEven,
        }));
        let floor_neg = self.emit_f64_floor(neg_x)?;
        let dst = self.alloc_ssa_vec(RegFile::GPR, 2);
        self.push_instr(Instr::new(OpDAdd {
            dst: dst.clone().into(),
            srcs: [Src::ZERO, Src::from(floor_neg).fneg()],
            rnd_mode: FRndMode::NearestEven,
        }));
        Ok(dst)
    }

    /// Emit SM-appropriate bitwise AND (LOP3 on SM70+, LOP2 on older).
    pub(super) fn emit_logic_and(&mut self, dst: SSAValue, a: Src, b: Src) {
        if self.sm.sm() >= 70 {
            self.push_instr(Instr::new(OpLop3 {
                dst: dst.into(),
                srcs: [a, b, Src::ZERO],
                op: LogicOp3::new_lut(&|a, b, _| a & b),
            }));
        } else {
            self.push_instr(Instr::new(OpLop2 {
                dst: dst.into(),
                srcs: [a, b],
                op: LogicOp2::And,
            }));
        }
    }

    /// Emit SM-appropriate bitwise OR (LOP3 on SM70+, LOP2 on older).
    pub(super) fn emit_logic_or(&mut self, dst: SSAValue, a: Src, b: Src) {
        if self.sm.sm() >= 70 {
            self.push_instr(Instr::new(OpLop3 {
                dst: dst.into(),
                srcs: [a, b, Src::ZERO],
                op: LogicOp3::new_lut(&|a, b, _| a | b),
            }));
        } else {
            self.push_instr(Instr::new(OpLop2 {
                dst: dst.into(),
                srcs: [a, b],
                op: LogicOp2::Or,
            }));
        }
    }

    /// Scale input by `1/(2*pi)` then apply a trig transcendental (Sin or Cos).
    pub(super) fn emit_f32_trig_scaled(
        &mut self,
        src: SSAValue,
        op: TranscendentalOp,
    ) -> Result<SSARef, CompileError> {
        let scaled = self.alloc_ssa(RegFile::GPR);
        let frac_1_2pi = 1.0 / (2.0 * std::f32::consts::PI);
        self.push_instr(Instr::new(OpFMul {
            dst: scaled.into(),
            srcs: [src.into(), Src::new_imm_u32(frac_1_2pi.to_bits())],
            saturate: false,
            rnd_mode: FRndMode::NearestEven,
            ftz: false,
            dnz: false,
        }));
        let dst = self.alloc_ssa(RegFile::GPR);
        self.push_instr(Instr::new(OpTranscendental {
            dst: dst.into(),
            op,
            src: scaled.into(),
        }));
        Ok(dst.into())
    }

    /// Emit `dot(v, v)` — self-dot-product for a vector via FMul + FFma chain.
    pub(super) fn emit_f32_dot_self(&mut self, v: &SSARef) -> SSAValue {
        let comps = v.comps();
        let mut acc = self.alloc_ssa(RegFile::GPR);
        self.push_instr(Instr::new(OpFMul {
            dst: acc.into(),
            srcs: [v[0].into(), v[0].into()],
            saturate: false,
            rnd_mode: FRndMode::NearestEven,
            ftz: false,
            dnz: false,
        }));
        for c in 1..comps as usize {
            let next = self.alloc_ssa(RegFile::GPR);
            self.push_instr(Instr::new(OpFFma {
                dst: next.into(),
                srcs: [v[c].into(), v[c].into(), acc.into()],
                saturate: false,
                rnd_mode: FRndMode::NearestEven,
                ftz: false,
                dnz: false,
            }));
            acc = next;
        }
        acc
    }
}
