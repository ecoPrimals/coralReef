// SPDX-License-Identifier: AGPL-3.0-only
//! Control flow translation: if/else, loops, phi node emission.
#![allow(clippy::wildcard_imports)]
use super::super::ir::*;
use super::func::{FuncTranslator, LoopContext};
use crate::error::CompileError;
use naga::Handle;

impl<'a, 'b> FuncTranslator<'a, 'b> {
    pub(super) fn translate_if(
        &mut self,
        condition: Handle<naga::Expression>,
        accept: &naga::Block,
        reject: &naga::Block,
    ) -> Result<(), CompileError> {
        let cond = self.ensure_expr(condition)?;
        let merge_label = self.label_alloc.alloc();
        let cond_src: Src = cond[0].into();

        let pre_if_storage = self.var_storage.clone();
        let need_phis = !self.var_storage.is_empty();

        let slot_phis: Vec<Vec<Phi>> = if need_phis {
            self.var_storage
                .iter()
                .map(|ssa| (0..ssa.comps()).map(|_| self.phi_alloc.alloc()).collect())
                .collect()
        } else {
            Vec::new()
        };

        if reject.is_empty() {
            if need_phis {
                let mut cond_phi_srcs = OpPhiSrcs::new();
                for (slot_idx, phis) in slot_phis.iter().enumerate() {
                    for (c, phi) in phis.iter().enumerate() {
                        cond_phi_srcs
                            .srcs
                            .push(*phi, Src::from(pre_if_storage[slot_idx][c]));
                    }
                }
                self.push_instr(Instr::new(cond_phi_srcs));
            }

            self.push_instr(Instr::new(OpBra {
                target: merge_label,
                cond: cond_src.bnot(),
            }));
            let cond_block = self.finish_block_no_fallthrough()?;

            self.start_block();
            let accept_start = self.next_block_id;
            self.dead_code = false;
            self.translate_block(accept)?;
            let accept_dead = self.dead_code;

            if !accept_dead {
                if need_phis {
                    let mut accept_phi_srcs = OpPhiSrcs::new();
                    for (slot_idx, phis) in slot_phis.iter().enumerate() {
                        for (c, phi) in phis.iter().enumerate() {
                            if slot_idx < self.var_storage.len()
                                && c < self.var_storage[slot_idx].comps() as usize
                            {
                                accept_phi_srcs
                                    .srcs
                                    .push(*phi, Src::from(self.var_storage[slot_idx][c]));
                            }
                        }
                    }
                    self.push_instr(Instr::new(accept_phi_srcs));
                }
                self.push_instr(Instr::new(OpBra {
                    target: merge_label,
                    cond: SrcRef::True.into(),
                }));
                let accept_end = self.finish_block_no_fallthrough()?;
                self.add_cfg_edge(cond_block, accept_start);

                self.start_block();
                self.current_label = merge_label;
                let merge_block = self.next_block_id;
                self.add_cfg_edge(cond_block, merge_block);
                self.add_cfg_edge(accept_end, merge_block);
            } else {
                let _dead = self.finish_block_no_fallthrough()?;
                self.add_cfg_edge(cond_block, accept_start);

                self.start_block();
                self.current_label = merge_label;
                let merge_block = self.next_block_id;
                self.add_cfg_edge(cond_block, merge_block);
                self.var_storage = pre_if_storage;
            }

            if need_phis && !accept_dead {
                let mut phi_dsts = OpPhiDsts::new();
                for (slot_idx, phis) in slot_phis.iter().enumerate() {
                    let comps = phis.len() as u8;
                    let new_ssa = self.alloc_ssa_vec(RegFile::GPR, comps);
                    for (c, phi) in phis.iter().enumerate() {
                        phi_dsts.dsts.push(*phi, Dst::from(new_ssa[c]));
                    }
                    self.var_storage[slot_idx] = new_ssa;
                }
                self.push_instr(Instr::new(phi_dsts));
            }
        } else {
            let reject_label = self.label_alloc.alloc();

            self.push_instr(Instr::new(OpBra {
                target: reject_label,
                cond: cond_src.bnot(),
            }));
            let cond_block = self.finish_block_no_fallthrough()?;

            self.start_block();
            let accept_start = self.next_block_id;
            self.dead_code = false;
            self.translate_block(accept)?;
            let accept_dead = self.dead_code;
            let accept_storage = self.var_storage.clone();

            if !accept_dead && need_phis {
                let mut accept_phi_srcs = OpPhiSrcs::new();
                for (slot_idx, phis) in slot_phis.iter().enumerate() {
                    for (c, phi) in phis.iter().enumerate() {
                        if slot_idx < accept_storage.len()
                            && c < accept_storage[slot_idx].comps() as usize
                        {
                            accept_phi_srcs
                                .srcs
                                .push(*phi, Src::from(accept_storage[slot_idx][c]));
                        }
                    }
                }
                self.push_instr(Instr::new(accept_phi_srcs));
            }

            if !accept_dead {
                self.push_instr(Instr::new(OpBra {
                    target: merge_label,
                    cond: SrcRef::True.into(),
                }));
            }
            let accept_end = self.finish_block_no_fallthrough()?;
            self.add_cfg_edge(cond_block, accept_start);

            self.var_storage = pre_if_storage.clone();
            self.start_block();
            self.current_label = reject_label;
            let reject_start = self.next_block_id;
            self.dead_code = false;
            self.translate_block(reject)?;
            let reject_dead = self.dead_code;

            if !reject_dead && need_phis {
                let mut reject_phi_srcs = OpPhiSrcs::new();
                for (slot_idx, phis) in slot_phis.iter().enumerate() {
                    for (c, phi) in phis.iter().enumerate() {
                        if slot_idx < self.var_storage.len()
                            && c < self.var_storage[slot_idx].comps() as usize
                        {
                            reject_phi_srcs
                                .srcs
                                .push(*phi, Src::from(self.var_storage[slot_idx][c]));
                        }
                    }
                }
                self.push_instr(Instr::new(reject_phi_srcs));
            }

            if !reject_dead {
                self.push_instr(Instr::new(OpBra {
                    target: merge_label,
                    cond: SrcRef::True.into(),
                }));
            }
            let reject_end = self.finish_block_no_fallthrough()?;
            self.add_cfg_edge(cond_block, reject_start);

            self.start_block();
            self.current_label = merge_label;
            let merge_block = self.next_block_id;
            if !accept_dead {
                self.add_cfg_edge(accept_end, merge_block);
            }
            if !reject_dead {
                self.add_cfg_edge(reject_end, merge_block);
            }

            if accept_dead && reject_dead {
                self.var_storage = pre_if_storage;
            } else if accept_dead {
                // Only reject path reaches merge.
            } else if reject_dead {
                self.var_storage = accept_storage;
            }

            let both_live = !accept_dead && !reject_dead;
            if need_phis && both_live {
                let mut phi_dsts = OpPhiDsts::new();
                for (slot_idx, phis) in slot_phis.iter().enumerate() {
                    let comps = phis.len() as u8;
                    let new_ssa = self.alloc_ssa_vec(RegFile::GPR, comps);
                    for (c, phi) in phis.iter().enumerate() {
                        phi_dsts.dsts.push(*phi, Dst::from(new_ssa[c]));
                    }
                    self.var_storage[slot_idx] = new_ssa;
                }
                self.push_instr(Instr::new(phi_dsts));
            }
        }

        self.dead_code = false;
        self.current_block_id = None;

        Ok(())
    }

    pub(super) fn translate_loop(
        &mut self,
        body: &naga::Block,
        continuing: &naga::Block,
        break_if: Option<Handle<naga::Expression>>,
    ) -> Result<(), CompileError> {
        let header_label = self.label_alloc.alloc();
        let continue_label = self.label_alloc.alloc();
        let exit_label = self.label_alloc.alloc();

        let pre_loop_storage = self.var_storage.clone();
        let num_slots = self.var_storage.len();

        let mut slot_phis: Vec<Vec<Phi>> = Vec::new();
        let mut phi_dsts_op = OpPhiDsts::new();
        for slot_idx in 0..num_slots {
            let comps = self.var_storage[slot_idx].comps();
            let mut phis = Vec::with_capacity(comps as usize);
            let new_ssa = self.alloc_ssa_vec(RegFile::GPR, comps);
            for c in 0..comps as usize {
                let phi = self.phi_alloc.alloc();
                phi_dsts_op.dsts.push(phi, Dst::from(new_ssa[c]));
                phis.push(phi);
            }
            self.var_storage[slot_idx] = new_ssa;
            slot_phis.push(phis);
        }

        let mut exit_slot_phis: Vec<Vec<Phi>> = Vec::new();
        for slot_idx in 0..num_slots {
            let comps = self.var_storage[slot_idx].comps();
            let mut phis = Vec::with_capacity(comps as usize);
            for _ in 0..comps as usize {
                phis.push(self.phi_alloc.alloc());
            }
            exit_slot_phis.push(phis);
        }

        let mut continue_slot_phis: Vec<Vec<Phi>> = Vec::new();
        for slot_idx in 0..num_slots {
            let comps = self.var_storage[slot_idx].comps();
            let mut phis = Vec::with_capacity(comps as usize);
            for _ in 0..comps as usize {
                phis.push(self.phi_alloc.alloc());
            }
            continue_slot_phis.push(phis);
        }

        if num_slots > 0 {
            let mut pre_phi_srcs = OpPhiSrcs::new();
            for (slot_idx, phis) in slot_phis.iter().enumerate() {
                for (c, phi) in phis.iter().enumerate() {
                    pre_phi_srcs
                        .srcs
                        .push(*phi, Src::from(pre_loop_storage[slot_idx][c]));
                }
            }
            self.push_instr(Instr::new(pre_phi_srcs));
        }
        let pre_block = self.finish_block_no_fallthrough()?;

        self.loop_stack.push(LoopContext {
            exit_label,
            continue_label,
            continue_block_id: 0,
            break_blocks: Vec::new(),
            continue_blocks: Vec::new(),
            slot_phis: slot_phis.clone(),
            exit_slot_phis: exit_slot_phis.clone(),
            continue_slot_phis: continue_slot_phis.clone(),
        });

        self.start_block();
        self.current_label = header_label;
        let header_block_id = self.next_block_id;
        self.add_cfg_edge(pre_block, header_block_id);
        if num_slots > 0 {
            self.push_instr(Instr::new(phi_dsts_op));
        }

        self.translate_block(body)?;

        let body_ended_dead = self.dead_code;

        if !body_ended_dead && num_slots > 0 {
            let loop_ctx = self
                .loop_stack
                .last()
                .ok_or_else(|| CompileError::NotImplemented("loop stack empty in body".into()))?;
            let mut cont_phi_srcs = OpPhiSrcs::new();
            for (slot_idx, phis) in loop_ctx.continue_slot_phis.iter().enumerate() {
                if slot_idx < self.var_storage.len() {
                    for (c, phi) in phis.iter().enumerate() {
                        if c < self.var_storage[slot_idx].comps() as usize {
                            cont_phi_srcs
                                .srcs
                                .push(*phi, Src::from(self.var_storage[slot_idx][c]));
                        }
                    }
                }
            }
            self.push_instr(Instr::new(cont_phi_srcs));
        }

        if !body_ended_dead {
            self.push_instr(Instr::new(OpBra {
                target: continue_label,
                cond: SrcRef::True.into(),
            }));
        }
        let body_end = self.finish_block_no_fallthrough()?;

        self.start_block();
        self.current_label = continue_label;
        let continue_block_id = self.next_block_id;
        if !body_ended_dead {
            self.add_cfg_edge(body_end, continue_block_id);
        }
        if let Some(ctx) = self.loop_stack.last_mut() {
            ctx.continue_block_id = continue_block_id;
        }

        self.dead_code = false;

        if num_slots > 0 {
            let cont_phis: Vec<Vec<Phi>> = self
                .loop_stack
                .last()
                .ok_or_else(|| CompileError::NotImplemented("loop stack empty in continue".into()))?
                .continue_slot_phis
                .clone();
            let mut cont_phi_dsts = OpPhiDsts::new();
            for (slot_idx, phis) in cont_phis.iter().enumerate() {
                let comps = phis.len() as u8;
                let new_ssa = self.alloc_ssa_vec(RegFile::GPR, comps);
                for (c, phi) in phis.iter().enumerate() {
                    cont_phi_dsts.dsts.push(*phi, Dst::from(new_ssa[c]));
                }
                if slot_idx < self.var_storage.len() {
                    self.var_storage[slot_idx] = new_ssa;
                }
            }
            self.push_instr(Instr::new(cont_phi_dsts));
        }

        self.translate_block(continuing)?;

        let has_continue_preds = !self
            .loop_stack
            .last()
            .ok_or_else(|| CompileError::NotImplemented("loop stack empty in back-edge".into()))?
            .continue_blocks
            .is_empty();
        let continuing_reachable = !body_ended_dead || has_continue_preds;

        let back_block = if continuing_reachable {
            let break_cond_ssa = if let Some(break_cond) = break_if {
                Some(self.ensure_expr(break_cond)?)
            } else {
                None
            };

            if num_slots > 0 {
                let mut phi_srcs = OpPhiSrcs::new();
                let loop_ctx = self.loop_stack.last().ok_or_else(|| {
                    CompileError::NotImplemented("loop stack empty in phi_srcs".into())
                })?;
                if break_cond_ssa.is_some() {
                    for (slot_idx, phis) in loop_ctx.exit_slot_phis.iter().enumerate() {
                        if slot_idx < self.var_storage.len() {
                            for (c, phi) in phis.iter().enumerate() {
                                if c < self.var_storage[slot_idx].comps() as usize {
                                    phi_srcs
                                        .srcs
                                        .push(*phi, Src::from(self.var_storage[slot_idx][c]));
                                }
                            }
                        }
                    }
                }
                for (slot_idx, phis) in loop_ctx.slot_phis.iter().enumerate() {
                    if slot_idx < self.var_storage.len() {
                        for (c, phi) in phis.iter().enumerate() {
                            if c < self.var_storage[slot_idx].comps() as usize {
                                phi_srcs
                                    .srcs
                                    .push(*phi, Src::from(self.var_storage[slot_idx][c]));
                            }
                        }
                    }
                }
                self.push_instr(Instr::new(phi_srcs));
            }

            if let Some(cond_ssa) = break_cond_ssa {
                self.push_instr(Instr::new(OpBra {
                    target: exit_label,
                    cond: cond_ssa[0].into(),
                }));
            }

            self.push_instr(Instr::new(OpBra {
                target: header_label,
                cond: SrcRef::True.into(),
            }));
            let bb = self.finish_block_no_fallthrough()?;
            self.add_cfg_edge(bb, header_block_id);
            Some(bb)
        } else {
            let _bb = self.finish_block_no_fallthrough()?;
            None
        };

        let loop_ctx = self
            .loop_stack
            .pop()
            .ok_or_else(|| CompileError::NotImplemented("loop stack empty at exit".into()))?;

        self.start_block();
        self.current_label = exit_label;
        let exit_block_id = self.next_block_id;

        if !loop_ctx.exit_slot_phis.is_empty() {
            let mut exit_phi_dsts = OpPhiDsts::new();
            for (slot_idx, phis) in loop_ctx.exit_slot_phis.iter().enumerate() {
                let comps = phis.len() as u8;
                let new_ssa = self.alloc_ssa_vec(RegFile::GPR, comps);
                for (c, phi) in phis.iter().enumerate() {
                    exit_phi_dsts.dsts.push(*phi, Dst::from(new_ssa[c]));
                }
                if slot_idx < self.var_storage.len() {
                    self.var_storage[slot_idx] = new_ssa;
                }
            }
            self.push_instr(Instr::new(exit_phi_dsts));
        }

        for bb in &loop_ctx.break_blocks {
            self.add_cfg_edge(*bb, exit_block_id);
        }
        if break_if.is_some() {
            if let Some(bb) = back_block {
                self.add_cfg_edge(bb, exit_block_id);
            }
        }
        for cb in &loop_ctx.continue_blocks {
            self.add_cfg_edge(*cb, continue_block_id);
        }
        self.current_block_id = None;

        Ok(())
    }

    pub(super) fn emit_loop_phi_srcs(&mut self) -> Result<(), CompileError> {
        if let Some(loop_ctx) = self.loop_stack.last() {
            let mut phi_srcs = OpPhiSrcs::new();
            for (slot_idx, phis) in loop_ctx.slot_phis.iter().enumerate() {
                if slot_idx < self.var_storage.len() {
                    for (c, phi) in phis.iter().enumerate() {
                        let current = &self.var_storage[slot_idx];
                        if c < current.comps() as usize {
                            phi_srcs.srcs.push(*phi, Src::from(current[c]));
                        }
                    }
                }
            }
            self.push_instr(Instr::new(phi_srcs));
        }
        Ok(())
    }

    pub(super) fn emit_loop_continue_phi_srcs(&mut self) -> Result<(), CompileError> {
        if let Some(loop_ctx) = self.loop_stack.last() {
            let mut phi_srcs = OpPhiSrcs::new();
            for (slot_idx, phis) in loop_ctx.continue_slot_phis.iter().enumerate() {
                if slot_idx < self.var_storage.len() {
                    for (c, phi) in phis.iter().enumerate() {
                        let current = &self.var_storage[slot_idx];
                        if c < current.comps() as usize {
                            phi_srcs.srcs.push(*phi, Src::from(current[c]));
                        }
                    }
                }
            }
            self.push_instr(Instr::new(phi_srcs));
        }
        Ok(())
    }

    pub(super) fn emit_loop_exit_phi_srcs(&mut self) -> Result<(), CompileError> {
        if let Some(loop_ctx) = self.loop_stack.last() {
            let mut phi_srcs = OpPhiSrcs::new();
            for (slot_idx, phis) in loop_ctx.exit_slot_phis.iter().enumerate() {
                if slot_idx < self.var_storage.len() {
                    for (c, phi) in phis.iter().enumerate() {
                        let current = &self.var_storage[slot_idx];
                        if c < current.comps() as usize {
                            phi_srcs.srcs.push(*phi, Src::from(current[c]));
                        }
                    }
                }
            }
            self.push_instr(Instr::new(phi_srcs));
        }
        Ok(())
    }
}
