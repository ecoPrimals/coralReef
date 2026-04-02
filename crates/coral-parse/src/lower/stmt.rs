// SPDX-License-Identifier: AGPL-3.0-only
//! Statement lowering → CoralIR control flow, stores, atomics, switch.

use super::{mem_access_global_b32, mem_access_shared_b32, FuncLowerer, LoopCtx};
use crate::ast;
use crate::ast::{Expression, Handle};
use crate::ast::Statement as AstStatement;
use coral_reef::codegen::ir::*;
use coral_reef::error::CompileError;

impl FuncLowerer<'_, '_> {
    pub(crate) fn lower_stmt(&mut self, stmt: &AstStatement, func: &ast::Function) -> Result<(), CompileError> {
        if self.dead_code {
            return Ok(());
        }
        match stmt {
            AstStatement::Store { pointer, value } => self.lower_store(*pointer, *value, func),
            AstStatement::If { condition, accept, reject } => {
                self.lower_if(*condition, accept, reject, func)
            }
            AstStatement::ForLoop { init, condition, update, body } => {
                self.lower_for_loop(init.as_deref(), *condition, update.as_deref(), body, func)
            }
            AstStatement::WhileLoop { condition, body } => {
                self.lower_for_loop(None, Some(*condition), None, body, func)
            }
            AstStatement::Loop { body, continuing, break_if } => {
                self.lower_loop(body, continuing, *break_if, func)
            }
            AstStatement::Break => self.lower_break(),
            AstStatement::Continue => self.lower_continue(),
            AstStatement::ControlBarrier(barrier) => {
                if barrier.workgroup {
                    self.push_instr(Instr::new(OpBar {}));
                }
                if barrier.storage {
                    self.push_instr(Instr::new(OpMemBar { scope: MemScope::System }));
                }
                Ok(())
            }
            AstStatement::MemoryBarrier(barrier) => {
                if barrier.storage {
                    self.push_instr(Instr::new(OpMemBar { scope: MemScope::System }));
                }
                Ok(())
            }
            AstStatement::Return { .. } => {
                self.push_instr(Instr::new(OpExit {}));
                self.finish_block_no_fallthrough()?;
                self.start_block();
                self.dead_code = true;
                Ok(())
            }
            AstStatement::Kill => {
                self.push_instr(Instr::new(OpKill {}));
                self.finish_block_no_fallthrough()?;
                self.start_block();
                self.dead_code = true;
                Ok(())
            }
            AstStatement::Block(inner) => {
                for s in inner {
                    self.lower_stmt(s, func)?;
                }
                Ok(())
            }
            AstStatement::LocalDecl { local_var_index } => {
                let lv = &func.local_variables[*local_var_index as usize];
                let ssa = if let Some(init) = lv.init {
                    self.ensure_expr(init, func)?
                } else {
                    let dst = self.alloc_ssa(RegFile::GPR);
                    self.push_instr(Instr::new(OpCopy { dst: dst.into(), src: Src::ZERO }));
                    dst.into()
                };
                while self.var_storage.len() <= *local_var_index as usize {
                    let pad = self.alloc_ssa(RegFile::GPR);
                    self.push_instr(Instr::new(OpCopy { dst: pad.into(), src: Src::ZERO }));
                    self.var_storage.push(pad.into());
                }
                self.var_storage[*local_var_index as usize] = ssa;
                Ok(())
            }
            AstStatement::Phony { value } => {
                let _ = self.ensure_expr(*value, func)?;
                Ok(())
            }
            AstStatement::Increment { pointer } => {
                self.lower_inc_dec(*pointer, true, func)
            }
            AstStatement::Decrement { pointer } => {
                self.lower_inc_dec(*pointer, false, func)
            }
            AstStatement::CompoundAssign { pointer, op, value } => {
                self.ensure_expr(*pointer, func)?;
                let rhs = self.ensure_expr(*value, func)?;
                let ptr_expr = &func.expressions[*pointer];
                if let Expression::LocalVariable(idx) = ptr_expr {
                    if (*idx as usize) < self.var_storage.len() {
                        let old = self.var_storage[*idx as usize].clone();
                        let result = self.lower_binary(*op, old, rhs)?;
                        self.var_storage[*idx as usize] = result;
                        return Ok(());
                    }
                }
                Ok(())
            }
            AstStatement::TextureStore { texture, coordinate, array_index, value } => {
                let _tex = self.ensure_expr(*texture, func)?;
                let _coord = self.ensure_expr(*coordinate, func)?;
                if let Some(a) = array_index {
                    let _ = self.ensure_expr(*a, func)?;
                }
                let _val = self.ensure_expr(*value, func)?;
                Ok(())
            }
            AstStatement::Emit(_) => Ok(()),
            AstStatement::Call { function: _, arguments, result: _ } => {
                for &arg in arguments {
                    let _ = self.ensure_expr(arg, func)?;
                }
                Ok(())
            }
            AstStatement::Atomic { pointer, fun, value, result } => {
                self.lower_atomic(*pointer, *fun, *value, *result, func)
            }
            AstStatement::Switch { selector, cases } => {
                self.lower_switch(*selector, cases, func)
            }
        }
    }

    fn lower_store(
        &mut self,
        pointer: Handle<Expression>,
        value: Handle<Expression>,
        func: &ast::Function,
    ) -> Result<(), CompileError> {
        self.ensure_expr(pointer, func)?;
        let val = self.ensure_expr(value, func)?;
        let ptr_key = pointer.index();

        let ptr_expr = &func.expressions[pointer];
        if let Expression::LocalVariable(idx) = ptr_expr {
            if (*idx as usize) < self.var_storage.len() {
                let old_ref = &self.var_storage[*idx as usize];
                let comps = val.comps().min(old_ref.comps());
                let new_ssa = self.alloc_ssa_vec(RegFile::GPR, comps);
                for i in 0..comps as usize {
                    self.push_instr(Instr::new(OpCopy {
                        dst: new_ssa[i].into(),
                        src: Src::from(val[i]),
                    }));
                }
                self.var_storage[*idx as usize] = new_ssa;
                return Ok(());
            }
        }

        if self.shared_ptrs.contains(&ptr_key) {
            let addr = self.ensure_expr(pointer, func)?;
            self.push_instr(Instr::new(OpSt {
                srcs: [Src::from(addr[0]), Src::from(val[0])],
                offset: 0,
                stride: OffsetStride::X1,
                access: mem_access_shared_b32(),
            }));
        } else {
            let addr = self.ensure_expr(pointer, func)?;
            self.push_instr(Instr::new(OpSt {
                srcs: [Src::from(addr[0]), Src::from(val[0])],
                offset: 0,
                stride: OffsetStride::X1,
                access: mem_access_global_b32(),
            }));
        }
        Ok(())
    }

    fn lower_if(
        &mut self,
        condition: Handle<Expression>,
        accept: &[AstStatement],
        reject: &[AstStatement],
        func: &ast::Function,
    ) -> Result<(), CompileError> {
        let cond = self.ensure_expr(condition, func)?;
        let pred = self.alloc_ssa(RegFile::Pred);
        self.push_instr(Instr::new(OpISetP {
            dst: pred.into(),
            set_op: PredSetOp::And,
            cmp_op: IntCmpOp::Ne,
            cmp_type: IntCmpType::U32,
            ex: false,
            srcs: [Src::from(cond[0]), Src::ZERO, SrcRef::True.into(), SrcRef::True.into()],
        }));

        let merge_label = self.label_alloc.alloc();
        let else_label = self.label_alloc.alloc();

        self.push_instr(Instr::new(OpBra {
            target: else_label,
            cond: Src::from(pred).bnot(),
        }));
        let if_start = self.finish_block_no_fallthrough()?;

        self.start_block();
        for s in accept {
            self.lower_stmt(s, func)?;
        }
        if !self.dead_code {
            self.push_instr(Instr::new(OpBra {
                target: merge_label,
                cond: SrcRef::True.into(),
            }));
        }
        let accept_end = self.finish_block_no_fallthrough()?;
        self.cfg_builder.add_edge(if_start, accept_end.saturating_sub(self.next_block_id.saturating_sub(1).saturating_sub(accept_end)));

        self.dead_code = false;
        self.start_block();
        for s in reject {
            self.lower_stmt(s, func)?;
        }
        if !self.dead_code {
            self.push_instr(Instr::new(OpBra {
                target: merge_label,
                cond: SrcRef::True.into(),
            }));
        }
        let _reject_end = self.finish_block_no_fallthrough()?;

        self.dead_code = false;
        self.start_block();
        Ok(())
    }

    fn lower_for_loop(
        &mut self,
        init: Option<&AstStatement>,
        condition: Option<Handle<Expression>>,
        update: Option<&AstStatement>,
        body: &[AstStatement],
        func: &ast::Function,
    ) -> Result<(), CompileError> {
        if let Some(init) = init {
            self.lower_stmt(init, func)?;
        }

        let loop_label = self.label_alloc.alloc();
        let exit_label = self.label_alloc.alloc();
        let cont_label = self.label_alloc.alloc();

        self.push_instr(Instr::new(OpBra { target: loop_label, cond: SrcRef::True.into() }));
        self.finish_block_no_fallthrough()?;

        self.loop_stack.push(LoopCtx {
            exit_label,
            continue_label: cont_label,
            break_blocks: Vec::new(),
            continue_blocks: Vec::new(),
        });

        self.start_block();

        if let Some(condition) = condition {
            let cond = self.ensure_expr(condition, func)?;
            let pred = self.alloc_ssa(RegFile::Pred);
            self.push_instr(Instr::new(OpISetP {
                dst: pred.into(),
                set_op: PredSetOp::And,
                cmp_op: IntCmpOp::Ne,
                cmp_type: IntCmpType::U32,
                ex: false,
                srcs: [Src::from(cond[0]), Src::ZERO, SrcRef::True.into(), SrcRef::True.into()],
            }));
            self.push_instr(Instr::new(OpBra {
                target: exit_label,
                cond: Src::from(pred).bnot(),
            }));
            self.finish_block_no_fallthrough()?;
            self.start_block();
        }

        for s in body {
            self.lower_stmt(s, func)?;
        }

        if let Some(update) = update {
            self.dead_code = false;
            self.lower_stmt(update, func)?;
        }

        self.push_instr(Instr::new(OpBra { target: loop_label, cond: SrcRef::True.into() }));
        self.finish_block_no_fallthrough()?;

        self.loop_stack.pop();
        self.dead_code = false;
        self.start_block();
        Ok(())
    }

    fn lower_loop(
        &mut self,
        body: &[AstStatement],
        continuing: &[AstStatement],
        break_if: Option<Handle<Expression>>,
        func: &ast::Function,
    ) -> Result<(), CompileError> {
        let loop_label = self.label_alloc.alloc();
        let exit_label = self.label_alloc.alloc();
        let cont_label = self.label_alloc.alloc();

        self.push_instr(Instr::new(OpBra { target: loop_label, cond: SrcRef::True.into() }));
        self.finish_block_no_fallthrough()?;

        self.loop_stack.push(LoopCtx {
            exit_label,
            continue_label: cont_label,
            break_blocks: Vec::new(),
            continue_blocks: Vec::new(),
        });

        self.start_block();
        for s in body {
            self.lower_stmt(s, func)?;
        }

        self.dead_code = false;
        for s in continuing {
            self.lower_stmt(s, func)?;
        }

        if let Some(bi) = break_if {
            let cond = self.ensure_expr(bi, func)?;
            let pred = self.alloc_ssa(RegFile::Pred);
            self.push_instr(Instr::new(OpISetP {
                dst: pred.into(),
                set_op: PredSetOp::And,
                cmp_op: IntCmpOp::Ne,
                cmp_type: IntCmpType::U32,
                ex: false,
                srcs: [Src::from(cond[0]), Src::ZERO, SrcRef::True.into(), SrcRef::True.into()],
            }));
            self.push_instr(Instr::new(OpBra {
                target: exit_label,
                cond: Src::from(pred),
            }));
        }

        self.push_instr(Instr::new(OpBra { target: loop_label, cond: SrcRef::True.into() }));
        self.finish_block_no_fallthrough()?;

        self.loop_stack.pop();
        self.dead_code = false;
        self.start_block();
        Ok(())
    }

    fn lower_break(&mut self) -> Result<(), CompileError> {
        if let Some(ctx) = self.loop_stack.last() {
            let exit = ctx.exit_label;
            self.push_instr(Instr::new(OpBra { target: exit, cond: SrcRef::True.into() }));
            let blk = self.finish_block_no_fallthrough()?;
            if let Some(ctx) = self.loop_stack.last_mut() {
                ctx.break_blocks.push(blk);
            }
            self.start_block();
            self.dead_code = true;
        }
        Ok(())
    }

    fn lower_continue(&mut self) -> Result<(), CompileError> {
        if let Some(ctx) = self.loop_stack.last() {
            let cont = ctx.continue_label;
            self.push_instr(Instr::new(OpBra { target: cont, cond: SrcRef::True.into() }));
            let blk = self.finish_block_no_fallthrough()?;
            if let Some(ctx) = self.loop_stack.last_mut() {
                ctx.continue_blocks.push(blk);
            }
            self.start_block();
            self.dead_code = true;
        }
        Ok(())
    }

    fn lower_inc_dec(
        &mut self,
        pointer: Handle<Expression>,
        is_inc: bool,
        func: &ast::Function,
    ) -> Result<(), CompileError> {
        self.ensure_expr(pointer, func)?;
        let ptr_expr = &func.expressions[pointer];
        if let Expression::LocalVariable(idx) = ptr_expr {
            if (*idx as usize) < self.var_storage.len() {
                let old = self.var_storage[*idx as usize].clone();
                let new = self.alloc_ssa(RegFile::GPR);
                if is_inc {
                    self.push_instr(Instr::new(OpIAdd2 {
                        dsts: [new.into(), Dst::None],
                        srcs: [Src::from(old[0]), Src::new_imm_u32(1)],
                    }));
                } else {
                    let one = self.alloc_ssa(RegFile::GPR);
                    self.push_instr(Instr::new(OpCopy { dst: one.into(), src: Src::new_imm_u32(1) }));
                    self.push_instr(Instr::new(OpIAdd2 {
                        dsts: [new.into(), Dst::None],
                        srcs: [Src::from(old[0]), Src::from(one).bnot()],
                    }));
                }
                self.var_storage[*idx as usize] = new.into();
            }
        }
        Ok(())
    }

    fn lower_atomic(
        &mut self,
        pointer: Handle<Expression>,
        fun: ast::AtomicFunction,
        value: Handle<Expression>,
        result: Option<Handle<Expression>>,
        func: &ast::Function,
    ) -> Result<(), CompileError> {
        let addr = self.ensure_expr(pointer, func)?;
        let data = self.ensure_expr(value, func)?;
        let ptr_key = pointer.index();

        let atom_op = match fun {
            ast::AtomicFunction::Add => AtomOp::Add,
            ast::AtomicFunction::Subtract => AtomOp::Add, // negate data
            ast::AtomicFunction::Min => AtomOp::Min,
            ast::AtomicFunction::Max => AtomOp::Max,
            ast::AtomicFunction::And => AtomOp::And,
            ast::AtomicFunction::Or => AtomOp::Or,
            ast::AtomicFunction::Xor => AtomOp::Xor,
            ast::AtomicFunction::Exchange => AtomOp::Exch,
            ast::AtomicFunction::CompareExchange => AtomOp::CmpExch(AtomCmpSrc::Packed),
        };

        let data_src = if matches!(fun, ast::AtomicFunction::Subtract) {
            let neg = self.alloc_ssa(RegFile::GPR);
            self.push_instr(Instr::new(OpFAdd {
                dst: neg.into(),
                srcs: [Src::ZERO, Src::from(data[0]).fneg()],
                saturate: false,
                rnd_mode: FRndMode::NearestEven,
                ftz: false,
            }));
            Src::from(neg)
        } else {
            Src::from(data[0])
        };

        let (mem_space, mem_order) = if self.shared_ptrs.contains(&ptr_key) {
            (MemSpace::Shared, MemOrder::Strong(MemScope::CTA))
        } else {
            (MemSpace::Global(MemAddrType::A64), MemOrder::Strong(MemScope::System))
        };

        let dst_val = self.alloc_ssa(RegFile::GPR);
        self.push_instr(Instr::new(OpAtom {
            dst: dst_val.into(),
            srcs: [Src::from(addr[0]), Src::ZERO, data_src],
            atom_op,
            atom_type: AtomType::U32,
            addr_offset: 0,
            addr_stride: OffsetStride::X1,
            mem_space,
            mem_order,
            mem_eviction_priority: MemEvictionPriority::Normal,
        }));

        if let Some(result_handle) = result {
            self.expr_map.insert(result_handle.index(), dst_val.into());
        }
        Ok(())
    }

    fn lower_switch(
        &mut self,
        selector: Handle<Expression>,
        cases: &[ast::SwitchCase],
        func: &ast::Function,
    ) -> Result<(), CompileError> {
        let sel = self.ensure_expr(selector, func)?;
        let merge_label = self.label_alloc.alloc();

        for case in cases {
            match case.value {
                ast::SwitchValue::Default => {
                    for s in &case.body {
                        self.lower_stmt(s, func)?;
                    }
                }
                ast::SwitchValue::I32(val) => {
                    let pred = self.alloc_ssa(RegFile::Pred);
                    self.push_instr(Instr::new(OpISetP {
                        dst: pred.into(),
                        set_op: PredSetOp::And,
                        cmp_op: IntCmpOp::Eq,
                        cmp_type: IntCmpType::I32,
                        ex: false,
                        srcs: [Src::from(sel[0]), Src::new_imm_u32(val as u32), SrcRef::True.into(), SrcRef::True.into()],
                    }));
                    let skip_label = self.label_alloc.alloc();
                    self.push_instr(Instr::new(OpBra {
                        target: skip_label,
                        cond: Src::from(pred).bnot(),
                    }));
                    self.finish_block_no_fallthrough()?;
                    self.start_block();
                    for s in &case.body {
                        self.lower_stmt(s, func)?;
                    }
                    if !case.fall_through && !self.dead_code {
                        self.push_instr(Instr::new(OpBra {
                            target: merge_label,
                            cond: SrcRef::True.into(),
                        }));
                        self.finish_block_no_fallthrough()?;
                    } else {
                        self.finish_block_no_fallthrough()?;
                    }
                    self.dead_code = false;
                    self.start_block();
                }
                ast::SwitchValue::U32(val) => {
                    let pred = self.alloc_ssa(RegFile::Pred);
                    self.push_instr(Instr::new(OpISetP {
                        dst: pred.into(),
                        set_op: PredSetOp::And,
                        cmp_op: IntCmpOp::Eq,
                        cmp_type: IntCmpType::U32,
                        ex: false,
                        srcs: [Src::from(sel[0]), Src::new_imm_u32(val), SrcRef::True.into(), SrcRef::True.into()],
                    }));
                    let skip_label = self.label_alloc.alloc();
                    self.push_instr(Instr::new(OpBra {
                        target: skip_label,
                        cond: Src::from(pred).bnot(),
                    }));
                    self.finish_block_no_fallthrough()?;
                    self.start_block();
                    for s in &case.body {
                        self.lower_stmt(s, func)?;
                    }
                    if !case.fall_through && !self.dead_code {
                        self.push_instr(Instr::new(OpBra {
                            target: merge_label,
                            cond: SrcRef::True.into(),
                        }));
                        self.finish_block_no_fallthrough()?;
                    } else {
                        self.finish_block_no_fallthrough()?;
                    }
                    self.dead_code = false;
                    self.start_block();
                }
            }
        }
        Ok(())
    }
}
