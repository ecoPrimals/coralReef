// SPDX-License-Identifier: AGPL-3.0-only
// Copyright © 2025-2026 ecoPrimals
// Derived from Collabora, Ltd. (2022)
//! Conversion, move, shuffle, predicate, and reduction op structs.

#![allow(clippy::wildcard_imports)]

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
        write!(f, " {}, {}", self.srcs[0], self.srcs[1],)
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
        write!(f, "{}{} {}", self.dst_type, self.src_type, self.src,)?;
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

#[derive(Copy, Clone)]
pub struct PrmtSelByte(u8);

impl PrmtSelByte {
    pub const INVALID: Self = Self(u8::MAX);

    pub fn new(src_idx: usize, byte_idx: usize, msb: bool) -> Self {
        assert!(src_idx < 2);
        assert!(byte_idx < 4);

        let mut nib = 0;
        nib |= (src_idx as u8) << 2;
        nib |= byte_idx as u8;
        if msb {
            nib |= 0x8;
        }
        Self(nib)
    }

    pub fn src(&self) -> usize {
        ((self.0 >> 2) & 0x1).into()
    }

    pub fn byte(&self) -> usize {
        (self.0 & 0x3).into()
    }

    pub const fn msb(&self) -> bool {
        (self.0 & 0x8) != 0
    }

    pub fn fold_u32(&self, u: u32) -> u8 {
        let mut sb = (u >> (self.byte() * 8)) as u8;
        if self.msb() {
            sb = ((sb as i8) >> 7) as u8;
        }
        sb
    }
}

#[derive(Clone, Copy, Eq, Hash, PartialEq)]
pub struct PrmtSel(pub u16);

impl PrmtSel {
    pub fn new(bytes: [PrmtSelByte; 4]) -> Self {
        let mut sel = 0;
        for i in 0..4 {
            assert!(bytes[i].0 <= 0xf);
            sel |= u16::from(bytes[i].0) << (i * 4);
        }
        Self(sel)
    }

    pub fn get(&self, byte_idx: usize) -> PrmtSelByte {
        assert!(byte_idx < 4);
        PrmtSelByte(((self.0 >> (byte_idx * 4)) & 0xf) as u8)
    }
}

#[allow(dead_code, reason = "ISA variant reserved for future encoding support")]
#[derive(Clone, Copy, Eq, Hash, PartialEq)]
pub enum PrmtMode {
    Index,
    Forward4Extract,
    Backward4Extract,
    Replicate8,
    EdgeClampLeft,
    EdgeClampRight,
    Replicate16,
}

impl fmt::Display for PrmtMode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Index => Ok(()),
            Self::Forward4Extract => write!(f, ".f4e"),
            Self::Backward4Extract => write!(f, ".b4e"),
            Self::Replicate8 => write!(f, ".rc8"),
            Self::EdgeClampLeft | Self::EdgeClampRight => write!(f, ".ecl"),
            Self::Replicate16 => write!(f, ".rc16"),
        }
    }
}

#[repr(C)]
#[derive(Clone, SrcsAsSlice, DstsAsSlice)]
/// Permutes `srcs` into `dst` using `selection`.
pub struct OpPrmt {
    #[dst_type(GPR)]
    pub dst: Dst,

    #[src_types(ALU, ALU, ALU)]
    #[src_names(src_a, src_b, sel)]
    pub srcs: [Src; 3],

    pub mode: PrmtMode,
}

impl OpPrmt {
    pub fn get_sel(&self) -> Option<PrmtSel> {
        // EVOLUTION(feature): PrmtSel for non-Index modes (Index is the only one used).
        if self.mode != PrmtMode::Index {
            return None;
        }

        self.sel().as_u32(SrcType::ALU).map(|sel| {
            // The top 16 bits are ignored
            PrmtSel(sel as u16)
        })
    }

    /// Reduces the sel immediate, if any.
    pub fn reduce_sel_imm(&mut self) {
        assert!(self.sel().modifier.is_none());
        if let SrcRef::Imm32(sel) = &mut self.sel_mut().reference {
            // Only the bottom 16 bits matter anyway
            *sel &= 0xffff;
        }
    }

    pub fn as_u32(&self) -> Option<u32> {
        let sel = self.get_sel()?;

        let mut imm = 0_u32;
        for b in 0..4 {
            let sel_byte = sel.get(b);
            let src_u32 = self.srcs[sel_byte.src()].as_u32(SrcType::ALU)?;

            let sb = sel_byte.fold_u32(src_u32);
            imm |= u32::from(sb) << (b * 8);
        }
        Some(imm)
    }
}

impl Foldable for OpPrmt {
    fn fold(&self, _sm: &dyn ShaderModel, f: &mut OpFoldData<'_>) {
        let srcs = [
            f.get_u32_src(self, &self.srcs[0]),
            f.get_u32_src(self, &self.srcs[1]),
        ];
        let sel = f.get_u32_src(self, self.sel());

        assert!(self.mode == PrmtMode::Index);
        let sel = PrmtSel(sel as u16);

        let mut dst = 0_u32;
        for b in 0..4 {
            let sel_byte = sel.get(b);
            let src = srcs[sel_byte.src()];
            let sb = sel_byte.fold_u32(src);
            dst |= u32::from(sb) << (b * 8);
        }

        f.set_u32_dst(self, &self.dst, dst);
    }
}

impl DisplayOp for OpPrmt {
    fn fmt_op(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "prmt{} {} [{}] {}",
            self.mode,
            self.srcs[0],
            self.sel(),
            self.srcs[1],
        )
    }
}
impl_display_for_op!(OpPrmt);

#[repr(C)]
#[derive(SrcsAsSlice, DstsAsSlice)]
pub struct OpSel {
    #[dst_type(GPR)]
    pub dst: Dst,

    #[src_types(Pred, ALU, ALU)]
    #[src_names(cond, src_a, src_b)]
    pub srcs: [Src; 3],
}

impl DisplayOp for OpSel {
    fn fmt_op(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "sel {} {} {}", self.cond(), self.srcs[1], self.srcs[2],)
    }
}
impl_display_for_op!(OpSel);

#[repr(C)]
#[derive(Clone, SrcsAsSlice, DstsAsSlice)]
pub struct OpSgxt {
    #[dst_type(GPR)]
    pub dst: Dst,

    #[src_types(ALU, ALU)]
    #[src_names(a, bits)]
    pub srcs: [Src; 2],

    pub signed: bool,
}

impl DisplayOp for OpSgxt {
    fn fmt_op(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let modifier = if self.signed { "" } else { ".u32" };
        write!(f, "sgxt{} {} {}", modifier, self.a(), self.bits())
    }
}
impl_display_for_op!(OpSgxt);

impl Foldable for OpSgxt {
    fn fold(&self, _sm: &dyn ShaderModel, f: &mut OpFoldData<'_>) {
        let a = f.get_u32_src(self, self.a());
        let bits = f.get_u32_src(self, self.bits());

        let dst = if bits >= 32 {
            a
        } else if bits == 0 {
            0
        } else {
            let shift = 32 - bits;
            let a = a << shift;
            if self.signed {
                let a = a as i32;
                (a >> shift) as u32
            } else {
                a >> shift
            }
        };
        f.set_u32_dst(self, &self.dst, dst);
    }
}

#[repr(C)]
#[derive(SrcsAsSlice, DstsAsSlice)]
pub struct OpShfl {
    #[dst_types(GPR, Pred)]
    #[dst_names(dst, in_bounds)]
    pub dsts: [Dst; 2],

    #[src_types(SSA, ALU, ALU)]
    #[src_names(src, lane, c)]
    pub srcs: [Src; 3],

    pub op: ShflOp,
}

impl OpShfl {
    /// Reduces the lane and c immediates, if any.  The hardware only uses
    /// some of the bits of `lane` and `c` and ignores the rest.  This method
    /// masks off the unused bits and ensures that any immediate values fit
    /// in the limited encoding space in the instruction.
    pub fn reduce_lane_c_imm(&mut self) {
        debug_assert!(self.lane().modifier.is_none());
        if let SrcRef::Imm32(lane) = &mut self.lane_mut().reference {
            *lane &= 0x1f;
        }

        debug_assert!(self.c().modifier.is_none());
        if let SrcRef::Imm32(c) = &mut self.c_mut().reference {
            *c &= 0x1f1f;
        }
    }
}

impl DisplayOp for OpShfl {
    fn fmt_op(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "shfl.{} {} {} {}",
            self.op,
            self.src(),
            self.lane(),
            self.c()
        )
    }
}
impl_display_for_op!(OpShfl);

#[repr(C)]
#[derive(SrcsAsSlice, DstsAsSlice)]
pub struct OpPLop3 {
    #[dst_type(Pred)]
    pub dsts: [Dst; 2],

    #[src_type(Pred)]
    pub srcs: [Src; 3],

    pub ops: [LogicOp3; 2],
}

impl DisplayOp for OpPLop3 {
    fn fmt_dsts(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} {}", self.dsts[0], self.dsts[1])
    }

    fn fmt_op(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "plop3 {} {} {} {} {}",
            self.srcs[0], self.srcs[1], self.srcs[2], self.ops[0], self.ops[1],
        )
    }
}
impl_display_for_op!(OpPLop3);

#[repr(C)]
#[derive(Clone, SrcsAsSlice, DstsAsSlice)]
pub struct OpPSetP {
    #[dst_type(Pred)]
    pub dsts: [Dst; 2],

    pub ops: [PredSetOp; 2],

    #[src_type(Pred)]
    pub srcs: [Src; 3],
}

impl Foldable for OpPSetP {
    fn fold(&self, _sm: &dyn ShaderModel, f: &mut OpFoldData<'_>) {
        let srcs = [
            f.get_pred_src(self, &self.srcs[0]),
            f.get_pred_src(self, &self.srcs[1]),
            f.get_pred_src(self, &self.srcs[2]),
        ];

        let tmp = self.ops[0].eval(srcs[0], srcs[1]);
        let dst0 = self.ops[1].eval(srcs[2], tmp);

        let tmp = self.ops[0].eval(!srcs[0], srcs[1]);
        let dst1 = self.ops[1].eval(srcs[2], tmp);

        f.set_pred_dst(self, &self.dsts[0], dst0);
        f.set_pred_dst(self, &self.dsts[1], dst1);
    }
}

impl DisplayOp for OpPSetP {
    fn fmt_op(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "psetp{}{} {} {} {}",
            self.ops[0], self.ops[1], self.srcs[0], self.srcs[1], self.srcs[2],
        )
    }
}

#[repr(C)]
#[derive(Clone, SrcsAsSlice, DstsAsSlice)]
pub struct OpPopC {
    #[dst_type(GPR)]
    pub dst: Dst,

    #[src_type(B32)]
    pub src: Src,
}

impl Foldable for OpPopC {
    fn fold(&self, _sm: &dyn ShaderModel, f: &mut OpFoldData<'_>) {
        let src = f.get_u32_bnot_src(self, &self.src);
        let dst = src.count_ones();
        f.set_u32_dst(self, &self.dst, dst);
    }
}

impl DisplayOp for OpPopC {
    fn fmt_op(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "popc {}", self.src,)
    }
}
impl_display_for_op!(OpPopC);

#[repr(C)]
#[derive(SrcsAsSlice, DstsAsSlice)]
pub struct OpR2UR {
    #[dst_type(GPR)]
    pub dst: Dst,

    #[src_type(GPR)]
    pub src: Src,
}

impl DisplayOp for OpR2UR {
    fn fmt_op(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "r2ur {}", self.src)
    }
}
impl_display_for_op!(OpR2UR);

#[derive(Copy, Clone, PartialEq, Eq)]
pub enum ReduxOp {
    And,
    Or,
    Xor,
    Sum,
    Min(IntCmpType),
    Max(IntCmpType),
}

impl fmt::Display for ReduxOp {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::And => write!(f, ".and"),
            Self::Or => write!(f, ".or"),
            Self::Xor => write!(f, ".xor"),
            Self::Sum => write!(f, ".sum"),
            Self::Min(cmp) => write!(f, ".min{cmp}"),
            Self::Max(cmp) => write!(f, ".max{cmp}"),
        }
    }
}

#[repr(C)]
#[derive(SrcsAsSlice, DstsAsSlice)]
pub struct OpRedux {
    #[dst_type(GPR)]
    pub dst: Dst,

    #[src_type(GPR)]
    pub src: Src,

    pub op: ReduxOp,
}

impl DisplayOp for OpRedux {
    fn fmt_op(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "redux{} {}", self.op, self.src)
    }
}
impl_display_for_op!(OpRedux);
