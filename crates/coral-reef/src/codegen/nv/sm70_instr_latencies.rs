// SPDX-License-Identifier: AGPL-3.0-only
// Copyright © 2025-2026 ecoPrimals
// Derived from BarraCUDA / ecoPrimals contributors (2026)
#![allow(non_camel_case_types, clippy::wildcard_imports, clippy::enum_glob_use)]

use super::sm75_instr_latencies::pred;
use crate::codegen::ir::*;

// SM70 (Volta GV100 / GV102) instruction latency tables.
//
// Volta does NOT have tensor-core (HMMA/IMMA) instructions — these were added
// in SM75 (Turing).  The coupled / decoupled model is otherwise the same.
//
// Data sources:
//  - arXiv:1804.06826 — Jia et al., "Dissecting the NVIDIA Volta GPU
//    Architecture via Microbenchmarking" (public, 2018).
//    FP32 FFMA latency ≈ 4cy, FP64 DFMA ≈ 8cy, INT IMAD ≈ 6cy.
//  - NVIDIA Volta white paper (public).
//  - sm75_instr_latencies.rs as structural template (Red Hat, MIT, 2025).
//
// Where SM70 data is not independently confirmed we err on the safe side
// (use SM75 values or conservative estimates).  Hardware tests on a Titan V
// (SM70) are the authoritative source; these tables should be refined as
// measurements arrive.

// ──────────────────────────────────────────────────────────────────────────────
// GPR latency categories for SM70
// ──────────────────────────────────────────────────────────────────────────────

#[allow(dead_code)]
#[derive(Debug, Clone, Copy)]
enum RegLatencySM70 {
    // Fixed-latency (coupled) instructions — need delays, not scoreboards
    CoupledDisp64, // CS2R 64-bit, LEPC, RPCMOV.64
    CoupledDisp,   // NOP, VOTE, S2R, etc.
    CoupledAlu,    // IAdd, ISetP, Mov, PLop, Lop, Shf, Sel, Prmt, …
    CoupledFMA,    // FFma, FAdd, FMul, FSwzAdd
    IMADLo,        // IMad lower result
    IMADWideLower, // IMad64 lower half
    IMADWideUpper, // IMad64 upper half
    IMADWideAB,    // IMad64 A/B source readers

    // Redirected (coupled but routed to separate execution units)
    RedirectedFP64, // DFma, DAdd, DMul, DMnMx, DSetP → FP64 unit
    RedirectedFP16, // HFma2, HAdd2, HMul2, HSet2, HSetP2, HMnMx2 → FP16 unit

    // Variable-latency (decoupled) — need scoreboards, not delays
    Decoupled,      // Tex, Ld, St, Atom, MuFu, F2F, F2I, I2F, FRnd, …
    DecoupledOther, // Read-only decoupled consumers (e.g., CCtl reads)

    // Special
    BMov,           // BMov writing to GPR register file
    GuardPredicate, // Predicate used as execution guard
}

impl RegLatencySM70 {
    fn op_category(op: &Op, reader: bool, op_reg_idx: usize) -> Self {
        use RegLatencySM70::*;
        match op {
            Op::IMad(_) | Op::IMul(_) => IMADLo,
            Op::IMad64(_) => {
                if reader {
                    match op_reg_idx {
                        0 | 1 => IMADWideAB,
                        2 => IMADWideLower,
                        _ => panic!("Illegal field in imadwide"),
                    }
                } else {
                    IMADWideUpper
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
            Op::DAdd(_) | Op::DFma(_) | Op::DMul(_) | Op::DSetP(_) | Op::DMnMx(_) => RedirectedFP64,

            Op::HAdd2(_)
            | Op::HFma2(_)
            | Op::HMul2(_)
            | Op::HSet2(_)
            | Op::HSetP2(_)
            | Op::HMnMx2(_) => RedirectedFP16,
            Op::R2UR(_) => {
                if reader {
                    Decoupled
                } else {
                    panic!("Illegal R2UR")
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
                Dst::Reg(reg) if reg.is_gpr() => BMov,
                _ => Decoupled,
            },

            Op::Nop(_) | Op::Vote(_) => CoupledDisp,
            Op::CCtl(_) => DecoupledOther,
            x => panic!("Illegal instruction in SM70 reg category: {x}"),
        }
    }

    // ── Read-After-Write ──────────────────────────────────────────────────────
    //
    // Source: arXiv:1804.06826 (Jia et al.):
    //   FP32 FFMA latency  ≈ 4cy
    //   FP64 DFMA latency  ≈ 8cy  (was placeholder 13cy — key correction)
    //   INT  IMAD latency  ≈ 6cy
    //   FP16 HFMA2 latency ≈ 5cy  (conservative; Volta FP16 pipeline)
    //
    // Where uncertain we use max(SM75, arXiv value) as a safe upper bound.
    pub fn read_after_write(writer: Self, reader: Self) -> u32 {
        use RegLatencySM70::*;
        match writer {
            IMADWideAB | DecoupledOther => {
                panic!("Illegal category for writer in RAW");
            }
            _ => {}
        }

        match reader {
            // ALU readers — 4cy from FP32 FMA (arXiv:1804.06826 §3.1)
            CoupledDisp64 | CoupledDisp | CoupledAlu => match writer {
                CoupledDisp64 => 6,
                CoupledAlu | CoupledDisp => 4,
                CoupledFMA | IMADLo => 5,
                IMADWideLower => 3,
                IMADWideUpper => 5,
                // DFMA ≈ 8cy to ALU reader (arXiv:1804.06826)
                RedirectedFP64 => 9,
                // FP16 conservative estimate
                RedirectedFP16 => 8,
                _ => 1,
            },

            CoupledFMA | IMADLo => match writer {
                CoupledDisp64 => 6,
                CoupledAlu | CoupledDisp => 5,
                // FP32 FMA → FP32 FMA: 4cy (arXiv:1804.06826)
                CoupledFMA | IMADLo => 4,
                IMADWideLower => 2,
                IMADWideUpper => 4,
                RedirectedFP64 => 9,
                RedirectedFP16 => 8,
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
                    _ => 1,
                },
                IMADWideUpper => match writer {
                    CoupledDisp64 => 4,
                    CoupledAlu | CoupledDisp => 3,
                    CoupledFMA | IMADLo => 2,
                    IMADWideLower => 2,
                    IMADWideUpper => 2,
                    RedirectedFP64 => 7,
                    RedirectedFP16 => 6,
                    _ => 1,
                },
                _ => panic!("Illegal IMAD field in RAW"),
            },

            // FP64 reader — DFMA → DFMA = 8cy (arXiv:1804.06826 §3.2)
            RedirectedFP64 => match writer {
                CoupledDisp64 => 6,
                CoupledAlu | CoupledDisp => 6,
                CoupledFMA | IMADLo => 6,
                IMADWideLower => 6,
                IMADWideUpper => 6,
                // FP64 → FP64: 8cy (Volta, arXiv:1804.06826)
                RedirectedFP64 => 8,
                RedirectedFP16 => 8,
                _ => 1,
            },

            RedirectedFP16 => match writer {
                CoupledDisp64 => 6,
                CoupledAlu | CoupledDisp => 6,
                CoupledFMA | IMADLo => 6,
                IMADWideLower => 6,
                IMADWideUpper => 6,
                RedirectedFP64 => 9,
                // FP16 → FP16: 5cy (conservative; Volta FP16 pipeline)
                RedirectedFP16 => 5,
                _ => 1,
            },

            Decoupled => match writer {
                CoupledDisp64 => 6,
                CoupledAlu | CoupledDisp => 6,
                CoupledFMA | IMADLo => 6,
                IMADWideLower => 6,
                IMADWideUpper => 6,
                RedirectedFP64 => 9,
                RedirectedFP16 => 8,
                _ => 1,
            },

            DecoupledOther => match writer {
                CoupledDisp64 => 8,
                CoupledAlu | CoupledDisp => 8,
                CoupledFMA | IMADLo => 8,
                IMADWideLower => 8,
                IMADWideUpper => 8,
                RedirectedFP64 => 9,
                RedirectedFP16 => 8,
                _ => 1,
            },

            BMov | GuardPredicate => panic!("Not a RAW category"),
        }
    }

    // ── Write-After-Write ─────────────────────────────────────────────────────
    fn write_after_write(writer1: Self, writer2: Self, has_pred: bool) -> u32 {
        use RegLatencySM70::*;
        match writer1 {
            IMADWideAB | DecoupledOther => {
                panic!("Illegal category for writer1 in WAW");
            }
            _ => {}
        }
        match writer2 {
            CoupledDisp64 => match writer1 {
                CoupledDisp64 | CoupledDisp | CoupledAlu | CoupledFMA | IMADLo | IMADWideLower
                | IMADWideUpper => 1,
                RedirectedFP64 => 4,
                RedirectedFP16 => 3,
                _ => 1,
            },
            CoupledDisp | CoupledAlu => match writer1 {
                CoupledDisp64 => 2,
                CoupledDisp | CoupledAlu | CoupledFMA | IMADLo | IMADWideLower | IMADWideUpper => 1,
                RedirectedFP64 => pred(has_pred, 4, 1),
                RedirectedFP16 => pred(has_pred, 3, 1),
                _ => 1,
            },
            CoupledFMA | IMADLo => match writer1 {
                CoupledDisp64 => 2,
                CoupledDisp | CoupledAlu | CoupledFMA | IMADLo | IMADWideLower => 1,
                IMADWideUpper => pred(has_pred, 1, 1),
                RedirectedFP64 => pred(has_pred, 4, 1),
                RedirectedFP16 => pred(has_pred, 3, 1),
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
                _ => 1,
            },
            IMADWideUpper => match writer1 {
                CoupledDisp64 => pred(has_pred, 1, 1),
                CoupledDisp | CoupledAlu | CoupledFMA | IMADLo | IMADWideLower | IMADWideUpper => 1,
                RedirectedFP64 => pred(has_pred, 4, 1),
                RedirectedFP16 => pred(has_pred, 3, 1),
                _ => 1,
            },
            RedirectedFP64 => match writer1 {
                CoupledDisp64 | CoupledDisp | CoupledAlu | CoupledFMA | IMADLo | IMADWideLower
                | IMADWideUpper => 2,
                RedirectedFP64 => 1,
                RedirectedFP16 => 2,
                _ => 1,
            },
            RedirectedFP16 => match writer1 {
                CoupledDisp64 | CoupledDisp | CoupledAlu | CoupledFMA | IMADLo | IMADWideLower
                | IMADWideUpper => 2,
                RedirectedFP64 => pred(has_pred, 1, 1),
                RedirectedFP16 => 1,
                _ => 1,
            },
            Decoupled => match writer1 {
                CoupledDisp64 | CoupledDisp | CoupledAlu | CoupledFMA | IMADLo | IMADWideLower
                | IMADWideUpper | RedirectedFP64 | RedirectedFP16 => 6,
                _ => 1,
            },
            BMov => match writer1 {
                CoupledDisp64 | CoupledDisp | CoupledAlu | CoupledFMA | IMADLo | IMADWideLower
                | IMADWideUpper | RedirectedFP64 | RedirectedFP16 => 9,
                _ => 1,
            },
            IMADWideAB | DecoupledOther | GuardPredicate => {
                panic!("Not a WAW category")
            }
        }
    }

    // ── Write-After-Read ──────────────────────────────────────────────────────
    fn write_after_read(reader: Self, writer: Self) -> u32 {
        use RegLatencySM70::*;
        match writer {
            CoupledDisp64 | CoupledDisp | CoupledAlu | CoupledFMA | IMADLo | IMADWideLower
            | IMADWideUpper => 1,
            RedirectedFP64 => match reader {
                RedirectedFP64 => 1,
                Decoupled => 1,
                _ => 2,
            },
            RedirectedFP16 => match reader {
                RedirectedFP16 => 1,
                Decoupled => 1,
                _ => 2,
            },
            Decoupled => match reader {
                Decoupled => 1,
                _ => 2,
            },
            BMov => match reader {
                Decoupled => 1,
                _ => 9,
            },
            IMADWideAB | DecoupledOther | GuardPredicate => {
                panic!("Illegal in WAR");
            }
        }
    }

    // ── Predicate Read-After-Write ────────────────────────────────────────────
    //
    // Volta predicate latencies are conservatively set to SM75 values;
    // the hardware may be faster but these are safe upper bounds.
    fn pred_read_after_write(writer: Self, reader: Self) -> u32 {
        use RegLatencySM70::*;
        match reader {
            CoupledDisp => match writer {
                CoupledDisp | CoupledAlu | CoupledFMA | IMADLo | IMADWideUpper | IMADWideLower => {
                    12
                }
                RedirectedFP64 => 15,
                RedirectedFP16 => 14,
                Decoupled => 1,
                _ => panic!("Illegal RAW in Predicate"),
            },
            CoupledAlu => match writer {
                CoupledDisp | CoupledAlu => 4,
                CoupledFMA | IMADLo | IMADWideUpper | IMADWideLower => 5,
                RedirectedFP64 => 9,
                RedirectedFP16 => 8,
                Decoupled => 1,
                _ => panic!("Illegal RAW in Predicate"),
            },
            CoupledFMA | IMADLo => match writer {
                CoupledDisp | CoupledAlu => 5,
                CoupledFMA | IMADLo | IMADWideUpper | IMADWideLower => 4,
                RedirectedFP64 => 9,
                RedirectedFP16 => 8,
                Decoupled => 1,
                _ => panic!("Illegal RAW in Predicate"),
            },
            IMADWideUpper | IMADWideLower => match writer {
                CoupledDisp | CoupledAlu => 5,
                CoupledFMA | IMADLo => 4,
                IMADWideUpper | IMADWideLower => 2,
                RedirectedFP64 => 9,
                RedirectedFP16 => 8,
                Decoupled => 1,
                _ => panic!("Illegal RAW in Predicate"),
            },
            RedirectedFP64 => match writer {
                CoupledDisp | CoupledAlu | CoupledFMA | IMADLo | IMADWideUpper | IMADWideLower => {
                    12
                }
                // Volta FP64 predicate write latency (DSetP) = 15cy (sm70.rs §paw_latency)
                RedirectedFP64 => 8,
                RedirectedFP16 => 14,
                Decoupled => 1,
                _ => panic!("Illegal RAW in Predicate"),
            },
            RedirectedFP16 => match writer {
                CoupledDisp | CoupledAlu | CoupledFMA | IMADLo | IMADWideUpper | IMADWideLower => {
                    12
                }
                RedirectedFP64 => 15,
                RedirectedFP16 => 6,
                Decoupled => 1,
                _ => panic!("Illegal RAW in Predicate"),
            },
            Decoupled | GuardPredicate => match writer {
                CoupledDisp | CoupledAlu | CoupledFMA | IMADLo | IMADWideUpper | IMADWideLower => {
                    12
                }
                RedirectedFP64 => 15,
                RedirectedFP16 => 14,
                Decoupled => 1,
                _ => panic!("Illegal RAW in Predicate"),
            },
            _ => panic!("Illegal reader in SM70 predicate RAW"),
        }
    }

    fn pred_write_after_write(writer1: Self, writer2: Self, has_pred: bool) -> u32 {
        use RegLatencySM70::*;
        match writer2 {
            CoupledDisp | CoupledAlu | CoupledFMA | IMADLo => match writer1 {
                CoupledDisp | CoupledAlu | CoupledFMA | IMADLo | IMADWideUpper | IMADWideLower => 1,
                RedirectedFP64 => pred(has_pred, 4, 1),
                RedirectedFP16 => pred(has_pred, 3, 1),
                Decoupled => 1,
                _ => panic!("Illegal RAW in Predicate"),
            },
            IMADWideUpper | IMADWideLower => match writer1 {
                CoupledDisp | CoupledAlu => pred(has_pred, 1, 2),
                CoupledFMA | IMADLo => pred(has_pred, 1, 1),
                IMADWideUpper | IMADWideLower => 1,
                RedirectedFP64 => pred(has_pred, 4, 3),
                RedirectedFP16 => pred(has_pred, 3, 3),
                Decoupled => 1,
                _ => panic!("Illegal RAW in Predicate"),
            },
            RedirectedFP64 => match writer1 {
                CoupledDisp | CoupledAlu | CoupledFMA | IMADLo | IMADWideUpper | IMADWideLower => {
                    pred(has_pred, 2, 2)
                }
                RedirectedFP64 => 1,
                RedirectedFP16 => pred(has_pred, 2, 4),
                Decoupled => 1,
                _ => panic!("Illegal RAW in Predicate"),
            },
            RedirectedFP16 => match writer1 {
                CoupledDisp | CoupledAlu | CoupledFMA | IMADLo | IMADWideUpper | IMADWideLower => {
                    pred(has_pred, 2, 4)
                }
                RedirectedFP64 => pred(has_pred, 2, 7),
                RedirectedFP16 => 1,
                Decoupled => 1,
                _ => panic!("Illegal RAW in Predicate"),
            },
            Decoupled => match writer1 {
                CoupledDisp | CoupledAlu | CoupledFMA | IMADLo | IMADWideUpper | IMADWideLower
                | RedirectedFP64 | RedirectedFP16 => 2,
                Decoupled => 1,
                _ => panic!("Illegal RAW in Predicate"),
            },
            _ => panic!("Illegal WAR category in SM70 Predicates"),
        }
    }

    fn pred_write_after_read(reader: Self, writer: Self) -> u32 {
        use RegLatencySM70::*;
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
            _ => panic!("Illegal WAR category in SM70 Predicates"),
        }
    }
}

// ──────────────────────────────────────────────────────────────────────────────
// Public interface — SM70Latency
// ──────────────────────────────────────────────────────────────────────────────

pub struct SM70Latency {}

impl SM70Latency {
    /// True if this instruction requires a scoreboard (variable / decoupled
    /// latency) rather than a fixed delay slot.
    pub fn needs_scoreboards(op: &Op) -> bool {
        match RegLatencySM70::op_category(op, false, 0) {
            RegLatencySM70::RedirectedFP64 |
            // FP16 is coupled on Volta (fixed latency ~5cy) — no scoreboard needed
            RegLatencySM70::Decoupled => true,
            _ => false,
        }
    }

    /// Read-after-write latency.
    /// If `read` is None, returns the worst-case (most conservative) latency.
    pub fn raw(write: &Op, dst_idx: usize, read: Option<&Op>, src_idx: usize) -> u32 {
        let Some(dst_file) = write.dsts_as_slice()[dst_idx].file() else {
            return 0;
        };

        match dst_file {
            RegFile::GPR => {
                let write_cat = RegLatencySM70::op_category(write, false, dst_idx);
                let read_cat = match read {
                    Some(op) => RegLatencySM70::op_category(op, true, src_idx),
                    // Worst case: FP64 reader (highest RAW latency)
                    None => RegLatencySM70::RedirectedFP64,
                };
                RegLatencySM70::read_after_write(write_cat, read_cat)
            }
            RegFile::Pred => {
                let write_cat = RegLatencySM70::op_category(write, false, dst_idx);
                let read_cat = match read {
                    Some(op) => RegLatencySM70::op_category(op, true, src_idx),
                    None => RegLatencySM70::GuardPredicate,
                };
                RegLatencySM70::pred_read_after_write(write_cat, read_cat)
            }
            RegFile::Bar => 0, // Hardware scoreboard handles barriers
            _ => panic!("SM70: unexpected dst register file in raw()"),
        }
    }

    /// Write-after-read latency.
    pub fn war(read: &Op, src_idx: usize, write: &Op, dst_idx: usize) -> u32 {
        let Some(dst_file) = write.dsts_as_slice()[dst_idx].file() else {
            return 0;
        };

        match dst_file {
            RegFile::GPR => {
                let write_cat = RegLatencySM70::op_category(write, false, dst_idx);
                let read_cat = RegLatencySM70::op_category(read, true, src_idx);
                RegLatencySM70::write_after_read(read_cat, write_cat)
            }
            RegFile::Pred => {
                let write_cat = RegLatencySM70::op_category(write, false, dst_idx);
                let read_cat = RegLatencySM70::op_category(read, true, src_idx);
                RegLatencySM70::pred_write_after_read(read_cat, write_cat)
            }
            _ => panic!("SM70: unexpected dst register file in war()"),
        }
    }

    /// Write-after-write latency.
    pub fn waw(a: &Op, a_dst_idx: usize, b: &Op, b_dst_idx: usize, a_has_pred: bool) -> u32 {
        let Some(dst_file) = a.dsts_as_slice()[a_dst_idx].file() else {
            return 0;
        };

        match dst_file {
            RegFile::GPR => {
                let cat_a = RegLatencySM70::op_category(a, false, a_dst_idx);
                let cat_b = RegLatencySM70::op_category(b, false, b_dst_idx);
                RegLatencySM70::write_after_write(cat_a, cat_b, a_has_pred)
            }
            RegFile::Pred => {
                let cat_a = RegLatencySM70::op_category(a, false, a_dst_idx);
                let cat_b = RegLatencySM70::op_category(b, false, b_dst_idx);
                RegLatencySM70::pred_write_after_write(cat_a, cat_b, a_has_pred)
            }
            _ => panic!("SM70: unexpected dst register file in waw()"),
        }
    }
}
