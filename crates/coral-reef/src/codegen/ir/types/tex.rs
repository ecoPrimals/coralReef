// SPDX-License-Identifier: AGPL-3.0-only
// Copyright © 2025-2026 ecoPrimals
// Derived from Collabora, Ltd. (2022)
//! Texture, image, and interpolation types.

use super::*;

#[derive(Clone, Copy, Eq, PartialEq)]
pub struct TexCBufRef {
    pub idx: u8,
    pub offset: u16,
}

#[derive(Clone, Copy, Eq, PartialEq)]
pub enum TexRef {
    Bound(u16),
    CBuf(TexCBufRef),
    Bindless,
}

impl fmt::Display for TexRef {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Bound(idx) => write!(f, "tex[{idx}]"),
            Self::CBuf(TexCBufRef { idx, offset }) => {
                write!(f, "c[{idx:#x}][{offset:#x}]")
            }
            Self::Bindless => write!(f, "bindless"),
        }
    }
}

#[derive(Clone, Copy, Eq, PartialEq)]
pub enum TexDim {
    _1D,
    Array1D,
    _2D,
    Array2D,
    _3D,
    Cube,
    ArrayCube,
}

impl fmt::Display for TexDim {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::_1D => write!(f, ".1d"),
            Self::Array1D => write!(f, ".a1d"),
            Self::_2D => write!(f, ".2d"),
            Self::Array2D => write!(f, ".a2d"),
            Self::_3D => write!(f, ".3d"),
            Self::Cube => write!(f, ".cube"),
            Self::ArrayCube => write!(f, ".acube"),
        }
    }
}

#[derive(Clone, Copy, Eq, PartialEq)]
pub enum TexLodMode {
    Auto,
    Zero,
    Bias,
    Lod,
    Clamp,
    BiasClamp,
}

impl TexLodMode {
    pub const fn is_explicit_lod(&self) -> bool {
        match self {
            Self::Auto | Self::Bias | Self::Clamp | Self::BiasClamp => false,
            Self::Zero | Self::Lod => true,
        }
    }
}

impl fmt::Display for TexLodMode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Auto => write!(f, ""),
            Self::Zero => write!(f, ".lz"),
            Self::Bias => write!(f, ".lb"),
            Self::Lod => write!(f, ".ll"),
            Self::Clamp => write!(f, ".lc"),
            Self::BiasClamp => write!(f, ".lb.lc"),
        }
    }
}

/// Derivative behavior for tex ops and FSwzAdd
///
/// The descriptions here may not be wholly accurate as they come from cobbling
/// together a bunch of pieces.  This is my (Faith's) best understanding of how
/// these things work.
#[derive(Clone, Copy, Eq, PartialEq)]
pub enum TexDerivMode {
    /// Automatic
    ///
    /// For partial (not full) quads, the derivative will default to the value
    /// of DEFAULT_PARTIAL in SET_SHADER_CONTROL.
    ///
    /// On Volta and earlier GPUs or on Blackwell B and later, derivatives in
    /// all non-fragment shaders stages are assumed to be partial.
    Auto,

    /// Assume a non-divergent (full) derivative
    ///
    /// Partial derivative checks are skipped and the hardware does the
    /// derivative anyway, possibly on rubbish data.
    NonDivergent,

    /// Force the derivative to be considered divergent (partial)
    ///
    /// This only exists as a separate thing on Blackwell A.  On Hopper and
    /// earlier, there is a .fdv that's part of the LodMode, but only for
    /// LodMode::Clamp.  On Blackwell B, it appears (according to the
    /// disassembler) to be removed again in favor of DerivXY.
    ForceDivergent,

    /// Attempt an X/Y derivative, ignoring shader stage
    ///
    /// This is (I think) identical to Auto except that it ignores the shader
    /// stage checks.  This is new on Blackwell B+.
    DerivXY,
}

impl fmt::Display for TexDerivMode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Auto => Ok(()),
            Self::NonDivergent => write!(f, ".ndv"),
            Self::ForceDivergent => write!(f, ".fdv"),
            Self::DerivXY => write!(f, ".dxy"),
        }
    }
}

#[derive(Clone, Copy, Eq, PartialEq)]
pub struct ChannelMask(u8);

impl ChannelMask {
    pub fn new(mask: u8) -> Self {
        assert!(mask != 0 && (mask & !0xf) == 0);
        Self(mask)
    }

    pub fn for_comps(comps: u8) -> Self {
        assert!(comps > 0 && comps <= 4);
        Self((1 << comps) - 1)
    }

    pub const fn to_bits(self) -> u8 {
        self.0
    }
}

impl fmt::Display for ChannelMask {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, ".")?;
        for (i, c) in ['r', 'g', 'b', 'a'].into_iter().enumerate() {
            if self.0 & (1 << i) != 0 {
                write!(f, "{c}")?;
            }
        }
        Ok(())
    }
}

#[derive(Clone, Copy, Eq, PartialEq)]
pub enum TexOffsetMode {
    None,
    AddOffI,
    PerPx, // tld4 only
}

impl fmt::Display for TexOffsetMode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::None => write!(f, ""),
            Self::AddOffI => write!(f, ".aoffi"),
            Self::PerPx => write!(f, ".ptp"),
        }
    }
}

#[derive(Clone, Copy, Eq, PartialEq)]
pub enum TexQuery {
    Dimension,
    TextureType,
    SamplerPos,
}

impl fmt::Display for TexQuery {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Dimension => write!(f, "dimension"),
            Self::TextureType => write!(f, "texture_type"),
            Self::SamplerPos => write!(f, "sampler_pos"),
        }
    }
}

#[derive(Clone, Copy, Eq, PartialEq)]
pub enum ImageDim {
    _1D,
    _1DBuffer,
    _1DArray,
    _2D,
    _2DArray,
    _3D,
}

impl ImageDim {
    pub const fn coord_comps(&self) -> u8 {
        match self {
            Self::_1D | Self::_1DBuffer => 1,
            Self::_1DArray | Self::_2D => 2,
            Self::_2DArray | Self::_3D => 3,
        }
    }
}

impl fmt::Display for ImageDim {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::_1D => write!(f, ".1d"),
            Self::_1DBuffer => write!(f, ".buf"),
            Self::_1DArray => write!(f, ".a1d"),
            Self::_2D => write!(f, ".2d"),
            Self::_2DArray => write!(f, ".a2d"),
            Self::_3D => write!(f, ".3d"),
        }
    }
}

#[derive(Clone, Copy, Eq, PartialEq)]
pub enum InterpFreq {
    Pass,
    PassMulW,
    Constant,
    State,
}

impl fmt::Display for InterpFreq {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Pass => write!(f, ".pass"),
            Self::PassMulW => write!(f, ".pass_mul_w"),
            Self::Constant => write!(f, ".constant"),
            Self::State => write!(f, ".state"),
        }
    }
}
#[derive(Clone, Copy, Eq, PartialEq)]
pub enum InterpLoc {
    Default,
    Centroid,
    Offset,
}

impl fmt::Display for InterpLoc {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Default => Ok(()),
            Self::Centroid => write!(f, ".centroid"),
            Self::Offset => write!(f, ".offset"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_tex_dim_display() {
        assert_eq!(format!("{}", TexDim::_1D), ".1d");
        assert_eq!(format!("{}", TexDim::Array1D), ".a1d");
        assert_eq!(format!("{}", TexDim::_2D), ".2d");
        assert_eq!(format!("{}", TexDim::Array2D), ".a2d");
        assert_eq!(format!("{}", TexDim::_3D), ".3d");
        assert_eq!(format!("{}", TexDim::Cube), ".cube");
        assert_eq!(format!("{}", TexDim::ArrayCube), ".acube");
    }

    #[test]
    fn test_tex_lod_mode_is_explicit_lod() {
        assert!(!TexLodMode::Auto.is_explicit_lod());
        assert!(TexLodMode::Zero.is_explicit_lod());
        assert!(!TexLodMode::Bias.is_explicit_lod());
        assert!(TexLodMode::Lod.is_explicit_lod());
        assert!(!TexLodMode::Clamp.is_explicit_lod());
        assert!(!TexLodMode::BiasClamp.is_explicit_lod());
    }

    #[test]
    fn test_channel_mask_new() {
        let m = ChannelMask::new(0b1111);
        assert_eq!(m.to_bits(), 0b1111);
        let m = ChannelMask::new(0b0001);
        assert_eq!(m.to_bits(), 0b0001);
    }

    #[test]
    #[should_panic(expected = "assertion")]
    fn test_channel_mask_new_invalid_zero() {
        ChannelMask::new(0);
    }

    #[test]
    #[should_panic(expected = "assertion")]
    fn test_channel_mask_new_invalid_bits() {
        ChannelMask::new(0b10000);
    }

    #[test]
    fn test_channel_mask_for_comps() {
        let m = ChannelMask::for_comps(1);
        assert_eq!(m.to_bits(), 0b0001);
        let m = ChannelMask::for_comps(2);
        assert_eq!(m.to_bits(), 0b0011);
        let m = ChannelMask::for_comps(3);
        assert_eq!(m.to_bits(), 0b0111);
        let m = ChannelMask::for_comps(4);
        assert_eq!(m.to_bits(), 0b1111);
    }

    #[test]
    fn test_channel_mask_to_bits() {
        let m = ChannelMask::new(0b1010);
        assert_eq!(m.to_bits(), 0b1010);
    }

    #[test]
    fn test_channel_mask_display() {
        assert_eq!(format!("{}", ChannelMask::for_comps(1)), ".r");
        assert_eq!(format!("{}", ChannelMask::for_comps(2)), ".rg");
        assert_eq!(format!("{}", ChannelMask::for_comps(3)), ".rgb");
        assert_eq!(format!("{}", ChannelMask::for_comps(4)), ".rgba");
    }

    #[test]
    fn test_image_dim_coord_comps() {
        assert_eq!(ImageDim::_1D.coord_comps(), 1);
        assert_eq!(ImageDim::_1DBuffer.coord_comps(), 1);
        assert_eq!(ImageDim::_1DArray.coord_comps(), 2);
        assert_eq!(ImageDim::_2D.coord_comps(), 2);
        assert_eq!(ImageDim::_2DArray.coord_comps(), 3);
        assert_eq!(ImageDim::_3D.coord_comps(), 3);
    }

    #[test]
    fn test_tex_ref_display() {
        assert_eq!(format!("{}", TexRef::Bound(0)), "tex[0]");
        assert_eq!(
            format!(
                "{}",
                TexRef::CBuf(TexCBufRef {
                    idx: 1,
                    offset: 0x10
                })
            ),
            "c[0x1][0x10]"
        );
        assert_eq!(format!("{}", TexRef::Bindless), "bindless");
    }

    #[test]
    fn test_tex_offset_mode_display() {
        assert_eq!(format!("{}", TexOffsetMode::None), "");
        assert_eq!(format!("{}", TexOffsetMode::AddOffI), ".aoffi");
        assert_eq!(format!("{}", TexOffsetMode::PerPx), ".ptp");
    }

    #[test]
    fn test_tex_query_display() {
        assert_eq!(format!("{}", TexQuery::Dimension), "dimension");
        assert_eq!(format!("{}", TexQuery::TextureType), "texture_type");
        assert_eq!(format!("{}", TexQuery::SamplerPos), "sampler_pos");
    }

    #[test]
    fn test_tex_lod_mode_display() {
        assert_eq!(format!("{}", TexLodMode::Auto), "");
        assert_eq!(format!("{}", TexLodMode::Zero), ".lz");
        assert_eq!(format!("{}", TexLodMode::Bias), ".lb");
        assert_eq!(format!("{}", TexLodMode::Lod), ".ll");
        assert_eq!(format!("{}", TexLodMode::Clamp), ".lc");
        assert_eq!(format!("{}", TexLodMode::BiasClamp), ".lb.lc");
    }
}
