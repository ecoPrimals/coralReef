// SPDX-License-Identifier: AGPL-3.0-only
// Copyright © 2026 ecoPrimals

//! Unit tests for `coral-gpu`, split by theme (mocks, context, drivers, FMA, PCIe, Linux-only).

mod common;
mod context_compile;
mod context_compile_dispatch;
mod context_from_parts;
mod driver_errors;
mod driver_preference;
mod error_display;
mod fma;
mod hash_wgsl;
mod kernel_metadata_cache;
#[cfg(target_os = "linux")]
mod linux;
mod mock_device;
mod no_device;
#[cfg(not(target_os = "linux"))]
mod non_linux;
mod pcie;
