// SPDX-License-Identifier: AGPL-3.0-only
//! `coral-parse` — sovereign shader parser for coral-reef.
//!
//! Parses WGSL (Evolution 1), SPIR-V (Evolution 3), and GLSL (Evolution 4)
//! into a shared sovereign AST, then lowers to CoralIR. Replaces naga.
//!
//! ## Architecture
//!
//! ```text
//! WGSL text ──→ wgsl::parse() ──→ ast::Module ──→ lower::lower() ──→ Shader
//! SPIR-V bin ──→ spirv::parse() ──→ ast::Module ──→ lower::lower() ──→ Shader
//! GLSL text  ──→ glsl::parse()  ──→ ast::Module ──→ lower::lower() ──→ Shader
//! ```
//!
//! The `CoralFrontend` implements `coral_reef::Frontend` and can be plugged
//! directly into the existing compilation pipeline.

pub mod ast;
pub mod error;
mod frontend;
pub mod glsl;
pub mod lower;
pub mod spirv;
pub mod wgsl;

pub use frontend::CoralFrontend;
