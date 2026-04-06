// SPDX-License-Identifier: AGPL-3.0-or-later
//! Sovereign GSP — Rust GPU System Processor.
//!
//! A learned, adaptive replacement for NVIDIA's proprietary GSP firmware.
//! Observes hardware initialization on GPUs that have working firmware
//! (Ampere+ with GSP, AMD with open-source drivers) and distills that
//! knowledge into init recipes that can bring up older/firmware-limited
//! GPUs (Volta, Turing without GSP).
//!
//! # Architecture
//!
//! ```text
//!                       ┌─────────────────────────────┐
//!                       │     Sovereign GSP            │
//!                       │                              │
//!  ┌──────────┐   learn │  ┌──────────┐  ┌──────────┐ │
//!  │ RTX 3090 │────────►│  │ Observer  │  │ Knowledge│ │
//!  │ (GSP fw) │         │  └────┬─────┘  └────▲─────┘ │
//!  └──────────┘         │       │              │       │
//!  ┌──────────┐   learn │  ┌────▼─────┐       │       │
//!  │ RX 6950  │────────►│  │ Distiller├───────┘       │
//!  │ (amdgpu) │         │  └──────────┘               │
//!  └──────────┘         │                              │
//!                       │  ┌──────────┐  ┌──────────┐ │
//!                       │  │Applicator│  │ Optimizer │ │
//!                       │  └────┬─────┘  └────┬─────┘ │
//!                       └───────┼─────────────┼───────┘
//!                          apply│        boost│
//!                       ┌──────▼──┐   ┌──────▼──┐
//!                       │ Titan V │   │ RTX 3090│
//!                       │ (no fw) │   │(optimze)│
//!                       └─────────┘   └─────────┘
//! ```
//!
//! # How it learns
//!
//! 1. **Observe**: Parse mmiotrace captures, GSP RPC logs, NVIDIA firmware
//!    blobs (`sw_bundle_init.bin`), and amdgpu open-source init paths
//! 2. **Distill**: Extract minimal register sequences, classify by function,
//!    map across architectures using envytools register databases
//! 3. **Apply**: Replay init sequences on firmware-limited GPUs via BAR0 MMIO
//! 4. **Optimize**: On modern GPUs with working firmware, provide learned
//!    dispatch hints (workgroup sizing, memory placement, clock awareness)
//!
//! # Phase roadmap
//!
//! - **Phase 0** (done): Firmware blob parser + GR init knowledge
//! - **Phase 1** (done): Cross-architecture register mapper + address space awareness
//! - **Phase 2** (done): Dispatch optimizer with learned hints
//! - **Phase 3**: BAR0 applicator for Volta (wired to nvPmu)
//! - **Phase 4**: Runtime learning from mmiotrace + live observation

mod applicator;
mod dispatch;
mod firmware_parser;
mod firmware_source;
mod gr_init;
mod knowledge;
pub mod rm_observer;
#[cfg(test)]
pub(crate) mod test_utils;

/// Applicator: bridge between learned init sequences and BAR0 hardware access.
pub use applicator::{
    ApplyError, RegisterAccess, apply_bar0, dry_run, split_for_application, verify_pre_init,
};
/// Dispatch hints — learned workgroup sizing, FP64 availability, etc.
pub use dispatch::{DispatchHints, build_dispatch_hints, build_hint_for};
/// Parsed GR firmware blobs from `/lib/firmware/nvidia/{chip}/gr/`.
pub use firmware_parser::{BundleEntry, FirmwareFormat, GrFirmwareBlobs, MethodEntry};
/// Pluggable NVIDIA firmware loading ([`NvidiaFirmwareSource`]) and filesystem implementation.
pub use firmware_source::{FilesystemFirmwareSource, NvidiaFirmwareSource};
/// GR engine init register sequence (learned from firmware).
pub use gr_init::{GrInitSequence, GrRegWrite, RegCategory};
/// Cross-architecture GPU knowledge base.
pub use knowledge::{
    AddressSpace, ArchKnowledge, GenerationStats, GpuKnowledge, RegisterTransferMap,
};
/// RM Protocol Observer — captures RM operations for virtual GSP learning.
pub use rm_observer::{LoggingObserver, RmAllocEvent, RmObserver, RmProtocolLog};
