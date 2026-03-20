// SPDX-License-Identifier: AGPL-3.0-only
//! `Encode` only supports structs with named fields.

#![allow(dead_code)]

use nak_ir_proc::Encode;

#[derive(Encode)]
enum Bad {
    V(u32),
}

fn main() {}
