// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright © 2025-2026 ecoPrimals
// Derived from Red Hat. (2025)
#![expect(
    non_camel_case_types,
    reason = "latency model mirrors hardware naming from Red Hat spec"
)]

use super::super::sm75_instr_latencies::pred;
use crate::codegen::ir::*;

#[derive(Debug)]
pub(super) enum URegLatencySM80 {
    Coupled,
    Decoupled,
    Cbu,
    CoupledBindless,
    DecoupledBindless,
    ToUr,
    Rpcmov_64,
    Udp,
    Uldc,
    Umov,
    VoteU,
}

#[derive(Debug)]
pub(super) enum UPredLatencySM80 {
    Coupled,
    Udp,
    VoteU,
    UGuard,
    Bra_Jmp,
    Uldc_Mma,
}

impl URegLatencySM80 {
    pub(super) fn op_category(op: &Op, reader: bool, op_reg_idx: usize) -> Self {
        use URegLatencySM80::*;
        // is this using a bindless cbuf as a src register.
        // this decides between the category types for readers.
        let bindless = reader && op.srcs_as_slice()[op_reg_idx].is_bindless_cbuf();

        let vcoupled = if bindless { CoupledBindless } else { Coupled };
        let vdecoupled = if bindless {
            DecoupledBindless
        } else {
            Decoupled
        };

        // if this is a reader from a ureg, it could be a U* instruction or a regular instruction.
        let uniform_op = op.is_uniform();

        let vcoupled = if uniform_op { Udp } else { vcoupled };
        let vdecoupled = if uniform_op { Udp } else { vdecoupled };

        // Uniform-only ops from tables: mov32i→Uldc, p2ur→Udp; UR2UP when modeled.
        match op {
            Op::BMsk(_) => vcoupled,
            Op::BRev(_) => vcoupled,
            // uclea?
            Op::Flo(_) => vdecoupled,
            Op::IAdd3(_) | Op::IAdd3X(_) => vcoupled,
            Op::IAbs(_) => vcoupled,
            Op::IDp4(_) => vcoupled,
            Op::IMnMx(_) => vcoupled,
            Op::IMad(_) => vcoupled,

            Op::IMad64(_) => vcoupled,
            Op::ISetP(_) => vcoupled,
            Op::Ldc(_) => {
                if uniform_op {
                    Uldc
                } else {
                    vdecoupled
                }
            }
            Op::Lea(_) => vcoupled,
            Op::LeaX(_) => vcoupled,
            Op::Lop2(_) | Op::Lop3(_) => vcoupled,

            Op::Transcendental(_) => vdecoupled,
            Op::Mov(_) => {
                if uniform_op {
                    Umov
                } else {
                    vcoupled
                }
            }

            Op::PLop3(_) => vcoupled,
            Op::PopC(_) => vdecoupled,
            Op::Prmt(_) => vcoupled,
            Op::PSetP(_) => vcoupled,
            Op::Sel(_) => vcoupled,
            Op::Sgxt(_) => vcoupled,
            Op::Shf(_) => vcoupled,
            Op::Shfl(_) => vdecoupled,

            Op::I2F(_) => vdecoupled,
            Op::F2I(_) => vdecoupled,
            Op::F2F(_) => vdecoupled,
            Op::R2UR(_) | Op::Redux(_) => {
                if !reader {
                    ToUr
                } else {
                    crate::codegen::ice!("Illegal R2UR in ureg");
                }
            }
            Op::S2R(_) => {
                if !reader {
                    ToUr
                } else {
                    crate::codegen::ice!("Illegal S2UR in ureg");
                }
            }
            Op::Vote(_) => VoteU,

            Op::FRnd(_) => vdecoupled,
            Op::F2FP(_)
            | Op::FAdd(_)
            | Op::FMul(_)
            | Op::FFma(_)
            | Op::FSet(_)
            | Op::FSetP(_)
            | Op::FMnMx(_)
            | Op::HAdd2(_)
            | Op::HMul2(_)
            | Op::HSet2(_)
            | Op::HFma2(_)
            | Op::HMnMx2(_)
            | Op::HSetP2(_) => vcoupled,
            Op::DMul(_) | Op::DFma(_) | Op::DAdd(_) | Op::DSetP(_) => vdecoupled,
            _ => {
                crate::codegen::ice!("Illegal instruction in ureg category {op}");
            }
        }
    }

    pub(super) fn read_after_write(writer: Self, reader: Self) -> u32 {
        use URegLatencySM80::*;
        match reader {
            Coupled => match writer {
                ToUr => 1,
                Udp => 6,
                Uldc => 2,
                Umov => 2,
                VoteU => 2,
                _ => {
                    crate::codegen::ice!("Illegal writer in raw ureg latency {writer:?}")
                }
            },
            Decoupled => match writer {
                ToUr => 1,
                Udp => 9,
                Uldc => 2,
                Umov => 2,
                VoteU => 2,
                _ => {
                    crate::codegen::ice!("Illegal writer in raw ureg latency {writer:?}")
                }
            },
            Cbu => match writer {
                ToUr => 1,
                Udp => 10,
                Uldc => 3,
                Umov => 3,
                VoteU => 3,
                _ => {
                    crate::codegen::ice!("Illegal writer in raw ureg latency {writer:?}")
                }
            },
            CoupledBindless | DecoupledBindless | Uldc => match writer {
                ToUr => 1,
                Udp => 12,
                Uldc => 5,
                Umov => 5,
                VoteU => 5,
                _ => {
                    crate::codegen::ice!("Illegal writer in raw ureg latency {writer:?}")
                }
            },
            Udp => match writer {
                ToUr => 1,
                Udp => 4,
                Uldc => 2,
                Umov => 2,
                VoteU => 2,
                _ => {
                    crate::codegen::ice!("Illegal writer in raw ureg latency {writer:?}")
                }
            },
            Umov | VoteU => match writer {
                ToUr => 1,
                Udp => 7,
                Uldc => 2,
                Umov => 2,
                VoteU => 2,
                _ => {
                    crate::codegen::ice!("Illegal writer in raw ureg latency {writer:?}")
                }
            },
            _ => {
                crate::codegen::ice!("Illegal read in ureg raw latency")
            }
        }
    }

    pub(super) fn write_after_write(writer1: Self, writer2: Self, has_pred: bool) -> u32 {
        use URegLatencySM80::*;
        match writer2 {
            ToUr => match writer1 {
                ToUr => 1,
                Udp => pred(has_pred, 4, 7),
                Uldc | Umov | VoteU => 4,
                _ => {
                    crate::codegen::ice!("Illegal writer in ureg waw latency")
                }
            },
            Udp => match writer1 {
                ToUr | Udp | Uldc | Umov | VoteU => 1,
                _ => {
                    crate::codegen::ice!("Illegal writer in ureg waw latency")
                }
            },
            Uldc | Umov | VoteU => match writer1 {
                ToUr => 1,
                Udp => 7,
                Uldc | Umov | VoteU => 1,
                _ => {
                    crate::codegen::ice!("Illegal writer in ureg waw latency")
                }
            },
            _ => {
                crate::codegen::ice!("Illegal writer in ureg waw latency")
            }
        }
    }

    pub(super) fn write_after_read(reader: Self, writer: Self) -> u32 {
        use URegLatencySM80::*;
        match writer {
            ToUr | Udp => match reader {
                Coupled | Decoupled | Cbu | CoupledBindless | DecoupledBindless | Rpcmov_64
                | Udp | Uldc | Umov => 1,
                _ => {
                    crate::codegen::ice!("Illegal reader in ureg war latency")
                }
            },
            Uldc | Umov | VoteU => match reader {
                Coupled | Decoupled | Cbu | CoupledBindless | DecoupledBindless | Rpcmov_64
                | Uldc | Umov => 1,
                Udp => 3,
                _ => {
                    crate::codegen::ice!("Illegal reader in ureg war latency")
                }
            },
            _ => {
                crate::codegen::ice!("Illegal writer in ureg war latency")
            }
        }
    }
}

impl UPredLatencySM80 {
    pub(super) fn op_category(op: &Op) -> Self {
        use UPredLatencySM80::*;
        let uniform_op = op.is_uniform();
        match op {
            Op::BMsk(_)
            | Op::BRev(_)
            | Op::Flo(_)
            | Op::IAdd3(_)
            | Op::IAdd3X(_)
            | Op::IMad(_)
            | Op::ISetP(_)
            | Op::Lea(_)
            | Op::LeaX(_)
            | Op::Lop3(_)
            | Op::Mov(_) => Udp,
            Op::Bra(_) => Bra_Jmp,
            Op::Ldc(_) => Uldc_Mma,
            Op::PLop3(_) => {
                if uniform_op {
                    Udp
                } else {
                    Coupled
                }
            }
            Op::PSetP(_) => {
                if uniform_op {
                    Udp
                } else {
                    Coupled
                }
            }
            Op::Sel(_) => {
                if uniform_op {
                    Udp
                } else {
                    Coupled
                }
            }
            Op::Vote(_) => {
                if uniform_op {
                    VoteU
                } else {
                    crate::codegen::ice!("Illegal Vote in upred");
                }
            }
            _ => {
                crate::codegen::ice!("Illegal instruction in upred category {op}");
            }
        }
    }

    pub(super) fn pred_read_after_write(writer: Self, reader: Self) -> u32 {
        use UPredLatencySM80::*;
        match reader {
            Coupled => match writer {
                Udp => 6,
                VoteU => 1,
                Coupled | UGuard | Bra_Jmp | Uldc_Mma => {
                    crate::codegen::ice!("Illegal writer in ureg praw latency")
                }
            },
            Udp => match writer {
                Udp => 4,
                VoteU => 1,
                Coupled | UGuard | Bra_Jmp | Uldc_Mma => {
                    crate::codegen::ice!("Illegal writer in ureg praw latency")
                }
            },
            UGuard => match writer {
                Udp => 11,
                VoteU => 5,
                Coupled | UGuard | Bra_Jmp | Uldc_Mma => {
                    crate::codegen::ice!("Illegal writer in ureg praw latency")
                }
            },
            Bra_Jmp => match writer {
                Udp => 9,
                VoteU => 2,
                Coupled | UGuard | Bra_Jmp | Uldc_Mma => {
                    crate::codegen::ice!("Illegal writer in ureg praw latency")
                }
            },
            Uldc_Mma => match writer {
                Udp => 11,
                VoteU => 5,
                Coupled | UGuard | Bra_Jmp | Uldc_Mma => {
                    crate::codegen::ice!("Illegal writer in ureg praw latency")
                }
            },
            VoteU => {
                crate::codegen::ice!("Illegal reader in ureg praw latency")
            }
        }
    }

    pub(super) fn pred_write_after_write(writer1: Self, writer2: Self) -> u32 {
        use UPredLatencySM80::*;
        match writer2 {
            Udp => match writer1 {
                Udp => 1,
                VoteU => 1,
                Coupled | UGuard | Bra_Jmp | Uldc_Mma => {
                    crate::codegen::ice!("Illegal writer in ureg waw latency")
                }
            },
            VoteU => match writer1 {
                Udp => 7,
                VoteU => 1,
                Coupled | UGuard | Bra_Jmp | Uldc_Mma => {
                    crate::codegen::ice!("Illegal writer in ureg waw latency")
                }
            },
            Coupled | UGuard | Bra_Jmp | Uldc_Mma => {
                crate::codegen::ice!("Illegal writer in ureg waw latency")
            }
        }
    }

    pub(super) fn pred_write_after_read(reader: Self, writer: Self) -> u32 {
        use UPredLatencySM80::*;
        match writer {
            Udp => match reader {
                Coupled | Udp | UGuard | Bra_Jmp | Uldc_Mma => 1,
                VoteU => {
                    crate::codegen::ice!("Illegal reader in ureg pwar latency")
                }
            },
            VoteU => match reader {
                Coupled | Udp => 2,
                UGuard | Bra_Jmp | Uldc_Mma => 1,
                VoteU => {
                    crate::codegen::ice!("Illegal reader in ureg pwar latency")
                }
            },
            Coupled | UGuard | Bra_Jmp | Uldc_Mma => {
                crate::codegen::ice!("Illegal writer in ureg pwar latency")
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{UPredLatencySM80, URegLatencySM80};
    use crate::codegen::ir::{
        CBuf, CBufRef, Dst, LabelAllocator, LdcMode, LogicOp3, MemType, Op, OpBra, OpLdc, OpMov,
        OpPLop3, OpS2R, OpVote, RegFile, RegRef, Src, VoteOp,
    };

    fn ugpr(n: u32) -> Dst {
        Dst::Reg(RegRef::new(RegFile::UGPR, n, 1))
    }

    #[test]
    fn ureg_op_category_uniform_s2r_writer() {
        let op = Op::S2R(Box::new(OpS2R {
            dst: ugpr(0),
            idx: 0,
        }));
        assert!(matches!(
            URegLatencySM80::op_category(&op, false, 0),
            URegLatencySM80::ToUr
        ));
    }

    #[test]
    fn ureg_op_category_uniform_mov() {
        let op = Op::Mov(Box::new(OpMov {
            dst: ugpr(0),
            src: Src::ZERO,
            quad_lanes: 0xf,
        }));
        assert!(matches!(
            URegLatencySM80::op_category(&op, false, 0),
            URegLatencySM80::Umov
        ));
    }

    #[test]
    fn ureg_op_category_non_uniform_mov() {
        let op = Op::Mov(Box::new(OpMov {
            dst: Dst::Reg(RegRef::new(RegFile::GPR, 0, 1)),
            src: Src::ZERO,
            quad_lanes: 0xf,
        }));
        let c = URegLatencySM80::op_category(&op, false, 0);
        assert!(matches!(c, URegLatencySM80::Coupled | URegLatencySM80::Udp));
    }

    #[test]
    fn ureg_op_category_uniform_vote() {
        let op = Op::Vote(Box::new(OpVote {
            op: VoteOp::All,
            dsts: [ugpr(0), Dst::None],
            pred: Src::new_imm_bool(true),
        }));
        assert!(matches!(
            URegLatencySM80::op_category(&op, false, 0),
            URegLatencySM80::VoteU
        ));
    }

    #[test]
    fn upred_op_category_udp_style() {
        let op = Op::Mov(Box::new(OpMov {
            dst: ugpr(0),
            src: Src::ZERO,
            quad_lanes: 0xf,
        }));
        assert!(matches!(
            UPredLatencySM80::op_category(&op),
            UPredLatencySM80::Udp
        ));
    }

    #[test]
    fn upred_op_category_bra() {
        let mut la = LabelAllocator::new();
        let op = Op::Bra(Box::new(OpBra {
            target: la.alloc(),
            cond: Src::new_imm_bool(true),
        }));
        assert!(matches!(
            UPredLatencySM80::op_category(&op),
            UPredLatencySM80::Bra_Jmp
        ));
    }

    #[test]
    fn upred_op_category_ldc_uniform() {
        let op = Op::Ldc(Box::new(OpLdc {
            dst: ugpr(0),
            srcs: [
                Src::from(CBufRef {
                    buf: CBuf::Binding(0),
                    offset: 0,
                }),
                Src::ZERO,
            ],
            mode: LdcMode::Indexed,
            mem_type: MemType::B32,
        }));
        assert!(matches!(
            UPredLatencySM80::op_category(&op),
            UPredLatencySM80::Uldc_Mma
        ));
    }

    #[test]
    fn upred_op_category_plop3_non_uniform() {
        let and = LogicOp3::new_lut(&|x, y, z| x & y & z);
        let op = Op::PLop3(Box::new(OpPLop3 {
            dsts: [Dst::Reg(RegRef::new(RegFile::Pred, 0, 1)), Dst::None],
            srcs: [
                Src::new_imm_bool(false),
                Src::new_imm_bool(false),
                Src::new_imm_bool(false),
            ],
            ops: [and, and],
        }));
        assert!(matches!(
            UPredLatencySM80::op_category(&op),
            UPredLatencySM80::Coupled
        ));
    }

    #[test]
    fn upred_op_category_plop3_uniform() {
        let and = LogicOp3::new_lut(&|x, y, z| x & y & z);
        let op = Op::PLop3(Box::new(OpPLop3 {
            dsts: [Dst::Reg(RegRef::new(RegFile::UPred, 0, 1)), Dst::None],
            srcs: [
                Src::new_imm_bool(false),
                Src::new_imm_bool(false),
                Src::new_imm_bool(false),
            ],
            ops: [and, and],
        }));
        assert!(matches!(
            UPredLatencySM80::op_category(&op),
            UPredLatencySM80::Udp
        ));
    }

    #[test]
    fn upred_op_category_vote_uniform() {
        let op = Op::Vote(Box::new(OpVote {
            op: VoteOp::All,
            dsts: [ugpr(0), Dst::None],
            pred: Src::new_imm_bool(true),
        }));
        assert!(matches!(
            UPredLatencySM80::op_category(&op),
            UPredLatencySM80::VoteU
        ));
    }

    #[test]
    fn upred_pred_read_after_write_table() {
        use UPredLatencySM80::*;
        assert_eq!(UPredLatencySM80::pred_read_after_write(Udp, Coupled), 6);
        assert_eq!(UPredLatencySM80::pred_read_after_write(VoteU, Coupled), 1);
        assert_eq!(UPredLatencySM80::pred_read_after_write(Udp, Udp), 4);
        assert_eq!(UPredLatencySM80::pred_read_after_write(VoteU, Udp), 1);
        assert_eq!(UPredLatencySM80::pred_read_after_write(Udp, UGuard), 11);
        assert_eq!(UPredLatencySM80::pred_read_after_write(VoteU, UGuard), 5);
        assert_eq!(UPredLatencySM80::pred_read_after_write(Udp, Bra_Jmp), 9);
        assert_eq!(UPredLatencySM80::pred_read_after_write(VoteU, Bra_Jmp), 2);
        assert_eq!(UPredLatencySM80::pred_read_after_write(Udp, Uldc_Mma), 11);
        assert_eq!(UPredLatencySM80::pred_read_after_write(VoteU, Uldc_Mma), 5);
    }

    #[test]
    fn upred_pred_write_after_write_table() {
        use UPredLatencySM80::*;
        assert_eq!(UPredLatencySM80::pred_write_after_write(Udp, Udp), 1);
        assert_eq!(UPredLatencySM80::pred_write_after_write(VoteU, Udp), 1);
        assert_eq!(UPredLatencySM80::pred_write_after_write(Udp, VoteU), 7);
        assert_eq!(UPredLatencySM80::pred_write_after_write(VoteU, VoteU), 1);
    }

    #[test]
    fn upred_pred_write_after_read_table() {
        use UPredLatencySM80::*;
        assert_eq!(UPredLatencySM80::pred_write_after_read(Coupled, Udp), 1);
        assert_eq!(UPredLatencySM80::pred_write_after_read(Udp, Udp), 1);
        assert_eq!(UPredLatencySM80::pred_write_after_read(Coupled, VoteU), 2);
        assert_eq!(UPredLatencySM80::pred_write_after_read(UGuard, VoteU), 1);
    }

    #[test]
    fn ureg_raw_waw_war_roundtrip() {
        use URegLatencySM80::*;
        let raw = URegLatencySM80::read_after_write(ToUr, Udp);
        assert!(raw >= 1);
        let waw = URegLatencySM80::write_after_write(Udp, ToUr, false);
        assert!(waw >= 1);
        let war = URegLatencySM80::write_after_read(Coupled, Udp);
        assert!(war >= 1);
    }
}
