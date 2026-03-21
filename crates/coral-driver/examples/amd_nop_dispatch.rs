// SPDX-License-Identifier: AGPL-3.0-only
//! AMD DRM NOP dispatch test — validates the entire PM4 submission pipeline.
//!
//! hotSpring Exp 072: Proves `AmdDevice::open()` → alloc → upload → dispatch →
//! sync works end-to-end on GCN5 (MI50) or RDNA hardware via `amdgpu` DRM.
//!
//! The shader is a single `s_endpgm` instruction (0xBF810000), which is
//! identical across all AMD architectures from GCN1 through RDNA4.
//!
//! Run with: `cargo run --example amd_nop_dispatch`
//! Requires: MI50/Radeon VII bound to `amdgpu` driver.

use coral_driver::amd::AmdDevice;
use coral_driver::{ComputeDevice, DispatchDims, MemoryDomain, ShaderInfo};

fn main() {
    println!("╔═══════════════════════════════════════════════╗");
    println!("║  AMD DRM NOP Dispatch Test (Exp 072)         ║");
    println!("╚═══════════════════════════════════════════════╝\n");

    // Phase 1: Open device
    print!("  Phase 1: AmdDevice::open()... ");
    let mut dev = match AmdDevice::open() {
        Ok(d) => {
            println!("OK ✓");
            d
        }
        Err(e) => {
            println!("FAILED: {e}");
            println!("\n  Hint: Is the MI50 bound to amdgpu?");
            println!("  Try: coralctl swap 0000:4d:00.0 amdgpu");
            std::process::exit(1);
        }
    };

    // Phase 2: Allocate shader buffer
    // s_endpgm is a 4-byte SOPP instruction: 0xBF810000
    // The PM4 builder aligns the shader to 256 bytes, so we allocate enough.
    let s_endpgm: [u8; 4] = 0xBF81_0000_u32.to_le_bytes();

    print!("  Phase 2: Alloc shader buffer (256 bytes GTT)... ");
    let shader_handle = match dev.alloc(256, MemoryDomain::Gtt) {
        Ok(h) => {
            println!("OK ✓");
            h
        }
        Err(e) => {
            println!("FAILED: {e}");
            std::process::exit(1);
        }
    };

    // Phase 3: Upload s_endpgm
    print!("  Phase 3: Upload s_endpgm (0xBF810000)... ");
    match dev.upload(shader_handle, 0, &s_endpgm) {
        Ok(()) => println!("OK ✓"),
        Err(e) => {
            println!("FAILED: {e}");
            std::process::exit(1);
        }
    }

    // Phase 4: Dispatch with minimal config
    let info = ShaderInfo {
        gpr_count: 4,
        shared_mem_bytes: 0,
        barrier_count: 0,
        workgroup: [1, 1, 1],
        wave_size: 32,
    };
    let dims = DispatchDims::new(1, 1, 1);

    print!("  Phase 4: Dispatch (1×1×1 workgroups)... ");
    match dev.dispatch(&s_endpgm, &[], dims, &info) {
        Ok(()) => println!("OK ✓"),
        Err(e) => {
            println!("FAILED: {e}");
            println!("\n  PM4 submission or CS ioctl failed.");
            println!("  This means the command buffer was rejected by the kernel.");
            std::process::exit(1);
        }
    }

    // Phase 5: Sync (wait for GPU completion)
    print!("  Phase 5: Sync (fence wait)... ");
    match dev.sync() {
        Ok(()) => println!("OK ✓"),
        Err(e) => {
            println!("FAILED: {e}");
            println!("\n  GPU dispatch timed out or faulted.");
            std::process::exit(1);
        }
    }

    println!("\n  ═══════════════════════════════════════════════");
    println!("  ✓ AMD DRM NOP DISPATCH: ALL PHASES PASSED");
    println!("  ═══════════════════════════════════════════════");
    println!("\n  PM4 command submission → GPU execution → fence completion.");
    println!("  The amdgpu DRM dispatch pipeline is functional.");
    println!("  Next: GCN5 backend in coral-reef for real compute shaders.");
}
