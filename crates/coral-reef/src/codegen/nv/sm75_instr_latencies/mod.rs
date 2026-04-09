// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright © 2025-2026 ecoPrimals
// Derived from Red Hat. (2025)
use crate::codegen::ir::*;

// This contains the register scheduling information provided by NVIDIA.  This
// file is for Turing only.
//
// Coupled instructions are ones with fixed latencies, they need delays but not
// scoreboards.  Decoupled instructions are ones with variable latencies, need
// scoreboards but not delays.  There are also redirected instructions which
// depending on the SM, can be coupled or decoupled so both delays and
// scoreboards needs to be provided.
//
// `needs_scoreboards` does not treat `RedirectedFP16` as needing scoreboards on Turing; revisit if
// hardware guidance changes.

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
            matches!(
                gpr::RegLatencySM75::op_category(op, false, 0),
                gpr::RegLatencySM75::RedirectedFP64
                    | gpr::RegLatencySM75::RedirectedHMMA_884_F16(_)
                    | gpr::RegLatencySM75::RedirectedHMMA_884_F32(_)
                    | gpr::RegLatencySM75::RedirectedHMMA_1688
                    | gpr::RegLatencySM75::RedirectedHMMA_16816
                    | gpr::RegLatencySM75::IMMA(_)
                    | gpr::RegLatencySM75::Decoupled
            )
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
            _ => crate::codegen::ice!("Not a register"),
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
            _ => crate::codegen::ice!("Not a register"),
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

                uniform::URegLatencySM75::write_after_write(
                    write1_latency,
                    write2_latency,
                    a_op_pred,
                )
            }
            RegFile::Pred => {
                let write1_latency = gpr::RegLatencySM75::op_category(a, false, a_dst_idx);
                let write2_latency = gpr::RegLatencySM75::op_category(b, false, b_dst_idx);

                gpr::RegLatencySM75::pred_write_after_write(
                    write1_latency,
                    write2_latency,
                    a_op_pred,
                )
            }
            RegFile::UPred => {
                let write1_latency = uniform::URegLatencySM75::op_category(a, false, a_dst_idx);
                let write2_latency = uniform::URegLatencySM75::op_category(b, false, b_dst_idx);

                uniform::URegLatencySM75::pred_write_after_write(write1_latency, write2_latency)
            }
            _ => crate::codegen::ice!("Not a register"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::codegen::ir::{
        Dst, FloatType, HmmaSize, ImmaSize, IntCmpOp, IntCmpType, IntType, Op, OpDFma, OpHmma,
        OpIAdd3, OpISetP, OpImma, OpMov, OpS2R, PredSetOp, RegFile, RegRef, Src,
    };

    fn ugpr_dst(idx: u32) -> Dst {
        Dst::Reg(RegRef::new(RegFile::UGPR, idx, 1))
    }

    /// Exercise uniform (UGPR) latency paths in URegLatencySM75
    #[test]
    fn test_uniform_raw_latency() {
        let write = Op::S2R(Box::new(OpS2R {
            dst: ugpr_dst(0),
            idx: 0,
        }));
        let read = Op::IAdd3(Box::new(OpIAdd3 {
            dsts: [ugpr_dst(1), Dst::None, Dst::None],
            srcs: [Src::ZERO, Src::ZERO, Src::ZERO],
        }));
        let lat = SM75Latency::raw(&write, 0, Some(&read), 0);
        assert!(lat > 0);
    }

    #[test]
    fn test_uniform_war_latency() {
        let read = Op::Mov(Box::new(OpMov {
            dst: ugpr_dst(0),
            src: Src::ZERO,
            quad_lanes: 0xf,
        }));
        let write = Op::S2R(Box::new(OpS2R {
            dst: ugpr_dst(1),
            idx: 0,
        }));
        let lat = SM75Latency::war(&read, 0, &write, 0);
        assert!(lat > 0);
    }

    /// Exercise predicate latency paths (gpr::RegLatencySM75 pred_read_after_write etc.)
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
        let read = Op::ISetP(Box::new(OpISetP {
            dst: Dst::Reg(RegRef::new(RegFile::Pred, 1, 1)),
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
        let lat = SM75Latency::raw(&write, 0, Some(&read), 0);
        assert!(lat > 0);
    }

    fn gpr_dst() -> Dst {
        Dst::Reg(RegRef::new(RegFile::GPR, 0, 1))
    }

    #[test]
    fn test_needs_scoreboards_uniform_s2r_is_r2ur_category() {
        let op = Op::S2R(Box::new(OpS2R {
            dst: ugpr_dst(0),
            idx: 0,
        }));
        assert!(op.is_uniform());
        assert!(SM75Latency::needs_scoreboards(&op));
    }

    #[test]
    fn test_needs_scoreboards_uniform_mov_not_scoreboarded() {
        let op = Op::Mov(Box::new(OpMov {
            dst: ugpr_dst(0),
            src: Src::ZERO,
            quad_lanes: 0xf,
        }));
        assert!(op.is_uniform());
        assert!(!SM75Latency::needs_scoreboards(&op));
    }

    #[test]
    fn test_needs_scoreboards_gpr_dfma() {
        let op = Op::DFma(Box::new(OpDFma {
            dst: gpr_dst(),
            srcs: [Src::ZERO, Src::ZERO, Src::ZERO],
            rnd_mode: crate::codegen::ir::FRndMode::NearestEven,
        }));
        assert!(!op.is_uniform());
        assert!(SM75Latency::needs_scoreboards(&op));
    }

    #[test]
    fn test_needs_scoreboards_gpr_mov() {
        let op = Op::Mov(Box::new(OpMov {
            dst: gpr_dst(),
            src: Src::ZERO,
            quad_lanes: 0xf,
        }));
        assert!(!SM75Latency::needs_scoreboards(&op));
    }

    #[test]
    fn test_needs_scoreboards_hmma_redirected() {
        let op = Op::Hmma(Box::new(OpHmma {
            dst: gpr_dst(),
            mat_size: HmmaSize::M16N8K8,
            src_type: FloatType::F16,
            dst_type: FloatType::F32,
            srcs: [Src::ZERO, Src::ZERO, Src::ZERO],
        }));
        assert!(SM75Latency::needs_scoreboards(&op));
    }

    #[test]
    fn test_needs_scoreboards_imma() {
        let op = Op::Imma(Box::new(OpImma {
            dst: gpr_dst(),
            mat_size: ImmaSize::M16N8K16,
            src_types: [IntType::I8, IntType::I8],
            saturate: false,
            srcs: [Src::ZERO, Src::ZERO, Src::ZERO],
        }));
        assert!(SM75Latency::needs_scoreboards(&op));
    }

    #[test]
    fn test_uniform_raw_none_worst_case_uses_uldc_reader() {
        let write = Op::S2R(Box::new(OpS2R {
            dst: ugpr_dst(0),
            idx: 0,
        }));
        let lat = SM75Latency::raw(&write, 0, None, 0);
        assert!(lat >= 1);
    }

    #[test]
    fn test_uniform_waw_latency() {
        let a = Op::S2R(Box::new(OpS2R {
            dst: ugpr_dst(0),
            idx: 0,
        }));
        let b = Op::Mov(Box::new(OpMov {
            dst: ugpr_dst(1),
            src: Src::ZERO,
            quad_lanes: 0xf,
        }));
        let lat = SM75Latency::waw(&a, 0, &b, 0, false);
        assert!(lat >= 1);
    }

    #[test]
    fn test_pred_war_latency() {
        let read = Op::ISetP(Box::new(OpISetP {
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
        let write = Op::ISetP(Box::new(OpISetP {
            dst: Dst::Reg(RegRef::new(RegFile::Pred, 1, 1)),
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
        let lat = SM75Latency::war(&read, 0, &write, 0);
        assert!(lat >= 1);
    }
}
