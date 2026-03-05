// Copyright © 2025 Red Hat.
// SPDX-License-Identifier: MIT
#![allow(non_camel_case_types, clippy::wildcard_imports, clippy::enum_glob_use)]

use super::super::ir::*;
use super::super::sm75_instr_latencies::pred;

#[allow(dead_code)]
#[derive(Debug)]
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
    pub(super) fn op_category(op: &Op) -> PredLatencySM80 {
        match op {
            Op::Atom(_) => PredLatencySM80::Decoupled,
            Op::Bra(_) => PredLatencySM80::Decoupled,
            Op::DSetP(_) => PredLatencySM80::RedirectedFP64,
            Op::FMnMx(_) | Op::FSetP(_) => PredLatencySM80::Coupled,
            Op::HFma2(_) => PredLatencySM80::FP16,
            Op::HMnMx2(_) => PredLatencySM80::FP16,
            Op::HSetP2(_) => PredLatencySM80::FP16,
            Op::IAdd3(_) => PredLatencySM80::Coupled,
            Op::IAdd3X(_) => PredLatencySM80::Coupled,
            Op::IMad(_) => PredLatencySM80::FMA,
            Op::IMad64(_) => PredLatencySM80::FMA,
            Op::IMnMx(_) => PredLatencySM80::Coupled,
            Op::IMul(_) => PredLatencySM80::FMA,
            Op::Ipa(_) => PredLatencySM80::Decoupled,
            Op::ISetP(_) => PredLatencySM80::Coupled,

            Op::Ld(_) => PredLatencySM80::Decoupled,

            Op::Lea(_) | Op::LeaX(_) => PredLatencySM80::Coupled,
            Op::PixLd(_) => PredLatencySM80::Decoupled,
            Op::PLop3(_) => PredLatencySM80::Coupled,
            Op::PSetP(_) => PredLatencySM80::Coupled,
            Op::R2UR(_) => PredLatencySM80::Decoupled,
            Op::Sel(_) => PredLatencySM80::Coupled,
            Op::Shfl(_) => PredLatencySM80::Decoupled,
            Op::SuLd(_) => PredLatencySM80::Decoupled,
            Op::SuSt(_) => PredLatencySM80::Decoupled,
            Op::Tex(_) => PredLatencySM80::Decoupled,
            Op::Tld(_) => PredLatencySM80::Decoupled,
            Op::Tld4(_) => PredLatencySM80::Decoupled,
            Op::Tmml(_) => PredLatencySM80::Decoupled,
            Op::Txd(_) => PredLatencySM80::Decoupled,
            Op::Txq(_) => PredLatencySM80::Decoupled,

            Op::Vote(_) => PredLatencySM80::Disp_Alu,
            Op::Match(_) => PredLatencySM80::Decoupled,
            _ => {
                panic!("Illegal op in sm80 pred latency {}", op);
            }
        }
    }

    pub(super) fn pred_read_after_write(
        writer: PredLatencySM80,
        reader: PredLatencySM80,
    ) -> u32 {
        match reader {
            PredLatencySM80::Disp_Alu => match writer {
                PredLatencySM80::Disp_Alu
                | PredLatencySM80::Coupled
                | PredLatencySM80::FMA
                | PredLatencySM80::FP16 => 13,
                PredLatencySM80::HFMA2_MMA | PredLatencySM80::RedirectedFP64 => 14,
                PredLatencySM80::Decoupled => 1,
                PredLatencySM80::Guard => {
                    panic!("Illegal writer in sm80 praw");
                }
            },
            PredLatencySM80::Coupled => match writer {
                PredLatencySM80::Disp_Alu | PredLatencySM80::Coupled => 4,
                PredLatencySM80::FMA | PredLatencySM80::FP16 => 5,
                PredLatencySM80::HFMA2_MMA | PredLatencySM80::RedirectedFP64 => 6,
                PredLatencySM80::Decoupled => 1,
                PredLatencySM80::Guard => {
                    panic!("Illegal writer in sm80 praw");
                }
            },
            PredLatencySM80::FMA => match writer {
                PredLatencySM80::Disp_Alu | PredLatencySM80::Coupled => 5,
                PredLatencySM80::FMA => 4,
                PredLatencySM80::FP16 => 5,
                PredLatencySM80::HFMA2_MMA | PredLatencySM80::RedirectedFP64 => 6,
                PredLatencySM80::Decoupled => 1,
                PredLatencySM80::Guard => {
                    panic!("Illegal writer in sm80 praw");
                }
            },
            PredLatencySM80::HFMA2_MMA | PredLatencySM80::RedirectedFP64 => match writer {
                PredLatencySM80::Disp_Alu
                | PredLatencySM80::Coupled
                | PredLatencySM80::FMA
                | PredLatencySM80::FP16 => 13,
                PredLatencySM80::HFMA2_MMA | PredLatencySM80::RedirectedFP64 => 6,
                PredLatencySM80::Decoupled => 1,
                PredLatencySM80::Guard => {
                    panic!("Illegal writer in sm80 praw");
                }
            },
            PredLatencySM80::FP16 => match writer {
                PredLatencySM80::Disp_Alu | PredLatencySM80::Coupled => 5,
                PredLatencySM80::FMA => 4,
                PredLatencySM80::FP16 => 5,
                PredLatencySM80::HFMA2_MMA | PredLatencySM80::RedirectedFP64 => 6,
                PredLatencySM80::Decoupled => 1,
                PredLatencySM80::Guard => {
                    panic!("Illegal writer in sm80 praw");
                }
            },
            PredLatencySM80::Decoupled | PredLatencySM80::Guard => match writer {
                PredLatencySM80::Disp_Alu
                | PredLatencySM80::Coupled
                | PredLatencySM80::FMA
                | PredLatencySM80::FP16 => 13,
                PredLatencySM80::HFMA2_MMA | PredLatencySM80::RedirectedFP64 => 14,
                PredLatencySM80::Decoupled => 1,
                PredLatencySM80::Guard => {
                    panic!("Illegal writer in sm80 praw");
                }
            },
        }
    }

    pub(super) fn pred_write_after_write(
        writer1: PredLatencySM80,
        writer2: PredLatencySM80,
        has_pred: bool,
    ) -> u32 {
        match writer2 {
            PredLatencySM80::Disp_Alu | PredLatencySM80::Coupled | PredLatencySM80::FMA => {
                match writer1 {
                    PredLatencySM80::Disp_Alu
                    | PredLatencySM80::Coupled
                    | PredLatencySM80::FMA
                    | PredLatencySM80::FP16 => 1,
                    PredLatencySM80::HFMA2_MMA | PredLatencySM80::RedirectedFP64 => 2,
                    PredLatencySM80::Decoupled => 1,
                    PredLatencySM80::Guard => {
                        panic!("Illegal writer in sm80 pwaw");
                    }
                }
            }
            PredLatencySM80::FP16 => match writer1 {
                PredLatencySM80::Disp_Alu | PredLatencySM80::Coupled | PredLatencySM80::FMA => {
                    pred(has_pred, 2, 7)
                }
                PredLatencySM80::FP16 => 1,
                PredLatencySM80::HFMA2_MMA | PredLatencySM80::RedirectedFP64 => {
                    pred(has_pred, 2, 8)
                }
                PredLatencySM80::Decoupled => 1,
                PredLatencySM80::Guard => {
                    panic!("Illegal writer in sm80 pwaw");
                }
            },
            PredLatencySM80::HFMA2_MMA | PredLatencySM80::RedirectedFP64 => match writer1 {
                PredLatencySM80::Disp_Alu
                | PredLatencySM80::Coupled
                | PredLatencySM80::FMA
                | PredLatencySM80::FP16 => pred(has_pred, 2, 5),
                PredLatencySM80::HFMA2_MMA
                | PredLatencySM80::RedirectedFP64
                | PredLatencySM80::Decoupled => 1,
                PredLatencySM80::Guard => {
                    panic!("Illegal writer in sm80 pwaw");
                }
            },
            PredLatencySM80::Decoupled => match writer1 {
                PredLatencySM80::Disp_Alu | PredLatencySM80::Coupled | PredLatencySM80::FMA => {
                    pred(has_pred, 2, 10)
                }
                PredLatencySM80::FP16 => pred(has_pred, 1, 11),
                PredLatencySM80::HFMA2_MMA | PredLatencySM80::RedirectedFP64 => {
                    pred(has_pred, 1, 12)
                }
                PredLatencySM80::Decoupled => 1,
                PredLatencySM80::Guard => {
                    panic!("Illegal writer in sm80 pwaw");
                }
            },
            PredLatencySM80::Guard => {
                panic!("Illegal writer in sm80 pwaw");
            }
        }
    }

    pub(super) fn pred_write_after_read(
        _reader: PredLatencySM80,
        _writer: PredLatencySM80,
    ) -> u32 {
        1
    }
}
