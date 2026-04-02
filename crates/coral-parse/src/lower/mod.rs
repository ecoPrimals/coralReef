// SPDX-License-Identifier: AGPL-3.0-only
//! Lower sovereign AST → CoralIR (`coral_reef::codegen::ir::Shader`).
//!
//! Walks the AST and emits SSA instructions into a CFG of BasicBlocks,
//! producing a `Shader` with `Function`s for entry points.

mod math;
mod binary;
mod convert;
mod stmt;
mod builtin;

use crate::ast;
use crate::ast::{AddressSpace, Binding, Expression, Handle, Literal, Type};
use coral_reef::codegen::ir::*;
use coral_reef::error::CompileError;
use coral_reef::FmaPolicy;
use coral_reef_stubs::cfg::CFGBuilder;
use coral_reef_stubs::fxhash::FxHashMap;
use std::collections::HashSet;

type IrFunction = coral_reef::codegen::ir::Function;

#[allow(unused)]
pub(crate) mod sys_regs {
    //! NVIDIA special register indices (S2R sources).
    pub const SR_TID_X: u8 = 0x21;
    pub const SR_TID_Y: u8 = 0x22;
    pub const SR_TID_Z: u8 = 0x23;
    pub const SR_CTAID_X: u8 = 0x25;
    pub const SR_CTAID_Y: u8 = 0x26;
    pub const SR_CTAID_Z: u8 = 0x27;
    pub const SR_NTID_X: u8 = 0x29;
    pub const SR_NTID_Y: u8 = 0x2a;
    pub const SR_NTID_Z: u8 = 0x2b;
    pub const SR_NCTAID_X: u8 = 0x2d;
    pub const SR_NCTAID_Y: u8 = 0x2e;
    pub const SR_NCTAID_Z: u8 = 0x2f;
}

pub(crate) fn mem_access_global_b32() -> MemAccess {
    MemAccess {
        mem_type: MemType::B32,
        space: MemSpace::Global(MemAddrType::A64),
        order: MemOrder::Weak,
        eviction_priority: MemEvictionPriority::Normal,
    }
}


pub(crate) fn mem_access_shared_b32() -> MemAccess {
    MemAccess {
        mem_type: MemType::B32,
        space: MemSpace::Shared,
        order: MemOrder::Strong(MemScope::CTA),
        eviction_priority: MemEvictionPriority::Normal,
    }
}

/// Per-function lowering state.
pub(crate) struct FuncLowerer<'a, 'sm> {
    #[allow(unused)]
    pub(crate) sm: &'sm dyn ShaderModel,
    pub(crate) module: &'a ast::Module,
    pub(crate) ssa_alloc: SSAValueAllocator,
    pub(crate) phi_alloc: PhiAllocator,
    pub(crate) label_alloc: LabelAllocator,
    pub(crate) cfg_builder: CFGBuilder<BasicBlock>,
    pub(crate) expr_map: FxHashMap<u32, SSARef>,
    pub(crate) var_storage: Vec<SSARef>,
    pub(crate) shared_ptrs: HashSet<u32>,
    pub(crate) shared_mem_offsets: FxHashMap<u32, u32>,
    pub(crate) uniform_refs: FxHashMap<u32, (SSARef, u16)>,
    pub(crate) current_instrs: Vec<Instr>,
    pub(crate) current_label: Label,
    pub(crate) current_block_id: Option<usize>,
    pub(crate) next_block_id: usize,
    pub(crate) dead_code: bool,
    pub(crate) workgroup_size: [u32; 3],
    pub(crate) loop_stack: Vec<LoopCtx>,
}

pub(crate) struct LoopCtx {
    pub(crate) exit_label: Label,
    pub(crate) continue_label: Label,
    pub(crate) break_blocks: Vec<usize>,
    pub(crate) continue_blocks: Vec<usize>,
}

impl<'a, 'sm> FuncLowerer<'a, 'sm> {
    pub(crate) fn new(sm: &'sm dyn ShaderModel, module: &'a ast::Module) -> Self {
        let mut la = LabelAllocator::new();
        let initial_label = la.alloc();
        Self {
            sm,
            module,
            ssa_alloc: SSAValueAllocator::new(),
            phi_alloc: PhiAllocator::new(),
            label_alloc: la,
            cfg_builder: CFGBuilder::new(),
            expr_map: FxHashMap::default(),
            var_storage: Vec::new(),
            shared_ptrs: HashSet::new(),
            shared_mem_offsets: FxHashMap::default(),
            uniform_refs: FxHashMap::default(),
            current_instrs: Vec::new(),
            current_label: initial_label,
            current_block_id: None,
            next_block_id: 0,
            dead_code: false,
            workgroup_size: [1, 1, 1],
            loop_stack: Vec::new(),
        }
    }

    pub(crate) fn alloc_ssa(&mut self, file: RegFile) -> SSAValue {
        self.ssa_alloc.alloc(file)
    }

    pub(crate) fn alloc_ssa_vec(&mut self, file: RegFile, comps: u8) -> SSARef {
        self.ssa_alloc.alloc_vec(file, comps)
    }

    pub(crate) fn push_instr(&mut self, instr: Instr) {
        if !self.dead_code {
            self.current_instrs.push(instr);
        }
    }

    pub(crate) fn start_block(&mut self) {
        self.current_label = self.label_alloc.alloc();
        self.current_instrs.clear();
    }

    pub(crate) fn finish_block(&mut self) -> Result<usize, CompileError> {
        let bb = BasicBlock {
            label: self.current_label,
            uniform: false,
            instrs: std::mem::take(&mut self.current_instrs),
        };
        let id = self.cfg_builder.add_block(bb);
        self.next_block_id += 1;
        if let Some(prev) = self.current_block_id {
            self.cfg_builder.add_edge(prev, id);
        }
        self.current_block_id = Some(id);
        Ok(id)
    }

    pub(crate) fn finish_block_no_fallthrough(&mut self) -> Result<usize, CompileError> {
        let bb = BasicBlock {
            label: self.current_label,
            uniform: false,
            instrs: std::mem::take(&mut self.current_instrs),
        };
        let id = self.cfg_builder.add_block(bb);
        self.next_block_id += 1;
        if let Some(prev) = self.current_block_id {
            self.cfg_builder.add_edge(prev, id);
        }
        self.current_block_id = None;
        Ok(id)
    }

    fn build_function(self) -> IrFunction {
        IrFunction {
            ssa_alloc: self.ssa_alloc,
            phi_alloc: self.phi_alloc,
            blocks: self.cfg_builder.build(),
        }
    }

    fn compute_shared_mem_layout(&self) -> (u16, FxHashMap<u32, u32>) {
        let mut total = 0u32;
        let mut offsets = FxHashMap::default();
        for (i, gv) in self.module.global_variables.iter().enumerate() {
            if gv.space == AddressSpace::WorkGroup {
                offsets.insert(i as u32, total);
                let ty = &self.module.types[gv.ty];
                let get_type = |h: Handle<Type>| -> &Type { &self.module.types[h] };
                total += ty.byte_size(&get_type);
            }
        }
        (total.min(u32::from(u16::MAX)) as u16, offsets)
    }

    pub(crate) fn ensure_expr(&mut self, handle: Handle<Expression>, func: &ast::Function) -> Result<SSARef, CompileError> {
        let key = handle.index();
        if let Some(ssa) = self.expr_map.get(&key) {
            return Ok(ssa.clone());
        }
        self.lower_expr(handle, func)
    }

    fn lower_expr(&mut self, handle: Handle<Expression>, func: &ast::Function) -> Result<SSARef, CompileError> {
        let key = handle.index();
        if let Some(ssa) = self.expr_map.get(&key) {
            return Ok(ssa.clone());
        }

        let expr = &func.expressions[handle];
        let result = match expr.clone() {
            Expression::Literal(lit) => self.lower_literal(&lit),
            Expression::ZeroValue(_ty) => {
                let dst = self.alloc_ssa(RegFile::GPR);
                self.push_instr(Instr::new(OpCopy { dst: dst.into(), src: Src::ZERO }));
                Ok(dst.into())
            }
            Expression::Binary { op, left, right } => {
                let l = self.ensure_expr(left, func)?;
                let r = self.ensure_expr(right, func)?;
                self.lower_binary(op, l, r)
            }
            Expression::Unary { op, expr: inner } => {
                let val = self.ensure_expr(inner, func)?;
                self.lower_unary(op, val)
            }
            Expression::Math { fun, arg, arg1, arg2 } => {
                let a = self.ensure_expr(arg, func)?;
                let b = arg1.map(|h| self.ensure_expr(h, func)).transpose()?;
                let c = arg2.map(|h| self.ensure_expr(h, func)).transpose()?;
                self.lower_math(fun, a, b, c)
            }
            Expression::Select { condition, accept, reject } => {
                let cond = self.ensure_expr(condition, func)?;
                let acc = self.ensure_expr(accept, func)?;
                let rej = self.ensure_expr(reject, func)?;
                self.lower_select(cond, acc, rej)
            }
            Expression::FunctionArgument(idx) => {
                let arg = &func.arguments[idx as usize];
                if let Some(Binding::BuiltIn(builtin)) = &arg.binding {
                    self.lower_builtin(*builtin)
                } else {
                    let dst = self.alloc_ssa(RegFile::GPR);
                    self.push_instr(Instr::new(OpCopy { dst: dst.into(), src: Src::ZERO }));
                    Ok(dst.into())
                }
            }
            Expression::GlobalVariable(gv_idx) => {
                let gv = &self.module.global_variables[gv_idx as usize];
                if gv.space == AddressSpace::WorkGroup {
                    let offset = self.shared_mem_offsets.get(&gv_idx).copied().unwrap_or(0);
                    let addr = self.alloc_ssa(RegFile::GPR);
                    self.push_instr(Instr::new(OpCopy {
                        dst: addr.into(),
                        src: Src::new_imm_u32(offset),
                    }));
                    self.shared_ptrs.insert(key);
                    Ok(addr.into())
                } else if let Some(binding) = &gv.binding {
                    let is_uniform = gv.space == AddressSpace::Uniform;
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
                    if is_uniform {
                        self.uniform_refs.insert(key, (addr.clone(), base_offset));
                    }
                    Ok(addr)
                } else {
                    let dst = self.alloc_ssa(RegFile::GPR);
                    self.push_instr(Instr::new(OpCopy { dst: dst.into(), src: Src::ZERO }));
                    Ok(dst.into())
                }
            }
            Expression::LocalVariable(lv_idx) => {
                if (lv_idx as usize) < self.var_storage.len() {
                    Ok(self.var_storage[lv_idx as usize].clone())
                } else {
                    let dst = self.alloc_ssa(RegFile::GPR);
                    self.push_instr(Instr::new(OpCopy { dst: dst.into(), src: Src::ZERO }));
                    Ok(dst.into())
                }
            }
            Expression::Load { pointer } => {
                let ptr_key = pointer.index();
                let ptr_expr = &func.expressions[pointer];

                if matches!(ptr_expr, Expression::LocalVariable(idx) if (*idx as usize) < self.var_storage.len()) {
                    if let Expression::LocalVariable(idx) = ptr_expr {
                        let result = self.var_storage[*idx as usize].clone();
                        self.expr_map.insert(key, result.clone());
                        return Ok(result);
                    }
                }

                self.ensure_expr(pointer, func)?;

                if self.shared_ptrs.contains(&ptr_key) {
                    let addr = self.ensure_expr(pointer, func)?;
                    let dst = self.alloc_ssa(RegFile::GPR);
                    self.push_instr(Instr::new(OpLd {
                        dst: dst.into(),
                        addr: Src::from(addr[0]),
                        offset: 0,
                        stride: OffsetStride::X1,
                        access: mem_access_shared_b32(),
                    }));
                    Ok(dst.into())
                } else if let Some((_, offset)) = self.uniform_refs.get(&ptr_key).cloned() {
                    let gv_key = ptr_key;
                    let buf_idx = self.resolve_cbuf_binding(gv_key, func);
                    let cbuf = CBufRef {
                        buf: CBuf::Binding(buf_idx),
                        offset,
                    };
                    let dst = self.alloc_ssa(RegFile::GPR);
                    self.push_instr(Instr::new(OpCopy {
                        dst: dst.into(),
                        src: Src::from(SrcRef::CBuf(cbuf)),
                    }));
                    Ok(dst.into())
                } else {
                    let addr = self.ensure_expr(pointer, func)?;
                    let dst = self.alloc_ssa(RegFile::GPR);
                    self.push_instr(Instr::new(OpLd {
                        dst: dst.into(),
                        addr: Src::from(addr[0]),
                        offset: 0,
                        stride: OffsetStride::X1,
                        access: mem_access_global_b32(),
                    }));
                    Ok(dst.into())
                }
            }
            Expression::Access { base, index } => {
                let base_ssa = self.ensure_expr(base, func)?;
                let idx_ssa = self.ensure_expr(index, func)?;
                let base_key = base.index();
                if self.shared_ptrs.contains(&base_key) {
                    self.shared_ptrs.insert(key);
                }

                let stride_imm = self.alloc_ssa(RegFile::GPR);
                self.push_instr(Instr::new(OpCopy {
                    dst: stride_imm.into(),
                    src: Src::new_imm_u32(4),
                }));
                let offset = self.alloc_ssa(RegFile::GPR);
                self.push_instr(Instr::new(OpIMul {
                    dst: offset.into(),
                    srcs: [Src::from(idx_ssa[0]), Src::from(stride_imm)],
                    signed: [false, false],
                    high: false,
                }));

                if self.shared_ptrs.contains(&base_key) {
                    let result = self.alloc_ssa(RegFile::GPR);
                    self.push_instr(Instr::new(OpIAdd2 {
                        dsts: [result.into(), Dst::None],
                        srcs: [Src::from(base_ssa[0]), Src::from(offset)],
                    }));
                    Ok(result.into())
                } else if base_ssa.comps() >= 2 {
                    let result = self.alloc_ssa_vec(RegFile::GPR, 2);
                    let carry = self.alloc_ssa(RegFile::Pred);
                    self.push_instr(Instr::new(OpIAdd2 {
                        dsts: [result[0].into(), carry.into()],
                        srcs: [Src::from(base_ssa[0]), Src::from(offset)],
                    }));
                    self.push_instr(Instr::new(OpIAdd2X {
                        dsts: [result[1].into(), Dst::None],
                        srcs: [Src::from(base_ssa[1]), Src::ZERO, Src::from(carry)],
                    }));
                    Ok(result)
                } else {
                    let result = self.alloc_ssa(RegFile::GPR);
                    self.push_instr(Instr::new(OpIAdd2 {
                        dsts: [result.into(), Dst::None],
                        srcs: [Src::from(base_ssa[0]), Src::from(offset)],
                    }));
                    Ok(result.into())
                }
            }
            Expression::AccessIndex { base, index } => {
                let base_ssa = self.ensure_expr(base, func)?;
                let base_key = base.index();
                if self.shared_ptrs.contains(&base_key) {
                    self.shared_ptrs.insert(key);
                }
                if index == 0 && base_ssa.comps() > 0 {
                    Ok(base_ssa[0].into())
                } else if (index as u8) < base_ssa.comps() {
                    Ok(base_ssa[index as usize].into())
                } else {
                    let byte_off = index * 4;
                    let off_val = self.alloc_ssa(RegFile::GPR);
                    self.push_instr(Instr::new(OpCopy {
                        dst: off_val.into(),
                        src: Src::new_imm_u32(byte_off),
                    }));
                    let result = self.alloc_ssa(RegFile::GPR);
                    self.push_instr(Instr::new(OpIAdd2 {
                        dsts: [result.into(), Dst::None],
                        srcs: [Src::from(base_ssa[0]), Src::from(off_val)],
                    }));
                    Ok(result.into())
                }
            }
            Expression::Compose { ty: _, components } => {
                let mut comp_ssas = Vec::new();
                for &c in &components {
                    comp_ssas.push(self.ensure_expr(c, func)?);
                }
                let total_comps: u8 = comp_ssas.iter().map(|r| r.comps()).sum();
                let dst = self.alloc_ssa_vec(RegFile::GPR, total_comps);
                let mut idx = 0usize;
                for comp in &comp_ssas {
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
            Expression::Splat { size, value } => {
                let val = self.ensure_expr(value, func)?;
                let n = size.count() as u8;
                let dst = self.alloc_ssa_vec(RegFile::GPR, n);
                for i in 0..n as usize {
                    self.push_instr(Instr::new(OpCopy {
                        dst: Dst::from(dst[i]),
                        src: Src::from(val[0]),
                    }));
                }
                Ok(dst)
            }
            Expression::Swizzle { vector, pattern, size } => {
                let vec = self.ensure_expr(vector, func)?;
                let n = size.count() as u8;
                let dst = self.alloc_ssa_vec(RegFile::GPR, n);
                for i in 0..n as usize {
                    let src_comp = pattern[i] as usize;
                    if src_comp < vec.comps() as usize {
                        self.push_instr(Instr::new(OpCopy {
                            dst: Dst::from(dst[i]),
                            src: Src::from(vec[src_comp]),
                        }));
                    }
                }
                Ok(dst)
            }
            Expression::As { expr: inner, kind, convert } => {
                let val = self.ensure_expr(inner, func)?;
                convert::lower_cast(self, val, kind, convert)
            }
            Expression::ArrayLength(base) => {
                let _base = self.ensure_expr(base, func)?;
                let dst = self.alloc_ssa(RegFile::GPR);
                self.push_instr(Instr::new(OpCopy { dst: dst.into(), src: Src::ZERO }));
                Ok(dst.into())
            }
            Expression::TextureSample { texture, sampler, coordinate, array_index, offset } => {
                let _tex = self.ensure_expr(texture, func)?;
                let _samp = self.ensure_expr(sampler, func)?;
                let _coord = self.ensure_expr(coordinate, func)?;
                if let Some(a) = array_index { let _ = self.ensure_expr(a, func)?; }
                if let Some(o) = offset { let _ = self.ensure_expr(o, func)?; }
                let dst = self.alloc_ssa(RegFile::GPR);
                self.push_instr(Instr::new(OpCopy { dst: dst.into(), src: Src::ZERO }));
                Ok(dst.into())
            }
            Expression::TextureSampleLevel { texture, sampler, coordinate, level, array_index, offset } => {
                let _tex = self.ensure_expr(texture, func)?;
                let _samp = self.ensure_expr(sampler, func)?;
                let _coord = self.ensure_expr(coordinate, func)?;
                let _lvl = self.ensure_expr(level, func)?;
                if let Some(a) = array_index { let _ = self.ensure_expr(a, func)?; }
                if let Some(o) = offset { let _ = self.ensure_expr(o, func)?; }
                let dst = self.alloc_ssa(RegFile::GPR);
                self.push_instr(Instr::new(OpCopy { dst: dst.into(), src: Src::ZERO }));
                Ok(dst.into())
            }
            Expression::TextureSampleBias { texture, sampler, coordinate, bias, array_index, offset } => {
                let _tex = self.ensure_expr(texture, func)?;
                let _samp = self.ensure_expr(sampler, func)?;
                let _coord = self.ensure_expr(coordinate, func)?;
                let _b = self.ensure_expr(bias, func)?;
                if let Some(a) = array_index { let _ = self.ensure_expr(a, func)?; }
                if let Some(o) = offset { let _ = self.ensure_expr(o, func)?; }
                let dst = self.alloc_ssa(RegFile::GPR);
                self.push_instr(Instr::new(OpCopy { dst: dst.into(), src: Src::ZERO }));
                Ok(dst.into())
            }
            Expression::TextureSampleCompare { texture, sampler, coordinate, depth_ref, array_index, offset } => {
                let _tex = self.ensure_expr(texture, func)?;
                let _samp = self.ensure_expr(sampler, func)?;
                let _coord = self.ensure_expr(coordinate, func)?;
                let _dr = self.ensure_expr(depth_ref, func)?;
                if let Some(a) = array_index { let _ = self.ensure_expr(a, func)?; }
                if let Some(o) = offset { let _ = self.ensure_expr(o, func)?; }
                let dst = self.alloc_ssa(RegFile::GPR);
                self.push_instr(Instr::new(OpCopy { dst: dst.into(), src: Src::ZERO }));
                Ok(dst.into())
            }
            Expression::TextureLoad { texture, coordinate, array_index, level, sample_index } => {
                let _tex = self.ensure_expr(texture, func)?;
                let _coord = self.ensure_expr(coordinate, func)?;
                if let Some(a) = array_index { let _ = self.ensure_expr(a, func)?; }
                if let Some(l) = level { let _ = self.ensure_expr(l, func)?; }
                if let Some(s) = sample_index { let _ = self.ensure_expr(s, func)?; }
                let dst = self.alloc_ssa(RegFile::GPR);
                self.push_instr(Instr::new(OpCopy { dst: dst.into(), src: Src::ZERO }));
                Ok(dst.into())
            }
            Expression::TextureDimensions { texture, level } => {
                let _tex = self.ensure_expr(texture, func)?;
                if let Some(l) = level { let _ = self.ensure_expr(l, func)?; }
                let dst = self.alloc_ssa(RegFile::GPR);
                self.push_instr(Instr::new(OpCopy { dst: dst.into(), src: Src::ZERO }));
                Ok(dst.into())
            }
            Expression::TextureNumLayers { texture }
            | Expression::TextureNumLevels { texture }
            | Expression::TextureNumSamples { texture } => {
                let _tex = self.ensure_expr(texture, func)?;
                let dst = self.alloc_ssa(RegFile::GPR);
                self.push_instr(Instr::new(OpCopy { dst: dst.into(), src: Src::ZERO }));
                Ok(dst.into())
            }
            Expression::Constant(inner) => self.ensure_expr(inner, func),
        }?;

        self.expr_map.insert(key, result.clone());
        Ok(result)
    }

    fn lower_literal(&mut self, lit: &Literal) -> Result<SSARef, CompileError> {
        let dst = self.alloc_ssa(RegFile::GPR);
        let src = match *lit {
            Literal::F32(f) => Src::new_imm_u32(f.to_bits()),
            Literal::F64(f) => {
                let bits = f.to_bits();
                let lo = bits as u32;
                let hi = (bits >> 32) as u32;
                let dst2 = self.alloc_ssa_vec(RegFile::GPR, 2);
                self.push_instr(Instr::new(OpCopy {
                    dst: dst2[0].into(),
                    src: Src::new_imm_u32(lo),
                }));
                self.push_instr(Instr::new(OpCopy {
                    dst: dst2[1].into(),
                    src: Src::new_imm_u32(hi),
                }));
                return Ok(dst2);
            }
            Literal::U32(u) => Src::new_imm_u32(u),
            Literal::I32(i) => Src::new_imm_u32(i as u32),
            Literal::Bool(b) => Src::new_imm_bool(b),
        };
        self.push_instr(Instr::new(OpCopy { dst: dst.into(), src }));
        Ok(dst.into())
    }

    fn lower_select(&mut self, cond: SSARef, accept: SSARef, reject: SSARef) -> Result<SSARef, CompileError> {
        let pred = self.alloc_ssa(RegFile::Pred);
        self.push_instr(Instr::new(OpISetP {
            dst: pred.into(),
            set_op: PredSetOp::And,
            cmp_op: IntCmpOp::Ne,
            cmp_type: IntCmpType::U32,
            ex: false,
            srcs: [Src::from(cond[0]), Src::ZERO, SrcRef::True.into(), SrcRef::True.into()],
        }));
        let dst = self.alloc_ssa(RegFile::GPR);
        self.push_instr(Instr::new(OpSel {
            dst: dst.into(),
            srcs: [Src::from(pred), Src::from(accept[0]), Src::from(reject[0])],
        }));
        Ok(dst.into())
    }

    fn resolve_cbuf_binding(&self, gv_key: u32, func: &ast::Function) -> u8 {
        let ptr_expr = &func.expressions[Handle::new(gv_key)];
        if let Expression::GlobalVariable(gv_idx) = ptr_expr {
            let gv = &self.module.global_variables[*gv_idx as usize];
            if let Some(binding) = &gv.binding {
                return binding.group as u8;
            }
        }
        0
    }
}

/// Lower a sovereign AST module to a `Shader` for a given entry point.
pub fn lower<'sm>(
    module: &ast::Module,
    sm: &'sm dyn ShaderModel,
    entry_point_name: &str,
) -> Result<Shader<'sm>, CompileError> {
    let ep = module
        .entry_points
        .iter()
        .find(|ep| ep.name == entry_point_name)
        .ok_or_else(|| CompileError::InvalidInput(format!("entry point '{entry_point_name}' not found").into()))?;

    let mut fl = FuncLowerer::new(sm, module);
    fl.workgroup_size = ep.workgroup_size;

    let (shared_mem_size, shared_offsets) = fl.compute_shared_mem_layout();
    fl.shared_mem_offsets = shared_offsets;

    fl.start_block();

    for s in &ep.function.body {
        fl.lower_stmt(s, &ep.function)?;
    }

    fl.push_instr(Instr::new(OpExit {}));
    fl.finish_block()?;

    let function = fl.build_function();

    let local_size = [
        ep.workgroup_size[0] as u16,
        ep.workgroup_size[1] as u16,
        ep.workgroup_size[2] as u16,
    ];

    let mut instr_count = 0u32;
    let mut barrier_count = 0u8;
    let mut has_global_st = false;
    for block in &function.blocks {
        for instr in &block.instrs {
            instr_count += 1;
            if matches!(instr.op, Op::Bar(_)) {
                barrier_count = barrier_count.saturating_add(1);
            }
            if matches!(instr.op, Op::St(_)) {
                has_global_st = true;
            }
        }
    }

    let info = ShaderInfo {
        max_warps_per_sm: 0,
        gpr_count: 0,
        control_barrier_count: barrier_count,
        instr_count,
        static_cycle_count: 0,
        spills_to_mem: 0,
        fills_from_mem: 0,
        spills_to_reg: 0,
        fills_from_reg: 0,
        shared_local_mem_size: shared_mem_size as u32,
        max_crs_depth: 0,
        uses_global_mem: true,
        writes_global_mem: has_global_st,
        uses_fp64: false,
        stage: ShaderStageInfo::Compute(ComputeShaderInfo {
            local_size,
            shared_mem_size,
        }),
        io: ShaderIoInfo::None,
    };

    Ok(Shader {
        sm,
        info,
        functions: vec![function],
        fma_policy: FmaPolicy::default(),
    })
}
