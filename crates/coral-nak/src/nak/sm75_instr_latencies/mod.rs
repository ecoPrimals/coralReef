// Copyright © 2025 Red Hat.
// SPDX-License-Identifier: MIT
#![allow(non_camel_case_types, clippy::wildcard_imports, clippy::enum_glob_use)]

use super::ir::*;

// This contains the register scheduling information provided by NVIDIA.  This
// file is for Turing only.
//
// Coupled instructions are ones with fixed latencies, they need delays but not
// scoreboards.  Decoupled instructions are ones with variable latencies, need
// scoreboards but not delays.  There are also redirected instructions which
// depending on the SM, can be coupled or decoupled so both delays and
// scoreboards needs to be provided.

mod gpr;
mod uniform;

pub const fn pred(has_pred: bool, a: u32, b: u32) -> u32 {
    if has_pred { a + b } else { b }
}

pub struct SM75Latency {}

impl SM75Latency {
    pub fn needs_scoreboards(op: &Op) -> bool {
        if op.is_uniform() {
            matches!(
                uniform::URegLatencySM75::op_category(op, false, 0),
                uniform::URegLatencySM75::R2UR
            )
        } else {
            match gpr::RegLatencySM75::op_category(op, false, 0) {
                gpr::RegLatencySM75::RedirectedFP64 |
                // We don't think fp16 needs scoreboarding on any known hw
                // Put this back if we figure out it does.
                //RegLatencySM75::RedirectedFP16 |
                gpr::RegLatencySM75::RedirectedHMMA_884_F16(_) |
                gpr::RegLatencySM75::RedirectedHMMA_884_F32(_) |
                gpr::RegLatencySM75::RedirectedHMMA_1688 |
                gpr::RegLatencySM75::RedirectedHMMA_16816 |
                gpr::RegLatencySM75::IMMA(_) |
                gpr::RegLatencySM75::Decoupled => true,
                _ => false
            }
        }
    }

    /// if read is None pick the worst case raw latency
    pub fn raw(write: &Op, dst_idx: usize, read: Option<&Op>, src_idx: usize) -> u32 {
        let Some(dst_file) = write.dsts_as_slice()[dst_idx].file() else {
            return 0;
        };

        match dst_file {
            RegFile::GPR => {
                let write_latency = gpr::RegLatencySM75::op_category(write, false, dst_idx);
                let read_latency = match read {
                    Some(op) => gpr::RegLatencySM75::op_category(op, true, src_idx),
                    None => gpr::RegLatencySM75::RedirectedFP64,
                };

                gpr::RegLatencySM75::read_after_write(write_latency, read_latency)
            }
            RegFile::UGPR => {
                let write_latency = uniform::URegLatencySM75::op_category(write, false, dst_idx);
                let read_latency = match read {
                    Some(op) => uniform::URegLatencySM75::op_category(op, true, src_idx),
                    None => uniform::URegLatencySM75::Uldc,
                };

                uniform::URegLatencySM75::read_after_write(write_latency, read_latency)
            }
            RegFile::Pred => {
                let write_latency = gpr::RegLatencySM75::op_category(write, false, dst_idx);
                let read_latency = match read {
                    Some(op) => gpr::RegLatencySM75::op_category(op, true, src_idx),
                    None => gpr::RegLatencySM75::GuardPredicate,
                };

                gpr::RegLatencySM75::pred_read_after_write(write_latency, read_latency)
            }
            RegFile::UPred => {
                let write_latency = uniform::URegLatencySM75::op_category(write, false, dst_idx);
                let read_latency = match read {
                    Some(op) => uniform::URegLatencySM75::op_category(op, true, src_idx),
                    None => uniform::URegLatencySM75::GuardPredicate,
                };

                uniform::URegLatencySM75::pred_read_after_write(write_latency, read_latency)
            }
            RegFile::Bar => 0, // Barriers have a HW scoreboard
            _ => panic!("Not a register"),
        }
    }

    pub fn war(read: &Op, src_idx: usize, write: &Op, dst_idx: usize) -> u32 {
        let Some(dst_file) = write.dsts_as_slice()[dst_idx].file() else {
            return 0;
        };

        match dst_file {
            RegFile::GPR => {
                let write_latency = gpr::RegLatencySM75::op_category(write, false, dst_idx);
                let read_latency = gpr::RegLatencySM75::op_category(read, true, src_idx);

                gpr::RegLatencySM75::write_after_read(read_latency, write_latency)
            }
            RegFile::UGPR => {
                let write_latency = uniform::URegLatencySM75::op_category(write, false, dst_idx);
                let read_latency = uniform::URegLatencySM75::op_category(read, true, src_idx);

                uniform::URegLatencySM75::write_after_read(read_latency, write_latency)
            }
            RegFile::Pred => {
                let write_latency = gpr::RegLatencySM75::op_category(write, false, dst_idx);
                let read_latency = gpr::RegLatencySM75::op_category(read, true, src_idx);

                gpr::RegLatencySM75::pred_write_after_read(read_latency, write_latency)
            }
            RegFile::UPred => {
                let write_latency = uniform::URegLatencySM75::op_category(write, false, dst_idx);
                let read_latency = uniform::URegLatencySM75::op_category(read, true, src_idx);

                uniform::URegLatencySM75::pred_write_after_read(read_latency, write_latency)
            }
            _ => panic!("Not a register"),
        }
    }

    pub fn waw(a: &Op, a_dst_idx: usize, b: &Op, b_dst_idx: usize, a_op_pred: bool) -> u32 {
        let Some(dst_file) = a.dsts_as_slice()[a_dst_idx].file() else {
            return 0;
        };

        match dst_file {
            RegFile::GPR => {
                let write1_latency = gpr::RegLatencySM75::op_category(a, false, a_dst_idx);
                let write2_latency = gpr::RegLatencySM75::op_category(b, false, b_dst_idx);

                gpr::RegLatencySM75::write_after_write(write1_latency, write2_latency, a_op_pred)
            }
            RegFile::UGPR => {
                let write1_latency = uniform::URegLatencySM75::op_category(a, false, a_dst_idx);
                let write2_latency = uniform::URegLatencySM75::op_category(b, false, b_dst_idx);

                uniform::URegLatencySM75::write_after_write(write1_latency, write2_latency, a_op_pred)
            }
            RegFile::Pred => {
                let write1_latency = gpr::RegLatencySM75::op_category(a, false, a_dst_idx);
                let write2_latency = gpr::RegLatencySM75::op_category(b, false, b_dst_idx);

                gpr::RegLatencySM75::pred_write_after_write(write1_latency, write2_latency, a_op_pred)
            }
            RegFile::UPred => {
                let write1_latency = uniform::URegLatencySM75::op_category(a, false, a_dst_idx);
                let write2_latency = uniform::URegLatencySM75::op_category(b, false, b_dst_idx);

                uniform::URegLatencySM75::pred_write_after_write(write1_latency, write2_latency)
            }
            _ => panic!("Not a register"),
        }
    }
}
