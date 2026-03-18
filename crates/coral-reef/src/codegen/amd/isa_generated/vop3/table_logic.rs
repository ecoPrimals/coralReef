// SPDX-License-Identifier: AGPL-3.0-only
// Copyright © 2026 ecoPrimals
//! AUTO-GENERATED from AMD RDNA2 ISA XML specification.
//!
//! Source: specs/amd/amdgpu_isa_rdna2.xml (MIT license, AMD GPUOpen)
//! Generator: tools/amd-isa-gen (pure Rust, sovereign toolchain)
//!
//! DO NOT EDIT BY HAND. Regenerate with:
//!   cargo run -p amd-isa-gen

use super::super::isa_types::InstrEntry;

pub const TABLE: &[InstrEntry] = &[
    InstrEntry { name: "V_LSHRREV_B32", opcode: 278, is_branch: false, is_terminator: false },
    InstrEntry { name: "V_ASHRREV_I32", opcode: 280, is_branch: false, is_terminator: false },
    InstrEntry { name: "V_LSHLREV_B32", opcode: 282, is_branch: false, is_terminator: false },
    InstrEntry { name: "V_AND_B32", opcode: 283, is_branch: false, is_terminator: false },
    InstrEntry { name: "V_OR_B32", opcode: 284, is_branch: false, is_terminator: false },
    InstrEntry { name: "V_XOR_B32", opcode: 285, is_branch: false, is_terminator: false },
    InstrEntry { name: "V_XNOR_B32", opcode: 286, is_branch: false, is_terminator: false },
    InstrEntry { name: "V_BFE_U32", opcode: 328, is_branch: false, is_terminator: false },
    InstrEntry { name: "V_BFE_I32", opcode: 329, is_branch: false, is_terminator: false },
    InstrEntry { name: "V_BFI_B32", opcode: 330, is_branch: false, is_terminator: false },
    InstrEntry { name: "V_ALIGNBIT_B32", opcode: 334, is_branch: false, is_terminator: false },
    InstrEntry { name: "V_ALIGNBYTE_B32", opcode: 335, is_branch: false, is_terminator: false },
    InstrEntry { name: "V_XOR3_B32", opcode: 376, is_branch: false, is_terminator: false },
    InstrEntry { name: "V_NOT_B32", opcode: 439, is_branch: false, is_terminator: false },
    InstrEntry { name: "V_BFREV_B32", opcode: 440, is_branch: false, is_terminator: false },
    InstrEntry { name: "V_FFBH_U32", opcode: 441, is_branch: false, is_terminator: false },
    InstrEntry { name: "V_FFBL_B32", opcode: 442, is_branch: false, is_terminator: false },
    InstrEntry { name: "V_FFBH_I32", opcode: 443, is_branch: false, is_terminator: false },
    InstrEntry { name: "V_LSHLREV_B64", opcode: 767, is_branch: false, is_terminator: false },
    InstrEntry { name: "V_LSHRREV_B64", opcode: 768, is_branch: false, is_terminator: false },
    InstrEntry { name: "V_ASHRREV_I64", opcode: 769, is_branch: false, is_terminator: false },
    InstrEntry { name: "V_LSHRREV_B16", opcode: 775, is_branch: false, is_terminator: false },
    InstrEntry { name: "V_ASHRREV_I16", opcode: 776, is_branch: false, is_terminator: false },
    InstrEntry { name: "V_LSHLREV_B16", opcode: 788, is_branch: false, is_terminator: false },
    InstrEntry { name: "V_PERM_B32", opcode: 836, is_branch: false, is_terminator: false },
    InstrEntry { name: "V_BFM_B32", opcode: 867, is_branch: false, is_terminator: false },
    InstrEntry { name: "V_BCNT_U32_B32", opcode: 868, is_branch: false, is_terminator: false },
    InstrEntry { name: "V_MBCNT_LO_U32_B32", opcode: 869, is_branch: false, is_terminator: false },
    InstrEntry { name: "V_MBCNT_HI_U32_B32", opcode: 870, is_branch: false, is_terminator: false },
    InstrEntry { name: "V_LSHL_OR_B32", opcode: 879, is_branch: false, is_terminator: false },
    InstrEntry { name: "V_AND_OR_B32", opcode: 881, is_branch: false, is_terminator: false },
    InstrEntry { name: "V_OR3_B32", opcode: 882, is_branch: false, is_terminator: false },
    InstrEntry { name: "V_PERMLANE16_B32", opcode: 887, is_branch: false, is_terminator: false },
    InstrEntry { name: "V_PERMLANEX16_B32", opcode: 888, is_branch: false, is_terminator: false },
];

#[must_use]
pub fn lookup(opcode: u16) -> Option<&'static InstrEntry> {
    TABLE.iter().find(|e| e.opcode == opcode)
}
