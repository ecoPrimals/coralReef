use super::*;

impl SM20Op for OpMov {
    fn legalize(&mut self, _b: &mut LegalizeBuilder) {}

    fn encode(&self, e: &mut SM20Encoder<'_>) {
        if let Some(imm32) = self.src.as_imm_not_i20() {
            e.encode_form_b_imm32(0x6, &self.dst, imm32);
        } else {
            e.encode_form_b(SM20Unit::Move, 0xa, &self.dst, &self.src);
        }
        e.set_field(5..9, self.quad_lanes);
    }
}

impl SM20Op for OpPrmt {
    fn legalize(&mut self, b: &mut LegalizeBuilder) {
        use RegFile::GPR;
        let [src0, src1] = &mut self.srcs;
        b.copy_alu_src_if_not_reg(src0, GPR, SrcType::ALU);
        b.copy_alu_src_if_not_reg(src1, GPR, SrcType::ALU);
        self.reduce_sel_imm();
    }

    fn encode(&self, e: &mut SM20Encoder<'_>) {
        e.encode_form_a(
            SM20Unit::Move,
            0x9,
            &self.dst,
            &self.srcs[0],
            &self.sel,
            Some(&self.srcs[1]),
        );
        e.set_field(
            5..8,
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

impl SM20Op for OpSel {
    fn legalize(&mut self, b: &mut LegalizeBuilder) {
        use RegFile::GPR;
        let [src0, src1] = &mut self.srcs;
        if swap_srcs_if_not_reg(src0, src1, GPR) {
            self.cond = self.cond.clone().bnot();
        }
        b.copy_alu_src_if_not_reg(src0, GPR, SrcType::ALU);
        b.copy_alu_src_if_i20_overflow(src1, GPR, SrcType::ALU);
    }

    fn encode(&self, e: &mut SM20Encoder<'_>) {
        e.encode_form_a(
            SM20Unit::Move,
            0x8,
            &self.dst,
            &self.srcs[0],
            &self.srcs[1],
            None,
        );
        e.set_pred_src(49..53, &self.cond);
    }
}

impl SM20Op for OpShfl {
    fn legalize(&mut self, b: &mut LegalizeBuilder) {
        use RegFile::GPR;
        if matches!(self.lane.src_ref, SrcRef::CBuf(_)) {
            b.copy_alu_src(&mut self.lane, GPR, SrcType::ALU);
        }
        if matches!(self.c.src_ref, SrcRef::CBuf(_)) {
            b.copy_alu_src(&mut self.c, GPR, SrcType::ALU);
        }
        self.reduce_lane_c_imm();
    }

    fn encode(&self, e: &mut SM20Encoder<'_>) {
        e.set_opcode(SM20Unit::Mem, 0x22);
        e.set_pred_dst2(8..10, 58..59, &self.in_bounds);
        e.set_dst(14..20, &self.dst);
        e.set_reg_src(20..26, &self.src);
        assert!(self.lane.src_mod.is_none());
        if let Some(u) = self.lane.src_ref.as_u32() {
            e.set_field(26..32, u);
            e.set_bit(5, true);
        } else {
            e.set_reg_src(26..32, &self.lane);
            e.set_bit(5, false);
        }
        assert!(self.c.src_mod.is_none());
        if let Some(u) = self.c.src_ref.as_u32() {
            e.set_field(42..55, u);
            e.set_bit(6, true);
        } else {
            e.set_reg_src(49..55, &self.c);
            e.set_bit(6, false);
        }
        e.set_field(
            55..57,
            match self.op {
                ShflOp::Idx => 0_u8,
                ShflOp::Up => 1_u8,
                ShflOp::Down => 2_u8,
                ShflOp::Bfly => 3_u8,
            },
        );
    }
}

impl SM20Op for OpPSetP {
    fn legalize(&mut self, _b: &mut LegalizeBuilder) {}

    fn encode(&self, e: &mut SM20Encoder<'_>) {
        e.set_opcode(SM20Unit::Move, 0x3);
        e.set_pred_dst(14..17, &self.dsts[1]);
        e.set_pred_dst(17..20, &self.dsts[0]);
        e.set_pred_src(20..24, &self.srcs[0]);
        e.set_pred_src(26..30, &self.srcs[1]);
        e.set_pred_set_op(30..32, self.ops[0]);
        e.set_pred_src(49..53, &self.srcs[2]);
        e.set_pred_set_op(53..55, self.ops[1]);
    }
}
