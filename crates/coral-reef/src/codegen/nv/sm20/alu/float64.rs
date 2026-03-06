// SPDX-License-Identifier: AGPL-3.0-only
use super::*;

impl SM20Op for OpDAdd {
    fn legalize(&mut self, b: &mut LegalizeBuilder) {
        use RegFile::GPR;
        let [src0, src1] = &mut self.srcs;
        swap_srcs_if_not_reg(src0, src1, GPR);
        b.copy_alu_src_if_not_reg(src0, GPR, SrcType::F64);
        b.copy_alu_src_if_f20_overflow(src1, GPR, SrcType::F64);
    }

    fn encode(&self, e: &mut SM20Encoder<'_>) {
        e.encode_form_a(
            SM20Unit::Double,
            0x12,
            &self.dst,
            &self.srcs[0],
            &self.srcs[1],
            None,
        );
        e.set_bit(6, self.srcs[1].modifier.has_fabs());
        e.set_bit(7, self.srcs[0].modifier.has_fabs());
        e.set_bit(8, self.srcs[1].modifier.has_fneg());
        e.set_bit(9, self.srcs[0].modifier.has_fneg());
        e.set_rnd_mode(55..57, self.rnd_mode);
    }
}

impl SM20Op for OpDFma {
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

    fn encode(&self, e: &mut SM20Encoder<'_>) {
        assert!(!self.srcs[0].modifier.has_fabs());
        assert!(!self.srcs[1].modifier.has_fabs());
        assert!(!self.srcs[2].modifier.has_fabs());
        e.encode_form_a(
            SM20Unit::Double,
            0x8,
            &self.dst,
            &self.srcs[0],
            &self.srcs[1],
            Some(&self.srcs[2]),
        );
        e.set_bit(8, self.srcs[2].modifier.has_fneg());
        let neg0 = self.srcs[0].modifier.has_fneg();
        let neg1 = self.srcs[1].modifier.has_fneg();
        e.set_bit(9, neg0 ^ neg1);
        e.set_rnd_mode(55..57, self.rnd_mode);
    }
}

impl SM20Op for OpDMnMx {
    fn legalize(&mut self, b: &mut LegalizeBuilder) {
        use RegFile::GPR;
        let [src0, src1] = &mut self.srcs;
        swap_srcs_if_not_reg(src0, src1, GPR);
        b.copy_alu_src_if_not_reg(src0, GPR, SrcType::F64);
        b.copy_alu_src_if_f20_overflow(src1, GPR, SrcType::F64);
    }

    fn encode(&self, e: &mut SM20Encoder<'_>) {
        e.encode_form_a(
            SM20Unit::Double,
            0x2,
            &self.dst,
            &self.srcs[0],
            &self.srcs[1],
            None,
        );
        e.set_bit(6, self.srcs[1].modifier.has_fabs());
        e.set_bit(7, self.srcs[0].modifier.has_fabs());
        e.set_bit(8, self.srcs[1].modifier.has_fneg());
        e.set_bit(9, self.srcs[0].modifier.has_fneg());
        e.set_pred_src(49..53, &self.min);
    }
}

impl SM20Op for OpDMul {
    fn legalize(&mut self, b: &mut LegalizeBuilder) {
        use RegFile::GPR;
        let [src0, src1] = &mut self.srcs;
        swap_srcs_if_not_reg(src0, src1, GPR);
        b.copy_alu_src_if_not_reg(src0, GPR, SrcType::F64);
        b.copy_alu_src_if_f20_overflow(src1, GPR, SrcType::F64);
    }

    fn encode(&self, e: &mut SM20Encoder<'_>) {
        assert!(!self.srcs[0].modifier.has_fabs());
        assert!(!self.srcs[1].modifier.has_fabs());
        e.encode_form_a(
            SM20Unit::Double,
            0x14,
            &self.dst,
            &self.srcs[0],
            &self.srcs[1],
            None,
        );
        let neg0 = self.srcs[0].modifier.has_fneg();
        let neg1 = self.srcs[1].modifier.has_fneg();
        e.set_bit(9, neg0 ^ neg1);
        e.set_rnd_mode(55..57, self.rnd_mode);
    }
}

impl SM20Op for OpDSetP {
    fn legalize(&mut self, b: &mut LegalizeBuilder) {
        use RegFile::GPR;
        let [src0, src1] = &mut self.srcs;
        swap_srcs_if_not_reg(src0, src1, GPR);
        b.copy_alu_src_if_not_reg(src0, GPR, SrcType::F64);
        b.copy_alu_src_if_f20_overflow(src1, GPR, SrcType::F64);
    }

    fn encode(&self, e: &mut SM20Encoder<'_>) {
        e.encode_form_a_no_dst(SM20Unit::Double, 0x6, &self.srcs[0], &self.srcs[1]);
        e.set_bit(6, self.srcs[1].modifier.has_fabs());
        e.set_bit(7, self.srcs[0].modifier.has_fabs());
        e.set_bit(8, self.srcs[1].modifier.has_fneg());
        e.set_bit(9, self.srcs[0].modifier.has_fneg());
        e.set_pred_dst(14..17, &Dst::None);
        e.set_pred_dst(17..20, &self.dst);
        e.set_pred_src(49..53, &self.accum);
        e.set_pred_set_op(53..55, self.set_op);
        e.set_float_cmp_op(55..59, self.cmp_op);
    }
}
