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

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum MemSpaceKind {
    Global,
    Shared,
}
