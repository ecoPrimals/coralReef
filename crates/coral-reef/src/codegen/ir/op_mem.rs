// SPDX-License-Identifier: AGPL-3.0-only
// Copyright © 2025-2026 ecoPrimals
// Derived from Collabora, Ltd. (2022)
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

#[allow(dead_code, reason = "ISA variant reserved for future encoding support")]
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
            Self::Indexed => Ok(()),
            Self::IndexedLinear => write!(f, ".il"),
            Self::IndexedSegmented => write!(f, ".is"),
            Self::IndexedSegmentedLinear => write!(f, ".isl"),
        }
    }
}

#[repr(C)]
#[derive(Clone, SrcsAsSlice, DstsAsSlice)]
pub struct OpLdc {
    pub dst: Dst,

    #[src_types(ALU, GPR)]
    #[src_names(cb, offset)]
    pub srcs: [Src; 2],

    pub mode: LdcMode,
    pub mem_type: MemType,
}

impl DisplayOp for OpLdc {
    fn fmt_op(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let SrcRef::CBuf(cb) = &self.cb().reference else {
            panic!("ICE: Not a cbuf");
        };
        write!(f, "ldc{}{} {}[", self.mode, self.mem_type, cb.buf)?;
        if self.offset().is_zero() {
            write!(f, "+{:#x}", cb.offset)?;
        } else if cb.offset == 0 {
            write!(f, "{}", self.offset())?;
        } else {
            write!(f, "{}+{:#x}", self.offset(), cb.offset)?;
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
            Self::M8N8 => write!(f, "m8n8"),
            Self::MT8N8 => write!(f, "m8n8.trans"),
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
    #[dst_types(Vec, Pred)]
    #[dst_names(dst, locked)]
    pub dsts: [Dst; 2],

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
    #[src_types(GPR, SSA)]
    #[src_names(addr, data)]
    pub srcs: [Src; 2],

    pub offset: i32,
    pub stride: OffsetStride,
    pub access: MemAccess,
}

impl DisplayOp for OpSt {
    fn fmt_op(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "st{} [{}{}", self.access, self.addr(), self.stride)?;
        if self.offset > 0 {
            write!(f, "+{:#x}", self.offset)?;
        }
        write!(f, "] {}", self.data())
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

    #[src_types(GPR, SSA)]
    #[src_names(addr, data)]
    pub srcs: [Src; 2],

    pub offset: i32,
    pub mem_type: MemType,
}

impl DisplayOp for OpStSCheckUnlock {
    fn fmt_op(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "stscul{} [{}", self.mem_type, self.addr())?;
        if self.offset > 0 {
            write!(f, "+{:#x}", self.offset)?;
        }
        write!(f, "] {}", self.data())
    }
}
impl_display_for_op!(OpStSCheckUnlock);

#[repr(C)]
#[derive(SrcsAsSlice, DstsAsSlice)]
pub struct OpAtom {
    pub dst: Dst,

    #[src_types(GPR, GPR, SSA)]
    #[src_names(addr, cmpr, data)]
    pub srcs: [Src; 3],

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
        if !self.addr().is_zero() {
            write!(f, "{}{}", self.addr(), self.addr_stride)?;
        }
        if self.addr_offset > 0 {
            if !self.addr().is_zero() {
                write!(f, "+")?;
            }
            write!(f, "{:#x}", self.addr_offset)?;
        }
        write!(f, "]")?;
        if self.atom_op == AtomOp::CmpExch(AtomCmpSrc::Separate) {
            write!(f, " {}", self.cmpr())?;
        }
        write!(f, " {}", self.data())
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

    #[src_types(GPR, GPR)]
    #[src_names(vtx, offset)]
    pub srcs: [Src; 2],

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
        if !self.vtx().is_zero() {
            write!(f, "[{}]", self.vtx())?;
        }
        write!(f, "[{:#x}", self.addr)?;
        if !self.offset().is_zero() {
            write!(f, "+{}", self.offset())?;
        }
        write!(f, "]")
    }
}
impl_display_for_op!(OpALd);

#[repr(C)]
#[derive(SrcsAsSlice, DstsAsSlice)]
pub struct OpASt {
    #[src_types(GPR, GPR, SSA)]
    #[src_names(vtx, offset, data)]
    pub srcs: [Src; 3],

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
        if !self.vtx().is_zero() {
            write!(f, "[{}]", self.vtx())?;
        }
        write!(f, "[{:#x}", self.addr)?;
        if !self.offset().is_zero() {
            write!(f, "+{}", self.offset())?;
        }
        write!(f, "] {}", self.data())
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

    #[src_types(GPR, GPR)]
    #[src_names(inv_w, offset)]
    pub srcs: [Src; 2],
}

impl DisplayOp for OpIpa {
    fn fmt_op(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "ipa{}{} a[{:#x}] {}",
            self.freq,
            self.loc,
            self.addr,
            self.inv_w()
        )?;
        if self.loc == InterpLoc::Offset {
            write!(f, " {}", self.offset())?;
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

#[allow(
    dead_code,
    reason = "ISA variant reserved for future cache control encoding"
)]
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
            Self::Qry1
            | Self::PF1
            | Self::PF1_5
            | Self::PF2
            | Self::WB
            | Self::IV
            | Self::RS
            | Self::RSLB => false,
            Self::IVAll | Self::IVAllP | Self::WBAll | Self::WBAllP => true,
        }
    }
}

impl fmt::Display for CCtlOp {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Qry1 => write!(f, "qry1"),
            Self::PF1 => write!(f, "pf1"),
            Self::PF1_5 => write!(f, "pf1.5"),
            Self::PF2 => write!(f, "pf2"),
            Self::WB => write!(f, "wb"),
            Self::IV => write!(f, "iv"),
            Self::IVAll => write!(f, "ivall"),
            Self::RS => write!(f, "rs"),
            Self::RSLB => write!(f, "rslb"),
            Self::IVAllP => write!(f, "ivallp"),
            Self::WBAll => write!(f, "wball"),
            Self::WBAllP => write!(f, "wballp"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn zero_src() -> Src {
        Src::ZERO
    }

    fn imm_src(u: u32) -> Src {
        Src::new_imm_u32(u)
    }

    fn default_mem_access() -> MemAccess {
        MemAccess {
            mem_type: MemType::B32,
            space: MemSpace::Global(MemAddrType::A32),
            order: MemOrder::Constant,
            eviction_priority: MemEvictionPriority::Normal,
        }
    }

    #[test]
    fn test_ldc_mode_display() {
        assert_eq!(format!("{}", LdcMode::Indexed), "");
        assert_eq!(format!("{}", LdcMode::IndexedLinear), ".il");
        assert_eq!(format!("{}", LdcMode::IndexedSegmented), ".is");
        assert_eq!(format!("{}", LdcMode::IndexedSegmentedLinear), ".isl");
    }

    #[test]
    fn test_ldsm_size_display() {
        assert_eq!(format!("{}", LdsmSize::M8N8), "m8n8");
        assert_eq!(format!("{}", LdsmSize::MT8N8), "m8n8.trans");
    }

    #[test]
    fn test_op_ld_display() {
        let op = OpLd {
            dst: Dst::None,
            addr: zero_src(),
            offset: 0,
            stride: OffsetStride::X1,
            access: default_mem_access(),
        };
        let s = format!("{op}");
        assert!(s.contains("ld"));
        assert!(s.contains("rZ"));
    }

    #[test]
    fn test_op_ld_with_offset() {
        let op = OpLd {
            dst: Dst::None,
            addr: imm_src(0x100),
            offset: 0x10,
            stride: OffsetStride::X4,
            access: default_mem_access(),
        };
        let s = format!("{op}");
        assert!(s.contains("+0x10"));
    }

    #[test]
    fn test_op_ldslk_display() {
        let op = OpLdSharedLock {
            dsts: [Dst::None, Dst::None],
            addr: zero_src(),
            offset: 0,
            mem_type: MemType::B32,
        };
        let s = format!("{op}");
        assert!(s.contains("ldslk"));
        assert!(s.contains(".b32"));
    }

    #[test]
    fn test_op_st_display() {
        let op = OpSt {
            srcs: [zero_src(), imm_src(0x42)],
            offset: 0,
            stride: OffsetStride::X1,
            access: default_mem_access(),
        };
        let s = format!("{op}");
        assert!(s.contains("st"));
        assert!(s.contains("0x42"));
    }

    #[test]
    fn test_op_stscheckunlock_display() {
        let op = OpStSCheckUnlock {
            locked: Dst::None,
            srcs: [zero_src(), imm_src(1)],
            offset: 0,
            mem_type: MemType::B32,
        };
        let s = format!("{op}");
        assert!(s.contains("stscul"));
    }

    #[test]
    fn test_op_atom_display() {
        let op = OpAtom {
            dst: Dst::None,
            srcs: [zero_src(), zero_src(), imm_src(1)],
            atom_op: AtomOp::Add,
            atom_type: AtomType::U32,
            addr_offset: 0,
            addr_stride: OffsetStride::X1,
            mem_space: MemSpace::Shared,
            mem_order: MemOrder::Constant,
            mem_eviction_priority: MemEvictionPriority::Normal,
        };
        let s = format!("{op}");
        assert!(s.contains("atom"));
        assert!(s.contains(".add"));
        assert!(s.contains(".u32"));
        assert!(s.contains(".shared"));
    }

    #[test]
    fn test_op_al2p_display() {
        let op = OpAL2P {
            dst: Dst::None,
            offset: zero_src(),
            addr: 0x10,
            comps: 4,
            output: false,
        };
        let s = format!("{op}");
        assert!(s.contains("al2p"));
        assert!(s.contains("0x10"));
    }

    #[test]
    fn test_op_ald_display() {
        let op = OpALd {
            dst: Dst::None,
            srcs: [zero_src(), zero_src()],
            addr: 0x20,
            comps: 4,
            patch: false,
            output: true,
            phys: false,
        };
        let s = format!("{op}");
        assert!(s.contains("ald"));
        assert!(s.contains(".o"));
    }

    #[test]
    fn test_op_ast_display() {
        let op = OpASt {
            srcs: [zero_src(), zero_src(), imm_src(0)],
            addr: 0x30,
            comps: 2,
            patch: true,
            phys: false,
        };
        let s = format!("{op}");
        assert!(s.contains("ast"));
        assert!(s.contains(".p"));
    }

    #[test]
    fn test_op_ldsm_display() {
        let op = OpLdsm {
            dst: Dst::None,
            mat_size: LdsmSize::M8N8,
            mat_count: 1,
            addr: zero_src(),
            offset: 0,
        };
        let s = format!("{op}");
        assert!(s.contains("ldsm"));
        assert!(s.contains("m8n8"));
    }

    #[test]
    fn test_op_ipa_display() {
        let op = OpIpa {
            dst: Dst::None,
            addr: 0x50,
            freq: InterpFreq::Pass,
            loc: InterpLoc::Default,
            srcs: [zero_src(), zero_src()],
        };
        let s = format!("{op}");
        assert!(s.contains("ipa"));
        assert!(s.contains(".pass"));
        assert!(s.contains("0x50"));
    }

    #[test]
    fn test_op_ldtram_display() {
        let op = OpLdTram {
            dst: Dst::None,
            addr: 0x40,
            use_c: true,
        };
        let s = format!("{op}");
        assert!(s.contains("ldtram"));
        assert!(s.contains(".c"));
    }

    #[test]
    fn test_ccctl_op_display() {
        assert_eq!(format!("{}", CCtlOp::WB), "wb");
        assert_eq!(format!("{}", CCtlOp::IVAll), "ivall");
    }

    #[test]
    fn test_ccctl_op_is_all() {
        assert!(!CCtlOp::WB.is_all());
        assert!(CCtlOp::IVAll.is_all());
    }
}
