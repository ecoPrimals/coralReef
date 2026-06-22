// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright © 2025-2026 ecoPrimals
// Derived from Collabora, Ltd. (2022)
//! Conversion and move op structs (F2F, F2I, I2F, I2I, FRnd, Mov, Movm).
//!
//! Permute/shuffle/predicate/reduction ops are in [`super::op_shuffle`].

use super::*;

#[repr(C)]
pub struct OpF2F {
    pub dst: Dst,
    pub src: Src,

    pub src_type: FloatType,
    pub dst_type: FloatType,
    pub rnd_mode: FRndMode,
    pub ftz: bool,
    /// For 16-bit down-conversions, place the result into the upper 16 bits of
    /// the destination register
    pub dst_high: bool,
    /// Round to the nearest integer rather than nearest float
    ///
    /// Not available on SM70+
    pub integer_rnd: bool,
}

impl OpF2F {
    pub fn is_high(&self) -> bool {
        if matches!(self.src_type, FloatType::F16) {
            // OpF2F with the same source and destination types is only allowed
            // pre-Volta and only with F32.
            assert!(!matches!(self.dst_type, FloatType::F16));

            matches!(self.src.swizzle, SrcSwizzle::Yy)
        } else if matches!(self.dst_type, FloatType::F16) {
            self.dst_high
        } else {
            assert!(!self.dst_high);
            false
        }
    }
}

impl AsSlice<Src> for OpF2F {
    type Attr = SrcType;

    fn as_slice(&self) -> &[Src] {
        std::slice::from_ref(&self.src)
    }

    fn as_mut_slice(&mut self) -> &mut [Src] {
        std::slice::from_mut(&mut self.src)
    }

    fn attrs(&self) -> SrcTypeList {
        let src_type = match self.src_type {
            FloatType::F16 => SrcType::F16v2,
            FloatType::F32 => SrcType::F32,
            FloatType::F64 => SrcType::F64,
        };
        SrcTypeList::Uniform(src_type)
    }
}

impl AsSlice<Dst> for OpF2F {
    type Attr = DstType;

    fn as_slice(&self) -> &[Dst] {
        std::slice::from_ref(&self.dst)
    }

    fn as_mut_slice(&mut self) -> &mut [Dst] {
        std::slice::from_mut(&mut self.dst)
    }

    fn attrs(&self) -> DstTypeList {
        let dst_type = match self.dst_type {
            FloatType::F16 => DstType::F16,
            FloatType::F32 => DstType::F32,
            FloatType::F64 => DstType::F64,
        };
        DstTypeList::Uniform(dst_type)
    }
}

impl DisplayOp for OpF2F {
    fn fmt_op(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "f2f")?;
        if self.ftz {
            write!(f, ".ftz")?;
        }
        if self.integer_rnd {
            write!(f, ".int")?;
        }
        if self.dst_high {
            write!(f, ".high")?;
        }
        write!(
            f,
            "{}{}{} {}",
            self.dst_type, self.src_type, self.rnd_mode, self.src,
        )
    }
}
impl_display_for_op!(OpF2F);

#[repr(C)]
#[derive(DstsAsSlice, SrcsAsSlice)]
pub struct OpF2FP {
    #[dst_type(GPR)]
    pub dst: Dst,

    #[src_type(ALU)]
    pub srcs: [Src; 2],

    pub rnd_mode: FRndMode,
}

impl DisplayOp for OpF2FP {
    fn fmt_op(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "f2fp.pack_ab")?;
        if self.rnd_mode != FRndMode::NearestEven {
            write!(f, "{}", self.rnd_mode)?;
        }
        write!(f, " {}, {}", self.srcs[0], self.srcs[1])
    }
}
impl_display_for_op!(OpF2FP);

#[repr(C)]
#[derive(DstsAsSlice)]
pub struct OpF2I {
    #[dst_type(GPR)]
    pub dst: Dst,

    pub src: Src,

    pub src_type: FloatType,
    pub dst_type: IntType,
    pub rnd_mode: FRndMode,
    pub ftz: bool,
}

impl AsSlice<Src> for OpF2I {
    type Attr = SrcType;

    fn as_slice(&self) -> &[Src] {
        std::slice::from_ref(&self.src)
    }

    fn as_mut_slice(&mut self) -> &mut [Src] {
        std::slice::from_mut(&mut self.src)
    }

    fn attrs(&self) -> SrcTypeList {
        let src_type = match self.src_type {
            FloatType::F16 => SrcType::F16,
            FloatType::F32 => SrcType::F32,
            FloatType::F64 => SrcType::F64,
        };
        SrcTypeList::Uniform(src_type)
    }
}

impl DisplayOp for OpF2I {
    fn fmt_op(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let ftz = if self.ftz { ".ftz" } else { "" };
        write!(
            f,
            "f2i{}{}{}{ftz} {}",
            self.dst_type, self.src_type, self.rnd_mode, self.src,
        )
    }
}
impl_display_for_op!(OpF2I);

#[repr(C)]
pub struct OpI2F {
    pub dst: Dst,
    pub src: Src,

    pub dst_type: FloatType,
    pub src_type: IntType,
    pub rnd_mode: FRndMode,
}

impl AsSlice<Src> for OpI2F {
    type Attr = SrcType;

    fn as_slice(&self) -> &[Src] {
        std::slice::from_ref(&self.src)
    }

    fn as_mut_slice(&mut self) -> &mut [Src] {
        std::slice::from_mut(&mut self.src)
    }

    fn attrs(&self) -> SrcTypeList {
        if self.src_type.bits() <= 32 {
            SrcTypeList::Uniform(SrcType::ALU)
        } else {
            SrcTypeList::Uniform(SrcType::GPR)
        }
    }
}

impl AsSlice<Dst> for OpI2F {
    type Attr = DstType;

    fn as_slice(&self) -> &[Dst] {
        std::slice::from_ref(&self.dst)
    }

    fn as_mut_slice(&mut self) -> &mut [Dst] {
        std::slice::from_mut(&mut self.dst)
    }

    fn attrs(&self) -> DstTypeList {
        let dst_type = match self.dst_type {
            FloatType::F16 => DstType::F16,
            FloatType::F32 => DstType::F32,
            FloatType::F64 => DstType::F64,
        };
        DstTypeList::Uniform(dst_type)
    }
}

impl DisplayOp for OpI2F {
    fn fmt_op(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "i2f{}{}{} {}",
            self.dst_type, self.src_type, self.rnd_mode, self.src,
        )
    }
}
impl_display_for_op!(OpI2F);

/// Not used on SM70+
#[repr(C)]
#[derive(SrcsAsSlice, DstsAsSlice)]
pub struct OpI2I {
    #[dst_type(GPR)]
    pub dst: Dst,

    #[src_type(ALU)]
    pub src: Src,

    pub src_type: IntType,
    pub dst_type: IntType,

    pub saturate: bool,
    pub abs: bool,
    pub neg: bool,
}

impl DisplayOp for OpI2I {
    fn fmt_op(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "i2i")?;
        if self.saturate {
            write!(f, ".sat ")?;
        }
        write!(f, "{}{} {}", self.dst_type, self.src_type, self.src)?;
        if self.abs {
            write!(f, ".abs")?;
        }
        if self.neg {
            write!(f, ".neg")?;
        }
        Ok(())
    }
}
impl_display_for_op!(OpI2I);

#[repr(C)]
#[derive(DstsAsSlice)]
pub struct OpFRnd {
    #[dst_type(F32)]
    pub dst: Dst,

    pub src: Src,

    pub dst_type: FloatType,
    pub src_type: FloatType,
    pub rnd_mode: FRndMode,
    pub ftz: bool,
}

impl AsSlice<Src> for OpFRnd {
    type Attr = SrcType;

    fn as_slice(&self) -> &[Src] {
        std::slice::from_ref(&self.src)
    }

    fn as_mut_slice(&mut self) -> &mut [Src] {
        std::slice::from_mut(&mut self.src)
    }

    fn attrs(&self) -> SrcTypeList {
        let src_type = match self.src_type {
            FloatType::F16 => SrcType::F16,
            FloatType::F32 => SrcType::F32,
            FloatType::F64 => SrcType::F64,
        };
        SrcTypeList::Uniform(src_type)
    }
}

impl DisplayOp for OpFRnd {
    fn fmt_op(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let ftz = if self.ftz { ".ftz" } else { "" };
        write!(
            f,
            "frnd{}{}{}{ftz} {}",
            self.dst_type, self.src_type, self.rnd_mode, self.src,
        )
    }
}
impl_display_for_op!(OpFRnd);

#[repr(C)]
#[derive(SrcsAsSlice, DstsAsSlice)]
pub struct OpMov {
    #[dst_type(GPR)]
    pub dst: Dst,

    #[src_type(ALU)]
    pub src: Src,

    pub quad_lanes: u8,
}

impl DisplayOp for OpMov {
    fn fmt_op(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.quad_lanes == 0xf {
            write!(f, "mov {}", self.src)
        } else {
            write!(f, "mov[{:#x}] {}", self.quad_lanes, self.src)
        }
    }
}
impl_display_for_op!(OpMov);

#[repr(C)]
#[derive(SrcsAsSlice, DstsAsSlice)]
pub struct OpMovm {
    pub dst: Dst,

    #[src_type(GPR)]
    pub src: Src,
}

impl DisplayOp for OpMovm {
    fn fmt_op(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "movm.16.m8n8.trans {}", self.src)
    }
}

impl_display_for_op!(OpMovm);

#[cfg(test)]
#[path = "op_conv_tests.rs"]
mod tests;
