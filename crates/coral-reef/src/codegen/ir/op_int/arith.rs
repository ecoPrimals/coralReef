// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright © 2025-2026 ecoPrimals
// Derived from Collabora, Ltd. (2022)
//! Integer arithmetic operations.

use super::*;
#[repr(C)]
#[derive(Clone, SrcsAsSlice, DstsAsSlice)]
pub struct OpIAbs {
    #[dst_type(GPR)]
    pub dst: Dst,

    #[src_type(ALU)]
    pub src: Src,
}

impl Foldable for OpIAbs {
    fn fold(&self, _sm: &dyn ShaderModel, f: &mut OpFoldData<'_>) {
        let src = f.get_u32_src(self, &self.src);
        let dst = (src as i32).unsigned_abs();
        f.set_u32_dst(self, &self.dst, dst);
    }
}

impl DisplayOp for OpIAbs {
    fn fmt_op(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "iabs {}", self.src)
    }
}
impl_display_for_op!(OpIAbs);

/// Only used on SM50
#[repr(C)]
#[derive(Clone, SrcsAsSlice, DstsAsSlice)]
pub struct OpIAdd2 {
    #[dst_types(GPR, Carry)]
    #[dst_names(dst, carry_out)]
    pub dsts: [Dst; 2],

    #[src_type(I32)]
    pub srcs: [Src; 2],
}

impl Foldable for OpIAdd2 {
    fn fold(&self, _sm: &dyn ShaderModel, f: &mut OpFoldData<'_>) {
        let srcs = [
            f.get_u32_src(self, &self.srcs[0]),
            f.get_u32_src(self, &self.srcs[1]),
        ];

        let mut sum = 0_u64;
        for i in 0..2 {
            if self.srcs[i].modifier.is_ineg() {
                // This is a very literal interpretation of 2's compliment.
                // This is not -u64::from(src) or u64::from(-src).
                sum += u64::from(!srcs[i]) + 1;
            } else {
                sum += u64::from(srcs[i]);
            }
        }

        f.set_u32_dst(self, self.dst(), sum as u32);
        f.set_carry_dst(self, self.carry_out(), sum >= (1 << 32));
    }
}

impl DisplayOp for OpIAdd2 {
    fn fmt_op(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "iadd2 {} {}", self.srcs[0], self.srcs[1])
    }
}
impl_display_for_op!(OpIAdd2);

/// Only used on SM50
#[repr(C)]
#[derive(Clone, SrcsAsSlice, DstsAsSlice)]
pub struct OpIAdd2X {
    #[dst_types(GPR, Carry)]
    #[dst_names(dst, carry_out)]
    pub dsts: [Dst; 2],

    #[src_types(B32, B32, Carry)]
    #[src_names(src_a, src_b, carry_in)]
    pub srcs: [Src; 3],
}

impl Foldable for OpIAdd2X {
    fn fold(&self, _sm: &dyn ShaderModel, f: &mut OpFoldData<'_>) {
        let srcs = [
            f.get_u32_bnot_src(self, &self.srcs[0]),
            f.get_u32_bnot_src(self, &self.srcs[1]),
        ];
        let carry_in = f.get_carry_src(self, self.carry_in());

        let sum = u64::from(srcs[0]) + u64::from(srcs[1]) + u64::from(carry_in);

        f.set_u32_dst(self, self.dst(), sum as u32);
        f.set_carry_dst(self, self.carry_out(), sum >= (1 << 32));
    }
}

impl DisplayOp for OpIAdd2X {
    fn fmt_op(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "iadd2.x {} {}", self.srcs[0], self.srcs[1])?;
        if !self.carry_in().is_zero() {
            write!(f, " {}", self.carry_in())?;
        }
        Ok(())
    }
}
impl_display_for_op!(OpIAdd2X);

#[repr(C)]
#[derive(Clone, SrcsAsSlice, DstsAsSlice)]
pub struct OpIAdd3 {
    #[dst_types(GPR, Pred, Pred)]
    #[dst_names(dst, overflow_0, overflow_1)]
    pub dsts: [Dst; 3],

    #[src_type(I32)]
    pub srcs: [Src; 3],
}

impl Foldable for OpIAdd3 {
    fn fold(&self, _sm: &dyn ShaderModel, f: &mut OpFoldData<'_>) {
        let srcs = [
            f.get_u32_src(self, &self.srcs[0]),
            f.get_u32_src(self, &self.srcs[1]),
            f.get_u32_src(self, &self.srcs[2]),
        ];

        let mut sum = 0_u64;
        for i in 0..3 {
            if self.srcs[i].modifier.is_ineg() {
                // This is a very literal interpretation of 2's compliment.
                // This is not -u64::from(src) or u64::from(-src).
                sum += u64::from(!srcs[i]) + 1;
            } else {
                sum += u64::from(srcs[i]);
            }
        }

        f.set_u32_dst(self, self.dst(), sum as u32);
        f.set_pred_dst(self, self.overflow_0(), sum >= 1_u64 << 32);
        f.set_pred_dst(self, self.overflow_1(), sum >= 2_u64 << 32);
    }
}

impl DisplayOp for OpIAdd3 {
    fn fmt_op(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "iadd3 {} {} {}",
            self.srcs[0], self.srcs[1], self.srcs[2],
        )
    }
}
impl_display_for_op!(OpIAdd3);

#[repr(C)]
#[derive(Clone, SrcsAsSlice, DstsAsSlice)]
pub struct OpIAdd3X {
    #[dst_types(GPR, Pred, Pred)]
    #[dst_names(dst, overflow_0, overflow_1)]
    pub dsts: [Dst; 3],

    #[src_types(B32, B32, B32, Pred, Pred)]
    #[src_names(src_a, src_b, src_c, carry_0, carry_1)]
    pub srcs: [Src; 5],
}

impl Foldable for OpIAdd3X {
    fn fold(&self, _sm: &dyn ShaderModel, f: &mut OpFoldData<'_>) {
        let srcs = [
            f.get_u32_bnot_src(self, &self.srcs[0]),
            f.get_u32_bnot_src(self, &self.srcs[1]),
            f.get_u32_bnot_src(self, &self.srcs[2]),
        ];
        let carry = [
            f.get_pred_src(self, self.carry_0()),
            f.get_pred_src(self, self.carry_1()),
        ];

        let mut sum = 0_u64;
        for i in 0..3 {
            sum += u64::from(srcs[i]);
        }

        for i in 0..2 {
            sum += u64::from(carry[i]);
        }

        f.set_u32_dst(self, self.dst(), sum as u32);
        f.set_pred_dst(self, self.overflow_0(), sum >= 1_u64 << 32);
        f.set_pred_dst(self, self.overflow_1(), sum >= 2_u64 << 32);
    }
}

impl DisplayOp for OpIAdd3X {
    fn fmt_op(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "iadd3.x {} {} {} {} {}",
            self.srcs[0],
            self.srcs[1],
            self.srcs[2],
            self.carry_0(),
            self.carry_1()
        )
    }
}
impl_display_for_op!(OpIAdd3X);

#[repr(C)]
#[derive(SrcsAsSlice, DstsAsSlice)]
pub struct OpIDp4 {
    #[dst_type(GPR)]
    pub dst: Dst,

    pub src_types: [IntType; 2],

    #[src_type(I32)]
    pub srcs: [Src; 3],
}

impl DisplayOp for OpIDp4 {
    fn fmt_op(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "idp4{}{} {} {} {}",
            self.src_types[0], self.src_types[1], self.srcs[0], self.srcs[1], self.srcs[2],
        )
    }
}
impl_display_for_op!(OpIDp4);

#[repr(C)]
#[derive(SrcsAsSlice, DstsAsSlice)]
pub struct OpIMad {
    #[dst_type(GPR)]
    pub dst: Dst,

    #[src_type(ALU)]
    pub srcs: [Src; 3],

    pub signed: bool,
}

impl DisplayOp for OpIMad {
    fn fmt_op(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "imad {} {} {}", self.srcs[0], self.srcs[1], self.srcs[2],)
    }
}
impl_display_for_op!(OpIMad);

/// Only used on SM50
#[repr(C)]
#[derive(SrcsAsSlice, DstsAsSlice)]
pub struct OpIMul {
    #[dst_type(GPR)]
    pub dst: Dst,

    #[src_type(ALU)]
    pub srcs: [Src; 2],

    pub signed: [bool; 2],
    pub high: bool,
}

impl DisplayOp for OpIMul {
    fn fmt_op(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "imul")?;
        if self.high {
            write!(f, ".hi")?;
        }
        let src_type = |signed| if signed { ".s32" } else { ".u32" };
        write!(
            f,
            "{}{}",
            src_type(self.signed[0]),
            src_type(self.signed[1])
        )?;
        write!(f, " {} {}", self.srcs[0], self.srcs[1])
    }
}
impl_display_for_op!(OpIMul);

#[repr(C)]
#[derive(SrcsAsSlice, DstsAsSlice)]
pub struct OpIMad64 {
    #[dst_type(Vec)]
    pub dst: Dst,

    #[src_type(ALU)]
    pub srcs: [Src; 3],

    pub signed: bool,
}

impl DisplayOp for OpIMad64 {
    fn fmt_op(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "imad64 {} {} {}",
            self.srcs[0], self.srcs[1], self.srcs[2],
        )
    }
}
impl_display_for_op!(OpIMad64);

#[repr(C)]
#[derive(Clone, SrcsAsSlice, DstsAsSlice)]
pub struct OpIMnMx {
    #[dst_type(GPR)]
    pub dst: Dst,

    pub cmp_type: IntCmpType,

    #[src_types(ALU, ALU, Pred)]
    #[src_names(src_a, src_b, min)]
    pub srcs: [Src; 3],
}

impl Foldable for OpIMnMx {
    fn fold(&self, _sm: &dyn ShaderModel, f: &mut OpFoldData<'_>) {
        let (a, b) = (
            f.get_u32_bnot_src(self, &self.srcs[0]),
            f.get_u32_bnot_src(self, &self.srcs[1]),
        );
        let min = f.get_pred_src(self, self.min());

        let res = match (min, self.cmp_type) {
            (true, IntCmpType::U32) => a.min(b),
            (true, IntCmpType::I32) => (a as i32).min(b as i32) as u32,
            (false, IntCmpType::U32) => a.max(b),
            (false, IntCmpType::I32) => (a as i32).max(b as i32) as u32,
        };

        f.set_u32_dst(self, &self.dst, res);
    }
}

impl DisplayOp for OpIMnMx {
    fn fmt_op(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "imnmx{} {} {} {}",
            self.cmp_type,
            self.srcs[0],
            self.srcs[1],
            self.min()
        )
    }
}
impl_display_for_op!(OpIMnMx);

#[repr(C)]
#[derive(Clone, SrcsAsSlice, DstsAsSlice)]
pub struct OpISetP {
    #[dst_type(Pred)]
    pub dst: Dst,

    pub set_op: PredSetOp,
    pub cmp_op: IntCmpOp,
    pub cmp_type: IntCmpType,
    pub ex: bool,

    #[src_types(ALU, ALU, Pred, Pred)]
    #[src_names(src_a, src_b, accum, low_cmp)]
    pub srcs: [Src; 4],
}

impl Foldable for OpISetP {
    fn fold(&self, sm: &dyn ShaderModel, f: &mut OpFoldData<'_>) {
        let x = f.get_u32_src(self, &self.srcs[0]);
        let y = f.get_u32_src(self, &self.srcs[1]);
        let accum = f.get_pred_src(self, self.accum());
        let low_cmp = f.get_pred_src(self, self.low_cmp());

        let cmp = if self.cmp_type.is_signed() {
            let x = x as i32;
            let y = y as i32;
            match &self.cmp_op {
                IntCmpOp::False => false,
                IntCmpOp::True => true,
                IntCmpOp::Eq => x == y,
                IntCmpOp::Ne => x != y,
                IntCmpOp::Lt => x < y,
                IntCmpOp::Le => x <= y,
                IntCmpOp::Gt => x > y,
                IntCmpOp::Ge => x >= y,
            }
        } else {
            match &self.cmp_op {
                IntCmpOp::False => false,
                IntCmpOp::True => true,
                IntCmpOp::Eq => x == y,
                IntCmpOp::Ne => x != y,
                IntCmpOp::Lt => x < y,
                IntCmpOp::Le => x <= y,
                IntCmpOp::Gt => x > y,
                IntCmpOp::Ge => x >= y,
            }
        };

        let cmp_op_is_const = matches!(self.cmp_op, IntCmpOp::False | IntCmpOp::True);
        let cmp = if self.ex && x == y && !cmp_op_is_const {
            // Pre-Volta, isetp.x takes the accumulator into account.  If we
            // want to support this, we need to take an an accumulator into
            // account.  Disallow it for now.
            assert!(sm.sm() >= 70);
            low_cmp
        } else {
            cmp
        };

        let dst = self.set_op.eval(cmp, accum);

        f.set_pred_dst(self, &self.dst, dst);
    }
}

impl DisplayOp for OpISetP {
    fn fmt_op(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "isetp{}{}", self.cmp_op, self.cmp_type)?;
        if !self.set_op.is_trivial(self.accum()) {
            write!(f, "{}", self.set_op)?;
        }
        if self.ex {
            write!(f, ".ex")?;
        }
        write!(f, " {} {}", self.srcs[0], self.srcs[1])?;
        if !self.set_op.is_trivial(self.accum()) {
            write!(f, " {}", self.accum())?;
        }
        if self.ex {
            write!(f, " {}", self.low_cmp())?;
        }
        Ok(())
    }
}
impl_display_for_op!(OpISetP);

#[cfg(test)]
mod tests {
    use super::*;

    fn zero_src() -> Src {
        Src::ZERO
    }

    fn imm_src(u: u32) -> Src {
        Src::new_imm_u32(u)
    }

    #[test]
    fn test_op_iabs_display() {
        let op = OpIAbs {
            dst: Dst::None,
            src: imm_src(0x42),
        };
        let s = format!("{op}");
        assert!(s.contains("iabs"));
        assert!(s.contains("0x42"));
    }

    #[test]
    fn test_op_iadd2_display() {
        let op = OpIAdd2 {
            dsts: [Dst::None, Dst::None],
            srcs: [imm_src(1), imm_src(2)],
        };
        let s = format!("{op}");
        assert!(s.contains("iadd2"));
    }

    #[test]
    fn test_op_iadd2x_display() {
        let op = OpIAdd2X {
            dsts: [Dst::None, Dst::None],
            srcs: [zero_src(), zero_src(), Src::ZERO],
        };
        let s = format!("{op}");
        assert!(s.contains("iadd2.x"));
    }

    #[test]
    fn test_op_iadd3_display() {
        let op = OpIAdd3 {
            dsts: [Dst::None, Dst::None, Dst::None],
            srcs: [imm_src(1), imm_src(2), imm_src(3)],
        };
        let s = format!("{op}");
        assert!(s.contains("iadd3"));
    }

    #[test]
    fn test_op_iadd3x_display() {
        let op = OpIAdd3X {
            dsts: [Dst::None, Dst::None, Dst::None],
            srcs: [
                zero_src(),
                zero_src(),
                zero_src(),
                Src::new_imm_bool(false),
                Src::new_imm_bool(false),
            ],
        };
        let s = format!("{op}");
        assert!(s.contains("iadd3.x"));
    }

    #[test]
    fn test_op_idp4_display() {
        let op = OpIDp4 {
            dst: Dst::None,
            src_types: [IntType::U32, IntType::I32],
            srcs: [zero_src(), zero_src(), zero_src()],
        };
        let s = format!("{op}");
        assert!(s.contains("idp4"));
        assert!(s.contains(".u32"));
        assert!(s.contains(".i32"));
    }

    #[test]
    fn test_op_imad_display() {
        let op = OpIMad {
            dst: Dst::None,
            srcs: [imm_src(1), imm_src(2), imm_src(3)],
            signed: true,
        };
        let s = format!("{op}");
        assert!(s.contains("imad"));
    }

    #[test]
    fn test_op_imul_display() {
        let op = OpIMul {
            dst: Dst::None,
            srcs: [zero_src(), zero_src()],
            signed: [true, false],
            high: true,
        };
        let s = format!("{op}");
        assert!(s.contains("imul"));
        assert!(s.contains(".hi"));
    }

    #[test]
    fn test_op_imad64_display() {
        let op = OpIMad64 {
            dst: Dst::None,
            srcs: [zero_src(), zero_src(), zero_src()],
            signed: false,
        };
        let s = format!("{op}");
        assert!(s.contains("imad64"));
    }

    #[test]
    fn test_op_imnmx_display() {
        let op = OpIMnMx {
            dst: Dst::None,
            cmp_type: IntCmpType::U32,
            srcs: [zero_src(), imm_src(5), Src::new_imm_bool(true)],
        };
        let s = format!("{op}");
        assert!(s.contains("imnmx"));
        assert!(s.contains(".u32"));
    }

    #[test]
    fn test_op_isetp_display() {
        let op = OpISetP {
            dst: Dst::None,
            set_op: PredSetOp::Or,
            cmp_op: IntCmpOp::Eq,
            cmp_type: IntCmpType::I32,
            ex: false,
            srcs: [
                zero_src(),
                imm_src(1),
                Src::new_imm_bool(false),
                Src::new_imm_bool(false),
            ],
        };
        let s = format!("{op}");
        assert!(s.contains("isetp"));
        assert!(s.contains(".eq"));
        assert!(s.contains(".i32"));
    }

    #[test]
    fn test_op_isetp_ex_display() {
        let op = OpISetP {
            dst: Dst::None,
            set_op: PredSetOp::And,
            cmp_op: IntCmpOp::Lt,
            cmp_type: IntCmpType::U32,
            ex: true,
            srcs: [
                zero_src(),
                zero_src(),
                Src::new_imm_bool(true),
                Src::new_imm_bool(false),
            ],
        };
        let s = format!("{op}");
        assert!(s.contains(".ex"));
    }
}
