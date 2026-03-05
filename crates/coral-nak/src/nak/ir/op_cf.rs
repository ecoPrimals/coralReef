// Copyright © 2022 Collabora, Ltd.
// SPDX-License-Identifier: MIT
//! Control flow, barrier, and system instruction op structs.

#![allow(clippy::wildcard_imports)]

use super::*;
#[repr(C)]
#[derive(SrcsAsSlice, DstsAsSlice)]
pub struct OpCCtl {
    pub op: CCtlOp,

    pub mem_space: MemSpace,

    #[src_type(GPR)]
    pub addr: Src,

    pub addr_offset: i32,
}

impl DisplayOp for OpCCtl {
    fn fmt_op(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "cctl{}", self.mem_space)?;
        if !self.op.is_all() {
            write!(f, " [{}", self.addr)?;
            if self.addr_offset > 0 {
                write!(f, "+{:#x}", self.addr_offset)?;
            }
            write!(f, "]")?;
        }
        Ok(())
    }
}
impl_display_for_op!(OpCCtl);

#[repr(C)]
#[derive(SrcsAsSlice, DstsAsSlice)]
pub struct OpMemBar {
    pub scope: MemScope,
}

impl DisplayOp for OpMemBar {
    fn fmt_op(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "membar.sc.{}", self.scope)
    }
}
impl_display_for_op!(OpMemBar);

#[repr(C)]
#[derive(SrcsAsSlice, DstsAsSlice)]
pub struct OpBClear {
    pub dst: Dst,
}

impl DisplayOp for OpBClear {
    fn fmt_op(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "bclear")
    }
}
impl_display_for_op!(OpBClear);

#[repr(C)]
#[derive(SrcsAsSlice, DstsAsSlice)]
pub struct OpBMov {
    pub dst: Dst,
    pub src: Src,
    pub clear: bool,
}

impl DisplayOp for OpBMov {
    fn fmt_op(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "bmov.32")?;
        if self.clear {
            write!(f, ".clear")?;
        }
        write!(f, " {}", self.src)
    }
}
impl_display_for_op!(OpBMov);

#[repr(C)]
#[derive(SrcsAsSlice, DstsAsSlice)]
pub struct OpBreak {
    #[dst_type(Bar)]
    pub bar_out: Dst,

    #[src_type(Bar)]
    pub bar_in: Src,

    #[src_type(Pred)]
    pub cond: Src,
}

impl DisplayOp for OpBreak {
    fn fmt_op(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "break {} {}", self.bar_in, self.cond)
    }
}
impl_display_for_op!(OpBreak);

#[repr(C)]
#[derive(SrcsAsSlice, DstsAsSlice)]
pub struct OpBSSy {
    #[dst_type(Bar)]
    pub bar_out: Dst,

    #[src_type(Pred)]
    pub bar_in: Src,

    #[src_type(Pred)]
    pub cond: Src,

    pub target: Label,
}

impl DisplayOp for OpBSSy {
    fn fmt_op(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "bssy {} {} {}", self.bar_in, self.cond, self.target)
    }
}
impl_display_for_op!(OpBSSy);

#[repr(C)]
#[derive(SrcsAsSlice, DstsAsSlice)]
pub struct OpBSync {
    #[src_type(Bar)]
    pub bar: Src,

    #[src_type(Pred)]
    pub cond: Src,
}

impl DisplayOp for OpBSync {
    fn fmt_op(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "bsync {} {}", self.bar, self.cond)
    }
}
impl_display_for_op!(OpBSync);

/// Takes the branch when the guard predicate and all sources evaluate to true.
#[repr(C)]
#[derive(Clone, SrcsAsSlice, DstsAsSlice)]
pub struct OpBra {
    pub target: Label,

    /// Can be a UPred if uniform
    // TODO: actually .u has another form with an additional UPred input.
    #[src_type(Pred)]
    pub cond: Src,
}

impl DisplayOp for OpBra {
    fn fmt_op(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "bra {} {}", self.cond, self.target)
    }
}
impl_display_for_op!(OpBra);

#[repr(C)]
#[derive(SrcsAsSlice, DstsAsSlice)]
pub struct OpSSy {
    pub target: Label,
}

impl DisplayOp for OpSSy {
    fn fmt_op(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "ssy {}", self.target)
    }
}
impl_display_for_op!(OpSSy);

#[repr(C)]
#[derive(SrcsAsSlice, DstsAsSlice)]
pub struct OpSync {
    pub target: Label,
}

impl DisplayOp for OpSync {
    fn fmt_op(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "sync {}", self.target)
    }
}
impl_display_for_op!(OpSync);

#[repr(C)]
#[derive(SrcsAsSlice, DstsAsSlice)]
pub struct OpBrk {
    pub target: Label,
}

impl DisplayOp for OpBrk {
    fn fmt_op(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "brk {}", self.target)
    }
}
impl_display_for_op!(OpBrk);

#[repr(C)]
#[derive(SrcsAsSlice, DstsAsSlice)]
pub struct OpPBk {
    pub target: Label,
}

impl DisplayOp for OpPBk {
    fn fmt_op(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "pbk {}", self.target)
    }
}
impl_display_for_op!(OpPBk);

#[repr(C)]
#[derive(SrcsAsSlice, DstsAsSlice)]
pub struct OpCont {
    pub target: Label,
}

impl DisplayOp for OpCont {
    fn fmt_op(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "cont {}", self.target)
    }
}
impl_display_for_op!(OpCont);

#[repr(C)]
#[derive(SrcsAsSlice, DstsAsSlice)]
pub struct OpPCnt {
    pub target: Label,
}

impl DisplayOp for OpPCnt {
    fn fmt_op(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "pcnt {}", self.target)
    }
}
impl_display_for_op!(OpPCnt);

#[repr(C)]
#[derive(Clone, SrcsAsSlice, DstsAsSlice)]
pub struct OpExit {}

impl DisplayOp for OpExit {
    fn fmt_op(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "exit")
    }
}
impl_display_for_op!(OpExit);

#[repr(C)]
#[derive(SrcsAsSlice, DstsAsSlice)]
pub struct OpWarpSync {
    pub mask: u32,
}

impl DisplayOp for OpWarpSync {
    fn fmt_op(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "warpsync 0x{:x}", self.mask)
    }
}
impl_display_for_op!(OpWarpSync);

#[repr(C)]
#[derive(SrcsAsSlice, DstsAsSlice)]
pub struct OpBar {}

impl DisplayOp for OpBar {
    fn fmt_op(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "bar.sync")
    }
}
impl_display_for_op!(OpBar);

/// Instruction only used on Kepler(A|B).
/// Kepler has explicit dependency tracking for texture loads.
/// When a texture load is executed, it is put on some kind of FIFO queue
/// for later execution.
/// Before the results of a texture are used we need to wait on the queue,
/// texdepbar waits until the queue has at most `textures_left` elements.
#[repr(C)]
#[derive(SrcsAsSlice, DstsAsSlice)]
pub struct OpTexDepBar {
    pub textures_left: u8,
}

impl OpTexDepBar {
    /// Maximum value of textures_left
    ///
    /// The maximum encodable value is 63.  However, nvcc starts emitting
    /// TEXDEPBAR 0x3e as soon as it hits 62 texture instructions.
    pub const MAX_TEXTURES_LEFT: u8 = 62;
}

impl DisplayOp for OpTexDepBar {
    fn fmt_op(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "texdepbar {}", self.textures_left)
    }
}
impl_display_for_op!(OpTexDepBar);
