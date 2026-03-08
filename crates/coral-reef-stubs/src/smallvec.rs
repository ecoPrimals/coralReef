// SPDX-License-Identifier: AGPL-3.0-only
//! Small vector with zero/one/many optimization.
//!
//! Used for instruction mapping results where most operations
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
        match std::mem::replace(self, Self::None) {
            Self::None => *self = Self::One(item),
            Self::One(existing) => {
                *self = Self::Many(vec![existing, item]);
            }
            Self::Many(mut v) => {
                v.push(item);
                *self = Self::Many(v);
            }
        }
    }

    /// Get a mutable reference to the last item.
    pub fn last_mut(&mut self) -> Option<&mut T> {
        match self {
            Self::None => None,
            Self::One(x) => Some(x),
            Self::Many(v) => v.last_mut(),
        }
    }

    /// Appends items from another `SmallVec`.
    pub fn append(&mut self, other: &mut Vec<T>) {
        match std::mem::replace(self, Self::None) {
            Self::None => *self = Self::Many(std::mem::take(other)),
            Self::One(item) => {
                let mut v = Vec::with_capacity(1 + other.len());
                v.push(item);
                v.append(other);
                *self = Self::Many(v);
            }
            Self::Many(mut v) => {
                v.append(other);
                *self = Self::Many(v);
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

    #[test]
    fn test_push_three_transitions() {
        let mut sv: SmallVec<i32> = SmallVec::None;
        sv.push(1);
        sv.push(2);
        sv.push(3);
        match &sv {
            SmallVec::Many(v) => assert_eq!(v, &[1, 2, 3]),
            _ => panic!("expected Many after 3 pushes"),
        }
    }

    #[test]
    fn test_append_empty_to_one() {
        let mut sv: SmallVec<i32> = SmallVec::One(42);
        let mut other = vec![];
        sv.append(&mut other);
        match &sv {
            SmallVec::Many(v) => assert_eq!(v, &[42]),
            _ => panic!("expected Many"),
        }
    }

    #[test]
    fn test_smallvec_with_custom_n() {
        let mut sv: SmallVec<i32, 8> = SmallVec::None;
        sv.push(1);
        match &sv {
            SmallVec::One(x) => assert_eq!(*x, 1),
            _ => panic!(),
        }
    }

    #[test]
    fn test_append_drains_other() {
        let mut sv: SmallVec<i32> = SmallVec::One(1);
        let mut other = vec![2, 3];
        sv.append(&mut other);
        assert!(other.is_empty());
        match &sv {
            SmallVec::Many(v) => assert_eq!(v, &[1, 2, 3]),
            _ => panic!(),
        }
    }

    #[test]
    fn test_multiple_push_to_many() {
        let mut sv: SmallVec<i32> = SmallVec::Many(vec![1]);
        sv.push(2);
        sv.push(3);
        sv.push(4);
        match &sv {
            SmallVec::Many(v) => assert_eq!(v, &[1, 2, 3, 4]),
            _ => panic!(),
        }
    }
}
