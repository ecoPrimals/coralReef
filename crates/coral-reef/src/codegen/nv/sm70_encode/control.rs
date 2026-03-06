// SPDX-License-Identifier: AGPL-3.0-only
// Copyright © 2025-2026 ecoPrimals
// Derived from Collabora, Ltd. (2022)
//! SM70 control flow, barrier, and miscellaneous instruction encoders.

#![allow(clippy::wildcard_imports)]

use super::encoder::*;

impl SM70Encoder<'_> {
    fn get_rel_offset(&self, label: &Label) -> i64 {
        let ip = u64::try_from(self.ip).unwrap();
        let ip = i64::try_from(ip).unwrap();

        let target_ip = *self.labels.get(label).unwrap();
        let target_ip = u64::try_from(target_ip).unwrap();
        let target_ip = i64::try_from(target_ip).unwrap();

        target_ip - ip - 4
    }

    fn set_rel_offset(&mut self, range: Range<usize>, label: &Label) {
        let rel_offset = self.get_rel_offset(label);
        self.set_field(range, rel_offset);
    }

    fn set_rel_offset2(&mut self, range1: Range<usize>, range2: Range<usize>, label: &Label) {
        let rel_offset = self.get_rel_offset(label);
        self.set_field2(range1, range2, rel_offset);
    }
}

impl SM70Op for OpBClear {
    fn legalize(&mut self, _b: &mut LegalizeBuilder) {
        // Nothing to do
    }

    fn encode(&self, e: &mut SM70Encoder<'_>) {
        e.set_opcode(0x355);

        e.set_dst(&Dst::None);
        e.set_bar_dst(24..28, &self.dst);

        e.set_bit(84, true); // .CLEAR
    }
}

impl SM70Op for OpBMov {
    fn legalize(&mut self, _b: &mut LegalizeBuilder) {
        // Nothing to do
    }

    fn encode(&self, e: &mut SM70Encoder<'_>) {
        if dst_is_bar(&self.dst) {
            e.set_opcode(0x356);

            e.set_bar_dst(24..28, &self.dst);
            e.set_reg_src(32..40, &self.src);
        } else {
            e.set_opcode(0x355);

            e.set_dst(&self.dst);
            e.set_bar_src(24..28, &self.src);
        }
        e.set_bit(84, self.clear);
    }
}

impl SM70Op for OpBreak {
    fn legalize(&mut self, _b: &mut LegalizeBuilder) {
        // Nothing to do
    }

    fn encode(&self, e: &mut SM70Encoder<'_>) {
        e.set_opcode(0x942);
        assert!(self.bar_in.reference.as_reg() == self.bar_out.as_reg());
        e.set_bar_dst(16..20, &self.bar_out);
        e.set_pred_src(87..90, 90, &self.cond);
    }
}

impl SM70Op for OpBSSy {
    fn legalize(&mut self, _b: &mut LegalizeBuilder) {
        // Nothing to do
    }

    fn encode(&self, e: &mut SM70Encoder<'_>) {
        e.set_opcode(0x945);
        assert!(self.bar_in.reference.as_reg() == self.bar_out.as_reg());
        e.set_bar_dst(16..20, &self.bar_out);
        e.set_rel_offset(34..64, &self.target);
        e.set_pred_src(87..90, 90, &self.cond);
    }
}

impl SM70Op for OpBSync {
    fn legalize(&mut self, _b: &mut LegalizeBuilder) {
        // Nothing to do
    }

    fn encode(&self, e: &mut SM70Encoder<'_>) {
        e.set_opcode(0x941);
        e.set_bar_src(16..20, &self.bar);
        e.set_pred_src(87..90, 90, &self.cond);
    }
}

impl SM70Op for OpBra {
    fn legalize(&mut self, _b: &mut LegalizeBuilder) {
        // Nothing to do
    }

    fn encode(&self, e: &mut SM70Encoder<'_>) {
        if self.cond.is_upred_reg() {
            assert!(e.sm >= 80);
            e.set_opcode(0x547);
            e.set_upred_src(24..27, 27, &self.cond);
            e.set_bit(32, true); // .U
            e.set_field(87..90, 0x7_u8);
            e.set_bit(91, true);
        } else {
            e.set_opcode(0x947);
            e.set_bit(32, false); // .U
            e.set_pred_src(87..90, 90, &self.cond);
        }

        if e.sm >= 100 {
            e.set_rel_offset2(16..24, 34..82, &self.target);
        } else {
            e.set_rel_offset(34..82, &self.target);
        }
    }
}

impl SM70Op for OpExit {
    fn legalize(&mut self, _b: &mut LegalizeBuilder) {
        // Nothing to do
    }

    fn encode(&self, e: &mut SM70Encoder<'_>) {
        e.set_opcode(0x94d);

        // ./.KEEPREFCOUNT/.PREEMPTED/.INVALID3
        e.set_field(84..85, false);
        e.set_field(85..86, false); // .NO_ATEXIT
        e.set_field(87..90, 0x7_u8); // TODO: Predicate
        e.set_field(90..91, false); // NOT
    }
}

impl SM70Op for OpWarpSync {
    fn legalize(&mut self, _b: &mut LegalizeBuilder) {
        // Nothing to do
    }

    fn encode(&self, e: &mut SM70Encoder<'_>) {
        e.encode_alu(0x148, None, None, Some(&Src::from(self.mask)), None);
        e.set_pred_src(87..90, 90, &SrcRef::True.into());
    }
}

impl SM70Op for OpBar {
    fn legalize(&mut self, _b: &mut LegalizeBuilder) {
        // Nothing to do
    }

    fn encode(&self, e: &mut SM70Encoder<'_>) {
        e.set_opcode(0xb1d);

        // e.set_opcode(0x31d);

        // // src0 == src1
        // e.set_reg_src(32..40, SrcRef::Zero.into());

        // // 00: RED.POPC
        // // 01: RED.AND
        // // 02: RED.OR
        // e.set_field(74..76, 0_u8);

        // // 00: SYNC
        // // 01: ARV
        // // 02: RED
        // // 03: SCAN
        // e.set_field(77..79, 0_u8);

        // e.set_pred_src(87..90, 90, SrcRef::True.into());
    }
}

impl SM70Op for OpCS2R {
    fn legalize(&mut self, _b: &mut LegalizeBuilder) {
        // Nothing to do
    }

    fn encode(&self, e: &mut SM70Encoder<'_>) {
        e.set_opcode(0x805);
        e.set_dst(&self.dst);
        e.set_field(72..80, self.idx);
        e.set_bit(80, self.dst.as_reg().unwrap().comps() == 2); // .64
    }
}

impl SM70Op for OpIsberd {
    fn legalize(&mut self, _b: &mut LegalizeBuilder) {
        // Nothing to do
    }

    fn encode(&self, e: &mut SM70Encoder<'_>) {
        e.set_opcode(0x923);
        e.set_dst(&self.dst);
        e.set_reg_src(24..32, &self.idx);
    }
}

impl SM70Op for OpKill {
    fn legalize(&mut self, _b: &mut LegalizeBuilder) {
        // Nothing to do
    }

    fn encode(&self, e: &mut SM70Encoder<'_>) {
        e.set_opcode(0x95b);
        e.set_pred_src(87..90, 90, &SrcRef::True.into());
    }
}

impl SM70Op for OpNop {
    fn legalize(&mut self, _b: &mut LegalizeBuilder) {
        // Nothing to do
    }

    fn encode(&self, e: &mut SM70Encoder<'_>) {
        e.set_opcode(0x918);
    }
}

impl SM70Op for OpPixLd {
    fn legalize(&mut self, _b: &mut LegalizeBuilder) {
        // Nothing to do
    }

    fn encode(&self, e: &mut SM70Encoder<'_>) {
        e.set_opcode(0x925);
        e.set_dst(&self.dst);
        e.set_field(
            78..81,
            match &self.val {
                PixVal::MsCount => 0_u8,
                PixVal::CovMask => 1_u8,
                PixVal::CentroidOffset => 2_u8,
                PixVal::MyIndex => 3_u8,
                PixVal::InnerCoverage => 4_u8,
                other => panic!("Unsupported PixVal: {other}"),
            },
        );
        e.set_pred_dst(81..84, &Dst::None);
    }
}

impl SM70Op for OpS2R {
    fn legalize(&mut self, _b: &mut LegalizeBuilder) {
        // Nothing to do
    }

    fn encode(&self, e: &mut SM70Encoder<'_>) {
        if self.is_uniform() {
            e.set_opcode(0x9c3);
            e.set_udst(&self.dst);
        } else {
            e.set_opcode(0x919);
            e.set_dst(&self.dst);
        }
        e.set_field(72..80, self.idx);
    }
}

impl SM70Op for OpOut {
    fn legalize(&mut self, b: &mut LegalizeBuilder) {
        let gpr = op_gpr(self);
        b.copy_alu_src_if_not_reg(&mut self.handle, gpr, SrcType::GPR);
        b.copy_alu_src_if_not_reg_or_imm(&mut self.stream, gpr, SrcType::ALU);
    }

    fn encode(&self, e: &mut SM70Encoder<'_>) {
        e.encode_alu(
            0x124,
            Some(&self.dst),
            Some(&self.handle),
            Some(&self.stream),
            None,
        );

        e.set_field(
            78..80,
            match self.out_type {
                OutType::Emit => 1_u8,
                OutType::Cut => 2_u8,
                OutType::EmitThenCut => 3_u8,
            },
        );
    }
}

impl SM70Op for OpOutFinal {
    fn legalize(&mut self, b: &mut LegalizeBuilder) {
        let gpr = op_gpr(self);
        b.copy_alu_src_if_not_reg(&mut self.handle, gpr, SrcType::GPR);
    }

    fn encode(&self, e: &mut SM70Encoder<'_>) {
        e.encode_alu(
            0x124,
            Some(&Dst::None),
            Some(&self.handle),
            Some(&Src::ZERO),
            None,
        );
    }
}

impl SM70Op for OpVote {
    fn legalize(&mut self, b: &mut LegalizeBuilder) {
        b.copy_src_if_upred(&mut self.pred);
    }

    fn encode(&self, e: &mut SM70Encoder<'_>) {
        if self.is_uniform() {
            e.set_opcode(0x886);
            e.set_udst(&self.ballot);
        } else {
            e.set_opcode(0x806);
            e.set_dst(&self.ballot);
        }

        e.set_field(
            72..74,
            match self.op {
                VoteOp::All => 0_u8,
                VoteOp::Any => 1_u8,
                VoteOp::Eq => 2_u8,
            },
        );

        e.set_pred_dst(81..84, &self.vote);
        e.set_pred_src(87..90, 90, &self.pred);
    }
}

impl SM70Op for OpMatch {
    fn legalize(&mut self, b: &mut LegalizeBuilder) {
        legalize_ext_instr(self, b);
    }

    fn encode(&self, e: &mut SM70Encoder<'_>) {
        e.set_opcode(0x3a1);

        e.set_dst(&self.mask);
        e.set_reg_src(24..32, &self.src);
        e.set_bit(73, self.u64);

        e.set_bit(
            79,
            match self.op {
                MatchOp::Any => {
                    assert!(matches!(self.pred, Dst::None));
                    true
                }
                MatchOp::All => false,
            },
        );

        e.set_pred_dst(81..84, &self.pred);
    }
}

impl SM70Op for OpImma {
    fn legalize(&mut self, b: &mut LegalizeBuilder) {
        legalize_ext_instr(self, b);
    }

    fn encode(&self, e: &mut SM70Encoder<'_>) {
        assert!(e.sm >= 75);

        e.set_opcode(0x237);
        e.set_dst(&self.dst);
        e.set_reg_src(24..32, &self.srcs[0]);
        e.set_reg_src(32..40, &self.srcs[1]);
        e.set_reg_src(64..72, &self.srcs[2]);
        e.set_bit(74, true); // SRC1.COL

        if e.sm >= 90 {
            e.set_rev_upred_src(87..90, 90, &true.into());
        }

        assert!(self.mat_size == ImmaSize::M8N8K16 || e.sm >= 80);
        e.set_field2(
            75..76,
            85..87,
            match self.mat_size {
                ImmaSize::M8N8K16 => 0u8,
                ImmaSize::M8N8K32 => 2u8,
                ImmaSize::M16N8K16 => 4u8,
                ImmaSize::M16N8K32 => 5u8,
                ImmaSize::M16N8K64 => 6u8,
            },
        );

        e.set_bit(76, self.src_types[0].is_signed());
        e.set_bit(78, self.src_types[1].is_signed());
        e.set_bit(82, self.saturate);

        match self.mat_size {
            ImmaSize::M8N8K32 | ImmaSize::M16N8K64 => {
                assert_eq!(self.src_types[0].bits(), 4);
                assert_eq!(self.src_types[1].bits(), 4);
            }
            ImmaSize::M16N8K32 => {
                assert!(matches!(self.src_types[0].bits(), 4 | 8));
                assert!(matches!(self.src_types[1].bits(), 4 | 8));
            }
            ImmaSize::M8N8K16 | ImmaSize::M16N8K16 => {
                assert_eq!(self.src_types[0].bits(), 8);
                assert_eq!(self.src_types[1].bits(), 8);
            }
        }
        e.set_bit(83, self.src_types[0].bits() == 4);
        e.set_bit(84, self.src_types[1].bits() == 4);
    }
}

impl SM70Op for OpHmma {
    fn legalize(&mut self, b: &mut LegalizeBuilder) {
        legalize_ext_instr(self, b);
    }

    fn encode(&self, e: &mut SM70Encoder<'_>) {
        assert!(e.sm >= 75);

        e.set_opcode(0x23c);
        e.set_dst(&self.dst);
        e.set_reg_src(24..32, &self.srcs[0]);
        e.set_reg_src(32..40, &self.srcs[1]);
        e.set_reg_src(64..72, &self.srcs[2]);

        if e.sm >= 90 {
            e.set_rev_upred_src(87..90, 90, &true.into());
        }

        assert!(self.mat_size != HmmaSize::M16N8K4 || e.sm >= 80);
        e.set_field2(
            75..76,
            78..79,
            match self.mat_size {
                HmmaSize::M16N8K8 => 0u8,
                HmmaSize::M16N8K16 => 1u8,
                HmmaSize::M16N8K4 => 2u8,
            },
        );

        assert!(matches!(self.dst_type, FloatType::F16 | FloatType::F32));
        e.set_bit(76, self.dst_type == FloatType::F32);
        e.set_field(
            82..84,
            match self.src_type {
                FloatType::F16 => 0u8,
                // FloatType::BF16 => 1u8,
                // FloatType::TF32 => 2u8,
                _ => unreachable!("unsupported src type!"),
            },
        );
    }
}

impl SM70Op for OpLdsm {
    fn legalize(&mut self, b: &mut LegalizeBuilder) {
        legalize_ext_instr(self, b);
    }

    fn encode(&self, e: &mut SM70Encoder<'_>) {
        assert!(e.sm >= 75);

        e.set_opcode(0x83b);
        e.set_dst(&self.dst);
        e.set_reg_src(24..32, &self.addr);
        e.set_field(40..64, self.offset);
        e.set_field(
            72..74,
            match self.mat_count {
                1 => 0u8,
                2 => 1u8,
                4 => 2u8,
                _ => panic!("Invalid LDSM mat count"),
            },
        );
        e.set_field(
            78..80,
            match self.mat_size {
                LdsmSize::M8N8 => 0u8,
                LdsmSize::MT8N8 => 1u8,
                // Those do value expansion and are weird, we'll probably never use them.
                // LdsmSize::M8N16 => 2,
                // LdsmSize::M8N32 => 3,
            },
        );
    }
}

impl SM70Op for OpMovm {
    fn legalize(&mut self, b: &mut LegalizeBuilder) {
        legalize_ext_instr(self, b);
    }

    fn encode(&self, e: &mut SM70Encoder<'_>) {
        assert!(e.sm >= 75);

        e.set_opcode(0x23a);
        e.set_dst(&self.dst);
        e.set_reg_src(24..32, &self.src);
        // TODO: 1: M832, 2: M864
        e.set_field(78..80, 0); // MT88
    }
}

macro_rules! sm70_op_match {
    ($op: expr, |$x: ident| $y: expr) => {
        match $op {
            Op::FAdd($x) => $y,
            Op::FFma($x) => $y,
            Op::FMnMx($x) => $y,
            Op::FMul($x) => $y,
            Op::FSet($x) => $y,
            Op::FSetP($x) => $y,
            Op::FSwzAdd($x) => $y,
            Op::DAdd($x) => $y,
            Op::DFma($x) => $y,
            Op::DMul($x) => $y,
            Op::DSetP($x) => $y,
            Op::HAdd2($x) => $y,
            Op::HFma2($x) => $y,
            Op::HMul2($x) => $y,
            Op::HSet2($x) => $y,
            Op::HSetP2($x) => $y,
            Op::HMnMx2($x) => $y,
            Op::Transcendental($x) => $y,
            Op::BMsk($x) => $y,
            Op::BRev($x) => $y,
            Op::Flo($x) => $y,
            Op::IAbs($x) => $y,
            Op::IAdd3($x) => $y,
            Op::IAdd3X($x) => $y,
            Op::IDp4($x) => $y,
            Op::IMad($x) => $y,
            Op::IMad64($x) => $y,
            Op::IMnMx($x) => $y,
            Op::ISetP($x) => $y,
            Op::Lea($x) => $y,
            Op::LeaX($x) => $y,
            Op::Lop3($x) => $y,
            Op::PopC($x) => $y,
            Op::Shf($x) => $y,
            Op::F2F($x) => $y,
            Op::F2FP($x) => $y,
            Op::F2I($x) => $y,
            Op::I2F($x) => $y,
            Op::FRnd($x) => $y,
            Op::Mov($x) => $y,
            Op::Movm($x) => $y,
            Op::Prmt($x) => $y,
            Op::Sel($x) => $y,
            Op::Sgxt($x) => $y,
            Op::Shfl($x) => $y,
            Op::PLop3($x) => $y,
            Op::R2UR($x) => $y,
            Op::Redux($x) => $y,
            Op::Tex($x) => $y,
            Op::Tld($x) => $y,
            Op::Tld4($x) => $y,
            Op::Tmml($x) => $y,
            Op::Txd($x) => $y,
            Op::Txq($x) => $y,
            Op::SuLd($x) => $y,
            Op::SuSt($x) => $y,
            Op::SuAtom($x) => $y,
            Op::Ld($x) => $y,
            Op::Ldc($x) => $y,
            Op::St($x) => $y,
            Op::Atom($x) => $y,
            Op::AL2P($x) => $y,
            Op::ALd($x) => $y,
            Op::ASt($x) => $y,
            Op::Ipa($x) => $y,
            Op::LdTram($x) => $y,
            Op::CCtl($x) => $y,
            Op::MemBar($x) => $y,
            Op::BClear($x) => $y,
            Op::BMov($x) => $y,
            Op::Break($x) => $y,
            Op::BSSy($x) => $y,
            Op::BSync($x) => $y,
            Op::Bra($x) => $y,
            Op::Exit($x) => $y,
            Op::WarpSync($x) => $y,
            Op::Bar($x) => $y,
            Op::CS2R($x) => $y,
            Op::Isberd($x) => $y,
            Op::Kill($x) => $y,
            Op::Nop($x) => $y,
            Op::PixLd($x) => $y,
            Op::S2R($x) => $y,
            Op::Out($x) => $y,
            Op::OutFinal($x) => $y,
            Op::Vote($x) => $y,
            Op::Match($x) => $y,
            Op::Hmma($x) => $y,
            Op::Imma($x) => $y,
            Op::Ldsm($x) => $y,
            _ => panic!("Unsupported op: {}", $op),
        }
    };
}

impl SM70Op for Op {
    fn legalize(&mut self, b: &mut LegalizeBuilder) {
        sm70_op_match!(self, |op| op.legalize(b));
    }
    fn encode(&self, e: &mut SM70Encoder<'_>) {
        sm70_op_match!(self, |op| op.encode(e));
    }
}
