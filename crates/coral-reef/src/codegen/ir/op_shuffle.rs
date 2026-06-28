// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright © 2025-2026 ecoPrimals
// Derived from Collabora, Ltd. (2022)
//! Permute, select, shuffle, predicate, and reduction op structs.
//!
//! Split from `op_conv` for code size compliance. These operations share
//! a common pattern: data rearrangement within or across lanes rather than
//! type conversion.

use super::*;

#[derive(Copy, Clone)]
pub struct PrmtSelByte(u8);

impl PrmtSelByte {
    pub const INVALID: Self = Self(u8::MAX);

    /// Whether this selector represents a valid hardware byte select (nibble ≤ 0xF).
    pub const fn is_valid(&self) -> bool {
        self.0 <= 0xf
    }

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
    /// Identity pass-through of src_a (bytes 3,2,1,0 → 3,2,1,0).
    pub const PASSTHROUGH_A: Self = Self(0x3210);

    /// Identity pass-through of src_b (bytes 7,6,5,4 → 3,2,1,0).
    pub const PASSTHROUGH_B: Self = Self(0x7654);

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
        write!(f, "sel {} {} {}", self.cond(), self.srcs[1], self.srcs[2])
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
impl_display_for_op!(OpPSetP);

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
        write!(f, "popc {}", self.src)
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

#[cfg(test)]
#[path = "op_shuffle_tests.rs"]
mod tests;
