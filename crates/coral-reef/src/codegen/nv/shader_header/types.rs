// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright © 2025-2026 ecoPrimals
// Derived from Collabora, Ltd. (2023)

//! Domain types for SPH construction: stage classification and I/O encodings.

use crate::codegen::ir::ShaderStageInfo;

/// Fragment shader variant key — controls SPH encoding for FS-specific behavior.
#[derive(Debug, Default, Clone, Copy)]
pub struct FragmentShaderKey {
    /// Whether the FS uses conservative rasterization underestimate mode.
    pub uses_underestimate: bool,
    /// Whether there is a depth/stencil self-dependency.
    pub zs_self_dep: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ShaderType {
    Vertex,
    TessellationInit,
    Tessellation,
    Geometry,
    Fragment,
}

impl From<&ShaderStageInfo> for ShaderType {
    fn from(value: &ShaderStageInfo) -> Self {
        match value {
            ShaderStageInfo::Vertex(_) => Self::Vertex,
            ShaderStageInfo::Fragment(_) => Self::Fragment,
            ShaderStageInfo::Geometry(_) => Self::Geometry,
            ShaderStageInfo::TessellationInit(_) => Self::TessellationInit,
            ShaderStageInfo::Tessellation(_) => Self::Tessellation,
            ShaderStageInfo::Compute(_) => {
                crate::codegen::ice!("Invalid ShaderStageInfo {value:?}")
            }
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OutputTopology {
    PointList,
    LineStrip,
    TriangleStrip,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PixelImap {
    Unused,
    Constant,
    Perspective,
    ScreenLinear,
}

impl From<PixelImap> for u8 {
    fn from(value: PixelImap) -> Self {
        match value {
            PixelImap::Unused => 0,
            PixelImap::Constant => 1,
            PixelImap::Perspective => 2,
            PixelImap::ScreenLinear => 3,
        }
    }
}
