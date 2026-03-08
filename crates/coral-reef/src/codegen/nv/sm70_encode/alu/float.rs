// SPDX-License-Identifier: AGPL-3.0-only
// Copyright © 2025-2026 ecoPrimals
// Derived from Collabora, Ltd. (2022)
//! SM70 ALU instruction encoders: FP32 ops.

use super::*;

impl SM70Op for OpFAdd {
    fn legalize(&mut self, b: &mut LegalizeBuilder) {
        let gpr = op_gpr(self);
        let [src0, src1] = &mut self.srcs;
        swap_srcs_if_not_reg(src0, src1, gpr);
        b.copy_alu_src_if_not_reg(src0, gpr, SrcType::F32);
    }

    fn encode(&self, e: &mut SM70Encoder<'_>) {
        if src_is_zero_or_gpr(&self.srcs[1]) {
            e.encode_alu(
                0x021,
                Some(&self.dst),
                Some(&self.srcs[0]),
                Some(&self.srcs[1]),
                None,
            );
        } else {
            e.encode_alu(
                0x021,
                Some(&self.dst),
                Some(&self.srcs[0]),
                Some(&Src::ZERO),
                Some(&self.srcs[1]),
            );
        }
        e.set_bit(77, self.saturate);
        e.set_rnd_mode(78..80, self.rnd_mode);
        e.set_bit(80, self.ftz);
    }
}

impl SM70Op for OpFFma {
    fn legalize(&mut self, b: &mut LegalizeBuilder) {
        let gpr = op_gpr(self);
        let [src0, src1, src2] = &mut self.srcs;
        swap_srcs_if_not_reg(src0, src1, gpr);
        b.copy_alu_src_if_not_reg(src0, gpr, SrcType::F32);
        b.copy_alu_src_if_both_not_reg(src1, src2, gpr, SrcType::F32);
    }

    fn encode(&self, e: &mut SM70Encoder<'_>) {
        e.encode_alu(
            0x023,
            Some(&self.dst),
            Some(&self.srcs[0]),
            Some(&self.srcs[1]),
            Some(&self.srcs[2]),
        );
        e.set_bit(76, self.dnz);
        e.set_bit(77, self.saturate);
        e.set_rnd_mode(78..80, self.rnd_mode);
        e.set_bit(80, self.ftz);
    }
}

impl SM70Op for OpFMnMx {
    fn legalize(&mut self, b: &mut LegalizeBuilder) {
        let gpr = op_gpr(self);
        let [src0, src1] = &mut self.srcs;
        swap_srcs_if_not_reg(src0, src1, gpr);
        b.copy_alu_src_if_not_reg(src0, gpr, SrcType::F32);
    }

    fn encode(&self, e: &mut SM70Encoder<'_>) {
        e.encode_alu(
            0x009,
            Some(&self.dst),
            Some(&self.srcs[0]),
            Some(&self.srcs[1]),
            Some(&Src::ZERO),
        );
        e.set_pred_src(87..90, 90, &self.min);
        e.set_bit(80, self.ftz);
    }
}

impl SM70Op for OpFMul {
    fn legalize(&mut self, b: &mut LegalizeBuilder) {
        let gpr = op_gpr(self);
        let [src0, src1] = &mut self.srcs;
        swap_srcs_if_not_reg(src0, src1, gpr);
        b.copy_alu_src_if_not_reg(src0, gpr, SrcType::F32);
    }

    fn encode(&self, e: &mut SM70Encoder<'_>) {
        e.encode_alu(
            0x020,
            Some(&self.dst),
            Some(&self.srcs[0]),
            Some(&self.srcs[1]),
            Some(&Src::ZERO),
        );
        e.set_bit(76, self.dnz);
        e.set_bit(77, self.saturate);
        e.set_rnd_mode(78..80, self.rnd_mode);
        e.set_bit(80, self.ftz);
        e.set_field(84..87, 0x4_u8); // DEBT(isa): PDIV
    }
}

impl SM70Op for OpFSet {
    fn legalize(&mut self, b: &mut LegalizeBuilder) {
        let gpr = op_gpr(self);
        let [src0, src1] = &mut self.srcs;
        if !src_is_reg(src0, gpr) && src_is_reg(src1, gpr) {
            std::mem::swap(src0, src1);
            self.cmp_op = self.cmp_op.flip();
        }
        b.copy_alu_src_if_not_reg(src0, gpr, SrcType::F32);
    }

    fn encode(&self, e: &mut SM70Encoder<'_>) {
        e.encode_alu(
            0x00a,
            Some(&self.dst),
            Some(&self.srcs[0]),
            Some(&self.srcs[1]),
            None,
        );
        e.set_float_cmp_op(76..80, self.cmp_op);
        e.set_bit(80, self.ftz);
        e.set_field(87..90, 0x7_u8); // DEBT(isa): src predicate
    }
}

impl SM70Op for OpFSetP {
    fn legalize(&mut self, b: &mut LegalizeBuilder) {
        let gpr = op_gpr(self);
        let [src0, src1] = &mut self.srcs;
        if !src_is_reg(src0, gpr) && src_is_reg(src1, gpr) {
            std::mem::swap(src0, src1);
            self.cmp_op = self.cmp_op.flip();
        }
        b.copy_alu_src_if_not_reg(src0, gpr, SrcType::F32);
    }

    fn encode(&self, e: &mut SM70Encoder<'_>) {
        e.encode_alu(0x00b, None, Some(&self.srcs[0]), Some(&self.srcs[1]), None);

        e.set_pred_set_op(74..76, self.set_op);
        e.set_float_cmp_op(76..80, self.cmp_op);
        e.set_bit(80, self.ftz);

        e.set_pred_dst(81..84, &self.dst);
        e.set_pred_dst(84..87, &Dst::None); // dst1

        e.set_pred_src(87..90, 90, &self.accum);
    }
}

impl SM70Op for OpFSwzAdd {
    fn legalize(&mut self, b: &mut LegalizeBuilder) {
        let gpr = op_gpr(self);
        let [src0, src1] = &mut self.srcs;
        b.copy_alu_src_if_not_reg(src0, gpr, SrcType::F32);
        b.copy_alu_src_if_not_reg(src1, gpr, SrcType::F32);
    }

    fn encode(&self, e: &mut SM70Encoder<'_>) {
        e.set_opcode(0x822);
        e.set_dst(&self.dst);

        e.set_reg_src(24..32, &self.srcs[0]);
        e.set_reg_src(64..72, &self.srcs[1]);

        let mut subop = 0x0_u8;

        for (i, swz_op) in self.ops.iter().enumerate() {
            let swz_op = match swz_op {
                FSwzAddOp::Add => 0,
                FSwzAddOp::SubRight => 2,
                FSwzAddOp::SubLeft => 1,
                FSwzAddOp::MoveLeft => 3,
            };

            subop |= swz_op << ((self.ops.len() - i - 1) * 2);
        }

        e.set_field(32..40, subop);

        e.set_tex_ndv(77, self.deriv_mode);
        e.set_rnd_mode(78..80, self.rnd_mode);
        e.set_bit(80, self.ftz);
    }
}
