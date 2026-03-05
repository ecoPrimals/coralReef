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
