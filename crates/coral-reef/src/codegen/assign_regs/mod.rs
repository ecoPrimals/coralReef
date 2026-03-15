// SPDX-License-Identifier: AGPL-3.0-only
// Copyright © 2025-2026 ecoPrimals
// Derived from Collabora, Ltd. (2022)

#![allow(clippy::wildcard_imports)]

use super::debug::{DEBUG, GetDebugFlags};
use super::ir::*;
use super::liveness::{Liveness, SimpleLiveness};

use std::cmp::{max, min};

mod block;
mod instr_assign;
mod reg_allocator;
mod types;

use block::*;
use instr_assign::*;
use reg_allocator::*;
use types::*;

impl Shader<'_> {
    /// Assigns hardware registers to virtual registers for all instructions.
    ///
    /// # Errors
    ///
    /// Returns [`CompileError`](crate::CompileError) if spilling or register allocation fails.
    pub fn assign_regs(&mut self) -> Result<(), crate::CompileError> {
        assert!(self.functions.len() == 1);
        let f = &mut self.functions[0];

        // Convert to CSSA before we spill or assign registers
        f.to_cssa();

        let mut live = SimpleLiveness::for_function(f);
        let mut max_live = live.calc_max_live_back_edge_aware(f);

        // We want at least one temporary GPR reserved for parallel copies.
        let mut tmp_gprs = 1_u8;

        let spill_files = [RegFile::UPred, RegFile::Pred, RegFile::UGPR, RegFile::Bar];
        for file in spill_files {
            let reg_count = self.sm.reg_count(file);
            if max_live[file] > reg_count {
                f.spill_values(file, reg_count, &mut self.info)?;

                // Re-calculate liveness after we spill
                live = SimpleLiveness::for_function(f);
                max_live = live.calc_max_live_back_edge_aware(f);

                if file == RegFile::Bar {
                    tmp_gprs = max(tmp_gprs, 2);
                }
            }
        }

        // An instruction can have at most 4 vector sources/destinations.  In
        // order to ensure we always succeed at allocation, regardless of
        // arbitrary choices, we need at least 16 GPRs.
        let mut gpr_limit = max(max_live[RegFile::GPR], 16);
        let mut total_gprs = gpr_limit + u32::from(tmp_gprs);

        let mut max_gpr_count = self.sm.reg_count(RegFile::GPR);

        if DEBUG.spill() {
            // To test spilling, reduce the number of registers to the minimum
            // practical for RA.  We need at least 16 registers to satisfy RA
            // constraints for texture ops.
            max_gpr_count = 16;

            // OpRegOut can use arbitrarily many GPRs
            for b in &f.blocks {
                for instr in b.instrs.iter().rev() {
                    match &instr.op {
                        Op::Exit(_) => (),
                        Op::RegOut(op) => {
                            let out_gprs =
                                u32::try_from(op.srcs.len()).expect("RegOut srcs len overflow");
                            max_gpr_count = max(max_gpr_count, out_gprs);
                        }
                        _ => break,
                    }
                }
            }

            // and another 2 for parallel copy
            max_gpr_count += 2;
        }

        let hw_reserved_gpr_count = self.sm.hw_reserved_gpr_count();
        if let ShaderStageInfo::Compute(cs_info) = &self.info.stage {
            max_gpr_count = min(
                max_gpr_count,
                gpr_limit_from_local_size(&cs_info.local_size) - hw_reserved_gpr_count,
            );
        }

        if total_gprs > max_gpr_count {
            // If we're spilling GPRs, we need to reserve 2 GPRs for OpParCopy
            // lowering because it needs to be able lower Mem copies which
            // require a temporary
            tmp_gprs = max(tmp_gprs, 2);
            total_gprs = max_gpr_count;
            gpr_limit = total_gprs - u32::from(tmp_gprs);

            f.spill_values(RegFile::GPR, gpr_limit, &mut self.info)?;

            // Re-calculate liveness one last time
            live = SimpleLiveness::for_function(f);
        } else {
            // GPRs are allocated in multiple of 8. That means we can give RA a
            // bit more freedom by making gprs up until the next multiple
            // available.
            let next_multiple_gprs =
                (total_gprs + hw_reserved_gpr_count).next_multiple_of(8) - hw_reserved_gpr_count;
            let free_gprs = next_multiple_gprs.min(max_gpr_count) - total_gprs;

            total_gprs += free_gprs;
            gpr_limit += free_gprs;
        }

        self.info.gpr_count = total_gprs.try_into().expect("gpr_count overflow");

        let limit = PerRegFile::new_with(|file| {
            if file == RegFile::GPR {
                gpr_limit
            } else {
                self.sm.reg_count(file)
            }
        });

        let mut phi_webs = PhiWebs::new(f);

        let mut blocks: Vec<AssignRegsBlock> = Vec::new();
        let mut unreachable: Vec<bool> = Vec::new();
        for b_idx in 0..f.blocks.len() {
            let pred = f.blocks.pred_indices(b_idx);
            let fwd_preds: Vec<usize> = pred.iter().filter(|&&p| p < b_idx).copied().collect();
            let has_back_edge = pred.iter().any(|&p| p >= b_idx);

            let is_unreachable = b_idx != 0 && pred.is_empty();
            unreachable.push(is_unreachable);

            let mut arb = AssignRegsBlock::new(&limit, tmp_gprs, b_idx);
            if !is_unreachable {
                let pred_ras: Vec<&PerRegFile<RegAllocator>> =
                    fwd_preds.iter().map(|&p| &blocks[p].ra).collect();

                let bl = live.block_live(b_idx);
                arb.first_pass(&mut f.blocks[b_idx], bl, &pred_ras, &mut phi_webs);

                // For loop headers: pre-allocate registers for live-in values
                // that come only from back-edge predecessors (not yet processed).
                // These fresh registers propagate through the loop body and exit
                // path. The latch's second_pass inserts copies to fill them.
                if has_back_edge {
                    let all_live_in = live.live_in_values(b_idx);
                    arb.pre_alloc_back_edge_live_in(&all_live_in);
                }
            }

            assert!(blocks.len() == b_idx);
            blocks.push(arb);
        }

        for b_idx in 0..f.blocks.len() {
            if unreachable[b_idx] {
                continue;
            }
            let arb = &blocks[b_idx];
            for sb_idx in f.blocks.succ_indices(b_idx).to_vec() {
                if !unreachable[sb_idx] {
                    arb.second_pass(&blocks[sb_idx], &mut f.blocks[b_idx]);
                }
            }
        }
        Ok(())
    }
}
