// SPDX-License-Identifier: AGPL-3.0-or-later
//! Shared constants and BAR0/falcon helpers for Exp 123-K K80 tests.

use coral_driver::gsp::RegisterAccess;
use coral_driver::nv::bar0::Bar0Access;
use coral_driver::nv::kepler_falcon;

pub const PMC_ENABLE: u32 = 0x200;
pub const PMC_SPOON_ENABLE: u32 = 0x204;

// GF100+ PMC_ENABLE bits (envytools)
pub const PMC_PXBAR: u32 = 1 << 2; // crossbar — needed for GPC access
pub const PMC_PMFB: u32 = 1 << 3; // memory FB
pub const PMC_PRING: u32 = 1 << 5; // PRI ring
pub const PMC_PCOPY0: u32 = 1 << 6; // copy engine
pub const PMC_PFIFO: u32 = 1 << 8; // PFIFO — command submission
pub const PMC_PGRAPH: u32 = 1 << 12; // PGRAPH — GR engine + falcons
pub const PMC_PDAEMON: u32 = 1 << 13; // PDAEMON (PMU)
pub const PMC_PTIMER: u32 = 1 << 16; // timer
pub const PMC_PBFB: u32 = 1 << 20; // more FB
pub const PMC_PFFB: u32 = 1 << 29; // frame buffer front

pub const PMC_ENABLE_FULL: u32 = PMC_PXBAR
    | PMC_PMFB
    | PMC_PRING
    | PMC_PCOPY0
    | PMC_PFIFO
    | PMC_PGRAPH
    | PMC_PDAEMON
    | PMC_PTIMER
    | PMC_PBFB
    | PMC_PFFB;

pub const KEPLER_FECS_BASE: u32 = kepler_falcon::FECS_BASE;
pub const KEPLER_GPCCS_BASE: u32 = kepler_falcon::GPCCS_BASE;

/// GK110-native firmware directory (extracted from linux kernel gk110 fuc3 headers).
pub const GK110_FW_DIR: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../data/firmware/nvidia/gk110"
);

pub struct FalconState {
    pub name: &'static str,
    pub _base: u32,
    pub cpuctl: u32,
    pub sctl: u32,
    pub exci: u32,
    pub mb0: u32,
    pub mb1: u32,
    pub hwcfg: u32,
}

pub fn read_reg(bar0: &Bar0Access, addr: u32) -> u32 {
    bar0.read_u32(addr).unwrap_or(0xDEAD_DEAD)
}

pub fn write_reg(bar0: &mut Bar0Access, addr: u32, val: u32) {
    bar0.write_u32(addr, val).unwrap_or_else(|e| {
        eprintln!("  WRITE FAILED: {addr:#010x} = {val:#010x}: {e}");
    });
}

pub fn read_falcon(bar0: &Bar0Access, name: &'static str, base: u32) -> FalconState {
    FalconState {
        name,
        _base: base,
        cpuctl: read_reg(bar0, base + 0x100),
        sctl: read_reg(bar0, base + 0x240),
        exci: read_reg(bar0, base + 0x04C),
        mb0: read_reg(bar0, base + 0x040),
        mb1: read_reg(bar0, base + 0x044),
        hwcfg: read_reg(bar0, base + 0x108),
    }
}

pub fn print_falcon(f: &FalconState) {
    let state = if f.cpuctl == 0xBADF_1100
        || f.cpuctl == 0xDEAD_DEAD
        || f.cpuctl == 0xBADF_5040
        || f.cpuctl & 0xBADF_0000 == 0xBADF_0000
    {
        "PRI_FAULT"
    } else if f.cpuctl & 0x20 != 0 {
        "HRESET"
    } else if f.cpuctl & 0x10 != 0 {
        "HALTED"
    } else {
        "RUNNING"
    };
    eprintln!(
        "  {:<6} cpuctl={:#010x} ({state})  sctl={:#010x}  exci={:#010x}",
        f.name, f.cpuctl, f.sctl, f.exci
    );
    eprintln!(
        "         mb0={:#010x}  mb1={:#010x}  hwcfg={:#010x}",
        f.mb0, f.mb1, f.hwcfg
    );
}

pub fn is_pri_fault(val: u32) -> bool {
    val & 0xBAD0_0000 == 0xBAD0_0000 || val == 0xDEAD_DEAD
}

pub fn find_k80_devices() -> Vec<String> {
    // If CORALREEF_VFIO_BDF is set, use that specific device
    if let Ok(bdf) = std::env::var("CORALREEF_VFIO_BDF") {
        let dev_path = format!("/sys/bus/pci/devices/{bdf}");
        if std::fs::metadata(&dev_path).is_ok() {
            eprintln!("  K80 target (env): {bdf}");
            return vec![dev_path];
        }
    }

    let mut devices = Vec::new();
    let pci_dir = "/sys/bus/pci/devices";
    if let Ok(entries) = std::fs::read_dir(pci_dir) {
        for entry in entries.flatten() {
            let name = entry.file_name().to_string_lossy().to_string();
            let dev_path = format!("{pci_dir}/{name}");
            let vendor = std::fs::read_to_string(format!("{dev_path}/vendor")).unwrap_or_default();
            let device = std::fs::read_to_string(format!("{dev_path}/device")).unwrap_or_default();
            if vendor.trim() == "0x10de" && device.trim() == "0x102d" {
                let driver = std::fs::read_link(format!("{dev_path}/driver"))
                    .ok()
                    .and_then(|p| p.file_name().map(|f| f.to_string_lossy().to_string()))
                    .unwrap_or_else(|| "none".to_string());
                eprintln!("  K80 found: {name} driver={driver}");
                if driver == "vfio-pci" {
                    devices.push(dev_path);
                }
            }
        }
    }
    devices.sort();
    devices
}

/// Load firmware from a directory, returning (fecs_inst, fecs_data, gpccs_inst, gpccs_data).
pub fn load_firmware(fw_dir: &str) -> (Vec<u8>, Vec<u8>, Vec<u8>, Vec<u8>) {
    let read = |name: &str| -> Vec<u8> {
        let path = format!("{fw_dir}/{name}");
        std::fs::read(&path).unwrap_or_else(|e| panic!("Cannot read {path}: {e}"))
    };
    (
        read("fecs_inst.bin"),
        read("fecs_data.bin"),
        read("gpccs_inst.bin"),
        read("gpccs_data.bin"),
    )
}

/// Rebind a GPU from vfio-pci → nouveau → (trigger GR init) → nouveau → vfio-pci.
///
/// Nouveau performs full DEVINIT, PMU boot, PGOB disable, and GR initialization
/// during probe on GK110/GK210. After rebinding back to vfio-pci, the GPU state
/// is preserved: GPCs are ungated, clocks are running, PRI ring is alive.
///
/// Returns Ok(()) on success, Err(message) on failure.
pub fn nouveau_gr_warmup(bdf: &str) -> Result<(), String> {
    use std::path::Path;

    let sysfs_dev = format!("/sys/bus/pci/devices/{bdf}");
    if !Path::new(&sysfs_dev).exists() {
        return Err(format!("sysfs device not found: {sysfs_dev}"));
    }

    let current_driver = std::fs::read_link(format!("{sysfs_dev}/driver"))
        .ok()
        .and_then(|p| p.file_name().map(|f| f.to_string_lossy().to_string()));

    eprintln!("[nouveau_warmup] BDF={bdf} current_driver={current_driver:?}");

    // Step 1: Unbind from current driver (vfio-pci)
    if let Some(ref drv) = current_driver {
        let unbind_path = format!("/sys/bus/pci/drivers/{drv}/unbind");
        eprintln!("[nouveau_warmup] Unbinding from {drv}...");
        std::fs::write(&unbind_path, bdf).map_err(|e| {
            format!("unbind from {drv} failed: {e} (are you root?)")
        })?;
        std::thread::sleep(std::time::Duration::from_millis(200));
    }

    // Step 2: Remove vfio-pci id override if present, so nouveau can probe
    let driver_override = format!("{sysfs_dev}/driver_override");
    if Path::new(&driver_override).exists() {
        eprintln!("[nouveau_warmup] Clearing driver_override...");
        let _ = std::fs::write(&driver_override, "\n");
        std::thread::sleep(std::time::Duration::from_millis(100));
    }

    // Step 3: Ensure nouveau module is loaded (it's blacklisted at boot
    // but available for explicit modprobe).
    let nouveau_bind = "/sys/bus/pci/drivers/nouveau/bind";
    if !Path::new("/sys/bus/pci/drivers/nouveau").exists() {
        eprintln!("[nouveau_warmup] Loading nouveau module...");
        let status = std::process::Command::new("modprobe")
            .arg("nouveau")
            .status()
            .map_err(|e| format!("modprobe nouveau failed: {e}"))?;
        if !status.success() {
            return Err(format!(
                "modprobe nouveau exited with {status}. Is nouveau available?"
            ));
        }
        std::thread::sleep(std::time::Duration::from_millis(500));

        if !Path::new("/sys/bus/pci/drivers/nouveau").exists() {
            return Err("nouveau driver dir missing after modprobe".to_string());
        }
        eprintln!("[nouveau_warmup] nouveau module loaded");
    }

    // Step 4: Bind to nouveau
    eprintln!("[nouveau_warmup] Binding to nouveau...");
    match std::fs::write(nouveau_bind, bdf) {
        Ok(()) => {}
        Err(e) => {
            eprintln!(
                "[nouveau_warmup] Direct bind failed ({e}), trying driver_override reprobe..."
            );
            let _ = std::fs::write(&driver_override, "nouveau");
            std::thread::sleep(std::time::Duration::from_millis(50));
            std::fs::write("/sys/bus/pci/drivers_probe", bdf).map_err(|e2| {
                format!("nouveau bind failed: direct={e}, reprobe={e2}")
            })?;
        }
    }

    // Step 5: Wait for nouveau to finish probe and GR init
    eprintln!("[nouveau_warmup] Waiting for nouveau probe + GR init...");
    let mut render_node = None;
    for attempt in 0..40 {
        std::thread::sleep(std::time::Duration::from_millis(250));

        let bound_driver = std::fs::read_link(format!("{sysfs_dev}/driver"))
            .ok()
            .and_then(|p| p.file_name().map(|f| f.to_string_lossy().to_string()));

        if bound_driver.as_deref() != Some("nouveau") {
            if attempt < 5 || attempt % 10 == 0 {
                eprintln!(
                    "[nouveau_warmup] [{attempt}] driver={bound_driver:?} (waiting for nouveau)"
                );
            }
            continue;
        }

        // Nouveau bound. Look for a render node in the DRM subsystem.
        if let Ok(drm_dir) = std::fs::read_dir(format!("{sysfs_dev}/drm")) {
            for entry in drm_dir.flatten() {
                let name = entry.file_name().to_string_lossy().to_string();
                if name.starts_with("renderD") {
                    render_node = Some(format!("/dev/dri/{name}"));
                    break;
                }
            }
        }

        if render_node.is_some() || attempt >= 15 {
            break;
        }
    }

    let bound = std::fs::read_link(format!("{sysfs_dev}/driver"))
        .ok()
        .and_then(|p| p.file_name().map(|f| f.to_string_lossy().to_string()));

    if bound.as_deref() != Some("nouveau") {
        // Restore vfio-pci binding before returning error
        let _ = std::fs::write(&driver_override, "vfio-pci");
        let _ = std::fs::write("/sys/bus/pci/drivers_probe", bdf);
        return Err(format!(
            "nouveau did not bind after 10s (driver={bound:?}). Is nouveau loaded?"
        ));
    }

    eprintln!(
        "[nouveau_warmup] nouveau bound. render_node={render_node:?}"
    );

    // Step 6: Open the render node to trigger lazy GR init (if applicable).
    // On some kernels, GR is initialized lazily on first channel creation.
    // Opening the render node and issuing a GEM_NEW ioctl triggers this.
    if let Some(ref rn) = render_node {
        match std::fs::OpenOptions::new().read(true).write(true).open(rn) {
            Ok(fd) => {
                eprintln!("[nouveau_warmup] Opened {rn} — triggering GR init");
                // Give nouveau time to run GR init with the device open
                std::thread::sleep(std::time::Duration::from_secs(2));
                drop(fd);
                eprintln!("[nouveau_warmup] Render node closed after GR init window");
            }
            Err(e) => {
                eprintln!("[nouveau_warmup] Could not open {rn}: {e} (non-fatal)");
            }
        }
    } else {
        // No render node — nouveau may have failed GR init, but let's still
        // give it time; PGOB might have run during probe anyway.
        eprintln!("[nouveau_warmup] No render node found — sleeping for nouveau init");
        std::thread::sleep(std::time::Duration::from_secs(3));
    }

    // Step 7: Unbind from nouveau
    eprintln!("[nouveau_warmup] Unbinding from nouveau...");
    std::fs::write("/sys/bus/pci/drivers/nouveau/unbind", bdf).map_err(|e| {
        format!("unbind from nouveau failed: {e}")
    })?;
    std::thread::sleep(std::time::Duration::from_millis(500));

    // Step 8: Rebind to vfio-pci
    eprintln!("[nouveau_warmup] Rebinding to vfio-pci...");
    let _ = std::fs::write(&driver_override, "vfio-pci");
    std::thread::sleep(std::time::Duration::from_millis(50));

    match std::fs::write("/sys/bus/pci/drivers/vfio-pci/bind", bdf) {
        Ok(()) => {}
        Err(e) => {
            eprintln!(
                "[nouveau_warmup] Direct vfio-pci bind failed ({e}), trying reprobe..."
            );
            std::fs::write("/sys/bus/pci/drivers_probe", bdf).map_err(|e2| {
                format!("vfio-pci rebind failed: direct={e}, reprobe={e2}")
            })?;
        }
    }

    std::thread::sleep(std::time::Duration::from_millis(500));

    let final_driver = std::fs::read_link(format!("{sysfs_dev}/driver"))
        .ok()
        .and_then(|p| p.file_name().map(|f| f.to_string_lossy().to_string()));

    if final_driver.as_deref() != Some("vfio-pci") {
        return Err(format!(
            "vfio-pci rebind failed (driver={final_driver:?})"
        ));
    }

    eprintln!("[nouveau_warmup] Success: nouveau GR warmup complete, back on vfio-pci");
    Ok(())
}

/// Find the BDF for the first K80 device (regardless of current driver).
pub fn find_k80_bdf() -> Option<String> {
    let pci_dir = "/sys/bus/pci/devices";
    if let Ok(entries) = std::fs::read_dir(pci_dir) {
        let mut bdfs: Vec<String> = entries
            .flatten()
            .filter_map(|entry| {
                let name = entry.file_name().to_string_lossy().to_string();
                let dev_path = format!("{pci_dir}/{name}");
                let vendor =
                    std::fs::read_to_string(format!("{dev_path}/vendor")).unwrap_or_default();
                let device =
                    std::fs::read_to_string(format!("{dev_path}/device")).unwrap_or_default();
                if vendor.trim() == "0x10de" && device.trim() == "0x102d" {
                    Some(name)
                } else {
                    None
                }
            })
            .collect();
        bdfs.sort();
        bdfs.into_iter().next()
    } else {
        None
    }
}
