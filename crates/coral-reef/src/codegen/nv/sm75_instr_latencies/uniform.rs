// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright © 2025-2026 ecoPrimals
// Derived from Red Hat. (2025)
use super::pred;
use crate::codegen::ir::*;

#[derive(Debug)]
pub(super) enum URegLatencySM75 {
    Udp,
    VectorCoupled,
    VectorDecoupled,
    Uldc,
    Umov,
    VectorCoupledBindless,
    VectorDecoupledBindless,
    VoteU,
    GuardPredicate,
    R2UR,
}

impl URegLatencySM75 {
    pub(super) fn op_category(op: &Op, reader: bool, op_reg_idx: usize) -> Self {
        use URegLatencySM75::*;
        // is this using a bindless cbuf as a src register.
        // this decides between the category types for readers.
        let bindless = reader && op.srcs_as_slice()[op_reg_idx].is_bindless_cbuf();

        let vcoupled = if bindless {
            VectorCoupledBindless
        } else {
            VectorCoupled
        };
        let vdecoupled = if bindless {
            VectorDecoupledBindless
        } else {
            VectorDecoupled
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
            Op::R2UR(_) => {
                if !reader {
                    R2UR
                } else {
                    crate::codegen::ice!("Illegal R2UR in ureg");
                }
            }
            Op::S2R(_) => {
                if !reader {
                    R2UR
                } else {
                    crate::codegen::ice!("Illegal S2UR in ureg");
                }
            }
            Op::Vote(_) => VoteU,

            Op::FRnd(_) => vdecoupled,
            Op::FAdd(_)
            | Op::FMul(_)
            | Op::FFma(_)
            | Op::FSet(_)
            | Op::FSetP(_)
            | Op::FMnMx(_)
            | Op::HAdd2(_)
            | Op::HMul2(_)
            | Op::HSet2(_)
            | Op::HFma2(_)
            | Op::HSetP2(_) => vcoupled,
            Op::DMul(_) | Op::DFma(_) | Op::DAdd(_) | Op::DSetP(_) => vdecoupled,
            _ => {
                crate::codegen::ice!("Illegal instruction in ureg category {op}");
            }
        }
    }

    pub(super) fn read_after_write(writer: Self, reader: Self) -> u32 {
        use URegLatencySM75::*;
        match reader {
            Udp => match writer {
                Udp => 4,
                R2UR => 2,
                Uldc | VoteU | Umov => 2,
                _ => {
                    crate::codegen::ice!("Illegal writer in raw ureg latency {writer:?}")
                }
            },
            VectorCoupled => match writer {
                Udp => 6,
                R2UR => 2,
                Uldc | VoteU | Umov => 2,
                _ => {
                    crate::codegen::ice!("Illegal writer in raw ureg latency {writer:?}")
                }
            },
            VectorDecoupled => match writer {
                Udp => 9,
                R2UR => 2,
                Uldc | VoteU | Umov => 2,
                _ => {
                    crate::codegen::ice!("Illegal writer in raw ureg latency {writer:?}")
                }
            },
            Uldc | VectorCoupledBindless | VectorDecoupledBindless => match writer {
                Udp => 12,
                R2UR => 2,
                Uldc | VoteU | Umov => 5,
                _ => {
                    crate::codegen::ice!("Illegal writer in raw ureg latency {writer:?}")
                }
            },
            Umov => match writer {
                Udp => 7,
                R2UR => 2,
                Uldc | VoteU | Umov => 2,
                _ => {
                    crate::codegen::ice!("Illegal writer in raw ureg latency")
                }
            },
            _ => {
                crate::codegen::ice!("Illegal read in ureg raw latency")
            }
        }
    }

    pub(super) fn write_after_write(writer1: Self, writer2: Self, has_pred: bool) -> u32 {
        use URegLatencySM75::*;
        match writer2 {
            Udp => match writer1 {
                Udp => 1,
                R2UR => 2,
                Uldc | VoteU | Umov => 1,
                _ => {
                    crate::codegen::ice!("Illegal writer in ureg waw latency")
                }
            },
            R2UR => match writer1 {
                Udp => pred(has_pred, 4, 6),
                R2UR => 2,
                Uldc | VoteU | Umov => 4,
                _ => {
                    crate::codegen::ice!("Illegal writer in ureg waw latency")
                }
            },
            Uldc | VoteU | Umov => match writer1 {
                Udp => 7,
                R2UR => 2,
                Uldc | VoteU | Umov => 1,
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
        use URegLatencySM75::*;
        match writer {
            Udp => 1,
            R2UR => 1,
            Uldc | VoteU | Umov => match reader {
                Udp => 3,
                _ => 1,
            },
            _ => {
                crate::codegen::ice!("Illegal writer in ureg war latency")
            }
        }
    }

    pub(super) fn pred_read_after_write(writer: Self, reader: Self) -> u32 {
        use URegLatencySM75::*;
        match reader {
            Udp => match writer {
                Udp => 4,
                VoteU => 1,
                _ => {
                    crate::codegen::ice!("Illegal writer in upred raw latency")
                }
            },
            VectorCoupled => match writer {
                Udp => 6,
                VoteU => 1,
                _ => {
                    crate::codegen::ice!("Illegal writer in upred raw latency")
                }
            },
            GuardPredicate => match writer {
                Udp => 11,
                VoteU => 5,
                _ => {
                    crate::codegen::ice!("Illegal writer in upred raw latency")
                }
            },
            _ => {
                crate::codegen::ice!("Illegal reader in upred raw latency")
            }
        }
    }

    pub(super) fn pred_write_after_write(writer1: Self, writer2: Self) -> u32 {
        use URegLatencySM75::*;
        match writer2 {
            Udp => 1,
            VoteU => match writer1 {
                Udp => 7,
                VoteU => 1,
                _ => {
                    crate::codegen::ice!("Illegal writer1 in upred raw latency")
                }
            },
            _ => {
                crate::codegen::ice!("Illegal writer2 in upred raw latency")
            }
        }
    }

    pub(super) fn pred_write_after_read(reader: Self, writer: Self) -> u32 {
        use URegLatencySM75::*;
        match writer {
            Udp => 1,
            VoteU => match reader {
                Udp => 2,
                _ => 1,
            },
            _ => {
                crate::codegen::ice!("Illegal writer2 in upred raw latency")
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::URegLatencySM75;

    #[test]
    fn upred_pred_read_after_write_table() {
        use URegLatencySM75::*;
        assert_eq!(URegLatencySM75::pred_read_after_write(Udp, Udp), 4);
        assert_eq!(URegLatencySM75::pred_read_after_write(VoteU, Udp), 1);
        assert_eq!(
            URegLatencySM75::pred_read_after_write(Udp, VectorCoupled),
            6
        );
        assert_eq!(
            URegLatencySM75::pred_read_after_write(VoteU, VectorCoupled),
            1
        );
        assert_eq!(
            URegLatencySM75::pred_read_after_write(Udp, GuardPredicate),
            11
        );
        assert_eq!(
            URegLatencySM75::pred_read_after_write(VoteU, GuardPredicate),
            5
        );
    }

    #[test]
    fn upred_pred_write_after_write_table() {
        use URegLatencySM75::*;
        assert_eq!(URegLatencySM75::pred_write_after_write(Udp, Udp), 1);
        assert_eq!(URegLatencySM75::pred_write_after_write(Udp, VoteU), 7);
        assert_eq!(URegLatencySM75::pred_write_after_write(VoteU, VoteU), 1);
    }

    #[test]
    fn upred_pred_write_after_read_table() {
        use URegLatencySM75::*;
        assert_eq!(URegLatencySM75::pred_write_after_read(Udp, Udp), 1);
        assert_eq!(URegLatencySM75::pred_write_after_read(Udp, VoteU), 2);
        assert_eq!(
            URegLatencySM75::pred_write_after_read(VectorCoupled, VoteU),
            1
        );
    }
}
