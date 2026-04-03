// SPDX-License-Identifier: AGPL-3.0-only
//! Type conversion lowering (Expression::As) → OpF2I / OpI2F / OpF2F / OpI2I.

use super::FuncLowerer;
use crate::ast::ScalarKind;
use coral_reef::codegen::ir::*;
use coral_reef::error::CompileError;

pub(crate) fn lower_cast(
    fl: &mut FuncLowerer<'_, '_>,
    val: SSARef,
    kind: ScalarKind,
    convert: Option<u8>,
) -> Result<SSARef, CompileError> {
    let width = convert.unwrap_or(4);
    let dst = fl.alloc_ssa(RegFile::GPR);

    match kind {
        ScalarKind::Float => {
            match width {
                4 => {
                    // int → f32
                    fl.push_instr(Instr::new(OpI2F {
                        dst: dst.into(),
                        src: Src::from(val[0]),
                        dst_type: FloatType::F32,
                        src_type: IntType::I32,
                        rnd_mode: FRndMode::NearestEven,
                    }));
                }
                8 => {
                    fl.push_instr(Instr::new(OpCopy {
                        dst: dst.into(),
                        src: Src::from(val[0]),
                    }));
                }
                _ => {
                    fl.push_instr(Instr::new(OpI2F {
                        dst: dst.into(),
                        src: Src::from(val[0]),
                        dst_type: FloatType::F32,
                        src_type: IntType::I32,
                        rnd_mode: FRndMode::NearestEven,
                    }));
                }
            }
        }
        ScalarKind::Sint => {
            // float → i32
            fl.push_instr(Instr::new(OpF2I {
                dst: dst.into(),
                src: Src::from(val[0]),
                src_type: FloatType::F32,
                dst_type: IntType::I32,
                rnd_mode: FRndMode::Zero,
                ftz: false,
            }));
        }
        ScalarKind::Uint => {
            // float → u32
            fl.push_instr(Instr::new(OpF2I {
                dst: dst.into(),
                src: Src::from(val[0]),
                src_type: FloatType::F32,
                dst_type: IntType::U32,
                rnd_mode: FRndMode::Zero,
                ftz: false,
            }));
        }
        ScalarKind::Bool => {
            let pred = fl.alloc_ssa(RegFile::Pred);
            fl.push_instr(Instr::new(OpISetP {
                dst: pred.into(),
                set_op: PredSetOp::And,
                cmp_op: IntCmpOp::Ne,
                cmp_type: IntCmpType::U32,
                ex: false,
                srcs: [Src::from(val[0]), Src::ZERO, SrcRef::True.into(), SrcRef::True.into()],
            }));
            fl.push_instr(Instr::new(OpSel {
                dst: dst.into(),
                srcs: [Src::from(pred), Src::new_imm_u32(1), Src::ZERO],
            }));
        }
    }

    Ok(dst.into())
}
