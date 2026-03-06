// SPDX-License-Identifier: AGPL-3.0-only
// Copyright © 2025-2026 ecoPrimals
// Derived from Collabora, Ltd. (2023)
//! NVIDIA-specific backend modules (SM20–SM120).
//!
//! This namespace groups all NVIDIA hardware-specific code:
//! architecture models, instruction encoders, latency tables, and the
//! Shader Program Header (SPH) encoder.  It is separated from the
//! shared compiler infrastructure to prepare for multi-vendor backend
//! extraction into dedicated crates.

pub(crate) mod shader_header;
pub(crate) mod sm120_instr_latencies;
pub(crate) mod sm20;
pub(crate) mod sm30_instr_latencies;
pub(crate) mod sm32;
pub(crate) mod sm50;
pub(crate) mod sm70;
pub(crate) mod sm70_encode;
pub(crate) mod sm70_instr_latencies;
pub(crate) mod sm75_instr_latencies;
pub(crate) mod sm80_instr_latencies;
