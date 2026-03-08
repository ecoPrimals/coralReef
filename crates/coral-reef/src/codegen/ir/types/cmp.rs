// SPDX-License-Identifier: AGPL-3.0-only
// Copyright © 2025-2026 ecoPrimals
// Derived from Collabora, Ltd. (2022)
//! Comparison and logic operation types.

use std::ops::{BitAnd, BitOr, Not};

use super::*;

#[derive(Clone, Copy, Eq, Hash, PartialEq)]
pub enum PredSetOp {
    And,
    Or,
    Xor,
}

impl PredSetOp {
    #[allow(dead_code, reason = "IR API for future constant folding")]
    pub const fn eval(&self, a: bool, b: bool) -> bool {
        match self {
            Self::And => a & b,
            Self::Or => a | b,
            Self::Xor => a ^ b,
        }
    }

    pub fn is_trivial(&self, accum: &Src) -> bool {
        accum.as_bool().is_some_and(|b| match self {
            Self::And => b,
            Self::Or | Self::Xor => !b,
        })
    }
}

impl fmt::Display for PredSetOp {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::And => write!(f, ".and"),
            Self::Or => write!(f, ".or"),
            Self::Xor => write!(f, ".xor"),
        }
    }
}

#[allow(dead_code, reason = "ISA variant reserved for future encoding support")]
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum FloatCmpOp {
    OrdEq,
    OrdNe,
    OrdLt,
    OrdLe,
    OrdGt,
    OrdGe,
    UnordEq,
    UnordNe,
    UnordLt,
    UnordLe,
    UnordGt,
    UnordGe,
    IsNum,
    IsNan,
}

impl FloatCmpOp {
    pub fn flip(self) -> Self {
        match self {
            Self::OrdEq | Self::OrdNe | Self::UnordEq | Self::UnordNe => self,
            Self::OrdLt => Self::OrdGt,
            Self::OrdLe => Self::OrdGe,
            Self::OrdGt => Self::OrdLt,
            Self::OrdGe => Self::OrdLe,
            Self::UnordLt => Self::UnordGt,
            Self::UnordLe => Self::UnordGe,
            Self::UnordGt => Self::UnordLt,
            Self::UnordGe => Self::UnordLe,
            Self::IsNum | Self::IsNan => panic!("ICE: Cannot flip unop"),
        }
    }
}

impl fmt::Display for FloatCmpOp {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::OrdEq => write!(f, ".eq"),
            Self::OrdNe => write!(f, ".ne"),
            Self::OrdLt => write!(f, ".lt"),
            Self::OrdLe => write!(f, ".le"),
            Self::OrdGt => write!(f, ".gt"),
            Self::OrdGe => write!(f, ".ge"),
            Self::UnordEq => write!(f, ".equ"),
            Self::UnordNe => write!(f, ".neu"),
            Self::UnordLt => write!(f, ".ltu"),
            Self::UnordLe => write!(f, ".leu"),
            Self::UnordGt => write!(f, ".gtu"),
            Self::UnordGe => write!(f, ".geu"),
            Self::IsNum => write!(f, ".num"),
            Self::IsNan => write!(f, ".nan"),
        }
    }
}

#[allow(dead_code, reason = "ISA variant reserved for future encoding support")]
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum IntCmpOp {
    False,
    True,
    Eq,
    Ne,
    Lt,
    Le,
    Gt,
    Ge,
}

impl IntCmpOp {
    pub const fn flip(self) -> Self {
        match self {
            Self::False | Self::True | Self::Eq | Self::Ne => self,
            Self::Lt => Self::Gt,
            Self::Le => Self::Ge,
            Self::Gt => Self::Lt,
            Self::Ge => Self::Le,
        }
    }
}

impl fmt::Display for IntCmpOp {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::False => write!(f, ".f"),
            Self::True => write!(f, ".t"),
            Self::Eq => write!(f, ".eq"),
            Self::Ne => write!(f, ".ne"),
            Self::Lt => write!(f, ".lt"),
            Self::Le => write!(f, ".le"),
            Self::Gt => write!(f, ".gt"),
            Self::Ge => write!(f, ".ge"),
        }
    }
}

#[derive(Clone, Copy, Eq, Hash, PartialEq)]
pub enum IntCmpType {
    U32,
    I32,
}

impl IntCmpType {
    pub const fn is_signed(&self) -> bool {
        match self {
            Self::U32 => false,
            Self::I32 => true,
        }
    }
}

impl fmt::Display for IntCmpType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::U32 => write!(f, ".u32"),
            Self::I32 => write!(f, ".i32"),
        }
    }
}

#[derive(Clone, Copy, Eq, Hash, PartialEq)]
pub enum LogicOp2 {
    And,
    Or,
    Xor,
    PassB,
}

impl fmt::Display for LogicOp2 {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::And => write!(f, "and"),
            Self::Or => write!(f, "or"),
            Self::Xor => write!(f, "xor"),
            Self::PassB => write!(f, "pass_b"),
        }
    }
}

impl LogicOp2 {
    pub fn to_lut(self) -> LogicOp3 {
        match self {
            Self::And => LogicOp3::new_lut(&|x, y, _| x & y),
            Self::Or => LogicOp3::new_lut(&|x, y, _| x | y),
            Self::Xor => LogicOp3::new_lut(&|x, y, _| x ^ y),
            Self::PassB => LogicOp3::new_lut(&|_, b, _| b),
        }
    }
}

#[derive(Clone, Copy, Eq, Hash, PartialEq)]
pub struct LogicOp3 {
    pub lut: u8,
}

impl LogicOp3 {
    pub const SRC_MASKS: [u8; 3] = [0xf0, 0xcc, 0xaa];

    #[inline]
    pub fn new_lut<F: Fn(u8, u8, u8) -> u8>(f: &F) -> Self {
        Self {
            lut: f(Self::SRC_MASKS[0], Self::SRC_MASKS[1], Self::SRC_MASKS[2]),
        }
    }

    pub const fn new_const(val: bool) -> Self {
        Self {
            lut: if val { !0 } else { 0 },
        }
    }

    pub const fn src_used(&self, src_idx: usize) -> bool {
        let mask = Self::SRC_MASKS[src_idx];
        let shift = Self::SRC_MASKS[src_idx].trailing_zeros();
        self.lut & !mask != (self.lut >> shift) & !mask
    }

    pub const fn fix_src(&mut self, src_idx: usize, val: bool) {
        let mask = Self::SRC_MASKS[src_idx];
        let shift = Self::SRC_MASKS[src_idx].trailing_zeros();
        if val {
            let t_bits = self.lut & mask;
            self.lut = t_bits | (t_bits >> shift);
        } else {
            let f_bits = self.lut & !mask;
            self.lut = (f_bits << shift) | f_bits;
        }
    }

    pub const fn invert_src(&mut self, src_idx: usize) {
        let mask = Self::SRC_MASKS[src_idx];
        let shift = Self::SRC_MASKS[src_idx].trailing_zeros();
        let t_bits = self.lut & mask;
        let f_bits = self.lut & !mask;
        self.lut = (f_bits << shift) | (t_bits >> shift);
    }

    pub fn eval<T: BitAnd<Output = T> + BitOr<Output = T> + Copy + Not<Output = T>>(
        &self,
        x: T,
        y: T,
        z: T,
    ) -> T {
        let mut res = x & !x; // zero
        if (self.lut & (1 << 0)) != 0 {
            res = res | (!x & !y & !z);
        }
        if (self.lut & (1 << 1)) != 0 {
            res = res | (!x & !y & z);
        }
        if (self.lut & (1 << 2)) != 0 {
            res = res | (!x & y & !z);
        }
        if (self.lut & (1 << 3)) != 0 {
            res = res | (!x & y & z);
        }
        if (self.lut & (1 << 4)) != 0 {
            res = res | (x & !y & !z);
        }
        if (self.lut & (1 << 5)) != 0 {
            res = res | (x & !y & z);
        }
        if (self.lut & (1 << 6)) != 0 {
            res = res | (x & y & !z);
        }
        if (self.lut & (1 << 7)) != 0 {
            res = res | (x & y & z);
        }
        res
    }
}

impl fmt::Display for LogicOp3 {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "LUT[{:#x}]", self.lut)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_pred_set_op_eval() {
        // And
        assert!(PredSetOp::And.eval(true, true));
        assert!(!PredSetOp::And.eval(true, false));
        assert!(!PredSetOp::And.eval(false, true));
        assert!(!PredSetOp::And.eval(false, false));

        // Or
        assert!(PredSetOp::Or.eval(true, true));
        assert!(PredSetOp::Or.eval(true, false));
        assert!(PredSetOp::Or.eval(false, true));
        assert!(!PredSetOp::Or.eval(false, false));

        // Xor
        assert!(!PredSetOp::Xor.eval(true, true));
        assert!(PredSetOp::Xor.eval(true, false));
        assert!(PredSetOp::Xor.eval(false, true));
        assert!(!PredSetOp::Xor.eval(false, false));
    }

    #[test]
    fn test_pred_set_op_is_trivial() {
        let true_src = Src::new_imm_bool(true);
        let false_src = Src::new_imm_bool(false);

        // And: trivial when accum is true
        assert!(PredSetOp::And.is_trivial(&true_src));
        assert!(!PredSetOp::And.is_trivial(&false_src));

        // Or and Xor: trivial when accum is false
        assert!(PredSetOp::Or.is_trivial(&false_src));
        assert!(!PredSetOp::Or.is_trivial(&true_src));
        assert!(PredSetOp::Xor.is_trivial(&false_src));
        assert!(!PredSetOp::Xor.is_trivial(&true_src));
    }

    #[test]
    fn test_float_cmp_op_display() {
        assert_eq!(format!("{}", FloatCmpOp::OrdEq), ".eq");
        assert_eq!(format!("{}", FloatCmpOp::OrdNe), ".ne");
        assert_eq!(format!("{}", FloatCmpOp::OrdLt), ".lt");
        assert_eq!(format!("{}", FloatCmpOp::OrdLe), ".le");
        assert_eq!(format!("{}", FloatCmpOp::OrdGt), ".gt");
        assert_eq!(format!("{}", FloatCmpOp::OrdGe), ".ge");
        assert_eq!(format!("{}", FloatCmpOp::UnordEq), ".equ");
        assert_eq!(format!("{}", FloatCmpOp::UnordNe), ".neu");
        assert_eq!(format!("{}", FloatCmpOp::IsNum), ".num");
        assert_eq!(format!("{}", FloatCmpOp::IsNan), ".nan");
    }

    #[test]
    fn test_int_cmp_op_display() {
        assert_eq!(format!("{}", IntCmpOp::False), ".f");
        assert_eq!(format!("{}", IntCmpOp::True), ".t");
        assert_eq!(format!("{}", IntCmpOp::Eq), ".eq");
        assert_eq!(format!("{}", IntCmpOp::Ne), ".ne");
        assert_eq!(format!("{}", IntCmpOp::Lt), ".lt");
        assert_eq!(format!("{}", IntCmpOp::Le), ".le");
        assert_eq!(format!("{}", IntCmpOp::Gt), ".gt");
        assert_eq!(format!("{}", IntCmpOp::Ge), ".ge");
    }

    #[test]
    fn test_logic_op2_to_lut() {
        let and = LogicOp2::And.to_lut();
        assert!(and.eval(true, true, false));
        assert!(!and.eval(true, false, false));
        assert!(!and.eval(false, true, false));

        let or = LogicOp2::Or.to_lut();
        assert!(or.eval(true, false, false));
        assert!(or.eval(false, true, false));
        assert!(!or.eval(false, false, false));

        let xor = LogicOp2::Xor.to_lut();
        assert!(xor.eval(true, false, false));
        assert!(xor.eval(false, true, false));
        assert!(!xor.eval(true, true, false));

        let pass_b = LogicOp2::PassB.to_lut();
        assert!(pass_b.eval(false, true, false));
        assert!(!pass_b.eval(false, false, false));
    }

    #[test]
    fn test_logic_op3_eval() {
        let and = LogicOp3::new_lut(&|x, y, z| x & y & z);
        assert_eq!(and.eval(1u8, 1u8, 1u8), 1);
        assert_eq!(and.eval(1u8, 0u8, 1u8), 0);

        let or = LogicOp3::new_lut(&|x, y, z| x | y | z);
        assert_eq!(or.eval(1u8, 0u8, 0u8), 1);
        assert_eq!(or.eval(0u8, 0u8, 0u8), 0);
    }

    #[test]
    fn test_logic_op3_src_used() {
        let and = LogicOp2::And.to_lut();
        assert!(and.src_used(0));
        assert!(and.src_used(1));
        assert!(!and.src_used(2));

        let pass_b = LogicOp2::PassB.to_lut();
        assert!(!pass_b.src_used(0));
        assert!(pass_b.src_used(1));
        assert!(!pass_b.src_used(2));
    }

    #[test]
    fn test_logic_op3_fix_src() {
        // Use a 3-input AND so z actually affects the result
        let mut op = LogicOp3::new_lut(&|x, y, z| x & y & z);
        op.fix_src(2, true);
        // With z fixed to true: AND(x,y,z) = x & y
        assert!(op.eval(true, true, true));
        assert!(op.eval(true, true, false));

        let mut op2 = LogicOp3::new_lut(&|x, y, z| x & y & z);
        op2.fix_src(2, false);
        assert!(!op2.eval(true, true, true));
        assert!(!op2.eval(true, true, false));
    }

    #[test]
    fn test_logic_op3_invert_src() {
        let mut op = LogicOp3::new_lut(&|x, _y, _z| x);
        op.invert_src(0);
        assert!(!op.eval(true, true, true));
        assert!(op.eval(false, true, true));
    }

    #[test]
    fn test_logic_op3_new_const() {
        let t = LogicOp3::new_const(true);
        assert!(t.eval(true, true, true));
        assert!(t.eval(false, false, false));

        let f = LogicOp3::new_const(false);
        assert!(!f.eval(true, true, true));
        assert!(!f.eval(false, false, false));
    }

    #[test]
    fn test_int_cmp_op_flip() {
        assert_eq!(IntCmpOp::Lt.flip(), IntCmpOp::Gt);
        assert_eq!(IntCmpOp::Gt.flip(), IntCmpOp::Lt);
        assert_eq!(IntCmpOp::Le.flip(), IntCmpOp::Ge);
        assert_eq!(IntCmpOp::Ge.flip(), IntCmpOp::Le);
        assert_eq!(IntCmpOp::Eq.flip(), IntCmpOp::Eq);
        assert_eq!(IntCmpOp::Ne.flip(), IntCmpOp::Ne);
        assert_eq!(IntCmpOp::False.flip(), IntCmpOp::False);
        assert_eq!(IntCmpOp::True.flip(), IntCmpOp::True);
    }

    #[test]
    fn test_float_cmp_op_flip() {
        assert_eq!(FloatCmpOp::OrdLt.flip(), FloatCmpOp::OrdGt);
        assert_eq!(FloatCmpOp::OrdGt.flip(), FloatCmpOp::OrdLt);
        assert_eq!(FloatCmpOp::OrdLe.flip(), FloatCmpOp::OrdGe);
        assert_eq!(FloatCmpOp::OrdGe.flip(), FloatCmpOp::OrdLe);
        assert_eq!(FloatCmpOp::UnordLt.flip(), FloatCmpOp::UnordGt);
        assert_eq!(FloatCmpOp::UnordGt.flip(), FloatCmpOp::UnordLt);
        assert_eq!(FloatCmpOp::OrdEq.flip(), FloatCmpOp::OrdEq);
        assert_eq!(FloatCmpOp::OrdNe.flip(), FloatCmpOp::OrdNe);
    }

    #[test]
    fn test_int_cmp_type_is_signed() {
        assert!(!IntCmpType::U32.is_signed());
        assert!(IntCmpType::I32.is_signed());
    }
}
