// Copyright © 2022 Collabora, Ltd.
// SPDX-License-Identifier: MIT
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
    /// Try to create from size in bytes and signedness.
    pub fn try_from_size(size: u8, is_signed: bool) -> Option<MemType> {
        Some(match (size, is_signed) {
            (1, false) => MemType::U8,
            (1, true) => MemType::I8,
            (2, false) => MemType::U16,
            (2, true) => MemType::I16,
            (4, _) => MemType::B32,
            (8, _) => MemType::B64,
            (16, _) => MemType::B128,
            _ => return None,
        })
    }

    /// # Panics
    ///
    /// Panics if `size` is not 1, 2, 4, 8, or 16.
    pub fn from_size(size: u8, is_signed: bool) -> MemType {
        Self::try_from_size(size, is_signed).expect("invalid memory load/store size")
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
        assert!(matches!(MemType::try_from_size(1, false), Some(MemType::U8)));
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
        assert!(matches!(AtomType::F(32), AtomType::F32));
        assert!(matches!(AtomType::F(64), AtomType::F64));
        assert!(matches!(AtomType::U(32), AtomType::U32));
        assert!(matches!(AtomType::U(64), AtomType::U64));
        assert!(matches!(AtomType::I(32), AtomType::I32));
        assert!(matches!(AtomType::I(64), AtomType::I64));
    }

    #[test]
    #[should_panic(expected = "16-bit float atomics not yet supported")]
    fn test_atom_type_f_16_panics() {
        AtomType::F(16);
    }

    #[test]
    #[should_panic(expected = "Invalid uint atomic type")]
    fn test_atom_type_u_invalid_panics() {
        AtomType::U(16);
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
}
