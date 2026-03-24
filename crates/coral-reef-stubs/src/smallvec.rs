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
        *sv.last_mut().expect("One variant has a last element") = 8;
        match &sv {
            SmallVec::One(x) => assert_eq!(*x, 8),
            _ => panic!("expected One after last_mut assignment"),
        }
    }

    #[test]
    fn test_last_mut_many() {
        let mut sv: SmallVec<i32> = SmallVec::Many(vec![1, 2, 3]);
        *sv.last_mut().expect("Many variant is non-empty") = 99;
        match &sv {
            SmallVec::Many(v) => assert_eq!(v[2], 99),
            _ => panic!("expected Many after last_mut assignment"),
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

    #[test]
    fn test_many_branch_iterates_elements() {
        let sv: SmallVec<i32> = SmallVec::Many(vec![10, 20, 30]);
        let sum: i32 = match &sv {
            SmallVec::Many(v) => v.iter().copied().sum(),
            _ => panic!("expected Many for iteration"),
        };
        assert_eq!(sum, 60);
    }

    #[test]
    fn test_last_mut_after_many_pushes_keeps_heap_vec() {
        let mut sv: SmallVec<i32> = SmallVec::None;
        for i in 0..16 {
            sv.push(i);
        }
        match &mut sv {
            SmallVec::Many(v) => {
                assert_eq!(v.len(), 16);
                *v.last_mut().expect("non-empty many") = 99;
            }
            _ => panic!("expected heap Many after repeated push"),
        }
        match &sv {
            SmallVec::Many(v) => assert_eq!(v[15], 99),
            _ => panic!("expected Many"),
        }
    }

    #[test]
    fn append_many_to_none_replaces_with_drained_vec() {
        let mut sv: SmallVec<i32> = SmallVec::None;
        let mut heap = vec![10, 20, 30];
        sv.append(&mut heap);
        assert!(heap.is_empty());
        match &sv {
            SmallVec::Many(v) => assert_eq!(v, &[10, 20, 30]),
            _ => panic!("expected Many from append"),
        }
    }

    #[test]
    fn push_after_many_appended_from_none() {
        let mut sv: SmallVec<i32> = SmallVec::None;
        sv.append(&mut vec![1, 2]);
        sv.push(3);
        match &sv {
            SmallVec::Many(v) => assert_eq!(v, &[1, 2, 3]),
            _ => panic!("expected Many"),
        }
    }

    #[test]
    fn one_push_then_append_builds_many_with_correct_order() {
        let mut sv: SmallVec<i32> = SmallVec::One(0);
        sv.append(&mut vec![1, 2, 3]);
        match &sv {
            SmallVec::Many(v) => assert_eq!(v, &[0, 1, 2, 3]),
            _ => panic!("expected Many"),
        }
    }

    #[test]
    fn last_mut_none_stays_none() {
        let mut sv: SmallVec<String> = SmallVec::None;
        assert!(sv.last_mut().is_none());
    }

    #[test]
    fn push_string_one_and_many() {
        let mut sv: SmallVec<String> = SmallVec::None;
        sv.push("a".to_string());
        sv.push("b".to_string());
        if let SmallVec::Many(v) = &mut sv {
            assert_eq!(v.len(), 2);
            v[0].push('z');
        } else {
            panic!("expected Many");
        }
        if let SmallVec::Many(v) = &sv {
            assert_eq!(v[0], "az");
        } else {
            panic!("expected Many");
        }
    }
}
