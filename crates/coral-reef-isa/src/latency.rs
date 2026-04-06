// SPDX-License-Identifier: AGPL-3.0-or-later
//! Instruction latency model for NVIDIA GPU scheduling.
//!
//! Each instruction has execution latency and throughput characteristics
//! that the scheduler uses to overlap independent operations.

/// Instruction latency information.
#[derive(Debug, Clone, Copy)]
pub struct InstrLatency {
    /// Execution latency in cycles.
    pub latency: u32,
    /// Throughput (instructions per cycle per SM).
    pub throughput: f32,
}

impl InstrLatency {
    /// Default latency for unknown instructions.
    pub const DEFAULT: Self = Self {
        latency: 6,
        throughput: 1.0,
    };

    /// DFMA (double-precision FMA) latency — critical for f64 software lowering.
    pub const DFMA: Self = Self {
        latency: 8,
        throughput: 0.5,
    };

    /// MUFU (multi-function unit) latency.
    pub const MUFU: Self = Self {
        latency: 5,
        throughput: 1.0,
    };

    /// Integer ALU.
    pub const IALU: Self = Self {
        latency: 4,
        throughput: 2.0,
    };
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_dfma_slower_than_mufu() {
        const { assert!(InstrLatency::DFMA.latency > InstrLatency::MUFU.latency) };
    }

    #[test]
    fn test_default_latency() {
        assert_eq!(InstrLatency::DEFAULT.latency, 6);
        assert!((InstrLatency::DEFAULT.throughput - 1.0).abs() < f32::EPSILON);
    }

    #[test]
    fn test_ialu_fastest() {
        const { assert!(InstrLatency::IALU.latency < InstrLatency::DEFAULT.latency) };
    }
}
