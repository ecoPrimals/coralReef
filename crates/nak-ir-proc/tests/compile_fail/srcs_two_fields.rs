// SPDX-License-Identifier: AGPL-3.0-only
//! Two separate `Src` fields are rejected (must use one array field).

#![allow(dead_code)]

use nak_ir_proc::SrcsAsSlice;

#[derive(Clone, Copy)]
struct Src(u8);

/// Attribute enum referenced by generated `AsSlice` impl (mirrors codegen IR).
#[derive(Clone, Copy)]
enum SrcType {
    DEFAULT,
    A,
    B,
}

#[derive(SrcsAsSlice)]
struct TwoFields {
    #[src_type(A)]
    a: Src,
    #[src_type(B)]
    b: Src,
}

fn main() {
    let _ = core::mem::size_of::<TwoFields>();
}
