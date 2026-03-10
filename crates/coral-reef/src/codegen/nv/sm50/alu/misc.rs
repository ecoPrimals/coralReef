// SPDX-License-Identifier: AGPL-3.0-only
// Copyright © 2025-2026 ecoPrimals
// Derived from Collabora, Ltd. (2023)

use std::ops::Range;

use super::*;

impl SM50Encoder<'_> {
    pub(super) fn set_float_cmp_op(&mut self, range: Range<usize>, op: FloatCmpOp) {
        assert!(range.len() == 4);
        self.set_field(
            range,
            match op {
                FloatCmpOp::OrdLt => 0x01_u8,
                FloatCmpOp::OrdEq => 0x02_u8,
                FloatCmpOp::OrdLe => 0x03_u8,
                FloatCmpOp::OrdGt => 0x04_u8,
                FloatCmpOp::OrdNe => 0x05_u8,
                FloatCmpOp::OrdGe => 0x06_u8,
                FloatCmpOp::UnordLt => 0x09_u8,
                FloatCmpOp::UnordEq => 0x0a_u8,
                FloatCmpOp::UnordLe => 0x0b_u8,
                FloatCmpOp::UnordGt => 0x0c_u8,
                FloatCmpOp::UnordNe => 0x0d_u8,
                FloatCmpOp::UnordGe => 0x0e_u8,
                FloatCmpOp::IsNum => 0x07_u8,
                FloatCmpOp::IsNan => 0x08_u8,
            },
        );
    }

    pub(super) fn set_pred_set_op(&mut self, range: Range<usize>, op: PredSetOp) {
        assert!(range.len() == 2);
        self.set_field(
            range,
            match op {
                PredSetOp::And => 0_u8,
                PredSetOp::Or => 1_u8,
                PredSetOp::Xor => 2_u8,
            },
        );
    }

    pub(super) fn set_int_cmp_op(&mut self, range: Range<usize>, op: IntCmpOp) {
        assert!(range.len() == 3);
        self.set_field(
            range,
            match op {
                IntCmpOp::False => 0_u8,
                IntCmpOp::True => 7_u8,
                IntCmpOp::Eq => 2_u8,
                IntCmpOp::Ne => 5_u8,
                IntCmpOp::Lt => 1_u8,
                IntCmpOp::Le => 3_u8,
                IntCmpOp::Gt => 4_u8,
                IntCmpOp::Ge => 6_u8,
            },
        );
    }
}

impl SM50Op for OpMov {
    fn legalize(&mut self, _b: &mut LegalizeBuilder) {
        // Nothing to do
    }

    fn encode(&self, e: &mut SM50Encoder<'_>) {
        match &self.src.reference {
            SrcRef::Zero | SrcRef::Reg(_) => {
                e.set_opcode(0x5c98);
                e.set_reg_src(20..28, &self.src);
                e.set_field(39..43, self.quad_lanes);
            }
            SrcRef::Imm32(imm32) => {
                e.set_opcode(0x0100);
                e.set_src_imm32(20..52, *imm32);
                e.set_field(12..16, self.quad_lanes);
            }
            SrcRef::CBuf(cb) => {
                e.set_opcode(0x4c98);
                e.set_src_cb(20..39, cb);
                e.set_field(39..43, self.quad_lanes);
            }
            src => panic!("Invalid mov src: {src}"),
        }

        e.set_dst(&self.dst);
    }
}

impl SM50Op for OpPrmt {
    fn legalize(&mut self, b: &mut LegalizeBuilder) {
        use RegFile::GPR;
        b.copy_alu_src_if_not_reg(&mut self.srcs[0], GPR, SrcType::GPR);
        b.copy_alu_src_if_not_reg(&mut self.srcs[1], GPR, SrcType::GPR);
        self.reduce_sel_imm();
    }

    fn encode(&self, e: &mut SM50Encoder<'_>) {
        match &self.sel().reference {
            SrcRef::Zero | SrcRef::Reg(_) => {
                e.set_opcode(0x5bc0);
                e.set_reg_src(20..28, self.sel());
            }
            SrcRef::Imm32(imm32) => {
                e.set_opcode(0x36c0);
                // Only the bottom 16 bits matter
                e.set_src_imm_i20(20..39, 56, *imm32);
            }
            SrcRef::CBuf(cb) => {
                e.set_opcode(0x4bc0);
                e.set_src_cb(20..39, cb);
            }
            src => panic!("Invalid prmt selector: {src}"),
        }

        e.set_dst(&self.dst);
        e.set_reg_src(8..16, &self.srcs[0]);
        e.set_reg_src(39..47, &self.srcs[1]);
        e.set_field(
            48..51,
            match self.mode {
                PrmtMode::Index => 0_u8,
                PrmtMode::Forward4Extract => 1_u8,
                PrmtMode::Backward4Extract => 2_u8,
                PrmtMode::Replicate8 => 3_u8,
                PrmtMode::EdgeClampLeft => 4_u8,
                PrmtMode::EdgeClampRight => 5_u8,
                PrmtMode::Replicate16 => 6_u8,
            },
        );
    }
}

impl SM50Op for OpSel {
    fn legalize(&mut self, b: &mut LegalizeBuilder) {
        use RegFile::GPR;
        let cond_val = self.cond().clone().bnot();
        let swapped = {
            let [_, src0, src1] = &mut self.srcs;
            swap_srcs_if_not_reg(src0, src1, GPR)
        };
        if swapped {
            *self.cond_mut() = cond_val;
        }
        b.copy_alu_src_if_not_reg(&mut self.srcs[1], GPR, SrcType::ALU);
        b.copy_alu_src_if_i20_overflow(&mut self.srcs[2], GPR, SrcType::ALU);
    }

    fn encode(&self, e: &mut SM50Encoder<'_>) {
        match &self.srcs[2].reference {
            SrcRef::Zero | SrcRef::Reg(_) => {
                e.set_opcode(0x5ca0);
                e.set_reg_src_ref(20..28, &self.srcs[2].reference);
            }
            SrcRef::Imm32(imm32) => {
                e.set_opcode(0x38a0);
                e.set_src_imm_i20(20..39, 56, *imm32);
            }
            SrcRef::CBuf(cbuf) => {
                e.set_opcode(0x4ca0);
                e.set_src_cb(20..39, cbuf);
            }
            src => panic!("Invalid sel src1: {src}"),
        }

        e.set_dst(&self.dst);
        e.set_reg_src(8..16, &self.srcs[1]);
        e.set_pred_src(39..42, 42, self.cond());
    }
}

impl SM50Op for OpShfl {
    fn legalize(&mut self, b: &mut LegalizeBuilder) {
        use RegFile::GPR;
        b.copy_alu_src_if_not_reg(self.src_mut(), GPR, SrcType::GPR);
        b.copy_alu_src_if_not_reg_or_imm(self.lane_mut(), GPR, SrcType::ALU);
        b.copy_alu_src_if_not_reg_or_imm(self.c_mut(), GPR, SrcType::ALU);
        self.reduce_lane_c_imm();
    }

    fn encode(&self, e: &mut SM50Encoder<'_>) {
        e.set_opcode(0xef10);

        e.set_dst(self.dst());
        e.set_pred_dst(48..51, self.in_bounds());
        e.set_reg_src(8..16, self.src());

        match &self.lane().reference {
            SrcRef::Zero | SrcRef::Reg(_) => {
                e.set_bit(28, false);
                e.set_reg_src(20..28, self.lane());
            }
            SrcRef::Imm32(imm32) => {
                e.set_bit(28, true);
                e.set_field(20..25, *imm32);
            }
            src => panic!("Invalid shfl lane: {src}"),
        }
        match &self.c().reference {
            SrcRef::Zero | SrcRef::Reg(_) => {
                e.set_bit(29, false);
                e.set_reg_src(39..47, self.c());
            }
            SrcRef::Imm32(imm32) => {
                e.set_bit(29, true);
                e.set_field(34..47, *imm32);
            }
            src => panic!("Invalid shfl c: {src}"),
        }

        e.set_field(
            30..32,
            match self.op {
                ShflOp::Idx => 0u8,
                ShflOp::Up => 1u8,
                ShflOp::Down => 2u8,
                ShflOp::Bfly => 3u8,
            },
        );
    }
}

impl SM50Op for OpPSetP {
    fn legalize(&mut self, _b: &mut LegalizeBuilder) {
        // Nothing to do
    }

    fn encode(&self, e: &mut SM50Encoder<'_>) {
        e.set_opcode(0x5090);

        e.set_pred_dst(3..6, &self.dsts[0]);
        e.set_pred_dst(0..3, &self.dsts[1]);

        e.set_pred_src(12..15, 15, &self.srcs[0]);
        e.set_pred_src(29..32, 32, &self.srcs[1]);
        e.set_pred_src(39..42, 42, &self.srcs[2]);

        e.set_pred_set_op(24..26, self.ops[0]);
        e.set_pred_set_op(45..47, self.ops[1]);
    }
}
