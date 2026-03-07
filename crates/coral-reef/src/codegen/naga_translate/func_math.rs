// SPDX-License-Identifier: AGPL-3.0-only
#![allow(clippy::wildcard_imports)]
use super::super::ir::*;
use super::func::FuncTranslator;
use crate::error::CompileError;
use naga::Handle;

impl<'a, 'b> FuncTranslator<'a, 'b> {
    pub(super) fn translate_math(
        &mut self,
        fun: naga::MathFunction,
        a: SSARef,
        b: Option<SSARef>,
        c: Option<SSARef>,
        arg_handle: Handle<naga::Expression>,
    ) -> Result<SSARef, CompileError> {
        let is_f64 = self.is_f64_expr(arg_handle);
        match fun {
            naga::MathFunction::Abs => {
                let dst = self.alloc_ssa(RegFile::GPR);
                self.push_instr(Instr::new(OpFAdd {
                    dst: dst.into(),
                    srcs: [Src::ZERO, Src::from(a[0]).fabs()],
                    saturate: false,
                    rnd_mode: FRndMode::NearestEven,
                    ftz: false,
                }));
                Ok(dst.into())
            }
            naga::MathFunction::Min => {
                let b =
                    b.ok_or_else(|| CompileError::InvalidInput("min requires 2 args".into()))?;
                let dst = self.alloc_ssa(RegFile::GPR);
                self.push_instr(Instr::new(OpFMnMx {
                    dst: dst.into(),
                    srcs: [a[0].into(), b[0].into()],
                    min: SrcRef::True.into(),
                    ftz: false,
                }));
                Ok(dst.into())
            }
            naga::MathFunction::Max => {
                let b =
                    b.ok_or_else(|| CompileError::InvalidInput("max requires 2 args".into()))?;
                let dst = self.alloc_ssa(RegFile::GPR);
                self.push_instr(Instr::new(OpFMnMx {
                    dst: dst.into(),
                    srcs: [a[0].into(), b[0].into()],
                    min: SrcRef::False.into(),
                    ftz: false,
                }));
                Ok(dst.into())
            }
            naga::MathFunction::Clamp => {
                let b =
                    b.ok_or_else(|| CompileError::InvalidInput("clamp requires 3 args".into()))?;
                let c =
                    c.ok_or_else(|| CompileError::InvalidInput("clamp requires 3 args".into()))?;
                let tmp = self.alloc_ssa(RegFile::GPR);
                self.push_instr(Instr::new(OpFMnMx {
                    dst: tmp.into(),
                    srcs: [a[0].into(), c[0].into()],
                    min: SrcRef::True.into(),
                    ftz: false,
                }));
                let dst = self.alloc_ssa(RegFile::GPR);
                self.push_instr(Instr::new(OpFMnMx {
                    dst: dst.into(),
                    srcs: [tmp.into(), b[0].into()],
                    min: SrcRef::False.into(),
                    ftz: false,
                }));
                Ok(dst.into())
            }
            naga::MathFunction::Floor => {
                if is_f64 {
                    self.emit_f64_floor(a)
                } else {
                    let dst = self.alloc_ssa(RegFile::GPR);
                    self.push_instr(Instr::new(OpFRnd {
                        dst: dst.into(),
                        src: a[0].into(),
                        dst_type: FloatType::F32,
                        src_type: FloatType::F32,
                        rnd_mode: FRndMode::NegInf,
                        ftz: false,
                    }));
                    Ok(dst.into())
                }
            }
            naga::MathFunction::Ceil => {
                if is_f64 {
                    self.emit_f64_ceil(a)
                } else {
                    let dst = self.alloc_ssa(RegFile::GPR);
                    self.push_instr(Instr::new(OpFRnd {
                        dst: dst.into(),
                        src: a[0].into(),
                        dst_type: FloatType::F32,
                        src_type: FloatType::F32,
                        rnd_mode: FRndMode::PosInf,
                        ftz: false,
                    }));
                    Ok(dst.into())
                }
            }
            naga::MathFunction::Round => {
                if is_f64 {
                    self.emit_f64_round(a)
                } else {
                    let dst = self.alloc_ssa(RegFile::GPR);
                    self.push_instr(Instr::new(OpFRnd {
                        dst: dst.into(),
                        src: a[0].into(),
                        dst_type: FloatType::F32,
                        src_type: FloatType::F32,
                        rnd_mode: FRndMode::NearestEven,
                        ftz: false,
                    }));
                    Ok(dst.into())
                }
            }
            naga::MathFunction::Sqrt => {
                if is_f64 {
                    let dst = self.alloc_ssa_vec(RegFile::GPR, 2);
                    self.push_instr(Instr::new(OpF64Sqrt {
                        dst: dst.clone().into(),
                        src: Src::from(a),
                    }));
                    Ok(dst)
                } else {
                    let rsq = self.alloc_ssa(RegFile::GPR);
                    self.push_instr(Instr::new(OpTranscendental {
                        dst: rsq.into(),
                        op: TranscendentalOp::Rsq,
                        src: a[0].into(),
                    }));
                    let rcp_rsq = self.alloc_ssa(RegFile::GPR);
                    self.push_instr(Instr::new(OpTranscendental {
                        dst: rcp_rsq.into(),
                        op: TranscendentalOp::Rcp,
                        src: rsq.into(),
                    }));
                    Ok(rcp_rsq.into())
                }
            }
            naga::MathFunction::InverseSqrt => {
                if is_f64 {
                    let dst = self.alloc_ssa_vec(RegFile::GPR, 2);
                    self.push_instr(Instr::new(OpF64Rcp {
                        dst: dst.clone().into(),
                        src: Src::from(a),
                    }));
                    Ok(dst)
                } else {
                    let dst = self.alloc_ssa(RegFile::GPR);
                    self.push_instr(Instr::new(OpTranscendental {
                        dst: dst.into(),
                        op: TranscendentalOp::Rsq,
                        src: a[0].into(),
                    }));
                    Ok(dst.into())
                }
            }
            naga::MathFunction::Sin => {
                let scaled = self.alloc_ssa(RegFile::GPR);
                let frac_1_2pi = 1.0 / (2.0 * std::f32::consts::PI);
                self.push_instr(Instr::new(OpFMul {
                    dst: scaled.into(),
                    srcs: [a[0].into(), Src::new_imm_u32(frac_1_2pi.to_bits())],
                    saturate: false,
                    rnd_mode: FRndMode::NearestEven,
                    ftz: false,
                    dnz: false,
                }));
                let dst = self.alloc_ssa(RegFile::GPR);
                self.push_instr(Instr::new(OpTranscendental {
                    dst: dst.into(),
                    op: TranscendentalOp::Sin,
                    src: scaled.into(),
                }));
                Ok(dst.into())
            }
            naga::MathFunction::Cos => {
                let scaled = self.alloc_ssa(RegFile::GPR);
                let frac_1_2pi = 1.0 / (2.0 * std::f32::consts::PI);
                self.push_instr(Instr::new(OpFMul {
                    dst: scaled.into(),
                    srcs: [a[0].into(), Src::new_imm_u32(frac_1_2pi.to_bits())],
                    saturate: false,
                    rnd_mode: FRndMode::NearestEven,
                    ftz: false,
                    dnz: false,
                }));
                let dst = self.alloc_ssa(RegFile::GPR);
                self.push_instr(Instr::new(OpTranscendental {
                    dst: dst.into(),
                    op: TranscendentalOp::Cos,
                    src: scaled.into(),
                }));
                Ok(dst.into())
            }
            naga::MathFunction::Exp2 => {
                if is_f64 {
                    let dst = self.alloc_ssa_vec(RegFile::GPR, 2);
                    self.push_instr(Instr::new(OpF64Exp2 {
                        dst: dst.clone().into(),
                        src: Src::from(a),
                    }));
                    Ok(dst)
                } else {
                    let dst = self.alloc_ssa(RegFile::GPR);
                    self.push_instr(Instr::new(OpTranscendental {
                        dst: dst.into(),
                        op: TranscendentalOp::Exp2,
                        src: a[0].into(),
                    }));
                    Ok(dst.into())
                }
            }
            naga::MathFunction::Log2 => {
                if is_f64 {
                    let dst = self.alloc_ssa_vec(RegFile::GPR, 2);
                    self.push_instr(Instr::new(OpF64Log2 {
                        dst: dst.clone().into(),
                        src: Src::from(a),
                    }));
                    Ok(dst)
                } else {
                    let dst = self.alloc_ssa(RegFile::GPR);
                    self.push_instr(Instr::new(OpTranscendental {
                        dst: dst.into(),
                        op: TranscendentalOp::Log2,
                        src: a[0].into(),
                    }));
                    Ok(dst.into())
                }
            }
            naga::MathFunction::Exp => {
                if is_f64 {
                    // exp(x) = exp2(x * log2(e))
                    let log2_e = std::f64::consts::LOG2_E;
                    let log2_e_bits = log2_e.to_bits();
                    let lo = (log2_e_bits & 0xFFFF_FFFF) as u32;
                    let hi = ((log2_e_bits >> 32) & 0xFFFF_FFFF) as u32;
                    let scale = self.alloc_ssa_vec(RegFile::GPR, 2);
                    self.push_instr(Instr::new(OpCopy {
                        dst: scale[0].into(),
                        src: Src::new_imm_u32(lo),
                    }));
                    self.push_instr(Instr::new(OpCopy {
                        dst: scale[1].into(),
                        src: Src::new_imm_u32(hi),
                    }));
                    let scaled = self.alloc_ssa_vec(RegFile::GPR, 2);
                    self.push_instr(Instr::new(OpDMul {
                        dst: scaled.clone().into(),
                        srcs: [Src::from(a), Src::from(scale)],
                        rnd_mode: FRndMode::NearestEven,
                    }));
                    let dst = self.alloc_ssa_vec(RegFile::GPR, 2);
                    self.push_instr(Instr::new(OpF64Exp2 {
                        dst: dst.clone().into(),
                        src: Src::from(scaled),
                    }));
                    Ok(dst)
                } else {
                    let log2_e: f32 = std::f32::consts::LOG2_E;
                    let scaled = self.alloc_ssa(RegFile::GPR);
                    self.push_instr(Instr::new(OpFMul {
                        dst: scaled.into(),
                        srcs: [a[0].into(), Src::new_imm_u32(log2_e.to_bits())],
                        saturate: false,
                        rnd_mode: FRndMode::NearestEven,
                        ftz: false,
                        dnz: false,
                    }));
                    let dst = self.alloc_ssa(RegFile::GPR);
                    self.push_instr(Instr::new(OpTranscendental {
                        dst: dst.into(),
                        op: TranscendentalOp::Exp2,
                        src: scaled.into(),
                    }));
                    Ok(dst.into())
                }
            }
            naga::MathFunction::Log => {
                if is_f64 {
                    // ln(x) = log2(x) * ln(2)
                    let ln2 = std::f64::consts::LN_2;
                    let ln2_bits = ln2.to_bits();
                    let lo = (ln2_bits & 0xFFFF_FFFF) as u32;
                    let hi = ((ln2_bits >> 32) & 0xFFFF_FFFF) as u32;
                    let scale = self.alloc_ssa_vec(RegFile::GPR, 2);
                    self.push_instr(Instr::new(OpCopy {
                        dst: scale[0].into(),
                        src: Src::new_imm_u32(lo),
                    }));
                    self.push_instr(Instr::new(OpCopy {
                        dst: scale[1].into(),
                        src: Src::new_imm_u32(hi),
                    }));
                    let log2_val = self.alloc_ssa_vec(RegFile::GPR, 2);
                    self.push_instr(Instr::new(OpF64Log2 {
                        dst: log2_val.clone().into(),
                        src: Src::from(a),
                    }));
                    let dst = self.alloc_ssa_vec(RegFile::GPR, 2);
                    self.push_instr(Instr::new(OpDMul {
                        dst: dst.clone().into(),
                        srcs: [Src::from(log2_val), Src::from(scale)],
                        rnd_mode: FRndMode::NearestEven,
                    }));
                    Ok(dst)
                } else {
                    let log2_val = self.alloc_ssa(RegFile::GPR);
                    self.push_instr(Instr::new(OpTranscendental {
                        dst: log2_val.into(),
                        op: TranscendentalOp::Log2,
                        src: a[0].into(),
                    }));
                    let ln2: f32 = std::f32::consts::LN_2;
                    let dst = self.alloc_ssa(RegFile::GPR);
                    self.push_instr(Instr::new(OpFMul {
                        dst: dst.into(),
                        srcs: [log2_val.into(), Src::new_imm_u32(ln2.to_bits())],
                        saturate: false,
                        rnd_mode: FRndMode::NearestEven,
                        ftz: false,
                        dnz: false,
                    }));
                    Ok(dst.into())
                }
            }
            naga::MathFunction::Fma => {
                let b =
                    b.ok_or_else(|| CompileError::InvalidInput("fma requires 3 args".into()))?;
                let c =
                    c.ok_or_else(|| CompileError::InvalidInput("fma requires 3 args".into()))?;
                let dst = self.alloc_ssa(RegFile::GPR);
                self.push_instr(Instr::new(OpFFma {
                    dst: dst.into(),
                    srcs: [a[0].into(), b[0].into(), c[0].into()],
                    saturate: false,
                    rnd_mode: FRndMode::NearestEven,
                    ftz: false,
                    dnz: false,
                }));
                Ok(dst.into())
            }
            naga::MathFunction::Pow => {
                let b = b.ok_or_else(|| {
                    CompileError::InvalidInput("Pow requires two arguments".into())
                })?;
                if is_f64 {
                    let log_x = self.alloc_ssa_vec(RegFile::GPR, 2);
                    self.push_instr(Instr::new(OpF64Log2 {
                        dst: log_x.clone().into(),
                        src: Src::from(a),
                    }));
                    let y_log_x = self.alloc_ssa_vec(RegFile::GPR, 2);
                    self.push_instr(Instr::new(OpDMul {
                        dst: y_log_x.clone().into(),
                        srcs: [Src::from(b), Src::from(log_x)],
                        rnd_mode: FRndMode::NearestEven,
                    }));
                    let dst = self.alloc_ssa_vec(RegFile::GPR, 2);
                    self.push_instr(Instr::new(OpF64Exp2 {
                        dst: dst.clone().into(),
                        src: Src::from(y_log_x),
                    }));
                    Ok(dst)
                } else {
                    let log_x = self.alloc_ssa(RegFile::GPR);
                    self.push_instr(Instr::new(OpTranscendental {
                        dst: log_x.into(),
                        op: TranscendentalOp::Log2,
                        src: a[0].into(),
                    }));
                    let y_log_x = self.alloc_ssa(RegFile::GPR);
                    self.push_instr(Instr::new(OpFMul {
                        dst: y_log_x.into(),
                        srcs: [b[0].into(), log_x.into()],
                        saturate: false,
                        rnd_mode: FRndMode::NearestEven,
                        ftz: false,
                        dnz: false,
                    }));
                    let dst = self.alloc_ssa(RegFile::GPR);
                    self.push_instr(Instr::new(OpTranscendental {
                        dst: dst.into(),
                        op: TranscendentalOp::Exp2,
                        src: y_log_x.into(),
                    }));
                    Ok(dst.into())
                }
            }
            _ => Err(CompileError::NotImplemented(format!(
                "math function {fun:?} not yet supported"
            ))),
        }
    }

    /// f64 round-to-nearest-even using the 2^52 magic number technique.
    /// result = (x + copysign(MAGIC, x)) - copysign(MAGIC, x)
    fn emit_f64_round(&mut self, x: SSARef) -> Result<SSARef, CompileError> {
        const MAGIC_HI: u32 = 0x4330_0000; // 2^52 high word

        let sign_bit = self.alloc_ssa(RegFile::GPR);
        if self.sm.sm() >= 70 {
            self.push_instr(Instr::new(OpLop3 {
                dst: sign_bit.into(),
                srcs: [x[1].into(), Src::new_imm_u32(0x8000_0000), Src::ZERO],
                op: LogicOp3::new_lut(&|a, b, _| a & b),
            }));
        } else {
            self.push_instr(Instr::new(OpLop2 {
                dst: sign_bit.into(),
                srcs: [x[1].into(), Src::new_imm_u32(0x8000_0000)],
                op: LogicOp2::And,
            }));
        }
        let signed_hi = self.alloc_ssa(RegFile::GPR);
        if self.sm.sm() >= 70 {
            self.push_instr(Instr::new(OpLop3 {
                dst: signed_hi.into(),
                srcs: [Src::new_imm_u32(MAGIC_HI), sign_bit.into(), Src::ZERO],
                op: LogicOp3::new_lut(&|a, b, _| a | b),
            }));
        } else {
            self.push_instr(Instr::new(OpLop2 {
                dst: signed_hi.into(),
                srcs: [Src::new_imm_u32(MAGIC_HI), sign_bit.into()],
                op: LogicOp2::Or,
            }));
        }
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
    /// floor(x) = round(x) adjusted for negative non-integers.
    pub(super) fn emit_f64_floor(&mut self, x: SSARef) -> Result<SSARef, CompileError> {
        let rounded = self.emit_f64_round(x.clone())?;
        // If rounded > x, subtract 1.0 (floor adjusts downward for negatives)
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
        // cmp: rounded > x
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
        // Select: if rounded > x, use adjusted; else use rounded
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

    /// f64 ceil using floor: ceil(x) = -floor(-x)
    fn emit_f64_ceil(&mut self, x: SSARef) -> Result<SSARef, CompileError> {
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
}
