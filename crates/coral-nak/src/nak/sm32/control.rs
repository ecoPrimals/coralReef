// Copyright © 2025 Collabora, Ltd.
// SPDX-License-Identifier: MIT
//! SM32 control flow instruction encoders.

#![allow(clippy::wildcard_imports, clippy::enum_glob_use)]

use super::super::ir::RegFile;
use super::encoder::*;
use super::mem::legalize_ext_instr;

impl SM32Op for OpCCtl {
    fn legalize(&mut self, b: &mut LegalizeBuilder) {
        legalize_ext_instr(self, b);
    }

    fn encode(&self, e: &mut SM32Encoder<'_>) {
        match self.mem_space {
            MemSpace::Global(addr_type) => {
                e.set_opcode(0x7b0, 2);

                assert!(self.addr_offset % 4 == 0);
                e.set_field(25..55, self.addr_offset / 4);
                e.set_field(
                    55..56,
                    match addr_type {
                        MemAddrType::A32 => 0_u8,
                        MemAddrType::A64 => 1_u8,
                    },
                );
            }
            MemSpace::Local => panic!("cctl does not support local"),
            MemSpace::Shared => {
                e.set_opcode(0x7c0, 2);

                assert!(self.addr_offset % 4 == 0);
                e.set_field(25..47, self.addr_offset / 4);
            }
        }
        e.set_field(
            2..6,
            match self.op {
                CCtlOp::Qry1 => 0_u8,
                CCtlOp::PF1 => 1_u8,
                CCtlOp::PF1_5 => 2_u8,
                CCtlOp::PF2 => 3_u8,
                CCtlOp::WB => 4_u8,
                CCtlOp::IV => 5_u8,
                CCtlOp::IVAll => 6_u8,
                CCtlOp::RS => 7_u8,
                CCtlOp::RSLB => 7_u8,
                op => panic!("Unsupported cache control {op:?}"),
            },
        );
        e.set_reg_src(10..18, &self.addr);
    }
}

impl SM32Op for OpMemBar {
    fn legalize(&mut self, _b: &mut LegalizeBuilder) {
        // Nothing to do
    }

    fn encode(&self, e: &mut SM32Encoder<'_>) {
        e.set_opcode(0x7cc, 2);

        e.set_field(
            10..12,
            match self.scope {
                MemScope::CTA => 0_u8,
                MemScope::GPU => 1_u8,
                MemScope::System => 2_u8,
            },
        );
    }
}

impl SM32Encoder<'_> {
    pub(super) fn set_rel_offset(&mut self, range: Range<usize>, label: &Label) {
        assert!(range.len() == 24);
        assert!(self.ip % 8 == 0);

        let ip = u32::try_from(self.ip).unwrap();
        let ip = i32::try_from(ip).unwrap();

        let target_ip = *self.labels.get(label).unwrap();
        let target_ip = u32::try_from(target_ip).unwrap();
        let target_ip = i32::try_from(target_ip).unwrap();
        assert!(target_ip % 8 == 0);

        let rel_offset = target_ip - ip - 8;

        assert!(rel_offset % 8 == 0);
        self.set_field(range, rel_offset);
    }
}

impl SM32Op for OpBra {
    fn legalize(&mut self, _b: &mut LegalizeBuilder) {
        // Nothing to do
    }

    fn encode(&self, e: &mut SM32Encoder<'_>) {
        e.set_opcode(0x120, 0);
        e.set_field(2..6, 0xf_u8); // Condition code
        // 7: bra to cbuf
        // 8: .lmt (limit)
        // 9: .u (uniform for warp)
        e.set_rel_offset(23..47, &self.target);
    }
}

impl SM32Op for OpSSy {
    fn legalize(&mut self, _b: &mut LegalizeBuilder) {
        // Nothing to do
    }

    fn encode(&self, e: &mut SM32Encoder<'_>) {
        e.set_opcode(0x148, 0);
        e.set_field(2..8, 0xf_u8); // flags
        e.set_rel_offset(23..47, &self.target);
    }
}

impl SM32Op for OpSync {
    fn legalize(&mut self, _b: &mut LegalizeBuilder) {
        // Nothing to do
    }

    fn encode(&self, e: &mut SM32Encoder<'_>) {
        // emit nop.s
        e.set_opcode(0x858, 2);
        e.set_field(10..14, 0xf_u8); // flags

        // sync bit (.s)
        // Kepler doesn't really have a "sync" instruction, instead
        // every instruction can become a sync if the bit 22 is enabled.
        // TODO: instead of adding a nop.s add the .s modifier to the
        //       next instruction (and handle addresses accordingly)
        e.set_bit(22, true); // .s
    }
}

impl SM32Op for OpBrk {
    fn legalize(&mut self, _b: &mut LegalizeBuilder) {
        // Nothing to do
    }

    fn encode(&self, e: &mut SM32Encoder<'_>) {
        e.set_opcode(0x1a0, 0);
        e.set_field(2..8, 0xf_u8); // flags
    }
}

impl SM32Op for OpPBk {
    fn legalize(&mut self, _b: &mut LegalizeBuilder) {
        // Nothing to do
    }

    fn encode(&self, e: &mut SM32Encoder<'_>) {
        e.set_opcode(0x150, 0);
        e.set_field(2..8, 0xf_u8); // flags
        e.set_rel_offset(23..47, &self.target);
    }
}

impl SM32Op for OpCont {
    fn legalize(&mut self, _b: &mut LegalizeBuilder) {
        // Nothing to do
    }

    fn encode(&self, e: &mut SM32Encoder<'_>) {
        e.set_opcode(0x1a8, 0);
        e.set_field(2..8, 0xf_u8); // flags
    }
}

impl SM32Op for OpPCnt {
    fn legalize(&mut self, _b: &mut LegalizeBuilder) {
        // Nothing to do
    }

    fn encode(&self, e: &mut SM32Encoder<'_>) {
        e.set_opcode(0x158, 0);
        e.set_field(2..8, 0xf_u8); // flags
        e.set_rel_offset(23..47, &self.target);
    }
}

impl SM32Op for OpExit {
    fn legalize(&mut self, _b: &mut LegalizeBuilder) {
        // Nothing to do
    }

    fn encode(&self, e: &mut SM32Encoder<'_>) {
        e.set_opcode(0x180, 0);
        e.set_field(2..8, 0xf_u8); // flags
    }
}

impl SM32Op for OpBar {
    fn legalize(&mut self, _b: &mut LegalizeBuilder) {
        // Nothing to do
    }

    fn encode(&self, e: &mut SM32Encoder<'_>) {
        e.set_opcode(0x854, 2);

        // Barrier id
        e.set_reg_src_ref(10..18, &SrcRef::Zero);
        // Thread count
        e.set_reg_src_ref(23..31, &SrcRef::Zero);

        // 00: SYNC
        // 01: ARV
        // 02: RED
        // 03: SCAN
        // 04: SYNCALL
        e.set_field(35..38, 0);

        // (only for RED)
        // 00: .POPC
        // 01: .AND
        // 02: .OR
        e.set_field(38..40, 0);

        // actually only useful for reductions.
        e.set_pred_src(42..46, &SrcRef::True.into());

        // 46: 1 if barr_id is immediate (imm: 10..18, max: 0xff)
        // 47: 1 if thread_count is immediate (imm: 23..35, max: 0xfff)
    }
}

impl SM32Op for OpTexDepBar {
    fn legalize(&mut self, _b: &mut LegalizeBuilder) {
        // Nothing to do
    }

    fn encode(&self, e: &mut SM32Encoder<'_>) {
        e.set_opcode(0x770, 2);
        // Max N of textures in queue
        e.set_field(23..29, self.textures_left);
    }
}

impl SM32Op for OpViLd {
    fn legalize(&mut self, _b: &mut LegalizeBuilder) {
        // Nothing to do
    }

    fn encode(&self, e: &mut SM32Encoder<'_>) {
        e.set_opcode(0x7f8, 2);
        e.set_dst(&self.dst);
        e.set_reg_src(10..18, &self.idx);
        e.set_field(23..31, self.off);
    }
}

impl SM32Op for OpKill {
    fn legalize(&mut self, _b: &mut LegalizeBuilder) {
        // Nothing to do
    }

    fn encode(&self, e: &mut SM32Encoder<'_>) {
        e.set_opcode(0x198, 0);
        e.set_field(2..8, 0xf_u8); // flags
    }
}

impl SM32Op for OpNop {
    fn legalize(&mut self, _b: &mut LegalizeBuilder) {
        // Nothing to do
    }

    fn encode(&self, e: &mut SM32Encoder<'_>) {
        e.set_opcode(0x858, 2);
        e.set_field(10..14, 0xf_u8); // flags
    }
}

impl SM32Op for OpPixLd {
    fn legalize(&mut self, b: &mut LegalizeBuilder) {
        legalize_ext_instr(self, b);
    }

    fn encode(&self, e: &mut SM32Encoder<'_>) {
        e.set_opcode(0x7f4, 2);
        e.set_dst(&self.dst);

        e.set_reg_src(10..18, &0.into()); // addr
        e.set_field(23..31, 0_u16); // offset

        e.set_field(
            34..37,
            match &self.val {
                PixVal::MsCount => 0_u8,
                PixVal::CovMask => 1_u8,
                PixVal::Covered => 2_u8,
                PixVal::Offset => 3_u8,
                PixVal::CentroidOffset => 4_u8,
                PixVal::MyIndex => 5_u8,
                PixVal::InnerCoverage => panic!("Unsupported PixVal: InnerCoverage"),
            },
        );

        e.set_pred_dst(48..51, &Dst::None); // dst1
    }
}

impl SM32Op for OpS2R {
    fn legalize(&mut self, _b: &mut LegalizeBuilder) {
        // Nothing to do
    }

    fn encode(&self, e: &mut SM32Encoder<'_>) {
        e.set_opcode(0x864, 2);
        e.set_dst(&self.dst);
        e.set_field(23..31, self.idx);
    }
}

impl SM32Op for OpVote {
    fn legalize(&mut self, _b: &mut LegalizeBuilder) {
        // Nothing to do
    }

    fn encode(&self, e: &mut SM32Encoder<'_>) {
        e.set_opcode(0x86c, 2);

        e.set_dst(&self.ballot);
        e.set_pred_dst(48..51, &self.vote);

        e.set_pred_src(42..46, &self.pred);

        e.set_field(
            51..53,
            match self.op {
                VoteOp::All => 0u8,
                VoteOp::Any => 1u8,
                VoteOp::Eq => 2u8,
            },
        );
    }
}

impl SM32Op for OpOut {
    fn legalize(&mut self, b: &mut LegalizeBuilder) {
        use RegFile::GPR;
        b.copy_alu_src_if_not_reg(&mut self.handle, GPR, SrcType::GPR);
        b.copy_alu_src_if_i20_overflow(&mut self.stream, GPR, SrcType::ALU);
    }

    fn encode(&self, e: &mut SM32Encoder<'_>) {
        e.encode_form_immreg(
            0xb70,
            0x1f0,
            Some(&self.dst),
            &self.handle,
            &self.stream,
            None,
            false,
        );

        e.set_field(
            42..44,
            match self.out_type {
                OutType::Emit => 1_u8,
                OutType::Cut => 2_u8,
                OutType::EmitThenCut => 3_u8,
            },
        );
    }
}

// Instructions left behind from codegen rewrite,
// we might use them in the future:
// - 0x138 pret.noinc
// - 0x1b8 quadon (enable all threads in quad)
// - 0x1c0 quadpop (redisable them)
// - 0x190 ret
