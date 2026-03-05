// Copyright © 2022 Collabora, Ltd.
// SPDX-License-Identifier: MIT
//! The Op enum and Op trait implementations.

#![allow(clippy::wildcard_imports, clippy::enum_glob_use)]

use std::fmt;

use nak_ir_proc::*;

use super::op_cf::*;
use super::op_conv::*;
use super::op_float::*;
use super::op_int::*;
use super::op_mem::*;
use super::op_misc::*;
use super::op_tex::*;
use super::*;

#[derive(DisplayOp, DstsAsSlice, SrcsAsSlice, FromVariants)]
pub enum Op {
    FAdd(Box<OpFAdd>),
    FFma(Box<OpFFma>),
    FMnMx(Box<OpFMnMx>),
    FMul(Box<OpFMul>),
    Rro(Box<OpRro>),
    MuFu(Box<OpMuFu>),
    FSet(Box<OpFSet>),
    FSetP(Box<OpFSetP>),
    FSwzAdd(Box<OpFSwzAdd>),
    FSwz(Box<OpFSwz>),
    DAdd(Box<OpDAdd>),
    DFma(Box<OpDFma>),
    F64Exp2(Box<OpF64Exp2>),
    F64Log2(Box<OpF64Log2>),
    F64Rcp(Box<OpF64Rcp>),
    F64Sin(Box<OpF64Sin>),
    F64Cos(Box<OpF64Cos>),
    F64Sqrt(Box<OpF64Sqrt>),
    DMnMx(Box<OpDMnMx>),
    DMul(Box<OpDMul>),
    DSetP(Box<OpDSetP>),
    HAdd2(Box<OpHAdd2>),
    HFma2(Box<OpHFma2>),
    HMul2(Box<OpHMul2>),
    HSet2(Box<OpHSet2>),
    HSetP2(Box<OpHSetP2>),
    Imma(Box<OpImma>),
    Hmma(Box<OpHmma>),
    Ldsm(Box<OpLdsm>),
    HMnMx2(Box<OpHMnMx2>),
    BMsk(Box<OpBMsk>),
    BRev(Box<OpBRev>),
    Bfe(Box<OpBfe>),
    Flo(Box<OpFlo>),
    IAbs(Box<OpIAbs>),
    IAdd2(Box<OpIAdd2>),
    IAdd2X(Box<OpIAdd2X>),
    IAdd3(Box<OpIAdd3>),
    IAdd3X(Box<OpIAdd3X>),
    IDp4(Box<OpIDp4>),
    IMad(Box<OpIMad>),
    IMad64(Box<OpIMad64>),
    IMul(Box<OpIMul>),
    IMnMx(Box<OpIMnMx>),
    ISetP(Box<OpISetP>),
    Lea(Box<OpLea>),
    LeaX(Box<OpLeaX>),
    Lop2(Box<OpLop2>),
    Lop3(Box<OpLop3>),
    PopC(Box<OpPopC>),
    Shf(Box<OpShf>),
    Shl(Box<OpShl>),
    Shr(Box<OpShr>),
    F2F(Box<OpF2F>),
    F2FP(Box<OpF2FP>),
    F2I(Box<OpF2I>),
    I2F(Box<OpI2F>),
    I2I(Box<OpI2I>),
    FRnd(Box<OpFRnd>),
    Mov(Box<OpMov>),
    Movm(Box<OpMovm>),
    Prmt(Box<OpPrmt>),
    Sel(Box<OpSel>),
    Sgxt(Box<OpSgxt>),
    Shfl(Box<OpShfl>),
    PLop3(Box<OpPLop3>),
    PSetP(Box<OpPSetP>),
    R2UR(Box<OpR2UR>),
    Redux(Box<OpRedux>),
    Tex(Box<OpTex>),
    Tld(Box<OpTld>),
    Tld4(Box<OpTld4>),
    Tmml(Box<OpTmml>),
    Txd(Box<OpTxd>),
    Txq(Box<OpTxq>),
    SuLd(Box<OpSuLd>),
    SuSt(Box<OpSuSt>),
    SuAtom(Box<OpSuAtom>),
    SuClamp(Box<OpSuClamp>),
    SuBfm(Box<OpSuBfm>),
    SuEau(Box<OpSuEau>),
    IMadSp(Box<OpIMadSp>),
    SuLdGa(Box<OpSuLdGa>),
    SuStGa(Box<OpSuStGa>),
    Ld(Box<OpLd>),
    Ldc(Box<OpLdc>),
    LdSharedLock(Box<OpLdSharedLock>),
    St(Box<OpSt>),
    StSCheckUnlock(Box<OpStSCheckUnlock>),
    Atom(Box<OpAtom>),
    AL2P(Box<OpAL2P>),
    ALd(Box<OpALd>),
    ASt(Box<OpASt>),
    Ipa(Box<OpIpa>),
    LdTram(Box<OpLdTram>),
    CCtl(Box<OpCCtl>),
    MemBar(Box<OpMemBar>),
    BClear(Box<OpBClear>),
    BMov(Box<OpBMov>),
    Break(Box<OpBreak>),
    BSSy(Box<OpBSSy>),
    BSync(Box<OpBSync>),
    Bra(Box<OpBra>),
    SSy(OpSSy),
    Sync(OpSync),
    Brk(OpBrk),
    PBk(OpPBk),
    Cont(OpCont),
    PCnt(OpPCnt),
    Exit(OpExit),
    WarpSync(Box<OpWarpSync>),
    Bar(Box<OpBar>),
    TexDepBar(Box<OpTexDepBar>),
    CS2R(Box<OpCS2R>),
    Isberd(Box<OpIsberd>),
    ViLd(Box<OpViLd>),
    Kill(Box<OpKill>),
    Nop(OpNop),
    PixLd(Box<OpPixLd>),
    S2R(Box<OpS2R>),
    Vote(Box<OpVote>),
    Match(Box<OpMatch>),
    Undef(Box<OpUndef>),
    SrcBar(Box<OpSrcBar>),
    PhiSrcs(Box<OpPhiSrcs>),
    PhiDsts(Box<OpPhiDsts>),
    Copy(Box<OpCopy>),
    Pin(Box<OpPin>),
    Unpin(Box<OpUnpin>),
    Swap(Box<OpSwap>),
    ParCopy(Box<OpParCopy>),
    RegOut(Box<OpRegOut>),
    Out(Box<OpOut>),
    OutFinal(Box<OpOutFinal>),
    Annotate(Box<OpAnnotate>),
}
impl_display_for_op!(Op);

#[cfg(target_arch = "x86_64")]
const _: () = {
    debug_assert!(std::mem::size_of::<Op>() == 16);
};

impl Op {
    pub fn is_branch(&self) -> bool {
        matches!(
            self,
            Op::Bra(_) | Op::Sync(_) | Op::Brk(_) | Op::Cont(_) | Op::Exit(_)
        )
    }

    pub fn is_fp64(&self) -> bool {
        match self {
            Op::MuFu(op) => matches!(op.op, MuFuOp::Rcp64H | MuFuOp::Rsq64H),
            Op::DAdd(_)
            | Op::DFma(_)
            | Op::F64Exp2(_)
            | Op::F64Log2(_)
            | Op::F64Rcp(_)
            | Op::F64Sin(_)
            | Op::F64Cos(_)
            | Op::F64Sqrt(_)
            | Op::DMnMx(_)
            | Op::DMul(_)
            | Op::DSetP(_) => true,
            Op::F2F(op) => op.src_type.bits() == 64 || op.dst_type.bits() == 64,
            Op::F2I(op) => op.src_type.bits() == 64 || op.dst_type.bits() == 64,
            Op::I2F(op) => op.src_type.bits() == 64 || op.dst_type.bits() == 64,
            Op::FRnd(op) => op.src_type.bits() == 64 || op.dst_type.bits() == 64,
            _ => false,
        }
    }

    pub fn has_fixed_latency(&self, sm: u8) -> bool {
        match self {
            // Float ALU
            Op::F2FP(_)
            | Op::FAdd(_)
            | Op::FFma(_)
            | Op::FMnMx(_)
            | Op::FMul(_)
            | Op::FSet(_)
            | Op::FSetP(_)
            | Op::HAdd2(_)
            | Op::HFma2(_)
            | Op::HMul2(_)
            | Op::HSet2(_)
            | Op::HSetP2(_)
            | Op::HMnMx2(_)
            | Op::FSwz(_)
            | Op::FSwzAdd(_) => true,

            // Multi-function unit is variable latency
            Op::Rro(_) | Op::MuFu(_) => false,

            // Double-precision float ALU
            Op::DAdd(_) | Op::DFma(_) | Op::DMnMx(_) | Op::DMul(_) | Op::DSetP(_) => false,

            // Matrix Multiply Add
            Op::Imma(_) | Op::Hmma(_) | Op::Ldsm(_) | Op::Movm(_) => false,

            // Integer ALU
            Op::BRev(_) | Op::Flo(_) | Op::PopC(_) => false,
            Op::IMad(_) | Op::IMul(_) => sm >= 70,
            Op::BMsk(_)
            | Op::IAbs(_)
            | Op::IAdd2(_)
            | Op::IAdd2X(_)
            | Op::IAdd3(_)
            | Op::IAdd3X(_)
            | Op::IDp4(_)
            | Op::IMad64(_)
            | Op::IMnMx(_)
            | Op::ISetP(_)
            | Op::Lea(_)
            | Op::LeaX(_)
            | Op::Lop2(_)
            | Op::Lop3(_)
            | Op::SuClamp(_)
            | Op::SuBfm(_)
            | Op::SuEau(_)
            | Op::IMadSp(_)
            | Op::Shf(_)
            | Op::Shl(_)
            | Op::Shr(_)
            | Op::Bfe(_) => true,

            // Conversions are variable latency?!?
            Op::F2F(_) | Op::F2I(_) | Op::I2F(_) | Op::I2I(_) | Op::FRnd(_) => false,

            // Move ops
            Op::Mov(_) | Op::Prmt(_) | Op::Sel(_) | Op::Sgxt(_) => true,
            Op::Shfl(_) => false,

            // Predicate ops
            Op::PLop3(_) | Op::PSetP(_) => true,

            // Uniform ops
            Op::R2UR(_) | Op::Redux(_) => false,

            // Texture ops
            Op::Tex(_) | Op::Tld(_) | Op::Tld4(_) | Op::Tmml(_) | Op::Txd(_) | Op::Txq(_) => false,

            // Surface ops
            Op::SuLd(_) | Op::SuSt(_) | Op::SuAtom(_) | Op::SuLdGa(_) | Op::SuStGa(_) => false,

            // Memory ops
            Op::Ld(_)
            | Op::Ldc(_)
            | Op::LdSharedLock(_)
            | Op::St(_)
            | Op::StSCheckUnlock(_)
            | Op::Atom(_)
            | Op::AL2P(_)
            | Op::ALd(_)
            | Op::ASt(_)
            | Op::Ipa(_)
            | Op::CCtl(_)
            | Op::LdTram(_)
            | Op::MemBar(_) => false,

            // Control-flow ops
            Op::BClear(_)
            | Op::Break(_)
            | Op::BSSy(_)
            | Op::BSync(_)
            | Op::SSy(_)
            | Op::Sync(_)
            | Op::Brk(_)
            | Op::PBk(_)
            | Op::Cont(_)
            | Op::PCnt(_)
            | Op::Bra(_)
            | Op::Exit(_)
            | Op::WarpSync(_) => false,

            // The barrier half is HW scoreboarded by the GPR isn't.  When
            // moving from a GPR to a barrier, we still need a token for WaR
            // hazards.
            Op::BMov(_) => false,

            // Geometry ops
            Op::Out(_) | Op::OutFinal(_) => false,

            // Miscellaneous ops
            Op::Bar(_)
            | Op::TexDepBar(_)
            | Op::CS2R(_)
            | Op::Isberd(_)
            | Op::ViLd(_)
            | Op::Kill(_)
            | Op::PixLd(_)
            | Op::S2R(_)
            | Op::Match(_) => false,
            Op::Nop(_) | Op::Vote(_) => true,

            // f64 transcendental placeholders (lowered before legalize)
            Op::F64Exp2(_) | Op::F64Log2(_) | Op::F64Rcp(_) | Op::F64Sin(_) | Op::F64Cos(_) | Op::F64Sqrt(_) => false,

            // Virtual ops
            Op::Undef(_)
            | Op::SrcBar(_)
            | Op::PhiSrcs(_)
            | Op::PhiDsts(_)
            | Op::Copy(_)
            | Op::Pin(_)
            | Op::Unpin(_)
            | Op::Swap(_)
            | Op::ParCopy(_)
            | Op::RegOut(_)
            | Op::Annotate(_) => {
                panic!("Not a hardware opcode")
            }
        }
    }

    /// Some decoupled instructions don't need
    /// scoreboards, due to our usage.
    pub fn no_scoreboard(&self) -> bool {
        matches!(
            self,
            Op::BClear(_)
                | Op::Break(_)
                | Op::BSSy(_)
                | Op::BSync(_)
                | Op::SSy(_)
                | Op::Sync(_)
                | Op::Brk(_)
                | Op::PBk(_)
                | Op::Cont(_)
                | Op::PCnt(_)
                | Op::Bra(_)
                | Op::Exit(_)
        )
    }

    pub fn is_virtual(&self) -> bool {
        match self {
            // Float ALU
            Op::F2FP(_)
            | Op::FAdd(_)
            | Op::FFma(_)
            | Op::FMnMx(_)
            | Op::FMul(_)
            | Op::FSet(_)
            | Op::FSetP(_)
            | Op::HAdd2(_)
            | Op::HFma2(_)
            | Op::HMul2(_)
            | Op::HSet2(_)
            | Op::HSetP2(_)
            | Op::HMnMx2(_)
            | Op::FSwz(_)
            | Op::FSwzAdd(_) => false,

            // Multi-function unit
            Op::Rro(_) | Op::MuFu(_) => false,

            // Double-precision float ALU
            Op::DAdd(_) | Op::DFma(_) | Op::DMnMx(_) | Op::DMul(_) | Op::DSetP(_) => false,

            // Matrix Multiply Add
            Op::Imma(_) | Op::Hmma(_) | Op::Ldsm(_) | Op::Movm(_) => false,

            // Integer ALU
            Op::BRev(_)
            | Op::Flo(_)
            | Op::PopC(_)
            | Op::IMad(_)
            | Op::IMul(_)
            | Op::BMsk(_)
            | Op::IAbs(_)
            | Op::IAdd2(_)
            | Op::IAdd2X(_)
            | Op::IAdd3(_)
            | Op::IAdd3X(_)
            | Op::IDp4(_)
            | Op::IMad64(_)
            | Op::IMnMx(_)
            | Op::ISetP(_)
            | Op::Lea(_)
            | Op::LeaX(_)
            | Op::Lop2(_)
            | Op::Lop3(_)
            | Op::SuClamp(_)
            | Op::SuBfm(_)
            | Op::SuEau(_)
            | Op::IMadSp(_)
            | Op::Shf(_)
            | Op::Shl(_)
            | Op::Shr(_)
            | Op::Bfe(_) => false,

            // Conversions
            Op::F2F(_) | Op::F2I(_) | Op::I2F(_) | Op::I2I(_) | Op::FRnd(_) => false,

            // Move ops
            Op::Mov(_) | Op::Prmt(_) | Op::Sel(_) | Op::Sgxt(_) | Op::Shfl(_) => false,

            // Predicate ops
            Op::PLop3(_) | Op::PSetP(_) => false,

            // Uniform ops
            Op::R2UR(op) => op.src.is_uniform() || op.dst.file() == Some(RegFile::UPred),
            Op::Redux(_) => false,

            // Texture ops
            Op::Tex(_) | Op::Tld(_) | Op::Tld4(_) | Op::Tmml(_) | Op::Txd(_) | Op::Txq(_) => false,

            // Surface ops
            Op::SuLd(_) | Op::SuSt(_) | Op::SuAtom(_) | Op::SuLdGa(_) | Op::SuStGa(_) => false,

            // Memory ops
            Op::Ld(_)
            | Op::Ldc(_)
            | Op::LdSharedLock(_)
            | Op::St(_)
            | Op::StSCheckUnlock(_)
            | Op::Atom(_)
            | Op::AL2P(_)
            | Op::ALd(_)
            | Op::ASt(_)
            | Op::Ipa(_)
            | Op::CCtl(_)
            | Op::LdTram(_)
            | Op::MemBar(_) => false,

            // Control-flow ops
            Op::BClear(_)
            | Op::Break(_)
            | Op::BSSy(_)
            | Op::BSync(_)
            | Op::SSy(_)
            | Op::Sync(_)
            | Op::Brk(_)
            | Op::PBk(_)
            | Op::Cont(_)
            | Op::PCnt(_)
            | Op::Bra(_)
            | Op::Exit(_)
            | Op::WarpSync(_) => false,

            // Barrier
            Op::BMov(_) => false,

            // Geometry ops
            Op::Out(_) | Op::OutFinal(_) => false,

            // Miscellaneous ops
            Op::Bar(_)
            | Op::TexDepBar(_)
            | Op::CS2R(_)
            | Op::Isberd(_)
            | Op::ViLd(_)
            | Op::Kill(_)
            | Op::PixLd(_)
            | Op::S2R(_)
            | Op::Match(_)
            | Op::Nop(_)
            | Op::Vote(_) => false,

            // f64 transcendental placeholders (lowered before legalize)
            Op::F64Exp2(_) | Op::F64Log2(_) | Op::F64Rcp(_) | Op::F64Sin(_) | Op::F64Cos(_) | Op::F64Sqrt(_) => true,

            // Virtual ops
            Op::Undef(_)
            | Op::SrcBar(_)
            | Op::PhiSrcs(_)
            | Op::PhiDsts(_)
            | Op::Copy(_)
            | Op::Pin(_)
            | Op::Unpin(_)
            | Op::Swap(_)
            | Op::ParCopy(_)
            | Op::RegOut(_)
            | Op::Annotate(_) => true,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_op_nop_variant() {
        let op = Op::Nop(OpNop { label: None });
        assert!(!op.is_branch());
        assert!(!op.is_fp64());
        assert!(!op.is_virtual());
    }

    #[test]
    fn test_op_exit_variant() {
        let op = Op::Exit(OpExit {});
        assert!(op.is_branch());
        assert!(op.no_scoreboard());
    }

    #[test]
    fn test_op_ssy_variant() {
        let mut alloc = LabelAllocator::new();
        let label = alloc.alloc();
        let op = Op::SSy(OpSSy { target: label });
        assert!(op.no_scoreboard());
    }

    #[test]
    fn test_op_mov_is_not_branch() {
        let op = Op::Mov(Box::new(OpMov {
            dst: Dst::None,
            src: Src::ZERO,
            quad_lanes: 0xf,
        }));
        assert!(!op.is_branch());
    }
}
