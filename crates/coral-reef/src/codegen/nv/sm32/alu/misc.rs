// SPDX-License-Identifier: AGPL-3.0-only
use super::*;

impl SM32Encoder<'_> {
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

impl SM32Op for OpMov {
    fn legalize(&mut self, _b: &mut LegalizeBuilder) {
        // Nothing to do
    }

    fn encode(&self, e: &mut SM32Encoder<'_>) {
        match &self.src.reference {
            SrcRef::Zero | SrcRef::Reg(_) => {
                e.set_opcode(0xe4c, 2);
                e.set_reg_src(23..31, &self.src);
                e.set_field(42..46, self.quad_lanes);
            }
            SrcRef::Imm32(limm) => {
                e.set_opcode(0x747, 2);
                e.set_field(23..55, *limm);

                e.set_field(14..18, self.quad_lanes);
            }
            SrcRef::CBuf(cb) => {
                e.set_opcode(0x64c, 2);
                e.set_src_cbuf(23..42, cb);
                e.set_field(42..46, self.quad_lanes);
            }
            src => crate::codegen::ice!("Invalid mov src: {src}"),
        }

        e.set_dst(&self.dst);
    }
}

impl SM32Op for OpPrmt {
    fn legalize(&mut self, b: &mut LegalizeBuilder) {
        use RegFile::GPR;
        b.copy_alu_src_if_not_reg(&mut self.srcs[0], GPR, SrcType::GPR);
        b.copy_alu_src_if_not_reg(&mut self.srcs[1], GPR, SrcType::GPR);
        self.reduce_sel_imm();
    }

    fn encode(&self, e: &mut SM32Encoder<'_>) {
        e.encode_form_immreg(
            0xb60,
            0x1e0,
            Some(&self.dst),
            &self.srcs[0],
            self.sel(),
            Some(&self.srcs[1]),
            false,
        );

        e.set_field(
            51..54,
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

impl SM32Op for OpSel {
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

    fn encode(&self, e: &mut SM32Encoder<'_>) {
        e.encode_form_immreg(
            0xc50,
            0x250,
            Some(&self.dst),
            &self.srcs[1],
            &self.srcs[2],
            None,
            false,
        );

        e.set_pred_src(42..46, self.cond());
    }
}

impl SM32Op for OpShfl {
    fn legalize(&mut self, b: &mut LegalizeBuilder) {
        use RegFile::GPR;
        b.copy_alu_src_if_not_reg(self.src_mut(), GPR, SrcType::GPR);
        b.copy_alu_src_if_not_reg_or_imm(self.lane_mut(), GPR, SrcType::ALU);
        // shfl.up alone requires lane to be 4-aligned ¯\_(ツ)_/¯
        if self.op == ShflOp::Up {
            b.align_reg(self.lane_mut(), 4, PadValue::Zero);
        }

        b.copy_alu_src_if_not_reg_or_imm(self.c_mut(), GPR, SrcType::ALU);
        self.reduce_lane_c_imm();
    }

    fn encode(&self, e: &mut SM32Encoder<'_>) {
        e.set_opcode(0x788, 2);

        e.set_dst(self.dst());
        e.set_pred_dst(51..54, self.in_bounds());
        e.set_reg_src(10..18, self.src());

        e.set_field(
            33..35,
            match self.op {
                ShflOp::Idx => 0u8,
                ShflOp::Up => 1u8,
                ShflOp::Down => 2u8,
                ShflOp::Bfly => 3u8,
            },
        );

        match &self.lane().reference {
            SrcRef::Zero | SrcRef::Reg(_) => {
                e.set_reg_src(23..31, self.lane());
                e.set_bit(31, false);
            }
            SrcRef::Imm32(imm32) => {
                e.set_field(23..28, *imm32);
                e.set_bit(31, true);
            }
            src => crate::codegen::ice!("Invalid shfl lane: {src}"),
        }
        match &self.c().reference {
            SrcRef::Zero | SrcRef::Reg(_) => {
                e.set_bit(32, false);
                e.set_reg_src(42..50, self.c());
            }
            SrcRef::Imm32(imm32) => {
                e.set_bit(32, true);
                e.set_field(37..50, *imm32);
            }
            src => crate::codegen::ice!("Invalid shfl c: {src}"),
        }
    }
}

impl SM32Op for OpPSetP {
    fn legalize(&mut self, _b: &mut LegalizeBuilder) {
        // Nothing to do
    }

    fn encode(&self, e: &mut SM32Encoder<'_>) {
        e.set_opcode(0x848, 2);

        e.set_pred_dst(5..8, &self.dsts[0]);
        e.set_pred_dst(2..5, &self.dsts[1]);

        e.set_pred_src(14..18, &self.srcs[0]);
        e.set_pred_src(32..36, &self.srcs[1]);
        e.set_pred_src(42..46, &self.srcs[2]);

        e.set_pred_set_op(27..29, self.ops[0]);
        e.set_pred_set_op(48..50, self.ops[1]);
    }
}
