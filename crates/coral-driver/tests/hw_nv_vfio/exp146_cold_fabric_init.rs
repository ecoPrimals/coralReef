// SPDX-License-Identifier: AGPL-3.0-only
//! Exp 146: Cold GPU Fabric Probe & Recovery — MMIO Gateway Architecture
//!
//! ALL GPU register operations route through ember RPCs.
//! No VFIO fd sharing, no local BAR0 mapping, no direct hardware access.
//!
//! Discovery: Enabling memory subsystem engines without trained DRAM
//! corrupts the PRI ring. BIOS POST doesn't train DRAM on secondary GPUs.
//!
//! This experiment:
//! A. Probes GPU state to determine if VRAM is accessible
//! B. If corrupted: restores PMC_ENABLE to safe cold value
//! C. Attempts SEC2 recovery via PMC bit cycle
//! D. Probes PRAMIN + ITFEN to assess fabric state
//!
//! ```text
//! Run:
//! CORALREEF_VFIO_BDF=0000:4c:00.0 RUST_LOG=info cargo test -p coral-driver \
//!   --features vfio --test hw_nv_vfio exp146 -- --ignored --nocapture --test-threads=1
//! ```

use crate::helpers::init_tracing;
use std::io::Write;

const TRACE_PATH: &str = "/var/lib/coralreef/traces/exp146_trace.log";

fn trace(msg: &str) {
    let ts = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis();
    let line = format!("[{ts}] {msg}\n");
    eprintln!("  TRACE: {msg}");
    if let Ok(mut f) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(TRACE_PATH)
    {
        let _ = f.write_all(line.as_bytes());
        let _ = f.sync_all();
    }
}

mod r146 {
    pub const PMC_ENABLE: usize = 0x000200;
    pub const SEC2_BASE: usize = 0x087000;
    pub const CPUCTL: usize = 0x100;
    pub const SCTL: usize = 0x240;
    pub const PC: usize = 0x030;
    pub const EXCI: usize = 0x148;
    pub const ITFEN: usize = 0x048;
    pub const BAR0_WINDOW: usize = 0x001700;
    pub const PRAMIN_BASE: usize = 0x700000;
}

fn ember_read(bdf: &str, offset: usize, label: &str) -> u32 {
    trace(&format!("MMIO READ {label} @ {offset:#x} ..."));
    match crate::ember_client::mmio_read(bdf, offset) {
        Ok(val) => {
            let pri_err = val & 0xFFF0_0000 == 0xBAD0_0000 || val & 0xFFF0_0000 == 0xBADF_0000;
            if pri_err {
                trace(&format!("MMIO READ {label} @ {offset:#x} = {val:#010x} [PRI FAULT]"));
            } else {
                trace(&format!("MMIO READ {label} @ {offset:#x} = {val:#010x}"));
            }
            val
        }
        Err(e) => {
            trace(&format!("MMIO READ {label} @ {offset:#x} FAILED: {e}"));
            eprintln!("  WARNING: ember.mmio.read failed: {e}");
            0xDEAD_DEAD
        }
    }
}

fn ember_write(bdf: &str, offset: usize, value: u32, label: &str) {
    trace(&format!("MMIO WRITE {label} @ {offset:#x} = {value:#010x}"));
    if let Err(e) = crate::ember_client::mmio_write(bdf, offset, value) {
        trace(&format!("MMIO WRITE {label} FAILED: {e}"));
        eprintln!("  WARNING: ember.mmio.write failed: {e}");
    }
}

fn is_pri_fault(val: u32) -> bool {
    val & 0xFFF0_0000 == 0xBAD0_0000 || val & 0xFFF0_0000 == 0xBADF_0000
}

fn falcon_state_via_ember(bdf: &str, name: &str, base: usize) {
    let ops = vec![
        ("r", base + r146::CPUCTL, 0u32),
        ("r", base + r146::SCTL, 0),
        ("r", base + r146::PC, 0),
        ("r", base + r146::EXCI, 0),
    ];
    match crate::ember_client::mmio_batch(bdf, &ops) {
        Ok(results) => {
            let cpuctl = results.first().and_then(|v| v.as_u64()).unwrap_or(0xDEAD) as u32;
            let sctl = results.get(1).and_then(|v| v.as_u64()).unwrap_or(0xDEAD) as u32;
            let pc = results.get(2).and_then(|v| v.as_u64()).unwrap_or(0xDEAD) as u32;
            let exci = results.get(3).and_then(|v| v.as_u64()).unwrap_or(0xDEAD) as u32;
            let pri_err = is_pri_fault(cpuctl);
            eprintln!(
                "  {name:6}: cpuctl={cpuctl:#010x} sctl={sctl:#010x} pc={pc:#06x} exci={exci:#010x}{}",
                if pri_err { " [PRI FAULT]" } else { "" }
            );
        }
        Err(e) => {
            eprintln!("  {name:6}: ember.mmio.batch failed: {e}");
        }
    }
}

#[test]
#[ignore = "requires VFIO-bound GPU hardware + Ember"]
fn exp146_recover_and_probe() {
    init_tracing();
    let eq = "=".repeat(70);
    let bdf = crate::helpers::vfio_bdf();

    eprintln!("{eq}");
    eprintln!("#  Exp 146: Cold GPU Recovery & Fabric Probe (MMIO Gateway)");
    eprintln!("#  ALL operations route through ember. No direct BAR0 access.");
    eprintln!("{eq}");

    let _ = std::fs::remove_file(TRACE_PATH);
    trace("exp146 STARTED (MMIO gateway mode)");

    // Experiment lifecycle
    struct ExperimentGuard { bdf: String }
    impl Drop for ExperimentGuard {
        fn drop(&mut self) {
            trace("LIFECYCLE: experiment_end via glowplug (guard drop)");
            if let Ok(mut c) = crate::glowplug_client::GlowPlugClient::connect() {
                let _ = c.experiment_end(&self.bdf);
            }
        }
    }
    let _exp_guard = ExperimentGuard { bdf: bdf.clone() };

    trace("LIFECYCLE: experiment_start via glowplug");
    if let Ok(mut gp) = crate::glowplug_client::GlowPlugClient::connect() {
        match gp.experiment_start(&bdf, "exp146_cold_fabric", 120) {
            Ok(_) => eprintln!("  glowplug: health probes paused (120s watchdog)"),
            Err(e) => eprintln!("  WARNING: glowplug experiment_start failed: {e}"),
        }
    }

    // ── Phase A: Probe GPU state ──
    trace("PHASE_A: probing GPU state via ember MMIO gateway");
    eprintln!("\n  PHASE A: GPU State (via ember MMIO gateway)");

    let boot0 = ember_read(&bdf, 0, "BOOT0");
    let pmc_now = ember_read(&bdf, r146::PMC_ENABLE, "PMC_ENABLE");
    eprintln!("  BOOT0={boot0:#010x}  PMC_ENABLE={pmc_now:#010x}");

    if boot0 == 0xDEAD_DEAD || boot0 == 0xFFFF_FFFF || boot0 == 0 {
        trace("PHASE_A: GPU unreachable — aborting");
        eprintln!("  *** GPU unreachable (BOOT0={boot0:#010x}). Check PCIe link. ***");
        return;
    }

    let sec2_cpuctl = ember_read(&bdf, r146::SEC2_BASE + r146::CPUCTL, "SEC2_CPUCTL");
    let sec2_faulted = is_pri_fault(sec2_cpuctl);
    eprintln!(
        "  SEC2: cpuctl={sec2_cpuctl:#010x} (PRI fault={sec2_faulted})"
    );

    let warm = pmc_now > 0x1000_0000;
    eprintln!(
        "  GPU fabric: {}",
        if warm { "WARM (engines alive)" } else { "COLD (minimal PMC)" }
    );
    trace("PHASE_A: complete");

    // ── Phase B: PMC Recovery (if needed) ──
    eprintln!("\n  PHASE B: PMC Recovery");

    let cold_pmc = 0x4000_0121_u32;
    if pmc_now != cold_pmc && sec2_faulted {
        trace("PHASE_B: restoring PMC_ENABLE to cold value");
        eprintln!("  PMC corrupt + SEC2 faulted — restoring to {cold_pmc:#010x}");
        ember_write(&bdf, r146::PMC_ENABLE, cold_pmc, "PMC_RESTORE");
        std::thread::sleep(std::time::Duration::from_millis(50));

        let pmc_after = ember_read(&bdf, r146::PMC_ENABLE, "PMC_AFTER_RESTORE");
        eprintln!("  PMC_ENABLE after restore: {pmc_after:#010x}");
        trace("PHASE_B: PMC restored");
    } else {
        eprintln!("  PMC appears healthy — no recovery needed");
        trace("PHASE_B: no recovery needed");
    }

    // ── Phase C: SEC2 Recovery Check ──
    eprintln!("\n  PHASE C: SEC2 Recovery Check");

    falcon_state_via_ember(&bdf, "SEC2", r146::SEC2_BASE);

    let sec2_cpuctl2 = ember_read(&bdf, r146::SEC2_BASE + r146::CPUCTL, "SEC2_CPUCTL_POST");
    let sec2_alive = !is_pri_fault(sec2_cpuctl2);
    eprintln!(
        "  SEC2 status: {}",
        if sec2_alive { "ALIVE" } else { "DEAD (PRI faults)" }
    );

    if !sec2_alive {
        trace("PHASE_C: SEC2 dead, attempting PMC bit cycle");
        eprintln!("  Attempting SEC2 PMC bit cycle...");
        let p = ember_read(&bdf, r146::PMC_ENABLE, "PMC_FOR_CYCLE");
        let mask = 1u32 << 5;

        ember_write(&bdf, r146::PMC_ENABLE, p & !mask, "SEC2_DISABLE");
        std::thread::sleep(std::time::Duration::from_millis(20));

        ember_write(&bdf, r146::PMC_ENABLE, p | mask, "SEC2_REENABLE");
        std::thread::sleep(std::time::Duration::from_millis(50));

        let sec2_cpuctl3 = ember_read(&bdf, r146::SEC2_BASE + r146::CPUCTL, "SEC2_AFTER_CYCLE");
        let recovered = !is_pri_fault(sec2_cpuctl3);
        eprintln!(
            "  SEC2 after PMC cycle: cpuctl={sec2_cpuctl3:#010x} recovered={recovered}"
        );

        if !recovered {
            trace("PHASE_C: SEC2 unrecoverable — attempting bare minimum PMC");
            eprintln!("  Attempting bare minimum PMC (disable all extras)...");
            ember_write(&bdf, r146::PMC_ENABLE, 0x4000_0001, "PMC_BARE_MIN");
            std::thread::sleep(std::time::Duration::from_millis(100));

            let p2 = ember_read(&bdf, r146::PMC_ENABLE, "PMC_BARE_MIN_READ");
            ember_write(&bdf, r146::PMC_ENABLE, p2 | (1 << 5) | (1 << 8), "PMC_SEC2_PFIFO");
            std::thread::sleep(std::time::Duration::from_millis(50));

            let sec2_cpuctl4 = ember_read(&bdf, r146::SEC2_BASE + r146::CPUCTL, "SEC2_FINAL_TRY");
            eprintln!("  After minimal PMC: SEC2={sec2_cpuctl4:#010x}");
            // NOTE: PRI_RING_INTR (0x120058) deliberately NOT read — it's poisonous
            eprintln!("  PRI_RING_INTR: SKIPPED (0x120058 is poisonous post-nouveau)");
        }
    }
    trace("PHASE_C: complete");

    // ── Phase D: PRAMIN Test ──
    eprintln!("\n  PHASE D: PRAMIN Test");
    trace("PHASE_D: PRAMIN probe via ember");

    // Set BAR0 window to VRAM 0x10000, then read via PRAMIN
    ember_write(&bdf, r146::BAR0_WINDOW, 0x00000001, "BAR0_WINDOW_SET");
    let pramin_val = ember_read(&bdf, r146::PRAMIN_BASE, "PRAMIN_0x10000");
    let pramin_alive = !is_pri_fault(pramin_val) && pramin_val != 0xDEAD_DEAD;
    eprintln!(
        "  PRAMIN@0x10000: {pramin_val:#010x} (PRI fault={}, alive={pramin_alive})",
        is_pri_fault(pramin_val)
    );

    // ── Phase E: ITFEN probe ──
    let sec2_alive_final = !is_pri_fault(
        ember_read(&bdf, r146::SEC2_BASE + r146::CPUCTL, "SEC2_FINAL_CHECK"),
    );
    if sec2_alive_final {
        eprintln!("\n  PHASE E: ITFEN Register Probe");
        let base = r146::SEC2_BASE;
        let itfen = ember_read(&bdf, base + r146::ITFEN, "ITFEN_READ");
        eprintln!("  ITFEN current: {itfen:#010x}");

        ember_write(&bdf, base + r146::ITFEN, (itfen & !0x30) | 0x30, "ITFEN_SET_BITS");
        let itfen2 = ember_read(&bdf, base + r146::ITFEN, "ITFEN_READBACK");
        eprintln!("  ITFEN after set [5:4]: {itfen2:#010x}");

        ember_write(&bdf, base + r146::ITFEN, itfen, "ITFEN_RESTORE");
    }

    // ── Summary ──
    let sec2_final = ember_read(&bdf, r146::SEC2_BASE + r146::CPUCTL, "SEC2_SUMMARY");
    let pmc_final = ember_read(&bdf, r146::PMC_ENABLE, "PMC_SUMMARY");

    trace("exp146 COMPLETE (MMIO gateway mode)");
    eprintln!("\n{eq}");
    eprintln!(
        "#  SEC2={} PRAMIN={} PMC={pmc_final:#010x}",
        if !is_pri_fault(sec2_final) { "ALIVE" } else { "DEAD" },
        if pramin_alive { "ALIVE" } else { "DEAD" },
    );
    if is_pri_fault(sec2_final) {
        eprintln!("#  GPU fabric corrupted. Reboot required.");
    } else if !pramin_alive {
        eprintln!("#  SEC2 alive but VRAM dead. System memory ACR boot viable.");
    }
    eprintln!("{eq}");
}
