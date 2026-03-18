// SPDX-License-Identifier: AGPL-3.0-only
// Copyright © 2025-2026 ecoPrimals
// Derived from Red Hat. (2025)
#![expect(
    non_camel_case_types,
    reason = "latency model mirrors hardware naming from Red Hat spec"
)]

use super::pred;
use crate::codegen::ir::*;

#[derive(Debug, Clone, Copy)]
pub(super) enum RegLatencySM75 {
    CoupledDisp64,
    CoupledDisp,
    CoupledAlu,
    CoupledFMA,
    IMADLo,
    IMADWideAB, // readers only
    IMADWideLower,
    IMADWideUpper,
    RedirectedFP64,
    RedirectedFP16,
    RedirectedHMMA_884_F16(usize),
    RedirectedHMMA_884_F32(usize),
    RedirectedHMMA_1688,
    RedirectedHMMA_16816,
    IMMA(usize),
    Decoupled,
    DecoupledOther, //reads only
    BMov,
    GuardPredicate,
}

impl RegLatencySM75 {
    pub(super) fn op_category(op: &Op, reader: bool, op_reg_idx: usize) -> Self {
        use RegLatencySM75::*;
        match op {
            // this will need updating if imad grows support for input predicates
            Op::IMad(_) | Op::IMul(_) => IMADLo,
            Op::IMad64(_) => {
                if reader {
                    match op_reg_idx {
                        0 | 1 => IMADWideAB,
                        2 => IMADWideLower, // vs upper C operand - work it out
                        _ => {
                            panic!("Illegal field in imadwide")
                        }
                    }
                } else {
                    IMADWideUpper // as above this needs more work
                }
            }

            Op::PopC(_)
            | Op::Flo(_)
            | Op::Ipa(_)
            | Op::Transcendental(_)
            | Op::F2F(_)
            | Op::F2I(_)
            | Op::I2F(_)
            | Op::FRnd(_)
            | Op::AL2P(_)
            | Op::Movm(_)
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
            | Op::ALd(_)
            | Op::ASt(_)
            | Op::Out(_)
            | Op::OutFinal(_)
            | Op::Ld(_)
            | Op::St(_)
            | Op::Atom(_)
            | Op::MemBar(_)
            | Op::SuLd(_)
            | Op::SuSt(_)
            | Op::SuAtom(_)
            | Op::PixLd(_)
            | Op::Isberd(_)
            | Op::LdTram(_)
            | Op::Shfl(_)
            | Op::Ldsm(_)
            | Op::Bar(_) => Decoupled,
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
            | Op::Prmt(_) => CoupledAlu,

            Op::FFma(_) | Op::FAdd(_) | Op::FMul(_) | Op::FSwzAdd(_) | Op::IDp4(_) => CoupledFMA,
            Op::DAdd(_) | Op::DFma(_) | Op::DMul(_) | Op::DSetP(_) | Op::DMnMx(_) => RedirectedFP64, // DMnMx not in docs

            Op::HAdd2(_)
            | Op::HFma2(_)
            | Op::HMul2(_)
            | Op::HSet2(_)
            | Op::HSetP2(_)
            | Op::HMnMx2(_) => RedirectedFP16, // HMnMx2 not in docs
            // let in for documentation purposes
            Op::Hmma(h) => match (h.mat_size, h.dst_type) {
                (HmmaSize::M16N8K8, _) => RedirectedHMMA_1688,
                (HmmaSize::M16N8K16, _) => RedirectedHMMA_16816,
                _ => panic!("Illegal HMMA in reg category {h}"),
            },
            // S2UR  => Decoupled,
            Op::R2UR(_) => {
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
            // B2R => Decoupled,
            // LEPC => CoupledDisp64
            Op::BMov(bmov) => match bmov.dst {
                Dst::Reg(reg) => {
                    if reg.is_gpr() {
                        BMov
                    } else {
                        Decoupled
                    }
                }
                _ => Decoupled,
            },
            Op::Nop(_) | Op::Vote(_) => CoupledDisp,
            Op::CCtl(_) => DecoupledOther,
            Op::Imma(_) => IMMA(op_reg_idx),
            x => {
                panic!("Illegal instuction in reg category {x}");
            }
        }
    }

    pub(super) fn read_after_write(writer: Self, reader: Self) -> u32 {
        use RegLatencySM75::*;
        match writer {
            IMADWideAB | DecoupledOther => {
                panic!("Illegal IMADWideAB for writer");
            }
            _ => {}
        }

        match reader {
            CoupledDisp64 | CoupledDisp | CoupledAlu => match writer {
                CoupledDisp64 => 6,
                CoupledAlu | CoupledDisp => 4,
                CoupledFMA | IMADLo => 5,
                IMADWideLower => 3,
                IMADWideUpper => 5,
                RedirectedFP64 => 9,
                RedirectedFP16 => 8,
                RedirectedHMMA_884_F16(_) => 13,
                RedirectedHMMA_884_F32(_) => 10,
                RedirectedHMMA_1688 => 14,
                RedirectedHMMA_16816 => 22,
                IMMA(_) => 10,
                _ => 1,
            },
            CoupledFMA | IMADLo => match writer {
                CoupledDisp64 => 6,
                CoupledAlu | CoupledDisp => 5,
                CoupledFMA | IMADLo => 4,
                IMADWideLower => 2,
                IMADWideUpper => 4,
                RedirectedFP64 => 9,
                RedirectedFP16 => 8,
                RedirectedHMMA_884_F16(_) => 13,
                RedirectedHMMA_884_F32(_) => 10,
                RedirectedHMMA_1688 => 14,
                RedirectedHMMA_16816 => 22,
                IMMA(_) => 10,
                _ => 1,
            },
            IMADWideAB => match writer {
                CoupledDisp64 => 6,
                CoupledAlu | CoupledDisp => 5,
                CoupledFMA | IMADLo => 4,
                IMADWideLower => 4,
                IMADWideUpper => 6,
                RedirectedFP64 => 9,
                RedirectedFP16 => 8,
                RedirectedHMMA_884_F16(_) => 13,
                RedirectedHMMA_884_F32(_) => 10,
                RedirectedHMMA_1688 => 14,
                RedirectedHMMA_16816 => 22,
                IMMA(_) => 10,
                _ => 1,
            },
            IMADWideLower | IMADWideUpper => match reader {
                IMADWideLower => match writer {
                    CoupledDisp64 => 6,
                    CoupledAlu | CoupledDisp => 5,
                    CoupledFMA | IMADLo => 4,
                    IMADWideLower => 2,
                    IMADWideUpper => 2,
                    RedirectedFP64 => 9,
                    RedirectedFP16 => 8,
                    RedirectedHMMA_884_F16(_) => 13,
                    RedirectedHMMA_884_F32(_) => 10,
                    RedirectedHMMA_1688 => 14,
                    RedirectedHMMA_16816 => 22,
                    IMMA(_) => 10,
                    _ => 1,
                },
                IMADWideUpper => match writer {
                    CoupledDisp64 => 4,
                    CoupledDisp | CoupledAlu => 3,
                    CoupledFMA | IMADLo => 2,
                    IMADWideLower => 2,
                    IMADWideUpper => 2,
                    RedirectedFP64 => 7,
                    RedirectedFP16 => 6,
                    RedirectedHMMA_884_F16(_) => 11,
                    RedirectedHMMA_884_F32(_) => 8,
                    RedirectedHMMA_1688 => 12,
                    RedirectedHMMA_16816 => 20,
                    IMMA(_) => 8,
                    _ => 1,
                },
                _ => {
                    panic!("Illegal IMAD field");
                }
            },
            RedirectedFP64 => match writer {
                CoupledDisp64 => 6,
                CoupledDisp | CoupledAlu => 6,
                CoupledFMA | IMADLo => 6,
                IMADWideLower => 6,
                IMADWideUpper => 6,
                RedirectedFP64 => 8,
                RedirectedFP16 => 8,
                RedirectedHMMA_884_F16(_) => 13,
                RedirectedHMMA_884_F32(_) => 10,
                RedirectedHMMA_1688 => 14,
                RedirectedHMMA_16816 => 22,
                IMMA(_) => 10,
                _ => 1,
            },
            RedirectedFP16 => match writer {
                CoupledDisp64 => 6,
                CoupledDisp | CoupledAlu => 6,
                CoupledFMA | IMADLo => 6,
                IMADWideLower => 6,
                IMADWideUpper => 6,
                RedirectedFP64 => 9,
                RedirectedFP16 => 6,
                RedirectedHMMA_884_F16(_) => 13,
                RedirectedHMMA_884_F32(_) => 10,
                RedirectedHMMA_1688 => 14,
                RedirectedHMMA_16816 => 22,
                IMMA(_) => 10,
                _ => 1,
            },
            RedirectedHMMA_884_F16(read_idx) => match writer {
                CoupledDisp64 => 6,
                CoupledDisp | CoupledAlu => 6,
                CoupledFMA | IMADLo => 6,
                IMADWideLower => 6,
                IMADWideUpper => 6,
                RedirectedFP64 => 9,
                RedirectedFP16 => 8,
                RedirectedHMMA_884_F16(_) if read_idx == 2 => 4,
                RedirectedHMMA_884_F16(_) => 13,
                RedirectedHMMA_884_F32(_) => 10,
                RedirectedHMMA_1688 => 14,
                RedirectedHMMA_16816 => 22,
                IMMA(_) => 10,
                _ => 1,
            },
            RedirectedHMMA_884_F32(read_idx) => match writer {
                CoupledDisp64 => 6,
                CoupledDisp | CoupledAlu => 6,
                CoupledFMA | IMADLo => 6,
                IMADWideLower => 6,
                IMADWideUpper => 6,
                RedirectedFP64 => 9,
                RedirectedFP16 => 8,
                RedirectedHMMA_884_F16(_) => 13,
                RedirectedHMMA_884_F32(_) if read_idx == 2 => 4,
                RedirectedHMMA_884_F32(_) => 10,
                RedirectedHMMA_1688 => 14,
                RedirectedHMMA_16816 => 22,
                IMMA(_) => 10,
                _ => 1,
            },
            RedirectedHMMA_1688 | RedirectedHMMA_16816 | Decoupled => match writer {
                CoupledDisp64 => 6,
                CoupledDisp | CoupledAlu => 6,
                CoupledFMA | IMADLo => 6,
                IMADWideLower => 6,
                IMADWideUpper => 6,
                RedirectedFP64 => 9,
                RedirectedFP16 => 8,
                RedirectedHMMA_884_F16(_) => 13,
                RedirectedHMMA_884_F32(_) => 10,
                RedirectedHMMA_1688 => 14,
                RedirectedHMMA_16816 => 22,
                IMMA(_) => 10,
                _ => 1,
            },
            IMMA(read_idx) => match writer {
                CoupledDisp64 => 8,
                CoupledDisp | CoupledAlu => 8,
                CoupledFMA | IMADLo => 8,
                IMADWideLower => 8,
                IMADWideUpper => 8,
                RedirectedFP64 => 9,
                RedirectedFP16 => 8,
                RedirectedHMMA_884_F16(_) => 13,
                RedirectedHMMA_884_F32(_) => 10,
                RedirectedHMMA_1688 => 14,
                RedirectedHMMA_16816 => 22,
                IMMA(_) if read_idx == 2 => 4,
                IMMA(_) => 10,
                _ => 1,
            },
            DecoupledOther => match writer {
                CoupledDisp64 => 8,
                CoupledDisp | CoupledAlu => 8,
                CoupledFMA | IMADLo => 8,
                IMADWideLower => 8,
                IMADWideUpper => 8,
                RedirectedFP64 => 9,
                RedirectedFP16 => 8,
                RedirectedHMMA_884_F16(_) => 13,
                RedirectedHMMA_884_F32(_) => 10,
                RedirectedHMMA_1688 => 14,
                RedirectedHMMA_16816 => 22,
                IMMA(_) => 10,
                _ => 1,
            },
            BMov | GuardPredicate => {
                panic!("Not a RAW category")
            }
        }
    }

    pub(super) fn write_after_write(writer1: Self, writer2: Self, has_pred: bool) -> u32 {
        use RegLatencySM75::*;
        match writer1 {
            IMADWideAB | DecoupledOther => {
                panic!("Illegal reg latency for writer");
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
            BMov => {
                // BMOV Writing to RF?
                match writer1 {
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
                }
            }
            IMADWideAB | DecoupledOther | GuardPredicate => {
                panic!("Not a WAW category")
            }
        }
    }

    pub(super) fn write_after_read(reader: Self, writer: Self) -> u32 {
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
                panic!("Illegal in WAR");
            }
        }
    }

    pub(super) fn pred_read_after_write(writer: Self, reader: Self) -> u32 {
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
                    panic!("Illegal RAW in Predicate");
                }
            },
            CoupledAlu => match writer {
                CoupledDisp | CoupledAlu => 4,
                CoupledFMA | IMADLo | IMADWideUpper | IMADWideLower => 5,
                RedirectedFP64 => 9,
                RedirectedFP16 => 8,
                Decoupled => 1,
                _ => {
                    panic!("Illegal RAW in Predicate");
                }
            },
            CoupledFMA | IMADLo => match writer {
                CoupledDisp | CoupledAlu => 5,
                CoupledFMA | IMADLo | IMADWideUpper | IMADWideLower => 4,
                RedirectedFP64 => 9,
                RedirectedFP16 => 8,
                Decoupled => 1,
                _ => {
                    panic!("Illegal RAW in Predicate");
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
                    panic!("Illegal RAW in Predicate");
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
                    panic!("Illegal RAW in Predicate");
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
                    panic!("Illegal RAW in Predicate");
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
                    panic!("Illegal RAW in Predicate");
                }
            },
            _ => {
                panic!("Illegal reader in reg predicate");
            }
        }
    }

    pub(super) fn pred_write_after_write(writer1: Self, writer2: Self, has_pred: bool) -> u32 {
        use RegLatencySM75::*;
        match writer2 {
            CoupledDisp | CoupledAlu | CoupledFMA | IMADLo => match writer1 {
                CoupledDisp | CoupledAlu | CoupledFMA | IMADLo | IMADWideUpper | IMADWideLower => 1,
                RedirectedFP64 => pred(has_pred, 4, 1),
                RedirectedFP16 => pred(has_pred, 3, 1),
                Decoupled => 1,
                _ => {
                    panic!("Illegal RAW in Predicate");
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
                    panic!("Illegal RAW in Predicate");
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
                    panic!("Illegal RAW in Predicate");
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
                    panic!("Illegal RAW in Predicate");
                }
            },
            Decoupled => match writer1 {
                CoupledDisp | CoupledAlu | CoupledFMA | IMADLo | IMADWideUpper | IMADWideLower
                | RedirectedFP64 | RedirectedFP16 => 2,
                Decoupled => 1,
                _ => {
                    panic!("Illegal RAW in Predicate");
                }
            },
            _ => {
                panic!("Illegal WAR category in Predicates");
            }
        }
    }

    pub(super) fn pred_write_after_read(reader: Self, writer: Self) -> u32 {
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
                panic!("Illegal WAR category in Predicates");
            }
        }
    }
}

#[cfg(test)]
#[path = "gpr_tests.rs"]
mod tests;
