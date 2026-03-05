// Copyright © 2022 Collabora, Ltd.
// SPDX-License-Identifier: MIT
//! Surface addressing, global array, and stride types.

use super::*;
#[derive(Copy, Clone, Debug)]
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
    pub fn unsigned(self) -> IMadSpSrcType {
        use IMadSpSrcType::*;
        match self {
            S32 => U32,
            S24 => U24,
            S16Hi => U16Hi,
            S16Lo => U16Lo,
            x => x,
        }
    }

    #[allow(dead_code)] // Used in hw_tests
    pub fn with_sign(self, sign: bool) -> Self {
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

    pub fn sign(self) -> bool {
        use IMadSpSrcType::*;
        match self {
            U32 | U24 | U16Hi | U16Lo => false,
            S32 | S24 | S16Hi | S16Lo => true,
        }
    }

    #[allow(dead_code)]
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
            IMadSpSrcType::U32 => "32",
            IMadSpSrcType::U24 => "24",
            IMadSpSrcType::U16Lo => "16h0",
            IMadSpSrcType::U16Hi => "16h1",
            _ => unreachable!(),
        };
        write!(f, "{}{}", sign, width)
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
            IMadSpMode::Explicit([a, b, c]) => write!(f, "{a}{b}{c}"),
            IMadSpMode::FromSrc1 => write!(f, ".sd"),
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
    fn fold(&self, _sm: &ShaderModelInfo, f: &mut OpFoldData<'_>) {
        let src0 = f.get_u32_src(self, &self.srcs[0]);
        let src1 = f.get_u32_src(self, &self.srcs[1]);
        let src2 = f.get_u32_src(self, &self.srcs[2]);

        let (src_type0, src_type1, src_type2) = match self.mode {
            IMadSpMode::Explicit([t0, t1, t2]) => (t0, t1, t2),
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

    /// Format for the loaded data, passed directly from the descriptor.
    #[src_type(GPR)]
    pub format: Src,

    /// This is not an address, but it's two registers that contain
    /// [addr >> 8, addr & 0xff].
    /// This works because addr >> 8 is 32-bits (GOB-aligned) and the
    /// rest 8-bits are extracted by the bit-field
    /// It's useful since in block-linear mode the lower bits and the higher
    /// bits are computed in different ways.
    #[src_type(SSA)]
    pub addr: Src,

    #[src_type(Pred)]
    pub out_of_bounds: Src,
}

impl DisplayOp for OpSuLdGa {
    fn fmt_op(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "suldga{}{} [{}] {} {}",
            self.mem_type, self.cache_op, self.addr, self.format, self.out_of_bounds
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

    #[src_type(GPR)]
    pub format: Src,

    #[src_type(SSA)]
    pub addr: Src,

    #[src_type(SSA)]
    pub data: Src,

    #[src_type(Pred)]
    pub out_of_bounds: Src,
}

impl DisplayOp for OpSuStGa {
    fn fmt_op(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "sustga{}{} [{}] {} {} {}",
            self.image_access, self.cache_op, self.addr, self.format, self.data, self.out_of_bounds,
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
            _ => Err("Unknown LdSt shift value {value}"),
        }
    }
}
