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
            _ => Ok(()),
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
}
