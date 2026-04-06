// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright © 2025-2026 ecoPrimals

use bitview::*;

pub use super::builder::{Builder, InstrBuilder, SSABuilder, SSAInstrBuilder};
use super::legalize::LegalizeBuilder;
use super::nv::shader_header::{OutputTopology, PixelImap};
pub use super::ssa_value::*;
use coral_reef_stubs::as_slice::*;
use nak_ir_proc::*;
use std::cmp::min;
use std::fmt;

mod types;
pub use types::*;

mod regs;
pub use regs::*;

mod src_dst;
pub use src_dst::*;

mod src;
pub use src::*;

mod pred;
pub use pred::*;

mod fold;
pub use fold::*;

use std::fmt::Write;
use std::iter::Zip;
use std::slice;

fn all_dsts_uniform(dsts: &[Dst]) -> bool {
    let mut uniform = None;
    for dst in dsts {
        let dst_uniform = match dst {
            Dst::None => continue,
            Dst::Reg(r) => r.is_uniform(),
            Dst::SSA(r) => r.file().is_uniform(),
        };
        assert!(uniform.is_none() || uniform == Some(dst_uniform));
        uniform = Some(dst_uniform);
    }
    uniform == Some(true)
}

#[repr(u8)]
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum DstType {
    Pred,
    GPR,
    F16,
    F16v2,
    F32,
    F64,
    Carry,
    Bar,
    Vec,
}

impl DstType {
    const DEFAULT: Self = Self::Vec;
}

pub type DstTypeList = AttrList<DstType>;

pub trait DstsAsSlice: AsSlice<Dst, Attr = DstType> {
    fn dsts_as_slice(&self) -> &[Dst] {
        self.as_slice()
    }

    fn dsts_as_mut_slice(&mut self) -> &mut [Dst] {
        self.as_mut_slice()
    }

    fn dst_idx(&self, dst: &Dst) -> usize {
        let slice = self.dsts_as_slice();
        let base = slice.as_ptr() as usize;
        let elem = std::ptr::from_ref(dst) as usize;
        let idx = (elem - base) / std::mem::size_of::<Dst>();
        assert!(idx < slice.len(), "dst not in slice");
        idx
    }
}

impl<T: AsSlice<Dst, Attr = DstType>> DstsAsSlice for T {}

pub trait IsUniform {
    fn is_uniform(&self) -> bool;
}

impl<T: DstsAsSlice> IsUniform for T {
    fn is_uniform(&self) -> bool {
        all_dsts_uniform(self.dsts_as_slice())
    }
}

fn fmt_dst_slice(f: &mut fmt::Formatter<'_>, dsts: &[Dst]) -> fmt::Result {
    if dsts.is_empty() {
        return Ok(());
    }

    // Figure out the last non-null dst
    //
    // Note: By making the top inclusive and starting at 0, we ensure that
    // at least one dst always gets printed.
    let mut last_dst = 0;
    for (i, dst) in dsts.iter().enumerate() {
        if !dst.is_none() {
            last_dst = i;
        }
    }

    for i in 0..(last_dst + 1) {
        if i != 0 {
            write!(f, " ")?;
        }
        write!(f, "{}", &dsts[i])?;
    }
    Ok(())
}

pub trait DisplayOp: DstsAsSlice {
    fn fmt_dsts(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt_dst_slice(f, self.dsts_as_slice())
    }

    fn fmt_op(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result;
}

/// Closure-to-Display adapter for reusing `fmt::Formatter` across calls.
///
/// Wraps an `Fn(&mut Formatter) -> fmt::Result` so it can participate
/// in `write!` / `format!` chains without intermediate `String` allocations.
pub struct Fmt<F>(pub F)
where
    F: Fn(&mut fmt::Formatter) -> fmt::Result;

impl<F> fmt::Display for Fmt<F>
where
    F: Fn(&mut fmt::Formatter) -> fmt::Result,
{
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        (self.0)(f)
    }
}

macro_rules! impl_display_for_op {
    ($op: ident) => {
        impl fmt::Display for $op {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                let mut s = String::new();
                write!(s, "{}", Fmt(|f| self.fmt_dsts(f)))?;
                if !s.is_empty() {
                    write!(f, "{} = ", s)?;
                }
                self.fmt_op(f)
            }
        }
    };
}

mod op_float;
pub use op_float::*;

mod op_int;
pub use op_int::*;

mod op_conv;
pub use op_conv::*;

mod op_tex;
pub use op_tex::*;

mod op_mem;
pub use op_mem::*;

mod op_cf;
pub use op_cf::*;

mod op_misc;
pub use op_misc::*;

mod op;
pub use op::*;

mod instr;
pub use instr::*;

mod program;
pub use program::*;

mod shader_io;
pub use shader_io::*;

mod shader_model;
pub use shader_model::*;

mod shader_info;
pub use shader_info::*;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_src_zero_and_imm() {
        assert!(Src::ZERO.is_zero());
        assert!(Src::new_imm_u32(0).is_zero());
        assert!(!Src::new_imm_u32(1).is_zero());
        assert!(Src::new_imm_u32(42).is_nonzero());
    }

    #[test]
    fn test_src_modifiers() {
        let imm = Src::new_imm_u32(0x8000_0000);
        let neg = imm.fneg();
        assert!(!neg.is_unmodified());
    }

    #[test]
    fn test_src_bool() {
        assert!(Src::new_imm_bool(true).is_true());
        assert!(Src::new_imm_bool(true).as_bool() == Some(true));
        assert!(Src::new_imm_bool(false).as_bool() == Some(false));
    }

    #[test]
    fn test_pred_from_bool() {
        let p_true: Pred = true.into();
        let p_false: Pred = false.into();
        assert!(p_true.is_true());
        assert!(p_false.is_false());
    }

    #[test]
    fn test_pred_bnot() {
        let p: Pred = true.into();
        let p_inv = p.bnot();
        assert!(p_inv.is_false());
    }

    #[test]
    fn test_pred_ref_is_none() {
        assert!(PredRef::None.is_none());
    }

    #[test]
    fn test_src_display() {
        let s = format!("{}", Src::ZERO);
        assert!(!s.is_empty());
    }
}
