use super::*;

impl SM32Op for OpF2F {
    fn legalize(&mut self, b: &mut LegalizeBuilder) {
        use RegFile::GPR;
        let src = &mut self.src;
        // No immediates supported
        b.copy_alu_src_if_imm(src, GPR, SrcType::ALU);
    }

    fn encode(&self, e: &mut SM32Encoder<'_>) {
        // integer_rnd on SM32 is inferred automatically when
        // the src_type and dst_type are the same.
        assert!(!self.integer_rnd || (self.src_type == self.dst_type));

        e.set_dst(&self.dst);

        // The swizzle is handled by the .high bit below.
        let src = self.src.clone().without_swizzle();
        match &src.src_ref {
            SrcRef::Zero | SrcRef::Reg(_) => {
                e.set_opcode(0xe54, 2);
                e.set_reg_src(23..31, &src);
            }
            SrcRef::CBuf(cb) => {
                e.set_opcode(0x654, 2);
                e.set_src_cbuf(23..42, cb);
            }
            src => panic!("Invalid f2f src: {src}"),
        }

        // We can't span 32 bits
        assert!(
            (self.dst_type.bits() <= 32 && self.src_type.bits() <= 32)
                || (self.dst_type.bits() >= 32 && self.src_type.bits() >= 32)
        );
        e.set_field(10..12, (self.dst_type.bits() / 8).ilog2());
        e.set_field(12..14, (self.src_type.bits() / 8).ilog2());

        e.set_rnd_mode(42..44, self.rnd_mode);
        e.set_bit(44, self.is_high());
        e.set_bit(45, self.integer_rnd);
        e.set_bit(47, self.ftz);
        e.set_bit(48, src.src_mod.has_fneg());
        e.set_bit(50, false); // dst.CC
        e.set_bit(52, src.src_mod.has_fabs());
        e.set_bit(53, false); // saturate
    }
}

impl SM32Op for OpF2I {
    fn legalize(&mut self, b: &mut LegalizeBuilder) {
        use RegFile::GPR;
        let src = &mut self.src;
        // No immediates supported
        b.copy_alu_src_if_imm(src, GPR, SrcType::ALU);
    }

    fn encode(&self, e: &mut SM32Encoder<'_>) {
        e.set_dst(&self.dst);

        match &self.src.src_ref {
            SrcRef::Zero | SrcRef::Reg(_) => {
                e.set_opcode(0xe58, 2);
                e.set_reg_src(23..31, &self.src);
            }
            SrcRef::CBuf(cb) => {
                e.set_opcode(0x658, 2);
                e.set_src_cbuf(23..42, cb);
            }
            src => panic!("Invalid f2i src: {src}"),
        }

        // We can't span 32 bits
        assert!(
            (self.dst_type.bits() <= 32 && self.src_type.bits() <= 32)
                || (self.dst_type.bits() >= 32 && self.src_type.bits() >= 32)
        );
        e.set_field(10..12, (self.dst_type.bits() / 8).ilog2());
        e.set_field(12..14, (self.src_type.bits() / 8).ilog2());
        e.set_bit(14, self.dst_type.is_signed());

        e.set_rnd_mode(42..44, self.rnd_mode);
        // 44: .h1
        e.set_bit(47, self.ftz);
        e.set_bit(48, self.src.src_mod.has_fneg());
        e.set_bit(50, false); // dst.CC
        e.set_bit(52, self.src.src_mod.has_fabs());
        e.set_bit(53, false); // saturate
    }
}

impl SM32Op for OpI2F {
    fn legalize(&mut self, b: &mut LegalizeBuilder) {
        use RegFile::GPR;
        let src = &mut self.src;
        // No immediates supported
        b.copy_alu_src_if_imm(src, GPR, SrcType::ALU);
    }

    fn encode(&self, e: &mut SM32Encoder<'_>) {
        e.set_dst(&self.dst);

        match &self.src.src_ref {
            SrcRef::Zero | SrcRef::Reg(_) => {
                e.set_opcode(0xe5c, 2);
                e.set_reg_src(23..31, &self.src);
            }
            SrcRef::CBuf(cb) => {
                e.set_opcode(0x65c, 2);
                e.set_src_cbuf(23..42, cb);
            }
            src => panic!("Invalid i2f src: {src}"),
        }

        // We can't span 32 bits
        assert!(
            (self.dst_type.bits() <= 32 && self.src_type.bits() <= 32)
                || (self.dst_type.bits() >= 32 && self.src_type.bits() >= 32)
        );
        e.set_field(10..12, (self.dst_type.bits() / 8).ilog2());
        e.set_field(12..14, (self.src_type.bits() / 8).ilog2());
        e.set_bit(15, self.src_type.is_signed());

        e.set_rnd_mode(42..44, self.rnd_mode);
        e.set_field(44..46, 0); // .b0-3
        e.set_bit(48, self.src.src_mod.is_ineg());
        e.set_bit(50, false); // dst.CC
        e.set_bit(52, false); // iabs
    }
}

impl SM32Op for OpI2I {
    fn legalize(&mut self, b: &mut LegalizeBuilder) {
        use RegFile::GPR;
        let src = &mut self.src;
        // No immediates supported
        b.copy_alu_src_if_imm(src, GPR, SrcType::ALU);
    }

    fn encode(&self, e: &mut SM32Encoder<'_>) {
        e.set_dst(&self.dst);

        match &self.src.src_ref {
            SrcRef::Zero | SrcRef::Reg(_) => {
                e.set_opcode(0xe60, 2);
                e.set_reg_src(23..31, &self.src);
            }
            SrcRef::CBuf(cb) => {
                e.set_opcode(0x660, 2);
                e.set_src_cbuf(23..42, cb);
            }
            src => panic!("Invalid i2i src: {src}"),
        }

        assert!(
            (self.dst_type.bits() <= 32 && self.src_type.bits() <= 32)
                || (self.dst_type.bits() >= 32 && self.src_type.bits() >= 32)
        );
        e.set_field(10..12, (self.dst_type.bits() / 8).ilog2());
        e.set_field(12..14, (self.src_type.bits() / 8).ilog2());
        e.set_bit(14, self.dst_type.is_signed());
        e.set_bit(15, self.src_type.is_signed());

        e.set_field(44..46, 0u8); // src.B1-3
        e.set_bit(48, self.neg);
        e.set_bit(50, false); // dst.CC
        e.set_bit(52, self.abs);
        e.set_bit(53, self.saturate);
    }
}
