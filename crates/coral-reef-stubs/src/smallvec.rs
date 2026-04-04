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
        assert!(
            matches!(&sv, SmallVec::None),
            "expected SmallVec::None after construction"
        );
    }

    #[test]
    fn test_create_one() {
        let sv: SmallVec<i32> = SmallVec::One(42);
        assert!(
            matches!(&sv, SmallVec::One(x) if *x == 42),
            "expected SmallVec::One(42)"
        );
    }

    #[test]
    fn test_create_many() {
        let sv: SmallVec<i32> = SmallVec::Many(vec![1, 2, 3]);
        assert!(
            matches!(&sv, SmallVec::Many(v) if v == &[1, 2, 3]),
            "expected SmallVec::Many([1, 2, 3])"
        );
    }

    #[test]
    fn test_push_to_none() {
        let mut sv: SmallVec<i32> = SmallVec::None;
        sv.push(10);
        assert!(
            matches!(&sv, SmallVec::One(x) if *x == 10),
            "expected SmallVec::One(10) after push to None"
        );
    }

    #[test]
    fn test_push_to_one_transitions_to_many() {
        let mut sv: SmallVec<i32> = SmallVec::One(1);
        sv.push(2);
        assert!(
            matches!(&sv, SmallVec::Many(v) if v == &[1, 2]),
            "expected SmallVec::Many([1, 2]) after second push"
        );
    }

    #[test]
    fn test_push_to_many() {
        let mut sv: SmallVec<i32> = SmallVec::Many(vec![1, 2]);
        sv.push(3);
        assert!(
            matches!(&sv, SmallVec::Many(v) if v == &[1, 2, 3]),
            "expected SmallVec::Many([1, 2, 3])"
        );
    }

    #[test]
    fn test_append_empty_to_none() {
        let mut sv: SmallVec<i32> = SmallVec::None;
        let mut other = vec![];
        sv.append(&mut other);
        assert!(
            matches!(&sv, SmallVec::Many(v) if v.is_empty()),
            "expected SmallVec::Many(empty) after appending empty vec to None"
        );
    }

    #[test]
    fn test_append_to_none() {
        let mut sv: SmallVec<i32> = SmallVec::None;
        let mut other = vec![5, 6];
        sv.append(&mut other);
        assert!(
            matches!(&sv, SmallVec::Many(v) if v == &[5, 6]),
            "expected SmallVec::Many([5, 6]) after append to None"
        );
        assert!(other.is_empty());
    }

    #[test]
    fn test_append_to_one() {
        let mut sv: SmallVec<i32> = SmallVec::One(1);
        let mut other = vec![2, 3];
        sv.append(&mut other);
        assert!(
            matches!(&sv, SmallVec::Many(v) if v == &[1, 2, 3]),
            "expected SmallVec::Many([1, 2, 3]) after append to One"
        );
    }

    #[test]
    fn test_append_to_many() {
        let mut sv: SmallVec<i32> = SmallVec::Many(vec![1, 2]);
        let mut other = vec![3, 4];
        sv.append(&mut other);
        assert!(
            matches!(&sv, SmallVec::Many(v) if v == &[1, 2, 3, 4]),
            "expected SmallVec::Many([1, 2, 3, 4]) after append to Many"
        );
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
        assert!(
            matches!(&sv, SmallVec::One(x) if *x == 8),
            "expected SmallVec::One(8) after last_mut assignment"
        );
    }

    #[test]
    fn test_last_mut_many() {
        let mut sv: SmallVec<i32> = SmallVec::Many(vec![1, 2, 3]);
        *sv.last_mut().expect("Many variant is non-empty") = 99;
        assert!(
            matches!(&sv, SmallVec::Many(v) if v[2] == 99),
            "expected last element 99 in SmallVec::Many"
        );
    }

    #[test]
    fn test_push_three_transitions() {
        let mut sv: SmallVec<i32> = SmallVec::None;
        sv.push(1);
        sv.push(2);
        sv.push(3);
        assert!(
            matches!(&sv, SmallVec::Many(v) if v == &[1, 2, 3]),
            "expected SmallVec::Many([1, 2, 3]) after three pushes"
        );
    }

    #[test]
    fn test_append_empty_to_one() {
        let mut sv: SmallVec<i32> = SmallVec::One(42);
        let mut other = vec![];
        sv.append(&mut other);
        assert!(
            matches!(&sv, SmallVec::Many(v) if v == &[42]),
            "expected SmallVec::Many([42]) after appending empty to One"
        );
    }

    #[test]
    fn test_smallvec_with_custom_n() {
        let mut sv: SmallVec<i32, 8> = SmallVec::None;
        sv.push(1);
        assert!(
            matches!(&sv, SmallVec::One(x) if *x == 1),
            "expected SmallVec::One(1) with custom const N after push from None"
        );
    }

    #[test]
    fn test_append_drains_other() {
        let mut sv: SmallVec<i32> = SmallVec::One(1);
        let mut other = vec![2, 3];
        sv.append(&mut other);
        assert!(other.is_empty());
        assert!(
            matches!(&sv, SmallVec::Many(v) if v == &[1, 2, 3]),
            "expected SmallVec::Many([1, 2, 3]) after append drains other vec"
        );
    }

    #[test]
    fn test_multiple_push_to_many() {
        let mut sv: SmallVec<i32> = SmallVec::Many(vec![1]);
        sv.push(2);
        sv.push(3);
        sv.push(4);
        assert!(
            matches!(&sv, SmallVec::Many(v) if v == &[1, 2, 3, 4]),
            "expected SmallVec::Many([1, 2, 3, 4]) after multiple pushes to Many"
        );
    }

    #[test]
    fn test_many_branch_iterates_elements() {
        let sv: SmallVec<i32> = SmallVec::Many(vec![10, 20, 30]);
        let SmallVec::Many(v) = &sv else {
            unreachable!("test constructs SmallVec::Many for iteration");
        };
        let sum: i32 = v.iter().copied().sum();
        assert_eq!(sum, 60);
    }

    #[test]
    fn test_last_mut_after_many_pushes_keeps_heap_vec() {
        let mut sv: SmallVec<i32> = SmallVec::None;
        for i in 0..16 {
            sv.push(i);
        }
        let SmallVec::Many(v) = &mut sv else {
            unreachable!("16 pushes from None always yield SmallVec::Many with heap Vec");
        };
        assert_eq!(v.len(), 16);
        *v.last_mut().expect("non-empty many") = 99;
        let SmallVec::Many(v) = &sv else {
            unreachable!("variant unchanged after last_mut write");
        };
        assert_eq!(v[15], 99);
    }

    #[test]
    fn append_many_to_none_replaces_with_drained_vec() {
        let mut sv: SmallVec<i32> = SmallVec::None;
        let mut heap = vec![10, 20, 30];
        sv.append(&mut heap);
        assert!(heap.is_empty());
        assert!(
            matches!(&sv, SmallVec::Many(v) if v == &[10, 20, 30]),
            "expected SmallVec::Many([10, 20, 30]) after append from None"
        );
    }

    #[test]
    fn push_after_many_appended_from_none() {
        let mut sv: SmallVec<i32> = SmallVec::None;
        sv.append(&mut vec![1, 2]);
        sv.push(3);
        assert!(
            matches!(&sv, SmallVec::Many(v) if v == &[1, 2, 3]),
            "expected SmallVec::Many([1, 2, 3]) after append then push"
        );
    }

    #[test]
    fn one_push_then_append_builds_many_with_correct_order() {
        let mut sv: SmallVec<i32> = SmallVec::One(0);
        sv.append(&mut vec![1, 2, 3]);
        assert!(
            matches!(&sv, SmallVec::Many(v) if v == &[0, 1, 2, 3]),
            "expected SmallVec::Many([0, 1, 2, 3]) after append to One"
        );
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
        let SmallVec::Many(v) = &mut sv else {
            unreachable!("two pushes from None produce SmallVec::Many");
        };
        assert_eq!(v.len(), 2);
        v[0].push('z');
        let SmallVec::Many(v) = &sv else {
            unreachable!("variant unchanged after mutating Many in place");
        };
        assert_eq!(v[0], "az");
    }
}
