// SPDX-License-Identifier: AGPL-3.0-only
// Copyright © 2025-2026 ecoPrimals
// Derived from Collabora, Ltd. (2023)

use super::*;

impl SM50Op for OpF2F {
    fn legalize(&mut self, b: &mut LegalizeBuilder) {
        use RegFile::GPR;
        b.copy_alu_src_if_f20_overflow(&mut self.src, GPR, SrcType::ALU);
    }

    fn encode(&self, e: &mut SM50Encoder<'_>) {
        // The swizzle is handled by the .high bit below.
        let src = self.src.clone().without_swizzle();
        match &src.reference {
            SrcRef::Zero | SrcRef::Reg(_) => {
                e.set_opcode(0x5ca8);
                e.set_reg_fmod_src(20..28, 49, 45, &src);
            }
            SrcRef::Imm32(imm32) => {
                e.set_opcode(0x38a8);
                e.set_src_imm_i20(20..39, 56, *imm32);
                assert!(src.is_unmodified());
            }
            SrcRef::CBuf(_) => {
                e.set_opcode(0x4ca8);
                e.set_cb_fmod_src(20..39, 49, 45, &src);
            }
            src => panic!("Invalid f2f src: {src}"),
        }

        // We can't span 32 bits
        assert!(
            (self.dst_type.bits() <= 32 && self.src_type.bits() <= 32)
                || (self.dst_type.bits() >= 32 && self.src_type.bits() >= 32)
        );
        e.set_field(8..10, (self.dst_type.bits() / 8).ilog2());
        e.set_field(10..12, (self.src_type.bits() / 8).ilog2());

        e.set_rnd_mode(39..41, self.rnd_mode);
        e.set_bit(41, self.is_high());
        e.set_bit(42, self.integer_rnd);
        e.set_bit(44, self.ftz);
        e.set_bit(50, false); // saturate

        e.set_dst(&self.dst);
    }
}

impl SM50Op for OpF2I {
    fn legalize(&mut self, b: &mut LegalizeBuilder) {
        use RegFile::GPR;
        b.copy_alu_src_if_f20_overflow(&mut self.src, GPR, SrcType::ALU);
    }

    fn encode(&self, e: &mut SM50Encoder<'_>) {
        match &self.src.reference {
            SrcRef::Zero | SrcRef::Reg(_) => {
                e.set_opcode(0x5cb0);
                e.set_reg_fmod_src(20..28, 49, 45, &self.src);
            }
            SrcRef::Imm32(imm32) => {
                e.set_opcode(0x38b0);
                e.set_src_imm_f20(20..39, 56, *imm32);
                assert!(self.src.is_unmodified());
            }
            SrcRef::CBuf(_) => {
                e.set_opcode(0x4cb0);
                e.set_cb_fmod_src(20..39, 49, 45, &self.src);
            }
            src => panic!("Invalid f2i src: {src}"),
        }

        e.set_dst(&self.dst);

        // We can't span 32 bits
        assert!(
            (self.dst_type.bits() <= 32 && self.src_type.bits() <= 32)
                || (self.dst_type.bits() >= 32 && self.src_type.bits() >= 32)
        );
        e.set_field(8..10, (self.dst_type.bits() / 8).ilog2());
        e.set_field(10..12, (self.src_type.bits() / 8).ilog2());
        e.set_bit(12, self.dst_type.is_signed());

        e.set_rnd_mode(39..41, self.rnd_mode);
        e.set_bit(44, self.ftz);
        e.set_bit(47, false); // .CC
    }
}

impl SM50Op for OpI2F {
    fn legalize(&mut self, b: &mut LegalizeBuilder) {
        use RegFile::GPR;
        b.copy_alu_src_if_i20_overflow(&mut self.src, GPR, SrcType::ALU);
    }

    fn encode(&self, e: &mut SM50Encoder<'_>) {
        match &self.src.reference {
            SrcRef::Zero | SrcRef::Reg(_) => {
                e.set_opcode(0x5cb8);
                e.set_reg_ineg_src(20..28, 45, &self.src);
            }
            SrcRef::Imm32(imm32) => {
                e.set_opcode(0x38b8);
                e.set_src_imm_i20(20..39, 56, *imm32);
                assert!(self.src.is_unmodified());
            }
            SrcRef::CBuf(_) => {
                e.set_opcode(0x4cb8);
                e.set_cb_ineg_src(20..39, 45, &self.src);
            }
            src => panic!("Invalid i2f src: {src}"),
        }

        e.set_dst(&self.dst);

        // We can't span 32 bits
        assert!(
            (self.dst_type.bits() <= 32 && self.src_type.bits() <= 32)
                || (self.dst_type.bits() >= 32 && self.src_type.bits() >= 32)
        );
        e.set_field(8..10, (self.dst_type.bits() / 8).ilog2());
        e.set_field(10..12, (self.src_type.bits() / 8).ilog2());
        e.set_bit(13, self.src_type.is_signed());

        e.set_rnd_mode(39..41, self.rnd_mode);
        e.set_field(41..43, 0_u8); // DEBT(isa): subop
        e.set_bit(49, false); // iabs
    }
}

impl SM50Op for OpI2I {
    fn legalize(&mut self, b: &mut LegalizeBuilder) {
        use RegFile::GPR;
        b.copy_alu_src_if_i20_overflow(&mut self.src, GPR, SrcType::ALU);
    }

    fn encode(&self, e: &mut SM50Encoder<'_>) {
        match &self.src.reference {
            SrcRef::Zero | SrcRef::Reg(_) => {
                e.set_opcode(0x5ce0);
                e.set_reg_src(20..28, &self.src);
            }
            SrcRef::Imm32(imm32) => {
                e.set_opcode(0x38e0);
                e.set_src_imm_i20(20..39, 56, *imm32);
            }
            SrcRef::CBuf(cbuf) => {
                e.set_opcode(0x4ce0);
                e.set_src_cb(20..39, cbuf);
            }
            src => panic!("Invalid i2i src: {src}"),
        }

        e.set_dst(&self.dst);

        // We can't span 32 bits
        assert!(
            (self.dst_type.bits() <= 32 && self.src_type.bits() <= 32)
                || (self.dst_type.bits() >= 32 && self.src_type.bits() >= 32)
        );
        e.set_field(8..10, (self.dst_type.bits() / 8).ilog2());
        e.set_field(10..12, (self.src_type.bits() / 8).ilog2());
        e.set_bit(12, self.dst_type.is_signed());
        e.set_bit(13, self.src_type.is_signed());

        e.set_field(41..43, 0u8); // src.B1-3
        e.set_bit(45, self.neg);
        e.set_bit(47, false); // dst.CC
        e.set_bit(49, self.abs);
        e.set_bit(50, self.saturate);
    }
}
