// SPDX-License-Identifier: AGPL-3.0-or-later
//! Slice view traits — replacement for `compiler::as_slice`.
//!
//! Used by codegen IR for accessing instruction sources and destinations as
//! contiguous slices, enabling zero-copy iteration over operands.
//!
//! The `Attr` associated type carries per-element metadata (e.g. `SrcType`,
//! `DstType`) that the register allocator and legalizer use.

/// Per-element attribute list — either uniform (all same) or per-element.
#[derive(Clone, Debug)]
pub enum AttrList<A: Copy> {
    /// All elements share the same attribute value.
    Uniform(A),
    /// Each element has its own attribute value.
    List(Vec<A>),
}

impl<A: Copy> AttrList<A> {
    /// Returns the attribute for the element at `idx`.
    ///
    /// # Panics
    ///
    /// Panics if `idx` is out of range for a `List` variant.
    pub fn at(&self, idx: usize) -> A {
        match self {
            Self::Uniform(a) => *a,
            Self::List(v) => v[idx],
        }
    }
}

impl<A: Copy> std::ops::Index<usize> for AttrList<A> {
    type Output = A;

    fn index(&self, idx: usize) -> &A {
        match self {
            Self::Uniform(a) => a,
            Self::List(v) => &v[idx],
        }
    }
}

/// Trait for types that can be viewed as a slice of `T` with per-element attributes.
///
/// This is the core abstraction for codegen instruction operands. Each instruction
/// op struct derives `SrcsAsSlice` / `DstsAsSlice` which generate `AsSlice<Src>`
/// / `AsSlice<Dst>` implementations.
pub trait AsSlice<T> {
    /// Per-element attribute type (e.g. `SrcType`, `DstType`).
    type Attr: Copy;

    /// View as a shared slice.
    fn as_slice(&self) -> &[T];

    /// View as a mutable slice.
    fn as_mut_slice(&mut self) -> &mut [T];

    /// Returns the attribute list for each element.
    fn attrs(&self) -> AttrList<Self::Attr>;
}

impl<T> AsSlice<T> for Vec<T> {
    type Attr = ();

    fn as_slice(&self) -> &[T] {
        self
    }

    fn as_mut_slice(&mut self) -> &mut [T] {
        self
    }

    fn attrs(&self) -> AttrList<()> {
        AttrList::Uniform(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_attr_list_uniform() {
        let al = AttrList::Uniform(42u8);
        assert_eq!(al.at(0), 42);
        assert_eq!(al.at(100), 42);
    }

    #[test]
    fn test_attr_list_per_element() {
        let al = AttrList::List(vec![1u8, 2, 3]);
        assert_eq!(al.at(0), 1);
        assert_eq!(al.at(1), 2);
        assert_eq!(al.at(2), 3);
    }

    #[test]
    fn test_vec_as_slice() {
        let v = vec![1, 2, 3];
        assert_eq!(AsSlice::as_slice(&v), &[1, 2, 3]);
    }

    #[test]
    fn test_vec_as_mut_slice() {
        let mut v = vec![1, 2, 3];
        AsSlice::as_mut_slice(&mut v)[0] = 10;
        assert_eq!(v[0], 10);
    }

    #[test]
    fn attr_list_index_uniform() {
        let al = AttrList::Uniform(7u8);
        assert_eq!(al[0], 7);
        assert_eq!(al[3], 7);
    }

    #[test]
    fn attr_list_index_list() {
        let al = AttrList::List(vec![10u16, 20, 30]);
        assert_eq!(al[1], 20);
    }

    #[test]
    fn vec_attrs_returns_uniform_unit() {
        let v = vec![1_i32, 2, 3];
        assert!(matches!(attrs::<i32>(&v), AttrList::Uniform(())));
    }

    fn attrs<T>(v: &Vec<T>) -> AttrList<()> {
        <Vec<T> as AsSlice<T>>::attrs(v)
    }

    #[test]
    fn attr_list_clone_debug() {
        let a = AttrList::List(vec![1u8, 2]);
        let b = a.clone();
        assert_eq!(format!("{a:?}"), format!("{b:?}"));
    }
}
