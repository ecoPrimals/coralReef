// SPDX-License-Identifier: AGPL-3.0-only
// Copyright © 2025-2026 ecoPrimals
// Derived from Collabora, Ltd. (2022)

use super::*;

pub trait SSABuilder: Builder {
    fn alloc_ssa(&mut self, file: RegFile) -> SSAValue;
    fn alloc_ssa_vec(&mut self, file: RegFile, comps: u8) -> SSARef;

    fn shl(&mut self, x: Src, shift: Src) -> SSAValue {
        let dst = self.alloc_ssa(RegFile::GPR);
        if self.sm() >= 70 {
            self.push_op(OpShf {
                dst: dst.into(),
                srcs: [x, 0.into(), shift],
                right: false,
                wrap: true,
                data_type: IntType::I32,
                dst_high: false,
            });
        } else {
            self.push_op(OpShl {
                dst: dst.into(),
                srcs: [x, shift],
                wrap: true,
            });
        }
        dst
    }

    fn shl64(&mut self, x: Src, shift: Src) -> SSARef {
        let x = x
            .as_ssa()
            .expect("shl64 requires 64-bit SSA source (SSARef with 2 components)");
        debug_assert!(shift.is_unmodified());

        let dst = self.alloc_ssa_vec(RegFile::GPR, 2);
        if self.sm() >= 70 {
            self.push_op(OpShf {
                dst: dst[0].into(),
                srcs: [x[0].into(), 0.into(), shift.clone()],
                right: false,
                wrap: true,
                data_type: IntType::U64,
                dst_high: false,
            });
        } else {
            // On Maxwell and earlier, shf.l doesn't work without .high so we
            // have to use only the high parts, hard-coding the lower parts
            // to rZ
            self.push_op(OpShf {
                dst: dst[0].into(),
                srcs: [0.into(), x[0].into(), shift.clone()],
                right: false,
                wrap: true,
                data_type: IntType::U64,
                dst_high: true,
            });
        }
        self.push_op(OpShf {
            dst: dst[1].into(),
            srcs: [x[0].into(), x[1].into(), shift],
            right: false,
            wrap: true,
            data_type: IntType::U64,
            dst_high: true,
        });
        dst
    }

    fn shr(&mut self, x: Src, shift: Src, signed: bool) -> SSAValue {
        let dst = self.alloc_ssa(RegFile::GPR);
        if self.sm() >= 70 {
            self.push_op(OpShf {
                dst: dst.into(),
                srcs: [0.into(), x, shift],
                right: true,
                wrap: true,
                data_type: if signed { IntType::I32 } else { IntType::U32 },
                dst_high: true,
            });
        } else {
            self.push_op(OpShr {
                dst: dst.into(),
                srcs: [x, shift],
                wrap: true,
                signed,
            });
        }
        dst
    }

    fn shr64(&mut self, x: Src, shift: Src, signed: bool) -> SSARef {
        let x = x
            .as_ssa()
            .expect("shr64 requires 64-bit SSA source (SSARef with 2 components)");
        debug_assert!(shift.is_unmodified());

        let dst = self.alloc_ssa_vec(RegFile::GPR, 2);
        self.push_op(OpShf {
            dst: dst[0].into(),
            srcs: [x[0].into(), x[1].into(), shift.clone()],
            right: true,
            wrap: true,
            data_type: if signed { IntType::I64 } else { IntType::U64 },
            dst_high: false,
        });
        self.push_op(OpShf {
            dst: dst[1].into(),
            srcs: [0.into(), x[1].into(), shift],
            right: true,
            wrap: true,
            data_type: if signed { IntType::I64 } else { IntType::U64 },
            dst_high: true,
        });
        dst
    }

    fn urol(&mut self, x: Src, shift: Src) -> SSAValue {
        let dst = self.alloc_ssa(RegFile::GPR);
        assert!(self.sm() >= 32);

        self.push_op(OpShf {
            dst: dst.into(),
            srcs: [x.clone(), x, shift],
            right: false,
            wrap: true,
            data_type: IntType::U32,
            dst_high: true,
        });

        dst
    }

    fn uror(&mut self, x: Src, shift: Src) -> SSAValue {
        let dst = self.alloc_ssa(RegFile::GPR);
        assert!(self.sm() >= 32);

        self.push_op(OpShf {
            dst: dst.into(),
            srcs: [x.clone(), x, shift],
            right: true,
            wrap: true,
            data_type: IntType::U32,
            dst_high: false,
        });

        dst
    }

    fn fadd(&mut self, x: Src, y: Src) -> SSAValue {
        let dst = self.alloc_ssa(RegFile::GPR);
        self.push_op(OpFAdd {
            dst: dst.into(),
            srcs: [x, y],
            saturate: false,
            rnd_mode: FRndMode::NearestEven,
            ftz: false,
        });
        dst
    }

    fn fmul(&mut self, x: Src, y: Src) -> SSAValue {
        let dst = self.alloc_ssa(RegFile::GPR);
        self.push_op(OpFMul {
            dst: dst.into(),
            srcs: [x, y],
            saturate: false,
            rnd_mode: FRndMode::NearestEven,
            ftz: false,
            dnz: false,
        });
        dst
    }

    fn fset(&mut self, cmp_op: FloatCmpOp, x: Src, y: Src) -> SSAValue {
        let dst = self.alloc_ssa(RegFile::GPR);
        self.push_op(OpFSet {
            dst: dst.into(),
            cmp_op,
            srcs: [x, y],
            ftz: false,
        });
        dst
    }

    fn fsetp(&mut self, cmp_op: FloatCmpOp, x: Src, y: Src) -> SSAValue {
        let dst = self.alloc_ssa(RegFile::Pred);
        self.push_op(OpFSetP {
            dst: dst.into(),
            set_op: PredSetOp::And,
            cmp_op,
            srcs: [x, y, SrcRef::True.into()], // accum
            ftz: false,
        });
        dst
    }

    fn hadd2(&mut self, x: Src, y: Src) -> SSAValue {
        let dst = self.alloc_ssa(RegFile::GPR);
        self.push_op(OpHAdd2 {
            dst: dst.into(),
            srcs: [x, y],
            saturate: false,
            ftz: false,
            f32: false,
        });
        dst
    }

    fn hset2(&mut self, cmp_op: FloatCmpOp, x: Src, y: Src) -> SSARef {
        let dst = self.alloc_ssa(RegFile::GPR);
        self.push_op(OpHSet2 {
            dst: dst.into(),
            set_op: PredSetOp::And,
            cmp_op,
            srcs: [x, y, SrcRef::True.into()], // accum
            ftz: false,
        });
        dst.into()
    }

    fn dsetp(&mut self, cmp_op: FloatCmpOp, x: Src, y: Src) -> SSAValue {
        let dst = self.alloc_ssa(RegFile::Pred);
        self.push_op(OpDSetP {
            dst: dst.into(),
            set_op: PredSetOp::And,
            cmp_op,
            srcs: [x, y, SrcRef::True.into()], // accum
        });
        dst
    }

    fn iabs(&mut self, i: Src) -> SSAValue {
        let dst = self.alloc_ssa(RegFile::GPR);
        if self.sm() >= 70 {
            self.push_op(OpIAbs {
                dst: dst.into(),
                src: i,
            });
        } else {
            self.push_op(OpI2I {
                dst: dst.into(),
                src: i,
                src_type: IntType::I32,
                dst_type: IntType::I32,
                saturate: false,
                abs: true,
                neg: false,
            });
        }
        dst
    }

    fn iadd(&mut self, x: Src, y: Src, z: Src) -> SSAValue {
        let dst = self.alloc_ssa(RegFile::GPR);
        if self.sm() >= 70 {
            self.push_op(OpIAdd3 {
                dsts: [dst.into(), Dst::None, Dst::None],
                srcs: [x, y, z],
            });
        } else {
            assert!(z.is_zero());
            self.push_op(OpIAdd2 {
                dsts: [dst.into(), Dst::None],
                srcs: [x, y],
            });
        }
        dst
    }

    fn iadd64(&mut self, x: Src, y: Src, z: Src) -> SSARef {
        fn split_iadd64_src(src: Src) -> [Src; 2] {
            match src.reference {
                SrcRef::Zero => [0.into(), 0.into()],
                SrcRef::SSA(ssa) => {
                    if src.modifier.is_ineg() {
                        [Src::from(ssa[0]).ineg(), Src::from(ssa[1]).bnot()]
                    } else {
                        [Src::from(ssa[0]), Src::from(ssa[1])]
                    }
                }
                _ => panic!("Unsupported iadd64 source"),
            }
        }

        let is_3src = !x.is_zero() && !y.is_zero() && !z.is_zero();

        let [x0, x1] = split_iadd64_src(x);
        let [y0, y1] = split_iadd64_src(y);
        let dst = self.alloc_ssa_vec(RegFile::GPR, 2);
        if self.sm() >= 70 {
            let carry1 = self.alloc_ssa(RegFile::Pred);
            let (carry2_dst, carry2_src) = if is_3src {
                let carry2 = self.alloc_ssa(RegFile::Pred);
                (carry2.into(), carry2.into())
            } else {
                // If one of the sources is known to be zero, we only need one
                // carry predicate.
                (Dst::None, false.into())
            };

            let [z0, z1] = split_iadd64_src(z);
            self.push_op(OpIAdd3 {
                dsts: [dst[0].into(), carry1.into(), carry2_dst],
                srcs: [x0, y0, z0],
            });
            self.push_op(OpIAdd3X {
                dsts: [dst[1].into(), Dst::None, Dst::None],
                srcs: [x1, y1, z1, carry1.into(), carry2_src],
            });
        } else {
            assert!(z.is_zero());
            let carry = self.alloc_ssa(RegFile::Carry);
            self.push_op(OpIAdd2 {
                dsts: [dst[0].into(), carry.into()],
                srcs: [x0, y0],
            });
            self.push_op(OpIAdd2X {
                dsts: [dst[1].into(), Dst::None],
                srcs: [x1, y1, carry.into()],
            });
        }
        dst
    }

    fn imnmx(&mut self, tp: IntCmpType, x: Src, y: Src, min: Src) -> SSAValue {
        let dst = self.alloc_ssa(RegFile::GPR);
        self.push_op(OpIMnMx {
            dst: dst.into(),
            cmp_type: tp,
            srcs: [x, y, min],
        });
        dst
    }

    fn imul(&mut self, x: Src, y: Src) -> SSAValue {
        let dst = self.alloc_ssa(RegFile::GPR);
        if self.sm() >= 70 {
            self.push_op(OpIMad {
                dst: dst.into(),
                srcs: [x, y, 0.into()],
                signed: false,
            });
        } else {
            self.push_op(OpIMul {
                dst: dst.into(),
                srcs: [x, y],
                signed: [false; 2],
                high: false,
            });
        }
        dst
    }

    fn imul_2x32_64(&mut self, x: Src, y: Src, signed: bool) -> SSARef {
        let dst = self.alloc_ssa_vec(RegFile::GPR, 2);
        if self.sm() >= 70 {
            self.push_op(OpIMad64 {
                dst: dst.clone().into(),
                srcs: [x, y, 0.into()],
                signed,
            });
        } else {
            self.push_op(OpIMul {
                dst: dst[0].into(),
                srcs: [x.clone(), y.clone()],
                signed: [signed; 2],
                high: false,
            });
            self.push_op(OpIMul {
                dst: dst[1].into(),
                srcs: [x, y],
                signed: [signed; 2],
                high: true,
            });
        }
        dst
    }

    fn ineg(&mut self, i: Src) -> SSAValue {
        let dst = self.alloc_ssa(RegFile::GPR);
        if self.sm() >= 70 {
            self.push_op(OpIAdd3 {
                dsts: [dst.into(), Dst::None, Dst::None],
                srcs: [0.into(), i.ineg(), 0.into()],
            });
        } else {
            self.push_op(OpIAdd2 {
                dsts: [dst.into(), Dst::None],
                srcs: [0.into(), i.ineg()],
            });
        }
        dst
    }

    fn ineg64(&mut self, x: Src) -> SSARef {
        self.iadd64(0.into(), x.ineg(), 0.into())
    }

    fn isetp(&mut self, cmp_type: IntCmpType, cmp_op: IntCmpOp, x: Src, y: Src) -> SSAValue {
        let dst = self.alloc_ssa(RegFile::Pred);
        self.push_op(OpISetP {
            dst: dst.into(),
            set_op: PredSetOp::And,
            cmp_op,
            cmp_type,
            ex: false,
            srcs: [x, y, true.into(), true.into()],
        });
        dst
    }

    fn isetp64(&mut self, cmp_type: IntCmpType, cmp_op: IntCmpOp, x: Src, y: Src) -> SSARef {
        let x = x
            .as_ssa()
            .expect("isetp64 requires 64-bit SSA sources (SSARef with 2 components)");
        let y = y
            .as_ssa()
            .expect("isetp64 requires 64-bit SSA sources (SSARef with 2 components)");

        // Low bits are always an unsigned comparison
        let low = self.isetp(IntCmpType::U32, cmp_op, x[0].into(), y[0].into());

        let dst = self.alloc_ssa(RegFile::Pred);
        match cmp_op {
            IntCmpOp::False | IntCmpOp::True => {
                panic!("These don't make sense for the builder helper");
            }
            IntCmpOp::Eq | IntCmpOp::Ne => {
                self.push_op(OpISetP {
                    dst: dst.into(),
                    set_op: match cmp_op {
                        IntCmpOp::Eq => PredSetOp::And,
                        IntCmpOp::Ne => PredSetOp::Or,
                        _ => panic!("Not an integer equality"),
                    },
                    cmp_op,
                    cmp_type: IntCmpType::U32,
                    ex: false,
                    srcs: [x[1].into(), y[1].into(), low.into(), true.into()],
                });
            }
            IntCmpOp::Ge | IntCmpOp::Gt | IntCmpOp::Le | IntCmpOp::Lt => {
                if self.sm() >= 70 {
                    self.push_op(OpISetP {
                        dst: dst.into(),
                        set_op: PredSetOp::And,
                        cmp_op,
                        cmp_type,
                        ex: true,
                        srcs: [x[1].into(), y[1].into(), true.into(), low.into()],
                    });
                } else {
                    // On Maxwell, iset.ex doesn't do what we want so we need to
                    // do it with 3 comparisons.  Fortunately, we can chain them
                    // together and don't need the extra logic that other IR
                    // lowering would emit.
                    let low_and_high_eq = self.alloc_ssa(RegFile::Pred);
                    self.push_op(OpISetP {
                        dst: low_and_high_eq.into(),
                        set_op: PredSetOp::And,
                        cmp_op: IntCmpOp::Eq,
                        cmp_type: IntCmpType::U32,
                        ex: false,
                        srcs: [x[1].into(), y[1].into(), low.into(), true.into()],
                    });
                    self.push_op(OpISetP {
                        dst: dst.into(),
                        set_op: PredSetOp::Or,
                        // We always want a strict inequality for the high part
                        // so it's false when the two are equal and safe to OR
                        // with the low part.
                        cmp_op: match cmp_op {
                            IntCmpOp::Lt | IntCmpOp::Le => IntCmpOp::Lt,
                            IntCmpOp::Gt | IntCmpOp::Ge => IntCmpOp::Gt,
                            _ => panic!("Not an integer inequality"),
                        },
                        cmp_type,
                        ex: false,
                        srcs: [
                            x[1].into(),
                            y[1].into(),
                            low_and_high_eq.into(),
                            true.into(),
                        ],
                    });
                }
            }
        }
        dst.into()
    }

    fn lea(&mut self, a: Src, b: Src, shift: u8) -> SSAValue {
        let dst = self.alloc_ssa(RegFile::GPR);
        assert!(self.sm() >= 70);

        self.push_op(OpLea {
            dsts: [dst.into(), Dst::None],
            srcs: [a, b, 0.into()],
            dst_high: false,
            shift: shift % 32,
            intermediate_mod: SrcMod::None,
        });

        dst
    }

    fn lea64(&mut self, a: Src, b: Src, shift: u8) -> SSARef {
        assert!(self.sm() >= 70);
        assert!(a.is_unmodified());
        assert!(b.is_unmodified());

        let a = a
            .as_ssa()
            .expect("lea64 requires 64-bit SSA sources (SSARef with 2 components)");
        let b = b
            .as_ssa()
            .expect("lea64 requires 64-bit SSA sources (SSARef with 2 components)");
        let dst = self.alloc_ssa_vec(RegFile::GPR, 2);
        let shift = shift % 64;
        if shift >= 32 {
            self.copy_to(dst[0].into(), b[0].into());
            self.push_op(OpLea {
                dsts: [dst[1].into(), Dst::None],
                srcs: [a[0].into(), b[1].into(), 0.into()],
                dst_high: false,
                shift: shift - 32,
                intermediate_mod: SrcMod::None,
            });
        } else {
            let carry = self.alloc_ssa(RegFile::Pred);
            self.push_op(OpLea {
                dsts: [dst[0].into(), carry.into()],
                srcs: [a[0].into(), b[0].into(), 0.into()],
                dst_high: false,
                shift,
                intermediate_mod: SrcMod::None,
            });
            self.push_op(OpLeaX {
                dsts: [dst[1].into(), Dst::None],
                srcs: [a[0].into(), b[1].into(), a[1].into(), carry.into()],
                dst_high: true,
                shift,
                intermediate_mod: SrcMod::None,
            });
        }
        dst
    }

    fn lop2(&mut self, op: LogicOp2, x: Src, y: Src) -> SSAValue {
        let dst = if x.is_predicate() {
            self.alloc_ssa(RegFile::Pred)
        } else {
            self.alloc_ssa(RegFile::GPR)
        };
        self.lop2_to(dst.into(), op, x, y);
        dst
    }

    fn brev(&mut self, x: Src) -> SSAValue {
        let dst = self.alloc_ssa(RegFile::GPR);
        if self.sm() >= 70 {
            self.push_op(OpBRev {
                dst: dst.into(),
                src: x,
            });
        } else {
            // No BREV in Maxwell
            self.push_op(OpBfe {
                dst: dst.into(),
                srcs: [x, Src::new_imm_u32(0x2000)],
                signed: false,
                reverse: true,
            });
        }
        dst
    }

    fn transcendental(&mut self, op: TranscendentalOp, src: Src) -> SSAValue {
        let dst = self.alloc_ssa(RegFile::GPR);
        self.push_op(OpTranscendental {
            dst: dst.into(),
            op,
            src,
        });
        dst
    }

    fn fsin(&mut self, src: Src) -> SSAValue {
        let tmp = if self.sm() >= 70 {
            let frac_1_2pi = 1.0 / (2.0 * std::f32::consts::PI);
            self.fmul(src, frac_1_2pi.into())
        } else {
            let tmp = self.alloc_ssa(RegFile::GPR);
            self.push_op(OpRro {
                dst: tmp.into(),
                op: RroOp::SinCos,
                src,
            });
            tmp
        };
        self.transcendental(TranscendentalOp::Sin, tmp.into())
    }

    fn fcos(&mut self, src: Src) -> SSAValue {
        let tmp = if self.sm() >= 70 {
            let frac_1_2pi = 1.0 / (2.0 * std::f32::consts::PI);
            self.fmul(src, frac_1_2pi.into())
        } else {
            let tmp = self.alloc_ssa(RegFile::GPR);
            self.push_op(OpRro {
                dst: tmp.into(),
                op: RroOp::SinCos,
                src,
            });
            tmp
        };
        self.transcendental(TranscendentalOp::Cos, tmp.into())
    }

    fn fexp2(&mut self, src: Src) -> SSAValue {
        let tmp = if self.sm() >= 70 {
            src
        } else {
            let tmp = self.alloc_ssa(RegFile::GPR);
            self.push_op(OpRro {
                dst: tmp.into(),
                op: RroOp::Exp2,
                src,
            });
            tmp.into()
        };
        self.transcendental(TranscendentalOp::Exp2, tmp)
    }

    fn prmt(&mut self, x: Src, y: Src, sel: [u8; 4]) -> SSAValue {
        let dst = self.alloc_ssa(RegFile::GPR);
        self.prmt_to(dst.into(), x, y, sel);
        dst
    }

    fn prmt4(&mut self, src: [Src; 4], sel: [u8; 4]) -> SSAValue {
        // Infallible: sel has 4 elements, so iter().max() always returns Some.
        let max_sel = *sel
            .iter()
            .max()
            .expect("sel has 4 elements; max() is never None");
        let [src0, src1, src2, src3] = src;
        if max_sel < 8 {
            self.prmt(src0, src1, sel)
        } else if max_sel < 12 {
            let mut sel_a = [0_u8; 4];
            let mut sel_b = [0_u8; 4];
            for i in 0..4_u8 {
                if sel[usize::from(i)] < 8 {
                    sel_a[usize::from(i)] = sel[usize::from(i)];
                    sel_b[usize::from(i)] = i;
                } else {
                    sel_b[usize::from(i)] = (sel[usize::from(i)] - 8) + 4;
                }
            }
            let a = self.prmt(src0, src1, sel_a);
            self.prmt(a.into(), src2, sel_b)
        } else if max_sel < 16 {
            let mut sel_a = [0_u8; 4];
            let mut sel_b = [0_u8; 4];
            let mut sel_c = [0_u8; 4];
            for i in 0..4_u8 {
                if sel[usize::from(i)] < 8 {
                    sel_a[usize::from(i)] = sel[usize::from(i)];
                    sel_c[usize::from(i)] = i;
                } else {
                    sel_b[usize::from(i)] = sel[usize::from(i)] - 8;
                    sel_c[usize::from(i)] = 4 + i;
                }
            }
            let a = self.prmt(src0, src1, sel_a);
            let b = self.prmt(src2, src3, sel_b);
            self.prmt(a.into(), b.into(), sel_c)
        } else {
            panic!("Invalid permute value: {max_sel}");
        }
    }

    fn sel(&mut self, cond: Src, x: Src, y: Src) -> SSAValue {
        assert!(cond.reference.is_predicate());
        assert!(x.is_predicate() == y.is_predicate());
        if x.is_predicate() {
            let dst = self.alloc_ssa(RegFile::Pred);
            if self.sm() >= 70 {
                self.push_op(OpPLop3 {
                    dsts: [dst.into(), Dst::None],
                    srcs: [cond, x, y],
                    ops: [
                        LogicOp3::new_lut(&|c, x, y| (c & x) | (!c & y)),
                        LogicOp3::new_const(false),
                    ],
                });
            } else {
                let tmp = self.alloc_ssa(RegFile::Pred);
                self.push_op(OpPSetP {
                    dsts: [tmp.into(), Dst::None],
                    ops: [PredSetOp::And, PredSetOp::And],
                    srcs: [cond.clone(), x, true.into()],
                });
                self.push_op(OpPSetP {
                    dsts: [dst.into(), Dst::None],
                    ops: [PredSetOp::And, PredSetOp::Or],
                    srcs: [cond.bnot(), y, tmp.into()],
                });
            }
            dst
        } else {
            let dst = self.alloc_ssa(RegFile::GPR);
            self.push_op(OpSel {
                dst: dst.into(),
                srcs: [cond, x, y],
            });
            dst
        }
    }

    fn undef(&mut self) -> SSAValue {
        let dst = self.alloc_ssa(RegFile::GPR);
        self.push_op(OpUndef { dst: dst.into() });
        dst
    }

    fn copy(&mut self, src: Src) -> SSAValue {
        let dst = if src.is_predicate() {
            self.alloc_ssa(RegFile::Pred)
        } else {
            self.alloc_ssa(RegFile::GPR)
        };
        self.copy_to(dst.into(), src);
        dst
    }

    fn bmov_to_bar(&mut self, src: Src) -> SSAValue {
        let src_ssa = src
            .reference
            .as_ssa()
            .expect("bmov_to_bar requires SSA GPR source");
        assert!(src_ssa.file() == RegFile::GPR);
        let dst = self.alloc_ssa(RegFile::Bar);
        self.push_op(OpBMov {
            dst: dst.into(),
            src,
            clear: false,
        });
        dst
    }

    fn bmov_to_gpr(&mut self, src: Src) -> SSAValue {
        let src_ssa = src
            .reference
            .as_ssa()
            .expect("bmov_to_gpr requires SSA Bar source");
        assert!(src_ssa.file() == RegFile::Bar);
        let dst = self.alloc_ssa(RegFile::GPR);
        self.push_op(OpBMov {
            dst: dst.into(),
            src,
            clear: false,
        });
        dst
    }
}
