// Copyright © 2022 Collabora, Ltd.
// SPDX-License-Identifier: MIT
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
    #[allow(dead_code)]
    pub fn eval(&self, a: bool, b: bool) -> bool {
        match self {
            PredSetOp::And => a & b,
            PredSetOp::Or => a | b,
            PredSetOp::Xor => a ^ b,
        }
    }

    pub fn is_trivial(&self, accum: &Src) -> bool {
        if let Some(b) = accum.as_bool() {
            match self {
                PredSetOp::And => b,
                PredSetOp::Or | PredSetOp::Xor => !b,
            }
        } else {
            false
        }
    }
}

impl fmt::Display for PredSetOp {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            PredSetOp::And => write!(f, ".and"),
            PredSetOp::Or => write!(f, ".or"),
            PredSetOp::Xor => write!(f, ".xor"),
        }
    }
}

#[allow(dead_code)]
#[derive(Clone, Copy, Eq, Hash, PartialEq)]
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
    pub fn flip(self) -> FloatCmpOp {
        match self {
            FloatCmpOp::OrdEq | FloatCmpOp::OrdNe | FloatCmpOp::UnordEq | FloatCmpOp::UnordNe => {
                self
            }
            FloatCmpOp::OrdLt => FloatCmpOp::OrdGt,
            FloatCmpOp::OrdLe => FloatCmpOp::OrdGe,
            FloatCmpOp::OrdGt => FloatCmpOp::OrdLt,
            FloatCmpOp::OrdGe => FloatCmpOp::OrdLe,
            FloatCmpOp::UnordLt => FloatCmpOp::UnordGt,
            FloatCmpOp::UnordLe => FloatCmpOp::UnordGe,
            FloatCmpOp::UnordGt => FloatCmpOp::UnordLt,
            FloatCmpOp::UnordGe => FloatCmpOp::UnordLe,
            FloatCmpOp::IsNum | FloatCmpOp::IsNan => panic!("Cannot flip unop"),
        }
    }
}

impl fmt::Display for FloatCmpOp {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            FloatCmpOp::OrdEq => write!(f, ".eq"),
            FloatCmpOp::OrdNe => write!(f, ".ne"),
            FloatCmpOp::OrdLt => write!(f, ".lt"),
            FloatCmpOp::OrdLe => write!(f, ".le"),
            FloatCmpOp::OrdGt => write!(f, ".gt"),
            FloatCmpOp::OrdGe => write!(f, ".ge"),
            FloatCmpOp::UnordEq => write!(f, ".equ"),
            FloatCmpOp::UnordNe => write!(f, ".neu"),
            FloatCmpOp::UnordLt => write!(f, ".ltu"),
            FloatCmpOp::UnordLe => write!(f, ".leu"),
            FloatCmpOp::UnordGt => write!(f, ".gtu"),
            FloatCmpOp::UnordGe => write!(f, ".geu"),
            FloatCmpOp::IsNum => write!(f, ".num"),
            FloatCmpOp::IsNan => write!(f, ".nan"),
        }
    }
}

#[allow(dead_code)]
#[derive(Clone, Copy, Eq, Hash, PartialEq)]
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
    pub fn flip(self) -> IntCmpOp {
        match self {
            IntCmpOp::False | IntCmpOp::True | IntCmpOp::Eq | IntCmpOp::Ne => self,
            IntCmpOp::Lt => IntCmpOp::Gt,
            IntCmpOp::Le => IntCmpOp::Ge,
            IntCmpOp::Gt => IntCmpOp::Lt,
            IntCmpOp::Ge => IntCmpOp::Le,
        }
    }
}

impl fmt::Display for IntCmpOp {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            IntCmpOp::False => write!(f, ".f"),
            IntCmpOp::True => write!(f, ".t"),
            IntCmpOp::Eq => write!(f, ".eq"),
            IntCmpOp::Ne => write!(f, ".ne"),
            IntCmpOp::Lt => write!(f, ".lt"),
            IntCmpOp::Le => write!(f, ".le"),
            IntCmpOp::Gt => write!(f, ".gt"),
            IntCmpOp::Ge => write!(f, ".ge"),
        }
    }
}

#[derive(Clone, Copy, Eq, Hash, PartialEq)]
pub enum IntCmpType {
    U32,
    I32,
}

impl IntCmpType {
    pub fn is_signed(&self) -> bool {
        match self {
            IntCmpType::U32 => false,
            IntCmpType::I32 => true,
        }
    }
}

impl fmt::Display for IntCmpType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            IntCmpType::U32 => write!(f, ".u32"),
            IntCmpType::I32 => write!(f, ".i32"),
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
            LogicOp2::And => write!(f, "and"),
            LogicOp2::Or => write!(f, "or"),
            LogicOp2::Xor => write!(f, "xor"),
            LogicOp2::PassB => write!(f, "pass_b"),
        }
    }
}

impl LogicOp2 {
    pub fn to_lut(self) -> LogicOp3 {
        match self {
            LogicOp2::And => LogicOp3::new_lut(&|x, y, _| x & y),
            LogicOp2::Or => LogicOp3::new_lut(&|x, y, _| x | y),
            LogicOp2::Xor => LogicOp3::new_lut(&|x, y, _| x ^ y),
            LogicOp2::PassB => LogicOp3::new_lut(&|_, b, _| b),
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
    pub fn new_lut<F: Fn(u8, u8, u8) -> u8>(f: &F) -> LogicOp3 {
        LogicOp3 {
            lut: f(
                LogicOp3::SRC_MASKS[0],
                LogicOp3::SRC_MASKS[1],
                LogicOp3::SRC_MASKS[2],
            ),
        }
    }

    pub fn new_const(val: bool) -> LogicOp3 {
        LogicOp3 {
            lut: if val { !0 } else { 0 },
        }
    }

    pub fn src_used(&self, src_idx: usize) -> bool {
        let mask = LogicOp3::SRC_MASKS[src_idx];
        let shift = LogicOp3::SRC_MASKS[src_idx].trailing_zeros();
        self.lut & !mask != (self.lut >> shift) & !mask
    }

    pub fn fix_src(&mut self, src_idx: usize, val: bool) {
        let mask = LogicOp3::SRC_MASKS[src_idx];
        let shift = LogicOp3::SRC_MASKS[src_idx].trailing_zeros();
        if val {
            let t_bits = self.lut & mask;
            self.lut = t_bits | (t_bits >> shift);
        } else {
            let f_bits = self.lut & !mask;
            self.lut = (f_bits << shift) | f_bits;
        }
    }

    pub fn invert_src(&mut self, src_idx: usize) {
        let mask = LogicOp3::SRC_MASKS[src_idx];
        let shift = LogicOp3::SRC_MASKS[src_idx].trailing_zeros();
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
}
