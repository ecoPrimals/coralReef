// Copyright © 2022 Collabora, Ltd.
// SPDX-License-Identifier: AGPL-3.0-only
// SPDX-License-Identifier: MIT

#![allow(clippy::wildcard_imports)]

use super::debug::{DEBUG, GetDebugFlags};
use super::ir::*;
use super::liveness::{Liveness, SimpleLiveness};
use super::*;

use std::cmp::{max, min};

mod block;
mod instr_assign;
mod reg_allocator;
mod types;

pub(super) use block::*;
pub(super) use instr_assign::*;
pub(super) use reg_allocator::*;
pub(super) use types::*;

impl Shader<'_> {
    pub fn assign_regs(&mut self) {
        assert!(self.functions.len() == 1);
        let f = &mut self.functions[0];

        // Convert to CSSA before we spill or assign registers
        f.to_cssa();

        let mut live = SimpleLiveness::for_function(f);
        let mut max_live = live.calc_max_live(f);

        // We want at least one temporary GPR reserved for parallel copies.
        let mut tmp_gprs = 1_u8;

        let spill_files = [RegFile::UPred, RegFile::Pred, RegFile::UGPR, RegFile::Bar];
        for file in spill_files {
            let num_regs = self.sm.num_regs(file);
            if max_live[file] > num_regs {
                f.spill_values(file, num_regs, &mut self.info);

                // Re-calculate liveness after we spill
                live = SimpleLiveness::for_function(f);
                max_live = live.calc_max_live(f);

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

        let mut max_gprs = self.sm.num_regs(RegFile::GPR);

        if DEBUG.spill() {
            // To test spilling, reduce the number of registers to the minimum
            // practical for RA.  We need at least 16 registers to satisfy RA
            // constraints for texture ops.
            max_gprs = 16;

            // OpRegOut can use arbitrarily many GPRs
            for b in &f.blocks {
                for instr in b.instrs.iter().rev() {
                    match &instr.op {
                        Op::Exit(_) => (),
                        Op::RegOut(op) => {
                            let out_gprs = u32::try_from(op.srcs.len()).unwrap();
                            max_gprs = max(max_gprs, out_gprs);
                        }
                        _ => break,
                    }
                }
            }

            // and another 2 for parallel copy
            max_gprs += 2;
        }

        let hw_reserved_gprs = self.sm.hw_reserved_gprs();
        if let ShaderStageInfo::Compute(cs_info) = &self.info.stage {
            max_gprs = min(
                max_gprs,
                gpr_limit_from_local_size(&cs_info.local_size) - hw_reserved_gprs,
            );
        }

        if total_gprs > max_gprs {
            // If we're spilling GPRs, we need to reserve 2 GPRs for OpParCopy
            // lowering because it needs to be able lower Mem copies which
            // require a temporary
            tmp_gprs = max(tmp_gprs, 2);
            total_gprs = max_gprs;
            gpr_limit = total_gprs - u32::from(tmp_gprs);

            f.spill_values(RegFile::GPR, gpr_limit, &mut self.info);

            // Re-calculate liveness one last time
            live = SimpleLiveness::for_function(f);
        } else {
            // GPRs are allocated in multiple of 8. That means we can give RA a
            // bit more freedom by making gprs up until the next multiple
            // available.
            let next_multiple_gprs =
                (total_gprs + hw_reserved_gprs).next_multiple_of(8) - hw_reserved_gprs;
            let free_gprs = next_multiple_gprs.min(max_gprs) - total_gprs;

            total_gprs += free_gprs;
            gpr_limit += free_gprs;
        }

        self.info.num_gprs = total_gprs.try_into().unwrap();

        let limit = PerRegFile::new_with(|file| {
            if file == RegFile::GPR {
                gpr_limit
            } else {
                self.sm.num_regs(file)
            }
        });

        let mut phi_webs = PhiWebs::new(f);

        let mut blocks: Vec<AssignRegsBlock> = Vec::new();
        for b_idx in 0..f.blocks.len() {
            let pred = f.blocks.pred_indices(b_idx);
            let pred_ra = if pred.is_empty() {
                None
            } else {
                // Start with the previous block's.
                Some(&blocks[pred[0]].ra)
            };

            let bl = live.block_live(b_idx);

            let mut arb = AssignRegsBlock::new(&limit, tmp_gprs);
            arb.first_pass(&mut f.blocks[b_idx], bl, pred_ra, &mut phi_webs);

            assert!(blocks.len() == b_idx);
            blocks.push(arb);
        }

        for b_idx in 0..f.blocks.len() {
            let arb = &blocks[b_idx];
            for sb_idx in f.blocks.succ_indices(b_idx).to_vec() {
                arb.second_pass(&blocks[sb_idx], &mut f.blocks[b_idx]);
            }
        }
    }
}
