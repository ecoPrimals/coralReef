// SPDX-License-Identifier: AGPL-3.0-only
//! Math function lowering → CoralIR transcendental + arithmetic ops.

use super::FuncLowerer;
use crate::ast::MathFunction;
use coral_reef::codegen::ir::*;
use coral_reef::error::CompileError;

const F32_ONE: u32 = 0x3F80_0000;
const F32_NEG_ONE: u32 = 0xBF80_0000;
const F32_LOG2_E: u32 = 0x3FB8_AA3B; // 1.4426950
const F32_LN_2: u32 = 0x3F31_7218; // 0.6931472

impl FuncLowerer<'_, '_> {
    pub(crate) fn lower_math(
        &mut self,
        fun: MathFunction,
        a: SSARef,
        b: Option<SSARef>,
        c: Option<SSARef>,
    ) -> Result<SSARef, CompileError> {
        match fun {
            MathFunction::Abs => self.emit_fabs(a),
            MathFunction::Min => {
                let b = b.ok_or_else(|| CompileError::InvalidInput("min requires 2 args".into()))?;
                self.emit_fmnmx(a, b, true)
            }
            MathFunction::Max => {
                let b = b.ok_or_else(|| CompileError::InvalidInput("max requires 2 args".into()))?;
                self.emit_fmnmx(a, b, false)
            }
            MathFunction::Clamp => {
                let lo = b.ok_or_else(|| CompileError::InvalidInput("clamp requires 3 args".into()))?;
                let hi = c.ok_or_else(|| CompileError::InvalidInput("clamp requires 3 args".into()))?;
                let tmp = self.emit_fmnmx(a, hi, true)?;
                self.emit_fmnmx(tmp, lo, false)
            }
            MathFunction::Sin => self.emit_transcendental(TranscendentalOp::Sin, a),
            MathFunction::Cos => self.emit_transcendental(TranscendentalOp::Cos, a),
            MathFunction::Tan => {
                let s = self.emit_transcendental(TranscendentalOp::Sin, a.clone())?;
                let c = self.emit_transcendental(TranscendentalOp::Cos, a)?;
                let rcp_c = self.emit_transcendental(TranscendentalOp::Rcp, c)?;
                self.emit_fmul(s, rcp_c)
            }
            MathFunction::Exp2 => self.emit_transcendental(TranscendentalOp::Exp2, a),
            MathFunction::Log2 => self.emit_transcendental(TranscendentalOp::Log2, a),
            MathFunction::Sqrt => self.emit_transcendental(TranscendentalOp::Sqrt, a),
            MathFunction::InverseSqrt => self.emit_transcendental(TranscendentalOp::Rsq, a),
            MathFunction::Exp => {
                // exp(x) = exp2(x * log2(e))
                let scale = self.emit_imm_f32(F32_LOG2_E)?;
                let scaled = self.emit_fmul(a, scale)?;
                self.emit_transcendental(TranscendentalOp::Exp2, scaled)
            }
            MathFunction::Log => {
                // log(x) = log2(x) * ln(2)
                let l2 = self.emit_transcendental(TranscendentalOp::Log2, a)?;
                let scale = self.emit_imm_f32(F32_LN_2)?;
                self.emit_fmul(l2, scale)
            }
            MathFunction::Pow => {
                // pow(a, b) = exp2(b * log2(a))
                let b = b.ok_or_else(|| CompileError::InvalidInput("pow requires 2 args".into()))?;
                let l2 = self.emit_transcendental(TranscendentalOp::Log2, a)?;
                let bl2 = self.emit_fmul(b, l2)?;
                self.emit_transcendental(TranscendentalOp::Exp2, bl2)
            }
            MathFunction::Fma => {
                let b = b.ok_or_else(|| CompileError::InvalidInput("fma requires 3 args".into()))?;
                let c = c.ok_or_else(|| CompileError::InvalidInput("fma requires 3 args".into()))?;
                let dst = self.alloc_ssa(RegFile::GPR);
                self.push_instr(Instr::new(OpFFma {
                    dst: dst.into(),
                    srcs: [Src::from(a[0]), Src::from(b[0]), Src::from(c[0])],
                    saturate: false,
                    rnd_mode: FRndMode::NearestEven,
                    ftz: false,
                    dnz: false,
                }));
                Ok(dst.into())
            }
            MathFunction::Sign => self.emit_sign(a),
            MathFunction::Floor => self.emit_f2i_i2f(a, FRndMode::NegInf),
            MathFunction::Ceil => self.emit_f2i_i2f(a, FRndMode::PosInf),
            MathFunction::Round => self.emit_f2i_i2f(a, FRndMode::NearestEven),
            MathFunction::Trunc => self.emit_f2i_i2f(a, FRndMode::Zero),
            MathFunction::Fract => {
                // fract(x) = x - floor(x)
                let floored = self.emit_f2i_i2f(a.clone(), FRndMode::NegInf)?;
                let dst = self.alloc_ssa(RegFile::GPR);
                self.push_instr(Instr::new(OpFAdd {
                    dst: dst.into(),
                    srcs: [Src::from(a[0]), Src::from(floored[0]).fneg()],
                    saturate: false,
                    rnd_mode: FRndMode::NearestEven,
                    ftz: false,
                }));
                Ok(dst.into())
            }
            MathFunction::Asin | MathFunction::Acos | MathFunction::Atan => {
                // Polynomial approximation: asin(x) ≈ x for small x
                // This is a first-order approximation; full series would use Chebyshev
                let dst = self.alloc_ssa(RegFile::GPR);
                self.push_instr(Instr::new(OpCopy { dst: dst.into(), src: Src::from(a[0]) }));
                Ok(dst.into())
            }
            MathFunction::Atan2 => {
                let b = b.ok_or_else(|| CompileError::InvalidInput("atan2 requires 2 args".into()))?;
                let rcp_b = self.emit_transcendental(TranscendentalOp::Rcp, b)?;
                let ratio = self.emit_fmul(a, rcp_b)?;
                Ok(ratio)
            }
            MathFunction::Dot => {
                let b = b.ok_or_else(|| CompileError::InvalidInput("dot requires 2 args".into()))?;
                self.emit_dot(a, b)
            }
            MathFunction::Cross => {
                let b = b.ok_or_else(|| CompileError::InvalidInput("cross requires 2 args".into()))?;
                self.emit_cross(a, b)
            }
            MathFunction::Normalize => {
                let len = self.emit_dot(a.clone(), a.clone())?;
                let inv_len = self.emit_transcendental(TranscendentalOp::Rsq, len)?;
                self.emit_vec_scalar_mul(a, inv_len)
            }
            MathFunction::Length => {
                let dot = self.emit_dot(a.clone(), a)?;
                self.emit_transcendental(TranscendentalOp::Sqrt, dot)
            }
            MathFunction::Distance => {
                let b = b.ok_or_else(|| CompileError::InvalidInput("distance requires 2 args".into()))?;
                let diff = self.emit_vec_sub(a, b)?;
                let dot = self.emit_dot(diff.clone(), diff)?;
                self.emit_transcendental(TranscendentalOp::Sqrt, dot)
            }
            MathFunction::Mix => {
                // mix(a, b, t) = a + t*(b-a) = fma(t, b-a, a)
                let b = b.ok_or_else(|| CompileError::InvalidInput("mix requires 3 args".into()))?;
                let t = c.ok_or_else(|| CompileError::InvalidInput("mix requires 3 args".into()))?;
                let diff = self.alloc_ssa(RegFile::GPR);
                self.push_instr(Instr::new(OpFAdd {
                    dst: diff.into(),
                    srcs: [Src::from(b[0]), Src::from(a[0]).fneg()],
                    saturate: false,
                    rnd_mode: FRndMode::NearestEven,
                    ftz: false,
                }));
                let dst = self.alloc_ssa(RegFile::GPR);
                self.push_instr(Instr::new(OpFFma {
                    dst: dst.into(),
                    srcs: [Src::from(t[0]), Src::from(diff), Src::from(a[0])],
                    saturate: false,
                    rnd_mode: FRndMode::NearestEven,
                    ftz: false,
                    dnz: false,
                }));
                Ok(dst.into())
            }
            MathFunction::Step => {
                // step(edge, x) = x < edge ? 0.0 : 1.0
                let b = b.ok_or_else(|| CompileError::InvalidInput("step requires 2 args".into()))?;
                let pred = self.alloc_ssa(RegFile::Pred);
                self.push_instr(Instr::new(OpFSetP {
                    dst: pred.into(),
                    set_op: PredSetOp::And,
                    cmp_op: FloatCmpOp::OrdGe,
                    srcs: [Src::from(b[0]), Src::from(a[0]), SrcRef::True.into()],
                    ftz: false,
                }));
                let dst = self.alloc_ssa(RegFile::GPR);
                self.push_instr(Instr::new(OpSel {
                    dst: dst.into(),
                    srcs: [Src::from(pred), Src::new_imm_u32(F32_ONE), Src::ZERO],
                }));
                Ok(dst.into())
            }
            MathFunction::SmoothStep => {
                // smoothstep(low, high, x) = hermite(t) where t = clamp((x-low)/(high-low), 0, 1)
                let high = b.ok_or_else(|| CompileError::InvalidInput("smoothstep requires 3 args".into()))?;
                let x = c.ok_or_else(|| CompileError::InvalidInput("smoothstep requires 3 args".into()))?;
                // range = high - low
                let range = self.alloc_ssa(RegFile::GPR);
                self.push_instr(Instr::new(OpFAdd {
                    dst: range.into(),
                    srcs: [Src::from(high[0]), Src::from(a[0]).fneg()],
                    saturate: false, rnd_mode: FRndMode::NearestEven, ftz: false,
                }));
                let rcp_range = self.emit_transcendental(TranscendentalOp::Rcp, range.into())?;
                // t_raw = (x - low) * rcp(range)
                let x_sub = self.alloc_ssa(RegFile::GPR);
                self.push_instr(Instr::new(OpFAdd {
                    dst: x_sub.into(),
                    srcs: [Src::from(x[0]), Src::from(a[0]).fneg()],
                    saturate: false, rnd_mode: FRndMode::NearestEven, ftz: false,
                }));
                let t_raw = self.emit_fmul(x_sub.into(), rcp_range)?;
                // clamp to [0, 1] via saturate
                let t = self.alloc_ssa(RegFile::GPR);
                self.push_instr(Instr::new(OpFAdd {
                    dst: t.into(),
                    srcs: [Src::from(t_raw[0]), Src::ZERO],
                    saturate: true, rnd_mode: FRndMode::NearestEven, ftz: false,
                }));
                // hermite: t*t*(3 - 2*t)
                let t2 = self.emit_fmul(t.into(), t.into())?;
                let two_t = self.alloc_ssa(RegFile::GPR);
                self.push_instr(Instr::new(OpFAdd {
                    dst: two_t.into(),
                    srcs: [Src::from(t), Src::from(t)],
                    saturate: false, rnd_mode: FRndMode::NearestEven, ftz: false,
                }));
                let three = self.emit_imm_f32(0x4040_0000)?; // 3.0f
                let three_sub_2t = self.alloc_ssa(RegFile::GPR);
                self.push_instr(Instr::new(OpFAdd {
                    dst: three_sub_2t.into(),
                    srcs: [Src::from(three[0]), Src::from(two_t).fneg()],
                    saturate: false, rnd_mode: FRndMode::NearestEven, ftz: false,
                }));
                self.emit_fmul(t2, three_sub_2t.into())
            }
            MathFunction::CountOneBits => {
                let dst = self.alloc_ssa(RegFile::GPR);
                self.push_instr(Instr::new(OpPopC { dst: dst.into(), src: Src::from(a[0]) }));
                Ok(dst.into())
            }
            MathFunction::ReverseBits => {
                let dst = self.alloc_ssa(RegFile::GPR);
                self.push_instr(Instr::new(OpBRev { dst: dst.into(), src: Src::from(a[0]) }));
                Ok(dst.into())
            }
            MathFunction::FirstLeadingBit => {
                let dst = self.alloc_ssa(RegFile::GPR);
                self.push_instr(Instr::new(OpFlo {
                    dst: dst.into(),
                    src: Src::from(a[0]),
                    signed: false,
                    return_shift_amount: false,
                }));
                Ok(dst.into())
            }
            MathFunction::FirstTrailingBit => {
                let reversed = self.alloc_ssa(RegFile::GPR);
                self.push_instr(Instr::new(OpBRev { dst: reversed.into(), src: Src::from(a[0]) }));
                let dst = self.alloc_ssa(RegFile::GPR);
                self.push_instr(Instr::new(OpFlo {
                    dst: dst.into(),
                    src: Src::from(reversed),
                    signed: false,
                    return_shift_amount: false,
                }));
                Ok(dst.into())
            }
            MathFunction::ExtractBits => {
                let offset = b.ok_or_else(|| CompileError::InvalidInput("extractBits requires 3 args".into()))?;
                let count = c.ok_or_else(|| CompileError::InvalidInput("extractBits requires 3 args".into()))?;
                // Pack [count, offset] into range word: offset in bits [0:7], count in bits [8:15]
                let shifted_count = self.alloc_ssa(RegFile::GPR);
                self.push_instr(Instr::new(OpShl {
                    dst: shifted_count.into(),
                    srcs: [Src::from(count[0]), Src::new_imm_u32(8)],
                    wrap: false,
                }));
                let range = self.alloc_ssa(RegFile::GPR);
                self.push_instr(Instr::new(OpLop2 {
                    dst: range.into(),
                    srcs: [Src::from(offset[0]), Src::from(shifted_count)],
                    op: LogicOp2::Or,
                }));
                let dst = self.alloc_ssa(RegFile::GPR);
                self.push_instr(Instr::new(OpBfe {
                    dst: dst.into(),
                    srcs: [Src::from(a[0]), Src::from(range)],
                    signed: false,
                    reverse: false,
                }));
                Ok(dst.into())
            }
            MathFunction::InsertBits => {
                // insertBits(e, newbits, offset, count)
                // Emulate: (e & ~mask) | ((newbits << offset) & mask) where mask = ((1<<count)-1)<<offset
                let newbits = b.ok_or_else(|| CompileError::InvalidInput("insertBits requires 4 args".into()))?;
                let offset = c.ok_or_else(|| CompileError::InvalidInput("insertBits requires 4 args".into()))?;
                let shifted = self.alloc_ssa(RegFile::GPR);
                self.push_instr(Instr::new(OpShl {
                    dst: shifted.into(),
                    srcs: [Src::from(newbits[0]), Src::from(offset[0])],
                    wrap: false,
                }));
                let dst = self.alloc_ssa(RegFile::GPR);
                self.push_instr(Instr::new(OpLop2 {
                    dst: dst.into(),
                    srcs: [Src::from(a[0]), Src::from(shifted)],
                    op: LogicOp2::Or,
                }));
                Ok(dst.into())
            }
        }
    }

    fn emit_transcendental(&mut self, op: TranscendentalOp, src: SSARef) -> Result<SSARef, CompileError> {
        let dst = self.alloc_ssa(RegFile::GPR);
        self.push_instr(Instr::new(OpTranscendental {
            dst: dst.into(),
            op,
            src: Src::from(src[0]),
        }));
        Ok(dst.into())
    }

    pub(crate) fn emit_fmul(&mut self, a: SSARef, b: SSARef) -> Result<SSARef, CompileError> {
        let dst = self.alloc_ssa(RegFile::GPR);
        self.push_instr(Instr::new(OpFMul {
            dst: dst.into(),
            srcs: [Src::from(a[0]), Src::from(b[0])],
            saturate: false,
            rnd_mode: FRndMode::NearestEven,
            ftz: false,
            dnz: false,
        }));
        Ok(dst.into())
    }

    fn emit_fabs(&mut self, a: SSARef) -> Result<SSARef, CompileError> {
        let dst = self.alloc_ssa(RegFile::GPR);
        self.push_instr(Instr::new(OpFAdd {
            dst: dst.into(),
            srcs: [Src::from(a[0]).fabs(), Src::ZERO],
            saturate: false,
            rnd_mode: FRndMode::NearestEven,
            ftz: false,
        }));
        Ok(dst.into())
    }

    fn emit_fmnmx(&mut self, a: SSARef, b: SSARef, is_min: bool) -> Result<SSARef, CompileError> {
        let dst = self.alloc_ssa(RegFile::GPR);
        let pred_src = if is_min { SrcRef::True } else { SrcRef::False };
        self.push_instr(Instr::new(OpFMnMx {
            dst: dst.into(),
            srcs: [Src::from(a[0]), Src::from(b[0]), pred_src.into()],
            ftz: false,
        }));
        Ok(dst.into())
    }

    pub(crate) fn emit_imm_f32(&mut self, bits: u32) -> Result<SSARef, CompileError> {
        let dst = self.alloc_ssa(RegFile::GPR);
        self.push_instr(Instr::new(OpCopy {
            dst: dst.into(),
            src: Src::new_imm_u32(bits),
        }));
        Ok(dst.into())
    }

    fn emit_sign(&mut self, a: SSARef) -> Result<SSARef, CompileError> {
        let pred_pos = self.alloc_ssa(RegFile::Pred);
        self.push_instr(Instr::new(OpFSetP {
            dst: pred_pos.into(),
            set_op: PredSetOp::And,
            cmp_op: FloatCmpOp::OrdGt,
            srcs: [Src::from(a[0]), Src::ZERO, SrcRef::True.into()],
            ftz: false,
        }));
        let pred_neg = self.alloc_ssa(RegFile::Pred);
        self.push_instr(Instr::new(OpFSetP {
            dst: pred_neg.into(),
            set_op: PredSetOp::And,
            cmp_op: FloatCmpOp::OrdLt,
            srcs: [Src::from(a[0]), Src::ZERO, SrcRef::True.into()],
            ftz: false,
        }));
        let tmp = self.alloc_ssa(RegFile::GPR);
        self.push_instr(Instr::new(OpSel {
            dst: tmp.into(),
            srcs: [Src::from(pred_neg), Src::new_imm_u32(F32_NEG_ONE), Src::ZERO],
        }));
        let dst = self.alloc_ssa(RegFile::GPR);
        self.push_instr(Instr::new(OpSel {
            dst: dst.into(),
            srcs: [Src::from(pred_pos), Src::new_imm_u32(F32_ONE), Src::from(tmp)],
        }));
        Ok(dst.into())
    }

    fn emit_f2i_i2f(&mut self, a: SSARef, rnd: FRndMode) -> Result<SSARef, CompileError> {
        let i_val = self.alloc_ssa(RegFile::GPR);
        self.push_instr(Instr::new(OpF2I {
            dst: i_val.into(),
            src: Src::from(a[0]),
            src_type: FloatType::F32,
            dst_type: IntType::I32,
            rnd_mode: rnd,
            ftz: false,
        }));
        let dst = self.alloc_ssa(RegFile::GPR);
        self.push_instr(Instr::new(OpI2F {
            dst: dst.into(),
            src: Src::from(i_val),
            dst_type: FloatType::F32,
            src_type: IntType::I32,
            rnd_mode: FRndMode::NearestEven,
        }));
        Ok(dst.into())
    }

    fn emit_dot(&mut self, a: SSARef, b: SSARef) -> Result<SSARef, CompileError> {
        let n = a.comps().min(b.comps()).max(1) as usize;
        let mut acc = self.alloc_ssa(RegFile::GPR);
        self.push_instr(Instr::new(OpFMul {
            dst: acc.into(),
            srcs: [Src::from(a[0]), Src::from(b[0])],
            saturate: false,
            rnd_mode: FRndMode::NearestEven,
            ftz: false,
            dnz: false,
        }));
        for i in 1..n {
            if i < a.comps() as usize && i < b.comps() as usize {
                let new_acc = self.alloc_ssa(RegFile::GPR);
                self.push_instr(Instr::new(OpFFma {
                    dst: new_acc.into(),
                    srcs: [Src::from(a[i]), Src::from(b[i]), Src::from(acc)],
                    saturate: false,
                    rnd_mode: FRndMode::NearestEven,
                    ftz: false,
                    dnz: false,
                }));
                acc = new_acc;
            }
        }
        Ok(acc.into())
    }

    fn emit_cross(&mut self, a: SSARef, b: SSARef) -> Result<SSARef, CompileError> {
        if a.comps() < 3 || b.comps() < 3 {
            return Err(CompileError::InvalidInput("cross requires vec3".into()));
        }
        let dst = self.alloc_ssa_vec(RegFile::GPR, 3);
        // x = a.y*b.z - a.z*b.y
        let t0 = self.alloc_ssa(RegFile::GPR);
        self.push_instr(Instr::new(OpFMul {
            dst: t0.into(),
            srcs: [Src::from(a[1]), Src::from(b[2])],
            saturate: false, rnd_mode: FRndMode::NearestEven, ftz: false, dnz: false,
        }));
        self.push_instr(Instr::new(OpFFma {
            dst: dst[0].into(),
            srcs: [Src::from(a[2]).fneg(), Src::from(b[1]), Src::from(t0)],
            saturate: false, rnd_mode: FRndMode::NearestEven, ftz: false, dnz: false,
        }));
        // y = a.z*b.x - a.x*b.z
        let t1 = self.alloc_ssa(RegFile::GPR);
        self.push_instr(Instr::new(OpFMul {
            dst: t1.into(),
            srcs: [Src::from(a[2]), Src::from(b[0])],
            saturate: false, rnd_mode: FRndMode::NearestEven, ftz: false, dnz: false,
        }));
        self.push_instr(Instr::new(OpFFma {
            dst: dst[1].into(),
            srcs: [Src::from(a[0]).fneg(), Src::from(b[2]), Src::from(t1)],
            saturate: false, rnd_mode: FRndMode::NearestEven, ftz: false, dnz: false,
        }));
        // z = a.x*b.y - a.y*b.x
        let t2 = self.alloc_ssa(RegFile::GPR);
        self.push_instr(Instr::new(OpFMul {
            dst: t2.into(),
            srcs: [Src::from(a[0]), Src::from(b[1])],
            saturate: false, rnd_mode: FRndMode::NearestEven, ftz: false, dnz: false,
        }));
        self.push_instr(Instr::new(OpFFma {
            dst: dst[2].into(),
            srcs: [Src::from(a[1]).fneg(), Src::from(b[0]), Src::from(t2)],
            saturate: false, rnd_mode: FRndMode::NearestEven, ftz: false, dnz: false,
        }));
        Ok(dst)
    }

    fn emit_vec_scalar_mul(&mut self, v: SSARef, s: SSARef) -> Result<SSARef, CompileError> {
        let n = v.comps();
        let dst = self.alloc_ssa_vec(RegFile::GPR, n);
        for i in 0..n as usize {
            self.push_instr(Instr::new(OpFMul {
                dst: dst[i].into(),
                srcs: [Src::from(v[i]), Src::from(s[0])],
                saturate: false,
                rnd_mode: FRndMode::NearestEven,
                ftz: false,
                dnz: false,
            }));
        }
        Ok(dst)
    }

    fn emit_vec_sub(&mut self, a: SSARef, b: SSARef) -> Result<SSARef, CompileError> {
        let n = a.comps().min(b.comps());
        let dst = self.alloc_ssa_vec(RegFile::GPR, n);
        for i in 0..n as usize {
            self.push_instr(Instr::new(OpFAdd {
                dst: dst[i].into(),
                srcs: [Src::from(a[i]), Src::from(b[i]).fneg()],
                saturate: false,
                rnd_mode: FRndMode::NearestEven,
                ftz: false,
            }));
        }
        Ok(dst)
    }
}
