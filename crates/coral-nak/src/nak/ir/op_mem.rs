// Copyright © 2022 Collabora, Ltd.
// SPDX-License-Identifier: MIT
//! Memory, load, store, and atomic instruction op structs.

#![allow(clippy::wildcard_imports)]

use super::*;
#[repr(C)]
#[derive(SrcsAsSlice, DstsAsSlice)]
pub struct OpLd {
    pub dst: Dst,

    #[src_type(GPR)]
    pub addr: Src,

    pub offset: i32,
    pub stride: OffsetStride,
    pub access: MemAccess,
}

impl DisplayOp for OpLd {
    fn fmt_op(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "ld{} [{}{}", self.access, self.addr, self.stride)?;
        if self.offset > 0 {
            write!(f, "+{:#x}", self.offset)?;
        }
        write!(f, "]")
    }
}
impl_display_for_op!(OpLd);

#[allow(dead_code)]
#[derive(Clone, Copy, Eq, Hash, PartialEq)]
pub enum LdcMode {
    Indexed,
    IndexedLinear,
    IndexedSegmented,
    IndexedSegmentedLinear,
}

impl fmt::Display for LdcMode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            LdcMode::Indexed => Ok(()),
            LdcMode::IndexedLinear => write!(f, ".il"),
            LdcMode::IndexedSegmented => write!(f, ".is"),
            LdcMode::IndexedSegmentedLinear => write!(f, ".isl"),
        }
    }
}

#[repr(C)]
#[derive(Clone, SrcsAsSlice, DstsAsSlice)]
pub struct OpLdc {
    pub dst: Dst,

    #[src_type(ALU)]
    pub cb: Src,

    #[src_type(GPR)]
    pub offset: Src,

    pub mode: LdcMode,
    pub mem_type: MemType,
}

impl DisplayOp for OpLdc {
    fn fmt_op(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let SrcRef::CBuf(cb) = &self.cb.src_ref else {
            panic!("Not a cbuf");
        };
        write!(f, "ldc{}{} {}[", self.mode, self.mem_type, cb.buf)?;
        if self.offset.is_zero() {
            write!(f, "+{:#x}", cb.offset)?;
        } else if cb.offset == 0 {
            write!(f, "{}", self.offset)?;
        } else {
            write!(f, "{}+{:#x}", self.offset, cb.offset)?;
        }
        write!(f, "]")
    }
}
impl_display_for_op!(OpLdc);

#[derive(Clone, Copy, Eq, PartialEq)]
pub enum LdsmSize {
    M8N8,
    MT8N8,
}

impl fmt::Display for LdsmSize {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            LdsmSize::M8N8 => write!(f, "m8n8"),
            LdsmSize::MT8N8 => write!(f, "m8n8.trans"),
        }
    }
}

#[repr(C)]
#[derive(SrcsAsSlice, DstsAsSlice)]
pub struct OpLdsm {
    #[dst_type(Vec)]
    pub dst: Dst,

    pub mat_size: LdsmSize,
    pub mat_count: u8,

    #[src_type(SSA)]
    pub addr: Src,

    pub offset: i32,
}

impl DisplayOp for OpLdsm {
    fn fmt_op(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "ldsm.16.{}.x{} [{}",
            self.mat_size, self.mat_count, self.addr,
        )?;
        if self.offset > 0 {
            write!(f, "+{:#x}", self.offset)?;
        }
        write!(f, "]")
    }
}

impl_display_for_op!(OpLdsm);

/// Used for Kepler to implement shared atomics.
/// In addition to the load, it tries to lock the address,
/// Kepler hardware has (1024?) hardware mutex locks.
#[repr(C)]
#[derive(SrcsAsSlice, DstsAsSlice)]
pub struct OpLdSharedLock {
    pub dst: Dst,
    #[dst_type(Pred)]
    pub locked: Dst,

    #[src_type(GPR)]
    pub addr: Src,

    pub offset: i32,
    pub mem_type: MemType,
}

impl DisplayOp for OpLdSharedLock {
    fn fmt_op(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "ldslk{} [{}", self.mem_type, self.addr)?;
        if self.offset > 0 {
            write!(f, "+{:#x}", self.offset)?;
        }
        write!(f, "]")
    }
}
impl_display_for_op!(OpLdSharedLock);

#[repr(C)]
#[derive(SrcsAsSlice, DstsAsSlice)]
pub struct OpSt {
    #[src_type(GPR)]
    pub addr: Src,

    #[src_type(SSA)]
    pub data: Src,

    pub offset: i32,
    pub stride: OffsetStride,
    pub access: MemAccess,
}

impl DisplayOp for OpSt {
    fn fmt_op(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "st{} [{}{}", self.access, self.addr, self.stride)?;
        if self.offset > 0 {
            write!(f, "+{:#x}", self.offset)?;
        }
        write!(f, "] {}", self.data)
    }
}
impl_display_for_op!(OpSt);

/// Used for Kepler to implement shared atomics.
/// It checks that the address is still properly locked, performs the
/// store operation and unlocks the previously unlocked address.
#[repr(C)]
#[derive(SrcsAsSlice, DstsAsSlice)]
pub struct OpStSCheckUnlock {
    #[dst_type(Pred)]
    pub locked: Dst,

    #[src_type(GPR)]
    pub addr: Src,
    #[src_type(SSA)]
    pub data: Src,

    pub offset: i32,
    pub mem_type: MemType,
}

impl DisplayOp for OpStSCheckUnlock {
    fn fmt_op(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "stscul{} [{}", self.mem_type, self.addr)?;
        if self.offset > 0 {
            write!(f, "+{:#x}", self.offset)?;
        }
        write!(f, "] {}", self.data)
    }
}
impl_display_for_op!(OpStSCheckUnlock);

#[repr(C)]
#[derive(SrcsAsSlice, DstsAsSlice)]
pub struct OpAtom {
    pub dst: Dst,

    #[src_type(GPR)]
    pub addr: Src,

    #[src_type(GPR)]
    pub cmpr: Src,

    #[src_type(SSA)]
    pub data: Src,

    pub atom_op: AtomOp,
    pub atom_type: AtomType,

    pub addr_offset: i32,
    pub addr_stride: OffsetStride,

    pub mem_space: MemSpace,
    pub mem_order: MemOrder,
    pub mem_eviction_priority: MemEvictionPriority,
}

impl DisplayOp for OpAtom {
    fn fmt_op(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "atom{}{}{}{}{}",
            self.atom_op,
            self.atom_type,
            self.mem_space,
            self.mem_order,
            self.mem_eviction_priority,
        )?;
        write!(f, " [")?;
        if !self.addr.is_zero() {
            write!(f, "{}{}", self.addr, self.addr_stride)?;
        }
        if self.addr_offset > 0 {
            if !self.addr.is_zero() {
                write!(f, "+")?;
            }
            write!(f, "{:#x}", self.addr_offset)?;
        }
        write!(f, "]")?;
        if self.atom_op == AtomOp::CmpExch(AtomCmpSrc::Separate) {
            write!(f, " {}", self.cmpr)?;
        }
        write!(f, " {}", self.data)
    }
}
impl_display_for_op!(OpAtom);

#[repr(C)]
#[derive(SrcsAsSlice, DstsAsSlice)]
pub struct OpAL2P {
    pub dst: Dst,

    #[src_type(GPR)]
    pub offset: Src,

    pub addr: u16,
    pub comps: u8,
    pub output: bool,
}

impl DisplayOp for OpAL2P {
    fn fmt_op(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "al2p")?;
        if self.output {
            write!(f, ".o")?;
        }
        write!(f, " a[{:#x}", self.addr)?;
        if !self.offset.is_zero() {
            write!(f, "+{}", self.offset)?;
        }
        write!(f, "]")
    }
}
impl_display_for_op!(OpAL2P);

#[repr(C)]
#[derive(SrcsAsSlice, DstsAsSlice)]
pub struct OpALd {
    pub dst: Dst,

    #[src_type(GPR)]
    pub vtx: Src,

    #[src_type(GPR)]
    pub offset: Src,

    pub addr: u16,
    pub comps: u8,
    pub patch: bool,
    pub output: bool,
    pub phys: bool,
}

impl DisplayOp for OpALd {
    fn fmt_op(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "ald")?;
        if self.output {
            write!(f, ".o")?;
        }
        if self.patch {
            write!(f, ".p")?;
        }
        if self.phys {
            write!(f, ".phys")?;
        }
        write!(f, " a")?;
        if !self.vtx.is_zero() {
            write!(f, "[{}]", self.vtx)?;
        }
        write!(f, "[{:#x}", self.addr)?;
        if !self.offset.is_zero() {
            write!(f, "+{}", self.offset)?;
        }
        write!(f, "]")
    }
}
impl_display_for_op!(OpALd);

#[repr(C)]
#[derive(SrcsAsSlice, DstsAsSlice)]
pub struct OpASt {
    #[src_type(GPR)]
    pub vtx: Src,

    #[src_type(GPR)]
    pub offset: Src,

    #[src_type(SSA)]
    pub data: Src,

    pub addr: u16,
    pub comps: u8,
    pub patch: bool,
    pub phys: bool,
}

impl DisplayOp for OpASt {
    fn fmt_op(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "ast")?;
        if self.patch {
            write!(f, ".p")?;
        }
        if self.phys {
            write!(f, ".phys")?;
        }
        write!(f, " a")?;
        if !self.vtx.is_zero() {
            write!(f, "[{}]", self.vtx)?;
        }
        write!(f, "[{:#x}", self.addr)?;
        if !self.offset.is_zero() {
            write!(f, "+{}", self.offset)?;
        }
        write!(f, "] {}", self.data)
    }
}
impl_display_for_op!(OpASt);

#[repr(C)]
#[derive(SrcsAsSlice, DstsAsSlice)]
pub struct OpIpa {
    pub dst: Dst,
    pub addr: u16,
    pub freq: InterpFreq,
    pub loc: InterpLoc,
    pub inv_w: Src,
    pub offset: Src,
}

impl DisplayOp for OpIpa {
    fn fmt_op(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "ipa{}{} a[{:#x}] {}",
            self.freq, self.loc, self.addr, self.inv_w
        )?;
        if self.loc == InterpLoc::Offset {
            write!(f, " {}", self.offset)?;
        }
        Ok(())
    }
}
impl_display_for_op!(OpIpa);

#[repr(C)]
#[derive(SrcsAsSlice, DstsAsSlice)]
pub struct OpLdTram {
    pub dst: Dst,
    pub addr: u16,
    pub use_c: bool,
}

impl DisplayOp for OpLdTram {
    fn fmt_op(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "ldtram")?;
        if self.use_c {
            write!(f, ".c")?;
        } else {
            write!(f, ".ab")?;
        }
        write!(f, " a[{:#x}]", self.addr)?;
        Ok(())
    }
}
impl_display_for_op!(OpLdTram);

#[allow(dead_code)]
#[derive(Copy, Clone, Debug)]
pub enum CCtlOp {
    Qry1, // Only available pre-Volta
    PF1,
    PF1_5, // Only available pre-Volta
    PF2,
    WB,
    IV,
    IVAll,
    RS,
    RSLB,   // Only available pre-Volta
    IVAllP, // Only available on Volta+
    WBAll,  // Only available on Volta+
    WBAllP, // Only available on Volta+
}

impl CCtlOp {
    pub fn is_all(&self) -> bool {
        match self {
            CCtlOp::Qry1
            | CCtlOp::PF1
            | CCtlOp::PF1_5
            | CCtlOp::PF2
            | CCtlOp::WB
            | CCtlOp::IV
            | CCtlOp::RS
            | CCtlOp::RSLB => false,
            CCtlOp::IVAll | CCtlOp::IVAllP | CCtlOp::WBAll | CCtlOp::WBAllP => true,
        }
    }
}

impl fmt::Display for CCtlOp {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            CCtlOp::Qry1 => write!(f, "qry1"),
            CCtlOp::PF1 => write!(f, "pf1"),
            CCtlOp::PF1_5 => write!(f, "pf1.5"),
            CCtlOp::PF2 => write!(f, "pf2"),
            CCtlOp::WB => write!(f, "wb"),
            CCtlOp::IV => write!(f, "iv"),
            CCtlOp::IVAll => write!(f, "ivall"),
            CCtlOp::RS => write!(f, "rs"),
            CCtlOp::RSLB => write!(f, "rslb"),
            CCtlOp::IVAllP => write!(f, "ivallp"),
            CCtlOp::WBAll => write!(f, "wball"),
            CCtlOp::WBAllP => write!(f, "wballp"),
        }
    }
}
