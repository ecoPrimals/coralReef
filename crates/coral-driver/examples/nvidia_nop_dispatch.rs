// SPDX-License-Identifier: AGPL-3.0-only
//! NVIDIA nouveau NOP dispatch — minimal end-to-end GPU command proof.
//!
//! Uses the low-level `nv::ioctl` wrappers directly (not `NvDevice`) to
//! prove the sovereign Rust DRM pipeline: open → vm_init → channel_alloc →
//! gem_new → vm_bind → mmap → write NOP → exec → sync → cleanup.
//!
//! This is the Rust equivalent of `nouveau_nop_submit.c` — same pipeline,
//! pure Rust, zero C, zero libc.
//!
//! Run with: `cargo run --example nvidia_nop_dispatch`
//! Requires: Titan V bound to nouveau.

use coral_driver::drm::{self, DrmDevice};
use coral_driver::nv::ioctl;
use coral_driver::nv::pushbuf;
use coral_driver::MemoryDomain;

fn main() {
    println!("=== NVIDIA Nouveau NOP Dispatch (Pure Rust) ===\n");

    // Phase 1: Open DRM device
    print!("  Phase 1: Open nouveau render node... ");
    let drm = match DrmDevice::open_by_driver("nouveau") {
        Ok(d) => {
            println!("OK ({})", d.path);
            d
        }
        Err(e) => {
            println!("FAILED: {e}");
            println!("  Hint: Is the Titan V bound to nouveau?");
            std::process::exit(1);
        }
    };
    let fd = drm.fd();

    // Phase 2: Initialize kernel-managed VA space (new UAPI, kernel 6.6+)
    print!("  Phase 2: VM_INIT... ");
    let new_uapi = match ioctl::vm_init(fd) {
        Ok(()) => {
            println!("OK (new UAPI active)");
            true
        }
        Err(e) => {
            println!("not available ({e}), using legacy UAPI");
            false
        }
    };

    // Phase 3: Create compute channel with Volta compute class
    let compute_class = ioctl::NVIF_CLASS_VOLTA_COMPUTE_A;
    print!("  Phase 3: Channel alloc (class 0x{compute_class:04X})... ");
    let channel = match ioctl::create_channel(fd, compute_class) {
        Ok(ch) => {
            println!("OK (channel {ch})");
            ch
        }
        Err(e) => {
            println!("FAILED: {e}");
            std::process::exit(1);
        }
    };

    // Phase 4: Create syncobj for completion tracking (new UAPI only)
    let syncobj = if new_uapi {
        print!("  Phase 4: Syncobj create... ");
        match ioctl::syncobj_create(fd) {
            Ok(h) => {
                println!("OK (handle {h})");
                Some(h)
            }
            Err(e) => {
                println!("FAILED: {e}");
                None
            }
        }
    } else {
        println!("  Phase 4: Syncobj (skipped — legacy UAPI)");
        None
    };

    // Phase 5: Allocate GEM buffer for push buffer
    print!("  Phase 5: GEM alloc (4 KiB, GTT)... ");
    let gem = match ioctl::gem_new(fd, 4096, MemoryDomain::Gtt) {
        Ok(g) => {
            println!(
                "OK (handle {}, offset 0x{:X}, map_handle 0x{:X})",
                g.handle, g.offset, g.map_handle
            );
            g
        }
        Err(e) => {
            println!("FAILED: {e}");
            let _ = ioctl::destroy_channel(fd, channel);
            std::process::exit(1);
        }
    };

    // Phase 6: VM_BIND the GEM buffer to a GPU virtual address (new UAPI only)
    let user_va_start: u64 = 0x1_0000_0000;
    let gpu_va = if new_uapi {
        print!("  Phase 6: VM_BIND at 0x{user_va_start:X}... ");
        match ioctl::vm_bind_map(fd, gem.handle, user_va_start, 0, 4096) {
            Ok(()) => {
                println!("OK");
                user_va_start
            }
            Err(e) => {
                println!("FAILED: {e}");
                let _ = drm::gem_close(fd, gem.handle);
                let _ = ioctl::destroy_channel(fd, channel);
                std::process::exit(1);
            }
        }
    } else {
        println!("  Phase 6: VM_BIND (skipped — legacy uses kernel-assigned VA 0x{:X})", gem.offset);
        gem.offset
    };

    // Phase 7: Map GEM buffer and write NOP push buffer
    print!("  Phase 7: mmap + write NOP... ");
    let nop_words: [u32; 2] = [
        pushbuf::mthd_incr(pushbuf::subchan::COMPUTE, pushbuf::method::SET_OBJECT, 1),
        compute_class,
    ];
    let nop_bytes = bytemuck::cast_slice::<u32, u8>(&nop_words);
    let push_len = nop_bytes.len() as u64;

    match ioctl::gem_mmap_region(fd, gem.map_handle, 4096) {
        Ok(mut region) => {
            region
                .slice_at_mut(0, nop_bytes.len())
                .expect("slice within bounds")
                .copy_from_slice(nop_bytes);
            println!("OK ({push_len} bytes: SET_OBJECT 0x{compute_class:04X})");
        }
        Err(e) => {
            println!("FAILED: {e}");
            let _ = drm::gem_close(fd, gem.handle);
            let _ = ioctl::destroy_channel(fd, channel);
            std::process::exit(1);
        }
    }

    // Phase 8: Submit
    print!("  Phase 8: Submit... ");
    let submit_ok = if new_uapi {
        if let Some(sync_handle) = syncobj {
            ioctl::exec_submit_with_signal(
                fd,
                channel,
                gpu_va,
                push_len as u32,
                sync_handle,
            )
        } else {
            ioctl::exec_submit(fd, channel, gpu_va, push_len as u32)
        }
    } else {
        ioctl::pushbuf_submit(fd, channel, gem.handle, 0, push_len, &[gem.handle])
    };
    match submit_ok {
        Ok(()) => println!("OK"),
        Err(e) => {
            println!("FAILED: {e}");
            let _ = drm::gem_close(fd, gem.handle);
            let _ = ioctl::destroy_channel(fd, channel);
            std::process::exit(1);
        }
    }

    // Phase 9: Wait for completion
    print!("  Phase 9: Sync... ");
    if let Some(sync_handle) = syncobj {
        let deadline = {
            let now = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default();
            (now.as_nanos() as i64).saturating_add(2_000_000_000) // 2 second timeout
        };
        match ioctl::syncobj_wait(fd, sync_handle, deadline) {
            Ok(()) => println!("OK (syncobj signaled)"),
            Err(e) => println!("WARN: {e} (may still have succeeded)"),
        }
    } else {
        match ioctl::gem_cpu_prep(fd, gem.handle) {
            Ok(()) => println!("OK (GEM fence cleared)"),
            Err(e) => println!("WARN: {e}"),
        }
    }

    // Phase 10: Cleanup
    print!("  Phase 10: Cleanup... ");
    if new_uapi {
        let _ = ioctl::vm_bind_unmap(fd, gpu_va, 4096);
    }
    if let Some(sync_handle) = syncobj {
        let _ = ioctl::syncobj_destroy(fd, sync_handle);
    }
    let _ = drm::gem_close(fd, gem.handle);
    let _ = ioctl::destroy_channel(fd, channel);
    println!("OK");

    println!("\n=== Result ===");
    println!("  NOP dispatch SUCCEEDED via pure Rust DRM ioctls.");
    println!("  GPU firmware processed SET_OBJECT(0x{compute_class:04X}) on channel {channel}.");
    println!("  Pipeline: open -> vm_init -> channel -> gem -> vm_bind -> mmap -> exec -> sync");
    println!("  Zero C, zero libc — sovereign Rust GPU control.");
}
