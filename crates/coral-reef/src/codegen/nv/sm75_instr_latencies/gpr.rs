// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright © 2025-2026 ecoPrimals
// Derived from Red Hat. (2025)
#![expect(
    non_camel_case_types,
    reason = "latency model mirrors hardware naming from Red Hat spec"
)]

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
        // Schedule classes from NVIDIA tables not mapped to `Op` variants here: S2UR→Decoupled,
        // B2R→Decoupled, LEPC→CoupledDisp64.
        match op {
            // this will need updating if imad grows support for input predicates
            Op::IMad(_) | Op::IMul(_) => IMADLo,
            Op::IMad64(_) => {
                if reader {
                    match op_reg_idx {
                        0 | 1 => IMADWideAB,
                        2 => IMADWideLower, // vs upper C operand - work it out
                        _ => {
                            crate::codegen::ice!("Illegal field in imadwide")
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
                _ => crate::codegen::ice!("Illegal HMMA in reg category {h}"),
            },
            Op::R2UR(_) => {
                if reader {
                    Decoupled
                } else {
                    crate::codegen::ice!("Illegal R2UR");
                }
            }
            Op::CS2R(cs2r) => {
                if cs2r.dst.comps() == 2 {
                    CoupledDisp64
                } else {
                    CoupledAlu
                }
            }
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
                crate::codegen::ice!("Illegal instruction in reg category {x}");
            }
        }
    }

    pub(super) fn read_after_write(writer: Self, reader: Self) -> u32 {
        use RegLatencySM75::*;
        match writer {
            IMADWideAB | DecoupledOther => {
                crate::codegen::ice!("Illegal IMADWideAB for writer");
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
                    crate::codegen::ice!("Illegal IMAD field");
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
                crate::codegen::ice!("Not a RAW category")
            }
        }
    }
}

#[cfg(test)]
#[path = "gpr_tests.rs"]
mod tests;
