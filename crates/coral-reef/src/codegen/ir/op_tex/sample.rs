// SPDX-License-Identifier: AGPL-3.0-only
// Copyright © 2025-2026 ecoPrimals
// Derived from Collabora, Ltd. (2022)
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

#[allow(dead_code, reason = "ISA variant reserved for future texture encoding")]
#[derive(Clone, Copy, Eq, PartialEq)]
pub enum ImageAccess {
    Binary(MemType),
    Formatted(ChannelMask),
}

impl fmt::Display for ImageAccess {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Binary(mem_type) => write!(f, ".b{mem_type}"),
            Self::Formatted(mask) => write!(f, ".p{mask}"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn zero_src() -> Src {
        Src::ZERO
    }

    fn make_op_tex(z_cmpr: bool, nodep: bool, mem_eviction_priority: MemEvictionPriority) -> OpTex {
        OpTex {
            dsts: [Dst::None, Dst::None],
            fault: Dst::None,
            tex: TexRef::Bound(0),
            srcs: [zero_src(), Src::new_imm_u32(1)],
            dim: TexDim::_2D,
            lod_mode: TexLodMode::Auto,
            deriv_mode: TexDerivMode::Auto,
            z_cmpr,
            offset_mode: TexOffsetMode::None,
            mem_eviction_priority,
            nodep,
            channel_mask: ChannelMask::for_comps(4),
        }
    }

    #[test]
    fn test_op_tex_display() {
        let op = make_op_tex(false, false, MemEvictionPriority::Normal);
        let s = format!("{}", op);
        assert!(s.contains("tex"));
        assert!(s.contains(".2d"));
        assert!(s.contains("tex[0]"));
        assert!(s.contains(".rgba"));
    }

    #[test]
    fn test_op_tex_display_z_cmpr_nodep() {
        let op = make_op_tex(true, true, MemEvictionPriority::First);
        let s = format!("{}", op);
        assert!(s.contains(".dc"));
        assert!(s.contains(".nodep"));
        assert!(s.contains(".ef"));
    }

    #[test]
    fn test_op_tex_field_access() {
        let op = make_op_tex(false, false, MemEvictionPriority::Normal);
        assert!(matches!(op.dim, TexDim::_2D));
        assert!(matches!(op.lod_mode, TexLodMode::Auto));
        assert_eq!(op.channel_mask.to_bits(), 0xf);
    }

    #[test]
    fn test_op_tld_display() {
        let op = OpTld {
            dsts: [Dst::None, Dst::None],
            fault: Dst::None,
            tex: TexRef::Bindless,
            srcs: [zero_src(), zero_src()],
            dim: TexDim::_3D,
            is_ms: true,
            lod_mode: TexLodMode::Zero,
            offset_mode: TexOffsetMode::AddOffI,
            mem_eviction_priority: MemEvictionPriority::Normal,
            nodep: true,
            channel_mask: ChannelMask::for_comps(2),
        };
        let s = format!("{}", op);
        assert!(s.contains("tld"));
        assert!(s.contains(".3d"));
        assert!(s.contains(".ms"));
        assert!(s.contains(".lz"));
        assert!(s.contains(".aoffi"));
        assert!(s.contains(".nodep"));
        assert!(s.contains(".rg"));
        assert!(s.contains("bindless"));
    }

    #[test]
    fn test_op_tld4_display() {
        let op = OpTld4 {
            dsts: [Dst::None, Dst::None],
            fault: Dst::None,
            tex: TexRef::CBuf(TexCBufRef {
                idx: 1,
                offset: 0x20,
            }),
            srcs: [zero_src(), zero_src()],
            dim: TexDim::_2D,
            comp: 0,
            offset_mode: TexOffsetMode::PerPx,
            z_cmpr: true,
            mem_eviction_priority: MemEvictionPriority::Last,
            nodep: false,
            channel_mask: ChannelMask::for_comps(1),
        };
        let s = format!("{}", op);
        assert!(s.contains("tld4"));
        assert!(s.contains(".g"));
        assert!(s.contains(".2d"));
        assert!(s.contains(".ptp"));
        assert!(s.contains(".dc"));
        assert!(s.contains(".el"));
        assert!(s.contains(".r"));
    }

    #[test]
    fn test_op_tmml_display() {
        let op = OpTmml {
            dsts: [Dst::None, Dst::None],
            tex: TexRef::Bound(2),
            srcs: [zero_src(), Src::new_imm_u32(0x42)],
            dim: TexDim::Cube,
            deriv_mode: TexDerivMode::NonDivergent,
            nodep: false,
            channel_mask: ChannelMask::for_comps(3),
        };
        let s = format!("{}", op);
        assert!(s.contains("tmml"));
        assert!(s.contains(".lod"));
        assert!(s.contains(".cube"));
        assert!(s.contains(".ndv"));
        assert!(s.contains(".rgb"));
        assert!(s.contains("tex[2]"));
    }

    #[test]
    fn test_op_txd_display() {
        let op = OpTxd {
            dsts: [Dst::None, Dst::None],
            fault: Dst::None,
            tex: TexRef::Bound(0),
            srcs: [zero_src(), zero_src()],
            dim: TexDim::Array2D,
            offset_mode: TexOffsetMode::None,
            mem_eviction_priority: MemEvictionPriority::NoAllocate,
            nodep: true,
            channel_mask: ChannelMask::for_comps(4),
        };
        let s = format!("{}", op);
        assert!(s.contains("txd"));
        assert!(s.contains(".a2d"));
        assert!(s.contains(".na"));
        assert!(s.contains(".nodep"));
    }

    #[test]
    fn test_op_txq_display() {
        let op = OpTxq {
            dsts: [Dst::None, Dst::None],
            tex: TexRef::Bound(0),
            src: zero_src(),
            query: TexQuery::Dimension,
            nodep: false,
            channel_mask: ChannelMask::for_comps(4),
        };
        let s = format!("{}", op);
        assert!(s.contains("txq"));
        assert!(s.contains("dimension"));
    }

    #[test]
    fn test_op_txq_display_nodep() {
        let op = OpTxq {
            dsts: [Dst::None, Dst::None],
            tex: TexRef::Bindless,
            src: Src::new_imm_u32(0),
            query: TexQuery::TextureType,
            nodep: true,
            channel_mask: ChannelMask::for_comps(1),
        };
        let s = format!("{}", op);
        assert!(s.contains(".nodep"));
        assert!(s.contains("texture_type"));
    }

    #[test]
    fn test_image_access_display() {
        assert_eq!(format!("{}", ImageAccess::Binary(MemType::B32)), ".b.b32");
        assert_eq!(
            format!("{}", ImageAccess::Formatted(ChannelMask::for_comps(4))),
            ".p.rgba"
        );
    }
}
