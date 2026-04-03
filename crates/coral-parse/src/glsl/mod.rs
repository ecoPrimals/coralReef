// SPDX-License-Identifier: AGPL-3.0-only
//! GLSL 450/460 front end (compute subset).
//!
//! `parse()` currently targets compute shaders (`void main()` + layout/local_size).
//! Vertex and fragment pipelines are not wired yet; extend the parser when those
//! stages are needed.

mod lexer;
mod parser;

pub use parser::parse;
