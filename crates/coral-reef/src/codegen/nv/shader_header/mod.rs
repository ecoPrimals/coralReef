// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright © 2025-2026 ecoPrimals
// Derived from Collabora, Ltd. (2023)

//! NVIDIA Shader Program Header (SPH) encoding for graphics pipelines.

mod encode;
mod program_header;
mod sphv3_layout;
mod types;

// Public surface of this module: not referenced by name inside `mod.rs`.
#[expect(
    unused_imports,
    reason = "pub use re-exports define the module API; names are not referenced inside this mod.rs"
)]
pub use self::{
    encode::encode_header,
    program_header::ShaderProgramHeader,
    sphv3_layout::*,
    types::{FragmentShaderKey, OutputTopology, PixelImap, ShaderType},
};

#[cfg(test)]
mod tests;
