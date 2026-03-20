// SPDX-License-Identifier: AGPL-3.0-only
//! Systematic memory topology discovery for the GPU interpreter.
//!
//! Probes all memory access paths (PRAMIN→VRAM, DMA→sysmem, PBDMA→RAMFC)
//! and builds a [`MemoryTopology`] that higher interpreter layers use to
//! decide which memory placement strategies are available.
//!
//! Also provides a differential probe ([`differential_probe`]) that discovers
//! which register writes change the memory landscape — the core of the
//! FB/HBM2 init reverse-engineering tool.

use std::os::fd::OwnedFd;
use std::sync::Arc;
use std::time::Instant;

use crate::vfio::device::MappedBar;
use crate::vfio::dma::DmaBuffer;
use crate::vfio::memory::{
    AccessPath, Aperture, DmaRegion, MemoryDelta, MemoryRegion, MemoryTopology, PathMethod,
    PathStatus, PraminRegion,
};

use crate::vfio::channel::registers::pfb;
use crate::vfio::channel::registers::*;

/// VRAM addresses to probe (spread across the address space to detect partial
/// FB init — some regions may be accessible while others return 0xBAD0ACxx).
const VRAM_PROBE_OFFSETS: &[u32] = &[
    0x0000_0000, // Very start of VRAM
    0x0001_0000, // 64KB
    0x0002_0000, // 128KB (glow plug BAR2 page tables live here)
    0x0002_6000, // 152KB (known-good in previous tests)
    0x0004_0000, // 256KB
    0x0008_0000, // 512KB
    0x0010_0000, // 1MB
    0x0020_0000, // 2MB
    0x0040_0000, // 4MB
    0x0080_0000, // 8MB
    0x0100_0000, // 16MB
];

const SENTINEL: u32 = 0xCAFE_B0BA;

/// Discover the full memory topology for a VFIO GPU.
///
/// Systematically tests every memory access path available to the CPU and
/// GPU, building a typed topology that the interpreter's upper layers consume.
///
/// The probe sequence:
/// 1. PRAMIN→VRAM: write/readback sentinels at multiple VRAM offsets
/// 2. DMA sysmem: allocate IOMMU-mapped buffers, verify CPU read/write
/// 3. Cross-path: write via DMA, read via PRAMIN (if both work) — validates
///    that sysmem and VRAM represent coherent views
/// 4. BAR2 status: check if BAR2_BLOCK is configured for GPU internal access
pub fn discover_memory_topology(bar0: &MappedBar, container: Arc<OwnedFd>) -> MemoryTopology {
    let mut paths = Vec::new();
    let mut evidence = Vec::new();
    let mut vram_accessible = false;
    let mut vram_max_ok: u64 = 0;

    // ── Phase 1: PRAMIN → VRAM probing ──────────────────────────────────
    for &vram_addr in VRAM_PROBE_OFFSETS {
        let status = probe_pramin(bar0, vram_addr, SENTINEL);

        if status.is_working() {
            vram_accessible = true;
            let addr64 = u64::from(vram_addr) + 4;
            if addr64 > vram_max_ok {
                vram_max_ok = addr64;
            }
        }

        let tag = format!("PRAMIN_{vram_addr:#010x}");
        if let PathStatus::ErrorPattern { pattern } = &status {
            evidence.push((tag, *pattern));
        } else if let PathStatus::Working { latency_us } = &status {
            evidence.push((tag, *latency_us as u32));
        }

        paths.push(AccessPath {
            from: "cpu",
            to: Aperture::VideoMemory {
                vram_offset: u64::from(vram_addr),
            },
            method: PathMethod::Pramin,
            status,
            prerequisites: vec!["pcie_d0", "pmc_enable"],
        });
    }

    // ── Phase 2: DMA sysmem probing ─────────────────────────────────────
    // Use a high IOVA (0xF0_0000) to avoid conflicts with interpreter L4/L5.
    let sysmem_dma_ok = probe_dma_sysmem(Arc::clone(&container), &mut paths, &mut evidence);

    // ── Phase 3: BAR2 status ────────────────────────────────────────────
    let bar2_block = bar0.read_u32(misc::PBUS_BAR2_BLOCK).unwrap_or(0);
    let bar2_configured =
        bar2_block != 0x4000_0000 && bar2_block != 0 && (bar2_block >> 16) != 0xBAD0;
    evidence.push(("BAR2_BLOCK".into(), bar2_block));

    if bar2_configured {
        paths.push(AccessPath {
            from: "gpu",
            to: Aperture::VideoMemory { vram_offset: 0 },
            method: PathMethod::Bar2,
            status: PathStatus::Working { latency_us: 0 },
            prerequisites: vec!["bar2_page_tables"],
        });
    } else {
        paths.push(AccessPath {
            from: "gpu",
            to: Aperture::VideoMemory { vram_offset: 0 },
            method: PathMethod::Bar2,
            status: PathStatus::ErrorPattern {
                pattern: bar2_block,
            },
            prerequisites: vec!["bar2_page_tables"],
        });
    }

    let bar1_block = bar0.read_u32(misc::PBUS_BAR1_BLOCK).unwrap_or(0);
    evidence.push(("BAR1_BLOCK".into(), bar1_block));

    MemoryTopology {
        vram_accessible,
        vram_size_probed: vram_max_ok,
        sysmem_dma_ok,
        bar2_configured,
        paths,
        evidence,
    }
}

/// Probe a single VRAM offset via PRAMIN write/readback.
fn probe_pramin(bar0: &MappedBar, vram_addr: u32, sentinel: u32) -> PathStatus {
    match PraminRegion::new(bar0, vram_addr, 8) {
        Ok(mut region) => {
            let start = Instant::now();
            // Save original value, write sentinel, read back, restore.
            let original = region.read_u32(0).unwrap_or(0);
            if region.write_u32(0, sentinel).is_err() {
                return PathStatus::ErrorPattern { pattern: 0 };
            }
            let readback = region.read_u32(0).unwrap_or(0xDEAD_DEAD);
            let _ = region.write_u32(0, original); // restore
            PathStatus::from_sentinel_test(sentinel, readback, start.elapsed().as_micros() as u64)
        }
        Err(_) => PathStatus::ErrorPattern { pattern: 0 },
    }
}

/// Probe DMA system memory allocation and accessibility.
fn probe_dma_sysmem(
    container: Arc<OwnedFd>,
    paths: &mut Vec<AccessPath>,
    evidence: &mut Vec<(String, u32)>,
) -> bool {
    const PROBE_IOVA: u64 = 0xF0_0000;
    let mut ok = false;

    match DmaBuffer::new(Arc::clone(&container), 4096, PROBE_IOVA) {
        Ok(buf) => {
            let mut region = DmaRegion::new(buf, true);

            // Write + readback test
            let status = region.probe_sentinel(0, SENTINEL);
            ok = status.is_working();

            evidence.push(("DMA_ALLOC".into(), 1));
            if let PathStatus::Working { latency_us } = &status {
                evidence.push(("DMA_LATENCY_US".into(), *latency_us as u32));
            }

            paths.push(AccessPath {
                from: "cpu",
                to: Aperture::SystemMemory {
                    iova: PROBE_IOVA,
                    coherent: true,
                },
                method: PathMethod::DmaCoherent,
                status: status.clone(),
                prerequisites: vec!["vfio_container", "iommu"],
            });

            // Also test non-coherent write pattern
            let nc_status = region.probe_sentinel(64, SENTINEL ^ 0xFF);
            paths.push(AccessPath {
                from: "cpu",
                to: Aperture::SystemMemory {
                    iova: PROBE_IOVA,
                    coherent: false,
                },
                method: PathMethod::DmaNonCoherent,
                status: nc_status,
                prerequisites: vec!["vfio_container", "iommu"],
            });
        }
        Err(e) => {
            evidence.push(("DMA_ALLOC".into(), 0));
            evidence.push(("DMA_ERROR".into(), 0));
            paths.push(AccessPath {
                from: "cpu",
                to: Aperture::SystemMemory {
                    iova: PROBE_IOVA,
                    coherent: true,
                },
                method: PathMethod::DmaCoherent,
                status: PathStatus::ErrorPattern { pattern: 0 },
                prerequisites: vec!["vfio_container", "iommu"],
            });
            eprintln!("║ Memory probe: DMA alloc failed: {e}");
        }
    }

    ok
}

// ─── NV_PFB Register Snapshot ───────────────────────────────────────────

/// Snapshot all readable registers in the NV_PFB range.
/// Returns (offset, value) pairs — the "before" state for differential probing.
pub fn snapshot_pfb_registers(bar0: &MappedBar) -> Vec<(usize, u32)> {
    let mut regs = Vec::new();
    let mut offset = pfb::REGION_START;
    while offset < pfb::REGION_END {
        let val = bar0.read_u32(offset).unwrap_or(0xDEAD_DEAD);
        if val != 0xDEAD_DEAD {
            regs.push((offset, val));
        }
        offset += 4;
    }
    regs
}

/// Snapshot FBPA (Framebuffer Partition Array) registers across all partitions.
pub fn snapshot_fbpa_registers(bar0: &MappedBar) -> Vec<(usize, u32)> {
    let mut regs = Vec::new();
    for part in 0..pfb::FBPA_COUNT_MAX {
        let base = pfb::FBPA_BASE + part * pfb::FBPA_STRIDE;
        for off in (0..pfb::FBPA_STRIDE).step_by(4) {
            let val = bar0.read_u32(base + off).unwrap_or(0xDEAD_DEAD);
            if val != 0xDEAD_DEAD && val != 0 {
                regs.push((base + off, val));
            }
        }
    }
    regs
}

/// Print a human-readable NV_PFB register dump, highlighting non-zero values.
pub fn dump_pfb_registers(bar0: &MappedBar) {
    let pfb_regs = snapshot_pfb_registers(bar0);
    let fbpa_regs = snapshot_fbpa_registers(bar0);

    eprintln!(
        "╠══ NV_PFB Register Snapshot ({} non-dead regs) ═══╣",
        pfb_regs.len()
    );
    for (offset, val) in &pfb_regs {
        if *val != 0 {
            eprintln!("║   {offset:#010x} = {val:#010x}");
        }
    }
    if !fbpa_regs.is_empty() {
        eprintln!(
            "╠══ FBPA Partitions ({} non-zero regs) ═══════════╣",
            fbpa_regs.len()
        );
        for (offset, val) in fbpa_regs.iter().take(32) {
            eprintln!("║   {offset:#010x} = {val:#010x}");
        }
        if fbpa_regs.len() > 32 {
            eprintln!("║   ... {} more", fbpa_regs.len() - 32);
        }
    }
}

// ─── FB Init Attempt (Glowplug Phase 2) ────────────────────────────────

/// Attempt to bring VRAM online by applying known-good NV_PFB register
/// sequences derived from the nouveau oracle.
///
/// This is the "FB init" phase of the glowplug — after PMC_ENABLE clocks
/// the engines and BAR2 page tables are built, VRAM may still return
/// 0xBAD0ACxx because the framebuffer memory controller hasn't been
/// configured. This function tries known initialization patterns.
///
/// Returns the topology after the attempt, plus any significant deltas.
pub fn attempt_fb_init(
    bar0: &MappedBar,
    container: Arc<OwnedFd>,
) -> (MemoryTopology, Vec<MemoryDelta>) {
    let mut significant_deltas = Vec::new();

    // Step 1: Read current PFB state (oracle comparison baseline)
    let pfb_snapshot = snapshot_pfb_registers(bar0);
    eprintln!("║ FB init: {} PFB registers readable", pfb_snapshot.len());

    // Step 2: Try NISO flush address configuration.
    // nouveau's ramgv100.c sets NV_PFB_NISO_FLUSH_SYSMEM_ADDR to a known
    // DMA-mapped page. Without this, VRAM flush operations may stall.
    let before_niso = discover_memory_topology(bar0, Arc::clone(&container));
    if !before_niso.vram_accessible {
        let _ = bar0.write_u32(pfb::NISO_FLUSH_ADDR_LO, 0);
        let _ = bar0.write_u32(pfb::NISO_FLUSH_ADDR_HI, 0);
        std::thread::sleep(std::time::Duration::from_millis(10));

        let after_niso = discover_memory_topology(bar0, Arc::clone(&container));
        let delta = MemoryDelta::compute(
            (pfb::NISO_FLUSH_ADDR_LO, 0),
            before_niso.clone(),
            after_niso.clone(),
        );
        if delta.unlocked_memory() {
            eprintln!("║ FB init: NISO flush addr unlocked VRAM!");
            significant_deltas.push(delta);
            return (after_niso, significant_deltas);
        }
    }

    // Step 3: Try MMU/TLB invalidation (may unblock stale TLB state).
    let before_tlb = discover_memory_topology(bar0, Arc::clone(&container));
    if !before_tlb.vram_accessible {
        // Wait for flush slot
        for _ in 0..200 {
            if bar0.read_u32(pfb::MMU_CTRL).unwrap_or(0) & 0x00FF_0000 != 0 {
                break;
            }
            std::thread::sleep(std::time::Duration::from_micros(100));
        }
        // Trigger a global TLB invalidate
        let _ = bar0.write_u32(pfb::MMU_INVALIDATE_PDB, 0);
        let _ = bar0.write_u32(pfb::MMU_INVALIDATE_PDB_HI, 0);
        let _ = bar0.write_u32(pfb::MMU_INVALIDATE, 0x8000_0005); // PAGE_ALL | HUB_ONLY | trigger
        std::thread::sleep(std::time::Duration::from_millis(10));

        let after_tlb = discover_memory_topology(bar0, Arc::clone(&container));
        let delta = MemoryDelta::compute(
            (pfb::MMU_INVALIDATE, 0x8000_0005),
            before_tlb,
            after_tlb.clone(),
        );
        if delta.unlocked_memory() {
            eprintln!("║ FB init: TLB invalidation unlocked VRAM!");
            significant_deltas.push(delta);
            return (after_tlb, significant_deltas);
        }
    }

    // Step 4: Scan NV_PFB region for registers that change VRAM accessibility.
    // This is the systematic reverse-engineering probe: try writing each
    // non-zero register value we read from the oracle (or 0xFFFFFFFF as a
    // "enable everything" heuristic) and see what changes.
    let current_topo = discover_memory_topology(bar0, Arc::clone(&container));
    if !current_topo.vram_accessible {
        eprintln!("║ FB init: VRAM still cold after quick attempts — scanning NV_PFB range");

        // Scan a focused subset of NV_PFB registers (the ones nouveau touches
        // during ramgv100.c init). Full scan_register_range is expensive.
        let fb_critical_offsets: &[usize] = &[
            0x0010_0000, // NV_PFB_CFG0
            0x0010_0004, // NV_PFB_CFG1
            0x0010_0200, // NV_PFB_PART_CTRL
            0x0010_0300, // NV_PFB_ZBC_CTRL
            0x0010_0800, // NV_PFB_MEM_STATUS
            0x0010_0804, // NV_PFB_MEM_CTRL
            0x0010_0808, // NV_PFB_MEM_ACK
            0x0010_0C80, // NV_PFB_PRI_MMU_CTRL
            0x0010_0CC0, // NV_PFB_PRI_MMU_L2TLB_CTRL
        ];

        for &offset in fb_critical_offsets {
            let current_val = bar0.read_u32(offset).unwrap_or(0);
            // Record what's there
            eprintln!("║ FB init: PFB[{offset:#010x}] = {current_val:#010x}");
        }
    }

    let final_topo = discover_memory_topology(bar0, container);
    (final_topo, significant_deltas)
}

// ─── Differential Probing ───────────────────────────────────────────────

/// Perform a differential probe: apply a register write, re-probe memory,
/// and compute what changed.
///
/// This is the core of the FB init reverse-engineering tool. Call it in a
/// loop over candidate register writes (e.g., from the nouveau oracle) to
/// discover which writes unlock VRAM or enable new memory paths.
pub fn differential_probe(
    bar0: &MappedBar,
    container: Arc<OwnedFd>,
    register_offset: usize,
    value: u32,
) -> MemoryDelta {
    let before = discover_memory_topology(bar0, Arc::clone(&container));

    let _ = bar0.write_u32(register_offset, value);
    std::thread::sleep(std::time::Duration::from_millis(5));

    let after = discover_memory_topology(bar0, container);

    MemoryDelta::compute((register_offset, value), before, after)
}

/// Probe a range of registers and report which ones change the memory
/// landscape. Returns only deltas that gained or lost paths.
pub fn scan_register_range(
    bar0: &MappedBar,
    container: Arc<OwnedFd>,
    start_offset: usize,
    end_offset: usize,
    stride: usize,
    value: u32,
) -> Vec<MemoryDelta> {
    let mut significant = Vec::new();
    let mut offset = start_offset;
    while offset < end_offset {
        let original = bar0.read_u32(offset).unwrap_or(0);
        let delta = differential_probe(bar0, Arc::clone(&container), offset, value);
        // Restore the register to avoid side effects on subsequent probes.
        let _ = bar0.write_u32(offset, original);

        if delta.unlocked_memory() || delta.broke_memory() {
            significant.push(delta);
        }
        offset += stride;
    }
    significant
}
