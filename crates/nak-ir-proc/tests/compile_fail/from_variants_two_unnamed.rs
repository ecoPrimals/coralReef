// SPDX-License-Identifier: AGPL-3.0-only
//! `FromVariants` requires exactly one field per variant.

#![allow(dead_code)]

use nak_ir_proc::FromVariants;

#[derive(FromVariants)]
enum Bad {
    Pair(u8, u8),
}

fn main() {}
