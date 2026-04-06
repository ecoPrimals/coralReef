// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright © 2026 ecoPrimals

//! Unit tests for `coral-gpu`, split by theme (mocks, context, drivers, FMA, PCIe, Linux-only).

mod common;
mod context_accessors_and_errors;
mod context_compile;
mod context_compile_dispatch;
mod context_from_parts;
mod context_wave_size;
mod driver_errors;
mod driver_preference;
mod driver_sm_arch;
#[cfg(all(target_os = "linux", feature = "vfio"))]
mod driver_vfio;
mod error_display;
mod fma;
mod hash_wgsl;
mod kernel_cache_roundtrip;
mod kernel_metadata_cache;
#[cfg(target_os = "linux")]
mod linux;
mod local_gpu;
mod mock_device;
mod no_device;
#[cfg(not(target_os = "linux"))]
mod non_linux;
mod pcie;
