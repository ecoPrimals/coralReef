// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright © 2025-2026 ecoPrimals
// Derived from Collabora, Ltd. (2023)

use super::*;

impl SM50Op for OpBfe {
    fn legalize(&mut self, b: &mut LegalizeBuilder) {
        use RegFile::GPR;
        b.copy_alu_src_if_not_reg(self.base_mut(), GPR, SrcType::ALU);
    }

    fn encode(&self, e: &mut SM50Encoder<'_>) {
        match &self.range().reference {
            SrcRef::Zero | SrcRef::Reg(_) => {
                e.set_opcode(0x5c00);
                e.set_reg_src(20..28, self.range());
            }
            SrcRef::Imm32(imm32) => {
                e.set_opcode(0x3800);
                // Only the bottom 16 bits of the immediate matter
                e.set_src_imm_i20(20..39, 56, *imm32 & 0xffff);
            }
            SrcRef::CBuf(cbuf) => {
                e.set_opcode(0x4c00);
                e.set_src_cb(20..39, cbuf);
            }
            src => crate::codegen::ice!("Invalid bfe range: {src}"),
        }

        if self.signed {
            e.set_bit(48, true);
        }

        if self.reverse {
            e.set_bit(40, true);
        }

        e.set_reg_src(8..16, self.base());
        e.set_dst(&self.dst);
    }
}

impl SM50Op for OpBRev {
    fn legalize(&mut self, b: &mut LegalizeBuilder) {
        use RegFile::GPR;
        b.copy_alu_src_if_i20_overflow(&mut self.src, GPR, SrcType::ALU);
    }

    fn encode(&self, e: &mut SM50Encoder<'_>) {
        e.set_opcode(0x3800);
        e.set_src_imm_i20(20..39, 56, 0x2000);
        e.set_bit(40, true);
        e.set_reg_src(8..16, &self.src);
        e.set_dst(&self.dst);
    }
}

impl SM50Op for OpFlo {
    fn legalize(&mut self, b: &mut LegalizeBuilder) {
        use RegFile::GPR;
        b.copy_alu_src_if_i20_overflow(&mut self.src, GPR, SrcType::ALU);
    }

    fn encode(&self, e: &mut SM50Encoder<'_>) {
        match &self.src.reference {
            SrcRef::Zero | SrcRef::Reg(_) => {
                e.set_opcode(0x5c30);
                e.set_reg_src_ref(20..28, &self.src.reference);
            }
            SrcRef::Imm32(imm32) => {
                e.set_opcode(0x3830);
                e.set_src_imm_i20(20..39, 56, *imm32);
                assert!(self.src.is_unmodified());
            }
            SrcRef::CBuf(cb) => {
                e.set_opcode(0x4c30);
                e.set_src_cb(20..39, cb);
            }
            src => crate::codegen::ice!("Invalid flo src: {src}"),
        }

        e.set_dst(&self.dst);
        e.set_bit(40, self.src.modifier.is_bnot());
        e.set_bit(48, self.signed);
        e.set_bit(41, self.return_shift_amount);
        e.set_bit(47, false); /* dst.CC */
    }
}

impl SM50Op for OpIAdd2 {
    fn legalize(&mut self, b: &mut LegalizeBuilder) {
        use RegFile::GPR;
        let carry_out_none = self.carry_out().is_none();
        let [src0, src1] = &mut self.srcs;
        swap_srcs_if_not_reg(src0, src1, GPR);
        if src0.modifier.is_ineg() && src1.modifier.is_ineg() {
            assert!(carry_out_none);
            b.copy_alu_src_and_lower_ineg(src0, GPR, SrcType::I32);
        }
        b.copy_alu_src_if_not_reg(src0, GPR, SrcType::I32);
        if !carry_out_none {
            b.copy_alu_src_if_ineg_imm(src1, GPR, SrcType::I32);
        }
    }

    fn encode(&self, e: &mut SM50Encoder<'_>) {
        // Hardware requires at least one of these be unmodified.  Otherwise, it
        // encodes as iadd.po which isn't what we want.
        assert!(self.srcs[0].is_unmodified() || self.srcs[1].is_unmodified());

        let carry_out = match self.carry_out() {
            Dst::Reg(reg) if reg.file() == RegFile::Carry => true,
            Dst::None => false,
            dst => crate::codegen::ice!("Invalid iadd carry_out: {dst}"),
        };

        if let Some(imm32) = self.srcs[1].as_imm_not_i20() {
            e.set_opcode(0x1c00);

            e.set_dst(self.dst());
            e.set_reg_ineg_src(8..16, 56, &self.srcs[0]);
            e.set_src_imm32(20..52, imm32);

            e.set_bit(52, carry_out);
            e.set_bit(53, false); // .X
        } else {
            match &self.srcs[1].reference {
                SrcRef::Zero | SrcRef::Reg(_) => {
                    e.set_opcode(0x5c10);
                    e.set_reg_ineg_src(20..28, 48, &self.srcs[1]);
                }
                SrcRef::Imm32(imm32) => {
                    e.set_opcode(0x3810);
                    e.set_src_imm_i20(20..39, 56, *imm32);
                    assert!(self.srcs[1].is_unmodified());
                }
                SrcRef::CBuf(_) => {
                    e.set_opcode(0x4c10);
                    e.set_cb_ineg_src(20..39, 48, &self.srcs[1]);
                }
                src => crate::codegen::ice!("Invalid iadd src1: {src}"),
            }

            e.set_dst(self.dst());
            e.set_reg_ineg_src(8..16, 49, &self.srcs[0]);

            e.set_bit(43, false); // .X
            e.set_bit(47, carry_out);
        }
    }
}

impl SM50Op for OpIAdd2X {
    fn legalize(&mut self, b: &mut LegalizeBuilder) {
        use RegFile::GPR;
        let [src0, src1, _carry_in] = &mut self.srcs;
        swap_srcs_if_not_reg(src0, src1, GPR);
        b.copy_alu_src_if_not_reg(src0, GPR, SrcType::I32);
    }

    fn encode(&self, e: &mut SM50Encoder<'_>) {
        match &self.carry_in().reference {
            SrcRef::Reg(reg) if reg.file() == RegFile::Carry => (),
            src => crate::codegen::ice!("Invalid iadd.x carry_in: {src}"),
        }

        let carry_out = match self.carry_out() {
            Dst::Reg(reg) if reg.file() == RegFile::Carry => true,
            Dst::None => false,
            dst => crate::codegen::ice!("Invalid iadd.x carry_out: {dst}"),
        };

        if let Some(imm32) = self.srcs[1].as_imm_not_i20() {
            e.set_opcode(0x1c00);

            e.set_dst(self.dst());
            e.set_reg_bnot_src(8..16, 56, &self.srcs[0]);
            e.set_src_imm32(20..52, imm32);

            e.set_bit(52, carry_out);
            e.set_bit(53, true); // .X
        } else {
            match &self.srcs[1].reference {
                SrcRef::Zero | SrcRef::Reg(_) => {
                    e.set_opcode(0x5c10);
                    e.set_reg_bnot_src(20..28, 48, &self.srcs[1]);
                }
                SrcRef::Imm32(imm32) => {
                    e.set_opcode(0x3810);
                    e.set_src_imm_i20(20..39, 56, *imm32);
                    assert!(self.srcs[1].is_unmodified());
                }
                SrcRef::CBuf(_) => {
                    e.set_opcode(0x4c10);
                    e.set_cb_bnot_src(20..39, 48, &self.srcs[1]);
                }
                src => crate::codegen::ice!("Invalid iadd.x src1: {src}"),
            }

            e.set_dst(self.dst());
            e.set_reg_bnot_src(8..16, 49, &self.srcs[0]);

            e.set_bit(43, true); // .X
            e.set_bit(47, carry_out);
        }
    }
}

impl SM50Op for OpIMad {
    fn legalize(&mut self, b: &mut LegalizeBuilder) {
        use RegFile::GPR;
        let [src0, src1, src2] = &mut self.srcs;
        swap_srcs_if_not_reg(src0, src1, GPR);
        b.copy_alu_src_if_not_reg(src0, GPR, SrcType::ALU);
        b.copy_alu_src_if_i20_overflow(src1, GPR, SrcType::ALU);
        if src_is_reg(src1, GPR) {
            b.copy_alu_src_if_imm(src2, GPR, SrcType::ALU);
        } else {
            b.copy_alu_src_if_not_reg(src2, GPR, SrcType::ALU);
        }
    }

    fn encode(&self, e: &mut SM50Encoder<'_>) {
        // There is one ineg bit shared by the two imul sources
        let ineg_imul = self.srcs[0].modifier.is_ineg() ^ self.srcs[1].modifier.is_ineg();
        let ineg_src2 = self.srcs[2].modifier.is_ineg();

        match &self.srcs[2].reference {
            SrcRef::Zero | SrcRef::Reg(_) => {
                match &self.srcs[1].reference {
                    SrcRef::Zero | SrcRef::Reg(_) => {
                        e.set_opcode(0x5a00);
                        e.set_reg_src_ref(20..28, &self.srcs[1].reference);
                    }
                    SrcRef::Imm32(imm32) => {
                        e.set_opcode(0x3400);
                        e.set_src_imm_i20(20..39, 56, *imm32);
                    }
                    SrcRef::CBuf(cb) => {
                        e.set_opcode(0x4a00);
                        e.set_src_cb(20..39, cb);
                    }
                    src => crate::codegen::ice!("Invalid imad src1: {src}"),
                }

                e.set_reg_src_ref(39..47, &self.srcs[2].reference);
            }
            SrcRef::CBuf(cb) => {
                e.set_opcode(0x5200);
                e.set_src_cb(20..39, cb);
                e.set_reg_src_ref(39..47, &self.srcs[1].reference);
            }
            src => crate::codegen::ice!("Invalid imad src2: {src}"),
        }

        e.set_dst(&self.dst);
        e.set_reg_src(8..16, &self.srcs[0]);

        e.set_bit(48, self.signed); // src0 signed
        e.set_bit(51, ineg_imul);
        e.set_bit(52, ineg_src2);
        e.set_bit(53, self.signed); // src1 signed
    }
}

impl SM50Op for OpIMul {
    fn legalize(&mut self, b: &mut LegalizeBuilder) {
        use RegFile::GPR;
        let [src0, src1] = &mut self.srcs;
        if swap_srcs_if_not_reg(src0, src1, GPR) {
            self.signed.swap(0, 1);
        }
        b.copy_alu_src_if_not_reg(src0, GPR, SrcType::ALU);
    }

    fn encode(&self, e: &mut SM50Encoder<'_>) {
        assert!(self.srcs[0].is_unmodified());
        assert!(self.srcs[1].is_unmodified());

        if let Some(i) = self.srcs[1].as_imm_not_i20() {
            e.set_opcode(0x1fc0);
            e.set_src_imm32(20..52, i);

            e.set_bit(53, self.high);
            e.set_bit(54, self.signed[0]);
            e.set_bit(55, self.signed[1]);
        } else {
            match &self.srcs[1].reference {
                SrcRef::Zero | SrcRef::Reg(_) => {
                    e.set_opcode(0x5c38);
                    e.set_reg_src(20..28, &self.srcs[1]);
                }
                SrcRef::Imm32(imm32) => {
                    e.set_opcode(0x3838);
                    e.set_src_imm_i20(20..39, 56, *imm32);
                }
                SrcRef::CBuf(cb) => {
                    e.set_opcode(0x4c38);
                    e.set_src_cb(20..39, cb);
                }
                src => crate::codegen::ice!("Invalid imul src1: {src}"),
            }

            e.set_bit(39, self.high);
            e.set_bit(40, self.signed[0]);
            e.set_bit(41, self.signed[1]);
        }

        e.set_dst(&self.dst);
        e.set_reg_src(8..16, &self.srcs[0]);
    }
}

impl SM50Op for OpIMnMx {
    fn legalize(&mut self, b: &mut LegalizeBuilder) {
        use RegFile::GPR;
        let [src0, src1, _min] = &mut self.srcs;
        swap_srcs_if_not_reg(src0, src1, GPR);
        b.copy_alu_src_if_not_reg(src0, GPR, SrcType::ALU);
        b.copy_alu_src_if_i20_overflow(src1, GPR, SrcType::ALU);
    }

    fn encode(&self, e: &mut SM50Encoder<'_>) {
        match &self.srcs[1].reference {
            SrcRef::Zero | SrcRef::Reg(_) => {
                e.set_opcode(0x5c20);
                e.set_reg_src(20..28, &self.srcs[1]);
            }
            SrcRef::Imm32(imm32) => {
                e.set_opcode(0x3820);
                e.set_src_imm_i20(20..39, 56, *imm32);
                assert!(self.srcs[1].is_unmodified());
            }
            SrcRef::CBuf(cb) => {
                e.set_opcode(0x4c20);
                e.set_src_cb(20..39, cb);
            }
            src => crate::codegen::ice!("Invalid imnmx src1: {src}"),
        }

        e.set_dst(&self.dst);
        e.set_reg_src(8..16, &self.srcs[0]);
        e.set_pred_src(39..42, 42, self.min());
        e.set_bit(47, false); // .CC
        e.set_bit(
            48,
            match self.cmp_type {
                IntCmpType::U32 => false,
                IntCmpType::I32 => true,
            },
        );
    }
}

impl SM50Op for OpISetP {
    fn legalize(&mut self, b: &mut LegalizeBuilder) {
        use RegFile::GPR;
        let [src0, src1, _accum, _low_cmp] = &mut self.srcs;
        if swap_srcs_if_not_reg(src0, src1, GPR) {
            self.cmp_op = self.cmp_op.flip();
        }
        b.copy_alu_src_if_not_reg(src0, GPR, SrcType::ALU);
        b.copy_alu_src_if_pred(src1, GPR, SrcType::ALU);
        b.copy_alu_src_if_i20_overflow(src1, GPR, SrcType::ALU);
    }

    fn encode(&self, e: &mut SM50Encoder<'_>) {
        match &self.srcs[1].reference {
            SrcRef::Zero | SrcRef::Reg(_) => {
                e.set_opcode(0x5b60);
                e.set_reg_src(20..28, &self.srcs[1]);
            }
            SrcRef::Imm32(imm32) => {
                e.set_opcode(0x3660);
                e.set_src_imm_i20(20..39, 56, *imm32);
                assert!(self.srcs[1].is_unmodified());
            }
            SrcRef::CBuf(cb) => {
                e.set_opcode(0x4b60);
                e.set_src_cb(20..39, cb);
            }
            src => crate::codegen::ice!("Invalid isetp src1: {src}"),
        }

        e.set_pred_dst(0..3, &Dst::None); // dst1
        e.set_pred_dst(3..6, &self.dst);
        e.set_reg_src(8..16, &self.srcs[0]);
        e.set_pred_src(39..42, 42, self.accum());

        // isetp.x seems to take the accumulator into account and we don't fully
        // understand how.  Until we do, disallow it.
        assert!(!self.ex);
        e.set_bit(43, self.ex);
        e.set_pred_set_op(45..47, self.set_op);

        e.set_field(
            48..49,
            match self.cmp_type {
                IntCmpType::U32 => 0_u32,
                IntCmpType::I32 => 1_u32,
            },
        );
        e.set_int_cmp_op(49..52, self.cmp_op);
    }
}

impl SM50Op for OpLop2 {
    fn legalize(&mut self, b: &mut LegalizeBuilder) {
        use RegFile::GPR;
        let [src0, src1] = &mut self.srcs;
        match self.op {
            LogicOp2::PassB => {
                *src0 = 0.into();
                b.copy_alu_src_if_i20_overflow(src1, GPR, SrcType::ALU);
            }
            LogicOp2::And | LogicOp2::Or | LogicOp2::Xor => {
                swap_srcs_if_not_reg(src0, src1, GPR);
                b.copy_alu_src_if_not_reg(src0, GPR, SrcType::ALU);
            }
        }
    }

    fn encode(&self, e: &mut SM50Encoder<'_>) {
        if let Some(imm32) = self.srcs[1].as_imm_not_i20() {
            e.set_opcode(0x0400);

            e.set_dst(&self.dst);
            e.set_reg_bnot_src(8..16, 55, &self.srcs[0]);
            e.set_src_imm32(20..52, imm32);
            e.set_field(
                53..55,
                match self.op {
                    LogicOp2::And => 0_u8,
                    LogicOp2::Or => 1_u8,
                    LogicOp2::Xor => 2_u8,
                    LogicOp2::PassB => {
                        crate::codegen::ice!("PASS_B is not supported for LOP32I");
                    }
                },
            );
            e.set_bit(56, self.srcs[1].modifier.is_bnot());
        } else {
            match &self.srcs[1].reference {
                SrcRef::Zero | SrcRef::Reg(_) => {
                    e.set_opcode(0x5c40);
                    e.set_reg_bnot_src(20..28, 40, &self.srcs[1]);
                }
                SrcRef::Imm32(imm32) => {
                    e.set_opcode(0x3840);
                    e.set_src_imm_i20(20..39, 56, *imm32);
                    assert!(self.srcs[1].is_unmodified());
                }
                SrcRef::CBuf(_) => {
                    e.set_opcode(0x4c40);
                    e.set_cb_bnot_src(20..39, 40, &self.srcs[1]);
                }
                src => crate::codegen::ice!("Invalid lop2 src1: {src}"),
            }

            e.set_dst(&self.dst);
            e.set_reg_bnot_src(8..16, 39, &self.srcs[0]);

            e.set_field(
                41..43,
                match self.op {
                    LogicOp2::And => 0_u8,
                    LogicOp2::Or => 1_u8,
                    LogicOp2::Xor => 2_u8,
                    LogicOp2::PassB => 3_u8,
                },
            );

            e.set_pred_dst(48..51, &Dst::None);
        }
    }
}

impl SM50Op for OpPopC {
    fn legalize(&mut self, b: &mut LegalizeBuilder) {
        use RegFile::GPR;
        b.copy_alu_src_if_i20_overflow(&mut self.src, GPR, SrcType::ALU);
    }

    fn encode(&self, e: &mut SM50Encoder<'_>) {
        match &self.src.reference {
            SrcRef::Zero | SrcRef::Reg(_) => {
                e.set_opcode(0x5c08);
                e.set_reg_bnot_src(20..28, 40, &self.src);
            }
            SrcRef::Imm32(imm32) => {
                e.set_opcode(0x3808);
                e.set_src_imm_i20(20..39, 56, *imm32);
                e.set_bit(40, self.src.modifier.is_bnot());
            }
            SrcRef::CBuf(_) => {
                e.set_opcode(0x4c08);
                e.set_cb_bnot_src(20..39, 40, &self.src);
            }
            src => crate::codegen::ice!("Invalid popc src1: {src}"),
        }

        e.set_dst(&self.dst);
    }
}

impl SM50Op for OpShf {
    fn legalize(&mut self, b: &mut LegalizeBuilder) {
        use RegFile::GPR;
        b.copy_alu_src_if_not_reg(self.high_mut(), GPR, SrcType::ALU);
        b.copy_alu_src_if_not_reg(self.low_mut(), GPR, SrcType::GPR);
        b.copy_alu_src_if_not_reg_or_imm(self.shift_mut(), GPR, SrcType::GPR);
        self.reduce_shift_imm();
    }

    fn encode(&self, e: &mut SM50Encoder<'_>) {
        match &self.shift().reference {
            SrcRef::Zero | SrcRef::Reg(_) => {
                e.set_opcode(if self.right { 0x5cf8 } else { 0x5bf8 });
                e.set_reg_src(20..28, self.shift());
            }
            SrcRef::Imm32(imm32) => {
                e.set_opcode(if self.right { 0x38f8 } else { 0x36f8 });
                e.set_src_imm_i20(20..39, 56, *imm32);
                assert!(self.shift().is_unmodified());
            }
            src => crate::codegen::ice!("Invalid shf shift: {src}"),
        }

        e.set_field(
            37..39,
            match self.data_type {
                IntType::I32 | IntType::U32 => 0_u8,
                IntType::U64 => 2_u8,
                IntType::I64 => 3_u8,
                _ => crate::codegen::ice!("Invalid shift data type"),
            },
        );

        e.set_dst(&self.dst);
        e.set_reg_src(8..16, self.low());
        e.set_reg_src(39..47, self.high());

        e.set_bit(47, false); // .CC

        // If we're shifting left, the HW will throw an illegal instrucction
        // encoding error if we set .high and will give us the high part anyway
        // if we don't.  This makes everything a bit more consistent.
        assert!(self.right || self.dst_high);
        e.set_bit(48, self.dst_high && self.right); // .high

        e.set_bit(49, false); // .X
        e.set_bit(50, self.wrap);
    }
}

impl SM50Op for OpShl {
    fn legalize(&mut self, b: &mut LegalizeBuilder) {
        use RegFile::GPR;
        b.copy_alu_src_if_not_reg(self.src_mut(), GPR, SrcType::GPR);
        self.reduce_shift_imm();
    }

    fn encode(&self, e: &mut SM50Encoder<'_>) {
        e.set_dst(&self.dst);
        e.set_reg_src(8..16, self.src());
        match &self.shift().reference {
            SrcRef::Zero | SrcRef::Reg(_) => {
                e.set_opcode(0x5c48);
                e.set_reg_src(20..28, self.shift());
            }
            SrcRef::Imm32(imm32) => {
                e.set_opcode(0x3848);
                e.set_src_imm_i20(20..39, 56, *imm32);
            }
            SrcRef::CBuf(cb) => {
                e.set_opcode(0x4c48);
                e.set_src_cb(20..39, cb);
            }
            src => crate::codegen::ice!("Invalid shl shift: {src}"),
        }

        e.set_bit(39, self.wrap);
    }
}

impl SM50Op for OpShr {
    fn legalize(&mut self, b: &mut LegalizeBuilder) {
        use RegFile::GPR;
        b.copy_alu_src_if_not_reg(self.src_mut(), GPR, SrcType::GPR);
        self.reduce_shift_imm();
    }

    fn encode(&self, e: &mut SM50Encoder<'_>) {
        e.set_dst(&self.dst);
        e.set_reg_src(8..16, self.src());
        match &self.shift().reference {
            SrcRef::Zero | SrcRef::Reg(_) => {
                e.set_opcode(0x5c28);
                e.set_reg_src(20..28, self.shift());
            }
            SrcRef::Imm32(imm32) => {
                e.set_opcode(0x3828);
                e.set_src_imm_i20(20..39, 56, *imm32);
            }
            SrcRef::CBuf(cb) => {
                e.set_opcode(0x4c28);
                e.set_src_cb(20..39, cb);
            }
            src => crate::codegen::ice!("Invalid shr shift: {src}"),
        }

        e.set_bit(39, self.wrap);
        e.set_bit(48, self.signed);
    }
}

#[cfg(test)]
#[path = "int_tests.rs"]
mod tests;
