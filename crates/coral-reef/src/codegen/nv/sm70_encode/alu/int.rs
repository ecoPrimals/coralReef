// SPDX-License-Identifier: AGPL-3.0-only
// Copyright © 2025-2026 ecoPrimals
// Derived from Collabora, Ltd. (2022)
//! SM70 ALU instruction encoders: integer ops.

use super::*;

pub(super) fn src_as_lop_imm(src: &Src) -> Option<bool> {
    let x = match src.reference {
        SrcRef::Zero => false,
        SrcRef::True => true,
        SrcRef::False => false,
        SrcRef::Imm32(i) => {
            if i == 0 {
                false
            } else if i == !0 {
                true
            } else {
                return None;
            }
        }
        _ => return None,
    };
    Some(x ^ src.modifier.is_bnot())
}

pub(super) fn fold_lop_src(src: &Src, x: &mut u8) {
    if let Some(i) = src_as_lop_imm(src) {
        *x = if i { !0 } else { 0 };
    }
    if src.modifier.is_bnot() {
        *x = !*x;
    }
}

impl SM70Op for OpBMsk {
    fn legalize(&mut self, b: &mut LegalizeBuilder) {
        let gpr = op_gpr(self);
        b.copy_alu_src_if_not_reg(&mut self.pos, gpr, SrcType::ALU);
    }

    fn encode(&self, e: &mut SM70Encoder<'_>) {
        if self.is_uniform() {
            e.encode_ualu(
                0x09b,
                Some(&self.dst),
                Some(&self.pos),
                Some(&self.width),
                None,
            );
        } else {
            e.encode_alu(
                0x01b,
                Some(&self.dst),
                Some(&self.pos),
                Some(&self.width),
                None,
            );
        }

        e.set_bit(75, self.wrap);
    }
}

impl SM70Op for OpBRev {
    fn legalize(&mut self, _b: &mut LegalizeBuilder) {
        // Nothing to do
    }

    fn encode(&self, e: &mut SM70Encoder<'_>) {
        if self.is_uniform() {
            e.encode_ualu(0x0be, Some(&self.dst), None, Some(&self.src), None);
        } else {
            e.encode_alu(0x101, Some(&self.dst), None, Some(&self.src), None);
        }
    }
}

impl SM70Op for OpFlo {
    fn legalize(&mut self, _b: &mut LegalizeBuilder) {
        // Nothing to do
    }

    fn encode(&self, e: &mut SM70Encoder<'_>) {
        if self.is_uniform() {
            e.encode_ualu(0x0bd, Some(&self.dst), None, Some(&self.src), None);
        } else {
            e.encode_alu(0x100, Some(&self.dst), None, Some(&self.src), None);
        }
        e.set_pred_dst(81..84, &Dst::None);
        e.set_field(74..75, self.return_shift_amount as u8);
        e.set_field(73..74, self.signed as u8);
        let not_mod = matches!(self.src.modifier, SrcMod::BNot);
        e.set_field(63..64, not_mod);
    }
}

impl SM70Op for OpIAbs {
    fn legalize(&mut self, _b: &mut LegalizeBuilder) {
        // Nothing to do
    }

    fn encode(&self, e: &mut SM70Encoder<'_>) {
        e.encode_alu(0x013, Some(&self.dst), None, Some(&self.src), None);
    }
}

impl SM70Op for OpIAdd3 {
    fn legalize(&mut self, b: &mut LegalizeBuilder) {
        let gpr = op_gpr(self);
        let [src0, src1, src2] = &mut self.srcs;
        swap_srcs_if_not_reg(src0, src1, gpr);
        swap_srcs_if_not_reg(src2, src1, gpr);
        if !src0.is_unmodified() && !src1.is_unmodified() {
            assert!(self.overflow[0].is_none());
            assert!(self.overflow[1].is_none());
            b.copy_alu_src_and_lower_ineg(src0, gpr, SrcType::I32);
        }
        b.copy_alu_src_if_not_reg(src0, gpr, SrcType::I32);
        b.copy_alu_src_if_both_not_reg(src1, src2, gpr, SrcType::I32);
        b.copy_alu_src_if_ineg_imm(src1, gpr, SrcType::I32);
        b.copy_alu_src_if_ineg_imm(src2, gpr, SrcType::I32);
    }

    fn encode(&self, e: &mut SM70Encoder<'_>) {
        assert!(self.srcs[0].is_unmodified() || self.srcs[1].is_unmodified());

        if self.is_uniform() {
            e.encode_ualu(
                0x090,
                Some(&self.dst),
                Some(&self.srcs[0]),
                Some(&self.srcs[1]),
                Some(&self.srcs[2]),
            );
        } else {
            e.encode_alu(
                0x010,
                Some(&self.dst),
                Some(&self.srcs[0]),
                Some(&self.srcs[1]),
                Some(&self.srcs[2]),
            );
        }

        e.set_pred_src(87..90, 90, &false.into());
        e.set_pred_src(77..80, 80, &false.into());

        e.set_pred_dst(81..84, &self.overflow[0]);
        e.set_pred_dst(84..87, &self.overflow[1]);
    }
}

impl SM70Op for OpIAdd3X {
    fn legalize(&mut self, b: &mut LegalizeBuilder) {
        let gpr = op_gpr(self);
        let [src0, src1, src2] = &mut self.srcs;
        swap_srcs_if_not_reg(src0, src1, gpr);
        swap_srcs_if_not_reg(src2, src1, gpr);
        if !src0.is_unmodified() && !src1.is_unmodified() {
            let val = b.alloc_ssa(gpr);
            let old_src0 = std::mem::replace(src0, val.into());
            b.push_op(Self {
                srcs: [Src::ZERO, old_src0, Src::ZERO],
                overflow: [Dst::None, Dst::None],
                dst: val.into(),
                carry: [false.into(), false.into()],
            });
        }
        b.copy_alu_src_if_not_reg(src0, gpr, SrcType::B32);
        b.copy_alu_src_if_both_not_reg(src1, src2, gpr, SrcType::B32);
        if !self.is_uniform() {
            b.copy_src_if_upred(&mut self.carry[0]);
            b.copy_src_if_upred(&mut self.carry[1]);
        }
    }

    fn encode(&self, e: &mut SM70Encoder<'_>) {
        // Hardware requires at least one of these be unmodified
        assert!(self.srcs[0].is_unmodified() || self.srcs[1].is_unmodified());

        if self.is_uniform() {
            e.encode_ualu(
                0x090,
                Some(&self.dst),
                Some(&self.srcs[0]),
                Some(&self.srcs[1]),
                Some(&self.srcs[2]),
            );

            e.set_upred_src(87..90, 90, &self.carry[0]);
            e.set_upred_src(77..80, 80, &self.carry[1]);
        } else {
            e.encode_alu(
                0x010,
                Some(&self.dst),
                Some(&self.srcs[0]),
                Some(&self.srcs[1]),
                Some(&self.srcs[2]),
            );

            e.set_pred_src(87..90, 90, &self.carry[0]);
            e.set_pred_src(77..80, 80, &self.carry[1]);
        }

        e.set_bit(74, true); // .X

        e.set_pred_dst(81..84, &self.overflow[0]);
        e.set_pred_dst(84..87, &self.overflow[1]);
    }
}

impl SM70Op for OpIDp4 {
    fn legalize(&mut self, b: &mut LegalizeBuilder) {
        let gpr = op_gpr(self);
        let [src_type0, src_type1] = &mut self.src_types;
        let [src0, src1, src2] = &mut self.srcs;
        if swap_srcs_if_not_reg(src0, src1, gpr) {
            std::mem::swap(src_type0, src_type1);
        }
        b.copy_alu_src_if_not_reg(src0, gpr, SrcType::ALU);
        b.copy_alu_src_if_ineg_imm(src1, gpr, SrcType::I32);
        b.copy_alu_src_if_not_reg(src2, gpr, SrcType::ALU);
    }

    fn encode(&self, e: &mut SM70Encoder<'_>) {
        e.encode_alu(
            0x026,
            Some(&self.dst),
            Some(&self.srcs[0]),
            Some(&self.srcs[1]),
            Some(&self.srcs[2]),
        );

        e.set_bit(
            73,
            match self.src_types[0] {
                IntType::U8 => false,
                IntType::I8 => true,
                _ => panic!("Invalid DP4 source type"),
            },
        );
        e.set_bit(
            74,
            match self.src_types[1] {
                IntType::U8 => false,
                IntType::I8 => true,
                _ => panic!("Invalid DP4 source type"),
            },
        );
    }
}

impl SM70Op for OpIMad {
    fn legalize(&mut self, b: &mut LegalizeBuilder) {
        let gpr = op_gpr(self);
        let [src0, src1, src2] = &mut self.srcs;
        swap_srcs_if_not_reg(src0, src1, gpr);
        b.copy_alu_src_if_not_reg(src0, gpr, SrcType::ALU);
        b.copy_alu_src_if_both_not_reg(src1, src2, gpr, SrcType::ALU);
    }

    fn encode(&self, e: &mut SM70Encoder<'_>) {
        if self.is_uniform() {
            e.encode_ualu(
                0x0a4,
                Some(&self.dst),
                Some(&self.srcs[0]),
                Some(&self.srcs[1]),
                Some(&self.srcs[2]),
            );
        } else {
            e.encode_alu(
                0x024,
                Some(&self.dst),
                Some(&self.srcs[0]),
                Some(&self.srcs[1]),
                Some(&self.srcs[2]),
            );
        }
        e.set_pred_dst(81..84, &Dst::None);
        e.set_bit(73, self.signed);
    }
}

impl SM70Op for OpIMad64 {
    fn legalize(&mut self, b: &mut LegalizeBuilder) {
        let gpr = op_gpr(self);
        let [src0, src1, src2] = &mut self.srcs;
        swap_srcs_if_not_reg(src0, src1, gpr);
        b.copy_alu_src_if_not_reg(src0, gpr, SrcType::ALU);
        b.copy_alu_src_if_both_not_reg(src1, src2, gpr, SrcType::ALU);
    }

    fn encode(&self, e: &mut SM70Encoder<'_>) {
        if self.is_uniform() {
            e.encode_ualu(
                0x0a5,
                Some(&self.dst),
                Some(&self.srcs[0]),
                Some(&self.srcs[1]),
                Some(&self.srcs[2]),
            );
        } else {
            e.encode_alu(
                0x025,
                Some(&self.dst),
                Some(&self.srcs[0]),
                Some(&self.srcs[1]),
                Some(&self.srcs[2]),
            );
        }
        e.set_pred_dst(81..84, &Dst::None);
        e.set_bit(73, self.signed);
    }
}

impl SM70Op for OpIMnMx {
    fn legalize(&mut self, b: &mut LegalizeBuilder) {
        let gpr = op_gpr(self);
        let [src0, src1] = &mut self.srcs;
        swap_srcs_if_not_reg(src0, src1, gpr);
        b.copy_alu_src_if_not_reg(src0, gpr, SrcType::ALU);
    }

    fn encode(&self, e: &mut SM70Encoder<'_>) {
        e.encode_alu(
            0x017,
            Some(&self.dst),
            Some(&self.srcs[0]),
            Some(&self.srcs[1]),
            None,
        );
        e.set_pred_src(87..90, 90, &self.min);
        e.set_bit(
            73,
            match self.cmp_type {
                IntCmpType::U32 => false,
                IntCmpType::I32 => true,
            },
        );
        if e.sm >= 120 {
            e.set_bit(74, false); // 64-bit
            e.set_pred_src(77..80, 80, &false.into());
            e.set_pred_dst(81..84, &Dst::None);
            e.set_pred_dst(84..87, &Dst::None);
        }
    }
}

impl SM70Op for OpISetP {
    fn legalize(&mut self, b: &mut LegalizeBuilder) {
        let gpr = op_gpr(self);
        let [src0, src1] = &mut self.srcs;
        if !src_is_reg(src0, gpr) && src_is_reg(src1, gpr) {
            std::mem::swap(src0, src1);
            self.cmp_op = self.cmp_op.flip();
        }
        b.copy_alu_src_if_not_reg(src0, gpr, SrcType::ALU);
        if !self.is_uniform() {
            b.copy_src_if_upred(&mut self.low_cmp);
            b.copy_src_if_upred(&mut self.accum);
        }
    }

    fn encode(&self, e: &mut SM70Encoder<'_>) {
        if self.is_uniform() {
            e.encode_ualu(0x08c, None, Some(&self.srcs[0]), Some(&self.srcs[1]), None);

            e.set_upred_src(68..71, 71, &self.low_cmp);
            e.set_upred_src(87..90, 90, &self.accum);
        } else {
            e.encode_alu(0x00c, None, Some(&self.srcs[0]), Some(&self.srcs[1]), None);

            e.set_pred_src(68..71, 71, &self.low_cmp);
            e.set_pred_src(87..90, 90, &self.accum);
        }

        e.set_bit(72, self.ex);

        e.set_field(
            73..74,
            match self.cmp_type {
                IntCmpType::U32 => 0_u32,
                IntCmpType::I32 => 1_u32,
            },
        );
        e.set_pred_set_op(74..76, self.set_op);
        e.set_int_cmp_op(76..79, self.cmp_op);

        e.set_pred_dst(81..84, &self.dst);
        e.set_pred_dst(84..87, &Dst::None); // dst1
    }
}

impl SM70Op for OpLea {
    fn legalize(&mut self, b: &mut LegalizeBuilder) {
        let gpr = op_gpr(self);
        b.copy_alu_src_if_not_reg(&mut self.a, gpr, SrcType::ALU);
        if self.dst_high {
            b.copy_alu_src_if_both_not_reg(&self.b, &mut self.a_high, gpr, SrcType::ALU);
        }
    }

    fn encode(&self, e: &mut SM70Encoder<'_>) {
        assert!(self.a.modifier == SrcMod::None);
        assert!(self.intermediate_mod == SrcMod::None || self.b.modifier == SrcMod::None);

        let zero = 0.into();
        let c = if self.dst_high {
            Some(&self.a_high)
        } else {
            // TODO: On Ada and earlier, src2 is ignored if !dst_high. On
            // Blackwell+, it seems to do something.
            Some(&zero)
        };

        if self.is_uniform() {
            e.encode_ualu(0x091, Some(&self.dst), Some(&self.a), Some(&self.b), c);
        } else {
            e.encode_alu(0x011, Some(&self.dst), Some(&self.a), Some(&self.b), c);
        }

        e.set_bit(72, self.intermediate_mod.is_ineg());
        e.set_field(75..80, self.shift);
        e.set_bit(80, self.dst_high);
        e.set_pred_dst(81..84, &self.overflow);
        e.set_bit(74, false); // .X
    }
}

impl SM70Op for OpLeaX {
    fn legalize(&mut self, b: &mut LegalizeBuilder) {
        let gpr = op_gpr(self);
        b.copy_alu_src_if_not_reg(&mut self.a, gpr, SrcType::ALU);
        if self.dst_high {
            b.copy_alu_src_if_both_not_reg(&self.b, &mut self.a_high, gpr, SrcType::ALU);
        }
    }

    fn encode(&self, e: &mut SM70Encoder<'_>) {
        assert!(self.a.modifier == SrcMod::None);
        assert!(self.intermediate_mod == SrcMod::None || self.b.modifier == SrcMod::None);

        let c = if self.dst_high {
            Some(&self.a_high)
        } else {
            // TODO: On Ada and earlier, src2 is ignored if !dst_high. On
            // Blackwell+, it seems to do something.
            Some(&Src::ZERO)
        };

        if self.is_uniform() {
            e.encode_ualu(0x091, Some(&self.dst), Some(&self.a), Some(&self.b), c);
            e.set_upred_src(87..90, 90, &self.carry);
        } else {
            e.encode_alu(0x011, Some(&self.dst), Some(&self.a), Some(&self.b), c);
            e.set_pred_src(87..90, 90, &self.carry);
        }

        e.set_bit(72, self.intermediate_mod.is_bnot());
        e.set_field(75..80, self.shift);
        e.set_bit(80, self.dst_high);
        e.set_pred_dst(81..84, &self.overflow);
        e.set_bit(74, true); // .X
    }
}

impl SM70Op for OpLop3 {
    fn legalize(&mut self, b: &mut LegalizeBuilder) {
        let gpr = op_gpr(self);
        // Fold constants and modifiers if we can
        self.op = LogicOp3::new_lut(&|mut x, mut y, mut z| {
            fold_lop_src(&self.srcs[0], &mut x);
            fold_lop_src(&self.srcs[1], &mut y);
            fold_lop_src(&self.srcs[2], &mut z);
            self.op.eval(x, y, z)
        });
        for src in &mut self.srcs {
            src.modifier = SrcMod::None;
            if src_as_lop_imm(src).is_some() {
                src.reference = SrcRef::Zero;
            }
        }

        let [src0, src1, src2] = &mut self.srcs;
        if !src_is_reg(src0, gpr) && src_is_reg(src1, gpr) {
            std::mem::swap(src0, src1);
            self.op = LogicOp3::new_lut(&|x, y, z| self.op.eval(y, x, z));
        }
        if !src_is_reg(src2, gpr) && src_is_reg(src1, gpr) {
            std::mem::swap(src2, src1);
            self.op = LogicOp3::new_lut(&|x, y, z| self.op.eval(x, z, y));
        }

        b.copy_alu_src_if_not_reg(src0, gpr, SrcType::ALU);
        b.copy_alu_src_if_not_reg(src2, gpr, SrcType::ALU);
    }

    fn encode(&self, e: &mut SM70Encoder<'_>) {
        if self.is_uniform() {
            e.encode_ualu(
                0x092,
                Some(&self.dst),
                Some(&self.srcs[0]),
                Some(&self.srcs[1]),
                Some(&self.srcs[2]),
            );

            e.set_upred_src(87..90, 90, &SrcRef::False.into());
        } else {
            e.encode_alu(
                0x012,
                Some(&self.dst),
                Some(&self.srcs[0]),
                Some(&self.srcs[1]),
                Some(&self.srcs[2]),
            );

            e.set_pred_src(87..90, 90, &SrcRef::False.into());
        }

        e.set_field(72..80, self.op.lut);
        e.set_bit(80, false); // .PAND
        e.set_field(81..84, 7_u32); // pred
    }
}

impl SM70Op for OpPopC {
    fn legalize(&mut self, _b: &mut LegalizeBuilder) {
        // Nothing to do
    }

    fn encode(&self, e: &mut SM70Encoder<'_>) {
        if self.is_uniform() {
            e.encode_ualu(0x0bf, Some(&self.dst), None, Some(&self.src), None);
        } else {
            e.encode_alu(0x109, Some(&self.dst), None, Some(&self.src), None);
        }

        let not_mod = matches!(self.src.modifier, SrcMod::BNot);
        e.set_field(63..64, not_mod);
    }
}

impl SM70Op for OpShf {
    fn legalize(&mut self, b: &mut LegalizeBuilder) {
        let gpr = op_gpr(self);
        b.copy_alu_src_if_not_reg(&mut self.low, gpr, SrcType::ALU);
        b.copy_alu_src_if_both_not_reg(&self.shift, &mut self.high, gpr, SrcType::ALU);
        self.reduce_shift_imm();
    }

    fn encode(&self, e: &mut SM70Encoder<'_>) {
        if self.is_uniform() {
            e.encode_ualu(
                0x099,
                Some(&self.dst),
                Some(&self.low),
                Some(&self.shift),
                Some(&self.high),
            );
        } else {
            e.encode_alu(
                0x019,
                Some(&self.dst),
                Some(&self.low),
                Some(&self.shift),
                Some(&self.high),
            );
        }

        e.set_field(
            73..75,
            match self.data_type {
                IntType::I64 => 0_u8,
                IntType::U64 => 1_u8,
                IntType::I32 => 2_u8,
                IntType::U32 => 3_u8,
                _ => panic!("Invalid shift data type"),
            },
        );
        e.set_bit(75, self.wrap);
        e.set_bit(76, self.right);
        e.set_bit(80, self.dst_high);
    }
}
