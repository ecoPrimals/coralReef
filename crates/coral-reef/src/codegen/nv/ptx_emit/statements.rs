// SPDX-License-Identifier: AGPL-3.0-or-later

use std::fmt::Write as _;

use crate::error::CompileError;

use super::PtxEmitter;
use super::types::{MemSpaceKind, PtxVal};

impl PtxEmitter<'_> {
    pub(super) fn emit_block(&mut self, block: &naga::Block) -> Result<(), CompileError> {
        for stmt in block {
            self.emit_stmt(stmt)?;
        }
        Ok(())
    }

    fn emit_stmt(&mut self, stmt: &naga::Statement) -> Result<(), CompileError> {
        match *stmt {
            naga::Statement::Emit(ref range) => {
                let handles: Vec<_> = range.clone().collect();
                for h in handles {
                    self.eval_expr(h)?;
                }
                Ok(())
            }
            naga::Statement::Store { pointer, value } => self.emit_store(pointer, value),
            naga::Statement::If {
                condition,
                ref accept,
                ref reject,
            } => {
                let cond = self.eval_expr(condition)?;
                let pred = self.ensure_pred(&cond)?;
                let end_label = self.alloc_label();

                if reject.is_empty() {
                    writeln!(self.body, "    @!{} bra $L{end_label};", pred.fmt_operand())
                        .expect("write to String");
                    self.emit_block(accept)?;
                } else {
                    let then_label = self.alloc_label();
                    let else_label = self.alloc_label();
                    writeln!(self.body, "    @{} bra $L{then_label};", pred.fmt_operand())
                        .expect("write to String");
                    writeln!(self.body, "    bra $L{else_label};").expect("write to String");
                    writeln!(self.body, "$L{then_label}:").expect("write to String");
                    self.emit_block(accept)?;
                    writeln!(self.body, "    bra $L{end_label};").expect("write to String");
                    writeln!(self.body, "$L{else_label}:").expect("write to String");
                    self.emit_block(reject)?;
                }
                writeln!(self.body, "$L{end_label}:").expect("write to String");
                Ok(())
            }
            naga::Statement::Loop {
                ref body,
                ref continuing,
                break_if,
            } => {
                let loop_label = self.alloc_label();
                let cont_label = self.alloc_label();
                let end_label = self.alloc_label();

                writeln!(self.body, "$L{loop_label}:").expect("write to String");
                self.emit_block(body)?;
                writeln!(self.body, "$L{cont_label}:").expect("write to String");
                self.emit_block(continuing)?;
                if let Some(break_cond) = break_if {
                    let cond = self.eval_expr(break_cond)?;
                    let pred = self.ensure_pred(&cond)?;
                    writeln!(self.body, "    @{} bra $L{end_label};", pred.fmt_operand())
                        .expect("write to String");
                }
                writeln!(self.body, "    bra $L{loop_label};").expect("write to String");
                writeln!(self.body, "$L{end_label}:").expect("write to String");
                Ok(())
            }
            naga::Statement::Return { value: _ } => {
                writeln!(self.body, "    ret;").expect("write to String");
                Ok(())
            }
            naga::Statement::ControlBarrier(_) => {
                self.barrier_count += 1;
                writeln!(self.body, "    bar.sync 0;").expect("write to String");
                Ok(())
            }
            naga::Statement::Block(ref block) => self.emit_block(block),
            naga::Statement::Break => {
                // Loops manage their own break labels — we use `bra $Lend`
                // This is handled via the break_if path. Standalone Break
                // would need a label stack; for now, emit ret as a safe fallback.
                writeln!(self.body, "    ret;").expect("write to String");
                Ok(())
            }
            naga::Statement::Continue => {
                // Same concern as Break — needs label stack for full support.
                Ok(())
            }
            naga::Statement::Switch {
                selector,
                ref cases,
            } => {
                let sel = self.eval_expr(selector)?;
                let end_label = self.alloc_label();

                let mut case_labels: Vec<(u32, i64)> = Vec::new();
                let mut default_label = None;

                for case in cases {
                    let lbl = self.alloc_label();
                    match case.value {
                        naga::SwitchValue::I32(v) => case_labels.push((lbl, i64::from(v))),
                        naga::SwitchValue::U32(v) => case_labels.push((lbl, i64::from(v))),
                        naga::SwitchValue::Default => default_label = Some(lbl),
                    }
                }

                let default_lbl = default_label.unwrap_or(end_label);

                for &(lbl, val) in &case_labels {
                    let pred = self.alloc_pred();
                    writeln!(
                        self.body,
                        "    setp.eq.s32 {}, {}, {val};",
                        pred.fmt_operand(),
                        sel.fmt_operand(),
                    )
                    .expect("write to String");
                    writeln!(self.body, "    @{} bra $L{lbl};", pred.fmt_operand())
                        .expect("write to String");
                }
                writeln!(self.body, "    bra $L{default_lbl};").expect("write to String");

                let mut label_iter = case_labels.iter().map(|(lbl, _)| *lbl);
                let mut default_iter = default_label.into_iter();
                for case in cases {
                    let lbl = match case.value {
                        naga::SwitchValue::Default => default_iter.next().unwrap_or(end_label),
                        _ => label_iter.next().unwrap_or(end_label),
                    };
                    writeln!(self.body, "$L{lbl}:").expect("write to String");
                    self.emit_block(&case.body)?;
                    if case.fall_through {
                        // fall through to next case
                    } else {
                        writeln!(self.body, "    bra $L{end_label};").expect("write to String");
                    }
                }

                writeln!(self.body, "$L{end_label}:").expect("write to String");
                Ok(())
            }
            naga::Statement::Kill => {
                writeln!(self.body, "    exit;").expect("write to String");
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
            naga::Statement::MemoryBarrier(barrier) => {
                let scope = if barrier.contains(naga::Barrier::STORAGE) {
                    "gl"
                } else {
                    "cta"
                };
                writeln!(self.body, "    membar.{scope};").expect("write to String");
                Ok(())
            }
            naga::Statement::Atomic {
                pointer,
                ref fun,
                value,
                result,
            } => self.emit_atomic(pointer, fun, value, result),
            _ => Ok(()),
        }
    }

    fn emit_atomic(
        &mut self,
        pointer: naga::Handle<naga::Expression>,
        fun: &naga::AtomicFunction,
        value: naga::Handle<naga::Expression>,
        result: Option<naga::Handle<naga::Expression>>,
    ) -> Result<(), CompileError> {
        let (addr, mem_space) = self.eval_pointer(pointer)?;
        let val = self.eval_expr(value)?;
        let val_scalar = self.scalar_of(value);

        let space = if mem_space == MemSpaceKind::Shared {
            "shared"
        } else {
            "global"
        };

        let type_suffix = Self::ptx_atom_type(val_scalar);

        let dst = self.alloc_for_scalar(val_scalar);

        match fun {
            naga::AtomicFunction::Exchange { compare: Some(cmp) } => {
                let cmp_val = self.eval_expr(*cmp)?;
                writeln!(
                    self.body,
                    "    atom.{space}.cas.{type_suffix} {}, [{}], {}, {};",
                    dst.fmt_operand(),
                    addr.fmt_operand(),
                    cmp_val.fmt_operand(),
                    val.fmt_operand(),
                )
                .expect("write to String");
            }
            naga::AtomicFunction::Subtract => {
                let neg = self.alloc_for_scalar(val_scalar);
                writeln!(
                    self.body,
                    "    neg.{type_suffix} {}, {};",
                    neg.fmt_operand(),
                    val.fmt_operand(),
                )
                .expect("write to String");
                writeln!(
                    self.body,
                    "    atom.{space}.add.{type_suffix} {}, [{}], {};",
                    dst.fmt_operand(),
                    addr.fmt_operand(),
                    neg.fmt_operand(),
                )
                .expect("write to String");
            }
            _ => {
                let op = Self::ptx_atom_op(fun);
                writeln!(
                    self.body,
                    "    atom.{space}.{op}.{type_suffix} {}, [{}], {};",
                    dst.fmt_operand(),
                    addr.fmt_operand(),
                    val.fmt_operand(),
                )
                .expect("write to String");
            }
        }

        if let Some(res_handle) = result {
            self.values.insert(res_handle, dst);
        }

        Ok(())
    }

    fn ptx_atom_op(fun: &naga::AtomicFunction) -> &'static str {
        match fun {
            naga::AtomicFunction::Add | naga::AtomicFunction::Subtract => "add",
            naga::AtomicFunction::And => "and",
            naga::AtomicFunction::InclusiveOr => "or",
            naga::AtomicFunction::ExclusiveOr => "xor",
            naga::AtomicFunction::Min => "min",
            naga::AtomicFunction::Max => "max",
            naga::AtomicFunction::Exchange { .. } => "exch",
        }
    }

    fn ptx_atom_type(scalar: naga::Scalar) -> &'static str {
        match (scalar.kind, scalar.width) {
            (naga::ScalarKind::Uint, 4) => "u32",
            (naga::ScalarKind::Sint, 4) => "s32",
            (naga::ScalarKind::Float, 4) => "f32",
            (naga::ScalarKind::Uint, 8) => "u64",
            (naga::ScalarKind::Sint, 8) => "s64",
            (naga::ScalarKind::Float, 8) => "f64",
            _ => "u32",
        }
    }

    fn emit_store(
        &mut self,
        pointer: naga::Handle<naga::Expression>,
        value: naga::Handle<naga::Expression>,
    ) -> Result<(), CompileError> {
        let val = self.eval_expr(value)?;
        let val_scalar = self.scalar_of(value);

        if let Some(lv_handle) = self.expr_is_local_var(pointer) {
            if let PtxVal::Vec(components) = &val {
                let new_val = PtxVal::Vec(components.clone());
                self.locals.insert(lv_handle, new_val);
            } else {
                let dst = self.locals.get(&lv_handle).cloned();
                if let Some(dst) = dst {
                    self.emit_mov(&dst, &val, val_scalar);
                } else {
                    self.locals.insert(lv_handle, val);
                }
            }
            return Ok(());
        }

        if let Some(lv_comp) = self.expr_is_local_var_component(pointer) {
            let (lv_handle, comp_idx) = lv_comp;
            if let Some(PtxVal::Vec(components)) = self.locals.get(&lv_handle).cloned() {
                let dst = components[comp_idx].clone();
                self.emit_mov(&dst, &val, val_scalar);
            }
            return Ok(());
        }

        let (addr, mem_space) = self.eval_pointer(pointer)?;
        let space_prefix = if mem_space == MemSpaceKind::Shared {
            "shared"
        } else {
            "global"
        };

        match &val {
            PtxVal::Vec(components) => {
                for (i, comp) in components.iter().enumerate() {
                    let offset = i as u32 * u32::from(val_scalar.width);
                    if offset == 0 {
                        writeln!(
                            self.body,
                            "    st.{space_prefix}.{} [{}], {};",
                            Self::ptx_mem_suffix(val_scalar),
                            addr.fmt_operand(),
                            comp.fmt_operand(),
                        )
                        .expect("write to String");
                    } else {
                        let off_reg = self.alloc_rd64();
                        writeln!(
                            self.body,
                            "    add.u64 {}, {}, {offset};",
                            off_reg.fmt_operand(),
                            addr.fmt_operand(),
                        )
                        .expect("write to String");
                        writeln!(
                            self.body,
                            "    st.{space_prefix}.{} [{}], {};",
                            Self::ptx_mem_suffix(val_scalar),
                            off_reg.fmt_operand(),
                            comp.fmt_operand(),
                        )
                        .expect("write to String");
                    }
                }
            }
            _ => {
                writeln!(
                    self.body,
                    "    st.{space_prefix}.{} [{}], {};",
                    Self::ptx_mem_suffix(val_scalar),
                    addr.fmt_operand(),
                    val.fmt_operand(),
                )
                .expect("write to String");
            }
        }
        Ok(())
    }

    pub(super) fn emit_mov(&mut self, dst: &PtxVal, src: &PtxVal, scalar: naga::Scalar) {
        let suffix = if scalar.width == 8 { "u64" } else { "u32" };
        writeln!(
            self.body,
            "    mov.{suffix} {}, {};",
            dst.fmt_operand(),
            src.fmt_operand(),
        )
        .expect("write to String");
    }

    fn emit_subgroup_ballot(
        &mut self,
        result: naga::Handle<naga::Expression>,
        predicate: Option<naga::Handle<naga::Expression>>,
    ) -> Result<(), CompileError> {
        let pred_op = if let Some(pred_h) = predicate {
            let p = self.eval_expr(pred_h)?;
            self.ensure_pred(&p)?.fmt_operand()
        } else {
            "1".to_string()
        };
        let dst = self.alloc_r32();
        writeln!(
            self.body,
            "    vote.sync.ballot.b32 {}, {pred_op}, 0xFFFFFFFF;",
            dst.fmt_operand(),
        )
        .expect("write to String");
        self.values.insert(result, dst);
        Ok(())
    }

    fn emit_subgroup_collective(
        &mut self,
        op: naga::SubgroupOperation,
        collective_op: naga::CollectiveOperation,
        argument: naga::Handle<naga::Expression>,
        result: naga::Handle<naga::Expression>,
    ) -> Result<(), CompileError> {
        let val = self.eval_expr(argument)?;
        let val_scalar = self.scalar_of(argument);
        let dst = self.alloc_for_scalar(val_scalar);
        let type_suffix = Self::ptx_atom_type(val_scalar);

        let reduce_op = match collective_op {
            naga::CollectiveOperation::Reduce => match op {
                naga::SubgroupOperation::All => "and",
                naga::SubgroupOperation::Any => "or",
                _ => "add",
            },
            naga::CollectiveOperation::InclusiveScan | naga::CollectiveOperation::ExclusiveScan => {
                return Err(CompileError::NotImplemented(
                    format!("PTX subgroup scan: {collective_op:?} {op:?}").into(),
                ));
            }
        };

        writeln!(
            self.body,
            "    redux.sync.{reduce_op}.{type_suffix} {}, {}, 0xFFFFFFFF;",
            dst.fmt_operand(),
            val.fmt_operand(),
        )
        .expect("write to String");
        self.values.insert(result, dst);
        Ok(())
    }

    fn emit_subgroup_gather(
        &mut self,
        mode: naga::GatherMode,
        argument: naga::Handle<naga::Expression>,
        result: naga::Handle<naga::Expression>,
    ) -> Result<(), CompileError> {
        let val = self.eval_expr(argument)?;
        let val_scalar = self.scalar_of(argument);
        let dst = self.alloc_for_scalar(val_scalar);

        match mode {
            naga::GatherMode::BroadcastFirst => {
                writeln!(
                    self.body,
                    "    shfl.sync.idx.b32 {}, {}, 0, 0x1f1f, 0xFFFFFFFF;",
                    dst.fmt_operand(),
                    val.fmt_operand(),
                )
                .expect("write to String");
            }
            naga::GatherMode::Broadcast(idx_h) => {
                let idx = self.eval_expr(idx_h)?;
                writeln!(
                    self.body,
                    "    shfl.sync.idx.b32 {}, {}, {}, 0x1f1f, 0xFFFFFFFF;",
                    dst.fmt_operand(),
                    val.fmt_operand(),
                    idx.fmt_operand(),
                )
                .expect("write to String");
            }
            naga::GatherMode::ShuffleDown(offset_h) => {
                let offset = self.eval_expr(offset_h)?;
                writeln!(
                    self.body,
                    "    shfl.sync.down.b32 {}, {}, {}, 0x1f, 0xFFFFFFFF;",
                    dst.fmt_operand(),
                    val.fmt_operand(),
                    offset.fmt_operand(),
                )
                .expect("write to String");
            }
            naga::GatherMode::ShuffleUp(offset_h) => {
                let offset = self.eval_expr(offset_h)?;
                writeln!(
                    self.body,
                    "    shfl.sync.up.b32 {}, {}, {}, 0, 0xFFFFFFFF;",
                    dst.fmt_operand(),
                    val.fmt_operand(),
                    offset.fmt_operand(),
                )
                .expect("write to String");
            }
            naga::GatherMode::ShuffleXor(mask_h) => {
                let mask = self.eval_expr(mask_h)?;
                writeln!(
                    self.body,
                    "    shfl.sync.bfly.b32 {}, {}, {}, 0x1f, 0xFFFFFFFF;",
                    dst.fmt_operand(),
                    val.fmt_operand(),
                    mask.fmt_operand(),
                )
                .expect("write to String");
            }
            naga::GatherMode::Shuffle(idx_h) => {
                let idx = self.eval_expr(idx_h)?;
                writeln!(
                    self.body,
                    "    shfl.sync.idx.b32 {}, {}, {}, 0x1f1f, 0xFFFFFFFF;",
                    dst.fmt_operand(),
                    val.fmt_operand(),
                    idx.fmt_operand(),
                )
                .expect("write to String");
            }
            naga::GatherMode::QuadBroadcast(idx_h) => {
                let idx = self.eval_expr(idx_h)?;
                writeln!(
                    self.body,
                    "    shfl.sync.idx.b32 {}, {}, {}, 0x0003, 0xFFFFFFFF;",
                    dst.fmt_operand(),
                    val.fmt_operand(),
                    idx.fmt_operand(),
                )
                .expect("write to String");
            }
            naga::GatherMode::QuadSwap(_direction) => {
                writeln!(
                    self.body,
                    "    shfl.sync.bfly.b32 {}, {}, 1, 0x03, 0xFFFFFFFF;",
                    dst.fmt_operand(),
                    val.fmt_operand(),
                )
                .expect("write to String");
            }
        }

        self.values.insert(result, dst);
        Ok(())
    }
}
