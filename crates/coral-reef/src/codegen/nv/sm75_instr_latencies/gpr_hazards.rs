// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright © 2025-2026 ecoPrimals
// Derived from Red Hat. (2025)
//! WAW, WAR, and predicate-register hazard latency tables for SM75+.
//!
//! Split from `gpr` for code size compliance. Each function maps
//! (writer/reader category × writer/reader category) → minimum stall
//! cycles required to avoid hardware data hazards.

use super::gpr::RegLatencySM75;
use super::pred;

impl RegLatencySM75 {
    pub(in crate::codegen::nv::sm75_instr_latencies) fn write_after_write(
        writer1: Self,
        writer2: Self,
        has_pred: bool,
    ) -> u32 {
        use RegLatencySM75::*;
        match writer1 {
            IMADWideAB | DecoupledOther => {
                crate::codegen::ice!("Illegal reg latency for writer");
            }
            _ => {}
        }
        match writer2 {
            CoupledDisp64 => match writer1 {
                CoupledDisp64 | CoupledDisp | CoupledAlu | CoupledFMA | IMADLo | IMADWideLower
                | IMADWideUpper => 1,
                RedirectedFP64 => 4,
                RedirectedFP16 => 3,
                RedirectedHMMA_884_F16(_) => 8,
                RedirectedHMMA_884_F32(_) => pred(has_pred, 2, 2),
                RedirectedHMMA_1688 => 9,
                RedirectedHMMA_16816 => 17,
                IMMA(_) => 5,
                _ => 1,
            },
            CoupledDisp | CoupledAlu => match writer1 {
                CoupledDisp64 => 2,
                CoupledDisp | CoupledAlu | CoupledFMA | IMADLo | IMADWideLower | IMADWideUpper => 1,
                RedirectedFP64 => pred(has_pred, 4, 1),
                RedirectedFP16 => pred(has_pred, 3, 1),
                RedirectedHMMA_884_F16(_) => pred(has_pred, 8, 1),
                RedirectedHMMA_884_F32(_) => pred(has_pred, 5, 1),
                RedirectedHMMA_1688 => pred(has_pred, 9, 1),
                RedirectedHMMA_16816 => pred(has_pred, 17, 1),
                IMMA(_) => pred(has_pred, 5, 1),
                _ => 1,
            },
            CoupledFMA | IMADLo => match writer1 {
                CoupledDisp64 => 2,
                CoupledDisp | CoupledAlu | CoupledFMA | IMADLo | IMADWideLower => 1,
                IMADWideUpper => pred(has_pred, 1, 1),
                RedirectedFP64 => pred(has_pred, 4, 1),
                RedirectedFP16 => pred(has_pred, 3, 1),
                RedirectedHMMA_884_F16(_) => pred(has_pred, 8, 1),
                RedirectedHMMA_884_F32(_) => pred(has_pred, 5, 1),
                RedirectedHMMA_1688 => pred(has_pred, 9, 1),
                RedirectedHMMA_16816 => pred(has_pred, 17, 1),
                IMMA(_) => pred(has_pred, 5, 1),
                _ => 1,
            },
            IMADWideLower => match writer1 {
                CoupledDisp64 => pred(has_pred, 2, 2),
                CoupledDisp | CoupledAlu => pred(has_pred, 2, 1),
                CoupledFMA | IMADLo => pred(has_pred, 1, 1),
                IMADWideLower => 1,
                IMADWideUpper => 1,
                RedirectedFP64 => pred(has_pred, 4, 3),
                RedirectedFP16 => pred(has_pred, 3, 3),
                RedirectedHMMA_884_F16(_) => pred(has_pred, 8, 3),
                RedirectedHMMA_884_F32(_) => pred(has_pred, 5, 3),
                RedirectedHMMA_1688 => pred(has_pred, 9, 3),
                RedirectedHMMA_16816 => pred(has_pred, 17, 3),
                IMMA(_) => pred(has_pred, 5, 3),
                _ => 1,
            },
            IMADWideUpper => match writer1 {
                CoupledDisp64 => pred(has_pred, 1, 1),
                CoupledDisp | CoupledAlu | CoupledFMA | IMADLo | IMADWideLower | IMADWideUpper => 1,
                RedirectedFP64 => pred(has_pred, 4, 1),
                RedirectedFP16 => pred(has_pred, 3, 1),
                RedirectedHMMA_884_F16(_) => pred(has_pred, 8, 1),
                RedirectedHMMA_884_F32(_) => pred(has_pred, 5, 1),
                RedirectedHMMA_1688 => pred(has_pred, 9, 1),
                RedirectedHMMA_16816 => pred(has_pred, 17, 1),
                IMMA(_) => pred(has_pred, 5, 1),
                _ => 1,
            },
            RedirectedFP64 => match writer1 {
                CoupledDisp64 | CoupledDisp | CoupledAlu | CoupledFMA | IMADLo | IMADWideLower
                | IMADWideUpper => 2,
                RedirectedFP64 => 1,
                RedirectedFP16 => 2,
                RedirectedHMMA_884_F16(_) => 5,
                RedirectedHMMA_884_F32(_) => 2,
                RedirectedHMMA_1688 => 6,
                RedirectedHMMA_16816 => 14,
                IMMA(_) => 2,
                _ => 1,
            },
            RedirectedFP16 => match writer1 {
                CoupledDisp64 | CoupledDisp | CoupledAlu | CoupledFMA | IMADLo | IMADWideLower
                | IMADWideUpper => 2,
                RedirectedFP64 => pred(has_pred, 1, 1),
                RedirectedFP16 => 1,
                RedirectedHMMA_884_F16(_) => pred(has_pred, 6, 1),
                RedirectedHMMA_884_F32(_) => pred(has_pred, 3, 1),
                RedirectedHMMA_1688 => pred(has_pred, 7, 1),
                RedirectedHMMA_16816 => pred(has_pred, 15, 1),
                IMMA(_) => pred(has_pred, 3, 1),
                _ => 1,
            },
            RedirectedHMMA_884_F16(_) => match writer1 {
                CoupledDisp64 | CoupledDisp | CoupledAlu | CoupledFMA | IMADLo | IMADWideLower
                | IMADWideUpper => 2,
                RedirectedFP64 => pred(has_pred, 3, 2),
                RedirectedFP16 => pred(has_pred, 2, 2),
                RedirectedHMMA_884_F16(_) => 1,
                RedirectedHMMA_884_F32(_) => pred(has_pred, 2, 4),
                RedirectedHMMA_1688 => pred(has_pred, 6, 4),
                RedirectedHMMA_16816 => pred(has_pred, 16, 2),
                IMMA(_) => pred(has_pred, 2, 4),
                _ => 1,
            },
            RedirectedHMMA_884_F32(_) => match writer1 {
                CoupledDisp64 | CoupledDisp | CoupledAlu | CoupledFMA | IMADLo | IMADWideLower
                | IMADWideUpper => 2,
                RedirectedFP64 => pred(has_pred, 3, 2),
                RedirectedFP16 => pred(has_pred, 2, 2),
                RedirectedHMMA_884_F16(_) => pred(has_pred, 4, 5),
                RedirectedHMMA_884_F32(_) => 1,
                RedirectedHMMA_1688 => pred(has_pred, 6, 4),
                RedirectedHMMA_16816 => pred(has_pred, 16, 2),
                IMMA(_) => pred(has_pred, 2, 4),
                _ => 1,
            },
            RedirectedHMMA_1688 => match writer1 {
                CoupledDisp64 | CoupledDisp | CoupledAlu | CoupledFMA | IMADLo | IMADWideLower
                | IMADWideUpper | RedirectedFP64 | RedirectedFP16 => 2,
                RedirectedHMMA_884_F16(_) => 4,
                RedirectedHMMA_884_F32(_) => 2,
                RedirectedHMMA_1688 => 1,
                RedirectedHMMA_16816 => 16,
                IMMA(_) => 2,
                _ => 1,
            },
            RedirectedHMMA_16816 => match writer1 {
                CoupledDisp64 | CoupledDisp | CoupledAlu | CoupledFMA | IMADLo | IMADWideLower
                | IMADWideUpper | RedirectedFP64 | RedirectedFP16 => 2,
                RedirectedHMMA_884_F16(_) => 4,
                RedirectedHMMA_884_F32(_) => 2,
                RedirectedHMMA_1688 => 6,
                RedirectedHMMA_16816 => 1,
                IMMA(_) => 2,
                _ => 1,
            },
            IMMA(_) => match writer1 {
                CoupledDisp64 | CoupledDisp | CoupledAlu | CoupledFMA | IMADLo | IMADWideLower
                | IMADWideUpper => pred(has_pred, 2, 2),
                RedirectedFP64 => pred(has_pred, 2, 3),
                RedirectedFP16 => pred(has_pred, 2, 2),
                RedirectedHMMA_884_F16(_) => pred(has_pred, 2, 7),
                RedirectedHMMA_884_F32(_) => pred(has_pred, 2, 4),
                RedirectedHMMA_1688 => pred(has_pred, 6, 4),
                RedirectedHMMA_16816 => pred(has_pred, 14, 4),
                IMMA(_) => 1,
                _ => 1,
            },
            Decoupled => match writer1 {
                CoupledDisp64
                | CoupledDisp
                | CoupledAlu
                | CoupledFMA
                | IMADLo
                | IMADWideLower
                | IMADWideUpper
                | RedirectedFP64
                | RedirectedFP16
                | RedirectedHMMA_884_F16(_)
                | RedirectedHMMA_884_F32(_)
                | RedirectedHMMA_1688 => 6,
                RedirectedHMMA_16816 => 14,
                IMMA(_) => 2,
                _ => 1,
            },
            BMov => match writer1 {
                CoupledDisp64
                | CoupledDisp
                | CoupledAlu
                | CoupledFMA
                | IMADLo
                | IMADWideLower
                | IMADWideUpper
                | RedirectedFP64
                | RedirectedFP16
                | RedirectedHMMA_884_F16(_)
                | RedirectedHMMA_884_F32(_)
                | RedirectedHMMA_1688 => 9,
                RedirectedHMMA_16816 => 14,
                IMMA(_) => 9,
                _ => 1,
            },
            IMADWideAB | DecoupledOther | GuardPredicate => {
                crate::codegen::ice!("Not a WAW category")
            }
        }
    }

    pub(in crate::codegen::nv::sm75_instr_latencies) fn write_after_read(
        reader: Self,
        writer: Self,
    ) -> u32 {
        use RegLatencySM75::*;
        match writer {
            CoupledDisp64 | CoupledDisp | CoupledAlu | CoupledFMA | IMADLo | IMADWideLower
            | IMADWideUpper => match reader {
                RedirectedHMMA_1688 => 5,
                RedirectedHMMA_16816 => 13,
                _ => 1,
            },
            RedirectedFP64 => match reader {
                RedirectedFP64 => 1,
                RedirectedHMMA_1688 => 6,
                RedirectedHMMA_16816 => 14,
                Decoupled => 1,
                _ => 2,
            },
            RedirectedFP16 => match reader {
                RedirectedFP16 => 1,
                RedirectedHMMA_1688 => 6,
                RedirectedHMMA_16816 => 14,
                Decoupled => 1,
                _ => 2,
            },
            RedirectedHMMA_884_F16(_) => match reader {
                RedirectedHMMA_884_F16(_) => 1,
                RedirectedHMMA_1688 => 6,
                RedirectedHMMA_16816 => 14,
                Decoupled => 1,
                _ => 2,
            },
            RedirectedHMMA_884_F32(_) => match reader {
                RedirectedHMMA_884_F32(_) => 1,
                RedirectedHMMA_1688 => 6,
                RedirectedHMMA_16816 => 14,
                Decoupled => 1,
                _ => 2,
            },
            RedirectedHMMA_1688 => match reader {
                RedirectedHMMA_1688 => 1,
                RedirectedHMMA_16816 => 14,
                Decoupled => 1,
                _ => 2,
            },
            RedirectedHMMA_16816 => match reader {
                RedirectedHMMA_1688 => 6,
                RedirectedHMMA_16816 => 1,
                Decoupled => 1,
                _ => 2,
            },
            IMMA(_) => match reader {
                RedirectedHMMA_1688 => 6,
                RedirectedHMMA_16816 => 14,
                IMMA(_) => 1,
                Decoupled => 1,
                _ => 2,
            },
            Decoupled => match reader {
                RedirectedHMMA_1688 => 2,
                RedirectedHMMA_16816 => 14,
                Decoupled => 1,
                _ => 2,
            },
            BMov => match reader {
                RedirectedHMMA_1688 => 9,
                RedirectedHMMA_16816 => 14,
                Decoupled => 1,
                _ => 9,
            },
            IMADWideAB | DecoupledOther | GuardPredicate => {
                crate::codegen::ice!("Illegal in WAR");
            }
        }
    }

    pub(in crate::codegen::nv::sm75_instr_latencies) fn pred_read_after_write(
        writer: Self,
        reader: Self,
    ) -> u32 {
        use RegLatencySM75::*;
        match reader {
            CoupledDisp => match writer {
                CoupledDisp | CoupledAlu | CoupledFMA | IMADLo | IMADWideUpper | IMADWideLower => {
                    12
                }
                RedirectedFP64 => 15,
                RedirectedFP16 => 14,
                Decoupled => 1,
                _ => {
                    crate::codegen::ice!("Illegal RAW in Predicate");
                }
            },
            CoupledAlu => match writer {
                CoupledDisp | CoupledAlu => 4,
                CoupledFMA | IMADLo | IMADWideUpper | IMADWideLower => 5,
                RedirectedFP64 => 9,
                RedirectedFP16 => 8,
                Decoupled => 1,
                _ => {
                    crate::codegen::ice!("Illegal RAW in Predicate");
                }
            },
            CoupledFMA | IMADLo => match writer {
                CoupledDisp | CoupledAlu => 5,
                CoupledFMA | IMADLo | IMADWideUpper | IMADWideLower => 4,
                RedirectedFP64 => 9,
                RedirectedFP16 => 8,
                Decoupled => 1,
                _ => {
                    crate::codegen::ice!("Illegal RAW in Predicate");
                }
            },
            IMADWideUpper | IMADWideLower => match writer {
                CoupledDisp | CoupledAlu => 5,
                CoupledFMA | IMADLo => 4,
                IMADWideUpper | IMADWideLower => 2,
                RedirectedFP64 => 9,
                RedirectedFP16 => 8,
                Decoupled => 1,
                _ => {
                    crate::codegen::ice!("Illegal RAW in Predicate");
                }
            },
            RedirectedFP64 => match writer {
                CoupledDisp | CoupledAlu | CoupledFMA | IMADLo | IMADWideUpper | IMADWideLower => {
                    12
                }
                RedirectedFP64 => 8,
                RedirectedFP16 => 14,
                Decoupled => 1,
                _ => {
                    crate::codegen::ice!("Illegal RAW in Predicate");
                }
            },
            RedirectedFP16 => match writer {
                CoupledDisp | CoupledAlu | CoupledFMA | IMADLo | IMADWideUpper | IMADWideLower => {
                    12
                }
                RedirectedFP64 => 15,
                RedirectedFP16 => 6,
                Decoupled => 1,
                _ => {
                    crate::codegen::ice!("Illegal RAW in Predicate");
                }
            },
            Decoupled | GuardPredicate => match writer {
                CoupledDisp | CoupledAlu | CoupledFMA | IMADLo | IMADWideUpper | IMADWideLower => {
                    12
                }
                RedirectedFP64 => 15,
                RedirectedFP16 => 14,
                Decoupled => 1,
                _ => {
                    crate::codegen::ice!("Illegal RAW in Predicate");
                }
            },
            _ => {
                crate::codegen::ice!("Illegal reader in reg predicate");
            }
        }
    }

    pub(in crate::codegen::nv::sm75_instr_latencies) fn pred_write_after_write(
        writer1: Self,
        writer2: Self,
        has_pred: bool,
    ) -> u32 {
        use RegLatencySM75::*;
        match writer2 {
            CoupledDisp | CoupledAlu | CoupledFMA | IMADLo => match writer1 {
                CoupledDisp | CoupledAlu | CoupledFMA | IMADLo | IMADWideUpper | IMADWideLower => 1,
                RedirectedFP64 => pred(has_pred, 4, 1),
                RedirectedFP16 => pred(has_pred, 3, 1),
                Decoupled => 1,
                _ => {
                    crate::codegen::ice!("Illegal RAW in Predicate");
                }
            },
            IMADWideUpper | IMADWideLower => match writer1 {
                CoupledDisp | CoupledAlu => pred(has_pred, 1, 2),
                CoupledFMA | IMADLo => pred(has_pred, 1, 1),
                IMADWideUpper | IMADWideLower => 1,
                RedirectedFP64 => pred(has_pred, 4, 3),
                RedirectedFP16 => pred(has_pred, 3, 3),
                Decoupled => 1,
                _ => {
                    crate::codegen::ice!("Illegal RAW in Predicate");
                }
            },
            RedirectedFP64 => match writer1 {
                CoupledDisp | CoupledAlu | CoupledFMA | IMADLo | IMADWideUpper | IMADWideLower => {
                    pred(has_pred, 2, 2)
                }
                RedirectedFP64 => 1,
                RedirectedFP16 => pred(has_pred, 2, 4),
                Decoupled => 1,
                _ => {
                    crate::codegen::ice!("Illegal RAW in Predicate");
                }
            },
            RedirectedFP16 => match writer1 {
                CoupledDisp | CoupledAlu | CoupledFMA | IMADLo | IMADWideUpper | IMADWideLower => {
                    pred(has_pred, 2, 4)
                }
                RedirectedFP64 => pred(has_pred, 2, 7),
                RedirectedFP16 => 1,
                Decoupled => 1,
                _ => {
                    crate::codegen::ice!("Illegal RAW in Predicate");
                }
            },
            Decoupled => match writer1 {
                CoupledDisp | CoupledAlu | CoupledFMA | IMADLo | IMADWideUpper | IMADWideLower
                | RedirectedFP64 | RedirectedFP16 => 2,
                Decoupled => 1,
                _ => {
                    crate::codegen::ice!("Illegal RAW in Predicate");
                }
            },
            _ => {
                crate::codegen::ice!("Illegal WAR category in Predicates");
            }
        }
    }

    pub(in crate::codegen::nv::sm75_instr_latencies) fn pred_write_after_read(
        reader: Self,
        writer: Self,
    ) -> u32 {
        use RegLatencySM75::*;
        match writer {
            CoupledDisp | CoupledAlu | CoupledFMA | IMADLo | IMADWideUpper | IMADWideLower => 1,
            RedirectedFP64 => match reader {
                CoupledAlu | CoupledFMA | IMADLo | IMADWideUpper | IMADWideLower
                | RedirectedFP16 => 2,
                _ => 1,
            },
            RedirectedFP16 => match reader {
                CoupledAlu | CoupledFMA | IMADLo | IMADWideUpper | IMADWideLower
                | RedirectedFP64 => 2,
                _ => 1,
            },
            Decoupled => match reader {
                CoupledAlu | CoupledFMA | IMADLo | IMADWideUpper | IMADWideLower
                | RedirectedFP16 | RedirectedFP64 => 2,
                _ => 1,
            },
            _ => {
                crate::codegen::ice!("Illegal WAR category in Predicates");
            }
        }
    }
}
