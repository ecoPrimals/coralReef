// SPDX-License-Identifier: AGPL-3.0-only
//! Instruction latency data for NVIDIA GPU instruction scheduling.
//!
//! SM100 (Blackwell B100) latency categories. Used by `sm120_instr_latencies`
//! for Blackwell consumer chips (RTX 50-series) which add padding on top.

/// SM100 instruction latency categories.
pub mod sm100 {
    /// Register (GPR) latency categories for SM100.
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
    #[allow(non_camel_case_types)]
    pub enum RegLatencySM100 {
        /// Simple ALU (coupled, fixed latency).
        Alu,
        /// Dual-issue ALU.
        Dualalu,
        /// Fused multiply-add and simple ALU.
        Fma,
        /// FP16 operations.
        Fp16,
        /// FP16 with ALU co-issue.
        Fp16Alu,
        /// FP16 with FP32 promotion.
        Fp16F32,
        /// 64-bit dispatch.
        Disp64,
        /// Integer multiply-add wide (read operands A/B).
        ImadWideReadAb,
        /// Integer multiply-add wide (read operand C, lower).
        ImadWideReadCl,
        /// Integer multiply-add wide (write destination, high).
        ImadWideWriteDh,
        /// Integer matrix multiply-accumulate.
        Imma,
        /// Double-precision matrix multiply-accumulate.
        Dmma,
        /// Half-precision matrix multiply-accumulate.
        Hmma,
        /// Decoupled (variable latency, scoreboarded).
        Decoupled,
        /// Decoupled with AGU (address generation unit).
        DecoupledAgu,
        /// Redirected FP64 operations.
        RedirectedFp64,
        /// Branch operations.
        Branch,
    }

    impl RegLatencySM100 {
        /// Raw register-to-register latency in cycles.
        #[must_use]
        pub fn raw(write: Self, read: Self, _coupled: bool) -> u32 {
            use RegLatencySM100::{
                Alu, Disp64, Dmma, Dualalu, Fma, Fp16, Fp16Alu, Fp16F32, Hmma, Imma, RedirectedFp64,
            };
            match (write, read) {
                (Hmma, _) | (_, Hmma) => 16,
                (Imma, _) | (_, Imma) => 12,
                (Dmma | Disp64 | RedirectedFp64, _) | (_, Dmma | Disp64 | RedirectedFp64) => 10,
                (Fma, Fma)
                | (Alu, Alu)
                | (Dualalu, Dualalu)
                | (Fp16 | Fp16Alu | Fp16F32, _)
                | (_, Fp16 | Fp16Alu | Fp16F32) => 5,
                _ => 6,
            }
        }

        /// Write-after-read latency in cycles.
        #[must_use]
        pub fn war(read: Self, write: Self, _coupled: bool) -> u32 {
            use RegLatencySM100::{Hmma, Imma};
            match (read, write) {
                (Hmma | Imma, _) | (_, Hmma | Imma) => 2,
                _ => 1,
            }
        }

        /// Write-after-write latency in cycles.
        #[must_use]
        pub fn waw(write1: Self, write2: Self, _has_pred: bool) -> u32 {
            use RegLatencySM100::Hmma;
            match (write1, write2) {
                (Hmma, _) | (_, Hmma) => 2,
                _ => 1,
            }
        }
    }

    /// Predicate register latency categories for SM100.
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
    #[allow(non_camel_case_types)]
    pub enum PredLatencySM100 {
        /// Fused multiply-add predicate output.
        Fma,
        /// Coupled predicate.
        Coupled,
        /// Dual-issue ALU predicate.
        Dualalu,
        /// FP16 predicate.
        Fp16,
        /// Register-to-uniform transfer predicate.
        R2Ur,
        /// Dispatch dual ALU.
        DispDualAlu,
        /// Decoupled (variable latency).
        Decoupled,
        /// Redirected FP64 operations.
        RedirectedFp64,
    }

    impl PredLatencySM100 {
        /// Raw predicate latency in cycles.
        #[must_use]
        pub fn raw(write: Self, read: Self, _coupled: bool) -> u32 {
            use PredLatencySM100::{Fma, RedirectedFp64};
            match (write, read) {
                (Fma, Fma) => 5,
                (RedirectedFp64, _) | (_, RedirectedFp64) => 10,
                _ => 6,
            }
        }

        /// Write-after-read predicate latency.
        #[must_use]
        pub fn war(read: Self, write: Self, _coupled: bool) -> u32 {
            let _ = (read, write);
            1
        }

        /// Write-after-write predicate latency.
        #[must_use]
        pub fn waw(write1: Self, write2: Self, _has_pred: bool) -> u32 {
            let _ = (write1, write2);
            1
        }
    }

    /// Uniform register (UGPR) latency categories for SM100.
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
    #[allow(non_camel_case_types)]
    pub enum UregLatencySM100 {
        /// Fused multiply-add (uniform path).
        Fma,
        /// Coupled uniform operation.
        Coupled,
        /// Coupled bindless uniform operation.
        CoupledBindless,
        /// Decoupled (variable latency).
        Decoupled,
        /// Decoupled bindless.
        DecoupledBindless,
        /// Uniform load constant.
        Uldc,
        /// Transfer to uniform register.
        ToUr,
        /// Texture operation (uniform path).
        Tex,
        /// Uniform data path.
        Udp,
        /// Uniform move.
        Umov,
        /// Register-to-uniform transfer.
        R2Ur,
        /// Uniform vote.
        Voteu,
    }

    impl UregLatencySM100 {
        /// Raw uniform register latency in cycles.
        #[must_use]
        pub fn raw(write: Self, read: Self, _coupled: bool) -> u32 {
            use UregLatencySM100::{Fma, Tex, Uldc};
            match (write, read) {
                (Fma, Fma) => 5,
                (Uldc | Tex, _) | (_, Uldc | Tex) => 8,
                _ => 6,
            }
        }

        /// Write-after-read uniform register latency.
        #[must_use]
        pub fn war(read: Self, write: Self, _coupled: bool) -> u32 {
            let _ = (read, write);
            1
        }

        /// Write-after-write uniform register latency.
        #[must_use]
        pub fn waw(write1: Self, write2: Self, _has_pred: bool) -> u32 {
            let _ = (write1, write2);
            1
        }
    }

    /// Uniform predicate register latency categories for SM100.
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
    #[allow(non_camel_case_types)]
    pub enum UpredLatencySM100 {
        /// Fused multiply-add (uniform predicate).
        Fma,
        /// Coupled uniform predicate.
        Coupled,
        /// Decoupled (variable latency).
        Decoupled,
        /// Uniform guard predicate.
        UGuard,
        /// Uniform data path predicate.
        Udp,
        /// Branch/jump predicate.
        BraJmp,
        /// Uniform load constant / MMA predicate.
        UldcMma,
        /// Uniform vote predicate.
        Voteu,
    }

    impl UpredLatencySM100 {
        /// Raw uniform predicate latency in cycles.
        #[must_use]
        pub fn raw(write: Self, read: Self, _coupled: bool) -> u32 {
            use UpredLatencySM100::Fma;
            match (write, read) {
                (Fma, Fma) => 5,
                _ => 6,
            }
        }

        /// Write-after-read uniform predicate latency.
        #[must_use]
        pub fn war(read: Self, write: Self, _coupled: bool) -> u32 {
            let _ = (read, write);
            1
        }

        /// Write-after-write uniform predicate latency.
        #[must_use]
        pub fn waw(write1: Self, write2: Self, _has_pred: bool) -> u32 {
            let _ = (write1, write2);
            1
        }
    }
}

#[cfg(test)]
mod tests {
    use super::sm100::{
        PredLatencySM100, RegLatencySM100, UregLatencySM100, UpredLatencySM100,
    };

    #[test]
    fn reg_latency_raw_hmma() {
        use super::sm100::RegLatencySM100;
        assert_eq!(RegLatencySM100::raw(RegLatencySM100::Hmma, RegLatencySM100::Alu, false), 16);
        assert_eq!(RegLatencySM100::raw(RegLatencySM100::Alu, RegLatencySM100::Hmma, false), 16);
    }

    #[test]
    fn reg_latency_raw_imma() {
        use super::sm100::RegLatencySM100;
        assert_eq!(RegLatencySM100::raw(RegLatencySM100::Imma, RegLatencySM100::Fma, false), 12);
        assert_eq!(RegLatencySM100::raw(RegLatencySM100::Alu, RegLatencySM100::Imma, false), 12);
    }

    #[test]
    fn reg_latency_raw_dmma_disp64_redirected_fp64() {
        use super::sm100::RegLatencySM100;
        assert_eq!(RegLatencySM100::raw(RegLatencySM100::Dmma, RegLatencySM100::Alu, false), 10);
        assert_eq!(RegLatencySM100::raw(RegLatencySM100::Disp64, RegLatencySM100::Fma, false), 10);
        assert_eq!(RegLatencySM100::raw(RegLatencySM100::RedirectedFp64, RegLatencySM100::Alu, false), 10);
    }

    #[test]
    fn reg_latency_raw_fma_fma_and_alu_alu() {
        use super::sm100::RegLatencySM100;
        assert_eq!(RegLatencySM100::raw(RegLatencySM100::Fma, RegLatencySM100::Fma, false), 5);
        assert_eq!(RegLatencySM100::raw(RegLatencySM100::Alu, RegLatencySM100::Alu, false), 5);
        assert_eq!(RegLatencySM100::raw(RegLatencySM100::Dualalu, RegLatencySM100::Dualalu, false), 5);
    }

    #[test]
    fn reg_latency_raw_fp16_variants() {
        use super::sm100::RegLatencySM100;
        assert_eq!(RegLatencySM100::raw(RegLatencySM100::Fp16, RegLatencySM100::Alu, false), 5);
        assert_eq!(RegLatencySM100::raw(RegLatencySM100::Fp16Alu, RegLatencySM100::Fma, false), 5);
        assert_eq!(RegLatencySM100::raw(RegLatencySM100::Fp16F32, RegLatencySM100::Alu, false), 5);
    }

    #[test]
    fn reg_latency_raw_default() {
        use super::sm100::RegLatencySM100;
        assert_eq!(RegLatencySM100::raw(RegLatencySM100::Decoupled, RegLatencySM100::Alu, false), 6);
        assert_eq!(RegLatencySM100::raw(RegLatencySM100::Branch, RegLatencySM100::Fma, false), 6);
    }

    #[test]
    fn reg_latency_war_hmma_imma() {
        use super::sm100::RegLatencySM100;
        assert_eq!(RegLatencySM100::war(RegLatencySM100::Hmma, RegLatencySM100::Alu, false), 2);
        assert_eq!(RegLatencySM100::war(RegLatencySM100::Imma, RegLatencySM100::Fma, false), 2);
        assert_eq!(RegLatencySM100::war(RegLatencySM100::Alu, RegLatencySM100::Hmma, false), 2);
    }

    #[test]
    fn reg_latency_war_default() {
        use super::sm100::RegLatencySM100;
        assert_eq!(RegLatencySM100::war(RegLatencySM100::Alu, RegLatencySM100::Fma, false), 1);
    }

    #[test]
    fn reg_latency_waw_hmma() {
        use super::sm100::RegLatencySM100;
        assert_eq!(RegLatencySM100::waw(RegLatencySM100::Hmma, RegLatencySM100::Alu, false), 2);
        assert_eq!(RegLatencySM100::waw(RegLatencySM100::Alu, RegLatencySM100::Hmma, false), 2);
    }

    #[test]
    fn reg_latency_waw_default() {
        use super::sm100::RegLatencySM100;
        assert_eq!(RegLatencySM100::waw(RegLatencySM100::Alu, RegLatencySM100::Fma, false), 1);
    }

    #[test]
    fn pred_latency_raw_fma_fma() {
        assert_eq!(PredLatencySM100::raw(PredLatencySM100::Fma, PredLatencySM100::Fma, false), 5);
    }

    #[test]
    fn pred_latency_raw_redirected_fp64() {
        assert_eq!(PredLatencySM100::raw(PredLatencySM100::RedirectedFp64, PredLatencySM100::Coupled, false), 10);
        assert_eq!(PredLatencySM100::raw(PredLatencySM100::Coupled, PredLatencySM100::RedirectedFp64, false), 10);
    }

    #[test]
    fn pred_latency_raw_default() {
        assert_eq!(PredLatencySM100::raw(PredLatencySM100::Coupled, PredLatencySM100::Dualalu, false), 6);
    }

    #[test]
    fn pred_latency_war_waw() {
        assert_eq!(PredLatencySM100::war(PredLatencySM100::Fma, PredLatencySM100::Coupled, false), 1);
        assert_eq!(PredLatencySM100::waw(PredLatencySM100::Fma, PredLatencySM100::Decoupled, false), 1);
    }

    #[test]
    fn ureg_latency_raw_fma_fma() {
        assert_eq!(UregLatencySM100::raw(UregLatencySM100::Fma, UregLatencySM100::Fma, false), 5);
    }

    #[test]
    fn ureg_latency_raw_uldc_tex() {
        assert_eq!(UregLatencySM100::raw(UregLatencySM100::Uldc, UregLatencySM100::Coupled, false), 8);
        assert_eq!(UregLatencySM100::raw(UregLatencySM100::Tex, UregLatencySM100::Fma, false), 8);
    }

    #[test]
    fn ureg_latency_raw_default() {
        assert_eq!(UregLatencySM100::raw(UregLatencySM100::Coupled, UregLatencySM100::Decoupled, false), 6);
    }

    #[test]
    fn ureg_latency_war_waw() {
        assert_eq!(UregLatencySM100::war(UregLatencySM100::Fma, UregLatencySM100::Tex, false), 1);
        assert_eq!(UregLatencySM100::waw(UregLatencySM100::Uldc, UregLatencySM100::Coupled, false), 1);
    }

    #[test]
    fn upred_latency_raw_fma_fma() {
        assert_eq!(UpredLatencySM100::raw(UpredLatencySM100::Fma, UpredLatencySM100::Fma, false), 5);
    }

    #[test]
    fn upred_latency_raw_default() {
        assert_eq!(UpredLatencySM100::raw(UpredLatencySM100::Coupled, UpredLatencySM100::Decoupled, false), 6);
    }

    #[test]
    fn upred_latency_war_waw() {
        assert_eq!(UpredLatencySM100::war(UpredLatencySM100::Fma, UpredLatencySM100::BraJmp, false), 1);
        assert_eq!(UpredLatencySM100::waw(UpredLatencySM100::UGuard, UpredLatencySM100::Voteu, false), 1);
    }

    #[test]
    fn latency_enums_derive_traits() {
        use std::collections::hash_map::DefaultHasher;
        use std::hash::{Hash, Hasher};

        let r = RegLatencySM100::Alu;
        let _ = format!("{r:?}");
        let _ = r;
        assert_eq!(r, RegLatencySM100::Alu);
        let mut h = DefaultHasher::new();
        r.hash(&mut h);
        let _ = h.finish();
    }
}
