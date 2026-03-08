// SPDX-License-Identifier: AGPL-3.0-only
// Copyright © 2025-2026 ecoPrimals
// Derived from Collabora, Ltd. (2022)

#![allow(clippy::wildcard_imports)]

use super::{
    api::{DEBUG, GetDebugFlags},
    ir::*,
};

use std::cmp::max;

struct LowerCopySwap {
    shared_local_mem_start: u32,
    shared_local_mem_size: u32,
}

impl LowerCopySwap {
    fn new(shared_local_mem_size: u32) -> Self {
        Self {
            shared_local_mem_start: shared_local_mem_size,
            shared_local_mem_size,
        }
    }

    fn lower_copy(&mut self, b: &mut impl Builder, copy: OpCopy) {
        let dst_reg = copy.dst.as_reg().unwrap_or_else(|| {
            panic!(
                "lower_copy_swap: OpCopy dst {dst} is not a Reg (must run after RA). src={src}",
                dst = copy.dst,
                src = copy.src,
            )
        });
        assert!(dst_reg.comps() == 1, "lower_copy_swap: multi-component dst");
        assert!(
            copy.src.is_uniform() || !dst_reg.is_uniform(),
            "lower_copy_swap: non-uniform src to uniform dst"
        );

        if !copy.src.is_unmodified() {
            if dst_reg.is_predicate() {
                match (&copy.src.reference, copy.src.modifier.is_bnot()) {
                    (SrcRef::True, false) | (SrcRef::False | SrcRef::Zero, true) => {
                        b.lop2_to(
                            copy.dst,
                            LogicOp2::PassB,
                            Src::new_imm_bool(true),
                            Src::new_imm_bool(true),
                        );
                    }
                    (SrcRef::False | SrcRef::Zero, false) | (SrcRef::True, true) => {
                        b.lop2_to(
                            copy.dst,
                            LogicOp2::PassB,
                            Src::new_imm_bool(true),
                            Src::new_imm_bool(false),
                        );
                    }
                    (SrcRef::Reg(reg), _) if reg.is_predicate() => {
                        b.lop2_to(copy.dst, LogicOp2::PassB, Src::new_imm_bool(true), copy.src);
                    }
                    (SrcRef::Reg(reg), is_bnot) if !reg.is_predicate() => {
                        let cmp_op = if is_bnot { IntCmpOp::Eq } else { IntCmpOp::Ne };
                        b.push_op(OpISetP {
                            dst: copy.dst,
                            set_op: PredSetOp::And,
                            cmp_op,
                            cmp_type: IntCmpType::U32,
                            ex: false,
                            srcs: [SrcRef::Reg(*reg).into(), Src::ZERO],
                            accum: SrcRef::True.into(),
                            low_cmp: SrcRef::False.into(),
                        });
                    }
                    (src_ref, bnot) => panic!(
                        "lower_copy_swap: cannot copy modified source to Pred: src_ref={src_ref}, bnot={bnot}"
                    ),
                }
            } else {
                b.push_op(OpMov {
                    dst: copy.dst,
                    src: copy.src,
                    quad_lanes: 0xf,
                });
            }
            return;
        }

        match dst_reg.file() {
            RegFile::GPR | RegFile::UGPR => match copy.src.reference {
                SrcRef::Zero | SrcRef::Imm32(_) => {
                    b.push_op(OpMov {
                        dst: copy.dst,
                        src: copy.src,
                        quad_lanes: 0xf,
                    });
                }
                SrcRef::CBuf(_) => match dst_reg.file() {
                    RegFile::GPR => {
                        if b.is_amd() || b.sm() >= 100 {
                            b.push_op(OpLdc {
                                dst: copy.dst,
                                cb: copy.src,
                                offset: 0.into(),
                                mode: LdcMode::Indexed,
                                mem_type: MemType::B32,
                            });
                        } else {
                            b.push_op(OpMov {
                                dst: copy.dst,
                                src: copy.src,
                                quad_lanes: 0xf,
                            });
                        }
                    }
                    RegFile::UGPR => {
                        b.push_op(OpLdc {
                            dst: copy.dst,
                            cb: copy.src,
                            offset: 0.into(),
                            mode: LdcMode::Indexed,
                            mem_type: MemType::B32,
                        });
                    }
                    other => {
                        panic!("lower_copy_swap: CBuf→{other} not supported (only GPR/UGPR)")
                    }
                },
                SrcRef::True => {
                    b.push_op(OpMov {
                        dst: copy.dst,
                        src: Src::new_imm_u32(1),
                        quad_lanes: 0xf,
                    });
                }
                SrcRef::False => {
                    b.push_op(OpMov {
                        dst: copy.dst,
                        src: Src::ZERO,
                        quad_lanes: 0xf,
                    });
                }
                SrcRef::Reg(src_reg) => match src_reg.file() {
                    RegFile::GPR | RegFile::UGPR => {
                        b.push_op(OpMov {
                            dst: copy.dst,
                            src: copy.src,
                            quad_lanes: 0xf,
                        });
                    }
                    RegFile::Pred | RegFile::UPred => {
                        b.push_op(OpSel {
                            dst: copy.dst,
                            cond: copy.src,
                            srcs: [Src::new_imm_u32(1), Src::ZERO],
                        });
                    }
                    RegFile::Bar => {
                        b.push_op(OpBMov {
                            dst: copy.dst,
                            src: copy.src,
                            clear: false,
                        });
                    }
                    RegFile::Mem => {
                        let access = MemAccess {
                            mem_type: MemType::B32,
                            space: MemSpace::Local,
                            order: MemOrder::Strong(MemScope::CTA),
                            eviction_priority: MemEvictionPriority::Normal,
                        };
                        let addr = self.shared_local_mem_start + src_reg.base_idx() * 4;
                        self.shared_local_mem_size = max(self.shared_local_mem_size, addr + 4);
                        b.push_op(OpLd {
                            dst: copy.dst,
                            addr: Src::ZERO,
                            offset: addr
                                .try_into()
                                .expect("SLM offset fits in i32; addr from base_idx*4"),
                            stride: OffsetStride::X1,
                            access,
                        });
                    }
                    RegFile::Carry => panic!("lower_copy_swap: Carry→GPR copy not supported"),
                },
                SrcRef::SSA(ssa) => panic!("lower_copy_swap: SSA {ssa}→GPR (must run after RA)"),
            },
            RegFile::Pred | RegFile::UPred => match copy.src.reference {
                SrcRef::Zero | SrcRef::Imm32(_) | SrcRef::CBuf(_) => {
                    panic!(
                        "lower_copy_swap: {src}→Pred not supported (need ISetP coercion)",
                        src = copy.src.reference,
                    );
                }
                SrcRef::True => {
                    b.lop2_to(
                        copy.dst,
                        LogicOp2::PassB,
                        Src::new_imm_bool(true),
                        Src::new_imm_bool(true),
                    );
                }
                SrcRef::False => {
                    b.lop2_to(
                        copy.dst,
                        LogicOp2::PassB,
                        Src::new_imm_bool(true),
                        Src::new_imm_bool(false),
                    );
                }
                SrcRef::Reg(src_reg) => match src_reg.file() {
                    RegFile::Pred => {
                        b.lop2_to(copy.dst, LogicOp2::PassB, Src::new_imm_bool(true), copy.src);
                    }
                    RegFile::UPred => {
                        // PLOP3 supports a UPred in src[2]
                        b.push_op(OpPLop3 {
                            dsts: [copy.dst, Dst::None],
                            srcs: [true.into(), true.into(), copy.src],
                            ops: [LogicOp3::new_lut(&|_, _, z| z), LogicOp3::new_const(false)],
                        });
                    }
                    other => {
                        panic!("lower_copy_swap: {other}→Pred not supported (need coercion pass)")
                    }
                },
                SrcRef::SSA(ssa) => panic!("lower_copy_swap: SSA {ssa}→Pred (must run after RA)"),
            },
            RegFile::Bar => match copy.src.reference {
                SrcRef::Reg(src_reg) => match src_reg.file() {
                    RegFile::GPR | RegFile::UGPR => {
                        b.push_op(OpBMov {
                            dst: copy.dst,
                            src: copy.src,
                            clear: false,
                        });
                    }
                    other => panic!("lower_copy_swap: {other}→Bar not supported"),
                },
                other => panic!("lower_copy_swap: {other}→Bar not supported"),
            },
            RegFile::Mem => match copy.src.reference {
                SrcRef::Reg(src_reg) => match src_reg.file() {
                    RegFile::GPR => {
                        let access = MemAccess {
                            mem_type: MemType::B32,
                            space: MemSpace::Local,
                            order: MemOrder::Strong(MemScope::CTA),
                            eviction_priority: MemEvictionPriority::Normal,
                        };
                        let addr = self.shared_local_mem_start + dst_reg.base_idx() * 4;
                        self.shared_local_mem_size = max(self.shared_local_mem_size, addr + 4);
                        b.push_op(OpSt {
                            addr: Src::ZERO,
                            data: copy.src,
                            offset: addr
                                .try_into()
                                .expect("SLM offset fits in i32; addr from base_idx*4"),
                            stride: OffsetStride::X1,
                            access,
                        });
                    }
                    other => panic!("lower_copy_swap: {other}→Mem not supported"),
                },
                other => panic!("lower_copy_swap: {other}→Mem not supported"),
            },
            RegFile::Carry => panic!("lower_copy_swap: Carry dst not supported"),
        }
    }

    fn lower_r2ur(&mut self, b: &mut impl Builder, r2ur: OpR2UR) {
        assert!(r2ur.src.is_unmodified());
        if r2ur.src.is_uniform() {
            let copy = OpCopy {
                dst: r2ur.dst,
                src: r2ur.src,
            };
            self.lower_copy(b, copy);
        } else {
            let src_file = r2ur
                .src
                .reference
                .as_reg()
                .expect("r2ur non-uniform src is Reg")
                .file();
            let dst_file = r2ur.dst.as_reg().expect("r2ur dst is always Reg").file();
            match src_file {
                RegFile::GPR => {
                    assert!(dst_file == RegFile::UGPR);
                    b.push_op(r2ur);
                }
                RegFile::Pred => {
                    assert!(dst_file == RegFile::UPred);
                    // It doesn't matter what channel we take
                    b.push_op(OpVote {
                        op: VoteOp::Any,
                        ballot: Dst::None,
                        vote: r2ur.dst,
                        pred: r2ur.src,
                    });
                }
                other => {
                    panic!("lower_copy_swap: R2UR from {other:?} has no matching uniform file")
                }
            }
        }
    }

    fn lower_swap(&self, b: &mut impl Builder, swap: OpSwap) {
        let x = *swap.dsts[0]
            .as_reg()
            .expect("OpSwap dsts are always Reg (two register refs)");
        let y = *swap.dsts[1]
            .as_reg()
            .expect("OpSwap dsts are always Reg (two register refs)");

        assert!(x.file() == y.file());
        assert!(x.file() != RegFile::Mem);
        assert!(x.comps() == 1 && y.comps() == 1);
        assert!(swap.srcs[0].is_unmodified());
        assert!(
            *swap.srcs[0]
                .reference
                .as_reg()
                .expect("OpSwap srcs mirror dsts; both are Reg")
                == y
        );
        assert!(swap.srcs[1].is_unmodified());
        assert!(
            *swap.srcs[1]
                .reference
                .as_reg()
                .expect("OpSwap srcs mirror dsts; both are Reg")
                == x
        );

        if x == y {
            // Nothing to do
        } else if x.is_predicate() && b.sm() >= 70 {
            b.push_op(OpPLop3 {
                dsts: [x.into(), y.into()],
                srcs: [x.into(), y.into(), Src::new_imm_bool(true)],
                ops: [
                    LogicOp3::new_lut(&|_, y, _| y),
                    LogicOp3::new_lut(&|x, _, _| x),
                ],
            });
        } else {
            b.lop2_to(x.into(), LogicOp2::Xor, x.into(), y.into());
            b.lop2_to(y.into(), LogicOp2::Xor, x.into(), y.into());
            b.lop2_to(x.into(), LogicOp2::Xor, x.into(), y.into());
        }
    }

    fn run(&mut self, s: &mut Shader) {
        let sm = s.sm;
        s.map_instrs(|instr: Instr, _| -> MappedInstrs {
            match instr.op {
                Op::R2UR(r2ur) => {
                    debug_assert!(instr.pred.is_true());
                    let mut b = InstrBuilder::new(sm);
                    if DEBUG.annotate() {
                        b.push_instr(Instr::new(OpAnnotate {
                            annotation: "r2ur lowered by lower_copy_swap".into(),
                        }));
                    }
                    self.lower_r2ur(&mut b, *r2ur);
                    b.into_mapped_instrs()
                }
                Op::Copy(copy) => {
                    debug_assert!(instr.pred.is_true());
                    let mut b = InstrBuilder::new(sm);
                    if DEBUG.annotate() {
                        b.push_instr(Instr::new(OpAnnotate {
                            annotation: "copy lowered by lower_copy_swap".into(),
                        }));
                    }
                    self.lower_copy(&mut b, *copy);
                    b.into_mapped_instrs()
                }
                Op::Swap(swap) => {
                    debug_assert!(instr.pred.is_true());
                    let mut b = InstrBuilder::new(sm);
                    if DEBUG.annotate() {
                        b.push_instr(Instr::new(OpAnnotate {
                            annotation: "swap lowered by lower_copy_swap".into(),
                        }));
                    }
                    self.lower_swap(&mut b, *swap);
                    b.into_mapped_instrs()
                }
                _ => MappedInstrs::One(instr),
            }
        });
    }
}

impl Shader<'_> {
    pub fn lower_copy_swap(&mut self) {
        let mut pass = LowerCopySwap::new(self.info.shared_local_mem_size);
        pass.run(self);
        self.info.shared_local_mem_size = pass.shared_local_mem_size;
    }
}
