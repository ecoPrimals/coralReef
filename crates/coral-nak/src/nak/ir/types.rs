// Copyright © 2022 Collabora, Ltd.
// SPDX-License-Identifier: MIT
//! Type enums used by instruction operands: comparison ops, float/int types,
//! texture types, memory types, interpolation modes, etc.

use std::fmt;
use std::ops::{BitAnd, BitOr, Not, Range};

use super::{ShaderModel, Src};

#[derive(Clone, Copy, Eq, Hash, PartialEq)]
pub enum PredSetOp {
    And,
    Or,
    Xor,
}

impl PredSetOp {
    #[allow(dead_code)]
    pub fn eval(&self, a: bool, b: bool) -> bool {
        match self {
            PredSetOp::And => a & b,
            PredSetOp::Or => a | b,
            PredSetOp::Xor => a ^ b,
        }
    }

    pub fn is_trivial(&self, accum: &Src) -> bool {
        if let Some(b) = accum.as_bool() {
            match self {
                PredSetOp::And => b,
                PredSetOp::Or | PredSetOp::Xor => !b,
            }
        } else {
            false
        }
    }
}

impl fmt::Display for PredSetOp {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            PredSetOp::And => write!(f, ".and"),
            PredSetOp::Or => write!(f, ".or"),
            PredSetOp::Xor => write!(f, ".xor"),
        }
    }
}

#[allow(dead_code)]
#[derive(Clone, Copy, Eq, Hash, PartialEq)]
pub enum FloatCmpOp {
    OrdEq,
    OrdNe,
    OrdLt,
    OrdLe,
    OrdGt,
    OrdGe,
    UnordEq,
    UnordNe,
    UnordLt,
    UnordLe,
    UnordGt,
    UnordGe,
    IsNum,
    IsNan,
}

impl FloatCmpOp {
    pub fn flip(self) -> FloatCmpOp {
        match self {
            FloatCmpOp::OrdEq | FloatCmpOp::OrdNe | FloatCmpOp::UnordEq | FloatCmpOp::UnordNe => {
                self
            }
            FloatCmpOp::OrdLt => FloatCmpOp::OrdGt,
            FloatCmpOp::OrdLe => FloatCmpOp::OrdGe,
            FloatCmpOp::OrdGt => FloatCmpOp::OrdLt,
            FloatCmpOp::OrdGe => FloatCmpOp::OrdLe,
            FloatCmpOp::UnordLt => FloatCmpOp::UnordGt,
            FloatCmpOp::UnordLe => FloatCmpOp::UnordGe,
            FloatCmpOp::UnordGt => FloatCmpOp::UnordLt,
            FloatCmpOp::UnordGe => FloatCmpOp::UnordLe,
            FloatCmpOp::IsNum | FloatCmpOp::IsNan => panic!("Cannot flip unop"),
        }
    }
}

impl fmt::Display for FloatCmpOp {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            FloatCmpOp::OrdEq => write!(f, ".eq"),
            FloatCmpOp::OrdNe => write!(f, ".ne"),
            FloatCmpOp::OrdLt => write!(f, ".lt"),
            FloatCmpOp::OrdLe => write!(f, ".le"),
            FloatCmpOp::OrdGt => write!(f, ".gt"),
            FloatCmpOp::OrdGe => write!(f, ".ge"),
            FloatCmpOp::UnordEq => write!(f, ".equ"),
            FloatCmpOp::UnordNe => write!(f, ".neu"),
            FloatCmpOp::UnordLt => write!(f, ".ltu"),
            FloatCmpOp::UnordLe => write!(f, ".leu"),
            FloatCmpOp::UnordGt => write!(f, ".gtu"),
            FloatCmpOp::UnordGe => write!(f, ".geu"),
            FloatCmpOp::IsNum => write!(f, ".num"),
            FloatCmpOp::IsNan => write!(f, ".nan"),
        }
    }
}

#[allow(dead_code)]
#[derive(Clone, Copy, Eq, Hash, PartialEq)]
pub enum IntCmpOp {
    False,
    True,
    Eq,
    Ne,
    Lt,
    Le,
    Gt,
    Ge,
}

impl IntCmpOp {
    pub fn flip(self) -> IntCmpOp {
        match self {
            IntCmpOp::False | IntCmpOp::True | IntCmpOp::Eq | IntCmpOp::Ne => self,
            IntCmpOp::Lt => IntCmpOp::Gt,
            IntCmpOp::Le => IntCmpOp::Ge,
            IntCmpOp::Gt => IntCmpOp::Lt,
            IntCmpOp::Ge => IntCmpOp::Le,
        }
    }
}

impl fmt::Display for IntCmpOp {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            IntCmpOp::False => write!(f, ".f"),
            IntCmpOp::True => write!(f, ".t"),
            IntCmpOp::Eq => write!(f, ".eq"),
            IntCmpOp::Ne => write!(f, ".ne"),
            IntCmpOp::Lt => write!(f, ".lt"),
            IntCmpOp::Le => write!(f, ".le"),
            IntCmpOp::Gt => write!(f, ".gt"),
            IntCmpOp::Ge => write!(f, ".ge"),
        }
    }
}

#[derive(Clone, Copy, Eq, Hash, PartialEq)]
pub enum IntCmpType {
    U32,
    I32,
}

impl IntCmpType {
    pub fn is_signed(&self) -> bool {
        match self {
            IntCmpType::U32 => false,
            IntCmpType::I32 => true,
        }
    }
}

impl fmt::Display for IntCmpType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            IntCmpType::U32 => write!(f, ".u32"),
            IntCmpType::I32 => write!(f, ".i32"),
        }
    }
}

#[derive(Clone, Copy, Eq, Hash, PartialEq)]
pub enum LogicOp2 {
    And,
    Or,
    Xor,
    PassB,
}

impl fmt::Display for LogicOp2 {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            LogicOp2::And => write!(f, "and"),
            LogicOp2::Or => write!(f, "or"),
            LogicOp2::Xor => write!(f, "xor"),
            LogicOp2::PassB => write!(f, "pass_b"),
        }
    }
}

impl LogicOp2 {
    pub fn to_lut(self) -> LogicOp3 {
        match self {
            LogicOp2::And => LogicOp3::new_lut(&|x, y, _| x & y),
            LogicOp2::Or => LogicOp3::new_lut(&|x, y, _| x | y),
            LogicOp2::Xor => LogicOp3::new_lut(&|x, y, _| x ^ y),
            LogicOp2::PassB => LogicOp3::new_lut(&|_, b, _| b),
        }
    }
}

#[derive(Clone, Copy, Eq, Hash, PartialEq)]
pub struct LogicOp3 {
    pub lut: u8,
}

impl LogicOp3 {
    pub const SRC_MASKS: [u8; 3] = [0xf0, 0xcc, 0xaa];

    #[inline]
    pub fn new_lut<F: Fn(u8, u8, u8) -> u8>(f: &F) -> LogicOp3 {
        LogicOp3 {
            lut: f(
                LogicOp3::SRC_MASKS[0],
                LogicOp3::SRC_MASKS[1],
                LogicOp3::SRC_MASKS[2],
            ),
        }
    }

    pub fn new_const(val: bool) -> LogicOp3 {
        LogicOp3 {
            lut: if val { !0 } else { 0 },
        }
    }

    pub fn src_used(&self, src_idx: usize) -> bool {
        let mask = LogicOp3::SRC_MASKS[src_idx];
        let shift = LogicOp3::SRC_MASKS[src_idx].trailing_zeros();
        self.lut & !mask != (self.lut >> shift) & !mask
    }

    pub fn fix_src(&mut self, src_idx: usize, val: bool) {
        let mask = LogicOp3::SRC_MASKS[src_idx];
        let shift = LogicOp3::SRC_MASKS[src_idx].trailing_zeros();
        if val {
            let t_bits = self.lut & mask;
            self.lut = t_bits | (t_bits >> shift);
        } else {
            let f_bits = self.lut & !mask;
            self.lut = (f_bits << shift) | f_bits;
        }
    }

    pub fn invert_src(&mut self, src_idx: usize) {
        let mask = LogicOp3::SRC_MASKS[src_idx];
        let shift = LogicOp3::SRC_MASKS[src_idx].trailing_zeros();
        let t_bits = self.lut & mask;
        let f_bits = self.lut & !mask;
        self.lut = (f_bits << shift) | (t_bits >> shift);
    }

    pub fn eval<T: BitAnd<Output = T> + BitOr<Output = T> + Copy + Not<Output = T>>(
        &self,
        x: T,
        y: T,
        z: T,
    ) -> T {
        let mut res = x & !x; // zero
        if (self.lut & (1 << 0)) != 0 {
            res = res | (!x & !y & !z);
        }
        if (self.lut & (1 << 1)) != 0 {
            res = res | (!x & !y & z);
        }
        if (self.lut & (1 << 2)) != 0 {
            res = res | (!x & y & !z);
        }
        if (self.lut & (1 << 3)) != 0 {
            res = res | (!x & y & z);
        }
        if (self.lut & (1 << 4)) != 0 {
            res = res | (x & !y & !z);
        }
        if (self.lut & (1 << 5)) != 0 {
            res = res | (x & !y & z);
        }
        if (self.lut & (1 << 6)) != 0 {
            res = res | (x & y & !z);
        }
        if (self.lut & (1 << 7)) != 0 {
            res = res | (x & y & z);
        }
        res
    }
}

impl fmt::Display for LogicOp3 {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "LUT[{:#x}]", self.lut)
    }
}

#[derive(Clone, Copy, Eq, Hash, PartialEq)]
pub enum FloatType {
    F16,
    F32,
    F64,
}

impl FloatType {
    pub fn from_bits(bytes: usize) -> FloatType {
        match bytes {
            16 => FloatType::F16,
            32 => FloatType::F32,
            64 => FloatType::F64,
            _ => panic!("Invalid float type size"),
        }
    }

    pub fn bits(&self) -> usize {
        match self {
            FloatType::F16 => 16,
            FloatType::F32 => 32,
            FloatType::F64 => 64,
        }
    }
}

impl fmt::Display for FloatType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            FloatType::F16 => write!(f, ".f16"),
            FloatType::F32 => write!(f, ".f32"),
            FloatType::F64 => write!(f, ".f64"),
        }
    }
}

#[derive(Clone, Copy, Eq, Hash, PartialEq)]
pub enum FRndMode {
    NearestEven,
    NegInf,
    PosInf,
    Zero,
}

impl fmt::Display for FRndMode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            FRndMode::NearestEven => write!(f, ".re"),
            FRndMode::NegInf => write!(f, ".rm"),
            FRndMode::PosInf => write!(f, ".rp"),
            FRndMode::Zero => write!(f, ".rz"),
        }
    }
}

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
            TexRef::Bound(idx) => write!(f, "tex[{idx}]"),
            TexRef::CBuf(TexCBufRef { idx, offset }) => {
                write!(f, "c[{idx:#x}][{offset:#x}]")
            }
            TexRef::Bindless => write!(f, "bindless"),
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
            TexDim::_1D => write!(f, ".1d"),
            TexDim::Array1D => write!(f, ".a1d"),
            TexDim::_2D => write!(f, ".2d"),
            TexDim::Array2D => write!(f, ".a2d"),
            TexDim::_3D => write!(f, ".3d"),
            TexDim::Cube => write!(f, ".cube"),
            TexDim::ArrayCube => write!(f, ".acube"),
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
    pub fn is_explicit_lod(&self) -> bool {
        match self {
            TexLodMode::Auto | TexLodMode::Bias | TexLodMode::Clamp | TexLodMode::BiasClamp => {
                false
            }
            TexLodMode::Zero | TexLodMode::Lod => true,
        }
    }
}

impl fmt::Display for TexLodMode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            TexLodMode::Auto => write!(f, ""),
            TexLodMode::Zero => write!(f, ".lz"),
            TexLodMode::Bias => write!(f, ".lb"),
            TexLodMode::Lod => write!(f, ".ll"),
            TexLodMode::Clamp => write!(f, ".lc"),
            TexLodMode::BiasClamp => write!(f, ".lb.lc"),
        }
    }
}

/// Derivative behavior for tex ops and FSwzAdd
///
/// The descriptions here may not be wholly accurate as they come from cobbling
/// together a bunch of pieces.  This is my (Faith's) best understanding of how
/// these things work.
#[allow(dead_code)]
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
            TexDerivMode::Auto => Ok(()),
            TexDerivMode::NonDivergent => write!(f, ".ndv"),
            TexDerivMode::ForceDivergent => write!(f, ".fdv"),
            TexDerivMode::DerivXY => write!(f, ".dxy"),
        }
    }
}

#[derive(Clone, Copy, Eq, PartialEq)]
pub struct ChannelMask(u8);

impl ChannelMask {
    pub fn new(mask: u8) -> Self {
        assert!(mask != 0 && (mask & !0xf) == 0);
        ChannelMask(mask)
    }

    pub fn for_comps(comps: u8) -> Self {
        assert!(comps > 0 && comps <= 4);
        ChannelMask((1 << comps) - 1)
    }

    pub fn to_bits(self) -> u8 {
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
            TexOffsetMode::None => write!(f, ""),
            TexOffsetMode::AddOffI => write!(f, ".aoffi"),
            TexOffsetMode::PerPx => write!(f, ".ptp"),
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
            TexQuery::Dimension => write!(f, "dimension"),
            TexQuery::TextureType => write!(f, "texture_type"),
            TexQuery::SamplerPos => write!(f, "sampler_pos"),
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
    pub fn coord_comps(&self) -> u8 {
        match self {
            ImageDim::_1D | ImageDim::_1DBuffer => 1,
            ImageDim::_1DArray | ImageDim::_2D => 2,
            ImageDim::_2DArray | ImageDim::_3D => 3,
        }
    }
}

impl fmt::Display for ImageDim {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ImageDim::_1D => write!(f, ".1d"),
            ImageDim::_1DBuffer => write!(f, ".buf"),
            ImageDim::_1DArray => write!(f, ".a1d"),
            ImageDim::_2D => write!(f, ".2d"),
            ImageDim::_2DArray => write!(f, ".a2d"),
            ImageDim::_3D => write!(f, ".3d"),
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum IntType {
    U8,
    I8,
    U16,
    I16,
    U32,
    I32,
    U64,
    I64,
}

impl IntType {
    pub fn from_bits(bits: usize, is_signed: bool) -> IntType {
        match bits {
            8 => {
                if is_signed {
                    IntType::I8
                } else {
                    IntType::U8
                }
            }
            16 => {
                if is_signed {
                    IntType::I16
                } else {
                    IntType::U16
                }
            }
            32 => {
                if is_signed {
                    IntType::I32
                } else {
                    IntType::U32
                }
            }
            64 => {
                if is_signed {
                    IntType::I64
                } else {
                    IntType::U64
                }
            }
            _ => panic!("Invalid integer type size"),
        }
    }

    pub fn is_signed(&self) -> bool {
        match self {
            IntType::U8 | IntType::U16 | IntType::U32 | IntType::U64 => false,
            IntType::I8 | IntType::I16 | IntType::I32 | IntType::I64 => true,
        }
    }

    pub fn bits(&self) -> usize {
        match self {
            IntType::U8 | IntType::I8 => 8,
            IntType::U16 | IntType::I16 => 16,
            IntType::U32 | IntType::I32 => 32,
            IntType::U64 | IntType::I64 => 64,
        }
    }
}

impl fmt::Display for IntType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            IntType::U8 => write!(f, ".u8"),
            IntType::I8 => write!(f, ".i8"),
            IntType::U16 => write!(f, ".u16"),
            IntType::I16 => write!(f, ".i16"),
            IntType::U32 => write!(f, ".u32"),
            IntType::I32 => write!(f, ".i32"),
            IntType::U64 => write!(f, ".u64"),
            IntType::I64 => write!(f, ".i64"),
        }
    }
}

#[derive(Clone, Copy, Eq, Hash, PartialEq)]
pub enum MemAddrType {
    A32,
    A64,
}

impl fmt::Display for MemAddrType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            MemAddrType::A32 => write!(f, ".a32"),
            MemAddrType::A64 => write!(f, ".a64"),
        }
    }
}

#[derive(Clone, Copy, Eq, Hash, PartialEq)]
pub enum MemType {
    U8,
    I8,
    U16,
    I16,
    B32,
    B64,
    B128,
}

impl MemType {
    pub fn from_size(size: u8, is_signed: bool) -> MemType {
        match size {
            1 => {
                if is_signed {
                    MemType::I8
                } else {
                    MemType::U8
                }
            }
            2 => {
                if is_signed {
                    MemType::I16
                } else {
                    MemType::U16
                }
            }
            4 => MemType::B32,
            8 => MemType::B64,
            16 => MemType::B128,
            _ => panic!("Invalid memory load/store size"),
        }
    }

    #[allow(dead_code)]
    pub fn bits(&self) -> usize {
        match self {
            MemType::U8 | MemType::I8 => 8,
            MemType::U16 | MemType::I16 => 16,
            MemType::B32 => 32,
            MemType::B64 => 64,
            MemType::B128 => 128,
        }
    }
}

impl fmt::Display for MemType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            MemType::U8 => write!(f, ".u8"),
            MemType::I8 => write!(f, ".i8"),
            MemType::U16 => write!(f, ".u16"),
            MemType::I16 => write!(f, ".i16"),
            MemType::B32 => write!(f, ".b32"),
            MemType::B64 => write!(f, ".b64"),
            MemType::B128 => write!(f, ".b128"),
        }
    }
}

#[derive(Clone, Copy, Eq, Hash, PartialEq)]
pub enum MemOrder {
    Constant,
    Weak,
    Strong(MemScope),
}

impl fmt::Display for MemOrder {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            MemOrder::Constant => write!(f, ".constant"),
            MemOrder::Weak => write!(f, ".weak"),
            MemOrder::Strong(scope) => write!(f, ".strong{scope}"),
        }
    }
}

#[derive(Clone, Copy, Eq, Hash, PartialEq)]
pub enum MemScope {
    CTA,
    GPU,
    System,
}

impl fmt::Display for MemScope {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            MemScope::CTA => write!(f, ".cta"),
            MemScope::GPU => write!(f, ".gpu"),
            MemScope::System => write!(f, ".sys"),
        }
    }
}

#[derive(Clone, Copy, Eq, Hash, PartialEq)]
pub enum MemSpace {
    Global(MemAddrType),
    Local,
    Shared,
}

impl MemSpace {
    pub fn addr_type(&self) -> MemAddrType {
        match self {
            MemSpace::Global(t) => *t,
            MemSpace::Local | MemSpace::Shared => MemAddrType::A32,
        }
    }
}

impl fmt::Display for MemSpace {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            MemSpace::Global(t) => write!(f, ".global{t}"),
            MemSpace::Local => write!(f, ".local"),
            MemSpace::Shared => write!(f, ".shared"),
        }
    }
}

#[allow(dead_code)]
#[derive(Clone, Copy, Eq, Hash, PartialEq)]
pub enum MemEvictionPriority {
    First,
    Normal,
    Last,
    LastUse,
    Unchanged,
    NoAllocate,
}

impl fmt::Display for MemEvictionPriority {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            MemEvictionPriority::First => write!(f, ".ef"),
            MemEvictionPriority::Normal => Ok(()),
            MemEvictionPriority::Last => write!(f, ".el"),
            MemEvictionPriority::LastUse => write!(f, ".lu"),
            MemEvictionPriority::Unchanged => write!(f, ".eu"),
            MemEvictionPriority::NoAllocate => write!(f, ".na"),
        }
    }
}

/// Memory load cache ops used by Kepler
#[allow(dead_code)]
#[expect(clippy::enum_variant_names)]
#[derive(Clone, Copy, Default, Eq, Hash, PartialEq)]
pub enum LdCacheOp {
    #[default]
    CacheAll,
    CacheGlobal,
    /// This cache mode not officially documented by NVIDIA.  What we do know is
    /// that the Cuda C programming gude says:
    ///
    /// > The read-only data cache load function is only supported by devices
    /// > of compute capability 5.0 and higher.
    /// > ```c
    /// > T __ldg(const T* address);
    /// > ```
    ///
    /// and we know that `__ldg()` compiles to `ld.global.nc` in PTX which
    /// compiles to `ld.ci`.  The PTX 5.0 docs say:
    ///
    /// > Load register variable `d` from the location specified by the source
    /// > address operand `a` in the global state space, and optionally cache in
    /// > non-coherent texture cache. Since the cache is non-coherent, the data
    /// > should be read-only within the kernel's process.
    ///
    /// Since `.nc` means "non-coherent", the name "incoherent" seems about
    /// right.  The quote above also seems to imply that these loads got loaded
    /// through the texture cache but we don't fully understand the implications
    /// of that.
    CacheIncoherent,
    CacheStreaming,
    CacheInvalidate,
}

impl fmt::Display for LdCacheOp {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            LdCacheOp::CacheAll => write!(f, ".ca"),
            LdCacheOp::CacheGlobal => write!(f, ".cg"),
            LdCacheOp::CacheIncoherent => write!(f, ".ci"),
            LdCacheOp::CacheStreaming => write!(f, ".cs"),
            LdCacheOp::CacheInvalidate => write!(f, ".cv"),
        }
    }
}

impl LdCacheOp {
    pub fn select(
        sm: &dyn ShaderModel,
        space: MemSpace,
        order: MemOrder,
        _eviction_priority: MemEvictionPriority,
    ) -> Self {
        match space {
            MemSpace::Global(_) => match order {
                MemOrder::Constant => {
                    if sm.sm() >= 50 {
                        // This is undocumented in the CUDA docs but NVIDIA uses
                        // it for constant loads.
                        LdCacheOp::CacheIncoherent
                    } else {
                        LdCacheOp::CacheAll
                    }
                }
                MemOrder::Strong(MemScope::System) => LdCacheOp::CacheInvalidate,
                _ => {
                    // From the CUDA 10.2 docs:
                    //
                    //    "The default load instruction cache operation is
                    //    ld.ca, which allocates cache lines in all levels (L1
                    //    and L2) with normal eviction policy. Global data is
                    //    coherent at the L2 level, but multiple L1 caches are
                    //    not coherent for global data. If one thread stores to
                    //    global memory via one L1 cache, and a second thread
                    //    loads that address via a second L1 cache with ld.ca,
                    //    the second thread may get stale L1 cache data"
                    //
                    // and
                    //
                    //    "L1 caching in Kepler GPUs is reserved only for local
                    //    memory accesses, such as register spills and stack
                    //    data. Global loads are cached in L2 only (or in the
                    //    Read-Only Data Cache)."
                    //
                    // We follow suit and use CacheGlobal for all global memory
                    // access on Kepler.  On Maxwell, it appears safe to use
                    // CacheAll for everything.
                    if sm.sm() >= 50 {
                        LdCacheOp::CacheAll
                    } else {
                        LdCacheOp::CacheGlobal
                    }
                }
            },
            MemSpace::Local | MemSpace::Shared => LdCacheOp::CacheAll,
        }
    }
}

/// Memory store cache ops used by Kepler
#[allow(dead_code)]
#[derive(Clone, Copy, Default, Eq, Hash, PartialEq)]
pub enum StCacheOp {
    #[default]
    WriteBack,
    CacheGlobal,
    CacheStreaming,
    WriteThrough,
}

impl fmt::Display for StCacheOp {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            StCacheOp::WriteBack => write!(f, ".wb"),
            StCacheOp::CacheGlobal => write!(f, ".cg"),
            StCacheOp::CacheStreaming => write!(f, ".cs"),
            StCacheOp::WriteThrough => write!(f, ".wt"),
        }
    }
}

impl StCacheOp {
    pub fn select(
        sm: &dyn ShaderModel,
        space: MemSpace,
        order: MemOrder,
        _eviction_priority: MemEvictionPriority,
    ) -> Self {
        match space {
            MemSpace::Global(_) => match order {
                MemOrder::Constant => panic!("Cannot store to constant"),
                MemOrder::Strong(MemScope::System) => StCacheOp::WriteThrough,
                _ => {
                    // See the corresponding comment in LdCacheOp::select()
                    if sm.sm() >= 50 {
                        StCacheOp::WriteBack
                    } else {
                        StCacheOp::CacheGlobal
                    }
                }
            },
            MemSpace::Local | MemSpace::Shared => StCacheOp::WriteBack,
        }
    }
}

#[derive(Clone)]
pub struct MemAccess {
    pub mem_type: MemType,
    pub space: MemSpace,
    pub order: MemOrder,
    pub eviction_priority: MemEvictionPriority,
}

impl fmt::Display for MemAccess {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{}{}{}{}",
            self.space, self.order, self.eviction_priority, self.mem_type,
        )
    }
}

impl MemAccess {
    pub fn ld_cache_op(&self, sm: &dyn ShaderModel) -> LdCacheOp {
        LdCacheOp::select(sm, self.space, self.order, self.eviction_priority)
    }

    pub fn st_cache_op(&self, sm: &dyn ShaderModel) -> StCacheOp {
        StCacheOp::select(sm, self.space, self.order, self.eviction_priority)
    }
}

#[allow(dead_code)]
#[derive(Clone, Copy, Eq, Hash, PartialEq)]
pub enum AtomType {
    F16x2,
    U32,
    I32,
    F32,
    U64,
    I64,
    F64,
}

impl AtomType {
    pub fn F(bits: u8) -> AtomType {
        match bits {
            16 => panic!("16-bit float atomics not yet supported"),
            32 => AtomType::F32,
            64 => AtomType::F64,
            _ => panic!("Invalid float atomic type"),
        }
    }

    pub fn U(bits: u8) -> AtomType {
        match bits {
            32 => AtomType::U32,
            64 => AtomType::U64,
            _ => panic!("Invalid uint atomic type"),
        }
    }

    pub fn I(bits: u8) -> AtomType {
        match bits {
            32 => AtomType::I32,
            64 => AtomType::I64,
            _ => panic!("Invalid int atomic type"),
        }
    }

    pub fn bits(&self) -> usize {
        match self {
            AtomType::F16x2 | AtomType::F32 | AtomType::U32 | AtomType::I32 => 32,
            AtomType::U64 | AtomType::I64 | AtomType::F64 => 64,
        }
    }

    pub fn is_float(&self) -> bool {
        match self {
            AtomType::F16x2 | AtomType::F32 | AtomType::F64 => true,
            AtomType::U32 | AtomType::I32 | AtomType::U64 | AtomType::I64 => false,
        }
    }
}

impl fmt::Display for AtomType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            AtomType::F16x2 => write!(f, ".f16x2"),
            AtomType::U32 => write!(f, ".u32"),
            AtomType::I32 => write!(f, ".i32"),
            AtomType::F32 => write!(f, ".f32"),
            AtomType::U64 => write!(f, ".u64"),
            AtomType::I64 => write!(f, ".i64"),
            AtomType::F64 => write!(f, ".f64"),
        }
    }
}

#[derive(Clone, Copy, Eq, Hash, PartialEq)]
pub enum AtomCmpSrc {
    /// The cmpr value is passed as a separate source
    Separate,
    /// The cmpr value is packed in with the data with cmpr coming first
    Packed,
}

#[allow(dead_code)]
#[derive(Clone, Copy, Eq, Hash, PartialEq)]
pub enum AtomOp {
    Add,
    Min,
    Max,
    Inc,
    Dec,
    And,
    Or,
    Xor,
    Exch,
    CmpExch(AtomCmpSrc),
}

impl AtomOp {
    pub fn is_reduction(&self) -> bool {
        match self {
            AtomOp::Add
            | AtomOp::Min
            | AtomOp::Max
            | AtomOp::Inc
            | AtomOp::Dec
            | AtomOp::And
            | AtomOp::Or
            | AtomOp::Xor => true,
            AtomOp::Exch | AtomOp::CmpExch(_) => false,
        }
    }
}

impl fmt::Display for AtomOp {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            AtomOp::Add => write!(f, ".add"),
            AtomOp::Min => write!(f, ".min"),
            AtomOp::Max => write!(f, ".max"),
            AtomOp::Inc => write!(f, ".inc"),
            AtomOp::Dec => write!(f, ".dec"),
            AtomOp::And => write!(f, ".and"),
            AtomOp::Or => write!(f, ".or"),
            AtomOp::Xor => write!(f, ".xor"),
            AtomOp::Exch => write!(f, ".exch"),
            AtomOp::CmpExch(AtomCmpSrc::Separate) => write!(f, ".cmpexch"),
            AtomOp::CmpExch(AtomCmpSrc::Packed) => write!(f, ".cmpexch.packed"),
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
            InterpFreq::Pass => write!(f, ".pass"),
            InterpFreq::PassMulW => write!(f, ".pass_mul_w"),
            InterpFreq::Constant => write!(f, ".constant"),
            InterpFreq::State => write!(f, ".state"),
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
            InterpLoc::Default => Ok(()),
            InterpLoc::Centroid => write!(f, ".centroid"),
            InterpLoc::Offset => write!(f, ".offset"),
        }
    }
}
