// SPDX-License-Identifier: AGPL-3.0-or-later
use super::*;

impl SM20Op for OpF2F {
    fn legalize(&mut self, b: &mut LegalizeBuilder) {
        use RegFile::GPR;
        b.copy_alu_src_if_f20_overflow(&mut self.src, GPR, SrcType::ALU);
    }

    fn encode(&self, e: &mut SM20Encoder<'_>) {
        e.encode_form_b(SM20Unit::Move, 0x4, &self.dst, &self.src);
        e.set_bit(5, false);
        e.set_bit(6, self.src.modifier.has_fabs());
        e.set_bit(7, self.integer_rnd);
        e.set_bit(8, self.src.modifier.has_fneg());
        e.set_field(20..22, (self.dst_type.bits() / 8).ilog2());
        e.set_field(23..25, (self.src_type.bits() / 8).ilog2());
        e.set_rnd_mode(49..51, self.rnd_mode);
        e.set_bit(55, self.ftz);
        e.set_bit(56, self.is_high());
    }
}

impl SM20Op for OpF2I {
    fn legalize(&mut self, b: &mut LegalizeBuilder) {
        use RegFile::GPR;
        b.copy_alu_src_if_f20_overflow(&mut self.src, GPR, SrcType::ALU);
    }

    fn encode(&self, e: &mut SM20Encoder<'_>) {
        e.encode_form_b(SM20Unit::Move, 0x5, &self.dst, &self.src);
        e.set_bit(6, self.src.modifier.has_fabs());
        e.set_bit(7, self.dst_type.is_signed());
        e.set_bit(8, self.src.modifier.has_fneg());
        e.set_field(20..22, (self.dst_type.bits() / 8).ilog2());
        e.set_field(23..25, (self.src_type.bits() / 8).ilog2());
        e.set_rnd_mode(49..51, self.rnd_mode);
        e.set_bit(55, self.ftz);
        e.set_bit(56, false);
    }
}

impl SM20Op for OpI2F {
    fn legalize(&mut self, b: &mut LegalizeBuilder) {
        use RegFile::GPR;
        b.copy_alu_src_if_i20_overflow(&mut self.src, GPR, SrcType::ALU);
    }

    fn encode(&self, e: &mut SM20Encoder<'_>) {
        assert!(self.src.is_unmodified());
        e.encode_form_b(SM20Unit::Move, 0x6, &self.dst, &self.src);
        e.set_bit(6, false);
        e.set_bit(8, false);
        e.set_bit(9, self.src_type.is_signed());
        e.set_field(20..22, (self.dst_type.bits() / 8).ilog2());
        e.set_field(23..25, (self.src_type.bits() / 8).ilog2());
        e.set_rnd_mode(49..51, self.rnd_mode);
        e.set_field(55..57, 0_u8);
    }
}

impl SM20Op for OpI2I {
    fn legalize(&mut self, b: &mut LegalizeBuilder) {
        use RegFile::GPR;
        b.copy_alu_src_if_i20_overflow(&mut self.src, GPR, SrcType::ALU);
    }

    fn encode(&self, e: &mut SM20Encoder<'_>) {
        assert!(self.src.is_unmodified());
        e.encode_form_b(SM20Unit::Move, 0x7, &self.dst, &self.src);
        e.set_bit(5, self.saturate);
        e.set_bit(6, self.abs);
        e.set_bit(7, self.dst_type.is_signed());
        e.set_bit(8, self.neg);
        e.set_bit(9, self.src_type.is_signed());
        e.set_field(20..22, (self.dst_type.bits() / 8).ilog2());
        e.set_field(23..25, (self.src_type.bits() / 8).ilog2());
        e.set_field(55..57, 0_u8);
    }
}
