// SPDX-License-Identifier: AGPL-3.0-only
// Copyright © 2025-2026 ecoPrimals
// Derived from Collabora, Ltd. (2022)
//! The Op enum and Op trait implementations.

use std::fmt;

use nak_ir_proc::*;

use super::op_conv::*;
use super::op_misc::*;
use super::*;

#[derive(DisplayOp, DstsAsSlice, SrcsAsSlice, FromVariants)]
pub enum Op {
    FAdd(Box<OpFAdd>),
    FFma(Box<OpFFma>),
    FMnMx(Box<OpFMnMx>),
    FMul(Box<OpFMul>),
    Rro(Box<OpRro>),
    Transcendental(Box<OpTranscendental>),
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
            Self::Bra(_) | Self::Sync(_) | Self::Brk(_) | Self::Cont(_) | Self::Exit(_)
        )
    }

    pub fn is_fp64(&self) -> bool {
        match self {
            Self::Transcendental(op) => {
                matches!(op.op, TranscendentalOp::Rcp64H | TranscendentalOp::Rsq64H)
            }
            Self::DAdd(_)
            | Self::DFma(_)
            | Self::F64Exp2(_)
            | Self::F64Log2(_)
            | Self::F64Rcp(_)
            | Self::F64Sin(_)
            | Self::F64Cos(_)
            | Self::F64Sqrt(_)
            | Self::DMnMx(_)
            | Self::DMul(_)
            | Self::DSetP(_) => true,
            Self::F2F(op) => op.src_type.bits() == 64 || op.dst_type.bits() == 64,
            Self::F2I(op) => op.src_type.bits() == 64 || op.dst_type.bits() == 64,
            Self::I2F(op) => op.src_type.bits() == 64 || op.dst_type.bits() == 64,
            Self::FRnd(op) => op.src_type.bits() == 64 || op.dst_type.bits() == 64,
            _ => false,
        }
    }

    pub fn has_fixed_latency(&self, sm: u8) -> bool {
        match self {
            // Float ALU
            Self::F2FP(_)
            | Self::FAdd(_)
            | Self::FFma(_)
            | Self::FMnMx(_)
            | Self::FMul(_)
            | Self::FSet(_)
            | Self::FSetP(_)
            | Self::HAdd2(_)
            | Self::HFma2(_)
            | Self::HMul2(_)
            | Self::HSet2(_)
            | Self::HSetP2(_)
            | Self::HMnMx2(_)
            | Self::FSwz(_)
            | Self::FSwzAdd(_) => true,

            // Multi-function unit is variable latency
            Self::Rro(_) | Self::Transcendental(_) => false,

            // Double-precision float ALU
            Self::DAdd(_) | Self::DFma(_) | Self::DMnMx(_) | Self::DMul(_) | Self::DSetP(_) => {
                false
            }

            // Matrix Multiply Add
            Self::Imma(_) | Self::Hmma(_) | Self::Ldsm(_) | Self::Movm(_) => false,

            // Integer ALU
            Self::BRev(_) | Self::Flo(_) | Self::PopC(_) => false,
            Self::IMad(_) | Self::IMul(_) => sm >= 70,
            Self::BMsk(_)
            | Self::IAbs(_)
            | Self::IAdd2(_)
            | Self::IAdd2X(_)
            | Self::IAdd3(_)
            | Self::IAdd3X(_)
            | Self::IDp4(_)
            | Self::IMad64(_)
            | Self::IMnMx(_)
            | Self::ISetP(_)
            | Self::Lea(_)
            | Self::LeaX(_)
            | Self::Lop2(_)
            | Self::Lop3(_)
            | Self::SuClamp(_)
            | Self::SuBfm(_)
            | Self::SuEau(_)
            | Self::IMadSp(_)
            | Self::Shf(_)
            | Self::Shl(_)
            | Self::Shr(_)
            | Self::Bfe(_) => true,

            // Conversions are variable latency?!?
            Self::F2F(_) | Self::F2I(_) | Self::I2F(_) | Self::I2I(_) | Self::FRnd(_) => false,

            // Move ops
            Self::Mov(_) | Self::Prmt(_) | Self::Sel(_) | Self::Sgxt(_) => true,
            Self::Shfl(_) => false,

            // Predicate ops
            Self::PLop3(_) | Self::PSetP(_) => true,

            // Uniform ops
            Self::R2UR(_) | Self::Redux(_) => false,

            // Texture ops
            Self::Tex(_)
            | Self::Tld(_)
            | Self::Tld4(_)
            | Self::Tmml(_)
            | Self::Txd(_)
            | Self::Txq(_) => false,

            // Surface ops
            Self::SuLd(_) | Self::SuSt(_) | Self::SuAtom(_) | Self::SuLdGa(_) | Self::SuStGa(_) => {
                false
            }

            // Memory ops
            Self::Ld(_)
            | Self::Ldc(_)
            | Self::LdSharedLock(_)
            | Self::St(_)
            | Self::StSCheckUnlock(_)
            | Self::Atom(_)
            | Self::AL2P(_)
            | Self::ALd(_)
            | Self::ASt(_)
            | Self::Ipa(_)
            | Self::CCtl(_)
            | Self::LdTram(_)
            | Self::MemBar(_) => false,

            // Control-flow ops
            Self::BClear(_)
            | Self::Break(_)
            | Self::BSSy(_)
            | Self::BSync(_)
            | Self::SSy(_)
            | Self::Sync(_)
            | Self::Brk(_)
            | Self::PBk(_)
            | Self::Cont(_)
            | Self::PCnt(_)
            | Self::Bra(_)
            | Self::Exit(_)
            | Self::WarpSync(_) => false,

            // The barrier half is HW scoreboarded by the GPR isn't.  When
            // moving from a GPR to a barrier, we still need a token for WaR
            // hazards.
            Self::BMov(_) => false,

            // Geometry ops
            Self::Out(_) | Self::OutFinal(_) => false,

            // Miscellaneous ops
            Self::Bar(_)
            | Self::TexDepBar(_)
            | Self::CS2R(_)
            | Self::Isberd(_)
            | Self::ViLd(_)
            | Self::Kill(_)
            | Self::PixLd(_)
            | Self::S2R(_)
            | Self::Match(_) => false,
            Self::Nop(_) | Self::Vote(_) => true,

            // f64 transcendental placeholders (lowered before legalize)
            Self::F64Exp2(_)
            | Self::F64Log2(_)
            | Self::F64Rcp(_)
            | Self::F64Sin(_)
            | Self::F64Cos(_)
            | Self::F64Sqrt(_) => false,

            // Virtual ops
            Self::Undef(_)
            | Self::SrcBar(_)
            | Self::PhiSrcs(_)
            | Self::PhiDsts(_)
            | Self::Copy(_)
            | Self::Pin(_)
            | Self::Unpin(_)
            | Self::Swap(_)
            | Self::ParCopy(_)
            | Self::RegOut(_)
            | Self::Annotate(_) => {
                panic!("Not a hardware opcode")
            }
        }
    }

    /// Some decoupled instructions don't need
    /// scoreboards, due to our usage.
    pub fn no_scoreboard(&self) -> bool {
        matches!(
            self,
            Self::BClear(_)
                | Self::Break(_)
                | Self::BSSy(_)
                | Self::BSync(_)
                | Self::SSy(_)
                | Self::Sync(_)
                | Self::Brk(_)
                | Self::PBk(_)
                | Self::Cont(_)
                | Self::PCnt(_)
                | Self::Bra(_)
                | Self::Exit(_)
        )
    }

    pub fn is_virtual(&self) -> bool {
        match self {
            // Float ALU
            Self::F2FP(_)
            | Self::FAdd(_)
            | Self::FFma(_)
            | Self::FMnMx(_)
            | Self::FMul(_)
            | Self::FSet(_)
            | Self::FSetP(_)
            | Self::HAdd2(_)
            | Self::HFma2(_)
            | Self::HMul2(_)
            | Self::HSet2(_)
            | Self::HSetP2(_)
            | Self::HMnMx2(_)
            | Self::FSwz(_)
            | Self::FSwzAdd(_) => false,

            // Multi-function unit
            Self::Rro(_) | Self::Transcendental(_) => false,

            // Double-precision float ALU
            Self::DAdd(_) | Self::DFma(_) | Self::DMnMx(_) | Self::DMul(_) | Self::DSetP(_) => {
                false
            }

            // Matrix Multiply Add
            Self::Imma(_) | Self::Hmma(_) | Self::Ldsm(_) | Self::Movm(_) => false,

            // Integer ALU
            Self::BRev(_)
            | Self::Flo(_)
            | Self::PopC(_)
            | Self::IMad(_)
            | Self::IMul(_)
            | Self::BMsk(_)
            | Self::IAbs(_)
            | Self::IAdd2(_)
            | Self::IAdd2X(_)
            | Self::IAdd3(_)
            | Self::IAdd3X(_)
            | Self::IDp4(_)
            | Self::IMad64(_)
            | Self::IMnMx(_)
            | Self::ISetP(_)
            | Self::Lea(_)
            | Self::LeaX(_)
            | Self::Lop2(_)
            | Self::Lop3(_)
            | Self::SuClamp(_)
            | Self::SuBfm(_)
            | Self::SuEau(_)
            | Self::IMadSp(_)
            | Self::Shf(_)
            | Self::Shl(_)
            | Self::Shr(_)
            | Self::Bfe(_) => false,

            // Conversions
            Self::F2F(_) | Self::F2I(_) | Self::I2F(_) | Self::I2I(_) | Self::FRnd(_) => false,

            // Move ops
            Self::Mov(_) | Self::Prmt(_) | Self::Sel(_) | Self::Sgxt(_) | Self::Shfl(_) => false,

            // Predicate ops
            Self::PLop3(_) | Self::PSetP(_) => false,

            // Uniform ops
            Self::R2UR(op) => op.src.is_uniform() || op.dst.file() == Some(RegFile::UPred),
            Self::Redux(_) => false,

            // Texture ops
            Self::Tex(_)
            | Self::Tld(_)
            | Self::Tld4(_)
            | Self::Tmml(_)
            | Self::Txd(_)
            | Self::Txq(_) => false,

            // Surface ops
            Self::SuLd(_) | Self::SuSt(_) | Self::SuAtom(_) | Self::SuLdGa(_) | Self::SuStGa(_) => {
                false
            }

            // Memory ops
            Self::Ld(_)
            | Self::Ldc(_)
            | Self::LdSharedLock(_)
            | Self::St(_)
            | Self::StSCheckUnlock(_)
            | Self::Atom(_)
            | Self::AL2P(_)
            | Self::ALd(_)
            | Self::ASt(_)
            | Self::Ipa(_)
            | Self::CCtl(_)
            | Self::LdTram(_)
            | Self::MemBar(_) => false,

            // Control-flow ops
            Self::BClear(_)
            | Self::Break(_)
            | Self::BSSy(_)
            | Self::BSync(_)
            | Self::SSy(_)
            | Self::Sync(_)
            | Self::Brk(_)
            | Self::PBk(_)
            | Self::Cont(_)
            | Self::PCnt(_)
            | Self::Bra(_)
            | Self::Exit(_)
            | Self::WarpSync(_) => false,

            // Barrier
            Self::BMov(_) => false,

            // Geometry ops
            Self::Out(_) | Self::OutFinal(_) => false,

            // Miscellaneous ops
            Self::Bar(_)
            | Self::TexDepBar(_)
            | Self::CS2R(_)
            | Self::Isberd(_)
            | Self::ViLd(_)
            | Self::Kill(_)
            | Self::PixLd(_)
            | Self::S2R(_)
            | Self::Match(_)
            | Self::Nop(_)
            | Self::Vote(_) => false,

            // f64 transcendental placeholders (lowered before legalize)
            Self::F64Exp2(_)
            | Self::F64Log2(_)
            | Self::F64Rcp(_)
            | Self::F64Sin(_)
            | Self::F64Cos(_)
            | Self::F64Sqrt(_) => true,

            // Virtual ops
            Self::Undef(_)
            | Self::SrcBar(_)
            | Self::PhiSrcs(_)
            | Self::PhiDsts(_)
            | Self::Copy(_)
            | Self::Pin(_)
            | Self::Unpin(_)
            | Self::Swap(_)
            | Self::ParCopy(_)
            | Self::RegOut(_)
            | Self::Annotate(_) => true,
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
