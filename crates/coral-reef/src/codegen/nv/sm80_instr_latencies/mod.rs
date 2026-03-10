// SPDX-License-Identifier: AGPL-3.0-only
// Copyright © 2025-2026 ecoPrimals
// Derived from Red Hat. (2025)
#![allow(non_camel_case_types, clippy::wildcard_imports, clippy::enum_glob_use)]

use crate::codegen::ir::*;

// This contains the register scheduling information provided by NVIDIA.  This
// file is for Ampere and Ada only.
//
// Coupled instructions are ones with fixed latencies, they need delays but not
// scoreboards.  Decoupled instructions are ones with variable latencies, need
// scoreboards but not delays.  There are also redirected instructions which
// depending on the SM, can be coupled or decoupled so both delays and
// scoreboards needs to be provided.

mod gpr;
mod pred;
mod uniform;

pub struct SM80Latency {}

impl SM80Latency {
    pub fn needs_scoreboards(op: &Op) -> bool {
        if op.is_uniform() {
            matches!(
                uniform::URegLatencySM80::op_category(op, false, 0),
                uniform::URegLatencySM80::ToUr
            )
        } else {
            matches!(
                gpr::RegLatencySM80::op_category(op, false, 0),
                gpr::RegLatencySM80::RedirectedFP64
                    | gpr::RegLatencySM80::Clmad
                    | gpr::RegLatencySM80::IMMA_88
                    | gpr::RegLatencySM80::MMA_1x_collect
                    | gpr::RegLatencySM80::MMA_2x_collect
                    | gpr::RegLatencySM80::DMMA
                    | gpr::RegLatencySM80::Cbu
                    | gpr::RegLatencySM80::Decoupled
                    | gpr::RegLatencySM80::DecoupledAgu
            )
        }
    }

    pub fn raw(write: &Op, dst_idx: usize, read: Option<&Op>, src_idx: usize) -> u32 {
        let Some(dst_file) = write.dsts_as_slice()[dst_idx].file() else {
            return 0;
        };

        match dst_file {
            RegFile::GPR => {
                let write_latency = gpr::RegLatencySM80::op_category(write, false, dst_idx);
                let read_latency = match read {
                    Some(op) => gpr::RegLatencySM80::op_category(op, true, src_idx),
                    None => gpr::RegLatencySM80::RedirectedFP64,
                };

                gpr::RegLatencySM80::read_after_write(write_latency, read_latency)
            }
            RegFile::UGPR => {
                let write_latency = uniform::URegLatencySM80::op_category(write, false, dst_idx);
                let read_latency = match read {
                    Some(op) => uniform::URegLatencySM80::op_category(op, true, src_idx),
                    None => uniform::URegLatencySM80::Uldc,
                };

                uniform::URegLatencySM80::read_after_write(write_latency, read_latency)
            }
            RegFile::Pred => {
                let write_latency = pred::PredLatencySM80::op_category(write);
                let read_latency = match read {
                    Some(op) => pred::PredLatencySM80::op_category(op),
                    None => pred::PredLatencySM80::RedirectedFP64,
                };

                pred::PredLatencySM80::pred_read_after_write(write_latency, read_latency)
            }
            RegFile::UPred => {
                let write_latency = uniform::UPredLatencySM80::op_category(write);
                let read_latency = match read {
                    Some(op) => uniform::UPredLatencySM80::op_category(op),
                    None => uniform::UPredLatencySM80::UGuard,
                };

                uniform::UPredLatencySM80::pred_read_after_write(write_latency, read_latency)
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
                let write_latency = gpr::RegLatencySM80::op_category(write, false, dst_idx);
                let read_latency = gpr::RegLatencySM80::op_category(read, true, src_idx);

                gpr::RegLatencySM80::write_after_read(read_latency, write_latency)
            }
            RegFile::UGPR => {
                let write_latency = uniform::URegLatencySM80::op_category(write, false, dst_idx);
                let read_latency = uniform::URegLatencySM80::op_category(read, true, src_idx);

                uniform::URegLatencySM80::write_after_read(read_latency, write_latency)
            }
            RegFile::Pred => {
                let write_latency = pred::PredLatencySM80::op_category(write);
                let read_latency = pred::PredLatencySM80::op_category(read);

                pred::PredLatencySM80::pred_write_after_read(read_latency, write_latency)
            }
            RegFile::UPred => {
                let write_latency = uniform::UPredLatencySM80::op_category(write);
                let read_latency = uniform::UPredLatencySM80::op_category(read);

                uniform::UPredLatencySM80::pred_write_after_read(read_latency, write_latency)
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
                let write1_latency = gpr::RegLatencySM80::op_category(a, false, a_dst_idx);
                let write2_latency = gpr::RegLatencySM80::op_category(b, false, b_dst_idx);

                gpr::RegLatencySM80::write_after_write(write1_latency, write2_latency, a_op_pred)
            }
            RegFile::UGPR => {
                let write1_latency = uniform::URegLatencySM80::op_category(a, false, a_dst_idx);
                let write2_latency = uniform::URegLatencySM80::op_category(b, false, b_dst_idx);

                uniform::URegLatencySM80::write_after_write(
                    write1_latency,
                    write2_latency,
                    a_op_pred,
                )
            }
            RegFile::Pred => {
                let write1_latency = pred::PredLatencySM80::op_category(a);
                let write2_latency = pred::PredLatencySM80::op_category(b);

                pred::PredLatencySM80::pred_write_after_write(
                    write1_latency,
                    write2_latency,
                    a_op_pred,
                )
            }
            RegFile::UPred => {
                let write1_latency = uniform::UPredLatencySM80::op_category(a);
                let write2_latency = uniform::UPredLatencySM80::op_category(b);

                uniform::UPredLatencySM80::pred_write_after_write(write1_latency, write2_latency)
            }
            _ => panic!("Not a register"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::codegen::ir::{
        Dst, FloatCmpOp, IntCmpOp, IntCmpType, Op, OpDSetP, OpIAdd3, OpISetP, OpMov, OpS2R,
        PredSetOp, RegFile, RegRef, Src,
    };

    fn ugpr_dst(idx: u32) -> Dst {
        Dst::Reg(RegRef::new(RegFile::UGPR, idx, 1))
    }

    /// Exercise pred::PredLatencySM80 paths
    #[test]
    fn test_pred_raw_latency() {
        let write = Op::ISetP(Box::new(OpISetP {
            dst: Dst::Reg(RegRef::new(RegFile::Pred, 0, 1)),
            set_op: PredSetOp::And,
            cmp_op: IntCmpOp::Eq,
            cmp_type: IntCmpType::U32,
            ex: false,
            srcs: [
                Src::ZERO,
                Src::ZERO,
                Src::new_imm_bool(false),
                Src::new_imm_bool(false),
            ],
        }));
        let read = Op::DSetP(Box::new(OpDSetP {
            dst: Dst::Reg(RegRef::new(RegFile::Pred, 1, 1)),
            set_op: PredSetOp::And,
            cmp_op: FloatCmpOp::OrdEq,
            srcs: [Src::ZERO, Src::ZERO, Src::new_imm_bool(false)],
        }));
        let lat = SM80Latency::raw(&write, 0, Some(&read), 0);
        assert!(lat > 0);
    }

    #[test]
    fn test_pred_waw_latency() {
        let a = Op::ISetP(Box::new(OpISetP {
            dst: Dst::Reg(RegRef::new(RegFile::Pred, 0, 1)),
            set_op: PredSetOp::And,
            cmp_op: IntCmpOp::Eq,
            cmp_type: IntCmpType::U32,
            ex: false,
            srcs: [
                Src::ZERO,
                Src::ZERO,
                Src::new_imm_bool(false),
                Src::new_imm_bool(false),
            ],
        }));
        let b = Op::IAdd3(Box::new(OpIAdd3 {
            dsts: [
                Dst::Reg(RegRef::new(RegFile::GPR, 0, 1)),
                Dst::Reg(RegRef::new(RegFile::Pred, 1, 1)),
                Dst::None,
            ],
            srcs: [Src::ZERO, Src::ZERO, Src::ZERO],
        }));
        let lat = SM80Latency::waw(&a, 0, &b, 1, false);
        assert!(lat > 0);
    }

    /// Exercise uniform::URegLatencySM80 and uniform::UPredLatencySM80 paths
    #[test]
    fn test_uniform_raw_latency() {
        let write = Op::S2R(Box::new(OpS2R {
            dst: ugpr_dst(0),
            idx: 0,
        }));
        let read = Op::Mov(Box::new(OpMov {
            dst: ugpr_dst(1),
            src: Src::ZERO,
            quad_lanes: 0xf,
        }));
        let lat = SM80Latency::raw(&write, 0, Some(&read), 0);
        assert!(lat > 0);
    }

    #[test]
    fn test_uniform_war_latency() {
        let read = Op::IAdd3(Box::new(OpIAdd3 {
            dsts: [ugpr_dst(0), Dst::None, Dst::None],
            srcs: [Src::ZERO, Src::ZERO, Src::ZERO],
        }));
        let write = Op::S2R(Box::new(OpS2R {
            dst: ugpr_dst(1),
            idx: 0,
        }));
        let lat = SM80Latency::war(&read, 0, &write, 0);
        assert!(lat > 0);
    }
}
