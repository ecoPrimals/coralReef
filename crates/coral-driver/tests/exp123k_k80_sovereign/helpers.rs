// SPDX-License-Identifier: AGPL-3.0-only
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
