#![allow(clippy::wildcard_imports)]
use super::func::FuncTranslator;
use super::super::ir::*;
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
            naga::MathFunction::Ceil => {
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
            naga::MathFunction::Round => {
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
                    self.push_instr(Instr::new(OpMuFu {
                        dst: rsq.into(),
                        op: MuFuOp::Rsq,
                        src: a[0].into(),
                    }));
                    let rcp_rsq = self.alloc_ssa(RegFile::GPR);
                    self.push_instr(Instr::new(OpMuFu {
                        dst: rcp_rsq.into(),
                        op: MuFuOp::Rcp,
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
                    self.push_instr(Instr::new(OpMuFu {
                        dst: dst.into(),
                        op: MuFuOp::Rsq,
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
                self.push_instr(Instr::new(OpMuFu {
                    dst: dst.into(),
                    op: MuFuOp::Sin,
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
                self.push_instr(Instr::new(OpMuFu {
                    dst: dst.into(),
                    op: MuFuOp::Cos,
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
                    self.push_instr(Instr::new(OpMuFu {
                        dst: dst.into(),
                        op: MuFuOp::Exp2,
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
                    self.push_instr(Instr::new(OpMuFu {
                        dst: dst.into(),
                        op: MuFuOp::Log2,
                        src: a[0].into(),
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
            _ => Err(CompileError::NotImplemented(format!(
                "math function {fun:?} not yet supported"
            ))),
        }
    }
}
