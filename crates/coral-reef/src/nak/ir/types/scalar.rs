// Copyright © 2022 Collabora, Ltd.
// SPDX-License-Identifier: MIT
//! Scalar numeric types — float and integer type enums.

use super::*;

#[derive(Clone, Copy, Eq, Hash, PartialEq)]
pub enum FloatType {
    F16,
    F32,
    F64,
}

impl TryFrom<usize> for FloatType {
    type Error = &'static str;

    fn try_from(bits: usize) -> Result<Self, Self::Error> {
        match bits {
            16 => Ok(FloatType::F16),
            32 => Ok(FloatType::F32),
            64 => Ok(FloatType::F64),
            _ => Err("invalid float type bit width (expected 16, 32, or 64)"),
        }
    }
}

impl FloatType {
    /// # Panics
    ///
    /// Panics if `bytes` is not 16, 32, or 64.
    pub fn from_bits(bytes: usize) -> FloatType {
        FloatType::try_from(bytes).expect("invalid float type bit width")
    }

    pub fn bits(&self) -> usize {
        match self {
            FloatType::F16 => 16,
            FloatType::F32 => 32,
            FloatType::F64 => 64,
        }
    }
}

impl fmt::Display for FloatType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            FloatType::F16 => write!(f, ".f16"),
            FloatType::F32 => write!(f, ".f32"),
            FloatType::F64 => write!(f, ".f64"),
        }
    }
}

#[derive(Clone, Copy, Eq, Hash, PartialEq)]
pub enum FRndMode {
    NearestEven,
    NegInf,
    PosInf,
    Zero,
}

impl fmt::Display for FRndMode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            FRndMode::NearestEven => write!(f, ".re"),
            FRndMode::NegInf => write!(f, ".rm"),
            FRndMode::PosInf => write!(f, ".rp"),
            FRndMode::Zero => write!(f, ".rz"),
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum IntType {
    U8,
    I8,
    U16,
    I16,
    U32,
    I32,
    U64,
    I64,
}

impl IntType {
    /// Try to create from bit width and signedness.
    pub fn try_from_bits(bits: usize, is_signed: bool) -> Option<IntType> {
        Some(match (bits, is_signed) {
            (8, false) => IntType::U8,
            (8, true) => IntType::I8,
            (16, false) => IntType::U16,
            (16, true) => IntType::I16,
            (32, false) => IntType::U32,
            (32, true) => IntType::I32,
            (64, false) => IntType::U64,
            (64, true) => IntType::I64,
            _ => return None,
        })
    }

    /// # Panics
    ///
    /// Panics if `bits` is not 8, 16, 32, or 64.
    pub fn from_bits(bits: usize, is_signed: bool) -> IntType {
        Self::try_from_bits(bits, is_signed).expect("invalid integer type bit width")
    }

    pub fn is_signed(&self) -> bool {
        match self {
            IntType::U8 | IntType::U16 | IntType::U32 | IntType::U64 => false,
            IntType::I8 | IntType::I16 | IntType::I32 | IntType::I64 => true,
        }
    }

    pub fn bits(&self) -> usize {
        match self {
            IntType::U8 | IntType::I8 => 8,
            IntType::U16 | IntType::I16 => 16,
            IntType::U32 | IntType::I32 => 32,
            IntType::U64 | IntType::I64 => 64,
        }
    }
}

impl fmt::Display for IntType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            IntType::U8 => write!(f, ".u8"),
            IntType::I8 => write!(f, ".i8"),
            IntType::U16 => write!(f, ".u16"),
            IntType::I16 => write!(f, ".i16"),
            IntType::U32 => write!(f, ".u32"),
            IntType::I32 => write!(f, ".i32"),
            IntType::U64 => write!(f, ".u64"),
            IntType::I64 => write!(f, ".i64"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_float_type_from_bits() {
        assert!(matches!(FloatType::from_bits(16), FloatType::F16));
        assert!(matches!(FloatType::from_bits(32), FloatType::F32));
        assert!(matches!(FloatType::from_bits(64), FloatType::F64));
    }

    #[test]
    #[should_panic(expected = "invalid float type bit width")]
    fn test_float_type_from_bits_invalid() {
        FloatType::from_bits(8);
    }

    #[test]
    fn test_float_type_try_from() {
        assert!(FloatType::try_from(16_usize).is_ok());
        assert!(FloatType::try_from(32_usize).is_ok());
        assert!(FloatType::try_from(64_usize).is_ok());
        assert!(FloatType::try_from(8_usize).is_err());
        assert!(FloatType::try_from(128_usize).is_err());
    }

    #[test]
    fn test_float_type_bits() {
        assert_eq!(FloatType::F16.bits(), 16);
        assert_eq!(FloatType::F32.bits(), 32);
        assert_eq!(FloatType::F64.bits(), 64);
    }

    #[test]
    fn test_float_type_display() {
        assert_eq!(format!("{}", FloatType::F16), ".f16");
        assert_eq!(format!("{}", FloatType::F32), ".f32");
        assert_eq!(format!("{}", FloatType::F64), ".f64");
    }

    #[test]
    fn test_int_type_from_bits() {
        assert!(matches!(IntType::from_bits(8, false), IntType::U8));
        assert!(matches!(IntType::from_bits(8, true), IntType::I8));
        assert!(matches!(IntType::from_bits(16, false), IntType::U16));
        assert!(matches!(IntType::from_bits(16, true), IntType::I16));
        assert!(matches!(IntType::from_bits(32, false), IntType::U32));
        assert!(matches!(IntType::from_bits(32, true), IntType::I32));
        assert!(matches!(IntType::from_bits(64, false), IntType::U64));
        assert!(matches!(IntType::from_bits(64, true), IntType::I64));
    }

    #[test]
    #[should_panic(expected = "invalid integer type bit width")]
    fn test_int_type_from_bits_invalid() {
        IntType::from_bits(7, false);
    }

    #[test]
    fn test_int_type_try_from_bits() {
        assert!(IntType::try_from_bits(8, true).is_some());
        assert!(IntType::try_from_bits(16, false).is_some());
        assert!(IntType::try_from_bits(32, true).is_some());
        assert!(IntType::try_from_bits(64, false).is_some());
        assert!(IntType::try_from_bits(7, false).is_none());
        assert!(IntType::try_from_bits(128, true).is_none());
    }

    #[test]
    fn test_int_type_bits() {
        assert_eq!(IntType::U8.bits(), 8);
        assert_eq!(IntType::I32.bits(), 32);
        assert_eq!(IntType::U64.bits(), 64);
    }

    #[test]
    fn test_int_type_is_signed() {
        assert!(!IntType::U8.is_signed());
        assert!(!IntType::U32.is_signed());
        assert!(IntType::I8.is_signed());
        assert!(IntType::I32.is_signed());
    }

    #[test]
    fn test_int_type_display() {
        assert_eq!(format!("{}", IntType::U8), ".u8");
        assert_eq!(format!("{}", IntType::I32), ".i32");
        assert_eq!(format!("{}", IntType::U64), ".u64");
    }

    #[test]
    fn test_frnd_mode_display() {
        assert_eq!(format!("{}", FRndMode::NearestEven), ".re");
        assert_eq!(format!("{}", FRndMode::NegInf), ".rm");
        assert_eq!(format!("{}", FRndMode::PosInf), ".rp");
        assert_eq!(format!("{}", FRndMode::Zero), ".rz");
    }
}
