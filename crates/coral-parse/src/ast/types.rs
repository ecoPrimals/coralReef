// SPDX-License-Identifier: AGPL-3.0-only
//! Sovereign type system — scalar, vector, matrix, array, struct, pointer, atomic.

use super::Handle;

/// A type in the AST.
#[derive(Debug, Clone)]
pub enum Type {
    Scalar(Scalar),
    Vector {
        scalar: Scalar,
        size: VectorSize,
    },
    Matrix {
        scalar: Scalar,
        columns: VectorSize,
        rows: VectorSize,
    },
    Array {
        base: Handle<Type>,
        size: ArraySize,
    },
    Struct {
        name: Option<String>,
        members: Vec<StructMember>,
    },
    Pointer {
        base: Handle<Type>,
        space: super::AddressSpace,
    },
    Atomic(Scalar),
    Bool,
    Sampler { comparison: bool },
    Texture {
        dim: ImageDimension,
        arrayed: bool,
        multisampled: bool,
        sample_type: TextureSampleType,
    },
    DepthTexture {
        dim: ImageDimension,
        arrayed: bool,
        multisampled: bool,
    },
    StorageTexture {
        dim: ImageDimension,
        arrayed: bool,
        format: StorageFormat,
        access: super::StorageAccess,
    },
}

impl Type {
    /// Byte size of this type (for layout purposes). Returns 0 for unsized arrays.
    #[must_use]
    pub fn byte_size<'a>(&'a self, get_type: &'a dyn Fn(Handle<Type>) -> &'a Type) -> u32 {
        match self {
            Self::Scalar(s) => s.byte_width(),
            Self::Vector { scalar, size } => scalar.byte_width() * size.count(),
            Self::Matrix {
                scalar,
                columns,
                rows,
            } => scalar.byte_width() * columns.count() * rows.count(),
            Self::Array { base, size } => match size {
                ArraySize::Constant(n) => get_type(*base).byte_size(get_type) * n,
                ArraySize::Dynamic => 0,
            },
            Self::Struct { members, .. } => members.iter().map(|m| get_type(m.ty).byte_size(get_type)).sum(),
            Self::Pointer { .. } => 8,
            Self::Atomic(s) => s.byte_width(),
            Self::Bool => 1,
            Self::Sampler { .. } | Self::Texture { .. } | Self::DepthTexture { .. } | Self::StorageTexture { .. } => 0,
        }
    }
}

/// Scalar numeric kind.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Scalar {
    pub kind: ScalarKind,
    pub width: u8,
}

impl Scalar {
    pub const F32: Self = Self { kind: ScalarKind::Float, width: 4 };
    pub const F64: Self = Self { kind: ScalarKind::Float, width: 8 };
    pub const U32: Self = Self { kind: ScalarKind::Uint, width: 4 };
    pub const I32: Self = Self { kind: ScalarKind::Sint, width: 4 };
    pub const BOOL: Self = Self { kind: ScalarKind::Bool, width: 1 };

    #[must_use]
    pub const fn byte_width(self) -> u32 {
        self.width as u32
    }
}

/// Scalar kind.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ScalarKind {
    Float,
    Sint,
    Uint,
    Bool,
}

/// Vector component count.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum VectorSize {
    Bi = 2,
    Tri = 3,
    Quad = 4,
}

impl VectorSize {
    #[must_use]
    pub const fn count(self) -> u32 {
        self as u32
    }
}

/// Array size: fixed or runtime-sized.
#[derive(Debug, Clone, Copy)]
pub enum ArraySize {
    Constant(u32),
    Dynamic,
}

/// A member of a struct type.
#[derive(Debug, Clone)]
pub struct StructMember {
    pub name: Option<String>,
    pub ty: Handle<Type>,
    pub offset: Option<u32>,
    pub binding: Option<super::Binding>,
}

/// Texture/image dimension.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ImageDimension {
    D1,
    D2,
    D3,
    Cube,
}

/// What a texture samples as.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum TextureSampleType {
    Float { filterable: bool },
    Depth,
    Sint,
    Uint,
}

/// Storage texture format.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum StorageFormat {
    R32Float,
    R32Sint,
    R32Uint,
    Rg32Float,
    Rg32Sint,
    Rg32Uint,
    Rgba8Unorm,
    Rgba8Snorm,
    Rgba8Uint,
    Rgba8Sint,
    Rgba16Float,
    Rgba16Sint,
    Rgba16Uint,
    Rgba32Float,
    Rgba32Sint,
    Rgba32Uint,
    Bgra8Unorm,
}

/// Interpolation type for vertex-to-fragment data.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Interpolation {
    Perspective,
    Linear,
    Flat,
}

/// Interpolation sampling point.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Sampling {
    Center,
    Centroid,
    Sample,
}
