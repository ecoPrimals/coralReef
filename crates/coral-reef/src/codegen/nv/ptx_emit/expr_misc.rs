// SPDX-License-Identifier: AGPL-3.0-or-later

use std::fmt::Write as _;

use crate::error::CompileError;

use super::PtxEmitter;
use super::types::{MemSpaceKind, PtxVal};

impl PtxEmitter<'_> {
    pub(super) fn eval_literal(&mut self, lit: &naga::Literal) -> Result<PtxVal, CompileError> {
        match *lit {
            naga::Literal::U32(v) => {
                let r = self.alloc_r32();
                writeln!(self.body, "    mov.u32 {}, {v};", r.fmt_operand())
                    .expect("write to String");
                Ok(r)
            }
            naga::Literal::I32(v) => {
                let r = self.alloc_r32();
                let bits = v as u32;
                writeln!(self.body, "    mov.u32 {}, {bits};", r.fmt_operand())
                    .expect("write to String");
                Ok(r)
            }
            naga::Literal::F32(v) => {
                let r = self.alloc_r32();
                let bits = v.to_bits();
                writeln!(self.body, "    mov.b32 {}, 0F{bits:08X};", r.fmt_operand())
                    .expect("write to String");
                Ok(r)
            }
            naga::Literal::F64(v) => {
                let r = self.alloc_rd64();
                let bits = v.to_bits();
                writeln!(self.body, "    mov.b64 {}, 0D{bits:016X};", r.fmt_operand())
                    .expect("write to String");
                Ok(r)
            }
            naga::Literal::Bool(v) => {
                let r = self.alloc_pred();
                let val = u32::from(v);
                let tmp = self.alloc_r32();
                writeln!(self.body, "    mov.u32 {}, {val};", tmp.fmt_operand())
                    .expect("write to String");
                writeln!(
                    self.body,
                    "    setp.ne.u32 {}, {}, 0;",
                    r.fmt_operand(),
                    tmp.fmt_operand()
                )
                .expect("write to String");
                Ok(r)
            }
            naga::Literal::U64(v) => {
                let r = self.alloc_rd64();
                writeln!(self.body, "    mov.u64 {}, {v};", r.fmt_operand())
                    .expect("write to String");
                Ok(r)
            }
            naga::Literal::I64(v) => {
                let r = self.alloc_rd64();
                let bits = v as u64;
                writeln!(self.body, "    mov.u64 {}, {bits};", r.fmt_operand())
                    .expect("write to String");
                Ok(r)
            }
            _ => Err(CompileError::NotImplemented(
                format!("PTX literal: {lit:?}").into(),
            )),
        }
    }

    pub(super) fn eval_const_expr(
        &mut self,
        h: naga::Handle<naga::Expression>,
    ) -> Result<PtxVal, CompileError> {
        let expr = &self.module.global_expressions[h];
        match *expr {
            naga::Expression::Literal(ref lit) => self.eval_literal(lit),
            naga::Expression::ZeroValue(ty) => self.eval_zero(ty),
            naga::Expression::Compose {
                ty: _,
                ref components,
            } => {
                let mut vals = Vec::with_capacity(components.len());
                for &c in components {
                    vals.push(self.eval_const_expr(c)?);
                }
                Ok(PtxVal::Vec(vals))
            }
            _ => {
                let r = self.alloc_r32();
                writeln!(self.body, "    mov.u32 {}, 0;", r.fmt_operand())
                    .expect("write to String");
                Ok(r)
            }
        }
    }

    pub(super) fn eval_zero(
        &mut self,
        ty: naga::Handle<naga::Type>,
    ) -> Result<PtxVal, CompileError> {
        match self.inner_type(ty) {
            naga::TypeInner::Scalar(s) => {
                let r = self.alloc_for_scalar(*s);
                self.zero_val(&r);
                Ok(r)
            }
            naga::TypeInner::Vector { size, scalar } => {
                let n = *size as usize;
                let s = *scalar;
                let mut components = Vec::with_capacity(n);
                for _ in 0..n {
                    let r = self.alloc_for_scalar(s);
                    self.zero_val(&r);
                    components.push(r);
                }
                Ok(PtxVal::Vec(components))
            }
            _ => {
                let r = self.alloc_r32();
                self.zero_val(&r);
                Ok(r)
            }
        }
    }

    pub(super) fn eval_load(
        &mut self,
        pointer: naga::Handle<naga::Expression>,
    ) -> Result<PtxVal, CompileError> {
        if let Some(lv_handle) = self.expr_is_local_var(pointer) {
            if let Some(val) = self.locals.get(&lv_handle).cloned() {
                return Ok(val);
            }
        }
        if let Some((lv_handle, comp)) = self.expr_is_local_var_component(pointer) {
            if let Some(local) = self.locals.get(&lv_handle).cloned() {
                return Ok(local.component(comp).clone());
            }
        }

        let (addr, mem_space) = self.eval_pointer(pointer)?;

        let expr_ty = self.resolve_expr_type_handle(pointer);
        let pointee_ty = match self.inner_type(expr_ty) {
            naga::TypeInner::Pointer { base, .. } => *base,
            _ => {
                // Access/AccessIndex on a pointer resolves to the element type
                // directly in our manual resolution. Use it as the pointee type.
                expr_ty
            }
        };

        let space = if mem_space == MemSpaceKind::Shared {
            "shared"
        } else {
            "global"
        };

        let inner = self.inner_type(pointee_ty).clone();
        match inner {
            naga::TypeInner::Scalar(s) => {
                let dst = self.alloc_for_scalar(s);
                writeln!(
                    self.body,
                    "    ld.{space}.{} {}, [{}];",
                    Self::ptx_mem_suffix(s),
                    dst.fmt_operand(),
                    addr.fmt_operand(),
                )
                .expect("write to String");
                Ok(dst)
            }
            naga::TypeInner::Vector { size, scalar } => {
                let n = size as usize;
                let s = scalar;
                let mut components = Vec::with_capacity(n);
                for i in 0..n {
                    let dst = self.alloc_for_scalar(s);
                    let offset = i as u32 * u32::from(s.width);
                    if offset == 0 {
                        writeln!(
                            self.body,
                            "    ld.{space}.{} {}, [{}];",
                            Self::ptx_mem_suffix(s),
                            dst.fmt_operand(),
                            addr.fmt_operand(),
                        )
                        .expect("write to String");
                    } else {
                        let off_addr = self.alloc_rd64();
                        writeln!(
                            self.body,
                            "    add.u64 {}, {}, {offset};",
                            off_addr.fmt_operand(),
                            addr.fmt_operand(),
                        )
                        .expect("write to String");
                        writeln!(
                            self.body,
                            "    ld.{space}.{} {}, [{}];",
                            Self::ptx_mem_suffix(s),
                            dst.fmt_operand(),
                            off_addr.fmt_operand(),
                        )
                        .expect("write to String");
                    }
                    components.push(dst);
                }
                Ok(PtxVal::Vec(components))
            }
            _ => {
                let dst = self.alloc_r32();
                writeln!(
                    self.body,
                    "    ld.{space}.u32 {}, [{}];",
                    dst.fmt_operand(),
                    addr.fmt_operand(),
                )
                .expect("write to String");
                Ok(dst)
            }
        }
    }

    pub(super) fn eval_access(
        &mut self,
        base: naga::Handle<naga::Expression>,
        index: naga::Handle<naga::Expression>,
    ) -> Result<PtxVal, CompileError> {
        let base_ty = self.resolve_expr_type(base);
        if matches!(base_ty, naga::TypeInner::Vector { .. }) {
            let vec_val = self.eval_expr(base)?;
            let idx_val = self.eval_expr(index)?;
            // Dynamic component access on vectors — use indexing
            // For simplicity, this emits a series of selects. Common case
            // is small vectors (2-4 elements).
            if let PtxVal::Vec(ref components) = vec_val {
                if components.len() <= 4 {
                    let result = self.alloc_r32();
                    writeln!(
                        self.body,
                        "    mov.u32 {}, {};",
                        result.fmt_operand(),
                        components[0].fmt_operand()
                    )
                    .expect("write to String");
                    for (i, comp) in components.iter().enumerate().skip(1) {
                        let pred = self.alloc_pred();
                        writeln!(
                            self.body,
                            "    setp.eq.u32 {}, {}, {};",
                            pred.fmt_operand(),
                            idx_val.fmt_operand(),
                            i,
                        )
                        .expect("write to String");
                        writeln!(
                            self.body,
                            "    @{} mov.u32 {}, {};",
                            pred.fmt_operand(),
                            result.fmt_operand(),
                            comp.fmt_operand(),
                        )
                        .expect("write to String");
                    }
                    return Ok(result);
                }
            }
        }

        // Pointer access — compute address
        let (base_addr, _space) = self.eval_pointer(base)?;
        let idx_val = self.eval_expr(index)?;
        let stride = self.pointer_element_stride(base);
        let addr = self.compute_element_addr(&base_addr, &idx_val, stride);
        // Return as a pointer (address in rd64 register)
        // The Load expression will dereference it later
        Ok(addr)
    }

    pub(super) fn eval_access_index(
        &mut self,
        _h: naga::Handle<naga::Expression>,
        base: naga::Handle<naga::Expression>,
        index: u32,
    ) -> Result<PtxVal, CompileError> {
        let base_ty = self.resolve_expr_type(base);
        match base_ty {
            naga::TypeInner::Vector { .. } => {
                let vec_val = self.eval_expr(base)?;
                Ok(vec_val.component(index as usize).clone())
            }
            naga::TypeInner::Struct { .. } => {
                let base_val = self.eval_expr(base)?;
                if let PtxVal::Vec(ref components) = base_val {
                    if (index as usize) < components.len() {
                        return Ok(components[index as usize].clone());
                    }
                }
                Ok(base_val)
            }
            _ => {
                // Pointer access — evaluate as pointer
                let base_val = self.eval_expr(base)?;
                Ok(base_val)
            }
        }
    }

    pub(super) fn eval_array_length(
        &mut self,
        ptr_expr: naga::Handle<naga::Expression>,
    ) -> Result<PtxVal, CompileError> {
        let gv_handle = self.find_global_variable(ptr_expr);
        let gv_handle = gv_handle.ok_or_else(|| {
            CompileError::NotImplemented("PTX arrayLength: cannot resolve buffer".into())
        })?;

        let binding_idx = self.binding_index(gv_handle).ok_or_else(|| {
            CompileError::NotImplemented("PTX arrayLength: unbound global variable".into())
        })?;

        let stride = self.bindings[binding_idx].element_stride;
        let idx = self.bindings[binding_idx].binding;
        let size_reg = self.alloc_rd64();
        writeln!(
            self.body,
            "    ld.param.u64 {}, [_buf{idx}_size];",
            size_reg.fmt_operand()
        )
        .expect("write to String");

        let result_64 = self.alloc_rd64();
        if stride.is_power_of_two() && stride > 1 {
            let shift = stride.trailing_zeros();
            writeln!(
                self.body,
                "    shr.u64 {}, {}, {shift};",
                result_64.fmt_operand(),
                size_reg.fmt_operand(),
            )
            .expect("write to String");
        } else if stride == 1 {
            writeln!(
                self.body,
                "    mov.u64 {}, {};",
                result_64.fmt_operand(),
                size_reg.fmt_operand(),
            )
            .expect("write to String");
        } else {
            let stride_reg = self.alloc_rd64();
            writeln!(
                self.body,
                "    mov.u64 {}, {stride};",
                stride_reg.fmt_operand()
            )
            .expect("write to String");
            writeln!(
                self.body,
                "    div.u64 {}, {}, {};",
                result_64.fmt_operand(),
                size_reg.fmt_operand(),
                stride_reg.fmt_operand(),
            )
            .expect("write to String");
        }

        let result = self.alloc_r32();
        writeln!(
            self.body,
            "    cvt.u32.u64 {}, {};",
            result.fmt_operand(),
            result_64.fmt_operand(),
        )
        .expect("write to String");
        Ok(result)
    }

    fn find_global_variable(
        &self,
        h: naga::Handle<naga::Expression>,
    ) -> Option<naga::Handle<naga::GlobalVariable>> {
        match self.func.expressions[h] {
            naga::Expression::GlobalVariable(gv) => Some(gv),
            naga::Expression::AccessIndex { base, .. } | naga::Expression::Access { base, .. } => {
                self.find_global_variable(base)
            }
            _ => None,
        }
    }
}
