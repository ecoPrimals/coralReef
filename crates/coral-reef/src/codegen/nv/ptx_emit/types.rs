// SPDX-License-Identifier: AGPL-3.0-or-later

/// Representation of one scalar/vector/register value in generated PTX.
#[derive(Clone, Debug)]
pub enum PtxVal {
    R32(u32),
    Rd64(u32),
    Pred(u32),
    Vec(Vec<Self>),
}

impl PtxVal {
    pub(crate) fn fmt_operand(&self) -> String {
        match self {
            Self::R32(id) => format!("%r{id}"),
            Self::Rd64(id) => format!("%rd{id}"),
            Self::Pred(id) => format!("%p{id}"),
            Self::Vec(_) => crate::codegen::ice!("cannot use vector as scalar operand"),
        }
    }

    pub(crate) fn component(&self, idx: usize) -> &Self {
        match self {
            Self::Vec(v) => &v[idx],
            _ if idx == 0 => self,
            _ => crate::codegen::ice!("scalar has no component {idx}"),
        }
    }

    pub(crate) fn is_64bit(&self) -> bool {
        matches!(self, Self::Rd64(_))
    }
}

/// Per storage/uniform buffer binding for parameter passing.
#[derive(Debug)]
pub struct BufferBinding {
    pub(crate) group: u32,
    pub(crate) binding: u32,
    pub(crate) gv_handle: naga::Handle<naga::GlobalVariable>,
    pub(crate) element_stride: u32,
}

/// Workgroup shared variable layout in the `.shared` blob.
#[derive(Debug)]
pub struct SharedVar {
    pub(crate) gv_handle: naga::Handle<naga::GlobalVariable>,
    #[allow(
        dead_code,
        reason = "reserved for future shared-memory alignment diagnostics"
    )]
    pub(crate) size_bytes: u32,
    #[allow(
        dead_code,
        reason = "reserved for future shared-memory alignment diagnostics"
    )]
    pub(crate) align: u32,
    pub(crate) offset: u32,
}

/// Per image/texture binding for surface operations.
#[derive(Debug)]
pub struct SurfaceBinding {
    pub(crate) binding: u32,
    pub(crate) gv_handle: naga::Handle<naga::GlobalVariable>,
    pub(crate) dim: ImageDim,
    pub(crate) texel_format: TexelFormat,
}

/// Image dimensionality for surface instructions.
#[derive(Debug, Clone, Copy)]
pub enum ImageDim {
    D1,
    D2,
    D3,
}

impl ImageDim {
    pub(crate) fn ptx_suffix(self) -> &'static str {
        match self {
            Self::D1 => "1d",
            Self::D2 => "2d",
            Self::D3 => "3d",
        }
    }
}

/// Texel format (determines PTX element type width and component count).
///
/// Maps naga `StorageFormat` variants to the PTX surface instruction's
/// type suffix (e.g. `v4.b8` for RGBA8, `b32` for R32).
#[derive(Debug, Clone, Copy)]
pub enum TexelFormat {
    R8,
    R16,
    R32,
    Rg8,
    Rg16,
    Rg32,
    Rgba8,
    Bgra8,
    Rgba16,
    Rgba32,
}

impl TexelFormat {
    pub(crate) fn ptx_type(self) -> &'static str {
        match self {
            Self::R8 => "b8",
            Self::R16 => "b16",
            Self::R32 => "b32",
            Self::Rg8 => "v2.b8",
            Self::Rg16 => "v2.b16",
            Self::Rg32 => "v2.b32",
            Self::Rgba8 | Self::Bgra8 => "v4.b8",
            Self::Rgba16 => "v4.b16",
            Self::Rgba32 => "v4.b32",
        }
    }

    pub(crate) fn component_count(self) -> usize {
        match self {
            Self::R8 | Self::R16 | Self::R32 => 1,
            Self::Rg8 | Self::Rg16 | Self::Rg32 => 2,
            Self::Rgba8 | Self::Bgra8 | Self::Rgba16 | Self::Rgba32 => 4,
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum MemSpaceKind {
    Global,
    Shared,
}
