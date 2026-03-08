// SPDX-License-Identifier: AGPL-3.0-only
// Copyright © 2026 ecoPrimals
//! AUTO-GENERATED from AMD RDNA2 ISA XML specification.
//!
//! Aggregation module for VOP3 comparison ops (float + integer).
//! Split into table_cmp_f (float) and table_cmp_i (integer) for line-count limits.
//!
//! Source: specs/amd/amdgpu_isa_rdna2.xml (MIT license, AMD GPUOpen)
//! Generator: tools/amd-isa-gen (pure Rust, sovereign toolchain)
//!
//! DO NOT EDIT BY HAND. Regenerate with:
//!   cargo run -p amd-isa-gen

use super::super::isa_types::InstrEntry;
use super::table_cmp_f;
use super::table_cmp_i;
use std::sync::OnceLock;

static TABLE_CACHE: OnceLock<Vec<InstrEntry>> = OnceLock::new();

/// Combined comparison ops table (float + integer).
#[must_use]
pub fn table() -> &'static [InstrEntry] {
    TABLE_CACHE
        .get_or_init(|| [table_cmp_f::TABLE, table_cmp_i::TABLE].concat())
        .as_slice()
}

#[must_use]
pub fn lookup(opcode: u16) -> Option<&'static InstrEntry> {
    table_cmp_f::lookup(opcode).or_else(|| table_cmp_i::lookup(opcode))
}
