// SPDX-License-Identifier: AGPL-3.0-only
//! Exp 123-T0: Parasitic probe — read GPU falcon state via sysfs BAR0 while nouveau is active.
//!
//! NO vfio-pci required. Works with nouveau bound.
//!
//! Run: `sudo cargo test --test exp123t_parasitic_probe -p coral-driver -- --ignored --nocapture`

use coral_driver::gsp::RegisterAccess;
use coral_driver::nv::bar0::Bar0Access;
use coral_driver::nv::identity;

/// Falcon engine descriptor for probing.
struct FalconDesc {
    name: &'static str,
    base: u32,
}

const FALCONS: &[FalconDesc] = &[
    FalconDesc { name: "FECS",  base: 0x0040_9000 },
    FalconDesc { name: "GPCCS", base: 0x0041_A000 },
    FalconDesc { name: "PMU",   base: 0x0010_A000 },
    FalconDesc { name: "SEC2",  base: 0x0008_7000 },
];

fn read_reg(bar0: &Bar0Access, addr: u32) -> u32 {
    bar0.read_u32(addr).unwrap_or(0xDEAD_DEAD)
}

fn probe_falcon(bar0: &Bar0Access, f: &FalconDesc) {
    let cpuctl = read_reg(bar0, f.base + 0x100);
    let sctl   = read_reg(bar0, f.base + 0x240);
    let exci   = read_reg(bar0, f.base + 0x04C);
    let mb0    = read_reg(bar0, f.base + 0x040);
    let mb1    = read_reg(bar0, f.base + 0x044);
    let hwcfg  = read_reg(bar0, f.base + 0x108);

    let state = if cpuctl == 0xBADF_1100 || cpuctl == 0xDEAD_DEAD || cpuctl == 0xBADF_5040 {
        "PRI_FAULT/GATED"
    } else if cpuctl & 0x20 != 0 {
        "HRESET"
    } else if cpuctl & 0x10 != 0 {
        "HALTED"
    } else {
        "RUNNING"
    };

    eprintln!("  {:<6} base={:#010x}  state={state}", f.name, f.base);
    eprintln!("         cpuctl={cpuctl:#010x}  sctl={sctl:#010x}  exci={exci:#010x}");
    eprintln!("         mb0={mb0:#010x}  mb1={mb1:#010x}  hwcfg={hwcfg:#010x}");

    if f.name == "FECS" || f.name == "GPCCS" {
        let status = read_reg(bar0, f.base + 0x800);
        let scratch0 = read_reg(bar0, f.base + 0x500);
        let scratch1 = read_reg(bar0, f.base + 0x504);
        eprintln!("         status={status:#010x}  scratch0={scratch0:#010x}  scratch1={scratch1:#010x}");
    }
}

fn find_sysfs_devices() -> Vec<String> {
    let mut devices = Vec::new();

    // Scan PCI bus directly — finds ALL NVIDIA GPUs regardless of driver
    let pci_dir = "/sys/bus/pci/devices";
    if let Ok(entries) = std::fs::read_dir(pci_dir) {
        for entry in entries.flatten() {
            let name = entry.file_name().to_string_lossy().to_string();
            let dev_path = format!("{pci_dir}/{name}");
            let vendor_path = format!("{dev_path}/vendor");
            let class_path = format!("{dev_path}/class");

            let vendor = std::fs::read_to_string(&vendor_path).unwrap_or_default();
            let class = std::fs::read_to_string(&class_path).unwrap_or_default();

            // NVIDIA vendor + VGA/3D controller class
            if vendor.trim() == "0x10de"
                && (class.trim().starts_with("0x0300") || class.trim().starts_with("0x0302"))
            {
                let driver = std::fs::read_link(format!("{dev_path}/driver"))
                    .ok()
                    .and_then(|p| p.file_name().map(|f| f.to_string_lossy().to_string()))
                    .unwrap_or_else(|| "none".to_string());
                eprintln!("  Found: {name} driver={driver}");
                devices.push(dev_path);
            }
        }
    }

    devices.sort();
    devices
}

#[test]
#[ignore = "requires root and NVIDIA GPU bound to nouveau"]
fn exp123t0_parasitic_falcon_probe() {
    let devices = find_sysfs_devices();
    if devices.is_empty() {
        eprintln!("No NVIDIA GPUs found in sysfs. Is nouveau loaded?");
        return;
    }

    for (idx, dev_path) in devices.iter().enumerate() {
        eprintln!("\n{}", "=".repeat(60));
        eprintln!("GPU #{idx}: {dev_path}");
        eprintln!("{}", "=".repeat(60));

        let bar0 = match Bar0Access::from_sysfs_device(dev_path) {
            Ok(b) => b,
            Err(e) => {
                eprintln!("  BAR0 access failed: {e}");
                continue;
            }
        };

        let boot0 = read_reg(&bar0, 0x0);
        let sm = identity::boot0_to_sm(boot0);
        let variant = identity::chipset_variant(boot0);
        let pmc_enable = read_reg(&bar0, 0x200);

        eprintln!("  BOOT0={boot0:#010x}  chipset={variant}  SM={sm:?}  PMC_ENABLE={pmc_enable:#010x}");

        eprintln!("\n  --- Falcon State ---");
        for f in FALCONS {
            probe_falcon(&bar0, f);
            eprintln!();
        }

        eprintln!("  --- WPR2 State ---");
        let wpr2_beg = read_reg(&bar0, 0x100CEC);
        let wpr2_end = read_reg(&bar0, 0x100CF0);
        let wpr_cfg  = read_reg(&bar0, 0x100CD0);
        eprintln!("  PFB_WPR2_BEG={wpr2_beg:#010x}  PFB_WPR2_END={wpr2_end:#010x}  WPR_CFG={wpr_cfg:#010x}");

        // Indexed WPR read
        let _ = bar0.read_u32(0x100CD4); // trigger indexed read
        let idx_lo = read_reg(&bar0, 0x100CD4);
        eprintln!("  INDEXED_WPR={idx_lo:#010x}");

        eprintln!("\n  --- PFIFO State ---");
        let pfifo_ctrl = read_reg(&bar0, 0x2200);
        let pfifo_stat = read_reg(&bar0, 0x2204);
        eprintln!("  PFIFO_CTRL={pfifo_ctrl:#010x}  PFIFO_STAT={pfifo_stat:#010x}");

        // Scan first 16 channels for active ones
        let mut active_chans = 0u32;
        for chid in 0..16 {
            let chan = read_reg(&bar0, 0x800000 + chid * 8);
            if chan & 0x8000_0000 != 0 {
                active_chans += 1;
                eprintln!("  CH[{chid:2}]: {chan:#010x} (ACTIVE, inst={:#x})", (chan & 0x0FFF_FFFF) << 12);
            }
        }
        if active_chans == 0 {
            eprintln!("  No active channels in first 16 slots");
        }

        eprintln!("\n  --- Memory Controller ---");
        let mmu_ctrl = read_reg(&bar0, 0x100C80);
        let fb_cc4   = read_reg(&bar0, 0x100CC4);
        let fb_cc8   = read_reg(&bar0, 0x100CC8);
        let fb_ccc   = read_reg(&bar0, 0x100CCC);
        eprintln!("  PFB_MMU_CTRL={mmu_ctrl:#010x}  100CC4={fb_cc4:#010x}  100CC8={fb_cc8:#010x}  100CCC={fb_ccc:#010x}");

        // GR engine status
        let gr_status = read_reg(&bar0, 0x400700);
        let gr_intr   = read_reg(&bar0, 0x400100);
        eprintln!("  GR_STATUS={gr_status:#010x}  GR_INTR={gr_intr:#010x}");
    }

    eprintln!("\n{}", "=".repeat(60));
    eprintln!("Exp 123-T0 complete.");
}
