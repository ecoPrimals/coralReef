// SPDX-License-Identifier: AGPL-3.0-only
//! Componentwise operations, unary, select, cast, type helpers, and function inlining.
#![allow(clippy::wildcard_imports)]
use super::super::ir::*;
use super::func::{FuncTranslator, VarRef};
use crate::error::CompileError;
use coral_reef_stubs::fxhash::FxHashMap;
use naga::Handle;

impl<'a, 'b> FuncTranslator<'a, 'b> {
    pub(super) fn emit_componentwise(
        &mut self,
        comps: u8,
        l: SSARef,
        r: SSARef,
        mut f: impl FnMut(&mut Self, SSAValue, SSAValue) -> SSAValue,
    ) -> Result<SSARef, CompileError> {
        let dst = self.alloc_ssa_vec(RegFile::GPR, comps);
        for c in 0..comps as usize {
            let lc = if c < l.comps() as usize { l[c] } else { l[0] };
            let rc = if c < r.comps() as usize { r[c] } else { r[0] };
            let result = f(self, lc, rc);
            self.push_instr(Instr::new(OpCopy {
                dst: dst[c].into(),
                src: result.into(),
            }));
        }
        Ok(dst)
    }

    pub(super) fn emit_int_componentwise(
        &mut self,
        comps: u8,
        l: SSARef,
        r: SSARef,
        f: impl FnMut(&mut Self, SSAValue, SSAValue) -> SSAValue,
    ) -> Result<SSARef, CompileError> {
        self.emit_componentwise(comps, l, r, f)
    }

    pub(super) fn emit_cmp_componentwise(
        &mut self,
        _comps: u8,
        l: SSARef,
        r: SSARef,
        mut f: impl FnMut(&mut Self, SSAValue, SSAValue) -> SSAValue,
    ) -> Result<SSARef, CompileError> {
        let result = f(self, l[0], r[0]);
        Ok(result.into())
    }

    pub(super) fn translate_unary(
        &mut self,
        op: naga::UnaryOperator,
        val: SSARef,
        inner_handle: Handle<naga::Expression>,
    ) -> Result<SSARef, CompileError> {
        let is_float = self.is_float_expr(inner_handle);
        let comps = val.comps();
        let dst = self.alloc_ssa_vec(RegFile::GPR, comps);

        for c in 0..comps as usize {
            match op {
                naga::UnaryOperator::Negate if is_float => {
                    self.push_instr(Instr::new(OpFAdd {
                        dst: dst[c].into(),
                        srcs: [Src::ZERO, Src::from(val[c]).fneg()],
                        saturate: false,
                        rnd_mode: FRndMode::NearestEven,
                        ftz: false,
                    }));
                }
                naga::UnaryOperator::Negate => {
                    if self.sm.sm() >= 70 {
                        self.push_instr(Instr::new(OpIAdd3 {
                            dst: dst[c].into(),
                            srcs: [Src::ZERO, Src::from(val[c]).ineg(), Src::ZERO],
                            overflow: [Dst::None, Dst::None],
                        }));
                    } else {
                        self.push_instr(Instr::new(OpIAdd2 {
                            dst: dst[c].into(),
                            srcs: [Src::ZERO, Src::from(val[c]).ineg()],
                            carry_out: Dst::None,
                        }));
                    }
                }
                naga::UnaryOperator::LogicalNot => {
                    let pred_dst = self.alloc_ssa(RegFile::Pred);
                    self.push_instr(Instr::new(OpCopy {
                        dst: pred_dst.into(),
                        src: Src::from(val[c]).bnot(),
                    }));
                    self.push_instr(Instr::new(OpCopy {
                        dst: dst[c].into(),
                        src: pred_dst.into(),
                    }));
                }
                naga::UnaryOperator::BitwiseNot => {
                    if self.sm.sm() >= 70 {
                        self.push_instr(Instr::new(OpLop3 {
                            dst: dst[c].into(),
                            srcs: [val[c].into(), Src::ZERO, Src::ZERO],
                            op: LogicOp3::new_lut(&|x, _, _| !x),
                        }));
                    } else {
                        self.push_instr(Instr::new(OpLop2 {
                            dst: dst[c].into(),
                            srcs: [val[c].into(), Src::ZERO],
                            op: LogicOp2::And,
                        }));
                    }
                }
            }
        }
        Ok(dst)
    }

    pub(super) fn translate_select(
        &mut self,
        cond: SSARef,
        accept: SSARef,
        reject: SSARef,
    ) -> Result<SSARef, CompileError> {
        let pred_src: Src = if cond[0].file() == RegFile::Pred {
            cond[0].into()
        } else {
            let pred = self.alloc_ssa(RegFile::Pred);
            self.push_instr(Instr::new(OpISetP {
                dst: pred.into(),
                set_op: PredSetOp::And,
                cmp_op: IntCmpOp::Ne,
                cmp_type: IntCmpType::U32,
                ex: false,
                srcs: [cond[0].into(), Src::ZERO],
                accum: SrcRef::True.into(),
                low_cmp: SrcRef::False.into(),
            }));
            pred.into()
        };
        let comps = accept.comps().max(reject.comps());
        let dst = self.alloc_ssa_vec(RegFile::GPR, comps);
        for c in 0..comps as usize {
            let acc = if (c as u8) < accept.comps() {
                accept[c]
            } else {
                accept[0]
            };
            let rej = if (c as u8) < reject.comps() {
                reject[c]
            } else {
                reject[0]
            };
            self.push_instr(Instr::new(OpSel {
                dst: dst[c].into(),
                cond: pred_src.clone(),
                srcs: [acc.into(), rej.into()],
            }));
        }
        Ok(dst)
    }

    /// Number of 32-bit GPR components needed to represent a type in registers.
    /// Returns 0 for types not eligible for register promotion.
    pub(super) fn type_reg_comps(&self, ty: Handle<naga::Type>) -> u8 {
        match &self.module.types[ty].inner {
            naga::TypeInner::Scalar(s) => {
                if s.width == 8 {
                    2
                } else {
                    1
                }
            }
            naga::TypeInner::Vector { size, scalar } => {
                let per_comp: u8 = if scalar.width == 8 { 2 } else { 1 };
                let n: u8 = match size {
                    naga::VectorSize::Bi => 2,
                    naga::VectorSize::Tri => 3,
                    naga::VectorSize::Quad => 4,
                };
                n * per_comp
            }
            naga::TypeInner::Pointer { base, .. } => self.type_reg_comps(*base),
            naga::TypeInner::Array {
                base,
                size: naga::ArraySize::Constant(count),
                ..
            } => {
                let elem = self.type_reg_comps(*base);
                if elem == 0 {
                    return 0;
                }
                let total = count.get().saturating_mul(elem as u32);
                if total > 32 { 0 } else { total as u8 }
            }
            _ => 0,
        }
    }

    pub(super) fn inline_call(
        &mut self,
        function: Handle<naga::Function>,
        arguments: &[Handle<naga::Expression>],
        result: Option<Handle<naga::Expression>>,
    ) -> Result<(), CompileError> {
        let module = self.module;
        let callee = &module.functions[function];

        let mut by_value_args: Vec<SSARef> = Vec::with_capacity(arguments.len());
        let mut ptr_arg_slots: FxHashMap<u32, usize> = FxHashMap::default();

        for (i, &arg_handle) in arguments.iter().enumerate() {
            let callee_arg_ty = &module.types[callee.arguments[i].ty].inner;
            if matches!(callee_arg_ty, naga::TypeInner::Pointer { .. }) {
                self.ensure_expr(arg_handle)?;
                if let Some(var_ref) = self.expr_to_var.get(&arg_handle).copied() {
                    match var_ref {
                        VarRef::Full(slot) => {
                            ptr_arg_slots.insert(i as u32, slot);
                            let placeholder = self.alloc_ssa(RegFile::GPR);
                            by_value_args.push(placeholder.into());
                        }
                        VarRef::Component(_, _) => {
                            return Err(CompileError::NotImplemented(
                                "pointer to variable component as function argument".into(),
                            ));
                        }
                    }
                } else {
                    return Err(CompileError::NotImplemented(
                        "non-local pointer argument in function call".into(),
                    ));
                }
            } else {
                let ssa = self.ensure_expr(arg_handle)?;
                by_value_args.push(ssa);
            }
        }

        let saved_func = self.func;
        let saved_expr_map = std::mem::take(&mut self.expr_map);
        let saved_uniform_refs = std::mem::take(&mut self.uniform_refs);
        let saved_expr_to_var = std::mem::take(&mut self.expr_to_var);
        let saved_local_var_slots = std::mem::take(&mut self.local_var_slots);
        let saved_inline_args = self.inline_args.take();
        let saved_inline_ptr = std::mem::take(&mut self.inline_ptr_arg_slots);
        let saved_inline_return = self.inline_return.take();
        let saved_dead_code = self.dead_code;
        let pre_inline_var_count = self.var_storage.len();

        self.func = callee;
        self.inline_args = Some(by_value_args);
        self.inline_ptr_arg_slots = ptr_arg_slots;
        self.inline_return = None;
        self.dead_code = false;

        self.pre_allocate_local_vars();

        let body = &module.functions[function].body;
        self.translate_block(body)?;

        let return_ssa = self.inline_return.take();
        let callee_ended_dead = self.dead_code;

        self.var_storage.truncate(pre_inline_var_count);
        self.func = saved_func;
        self.expr_map = saved_expr_map;
        self.uniform_refs = saved_uniform_refs;
        self.expr_to_var = saved_expr_to_var;
        self.local_var_slots = saved_local_var_slots;
        self.inline_args = saved_inline_args;
        self.inline_ptr_arg_slots = saved_inline_ptr;
        self.inline_return = saved_inline_return;
        self.dead_code = saved_dead_code;

        if callee_ended_dead {
            self.finish_block()?;
            self.start_block();
        }

        if let Some(res) = result {
            if let Some(ret_ssa) = return_ssa {
                self.expr_map.insert(res, ret_ssa);
            } else {
                let dst = self.alloc_ssa(RegFile::GPR);
                self.push_instr(Instr::new(OpUndef { dst: dst.into() }));
                self.expr_map.insert(res, dst.into());
            }
        }

        Ok(())
    }

    pub(super) fn translate_array_length(
        &mut self,
        ptr_expr: Handle<naga::Expression>,
    ) -> Result<SSARef, CompileError> {
        let gv = self.find_global_variable(ptr_expr)?;
        let global = &self.module.global_variables[gv];
        let binding = global
            .binding
            .as_ref()
            .ok_or_else(|| CompileError::InvalidInput("arrayLength on unbound global".into()))?;

        let element_stride = self.array_element_stride(global.ty)?;

        let buf_idx = binding.group as u8;
        let size_offset = (binding.binding * 8 + 8) as u16;
        let cbuf = CBufRef {
            buf: CBuf::Binding(buf_idx),
            offset: size_offset,
        };
        let buf_size = self.alloc_ssa(RegFile::GPR);
        self.push_instr(Instr::new(OpCopy {
            dst: buf_size.into(),
            src: Src::from(SrcRef::CBuf(cbuf)),
        }));

        if element_stride == 1 {
            return Ok(buf_size.into());
        }

        let stride_val = self.alloc_ssa(RegFile::GPR);
        self.push_instr(Instr::new(OpCopy {
            dst: stride_val.into(),
            src: Src::new_imm_u32(element_stride),
        }));

        let fa = self.alloc_ssa(RegFile::GPR);
        self.push_instr(Instr::new(OpI2F {
            dst: fa.into(),
            src: buf_size.into(),
            dst_type: FloatType::F32,
            src_type: IntType::I32,
            rnd_mode: FRndMode::NearestEven,
        }));
        let fb = self.alloc_ssa(RegFile::GPR);
        self.push_instr(Instr::new(OpI2F {
            dst: fb.into(),
            src: stride_val.into(),
            dst_type: FloatType::F32,
            src_type: IntType::I32,
            rnd_mode: FRndMode::NearestEven,
        }));
        let rcp = self.alloc_ssa(RegFile::GPR);
        self.push_instr(Instr::new(OpTranscendental {
            dst: rcp.into(),
            op: TranscendentalOp::Rcp,
            src: fb.into(),
        }));
        let quot_f = self.alloc_ssa(RegFile::GPR);
        self.push_instr(Instr::new(OpFMul {
            dst: quot_f.into(),
            srcs: [fa.into(), rcp.into()],
            saturate: false,
            rnd_mode: FRndMode::NearestEven,
            ftz: false,
            dnz: false,
        }));
        let dst = self.alloc_ssa(RegFile::GPR);
        self.push_instr(Instr::new(OpF2I {
            dst: dst.into(),
            src: quot_f.into(),
            dst_type: IntType::I32,
            src_type: FloatType::F32,
            rnd_mode: FRndMode::Zero,
            ftz: false,
        }));
        Ok(dst.into())
    }

    fn find_global_variable(
        &self,
        expr_handle: Handle<naga::Expression>,
    ) -> Result<Handle<naga::GlobalVariable>, CompileError> {
        match self.func.expressions[expr_handle] {
            naga::Expression::GlobalVariable(gv) => Ok(gv),
            naga::Expression::AccessIndex { base, .. } => self.find_global_variable(base),
            naga::Expression::Access { base, .. } => self.find_global_variable(base),
            _ => Err(CompileError::InvalidInput(
                "arrayLength: cannot find underlying global variable".into(),
            )),
        }
    }

    fn array_element_stride(&self, ty: Handle<naga::Type>) -> Result<u32, CompileError> {
        match &self.module.types[ty].inner {
            naga::TypeInner::Array { stride, .. } => Ok(*stride),
            naga::TypeInner::BindingArray { .. } => Ok(1),
            naga::TypeInner::Struct { members, .. } => {
                if let Some(last) = members.last() {
                    self.array_element_stride(last.ty)
                } else {
                    Err(CompileError::InvalidInput(
                        "arrayLength on struct with no members".into(),
                    ))
                }
            }
            naga::TypeInner::Pointer { base, .. } => self.array_element_stride(*base),
            _ => Err(CompileError::InvalidInput(
                format!(
                    "arrayLength on non-array type: {:?}",
                    self.module.types[ty].inner
                )
                .into(),
            )),
        }
    }

    pub(super) fn translate_cast(
        &mut self,
        val: SSARef,
        kind: naga::ScalarKind,
        convert: Option<naga::Bytes>,
        _inner_handle: Handle<naga::Expression>,
    ) -> Result<SSARef, CompileError> {
        match (kind, convert) {
            (naga::ScalarKind::Float, Some(4)) => {
                let dst = self.alloc_ssa(RegFile::GPR);
                self.push_instr(Instr::new(OpI2F {
                    dst: dst.into(),
                    src: val[0].into(),
                    dst_type: FloatType::F32,
                    src_type: IntType::I32,
                    rnd_mode: FRndMode::NearestEven,
                }));
                Ok(dst.into())
            }
            (naga::ScalarKind::Uint | naga::ScalarKind::Sint, Some(4)) => {
                let dst = self.alloc_ssa(RegFile::GPR);
                self.push_instr(Instr::new(OpF2I {
                    dst: dst.into(),
                    src: val[0].into(),
                    dst_type: IntType::I32,
                    src_type: FloatType::F32,
                    rnd_mode: FRndMode::NearestEven,
                    ftz: false,
                }));
                Ok(dst.into())
            }
            (naga::ScalarKind::Float, Some(8)) => {
                if val.comps() >= 2 {
                    Ok(val)
                } else {
                    let f32_val = self.alloc_ssa(RegFile::GPR);
                    self.push_instr(Instr::new(OpI2F {
                        dst: f32_val.into(),
                        src: val[0].into(),
                        dst_type: FloatType::F32,
                        src_type: IntType::I32,
                        rnd_mode: FRndMode::NearestEven,
                    }));
                    let dst = self.alloc_ssa_vec(RegFile::GPR, 2);
                    self.push_instr(Instr::new(OpF2F {
                        dst: dst.clone().into(),
                        src: f32_val.into(),
                        dst_type: FloatType::F64,
                        src_type: FloatType::F32,
                        rnd_mode: FRndMode::NearestEven,
                        ftz: false,
                        dst_high: false,
                        integer_rnd: false,
                    }));
                    Ok(dst)
                }
            }
            (naga::ScalarKind::Uint | naga::ScalarKind::Sint, Some(8)) => {
                let dst = self.alloc_ssa_vec(RegFile::GPR, 2);
                self.push_instr(Instr::new(OpCopy {
                    dst: dst[0].into(),
                    src: val[0].into(),
                }));
                if kind == naga::ScalarKind::Sint {
                    self.push_instr(Instr::new(OpShf {
                        dst: dst[1].into(),
                        high: val[0].into(),
                        low: Src::ZERO,
                        shift: Src::new_imm_u32(31),
                        right: true,
                        wrap: false,
                        data_type: IntType::I32,
                        dst_high: false,
                    }));
                } else {
                    self.push_instr(Instr::new(OpCopy {
                        dst: dst[1].into(),
                        src: Src::ZERO,
                    }));
                }
                Ok(dst)
            }
            _ => {
                let dst = self.alloc_ssa(RegFile::GPR);
                self.push_instr(Instr::new(OpCopy {
                    dst: dst.into(),
                    src: val[0].into(),
                }));
                Ok(dst.into())
            }
        }
    }
}
