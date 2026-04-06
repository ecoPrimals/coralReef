// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright © 2025-2026 ecoPrimals
// Derived from Collabora, Ltd. (2022)
//! SM70 ALU instruction encoders: FP64 ops.

use super::*;

impl SM70Op for OpDAdd {
    fn legalize(&mut self, b: &mut LegalizeBuilder) {
        let gpr = op_gpr(self);
        let [src0, src1] = &mut self.srcs;
        swap_srcs_if_not_reg(src0, src1, gpr);
        b.copy_alu_src_if_not_reg(src0, gpr, SrcType::F64);
    }

    fn encode(&self, e: &mut SM70Encoder<'_>) {
        e.encode_alu(
            0x029,
            Some(&self.dst),
            Some(&self.srcs[0]),
            None,
            Some(&self.srcs[1]),
        );
        e.set_rnd_mode(78..80, self.rnd_mode);
    }
}

impl SM70Op for OpDFma {
    fn legalize(&mut self, b: &mut LegalizeBuilder) {
        let gpr = op_gpr(self);
        let [src0, src1, src2] = &mut self.srcs;
        swap_srcs_if_not_reg(src0, src1, gpr);
        b.copy_alu_src_if_not_reg(src0, gpr, SrcType::F64);
        b.copy_alu_src_if_both_not_reg(src1, src2, gpr, SrcType::F64);
    }

    fn encode(&self, e: &mut SM70Encoder<'_>) {
        e.encode_alu(
            0x02b,
            Some(&self.dst),
            Some(&self.srcs[0]),
            Some(&self.srcs[1]),
            Some(&self.srcs[2]),
        );
        e.set_rnd_mode(78..80, self.rnd_mode);
    }
}

impl SM70Op for OpDMul {
    fn legalize(&mut self, b: &mut LegalizeBuilder) {
        let gpr = op_gpr(self);
        let [src0, src1] = &mut self.srcs;
        swap_srcs_if_not_reg(src0, src1, gpr);
        b.copy_alu_src_if_not_reg(src0, gpr, SrcType::F64);
    }

    fn encode(&self, e: &mut SM70Encoder<'_>) {
        e.encode_alu(
            0x028,
            Some(&self.dst),
            Some(&self.srcs[0]),
            Some(&self.srcs[1]),
            None,
        );
        e.set_rnd_mode(78..80, self.rnd_mode);
    }
}

impl SM70Op for OpDSetP {
    fn legalize(&mut self, b: &mut LegalizeBuilder) {
        let gpr = op_gpr(self);
        let [src0, src1, _accum] = &mut self.srcs;
        if !src_is_reg(src0, gpr) && src_is_reg(src1, gpr) {
            std::mem::swap(src0, src1);
            self.cmp_op = self.cmp_op.flip();
        }
        b.copy_alu_src_if_not_reg(src0, gpr, SrcType::F64);
        b.copy_alu_src_if_pred(src1, gpr, SrcType::F64);
    }

    fn encode(&self, e: &mut SM70Encoder<'_>) {
        if src_is_zero_or_gpr(&self.srcs[1]) {
            e.encode_alu(0x02a, None, Some(&self.srcs[0]), Some(&self.srcs[1]), None);
        } else {
            e.encode_alu(0x02a, None, Some(&self.srcs[0]), None, Some(&self.srcs[1]));
        }

        e.set_pred_set_op(74..76, self.set_op);
        e.set_float_cmp_op(76..80, self.cmp_op);

        e.set_pred_dst(81..84, &self.dst);
        e.set_pred_dst(84..87, &Dst::None); /* dst1 */

        e.set_pred_src(87..90, 90, self.accum());
    }
}
