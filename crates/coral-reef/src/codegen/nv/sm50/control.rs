// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright © 2025-2026 ecoPrimals
// Derived from Collabora, Ltd. (2023)
//! SM50 control flow instruction encoders.

use super::encoder::*;
use crate::codegen::ir::RegFile;

/// Unconditional predicate: PT (predicate true, bits 0..5 = 0xF).
/// All control flow instructions use this when not predicated.
const PRED_TRUE: u8 = 0xF;

/// Unconditional condition code: CC.T (bits 0..4 = 0xF, always true).
const CC_TRUE: u8 = 0xF;

impl SM50Op for OpBra {
    fn legalize(&mut self, _b: &mut LegalizeBuilder) {
        // Nothing to do
    }

    fn encode(&self, e: &mut SM50Encoder<'_>) {
        e.set_opcode(0xe240);
        e.set_rel_offset(20..44, &self.target);
        e.set_field(0..5, PRED_TRUE);
    }
}

impl SM50Op for OpSSy {
    fn legalize(&mut self, _b: &mut LegalizeBuilder) {
        // Nothing to do
    }

    fn encode(&self, e: &mut SM50Encoder<'_>) {
        e.set_opcode(0xe290);
        e.set_rel_offset(20..44, &self.target);
        e.set_field(0..5, PRED_TRUE);
    }
}

impl SM50Op for OpSync {
    fn legalize(&mut self, _b: &mut LegalizeBuilder) {
        // Nothing to do
    }

    fn encode(&self, e: &mut SM50Encoder<'_>) {
        e.set_opcode(0xf0f8);
        e.set_field(0..5, PRED_TRUE);
    }
}

impl SM50Op for OpBrk {
    fn legalize(&mut self, _b: &mut LegalizeBuilder) {
        // Nothing to do
    }

    fn encode(&self, e: &mut SM50Encoder<'_>) {
        e.set_opcode(0xe340);
        e.set_field(0..5, PRED_TRUE);
    }
}

impl SM50Op for OpPBk {
    fn legalize(&mut self, _b: &mut LegalizeBuilder) {
        // Nothing to do
    }

    fn encode(&self, e: &mut SM50Encoder<'_>) {
        e.set_opcode(0xe2a0);
        e.set_rel_offset(20..44, &self.target);
        e.set_field(0..5, PRED_TRUE);
    }
}

impl SM50Op for OpCont {
    fn legalize(&mut self, _b: &mut LegalizeBuilder) {
        // Nothing to do
    }

    fn encode(&self, e: &mut SM50Encoder<'_>) {
        e.set_opcode(0xe350);
        e.set_field(0..5, PRED_TRUE);
    }
}

impl SM50Op for OpPCnt {
    fn legalize(&mut self, _b: &mut LegalizeBuilder) {
        // Nothing to do
    }

    fn encode(&self, e: &mut SM50Encoder<'_>) {
        e.set_opcode(0xe2b0);
        e.set_rel_offset(20..44, &self.target);
        e.set_field(0..5, PRED_TRUE);
    }
}

impl SM50Op for OpExit {
    fn legalize(&mut self, _b: &mut LegalizeBuilder) {
        // Nothing to do
    }

    fn encode(&self, e: &mut SM50Encoder<'_>) {
        e.set_opcode(0xe300);

        e.set_field(0..4, CC_TRUE);
    }
}

impl SM50Op for OpBar {
    fn legalize(&mut self, _b: &mut LegalizeBuilder) {
        // Nothing to do
    }

    fn encode(&self, e: &mut SM50Encoder<'_>) {
        e.set_opcode(0xf0a8);

        e.set_reg_src(8..16, &SrcRef::Zero.into());

        // 00: RED.POPC
        // 01: RED.AND
        // 02: RED.OR
        e.set_field(35..37, 0_u8);

        // 00: SYNC
        // 01: ARV
        // 02: RED
        // 03: SCAN
        e.set_field(32..35, 0_u8);

        e.set_pred_src(39..42, 42, &SrcRef::True.into());
    }
}

impl SM50Op for OpCS2R {
    fn legalize(&mut self, _b: &mut LegalizeBuilder) {
        // Nothing to do
    }

    fn encode(&self, e: &mut SM50Encoder<'_>) {
        e.set_opcode(0x50c8);
        e.set_dst(&self.dst);
        e.set_field(20..28, self.idx);
    }
}

impl SM50Op for OpIsberd {
    fn legalize(&mut self, _b: &mut LegalizeBuilder) {
        // Nothing to do
    }

    fn encode(&self, e: &mut SM50Encoder<'_>) {
        e.set_opcode(0xefd0);
        e.set_dst(&self.dst);
        e.set_reg_src(8..16, &self.idx);
    }
}

impl SM50Op for OpKill {
    fn legalize(&mut self, _b: &mut LegalizeBuilder) {
        // Nothing to do
    }

    fn encode(&self, e: &mut SM50Encoder<'_>) {
        e.set_opcode(0xe330);
        e.set_field(0..5, 0x0f_u8);
    }
}

impl SM50Op for OpNop {
    fn legalize(&mut self, _b: &mut LegalizeBuilder) {
        // Nothing to do
    }

    fn encode(&self, e: &mut SM50Encoder<'_>) {
        e.set_opcode(0x50b0);

        e.set_field(8..12, CC_TRUE);
    }
}

impl SM50Op for OpPixLd {
    fn legalize(&mut self, _b: &mut LegalizeBuilder) {
        // Nothing to do
    }

    fn encode(&self, e: &mut SM50Encoder<'_>) {
        e.set_opcode(0xefe8);
        e.set_dst(&self.dst);
        e.set_reg_src(8..16, &0.into());
        e.set_field(
            31..34,
            match &self.val {
                PixVal::CovMask => 1_u8,
                PixVal::Covered => 2_u8,
                PixVal::Offset => 3_u8,
                PixVal::CentroidOffset => 4_u8,
                PixVal::MyIndex => 5_u8,
                other => crate::codegen::ice!("Unsupported PixVal: {other}"),
            },
        );
        e.set_pred_dst(45..48, &Dst::None);
    }
}

impl SM50Op for OpS2R {
    fn legalize(&mut self, _b: &mut LegalizeBuilder) {
        // Nothing to do
    }

    fn encode(&self, e: &mut SM50Encoder<'_>) {
        e.set_opcode(0xf0c8);
        e.set_dst(&self.dst);
        e.set_field(20..28, self.idx);
    }
}

impl SM50Op for OpVote {
    fn legalize(&mut self, _b: &mut LegalizeBuilder) {
        // Nothing to do
    }

    fn encode(&self, e: &mut SM50Encoder<'_>) {
        e.set_opcode(0x50d8);

        e.set_dst(self.ballot());
        e.set_pred_dst(45..48, self.vote());
        e.set_pred_src(39..42, 42, &self.pred);

        e.set_field(
            48..50,
            match self.op {
                VoteOp::All => 0u8,
                VoteOp::Any => 1u8,
                VoteOp::Eq => 2u8,
            },
        );
    }
}

impl SM50Op for OpOut {
    fn legalize(&mut self, b: &mut LegalizeBuilder) {
        use RegFile::GPR;
        b.copy_alu_src_if_not_reg(self.handle_mut(), GPR, SrcType::GPR);
        b.copy_alu_src_if_i20_overflow(self.stream_mut(), GPR, SrcType::ALU);
    }

    fn encode(&self, e: &mut SM50Encoder<'_>) {
        match &self.stream().reference {
            SrcRef::Zero | SrcRef::Reg(_) => {
                e.set_opcode(0xfbe0);
                e.set_reg_src(20..28, self.stream());
            }
            SrcRef::Imm32(imm32) => {
                e.set_opcode(0xf6e0);
                e.set_src_imm_i20(20..39, 56, *imm32);
            }
            SrcRef::CBuf(cbuf) => {
                e.set_opcode(0xebe0);
                e.set_src_cb(20..39, cbuf);
            }
            src => crate::codegen::ice!("Invalid out stream: {src}"),
        }

        e.set_field(
            39..41,
            match self.out_type {
                OutType::Emit => 1_u8,
                OutType::Cut => 2_u8,
                OutType::EmitThenCut => 3_u8,
            },
        );

        e.set_reg_src(8..16, self.handle());
        e.set_dst(&self.dst);
    }
}
