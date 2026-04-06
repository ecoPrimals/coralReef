// SPDX-License-Identifier: AGPL-3.0-or-later

use super::super::*;

#[repr(C)]
#[derive(SrcsAsSlice, DstsAsSlice)]
pub struct OpDAdd {
    #[dst_type(F64)]
    pub dst: Dst,

    #[src_type(F64)]
    pub srcs: [Src; 2],

    pub rnd_mode: FRndMode,
}

impl DisplayOp for OpDAdd {
    fn fmt_op(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "dadd")?;
        if self.rnd_mode != FRndMode::NearestEven {
            write!(f, "{}", self.rnd_mode)?;
        }
        write!(f, " {} {}", self.srcs[0], self.srcs[1],)
    }
}
impl_display_for_op!(OpDAdd);

#[repr(C)]
#[derive(SrcsAsSlice, DstsAsSlice)]
pub struct OpDMul {
    #[dst_type(F64)]
    pub dst: Dst,

    #[src_type(F64)]
    pub srcs: [Src; 2],

    pub rnd_mode: FRndMode,
}

impl DisplayOp for OpDMul {
    fn fmt_op(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "dmul")?;
        if self.rnd_mode != FRndMode::NearestEven {
            write!(f, "{}", self.rnd_mode)?;
        }
        write!(f, " {} {}", self.srcs[0], self.srcs[1],)
    }
}
impl_display_for_op!(OpDMul);

#[repr(C)]
#[derive(SrcsAsSlice, DstsAsSlice)]
pub struct OpDFma {
    #[dst_type(F64)]
    pub dst: Dst,

    #[src_type(F64)]
    pub srcs: [Src; 3],

    pub rnd_mode: FRndMode,
}

impl DisplayOp for OpDFma {
    fn fmt_op(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "dfma")?;
        if self.rnd_mode != FRndMode::NearestEven {
            write!(f, "{}", self.rnd_mode)?;
        }
        write!(f, " {} {} {}", self.srcs[0], self.srcs[1], self.srcs[2])
    }
}
impl_display_for_op!(OpDFma);

/// Placeholder op for f64 sqrt. Lowered to Newton-Raphson via MUFU.Rsq64H + DFMA.
#[repr(C)]
#[derive(SrcsAsSlice, DstsAsSlice)]
pub struct OpF64Sqrt {
    #[dst_type(F64)]
    pub dst: Dst,

    #[src_type(F64)]
    pub src: Src,
}

impl DisplayOp for OpF64Sqrt {
    fn fmt_op(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "f64sqrt {}", self.src)
    }
}
impl_display_for_op!(OpF64Sqrt);

/// Placeholder op for f64 reciprocal. Lowered to Newton-Raphson via MUFU.RCP64H + DFMA.
#[repr(C)]
#[derive(SrcsAsSlice, DstsAsSlice)]
pub struct OpF64Rcp {
    #[dst_type(F64)]
    pub dst: Dst,

    #[src_type(F64)]
    pub src: Src,
}

impl DisplayOp for OpF64Rcp {
    fn fmt_op(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "f64rcp {}", self.src)
    }
}
impl_display_for_op!(OpF64Rcp);

/// Placeholder op for f64 exp2. Lowered to Horner polynomial via DFMA.
#[repr(C)]
#[derive(SrcsAsSlice, DstsAsSlice)]
pub struct OpF64Exp2 {
    #[dst_type(F64)]
    pub dst: Dst,

    #[src_type(F64)]
    pub src: Src,
}

impl DisplayOp for OpF64Exp2 {
    fn fmt_op(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "f64exp2 {}", self.src)
    }
}
impl_display_for_op!(OpF64Exp2);

/// Placeholder op for f64 log2. Lowered to MUFU.LOG2 f32 seed extended to f64.
#[repr(C)]
#[derive(SrcsAsSlice, DstsAsSlice)]
pub struct OpF64Log2 {
    #[dst_type(F64)]
    pub dst: Dst,

    #[src_type(F64)]
    pub src: Src,
}

impl DisplayOp for OpF64Log2 {
    fn fmt_op(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "f64log2 {}", self.src)
    }
}
impl_display_for_op!(OpF64Log2);

/// Placeholder op for f64 sin. Lowered to minimax polynomial.
#[repr(C)]
#[derive(SrcsAsSlice, DstsAsSlice)]
pub struct OpF64Sin {
    #[dst_type(F64)]
    pub dst: Dst,

    #[src_type(F64)]
    pub src: Src,
}

impl DisplayOp for OpF64Sin {
    fn fmt_op(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "f64sin {}", self.src)
    }
}
impl_display_for_op!(OpF64Sin);

/// Placeholder op for f64 cos. Lowered to minimax polynomial.
#[repr(C)]
#[derive(SrcsAsSlice, DstsAsSlice)]
pub struct OpF64Cos {
    #[dst_type(F64)]
    pub dst: Dst,

    #[src_type(F64)]
    pub src: Src,
}

impl DisplayOp for OpF64Cos {
    fn fmt_op(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "f64cos {}", self.src)
    }
}
impl_display_for_op!(OpF64Cos);

#[repr(C)]
#[derive(SrcsAsSlice, DstsAsSlice)]
pub struct OpDMnMx {
    #[dst_type(F64)]
    pub dst: Dst,

    #[src_types(F64, F64, Pred)]
    #[src_names(src_a, src_b, min)]
    pub srcs: [Src; 3],
}

impl DisplayOp for OpDMnMx {
    fn fmt_op(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "dmnmx {} {} {}", self.srcs[0], self.srcs[1], self.min())
    }
}
impl_display_for_op!(OpDMnMx);

#[repr(C)]
#[derive(Clone, SrcsAsSlice, DstsAsSlice)]
pub struct OpDSetP {
    #[dst_type(Pred)]
    pub dst: Dst,

    pub set_op: PredSetOp,
    pub cmp_op: FloatCmpOp,

    #[src_types(F64, F64, Pred)]
    #[src_names(src_a, src_b, accum)]
    pub srcs: [Src; 3],
}

impl Foldable for OpDSetP {
    fn fold(&self, _sm: &dyn ShaderModel, f: &mut OpFoldData<'_>) {
        let a = f.get_f64_src(self, &self.srcs[0]);
        let b = f.get_f64_src(self, &self.srcs[1]);
        let accum = f.get_pred_src(self, self.accum());

        let ordered = !a.is_nan() && !b.is_nan();
        let cmp_res = match self.cmp_op {
            FloatCmpOp::OrdEq => ordered && a == b,
            FloatCmpOp::OrdNe => ordered && a != b,
            FloatCmpOp::OrdLt => ordered && a < b,
            FloatCmpOp::OrdLe => ordered && a <= b,
            FloatCmpOp::OrdGt => ordered && a > b,
            FloatCmpOp::OrdGe => ordered && a >= b,
            FloatCmpOp::UnordEq => !ordered || a == b,
            FloatCmpOp::UnordNe => !ordered || a != b,
            FloatCmpOp::UnordLt => !ordered || a < b,
            FloatCmpOp::UnordLe => !ordered || a <= b,
            FloatCmpOp::UnordGt => !ordered || a > b,
            FloatCmpOp::UnordGe => !ordered || a >= b,
            FloatCmpOp::IsNum => ordered,
            FloatCmpOp::IsNan => !ordered,
        };
        let res = self.set_op.eval(cmp_res, accum);

        f.set_pred_dst(self, &self.dst, res);
    }
}

impl DisplayOp for OpDSetP {
    fn fmt_op(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "dsetp{}", self.cmp_op)?;
        if !self.set_op.is_trivial(self.accum()) {
            write!(f, "{}", self.set_op)?;
        }
        write!(f, " {} {}", self.srcs[0], self.srcs[1])?;
        if !self.set_op.is_trivial(self.accum()) {
            write!(f, " {}", self.accum())?;
        }
        Ok(())
    }
}
impl_display_for_op!(OpDSetP);
