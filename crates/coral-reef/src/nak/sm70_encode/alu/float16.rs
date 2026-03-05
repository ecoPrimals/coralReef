// Copyright © 2022 Collabora, Ltd.
// SPDX-License-Identifier: MIT
//! SM70 ALU instruction encoders: FP16 ops.

use super::*;

impl SM70Op for OpHAdd2 {
    fn legalize(&mut self, b: &mut LegalizeBuilder) {
        let gpr = op_gpr(self);
        let [src0, src1] = &mut self.srcs;
        swap_srcs_if_not_reg(src0, src1, gpr);
        b.copy_alu_src_if_not_reg(src0, gpr, SrcType::F16v2);
    }

    fn encode(&self, e: &mut SM70Encoder<'_>) {
        if src_is_zero_or_gpr(&self.srcs[1]) {
            e.encode_fp16_alu(
                0x030,
                Some(&self.dst),
                Some(&self.srcs[0]),
                Some(&self.srcs[1]),
                None,
            );
        } else {
            e.encode_fp16_alu(
                0x030,
                Some(&self.dst),
                Some(&self.srcs[0]),
                None,
                Some(&self.srcs[1]),
            );
        }

        e.set_bit(77, self.saturate);
        e.set_bit(78, self.f32);
        e.set_bit(80, self.ftz);
        e.set_bit(85, false); // .BF16_V2 (SM90+)
    }
}

impl SM70Op for OpHFma2 {
    fn legalize(&mut self, b: &mut LegalizeBuilder) {
        let gpr = op_gpr(self);
        let [src0, src1, src2] = &mut self.srcs;
        swap_srcs_if_not_reg(src0, src1, gpr);
        b.copy_alu_src_if_not_reg(src0, gpr, SrcType::F16v2);
        b.copy_alu_src_if_both_not_reg(src1, src2, gpr, SrcType::F16v2);
    }

    fn encode(&self, e: &mut SM70Encoder<'_>) {
        e.encode_fp16_alu(
            0x031,
            Some(&self.dst),
            Some(&self.srcs[0]),
            Some(&self.srcs[1]),
            Some(&self.srcs[2]),
        );

        e.set_bit(76, self.dnz);
        e.set_bit(77, self.saturate);
        e.set_bit(78, self.f32);
        e.set_bit(79, false); // .RELU (SM86+)
        e.set_bit(80, self.ftz);
        e.set_bit(85, false); // .BF16_V2 (SM86+)
    }
}

impl SM70Op for OpHMul2 {
    fn legalize(&mut self, b: &mut LegalizeBuilder) {
        let gpr = op_gpr(self);
        let [src0, src1] = &mut self.srcs;
        swap_srcs_if_not_reg(src0, src1, gpr);
        b.copy_alu_src_if_not_reg(src0, gpr, SrcType::F16v2);
    }

    fn encode(&self, e: &mut SM70Encoder<'_>) {
        e.encode_fp16_alu(
            0x032,
            Some(&self.dst),
            Some(&self.srcs[0]),
            Some(&self.srcs[1]),
            None,
        );

        e.set_bit(76, self.dnz);
        e.set_bit(77, self.saturate);
        e.set_bit(78, false); // .F32 (SM70-SM75)
        e.set_bit(79, false); // .RELU (SM86+)
        e.set_bit(80, self.ftz);
        e.set_bit(85, false); // .BF16_V2 (SM90+)
    }
}

impl SM70Op for OpHSet2 {
    fn legalize(&mut self, b: &mut LegalizeBuilder) {
        let gpr = op_gpr(self);
        let [src0, src1] = &mut self.srcs;
        if !src_is_reg(src0, gpr) && src_is_reg(src1, gpr) {
            std::mem::swap(src0, src1);
            self.cmp_op = self.cmp_op.flip();
        }
        b.copy_alu_src_if_not_reg(src0, gpr, SrcType::F16v2);
    }

    fn encode(&self, e: &mut SM70Encoder<'_>) {
        if src_is_zero_or_gpr(&self.srcs[1]) {
            e.encode_fp16_alu(
                0x033,
                Some(&self.dst),
                Some(&self.srcs[0]),
                Some(&self.srcs[1]),
                None,
            );
        } else {
            e.encode_fp16_alu(
                0x033,
                Some(&self.dst),
                Some(&self.srcs[0]),
                None,
                Some(&self.srcs[1]),
            );
        }

        e.set_bit(65, false); // .BF16_V2 (SM90+)
        e.set_pred_set_op(69..71, self.set_op);

        // This differentiate between integer and fp16 output
        e.set_bit(71, true); // .BF
        e.set_float_cmp_op(76..80, self.cmp_op);
        e.set_bit(80, self.ftz);

        e.set_pred_src(87..90, 90, &self.accum);
    }
}

impl SM70Op for OpHSetP2 {
    fn legalize(&mut self, b: &mut LegalizeBuilder) {
        let gpr = op_gpr(self);
        let [src0, src1] = &mut self.srcs;
        if !src_is_reg(src0, gpr) && src_is_reg(src1, gpr) {
            std::mem::swap(src0, src1);
            self.cmp_op = self.cmp_op.flip();
        }
        b.copy_alu_src_if_not_reg(src0, gpr, SrcType::F16v2);
    }

    fn encode(&self, e: &mut SM70Encoder<'_>) {
        if src_is_zero_or_gpr(&self.srcs[1]) {
            e.encode_fp16_alu(0x034, None, Some(&self.srcs[0]), Some(&self.srcs[1]), None);
        } else {
            e.encode_fp16_alu(0x034, None, Some(&self.srcs[0]), None, Some(&self.srcs[1]));
        }

        e.set_bit(65, false); // .BF16_V2 (SM90+)
        e.set_pred_set_op(69..71, self.set_op);
        e.set_bit(71, self.horizontal); // .H_AND
        e.set_float_cmp_op(76..80, self.cmp_op);
        e.set_bit(80, self.ftz);

        e.set_pred_dst(81..84, &self.dsts[0]);
        e.set_pred_dst(84..87, &self.dsts[1]);

        e.set_pred_src(87..90, 90, &self.accum);
    }
}

impl SM70Op for OpHMnMx2 {
    fn legalize(&mut self, b: &mut LegalizeBuilder) {
        let gpr = op_gpr(self);
        let [src0, src1] = &mut self.srcs;
        swap_srcs_if_not_reg(src0, src1, gpr);
        b.copy_alu_src_if_not_reg(src0, gpr, SrcType::F16v2);
    }

    fn encode(&self, e: &mut SM70Encoder<'_>) {
        assert!(e.sm >= 80);

        e.encode_fp16_alu(
            0x040,
            Some(&self.dst),
            Some(&self.srcs[0]),
            Some(&self.srcs[1]),
            None,
        );

        // This differentiate between integer and fp16 output
        e.set_bit(78, false); // .F32 (SM86)
        e.set_bit(80, self.ftz);
        e.set_bit(81, false); // .NAN
        e.set_bit(82, false); // .XORSIGN
        e.set_bit(85, false); // .BF16_V2

        e.set_pred_src(87..90, 90, &self.min);
    }
}
