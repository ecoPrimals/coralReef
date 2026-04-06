// SPDX-License-Identifier: AGPL-3.0-or-later
//! Layered GPU interpreter — discovers how to interface with a blank GPU.
//!
//! Each layer probes the GPU and produces a typed output that the next layer
//! consumes. The type system enforces the dependency chain: Layer N+1 cannot
//! compile without Layer N's output. Failures carry diagnostic evidence,
//! enabling informed retry or alternative strategies.
//!
//! ```text
//! Layer 0: BAR      → BarTopology      (can I MMIO?)
//! Layer 1: Identity → GpuIdentity      (what GPU is this?)
//! Layer 2: Power    → PowerState       (is it alive? can I wake it?)
//! Layer 3: Engines  → EngineTopology   (what PBDMAs/runlists exist?)
//! Layer 4: MMU      → MmuConfig        (can it translate addresses?)
//! Layer 5: DMA      → DmaCapability    (can it read/write system memory?)
//! Layer 6: Channel  → ChannelConfig    (can I create a command channel?)
//! Layer 7: Dispatch → DispatchResult   (can I execute GPU commands?)
//! ```

pub mod layers;
pub mod memory_probe;
mod probe;

pub use layers::*;
pub use probe::{ProbeInterpreter, ProbeReport};
