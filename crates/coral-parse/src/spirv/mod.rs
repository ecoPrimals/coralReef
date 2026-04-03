// SPDX-License-Identifier: AGPL-3.0-only
//! SPIR-V binary → sovereign [`crate::ast::Module`].

mod reader;
pub use reader::parse;
