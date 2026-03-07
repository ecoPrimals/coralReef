// SPDX-License-Identifier: AGPL-3.0-only
//! Memory access, load, store, and address computation for Naga translation.
#![allow(clippy::wildcard_imports)]
use super::super::ir::*;
use super::func::{FuncTranslator, VarRef};
use crate::error::CompileError;
use naga::Handle;

impl<'a, 'b> FuncTranslator<'a, 'b> {
    pub(super) fn emit_store(
        &mut self,
        pointer: Handle<naga::Expression>,
        value: Handle<naga::Expression>,
    ) -> Result<(), CompileError> {
        if let Some(var_ref) = self.expr_to_var.get(&pointer).copied() {
            let val_ssa = self
                .expr_map
                .get(&value)
                .cloned()
                .ok_or_else(|| CompileError::InvalidInput("store value not resolved".into()))?;
            match var_ref {
                VarRef::Full(slot) => {
                    self.var_storage[slot] = val_ssa;
                }
                VarRef::Component(slot, comp) => {
                    let old = self.var_storage[slot].clone();
                    let n = old.comps();
                    let new_ssa = self.alloc_ssa_vec(RegFile::GPR, n);
                    for i in 0..n as usize {
                        if i == comp as usize {
                            self.push_instr(Instr::new(OpCopy {
                                dst: new_ssa[i].into(),
                                src: Src::from(val_ssa[0]),
                            }));
                        } else {
                            self.push_instr(Instr::new(OpCopy {
                                dst: new_ssa[i].into(),
                                src: Src::from(old[i]),
                            }));
                        }
                    }
                    self.var_storage[slot] = new_ssa;
                }
            }
            return Ok(());
        }

        let addr = self
            .expr_map
            .get(&pointer)
            .cloned()
            .ok_or_else(|| CompileError::InvalidInput("store pointer not resolved".into()))?;
        let val = self
            .expr_map
            .get(&value)
            .cloned()
            .ok_or_else(|| CompileError::InvalidInput("store value not resolved".into()))?;

        for c in 0..val.comps() as usize {
            self.push_instr(Instr::new(OpSt {
                addr: addr[0].into(),
                data: val[c].into(),
                offset: (c as i32) * 4,
                stride: OffsetStride::X1,
                access: super::mem_access_global_b32(),
            }));
        }
        Ok(())
    }

    pub(super) fn emit_load(&mut self, addr: SSARef) -> Result<SSARef, CompileError> {
        let dst = self.alloc_ssa(RegFile::GPR);
        self.push_instr(Instr::new(OpLd {
            dst: dst.into(),
            addr: addr[0].into(),
            offset: 0,
            stride: OffsetStride::X1,
            access: super::mem_access_global_b32(),
        }));
        Ok(dst.into())
    }

    pub(super) fn emit_load_f64(&mut self, addr: SSARef) -> Result<SSARef, CompileError> {
        let dst = self.alloc_ssa_vec(RegFile::GPR, 2);
        self.push_instr(Instr::new(OpLd {
            dst: dst[0].into(),
            addr: addr[0].into(),
            offset: 0,
            stride: OffsetStride::X1,
            access: super::mem_access_global_b32(),
        }));
        self.push_instr(Instr::new(OpLd {
            dst: dst[1].into(),
            addr: addr[0].into(),
            offset: 4,
            stride: OffsetStride::X1,
            access: super::mem_access_global_b32(),
        }));
        Ok(dst)
    }

    /// Load a value from a uniform buffer (CBuf) at a known offset.
    ///
    /// Handles scalars (32/64-bit), vectors, and matrices by emitting
    /// consecutive `OpCopy` from `SrcRef::CBuf` at increasing offsets.
    pub(super) fn emit_uniform_load(
        &mut self,
        pointer: Handle<naga::Expression>,
        cbuf_idx: u8,
        offset: u16,
    ) -> Result<SSARef, CompileError> {
        let ptr_ty = self.resolve_expr_type_handle(pointer)?;
        let load_ty = match &self.module.types[ptr_ty].inner {
            naga::TypeInner::Pointer { base, .. } => *base,
            _ => ptr_ty,
        };
        let inner = &self.module.types[load_ty].inner;

        match *inner {
            naga::TypeInner::Scalar(s) if s.width == 8 => {
                let dst = self.alloc_ssa_vec(RegFile::GPR, 2);
                self.emit_cbuf_copy(dst[0], cbuf_idx, offset);
                self.emit_cbuf_copy(dst[1], cbuf_idx, offset + 4);
                Ok(dst)
            }
            naga::TypeInner::Scalar(_) => {
                let dst = self.alloc_ssa(RegFile::GPR);
                self.emit_cbuf_copy(dst, cbuf_idx, offset);
                Ok(dst.into())
            }
            naga::TypeInner::Vector { size, scalar } => {
                let comps = size as u8;
                let comp_bytes = scalar.width as u16;
                if scalar.width == 8 {
                    let dst = self.alloc_ssa_vec(RegFile::GPR, comps * 2);
                    for i in 0..comps as u16 {
                        let comp_off = offset + i * comp_bytes;
                        self.emit_cbuf_copy(dst[(i * 2) as usize], cbuf_idx, comp_off);
                        self.emit_cbuf_copy(dst[(i * 2 + 1) as usize], cbuf_idx, comp_off + 4);
                    }
                    Ok(dst)
                } else {
                    let dst = self.alloc_ssa_vec(RegFile::GPR, comps);
                    for i in 0..comps as u16 {
                        self.emit_cbuf_copy(dst[i as usize], cbuf_idx, offset + i * comp_bytes);
                    }
                    Ok(dst)
                }
            }
            naga::TypeInner::Matrix {
                columns,
                rows,
                scalar,
            } => {
                let total_regs = columns as u8 * rows as u8;
                let comp_bytes = scalar.width as u16;
                let col_stride = rows as u16 * comp_bytes;
                let dst = self.alloc_ssa_vec(RegFile::GPR, total_regs);
                let mut reg = 0usize;
                for col in 0..columns as u16 {
                    for row in 0..rows as u16 {
                        let comp_off = offset + col * col_stride + row * comp_bytes;
                        self.emit_cbuf_copy(dst[reg], cbuf_idx, comp_off);
                        reg += 1;
                    }
                }
                Ok(dst)
            }
            _ => {
                let dst = self.alloc_ssa(RegFile::GPR);
                self.emit_cbuf_copy(dst, cbuf_idx, offset);
                Ok(dst.into())
            }
        }
    }

    fn emit_cbuf_copy(&mut self, dst: SSAValue, cbuf_idx: u8, offset: u16) {
        self.push_instr(Instr::new(OpCopy {
            dst: dst.into(),
            src: Src::from(SrcRef::CBuf(CBufRef {
                buf: CBuf::Binding(cbuf_idx),
                offset,
            })),
        }));
    }

    /// Handle `uniform_array[dynamic_index]` — the index is a runtime SSA value.
    ///
    /// CBuf offsets are static in the NVIDIA ISA, so true dynamic indexing
    /// into uniform buffers requires `LDC` with register offset. For now,
    /// we report this as not yet implemented; all known spring WGSL shaders
    /// use static (`AccessIndex`) paths for uniform structs.
    pub(super) fn emit_uniform_dynamic_access(
        &mut self,
        _handle: Handle<naga::Expression>,
        _cbuf_idx: u8,
        _base_offset: u16,
        _stride: u32,
        _index: SSARef,
    ) -> Result<SSARef, CompileError> {
        Err(CompileError::NotImplemented(
            "dynamic indexing into uniform buffers (requires LDC with register offset)".into(),
        ))
    }

    pub(super) fn emit_access(
        &mut self,
        base: SSARef,
        index: SSARef,
        base_handle: Handle<naga::Expression>,
    ) -> Result<SSARef, CompileError> {
        let stride = self.type_stride(base_handle)?;
        let scaled_idx = self.emit_imad(index[0].into(), Src::new_imm_u32(stride), Src::ZERO);
        if base.comps() >= 2 {
            let dst = self.alloc_ssa_vec(RegFile::GPR, 2);
            if self.sm.sm() >= 70 {
                self.push_instr(Instr::new(OpIAdd3 {
                    dst: dst[0].into(),
                    srcs: [base[0].into(), scaled_idx.into(), Src::ZERO],
                    overflow: [Dst::None, Dst::None],
                }));
            } else {
                self.push_instr(Instr::new(OpIAdd2 {
                    dst: dst[0].into(),
                    srcs: [base[0].into(), scaled_idx.into()],
                    carry_out: Dst::None,
                }));
            }
            self.push_instr(Instr::new(OpCopy {
                dst: dst[1].into(),
                src: base[1].into(),
            }));
            Ok(dst)
        } else {
            let dst = self.alloc_ssa(RegFile::GPR);
            if self.sm.sm() >= 70 {
                self.push_instr(Instr::new(OpIAdd3 {
                    dst: dst.into(),
                    srcs: [base[0].into(), scaled_idx.into(), Src::ZERO],
                    overflow: [Dst::None, Dst::None],
                }));
            } else {
                self.push_instr(Instr::new(OpIAdd2 {
                    dst: dst.into(),
                    srcs: [base[0].into(), scaled_idx.into()],
                    carry_out: Dst::None,
                }));
            }
            Ok(dst.into())
        }
    }

    pub(super) fn emit_access_index(
        &mut self,
        base: SSARef,
        index: u32,
        base_handle: Handle<naga::Expression>,
    ) -> Result<SSARef, CompileError> {
        let base_ty = self.resolve_expr_type_handle(base_handle)?;
        if matches!(
            self.module.types[base_ty].inner,
            naga::TypeInner::Vector { .. }
        ) {
            let comp = index as u8;
            if comp < base.comps() {
                return Ok(base[comp as usize].into());
            }
        }

        let stride = self.type_stride(base_handle)?;
        let byte_offset = index * stride;
        if byte_offset == 0 {
            return Ok(base);
        }
        if base.comps() >= 2 {
            let dst = self.alloc_ssa_vec(RegFile::GPR, 2);
            if self.sm.sm() >= 70 {
                self.push_instr(Instr::new(OpIAdd3 {
                    dst: dst[0].into(),
                    srcs: [base[0].into(), Src::new_imm_u32(byte_offset), Src::ZERO],
                    overflow: [Dst::None, Dst::None],
                }));
            } else {
                self.push_instr(Instr::new(OpIAdd2 {
                    dst: dst[0].into(),
                    srcs: [base[0].into(), Src::new_imm_u32(byte_offset)],
                    carry_out: Dst::None,
                }));
            }
            self.push_instr(Instr::new(OpCopy {
                dst: dst[1].into(),
                src: base[1].into(),
            }));
            Ok(dst)
        } else {
            let dst = self.alloc_ssa(RegFile::GPR);
            if self.sm.sm() >= 70 {
                self.push_instr(Instr::new(OpIAdd3 {
                    dst: dst.into(),
                    srcs: [base[0].into(), Src::new_imm_u32(byte_offset), Src::ZERO],
                    overflow: [Dst::None, Dst::None],
                }));
            } else {
                self.push_instr(Instr::new(OpIAdd2 {
                    dst: dst.into(),
                    srcs: [base[0].into(), Src::new_imm_u32(byte_offset)],
                    carry_out: Dst::None,
                }));
            }
            Ok(dst.into())
        }
    }

    pub(super) fn emit_atomic(
        &mut self,
        pointer: Handle<naga::Expression>,
        fun: &naga::AtomicFunction,
        value: Handle<naga::Expression>,
        result: Option<Handle<naga::Expression>>,
    ) -> Result<(), CompileError> {
        let addr = self.ensure_expr(pointer)?;
        let val = self.ensure_expr(value)?;

        let atom_op = match fun {
            naga::AtomicFunction::Add => AtomOp::Add,
            naga::AtomicFunction::Subtract => AtomOp::Add,
            naga::AtomicFunction::And => AtomOp::And,
            naga::AtomicFunction::InclusiveOr => AtomOp::Or,
            naga::AtomicFunction::ExclusiveOr => AtomOp::Xor,
            naga::AtomicFunction::Min => AtomOp::Min,
            naga::AtomicFunction::Max => AtomOp::Max,
            naga::AtomicFunction::Exchange { compare: None } => AtomOp::Exch,
            naga::AtomicFunction::Exchange { compare: Some(_) } => {
                AtomOp::CmpExch(AtomCmpSrc::Separate)
            }
        };

        let data_src: Src = if matches!(fun, naga::AtomicFunction::Subtract) {
            Src::from(val[0]).ineg()
        } else {
            val[0].into()
        };

        let result_ssa = if result.is_some() {
            Some(self.alloc_ssa(RegFile::GPR))
        } else {
            None
        };

        self.push_instr(Instr::new(OpAtom {
            dst: result_ssa.map_or(Dst::None, Dst::from),
            addr: addr[0].into(),
            cmpr: Src::ZERO,
            data: data_src,
            atom_op,
            atom_type: AtomType::U32,
            addr_offset: 0,
            addr_stride: OffsetStride::X1,
            mem_space: MemSpace::Global(MemAddrType::A64),
            mem_order: MemOrder::Weak,
            mem_eviction_priority: MemEvictionPriority::Normal,
        }));

        if let (Some(result_handle), Some(ssa)) = (result, result_ssa) {
            self.expr_map.insert(result_handle, ssa.into());
        }

        Ok(())
    }

    pub(super) fn type_stride(
        &self,
        base_handle: Handle<naga::Expression>,
    ) -> Result<u32, CompileError> {
        let ty_handle = self.resolve_expr_type_handle(base_handle)?;
        let inner = &self.module.types[ty_handle].inner;
        Ok(match *inner {
            naga::TypeInner::Array { stride, .. } => stride,
            naga::TypeInner::Vector { scalar, .. } => scalar.width as u32,
            naga::TypeInner::Pointer { base, .. } => {
                let base_inner = &self.module.types[base].inner;
                match *base_inner {
                    naga::TypeInner::Array { stride, .. } => stride,
                    _ => base_inner.size(self.module.to_ctx()),
                }
            }
            _ => inner.size(self.module.to_ctx()),
        })
    }
}
