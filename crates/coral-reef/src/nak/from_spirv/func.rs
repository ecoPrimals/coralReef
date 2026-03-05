#![allow(clippy::wildcard_imports)]
use super::super::ir::*;
use crate::error::CompileError;
use coral_reef_stubs::cfg::CFGBuilder;
use naga::Handle;
use std::collections::HashMap;

/// Per-function translation state.
pub(super) struct FuncTranslator<'a, 'b> {
    pub(super) sm: &'a ShaderModelInfo,
    pub(super) module: &'b naga::Module,
    pub(super) func: &'b naga::Function,
    pub(super) ssa_alloc: SSAValueAllocator,
    pub(super) phi_alloc: PhiAllocator,
    pub(super) label_alloc: LabelAllocator,
    pub(super) cfg_builder: CFGBuilder<BasicBlock>,
    pub(super) expr_map: HashMap<Handle<naga::Expression>, SSARef>,
    pub(super) current_instrs: Vec<Instr>,
    pub(super) current_label: Label,
    pub(super) current_block_id: Option<usize>,
    pub(super) next_block_id: usize,
}

impl<'a, 'b> FuncTranslator<'a, 'b> {
    pub(super) fn new(
        sm: &'a ShaderModelInfo,
        module: &'b naga::Module,
        func: &'b naga::Function,
    ) -> Self {
        let mut la = LabelAllocator::new();
        let initial_label = la.alloc();
        Self {
            sm,
            module,
            func,
            ssa_alloc: SSAValueAllocator::new(),
            phi_alloc: PhiAllocator::new(),
            label_alloc: la,
            cfg_builder: CFGBuilder::new(),
            expr_map: HashMap::new(),
            current_instrs: Vec::new(),
            current_label: initial_label,
            current_block_id: None,
            next_block_id: 0,
        }
    }

    pub(super) fn start_block(&mut self) {
        self.current_label = self.label_alloc.alloc();
        self.current_instrs.clear();
    }

    pub(super) fn finish_block(&mut self) -> usize {
        let bb = BasicBlock {
            label: self.current_label,
            uniform: false,
            instrs: std::mem::take(&mut self.current_instrs),
        };
        let id = self.cfg_builder.add_block(bb);
        assert_eq!(id, self.next_block_id);
        self.next_block_id += 1;
        if let Some(prev) = self.current_block_id {
            self.cfg_builder.add_edge(prev, id);
        }
        self.current_block_id = Some(id);
        id
    }

    fn finish_block_no_fallthrough(&mut self) -> usize {
        let bb = BasicBlock {
            label: self.current_label,
            uniform: false,
            instrs: std::mem::take(&mut self.current_instrs),
        };
        let id = self.cfg_builder.add_block(bb);
        assert_eq!(id, self.next_block_id);
        self.next_block_id += 1;
        if let Some(prev) = self.current_block_id {
            self.cfg_builder.add_edge(prev, id);
        }
        self.current_block_id = None;
        id
    }

    pub(super) fn push_instr(&mut self, instr: Instr) {
        self.current_instrs.push(instr);
    }

    pub(super) fn alloc_ssa(&mut self, file: RegFile) -> SSAValue {
        self.ssa_alloc.alloc(file)
    }

    pub(super) fn alloc_ssa_vec(&mut self, file: RegFile, comps: u8) -> SSARef {
        self.ssa_alloc.alloc_vec(file, comps)
    }

    pub(super) fn build_function(self) -> Function {
        Function {
            ssa_alloc: self.ssa_alloc,
            phi_alloc: self.phi_alloc,
            blocks: self.cfg_builder.build(),
        }
    }

    pub(super) fn emit_compute_prologue(&mut self, _ep: &naga::EntryPoint) -> Result<(), CompileError> {
        for (_handle, gv) in self.module.global_variables.iter() {
            if gv.binding.is_some() {
                // TODO: process bindings for compute prologue
            }
        }
        Ok(())
    }

    pub(super) fn translate_block(&mut self, block: &naga::Block) -> Result<(), CompileError> {
        for stmt in block {
            self.translate_statement(stmt)?;
        }
        Ok(())
    }

    fn translate_statement(&mut self, stmt: &naga::Statement) -> Result<(), CompileError> {
        match *stmt {
            naga::Statement::Emit(ref range) => {
                for expr_handle in range.clone() {
                    self.translate_expression(expr_handle)?;
                }
                Ok(())
            }
            naga::Statement::Store { pointer, value } => {
                self.ensure_expr(pointer)?;
                self.ensure_expr(value)?;
                self.emit_store(pointer, value)
            }
            naga::Statement::If {
                condition,
                ref accept,
                ref reject,
            } => self.translate_if(condition, accept, reject),
            naga::Statement::Loop {
                ref body,
                ref continuing,
                break_if,
            } => self.translate_loop(body, continuing, break_if),
            naga::Statement::Return { value } => {
                if let Some(val) = value {
                    self.ensure_expr(val)?;
                }
                Ok(())
            }
            naga::Statement::Block(ref inner) => self.translate_block(inner),
            naga::Statement::Barrier(barrier) => {
                if barrier.contains(naga::Barrier::WORK_GROUP) {
                    self.push_instr(Instr::new(OpBar {}));
                }
                if barrier.contains(naga::Barrier::STORAGE) {
                    self.push_instr(Instr::new(OpMemBar {
                        scope: MemScope::System,
                    }));
                }
                Ok(())
            }
            naga::Statement::Break => Ok(()),
            naga::Statement::Continue => Ok(()),
            naga::Statement::Kill => {
                self.push_instr(Instr::new(OpKill {}));
                Ok(())
            }
            naga::Statement::Call {
                function,
                ref arguments,
                result,
            } => {
                for &arg in arguments {
                    self.ensure_expr(arg)?;
                }
                if let Some(res) = result {
                    let dst = self.alloc_ssa(RegFile::GPR);
                    self.expr_map.insert(res, dst.into());
                }
                Err(CompileError::NotImplemented(
                    "function calls not yet supported".into(),
                ))
            }
            _ => Err(CompileError::NotImplemented(format!(
                "statement {:?} not yet supported",
                std::mem::discriminant(stmt),
            ))),
        }
    }

    pub(super) fn emit_store(
        &mut self,
        pointer: Handle<naga::Expression>,
        value: Handle<naga::Expression>,
    ) -> Result<(), CompileError> {
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

    pub(super) fn type_stride(&self, base_handle: Handle<naga::Expression>) -> Result<u32, CompileError> {
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

    fn translate_if(
        &mut self,
        condition: Handle<naga::Expression>,
        accept: &naga::Block,
        reject: &naga::Block,
    ) -> Result<(), CompileError> {
        let cond = self.ensure_expr(condition)?;
        let merge_label = self.label_alloc.alloc();

        let cond_block = self.finish_block_no_fallthrough();

        self.start_block();
        self.translate_block(accept)?;
        self.push_instr(Instr::new(OpBra {
            target: merge_label,
            cond: SrcRef::True.into(),
        }));
        let accept_block = self.finish_block_no_fallthrough();
        self.cfg_builder.add_edge(cond_block, accept_block);

        self.start_block();
        self.translate_block(reject)?;
        self.push_instr(Instr::new(OpBra {
            target: merge_label,
            cond: SrcRef::True.into(),
        }));
        let reject_block = self.finish_block_no_fallthrough();
        self.cfg_builder.add_edge(cond_block, reject_block);

        self.start_block();
        self.current_label = merge_label;
        let merge_block = self.next_block_id;
        self.cfg_builder.add_edge(accept_block, merge_block);
        self.cfg_builder.add_edge(reject_block, merge_block);
        self.current_block_id = None;

        let _ = cond;
        Ok(())
    }

    fn translate_loop(
        &mut self,
        body: &naga::Block,
        continuing: &naga::Block,
        _break_if: Option<Handle<naga::Expression>>,
    ) -> Result<(), CompileError> {
        let header_label = self.label_alloc.alloc();
        let _exit_label = self.label_alloc.alloc();

        let pre_block = self.finish_block_no_fallthrough();

        self.start_block();
        self.current_label = header_label;
        let header_block_id = self.next_block_id;
        self.cfg_builder.add_edge(pre_block, header_block_id);
        self.translate_block(body)?;
        self.translate_block(continuing)?;
        self.push_instr(Instr::new(OpBra {
            target: header_label,
            cond: SrcRef::True.into(),
        }));
        let body_block = self.finish_block_no_fallthrough();
        if body_block != header_block_id {
            self.cfg_builder.add_edge(body_block, header_block_id);
        }

        self.start_block();
        self.current_block_id = None;

        Ok(())
    }

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
        mut f: impl FnMut(&mut Self, SSAValue, SSAValue) -> SSAValue,
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
                cond: cond[0].into(),
                srcs: [acc.into(), rej.into()],
            }));
        }
        Ok(dst)
    }

    pub(super) fn translate_cast(
        &mut self,
        val: SSARef,
        kind: naga::ScalarKind,
        convert: Option<naga::Bytes>,
        _inner_handle: Handle<naga::Expression>,
    ) -> Result<SSARef, CompileError> {
        let dst = self.alloc_ssa(RegFile::GPR);
        match (kind, convert) {
            (naga::ScalarKind::Float, Some(4)) => {
                self.push_instr(Instr::new(OpI2F {
                    dst: dst.into(),
                    src: val[0].into(),
                    dst_type: FloatType::F32,
                    src_type: IntType::I32,
                    rnd_mode: FRndMode::NearestEven,
                }));
            }
            (naga::ScalarKind::Uint | naga::ScalarKind::Sint, Some(4)) => {
                self.push_instr(Instr::new(OpF2I {
                    dst: dst.into(),
                    src: val[0].into(),
                    dst_type: IntType::I32,
                    src_type: FloatType::F32,
                    rnd_mode: FRndMode::NearestEven,
                    ftz: false,
                }));
            }
            _ => {
                self.push_instr(Instr::new(OpCopy {
                    dst: dst.into(),
                    src: val[0].into(),
                }));
            }
        }
        Ok(dst.into())
    }
}
