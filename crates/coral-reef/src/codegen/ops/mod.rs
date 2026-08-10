// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright © 2026 ecoPrimals
//! Unified ops encoder — vendor-agnostic operation encoding via `EncodeOp<E>`.
//!
//! This module organizes GPU instruction encoding by **operation category**
//! rather than by vendor. Each op implements `EncodeOp<E>` for each encoder
//! type (`AmdOpEncoder`, and eventually `SM70Encoder`), keeping all vendor
//! implementations together per operation.
//!
//! ## Architecture
//!
//! ```text
//!   Op enum ──→ op_encode! macro ──→ category module (e.g. memory.rs)
//!                                        ├─ impl EncodeOp<AmdOpEncoder>
//!                                        └─ impl EncodeOp<SM70Encoder>  (future)
//! ```

pub mod alu_float;
pub mod alu_int;
mod alu_int_plop3;
pub mod control;
pub mod convert;
pub mod memory;
pub mod system;

mod amd_dispatch;
mod encoding_helpers;
mod gfx9;

#[cfg(test)]
#[path = "control_tests.rs"]
mod control_tests;
#[cfg(test)]
#[path = "convert_tests.rs"]
mod convert_tests;
#[cfg(test)]
#[path = "memory_tests.rs"]
mod memory_tests;
#[cfg(test)]
#[path = "system_tests.rs"]
mod system_tests;

pub use amd_dispatch::{AmdOpEncoder, EncodeOp, encode_amd_op};
pub use encoding_helpers::{
    SrcEncoding, cbuf_to_user_sgpr_encoding, dst_to_vgpr_index, encode_vop2_from_srcs,
    encode_vop3_f64_from_srcs, encode_vop3_from_srcs, encode_vopc_legalized,
    materialize_f64_if_literal, materialize_if_literal, src_to_encoding, src_to_vgpr_index,
};
