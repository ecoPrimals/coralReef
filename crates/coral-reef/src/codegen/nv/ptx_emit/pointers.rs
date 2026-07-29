// SPDX-License-Identifier: AGPL-3.0-or-later

use crate::error::CompileError;

use super::PtxEmitter;
use super::types::{MemSpaceKind, PtxVal};

impl PtxEmitter<'_> {
    pub(super) fn eval_pointer(
        &mut self,
        h: naga::Handle<naga::Expression>,
    ) -> Result<(PtxVal, MemSpaceKind), CompileError> {
        let expr = self.func.expressions[h].clone();
        match expr {
            naga::Expression::GlobalVariable(gv) => {
                let global = &self.module.global_variables[gv];
                if global.space == naga::AddressSpace::WorkGroup {
                    let sv = self.shared_var(gv);
                    let offset = sv.map_or(0, |s| s.offset);
                    let addr = self.alloc_rd64();
                    writeln_ptx!(self.body, "    mov.u64 {}, _shared;", addr.fmt_operand());
                    if offset > 0 {
                        writeln_ptx!(
                            self.body,
                            "    add.u64 {0}, {0}, {offset};",
                            addr.fmt_operand()
                        );
                    }
                    return Ok((addr, MemSpaceKind::Shared));
                }
                if let Some((ptr_reg, _)) = self.gv_ptr_regs.get(&gv).cloned() {
                    Ok((ptr_reg, MemSpaceKind::Global))
                } else {
                    Err(CompileError::NotImplemented(
                        "PTX: unbound global variable".into(),
                    ))
                }
            }
            naga::Expression::Access { base, index } => {
                let (base_addr, space) = self.eval_pointer(base)?;
                let idx_val = self.eval_expr(index)?;
                let stride = self.pointer_element_stride(base);
                let addr = self.compute_element_addr(&base_addr, &idx_val, stride);
                Ok((addr, space))
            }
            naga::Expression::AccessIndex { base, index } => {
                let (base_addr, space) = self.eval_pointer(base)?;
                let offset = self.access_index_byte_offset(base, index);
                if offset == 0 {
                    return Ok((base_addr, space));
                }
                let addr = self.alloc_rd64();
                writeln_ptx!(
                    self.body,
                    "    add.u64 {}, {}, {offset};",
                    addr.fmt_operand(),
                    base_addr.fmt_operand(),
                );
                Ok((addr, space))
            }
            _ => {
                if let Some(cached) = self.values.get(&h).cloned() {
                    if cached.is_64bit() {
                        return Ok((cached, MemSpaceKind::Global));
                    }
                }
                Err(CompileError::NotImplemented(
                    format!("PTX pointer expression: {:?}", self.func.expressions[h]).into(),
                ))
            }
        }
    }

    pub(super) fn pointer_element_stride(&self, ptr_expr: naga::Handle<naga::Expression>) -> u32 {
        let ty = self.resolve_expr_type(ptr_expr);
        match ty {
            naga::TypeInner::Pointer { base, .. } => match self.inner_type(*base) {
                naga::TypeInner::Array { stride, .. } => *stride,
                naga::TypeInner::Vector { scalar, .. } => u32::from(scalar.width),
                _ => 4,
            },
            naga::TypeInner::ValuePointer { scalar, .. } => u32::from(scalar.width),
            _ => 4,
        }
    }

    pub(super) fn access_index_byte_offset(
        &self,
        base: naga::Handle<naga::Expression>,
        index: u32,
    ) -> u32 {
        let ty = self.resolve_expr_type(base);
        match ty {
            naga::TypeInner::Pointer { base: base_ty, .. } => match self.inner_type(*base_ty) {
                naga::TypeInner::Struct { members, .. } => {
                    members.get(index as usize).map_or(0, |m| m.offset)
                }
                naga::TypeInner::Array { stride, .. } => index * stride,
                naga::TypeInner::Vector { scalar, .. } => index * u32::from(scalar.width),
                _ => index * 4,
            },
            _ => index * 4,
        }
    }

    pub(super) fn compute_element_addr(
        &mut self,
        base: &PtxVal,
        index: &PtxVal,
        stride: u32,
    ) -> PtxVal {
        let idx64 = self.alloc_rd64();
        let offset = self.alloc_rd64();
        let addr = self.alloc_rd64();

        writeln_ptx!(
            self.body,
            "    cvt.u64.u32 {}, {};",
            idx64.fmt_operand(),
            index.fmt_operand(),
        );

        if stride.is_power_of_two() && stride > 1 {
            let shift = stride.trailing_zeros();
            writeln_ptx!(
                self.body,
                "    shl.b64 {}, {}, {shift};",
                offset.fmt_operand(),
                idx64.fmt_operand(),
            );
        } else if stride == 1 {
            writeln_ptx!(
                self.body,
                "    mov.u64 {}, {};",
                offset.fmt_operand(),
                idx64.fmt_operand(),
            );
        } else {
            let stride_reg = self.alloc_rd64();
            writeln_ptx!(
                self.body,
                "    mov.u64 {}, {stride};",
                stride_reg.fmt_operand()
            );
            writeln_ptx!(
                self.body,
                "    mul.lo.u64 {}, {}, {};",
                offset.fmt_operand(),
                idx64.fmt_operand(),
                stride_reg.fmt_operand(),
            );
        }

        writeln_ptx!(
            self.body,
            "    add.u64 {}, {}, {};",
            addr.fmt_operand(),
            base.fmt_operand(),
            offset.fmt_operand(),
        );

        addr
    }

    pub(super) fn expr_is_local_var(
        &self,
        h: naga::Handle<naga::Expression>,
    ) -> Option<naga::Handle<naga::LocalVariable>> {
        match self.func.expressions[h] {
            naga::Expression::LocalVariable(lv) => Some(lv),
            _ => None,
        }
    }

    pub(super) fn expr_is_local_var_component(
        &self,
        h: naga::Handle<naga::Expression>,
    ) -> Option<(naga::Handle<naga::LocalVariable>, usize)> {
        match self.func.expressions[h] {
            naga::Expression::AccessIndex { base, index } => {
                if let naga::Expression::LocalVariable(lv) = self.func.expressions[base] {
                    Some((lv, index as usize))
                } else {
                    None
                }
            }
            _ => None,
        }
    }
}
