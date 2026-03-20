// SPDX-License-Identifier: AGPL-3.0-only
use super::*;

impl SM32Op for OpBfe {
    fn legalize(&mut self, b: &mut LegalizeBuilder) {
        use RegFile::GPR;
        b.copy_alu_src_if_not_reg(self.base_mut(), GPR, SrcType::ALU);
        if let SrcRef::Imm32(imm) = &mut self.range_mut().reference {
            *imm &= 0xffff; // Only the lower 2 bytes matter
        }
    }

    fn encode(&self, e: &mut SM32Encoder<'_>) {
        e.encode_form_immreg(
            0xc00,
            0x200,
            Some(&self.dst),
            self.base(),
            self.range(),
            None,
            false,
        );

        e.set_bit(43, self.reverse);
        e.set_bit(51, self.signed);
    }
}

impl SM32Op for OpFlo {
    fn legalize(&mut self, b: &mut LegalizeBuilder) {
        use RegFile::GPR;
        b.copy_alu_src_if_imm(&mut self.src, GPR, SrcType::ALU);
    }

    fn encode(&self, e: &mut SM32Encoder<'_>) {
        match &self.src.reference {
            SrcRef::Zero | SrcRef::Reg(_) => {
                e.set_opcode(0xe18, 2);
                e.set_reg_src(23..31, &self.src);
            }
            SrcRef::CBuf(cb) => {
                e.set_opcode(0x618, 2);
                e.set_src_cbuf(23..42, cb);
            }
            _ => crate::codegen::ice!("Invalid flo src"),
        }

        e.set_bit(43, self.src.modifier.is_bnot());
        e.set_bit(44, self.return_shift_amount);
        e.set_bit(51, self.signed);

        e.set_dst(&self.dst);
    }
}

impl SM32Op for OpIAdd2 {
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
    }

    fn encode(&self, e: &mut SM32Encoder<'_>) {
        // Hardware requires at least one of these be unmodified.  Otherwise, it
        // encodes as iadd.po which isn't what we want.
        assert!(self.srcs[0].modifier.is_none() || self.srcs[1].modifier.is_none());

        let carry_out = match self.carry_out() {
            Dst::Reg(reg) if reg.file() == RegFile::Carry => true,
            Dst::None => false,
            dst => crate::codegen::ice!("Invalid iadd carry_out: {dst}"),
        };

        if let Some(limm) = self.srcs[1].as_imm_not_i20() {
            e.set_opcode(0x400, 1);
            e.set_dst(self.dst());

            e.set_reg_src(10..18, &self.srcs[0]);
            e.set_field(23..55, limm);

            e.set_bit(59, self.srcs[0].modifier.is_ineg());
            e.set_bit(55, carry_out); // .cc
            e.set_bit(56, false); // .X
        } else {
            e.encode_form_immreg(
                0xc08,
                0x208,
                Some(self.dst()),
                &self.srcs[0],
                &self.srcs[1],
                None,
                false,
            );

            e.set_bit(52, self.srcs[0].modifier.is_ineg());
            e.set_bit(51, self.srcs[1].modifier.is_ineg());
            e.set_bit(50, carry_out);
            e.set_bit(46, false); // .x
        }
    }
}

impl SM32Op for OpIAdd2X {
    fn legalize(&mut self, b: &mut LegalizeBuilder) {
        use RegFile::GPR;
        let [src0, src1, _carry_in] = &mut self.srcs;
        swap_srcs_if_not_reg(src0, src1, GPR);
        b.copy_alu_src_if_not_reg(src0, GPR, SrcType::I32);
    }

    fn encode(&self, e: &mut SM32Encoder<'_>) {
        match &self.carry_in().reference {
            SrcRef::Reg(reg) if reg.file() == RegFile::Carry => (),
            src => crate::codegen::ice!("Invalid iadd.x carry_in: {src}"),
        }

        let carry_out = match self.carry_out() {
            Dst::Reg(reg) if reg.file() == RegFile::Carry => true,
            Dst::None => false,
            dst => crate::codegen::ice!("Invalid iadd.x carry_out: {dst}"),
        };

        if let Some(limm) = self.srcs[1].as_imm_not_i20() {
            e.set_opcode(0x400, 1);
            e.set_dst(self.dst());

            e.set_reg_src(10..18, &self.srcs[0]);
            e.set_field(23..55, limm);

            e.set_bit(59, self.srcs[0].modifier.is_bnot());
            e.set_bit(55, carry_out); // .cc
            e.set_bit(56, true); // .X
        } else {
            e.encode_form_immreg(
                0xc08,
                0x208,
                Some(self.dst()),
                &self.srcs[0],
                &self.srcs[1],
                None,
                false,
            );

            e.set_bit(52, self.srcs[0].modifier.is_bnot());
            e.set_bit(51, self.srcs[1].modifier.is_bnot());
            e.set_bit(50, carry_out);
            e.set_bit(46, true); // .x
        }
    }
}

impl SM32Op for OpIMad {
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

    fn encode(&self, e: &mut SM32Encoder<'_>) {
        e.encode_form_immreg(
            0xa00,
            0x110,
            Some(&self.dst),
            &self.srcs[0],
            &self.srcs[1],
            Some(&self.srcs[2]),
            false,
        );
        // 57: .hi
        e.set_bit(56, self.signed);
        e.set_bit(
            55,
            self.srcs[0].modifier.is_ineg() ^ self.srcs[1].modifier.is_ineg(),
        );
        e.set_bit(54, self.srcs[2].modifier.is_ineg());
        // 53: .sat
        // 52: .x
        e.set_bit(51, self.signed);
        // 50: cc
    }
}

impl SM32Op for OpIMul {
    fn legalize(&mut self, b: &mut LegalizeBuilder) {
        use RegFile::GPR;
        let [src0, src1] = &mut self.srcs;
        if swap_srcs_if_not_reg(src0, src1, GPR) {
            self.signed.swap(0, 1);
        }
        b.copy_alu_src_if_not_reg(src0, GPR, SrcType::ALU);
    }

    fn encode(&self, e: &mut SM32Encoder<'_>) {
        assert!(self.srcs[0].modifier.is_none());
        assert!(self.srcs[1].modifier.is_none());

        if let Some(limm) = self.srcs[1].as_imm_not_i20() {
            e.set_opcode(0x280, 2);
            e.set_dst(&self.dst);

            e.set_reg_src(10..18, &self.srcs[0]);
            e.set_field(23..55, limm);

            e.set_bit(58, self.signed[1]);
            e.set_bit(57, self.signed[0]);
            e.set_bit(56, self.high);
        } else {
            e.encode_form_immreg(
                0xc1c,
                0x21c,
                Some(&self.dst),
                &self.srcs[0],
                &self.srcs[1],
                None,
                false,
            );

            e.set_bit(44, self.signed[1]);
            e.set_bit(43, self.signed[0]);
            e.set_bit(42, self.high);
        }
    }
}

impl SM32Op for OpIMnMx {
    fn legalize(&mut self, b: &mut LegalizeBuilder) {
        use RegFile::GPR;
        let [src0, src1, _min] = &mut self.srcs;
        swap_srcs_if_not_reg(src0, src1, GPR);
        b.copy_alu_src_if_not_reg(src0, GPR, SrcType::ALU);
        b.copy_alu_src_if_i20_overflow(src1, GPR, SrcType::ALU);
    }

    fn encode(&self, e: &mut SM32Encoder<'_>) {
        e.encode_form_immreg(
            0xc10,
            0x210,
            Some(&self.dst),
            &self.srcs[0],
            &self.srcs[1],
            None,
            false,
        );

        e.set_pred_src(42..46, self.min());
        // 46..48: ?|xlo|xmed|xhi
        e.set_bit(
            51,
            match self.cmp_type {
                IntCmpType::U32 => false,
                IntCmpType::I32 => true,
            },
        );
    }
}

impl SM32Op for OpISetP {
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

    fn encode(&self, e: &mut SM32Encoder<'_>) {
        e.encode_form_immreg(
            0xb30,
            0x1b0,
            None,
            &self.srcs[0],
            &self.srcs[1],
            None,
            false,
        );
        e.set_pred_dst(2..5, &Dst::None); // dst1
        e.set_pred_dst(5..8, &self.dst); // dst0
        e.set_pred_src(42..46, self.accum());

        e.set_bit(46, self.ex);
        e.set_pred_set_op(48..50, self.set_op);

        e.set_field(
            51..52,
            match self.cmp_type {
                IntCmpType::U32 => 0_u8,
                IntCmpType::I32 => 1_u8,
            },
        );
        e.set_int_cmp_op(52..55, self.cmp_op);
    }
}

impl SM32Op for OpLop2 {
    fn legalize(&mut self, b: &mut LegalizeBuilder) {
        use RegFile::GPR;
        let [src0, src1] = &mut self.srcs;
        match self.op {
            LogicOp2::And | LogicOp2::Or | LogicOp2::Xor => {
                swap_srcs_if_not_reg(src0, src1, GPR);
                b.copy_alu_src_if_not_reg(src0, GPR, SrcType::ALU);
            }
            LogicOp2::PassB => {
                *src0 = 0.into();
                b.copy_alu_src_if_i20_overflow(src1, GPR, SrcType::ALU);
            }
        }
    }

    fn encode(&self, e: &mut SM32Encoder<'_>) {
        if let Some(limm) = self.srcs[1].as_imm_not_i20() {
            e.set_opcode(0x200, 0);

            e.set_dst(&self.dst);
            e.set_reg_src(10..18, &self.srcs[0]);
            e.set_field(23..55, limm);
            e.set_field(
                56..58,
                match self.op {
                    LogicOp2::And => 0_u8,
                    LogicOp2::Or => 1_u8,
                    LogicOp2::Xor => 2_u8,
                    LogicOp2::PassB => crate::codegen::ice!("Not supported for imm32"),
                },
            );
            e.set_bit(58, self.srcs[0].modifier.is_bnot());
            e.set_bit(59, self.srcs[1].modifier.is_bnot());
        } else {
            e.encode_form_immreg(
                0xc20,
                0x220,
                Some(&self.dst),
                &self.srcs[0],
                &self.srcs[1],
                None,
                false,
            );

            e.set_bit(42, self.srcs[0].modifier.is_bnot());
            e.set_bit(43, self.srcs[1].modifier.is_bnot());

            e.set_field(
                44..46,
                match self.op {
                    LogicOp2::And => 0_u8,
                    LogicOp2::Or => 1_u8,
                    LogicOp2::Xor => 2_u8,
                    LogicOp2::PassB => 3_u8,
                },
            );
        }
    }
}

impl SM32Op for OpPopC {
    fn legalize(&mut self, b: &mut LegalizeBuilder) {
        use RegFile::GPR;
        b.copy_alu_src_if_not_reg(&mut self.src, GPR, SrcType::ALU);
    }

    fn encode(&self, e: &mut SM32Encoder<'_>) {
        // popc on Kepler takes two sources and ANDs them and counts the
        // intersecting bits.  Pass it !rZ as the second source.
        let mask = Src::from(0).bnot();
        e.encode_form_immreg(0xc04, 0x204, Some(&self.dst), &mask, &self.src, None, false);
        e.set_bit(42, mask.modifier.is_bnot());
        e.set_bit(43, self.src.modifier.is_bnot());
    }
}

impl SM32Op for OpShf {
    fn legalize(&mut self, b: &mut LegalizeBuilder) {
        use RegFile::GPR;
        b.copy_alu_src_if_not_reg(self.high_mut(), GPR, SrcType::ALU);
        b.copy_alu_src_if_not_reg(self.low_mut(), GPR, SrcType::GPR);
        b.copy_alu_src_if_not_reg_or_imm(self.shift_mut(), GPR, SrcType::GPR);
        self.reduce_shift_imm();
    }

    fn encode(&self, e: &mut SM32Encoder<'_>) {
        if self.right {
            e.encode_form_immreg(
                0xc7c,
                0x27c,
                Some(&self.dst),
                self.low(),
                self.shift(),
                Some(self.high()),
                false,
            );
        } else {
            e.encode_form_immreg(
                0xb7c,
                0x1fc,
                Some(&self.dst),
                self.low(),
                self.shift(),
                Some(self.high()),
                false,
            );
        }

        e.set_bit(53, self.wrap);
        e.set_bit(52, false); // .x

        // Fun behavior: As for Maxwell, Kepler does not support shf.l.hi
        // but it still always takes the high part of the result.
        // If we encode .hi it traps with illegal instruction encoding.
        // We can encode a low shf.l by using only the high part and
        // hard-wiring the low part to rZ.
        assert!(self.right || self.dst_high);
        e.set_bit(51, self.right && self.dst_high); // .hi
        e.set_bit(50, false); // .cc

        e.set_field(
            40..42,
            match self.data_type {
                IntType::I32 | IntType::U32 => 0_u8,
                IntType::U64 => 2_u8,
                IntType::I64 => 3_u8,
                _ => crate::codegen::ice!("Invalid shift data type"),
            },
        );
    }
}

impl SM32Op for OpShl {
    fn legalize(&mut self, b: &mut LegalizeBuilder) {
        use RegFile::GPR;
        b.copy_alu_src_if_not_reg(self.src_mut(), GPR, SrcType::GPR);
        self.reduce_shift_imm();
    }

    fn encode(&self, e: &mut SM32Encoder<'_>) {
        e.encode_form_immreg(
            0xc24,
            0x224,
            Some(&self.dst),
            self.src(),
            self.shift(),
            None,
            false,
        );
        e.set_bit(42, self.wrap);
        // 46: .x(?)
        // 50: .cc(?)
    }
}

impl SM32Op for OpShr {
    fn legalize(&mut self, b: &mut LegalizeBuilder) {
        use RegFile::GPR;
        b.copy_alu_src_if_not_reg(self.src_mut(), GPR, SrcType::GPR);
        self.reduce_shift_imm();
    }

    fn encode(&self, e: &mut SM32Encoder<'_>) {
        e.encode_form_immreg(
            0xc14,
            0x214,
            Some(&self.dst),
            self.src(),
            self.shift(),
            None,
            false,
        );
        e.set_bit(42, self.wrap);
        // 43: .brev
        // 47: .x(?)
        // 50: .cc(?)
        e.set_bit(51, self.signed);
    }
}

#[cfg(test)]
#[path = "int_tests.rs"]
mod tests;
