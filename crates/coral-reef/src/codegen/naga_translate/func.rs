// SPDX-License-Identifier: AGPL-3.0-or-later
//! Per-function translation state: types, constructors, block management, statement dispatch.
use super::super::ir::*;
use crate::error::CompileError;
use coral_reef_stubs::cfg::CFGBuilder;
use coral_reef_stubs::fxhash::FxHashMap;
use naga::Handle;

/// Reference to a register-promoted local variable (or component thereof).
#[derive(Clone, Copy, Debug)]
pub(super) enum VarRef {
    Full(usize),
    Component(usize, u32),
}

/// Active loop context for Break/Continue translation.
pub(super) struct LoopContext {
    pub exit_label: Label,
    pub continue_label: Label,
    pub continue_block_id: usize,
    pub break_blocks: Vec<usize>,
    pub continue_blocks: Vec<usize>,
    /// Phi identifiers for loop-header (back-edge) merges.
    pub slot_phis: Vec<Vec<Phi>>,
    /// Phi identifiers for the loop *exit* path — collects values from Break
    /// statements and break_if so the post-loop code uses properly defined SSA.
    pub exit_slot_phis: Vec<Vec<Phi>>,
    /// Phi identifiers for the *continuing* block entry — merges values from
    /// the normal body path and any Continue statement paths.
    pub continue_slot_phis: Vec<Vec<Phi>>,
}

/// Per-function translation state.
pub(super) struct FuncTranslator<'a, 'b> {
    pub(super) sm: &'a dyn ShaderModel,
    pub(super) module: &'b naga::Module,
    pub(super) func: &'b naga::Function,
    pub(super) ssa_alloc: SSAValueAllocator,
    pub(super) phi_alloc: PhiAllocator,
    pub(super) label_alloc: LabelAllocator,
    pub(super) cfg_builder: CFGBuilder<BasicBlock>,
    pub(super) expr_map: FxHashMap<Handle<naga::Expression>, SSARef>,
    /// Tracks expressions that refer to uniform buffer data: (buffer_addr_ssa, byte_offset).
    /// The SSARef is a 2-component VGPR pair holding the buffer virtual address.
    pub(super) uniform_refs: FxHashMap<Handle<naga::Expression>, (SSARef, u16)>,
    /// Register-promoted local variable slots (shared across inline boundaries).
    pub(super) var_storage: Vec<SSARef>,
    /// Maps expression handles to local variable references (per-function context).
    pub(super) expr_to_var: FxHashMap<Handle<naga::Expression>, VarRef>,
    /// Pre-allocated local variable handle → var_storage slot index.
    pub(super) local_var_slots: FxHashMap<Handle<naga::LocalVariable>, usize>,
    /// During inline: by-value argument SSA values indexed by argument position.
    pub(super) inline_args: Option<Vec<SSARef>>,
    /// During inline: pointer argument → var slot mappings.
    pub(super) inline_ptr_arg_slots: FxHashMap<u32, usize>,
    /// Captured return value during inline expansion.
    pub(super) inline_return: Option<SSARef>,
    /// Loop context stack for Break/Continue translation.
    pub(super) loop_stack: Vec<LoopContext>,
    pub(super) current_instrs: Vec<Instr>,
    pub(super) current_label: Label,
    pub(super) current_block_id: Option<usize>,
    pub(super) next_block_id: usize,
    /// True when accumulated instructions are in unreachable code (after Break/Continue).
    pub(super) dead_code: bool,
    /// Compile-time workgroup size from `@workgroup_size()`. Used to resolve
    /// `global_invocation_id` and `local_invocation_index` without system
    /// register reads (RDNA2 lacks hardware registers for workgroup size).
    pub(super) workgroup_size: [u32; 3],
}

impl<'a, 'b> FuncTranslator<'a, 'b> {
    pub(super) fn new(
        sm: &'a dyn ShaderModel,
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
            expr_map: FxHashMap::default(),
            uniform_refs: FxHashMap::default(),
            var_storage: Vec::new(),
            expr_to_var: FxHashMap::default(),
            local_var_slots: FxHashMap::default(),
            inline_args: None,
            inline_ptr_arg_slots: FxHashMap::default(),
            inline_return: None,
            loop_stack: Vec::new(),
            current_instrs: Vec::new(),
            current_label: initial_label,
            current_block_id: None,
            next_block_id: 0,
            dead_code: false,
            workgroup_size: [1, 1, 1],
        }
    }

    pub(super) fn start_block(&mut self) {
        self.current_label = self.label_alloc.alloc();
        self.current_instrs.clear();
    }

    /// Start a new block at a pre-allocated label (used by switch lowering).
    pub(super) fn start_block_at(&mut self, label: Label) {
        self.current_label = label;
        self.current_instrs.clear();
    }

    fn verify_block_id(&self, id: usize) -> Result<(), CompileError> {
        if id != self.next_block_id {
            return Err(CompileError::Internal(
                format!(
                    "CFG block id mismatch: expected {}, got {id}",
                    self.next_block_id,
                )
                .into(),
            ));
        }
        Ok(())
    }

    pub(super) fn finish_block(&mut self) -> Result<usize, CompileError> {
        let bb = BasicBlock {
            label: self.current_label,
            uniform: false,
            instrs: std::mem::take(&mut self.current_instrs),
        };
        let id = self.cfg_builder.add_block(bb);
        self.verify_block_id(id)?;
        self.next_block_id += 1;
        if let Some(prev) = self.current_block_id {
            self.add_cfg_edge(prev, id);
        }
        self.current_block_id = Some(id);
        Ok(id)
    }

    pub(super) fn finish_block_no_fallthrough(&mut self) -> Result<usize, CompileError> {
        let bb = BasicBlock {
            label: self.current_label,
            uniform: false,
            instrs: std::mem::take(&mut self.current_instrs),
        };
        let id = self.cfg_builder.add_block(bb);
        self.verify_block_id(id)?;
        self.next_block_id += 1;
        if let Some(prev) = self.current_block_id {
            self.add_cfg_edge(prev, id);
        }
        self.current_block_id = None;
        Ok(id)
    }

    pub(super) fn push_instr(&mut self, instr: Instr) {
        if !self.dead_code {
            self.current_instrs.push(instr);
        }
    }

    pub(super) fn add_cfg_edge(&mut self, from: usize, to: usize) {
        self.cfg_builder.add_edge(from, to);
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

    /// Pre-allocate var_storage slots for all register-promotable local variables
    /// so they exist before any loops or ifs create phis.
    pub(super) fn pre_allocate_local_vars(&mut self) {
        for (lv_handle, lv) in self.func.local_variables.iter() {
            let comps = self.type_reg_comps(lv.ty);
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
                self.local_var_slots.insert(lv_handle, slot_id);
            }
        }
    }

    pub(super) fn emit_compute_prologue(
        &mut self,
        ep: &naga::EntryPoint,
    ) -> Result<(), CompileError> {
        self.workgroup_size = ep.workgroup_size;
        for (_handle, gv) in self.module.global_variables.iter() {
            let Some(binding) = &gv.binding else {
                continue;
            };
            match gv.space {
                naga::AddressSpace::Storage { .. } => {
                    let _ = binding;
                }
                naga::AddressSpace::Uniform => {
                    let _ = binding;
                }
                naga::AddressSpace::Handle => {
                    // Texture/sampler globals: the binding metadata is used
                    // by expression evaluation (ImageLoad, ImageSample, etc.)
                    // when it encounters GlobalVariable(handle). No prologue
                    // setup needed — PTX tex/surf declarations are emitted
                    // on first use.
                    let _ = binding;
                }
                naga::AddressSpace::Immediate => {
                    // Push constants: treated like a read-only uniform
                    // binding. The driver maps them through constant buffer
                    // slot 0 (NV) or user SGPRs (AMD). If the global has a
                    // binding, expr translation handles it via the standard
                    // CBUF path; if not, the data must be inlined by the
                    // driver at dispatch time.
                    let _ = binding;
                }
                naga::AddressSpace::TaskPayload => {
                    return Err(CompileError::NotImplemented(
                        "task payload bindings in compute prologue not yet supported".into(),
                    ));
                }
                naga::AddressSpace::Function
                | naga::AddressSpace::Private
                | naga::AddressSpace::WorkGroup => {}
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
        if self.dead_code {
            return Ok(());
        }
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
            naga::Statement::Switch {
                selector,
                ref cases,
            } => self.translate_switch(selector, cases),
            naga::Statement::Return { value } => {
                if let Some(val) = value {
                    let ssa = self.ensure_expr(val)?;
                    if self.inline_args.is_some() {
                        self.inline_return = Some(ssa);
                    }
                }
                if self.inline_args.is_none() {
                    self.push_instr(Instr::new(OpExit {}));
                    self.finish_block_no_fallthrough()?;
                    self.start_block();
                    self.current_instrs.push(Instr::new(OpExit {}));
                } else {
                    // During inlining: mark as dead; inline_call handles cleanup.
                }
                self.dead_code = true;
                Ok(())
            }
            naga::Statement::Block(ref inner) => self.translate_block(inner),
            naga::Statement::ControlBarrier(barrier) => {
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
            naga::Statement::MemoryBarrier(barrier) => {
                if barrier.contains(naga::Barrier::STORAGE) {
                    self.push_instr(Instr::new(OpMemBar {
                        scope: MemScope::System,
                    }));
                }
                Ok(())
            }
            naga::Statement::Break => {
                let loop_ctx = self
                    .loop_stack
                    .last()
                    .ok_or_else(|| CompileError::Internal("break outside loop".into()))?;
                let exit_label = loop_ctx.exit_label;
                self.emit_loop_exit_phi_srcs()?;
                self.push_instr(Instr::new(OpBra {
                    target: exit_label,
                    cond: SrcRef::True.into(),
                }));
                let break_block = self.finish_block_no_fallthrough()?;
                self.loop_stack
                    .last_mut()
                    .ok_or_else(|| CompileError::Internal("break outside loop".into()))?
                    .break_blocks
                    .push(break_block);
                self.start_block();
                self.current_instrs.push(Instr::new(OpExit {}));
                self.dead_code = true;
                Ok(())
            }
            naga::Statement::Continue => {
                let loop_ctx = self
                    .loop_stack
                    .last()
                    .ok_or_else(|| CompileError::Internal("continue outside loop".into()))?;
                let continue_label = loop_ctx.continue_label;
                self.emit_loop_continue_phi_srcs()?;
                self.push_instr(Instr::new(OpBra {
                    target: continue_label,
                    cond: SrcRef::True.into(),
                }));
                let cont_block = self.finish_block_no_fallthrough()?;
                self.loop_stack
                    .last_mut()
                    .ok_or_else(|| CompileError::Internal("continue outside loop".into()))?
                    .continue_blocks
                    .push(cont_block);
                self.start_block();
                self.current_instrs.push(Instr::new(OpExit {}));
                self.dead_code = true;
                Ok(())
            }
            naga::Statement::Kill => {
                self.push_instr(Instr::new(OpKill {}));
                Ok(())
            }
            naga::Statement::Call {
                function,
                ref arguments,
                result,
            } => {
                self.inline_call(function, arguments, result)?;
                Ok(())
            }
            naga::Statement::Atomic {
                pointer,
                ref fun,
                value,
                result,
            } => {
                self.emit_atomic(pointer, fun, value, result)?;
                Ok(())
            }
            naga::Statement::SubgroupBallot { result, predicate } => {
                self.emit_subgroup_ballot(result, predicate)
            }
            naga::Statement::SubgroupCollectiveOperation {
                op,
                collective_op,
                argument,
                result,
            } => self.emit_subgroup_collective(op, collective_op, argument, result),
            naga::Statement::SubgroupGather {
                mode,
                argument,
                result,
            } => self.emit_subgroup_gather(mode, argument, result),
            _ => Err(CompileError::NotImplemented(
                format!(
                    "statement {:?} not yet supported",
                    std::mem::discriminant(stmt),
                )
                .into(),
            )),
        }
    }

    fn emit_subgroup_ballot(
        &mut self,
        result: Handle<naga::Expression>,
        predicate: Option<Handle<naga::Expression>>,
    ) -> Result<(), CompileError> {
        let pred_src = if let Some(pred_h) = predicate {
            let pred_ssa = self.ensure_expr(pred_h)?;
            Src::from(pred_ssa)
        } else {
            Src::new_imm_bool(true)
        };
        let ballot_dst = self.alloc_ssa(RegFile::GPR);
        self.push_instr(Instr::new(OpVote {
            op: VoteOp::Any,
            dsts: [ballot_dst.into(), Dst::None],
            pred: pred_src,
        }));
        self.expr_map.insert(result, ballot_dst.into());
        Ok(())
    }

    fn emit_subgroup_collective(
        &mut self,
        op: naga::SubgroupOperation,
        collective_op: naga::CollectiveOperation,
        argument: Handle<naga::Expression>,
        result: Handle<naga::Expression>,
    ) -> Result<(), CompileError> {
        let arg_ssa = self.ensure_expr(argument)?;
        let src = Src::from(arg_ssa);

        match collective_op {
            naga::CollectiveOperation::Reduce => {
                let is_signed = self.is_signed_int_expr(argument);
                if self.sm.sm() >= 73 {
                    let redux_op = subgroup_op_to_redux(op, is_signed)?;
                    let dst_val = self.alloc_ssa(RegFile::GPR);
                    self.push_instr(Instr::new(OpRedux {
                        dst: dst_val.into(),
                        src,
                        op: redux_op,
                    }));
                    self.expr_map.insert(result, dst_val.into());
                } else {
                    let is_float = self.is_float_expr(argument);
                    let dst_ssa = self.emit_reduce_via_shfl(src, op, is_float)?;
                    self.expr_map.insert(result, dst_ssa);
                }
            }
            naga::CollectiveOperation::InclusiveScan | naga::CollectiveOperation::ExclusiveScan => {
                let is_exclusive = collective_op == naga::CollectiveOperation::ExclusiveScan;
                let is_float = self.is_float_expr(argument);
                let dst_ssa = self.emit_scan_via_shfl(src, op, is_exclusive, is_float)?;
                self.expr_map.insert(result, dst_ssa);
            }
        }
        Ok(())
    }

    /// Emit a warp-wide reduction via butterfly shfl pattern (SM70 fallback).
    fn emit_reduce_via_shfl(
        &mut self,
        src: Src,
        op: naga::SubgroupOperation,
        is_float: bool,
    ) -> Result<SSARef, CompileError> {
        let mut acc_val = self.alloc_ssa(RegFile::GPR);
        self.push_instr(Instr::new(OpCopy {
            dst: acc_val.into(),
            src,
        }));

        for offset in [16u32, 8, 4, 2, 1] {
            let shfl_dst = self.alloc_ssa(RegFile::GPR);
            self.push_instr(Instr::new(OpShfl {
                dsts: [shfl_dst.into(), Dst::None],
                srcs: [
                    Src::from(SSARef::from(acc_val)),
                    Src::new_imm_u32(offset),
                    Src::new_imm_u32(0x1f),
                ],
                op: ShflOp::Bfly,
            }));

            let combined = self.alloc_ssa(RegFile::GPR);
            self.emit_subgroup_combine(
                combined.into(),
                Src::from(SSARef::from(acc_val)),
                Src::from(SSARef::from(shfl_dst)),
                op,
                is_float,
            )?;
            acc_val = combined;
        }

        Ok(SSARef::from(acc_val))
    }

    fn emit_subgroup_gather(
        &mut self,
        mode: naga::GatherMode,
        argument: Handle<naga::Expression>,
        result: Handle<naga::Expression>,
    ) -> Result<(), CompileError> {
        let arg_ssa = self.ensure_expr(argument)?;
        let src = Src::from(arg_ssa);

        let (shfl_op, lane_src) = match mode {
            naga::GatherMode::BroadcastFirst => (ShflOp::Idx, Src::new_imm_u32(0)),
            naga::GatherMode::Broadcast(idx_h) => {
                let idx_ssa = self.ensure_expr(idx_h)?;
                (ShflOp::Idx, Src::from(idx_ssa))
            }
            naga::GatherMode::Shuffle(idx_h) => {
                let idx_ssa = self.ensure_expr(idx_h)?;
                (ShflOp::Idx, Src::from(idx_ssa))
            }
            naga::GatherMode::ShuffleDown(offset_h) => {
                let off_ssa = self.ensure_expr(offset_h)?;
                (ShflOp::Down, Src::from(off_ssa))
            }
            naga::GatherMode::ShuffleUp(offset_h) => {
                let off_ssa = self.ensure_expr(offset_h)?;
                (ShflOp::Up, Src::from(off_ssa))
            }
            naga::GatherMode::ShuffleXor(mask_h) => {
                let mask_ssa = self.ensure_expr(mask_h)?;
                (ShflOp::Bfly, Src::from(mask_ssa))
            }
            _ => {
                return Err(CompileError::NotImplemented(
                    "unsupported SubgroupGather mode".into(),
                ));
            }
        };

        let dst_val = self.alloc_ssa(RegFile::GPR);
        self.push_instr(Instr::new(OpShfl {
            dsts: [dst_val.into(), Dst::None],
            srcs: [src, lane_src, Src::new_imm_u32(0x1f)],
            op: shfl_op,
        }));
        self.expr_map.insert(result, dst_val.into());
        Ok(())
    }

    /// Emit a warp-level scan (inclusive or exclusive) via iterated `shfl.up`.
    ///
    /// Uses butterfly pattern: for each power-of-2 offset, shuffle up and
    /// conditionally accumulate based on the in_bounds predicate.
    fn emit_scan_via_shfl(
        &mut self,
        src: Src,
        op: naga::SubgroupOperation,
        exclusive: bool,
        is_float: bool,
    ) -> Result<SSARef, CompileError> {
        let mut acc_val = self.alloc_ssa(RegFile::GPR);
        self.push_instr(Instr::new(OpCopy {
            dst: acc_val.into(),
            src,
        }));

        for offset in [1u32, 2, 4, 8, 16] {
            let shfl_dst = self.alloc_ssa(RegFile::GPR);
            let pred_dst = self.alloc_ssa(RegFile::Pred);
            self.push_instr(Instr::new(OpShfl {
                dsts: [shfl_dst.into(), pred_dst.into()],
                srcs: [
                    Src::from(SSARef::from(acc_val)),
                    Src::new_imm_u32(offset),
                    Src::new_imm_u32(0),
                ],
                op: ShflOp::Up,
            }));

            let temp = self.alloc_ssa(RegFile::GPR);
            self.emit_subgroup_combine(
                temp.into(),
                Src::from(SSARef::from(acc_val)),
                Src::from(SSARef::from(shfl_dst)),
                op,
                is_float,
            )?;

            let combined = self.alloc_ssa(RegFile::GPR);
            self.push_instr(Instr::new(OpSel {
                dst: combined.into(),
                srcs: [
                    Src::from(SSARef::from(pred_dst)),
                    Src::from(SSARef::from(temp)),
                    Src::from(SSARef::from(acc_val)),
                ],
            }));
            acc_val = combined;
        }

        if exclusive {
            let shfl_dst = self.alloc_ssa(RegFile::GPR);
            self.push_instr(Instr::new(OpShfl {
                dsts: [shfl_dst.into(), Dst::None],
                srcs: [
                    Src::from(SSARef::from(acc_val)),
                    Src::new_imm_u32(1),
                    Src::new_imm_u32(0),
                ],
                op: ShflOp::Up,
            }));
            acc_val = shfl_dst;
        }

        Ok(SSARef::from(acc_val))
    }

    /// Emit the combine step for a subgroup scan/reduce, dispatching on
    /// operation and scalar type. Floats use `OpFAdd`/`OpFMnMx`; integers
    /// use `OpIAdd2`/`OpIMnMx`/`OpLop2`.
    fn emit_subgroup_combine(
        &mut self,
        dst: Dst,
        lhs: Src,
        rhs: Src,
        op: naga::SubgroupOperation,
        is_float: bool,
    ) -> Result<(), CompileError> {
        if is_float {
            match op {
                naga::SubgroupOperation::Add => {
                    self.push_instr(Instr::new(OpFAdd {
                        dst,
                        srcs: [lhs, rhs],
                        saturate: false,
                        rnd_mode: FRndMode::NearestEven,
                        ftz: false,
                    }));
                }
                naga::SubgroupOperation::Min => {
                    self.push_instr(Instr::new(OpFMnMx {
                        dst,
                        srcs: [lhs, rhs, Src::new_imm_bool(true)],
                        ftz: false,
                    }));
                }
                naga::SubgroupOperation::Max => {
                    self.push_instr(Instr::new(OpFMnMx {
                        dst,
                        srcs: [lhs, rhs, Src::new_imm_bool(false)],
                        ftz: false,
                    }));
                }
                _ => {
                    self.push_instr(Instr::new(OpFAdd {
                        dst,
                        srcs: [lhs, rhs],
                        saturate: false,
                        rnd_mode: FRndMode::NearestEven,
                        ftz: false,
                    }));
                }
            }
        } else {
            match op {
                naga::SubgroupOperation::Add => {
                    self.push_instr(Instr::new(OpIAdd3 {
                        dsts: [dst, Dst::None, Dst::None],
                        srcs: [lhs, rhs, Src::new_imm_u32(0)],
                    }));
                }
                naga::SubgroupOperation::Min => {
                    self.push_instr(Instr::new(OpIMnMx {
                        dst,
                        cmp_type: IntCmpType::I32,
                        srcs: [lhs, rhs, Src::new_imm_bool(true)],
                    }));
                }
                naga::SubgroupOperation::Max => {
                    self.push_instr(Instr::new(OpIMnMx {
                        dst,
                        cmp_type: IntCmpType::I32,
                        srcs: [lhs, rhs, Src::new_imm_bool(false)],
                    }));
                }
                naga::SubgroupOperation::And | naga::SubgroupOperation::All => {
                    self.push_instr(Instr::new(OpLop3 {
                        dst,
                        srcs: [lhs, rhs, Src::new_imm_u32(0)],
                        op: LogicOp2::And.to_lut(),
                    }));
                }
                naga::SubgroupOperation::Or | naga::SubgroupOperation::Any => {
                    self.push_instr(Instr::new(OpLop3 {
                        dst,
                        srcs: [lhs, rhs, Src::new_imm_u32(0)],
                        op: LogicOp2::Or.to_lut(),
                    }));
                }
                naga::SubgroupOperation::Xor => {
                    self.push_instr(Instr::new(OpLop3 {
                        dst,
                        srcs: [lhs, rhs, Src::new_imm_u32(0)],
                        op: LogicOp2::Xor.to_lut(),
                    }));
                }
                naga::SubgroupOperation::Mul => {
                    return Err(CompileError::NotImplemented(
                        "integer subgroup multiply scan/reduce via shfl".into(),
                    ));
                }
            }
        }
        Ok(())
    }
}

fn subgroup_op_to_redux(
    op: naga::SubgroupOperation,
    is_signed: bool,
) -> Result<ReduxOp, CompileError> {
    let int_type = if is_signed {
        IntCmpType::I32
    } else {
        IntCmpType::U32
    };
    match op {
        naga::SubgroupOperation::Add => Ok(ReduxOp::Sum),
        naga::SubgroupOperation::And | naga::SubgroupOperation::All => Ok(ReduxOp::And),
        naga::SubgroupOperation::Or | naga::SubgroupOperation::Any => Ok(ReduxOp::Or),
        naga::SubgroupOperation::Xor => Ok(ReduxOp::Xor),
        naga::SubgroupOperation::Min => Ok(ReduxOp::Min(int_type)),
        naga::SubgroupOperation::Max => Ok(ReduxOp::Max(int_type)),
        // NVIDIA `redux.sync` supports {.add, .min, .max, .and, .or, .xor} but not
        // multiply. No SM generation (SM70–SM120) provides a hardware mul reduction.
        naga::SubgroupOperation::Mul => Err(CompileError::NotImplemented(
            "subgroup multiply reduction has no redux hardware op (SM70-SM120)".into(),
        )),
    }
}
