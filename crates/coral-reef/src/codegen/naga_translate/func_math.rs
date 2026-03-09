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
            naga::MathFunction::Sin => self.emit_f32_trig_scaled(a[0], TranscendentalOp::Sin),
            naga::MathFunction::Cos => self.emit_f32_trig_scaled(a[0], TranscendentalOp::Cos),
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
            naga::MathFunction::Tan => {
                if is_f64 {
                    let sin_val = self.alloc_ssa_vec(RegFile::GPR, 2);
                    self.push_instr(Instr::new(OpF64Sin {
                        dst: sin_val.clone().into(),
                        src: Src::from(a.clone()),
                    }));
                    let cos_val = self.alloc_ssa_vec(RegFile::GPR, 2);
                    self.push_instr(Instr::new(OpF64Cos {
                        dst: cos_val.clone().into(),
                        src: Src::from(a),
                    }));
                    let rcp_cos = self.alloc_ssa_vec(RegFile::GPR, 2);
                    self.push_instr(Instr::new(OpF64Rcp {
                        dst: rcp_cos.clone().into(),
                        src: Src::from(cos_val),
                    }));
                    let dst = self.alloc_ssa_vec(RegFile::GPR, 2);
                    self.push_instr(Instr::new(OpDMul {
                        dst: dst.clone().into(),
                        srcs: [Src::from(sin_val), Src::from(rcp_cos)],
                        rnd_mode: FRndMode::NearestEven,
                    }));
                    Ok(dst)
                } else {
                    let frac_1_2pi = 1.0 / (2.0 * std::f32::consts::PI);
                    let scaled = self.alloc_ssa(RegFile::GPR);
                    self.push_instr(Instr::new(OpFMul {
                        dst: scaled.into(),
                        srcs: [a[0].into(), Src::new_imm_u32(frac_1_2pi.to_bits())],
                        saturate: false,
                        rnd_mode: FRndMode::NearestEven,
                        ftz: false,
                        dnz: false,
                    }));
                    let sin_val = self.alloc_ssa(RegFile::GPR);
                    self.push_instr(Instr::new(OpTranscendental {
                        dst: sin_val.into(),
                        op: TranscendentalOp::Sin,
                        src: scaled.into(),
                    }));
                    let cos_val = self.alloc_ssa(RegFile::GPR);
                    self.push_instr(Instr::new(OpTranscendental {
                        dst: cos_val.into(),
                        op: TranscendentalOp::Cos,
                        src: scaled.into(),
                    }));
                    let rcp_cos = self.alloc_ssa(RegFile::GPR);
                    self.push_instr(Instr::new(OpTranscendental {
                        dst: rcp_cos.into(),
                        op: TranscendentalOp::Rcp,
                        src: cos_val.into(),
                    }));
                    let dst = self.alloc_ssa(RegFile::GPR);
                    self.push_instr(Instr::new(OpFMul {
                        dst: dst.into(),
                        srcs: [sin_val.into(), rcp_cos.into()],
                        saturate: false,
                        rnd_mode: FRndMode::NearestEven,
                        ftz: false,
                        dnz: false,
                    }));
                    Ok(dst.into())
                }
            }
            naga::MathFunction::CountOneBits => {
                let comps = a.comps();
                let dst = self.alloc_ssa_vec(RegFile::GPR, comps);
                for c in 0..comps as usize {
                    self.push_instr(Instr::new(OpPopC {
                        dst: dst[c].into(),
                        src: a[c].into(),
                    }));
                }
                Ok(dst)
            }
            naga::MathFunction::ReverseBits => {
                let comps = a.comps();
                let dst = self.alloc_ssa_vec(RegFile::GPR, comps);
                for c in 0..comps as usize {
                    self.push_instr(Instr::new(OpBRev {
                        dst: dst[c].into(),
                        src: a[c].into(),
                    }));
                }
                Ok(dst)
            }
            naga::MathFunction::FirstLeadingBit => {
                let signed = self.is_signed_int_expr(arg_handle);
                let comps = a.comps();
                let dst = self.alloc_ssa_vec(RegFile::GPR, comps);
                for c in 0..comps as usize {
                    self.push_instr(Instr::new(OpFlo {
                        dst: dst[c].into(),
                        src: a[c].into(),
                        signed,
                        return_shift_amount: false,
                    }));
                }
                Ok(dst)
            }
            naga::MathFunction::CountLeadingZeros => {
                let comps = a.comps();
                let dst = self.alloc_ssa_vec(RegFile::GPR, comps);
                for c in 0..comps as usize {
                    self.push_instr(Instr::new(OpFlo {
                        dst: dst[c].into(),
                        src: a[c].into(),
                        signed: false,
                        return_shift_amount: true,
                    }));
                }
                Ok(dst)
            }
            naga::MathFunction::Tanh => {
                if is_f64 {
                    let sin_val = self.alloc_ssa_vec(RegFile::GPR, 2);
                    self.push_instr(Instr::new(OpF64Sin {
                        dst: sin_val.clone().into(),
                        src: Src::from(a.clone()),
                    }));
                    let cos_val = self.alloc_ssa_vec(RegFile::GPR, 2);
                    self.push_instr(Instr::new(OpF64Cos {
                        dst: cos_val.clone().into(),
                        src: Src::from(a),
                    }));
                    let rcp_cos = self.alloc_ssa_vec(RegFile::GPR, 2);
                    self.push_instr(Instr::new(OpF64Rcp {
                        dst: rcp_cos.clone().into(),
                        src: Src::from(cos_val),
                    }));
                    let tan_val = self.alloc_ssa_vec(RegFile::GPR, 2);
                    self.push_instr(Instr::new(OpDMul {
                        dst: tan_val.clone().into(),
                        srcs: [Src::from(sin_val), Src::from(rcp_cos)],
                        rnd_mode: FRndMode::NearestEven,
                    }));
                    Ok(tan_val)
                } else {
                    // tanh(x) = (exp(2x) - 1) / (exp(2x) + 1)
                    let two_x = self.alloc_ssa(RegFile::GPR);
                    self.push_instr(Instr::new(OpFAdd {
                        dst: two_x.into(),
                        srcs: [a[0].into(), a[0].into()],
                        saturate: false,
                        rnd_mode: FRndMode::NearestEven,
                        ftz: false,
                    }));
                    let log2_e: f32 = std::f32::consts::LOG2_E;
                    let scaled = self.alloc_ssa(RegFile::GPR);
                    self.push_instr(Instr::new(OpFMul {
                        dst: scaled.into(),
                        srcs: [two_x.into(), Src::new_imm_u32(log2_e.to_bits())],
                        saturate: false,
                        rnd_mode: FRndMode::NearestEven,
                        ftz: false,
                        dnz: false,
                    }));
                    let exp2x = self.alloc_ssa(RegFile::GPR);
                    self.push_instr(Instr::new(OpTranscendental {
                        dst: exp2x.into(),
                        op: TranscendentalOp::Exp2,
                        src: scaled.into(),
                    }));
                    // exp2x - 1
                    let num = self.alloc_ssa(RegFile::GPR);
                    let neg_one: f32 = -1.0;
                    self.push_instr(Instr::new(OpFAdd {
                        dst: num.into(),
                        srcs: [exp2x.into(), Src::new_imm_u32(neg_one.to_bits())],
                        saturate: false,
                        rnd_mode: FRndMode::NearestEven,
                        ftz: false,
                    }));
                    // exp2x + 1
                    let den = self.alloc_ssa(RegFile::GPR);
                    let one: f32 = 1.0;
                    self.push_instr(Instr::new(OpFAdd {
                        dst: den.into(),
                        srcs: [exp2x.into(), Src::new_imm_u32(one.to_bits())],
                        saturate: false,
                        rnd_mode: FRndMode::NearestEven,
                        ftz: false,
                    }));
                    let rcp_den = self.alloc_ssa(RegFile::GPR);
                    self.push_instr(Instr::new(OpTranscendental {
                        dst: rcp_den.into(),
                        op: TranscendentalOp::Rcp,
                        src: den.into(),
                    }));
                    let dst = self.alloc_ssa(RegFile::GPR);
                    self.push_instr(Instr::new(OpFMul {
                        dst: dst.into(),
                        srcs: [num.into(), rcp_den.into()],
                        saturate: false,
                        rnd_mode: FRndMode::NearestEven,
                        ftz: false,
                        dnz: false,
                    }));
                    Ok(dst.into())
                }
            }
            naga::MathFunction::Fract => {
                if is_f64 {
                    let floored = self.emit_f64_floor(a.clone())?;
                    let dst = self.alloc_ssa_vec(RegFile::GPR, 2);
                    self.push_instr(Instr::new(OpDAdd {
                        dst: dst.clone().into(),
                        srcs: [Src::from(a), Src::from(floored).fneg()],
                        rnd_mode: FRndMode::NearestEven,
                    }));
                    Ok(dst)
                } else {
                    let floored = self.alloc_ssa(RegFile::GPR);
                    self.push_instr(Instr::new(OpFRnd {
                        dst: floored.into(),
                        src: a[0].into(),
                        dst_type: FloatType::F32,
                        src_type: FloatType::F32,
                        rnd_mode: FRndMode::NegInf,
                        ftz: false,
                    }));
                    let dst = self.alloc_ssa(RegFile::GPR);
                    self.push_instr(Instr::new(OpFAdd {
                        dst: dst.into(),
                        srcs: [a[0].into(), Src::from(floored).fneg()],
                        saturate: false,
                        rnd_mode: FRndMode::NearestEven,
                        ftz: false,
                    }));
                    Ok(dst.into())
                }
            }
            naga::MathFunction::Sign => {
                let dst = self.alloc_ssa(RegFile::GPR);
                // x > 0 → 1.0
                let pos = self.alloc_ssa(RegFile::Pred);
                self.push_instr(Instr::new(OpFSetP {
                    dst: pos.into(),
                    set_op: PredSetOp::And,
                    cmp_op: FloatCmpOp::OrdGt,
                    srcs: [a[0].into(), Src::ZERO],
                    accum: SrcRef::True.into(),
                    ftz: false,
                }));
                let neg = self.alloc_ssa(RegFile::Pred);
                self.push_instr(Instr::new(OpFSetP {
                    dst: neg.into(),
                    set_op: PredSetOp::And,
                    cmp_op: FloatCmpOp::OrdLt,
                    srcs: [a[0].into(), Src::ZERO],
                    accum: SrcRef::True.into(),
                    ftz: false,
                }));
                // start with 0.0, select -1.0 if negative, 1.0 if positive
                self.push_instr(Instr::new(OpCopy {
                    dst: dst.into(),
                    src: Src::ZERO,
                }));
                let neg_one: f32 = -1.0;
                let tmp = self.alloc_ssa(RegFile::GPR);
                self.push_instr(Instr::new(OpSel {
                    dst: tmp.into(),
                    cond: neg.into(),
                    srcs: [Src::new_imm_u32(neg_one.to_bits()), dst.into()],
                }));
                let result = self.alloc_ssa(RegFile::GPR);
                let one: f32 = 1.0;
                self.push_instr(Instr::new(OpSel {
                    dst: result.into(),
                    cond: pos.into(),
                    srcs: [Src::new_imm_u32(one.to_bits()), tmp.into()],
                }));
                Ok(result.into())
            }
            naga::MathFunction::Dot => {
                let b =
                    b.ok_or_else(|| CompileError::InvalidInput("dot requires 2 args".into()))?;
                let comps = a.comps().min(b.comps());
                // a[0]*b[0]
                let mut acc = self.alloc_ssa(RegFile::GPR);
                self.push_instr(Instr::new(OpFMul {
                    dst: acc.into(),
                    srcs: [a[0].into(), b[0].into()],
                    saturate: false,
                    rnd_mode: FRndMode::NearestEven,
                    ftz: false,
                    dnz: false,
                }));
                for c in 1..comps as usize {
                    let next = self.alloc_ssa(RegFile::GPR);
                    self.push_instr(Instr::new(OpFFma {
                        dst: next.into(),
                        srcs: [a[c].into(), b[c].into(), acc.into()],
                        saturate: false,
                        rnd_mode: FRndMode::NearestEven,
                        ftz: false,
                        dnz: false,
                    }));
                    acc = next;
                }
                Ok(acc.into())
            }
            naga::MathFunction::Mix => {
                let b =
                    b.ok_or_else(|| CompileError::InvalidInput("mix requires 3 args".into()))?;
                let t =
                    c.ok_or_else(|| CompileError::InvalidInput("mix requires 3 args".into()))?;
                // mix(a, b, t) = a + t*(b - a) = a*(1-t) + b*t
                // Using FMA: result = b*t + a*(1-t) = (b-a)*t + a
                let diff = self.alloc_ssa(RegFile::GPR);
                self.push_instr(Instr::new(OpFAdd {
                    dst: diff.into(),
                    srcs: [b[0].into(), Src::from(a[0]).fneg()],
                    saturate: false,
                    rnd_mode: FRndMode::NearestEven,
                    ftz: false,
                }));
                let dst = self.alloc_ssa(RegFile::GPR);
                self.push_instr(Instr::new(OpFFma {
                    dst: dst.into(),
                    srcs: [diff.into(), t[0].into(), a[0].into()],
                    saturate: false,
                    rnd_mode: FRndMode::NearestEven,
                    ftz: false,
                    dnz: false,
                }));
                Ok(dst.into())
            }
            naga::MathFunction::Step => {
                let b =
                    b.ok_or_else(|| CompileError::InvalidInput("step requires 2 args".into()))?;
                // step(edge, x) = x >= edge ? 1.0 : 0.0
                let pred = self.alloc_ssa(RegFile::Pred);
                self.push_instr(Instr::new(OpFSetP {
                    dst: pred.into(),
                    set_op: PredSetOp::And,
                    cmp_op: FloatCmpOp::OrdGe,
                    srcs: [b[0].into(), a[0].into()],
                    accum: SrcRef::True.into(),
                    ftz: false,
                }));
                let one: f32 = 1.0;
                let dst = self.alloc_ssa(RegFile::GPR);
                self.push_instr(Instr::new(OpSel {
                    dst: dst.into(),
                    cond: pred.into(),
                    srcs: [Src::new_imm_u32(one.to_bits()), Src::ZERO],
                }));
                Ok(dst.into())
            }
            naga::MathFunction::SmoothStep => {
                let b = b.ok_or_else(|| {
                    CompileError::InvalidInput("smoothstep requires 3 args".into())
                })?;
                let x = c.ok_or_else(|| {
                    CompileError::InvalidInput("smoothstep requires 3 args".into())
                })?;
                // smoothstep(lo, hi, x): t = clamp((x-lo)/(hi-lo), 0, 1); return t*t*(3-2*t)
                let diff_x = self.alloc_ssa(RegFile::GPR);
                self.push_instr(Instr::new(OpFAdd {
                    dst: diff_x.into(),
                    srcs: [x[0].into(), Src::from(a[0]).fneg()],
                    saturate: false,
                    rnd_mode: FRndMode::NearestEven,
                    ftz: false,
                }));
                let range = self.alloc_ssa(RegFile::GPR);
                self.push_instr(Instr::new(OpFAdd {
                    dst: range.into(),
                    srcs: [b[0].into(), Src::from(a[0]).fneg()],
                    saturate: false,
                    rnd_mode: FRndMode::NearestEven,
                    ftz: false,
                }));
                let rcp_range = self.alloc_ssa(RegFile::GPR);
                self.push_instr(Instr::new(OpTranscendental {
                    dst: rcp_range.into(),
                    op: TranscendentalOp::Rcp,
                    src: range.into(),
                }));
                let t_raw = self.alloc_ssa(RegFile::GPR);
                self.push_instr(Instr::new(OpFMul {
                    dst: t_raw.into(),
                    srcs: [diff_x.into(), rcp_range.into()],
                    saturate: false,
                    rnd_mode: FRndMode::NearestEven,
                    ftz: false,
                    dnz: false,
                }));
                // clamp to [0, 1] via saturate
                let t = self.alloc_ssa(RegFile::GPR);
                self.push_instr(Instr::new(OpFAdd {
                    dst: t.into(),
                    srcs: [t_raw.into(), Src::ZERO],
                    saturate: true,
                    rnd_mode: FRndMode::NearestEven,
                    ftz: false,
                }));
                // t * t
                let t2 = self.alloc_ssa(RegFile::GPR);
                self.push_instr(Instr::new(OpFMul {
                    dst: t2.into(),
                    srcs: [t.into(), t.into()],
                    saturate: false,
                    rnd_mode: FRndMode::NearestEven,
                    ftz: false,
                    dnz: false,
                }));
                // 3 - 2*t
                let two: f32 = 2.0;
                let three: f32 = 3.0;
                let two_t = self.alloc_ssa(RegFile::GPR);
                self.push_instr(Instr::new(OpFMul {
                    dst: two_t.into(),
                    srcs: [Src::new_imm_u32(two.to_bits()), t.into()],
                    saturate: false,
                    rnd_mode: FRndMode::NearestEven,
                    ftz: false,
                    dnz: false,
                }));
                let three_minus_2t = self.alloc_ssa(RegFile::GPR);
                self.push_instr(Instr::new(OpFAdd {
                    dst: three_minus_2t.into(),
                    srcs: [Src::new_imm_u32(three.to_bits()), Src::from(two_t).fneg()],
                    saturate: false,
                    rnd_mode: FRndMode::NearestEven,
                    ftz: false,
                }));
                let dst = self.alloc_ssa(RegFile::GPR);
                self.push_instr(Instr::new(OpFMul {
                    dst: dst.into(),
                    srcs: [t2.into(), three_minus_2t.into()],
                    saturate: false,
                    rnd_mode: FRndMode::NearestEven,
                    ftz: false,
                    dnz: false,
                }));
                Ok(dst.into())
            }
            naga::MathFunction::Length => {
                let comps = a.comps();
                if comps == 1 {
                    let dst = self.alloc_ssa(RegFile::GPR);
                    self.push_instr(Instr::new(OpFAdd {
                        dst: dst.into(),
                        srcs: [Src::ZERO, Src::from(a[0]).fabs()],
                        saturate: false,
                        rnd_mode: FRndMode::NearestEven,
                        ftz: false,
                    }));
                    return Ok(dst.into());
                }
                let dot = self.emit_f32_dot_self(&a);
                let rsq = self.alloc_ssa(RegFile::GPR);
                self.push_instr(Instr::new(OpTranscendental {
                    dst: rsq.into(),
                    op: TranscendentalOp::Rsq,
                    src: dot.into(),
                }));
                let dst = self.alloc_ssa(RegFile::GPR);
                self.push_instr(Instr::new(OpTranscendental {
                    dst: dst.into(),
                    op: TranscendentalOp::Rcp,
                    src: rsq.into(),
                }));
                Ok(dst.into())
            }
            naga::MathFunction::Normalize => {
                let comps = a.comps();
                let dot = self.emit_f32_dot_self(&a);
                let inv_len = self.alloc_ssa(RegFile::GPR);
                self.push_instr(Instr::new(OpTranscendental {
                    dst: inv_len.into(),
                    op: TranscendentalOp::Rsq,
                    src: dot.into(),
                }));
                let dst = self.alloc_ssa_vec(RegFile::GPR, comps);
                for c in 0..comps as usize {
                    self.push_instr(Instr::new(OpFMul {
                        dst: dst[c].into(),
                        srcs: [a[c].into(), inv_len.into()],
                        saturate: false,
                        rnd_mode: FRndMode::NearestEven,
                        ftz: false,
                        dnz: false,
                    }));
                }
                Ok(dst)
            }
            naga::MathFunction::Cross => {
                let b =
                    b.ok_or_else(|| CompileError::InvalidInput("cross requires 2 args".into()))?;
                // cross(a, b) = (a.y*b.z - a.z*b.y, a.z*b.x - a.x*b.z, a.x*b.y - a.y*b.x)
                let dst = self.alloc_ssa_vec(RegFile::GPR, 3);
                // x = a.y*b.z - a.z*b.y
                let tmp0 = self.alloc_ssa(RegFile::GPR);
                self.push_instr(Instr::new(OpFMul {
                    dst: tmp0.into(),
                    srcs: [a[1].into(), b[2].into()],
                    saturate: false,
                    rnd_mode: FRndMode::NearestEven,
                    ftz: false,
                    dnz: false,
                }));
                self.push_instr(Instr::new(OpFFma {
                    dst: dst[0].into(),
                    srcs: [Src::from(a[2]).fneg(), b[1].into(), tmp0.into()],
                    saturate: false,
                    rnd_mode: FRndMode::NearestEven,
                    ftz: false,
                    dnz: false,
                }));
                // y = a.z*b.x - a.x*b.z
                let tmp1 = self.alloc_ssa(RegFile::GPR);
                self.push_instr(Instr::new(OpFMul {
                    dst: tmp1.into(),
                    srcs: [a[2].into(), b[0].into()],
                    saturate: false,
                    rnd_mode: FRndMode::NearestEven,
                    ftz: false,
                    dnz: false,
                }));
                self.push_instr(Instr::new(OpFFma {
                    dst: dst[1].into(),
                    srcs: [Src::from(a[0]).fneg(), b[2].into(), tmp1.into()],
                    saturate: false,
                    rnd_mode: FRndMode::NearestEven,
                    ftz: false,
                    dnz: false,
                }));
                // z = a.x*b.y - a.y*b.x
                let tmp2 = self.alloc_ssa(RegFile::GPR);
                self.push_instr(Instr::new(OpFMul {
                    dst: tmp2.into(),
                    srcs: [a[0].into(), b[1].into()],
                    saturate: false,
                    rnd_mode: FRndMode::NearestEven,
                    ftz: false,
                    dnz: false,
                }));
                self.push_instr(Instr::new(OpFFma {
                    dst: dst[2].into(),
                    srcs: [Src::from(a[1]).fneg(), b[0].into(), tmp2.into()],
                    saturate: false,
                    rnd_mode: FRndMode::NearestEven,
                    ftz: false,
                    dnz: false,
                }));
                Ok(dst)
            }
            naga::MathFunction::Trunc => {
                if is_f64 {
                    // trunc = copysign(floor(abs(x)), x)
                    let abs_x = self.alloc_ssa_vec(RegFile::GPR, 2);
                    self.push_instr(Instr::new(OpDAdd {
                        dst: abs_x.clone().into(),
                        srcs: [Src::ZERO, Src::from(a.clone()).fabs()],
                        rnd_mode: FRndMode::NearestEven,
                    }));
                    let floored = self.emit_f64_floor(abs_x)?;

                    let sign_bit = self.alloc_ssa(RegFile::GPR);
                    self.emit_logic_and(sign_bit, a[1].into(), Src::new_imm_u32(0x8000_0000));

                    let cleared = self.alloc_ssa(RegFile::GPR);
                    self.emit_logic_and(cleared, floored[1].into(), Src::new_imm_u32(0x7FFF_FFFF));

                    let dst = self.alloc_ssa_vec(RegFile::GPR, 2);
                    self.push_instr(Instr::new(OpCopy {
                        dst: dst[0].into(),
                        src: floored[0].into(),
                    }));
                    self.emit_logic_or(dst[1], cleared.into(), sign_bit.into());
                    Ok(dst)
                } else {
                    let dst = self.alloc_ssa(RegFile::GPR);
                    self.push_instr(Instr::new(OpFRnd {
                        dst: dst.into(),
                        src: a[0].into(),
                        dst_type: FloatType::F32,
                        src_type: FloatType::F32,
                        rnd_mode: FRndMode::Zero,
                        ftz: false,
                    }));
                    Ok(dst.into())
                }
            }
            naga::MathFunction::Atan => {
                // atan(x) via polynomial approximation with range reduction
                self.emit_f32_atan(a[0])
            }
            naga::MathFunction::Atan2 => {
                let b = b.ok_or_else(|| {
                    CompileError::InvalidInput("atan2 requires 2 args".into())
                })?;
                // atan2(y, x)
                self.emit_f32_atan2(a[0], b[0])
            }
            naga::MathFunction::Asin => {
                // asin(x) = atan2(x, sqrt(1 - x*x))
                let x2 = self.alloc_ssa(RegFile::GPR);
                self.push_instr(Instr::new(OpFMul {
                    dst: x2.into(),
                    srcs: [a[0].into(), a[0].into()],
                    saturate: false,
                    rnd_mode: FRndMode::NearestEven,
                    ftz: false,
                    dnz: false,
                }));
                let one_minus_x2 = self.alloc_ssa(RegFile::GPR);
                let one: f32 = 1.0;
                self.push_instr(Instr::new(OpFAdd {
                    dst: one_minus_x2.into(),
                    srcs: [Src::new_imm_u32(one.to_bits()), Src::from(x2).fneg()],
                    saturate: false,
                    rnd_mode: FRndMode::NearestEven,
                    ftz: false,
                }));
                let rsq = self.alloc_ssa(RegFile::GPR);
                self.push_instr(Instr::new(OpTranscendental {
                    dst: rsq.into(),
                    op: TranscendentalOp::Rsq,
                    src: one_minus_x2.into(),
                }));
                let sqrt_val = self.alloc_ssa(RegFile::GPR);
                self.push_instr(Instr::new(OpTranscendental {
                    dst: sqrt_val.into(),
                    op: TranscendentalOp::Rcp,
                    src: rsq.into(),
                }));
                self.emit_f32_atan2(a[0], sqrt_val)
            }
            naga::MathFunction::Acos => {
                // acos(x) = atan2(sqrt(1 - x*x), x)
                let x2 = self.alloc_ssa(RegFile::GPR);
                self.push_instr(Instr::new(OpFMul {
                    dst: x2.into(),
                    srcs: [a[0].into(), a[0].into()],
                    saturate: false,
                    rnd_mode: FRndMode::NearestEven,
                    ftz: false,
                    dnz: false,
                }));
                let one_minus_x2 = self.alloc_ssa(RegFile::GPR);
                let one: f32 = 1.0;
                self.push_instr(Instr::new(OpFAdd {
                    dst: one_minus_x2.into(),
                    srcs: [Src::new_imm_u32(one.to_bits()), Src::from(x2).fneg()],
                    saturate: false,
                    rnd_mode: FRndMode::NearestEven,
                    ftz: false,
                }));
                let rsq = self.alloc_ssa(RegFile::GPR);
                self.push_instr(Instr::new(OpTranscendental {
                    dst: rsq.into(),
                    op: TranscendentalOp::Rsq,
                    src: one_minus_x2.into(),
                }));
                let sqrt_val = self.alloc_ssa(RegFile::GPR);
                self.push_instr(Instr::new(OpTranscendental {
                    dst: sqrt_val.into(),
                    op: TranscendentalOp::Rcp,
                    src: rsq.into(),
                }));
                self.emit_f32_atan2(sqrt_val, a[0])
            }
            naga::MathFunction::Asinh => {
                // asinh(x) = ln(x + sqrt(x*x + 1))
                // = log2(x + sqrt(x*x + 1)) * ln(2)
                let x2 = self.alloc_ssa(RegFile::GPR);
                self.push_instr(Instr::new(OpFMul {
                    dst: x2.into(),
                    srcs: [a[0].into(), a[0].into()],
                    saturate: false, rnd_mode: FRndMode::NearestEven, ftz: false, dnz: false,
                }));
                let one: f32 = 1.0;
                let x2p1 = self.alloc_ssa(RegFile::GPR);
                self.push_instr(Instr::new(OpFAdd {
                    dst: x2p1.into(),
                    srcs: [x2.into(), Src::new_imm_u32(one.to_bits())],
                    saturate: false, rnd_mode: FRndMode::NearestEven, ftz: false,
                }));
                let rsq = self.alloc_ssa(RegFile::GPR);
                self.push_instr(Instr::new(OpTranscendental { dst: rsq.into(), op: TranscendentalOp::Rsq, src: x2p1.into() }));
                let sq = self.alloc_ssa(RegFile::GPR);
                self.push_instr(Instr::new(OpTranscendental { dst: sq.into(), op: TranscendentalOp::Rcp, src: rsq.into() }));
                let sum = self.alloc_ssa(RegFile::GPR);
                self.push_instr(Instr::new(OpFAdd {
                    dst: sum.into(), srcs: [a[0].into(), sq.into()],
                    saturate: false, rnd_mode: FRndMode::NearestEven, ftz: false,
                }));
                let log2_val = self.alloc_ssa(RegFile::GPR);
                self.push_instr(Instr::new(OpTranscendental { dst: log2_val.into(), op: TranscendentalOp::Log2, src: sum.into() }));
                let ln2: f32 = std::f32::consts::LN_2;
                let dst = self.alloc_ssa(RegFile::GPR);
                self.push_instr(Instr::new(OpFMul {
                    dst: dst.into(),
                    srcs: [log2_val.into(), Src::new_imm_u32(ln2.to_bits())],
                    saturate: false, rnd_mode: FRndMode::NearestEven, ftz: false, dnz: false,
                }));
                Ok(dst.into())
            }
            naga::MathFunction::Acosh => {
                // acosh(x) = ln(x + sqrt(x*x - 1))
                let x2 = self.alloc_ssa(RegFile::GPR);
                self.push_instr(Instr::new(OpFMul {
                    dst: x2.into(), srcs: [a[0].into(), a[0].into()],
                    saturate: false, rnd_mode: FRndMode::NearestEven, ftz: false, dnz: false,
                }));
                let one: f32 = 1.0;
                let x2m1 = self.alloc_ssa(RegFile::GPR);
                self.push_instr(Instr::new(OpFAdd {
                    dst: x2m1.into(),
                    srcs: [x2.into(), Src::new_imm_u32((-one).to_bits())],
                    saturate: false, rnd_mode: FRndMode::NearestEven, ftz: false,
                }));
                let rsq = self.alloc_ssa(RegFile::GPR);
                self.push_instr(Instr::new(OpTranscendental { dst: rsq.into(), op: TranscendentalOp::Rsq, src: x2m1.into() }));
                let sq = self.alloc_ssa(RegFile::GPR);
                self.push_instr(Instr::new(OpTranscendental { dst: sq.into(), op: TranscendentalOp::Rcp, src: rsq.into() }));
                let sum = self.alloc_ssa(RegFile::GPR);
                self.push_instr(Instr::new(OpFAdd {
                    dst: sum.into(), srcs: [a[0].into(), sq.into()],
                    saturate: false, rnd_mode: FRndMode::NearestEven, ftz: false,
                }));
                let log2_val = self.alloc_ssa(RegFile::GPR);
                self.push_instr(Instr::new(OpTranscendental { dst: log2_val.into(), op: TranscendentalOp::Log2, src: sum.into() }));
                let ln2: f32 = std::f32::consts::LN_2;
                let dst = self.alloc_ssa(RegFile::GPR);
                self.push_instr(Instr::new(OpFMul {
                    dst: dst.into(),
                    srcs: [log2_val.into(), Src::new_imm_u32(ln2.to_bits())],
                    saturate: false, rnd_mode: FRndMode::NearestEven, ftz: false, dnz: false,
                }));
                Ok(dst.into())
            }
            naga::MathFunction::Atanh => {
                // atanh(x) = 0.5 * ln((1+x)/(1-x))
                let one: f32 = 1.0;
                let half: f32 = 0.5;
                let one_plus_x = self.alloc_ssa(RegFile::GPR);
                self.push_instr(Instr::new(OpFAdd {
                    dst: one_plus_x.into(),
                    srcs: [Src::new_imm_u32(one.to_bits()), a[0].into()],
                    saturate: false, rnd_mode: FRndMode::NearestEven, ftz: false,
                }));
                let one_minus_x = self.alloc_ssa(RegFile::GPR);
                self.push_instr(Instr::new(OpFAdd {
                    dst: one_minus_x.into(),
                    srcs: [Src::new_imm_u32(one.to_bits()), Src::from(a[0]).fneg()],
                    saturate: false, rnd_mode: FRndMode::NearestEven, ftz: false,
                }));
                let rcp_denom = self.alloc_ssa(RegFile::GPR);
                self.push_instr(Instr::new(OpTranscendental { dst: rcp_denom.into(), op: TranscendentalOp::Rcp, src: one_minus_x.into() }));
                let ratio = self.alloc_ssa(RegFile::GPR);
                self.push_instr(Instr::new(OpFMul {
                    dst: ratio.into(), srcs: [one_plus_x.into(), rcp_denom.into()],
                    saturate: false, rnd_mode: FRndMode::NearestEven, ftz: false, dnz: false,
                }));
                let log2_val = self.alloc_ssa(RegFile::GPR);
                self.push_instr(Instr::new(OpTranscendental { dst: log2_val.into(), op: TranscendentalOp::Log2, src: ratio.into() }));
                let ln2: f32 = std::f32::consts::LN_2;
                let ln_val = self.alloc_ssa(RegFile::GPR);
                self.push_instr(Instr::new(OpFMul {
                    dst: ln_val.into(),
                    srcs: [log2_val.into(), Src::new_imm_u32(ln2.to_bits())],
                    saturate: false, rnd_mode: FRndMode::NearestEven, ftz: false, dnz: false,
                }));
                let dst = self.alloc_ssa(RegFile::GPR);
                self.push_instr(Instr::new(OpFMul {
                    dst: dst.into(),
                    srcs: [ln_val.into(), Src::new_imm_u32(half.to_bits())],
                    saturate: false, rnd_mode: FRndMode::NearestEven, ftz: false, dnz: false,
                }));
                Ok(dst.into())
            }
            naga::MathFunction::Sinh => {
                // sinh(x) = (exp(x) - exp(-x)) / 2
                let log2_e: f32 = std::f32::consts::LOG2_E;
                let half: f32 = 0.5;
                let s1 = self.alloc_ssa(RegFile::GPR);
                self.push_instr(Instr::new(OpFMul {
                    dst: s1.into(), srcs: [a[0].into(), Src::new_imm_u32(log2_e.to_bits())],
                    saturate: false, rnd_mode: FRndMode::NearestEven, ftz: false, dnz: false,
                }));
                let exp_pos = self.alloc_ssa(RegFile::GPR);
                self.push_instr(Instr::new(OpTranscendental { dst: exp_pos.into(), op: TranscendentalOp::Exp2, src: s1.into() }));
                let s2 = self.alloc_ssa(RegFile::GPR);
                self.push_instr(Instr::new(OpFMul {
                    dst: s2.into(), srcs: [Src::from(a[0]).fneg(), Src::new_imm_u32(log2_e.to_bits())],
                    saturate: false, rnd_mode: FRndMode::NearestEven, ftz: false, dnz: false,
                }));
                let exp_neg = self.alloc_ssa(RegFile::GPR);
                self.push_instr(Instr::new(OpTranscendental { dst: exp_neg.into(), op: TranscendentalOp::Exp2, src: s2.into() }));
                let diff = self.alloc_ssa(RegFile::GPR);
                self.push_instr(Instr::new(OpFAdd {
                    dst: diff.into(), srcs: [exp_pos.into(), Src::from(exp_neg).fneg()],
                    saturate: false, rnd_mode: FRndMode::NearestEven, ftz: false,
                }));
                let dst = self.alloc_ssa(RegFile::GPR);
                self.push_instr(Instr::new(OpFMul {
                    dst: dst.into(), srcs: [diff.into(), Src::new_imm_u32(half.to_bits())],
                    saturate: false, rnd_mode: FRndMode::NearestEven, ftz: false, dnz: false,
                }));
                Ok(dst.into())
            }
            naga::MathFunction::Cosh => {
                // cosh(x) = (exp(x) + exp(-x)) / 2
                let log2_e: f32 = std::f32::consts::LOG2_E;
                let half: f32 = 0.5;
                let s1 = self.alloc_ssa(RegFile::GPR);
                self.push_instr(Instr::new(OpFMul {
                    dst: s1.into(), srcs: [a[0].into(), Src::new_imm_u32(log2_e.to_bits())],
                    saturate: false, rnd_mode: FRndMode::NearestEven, ftz: false, dnz: false,
                }));
                let exp_pos = self.alloc_ssa(RegFile::GPR);
                self.push_instr(Instr::new(OpTranscendental { dst: exp_pos.into(), op: TranscendentalOp::Exp2, src: s1.into() }));
                let s2 = self.alloc_ssa(RegFile::GPR);
                self.push_instr(Instr::new(OpFMul {
                    dst: s2.into(), srcs: [Src::from(a[0]).fneg(), Src::new_imm_u32(log2_e.to_bits())],
                    saturate: false, rnd_mode: FRndMode::NearestEven, ftz: false, dnz: false,
                }));
                let exp_neg = self.alloc_ssa(RegFile::GPR);
                self.push_instr(Instr::new(OpTranscendental { dst: exp_neg.into(), op: TranscendentalOp::Exp2, src: s2.into() }));
                let sum = self.alloc_ssa(RegFile::GPR);
                self.push_instr(Instr::new(OpFAdd {
                    dst: sum.into(), srcs: [exp_pos.into(), exp_neg.into()],
                    saturate: false, rnd_mode: FRndMode::NearestEven, ftz: false,
                }));
                let dst = self.alloc_ssa(RegFile::GPR);
                self.push_instr(Instr::new(OpFMul {
                    dst: dst.into(), srcs: [sum.into(), Src::new_imm_u32(half.to_bits())],
                    saturate: false, rnd_mode: FRndMode::NearestEven, ftz: false, dnz: false,
                }));
                Ok(dst.into())
            }
            _ => Err(CompileError::NotImplemented(
                format!("math function {fun:?} not yet supported").into(),
            )),
        }
    }
}
