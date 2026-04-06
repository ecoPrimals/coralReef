// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright © 2025-2026 ecoPrimals
// Derived from Collabora, Ltd. (2022)
//! SM70 ALU instruction encoders: conversion ops.

use super::*;

impl SM70Op for OpF2F {
    fn legalize(&mut self, _b: &mut LegalizeBuilder) {
        // Nothing to do
    }

    fn encode(&self, e: &mut SM70Encoder<'_>) {
        assert!(!self.integer_rnd);

        // The swizzle is handled by the .high bit below.
        let src = self.src.clone().without_swizzle();
        if self.src_type.bits() <= 32 && self.dst_type.bits() <= 32 {
            e.encode_alu(0x104, Some(&self.dst), None, Some(&src), None);
        } else {
            e.encode_alu(0x110, Some(&self.dst), None, Some(&src), None);
        }

        if self.is_high() {
            e.set_field(60..62, 1_u8); // .H1
        }

        e.set_field(75..77, (self.dst_type.bits() / 8).ilog2());
        e.set_rnd_mode(78..80, self.rnd_mode);
        e.set_bit(80, self.ftz);
        e.set_field(84..86, (self.src_type.bits() / 8).ilog2());
    }
}

impl SM70Op for OpF2FP {
    fn legalize(&mut self, b: &mut LegalizeBuilder) {
        let gpr = op_gpr(self);
        let [src0, _src1] = &mut self.srcs;
        b.copy_alu_src_if_not_reg(src0, gpr, SrcType::ALU);
    }

    fn encode(&self, e: &mut SM70Encoder<'_>) {
        e.encode_alu(
            0x03e,
            Some(&self.dst),
            Some(&self.srcs[0]),
            Some(&self.srcs[1]),
            Some(&Src::ZERO),
        );

        // .MERGE_C behavior
        // Use src1 and src2, src0 is unused
        // src1 get converted and packed in the lower 16 bits of dest.
        // src2 lower or high 16 bits (decided by .H1 flag) get packed in the upper of dest.
        e.set_bit(78, false); // .MERGE_C: disabled (not using merge conversion)
        e.set_bit(72, false); // .H1 (MERGE_C only)
        e.set_rnd_mode(79..81, self.rnd_mode);
    }
}

impl SM70Op for OpF2I {
    fn legalize(&mut self, _b: &mut LegalizeBuilder) {
        // Nothing to do
    }

    fn encode(&self, e: &mut SM70Encoder<'_>) {
        if self.src_type.bits() <= 32 && self.dst_type.bits() <= 32 {
            e.encode_alu(0x105, Some(&self.dst), None, Some(&self.src), None);
        } else {
            e.encode_alu(0x111, Some(&self.dst), None, Some(&self.src), None);
        }

        e.set_bit(72, self.dst_type.is_signed());
        e.set_field(75..77, (self.dst_type.bits() / 8).ilog2());
        e.set_bit(77, false); // NTZ
        e.set_rnd_mode(78..80, self.rnd_mode);
        e.set_bit(80, self.ftz);
        e.set_field(84..86, (self.src_type.bits() / 8).ilog2());
    }
}

impl SM70Op for OpI2F {
    fn legalize(&mut self, _b: &mut LegalizeBuilder) {
        // Nothing to do
    }

    fn encode(&self, e: &mut SM70Encoder<'_>) {
        if self.src_type.bits() <= 32 && self.dst_type.bits() <= 32 {
            e.encode_alu(0x106, Some(&self.dst), None, Some(&self.src), None);
        } else {
            e.encode_alu(0x112, Some(&self.dst), None, Some(&self.src), None);
        }

        e.set_field(60..62, 0_u8); // subop: default (no special sub-operation)
        e.set_bit(74, self.src_type.is_signed());
        e.set_field(75..77, (self.dst_type.bits() / 8).ilog2());
        e.set_rnd_mode(78..80, self.rnd_mode);
        e.set_field(84..86, (self.src_type.bits() / 8).ilog2());
    }
}

impl SM70Op for OpFRnd {
    fn legalize(&mut self, _b: &mut LegalizeBuilder) {
        // Nothing to do
    }

    fn encode(&self, e: &mut SM70Encoder<'_>) {
        if self.src_type.bits() <= 32 && self.dst_type.bits() <= 32 {
            e.encode_alu(0x107, Some(&self.dst), None, Some(&self.src), None);
        } else {
            e.encode_alu(0x113, Some(&self.dst), None, Some(&self.src), None);
        }

        e.set_field(84..86, (self.src_type.bits() / 8).ilog2());
        e.set_bit(80, self.ftz);
        e.set_rnd_mode(78..80, self.rnd_mode);
        e.set_field(75..77, (self.dst_type.bits() / 8).ilog2());
    }
}
