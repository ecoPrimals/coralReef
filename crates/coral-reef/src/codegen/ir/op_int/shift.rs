// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright © 2025-2026 ecoPrimals
// Derived from Collabora, Ltd. (2022)
//! Shift and address computation operations.

use super::*;
#[repr(C)]
#[derive(Clone, SrcsAsSlice, DstsAsSlice)]
pub struct OpLea {
    #[dst_types(GPR, Pred)]
    #[dst_names(dst, overflow)]
    pub dsts: [Dst; 2],

    #[src_types(ALU, I32, ALU)]
    #[src_names(a, b, a_high)]
    pub srcs: [Src; 3],

    pub shift: u8,
    pub dst_high: bool,
    pub intermediate_mod: SrcMod, // Modifier for shifted temporary (a << shift)
}

impl Foldable for OpLea {
    fn fold(&self, _sm: &dyn ShaderModel, f: &mut OpFoldData<'_>) {
        let a = f.get_u32_src(self, self.a());
        let mut b = f.get_u32_src(self, self.b());
        let a_high = f.get_u32_src(self, self.a_high());

        let mut overflow = false;

        let mut shift_result = if self.dst_high {
            let a = a as u64;
            let a_high = a_high as u64;
            let a = (a_high << 32) | a;

            (a >> (32 - self.shift)) as u32
        } else {
            a << self.shift
        };

        if self.intermediate_mod.is_ineg() {
            let o;
            (shift_result, o) = u32::overflowing_add(!shift_result, 1);
            overflow |= o;
        }

        if self.b().modifier.is_ineg() {
            let o;
            (b, o) = u32::overflowing_add(!b, 1);
            overflow |= o;
        }

        let (dst, o) = u32::overflowing_add(shift_result, b);
        overflow |= o;

        f.set_u32_dst(self, self.dst(), dst);
        f.set_pred_dst(self, self.overflow(), overflow);
    }
}

impl DisplayOp for OpLea {
    fn fmt_op(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "lea")?;
        if self.dst_high {
            write!(f, ".hi")?;
        }
        write!(f, " {} {} {}", self.a(), self.shift, self.b())?;
        if self.dst_high {
            write!(f, " {}", self.a_high())?;
        }
        Ok(())
    }
}
impl_display_for_op!(OpLea);

#[repr(C)]
#[derive(Clone, SrcsAsSlice, DstsAsSlice)]
pub struct OpLeaX {
    #[dst_types(GPR, Pred)]
    #[dst_names(dst, overflow)]
    pub dsts: [Dst; 2],

    #[src_types(ALU, B32, ALU, Pred)]
    #[src_names(a, b, a_high, carry)]
    pub srcs: [Src; 4],

    pub shift: u8,
    pub dst_high: bool,
    pub intermediate_mod: SrcMod, // Modifier for shifted temporary (a << shift)
}

impl Foldable for OpLeaX {
    fn fold(&self, _sm: &dyn ShaderModel, f: &mut OpFoldData<'_>) {
        let a = f.get_u32_src(self, self.a());
        let mut b = f.get_u32_src(self, self.b());
        let a_high = f.get_u32_src(self, self.a_high());
        let carry = f.get_pred_src(self, self.carry());

        let mut overflow = false;

        let mut shift_result = if self.dst_high {
            let a = a as u64;
            let a_high = a_high as u64;
            let a = (a_high << 32) | a;

            (a >> (32 - self.shift)) as u32
        } else {
            a << self.shift
        };

        if self.intermediate_mod.is_bnot() {
            shift_result = !shift_result;
        }

        if self.b().modifier.is_bnot() {
            b = !b;
        }

        let (dst, o) = u32::overflowing_add(shift_result, b);
        overflow |= o;

        let (dst, o) = u32::overflowing_add(dst, if carry { 1 } else { 0 });
        overflow |= o;

        f.set_u32_dst(self, self.dst(), dst);
        f.set_pred_dst(self, self.overflow(), overflow);
    }
}

impl DisplayOp for OpLeaX {
    fn fmt_op(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "lea.x")?;
        if self.dst_high {
            write!(f, ".hi")?;
        }
        write!(f, " {} {} {}", self.a(), self.shift, self.b())?;
        if self.dst_high {
            write!(f, " {}", self.a_high())?;
        }
        write!(f, " {}", self.carry())?;
        Ok(())
    }
}
impl_display_for_op!(OpLeaX);

fn reduce_shift_imm(shift: &mut Src, wrap: bool, bits: u32) {
    debug_assert!(shift.modifier.is_none());
    if let SrcRef::Imm32(shift) = &mut shift.reference {
        if wrap {
            *shift &= bits - 1;
        } else {
            *shift = std::cmp::min(*shift, bits);
        }
    }
}

#[repr(C)]
#[derive(Clone, SrcsAsSlice, DstsAsSlice)]
pub struct OpShf {
    #[dst_type(GPR)]
    pub dst: Dst,

    #[src_types(GPR, ALU, ALU)]
    #[src_names(low, high, shift)]
    pub srcs: [Src; 3],

    pub right: bool,
    pub wrap: bool,
    pub data_type: IntType,
    pub dst_high: bool,
}

impl OpShf {
    /// Reduces the shift immediate, if any.  Out-of-range shifts are either
    /// clamped to the maximum or wrapped as needed.
    pub fn reduce_shift_imm(&mut self) {
        let bits = self
            .data_type
            .bits()
            .try_into()
            .expect("IntType bits must fit in u32");
        let wrap = self.wrap;
        reduce_shift_imm(self.shift_mut(), wrap, bits);
    }
}

impl Foldable for OpShf {
    fn fold(&self, sm: &dyn ShaderModel, f: &mut OpFoldData<'_>) {
        let low = f.get_u32_src(self, self.low());
        let high = f.get_u32_src(self, self.high());
        let shift = f.get_u32_src(self, self.shift());

        let bits: u32 = self
            .data_type
            .bits()
            .try_into()
            .expect("IntType bits must fit in u32");
        let shift = if self.wrap {
            shift & (bits - 1)
        } else {
            min(shift, bits)
        };

        let x = u64::from(low) | (u64::from(high) << 32);
        let shifted = if sm.sm() < 70 && self.dst_high && self.data_type != IntType::I64 {
            if self.right {
                x.checked_shr(shift).unwrap_or(0)
            } else {
                x.checked_shl(shift).unwrap_or(0)
            }
        } else if self.data_type.is_signed() {
            if self.right {
                let x = x as i64;
                x.checked_shr(shift).unwrap_or(x >> 63) as u64
            } else {
                x.checked_shl(shift).unwrap_or(0)
            }
        } else if self.right {
            x.checked_shr(shift).unwrap_or(0)
        } else {
            x.checked_shl(shift).unwrap_or(0)
        };

        let dst = if (sm.sm() < 70 && !self.right) || self.dst_high {
            (shifted >> 32) as u32
        } else {
            shifted as u32
        };

        f.set_u32_dst(self, &self.dst, dst);
    }
}

impl DisplayOp for OpShf {
    fn fmt_op(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "shf")?;
        if self.right {
            write!(f, ".r")?;
        } else {
            write!(f, ".l")?;
        }
        if self.wrap {
            write!(f, ".w")?;
        }
        write!(f, "{}", self.data_type)?;
        if self.dst_high {
            write!(f, ".hi")?;
        }
        write!(f, " {} {} {}", self.low(), self.high(), self.shift())
    }
}
impl_display_for_op!(OpShf);

/// Only used on SM50
#[repr(C)]
#[derive(Clone, SrcsAsSlice, DstsAsSlice)]
pub struct OpShl {
    #[dst_type(GPR)]
    pub dst: Dst,

    #[src_types(GPR, ALU)]
    #[src_names(src, shift)]
    pub srcs: [Src; 2],

    pub wrap: bool,
}

impl OpShl {
    /// Reduces the shift immediate, if any.  Out-of-range shifts are either
    /// clamped to the maximum or wrapped as needed.
    pub fn reduce_shift_imm(&mut self) {
        let wrap = self.wrap;
        reduce_shift_imm(self.shift_mut(), wrap, 32);
    }
}

impl DisplayOp for OpShl {
    fn fmt_op(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "shl")?;
        if self.wrap {
            write!(f, ".w")?;
        }
        write!(f, " {} {}", self.src(), self.shift())
    }
}

impl Foldable for OpShl {
    fn fold(&self, _sm: &dyn ShaderModel, f: &mut OpFoldData<'_>) {
        let x = f.get_u32_src(self, self.src());
        let shift = f.get_u32_src(self, self.shift());

        let shift = if self.wrap {
            shift & 31
        } else {
            min(shift, 32)
        };
        let dst = x.checked_shl(shift).unwrap_or(0);
        f.set_u32_dst(self, &self.dst, dst);
    }
}

/// Only used on SM50
#[repr(C)]
#[derive(Clone, SrcsAsSlice, DstsAsSlice)]
pub struct OpShr {
    #[dst_type(GPR)]
    pub dst: Dst,

    #[src_types(GPR, ALU)]
    #[src_names(src, shift)]
    pub srcs: [Src; 2],

    pub wrap: bool,
    pub signed: bool,
}

impl DisplayOp for OpShr {
    fn fmt_op(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "shr")?;
        if self.wrap {
            write!(f, ".w")?;
        }
        if !self.signed {
            write!(f, ".u32")?;
        }
        write!(f, " {} {}", self.src(), self.shift())
    }
}

impl OpShr {
    /// Reduces the shift immediate, if any.  Out-of-range shifts are either
    /// clamped to the maximum or wrapped as needed.
    pub fn reduce_shift_imm(&mut self) {
        let wrap = self.wrap;
        reduce_shift_imm(self.shift_mut(), wrap, 32);
    }
}

impl Foldable for OpShr {
    fn fold(&self, _sm: &dyn ShaderModel, f: &mut OpFoldData<'_>) {
        let x = f.get_u32_src(self, self.src());
        let shift = f.get_u32_src(self, self.shift());

        let shift = if self.wrap {
            shift & 31
        } else {
            min(shift, 32)
        };
        let dst = if self.signed {
            let x = x as i32;
            x.checked_shr(shift).unwrap_or(x >> 31) as u32
        } else {
            x.checked_shr(shift).unwrap_or(0)
        };
        f.set_u32_dst(self, &self.dst, dst);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn imm_src(u: u32) -> Src {
        Src::new_imm_u32(u)
    }

    #[test]
    fn test_op_lea_display() {
        let op = OpLea {
            dsts: [Dst::None, Dst::None],
            srcs: [imm_src(1), imm_src(2), imm_src(0)],
            shift: 4,
            dst_high: false,
            intermediate_mod: SrcMod::None,
        };
        let s = format!("{op}");
        assert!(s.contains("lea"));
        assert!(s.contains('4'));
    }

    #[test]
    fn test_op_lea_hi_display() {
        let op = OpLea {
            dsts: [Dst::None, Dst::None],
            srcs: [imm_src(1), imm_src(2), imm_src(0)],
            shift: 8,
            dst_high: true,
            intermediate_mod: SrcMod::None,
        };
        let s = format!("{op}");
        assert!(s.contains("lea.hi"));
        assert!(s.contains('8'));
    }

    #[test]
    fn test_op_leax_display() {
        let op = OpLeaX {
            dsts: [Dst::None, Dst::None],
            srcs: [imm_src(1), imm_src(2), imm_src(0), Src::new_imm_bool(false)],
            shift: 4,
            dst_high: false,
            intermediate_mod: SrcMod::None,
        };
        let s = format!("{op}");
        assert!(s.contains("lea.x"));
    }

    #[test]
    fn test_op_shf_display_left() {
        let op = OpShf {
            dst: Dst::None,
            srcs: [imm_src(0), imm_src(0), imm_src(4)],
            right: false,
            wrap: false,
            data_type: IntType::U32,
            dst_high: false,
        };
        let s = format!("{op}");
        assert!(s.contains("shf.l"));
        assert!(s.contains(".u32"));
    }

    #[test]
    fn test_op_shf_display_right_wrap() {
        let op = OpShf {
            dst: Dst::None,
            srcs: [imm_src(0), imm_src(0), imm_src(4)],
            right: true,
            wrap: true,
            data_type: IntType::I64,
            dst_high: true,
        };
        let s = format!("{op}");
        assert!(s.contains("shf.r"));
        assert!(s.contains(".w"));
        assert!(s.contains(".i64"));
        assert!(s.contains(".hi"));
    }

    #[test]
    fn test_op_shf_reduce_shift_imm_clamp() {
        let mut op = OpShf {
            dst: Dst::None,
            srcs: [imm_src(0), imm_src(0), Src::new_imm_u32(100)],
            right: false,
            wrap: false,
            data_type: IntType::U32,
            dst_high: false,
        };
        op.reduce_shift_imm();
        if let SrcRef::Imm32(v) = op.shift().reference {
            assert_eq!(v, 32, "clamp should cap at 32 for u32");
        } else {
            crate::codegen::ice!("shift should remain Imm32 after reduce");
        }
    }

    #[test]
    fn test_op_shf_reduce_shift_imm_wrap() {
        let mut op = OpShf {
            dst: Dst::None,
            srcs: [imm_src(0), imm_src(0), Src::new_imm_u32(37)],
            right: false,
            wrap: true,
            data_type: IntType::U32,
            dst_high: false,
        };
        op.reduce_shift_imm();
        if let SrcRef::Imm32(v) = op.shift().reference {
            assert_eq!(v, 5, "wrap 37 & 31 = 5");
        } else {
            crate::codegen::ice!("shift should remain Imm32 after reduce");
        }
    }

    #[test]
    fn test_op_shl_struct() {
        let op = OpShl {
            dst: Dst::None,
            srcs: [imm_src(1), imm_src(4)],
            wrap: false,
        };
        assert!(!op.wrap);
        assert!(matches!(op.shift().reference, SrcRef::Imm32(4)));
    }

    #[test]
    fn test_op_shl_reduce_shift_imm() {
        let mut op = OpShl {
            dst: Dst::None,
            srcs: [imm_src(1), Src::new_imm_u32(50)],
            wrap: false,
        };
        op.reduce_shift_imm();
        if let SrcRef::Imm32(v) = op.shift().reference {
            assert_eq!(v, 32);
        }
    }

    #[test]
    fn test_op_shr_signed() {
        let op = OpShr {
            dst: Dst::None,
            srcs: [imm_src(0x8000_0000), imm_src(1)],
            wrap: false,
            signed: true,
        };
        assert!(op.signed);
    }

    #[test]
    fn test_op_shr_unsigned() {
        let op = OpShr {
            dst: Dst::None,
            srcs: [imm_src(0x8000_0000), imm_src(1)],
            wrap: false,
            signed: false,
        };
        assert!(!op.signed);
    }

    #[test]
    fn test_op_shr_reduce_shift_imm_wrap() {
        let mut op = OpShr {
            dst: Dst::None,
            srcs: [imm_src(1), Src::new_imm_u32(35)],
            wrap: true,
            signed: false,
        };
        op.reduce_shift_imm();
        if let SrcRef::Imm32(v) = op.shift().reference {
            assert_eq!(v, 3, "35 & 31 = 3");
        }
    }
}
