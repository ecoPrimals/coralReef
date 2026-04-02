// SPDX-License-Identifier: AGPL-3.0-only
//! WGSL parser — recursive descent, zero external dependencies.

mod lexer;
mod parser;

pub use parser::parse;
