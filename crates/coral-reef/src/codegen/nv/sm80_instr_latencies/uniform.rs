// SPDX-License-Identifier: AGPL-3.0-only
// Copyright © 2025-2026 ecoPrimals
// Derived from Red Hat. (2025)
#![allow(non_camel_case_types, clippy::wildcard_imports, clippy::enum_glob_use)]

use super::super::sm75_instr_latencies::pred;
use crate::codegen::ir::*;

#[expect(dead_code, reason = "latency model for future SM target support")]
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

            // mov32i => URegLatency::Uldc,
            // p2ur => Udp,
            Op::PLop3(_) => vcoupled,
            Op::PopC(_) => vdecoupled,
            Op::Prmt(_) => vcoupled,
            Op::PSetP(_) => vcoupled,
            // UR2UP
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
                    panic!("Illegal R2UR in ureg");
                }
            }
            Op::S2R(_) => {
                if !reader {
                    ToUr
                } else {
                    panic!("Illegal S2UR in ureg");
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
                panic!("Illegal instuction in ureg category {}", op);
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
                    panic!("Illegal writer in raw ureg latency {:?}", writer)
                }
            },
            Decoupled => match writer {
                ToUr => 1,
                Udp => 9,
                Uldc => 2,
                Umov => 2,
                VoteU => 2,
                _ => {
                    panic!("Illegal writer in raw ureg latency {:?}", writer)
                }
            },
            Cbu => match writer {
                ToUr => 1,
                Udp => 10,
                Uldc => 3,
                Umov => 3,
                VoteU => 3,
                _ => {
                    panic!("Illegal writer in raw ureg latency {:?}", writer)
                }
            },
            CoupledBindless | DecoupledBindless | Uldc => match writer {
                ToUr => 1,
                Udp => 12,
                Uldc => 5,
                Umov => 5,
                VoteU => 5,
                _ => {
                    panic!("Illegal writer in raw ureg latency {:?}", writer)
                }
            },
            Udp => match writer {
                ToUr => 1,
                Udp => 4,
                Uldc => 2,
                Umov => 2,
                VoteU => 2,
                _ => {
                    panic!("Illegal writer in raw ureg latency {:?}", writer)
                }
            },
            Umov | VoteU => match writer {
                ToUr => 1,
                Udp => 7,
                Uldc => 2,
                Umov => 2,
                VoteU => 2,
                _ => {
                    panic!("Illegal writer in raw ureg latency {:?}", writer)
                }
            },
            _ => {
                panic!("Illegal read in ureg raw latency")
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
                    panic!("Illegal writer in ureg waw latency")
                }
            },
            Udp => match writer1 {
                ToUr | Udp | Uldc | Umov | VoteU => 1,
                _ => {
                    panic!("Illegal writer in ureg waw latency")
                }
            },
            Uldc | Umov | VoteU => match writer1 {
                ToUr => 1,
                Udp => 7,
                Uldc | Umov | VoteU => 1,
                _ => {
                    panic!("Illegal writer in ureg waw latency")
                }
            },
            _ => {
                panic!("Illegal writer in ureg waw latency")
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
                    panic!("Illegal reader in ureg war latency")
                }
            },
            Uldc | Umov | VoteU => match reader {
                Coupled | Decoupled | Cbu | CoupledBindless | DecoupledBindless | Rpcmov_64
                | Uldc | Umov => 1,
                Udp => 3,
                _ => {
                    panic!("Illegal reader in ureg war latency")
                }
            },
            _ => {
                panic!("Illegal writer in ureg war latency")
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
                    panic!("Illegal Vote in upred");
                }
            }
            _ => {
                panic!("Illegal instuction in upred category {}", op);
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
                    panic!("Illegal writer in ureg praw latency")
                }
            },
            Udp => match writer {
                Udp => 4,
                VoteU => 1,
                Coupled | UGuard | Bra_Jmp | Uldc_Mma => {
                    panic!("Illegal writer in ureg praw latency")
                }
            },
            UGuard => match writer {
                Udp => 11,
                VoteU => 5,
                Coupled | UGuard | Bra_Jmp | Uldc_Mma => {
                    panic!("Illegal writer in ureg praw latency")
                }
            },
            Bra_Jmp => match writer {
                Udp => 9,
                VoteU => 2,
                Coupled | UGuard | Bra_Jmp | Uldc_Mma => {
                    panic!("Illegal writer in ureg praw latency")
                }
            },
            Uldc_Mma => match writer {
                Udp => 11,
                VoteU => 5,
                Coupled | UGuard | Bra_Jmp | Uldc_Mma => {
                    panic!("Illegal writer in ureg praw latency")
                }
            },
            VoteU => {
                panic!("Illegal reader in ureg praw latency")
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
                    panic!("Illegal writer in ureg waw latency")
                }
            },
            VoteU => match writer1 {
                Udp => 7,
                VoteU => 1,
                Coupled | UGuard | Bra_Jmp | Uldc_Mma => {
                    panic!("Illegal writer in ureg waw latency")
                }
            },
            Coupled | UGuard | Bra_Jmp | Uldc_Mma => {
                panic!("Illegal writer in ureg waw latency")
            }
        }
    }

    pub(super) fn pred_write_after_read(reader: Self, writer: Self) -> u32 {
        use UPredLatencySM80::*;
        match writer {
            Udp => match reader {
                Coupled | Udp | UGuard | Bra_Jmp | Uldc_Mma => 1,
                VoteU => {
                    panic!("Illegal reader in ureg pwar latency")
                }
            },
            VoteU => match reader {
                Coupled | Udp => 2,
                UGuard | Bra_Jmp | Uldc_Mma => 1,
                VoteU => {
                    panic!("Illegal reader in ureg pwar latency")
                }
            },
            Coupled | UGuard | Bra_Jmp | Uldc_Mma => {
                panic!("Illegal writer in ureg pwar latency")
            }
        }
    }
}
