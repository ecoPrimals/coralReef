// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright © 2025-2026 ecoPrimals
// Derived from Collabora, Ltd. (2023)

use super::*;

impl SM50Op for OpDAdd {
    fn legalize(&mut self, b: &mut LegalizeBuilder) {
        use RegFile::GPR;
        let [src0, src1] = &mut self.srcs;
        swap_srcs_if_not_reg(src0, src1, GPR);
        b.copy_alu_src_if_not_reg(src0, GPR, SrcType::F64);
        b.copy_alu_src_if_f20_overflow(src1, GPR, SrcType::F64);
    }

    fn encode(&self, e: &mut SM50Encoder<'_>) {
        match &self.srcs[1].reference {
            SrcRef::Zero | SrcRef::Reg(_) => {
                e.set_opcode(0x5c70);
                e.set_reg_fmod_src(20..28, 49, 45, &self.srcs[1]);
            }
            SrcRef::Imm32(imm32) => {
                e.set_opcode(0x3870);
                e.set_src_imm_f20(20..39, 56, *imm32);
                assert!(self.srcs[1].is_unmodified());
            }
            SrcRef::CBuf(_) => {
                e.set_opcode(0x4c70);
                e.set_cb_fmod_src(20..39, 49, 45, &self.srcs[1]);
            }
            src => crate::codegen::ice!("Invalid dadd src1: {src}"),
        }

        e.set_dst(&self.dst);
        e.set_reg_fmod_src(8..16, 46, 48, &self.srcs[0]);
        e.set_rnd_mode(39..41, self.rnd_mode);
    }
}

impl SM50Op for OpDFma {
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

    fn encode(&self, e: &mut SM50Encoder<'_>) {
        // dfma doesn't have any abs flags.
        assert!(!self.srcs[0].modifier.has_fabs());
        assert!(!self.srcs[1].modifier.has_fabs());
        assert!(!self.srcs[2].modifier.has_fabs());

        // There is one fneg bit shared by the two fmul sources
        let fneg_fmul = self.srcs[0].modifier.has_fneg() ^ self.srcs[1].modifier.has_fneg();
        let fneg_src2 = self.srcs[2].modifier.has_fneg();

        match &self.srcs[2].reference {
            SrcRef::Zero | SrcRef::Reg(_) => {
                match &self.srcs[1].reference {
                    SrcRef::Zero | SrcRef::Reg(_) => {
                        e.set_opcode(0x5b70);
                        e.set_reg_src_ref(20..28, &self.srcs[1].reference);
                    }
                    SrcRef::Imm32(imm32) => {
                        e.set_opcode(0x3670);
                        e.set_src_imm_f20(20..39, 56, *imm32);
                    }
                    SrcRef::CBuf(cb) => {
                        e.set_opcode(0x4b70);
                        e.set_src_cb(20..39, cb);
                    }
                    src => crate::codegen::ice!("Invalid dfma src1: {src}"),
                }

                e.set_reg_src_ref(39..47, &self.srcs[2].reference);
            }
            SrcRef::CBuf(cb) => {
                e.set_opcode(0x5370);
                e.set_src_cb(20..39, cb);
                e.set_reg_src_ref(39..47, &self.srcs[1].reference);
            }
            src => crate::codegen::ice!("Invalid dfma src2: {src}"),
        }

        e.set_dst(&self.dst);
        e.set_reg_src_ref(8..16, &self.srcs[0].reference);

        e.set_bit(48, fneg_fmul);
        e.set_bit(49, fneg_src2);

        e.set_rnd_mode(50..52, self.rnd_mode);
    }
}

impl SM50Op for OpDMnMx {
    fn legalize(&mut self, b: &mut LegalizeBuilder) {
        use RegFile::GPR;
        let [src0, src1, _min] = &mut self.srcs;
        swap_srcs_if_not_reg(src0, src1, GPR);
        b.copy_alu_src_if_not_reg(src0, GPR, SrcType::F64);
        b.copy_alu_src_if_f20_overflow(src1, GPR, SrcType::F64);
    }

    fn encode(&self, e: &mut SM50Encoder<'_>) {
        match &self.srcs[1].reference {
            SrcRef::Zero | SrcRef::Reg(_) => {
                e.set_opcode(0x5c50);
                e.set_reg_fmod_src(20..28, 49, 45, &self.srcs[1]);
            }
            SrcRef::Imm32(imm32) => {
                e.set_opcode(0x3850);
                e.set_src_imm_f20(20..39, 56, *imm32);
                assert!(self.srcs[1].is_unmodified());
            }
            SrcRef::CBuf(_) => {
                e.set_opcode(0x4c50);
                e.set_cb_fmod_src(20..39, 49, 45, &self.srcs[1]);
            }
            src => crate::codegen::ice!("Invalid dmnmx src1: {src}"),
        }

        e.set_reg_fmod_src(8..16, 46, 48, &self.srcs[0]);
        e.set_dst(&self.dst);
        e.set_pred_src(39..42, 42, self.min());
    }
}

impl SM50Op for OpDMul {
    fn legalize(&mut self, b: &mut LegalizeBuilder) {
        use RegFile::GPR;
        let [src0, src1] = &mut self.srcs;
        b.copy_alu_src_if_fabs(src0, GPR, SrcType::F64);
        b.copy_alu_src_if_fabs(src1, GPR, SrcType::F64);
        swap_srcs_if_not_reg(src0, src1, GPR);
        b.copy_alu_src_if_not_reg(src0, GPR, SrcType::F64);
        b.copy_alu_src_if_f20_overflow(src1, GPR, SrcType::F64);
    }

    fn encode(&self, e: &mut SM50Encoder<'_>) {
        assert!(!self.srcs[0].modifier.has_fabs());
        assert!(!self.srcs[1].modifier.has_fabs());

        // There is one fneg bit shared by both sources
        let fneg = self.srcs[0].modifier.has_fneg() ^ self.srcs[1].modifier.has_fneg();

        match &self.srcs[1].reference {
            SrcRef::Zero | SrcRef::Reg(_) => {
                e.set_opcode(0x5c80);
                e.set_reg_src_ref(20..28, &self.srcs[1].reference);
            }
            SrcRef::Imm32(imm32) => {
                e.set_opcode(0x3880);
                e.set_src_imm_f20(20..39, 56, *imm32);
            }
            SrcRef::CBuf(cb) => {
                e.set_opcode(0x4c80);
                e.set_src_cb(20..39, cb);
            }
            src => crate::codegen::ice!("Invalid dmul src1: {src}"),
        }

        e.set_dst(&self.dst);
        e.set_reg_src_ref(8..16, &self.srcs[0].reference);

        e.set_rnd_mode(39..41, self.rnd_mode);
        e.set_bit(48, fneg);
    }
}

impl SM50Op for OpDSetP {
    fn legalize(&mut self, b: &mut LegalizeBuilder) {
        use RegFile::GPR;
        let [src0, src1, _accum] = &mut self.srcs;
        if swap_srcs_if_not_reg(src0, src1, GPR) {
            self.cmp_op = self.cmp_op.flip();
        }
        b.copy_alu_src_if_not_reg(src0, GPR, SrcType::F64);
        b.copy_alu_src_if_pred(src1, GPR, SrcType::F64);
        b.copy_alu_src_if_f20_overflow(src1, GPR, SrcType::F64);
    }

    fn encode(&self, e: &mut SM50Encoder<'_>) {
        match &self.srcs[1].reference {
            SrcRef::Zero | SrcRef::Reg(_) => {
                e.set_opcode(0x5b80);
                e.set_reg_fmod_src(20..28, 44, 6, &self.srcs[1]);
            }
            SrcRef::Imm32(imm32) => {
                e.set_opcode(0x3680);
                e.set_src_imm_f20(20..39, 56, *imm32);
                assert!(self.srcs[1].is_unmodified());
            }
            SrcRef::CBuf(_) => {
                e.set_opcode(0x4b80);
                e.set_reg_fmod_src(20..39, 44, 6, &self.srcs[1]);
            }
            src => crate::codegen::ice!("Invalid dsetp src1: {src}"),
        }

        e.set_pred_dst(3..6, &self.dst);
        e.set_pred_dst(0..3, &Dst::None); // dst1
        e.set_pred_src(39..42, 42, self.accum());
        e.set_pred_set_op(45..47, self.set_op);
        e.set_float_cmp_op(48..52, self.cmp_op);
        e.set_reg_fmod_src(8..16, 7, 43, &self.srcs[0]);
    }
}

#[cfg(test)]
#[path = "float64_tests.rs"]
mod tests;
