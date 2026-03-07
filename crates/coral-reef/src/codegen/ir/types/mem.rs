// SPDX-License-Identifier: AGPL-3.0-only
// Copyright © 2025-2026 ecoPrimals
// Derived from Collabora, Ltd. (2022)
//! Memory, cache, and atomic operation types.

use super::*;

#[derive(Clone, Copy, Eq, Hash, PartialEq)]
pub enum MemAddrType {
    A32,
    A64,
}

impl fmt::Display for MemAddrType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::A32 => write!(f, ".a32"),
            Self::A64 => write!(f, ".a64"),
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
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
    /// Try to create from size in bytes and signedness.
    pub const fn try_from_size(size: u8, is_signed: bool) -> Option<Self> {
        Some(match (size, is_signed) {
            (1, false) => Self::U8,
            (1, true) => Self::I8,
            (2, false) => Self::U16,
            (2, true) => Self::I16,
            (4, _) => Self::B32,
            (8, _) => Self::B64,
            (16, _) => Self::B128,
            _ => return None,
        })
    }

    /// # Panics
    ///
    /// Panics if `size` is not 1, 2, 4, 8, or 16.
    #[expect(clippy::missing_const_for_fn, reason = "calls non-const .expect()")]
    pub fn from_size(size: u8, is_signed: bool) -> Self {
        Self::try_from_size(size, is_signed).expect("invalid memory load/store size")
    }

    pub const fn bits(&self) -> usize {
        match self {
            Self::U8 | Self::I8 => 8,
            Self::U16 | Self::I16 => 16,
            Self::B32 => 32,
            Self::B64 => 64,
            Self::B128 => 128,
        }
    }
}

impl fmt::Display for MemType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::U8 => write!(f, ".u8"),
            Self::I8 => write!(f, ".i8"),
            Self::U16 => write!(f, ".u16"),
            Self::I16 => write!(f, ".i16"),
            Self::B32 => write!(f, ".b32"),
            Self::B64 => write!(f, ".b64"),
            Self::B128 => write!(f, ".b128"),
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
            Self::Constant => write!(f, ".constant"),
            Self::Weak => write!(f, ".weak"),
            Self::Strong(scope) => write!(f, ".strong{scope}"),
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
            Self::CTA => write!(f, ".cta"),
            Self::GPU => write!(f, ".gpu"),
            Self::System => write!(f, ".sys"),
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
    pub const fn addr_type(&self) -> MemAddrType {
        match self {
            Self::Global(t) => *t,
            Self::Local | Self::Shared => MemAddrType::A32,
        }
    }
}

impl fmt::Display for MemSpace {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Global(t) => write!(f, ".global{t}"),
            Self::Local => write!(f, ".local"),
            Self::Shared => write!(f, ".shared"),
        }
    }
}

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
            Self::First => write!(f, ".ef"),
            Self::Normal => Ok(()),
            Self::Last => write!(f, ".el"),
            Self::LastUse => write!(f, ".lu"),
            Self::Unchanged => write!(f, ".eu"),
            Self::NoAllocate => write!(f, ".na"),
        }
    }
}

/// Memory load cache ops used by Kepler
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
            Self::CacheAll => write!(f, ".ca"),
            Self::CacheGlobal => write!(f, ".cg"),
            Self::CacheIncoherent => write!(f, ".ci"),
            Self::CacheStreaming => write!(f, ".cs"),
            Self::CacheInvalidate => write!(f, ".cv"),
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
                        Self::CacheIncoherent
                    } else {
                        Self::CacheAll
                    }
                }
                MemOrder::Strong(MemScope::System) => Self::CacheInvalidate,
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
                        Self::CacheAll
                    } else {
                        Self::CacheGlobal
                    }
                }
            },
            MemSpace::Local | MemSpace::Shared => Self::CacheAll,
        }
    }
}

/// Memory store cache ops used by Kepler
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
            Self::WriteBack => write!(f, ".wb"),
            Self::CacheGlobal => write!(f, ".cg"),
            Self::CacheStreaming => write!(f, ".cs"),
            Self::WriteThrough => write!(f, ".wt"),
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
                MemOrder::Constant => {
                    debug_assert!(false, "Cannot store to constant memory");
                    Self::WriteThrough
                }
                MemOrder::Strong(MemScope::System) => Self::WriteThrough,
                _ => {
                    // See the corresponding comment in LdCacheOp::select()
                    if sm.sm() >= 50 {
                        Self::WriteBack
                    } else {
                        Self::CacheGlobal
                    }
                }
            },
            MemSpace::Local | MemSpace::Shared => Self::WriteBack,
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
    pub const fn F(bits: u8) -> Option<Self> {
        match bits {
            32 => Some(Self::F32),
            64 => Some(Self::F64),
            _ => None,
        }
    }

    pub const fn U(bits: u8) -> Option<Self> {
        match bits {
            32 => Some(Self::U32),
            64 => Some(Self::U64),
            _ => None,
        }
    }

    pub const fn I(bits: u8) -> Option<Self> {
        match bits {
            32 => Some(Self::I32),
            64 => Some(Self::I64),
            _ => None,
        }
    }

    pub const fn bits(&self) -> usize {
        match self {
            Self::F16x2 | Self::F32 | Self::U32 | Self::I32 => 32,
            Self::U64 | Self::I64 | Self::F64 => 64,
        }
    }

    pub const fn is_float(&self) -> bool {
        match self {
            Self::F16x2 | Self::F32 | Self::F64 => true,
            Self::U32 | Self::I32 | Self::U64 | Self::I64 => false,
        }
    }
}

impl fmt::Display for AtomType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::F16x2 => write!(f, ".f16x2"),
            Self::U32 => write!(f, ".u32"),
            Self::I32 => write!(f, ".i32"),
            Self::F32 => write!(f, ".f32"),
            Self::U64 => write!(f, ".u64"),
            Self::I64 => write!(f, ".i64"),
            Self::F64 => write!(f, ".f64"),
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
    pub const fn is_reduction(&self) -> bool {
        match self {
            Self::Add
            | Self::Min
            | Self::Max
            | Self::Inc
            | Self::Dec
            | Self::And
            | Self::Or
            | Self::Xor => true,
            Self::Exch | Self::CmpExch(_) => false,
        }
    }
}

impl fmt::Display for AtomOp {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Add => write!(f, ".add"),
            Self::Min => write!(f, ".min"),
            Self::Max => write!(f, ".max"),
            Self::Inc => write!(f, ".inc"),
            Self::Dec => write!(f, ".dec"),
            Self::And => write!(f, ".and"),
            Self::Or => write!(f, ".or"),
            Self::Xor => write!(f, ".xor"),
            Self::Exch => write!(f, ".exch"),
            Self::CmpExch(AtomCmpSrc::Separate) => write!(f, ".cmpexch"),
            Self::CmpExch(AtomCmpSrc::Packed) => write!(f, ".cmpexch.packed"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_mem_type_from_size() {
        assert!(matches!(MemType::from_size(1, false), MemType::U8));
        assert!(matches!(MemType::from_size(1, true), MemType::I8));
        assert!(matches!(MemType::from_size(2, false), MemType::U16));
        assert!(matches!(MemType::from_size(2, true), MemType::I16));
        assert!(matches!(MemType::from_size(4, false), MemType::B32));
        assert!(matches!(MemType::from_size(4, true), MemType::B32));
        assert!(matches!(MemType::from_size(8, false), MemType::B64));
        assert!(matches!(MemType::from_size(16, false), MemType::B128));
    }

    #[test]
    #[should_panic(expected = "invalid memory load/store size")]
    fn test_mem_type_from_size_invalid() {
        MemType::from_size(3, false);
    }

    #[test]
    fn test_mem_type_bits() {
        assert_eq!(MemType::U8.bits(), 8);
        assert_eq!(MemType::I16.bits(), 16);
        assert_eq!(MemType::B32.bits(), 32);
        assert_eq!(MemType::B64.bits(), 64);
        assert_eq!(MemType::B128.bits(), 128);
    }

    #[test]
    fn test_mem_space_addr_type() {
        assert!(MemSpace::Global(MemAddrType::A32).addr_type() == MemAddrType::A32);
        assert!(MemSpace::Global(MemAddrType::A64).addr_type() == MemAddrType::A64);
        assert!(MemSpace::Local.addr_type() == MemAddrType::A32);
        assert!(MemSpace::Shared.addr_type() == MemAddrType::A32);
    }

    #[test]
    fn test_atom_op_is_reduction() {
        assert!(AtomOp::Add.is_reduction());
        assert!(AtomOp::Min.is_reduction());
        assert!(AtomOp::Max.is_reduction());
        assert!(AtomOp::Inc.is_reduction());
        assert!(AtomOp::Dec.is_reduction());
        assert!(AtomOp::And.is_reduction());
        assert!(AtomOp::Or.is_reduction());
        assert!(AtomOp::Xor.is_reduction());
        assert!(!AtomOp::Exch.is_reduction());
        assert!(!AtomOp::CmpExch(AtomCmpSrc::Separate).is_reduction());
        assert!(!AtomOp::CmpExch(AtomCmpSrc::Packed).is_reduction());
    }

    #[test]
    fn test_mem_type_display() {
        assert_eq!(format!("{}", MemType::U8), ".u8");
        assert_eq!(format!("{}", MemType::B32), ".b32");
        assert_eq!(format!("{}", MemType::B128), ".b128");
    }

    #[test]
    fn test_mem_addr_type_display() {
        assert_eq!(format!("{}", MemAddrType::A32), ".a32");
        assert_eq!(format!("{}", MemAddrType::A64), ".a64");
    }

    #[test]
    fn test_mem_space_display() {
        assert_eq!(
            format!("{}", MemSpace::Global(MemAddrType::A64)),
            ".global.a64"
        );
        assert_eq!(format!("{}", MemSpace::Local), ".local");
        assert_eq!(format!("{}", MemSpace::Shared), ".shared");
    }

    #[test]
    fn test_atom_op_display() {
        assert_eq!(format!("{}", AtomOp::Add), ".add");
        assert_eq!(format!("{}", AtomOp::Exch), ".exch");
        assert_eq!(
            format!("{}", AtomOp::CmpExch(AtomCmpSrc::Packed)),
            ".cmpexch.packed"
        );
    }

    #[test]
    fn test_mem_type_try_from_size() {
        assert!(matches!(
            MemType::try_from_size(1, false),
            Some(MemType::U8)
        ));
        assert!(matches!(MemType::try_from_size(1, true), Some(MemType::I8)));
        assert!(MemType::try_from_size(3, false).is_none());
        assert!(MemType::try_from_size(32, false).is_none());
    }

    #[test]
    fn test_mem_order_display() {
        assert_eq!(format!("{}", MemOrder::Constant), ".constant");
        assert_eq!(format!("{}", MemOrder::Weak), ".weak");
        assert_eq!(
            format!("{}", MemOrder::Strong(MemScope::CTA)),
            ".strong.cta"
        );
        assert_eq!(
            format!("{}", MemOrder::Strong(MemScope::GPU)),
            ".strong.gpu"
        );
        assert_eq!(
            format!("{}", MemOrder::Strong(MemScope::System)),
            ".strong.sys"
        );
    }

    #[test]
    fn test_mem_scope_display() {
        assert_eq!(format!("{}", MemScope::CTA), ".cta");
        assert_eq!(format!("{}", MemScope::GPU), ".gpu");
        assert_eq!(format!("{}", MemScope::System), ".sys");
    }

    #[test]
    fn test_mem_eviction_priority_display() {
        assert_eq!(format!("{}", MemEvictionPriority::First), ".ef");
        assert_eq!(format!("{}", MemEvictionPriority::Normal), "");
        assert_eq!(format!("{}", MemEvictionPriority::Last), ".el");
        assert_eq!(format!("{}", MemEvictionPriority::LastUse), ".lu");
        assert_eq!(format!("{}", MemEvictionPriority::Unchanged), ".eu");
        assert_eq!(format!("{}", MemEvictionPriority::NoAllocate), ".na");
    }

    #[test]
    fn test_ld_cache_op_display() {
        assert_eq!(format!("{}", LdCacheOp::CacheAll), ".ca");
        assert_eq!(format!("{}", LdCacheOp::CacheGlobal), ".cg");
        assert_eq!(format!("{}", LdCacheOp::CacheIncoherent), ".ci");
        assert_eq!(format!("{}", LdCacheOp::CacheStreaming), ".cs");
        assert_eq!(format!("{}", LdCacheOp::CacheInvalidate), ".cv");
    }

    #[test]
    fn test_st_cache_op_display() {
        assert_eq!(format!("{}", StCacheOp::WriteBack), ".wb");
        assert_eq!(format!("{}", StCacheOp::CacheGlobal), ".cg");
        assert_eq!(format!("{}", StCacheOp::CacheStreaming), ".cs");
        assert_eq!(format!("{}", StCacheOp::WriteThrough), ".wt");
    }

    #[test]
    fn test_mem_access_display() {
        let access = MemAccess {
            mem_type: MemType::B32,
            space: MemSpace::Shared,
            order: MemOrder::Weak,
            eviction_priority: MemEvictionPriority::Normal,
        };
        let s = format!("{access}");
        assert!(s.contains(".shared"));
        assert!(s.contains(".weak"));
        assert!(s.contains(".b32"));
    }

    #[test]
    fn test_atom_type_f_u_i() {
        assert!(matches!(AtomType::F(32), Some(AtomType::F32)));
        assert!(matches!(AtomType::F(64), Some(AtomType::F64)));
        assert!(matches!(AtomType::U(32), Some(AtomType::U32)));
        assert!(matches!(AtomType::U(64), Some(AtomType::U64)));
        assert!(matches!(AtomType::I(32), Some(AtomType::I32)));
        assert!(matches!(AtomType::I(64), Some(AtomType::I64)));
    }

    #[test]
    fn test_atom_type_f_16_returns_none() {
        assert!(AtomType::F(16).is_none());
    }

    #[test]
    fn test_atom_type_u_invalid_returns_none() {
        assert!(AtomType::U(16).is_none());
    }

    #[test]
    fn test_atom_type_bits_and_is_float() {
        assert_eq!(AtomType::F32.bits(), 32);
        assert_eq!(AtomType::F64.bits(), 64);
        assert!(AtomType::F32.is_float());
        assert!(AtomType::F64.is_float());
        assert!(!AtomType::U32.is_float());
        assert!(!AtomType::I64.is_float());
    }

    #[test]
    fn test_atom_type_display() {
        assert_eq!(format!("{}", AtomType::F16x2), ".f16x2");
        assert_eq!(format!("{}", AtomType::F32), ".f32");
        assert_eq!(format!("{}", AtomType::F64), ".f64");
        assert_eq!(format!("{}", AtomType::U64), ".u64");
    }

    #[test]
    fn test_mem_type_size_roundtrip() {
        for (size, is_signed) in [
            (1, false),
            (1, true),
            (2, false),
            (2, true),
            (4, false),
            (8, false),
            (16, false),
        ] {
            let mt = MemType::from_size(size, is_signed);
            assert_eq!(
                mt.bits(),
                usize::from(size) * 8,
                "MemType {:?} should have {} bits",
                mt,
                usize::from(size) * 8
            );
        }
    }

    #[test]
    fn test_mem_access_ld_st_cache_ops() {
        use crate::codegen::nv::sm70::ShaderModel70;
        let sm = ShaderModel70::new(70);
        let access = MemAccess {
            mem_type: MemType::B32,
            space: MemSpace::Shared,
            order: MemOrder::Weak,
            eviction_priority: MemEvictionPriority::Normal,
        };
        let _ld = access.ld_cache_op(&sm);
        let _st = access.st_cache_op(&sm);
    }
}
