// SPDX-License-Identifier: AGPL-3.0-only
//! `SrcsAsSlice` requires a struct with named fields.

#![allow(dead_code)]

use nak_ir_proc::SrcsAsSlice;

#[derive(Clone, Copy)]
struct Src(u8);

#[derive(SrcsAsSlice)]
struct BadTuple(Src);

fn main() {}
