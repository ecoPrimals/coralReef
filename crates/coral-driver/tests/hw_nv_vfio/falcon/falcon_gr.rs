// SPDX-License-Identifier: AGPL-3.0-only

use crate::ember_client;
use crate::helpers::{init_tracing, open_vfio, vfio_bdf};
use coral_driver::{ComputeDevice, DispatchDims, ShaderInfo};

/// GR context lifecycle test — requires FECS to be running (warm handoff from nouveau).
///
/// Exercises the full chain: discover sizes → allocate → bind → golden save.
/// Swap to nouveau first to get FECS warm, then swap to VFIO and test.
#[test]
#[ignore = "requires VFIO-bound GPU hardware with warm FECS"]
fn vfio_gr_context_lifecycle() {
    use coral_driver::nv::vfio_compute::gr_context;

    init_tracing();
    let mut dev = open_vfio();
    let bar0 = dev.bar0_ref();

    eprintln!("\n=== GR Context Lifecycle Test ===\n");

    let alive = gr_context::fecs_is_alive(bar0);
    eprintln!("FECS alive: {alive}");

    if !alive {
        eprintln!("FECS not running — need warm handoff from nouveau first.");
        eprintln!("Run: coralctl swap <bdf> nouveau && coralctl swap <bdf> vfio");
        eprintln!("=== End GR Context Lifecycle (skipped) ===");
        return;
    }

    let status = dev.probe_gr_context();
    eprintln!("{status}");

    eprintln!("\n=== End GR Context Lifecycle ===");
}

/// Exp 092: Full sovereign GR falcon boot — no SEC2/ACR, no nouveau.
///
/// Loads fresh GPCCS+FECS firmware directly via IMEM PIO, applies
/// the complete init sequence (sw_nonctx + INTR_EN_SET + watchdog),
/// and waits for FECS to signal ready via CTXSW_MAILBOX.
///
/// Key fixes over earlier attempts:
/// - Boot GPCCS first (FECS depends on GPCCS)
/// - Set INTR_EN_SET at falcon_base + 0x00C (not 0x010 which is INTR_EN_CLR)
/// - Set watchdog timer (0x7FFFFFFF)
/// - Apply GR BAR0 init (sw_nonctx + dynamic) before starting falcons
#[test]
#[ignore = "requires VFIO-bound GPU hardware"]
fn vfio_sovereign_gr_boot() {
    use coral_driver::nv::vfio_compute::fecs_boot;

    init_tracing();
    let dev = open_vfio();
    let bar0 = dev.bar0_ref();
    let chip = "gv100";

    eprintln!("\n=== Exp 092: Sovereign GR Boot (complete sequence) ===\n");

    let fecs_base: usize = 0x409000;
    let gpccs_base: usize = 0x41a000;
    let r = |addr: usize| bar0.read_u32(addr).unwrap_or(0xDEAD_DEAD);
    let w = |addr: usize, val: u32| {
        let _ = bar0.write_u32(addr, val);
    };

    // Step 0: Pre-boot diagnostics
    eprintln!("── Step 0: Pre-boot state ──");
    let fecs_cpuctl_pre = r(fecs_base + 0x100);
    let gpccs_cpuctl_pre = r(gpccs_base + 0x100);
    let fecs_sctl = r(fecs_base + 0x240);
    let gpccs_sctl = r(gpccs_base + 0x240);
    eprintln!("FECS:  cpuctl={fecs_cpuctl_pre:#010x} sctl={fecs_sctl:#010x}");
    eprintln!("GPCCS: cpuctl={gpccs_cpuctl_pre:#010x} sctl={gpccs_sctl:#010x}");

    // SCTL is fuse-enforced on GV100 (always LS=0x3000). This is informational —
    // PIO works regardless with correct IMEMC format (BIT(24) write, BIT(25) read).
    // FLR is not supported on Titan V (FLReset-).
    if fecs_sctl != 0 || gpccs_sctl != 0 {
        eprintln!("SCTL non-zero (LS mode) — this is normal for GV100. PIO works regardless.");
    }

    // Step 1: Clock gating restore + halt falcons
    // PMC_UNK260=1 is critical — matches nouveau's nvkm_mc_unk260(device, 1)
    // before FECS/GPCCS start. Without it, falcon clocks may be gated.
    eprintln!("\n── Step 1: PMC clock gate + halt falcons ──");
    let pmc_unk260_before = r(0x260);
    w(0x260, 1); // PMC_UNK260 = 1 (restore clock gating)
    let pmc_unk260_after = r(0x260);
    eprintln!("PMC_UNK260: {pmc_unk260_before:#010x} → {pmc_unk260_after:#010x}");

    for (name, base) in [("FECS", fecs_base), ("GPCCS", gpccs_base)] {
        let cpuctl = r(base + 0x100);
        let exci_before = r(base + 0x148);
        let pc_before = r(base + 0x030);
        if cpuctl & 0x10 == 0 && cpuctl != 0xDEAD_DEAD {
            w(base + 0x100, 0x10); // set HRESET
            std::thread::sleep(std::time::Duration::from_millis(5));
            let after = r(base + 0x100);
            eprintln!(
                "{name}: halted (cpuctl {cpuctl:#010x} → {after:#010x}) pre-exci={exci_before:#010x} pre-pc={pc_before:#06x}"
            );
        } else {
            eprintln!(
                "{name}: already halted cpuctl={cpuctl:#010x} pre-exci={exci_before:#010x} pre-pc={pc_before:#06x}"
            );
        }
    }

    // Step 2: Load + upload firmware with secure page tags
    // In LS mode (SCTL=0x3000), the falcon may validate IMEM page tags on
    // execution. Upload with secure=true to mark pages as authenticated.
    eprintln!("\n── Step 2: Load fresh firmware (secure tags) ──");
    let gpccs_fw = fecs_boot::GpccsFirmware::load(chip).expect("GPCCS firmware");
    let fecs_fw = fecs_boot::FecsFirmware::load(chip).expect("FECS firmware");
    eprintln!(
        "GPCCS: bl={}B bl_imem_off={:#06x} inst={}B data={}B",
        gpccs_fw.bootloader.len(),
        gpccs_fw.bl_imem_off,
        gpccs_fw.inst.len(),
        gpccs_fw.data.len()
    );
    eprintln!(
        "FECS:  bl={}B bl_imem_off={:#06x} inst={}B data={}B",
        fecs_fw.bootloader.len(),
        fecs_fw.bl_imem_off,
        fecs_fw.inst.len(),
        fecs_fw.data.len()
    );

    // Upload with secure=true (IMEM page tags marked as authenticated)
    fecs_boot::falcon_upload_imem(bar0, gpccs_base, 0, &gpccs_fw.inst, true);
    fecs_boot::falcon_upload_imem(
        bar0,
        gpccs_base,
        gpccs_fw.bl_imem_off,
        &gpccs_fw.bootloader,
        true,
    );
    fecs_boot::falcon_upload_dmem(bar0, gpccs_base, 0, &gpccs_fw.data);
    eprintln!(
        "GPCCS firmware uploaded (inst@0x0000 bl@{:#06x}) secure=true",
        gpccs_fw.bl_imem_off
    );

    fecs_boot::falcon_upload_imem(bar0, fecs_base, 0, &fecs_fw.inst, true);
    fecs_boot::falcon_upload_imem(
        bar0,
        fecs_base,
        fecs_fw.bl_imem_off,
        &fecs_fw.bootloader,
        true,
    );
    fecs_boot::falcon_upload_dmem(bar0, fecs_base, 0, &fecs_fw.data);
    eprintln!(
        "FECS firmware uploaded (inst@0x0000 bl@{:#06x}) secure=true",
        fecs_fw.bl_imem_off
    );

    // IMEM verify: read back first 4 words at 0x0000 and bl_imem_off
    eprintln!("\n── Step 2b: IMEM verify ──");
    for (name, base, bv) in [
        ("GPCCS", gpccs_base, gpccs_fw.bl_imem_off),
        ("FECS", fecs_base, fecs_fw.bl_imem_off),
    ] {
        w(base + 0x180, 0x0200_0000); // IMEMC read @ 0
        let w0 = r(base + 0x184);
        let w1 = r(base + 0x184);
        w(base + 0x180, 0x0200_0000 | bv); // IMEMC read @ bl_imem_off
        let b0 = r(base + 0x184);
        let b1 = r(base + 0x184);
        eprintln!(
            "{name}: IMEM[0x0000]={w0:#010x},{w1:#010x} IMEM[{bv:#06x}]={b0:#010x},{b1:#010x}"
        );
    }

    // Step 3: Configure falcon environment
    eprintln!("\n── Step 3: Configure falcon environment ──");
    // GR engine pre-init (nouveau gf100_gr_init_fecs_exceptions + SCC)
    w(fecs_base + 0xC24, 0x000e_0002); // FECS EXCEPTION_REG
    w(fecs_base + 0x802C, 0x0000_0001); // GR_CLASS_CFG
    eprintln!("GR pre-init: EXCEPTION_REG=0x000e0002 GR_CLASS_CFG=0x00000001");

    for (name, base, bv) in [
        ("GPCCS", gpccs_base, gpccs_fw.bl_imem_off),
        ("FECS", fecs_base, fecs_fw.bl_imem_off),
    ] {
        w(base + 0x00C, 0xfc24); // IRQMODE
        w(base + 0x048, 0x04); // ITFEN
        w(base + 0x10C, 0); // DMACTL = 0 (clear DMA state)
        w(base + 0x034, 0x7fff_ffff); // WATCHDOG
        w(base + 0x040, 0); // MAILBOX0 = 0
        w(base + 0x044, 0); // MAILBOX1 = 0
        w(base + 0x104, bv); // BOOTVEC = bl_imem_off
        w(base + 0x100, 0x01); // CPUCTL_IINVAL
        std::thread::sleep(std::time::Duration::from_millis(1));
        let exci_pre = r(base + 0x148);
        let bootvec_rb = r(base + 0x104);
        eprintln!(
            "{name}: BOOTVEC={bootvec_rb:#06x} IRQMODE=0xfc24 ITFEN=0x04 pre-start-exci={exci_pre:#010x}"
        );
    }

    w(0x409800, 0); // FECS_CTXSW_MAILBOX
    w(fecs_base + 0x800, 0); // FECS MTHD_STATUS
    w(gpccs_base + 0x10C, 0); // GPCCS DMACTL

    // Step 4: Start GPCCS first, then FECS
    eprintln!("\n── Step 4: STARTCPU ──");
    w(gpccs_base + 0x100, 0x02); // STARTCPU
    std::thread::sleep(std::time::Duration::from_millis(20));
    let gpccs_cpu_post = r(gpccs_base + 0x100);
    let gpccs_exci_post = r(gpccs_base + 0x148);
    let gpccs_pc_post = r(gpccs_base + 0x030);
    eprintln!(
        "GPCCS: cpuctl={gpccs_cpu_post:#010x} exci={gpccs_exci_post:#010x} pc={gpccs_pc_post:#06x}"
    );

    // Sample GPCCS PC over 100ms to check if it's executing
    let mut gpccs_pcs = Vec::new();
    for _ in 0..10 {
        std::thread::sleep(std::time::Duration::from_millis(10));
        gpccs_pcs.push(r(gpccs_base + 0x030));
    }
    eprintln!("GPCCS PC trace (100ms): {gpccs_pcs:02x?}");

    w(fecs_base + 0x100, 0x02); // STARTCPU
    std::thread::sleep(std::time::Duration::from_millis(20));
    let fecs_cpu_post = r(fecs_base + 0x100);
    let fecs_exci_post = r(fecs_base + 0x148);
    let fecs_pc_post = r(fecs_base + 0x030);
    eprintln!(
        "FECS:  cpuctl={fecs_cpu_post:#010x} exci={fecs_exci_post:#010x} pc={fecs_pc_post:#06x}"
    );

    // Step 5: Poll for FECS ready (CTXSW_MAILBOX bit 0)
    eprintln!("\n── Step 5: Wait for FECS ready ──");
    let start = std::time::Instant::now();
    let mut ready = false;
    let mut last_pc = 0u32;
    let mut last_log = std::time::Instant::now();

    while start.elapsed() < std::time::Duration::from_secs(30) {
        std::thread::sleep(std::time::Duration::from_millis(100));

        let mbox = r(0x409800);
        let fecs_cpu = r(fecs_base + 0x100);
        let fecs_pc = r(fecs_base + 0x030);
        let gpccs_cpu = r(gpccs_base + 0x100);
        let gpccs_exci = r(gpccs_base + 0x148);

        if mbox & 1 != 0 {
            eprintln!(
                "*** FECS READY! mbox={mbox:#010x} fecs_cpu={fecs_cpu:#010x} ({}ms) ***",
                start.elapsed().as_millis()
            );
            ready = true;
            break;
        }

        // Log every 2 seconds or on PC change
        if fecs_pc != last_pc || last_log.elapsed() > std::time::Duration::from_secs(2) {
            eprintln!(
                "[{:>5}ms] FECS: cpu={fecs_cpu:#010x} pc={fecs_pc:#06x} | GPCCS: cpu={gpccs_cpu:#010x} exci={gpccs_exci:#010x} | mbox={mbox:#010x}",
                start.elapsed().as_millis()
            );
            last_pc = fecs_pc;
            last_log = std::time::Instant::now();
        }

        // Abort if FECS halted unexpectedly
        if fecs_cpu & 0x30 != 0 {
            eprintln!(
                "FECS halted/reset: cpuctl={fecs_cpu:#010x} ({}ms)",
                start.elapsed().as_millis()
            );
            break;
        }
    }

    if !ready {
        eprintln!("FECS did not signal ready within 30s");
    }

    // Step 6: Try FECS method probe if ready
    if ready {
        eprintln!("\n── Step 6: FECS method probe ──");
        use coral_driver::nv::vfio_compute::acr_boot::fecs_method;
        fecs_method::fecs_init_exceptions(bar0);
        let mprobe = fecs_method::fecs_probe_methods(bar0);
        eprintln!("{mprobe}");

        if mprobe.ctx_size.is_ok() {
            eprintln!("\n****************************************************");
            eprintln!("*  SOVEREIGN GR BOOT SUCCEEDED!                    *");
            eprintln!("*  FECS responding to methods — Layer 11 unblocked *");
            eprintln!("****************************************************");
        }
    }

    // Final state dump
    eprintln!("\n── Final state ──");
    let probe = dev.falcon_probe();
    eprintln!("{probe}");

    eprintln!("\n=== End Exp 092 ===");
}
