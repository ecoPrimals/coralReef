// SPDX-License-Identifier: AGPL-3.0-only
// Copyright © 2025-2026 ecoPrimals
// Derived from Red Hat. (2025)
#![allow(non_camel_case_types, clippy::wildcard_imports, clippy::enum_glob_use)]

use super::super::sm75_instr_latencies::pred;
use crate::codegen::ir::*;

#[allow(dead_code, reason = "latency model for future SM target support")]
#[derive(Debug, Clone, Copy)]
pub(super) enum RegLatencySM80 {
    CoupledAlu,
    CoupledDisp64,
    CoupledFMA,
    IMADWideReadAB,
    IMADWideReadCL,
    IMADWideReadCH,
    IMADWideWriteDL,
    IMADWideWriteDH,
    FP16,
    FP16_Alu,
    FP16_F32,
    HFMA2_MMA,
    RedirectedFP64,
    Clmad,
    IMMA_88,
    MMA_1x_collect,
    MMA_2x_collect,
    DMMA,
    Cbu,
    Decoupled,
    DecoupledAgu,
}

impl RegLatencySM80 {
    pub(super) fn op_category(op: &Op, reader: bool, op_reg_idx: usize) -> Self {
        use RegLatencySM80::*;
        match op {
            // this will need updating if imad grows support for input predicates
            Op::IMad(_) | Op::IMul(_) => CoupledFMA,
            Op::IMad64(_) => {
                if reader {
                    match op_reg_idx {
                        0 | 1 => IMADWideReadAB,
                        2 => IMADWideReadCL, // vs upper C operand - work it out
                        _ => {
                            panic!("Illegal field in imadwide")
                        }
                    }
                } else {
                    IMADWideWriteDH // as above this needs more work
                }
            }

            Op::PopC(_)
            | Op::Flo(_)
            | Op::Transcendental(_)
            | Op::F2F(_)
            | Op::F2I(_)
            | Op::I2F(_)
            | Op::FRnd(_)
            | Op::AL2P(_)
            | Op::BRev(_)
            | Op::Match(_)
            | Op::S2R(_)
            | Op::BClear(_)
            | Op::Bra(_)
            | Op::BSSy(_)
            | Op::Kill(_)
            | Op::Exit(_)
            | Op::BSync(_)
            | Op::Tex(_)
            | Op::Tld(_)
            | Op::Tld4(_)
            | Op::Tmml(_)
            | Op::Txd(_)
            | Op::Txq(_)
            | Op::Ldc(_)
            | Op::MemBar(_)
            | Op::SuLd(_)
            | Op::SuSt(_)
            | Op::SuAtom(_) => Decoupled,
            Op::IAdd3(_) | Op::IAdd3X(_) => CoupledAlu,

            Op::BMsk(_)
            | Op::Sgxt(_)
            | Op::Lop3(_)
            | Op::ISetP(_)
            | Op::IAbs(_)
            | Op::Lea(_)
            | Op::LeaX(_)
            | Op::IMnMx(_)
            | Op::I2I(_)
            | Op::Shf(_)
            | Op::F2FP(_)
            | Op::FMnMx(_)
            | Op::FSet(_)
            | Op::FSetP(_)
            | Op::Mov(_)
            | Op::Sel(_)
            | Op::PLop3(_)
            | Op::Prmt(_)
            | Op::Vote(_) => CoupledAlu,

            Op::FFma(_) | Op::FAdd(_) | Op::FMul(_) | Op::FSwzAdd(_) | Op::IDp4(_) => CoupledFMA,
            Op::DAdd(_) | Op::DFma(_) | Op::DMul(_) | Op::DSetP(_) | Op::DMnMx(_) => RedirectedFP64, // DMnMx not in docs

            Op::HAdd2(hadd2) => {
                if hadd2.f32 {
                    FP16_F32
                } else {
                    FP16
                }
            }
            Op::HFma2(_) | Op::HMul2(_) => FP16,

            Op::HSet2(_) | Op::HSetP2(_) | Op::HMnMx2(_) => FP16_Alu,
            // let in for documentation purposes
            Op::Hmma(h) => match (h.mat_size, h.dst_type, h.src_type) {
                // (HmmaSize::M16N8K8, FloatType::F32, FloatType::TF32) => MMA_2x_collect,
                (HmmaSize::M16N8K8, FloatType::F32, FloatType::F16)
                | (HmmaSize::M16N8K8, FloatType::F16, _) => MMA_1x_collect,
                (HmmaSize::M16N8K16, _, _) => MMA_2x_collect,
                _ => panic!("Illegal HMMA in reg category {h}"),
            },
            Op::Ipa(_)
            | Op::Movm(_)
            | Op::Bar(_)
            | Op::ALd(_)
            | Op::ASt(_)
            | Op::Out(_)
            | Op::OutFinal(_)
            | Op::Ld(_)
            | Op::St(_)
            | Op::Atom(_)
            | Op::CCtl(_)
            | Op::PixLd(_)
            | Op::Isberd(_)
            | Op::LdTram(_)
            | Op::Shfl(_)
            | Op::Ldsm(_) => DecoupledAgu,
            // S2UR  => Decoupled,
            Op::R2UR(_) | Op::Redux(_) => {
                if reader {
                    Decoupled
                } else {
                    panic!("Illegal R2UR");
                }
            }
            Op::CS2R(cs2r) => {
                if cs2r.dst.comps() == 2 {
                    CoupledDisp64
                } else {
                    CoupledAlu
                }
            }
            // B2R => DecoupledAgu,
            // LEPC => CoupledDisp64
            Op::BMov(_) => Cbu,
            Op::Nop(_) => CoupledDisp64,
            Op::Imma(i) => match (i.mat_size, i.src_types[0]) {
                (ImmaSize::M16N8K64, _) | (ImmaSize::M16N8K32, IntType::I8 | IntType::U8) => {
                    MMA_2x_collect
                }
                (ImmaSize::M16N8K16, _) => MMA_1x_collect,
                (ImmaSize::M8N8K32 | ImmaSize::M8N8K16, _) => IMMA_88,
                _ => panic!("Illegal IMMA in reg category {i}"),
            },
            x => {
                panic!("Illegal instuction in reg category {x}");
            }
        }
    }

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
                    panic!("Illegal writer in sm80 raw");
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
                    panic!("Illegal writer in sm80 raw");
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
                    panic!("Illegal writer in sm80 raw");
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
                    panic!("Illegal writer in sm80 raw");
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
                    panic!("Illegal writer in sm80 raw");
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
                    panic!("Illegal writer in sm80 raw");
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
                    panic!("Illegal writer in sm80 raw");
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
                    panic!("Illegal writer in sm80 raw");
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
                    panic!("Illegal writer in sm80 raw");
                }
            },
            IMMA_88 | MMA_1x_collect => {
                match writer {
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
                    IMMA_88 => 14,        //6??
                    MMA_1x_collect => 16, //6??
                    MMA_2x_collect => 24,
                    DMMA => 26,
                    Cbu => 1,
                    Decoupled => 1,
                    DecoupledAgu => 1,
                    _ => {
                        panic!("Illegal writer in sm80 raw");
                    }
                }
            }
            MMA_2x_collect => {
                match writer {
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
                    MMA_2x_collect => 24, //10??
                    DMMA => 26,
                    Cbu => 1,
                    Decoupled => 1,
                    DecoupledAgu => 1,
                    _ => {
                        panic!("Illegal writer in sm80 raw");
                    }
                }
            }
            DMMA => {
                match writer {
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
                    DMMA => 26, //18??
                    Cbu => 1,
                    Decoupled => 1,
                    DecoupledAgu => 1,
                    _ => {
                        panic!("Illegal writer in sm80 raw");
                    }
                }
            }
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
                    panic!("Illegal writer in sm80 raw");
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
                    panic!("Illegal writer in sm80 raw");
                }
            },
            CoupledDisp64 | IMADWideWriteDL | IMADWideWriteDH => {
                panic!("Illegal reader in sm80 raw");
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
                    panic!("Illegal writer in sm80 waw");
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
                    panic!("Illegal writer in sm80 waw");
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
                    panic!("Illegal writer in sm80 waw");
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
                    panic!("Illegal writer in sm80 waw");
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
                    panic!("Illegal writer in sm80 waw");
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
                    panic!("Illegal writer in sm80 waw");
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
                    panic!("Illegal writer in sm80 waw");
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
                    panic!("Illegal writer in sm80 waw");
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
                    panic!("Illegal writer in sm80 waw");
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
                    panic!("Illegal writer in sm80 waw");
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
                    panic!("Illegal writer in sm80 waw");
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
                    panic!("Illegal writer in sm80 waw");
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
                    panic!("Illegal writer in sm80 waw");
                }
            },
            _ => {
                panic!("Illegal writer in sm80 waw");
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
                panic!("Illegal writer in sm80 war");
            }
        }
    }
}

#[cfg(test)]
#[path = "gpr_tests.rs"]
mod tests;
