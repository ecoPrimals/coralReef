// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright © 2025-2026 ecoPrimals
// Derived from Red Hat. (2025)
#![expect(
    non_camel_case_types,
    reason = "latency model mirrors hardware naming from Red Hat spec"
)]

use crate::codegen::ir::*;

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
        // Schedule classes from NVIDIA tables not mapped to `Op` variants here: S2UR→DecoupledAgu,
        // B2R→DecoupledAgu, LEPC→CoupledDisp64.
        match op {
            // this will need updating if imad grows support for input predicates
            Op::IMad(_) | Op::IMul(_) => CoupledFMA,
            Op::IMad64(_) => {
                if reader {
                    match op_reg_idx {
                        0 | 1 => IMADWideReadAB,
                        2 => IMADWideReadCL, // vs upper C operand - work it out
                        _ => {
                            crate::codegen::ice!("Illegal field in imadwide")
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
            // HMMA: M16N8K8 with TF32 sources maps to MMA_2x_collect in hardware; not modeled.
            Op::Hmma(h) => match (h.mat_size, h.dst_type, h.src_type) {
                (HmmaSize::M16N8K8, FloatType::F32, FloatType::F16)
                | (HmmaSize::M16N8K8, FloatType::F16, _) => MMA_1x_collect,
                (HmmaSize::M16N8K16, _, _) => MMA_2x_collect,
                _ => crate::codegen::ice!("Illegal HMMA in reg category {h}"),
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
            Op::R2UR(_) | Op::Redux(_) => {
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
            Op::BMov(_) => Cbu,
            Op::Nop(_) => CoupledDisp64,
            Op::Imma(i) => match (i.mat_size, i.src_types[0]) {
                (ImmaSize::M16N8K64, _) | (ImmaSize::M16N8K32, IntType::I8 | IntType::U8) => {
                    MMA_2x_collect
                }
                (ImmaSize::M16N8K16, _) => MMA_1x_collect,
                (ImmaSize::M8N8K32 | ImmaSize::M8N8K16, _) => IMMA_88,
                _ => crate::codegen::ice!("Illegal IMMA in reg category {i}"),
            },
            x => {
                crate::codegen::ice!("Illegal instruction in reg category {x}");
            }
        }
    }
}

#[cfg(test)]
#[path = "gpr_tests.rs"]
mod tests;
