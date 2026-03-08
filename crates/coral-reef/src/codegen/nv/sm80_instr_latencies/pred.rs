// SPDX-License-Identifier: AGPL-3.0-only
// Copyright © 2025-2026 ecoPrimals
// Derived from Red Hat. (2025)
#![allow(non_camel_case_types, clippy::wildcard_imports, clippy::enum_glob_use)]

use super::super::sm75_instr_latencies::pred;
use crate::codegen::ir::*;

#[expect(dead_code, reason = "latency model for future SM target support")]
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
                panic!("Illegal op in sm80 pred latency {}", op);
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
                    panic!("Illegal writer in sm80 praw");
                }
            },
            Self::Coupled => match writer {
                Self::Disp_Alu | Self::Coupled => 4,
                Self::FMA | Self::FP16 => 5,
                Self::HFMA2_MMA | Self::RedirectedFP64 => 6,
                Self::Decoupled => 1,
                Self::Guard => {
                    panic!("Illegal writer in sm80 praw");
                }
            },
            Self::FMA => match writer {
                Self::Disp_Alu | Self::Coupled => 5,
                Self::FMA => 4,
                Self::FP16 => 5,
                Self::HFMA2_MMA | Self::RedirectedFP64 => 6,
                Self::Decoupled => 1,
                Self::Guard => {
                    panic!("Illegal writer in sm80 praw");
                }
            },
            Self::HFMA2_MMA | Self::RedirectedFP64 => match writer {
                Self::Disp_Alu | Self::Coupled | Self::FMA | Self::FP16 => 13,
                Self::HFMA2_MMA | Self::RedirectedFP64 => 6,
                Self::Decoupled => 1,
                Self::Guard => {
                    panic!("Illegal writer in sm80 praw");
                }
            },
            Self::FP16 => match writer {
                Self::Disp_Alu | Self::Coupled => 5,
                Self::FMA => 4,
                Self::FP16 => 5,
                Self::HFMA2_MMA | Self::RedirectedFP64 => 6,
                Self::Decoupled => 1,
                Self::Guard => {
                    panic!("Illegal writer in sm80 praw");
                }
            },
            Self::Decoupled | Self::Guard => match writer {
                Self::Disp_Alu | Self::Coupled | Self::FMA | Self::FP16 => 13,
                Self::HFMA2_MMA | Self::RedirectedFP64 => 14,
                Self::Decoupled => 1,
                Self::Guard => {
                    panic!("Illegal writer in sm80 praw");
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
                    panic!("Illegal writer in sm80 pwaw");
                }
            },
            Self::FP16 => match writer1 {
                Self::Disp_Alu | Self::Coupled | Self::FMA => pred(has_pred, 2, 7),
                Self::FP16 => 1,
                Self::HFMA2_MMA | Self::RedirectedFP64 => pred(has_pred, 2, 8),
                Self::Decoupled => 1,
                Self::Guard => {
                    panic!("Illegal writer in sm80 pwaw");
                }
            },
            Self::HFMA2_MMA | Self::RedirectedFP64 => match writer1 {
                Self::Disp_Alu | Self::Coupled | Self::FMA | Self::FP16 => pred(has_pred, 2, 5),
                Self::HFMA2_MMA | Self::RedirectedFP64 | Self::Decoupled => 1,
                Self::Guard => {
                    panic!("Illegal writer in sm80 pwaw");
                }
            },
            Self::Decoupled => match writer1 {
                Self::Disp_Alu | Self::Coupled | Self::FMA => pred(has_pred, 2, 10),
                Self::FP16 => pred(has_pred, 1, 11),
                Self::HFMA2_MMA | Self::RedirectedFP64 => pred(has_pred, 1, 12),
                Self::Decoupled => 1,
                Self::Guard => {
                    panic!("Illegal writer in sm80 pwaw");
                }
            },
            Self::Guard => {
                panic!("Illegal writer in sm80 pwaw");
            }
        }
    }

    pub(super) fn pred_write_after_read(_reader: Self, _writer: Self) -> u32 {
        1
    }
}
