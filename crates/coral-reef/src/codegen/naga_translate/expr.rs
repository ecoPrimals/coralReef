// SPDX-License-Identifier: AGPL-3.0-only
//! Expression translation for Naga IR → coralReef IR.
//!
//! ## OpUndef for pointer/reference slots
//!
//! Naga expressions that represent memory locations (pointers, references,
//! uniform bindings) are translated into `OpUndef` placeholders. These SSA
//! values act as keys in `expr_map` to identify the memory location, but
//! carry no runtime data — actual values are read via `Load` / written via
//! `Store`. This is intentional: Naga's type system distinguishes value and
//! reference types, and we preserve that distinction in the IR.
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
                self.translate_global_expr(expr_handle)
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
                if let Some(ref args) = self.inline_args {
                    if let Some(&slot) = self.inline_ptr_arg_slots.get(&idx) {
                        self.expr_to_var
                            .insert(handle, super::func::VarRef::Full(slot));
                        let placeholder = self.alloc_ssa(RegFile::GPR);
                        self.push_instr(Instr::new(OpUndef {
                            dst: placeholder.into(),
                        }));
                        Ok(placeholder.into())
                    } else {
                        Ok(args[idx as usize].clone())
                    }
                } else {
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
            }
            naga::Expression::GlobalVariable(gv) => {
                let global = &self.module.global_variables[gv];
                if let Some(binding) = &global.binding {
                    if global.space == naga::AddressSpace::Uniform {
                        // Uniform buffers: data is directly in CBuf. Record the
                        // base reference; actual reads happen in Load via CBuf.
                        let buf_idx = binding.group as u8;
                        self.uniform_refs.insert(handle, (buf_idx, 0));
                        let dummy = self.alloc_ssa(RegFile::GPR);
                        self.push_instr(Instr::new(OpUndef { dst: dummy.into() }));
                        Ok(dummy.into())
                    } else {
                        // Storage buffers: CBuf holds descriptor address
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
                    }
                } else {
                    let dst = self.alloc_ssa(RegFile::GPR);
                    self.push_instr(Instr::new(OpUndef { dst: dst.into() }));
                    Ok(dst.into())
                }
            }
            naga::Expression::LocalVariable(lv) => {
                if let Some(&slot_id) = self.local_var_slots.get(&lv) {
                    self.expr_to_var
                        .insert(handle, super::func::VarRef::Full(slot_id));
                    let placeholder = self.alloc_ssa(RegFile::GPR);
                    self.push_instr(Instr::new(OpUndef {
                        dst: placeholder.into(),
                    }));
                    Ok(placeholder.into())
                } else {
                    let lv_ty = self.func.local_variables[lv].ty;
                    let comps = self.type_reg_comps(lv_ty);
                    if comps > 0 {
                        let ssa = self.alloc_ssa_vec(RegFile::GPR, comps);
                        for c in 0..comps as usize {
                            self.push_instr(Instr::new(OpCopy {
                                dst: ssa[c].into(),
                                src: Src::ZERO,
                            }));
                        }
                        let slot_id = self.var_storage.len();
                        self.var_storage.push(ssa);
                        self.expr_to_var
                            .insert(handle, super::func::VarRef::Full(slot_id));
                        let placeholder = self.alloc_ssa(RegFile::GPR);
                        self.push_instr(Instr::new(OpUndef {
                            dst: placeholder.into(),
                        }));
                        Ok(placeholder.into())
                    } else {
                        let dst = self.alloc_ssa(RegFile::GPR);
                        self.push_instr(Instr::new(OpUndef { dst: dst.into() }));
                        Ok(dst.into())
                    }
                }
            }
            naga::Expression::Load { pointer } => {
                if let Some(var_ref) = self.expr_to_var.get(&pointer).copied() {
                    let result = match var_ref {
                        super::func::VarRef::Full(slot) => self.var_storage[slot].clone(),
                        super::func::VarRef::Component(slot, comp) => {
                            let full = &self.var_storage[slot];
                            full[comp as usize].into()
                        }
                    };
                    self.expr_map.insert(handle, result.clone());
                    return Ok(result);
                }
                if let Some(&(cbuf_idx, offset)) = self.uniform_refs.get(&pointer) {
                    return self.emit_uniform_load(pointer, cbuf_idx, offset);
                } else {
                    let addr = self.ensure_expr(pointer)?;
                    let ptr_ty = self.resolve_expr_type_handle(pointer)?;
                    let load_ty = match &self.module.types[ptr_ty].inner {
                        naga::TypeInner::Pointer { base, .. } => *base,
                        _ => ptr_ty,
                    };
                    let is_64bit = match &self.module.types[load_ty].inner {
                        naga::TypeInner::Scalar(s) => s.width == 8,
                        _ => false,
                    };
                    if is_64bit {
                        self.emit_load_f64(addr)
                    } else {
                        self.emit_load(addr)
                    }
                }
            }
            naga::Expression::Access { base, index } => {
                if let Some(&(cbuf_idx, base_offset)) = self.uniform_refs.get(&base) {
                    let stride = self.type_stride(base)?;
                    let idx_val = self.ensure_expr(index)?;
                    return self.emit_uniform_dynamic_access(
                        handle,
                        cbuf_idx,
                        base_offset,
                        stride,
                        idx_val,
                    );
                }
                let base_val = self.ensure_expr(base)?;
                let idx_val = self.ensure_expr(index)?;
                self.emit_access(base_val, idx_val, base)
            }
            naga::Expression::AccessIndex { base, index } => {
                if let Some(&(cbuf_idx, base_offset)) = self.uniform_refs.get(&base) {
                    let field_offset = self.uniform_field_byte_offset(base, index)?;
                    let total_offset = base_offset + field_offset;
                    self.uniform_refs.insert(handle, (cbuf_idx, total_offset));
                    let dummy = self.alloc_ssa(RegFile::GPR);
                    self.push_instr(Instr::new(OpUndef { dst: dummy.into() }));
                    Ok(dummy.into())
                } else if let Some(var_ref) = self.expr_to_var.get(&base).copied() {
                    let sub_ref = match var_ref {
                        super::func::VarRef::Full(slot) => {
                            super::func::VarRef::Component(slot, index)
                        }
                        super::func::VarRef::Component(slot, base_comp) => {
                            super::func::VarRef::Component(slot, base_comp + index)
                        }
                    };
                    self.expr_to_var.insert(handle, sub_ref);
                    let placeholder = self.alloc_ssa(RegFile::GPR);
                    self.push_instr(Instr::new(OpUndef {
                        dst: placeholder.into(),
                    }));
                    Ok(placeholder.into())
                } else {
                    let base_val = self.ensure_expr(base)?;
                    self.emit_access_index(base_val, index, base)
                }
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
                let per_elem = val.comps();
                let total_comps = n * per_elem;
                let dst = self.alloc_ssa_vec(RegFile::GPR, total_comps);
                for c in 0..n as usize {
                    for p in 0..per_elem as usize {
                        self.push_instr(Instr::new(OpCopy {
                            dst: Dst::from(dst[c * per_elem as usize + p]),
                            src: Src::from(val[p]),
                        }));
                    }
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
            naga::Expression::ArrayLength(ptr_expr) => self.translate_array_length(ptr_expr),
            naga::Expression::Relational { fun, argument } => {
                let arg = self.ensure_expr(argument)?;
                self.translate_relational(fun, arg, argument)
            }
            naga::Expression::CallResult(_) => {
                let dst = self.alloc_ssa(RegFile::GPR);
                self.push_instr(Instr::new(OpUndef { dst: dst.into() }));
                Ok(dst.into())
            }
            _ => Err(CompileError::NotImplemented(
                format!(
                    "expression {:?} not yet supported",
                    std::mem::discriminant(expr),
                )
                .into(),
            )),
        }?;

        self.expr_map.insert(handle, result.clone());
        Ok(result)
    }

    /// Evaluate a global (module-scope) expression to SSA.
    /// Handles Literal, ZeroValue, Compose, Constant (recursive), and Splat.
    pub(super) fn translate_global_expr(
        &mut self,
        handle: Handle<naga::Expression>,
    ) -> Result<SSARef, CompileError> {
        let init_expr = &self.module.global_expressions[handle];
        match *init_expr {
            naga::Expression::Literal(ref lit) => self.translate_literal(lit),
            naga::Expression::ZeroValue(ty) => self.translate_zero_value(ty),
            naga::Expression::Constant(c) => {
                let inner_handle = self.module.constants[c].init;
                self.translate_global_expr(inner_handle)
            }
            naga::Expression::Compose {
                ty: _,
                ref components,
            } => {
                let comps: Vec<SSARef> = components
                    .iter()
                    .map(|&h| self.translate_global_expr(h))
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
                let val = self.translate_global_expr(value)?;
                let n = match size {
                    naga::VectorSize::Bi => 2u8,
                    naga::VectorSize::Tri => 3,
                    naga::VectorSize::Quad => 4,
                };
                let per_elem = val.comps();
                let total_comps = n * per_elem;
                let dst = self.alloc_ssa_vec(RegFile::GPR, total_comps);
                for c in 0..n as usize {
                    for p in 0..per_elem as usize {
                        self.push_instr(Instr::new(OpCopy {
                            dst: Dst::from(dst[c * per_elem as usize + p]),
                            src: Src::from(val[p]),
                        }));
                    }
                }
                Ok(dst)
            }
            _ => Err(CompileError::NotImplemented(
                format!(
                    "global expression {:?} not yet supported as constant initializer",
                    std::mem::discriminant(init_expr),
                )
                .into(),
            )),
        }
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
            _ => Err(CompileError::NotImplemented(
                format!("literal {lit:?} not yet supported").into(),
            )),
        }
    }

    fn translate_zero_value(&mut self, ty: Handle<naga::Type>) -> Result<SSARef, CompileError> {
        let comps = self.type_reg_comps(ty);
        let comps = if comps == 0 {
            let inner = &self.module.types[ty].inner;
            match *inner {
                naga::TypeInner::Scalar(_) => 1u8,
                naga::TypeInner::Vector { size, .. } => match size {
                    naga::VectorSize::Bi => 2,
                    naga::VectorSize::Tri => 3,
                    naga::VectorSize::Quad => 4,
                },
                _ => 1,
            }
        } else {
            comps
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
            naga::Expression::As { kind, convert, .. } => {
                let width = convert.unwrap_or(4);
                self.scalar_type_handle(naga::Scalar { kind, width })
            }
            naga::Expression::Math { arg, .. } => self.resolve_expr_type_handle(arg),
            naga::Expression::Select { accept, .. } => self.resolve_expr_type_handle(accept),
            naga::Expression::Splat { value, .. } => self.resolve_expr_type_handle(value),
            naga::Expression::Swizzle { vector, .. } => self.resolve_expr_type_handle(vector),
            naga::Expression::Relational { argument, .. } => {
                self.resolve_expr_type_handle(argument)
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

    fn uniform_field_byte_offset(
        &self,
        base: Handle<naga::Expression>,
        field_index: u32,
    ) -> Result<u16, CompileError> {
        let ty_handle = self.resolve_expr_type_handle(base)?;
        let inner = &self.module.types[ty_handle].inner;
        let members = match inner {
            naga::TypeInner::Struct { members, .. } => members,
            naga::TypeInner::Pointer { base, .. } => match &self.module.types[*base].inner {
                naga::TypeInner::Struct { members, .. } => members,
                _ => return Ok(field_index as u16 * 4),
            },
            _ => return Ok(field_index as u16 * 4),
        };
        let member = members.get(field_index as usize).ok_or_else(|| {
            CompileError::InvalidInput(
                format!(
                    "uniform struct field index {} out of range (struct has {} members)",
                    field_index,
                    members.len()
                )
                .into(),
            )
        })?;
        Ok(member.offset as u16)
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

    pub(super) fn is_signed_int_expr(&self, handle: Handle<naga::Expression>) -> bool {
        let Ok(ty_handle) = self.resolve_expr_type_handle(handle) else {
            return false;
        };
        let inner = &self.module.types[ty_handle].inner;
        matches!(
            inner,
            naga::TypeInner::Scalar(naga::Scalar {
                kind: naga::ScalarKind::Sint,
                ..
            }) | naga::TypeInner::Vector {
                scalar: naga::Scalar {
                    kind: naga::ScalarKind::Sint,
                    ..
                },
                ..
            }
        )
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
}
