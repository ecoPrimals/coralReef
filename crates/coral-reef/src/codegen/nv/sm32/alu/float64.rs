// SPDX-License-Identifier: AGPL-3.0-only
use super::*;

impl SM32Op for OpDAdd {
    fn legalize(&mut self, b: &mut LegalizeBuilder) {
        use RegFile::GPR;
        let [src0, src1] = &mut self.srcs;
        swap_srcs_if_not_reg(src0, src1, GPR);
        b.copy_alu_src_if_not_reg(src0, GPR, SrcType::F64);
        b.copy_alu_src_if_f20_overflow(src1, GPR, SrcType::F64);
    }

    fn encode(&self, e: &mut SM32Encoder<'_>) {
        e.encode_form_immreg(
            0xc38,
            0x238,
            Some(&self.dst),
            &self.srcs[0],
            &self.srcs[1],
            None,
            true,
        );

        e.set_rnd_mode(42..44, self.rnd_mode);
        // 47: .ftz
        e.set_bit(48, self.srcs[1].modifier.has_fneg());
        e.set_bit(49, self.srcs[0].modifier.has_fabs());
        // 50: .cc?
        e.set_bit(51, self.srcs[0].modifier.has_fneg());
        e.set_bit(52, self.srcs[1].modifier.has_fabs());
        // 53: .sat
    }
}

impl SM32Op for OpDFma {
    fn legalize(&mut self, b: &mut LegalizeBuilder) {
        use RegFile::GPR;
        let [src0, src1, src2] = &mut self.srcs;
        b.copy_alu_src_if_fabs(src0, GPR, SrcType::F64);
        b.copy_alu_src_if_fabs(src1, GPR, SrcType::F64);
        b.copy_alu_src_if_fabs(src2, GPR, SrcType::F64);
        swap_srcs_if_not_reg(src0, src1, GPR);
        b.copy_alu_src_if_not_reg(src0, GPR, SrcType::F64);
        b.copy_alu_src_if_f20_overflow(src1, GPR, SrcType::F64);
        if src_is_reg(src1, GPR) {
            b.copy_alu_src_if_imm(src2, GPR, SrcType::F64);
        } else {
            b.copy_alu_src_if_not_reg(src2, GPR, SrcType::F64);
        }
    }

    fn encode(&self, e: &mut SM32Encoder<'_>) {
        // ffma doesn't have any abs flags.
        assert!(!self.srcs[0].modifier.has_fabs());
        assert!(!self.srcs[1].modifier.has_fabs());
        assert!(!self.srcs[2].modifier.has_fabs());

        // There is one fneg bit shared by the two fmul sources
        let fneg_fmul = self.srcs[0].modifier.has_fneg() ^ self.srcs[1].modifier.has_fneg();

        e.encode_form_immreg(
            0xb38,
            0x1b8,
            Some(&self.dst),
            &self.srcs[0],
            &self.srcs[1],
            Some(&self.srcs[2]),
            true,
        );

        e.set_bit(51, fneg_fmul);
        e.set_bit(52, self.srcs[2].modifier.has_fneg());
        e.set_rnd_mode(53..55, self.rnd_mode);
    }
}

impl SM32Op for OpDMnMx {
    fn legalize(&mut self, b: &mut LegalizeBuilder) {
        use RegFile::GPR;
        let [src0, src1] = &mut self.srcs;
        swap_srcs_if_not_reg(src0, src1, GPR);
        b.copy_alu_src_if_not_reg(src0, GPR, SrcType::F64);
        b.copy_alu_src_if_f20_overflow(src1, GPR, SrcType::F64);
    }

    fn encode(&self, e: &mut SM32Encoder<'_>) {
        e.encode_form_immreg(
            0xc28,
            0x228,
            Some(&self.dst),
            &self.srcs[0],
            &self.srcs[1],
            None,
            true,
        );

        e.set_pred_src(42..46, &self.min);
        e.set_bit(48, self.srcs[1].modifier.has_fneg());
        e.set_bit(49, self.srcs[0].modifier.has_fabs());
        // 50: .cc?
        e.set_bit(51, self.srcs[0].modifier.has_fneg());
        e.set_bit(52, self.srcs[1].modifier.has_fabs());
    }
}

impl SM32Op for OpDMul {
    fn legalize(&mut self, b: &mut LegalizeBuilder) {
        use RegFile::GPR;
        let [src0, src1] = &mut self.srcs;
        b.copy_alu_src_if_fabs(src0, GPR, SrcType::F64);
        b.copy_alu_src_if_fabs(src1, GPR, SrcType::F64);
        swap_srcs_if_not_reg(src0, src1, GPR);
        b.copy_alu_src_if_not_reg(src0, GPR, SrcType::F64);
        b.copy_alu_src_if_f20_overflow(src1, GPR, SrcType::F64);
    }

    fn encode(&self, e: &mut SM32Encoder<'_>) {
        // fmul doesn't have any abs flags.
        assert!(!self.srcs[0].modifier.has_fabs());
        assert!(!self.srcs[1].modifier.has_fabs());

        // There is one fneg bit shared by both sources
        let fneg = self.srcs[0].modifier.has_fneg() ^ self.srcs[1].modifier.has_fneg();

        e.encode_form_immreg(
            0xc40,
            0x240,
            Some(&self.dst),
            &self.srcs[0],
            &self.srcs[1],
            None,
            true,
        );

        e.set_rnd_mode(42..44, self.rnd_mode);
        e.set_bit(51, fneg);
    }
}

impl SM32Op for OpDSetP {
    fn legalize(&mut self, b: &mut LegalizeBuilder) {
        use RegFile::GPR;
        let [src0, src1] = &mut self.srcs;
        if swap_srcs_if_not_reg(src0, src1, GPR) {
            self.cmp_op = self.cmp_op.flip();
        }
        b.copy_alu_src_if_not_reg(src0, GPR, SrcType::F64);
        b.copy_alu_src_if_f20_overflow(src1, GPR, SrcType::F64);
    }

    fn encode(&self, e: &mut SM32Encoder<'_>) {
        e.encode_form_immreg(0xb40, 0x1c0, None, &self.srcs[0], &self.srcs[1], None, true);
        e.set_pred_dst(2..5, &Dst::None); // dst1
        e.set_pred_dst(5..8, &self.dst); // dst0
        e.set_pred_src(42..46, &self.accum);

        e.set_bit(8, self.srcs[1].modifier.has_fneg());
        e.set_bit(9, self.srcs[0].modifier.has_fabs());
        e.set_bit(46, self.srcs[0].modifier.has_fneg());
        e.set_bit(47, self.srcs[1].modifier.has_fabs());

        e.set_pred_set_op(48..50, self.set_op);
        // 50: ftz
        e.set_float_cmp_op(51..55, self.cmp_op);
    }
}
