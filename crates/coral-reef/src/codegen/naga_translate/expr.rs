// SPDX-License-Identifier: AGPL-3.0-only
#![allow(clippy::wildcard_imports)]
use super::super::ir::*;
use super::func::FuncTranslator;
use crate::error::CompileError;
use naga::Handle;

impl<'a, 'b> FuncTranslator<'a, 'b> {
    pub(super) fn ensure_expr(
        &mut self,
        handle: Handle<naga::Expression>,
    ) -> Result<SSARef, CompileError> {
        if let Some(ssa) = self.expr_map.get(&handle) {
            return Ok(ssa.clone());
        }
        self.translate_expression(handle)
    }

    pub(super) fn translate_expression(
        &mut self,
        handle: Handle<naga::Expression>,
    ) -> Result<SSARef, CompileError> {
        if let Some(ssa) = self.expr_map.get(&handle) {
            return Ok(ssa.clone());
        }

        let expr = &self.func.expressions[handle];
        let result = match *expr {
            naga::Expression::Literal(ref lit) => self.translate_literal(lit),
            naga::Expression::Constant(c) => {
                let constant = &self.module.constants[c];
                let expr_handle = constant.init;
                let init_expr = &self.module.global_expressions[expr_handle];
                match *init_expr {
                    naga::Expression::Literal(ref lit) => self.translate_literal(lit),
                    naga::Expression::ZeroValue(ty) => self.translate_zero_value(ty),
                    _ => Err(CompileError::NotImplemented(
                        "non-literal constant initializer".into(),
                    )),
                }
            }
            naga::Expression::ZeroValue(ty) => self.translate_zero_value(ty),
            naga::Expression::Binary { op, left, right } => {
                let l = self.ensure_expr(left)?;
                let r = self.ensure_expr(right)?;
                self.translate_binary(op, l, r, left, right)
            }
            naga::Expression::Unary { op, expr: inner } => {
                let val = self.ensure_expr(inner)?;
                self.translate_unary(op, val, inner)
            }
            naga::Expression::Math {
                fun,
                arg,
                arg1,
                arg2,
                arg3: _,
            } => {
                let a = self.ensure_expr(arg)?;
                let b = arg1.map(|h| self.ensure_expr(h)).transpose()?;
                let c = arg2.map(|h| self.ensure_expr(h)).transpose()?;
                self.translate_math(fun, a, b, c, arg)
            }
            naga::Expression::Select {
                condition,
                accept,
                reject,
            } => {
                let cond = self.ensure_expr(condition)?;
                let acc = self.ensure_expr(accept)?;
                let rej = self.ensure_expr(reject)?;
                self.translate_select(cond, acc, rej)
            }
            naga::Expression::FunctionArgument(idx) => {
                let binding = self
                    .func
                    .arguments
                    .get(idx as usize)
                    .and_then(|a| a.binding.as_ref());
                if let Some(naga::Binding::BuiltIn(builtin)) = binding {
                    self.resolve_builtin(*builtin)
                } else {
                    let dst = self.alloc_ssa(RegFile::GPR);
                    self.push_instr(Instr::new(OpUndef { dst: dst.into() }));
                    Ok(dst.into())
                }
            }
            naga::Expression::GlobalVariable(gv) => {
                let global = &self.module.global_variables[gv];
                if let Some(binding) = &global.binding {
                    let addr = self.alloc_ssa_vec(RegFile::GPR, 2);
                    let buf_idx = binding.group as u8;
                    let base_offset = (binding.binding * 8) as u16;
                    let cbuf = CBufRef {
                        buf: CBuf::Binding(buf_idx),
                        offset: base_offset,
                    };
                    self.push_instr(Instr::new(OpCopy {
                        dst: addr[0].into(),
                        src: Src::from(SrcRef::CBuf(cbuf)),
                    }));
                    let cbuf_hi = CBufRef {
                        buf: CBuf::Binding(buf_idx),
                        offset: base_offset + 4,
                    };
                    self.push_instr(Instr::new(OpCopy {
                        dst: addr[1].into(),
                        src: Src::from(SrcRef::CBuf(cbuf_hi)),
                    }));
                    Ok(addr)
                } else {
                    let dst = self.alloc_ssa(RegFile::GPR);
                    self.push_instr(Instr::new(OpUndef { dst: dst.into() }));
                    Ok(dst.into())
                }
            }
            naga::Expression::LocalVariable(_) => {
                let dst = self.alloc_ssa(RegFile::GPR);
                self.push_instr(Instr::new(OpUndef { dst: dst.into() }));
                Ok(dst.into())
            }
            naga::Expression::Load { pointer } => {
                let addr = self.ensure_expr(pointer)?;
                self.emit_load(addr)
            }
            naga::Expression::Access { base, index } => {
                let base_val = self.ensure_expr(base)?;
                let idx_val = self.ensure_expr(index)?;
                self.emit_access(base_val, idx_val, base)
            }
            naga::Expression::AccessIndex { base, index } => {
                let base_val = self.ensure_expr(base)?;
                self.emit_access_index(base_val, index, base)
            }
            naga::Expression::Compose {
                ty: _,
                ref components,
            } => {
                let comps: Vec<SSARef> = components
                    .iter()
                    .map(|&h| self.ensure_expr(h))
                    .collect::<Result<_, _>>()?;
                let total_comps: u8 = comps.iter().map(|r| r.comps()).sum();
                let dst = self.alloc_ssa_vec(RegFile::GPR, total_comps);
                let mut idx = 0usize;
                for comp in &comps {
                    for c in 0..comp.comps() as usize {
                        self.push_instr(Instr::new(OpCopy {
                            dst: Dst::from(dst[idx]),
                            src: Src::from(comp[c]),
                        }));
                        idx += 1;
                    }
                }
                Ok(dst)
            }
            naga::Expression::Splat { size, value } => {
                let val = self.ensure_expr(value)?;
                let n = match size {
                    naga::VectorSize::Bi => 2u8,
                    naga::VectorSize::Tri => 3,
                    naga::VectorSize::Quad => 4,
                };
                let dst = self.alloc_ssa_vec(RegFile::GPR, n);
                for c in 0..n as usize {
                    self.push_instr(Instr::new(OpCopy {
                        dst: Dst::from(dst[c]),
                        src: Src::from(val[0]),
                    }));
                }
                Ok(dst)
            }
            naga::Expression::Swizzle {
                size,
                vector,
                pattern,
            } => {
                let vec_val = self.ensure_expr(vector)?;
                let n = match size {
                    naga::VectorSize::Bi => 2u8,
                    naga::VectorSize::Tri => 3,
                    naga::VectorSize::Quad => 4,
                };
                let dst = self.alloc_ssa_vec(RegFile::GPR, n);
                for c in 0..n as usize {
                    let idx = match pattern[c] {
                        naga::SwizzleComponent::X => 0usize,
                        naga::SwizzleComponent::Y => 1,
                        naga::SwizzleComponent::Z => 2,
                        naga::SwizzleComponent::W => 3,
                    };
                    if idx < vec_val.comps() as usize {
                        self.push_instr(Instr::new(OpCopy {
                            dst: Dst::from(dst[c]),
                            src: Src::from(vec_val[idx]),
                        }));
                    } else {
                        self.push_instr(Instr::new(OpCopy {
                            dst: Dst::from(dst[c]),
                            src: Src::ZERO,
                        }));
                    }
                }
                Ok(dst)
            }
            naga::Expression::As {
                expr: inner,
                kind,
                convert,
            } => {
                let val = self.ensure_expr(inner)?;
                self.translate_cast(val, kind, convert, inner)
            }
            _ => Err(CompileError::NotImplemented(format!(
                "expression {:?} not yet supported",
                std::mem::discriminant(expr),
            ))),
        }?;

        self.expr_map.insert(handle, result.clone());
        Ok(result)
    }

    fn translate_literal(&mut self, lit: &naga::Literal) -> Result<SSARef, CompileError> {
        match *lit {
            naga::Literal::F32(f) => {
                let dst = self.alloc_ssa(RegFile::GPR);
                self.push_instr(Instr::new(OpCopy {
                    dst: dst.into(),
                    src: Src::new_imm_u32(f.to_bits()),
                }));
                Ok(dst.into())
            }
            naga::Literal::U32(u) => {
                let dst = self.alloc_ssa(RegFile::GPR);
                self.push_instr(Instr::new(OpCopy {
                    dst: dst.into(),
                    src: Src::new_imm_u32(u),
                }));
                Ok(dst.into())
            }
            naga::Literal::I32(i) => {
                let dst = self.alloc_ssa(RegFile::GPR);
                self.push_instr(Instr::new(OpCopy {
                    dst: dst.into(),
                    src: Src::new_imm_u32(i as u32),
                }));
                Ok(dst.into())
            }
            naga::Literal::Bool(b) => {
                let dst = self.alloc_ssa(RegFile::Pred);
                self.push_instr(Instr::new(OpCopy {
                    dst: dst.into(),
                    src: Src::new_imm_bool(b),
                }));
                Ok(dst.into())
            }
            naga::Literal::F64(f) => {
                let bits = f.to_bits();
                let lo = self.alloc_ssa(RegFile::GPR);
                let hi = self.alloc_ssa(RegFile::GPR);
                self.push_instr(Instr::new(OpCopy {
                    dst: lo.into(),
                    src: Src::new_imm_u32(bits as u32),
                }));
                self.push_instr(Instr::new(OpCopy {
                    dst: hi.into(),
                    src: Src::new_imm_u32((bits >> 32) as u32),
                }));
                let dst = self.alloc_ssa_vec(RegFile::GPR, 2);
                self.push_instr(Instr::new(OpCopy {
                    dst: dst[0].into(),
                    src: lo.into(),
                }));
                self.push_instr(Instr::new(OpCopy {
                    dst: dst[1].into(),
                    src: hi.into(),
                }));
                Ok(dst)
            }
            naga::Literal::I64(i) => {
                let bits = i as u64;
                let dst = self.alloc_ssa_vec(RegFile::GPR, 2);
                self.push_instr(Instr::new(OpCopy {
                    dst: dst[0].into(),
                    src: Src::new_imm_u32(bits as u32),
                }));
                self.push_instr(Instr::new(OpCopy {
                    dst: dst[1].into(),
                    src: Src::new_imm_u32((bits >> 32) as u32),
                }));
                Ok(dst)
            }
            naga::Literal::U64(u) => {
                let dst = self.alloc_ssa_vec(RegFile::GPR, 2);
                self.push_instr(Instr::new(OpCopy {
                    dst: dst[0].into(),
                    src: Src::new_imm_u32(u as u32),
                }));
                self.push_instr(Instr::new(OpCopy {
                    dst: dst[1].into(),
                    src: Src::new_imm_u32((u >> 32) as u32),
                }));
                Ok(dst)
            }
            _ => Err(CompileError::NotImplemented(format!(
                "literal {lit:?} not yet supported"
            ))),
        }
    }

    fn translate_zero_value(&mut self, ty: Handle<naga::Type>) -> Result<SSARef, CompileError> {
        let inner = &self.module.types[ty].inner;
        let comps = match *inner {
            naga::TypeInner::Scalar(_) => 1u8,
            naga::TypeInner::Vector { size, .. } => match size {
                naga::VectorSize::Bi => 2,
                naga::VectorSize::Tri => 3,
                naga::VectorSize::Quad => 4,
            },
            _ => 1,
        };
        let dst = self.alloc_ssa_vec(RegFile::GPR, comps);
        for c in 0..comps as usize {
            self.push_instr(Instr::new(OpCopy {
                dst: dst[c].into(),
                src: Src::ZERO,
            }));
        }
        Ok(dst)
    }

    pub(super) fn resolve_expr_type_handle(
        &self,
        handle: Handle<naga::Expression>,
    ) -> Result<Handle<naga::Type>, CompileError> {
        let expr = &self.func.expressions[handle];
        match *expr {
            naga::Expression::GlobalVariable(gv) => Ok(self.module.global_variables[gv].ty),
            naga::Expression::LocalVariable(lv) => Ok(self.func.local_variables[lv].ty),
            naga::Expression::FunctionArgument(idx) => Ok(self.func.arguments[idx as usize].ty),
            naga::Expression::Literal(ref lit) => self.scalar_type_handle(super::lit_scalar(lit)),
            naga::Expression::Binary { left, .. } => self.resolve_expr_type_handle(left),
            naga::Expression::Unary { expr: inner, .. } => self.resolve_expr_type_handle(inner),
            naga::Expression::Constant(c) => Ok(self.module.constants[c].ty),
            naga::Expression::ZeroValue(ty) | naga::Expression::Compose { ty, .. } => Ok(ty),
            naga::Expression::Access { base, .. } | naga::Expression::AccessIndex { base, .. } => {
                let base_ty = self.resolve_expr_type_handle(base)?;
                let base_inner = &self.module.types[base_ty].inner;
                match *base_inner {
                    naga::TypeInner::Array { base, .. } | naga::TypeInner::Pointer { base, .. } => {
                        Ok(base)
                    }
                    naga::TypeInner::Vector { scalar, .. } => self.scalar_type_handle(scalar),
                    _ => Ok(base_ty),
                }
            }
            naga::Expression::Load { pointer } => {
                let ptr_ty = self.resolve_expr_type_handle(pointer)?;
                let ptr_inner = &self.module.types[ptr_ty].inner;
                match *ptr_inner {
                    naga::TypeInner::Pointer { base, .. } => Ok(base),
                    _ => Ok(ptr_ty),
                }
            }
            _ => self.any_type_handle(),
        }
    }

    fn any_type_handle(&self) -> Result<Handle<naga::Type>, CompileError> {
        self.module
            .types
            .iter()
            .next()
            .map(|(h, _)| h)
            .ok_or_else(|| CompileError::InvalidInput("module has no types".into()))
    }

    fn scalar_type_handle(&self, scalar: naga::Scalar) -> Result<Handle<naga::Type>, CompileError> {
        for (handle, ty) in self.module.types.iter() {
            if ty.inner == naga::TypeInner::Scalar(scalar) {
                return Ok(handle);
            }
        }
        self.any_type_handle()
    }

    pub(super) fn is_float_expr(&self, handle: Handle<naga::Expression>) -> bool {
        let expr = &self.func.expressions[handle];
        match *expr {
            naga::Expression::Literal(ref lit) => {
                matches!(lit, naga::Literal::F32(_) | naga::Literal::F64(_))
            }
            naga::Expression::Binary { left, .. } => self.is_float_expr(left),
            naga::Expression::Unary { expr: inner, .. } => self.is_float_expr(inner),
            naga::Expression::Math { arg, .. } => self.is_float_expr(arg),
            _ => {
                let Ok(ty_handle) = self.resolve_expr_type_handle(handle) else {
                    return false;
                };
                let inner = &self.module.types[ty_handle].inner;
                matches!(
                    inner,
                    naga::TypeInner::Scalar(naga::Scalar {
                        kind: naga::ScalarKind::Float,
                        ..
                    }) | naga::TypeInner::Vector {
                        scalar: naga::Scalar {
                            kind: naga::ScalarKind::Float,
                            ..
                        },
                        ..
                    }
                )
            }
        }
    }

    pub(super) fn is_f64_expr(&self, handle: Handle<naga::Expression>) -> bool {
        let expr = &self.func.expressions[handle];
        match *expr {
            naga::Expression::Literal(ref lit) => matches!(lit, naga::Literal::F64(_)),
            naga::Expression::Binary { left, .. } => self.is_f64_expr(left),
            naga::Expression::Unary { expr: inner, .. } => self.is_f64_expr(inner),
            naga::Expression::Math { arg, .. } => self.is_f64_expr(arg),
            _ => {
                let Ok(ty_handle) = self.resolve_expr_type_handle(handle) else {
                    return false;
                };
                let inner = &self.module.types[ty_handle].inner;
                match inner {
                    naga::TypeInner::Scalar(s) => s.kind == naga::ScalarKind::Float && s.width == 8,
                    naga::TypeInner::Vector { scalar, .. } => {
                        scalar.kind == naga::ScalarKind::Float && scalar.width == 8
                    }
                    _ => false,
                }
            }
        }
    }

    fn translate_binary(
        &mut self,
        op: naga::BinaryOperator,
        l: SSARef,
        r: SSARef,
        left_handle: Handle<naga::Expression>,
        _right_handle: Handle<naga::Expression>,
    ) -> Result<SSARef, CompileError> {
        let is_float = self.is_float_expr(left_handle);
        let comps = l.comps().max(1);

        match op {
            naga::BinaryOperator::Add if is_float => {
                self.emit_componentwise(comps, l, r, |s, a, b| {
                    let dst = s.alloc_ssa(RegFile::GPR);
                    s.push_instr(Instr::new(OpFAdd {
                        dst: dst.into(),
                        srcs: [a.into(), b.into()],
                        saturate: false,
                        rnd_mode: FRndMode::NearestEven,
                        ftz: false,
                    }));
                    dst
                })
            }
            naga::BinaryOperator::Add => self.emit_int_componentwise(comps, l, r, |s, a, b| {
                let dst = s.alloc_ssa(RegFile::GPR);
                if s.sm.sm() >= 70 {
                    s.push_instr(Instr::new(OpIAdd3 {
                        dst: dst.into(),
                        srcs: [a.into(), b.into(), Src::ZERO],
                        overflow: [Dst::None, Dst::None],
                    }));
                } else {
                    s.push_instr(Instr::new(OpIAdd2 {
                        dst: dst.into(),
                        srcs: [a.into(), b.into()],
                        carry_out: Dst::None,
                    }));
                }
                dst
            }),
            naga::BinaryOperator::Subtract if is_float => {
                self.emit_componentwise(comps, l, r, |s, a, b| {
                    let dst = s.alloc_ssa(RegFile::GPR);
                    s.push_instr(Instr::new(OpFAdd {
                        dst: dst.into(),
                        srcs: [a.into(), Src::from(b).fneg()],
                        saturate: false,
                        rnd_mode: FRndMode::NearestEven,
                        ftz: false,
                    }));
                    dst
                })
            }
            naga::BinaryOperator::Subtract => {
                self.emit_int_componentwise(comps, l, r, |s, a, b| {
                    let dst = s.alloc_ssa(RegFile::GPR);
                    if s.sm.sm() >= 70 {
                        s.push_instr(Instr::new(OpIAdd3 {
                            dst: dst.into(),
                            srcs: [a.into(), Src::from(b).ineg(), Src::ZERO],
                            overflow: [Dst::None, Dst::None],
                        }));
                    } else {
                        s.push_instr(Instr::new(OpIAdd2 {
                            dst: dst.into(),
                            srcs: [a.into(), Src::from(b).ineg()],
                            carry_out: Dst::None,
                        }));
                    }
                    dst
                })
            }
            naga::BinaryOperator::Multiply if is_float => {
                self.emit_componentwise(comps, l, r, |s, a, b| {
                    let dst = s.alloc_ssa(RegFile::GPR);
                    s.push_instr(Instr::new(OpFMul {
                        dst: dst.into(),
                        srcs: [a.into(), b.into()],
                        saturate: false,
                        rnd_mode: FRndMode::NearestEven,
                        ftz: false,
                        dnz: false,
                    }));
                    dst
                })
            }
            naga::BinaryOperator::Multiply => {
                self.emit_int_componentwise(comps, l, r, |s, a, b| {
                    let dst = s.alloc_ssa(RegFile::GPR);
                    if s.sm.sm() >= 70 {
                        s.push_instr(Instr::new(OpIMad {
                            dst: dst.into(),
                            srcs: [a.into(), b.into(), Src::ZERO],
                            signed: false,
                        }));
                    } else {
                        s.push_instr(Instr::new(OpIMul {
                            dst: dst.into(),
                            srcs: [a.into(), b.into()],
                            signed: [false; 2],
                            high: false,
                        }));
                    }
                    dst
                })
            }
            naga::BinaryOperator::Divide if is_float => {
                self.emit_componentwise(comps, l, r, |s, a, b| {
                    let rcp = s.alloc_ssa(RegFile::GPR);
                    s.push_instr(Instr::new(OpTranscendental {
                        dst: rcp.into(),
                        op: TranscendentalOp::Rcp,
                        src: b.into(),
                    }));
                    let dst = s.alloc_ssa(RegFile::GPR);
                    s.push_instr(Instr::new(OpFMul {
                        dst: dst.into(),
                        srcs: [a.into(), rcp.into()],
                        saturate: false,
                        rnd_mode: FRndMode::NearestEven,
                        ftz: false,
                        dnz: false,
                    }));
                    dst
                })
            }
            naga::BinaryOperator::And => self.emit_int_componentwise(comps, l, r, |s, a, b| {
                let dst = s.alloc_ssa(RegFile::GPR);
                if s.sm.sm() >= 70 {
                    s.push_instr(Instr::new(OpLop3 {
                        dst: dst.into(),
                        srcs: [a.into(), b.into(), Src::ZERO],
                        op: LogicOp3::new_lut(&|x, y, _| x & y),
                    }));
                } else {
                    s.push_instr(Instr::new(OpLop2 {
                        dst: dst.into(),
                        srcs: [a.into(), b.into()],
                        op: LogicOp2::And,
                    }));
                }
                dst
            }),
            naga::BinaryOperator::InclusiveOr => {
                self.emit_int_componentwise(comps, l, r, |s, a, b| {
                    let dst = s.alloc_ssa(RegFile::GPR);
                    if s.sm.sm() >= 70 {
                        s.push_instr(Instr::new(OpLop3 {
                            dst: dst.into(),
                            srcs: [a.into(), b.into(), Src::ZERO],
                            op: LogicOp3::new_lut(&|x, y, _| x | y),
                        }));
                    } else {
                        s.push_instr(Instr::new(OpLop2 {
                            dst: dst.into(),
                            srcs: [a.into(), b.into()],
                            op: LogicOp2::Or,
                        }));
                    }
                    dst
                })
            }
            naga::BinaryOperator::ExclusiveOr => {
                self.emit_int_componentwise(comps, l, r, |s, a, b| {
                    let dst = s.alloc_ssa(RegFile::GPR);
                    if s.sm.sm() >= 70 {
                        s.push_instr(Instr::new(OpLop3 {
                            dst: dst.into(),
                            srcs: [a.into(), b.into(), Src::ZERO],
                            op: LogicOp3::new_lut(&|x, y, _| x ^ y),
                        }));
                    } else {
                        s.push_instr(Instr::new(OpLop2 {
                            dst: dst.into(),
                            srcs: [a.into(), b.into()],
                            op: LogicOp2::Xor,
                        }));
                    }
                    dst
                })
            }
            naga::BinaryOperator::ShiftLeft => {
                self.emit_int_componentwise(comps, l, r, |s, a, b| {
                    let dst = s.alloc_ssa(RegFile::GPR);
                    if s.sm.sm() >= 70 {
                        s.push_instr(Instr::new(OpShf {
                            dst: dst.into(),
                            low: a.into(),
                            high: Src::ZERO,
                            shift: b.into(),
                            right: false,
                            wrap: true,
                            data_type: IntType::I32,
                            dst_high: false,
                        }));
                    } else {
                        s.push_instr(Instr::new(OpShl {
                            dst: dst.into(),
                            src: a.into(),
                            shift: b.into(),
                            wrap: true,
                        }));
                    }
                    dst
                })
            }
            naga::BinaryOperator::ShiftRight => {
                self.emit_int_componentwise(comps, l, r, |s, a, b| {
                    let dst = s.alloc_ssa(RegFile::GPR);
                    if s.sm.sm() >= 70 {
                        s.push_instr(Instr::new(OpShf {
                            dst: dst.into(),
                            low: Src::ZERO,
                            high: a.into(),
                            shift: b.into(),
                            right: true,
                            wrap: true,
                            data_type: IntType::U32,
                            dst_high: true,
                        }));
                    } else {
                        s.push_instr(Instr::new(OpShr {
                            dst: dst.into(),
                            src: a.into(),
                            shift: b.into(),
                            wrap: true,
                            signed: false,
                        }));
                    }
                    dst
                })
            }
            naga::BinaryOperator::Equal if is_float => {
                self.emit_cmp_componentwise(comps, l, r, |s, a, b| {
                    let dst = s.alloc_ssa(RegFile::Pred);
                    s.push_instr(Instr::new(OpFSetP {
                        dst: dst.into(),
                        set_op: PredSetOp::And,
                        cmp_op: FloatCmpOp::OrdEq,
                        srcs: [a.into(), b.into()],
                        accum: SrcRef::True.into(),
                        ftz: false,
                    }));
                    dst
                })
            }
            naga::BinaryOperator::Equal => self.emit_cmp_componentwise(comps, l, r, |s, a, b| {
                let dst = s.alloc_ssa(RegFile::Pred);
                s.push_instr(Instr::new(OpISetP {
                    dst: dst.into(),
                    set_op: PredSetOp::And,
                    cmp_op: IntCmpOp::Eq,
                    cmp_type: IntCmpType::U32,
                    ex: false,
                    srcs: [a.into(), b.into()],
                    accum: true.into(),
                    low_cmp: true.into(),
                }));
                dst
            }),
            naga::BinaryOperator::NotEqual if is_float => {
                self.emit_cmp_componentwise(comps, l, r, |s, a, b| {
                    let dst = s.alloc_ssa(RegFile::Pred);
                    s.push_instr(Instr::new(OpFSetP {
                        dst: dst.into(),
                        set_op: PredSetOp::And,
                        cmp_op: FloatCmpOp::OrdNe,
                        srcs: [a.into(), b.into()],
                        accum: SrcRef::True.into(),
                        ftz: false,
                    }));
                    dst
                })
            }
            naga::BinaryOperator::NotEqual => {
                self.emit_cmp_componentwise(comps, l, r, |s, a, b| {
                    let dst = s.alloc_ssa(RegFile::Pred);
                    s.push_instr(Instr::new(OpISetP {
                        dst: dst.into(),
                        set_op: PredSetOp::And,
                        cmp_op: IntCmpOp::Ne,
                        cmp_type: IntCmpType::U32,
                        ex: false,
                        srcs: [a.into(), b.into()],
                        accum: true.into(),
                        low_cmp: true.into(),
                    }));
                    dst
                })
            }
            naga::BinaryOperator::Less if is_float => {
                self.emit_cmp_componentwise(comps, l, r, |s, a, b| {
                    let dst = s.alloc_ssa(RegFile::Pred);
                    s.push_instr(Instr::new(OpFSetP {
                        dst: dst.into(),
                        set_op: PredSetOp::And,
                        cmp_op: FloatCmpOp::OrdLt,
                        srcs: [a.into(), b.into()],
                        accum: SrcRef::True.into(),
                        ftz: false,
                    }));
                    dst
                })
            }
            naga::BinaryOperator::Less => self.emit_cmp_componentwise(comps, l, r, |s, a, b| {
                let dst = s.alloc_ssa(RegFile::Pred);
                s.push_instr(Instr::new(OpISetP {
                    dst: dst.into(),
                    set_op: PredSetOp::And,
                    cmp_op: IntCmpOp::Lt,
                    cmp_type: IntCmpType::U32,
                    ex: false,
                    srcs: [a.into(), b.into()],
                    accum: true.into(),
                    low_cmp: true.into(),
                }));
                dst
            }),
            naga::BinaryOperator::LessEqual if is_float => {
                self.emit_cmp_componentwise(comps, l, r, |s, a, b| {
                    let dst = s.alloc_ssa(RegFile::Pred);
                    s.push_instr(Instr::new(OpFSetP {
                        dst: dst.into(),
                        set_op: PredSetOp::And,
                        cmp_op: FloatCmpOp::OrdLe,
                        srcs: [a.into(), b.into()],
                        accum: SrcRef::True.into(),
                        ftz: false,
                    }));
                    dst
                })
            }
            naga::BinaryOperator::LessEqual => {
                self.emit_cmp_componentwise(comps, l, r, |s, a, b| {
                    let dst = s.alloc_ssa(RegFile::Pred);
                    s.push_instr(Instr::new(OpISetP {
                        dst: dst.into(),
                        set_op: PredSetOp::And,
                        cmp_op: IntCmpOp::Le,
                        cmp_type: IntCmpType::U32,
                        ex: false,
                        srcs: [a.into(), b.into()],
                        accum: true.into(),
                        low_cmp: true.into(),
                    }));
                    dst
                })
            }
            naga::BinaryOperator::Greater if is_float => {
                self.emit_cmp_componentwise(comps, l, r, |s, a, b| {
                    let dst = s.alloc_ssa(RegFile::Pred);
                    s.push_instr(Instr::new(OpFSetP {
                        dst: dst.into(),
                        set_op: PredSetOp::And,
                        cmp_op: FloatCmpOp::OrdGt,
                        srcs: [a.into(), b.into()],
                        accum: SrcRef::True.into(),
                        ftz: false,
                    }));
                    dst
                })
            }
            naga::BinaryOperator::Greater => self.emit_cmp_componentwise(comps, l, r, |s, a, b| {
                let dst = s.alloc_ssa(RegFile::Pred);
                s.push_instr(Instr::new(OpISetP {
                    dst: dst.into(),
                    set_op: PredSetOp::And,
                    cmp_op: IntCmpOp::Gt,
                    cmp_type: IntCmpType::U32,
                    ex: false,
                    srcs: [a.into(), b.into()],
                    accum: true.into(),
                    low_cmp: true.into(),
                }));
                dst
            }),
            naga::BinaryOperator::GreaterEqual if is_float => {
                self.emit_cmp_componentwise(comps, l, r, |s, a, b| {
                    let dst = s.alloc_ssa(RegFile::Pred);
                    s.push_instr(Instr::new(OpFSetP {
                        dst: dst.into(),
                        set_op: PredSetOp::And,
                        cmp_op: FloatCmpOp::OrdGe,
                        srcs: [a.into(), b.into()],
                        accum: SrcRef::True.into(),
                        ftz: false,
                    }));
                    dst
                })
            }
            naga::BinaryOperator::GreaterEqual => {
                self.emit_cmp_componentwise(comps, l, r, |s, a, b| {
                    let dst = s.alloc_ssa(RegFile::Pred);
                    s.push_instr(Instr::new(OpISetP {
                        dst: dst.into(),
                        set_op: PredSetOp::And,
                        cmp_op: IntCmpOp::Ge,
                        cmp_type: IntCmpType::U32,
                        ex: false,
                        srcs: [a.into(), b.into()],
                        accum: true.into(),
                        low_cmp: true.into(),
                    }));
                    dst
                })
            }
            naga::BinaryOperator::LogicalAnd => {
                let dst = self.alloc_ssa(RegFile::Pred);
                if self.sm.sm() >= 70 {
                    self.push_instr(Instr::new(OpPLop3 {
                        dsts: [dst.into(), Dst::None],
                        srcs: [l[0].into(), r[0].into(), true.into()],
                        ops: [
                            LogicOp3::new_lut(&|x, y, _| x & y),
                            LogicOp3::new_const(false),
                        ],
                    }));
                } else {
                    self.push_instr(Instr::new(OpPSetP {
                        dsts: [dst.into(), Dst::None],
                        ops: [PredSetOp::And, PredSetOp::And],
                        srcs: [l[0].into(), r[0].into(), true.into()],
                    }));
                }
                Ok(dst.into())
            }
            naga::BinaryOperator::LogicalOr => {
                let dst = self.alloc_ssa(RegFile::Pred);
                if self.sm.sm() >= 70 {
                    self.push_instr(Instr::new(OpPLop3 {
                        dsts: [dst.into(), Dst::None],
                        srcs: [l[0].into(), r[0].into(), true.into()],
                        ops: [
                            LogicOp3::new_lut(&|x, y, _| x | y),
                            LogicOp3::new_const(false),
                        ],
                    }));
                } else {
                    self.push_instr(Instr::new(OpPSetP {
                        dsts: [dst.into(), Dst::None],
                        ops: [PredSetOp::Or, PredSetOp::And],
                        srcs: [l[0].into(), r[0].into(), true.into()],
                    }));
                }
                Ok(dst.into())
            }
            _ => Err(CompileError::NotImplemented(format!(
                "binary op {op:?} not yet supported"
            ))),
        }
    }
}
