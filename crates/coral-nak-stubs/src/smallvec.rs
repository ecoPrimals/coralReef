// SPDX-License-Identifier: AGPL-3.0-only
//! Small vector with zero/one/many optimization.
//!
//! NAK uses this for instruction mapping results where most operations
//! produce exactly one output instruction (one-to-one), some produce zero
//! (dead code elimination), and a few produce multiple (lowering).

/// Small vector with enum variants for zero, one, or many items.
///
/// Avoids heap allocation for the common zero/one cases. The const generic
/// `N` is reserved for future inline-storage optimization (currently unused).
pub enum SmallVec<T, const N: usize = 4> {
    /// No items (e.g. instruction removed by DCE).
    None,
    /// Exactly one item (most common: 1-to-1 instruction mapping).
    One(T),
    /// Multiple items (e.g. instruction lowered into several).
    Many(Vec<T>),
}

impl<T, const N: usize> SmallVec<T, N> {
    /// Push an item onto the vector.
    pub fn push(&mut self, item: T) {
        match std::mem::replace(self, SmallVec::None) {
            SmallVec::None => *self = SmallVec::One(item),
            SmallVec::One(existing) => {
                let mut v = Vec::with_capacity(2);
                v.push(existing);
                v.push(item);
                *self = SmallVec::Many(v);
            }
            SmallVec::Many(mut v) => {
                v.push(item);
                *self = SmallVec::Many(v);
            }
        }
    }

    /// Get a mutable reference to the last item.
    pub fn last_mut(&mut self) -> Option<&mut T> {
        match self {
            SmallVec::None => None,
            SmallVec::One(x) => Some(x),
            SmallVec::Many(v) => v.last_mut(),
        }
    }

    /// Appends items from another `SmallVec`.
    pub fn append(&mut self, other: &mut Vec<T>) {
        match std::mem::replace(self, SmallVec::None) {
            SmallVec::None => *self = SmallVec::Many(std::mem::take(other)),
            SmallVec::One(item) => {
                let mut v = Vec::with_capacity(1 + other.len());
                v.push(item);
                v.append(other);
                *self = SmallVec::Many(v);
            }
            SmallVec::Many(mut v) => {
                v.append(other);
                *self = SmallVec::Many(v);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_create_none() {
        let sv: SmallVec<i32> = SmallVec::None;
        match &sv {
            SmallVec::None => {}
            _ => panic!("expected None"),
        }
    }

    #[test]
    fn test_create_one() {
        let sv: SmallVec<i32> = SmallVec::One(42);
        match &sv {
            SmallVec::One(x) => assert_eq!(*x, 42),
            _ => panic!("expected One"),
        }
    }

    #[test]
    fn test_create_many() {
        let sv: SmallVec<i32> = SmallVec::Many(vec![1, 2, 3]);
        match &sv {
            SmallVec::Many(v) => assert_eq!(v, &[1, 2, 3]),
            _ => panic!("expected Many"),
        }
    }

    #[test]
    fn test_push_to_none() {
        let mut sv: SmallVec<i32> = SmallVec::None;
        sv.push(10);
        match &sv {
            SmallVec::One(x) => assert_eq!(*x, 10),
            _ => panic!("expected One after push to None"),
        }
    }

    #[test]
    fn test_push_to_one_transitions_to_many() {
        let mut sv: SmallVec<i32> = SmallVec::One(1);
        sv.push(2);
        match &sv {
            SmallVec::Many(v) => assert_eq!(v, &[1, 2]),
            _ => panic!("expected Many after second push"),
        }
    }

    #[test]
    fn test_push_to_many() {
        let mut sv: SmallVec<i32> = SmallVec::Many(vec![1, 2]);
        sv.push(3);
        match &sv {
            SmallVec::Many(v) => assert_eq!(v, &[1, 2, 3]),
            _ => panic!("expected Many"),
        }
    }

    #[test]
    fn test_append_empty_to_none() {
        let mut sv: SmallVec<i32> = SmallVec::None;
        let mut other = vec![];
        sv.append(&mut other);
        match &sv {
            SmallVec::Many(v) => assert!(v.is_empty()),
            _ => panic!("expected Many (empty)"),
        }
    }

    #[test]
    fn test_append_to_none() {
        let mut sv: SmallVec<i32> = SmallVec::None;
        let mut other = vec![5, 6];
        sv.append(&mut other);
        match &sv {
            SmallVec::Many(v) => assert_eq!(v, &[5, 6]),
            _ => panic!("expected Many"),
        }
        assert!(other.is_empty());
    }

    #[test]
    fn test_append_to_one() {
        let mut sv: SmallVec<i32> = SmallVec::One(1);
        let mut other = vec![2, 3];
        sv.append(&mut other);
        match &sv {
            SmallVec::Many(v) => assert_eq!(v, &[1, 2, 3]),
            _ => panic!("expected Many"),
        }
    }

    #[test]
    fn test_append_to_many() {
        let mut sv: SmallVec<i32> = SmallVec::Many(vec![1, 2]);
        let mut other = vec![3, 4];
        sv.append(&mut other);
        match &sv {
            SmallVec::Many(v) => assert_eq!(v, &[1, 2, 3, 4]),
            _ => panic!("expected Many"),
        }
    }

    #[test]
    fn test_last_mut_none() {
        let mut sv: SmallVec<i32> = SmallVec::None;
        assert!(sv.last_mut().is_none());
    }

    #[test]
    fn test_last_mut_one() {
        let mut sv: SmallVec<i32> = SmallVec::One(7);
        *sv.last_mut().unwrap() = 8;
        match &sv {
            SmallVec::One(x) => assert_eq!(*x, 8),
            _ => panic!(),
        }
    }

    #[test]
    fn test_last_mut_many() {
        let mut sv: SmallVec<i32> = SmallVec::Many(vec![1, 2, 3]);
        *sv.last_mut().unwrap() = 99;
        match &sv {
            SmallVec::Many(v) => assert_eq!(v[2], 99),
            _ => panic!(),
        }
    }
}
