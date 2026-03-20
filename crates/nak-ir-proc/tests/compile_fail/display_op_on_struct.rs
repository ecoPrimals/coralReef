// SPDX-License-Identifier: AGPL-3.0-only
//! `DisplayOp` is only implemented for enums.

#![allow(dead_code)]

use nak_ir_proc::DisplayOp;

#[derive(DisplayOp)]
struct NotAnEnum;

fn main() {}
