// SPDX-License-Identifier: AGPL-3.0-only
//! Hot-handoff test: inject a channel while nouveau is still running.
//!
//! Maps BAR0 directly via sysfs `resource0` (no VFIO) alongside nouveau,
//! verifies the firmware layer (PMU, FECS) is alive via `FalconProbe`,
//! then writes a channel + runlist into VRAM via PRAMIN and submits
//! it to the live scheduler.
//!
//! All structures (instance block, GPFIFO, USERD, runlist, page tables,
//! push buffer) reside in VRAM to avoid needing VFIO/IOMMU.
//!
//! Usage:
//!   sudo cargo run -p coral-driver --features vfio --example hot_handoff_nouveau -- 0000:03:00.0
//!   sudo cargo run -p coral-driver --features vfio --example hot_handoff_nouveau -- 0000:03:00.0 --nop
//!
//! Without --nop: proves channel injection (PCCSR ENABLE, no faults).
//! With --nop: submits a NOP GPU method through GPFIFO and verifies GP_GET advances.

use std::os::fd::AsFd;

use coral_driver::nv::vfio_compute::falcon_capability::FalconProbe;
use coral_driver::vfio::device::MappedBar;
use rustix::mm::{MapFlags, ProtFlags, mmap};

const BAR0_SIZE: usize = 16 * 1024 * 1024;

// VRAM layout: 0x80000..0x89FFF (40KB at 512KB offset).
// All within one 64KB PRAMIN window (window=0x8 → 0x80000..0x8FFFF).
const VRAM_BASE: u32 = 0x0008_0000;
const VRAM_INST: u32 = VRAM_BASE;             // 0x80000: instance block
const VRAM_RUNLIST: u32 = VRAM_BASE + 0x1000; // 0x81000: runlist
const VRAM_GPFIFO: u32 = VRAM_BASE + 0x2000;  // 0x82000: GPFIFO ring
const VRAM_USERD: u32 = VRAM_BASE + 0x3000;   // 0x83000: USERD page
const VRAM_PUSHBUF: u32 = VRAM_BASE + 0x4000; // 0x84000: NOP push buffer
const VRAM_PD3: u32 = VRAM_BASE + 0x5000;     // 0x85000: page dir level 3
const VRAM_PD2: u32 = VRAM_BASE + 0x6000;     // 0x86000: page dir level 2
const VRAM_PD1: u32 = VRAM_BASE + 0x7000;     // 0x87000: page dir level 1
const VRAM_PD0: u32 = VRAM_BASE + 0x8000;     // 0x88000: page dir level 0
const VRAM_SPT: u32 = VRAM_BASE + 0x9000;     // 0x89000: small page table

// Channel ID — high to avoid colliding with nouveau's channels (typically 0..~32).
const CHANNEL_ID: u32 = 500;
// Default runlist: CE (runlist 2) — doesn't need FECS/GPCCS to schedule.
// GR (runlist 1) requires FECS alive; host (runlist 0) is PBDMA-only.
const DEFAULT_RUNLIST_ID: u32 = 2;

// BAR0 register offsets.
const PRAMIN_BASE: usize = 0x0070_0000;
const BAR0_WINDOW: usize = 0x0000_1700;
const PCCSR_INST: usize = 0x0080_0000;
const PCCSR_CHAN: usize = 0x0080_0004;
const RL_BASE: usize = 0x2270;
const RL_SUBMIT: usize = 0x2274;
const DOORBELL: usize = 0x0081_0090;

// USERD offsets for GP_GET/GP_PUT (from ramuserd spec).
const USERD_GP_GET: usize = 34 * 4; // 0x88
const USERD_GP_PUT: usize = 35 * 4; // 0x8C

fn pccsr_inst(id: u32) -> usize { PCCSR_INST + (id as usize) * 8 }
fn pccsr_chan(id: u32) -> usize { PCCSR_CHAN + (id as usize) * 8 }
fn runlist_base(rl: u32) -> usize { RL_BASE + (rl as usize) * 0x10 }
fn runlist_submit(rl: u32) -> usize { RL_SUBMIT + (rl as usize) * 0x10 }

/// Encode a V2 PDE pointing to a sub-level page table in VRAM.
/// PDE format: `(phys_addr >> 4) | flags`. Aperture=VRAM(1), VOL=0.
fn encode_vram_pde(vram_addr: u32) -> u64 {
    ((vram_addr as u64) >> 4) | (1 << 1) // aperture=1 (VRAM)
}

/// Encode a V2 PD0 dual PDE with SPT_PRESENT for VRAM.
fn encode_vram_pd0_pde(vram_addr: u32) -> u64 {
    encode_vram_pde(vram_addr) | (1 << 4) // SPT_PRESENT
}

/// Encode a V2 small-page PTE for a VRAM physical address.
/// PTE format: `(phys_addr >> 4) | flags`. Aperture=VRAM(0), VOL=0, VALID=1.
fn encode_vram_pte(vram_addr: u64) -> u64 {
    (vram_addr >> 4) | 1 // VALID only; aperture=0 (VRAM), VOL=0
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let non_flag_args: Vec<&str> = args.iter().skip(1).filter(|a| !a.starts_with("--")).map(String::as_str).collect();
    let bdf = non_flag_args.first().copied().unwrap_or("0000:03:00.0");
    let nop_dispatch = args.iter().any(|a| a == "--nop");
    let runlist_id: u32 = args.iter().position(|a| a == "--rl")
        .and_then(|i| args.get(i + 1))
        .and_then(|s| s.parse().ok())
        .unwrap_or(DEFAULT_RUNLIST_ID);
    let resource0_path = format!("/sys/bus/pci/devices/{bdf}/resource0");

    eprintln!("═══ Hot Handoff: Nouveau Coexistence Test ═══");
    eprintln!("  BDF: {bdf}");
    eprintln!("  BAR0: {resource0_path}");
    eprintln!("  NOP dispatch: {nop_dispatch}");
    eprintln!("  Runlist: {runlist_id} (0=host, 1=GR, 2=CE)");

    // ── Phase 0: Map BAR0 from sysfs resource0 ──────────────────────────
    let fd = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .open(&resource0_path)
        .unwrap_or_else(|e| {
            eprintln!("ERROR: Cannot open {resource0_path}: {e}");
            eprintln!("  Is nouveau bound? Check: lspci -ks {bdf}");
            eprintln!("  Run as root? sudo is required for resource0 access.");
            std::process::exit(1);
        });

    let bar0 = unsafe {
        let ptr = mmap(
            std::ptr::null_mut(),
            BAR0_SIZE,
            ProtFlags::READ | ProtFlags::WRITE,
            MapFlags::SHARED,
            fd.as_fd(),
            0,
        ).unwrap_or_else(|e| {
            eprintln!("ERROR: mmap failed: {e}");
            std::process::exit(1);
        });
        MappedBar::from_raw(ptr.cast(), BAR0_SIZE)
    };
    std::mem::forget(fd); // Keep fd alive; MappedBar will munmap on drop.
    eprintln!("  BAR0 mapped: {BAR0_SIZE} bytes\n");

    let r = |reg: usize| bar0.read_u32(reg).unwrap_or(0xDEAD_DEAD);
    let w = |reg: usize, val: u32| {
        bar0.write_u32(reg, val)
            .unwrap_or_else(|e| eprintln!("  WARN: write {reg:#x}={val:#x} failed: {e}"))
    };

    // ── Phase 1: Firmware Boundary Probe ────────────────────────────────
    eprintln!("▶ Phase 1: Firmware Boundary Probe");
    let probe = FalconProbe::discover(&bar0);
    eprintln!("{probe}");

    if !probe.dispatch_viable() {
        eprintln!("\n✗ Dispatch NOT viable — firmware layer is dead.");
        for b in probe.dispatch_blockers() {
            eprintln!("  BLOCKED: {b}");
        }
        eprintln!("  Hot handoff requires a running firmware stack.");
        eprintln!("  Ensure nouveau is bound and loaded: lspci -ks {bdf}");
        std::process::exit(2);
    }
    eprintln!("\n✓ PMU alive — firmware layer is functional. Proceeding with channel injection.\n");

    // ── Phase 2: PFIFO / Scheduler state snapshot ───────────────────────
    eprintln!("▶ Phase 2: Pre-injection state snapshot");
    let pfifo_en = r(0x2200);
    let pfifo_intr = r(0x2100);
    let sched_en = r(0x2504);
    eprintln!("  PFIFO_ENABLE = {pfifo_en:#010x}");
    eprintln!("  PFIFO_INTR   = {pfifo_intr:#010x}");
    eprintln!("  SCHED_EN     = {sched_en:#010x}");

    // Check our channel slot is free.
    let pre_pccsr = r(pccsr_chan(CHANNEL_ID));
    eprintln!("  PCCSR[{CHANNEL_ID}]  = {pre_pccsr:#010x} (should be 0 or no ENABLE)");
    if pre_pccsr & 1 != 0 {
        eprintln!("  WARN: Channel {CHANNEL_ID} already enabled! Disabling first.");
        w(pccsr_chan(CHANNEL_ID), 1 << 11); // ENABLE_CLR
        std::thread::sleep(std::time::Duration::from_millis(10));
    }

    // ── Phase 3: Write channel structures to VRAM via PRAMIN ────────────
    eprintln!("\n▶ Phase 3: Writing channel structures to VRAM via PRAMIN");
    let vram_window = VRAM_BASE >> 16;
    eprintln!("  BAR0_WINDOW = {vram_window:#x} (VRAM offset {VRAM_BASE:#x})");
    w(BAR0_WINDOW, vram_window);
    std::thread::sleep(std::time::Duration::from_millis(1));

    // PRAMIN offset helper: converts absolute VRAM address to PRAMIN write offset.
    let pramin = |vram_addr: u32, field_off: usize| -> usize {
        PRAMIN_BASE + (vram_addr - (vram_window << 16)) as usize + field_off
    };

    // Zero the entire 40KB region (0x80000..0x89FFF).
    for off in (0..0xA000).step_by(4) {
        w(PRAMIN_BASE + off, 0);
    }

    let gpfifo_entries: u32 = 8;

    // 3a: V2 Page tables (PD3 → PD2 → PD1 → PD0 → SPT).
    // Identity-map first 2MB of GPU VA space to VRAM physical addresses.
    // This lets the PBDMA DMA-fetch our GPFIFO and push buffer from VRAM.
    eprintln!("  PAGE TABLES: PD3={VRAM_PD3:#x} PD2={VRAM_PD2:#x} PD1={VRAM_PD1:#x} PD0={VRAM_PD0:#x} SPT={VRAM_SPT:#x}");
    {
        // PD3[0] → PD2 (VRAM aperture)
        let pde3 = encode_vram_pde(VRAM_PD2);
        w(pramin(VRAM_PD3, 0), pde3 as u32);
        w(pramin(VRAM_PD3, 4), (pde3 >> 32) as u32);

        // PD2[0] → PD1
        let pde2 = encode_vram_pde(VRAM_PD1);
        w(pramin(VRAM_PD2, 0), pde2 as u32);
        w(pramin(VRAM_PD2, 4), (pde2 >> 32) as u32);

        // PD1[0] → PD0
        let pde1 = encode_vram_pde(VRAM_PD0);
        w(pramin(VRAM_PD1, 0), pde1 as u32);
        w(pramin(VRAM_PD1, 4), (pde1 >> 32) as u32);

        // PD0[0] → SPT (dual PDE format: small PDE at [0:7], large PDE at [8:15])
        let pd0_pde = encode_vram_pd0_pde(VRAM_SPT);
        w(pramin(VRAM_PD0, 0), pd0_pde as u32);
        w(pramin(VRAM_PD0, 4), (pd0_pde >> 32) as u32);

        // SPT: identity-map pages 1..511 (skip page 0 as null guard).
        // Each SPT entry maps one 4KB page: GPU VA i*4096 → VRAM i*4096.
        for i in 1..512u32 {
            let phys = (i as u64) * 4096;
            let pte = encode_vram_pte(phys);
            let off = (i as usize) * 8;
            w(pramin(VRAM_SPT, off), pte as u32);
            w(pramin(VRAM_SPT, off + 4), (pte >> 32) as u32);
        }

        // Verify PD3[0] readback.
        let rb_pde3 = r(pramin(VRAM_PD3, 0));
        eprintln!("  PD3[0] readback: {rb_pde3:#010x} (expect {:#010x})", pde3 as u32);
    }

    // 3b: NOP push buffer at VRAM_PUSHBUF.
    eprintln!("  PUSHBUF: VRAM {VRAM_PUSHBUF:#x} (NOP method)");
    {
        // NOP method header: SZ_INCR type, count=1, subchannel=0, method=0x40 (NOP).
        let nop_hdr: u32 = (1 << 29) | (1 << 16) | 0x40;
        w(pramin(VRAM_PUSHBUF, 0), nop_hdr);
        w(pramin(VRAM_PUSHBUF, 4), 0); // NOP data word
    }

    // 3c: GPFIFO ring at VRAM_GPFIFO.
    eprintln!("  GPFIFO: VRAM {VRAM_GPFIFO:#x}, {gpfifo_entries} entries");
    if nop_dispatch {
        // Write one GPFIFO entry pointing to our NOP push buffer.
        // GPU VA of push buffer = VRAM_PUSHBUF (identity mapped via our page tables).
        // GPFIFO entry: DW0=[31:2]=addr_lo, DW1=[20:10]=length_dwords.
        let pb_gpu_va = VRAM_PUSHBUF as u64;
        let gp_entry: u64 = (pb_gpu_va & 0xFFFF_FFFC) | (2u64 << 42); // length=2 dwords
        w(pramin(VRAM_GPFIFO, 0), gp_entry as u32);
        w(pramin(VRAM_GPFIFO, 4), (gp_entry >> 32) as u32);
        eprintln!("  GPFIFO[0] = {gp_entry:#018x} (NOP @ GPU VA {pb_gpu_va:#x}, 2 dwords)");
    }

    // 3d: USERD page at VRAM_USERD.
    eprintln!("  USERD:  VRAM {VRAM_USERD:#x}");
    if nop_dispatch {
        // Set GP_PUT=1 in USERD to tell PBDMA there's 1 entry to fetch.
        w(pramin(VRAM_USERD, USERD_GP_PUT), 1);
        w(pramin(VRAM_USERD, USERD_GP_GET), 0);
        eprintln!("  USERD GP_PUT=1, GP_GET=0 (1 pending GPFIFO entry)");
    }

    // 3e: Instance block (RAMFC + RAMIN PDB) at VRAM_INST.
    eprintln!("  INST:   VRAM {VRAM_INST:#x}");
    {
        let pm = |off: usize, val: u32| w(pramin(VRAM_INST, off), val);
        let limit2 = gpfifo_entries.ilog2();

        // RAMFC: USERD is a physical DMA address (target=VID_MEM=0).
        pm(0x008, VRAM_USERD & 0xFFFF_FE00); // USERD_LO (target=0=VID_MEM)
        pm(0x00C, 0);                          // USERD_HI
        pm(0x010, 0x0000_FACE);               // SIGNATURE
        pm(0x030, 0x7FFF_F902);               // ACQUIRE

        // GP_BASE is a GPU virtual address (translated via our page tables).
        let gpfifo_gpu_va = VRAM_GPFIFO as u64; // identity mapped
        pm(0x048, gpfifo_gpu_va as u32);       // GP_BASE_LO
        pm(0x04C, (gpfifo_gpu_va >> 32) as u32 | (limit2 << 16)); // GP_BASE_HI + limit
        if nop_dispatch {
            pm(0x054, 1);                      // GP_PUT = 1 (match USERD)
        }
        pm(0x058, 0);                          // GP_GET = 0

        pm(0x084, 0x2040_0000);               // PB_HEADER
        pm(0x094, 0x3000_0000 | 0xFFF);       // SUBDEVICE
        pm(0x0E4, 0x0000_0020);               // HCE_CTRL
        pm(0x0E8, CHANNEL_ID);                // CHID
        pm(0x0F4, 0x0000_0000);               // PBDMA target (VID_MEM)
        pm(0x0F8, 0x1000_3080);               // PBDMA format

        // RAMIN PDB: point to our real VRAM page tables.
        let pdb_lo = VRAM_PD3
            | (1 << 11)   // BIG_PAGE_SIZE = 64 KiB
            | (1 << 10);  // USE_VER2_PT_FORMAT (aperture=0=VID_MEM, VOL=0)
        pm(0x200, pdb_lo);                    // PAGE_DIR_BASE_LO
        pm(0x204, 0);                          // PAGE_DIR_BASE_HI
        pm(0x208, 0xFFFF_FFFF);               // ADDR_LIMIT_LO
        pm(0x20C, 0x0001_FFFF);               // ADDR_LIMIT_HI
        pm(0x218, 0);                          // ENGINE_WFI_VEID
        pm(0x298, 1);                          // SC_PDB_VALID
        pm(0x2A0, pdb_lo);                    // SC0_PAGE_DIR_BASE_LO
        pm(0x2A4, 0);                          // SC0_PAGE_DIR_BASE_HI
        pm(0x2B0, 1);                          // SC1_PAGE_DIR_BASE_LO (sentinel)
        pm(0x2B4, 1);                          // SC1_PAGE_DIR_BASE_HI (sentinel)
    }

    // Verify instance block readback.
    let rb_sig = r(pramin(VRAM_INST, 0x010));
    let rb_chid = r(pramin(VRAM_INST, 0x0E8));
    let rb_gpbase = r(pramin(VRAM_INST, 0x048));
    let rb_pdb = r(pramin(VRAM_INST, 0x200));
    eprintln!("  Instance readback: SIG={rb_sig:#010x} CHID={rb_chid} GPBASE={rb_gpbase:#010x} PDB={rb_pdb:#010x}");

    // 3f: Runlist (TSG header + channel entry).
    // GV100 RAMRL format from nouveau gv100_runl_insert_cgrp / gv100_runl_insert_chan.
    let tsg_id: u32 = 100; // Avoid nouveau's TSG IDs (typically 0..~32).
    eprintln!("  RUNLIST: VRAM {VRAM_RUNLIST:#x} (TSG {tsg_id}, CH {CHANNEL_ID})");
    {
        let pm = |off: usize, val: u32| w(pramin(VRAM_RUNLIST, off), val);

        // TSG header (16 bytes).
        // DW0: [31:26]=ch_count, [25:18]=tsg_id, [0]=ENTRY_TYPE(1=TSG)
        let tsg_dw0 = (1u32 << 26) | (tsg_id << 18) | 1;
        // DW1: timeslice in 64us units (128 = ~8ms, normal priority)
        pm(0x00, tsg_dw0);
        pm(0x04, 128); // timeslice
        pm(0x08, 0);
        pm(0x0C, 0);
        eprintln!("  TSG DW0={tsg_dw0:#010x} (ch_count=1, tsg_id={tsg_id}, type=TSG)");

        // Channel entry (16 bytes) — USERD in VRAM, INST in VRAM.
        // DW0: [31:12]=USERD_ADDR, [7:6]=TARGET(0=VRAM), [1]=RUNQ, [0]=TYPE(0=chan)
        let userd_dw0 = VRAM_USERD & 0xFFFF_F000;
        pm(0x10, userd_dw0);
        pm(0x14, 0);
        // DW2: [31:12]=INST_ADDR, [11:0]=CHID
        let inst_dw2 = (VRAM_INST & 0xFFFF_F000) | CHANNEL_ID;
        pm(0x18, inst_dw2);
        pm(0x1C, 0);
        eprintln!("  CHAN DW0={userd_dw0:#010x} DW2={inst_dw2:#010x}");
    }

    // Restore PRAMIN window.
    w(BAR0_WINDOW, 0);
    eprintln!("  Structures written. PRAMIN window restored.\n");

    // ── Phase 4: PCCSR Bind + Enable ────────────────────────────────────
    eprintln!("▶ Phase 4: PCCSR Channel Bind + Enable");

    // Clear any stale state on our channel slot.
    let stale = r(pccsr_chan(CHANNEL_ID));
    if stale != 0 {
        eprintln!("  Clearing stale PCCSR: {stale:#010x}");
        w(pccsr_chan(CHANNEL_ID), (1 << 22) | (1 << 23) | (1 << 11)); // fault reset + disable
        std::thread::sleep(std::time::Duration::from_millis(5));
    }

    // Bind: INST_PTR = VRAM_INST >> 12, INST_TARGET = 0 (VID_MEM), INST_BIND = 1.
    let pccsr_inst_val = (VRAM_INST >> 12) | (1u32 << 31); // INST_BIND_TRUE
    eprintln!("  PCCSR_INST[{CHANNEL_ID}] = {pccsr_inst_val:#010x}");
    w(pccsr_inst(CHANNEL_ID), pccsr_inst_val);
    std::thread::sleep(std::time::Duration::from_millis(5));

    // Enable channel.
    eprintln!("  PCCSR_CHAN[{CHANNEL_ID}] <= ENABLE_SET (bit 10)");
    w(pccsr_chan(CHANNEL_ID), 1 << 10); // CHANNEL_ENABLE_SET
    std::thread::sleep(std::time::Duration::from_millis(5));

    let pccsr_post = r(pccsr_chan(CHANNEL_ID));
    let enabled = pccsr_post & 1 != 0;
    let status = (pccsr_post >> 24) & 0xF;
    let pbdma_faulted = pccsr_post & (1 << 22) != 0;
    let eng_faulted = pccsr_post & (1 << 23) != 0;
    eprintln!("  PCCSR after enable: {pccsr_post:#010x}");
    eprintln!("    ENABLE={enabled} STATUS={status} PBDMA_FAULTED={pbdma_faulted} ENG_FAULTED={eng_faulted}");

    // ── Phase 5: Runlist Submit ─────────────────────────────────────────
    eprintln!("\n▶ Phase 5: Runlist Submit (runlist {runlist_id})");

    let rl_base_val = VRAM_RUNLIST >> 12; // target=0 (VID_MEM)
    let rl_submit_val = 2u32 << 16; // 2 entries (TSG + channel), upper_addr=0
    eprintln!("  RUNLIST_BASE[{runlist_id}]  <= {rl_base_val:#010x}");
    eprintln!("  RUNLIST_SUBMIT[{runlist_id}] <= {rl_submit_val:#010x}");
    w(runlist_base(runlist_id), rl_base_val);
    w(runlist_submit(runlist_id), rl_submit_val);

    // Readback verification — if PRI-gated, the write was silently dropped.
    let rb_rl_base = r(runlist_base(runlist_id));
    let rb_rl_submit = r(runlist_submit(runlist_id));
    eprintln!("  Readback: BASE={rb_rl_base:#010x} SUBMIT={rb_rl_submit:#010x}");
    if rb_rl_base == 0xBAD0_0200 || rb_rl_submit == 0xBAD0_0200 {
        eprintln!("  ⚠ Runlist registers are PRI-GATED — write was silently dropped!");
        eprintln!("  The PFIFO scheduler domain is inaccessible from host BAR0.");
        eprintln!("  Scheduler is managed by firmware (PMU/FECS), not direct host registers.");
    } else if rb_rl_base == 0 && rb_rl_submit == 0 {
        eprintln!("  ⚠ Runlist registers read back as 0 — may be write-only or PRI-gated.");
    }

    // Check additional PFIFO health registers.
    let pfifo_cfg = r(0x2004); // PBDMA_MAP
    let pfifo_rl0_info = r(0x2270); // RL0 info
    eprintln!("  PBDMA_MAP  = {pfifo_cfg:#010x}");
    eprintln!("  RL0_INFO   = {pfifo_rl0_info:#010x}");

    // Wait for scheduler to process the runlist.
    std::thread::sleep(std::time::Duration::from_millis(100));

    // ── Phase 5.5: Ring doorbell if NOP dispatch ────────────────────────
    if nop_dispatch {
        eprintln!("\n▶ Phase 5.5: Doorbell Ring");
        eprintln!("  Writing channel ID {CHANNEL_ID} to NOTIFY_CHANNEL_PENDING ({DOORBELL:#x})");
        w(DOORBELL, CHANNEL_ID);
        std::thread::sleep(std::time::Duration::from_millis(200));

        // Check if scheduler picked up our channel.
        let post_doorbell = r(pccsr_chan(CHANNEL_ID));
        let post_status = (post_doorbell >> 24) & 0xF;
        eprintln!("  PCCSR after doorbell: {post_doorbell:#010x} STATUS={post_status}");

        if post_status <= 1 {
            // Channel still PENDING — scheduler didn't load it. Try direct PBDMA.
            eprintln!("  Channel still PENDING after doorbell — trying direct PBDMA programming.");
            eprintln!("\n▶ Phase 5.6: Direct PBDMA Programming (bypass scheduler)");

            // Use PBDMA0 (0x40000). Even though PBDMA_MAP shows it unmapped,
            // direct register programming might still work.
            let pb: usize = 0x40000; // PBDMA0 base

            // Write channel context directly to PBDMA registers.
            let gpfifo_gpu_va = VRAM_GPFIFO as u64;
            let limit2 = gpfifo_entries.ilog2();

            w(pb + 0x0C0, 0x0000_FACE); // SIGNATURE
            w(pb + 0x040, gpfifo_gpu_va as u32); // GP_BASE_LO
            w(pb + 0x044, (gpfifo_gpu_va >> 32) as u32 | (limit2 << 16)); // GP_BASE_HI
            w(pb + 0x0D0, VRAM_USERD & 0xFFFF_FE00); // USERD_LO (target=0=VID_MEM)
            w(pb + 0x0D4, 0); // USERD_HI
            w(pb + 0x0A8, 0x0000_0000); // CONFIG (VRAM target)
            w(pb + 0x0AC, 0x1000_3080); // CHANNEL_INFO
            w(pb + 0x048, 0); // GP_FETCH = 0
            w(pb + 0x04C, 0); // GP_STATE = 0
            w(pb + 0x054, 1); // GP_PUT = 1

            // Re-enable with SCHED bit.
            w(pccsr_chan(CHANNEL_ID), (1 << 10) | 0x2); // ENABLE_SET + SCHED

            std::thread::sleep(std::time::Duration::from_millis(20));
            w(DOORBELL, CHANNEL_ID); // Ring doorbell again
            std::thread::sleep(std::time::Duration::from_millis(200));

            // Readback PBDMA0 state after direct programming.
            let pb_sig = r(pb + 0x0C0);
            let pb_gp_base = r(pb + 0x040);
            let pb_userd = r(pb + 0x0D0);
            let pb_gp_put = r(pb + 0x054);
            let pb_gp_fetch = r(pb + 0x048);
            let pb_idle = r(pb + 0x04);
            let pb_status = r(pb + 0x0B0);
            eprintln!("  PBDMA0 after direct write:");
            eprintln!("    SIG={pb_sig:#010x} GP_BASE={pb_gp_base:#010x} USERD={pb_userd:#010x}");
            eprintln!("    GP_PUT={pb_gp_put} GP_FETCH={pb_gp_fetch} IDLE={pb_idle:#010x} STATE={pb_status:#010x}");

            // Also check if PBDMA accepted the context by checking PCCSR again.
            let post_direct = r(pccsr_chan(CHANNEL_ID));
            let direct_status = (post_direct >> 24) & 0xF;
            eprintln!("  PCCSR after direct PBDMA: {post_direct:#010x} STATUS={direct_status}");
        }
    }

    // ── Phase 6: Post-submit diagnostic ─────────────────────────────────
    eprintln!("\n▶ Phase 6: Post-submit Diagnostic");

    let pccsr_final = r(pccsr_chan(CHANNEL_ID));
    let final_enabled = pccsr_final & 1 != 0;
    let final_status = (pccsr_final >> 24) & 0xF;
    let final_pbdma_faulted = pccsr_final & (1 << 22) != 0;
    let final_eng_faulted = pccsr_final & (1 << 23) != 0;
    let final_busy = pccsr_final & (1 << 28) != 0;

    eprintln!("  PCCSR[{CHANNEL_ID}] = {pccsr_final:#010x}");
    eprintln!("    ENABLE={final_enabled} STATUS={final_status} BUSY={final_busy}");
    eprintln!("    PBDMA_FAULTED={final_pbdma_faulted} ENG_FAULTED={final_eng_faulted}");

    let pfifo_intr_post = r(0x2100);
    eprintln!("  PFIFO_INTR = {pfifo_intr_post:#010x}");
    if pfifo_intr_post & (1 << 30) != 0 {
        eprintln!("    ✓ RL_COMPLETE bit set — runlist was processed!");
    }
    if pfifo_intr_post & (1 << 16) != 0 {
        let chsw_err = r(0x256C);
        eprintln!("    ✗ CHSW_ERROR: {chsw_err:#010x}");
    }

    // Check PBDMA state (PBDMA 1 is typical for GR runlist 1).
    for pid in [0_usize, 1, 2, 3] {
        let b = 0x40000 + pid * 0x2000;
        let idle = r(b + 0x04);
        let userd = r(b + 0xD0);
        let gp_base_lo = r(b + 0x40);
        let gp_base_hi = r(b + 0x44);
        let gp_get = r(b + 0x58);
        let gp_put = r(b + 0x54);
        let sig = r(b + 0xC0);
        eprintln!(
            "  PBDMA{pid}: IDLE={idle:#010x} USERD={userd:#010x} GP={gp_base_lo:#010x}:{gp_base_hi:#010x} GET/PUT={gp_get}/{gp_put} SIG={sig:#010x}"
        );
    }

    // ── Phase 6.5: Read USERD GP_GET/GP_PUT from VRAM ─────────────────
    if nop_dispatch {
        eprintln!("\n▶ Phase 6.5: GPFIFO Progress Check (USERD in VRAM)");
        // Re-open PRAMIN window to read USERD.
        w(BAR0_WINDOW, VRAM_BASE >> 16);
        std::thread::sleep(std::time::Duration::from_millis(1));

        let gp_get = r(pramin(VRAM_USERD, USERD_GP_GET));
        let gp_put = r(pramin(VRAM_USERD, USERD_GP_PUT));
        eprintln!("  USERD GP_GET={gp_get} GP_PUT={gp_put}");

        // Also read RAMFC GP_GET/GP_PUT from instance block.
        let ramfc_gp_get = r(pramin(VRAM_INST, 0x058));
        let ramfc_gp_put = r(pramin(VRAM_INST, 0x054));
        eprintln!("  RAMFC GP_GET={ramfc_gp_get} GP_PUT={ramfc_gp_put}");

        w(BAR0_WINDOW, 0);

        if gp_get >= 1 {
            eprintln!("  ★ GP_GET advanced to {gp_get} — GPU processed our GPFIFO entry!");
            eprintln!("  ★ NOP dispatch SUCCEEDED — end-to-end GPU command execution proven.");
        } else {
            eprintln!("  GP_GET still at {gp_get} — GPFIFO entry not yet consumed.");
            eprintln!("  Possible causes: PBDMA stalled, MMU fault on GPFIFO fetch, or channel not scheduled.");
        }
    }

    // Re-probe falcons after our injection.
    eprintln!("\n▶ Phase 7: Post-injection Falcon State");
    let probe_post = FalconProbe::discover(&bar0);
    eprintln!("{probe_post}");

    // ── Verdict ─────────────────────────────────────────────────────────
    eprintln!("\n═══ Verdict ═══");
    if final_enabled && !final_pbdma_faulted && !final_eng_faulted {
        eprintln!("✓ Channel {CHANNEL_ID} is ENABLED, no faults — hot handoff SUCCEEDED.");
        eprintln!("  The scheduler accepted our channel alongside nouveau.");
        if final_status >= 5 {
            eprintln!("  STATUS={final_status} — channel is ON_PBDMA (actively scheduled)!");
        }
        if nop_dispatch {
            // Re-read GP_GET for final verdict.
            w(BAR0_WINDOW, VRAM_BASE >> 16);
            std::thread::sleep(std::time::Duration::from_millis(1));
            let final_gp_get = r(pramin(VRAM_USERD, USERD_GP_GET));
            w(BAR0_WINDOW, 0);
            if final_gp_get >= 1 {
                eprintln!("✓ NOP DISPATCH: GP_GET={final_gp_get} — GPU executed our NOP command!");
            } else {
                eprintln!("? NOP DISPATCH: GP_GET={final_gp_get} — command may not have been processed yet.");
            }
        }
    } else if final_pbdma_faulted || final_eng_faulted {
        eprintln!("✗ Channel faulted — scheduler rejected our channel structures.");
        if nop_dispatch {
            eprintln!("  With --nop: likely an MMU fault during GPFIFO/pushbuf DMA fetch.");
            eprintln!("  Check if page tables correctly map GPU VA → VRAM physical.");
        }
    } else {
        eprintln!("? Channel state unclear: PCCSR={pccsr_final:#010x}");
        eprintln!("  May need runlist format adjustment or different channel ID.");
    }

    // Cleanup: disable our channel so we don't leave garbage in the scheduler.
    w(pccsr_chan(CHANNEL_ID), (1 << 11) | (1 << 22) | (1 << 23));
    eprintln!("  Channel {CHANNEL_ID} disabled (cleanup).");
}
