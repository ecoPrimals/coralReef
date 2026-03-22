// SPDX-License-Identifier: AGPL-3.0-only
//! MMU fault diagnostic — opens VFIO device, creates channel + page tables,
//! submits NOP GPFIFO, then captures structured MMU fault state.
//!
//! Usage: `cargo run --example bench_mmu_fault_diagnostic --features vfio -- <BDF>`
//! Example: `cargo run --example bench_mmu_fault_diagnostic --features vfio -- 0000:06:00.0`
//!
//! Requires: GPU bound to `vfio-pci`, IOMMU enabled.

use coral_driver::nv::vfio_compute::RawVfioDevice;
use coral_driver::vfio::channel::mmu_fault;
use coral_driver::vfio::channel::VfioChannel;

fn main() {
    let bdf = std::env::args().nth(1).unwrap_or_else(|| {
        eprintln!("Usage: bench_mmu_fault_diagnostic <BDF>");
        eprintln!("Example: bench_mmu_fault_diagnostic 0000:06:00.0");
        std::process::exit(1);
    });

    eprintln!("═══════════════════════════════════════════════════════════════");
    eprintln!("  MMU Fault Diagnostic — {bdf}");
    eprintln!("═══════════════════════════════════════════════════════════════");

    eprintln!("\n▶ Phase 1: Open VFIO device");
    let raw = match RawVfioDevice::open(&bdf) {
        Ok(dev) => dev,
        Err(e) => {
            eprintln!("  ✗ Failed to open VFIO device: {e}");
            std::process::exit(1);
        }
    };
    eprintln!("  ✓ VFIO device opened");

    let boot0 = raw.bar0.read_u32(0x0000_0000).unwrap_or(0xDEAD);
    eprintln!("  BOOT0 = {boot0:#010x}");

    eprintln!("\n▶ Phase 2: Pre-channel MMU state");
    let pre_fault = mmu_fault::read_mmu_faults(&raw.bar0);
    print_fault("pre-channel", &pre_fault);

    eprintln!("\n▶ Phase 3: Create PFIFO channel");
    let channel = match VfioChannel::create(
        raw.container.clone(),
        &raw.bar0,
        RawVfioDevice::gpfifo_iova(),
        RawVfioDevice::gpfifo_entries(),
        RawVfioDevice::userd_iova(),
        0,
    ) {
        Ok(ch) => {
            eprintln!("  ✓ Channel created (id={})", ch.id());
            ch
        }
        Err(e) => {
            eprintln!("  ✗ Channel creation failed: {e}");
            eprintln!("\n▶ Post-failure MMU state:");
            let fail_fault = mmu_fault::read_mmu_faults(&raw.bar0);
            print_fault("post-channel-fail", &fail_fault);
            raw.leak();
            std::process::exit(1);
        }
    };

    eprintln!("\n▶ Phase 4: Post-channel MMU state");
    let post_ch_fault = mmu_fault::read_mmu_faults(&raw.bar0);
    print_fault("post-channel", &post_ch_fault);

    eprintln!("\n▶ Phase 5: Submit NOP GPFIFO entry");
    // Write a NOP GPFIFO entry (zero = NOP) to slot 0.
    let ring_slice = raw.gpfifo_ring.as_slice();
    let ring_ptr = ring_slice.as_ptr() as *mut u64;
    // SAFETY: gpfifo_ring DMA buffer is valid; writing slot 0.
    unsafe { std::ptr::write_volatile(ring_ptr, 0u64) };

    // Write GP_PUT=1 to USERD at Volta RAMUSERD offset 0x8C.
    let userd_slice = raw.userd.as_slice();
    let userd_ptr = userd_slice.as_ptr();
    // SAFETY: userd DMA buffer is valid 4096-byte page; 0x8C within bounds.
    unsafe {
        let gp_put_ptr = userd_ptr.add(0x8C) as *mut u32;
        std::ptr::write_volatile(gp_put_ptr, 1);
    }

    std::sync::atomic::fence(std::sync::atomic::Ordering::SeqCst);

    if let Err(e) = raw.bar0.write_u32(VfioChannel::doorbell_offset(), channel.id()) {
        eprintln!("  ✗ Doorbell write failed: {e}");
    } else {
        eprintln!("  ✓ Doorbell written (channel_id={})", channel.id());
    }

    std::thread::sleep(std::time::Duration::from_millis(500));

    eprintln!("\n▶ Phase 6: Post-NOP MMU state");
    let post_nop_fault = mmu_fault::read_mmu_faults(&raw.bar0);
    print_fault("post-nop", &post_nop_fault);

    // Read GP_GET from USERD at Volta RAMUSERD offset 0x88.
    let gp_get = unsafe {
        let gp_get_ptr = userd_ptr.add(0x88) as *const u32;
        std::ptr::read_volatile(gp_get_ptr)
    };
    eprintln!("  USERD GP_GET = {gp_get} (expected: 1 if consumed)");

    eprintln!("\n▶ Phase 7: PBDMA state");
    for pbdma_id in 0..4_usize {
        let base = 0x40000 + pbdma_id * 0x2000;
        let intr = raw.bar0.read_u32(base + 0x108).unwrap_or(0xDEAD);
        let state = raw.bar0.read_u32(base + 0xB0).unwrap_or(0xDEAD);
        let gp_fetch = raw.bar0.read_u32(base + 0x48).unwrap_or(0xDEAD);
        let gp_put = raw.bar0.read_u32(base + 0x54).unwrap_or(0xDEAD);
        let userd_lo = raw.bar0.read_u32(base + 0xD0).unwrap_or(0xDEAD);
        let gpbase = raw.bar0.read_u32(base + 0x40).unwrap_or(0xDEAD);
        let sig = raw.bar0.read_u32(base + 0xC0).unwrap_or(0xDEAD);
        if intr != 0 || state != 0 || gp_fetch != 0 {
            eprintln!(
                "  PBDMA{pbdma_id}: INTR={intr:#010x} STATE={state:#010x} GP_FETCH={gp_fetch} GP_PUT={gp_put} USERD={userd_lo:#010x} GP_BASE={gpbase:#010x} SIG={sig:#010x}"
            );
        }
    }

    let pfifo_intr = raw.bar0.read_u32(0x2100).unwrap_or(0xDEAD);
    let priv_ring = raw.bar0.read_u32(0x0001_2070).unwrap_or(0xDEAD);
    eprintln!("  PFIFO_INTR = {pfifo_intr:#010x}");
    eprintln!("  PRIV_RING  = {priv_ring:#010x}");

    eprintln!("\n═══════════════════════════════════════════════════════════════");
    if post_nop_fault.has_fault {
        eprintln!("  RESULT: MMU fault detected — see decoded fault above");
        eprintln!("  Fault type: {}", post_nop_fault.fault_type);
        eprintln!("  Faulting VA: {:#018x}", post_nop_fault.fault_va);
        eprintln!("  Engine: {}", post_nop_fault.engine);
    } else if gp_get >= 1 {
        eprintln!("  RESULT: NOP consumed! GPFIFO dispatch succeeded.");
    } else {
        eprintln!("  RESULT: No fault but GP_GET=0 — PBDMA did not fetch.");
    }
    eprintln!("═══════════════════════════════════════════════════════════════");

    std::mem::forget(channel);
    raw.leak();
}

fn print_fault(label: &str, info: &mmu_fault::MmuFaultInfo) {
    eprintln!("  [{label}] fault_status  = {:#010x}", info.fault_status);
    eprintln!("  [{label}] fault_va      = {:#018x}", info.fault_va);
    eprintln!(
        "  [{label}] fault_inst    = {:#010x}_{:#010x}",
        info.fault_inst_hi, info.fault_inst_lo
    );
    eprintln!("  [{label}] mmu_ctrl      = {:#010x}", info.mmu_ctrl);
    eprintln!("  [{label}] hubtlb_err    = {:#010x}", info.hubtlb_err);
    eprintln!(
        "  [{label}] fault_buf0    = GET={} PUT={}",
        info.fault_buf0_get, info.fault_buf0_put
    );
    if info.has_fault {
        eprintln!(
            "  [{label}] ⚠ FAULT: type={} access={} engine={} aperture={}",
            info.fault_type, info.access_type, info.engine, info.aperture
        );
    } else {
        eprintln!("  [{label}] (no fault)");
    }
}
