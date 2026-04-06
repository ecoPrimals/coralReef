// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright © 2025-2026 ecoPrimals
// Derived from Collabora, Ltd. (2025)
//! SM20 control flow instruction encoders.

use super::encoder::*;

impl SM20Op for OpBra {
    fn legalize(&mut self, _b: &mut LegalizeBuilder) {}

    fn encode(&self, e: &mut SM20Encoder<'_>) {
        e.set_opcode(SM20Unit::Exec, 0x10);
        e.set_field(5..9, 0xf_u8);
        e.set_bit(15, false);
        e.set_bit(16, false);
        e.set_rel_offset(26..50, &self.target);
    }
}

impl SM20Op for OpSSy {
    fn legalize(&mut self, _b: &mut LegalizeBuilder) {}

    fn encode(&self, e: &mut SM20Encoder<'_>) {
        e.set_opcode(SM20Unit::Exec, 0x18);
        e.set_rel_offset(26..50, &self.target);
    }
}

impl SM20Op for OpSync {
    fn legalize(&mut self, _b: &mut LegalizeBuilder) {}

    fn encode(&self, e: &mut SM20Encoder<'_>) {
        e.set_opcode(SM20Unit::Move, 0x10);
        e.set_field(5..9, 0xf_u8);
        e.set_bit(4, true);
    }
}

impl SM20Op for OpBrk {
    fn legalize(&mut self, _b: &mut LegalizeBuilder) {}

    fn encode(&self, e: &mut SM20Encoder<'_>) {
        e.set_opcode(SM20Unit::Exec, 0x2a);
        e.set_field(5..9, 0xf_u8);
    }
}

impl SM20Op for OpPBk {
    fn legalize(&mut self, _b: &mut LegalizeBuilder) {}

    fn encode(&self, e: &mut SM20Encoder<'_>) {
        e.set_opcode(SM20Unit::Exec, 0x1a);
        e.set_rel_offset(26..50, &self.target);
    }
}

impl SM20Op for OpCont {
    fn legalize(&mut self, _b: &mut LegalizeBuilder) {}

    fn encode(&self, e: &mut SM20Encoder<'_>) {
        e.set_opcode(SM20Unit::Exec, 0x2c);
        e.set_field(5..9, 0xf_u8);
    }
}

impl SM20Op for OpPCnt {
    fn legalize(&mut self, _b: &mut LegalizeBuilder) {}

    fn encode(&self, e: &mut SM20Encoder<'_>) {
        e.set_opcode(SM20Unit::Exec, 0x1c);
        e.set_rel_offset(26..50, &self.target);
    }
}

impl SM20Op for OpExit {
    fn legalize(&mut self, _b: &mut LegalizeBuilder) {}

    fn encode(&self, e: &mut SM20Encoder<'_>) {
        e.set_opcode(SM20Unit::Exec, 0x20);
        e.set_field(5..9, 0xf_u8);
    }
}

impl SM20Op for OpBar {
    fn legalize(&mut self, _b: &mut LegalizeBuilder) {}

    fn encode(&self, e: &mut SM20Encoder<'_>) {
        e.set_opcode(SM20Unit::Move, 0x14);
        e.set_field(5..7, 0_u8);
        e.set_field(7..9, 0_u8);
        e.set_reg_src(20..26, &0.into());
        e.set_reg_src(26..32, &0.into());
        e.set_bit(46, false);
        e.set_bit(47, false);
        e.set_pred_src(49..53, &true.into());
        e.set_pred_dst(53..56, &Dst::None);
    }
}

impl SM20Op for OpKill {
    fn legalize(&mut self, _b: &mut LegalizeBuilder) {}

    fn encode(&self, e: &mut SM20Encoder<'_>) {
        e.set_opcode(SM20Unit::Exec, 0x26);
        e.set_field(5..9, 0xf_u8);
    }
}

impl SM20Op for OpNop {
    fn legalize(&mut self, _b: &mut LegalizeBuilder) {}

    fn encode(&self, e: &mut SM20Encoder<'_>) {
        e.set_opcode(SM20Unit::Move, 0x10);
        e.set_field(5..9, 0xf_u8);
    }
}
