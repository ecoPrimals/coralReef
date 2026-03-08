// SPDX-License-Identifier: AGPL-3.0-only
//! Per-function translation state: types, constructors, block management, statement dispatch.
#![allow(clippy::wildcard_imports)]
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
    /// Tracks expressions that refer to uniform CBuf data: (cbuf_idx, byte_offset).
    pub(super) uniform_refs: FxHashMap<Handle<naga::Expression>, (u8, u16)>,
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

    pub(super) fn finish_block(&mut self) -> Result<usize, CompileError> {
        let bb = BasicBlock {
            label: self.current_label,
            uniform: false,
            instrs: std::mem::take(&mut self.current_instrs),
        };
        let id = self.cfg_builder.add_block(bb);
        if id != self.next_block_id {
            return Err(CompileError::InvalidInput(
                format!(
                    "CFG block id mismatch: expected {}, got {}",
                    self.next_block_id, id
                )
                .into(),
            ));
        }
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
        if id != self.next_block_id {
            return Err(CompileError::InvalidInput(
                format!(
                    "CFG block id mismatch: expected {}, got {}",
                    self.next_block_id, id
                )
                .into(),
            ));
        }
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

    pub(super) fn emit_compute_prologue(&self, _ep: &naga::EntryPoint) -> Result<(), CompileError> {
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
                    return Err(CompileError::NotImplemented(
                        "texture/sampler bindings in compute prologue not yet supported".into(),
                    ));
                }
                naga::AddressSpace::Immediate => {
                    return Err(CompileError::NotImplemented(
                        "push constant bindings in compute prologue not yet supported".into(),
                    ));
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
                    .ok_or_else(|| CompileError::NotImplemented("break outside loop".into()))?;
                let exit_label = loop_ctx.exit_label;
                self.emit_loop_exit_phi_srcs()?;
                self.push_instr(Instr::new(OpBra {
                    target: exit_label,
                    cond: SrcRef::True.into(),
                }));
                let break_block = self.finish_block_no_fallthrough()?;
                self.loop_stack
                    .last_mut()
                    .ok_or_else(|| CompileError::NotImplemented("break outside loop".into()))?
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
                    .ok_or_else(|| CompileError::NotImplemented("continue outside loop".into()))?;
                let continue_label = loop_ctx.continue_label;
                self.emit_loop_continue_phi_srcs()?;
                self.push_instr(Instr::new(OpBra {
                    target: continue_label,
                    cond: SrcRef::True.into(),
                }));
                let cont_block = self.finish_block_no_fallthrough()?;
                self.loop_stack
                    .last_mut()
                    .ok_or_else(|| CompileError::NotImplemented("continue outside loop".into()))?
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
            _ => Err(CompileError::NotImplemented(
                format!(
                    "statement {:?} not yet supported",
                    std::mem::discriminant(stmt),
                )
                .into(),
            )),
        }
    }
}
