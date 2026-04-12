// SPDX-License-Identifier: AGPL-3.0-only
//! Test: create GR compute context via GROBJ_ALLOC after CHANNEL_ALLOC.
//!
//! On Volta+, CHANNEL_ALLOC creates a bare GPFIFO channel. The GR engine
//! context must be created separately. This test uses GROBJ_ALLOC to create
//! the VOLTA_COMPUTE_A object within the channel, then submits a SET_OBJECT
//! push buffer to verify the context is valid.

use coral_driver::drm::{self, DrmDevice};
use coral_driver::nv::{ioctl, pushbuf};
use coral_driver::MemoryDomain;

fn main() {
    println!("=== GROBJ_ALLOC Context Test ===\n");

    let drm = DrmDevice::open_by_driver("nouveau").expect("open nouveau");
    let fd = drm.fd();
    println!("  Opened {}", drm.path);

    // Phase 1: VM_INIT (new UAPI)
    print!("  VM_INIT... ");
    let new_uapi = match ioctl::vm_init(fd) {
        Ok(()) => {
            println!("OK (new UAPI)");
            true
        }
        Err(e) => {
            println!("not available ({e}), legacy");
            false
        }
    };

    // Phase 2: CHANNEL_ALLOC (bare GPFIFO channel)
    let compute_class = ioctl::NVIF_CLASS_VOLTA_COMPUTE_A;
    print!("  CHANNEL_ALLOC (class 0x{compute_class:04X})... ");
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

    // Phase 3: Query supported subchannel classes via NVIF SCLASS.
    let channel_token = u64::from(channel);
    print!("  NVIF SCLASS (token={channel_token})... ");
    match ioctl::nvif_sclass(fd, channel_token) {
        Ok(classes) => {
            println!("OK ({} classes)", classes.len());
            for c in &classes {
                println!("    class 0x{c:08X}");
            }
        }
        Err(e) => println!("FAILED: {e}"),
    }

    // Phase 4: Create compute subchannel object via NVIF NEW.
    // Creating VOLTA_COMPUTE_A implicitly creates GR context + related objects.
    print!("  NVIF NEW (compute, 0x{compute_class:04X})... ");
    match ioctl::nvif_object_new(fd, channel_token, 1, compute_class as i32) {
        Ok(()) => println!("OK"),
        Err(e) => println!("FAILED: {e}"),
    }

    // Try SET_OBJECT with various values to find what the GR engine accepts.
    // After NVIF NEW, test: nop, class_code, handle, gr_class.
    // Also try skipping SET_OBJECT and sending compute methods directly.

    // Phase 4: Create syncobj for completion tracking
    let syncobj = if new_uapi {
        print!("  Syncobj... ");
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
        None
    };

    // Phase 5: Allocate GEM buffer for push buffer
    let gem = ioctl::gem_new(fd, 4096, MemoryDomain::Gtt).expect("gem alloc");
    println!(
        "  GEM: handle={}, offset=0x{:X}",
        gem.handle, gem.offset,
    );

    // Phase 6: VM_BIND (if new UAPI)
    let user_va: u64 = 0x1_0000_0000;
    let gpu_va = if new_uapi {
        ioctl::vm_bind_map(fd, gem.handle, user_va, 0, 4096).expect("vm_bind");
        println!("  VM_BIND at 0x{user_va:X}");
        user_va
    } else {
        gem.offset
    };

    // Phase 7: Write push buffer.
    // NVK subchannel mapping: 0=3D(0xC397), 1=Compute(0xC3C0)
    let mode = std::env::var("PB_MODE").unwrap_or_else(|_| "nop".to_string());
    let pb_words: Vec<u32> = match mode.as_str() {
        // subchan 0 + compute class → CLASS_SUBCH_MISMATCH (known bad)
        "class" => vec![
            pushbuf::mthd_incr(0, pushbuf::method::SET_OBJECT, 1),
            compute_class,
        ],
        // subchan 0 + GR class → works (known good)
        "gr" => vec![
            pushbuf::mthd_incr(0, pushbuf::method::SET_OBJECT, 1),
            0xC397,
        ],
        // subchan 1 + compute class (NVK's actual compute subchannel)
        "sc1" => vec![
            pushbuf::mthd_incr(1, pushbuf::method::SET_OBJECT, 1),
            compute_class,
        ],
        // subchan 0=GR + subchan 1=compute (full NVK init)
        "full" => vec![
            pushbuf::mthd_incr(0, pushbuf::method::SET_OBJECT, 1),
            0xC397,
            pushbuf::mthd_incr(1, pushbuf::method::SET_OBJECT, 1),
            compute_class,
        ],
        // subchan 1=compute + INVALIDATE_SHADER_CACHES (0x021C for NVC3C0)
        "sc1_inv" => vec![
            pushbuf::mthd_incr(1, pushbuf::method::SET_OBJECT, 1),
            compute_class,
            pushbuf::mthd_incr(1, 0x021C, 1),
            0x11, // invalidate instr + data
        ],
        // subchan 1 + corrected NVC3C0 offsets (no real QMD yet)
        "sc1_dispatch" => {
            let local_mem: u64 = 0xFF00_0000;
            vec![
                pushbuf::mthd_incr(1, 0x0000, 1), // SET_OBJECT
                compute_class,
                pushbuf::mthd_incr(1, 0x021C, 1), // INVALIDATE_SHADER_CACHES
                0x11,
                pushbuf::mthd_incr(1, 0x07B0, 1), // SET_SHADER_LOCAL_MEMORY_WINDOW_A
                (local_mem >> 32) as u32,
                pushbuf::mthd_incr(1, 0x07B4, 1), // SET_SHADER_LOCAL_MEMORY_WINDOW_B
                local_mem as u32,
            ]
        },
        _ => vec![0x0000_0000], // NOP
    };
    println!("  Push buffer mode: {mode} ({} words)", pb_words.len());
    let pb_bytes = bytemuck::cast_slice::<u32, u8>(pb_words.as_slice());

    let mut region = ioctl::gem_mmap_region(fd, gem.map_handle, 4096).expect("mmap");
    region
        .slice_at_mut(0, pb_bytes.len())
        .expect("slice")
        .copy_from_slice(pb_bytes);
    println!("  Wrote {} bytes to push buffer", pb_bytes.len());

    // Phase 8: Submit
    print!("  Submit... ");
    let submit_ok = if new_uapi {
        if let Some(sync_handle) = syncobj {
            ioctl::exec_submit_with_signal(fd, channel, gpu_va, pb_bytes.len() as u32, sync_handle)
        } else {
            ioctl::exec_submit(fd, channel, gpu_va, pb_bytes.len() as u32)
        }
    } else {
        ioctl::pushbuf_submit(fd, channel, gem.handle, 0, pb_bytes.len() as u64, &[gem.handle])
    };
    match submit_ok {
        Ok(()) => println!("OK"),
        Err(e) => {
            println!("FAILED: {e}");
        }
    }

    // Phase 9: Wait
    print!("  Sync... ");
    if let Some(sync_handle) = syncobj {
        let deadline = {
            let now = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default();
            (now.as_nanos() as i64).saturating_add(2_000_000_000)
        };
        match ioctl::syncobj_wait(fd, sync_handle, deadline) {
            Ok(()) => println!("OK (syncobj signaled)"),
            Err(e) => println!("WARN: {e}"),
        }
    } else {
        match ioctl::gem_cpu_prep(fd, gem.handle) {
            Ok(()) => println!("OK (gem_cpu_prep)"),
            Err(e) => println!("WARN: {e}"),
        }
    }

    // Cleanup
    if new_uapi {
        let _ = ioctl::vm_bind_unmap(fd, user_va, 4096);
    }
    if let Some(sync_handle) = syncobj {
        let _ = ioctl::syncobj_destroy(fd, sync_handle);
    }
    let _ = drm::gem_close(fd, gem.handle);
    let _ = ioctl::destroy_channel(fd, channel);
    println!("\n  Done.");
}
