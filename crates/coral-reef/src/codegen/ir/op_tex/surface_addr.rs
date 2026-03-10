// SPDX-License-Identifier: AGPL-3.0-only
// Copyright © 2025-2026 ecoPrimals
// Derived from Collabora, Ltd. (2022)
//! Surface addressing, global array, and stride types.

use super::*;
#[derive(Copy, Clone, Debug, PartialEq)]
pub enum IMadSpSrcType {
    U32,
    U24,
    U16Hi,
    U16Lo,
    S32,
    S24,
    S16Hi,
    S16Lo,
}

impl IMadSpSrcType {
    pub const fn unsigned(self) -> Self {
        use IMadSpSrcType::*;
        match self {
            S32 => U32,
            S24 => U24,
            S16Hi => U16Hi,
            S16Lo => U16Lo,
            x => x,
        }
    }

    pub const fn with_sign(self, sign: bool) -> Self {
        use IMadSpSrcType::*;
        if !sign {
            return self.unsigned();
        }
        match self {
            U32 => S32,
            U24 => S24,
            U16Hi => S16Hi,
            U16Lo => S16Lo,
            x => x,
        }
    }

    pub const fn sign(self) -> bool {
        use IMadSpSrcType::*;
        match self {
            U32 | U24 | U16Hi | U16Lo => false,
            S32 | S24 | S16Hi | S16Lo => true,
        }
    }

    fn cast(&self, v: u32) -> i64 {
        use IMadSpSrcType::*;
        match self {
            U32 => v as i64,
            U24 => (v & 0x00ff_ffff) as i64,
            U16Lo => (v as u16) as i64,
            U16Hi => (v >> 16) as i64,
            S32 => (v as i32) as i64,
            S24 => (((v as i32) << 8) >> 8) as i64, // Sign extend
            S16Lo => (v as i16) as i64,
            S16Hi => ((v >> 16) as i16) as i64,
        }
    }
}

impl fmt::Display for IMadSpSrcType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let sign = if self.sign() { ".s" } else { ".u" };
        let width = match self.unsigned() {
            Self::U32 => "32",
            Self::U24 => "24",
            Self::U16Lo => "16h0",
            Self::U16Hi => "16h1",
            _ => unreachable!(),
        };
        write!(f, "{sign}{width}")
    }
}

#[derive(Clone, Copy, Debug)]
pub enum IMadSpMode {
    Explicit([IMadSpSrcType; 3]),
    // Parameters are loaded from src1 bits 26..32
    FromSrc1,
}

impl fmt::Display for IMadSpMode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Explicit([a, b, c]) => write!(f, "{a}{b}{c}"),
            Self::FromSrc1 => write!(f, ".sd"),
        }
    }
}

/// Kepler only
/// Extracted Integer Multiply and Add.
/// It does the same operation as an imad op, but it can extract the
/// sources from a subset of the register (only 32, 24 or 16 bits).
/// It can also do a "load parameters" mode where the modifiers are
/// loaded from the higher bits in src2 (check Foldable impl for details).
/// Limits: src1 can never be U32 or U16Hi,
///         src2 can never be U16Hi
///         src2 signedness is tied to src1 and src0 signedness,
///           if either is signed, src2 must be signed too.
#[repr(C)]
#[derive(SrcsAsSlice, DstsAsSlice, Clone)]
pub struct OpIMadSp {
    #[dst_type(GPR)]
    pub dst: Dst,

    #[src_type(ALU)]
    pub srcs: [Src; 3],

    pub mode: IMadSpMode,
}

impl Foldable for OpIMadSp {
    fn fold(&self, _sm: &dyn ShaderModel, f: &mut OpFoldData<'_>) {
        let src0 = f.get_u32_src(self, &self.srcs[0]);
        let src1 = f.get_u32_src(self, &self.srcs[1]);
        let src2 = f.get_u32_src(self, &self.srcs[2]);

        let (src_type0, src_type1, src_type2) = match self.mode {
            IMadSpMode::Explicit(types) => types.into(),
            IMadSpMode::FromSrc1 => {
                let params = &src1;

                let st2 = params.get_bit_range_u64(26..28) as usize;
                let st1 = params.get_bit_range_u64(28..30) as usize;
                let st0 = params.get_bit_range_u64(30..32) as usize;

                use IMadSpSrcType::*;
                let types0 = [U32, U24, U16Lo, U16Hi];
                let types1 = [U16Lo, U24, U16Lo, U24];
                let types2 = [U32, U24, U16Lo, U32];

                (
                    types0[st0].unsigned(),
                    types1[st1].unsigned(),
                    types2[st2].unsigned(),
                )
            }
        };

        let src0 = src_type0.cast(src0);
        let src1 = src_type1.cast(src1);
        let src2 = src_type2.cast(src2);

        f.set_u32_dst(self, &self.dst, (src0 * src1 + src2) as u32);
    }
}

impl DisplayOp for OpIMadSp {
    fn fmt_op(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "imadsp{} {} {} {}",
            self.mode, self.srcs[0], self.srcs[1], self.srcs[2]
        )
    }
}
impl_display_for_op!(OpIMadSp);

/// In SuGa ops, the address is always specified in two parts, the higher
/// part contains the base address without the lower 8 bits (base_addr >> 8),
/// while the lower part might contain either the missing 8 bits (U8) or
/// a full 32-bit offset that must not be shifted (U32).
///
/// In short:
/// U8 : real_address = (addr_hi << 8) + (addr_lo & 0xFF)
/// U32: real_address = (addr_hi << 8) + addr_lo
/// The signed variants do the same but with sign extension probably
#[derive(Clone, Copy)]
pub enum SuGaOffsetMode {
    U32,
    S32,
    U8,
    S8,
}

/// Kepler only
/// Load a pixel from an image, takes the pixel address and format as an
/// argument. Since the image coordinates are not present, the instruction
/// also needs an `out_of_bounds` predicate, when true it always load (0, 0, 0, 1)
#[repr(C)]
#[derive(SrcsAsSlice, DstsAsSlice)]
pub struct OpSuLdGa {
    pub dst: Dst,

    pub mem_type: MemType,
    pub offset_mode: SuGaOffsetMode,
    pub cache_op: LdCacheOp,

    #[src_types(GPR, SSA, Pred)]
    #[src_names(format, addr, out_of_bounds)]
    pub srcs: [Src; 3],
}

impl DisplayOp for OpSuLdGa {
    fn fmt_op(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "suldga{}{} [{}] {} {}",
            self.mem_type,
            self.cache_op,
            self.addr(),
            self.format(),
            self.out_of_bounds()
        )
    }
}
impl_display_for_op!(OpSuLdGa);

/// Kepler only
/// Store a pixel in an image, takes the pixel address and format as an
/// argument. Since the image coordinates are not present, the instruction
/// also needs an `out_of_bounds` predicate, when true, stores are ingored
#[repr(C)]
#[derive(SrcsAsSlice, DstsAsSlice)]
pub struct OpSuStGa {
    pub image_access: ImageAccess,
    pub offset_mode: SuGaOffsetMode,
    pub cache_op: StCacheOp,

    #[src_types(GPR, SSA, SSA, Pred)]
    #[src_names(format, addr, data, out_of_bounds)]
    pub srcs: [Src; 4],
}

impl DisplayOp for OpSuStGa {
    fn fmt_op(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "sustga{}{} [{}] {} {} {}",
            self.image_access,
            self.cache_op,
            self.addr(),
            self.format(),
            self.data(),
            self.out_of_bounds(),
        )
    }
}
impl_display_for_op!(OpSuStGa);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OffsetStride {
    X1 = 0,
    X4 = 2,
    X8 = 3,
    X16 = 4,
}

impl fmt::Display for OffsetStride {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let s = match self {
            Self::X1 => return Ok(()),
            Self::X4 => ".x4",
            Self::X8 => ".x8",
            Self::X16 => ".x16",
        };
        write!(f, "{s}")
    }
}

impl TryFrom<u8> for OffsetStride {
    type Error = &'static str;

    fn try_from(value: u8) -> Result<Self, Self::Error> {
        match value {
            0 => Ok(Self::X1),
            2 => Ok(Self::X4),
            3 => Ok(Self::X8),
            4 => Ok(Self::X16),
            _ => Err("Unknown LdSt shift value"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn zero_src() -> Src {
        Src::ZERO
    }

    #[test]
    fn test_imad_sp_src_type_display() {
        assert_eq!(format!("{}", IMadSpSrcType::U32), ".u32");
        assert_eq!(format!("{}", IMadSpSrcType::S32), ".s32");
        assert_eq!(format!("{}", IMadSpSrcType::U16Lo), ".u16h0");
        assert_eq!(format!("{}", IMadSpSrcType::U16Hi), ".u16h1");
    }

    #[test]
    fn test_imad_sp_src_type_unsigned_and_sign() {
        assert!(!IMadSpSrcType::U32.sign());
        assert!(IMadSpSrcType::S32.sign());
        assert!(matches!(IMadSpSrcType::S32.unsigned(), IMadSpSrcType::U32));
    }

    #[test]
    fn test_imad_sp_mode_display() {
        let mode =
            IMadSpMode::Explicit([IMadSpSrcType::U32, IMadSpSrcType::U24, IMadSpSrcType::U16Lo]);
        let s = format!("{mode}");
        assert!(s.contains(".u32"));
        assert!(s.contains(".u24"));
        assert!(s.contains(".u16h0"));

        assert_eq!(format!("{}", IMadSpMode::FromSrc1), ".sd");
    }

    #[test]
    fn test_op_imadsp_display() {
        let op = OpIMadSp {
            dst: Dst::None,
            srcs: [zero_src(), zero_src(), Src::new_imm_u32(1)],
            mode: IMadSpMode::Explicit([
                IMadSpSrcType::U32,
                IMadSpSrcType::U32,
                IMadSpSrcType::U32,
            ]),
        };
        let s = format!("{op}");
        assert!(s.contains("imadsp"));
    }

    #[test]
    fn test_op_suldga_display() {
        let op = OpSuLdGa {
            dst: Dst::None,
            mem_type: MemType::B32,
            offset_mode: SuGaOffsetMode::U32,
            cache_op: LdCacheOp::CacheGlobal,
            srcs: [zero_src(), zero_src(), Src::new_imm_bool(false)],
        };
        let s = format!("{op}");
        assert!(s.contains("suldga"));
        assert!(s.contains(".b32"));
        assert!(s.contains(".cg"));
    }

    #[test]
    fn test_op_sustga_display() {
        let op = OpSuStGa {
            image_access: ImageAccess::Formatted(ChannelMask::for_comps(4)),
            offset_mode: SuGaOffsetMode::U8,
            cache_op: StCacheOp::WriteBack,
            srcs: [
                zero_src(),
                zero_src(),
                Src::new_imm_u32(0),
                Src::new_imm_bool(true),
            ],
        };
        let s = format!("{op}");
        assert!(s.contains("sustga"));
        assert!(s.contains(".p.rgba"));
        assert!(s.contains(".wb"));
    }

    #[test]
    fn test_offset_stride_display() {
        assert_eq!(format!("{}", OffsetStride::X1), "");
        assert_eq!(format!("{}", OffsetStride::X4), ".x4");
        assert_eq!(format!("{}", OffsetStride::X8), ".x8");
        assert_eq!(format!("{}", OffsetStride::X16), ".x16");
    }

    #[test]
    fn test_offset_stride_try_from() {
        assert!(matches!(OffsetStride::try_from(0), Ok(OffsetStride::X1)));
        assert!(matches!(OffsetStride::try_from(2), Ok(OffsetStride::X4)));
        assert!(matches!(OffsetStride::try_from(3), Ok(OffsetStride::X8)));
        assert!(matches!(OffsetStride::try_from(4), Ok(OffsetStride::X16)));
        assert!(OffsetStride::try_from(1).is_err());
        assert!(OffsetStride::try_from(5).is_err());
    }
}
