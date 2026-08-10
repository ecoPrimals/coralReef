// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright © 2025-2026 ecoPrimals
// Derived from Red Hat. (2025)
//! RAW, WAW, and WAR hazard latency tables for SM80+ (Ampere/Ada).
//!
//! Split from `gpr` for code size compliance. Each function maps
//! (writer/reader category × writer/reader category) → minimum stall
//! cycles required to avoid hardware data hazards.

use super::super::sm75_instr_latencies::pred;
use super::gpr::RegLatencySM80;

impl RegLatencySM80 {
    pub(super) fn read_after_write(writer: Self, reader: Self) -> u32 {
        use RegLatencySM80::*;
        match reader {
            CoupledAlu => match writer {
                CoupledAlu => 4,
                CoupledDisp64 => 6,
                CoupledFMA => 5,
                IMADWideWriteDL => 3,
                IMADWideWriteDH => 5,
                FP16 => 5,
                FP16_Alu => 5,
                FP16_F32 => 5,
                HFMA2_MMA => 10,
                RedirectedFP64 => 10,
                Clmad => 12,
                IMMA_88 => 13,
                MMA_1x_collect => 16,
                MMA_2x_collect => 24,
                DMMA => 25,
                Cbu => 1,
                Decoupled => 1,
                DecoupledAgu => 1,
                _ => {
                    crate::codegen::ice!("Illegal writer in sm80 raw");
                }
            },
            CoupledFMA | IMADWideReadCL => match writer {
                CoupledAlu => 5,
                CoupledDisp64 => 6,
                CoupledFMA => 4,
                IMADWideWriteDL => 2,
                IMADWideWriteDH => 4,
                FP16 => 5,
                FP16_Alu => 5,
                FP16_F32 => 5,
                HFMA2_MMA => 10,
                RedirectedFP64 => 10,
                Clmad => 12,
                IMMA_88 => 13,
                MMA_1x_collect => 16,
                MMA_2x_collect => 24,
                DMMA => 25,
                Cbu => 1,
                Decoupled => 1,
                DecoupledAgu => 1,
                _ => {
                    crate::codegen::ice!("Illegal writer in sm80 raw");
                }
            },
            IMADWideReadAB => match writer {
                CoupledAlu => 5,
                CoupledDisp64 => 6,
                CoupledFMA => 4,
                IMADWideWriteDL => 4,
                IMADWideWriteDH => 6,
                FP16 => 5,
                FP16_Alu => 5,
                FP16_F32 => 5,
                HFMA2_MMA => 10,
                RedirectedFP64 => 10,
                Clmad => 12,
                IMMA_88 => 13,
                MMA_1x_collect => 16,
                MMA_2x_collect => 24,
                DMMA => 25,
                Cbu => 1,
                Decoupled => 1,
                DecoupledAgu => 1,
                _ => {
                    crate::codegen::ice!("Illegal writer in sm80 raw");
                }
            },
            IMADWideReadCH => match writer {
                CoupledAlu => 3,
                CoupledDisp64 => 4,
                CoupledFMA => 2,
                IMADWideWriteDL => 2,
                IMADWideWriteDH => 2,
                FP16 => 3,
                FP16_Alu => 3,
                FP16_F32 => 3,
                HFMA2_MMA => 8,
                RedirectedFP64 => 8,
                Clmad => 10,
                IMMA_88 => 11,
                MMA_1x_collect => 14,
                MMA_2x_collect => 22,
                DMMA => 23,
                Cbu => 1,
                Decoupled => 1,
                DecoupledAgu => 1,
                _ => {
                    crate::codegen::ice!("Illegal writer in sm80 raw");
                }
            },
            FP16 => match writer {
                CoupledAlu => 5,
                CoupledDisp64 => 6,
                CoupledFMA => 5,
                IMADWideWriteDL => 3,
                IMADWideWriteDH => 5,
                FP16 => 4,
                FP16_Alu => 5,
                FP16_F32 => 5,
                HFMA2_MMA => 10,
                RedirectedFP64 => 10,
                Clmad => 12,
                IMMA_88 => 13,
                MMA_1x_collect => 16,
                MMA_2x_collect => 24,
                DMMA => 25,
                Cbu => 1,
                Decoupled => 1,
                DecoupledAgu => 1,
                _ => {
                    crate::codegen::ice!("Illegal writer in sm80 raw");
                }
            },
            FP16_Alu => match writer {
                CoupledAlu => 5,
                CoupledDisp64 => 6,
                CoupledFMA => 5,
                IMADWideWriteDL => 3,
                IMADWideWriteDH => 5,
                FP16 => 5,
                FP16_Alu => 4,
                FP16_F32 => 5,
                HFMA2_MMA => 10,
                RedirectedFP64 => 10,
                Clmad => 12,
                IMMA_88 => 13,
                MMA_1x_collect => 16,
                MMA_2x_collect => 24,
                DMMA => 25,
                Cbu => 1,
                Decoupled => 1,
                DecoupledAgu => 1,
                _ => {
                    crate::codegen::ice!("Illegal writer in sm80 raw");
                }
            },
            FP16_F32 => match writer {
                CoupledAlu => 5,
                CoupledDisp64 => 6,
                CoupledFMA => 5,
                IMADWideWriteDL => 3,
                IMADWideWriteDH => 5,
                FP16 => 5,
                FP16_Alu => 5,
                FP16_F32 => 5,
                HFMA2_MMA => 10,
                RedirectedFP64 => 10,
                Clmad => 12,
                IMMA_88 => 13,
                MMA_1x_collect => 16,
                MMA_2x_collect => 24,
                DMMA => 25,
                Cbu => 1,
                Decoupled => 1,
                DecoupledAgu => 1,
                _ => {
                    crate::codegen::ice!("Illegal writer in sm80 raw");
                }
            },
            HFMA2_MMA | RedirectedFP64 => match writer {
                CoupledAlu => 6,
                CoupledDisp64 => 6,
                CoupledFMA => 6,
                IMADWideWriteDL => 6,
                IMADWideWriteDH => 6,
                FP16 => 6,
                FP16_Alu => 6,
                FP16_F32 => 6,
                HFMA2_MMA => 6,
                RedirectedFP64 => 6,
                Clmad => 12,
                IMMA_88 => 13,
                MMA_1x_collect => 16,
                MMA_2x_collect => 24,
                DMMA => 25,
                Cbu => 1,
                Decoupled => 1,
                DecoupledAgu => 1,
                _ => {
                    crate::codegen::ice!("Illegal writer in sm80 raw");
                }
            },
            Clmad => match writer {
                CoupledAlu => 6,
                CoupledDisp64 => 6,
                CoupledFMA => 6,
                IMADWideWriteDL => 6,
                IMADWideWriteDH => 6,
                FP16 => 6,
                FP16_Alu => 6,
                FP16_F32 => 6,
                HFMA2_MMA => 10,
                RedirectedFP64 => 10,
                Clmad => 8,
                IMMA_88 => 13,
                MMA_1x_collect => 16,
                MMA_2x_collect => 24,
                DMMA => 25,
                Cbu => 1,
                Decoupled => 1,
                DecoupledAgu => 1,
                _ => {
                    crate::codegen::ice!("Illegal writer in sm80 raw");
                }
            },
            IMMA_88 | MMA_1x_collect => match writer {
                CoupledAlu => 7,
                CoupledDisp64 => 7,
                CoupledFMA => 7,
                IMADWideWriteDL => 7,
                IMADWideWriteDH => 7,
                FP16 => 7,
                FP16_Alu => 7,
                FP16_F32 => 7,
                HFMA2_MMA => 11,
                RedirectedFP64 => 11,
                Clmad => 13,
                IMMA_88 => 14,
                MMA_1x_collect => 16,
                MMA_2x_collect => 24,
                DMMA => 26,
                Cbu => 1,
                Decoupled => 1,
                DecoupledAgu => 1,
                _ => {
                    crate::codegen::ice!("Illegal writer in sm80 raw");
                }
            },
            MMA_2x_collect => match writer {
                CoupledAlu => 7,
                CoupledDisp64 => 7,
                CoupledFMA => 7,
                IMADWideWriteDL => 7,
                IMADWideWriteDH => 7,
                FP16 => 7,
                FP16_Alu => 7,
                FP16_F32 => 7,
                HFMA2_MMA => 11,
                RedirectedFP64 => 11,
                Clmad => 13,
                IMMA_88 => 14,
                MMA_1x_collect => 16,
                MMA_2x_collect => 24,
                DMMA => 26,
                Cbu => 1,
                Decoupled => 1,
                DecoupledAgu => 1,
                _ => {
                    crate::codegen::ice!("Illegal writer in sm80 raw");
                }
            },
            DMMA => match writer {
                CoupledAlu => 7,
                CoupledDisp64 => 7,
                CoupledFMA => 7,
                IMADWideWriteDL => 7,
                IMADWideWriteDH => 7,
                FP16 => 7,
                FP16_Alu => 7,
                FP16_F32 => 7,
                HFMA2_MMA => 11,
                RedirectedFP64 => 11,
                Clmad => 13,
                IMMA_88 => 14,
                MMA_1x_collect => 16,
                MMA_2x_collect => 24,
                DMMA => 26,
                Cbu => 1,
                Decoupled => 1,
                DecoupledAgu => 1,
                _ => {
                    crate::codegen::ice!("Illegal writer in sm80 raw");
                }
            },
            Cbu | Decoupled => match writer {
                CoupledAlu => 4,
                CoupledDisp64 => 4,
                CoupledFMA => 4,
                IMADWideWriteDL => 4,
                IMADWideWriteDH => 4,
                FP16 => 4,
                FP16_Alu => 4,
                FP16_F32 => 4,
                HFMA2_MMA => 6,
                RedirectedFP64 => 6,
                Clmad => 8,
                IMMA_88 => 11,
                MMA_1x_collect => 14,
                MMA_2x_collect => 22,
                DMMA => 23,
                Cbu => 1,
                Decoupled => 1,
                DecoupledAgu => 1,
                _ => {
                    crate::codegen::ice!("Illegal writer in sm80 raw");
                }
            },
            DecoupledAgu => match writer {
                CoupledAlu => 5,
                CoupledDisp64 => 5,
                CoupledFMA => 5,
                IMADWideWriteDL => 5,
                IMADWideWriteDH => 5,
                FP16 => 5,
                FP16_Alu => 5,
                FP16_F32 => 5,
                HFMA2_MMA => 7,
                RedirectedFP64 => 7,
                Clmad => 9,
                IMMA_88 => 12,
                MMA_1x_collect => 15,
                MMA_2x_collect => 23,
                DMMA => 24,
                Cbu => 1,
                Decoupled => 1,
                DecoupledAgu => 1,
                _ => {
                    crate::codegen::ice!("Illegal writer in sm80 raw");
                }
            },
            CoupledDisp64 | IMADWideWriteDL | IMADWideWriteDH => {
                crate::codegen::ice!("Illegal reader in sm80 raw");
            }
        }
    }

    pub(super) fn write_after_write(writer1: Self, writer2: Self, has_pred: bool) -> u32 {
        use RegLatencySM80::*;
        match writer2 {
            CoupledAlu => match writer1 {
                CoupledDisp64 => pred(has_pred, 1, 1),
                CoupledAlu | CoupledFMA | IMADWideWriteDL | IMADWideWriteDH | FP16 | FP16_Alu
                | FP16_F32 => 1,
                HFMA2_MMA | RedirectedFP64 => pred(has_pred, 3, 3),
                Clmad => pred(has_pred, 5, 3),
                IMMA_88 => pred(has_pred, 8, 1),
                MMA_1x_collect => pred(has_pred, 11, 1),
                MMA_2x_collect => pred(has_pred, 19, 1),
                DMMA => pred(has_pred, 20, 1),
                Cbu => 1,
                Decoupled => 1,
                DecoupledAgu => 1,
                _ => {
                    crate::codegen::ice!("Illegal writer in sm80 waw");
                }
            },
            CoupledDisp64 => match writer1 {
                CoupledAlu | CoupledDisp64 | CoupledFMA | IMADWideWriteDL | IMADWideWriteDH
                | FP16 | FP16_Alu | FP16_F32 => 1,
                HFMA2_MMA | RedirectedFP64 => pred(has_pred, 3, 1),
                Clmad => pred(has_pred, 5, 1),
                IMMA_88 => 8,
                MMA_1x_collect => 11,
                MMA_2x_collect => 19,
                DMMA => 20,
                Cbu => 1,
                Decoupled => 1,
                DecoupledAgu => 1,
                _ => {
                    crate::codegen::ice!("Illegal writer in sm80 waw");
                }
            },
            CoupledFMA => match writer1 {
                CoupledDisp64 => pred(has_pred, 1, 1),
                CoupledAlu | CoupledFMA | IMADWideWriteDL | FP16 | FP16_Alu | FP16_F32 => 1,
                IMADWideWriteDH => pred(has_pred, 1, 1),
                HFMA2_MMA | RedirectedFP64 => pred(has_pred, 3, 3),
                Clmad => pred(has_pred, 5, 3),
                IMMA_88 => pred(has_pred, 8, 1),
                MMA_1x_collect => pred(has_pred, 11, 1),
                MMA_2x_collect => pred(has_pred, 19, 1),
                DMMA => pred(has_pred, 20, 1),
                Cbu => 1,
                Decoupled => 1,
                DecoupledAgu => 1,
                _ => {
                    crate::codegen::ice!("Illegal writer in sm80 waw");
                }
            },
            IMADWideWriteDL => match writer1 {
                CoupledAlu => pred(has_pred, 1, 2),
                CoupledDisp64 => pred(has_pred, 1, 3),
                CoupledFMA => pred(has_pred, 1, 1),
                IMADWideWriteDL => 1,
                IMADWideWriteDH => pred(has_pred, 1, 1),
                FP16 | FP16_Alu | FP16_F32 => pred(has_pred, 1, 2),
                HFMA2_MMA | RedirectedFP64 => pred(has_pred, 5, 3),
                Clmad => pred(has_pred, 5, 5),
                IMMA_88 => pred(has_pred, 8, 3),
                MMA_1x_collect => pred(has_pred, 11, 3),
                MMA_2x_collect => pred(has_pred, 19, 3),
                DMMA => pred(has_pred, 20, 3),
                Cbu => 1,
                Decoupled => 1,
                DecoupledAgu => 1,
                _ => {
                    crate::codegen::ice!("Illegal writer in sm80 waw");
                }
            },
            IMADWideWriteDH => match writer1 {
                CoupledAlu => 1,
                CoupledDisp64 => pred(has_pred, 1, 1),
                CoupledFMA => 1,
                IMADWideWriteDL | IMADWideWriteDH | FP16 | FP16_Alu | FP16_F32 => 1,
                HFMA2_MMA | RedirectedFP64 => pred(has_pred, 5, 1),
                Clmad => pred(has_pred, 5, 3),
                IMMA_88 => pred(has_pred, 8, 1),
                MMA_1x_collect => pred(has_pred, 11, 1),
                MMA_2x_collect => pred(has_pred, 19, 1),
                DMMA => pred(has_pred, 20, 1),
                Cbu => 1,
                Decoupled => 1,
                DecoupledAgu => 1,
                _ => {
                    crate::codegen::ice!("Illegal writer in sm80 waw");
                }
            },
            FP16 | FP16_Alu => match writer1 {
                CoupledAlu => 1,
                CoupledDisp64 => pred(has_pred, 1, 1),
                CoupledFMA => 1,
                IMADWideWriteDL | IMADWideWriteDH | FP16 | FP16_Alu | FP16_F32 => 1,
                HFMA2_MMA | RedirectedFP64 => pred(has_pred, 3, 3),
                Clmad => pred(has_pred, 5, 3),
                IMMA_88 => pred(has_pred, 8, 1),
                MMA_1x_collect => pred(has_pred, 11, 1),
                MMA_2x_collect => pred(has_pred, 19, 1),
                DMMA => pred(has_pred, 20, 1),
                Cbu => 1,
                Decoupled => 1,
                DecoupledAgu => 1,
                _ => {
                    crate::codegen::ice!("Illegal writer in sm80 waw");
                }
            },
            FP16_F32 => match writer1 {
                CoupledAlu | CoupledDisp64 | CoupledFMA | IMADWideWriteDL | IMADWideWriteDH
                | FP16 | FP16_Alu | FP16_F32 => 1,
                HFMA2_MMA | RedirectedFP64 => pred(has_pred, 3, 2),
                Clmad => pred(has_pred, 5, 2),
                IMMA_88 => 8,
                MMA_1x_collect => 11,
                MMA_2x_collect => 19,
                DMMA => 20,
                Cbu => 1,
                Decoupled => 1,
                DecoupledAgu => 1,
                _ => {
                    crate::codegen::ice!("Illegal writer in sm80 waw");
                }
            },
            HFMA2_MMA => match writer1 {
                CoupledAlu | CoupledDisp64 | CoupledFMA | IMADWideWriteDL | IMADWideWriteDH
                | FP16 | FP16_Alu | FP16_F32 => 1,
                HFMA2_MMA => 2,
                RedirectedFP64 => 3,
                Clmad => pred(has_pred, 5, 1),
                IMMA_88 => 8,
                MMA_1x_collect => 11,
                MMA_2x_collect => 19,
                DMMA => 20,
                Cbu => 1,
                Decoupled => 1,
                DecoupledAgu => 1,
                _ => {
                    crate::codegen::ice!("Illegal writer in sm80 waw");
                }
            },
            RedirectedFP64 => match writer1 {
                CoupledAlu | CoupledDisp64 | CoupledFMA | IMADWideWriteDL | IMADWideWriteDH
                | FP16 | FP16_Alu | FP16_F32 => 1,
                HFMA2_MMA => 2,
                RedirectedFP64 => 2,
                Clmad => pred(has_pred, 4, 2),
                IMMA_88 => 7,
                MMA_1x_collect => 10,
                MMA_2x_collect => 18,
                DMMA => 19,
                Cbu => 1,
                Decoupled => 1,
                DecoupledAgu => 1,
                _ => {
                    crate::codegen::ice!("Illegal writer in sm80 waw");
                }
            },
            Clmad => match writer1 {
                CoupledAlu | CoupledDisp64 | CoupledFMA | IMADWideWriteDL | IMADWideWriteDH
                | FP16 | FP16_Alu | FP16_F32 | HFMA2_MMA | RedirectedFP64 | Clmad => 2,
                IMMA_88 => 7,
                MMA_1x_collect => 10,
                MMA_2x_collect => 18,
                DMMA => 19,
                Cbu => 1,
                Decoupled => 1,
                DecoupledAgu => 1,
                _ => {
                    crate::codegen::ice!("Illegal writer in sm80 waw");
                }
            },
            IMMA_88 | MMA_1x_collect => match writer1 {
                CoupledAlu | CoupledDisp64 | CoupledFMA | IMADWideWriteDL | IMADWideWriteDH
                | FP16 | FP16_Alu | FP16_F32 | HFMA2_MMA | RedirectedFP64 | Clmad => 2,
                IMMA_88 => 4,
                MMA_1x_collect => 8,
                MMA_2x_collect => 16,
                DMMA => 17,
                Cbu => 1,
                Decoupled => 1,
                DecoupledAgu => 1,
                _ => {
                    crate::codegen::ice!("Illegal writer in sm80 waw");
                }
            },
            MMA_2x_collect | DMMA => match writer1 {
                CoupledAlu | CoupledDisp64 | CoupledFMA | IMADWideWriteDL | IMADWideWriteDH
                | FP16 | FP16_Alu | FP16_F32 | HFMA2_MMA | RedirectedFP64 | Clmad => 2,
                IMMA_88 => 4,
                MMA_1x_collect => 8,
                MMA_2x_collect => 16,
                DMMA => 16,
                Cbu => 1,
                Decoupled => 1,
                DecoupledAgu => 1,
                _ => {
                    crate::codegen::ice!("Illegal writer in sm80 waw");
                }
            },
            Cbu | Decoupled | DecoupledAgu => match writer1 {
                CoupledAlu | CoupledDisp64 | CoupledFMA | IMADWideWriteDL | IMADWideWriteDH
                | FP16 | FP16_Alu | FP16_F32 => pred(has_pred, 1, 5),
                HFMA2_MMA | RedirectedFP64 => pred(has_pred, 1, 9),
                Clmad => pred(has_pred, 1, 11),
                IMMA_88 => pred(has_pred, 7, 6),
                MMA_1x_collect => pred(has_pred, 10, 5),
                MMA_2x_collect => pred(has_pred, 18, 5),
                DMMA => pred(has_pred, 19, 6),
                Cbu => 1,
                Decoupled => 1,
                DecoupledAgu => 1,
                _ => {
                    crate::codegen::ice!("Illegal writer in sm80 waw");
                }
            },
            _ => {
                crate::codegen::ice!("Illegal writer in sm80 waw");
            }
        }
    }

    pub(super) fn write_after_read(reader: Self, writer: Self) -> u32 {
        use RegLatencySM80::*;
        match writer {
            CoupledAlu | CoupledDisp64 | CoupledFMA | IMADWideWriteDL | IMADWideWriteDH | FP16
            | FP16_Alu | FP16_F32 | HFMA2_MMA => match reader {
                MMA_2x_collect => 7,
                _ => 1,
            },
            RedirectedFP64 => 1,
            Clmad | IMMA_88 | MMA_1x_collect | MMA_2x_collect | DMMA | Decoupled | DecoupledAgu => {
                match reader {
                    CoupledAlu | CoupledDisp64 | CoupledFMA | IMADWideReadAB | IMADWideReadCL
                    | IMADWideReadCH | FP16 | FP16_Alu | FP16_F32 | HFMA2_MMA => 2,
                    _ => 1,
                }
            }
            Cbu => match reader {
                CoupledAlu | CoupledDisp64 | CoupledFMA | IMADWideReadAB | IMADWideReadCL
                | IMADWideReadCH | FP16 | FP16_Alu | FP16_F32 | HFMA2_MMA => 2,
                MMA_2x_collect => 7,
                _ => 1,
            },
            _ => {
                crate::codegen::ice!("Illegal writer in sm80 war");
            }
        }
    }
}
