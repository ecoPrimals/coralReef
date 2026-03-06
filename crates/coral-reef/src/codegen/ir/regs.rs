// SPDX-License-Identifier: AGPL-3.0-only
// Copyright © 2025-2026 ecoPrimals
// Derived from Collabora, Ltd. (2022)
//! Register types: `RegFile`, `RegRef`, `RegFileSet`, `PerRegFile`.

#![allow(clippy::wildcard_imports, clippy::enum_glob_use)]

use std::fmt;
use std::ops::{Index, IndexMut, Range};
use std::slice;

use crate::codegen::ssa_value::SSAValue;

#[derive(Clone, Copy, Eq, Hash, PartialEq)]
pub struct Label {
    idx: u32,
}

impl fmt::Display for Label {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "L{}", self.idx)
    }
}

pub struct LabelAllocator {
    count: u32,
}

impl LabelAllocator {
    pub const fn new() -> Self {
        Self { count: 0 }
    }

    pub const fn alloc(&mut self) -> Label {
        let idx = self.count;
        self.count += 1;
        Label { idx }
    }
}

/// Represents a register file
#[repr(u8)]
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum RegFile {
    /// The general-purpose register file
    ///
    /// General-purpose registers are 32 bits per SIMT channel.
    GPR = 0,

    /// The general-purpose uniform register file
    ///
    /// General-purpose uniform registers are 32 bits each and uniform across a
    /// wave.
    UGPR = 1,

    /// The predicate reigster file
    ///
    /// Predicate registers are 1 bit per SIMT channel.
    Pred = 2,

    /// The uniform predicate reigster file
    ///
    /// Uniform predicate registers are 1 bit and uniform across a wave.
    UPred = 3,

    /// The carry flag register file
    ///
    /// Only one carry flag register exists in hardware, but representing it as
    /// a reg file simplifies dependency tracking.
    ///
    /// This is used only on SM50.
    Carry = 4,

    /// The barrier register file
    ///
    /// This is a lane mask used for wave re-convergence instructions.
    Bar = 5,

    /// The memory register file
    ///
    /// This is a virtual register file for things which will get spilled to
    /// local memory.  Each memory location is 32 bits per SIMT channel.
    Mem = 6,
}

const NUM_REG_FILES: usize = 7;

impl RegFile {
    /// Returns true if the register file is uniform across a wave.
    #[must_use]
    pub const fn is_uniform(&self) -> bool {
        match self {
            Self::GPR | Self::Pred | Self::Carry | Self::Bar | Self::Mem => false,
            Self::UGPR | Self::UPred => true,
        }
    }

    /// Returns the uniform form of this register file, if any.  For `GPR` and
    /// `UGPR, this returns `UGPR` and for `Pred` and `UPred`, this returns
    /// `UPred`.
    #[must_use]
    pub const fn to_uniform(self) -> Option<Self> {
        match self {
            Self::GPR | Self::UGPR => Some(Self::UGPR),
            Self::Pred | Self::UPred => Some(Self::UPred),
            Self::Carry | Self::Bar | Self::Mem => None,
        }
    }

    /// Returns warp-wide version of this register file.
    #[must_use]
    pub const fn to_warp(self) -> Self {
        match self {
            Self::GPR | Self::UGPR => Self::GPR,
            Self::Pred | Self::UPred => Self::Pred,
            Self::Carry | Self::Bar | Self::Mem => self,
        }
    }

    /// Returns true if the register file is GPR or UGPR.
    #[must_use]
    pub const fn is_gpr(&self) -> bool {
        match self {
            Self::GPR | Self::UGPR => true,
            Self::Pred | Self::UPred | Self::Carry | Self::Bar | Self::Mem => false,
        }
    }

    /// Returns true if the register file is a predicate register file.
    #[must_use]
    pub const fn is_predicate(&self) -> bool {
        match self {
            Self::GPR | Self::UGPR | Self::Carry | Self::Bar | Self::Mem => false,
            Self::Pred | Self::UPred => true,
        }
    }

    #[must_use]
    pub const fn fmt_prefix(&self) -> &'static str {
        match self {
            Self::GPR => "r",
            Self::UGPR => "ur",
            Self::Pred => "p",
            Self::UPred => "up",
            Self::Carry => "c",
            Self::Bar => "b",
            Self::Mem => "m",
        }
    }
}

impl fmt::Display for RegFile {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::GPR => write!(f, "GPR"),
            Self::UGPR => write!(f, "UGPR"),
            Self::Pred => write!(f, "Pred"),
            Self::UPred => write!(f, "UPred"),
            Self::Carry => write!(f, "Carry"),
            Self::Bar => write!(f, "Bar"),
            Self::Mem => write!(f, "Mem"),
        }
    }
}

impl From<RegFile> for u8 {
    fn from(value: RegFile) -> Self {
        value as Self
    }
}

impl TryFrom<u32> for RegFile {
    type Error = &'static str;

    fn try_from(value: u32) -> Result<Self, Self::Error> {
        match value {
            0 => Ok(Self::GPR),
            1 => Ok(Self::UGPR),
            2 => Ok(Self::Pred),
            3 => Ok(Self::UPred),
            4 => Ok(Self::Carry),
            5 => Ok(Self::Bar),
            6 => Ok(Self::Mem),
            _ => Err("Invalid register file number"),
        }
    }
}

impl TryFrom<u16> for RegFile {
    type Error = &'static str;

    fn try_from(value: u16) -> Result<Self, Self::Error> {
        Self::try_from(u32::from(value))
    }
}

impl TryFrom<u8> for RegFile {
    type Error = &'static str;

    fn try_from(value: u8) -> Result<Self, Self::Error> {
        Self::try_from(u32::from(value))
    }
}

/// A trait for things which have an associated register file
pub trait HasRegFile {
    fn file(&self) -> RegFile;

    fn is_uniform(&self) -> bool {
        self.file().is_uniform()
    }

    fn is_gpr(&self) -> bool {
        self.file().is_gpr()
    }

    fn is_predicate(&self) -> bool {
        self.file().is_predicate()
    }
}

impl HasRegFile for &[SSAValue] {
    fn file(&self) -> RegFile {
        let comps = self.len();
        let file = self[0].file();
        for i in 1..comps {
            if self[i].file() != file {
                panic!("Illegal mix of RegFiles")
            }
        }
        file
    }
}

#[derive(Clone)]
pub struct RegFileSet {
    bits: u8,
}

impl RegFileSet {
    pub const fn new() -> Self {
        Self { bits: 0 }
    }

    pub const fn len(&self) -> usize {
        self.bits.count_ones() as usize
    }

    pub const fn contains(&self, file: RegFile) -> bool {
        self.bits & (1 << (file as u8)) != 0
    }

    pub const fn insert(&mut self, file: RegFile) -> bool {
        let has_file = self.contains(file);
        self.bits |= 1 << (file as u8);
        !has_file
    }

    pub const fn is_empty(&self) -> bool {
        self.bits == 0
    }

    #[allow(dead_code)]
    pub fn iter(&self) -> Self {
        self.clone()
    }

    pub const fn remove(&mut self, file: RegFile) -> bool {
        let has_file = self.contains(file);
        self.bits &= !(1 << (file as u8));
        has_file
    }
}

impl FromIterator<RegFile> for RegFileSet {
    fn from_iter<T: IntoIterator<Item = RegFile>>(iter: T) -> Self {
        let mut set = Self::new();
        for file in iter {
            set.insert(file);
        }
        set
    }
}

impl Iterator for RegFileSet {
    type Item = RegFile;

    fn next(&mut self) -> Option<RegFile> {
        if self.is_empty() {
            None
        } else {
            let file = self.bits.trailing_zeros().try_into().unwrap();
            self.remove(file);
            Some(file)
        }
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        let len = self.len();
        (len, Some(len))
    }
}

/// A container mapping register files to items.
///
/// This is used by several passes which need to replicate a data structure
/// per-register-file.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PerRegFile<T> {
    per_file: [T; NUM_REG_FILES],
}

impl<T> PerRegFile<T> {
    /// Creates a new per-register-file container.
    ///
    /// Because this container assumes it always has an item for each register
    /// file, it takes a callback which maps register files to initial values
    /// to avoid adding a bunch of `Option<T>` or requiring `T` to implement
    /// `Default`.  If `T` does implement `Default`, then so does
    /// `PerRefFile<T>`.
    pub fn new_with<F: Fn(RegFile) -> T>(f: F) -> Self {
        Self {
            per_file: [
                f(RegFile::GPR),
                f(RegFile::UGPR),
                f(RegFile::Pred),
                f(RegFile::UPred),
                f(RegFile::Carry),
                f(RegFile::Bar),
                f(RegFile::Mem),
            ],
        }
    }

    /// Iterates over the values in this container.
    pub fn values(&self) -> slice::Iter<'_, T> {
        self.per_file.iter()
    }

    /// Iterates over the mutable values in this container.
    pub fn values_mut(&mut self) -> slice::IterMut<'_, T> {
        self.per_file.iter_mut()
    }
}

impl<T: Default> Default for PerRegFile<T> {
    fn default() -> Self {
        Self {
            per_file: Default::default(),
        }
    }
}

impl<T> Index<RegFile> for PerRegFile<T> {
    type Output = T;

    fn index(&self, idx: RegFile) -> &T {
        &self.per_file[idx as u8 as usize]
    }
}

impl<T> IndexMut<RegFile> for PerRegFile<T> {
    fn index_mut(&mut self, idx: RegFile) -> &mut T {
        &mut self.per_file[idx as u8 as usize]
    }
}

/// A reference to a contiguous range of registers in a particular register
/// file.
#[derive(Clone, Copy, Eq, Hash, PartialEq)]
pub struct RegRef {
    packed: u32,
}

impl RegRef {
    pub const MAX_IDX: u32 = (1 << 26) - 1;

    /// Creates a new register reference.
    ///
    /// # Panics
    ///
    /// This method panics if `base_idx > RegRef::MAX_IDX` or if `comps > 8`.
    pub fn new(file: RegFile, base_idx: u32, comps: u8) -> Self {
        assert!(base_idx <= Self::MAX_IDX);
        let mut packed = base_idx;
        assert!(comps > 0 && comps <= 8);
        packed |= u32::from(comps - 1) << 26;
        assert!(u8::from(file) < 8);
        packed |= u32::from(u8::from(file)) << 29;
        Self { packed }
    }

    /// Returns the index of the first register referenced.
    pub const fn base_idx(&self) -> u32 {
        self.packed & 0x03ff_ffff
    }

    /// Returns the range of register indices referenced.
    pub fn idx_range(&self) -> Range<u32> {
        let start = self.base_idx();
        let end = start + u32::from(self.comps());
        start..end
    }

    /// Returns the number of registers referenced.
    pub fn comps(&self) -> u8 {
        (((self.packed >> 26) & 0x7) + 1).try_into().unwrap()
    }

    /// Returns a reference to the single register at `base_idx() + c`.
    pub fn comp(&self, c: u8) -> Self {
        assert!(c < self.comps());
        Self::new(self.file(), self.base_idx() + u32::from(c), 1)
    }
}

impl HasRegFile for RegRef {
    fn file(&self) -> RegFile {
        ((self.packed >> 29) & 0x7).try_into().unwrap()
    }
}

impl fmt::Display for RegRef {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}{}", self.file().fmt_prefix(), self.base_idx())?;
        if self.comps() > 1 {
            write!(f, "..{}", self.idx_range().end)?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_reg_ref_new_single() {
        let reg = RegRef::new(RegFile::GPR, 0, 1);
        assert_eq!(reg.base_idx(), 0);
        assert_eq!(reg.comps(), 1);
        assert!(reg.file().is_gpr());
        assert_eq!(reg.idx_range(), 0..1);
    }

    #[test]
    fn test_reg_ref_multi_comps() {
        let reg = RegRef::new(RegFile::GPR, 10, 4);
        assert_eq!(reg.base_idx(), 10);
        assert_eq!(reg.comps(), 4);
        assert_eq!(reg.idx_range(), 10..14);
    }

    #[test]
    fn test_reg_ref_comp() {
        let reg = RegRef::new(RegFile::GPR, 5, 3);
        let c0 = reg.comp(0);
        let c1 = reg.comp(1);
        assert_eq!(c0.base_idx(), 5);
        assert_eq!(c1.base_idx(), 6);
        assert_eq!(c0.comps(), 1);
    }

    #[test]
    fn test_reg_file_properties() {
        assert!(RegFile::GPR.is_gpr());
        assert!(RegFile::UGPR.is_gpr());
        assert!(RegFile::Pred.is_predicate());
        assert!(RegFile::UGPR.is_uniform());
        assert!(!RegFile::GPR.is_uniform());
    }

    #[test]
    fn test_reg_file_set() {
        let mut set = RegFileSet::new();
        assert!(set.is_empty());
        set.insert(RegFile::GPR);
        set.insert(RegFile::Pred);
        assert_eq!(set.len(), 2);
        assert!(set.contains(RegFile::GPR));
        set.remove(RegFile::GPR);
        assert!(!set.contains(RegFile::GPR));
    }

    #[test]
    fn test_reg_file_to_uniform_to_warp() {
        assert_eq!(RegFile::GPR.to_uniform(), Some(RegFile::UGPR));
        assert_eq!(RegFile::UGPR.to_uniform(), Some(RegFile::UGPR));
        assert_eq!(RegFile::Pred.to_uniform(), Some(RegFile::UPred));
        assert_eq!(RegFile::Carry.to_uniform(), None);
        assert_eq!(RegFile::GPR.to_warp(), RegFile::GPR);
        assert_eq!(RegFile::UGPR.to_warp(), RegFile::GPR);
    }

    #[test]
    fn test_reg_file_try_from() {
        assert!(matches!(RegFile::try_from(0u8), Ok(RegFile::GPR)));
        assert!(matches!(RegFile::try_from(2u8), Ok(RegFile::Pred)));
        assert!(RegFile::try_from(10u8).is_err());
    }

    #[test]
    fn test_reg_file_display() {
        assert_eq!(format!("{}", RegFile::GPR), "GPR");
        assert_eq!(format!("{}", RegFile::Pred), "Pred");
        assert_eq!(format!("{}", RegFile::Mem), "Mem");
    }

    #[test]
    fn test_reg_file_set_from_iter() {
        let set: RegFileSet = [RegFile::GPR, RegFile::UGPR, RegFile::GPR]
            .into_iter()
            .collect();
        assert_eq!(set.len(), 2);
        assert!(set.contains(RegFile::GPR));
        assert!(set.contains(RegFile::UGPR));
    }

    #[test]
    fn test_per_reg_file_index() {
        let per: PerRegFile<u32> = PerRegFile::new_with(|_| 0);
        assert_eq!(per[RegFile::GPR], 0);
        assert_eq!(per[RegFile::Pred], 0);
    }

    #[test]
    fn test_label_allocator() {
        let mut alloc = LabelAllocator::new();
        let l0 = alloc.alloc();
        let l1 = alloc.alloc();
        assert_ne!(format!("{}", l0), format!("{}", l1));
    }
}
