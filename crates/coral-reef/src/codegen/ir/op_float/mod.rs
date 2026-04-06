// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright © 2025-2026 ecoPrimals
// Derived from Collabora, Ltd. (2022)
//! Float, double, and half-precision ALU instruction op structs.

mod f16_ops;
mod f32_ops;
mod f64_ops;

pub use f16_ops::*;
pub use f32_ops::*;
pub use f64_ops::*;

#[cfg(test)]
mod tests {
    use super::super::*;
    use super::*;

    fn zero_src() -> Src {
        Src::ZERO
    }

    fn imm_src(u: u32) -> Src {
        Src::new_imm_u32(u)
    }

    #[test]
    fn test_op_fadd_display() {
        let op = OpFAdd {
            dst: Dst::None,
            srcs: [zero_src(), imm_src(0x42)],
            saturate: false,
            rnd_mode: FRndMode::NearestEven,
            ftz: false,
        };
        assert!(format!("{op}").contains("fadd"));
        assert!(format!("{op}").contains("rZ"));
        assert!(format!("{op}").contains("0x42"));
    }

    #[test]
    fn test_op_fadd_saturate_ftz() {
        let op = OpFAdd {
            dst: Dst::None,
            srcs: [zero_src(), zero_src()],
            saturate: true,
            rnd_mode: FRndMode::NearestEven,
            ftz: true,
        };
        let s = format!("{op}");
        assert!(s.contains(".sat"));
        assert!(s.contains(".ftz"));
    }

    #[test]
    fn test_op_ffma_display() {
        let op = OpFFma {
            dst: Dst::None,
            srcs: [zero_src(), zero_src(), imm_src(1)],
            saturate: false,
            rnd_mode: FRndMode::NegInf,
            ftz: false,
            dnz: false,
        };
        let s = format!("{op}");
        assert!(s.contains("ffma"));
        assert!(s.contains(".rm"));
    }

    #[test]
    fn test_op_fmnmx_display() {
        let op = OpFMnMx {
            dst: Dst::None,
            srcs: [zero_src(), imm_src(2), Src::new_imm_bool(true)],
            ftz: true,
        };
        let s = format!("{op}");
        assert!(s.contains("fmnmx"));
        assert!(s.contains(".ftz"));
    }

    #[test]
    fn test_op_fmul_display() {
        let op = OpFMul {
            dst: Dst::None,
            srcs: [zero_src(), zero_src()],
            saturate: false,
            rnd_mode: FRndMode::NearestEven,
            ftz: false,
            dnz: true,
        };
        let s = format!("{op}");
        assert!(s.contains("fmul"));
        assert!(s.contains(".dnz"));
    }

    #[test]
    fn test_op_fset_display() {
        let op = OpFSet {
            dst: Dst::None,
            cmp_op: FloatCmpOp::OrdEq,
            srcs: [zero_src(), zero_src()],
            ftz: false,
        };
        let s = format!("{op}");
        assert!(s.contains("fset"));
        assert!(s.contains(".eq"));
    }

    #[test]
    fn test_op_fsetp_display() {
        let op = OpFSetP {
            dst: Dst::None,
            set_op: PredSetOp::And,
            cmp_op: FloatCmpOp::OrdLt,
            srcs: [zero_src(), imm_src(1), Src::new_imm_bool(true)],
            ftz: false,
        };
        let s = format!("{op}");
        assert!(s.contains("fsetp"));
        assert!(s.contains(".lt"));
    }

    #[test]
    fn test_fswz_add_op_display() {
        assert_eq!(format!("{}", FSwzAddOp::Add), "add");
        assert_eq!(format!("{}", FSwzAddOp::SubRight), "subr");
        assert_eq!(format!("{}", FSwzAddOp::SubLeft), "sub");
        assert_eq!(format!("{}", FSwzAddOp::MoveLeft), "mov2");
    }

    #[test]
    fn test_op_fswzadd_display() {
        let op = OpFSwzAdd {
            dst: Dst::None,
            srcs: [zero_src(), zero_src()],
            rnd_mode: FRndMode::NearestEven,
            ftz: false,
            deriv_mode: TexDerivMode::Auto,
            ops: [
                FSwzAddOp::Add,
                FSwzAddOp::SubRight,
                FSwzAddOp::SubLeft,
                FSwzAddOp::MoveLeft,
            ],
        };
        let s = format!("{op}");
        assert!(s.contains("fswzadd"));
        assert!(s.contains("add"));
        assert!(s.contains("subr"));
    }

    #[test]
    fn test_fswz_shuffle_display() {
        assert_eq!(format!("{}", FSwzShuffle::Quad0), ".0000");
        assert_eq!(format!("{}", FSwzShuffle::SwapHorizontal), ".1032");
        assert_eq!(format!("{}", FSwzShuffle::SwapVertical), ".2301");
    }

    #[test]
    fn test_op_fswz_display() {
        let op = OpFSwz {
            dst: Dst::None,
            srcs: [zero_src(), zero_src()],
            rnd_mode: FRndMode::NearestEven,
            ftz: false,
            deriv_mode: TexDerivMode::NonDivergent,
            shuffle: FSwzShuffle::Quad1,
            ops: [FSwzAddOp::Add; 4],
        };
        let s = format!("{op}");
        assert!(s.contains("fswz"));
        assert!(s.contains(".1111"));
    }

    #[test]
    fn test_rro_op_display() {
        assert_eq!(format!("{}", RroOp::SinCos), ".sincos");
        assert_eq!(format!("{}", RroOp::Exp2), ".exp2");
    }

    #[test]
    fn test_op_rro_display() {
        let op = OpRro {
            dst: Dst::None,
            op: RroOp::SinCos,
            src: zero_src(),
        };
        let s = format!("{op}");
        assert!(s.contains("rro"));
        assert!(s.contains(".sincos"));
    }

    #[test]
    fn test_mufu_op_display() {
        assert_eq!(format!("{}", TranscendentalOp::Cos), "cos");
        assert_eq!(format!("{}", TranscendentalOp::Sin), "sin");
        assert_eq!(format!("{}", TranscendentalOp::Sqrt), "sqrt");
        assert_eq!(format!("{}", TranscendentalOp::Rcp), "rcp");
    }

    #[test]
    fn test_op_mufu_display() {
        let op = OpTranscendental {
            dst: Dst::None,
            op: TranscendentalOp::Sqrt,
            src: zero_src(),
        };
        let s = format!("{op}");
        assert!(s.contains("transcendental"));
        assert!(s.contains("sqrt"));
    }

    #[test]
    fn test_op_dadd_display() {
        let op = OpDAdd {
            dst: Dst::None,
            srcs: [zero_src(), zero_src()],
            rnd_mode: FRndMode::Zero,
        };
        let s = format!("{op}");
        assert!(s.contains("dadd"));
        assert!(s.contains(".rz"));
    }

    #[test]
    fn test_op_dmul_display() {
        let op = OpDMul {
            dst: Dst::None,
            srcs: [zero_src(), imm_src(0xdead)],
            rnd_mode: FRndMode::NearestEven,
        };
        let s = format!("{op}");
        assert!(s.contains("dmul"));
    }

    #[test]
    fn test_op_dfma_display() {
        let op = OpDFma {
            dst: Dst::None,
            srcs: [zero_src(), zero_src(), zero_src()],
            rnd_mode: FRndMode::PosInf,
        };
        let s = format!("{op}");
        assert!(s.contains("dfma"));
        assert!(s.contains(".rp"));
    }

    #[test]
    fn test_op_f64_sqrt_rcp_exp2_log2_sin_cos_display() {
        let sqrt = OpF64Sqrt {
            dst: Dst::None,
            src: zero_src(),
        };
        assert!(format!("{sqrt}").contains("f64sqrt"));

        let rcp = OpF64Rcp {
            dst: Dst::None,
            src: zero_src(),
        };
        assert!(format!("{rcp}").contains("f64rcp"));

        let exp2 = OpF64Exp2 {
            dst: Dst::None,
            src: zero_src(),
        };
        assert!(format!("{exp2}").contains("f64exp2"));

        let log2 = OpF64Log2 {
            dst: Dst::None,
            src: zero_src(),
        };
        assert!(format!("{log2}").contains("f64log2"));

        let sin = OpF64Sin {
            dst: Dst::None,
            src: zero_src(),
        };
        assert!(format!("{sin}").contains("f64sin"));

        let cos = OpF64Cos {
            dst: Dst::None,
            src: zero_src(),
        };
        assert!(format!("{cos}").contains("f64cos"));
    }

    #[test]
    fn test_op_dmnmx_display() {
        let op = OpDMnMx {
            dst: Dst::None,
            srcs: [zero_src(), zero_src(), Src::new_imm_bool(false)],
        };
        let s = format!("{op}");
        assert!(s.contains("dmnmx"));
    }

    #[test]
    fn test_op_hadd2_display() {
        let op = OpHAdd2 {
            dst: Dst::None,
            srcs: [zero_src(), zero_src()],
            saturate: true,
            ftz: false,
            f32: true,
        };
        let s = format!("{op}");
        assert!(s.contains("hadd2"));
        assert!(s.contains(".sat"));
        assert!(s.contains(".f32"));
    }

    #[test]
    fn test_op_hmul2_display() {
        let op = OpHMul2 {
            dst: Dst::None,
            srcs: [zero_src(), zero_src()],
            saturate: false,
            ftz: true,
            dnz: false,
        };
        let s = format!("{op}");
        assert!(s.contains("hmul2"));
        assert!(s.contains(".ftz"));
    }

    #[test]
    fn test_imma_size_display() {
        assert_eq!(format!("{}", ImmaSize::M8N8K16), ".m8n8k16");
        assert_eq!(format!("{}", ImmaSize::M16N8K64), ".m16n8k64");
    }

    #[test]
    fn test_op_imma_display() {
        let op = OpImma {
            dst: Dst::None,
            mat_size: ImmaSize::M8N8K32,
            src_types: [IntType::U8, IntType::I8],
            saturate: false,
            srcs: [zero_src(), zero_src(), zero_src()],
        };
        let s = format!("{op}");
        assert!(s.contains("imma"));
        assert!(s.contains(".m8n8k32"));
    }

    #[test]
    fn test_hmma_size_display() {
        assert_eq!(format!("{}", HmmaSize::M16N8K16), ".m16n8k16");
        assert_eq!(format!("{}", HmmaSize::M16N8K4), ".m16n8k4");
    }

    #[test]
    fn test_op_hmma_display() {
        let op = OpHmma {
            dst: Dst::None,
            mat_size: HmmaSize::M16N8K8,
            src_type: FloatType::F16,
            dst_type: FloatType::F32,
            srcs: [zero_src(), zero_src(), zero_src()],
        };
        let s = format!("{op}");
        assert!(s.contains("hmma"));
        assert!(s.contains(".m16n8k8"));
    }

    #[test]
    fn test_op_hfma2_display() {
        let op = OpHFma2 {
            dst: Dst::None,
            srcs: [zero_src(), zero_src(), zero_src()],
            saturate: false,
            ftz: false,
            dnz: true,
            f32: false,
        };
        let s = format!("{op}");
        assert!(s.contains("hfma2"));
        assert!(s.contains(".dnz"));
    }

    #[test]
    fn test_op_hmnmx2_display() {
        let op = OpHMnMx2 {
            dst: Dst::None,
            srcs: [zero_src(), zero_src(), Src::new_imm_bool(true)],
            ftz: false,
        };
        let s = format!("{op}");
        assert!(s.contains("hmnmx2"));
    }

    #[test]
    fn test_op_hset2_display() {
        let op = OpHSet2 {
            dst: Dst::None,
            set_op: PredSetOp::And,
            cmp_op: FloatCmpOp::OrdLt,
            srcs: [zero_src(), zero_src(), Src::new_imm_bool(true)],
            ftz: true,
        };
        let s = format!("{op}");
        assert!(s.contains("hset2"));
        assert!(s.contains(".ftz"));
    }

    #[test]
    fn test_op_hset2_no_ftz() {
        let op = OpHSet2 {
            dst: Dst::None,
            set_op: PredSetOp::And,
            cmp_op: FloatCmpOp::OrdEq,
            srcs: [zero_src(), imm_src(1), Src::new_imm_bool(true)],
            ftz: false,
        };
        let s = format!("{op}");
        assert!(s.contains("hset2"));
        assert!(!s.contains(".ftz"));
    }

    #[test]
    fn test_op_hsetp2_display() {
        let op = OpHSetP2 {
            dsts: [Dst::None, Dst::None],
            set_op: PredSetOp::And,
            cmp_op: FloatCmpOp::OrdLt,
            srcs: [zero_src(), zero_src(), Src::new_imm_bool(true)],
            ftz: true,
            horizontal: false,
        };
        let s = format!("{op}");
        assert!(s.contains("hsetp2"));
        assert!(s.contains(".ftz"));
    }

    #[test]
    fn test_op_hsetp2_no_ftz() {
        let op = OpHSetP2 {
            dsts: [Dst::None, Dst::None],
            set_op: PredSetOp::And,
            cmp_op: FloatCmpOp::OrdGe,
            srcs: [zero_src(), zero_src(), Src::new_imm_bool(true)],
            ftz: false,
            horizontal: true,
        };
        let s = format!("{op}");
        assert!(s.contains("hsetp2"));
        assert!(!s.contains(".ftz"));
    }

    #[test]
    fn test_op_hmul2_dnz() {
        let op = OpHMul2 {
            dst: Dst::None,
            srcs: [zero_src(), zero_src()],
            saturate: true,
            ftz: false,
            dnz: true,
        };
        let s = format!("{op}");
        assert!(s.contains("hmul2"));
        assert!(s.contains(".sat"));
        assert!(s.contains(".dnz"));
        assert!(!s.contains(".ftz"));
    }

    #[test]
    fn test_op_hadd2_ftz_only() {
        let op = OpHAdd2 {
            dst: Dst::None,
            srcs: [zero_src(), zero_src()],
            saturate: false,
            ftz: true,
            f32: false,
        };
        let s = format!("{op}");
        assert!(s.contains("hadd2"));
        assert!(s.contains(".ftz"));
        assert!(!s.contains(".sat"));
        assert!(!s.contains(".f32"));
    }

    #[test]
    fn test_op_hfma2_ftz_and_sat() {
        let op = OpHFma2 {
            dst: Dst::None,
            srcs: [zero_src(), zero_src(), zero_src()],
            saturate: true,
            ftz: true,
            dnz: false,
            f32: true,
        };
        let s = format!("{op}");
        assert!(s.contains("hfma2"));
        assert!(s.contains(".sat"));
        assert!(s.contains(".f32"));
        assert!(s.contains(".ftz"));
        assert!(!s.contains(".dnz"));
    }

    #[test]
    fn test_op_hmnmx2_ftz() {
        let op = OpHMnMx2 {
            dst: Dst::None,
            srcs: [zero_src(), zero_src(), Src::new_imm_bool(false)],
            ftz: true,
        };
        let s = format!("{op}");
        assert!(s.contains("hmnmx2"));
        assert!(s.contains(".ftz"));
    }

    #[test]
    fn test_imma_size_display_all() {
        assert_eq!(format!("{}", ImmaSize::M8N8K32), ".m8n8k32");
        assert_eq!(format!("{}", ImmaSize::M16N8K16), ".m16n8k16");
        assert_eq!(format!("{}", ImmaSize::M16N8K32), ".m16n8k32");
    }

    #[test]
    fn test_hmma_size_display_all() {
        assert_eq!(format!("{}", HmmaSize::M16N8K8), ".m16n8k8");
    }

    #[test]
    fn test_op_imma_saturate() {
        let op = OpImma {
            dst: Dst::None,
            mat_size: ImmaSize::M16N8K16,
            src_types: [IntType::I8, IntType::I8],
            saturate: true,
            srcs: [zero_src(), zero_src(), zero_src()],
        };
        let s = format!("{op}");
        assert!(s.contains("imma"));
        assert!(s.contains(".sat"));
    }
}
