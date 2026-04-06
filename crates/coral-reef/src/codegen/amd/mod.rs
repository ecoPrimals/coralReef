// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright © 2026 ecoPrimals
//! AMD GPU backend — RDNA2+ (GFX10/GFX11) instruction encoding and dispatch.
//!
//! This module implements the AMD backend for coralReef, targeting RDNA2
//! (GFX1030, e.g. RX 6950 XT) and later architectures.
//!
//! ## Architecture
//!
//! AMD GPUs use a fundamentally different ISA from NVIDIA:
//! - **Wave32/64** execution (vs NVIDIA warp=32)
//! - **VGPR/SGPR** register split (vector/scalar, vs NVIDIA GPR/UGPR)
//! - **Exec mask** predication (vs NVIDIA per-thread predicates)
//! - **Fixed-width encodings** for most instructions (32/64-bit)
//! - **No SPH** — compute kernels use an ELF-like metadata format
//!
//! ## ISA Reference
//!
//! Encoding tables are derived from AMD's machine-readable ISA XML
//! specifications (MIT license, GPUOpen). The XML spec for RDNA2 is
//! stored at `specs/amd/amdgpu_isa_rdna2.xml`.

pub mod encoding;
pub mod isa;
pub mod isa_generated;
pub mod reg;
pub mod shader_model;
