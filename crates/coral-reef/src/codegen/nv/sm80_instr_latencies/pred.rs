// SPDX-License-Identifier: AGPL-3.0-only
// Copyright © 2025-2026 ecoPrimals
// Derived from Red Hat. (2025)
#![expect(
    non_camel_case_types,
    reason = "latency model mirrors hardware naming from Red Hat spec"
)]

use super::super::sm75_instr_latencies::pred;
use crate::codegen::ir::*;

#[derive(Clone, Copy, Debug)]
pub(super) enum PredLatencySM80 {
    Disp_Alu,
    Coupled,
    FMA,
    FP16,
    HFMA2_MMA,
    RedirectedFP64,
    Decoupled,
    Guard,
}

impl PredLatencySM80 {
    pub(super) fn op_category(op: &Op) -> Self {
        match op {
            Op::Atom(_) => Self::Decoupled,
            Op::Bra(_) => Self::Decoupled,
            Op::DSetP(_) => Self::RedirectedFP64,
            Op::FMnMx(_) | Op::FSetP(_) => Self::Coupled,
            Op::HFma2(_) => Self::FP16,
            Op::HMnMx2(_) => Self::FP16,
            Op::HSetP2(_) => Self::FP16,
            Op::IAdd3(_) => Self::Coupled,
            Op::IAdd3X(_) => Self::Coupled,
            Op::IMad(_) => Self::FMA,
            Op::IMad64(_) => Self::FMA,
            Op::IMnMx(_) => Self::Coupled,
            Op::IMul(_) => Self::FMA,
            Op::Ipa(_) => Self::Decoupled,
            Op::ISetP(_) => Self::Coupled,

            Op::Ld(_) => Self::Decoupled,

            Op::Lea(_) | Op::LeaX(_) => Self::Coupled,
            Op::PixLd(_) => Self::Decoupled,
            Op::PLop3(_) => Self::Coupled,
            Op::PSetP(_) => Self::Coupled,
            Op::R2UR(_) => Self::Decoupled,
            Op::Sel(_) => Self::Coupled,
            Op::Shfl(_) => Self::Decoupled,
            Op::SuLd(_) => Self::Decoupled,
            Op::SuSt(_) => Self::Decoupled,
            Op::Tex(_) => Self::Decoupled,
            Op::Tld(_) => Self::Decoupled,
            Op::Tld4(_) => Self::Decoupled,
            Op::Tmml(_) => Self::Decoupled,
            Op::Txd(_) => Self::Decoupled,
            Op::Txq(_) => Self::Decoupled,

            Op::Vote(_) => Self::Disp_Alu,
            Op::Match(_) => Self::Decoupled,
            _ => {
                crate::codegen::ice!("Illegal op in sm80 pred latency {op}");
            }
        }
    }

    pub(super) fn pred_read_after_write(writer: Self, reader: Self) -> u32 {
        match reader {
            Self::Disp_Alu => match writer {
                Self::Disp_Alu | Self::Coupled | Self::FMA | Self::FP16 => 13,
                Self::HFMA2_MMA | Self::RedirectedFP64 => 14,
                Self::Decoupled => 1,
                Self::Guard => {
                    crate::codegen::ice!("Illegal writer in sm80 praw");
                }
            },
            Self::Coupled => match writer {
                Self::Disp_Alu | Self::Coupled => 4,
                Self::FMA | Self::FP16 => 5,
                Self::HFMA2_MMA | Self::RedirectedFP64 => 6,
                Self::Decoupled => 1,
                Self::Guard => {
                    crate::codegen::ice!("Illegal writer in sm80 praw");
                }
            },
            Self::FMA => match writer {
                Self::Disp_Alu | Self::Coupled => 5,
                Self::FMA => 4,
                Self::FP16 => 5,
                Self::HFMA2_MMA | Self::RedirectedFP64 => 6,
                Self::Decoupled => 1,
                Self::Guard => {
                    crate::codegen::ice!("Illegal writer in sm80 praw");
                }
            },
            Self::HFMA2_MMA | Self::RedirectedFP64 => match writer {
                Self::Disp_Alu | Self::Coupled | Self::FMA | Self::FP16 => 13,
                Self::HFMA2_MMA | Self::RedirectedFP64 => 6,
                Self::Decoupled => 1,
                Self::Guard => {
                    crate::codegen::ice!("Illegal writer in sm80 praw");
                }
            },
            Self::FP16 => match writer {
                Self::Disp_Alu | Self::Coupled => 5,
                Self::FMA => 4,
                Self::FP16 => 5,
                Self::HFMA2_MMA | Self::RedirectedFP64 => 6,
                Self::Decoupled => 1,
                Self::Guard => {
                    crate::codegen::ice!("Illegal writer in sm80 praw");
                }
            },
            Self::Decoupled | Self::Guard => match writer {
                Self::Disp_Alu | Self::Coupled | Self::FMA | Self::FP16 => 13,
                Self::HFMA2_MMA | Self::RedirectedFP64 => 14,
                Self::Decoupled => 1,
                Self::Guard => {
                    crate::codegen::ice!("Illegal writer in sm80 praw");
                }
            },
        }
    }

    pub(super) fn pred_write_after_write(writer1: Self, writer2: Self, has_pred: bool) -> u32 {
        match writer2 {
            Self::Disp_Alu | Self::Coupled | Self::FMA => match writer1 {
                Self::Disp_Alu | Self::Coupled | Self::FMA | Self::FP16 => 1,
                Self::HFMA2_MMA | Self::RedirectedFP64 => 2,
                Self::Decoupled => 1,
                Self::Guard => {
                    crate::codegen::ice!("Illegal writer in sm80 pwaw");
                }
            },
            Self::FP16 => match writer1 {
                Self::Disp_Alu | Self::Coupled | Self::FMA => pred(has_pred, 2, 7),
                Self::FP16 => 1,
                Self::HFMA2_MMA | Self::RedirectedFP64 => pred(has_pred, 2, 8),
                Self::Decoupled => 1,
                Self::Guard => {
                    crate::codegen::ice!("Illegal writer in sm80 pwaw");
                }
            },
            Self::HFMA2_MMA | Self::RedirectedFP64 => match writer1 {
                Self::Disp_Alu | Self::Coupled | Self::FMA | Self::FP16 => pred(has_pred, 2, 5),
                Self::HFMA2_MMA | Self::RedirectedFP64 | Self::Decoupled => 1,
                Self::Guard => {
                    crate::codegen::ice!("Illegal writer in sm80 pwaw");
                }
            },
            Self::Decoupled => match writer1 {
                Self::Disp_Alu | Self::Coupled | Self::FMA => pred(has_pred, 2, 10),
                Self::FP16 => pred(has_pred, 1, 11),
                Self::HFMA2_MMA | Self::RedirectedFP64 => pred(has_pred, 1, 12),
                Self::Decoupled => 1,
                Self::Guard => {
                    crate::codegen::ice!("Illegal writer in sm80 pwaw");
                }
            },
            Self::Guard => {
                crate::codegen::ice!("Illegal writer in sm80 pwaw");
            }
        }
    }

    pub(super) fn pred_write_after_read(_reader: Self, _writer: Self) -> u32 {
        1
    }
}

#[cfg(test)]
mod tests {
    use super::PredLatencySM80;
    use crate::codegen::ir::{
        Dst, FloatCmpOp, IntCmpOp, IntCmpType, LabelAllocator, MemAccess, MemAddrType,
        MemEvictionPriority, MemOrder, MemScope, MemSpace, MemType, OffsetStride, Op, OpBra,
        OpDSetP, OpHFma2, OpIAdd3, OpIMad, OpISetP, OpLd, PredSetOp, RegFile, RegRef, Src, VoteOp,
    };

    fn gpr() -> Dst {
        Dst::Reg(RegRef::new(RegFile::GPR, 0, 1))
    }

    #[test]
    fn op_category_representative_ops() {
        let mut la = LabelAllocator::new();
        assert!(matches!(
            PredLatencySM80::op_category(&Op::Bra(Box::new(OpBra {
                target: la.alloc(),
                cond: Src::new_imm_bool(true),
            }))),
            PredLatencySM80::Decoupled
        ));

        assert!(matches!(
            PredLatencySM80::op_category(&Op::DSetP(Box::new(OpDSetP {
                dst: Dst::Reg(RegRef::new(RegFile::Pred, 0, 1)),
                set_op: PredSetOp::And,
                cmp_op: FloatCmpOp::OrdEq,
                srcs: [Src::ZERO, Src::ZERO, Src::new_imm_bool(false)],
            }))),
            PredLatencySM80::RedirectedFP64
        ));

        assert!(matches!(
            PredLatencySM80::op_category(&Op::HFma2(Box::new(OpHFma2 {
                dst: gpr(),
                srcs: [Src::ZERO, Src::ZERO, Src::ZERO],
                saturate: false,
                ftz: false,
                dnz: false,
                f32: false,
            }))),
            PredLatencySM80::FP16
        ));

        assert!(matches!(
            PredLatencySM80::op_category(&Op::IAdd3(Box::new(OpIAdd3 {
                dsts: [gpr(), Dst::None, Dst::None],
                srcs: [Src::ZERO, Src::ZERO, Src::ZERO],
            }))),
            PredLatencySM80::Coupled
        ));

        assert!(matches!(
            PredLatencySM80::op_category(&Op::IMad(Box::new(OpIMad {
                dst: gpr(),
                srcs: [Src::ZERO, Src::ZERO, Src::ZERO],
                signed: false,
            }))),
            PredLatencySM80::FMA
        ));

        assert!(matches!(
            PredLatencySM80::op_category(&Op::Ld(Box::new(OpLd {
                dst: gpr(),
                addr: Src::ZERO,
                offset: 0,
                stride: OffsetStride::X1,
                access: MemAccess {
                    mem_type: MemType::B32,
                    space: MemSpace::Global(MemAddrType::A32),
                    order: MemOrder::Strong(MemScope::CTA),
                    eviction_priority: MemEvictionPriority::Normal,
                },
            }))),
            PredLatencySM80::Decoupled
        ));

        assert!(matches!(
            PredLatencySM80::op_category(&Op::Vote(Box::new(crate::codegen::ir::OpVote {
                op: VoteOp::All,
                dsts: [gpr(), Dst::None],
                pred: Src::new_imm_bool(true),
            }))),
            PredLatencySM80::Disp_Alu
        ));

        assert!(matches!(
            PredLatencySM80::op_category(&Op::ISetP(Box::new(OpISetP {
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
            }))),
            PredLatencySM80::Coupled
        ));
    }

    #[test]
    fn pred_read_after_write_all_valid_pairs() {
        use PredLatencySM80::*;
        let writers = [
            Disp_Alu,
            Coupled,
            FMA,
            FP16,
            HFMA2_MMA,
            RedirectedFP64,
            Decoupled,
        ];
        let readers = [
            Disp_Alu,
            Coupled,
            FMA,
            FP16,
            HFMA2_MMA,
            RedirectedFP64,
            Decoupled,
            Guard,
        ];
        for w in writers {
            for r in readers {
                let lat = PredLatencySM80::pred_read_after_write(w, r);
                assert!(lat >= 1, "praw {w:?} -> {r:?} = {lat}");
            }
        }
    }

    #[test]
    fn pred_write_after_write_all_valid_pairs() {
        use PredLatencySM80::*;
        let writers = [
            Disp_Alu,
            Coupled,
            FMA,
            FP16,
            HFMA2_MMA,
            RedirectedFP64,
            Decoupled,
        ];
        for w1 in writers {
            for w2 in writers {
                for has_pred in [false, true] {
                    let lat = PredLatencySM80::pred_write_after_write(w1, w2, has_pred);
                    assert!(lat >= 1, "pwaw {w1:?} {w2:?} p={has_pred} = {lat}");
                }
            }
        }
    }

    #[test]
    fn pred_write_after_read_is_constant() {
        use PredLatencySM80::*;
        assert_eq!(
            PredLatencySM80::pred_write_after_read(Coupled, Decoupled),
            1
        );
        assert_eq!(PredLatencySM80::pred_write_after_read(Disp_Alu, Coupled), 1);
    }
}
