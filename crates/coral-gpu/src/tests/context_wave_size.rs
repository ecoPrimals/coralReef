// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright © 2026 ecoPrimals

//! [`GpuContext::wave_size`] for AMD wave64 vs wave32 vs NVIDIA.

use crate::GpuContext;
use coral_reef::{AmdArch, GpuTarget, NvArch};

#[test]
fn wave_size_nvidia_is_32() {
    let ctx =
        GpuContext::new(GpuTarget::Nvidia(NvArch::Sm86)).expect("nvidia target should construct");
    assert_eq!(ctx.wave_size(), 32);
}

#[test]
fn wave_size_amd_gcn5_is_64() {
    let ctx = GpuContext::new(GpuTarget::Amd(AmdArch::Gcn5)).expect("amd target");
    assert_eq!(ctx.wave_size(), 64);
}

#[test]
fn wave_size_amd_rdna2_is_32() {
    let ctx = GpuContext::new(GpuTarget::Amd(AmdArch::Rdna2)).expect("amd target");
    assert_eq!(ctx.wave_size(), 32);
}
