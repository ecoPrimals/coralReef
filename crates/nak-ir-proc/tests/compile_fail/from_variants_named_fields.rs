// SPDX-License-Identifier: AGPL-3.0-or-later
//! `FromVariants` requires each variant to have exactly one unnamed field.

#![allow(dead_code)]

use nak_ir_proc::FromVariants;

#[derive(FromVariants)]
enum Bad {
    Named { x: u8 },
}

fn main() {}
