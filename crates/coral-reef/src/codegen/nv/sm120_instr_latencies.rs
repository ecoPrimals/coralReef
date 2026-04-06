// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright © 2025-2026 ecoPrimals
// Derived from Red Hat. (2025)

use crate::codegen::ir::*;

use coral_reef_stubs::nak_latencies::sm100::*;

// This contains the register scheduling information provided by NVIDIA.  This
// file is for Blackwell only.
//
// These latencies come from B100 (SM100) and not the consumer RTX chips
// (SM120).  We have to add some padding to get everything passing on the RTX
// chips so that's done in this file while using the sm100 CSVs.

// Coupled instructions are ones with fixed latencies, they need delays but not
// scoreboards.  Decoupled instructions are ones with variable latencies, need
// scoreboards but not delays.  There are also redirected instructions which
// depending on the SM, can be coupled or Decoupled so both delays and
// scoreboards needs to be provided.

fn op_reg_latency(op: &Op, reader: bool, op_reg_idx: usize) -> RegLatencySM100 {
    use RegLatencySM100::*;
    match op {
        // this will need updating if imad grows support for input predicates
        Op::IMad(_) | Op::IMul(_) => Fma,
        Op::IMad64(_) => {
            if reader {
                match op_reg_idx {
                    0 | 1 => ImadWideReadAb,
                    2 => ImadWideReadCl, // vs upper C operand - work it out
                    _ => {
                        crate::codegen::ice!("Illegal field in imadwide")
                    }
                }
            } else {
                ImadWideWriteDh // as above this needs more work
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
        Op::IAdd3(_) | Op::IAdd3X(_) => Alu,

        Op::BMsk(_)
        | Op::Sgxt(_)
        | Op::Lop3(_)
        | Op::IAbs(_)
        | Op::Lea(_)
        | Op::LeaX(_)
        | Op::I2I(_)
        | Op::Shf(_)
        | Op::F2FP(_)
        | Op::PLop3(_)
        | Op::Prmt(_) => Alu,
        Op::ISetP(_)
        | Op::IMnMx(_)
        | Op::FMnMx(_)
        | Op::FSet(_)
        | Op::FSetP(_)
        | Op::Mov(_)
        | Op::Sel(_) => Dualalu,
        Op::FFma(_) | Op::FAdd(_) | Op::FMul(_) | Op::FSwzAdd(_) | Op::IDp4(_) => Fma,
        Op::DAdd(_) | Op::DFma(_) | Op::DMul(_) | Op::DSetP(_) | Op::DMnMx(_) => RedirectedFp64, // DMnMx not in docs

        Op::HAdd2(hadd2) => {
            if hadd2.f32 {
                Fp16F32
            } else {
                Fp16
            }
        }
        Op::HFma2(_) | Op::HMul2(_) => Fp16,

        Op::HSet2(_) | Op::HSetP2(_) | Op::HMnMx2(_) => Fp16Alu,
        Op::Hmma(_) => Hmma,
        Op::Ipa(_)
        | Op::Movm(_)
        | Op::Bar(_)
        | Op::S2R(_)
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
        Op::R2UR(_) => Alu,
        Op::Redux(_) => {
            if reader {
                Decoupled
            } else {
                crate::codegen::ice!("Illegal R2UR");
            }
        }
        Op::CS2R(cs2r) => {
            if cs2r.dst.comps() == 2 {
                Disp64
            } else {
                Dualalu
            }
        }
        // B2R => DecoupledAgu,
        // LEPC => Disp64
        Op::BMov(_) => Branch,
        Op::Nop(_) => Disp64,
        Op::Imma(_) => Imma,
        x => {
            crate::codegen::ice!("Illegal instruction in reg category {x}");
        }
    }
}

fn op_pred_latency(op: &Op) -> PredLatencySM100 {
    use PredLatencySM100::*;
    match op {
        Op::Atom(_) => Decoupled,
        Op::Bra(_) => Decoupled,
        Op::DSetP(_) => RedirectedFp64,
        Op::FMnMx(_) | Op::FSetP(_) => Dualalu,
        Op::HFma2(_) | Op::HMnMx2(_) | Op::HSetP2(_) => Fp16,
        Op::IAdd3(_) | Op::IAdd3X(_) => Coupled,
        Op::IMad(_) | Op::IMad64(_) | Op::IMul(_) => Fma,
        Op::IMnMx(_) => Dualalu,
        Op::Ipa(_) => Decoupled,
        Op::ISetP(_) => Dualalu,

        Op::Ld(_) => Decoupled,

        Op::Lea(_) | Op::LeaX(_) | Op::PLop3(_) | Op::PSetP(_) => Coupled,
        Op::PixLd(_) => Decoupled,
        Op::R2UR(_) => R2Ur,
        Op::Sel(_) => Dualalu,
        Op::Shfl(_)
        | Op::SuLd(_)
        | Op::SuSt(_)
        | Op::Tex(_)
        | Op::Tld(_)
        | Op::Tld4(_)
        | Op::Tmml(_)
        | Op::Txd(_)
        | Op::Txq(_) => Decoupled,

        Op::Vote(_) => DispDualAlu,
        Op::Match(_) => Decoupled,
        _ => {
            crate::codegen::ice!("Illegal op in sm120 pred latency {op}");
        }
    }
}

fn op_ureg_latency(op: &Op, reader: bool, op_reg_idx: usize) -> UregLatencySM100 {
    use UregLatencySM100::*;
    // this decides between the category types for readers.
    let bindless = reader && op.srcs_as_slice()[op_reg_idx].is_bindless_cbuf();

    let coupled = if bindless { CoupledBindless } else { Coupled };
    let decoupled = if bindless {
        DecoupledBindless
    } else {
        Decoupled
    };

    // if this is a reader from a ureg, it could be a U* instruction or a
    // regular instruction.
    let uniform_op = op.is_uniform();

    let coupled = if uniform_op { Udp } else { coupled };
    let decoupled = if uniform_op { Udp } else { decoupled };

    match op {
        Op::BMsk(_) => coupled,
        Op::BRev(_) => decoupled,
        // uclea?
        Op::Flo(_) => decoupled,
        Op::IAdd3(_) | Op::IAdd3X(_) => coupled,
        Op::IAbs(_) => coupled,
        Op::IDp4(_) => coupled,
        Op::IMnMx(_) => coupled,
        Op::IMad(_) => coupled,

        Op::IMad64(_) => coupled,
        Op::ISetP(_) => coupled,
        Op::Ldc(_) => {
            if uniform_op {
                ToUr
            } else {
                decoupled
            }
        }
        Op::Lea(_) => coupled,
        Op::LeaX(_) => coupled,
        Op::Lop2(_) | Op::Lop3(_) => coupled,

        Op::Transcendental(_) => decoupled,
        Op::Mov(_) => {
            if uniform_op {
                Umov
            } else {
                coupled
            }
        }

        // mov32i => uldc
        // p2ur => udp,
        Op::PLop3(_) => coupled,
        Op::PopC(_) => {
            if uniform_op {
                coupled
            } else {
                decoupled
            }
        }
        Op::Prmt(_) => coupled,
        Op::PSetP(_) => coupled,
        // UR2UP
        Op::Sel(_) => coupled,
        Op::Sgxt(_) => coupled,
        Op::Shf(_) => coupled,
        Op::Shfl(_) => decoupled,

        Op::I2F(_) => decoupled,
        Op::F2I(_) => decoupled,
        Op::F2F(_) => decoupled,
        Op::R2UR(_) => {
            if !reader {
                R2Ur
            } else {
                crate::codegen::ice!("Illegal R2UR in ureg");
            }
        }
        Op::Redux(_) => {
            if !reader {
                ToUr
            } else {
                crate::codegen::ice!("Illegal R2UR in ureg");
            }
        }
        Op::Vote(_) => Voteu,
        Op::S2R(_) => ToUr,

        Op::Tex(_) | Op::Tld(_) | Op::Tld4(_) | Op::Txq(_) => Tex,
        Op::FRnd(_) => decoupled,
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
        | Op::HSetP2(_) => coupled,
        Op::DMul(_) | Op::DFma(_) | Op::DAdd(_) | Op::DSetP(_) => decoupled,
        _ => {
            crate::codegen::ice!("Illegal instruction in ureg category {op}");
        }
    }
}

fn op_upred_latency(op: &Op) -> UpredLatencySM100 {
    use UpredLatencySM100::*;
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
        Op::Bra(_) => BraJmp,
        Op::Ldc(_) => UldcMma,
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
                Voteu
            } else {
                crate::codegen::ice!("Illegal Vote in upred");
            }
        }
        _ => {
            crate::codegen::ice!("Illegal instruction in upred category {op}");
        }
    }
}

pub struct SM120Latency {}

impl SM120Latency {
    pub fn needs_scoreboards(op: &Op) -> bool {
        if op.is_uniform() {
            matches!(
                op_ureg_latency(op, false, 0),
                UregLatencySM100::Uldc | UregLatencySM100::ToUr | UregLatencySM100::Tex
            )
        } else {
            matches!(
                op_reg_latency(op, false, 0),
                RegLatencySM100::Dmma
                    | RegLatencySM100::Hmma
                    | RegLatencySM100::RedirectedFp64
                    | RegLatencySM100::Branch
                    | RegLatencySM100::Decoupled
                    | RegLatencySM100::DecoupledAgu
            )
        }
    }

    pub fn raw(write: &Op, dst_idx: usize, read: Option<&Op>, src_idx: usize) -> u32 {
        let Some(dst_file) = write.dsts_as_slice()[dst_idx].file() else {
            return 0;
        };

        match dst_file {
            RegFile::GPR => {
                let write_latency = op_reg_latency(write, false, dst_idx);
                let read_latency = match read {
                    Some(op) => op_reg_latency(op, true, src_idx),
                    None => RegLatencySM100::RedirectedFp64,
                };
                // The latencies are for SM100 docs, but some chips need large
                // one just override here.
                if write_latency == RegLatencySM100::Hmma || read_latency == RegLatencySM100::Hmma {
                    RegLatencySM100::raw(write_latency, read_latency, false) + 9
                } else if write_latency == RegLatencySM100::Imma
                    || read_latency == RegLatencySM100::Imma
                {
                    RegLatencySM100::raw(write_latency, read_latency, false) + 5
                } else {
                    RegLatencySM100::raw(write_latency, read_latency, false) + 1
                }
            }
            RegFile::UGPR => {
                let write_latency = op_ureg_latency(write, false, dst_idx);
                let read_latency = match read {
                    Some(op) => op_ureg_latency(op, true, src_idx),
                    None => UregLatencySM100::Uldc,
                };
                UregLatencySM100::raw(write_latency, read_latency, false) + 1
            }
            RegFile::Pred => {
                let write_latency = op_pred_latency(write);
                let read_latency = match read {
                    Some(op) => op_pred_latency(op),
                    None => PredLatencySM100::RedirectedFp64,
                };
                PredLatencySM100::raw(write_latency, read_latency, false) + 1
            }
            RegFile::UPred => {
                let write_latency = op_upred_latency(write);
                let read_latency = match read {
                    Some(op) => op_upred_latency(op),
                    None => UpredLatencySM100::UGuard,
                };
                UpredLatencySM100::raw(write_latency, read_latency, false) + 1
            }
            RegFile::Bar => 0, // Barriers have a HW scoreboard
            _ => crate::codegen::ice!("Not a register"),
        }
    }

    pub fn war(read: &Op, src_idx: usize, write: &Op, dst_idx: usize) -> u32 {
        let Some(dst_file) = write.dsts_as_slice()[dst_idx].file() else {
            return 0;
        };

        match dst_file {
            RegFile::GPR => {
                let write_latency = op_reg_latency(write, false, dst_idx);
                let read_latency = op_reg_latency(read, true, src_idx);

                if write_latency == RegLatencySM100::Hmma || read_latency == RegLatencySM100::Hmma {
                    RegLatencySM100::war(read_latency, write_latency, false) + 7
                } else {
                    RegLatencySM100::war(read_latency, write_latency, false)
                }
            }
            RegFile::UGPR => {
                let write_latency = op_ureg_latency(write, false, dst_idx);
                let read_latency = op_ureg_latency(read, true, src_idx);
                UregLatencySM100::war(read_latency, write_latency, false)
            }
            RegFile::Pred => {
                let write_latency = op_pred_latency(write);
                let read_latency = op_pred_latency(read);
                PredLatencySM100::war(read_latency, write_latency, false)
            }
            RegFile::UPred => {
                let write_latency = op_upred_latency(write);
                let read_latency = op_upred_latency(read);
                UpredLatencySM100::war(read_latency, write_latency, false)
            }
            _ => crate::codegen::ice!("Not a register"),
        }
    }

    pub fn waw(a: &Op, a_dst_idx: usize, b: &Op, b_dst_idx: usize, a_op_pred: bool) -> u32 {
        let Some(dst_file) = a.dsts_as_slice()[a_dst_idx].file() else {
            return 0;
        };

        match dst_file {
            RegFile::GPR => {
                let write1_latency = op_reg_latency(a, false, a_dst_idx);
                let write2_latency = op_reg_latency(b, false, b_dst_idx);
                if write1_latency == RegLatencySM100::Hmma
                    || write2_latency == RegLatencySM100::Hmma
                {
                    RegLatencySM100::waw(write1_latency, write2_latency, a_op_pred) + 7
                } else {
                    RegLatencySM100::waw(write1_latency, write2_latency, a_op_pred)
                }
            }
            RegFile::UGPR => {
                let write1_latency = op_ureg_latency(a, false, a_dst_idx);
                let write2_latency = op_ureg_latency(b, false, b_dst_idx);
                UregLatencySM100::waw(write1_latency, write2_latency, a_op_pred)
            }
            RegFile::Pred => {
                let write1_latency = op_pred_latency(a);
                let write2_latency = op_pred_latency(b);
                PredLatencySM100::waw(write1_latency, write2_latency, a_op_pred)
            }
            RegFile::UPred => {
                let write1_latency = op_upred_latency(a);
                let write2_latency = op_upred_latency(b);
                UpredLatencySM100::waw(write1_latency, write2_latency, false)
            }
            _ => crate::codegen::ice!("Not a register"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::codegen::ir::{
        Dst, MemAccess, MemAddrType, MemEvictionPriority, MemOrder, MemSpace, MemType, Op, OpDAdd,
        OpFFma, OpIAdd3, OpLd, OpMov, RegFile, RegRef, Src,
    };
    use crate::codegen::ir::{FRndMode, OffsetStride};

    fn gpr_dst(idx: u32) -> Dst {
        Dst::Reg(RegRef::new(RegFile::GPR, idx, 1))
    }

    fn default_mem_access() -> MemAccess {
        MemAccess {
            mem_type: MemType::B32,
            space: MemSpace::Global(MemAddrType::A32),
            order: MemOrder::Constant,
            eviction_priority: MemEvictionPriority::Normal,
        }
    }

    #[test]
    fn test_needs_scoreboards() {
        // Decoupled ops (Ld) need scoreboards
        let ld = Op::Ld(Box::new(OpLd {
            dst: gpr_dst(0),
            addr: Src::ZERO,
            offset: 0,
            stride: OffsetStride::X1,
            access: default_mem_access(),
        }));
        assert!(SM120Latency::needs_scoreboards(&ld));

        // Coupled ALU (IAdd3) does not need scoreboards for GPR
        let iadd3 = Op::IAdd3(Box::new(OpIAdd3 {
            dsts: [gpr_dst(0), Dst::None, Dst::None],
            srcs: [Src::ZERO, Src::ZERO, Src::ZERO],
        }));
        assert!(!SM120Latency::needs_scoreboards(&iadd3));
    }

    #[test]
    fn test_raw_latency() {
        let write = Op::FFma(Box::new(OpFFma {
            dst: gpr_dst(0),
            srcs: [Src::ZERO, Src::ZERO, Src::ZERO],
            saturate: false,
            rnd_mode: FRndMode::NearestEven,
            ftz: false,
            dnz: false,
        }));
        let read = Op::Mov(Box::new(OpMov {
            dst: gpr_dst(1),
            src: Src::ZERO,
            quad_lanes: 0xf,
        }));
        let lat = SM120Latency::raw(&write, 0, Some(&read), 0);
        assert!(lat > 0);
    }

    #[test]
    fn test_war_latency() {
        let read = Op::IAdd3(Box::new(OpIAdd3 {
            dsts: [gpr_dst(0), Dst::None, Dst::None],
            srcs: [Src::ZERO, Src::ZERO, Src::ZERO],
        }));
        let write = Op::DAdd(Box::new(OpDAdd {
            dst: gpr_dst(1),
            srcs: [Src::ZERO, Src::ZERO],
            rnd_mode: FRndMode::NearestEven,
        }));
        let lat = SM120Latency::war(&read, 0, &write, 0);
        assert!(lat > 0);
    }

    #[test]
    fn test_waw_latency() {
        let a = Op::FFma(Box::new(OpFFma {
            dst: gpr_dst(0),
            srcs: [Src::ZERO, Src::ZERO, Src::ZERO],
            saturate: false,
            rnd_mode: FRndMode::NearestEven,
            ftz: false,
            dnz: false,
        }));
        let b = Op::IAdd3(Box::new(OpIAdd3 {
            dsts: [gpr_dst(1), Dst::None, Dst::None],
            srcs: [Src::ZERO, Src::ZERO, Src::ZERO],
        }));
        let lat = SM120Latency::waw(&a, 0, &b, 0, false);
        assert!(lat > 0);
    }
}
