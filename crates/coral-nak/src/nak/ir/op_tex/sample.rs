// Copyright © 2022 Collabora, Ltd.
// SPDX-License-Identifier: MIT
//! Texture sampling operations.

use super::*;
#[repr(C)]
#[derive(SrcsAsSlice, DstsAsSlice)]
pub struct OpTex {
    pub dsts: [Dst; 2],
    pub fault: Dst,

    pub tex: TexRef,

    #[src_type(SSA)]
    pub srcs: [Src; 2],

    pub dim: TexDim,
    pub lod_mode: TexLodMode,
    pub deriv_mode: TexDerivMode,
    pub z_cmpr: bool,
    pub offset_mode: TexOffsetMode,
    pub mem_eviction_priority: MemEvictionPriority,
    pub nodep: bool,
    pub channel_mask: ChannelMask,
}

impl DisplayOp for OpTex {
    fn fmt_op(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "tex{}{}{}{}",
            self.dim, self.lod_mode, self.offset_mode, self.deriv_mode
        )?;
        if self.z_cmpr {
            write!(f, ".dc")?;
        }
        write!(f, "{}", self.mem_eviction_priority)?;
        if self.nodep {
            write!(f, ".nodep")?;
        }
        write!(f, "{}", self.channel_mask)?;
        write!(f, " {} {} {}", self.tex, self.srcs[0], self.srcs[1])
    }
}
impl_display_for_op!(OpTex);

#[repr(C)]
#[derive(SrcsAsSlice, DstsAsSlice)]
pub struct OpTld {
    pub dsts: [Dst; 2],
    pub fault: Dst,

    pub tex: TexRef,

    #[src_type(SSA)]
    pub srcs: [Src; 2],

    pub dim: TexDim,
    pub is_ms: bool,
    pub lod_mode: TexLodMode,
    pub offset_mode: TexOffsetMode,
    pub mem_eviction_priority: MemEvictionPriority,
    pub nodep: bool,
    pub channel_mask: ChannelMask,
}

impl DisplayOp for OpTld {
    fn fmt_op(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "tld{}{}{}", self.dim, self.lod_mode, self.offset_mode)?;
        if self.is_ms {
            write!(f, ".ms")?;
        }
        write!(f, "{}", self.mem_eviction_priority)?;
        if self.nodep {
            write!(f, ".nodep")?;
        }
        write!(f, "{}", self.channel_mask)?;
        write!(f, " {} {} {}", self.tex, self.srcs[0], self.srcs[1])
    }
}
impl_display_for_op!(OpTld);

#[repr(C)]
#[derive(SrcsAsSlice, DstsAsSlice)]
pub struct OpTld4 {
    pub dsts: [Dst; 2],
    pub fault: Dst,

    pub tex: TexRef,

    #[src_type(SSA)]
    pub srcs: [Src; 2],

    pub dim: TexDim,
    pub comp: u8,
    pub offset_mode: TexOffsetMode,
    pub z_cmpr: bool,
    pub mem_eviction_priority: MemEvictionPriority,
    pub nodep: bool,
    pub channel_mask: ChannelMask,
}

impl DisplayOp for OpTld4 {
    fn fmt_op(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "tld4.g{}{}", self.dim, self.offset_mode)?;
        if self.z_cmpr {
            write!(f, ".dc")?;
        }
        write!(f, "{}", self.mem_eviction_priority)?;
        if self.nodep {
            write!(f, ".nodep")?;
        }
        write!(f, "{}", self.channel_mask)?;
        write!(f, " {} {} {}", self.tex, self.srcs[0], self.srcs[1])
    }
}
impl_display_for_op!(OpTld4);

#[repr(C)]
#[derive(SrcsAsSlice, DstsAsSlice)]
pub struct OpTmml {
    pub dsts: [Dst; 2],

    pub tex: TexRef,

    #[src_type(SSA)]
    pub srcs: [Src; 2],

    pub dim: TexDim,
    pub deriv_mode: TexDerivMode,
    pub nodep: bool,
    pub channel_mask: ChannelMask,
}

impl DisplayOp for OpTmml {
    fn fmt_op(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "tmml.lod{}{}", self.dim, self.deriv_mode)?;
        if self.nodep {
            write!(f, ".nodep")?;
        }
        write!(f, "{}", self.channel_mask)?;
        write!(f, " {} {} {}", self.tex, self.srcs[0], self.srcs[1])
    }
}
impl_display_for_op!(OpTmml);

#[repr(C)]
#[derive(SrcsAsSlice, DstsAsSlice)]
pub struct OpTxd {
    pub dsts: [Dst; 2],
    pub fault: Dst,

    pub tex: TexRef,

    #[src_type(SSA)]
    pub srcs: [Src; 2],

    pub dim: TexDim,
    pub offset_mode: TexOffsetMode,
    pub mem_eviction_priority: MemEvictionPriority,
    pub nodep: bool,
    pub channel_mask: ChannelMask,
}

impl DisplayOp for OpTxd {
    fn fmt_op(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "txd{}{}{}",
            self.dim, self.offset_mode, self.mem_eviction_priority
        )?;
        if self.nodep {
            write!(f, ".nodep")?;
        }
        write!(f, "{}", self.channel_mask)?;
        write!(f, " {} {} {}", self.tex, self.srcs[0], self.srcs[1])
    }
}
impl_display_for_op!(OpTxd);

#[repr(C)]
#[derive(SrcsAsSlice, DstsAsSlice)]
pub struct OpTxq {
    pub dsts: [Dst; 2],

    pub tex: TexRef,

    #[src_type(SSA)]
    pub src: Src,

    pub query: TexQuery,
    pub nodep: bool,
    pub channel_mask: ChannelMask,
}

impl DisplayOp for OpTxq {
    fn fmt_op(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "txq")?;
        if self.nodep {
            write!(f, ".nodep")?;
        }
        write!(f, "{}", self.channel_mask)?;
        write!(f, " {} {} {}", self.tex, self.src, self.query)
    }
}
impl_display_for_op!(OpTxq);

#[allow(dead_code)]
#[derive(Clone, Copy, Eq, PartialEq)]
pub enum ImageAccess {
    Binary(MemType),
    Formatted(ChannelMask),
}

impl fmt::Display for ImageAccess {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ImageAccess::Binary(mem_type) => write!(f, ".b{mem_type}"),
            ImageAccess::Formatted(mask) => write!(f, ".p{mask}"),
        }
    }
}
