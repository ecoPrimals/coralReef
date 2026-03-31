// SPDX-License-Identifier: AGPL-3.0-only
//! Shared helpers for Exp 123-K K80 sovereign hardware tests.

pub use coral_driver::gsp::{ApplyError, RegisterAccess};
pub use coral_driver::nv::bar0::Bar0Access;
use coral_driver::nv::kepler_falcon;

/// BAR0 access that routes through glowplug/ember daemons when available,
/// falling back to sysfs Bar0Access. Implements the same read/write interface
/// the exp123k tests use, enabling root-free register diagnostics.
pub enum K80Bar0 {
    /// Daemon-backed: reads via ember.mmio.read, writes via glowplug.write_register.
    Daemon { bdf: String },
    /// Direct sysfs mmap (requires root or BAR0 permissions).
    Sysfs(Bar0Access),
}

impl K80Bar0 {
    /// Try daemon path first, fall back to sysfs.
    pub fn open(sysfs_dev: &str) -> Self {
        let bdf = sysfs_dev
            .rsplit('/')
            .next()
            .unwrap_or(sysfs_dev)
            .to_string();

        if crate::ember_client::mmio_read(&bdf, 0).is_ok() {
            eprintln!("  K80Bar0: using daemon path for {bdf}");
            K80Bar0::Daemon { bdf }
        } else {
            eprintln!("  K80Bar0: ember unavailable, trying sysfs for {sysfs_dev}");
            match Bar0Access::from_sysfs_device(sysfs_dev) {
                Ok(bar0) => {
                    eprintln!(
                        "  K80Bar0: sysfs BAR0 open OK ({} MiB)",
                        bar0.size() / (1024 * 1024)
                    );
                    K80Bar0::Sysfs(bar0)
                }
                Err(e) => panic!("K80Bar0: cannot open BAR0 (no ember, no sysfs): {e}"),
            }
        }
    }

    pub fn size(&self) -> usize {
        match self {
            K80Bar0::Daemon { .. } => 16 * 1024 * 1024,
            K80Bar0::Sysfs(bar0) => bar0.size(),
        }
    }

    pub fn is_daemon(&self) -> bool {
        matches!(self, K80Bar0::Daemon { .. })
    }
}

impl RegisterAccess for K80Bar0 {
    fn read_u32(&self, offset: u32) -> Result<u32, ApplyError> {
        match self {
            K80Bar0::Daemon { bdf } => crate::ember_client::mmio_read(bdf, offset)
                .map_err(|e| ApplyError::MmioFailed { offset, detail: e }),
            K80Bar0::Sysfs(bar0) => bar0.read_u32(offset),
        }
    }

    fn write_u32(&mut self, offset: u32, value: u32) -> Result<(), ApplyError> {
        match self {
            K80Bar0::Daemon { bdf } => {
                let mut gp = crate::glowplug_client::GlowPlugClient::connect().map_err(|e| {
                    ApplyError::MmioFailed {
                        offset,
                        detail: format!("glowplug connect: {e}"),
                    }
                })?;
                gp.write_register(bdf, offset as u64, value, true)
                    .map_err(|e| ApplyError::MmioFailed {
                        offset,
                        detail: format!("glowplug write: {e}"),
                    })?;
                Ok(())
            }
            K80Bar0::Sysfs(bar0) => bar0.write_u32(offset, value),
        }
    }
}

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

pub struct FalconState {
    pub name: &'static str,
    pub base: u32,
    pub cpuctl: u32,
    pub sctl: u32,
    pub exci: u32,
    pub mb0: u32,
    pub mb1: u32,
    pub hwcfg: u32,
}

pub fn read_reg(bar0: &impl RegisterAccess, addr: u32) -> u32 {
    bar0.read_u32(addr).unwrap_or(0xDEAD_DEAD)
}

pub fn write_reg(bar0: &mut impl RegisterAccess, addr: u32, val: u32) {
    bar0.write_u32(addr, val).unwrap_or_else(|e| {
        eprintln!("  WRITE FAILED: {addr:#010x} = {val:#010x}: {e}");
    });
}

/// Open K80 BAR0 via daemon path (ember+glowplug) or sysfs fallback.
/// Uses `CORALREEF_K80_BDF` / `CORALREEF_VFIO_BDF` env vars, or auto-discover.
pub fn open_k80_bar0() -> K80Bar0 {
    let devices = find_k80_devices();
    if devices.is_empty() {
        panic!("No K80 devices found on vfio-pci");
    }
    K80Bar0::open(&devices[0])
}

pub fn read_falcon(bar0: &impl RegisterAccess, name: &'static str, base: u32) -> FalconState {
    FalconState {
        name,
        base,
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
/// GK110-native firmware directory (extracted from linux kernel gk110 fuc3 headers).
pub const GK110_FW_DIR: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../data/firmware/nvidia/gk110"
);

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
/// Read the VBIOS PROM from BAR0 using `Bar0Access`.
pub fn read_vbios_prom(bar0: &mut impl RegisterAccess) -> Vec<u8> {
    const PROM_BASE: u32 = 0x0030_0000;

    // Enable PROM access
    let enable_reg = bar0.read_u32(0x1854).unwrap_or(0);
    let _ = bar0.write_u32(0x1854, enable_reg & !1);

    let sig = bar0.read_u32(PROM_BASE).unwrap_or(0);
    assert_eq!(sig & 0xFFFF, 0xAA55, "VBIOS PROM signature missing");

    let blocks = ((sig >> 16) & 0xFF) as usize;
    let image_size = if blocks > 0 { blocks * 512 } else { 64 * 1024 };
    let read_size = image_size.clamp(256 * 1024, 512 * 1024);

    let mut rom = Vec::with_capacity(read_size);
    for off in (0..read_size).step_by(4) {
        let word = bar0.read_u32(PROM_BASE + off as u32).unwrap_or(0xFFFF_FFFF);
        if off > image_size && word == 0xFFFF_FFFF {
            let next = bar0
                .read_u32(PROM_BASE + off as u32 + 4)
                .unwrap_or(0xFFFF_FFFF);
            if next == 0xFFFF_FFFF {
                break;
            }
        }
        rom.extend_from_slice(&word.to_le_bytes());
    }

    let _ = bar0.write_u32(0x1854, enable_reg);
    rom
}

/// Find BIT table in VBIOS ROM. Returns (init_tables_base, condition_table_offset).
pub fn find_bit_init_tables(rom: &[u8]) -> (usize, usize) {
    let sig: &[u8] = &[0xFF, 0xB8, b'B', b'I', b'T'];
    let bit_off = rom
        .windows(sig.len())
        .position(|w| w == sig)
        .expect("BIT signature not found");

    let entry_size = rom[bit_off + 9] as usize;
    let entry_count = rom[bit_off + 10] as usize;
    let entries_start = bit_off + 12;

    eprintln!("  BIT at {bit_off:#06x}: {entry_count} entries of {entry_size} bytes");

    let mut i_data_off = 0usize;
    for i in 0..entry_count {
        let e = entries_start + i * entry_size;
        if e + 6 > rom.len() {
            break;
        }
        let id = rom[e];
        let data_off = u16::from_le_bytes([rom[e + 4], rom[e + 5]]) as usize;
        let data_sz = u16::from_le_bytes([rom[e + 2], rom[e + 3]]);
        if id != 0 {
            eprintln!(
                "    '{}' (v{}) data_off={data_off:#06x} size={data_sz}",
                id as char,
                rom[e + 1]
            );
        }
        if id == b'I' {
            i_data_off = data_off;
        }
    }

    assert!(
        i_data_off > 0 && i_data_off + 2 <= rom.len(),
        "BIT 'I' not found"
    );
    // BIT 'I' data layout (u16 pointers):
    // [0] init script table, [2] macro index, [4] macro, [6] condition table, ...
    let script_list_ptr = u16::from_le_bytes([rom[i_data_off], rom[i_data_off + 1]]) as usize;
    let cond_table = if i_data_off + 8 <= rom.len() {
        u16::from_le_bytes([rom[i_data_off + 6], rom[i_data_off + 7]]) as usize
    } else {
        0
    };
    eprintln!("  script_list_ptr={script_list_ptr:#06x}  cond_table={cond_table:#06x}");
    (script_list_ptr, cond_table)
}

/// Minimal VBIOS init script interpreter using `Bar0Access`.
pub fn interpret_vbios_scripts(
    bar0: &mut impl RegisterAccess,
    rom: &[u8],
) -> (usize, usize, usize) {
    let (init_tables_base, cond_table) = find_bit_init_tables(rom);
    // init_tables_base is a pointer to a list of u16 script pointers (direct from BIT 'I' offset 0)
    let script_table = init_tables_base;

    let rd08 = |rom: &[u8], off: usize| -> u8 { rom.get(off).copied().unwrap_or(0) };
    let rd16 = |rom: &[u8], off: usize| -> u16 {
        if off + 2 <= rom.len() {
            u16::from_le_bytes([rom[off], rom[off + 1]])
        } else {
            0
        }
    };
    let rd32 = |rom: &[u8], off: usize| -> u32 {
        if off + 4 <= rom.len() {
            u32::from_le_bytes([rom[off], rom[off + 1], rom[off + 2], rom[off + 3]])
        } else {
            0
        }
    };

    let mut total_ops = 0usize;
    let mut total_writes = 0usize;
    let mut total_scripts = 0usize;

    let mut script_idx = 0;
    loop {
        let entry_off = script_table + script_idx * 2;
        if entry_off + 2 > rom.len() {
            break;
        }
        let script_off = rd16(rom, entry_off) as usize;
        if script_off == 0 || script_off >= rom.len() {
            break;
        }

        eprintln!("    Script {script_idx} at {script_off:#06x}");
        let mut off = script_off;
        let mut execute = true;
        let mut ops = 0usize;
        let mut writes = 0usize;
        let max_ops = 50_000;

        while off != 0 && ops < max_ops {
            let op = rd08(rom, off);
            ops += 1;

            match op {
                0x71 => {
                    off = 0;
                } // DONE
                0x72 => {
                    execute = true;
                    off += 1;
                } // RESUME
                0x38 => {
                    execute = !execute;
                    off += 1;
                } // NOT
                0x7A => {
                    // ZM_REG: reg(u32) + val(u32)
                    let reg = rd32(rom, off + 1);
                    let val = rd32(rom, off + 5);
                    if execute && reg < 0x0100_0000 {
                        let _ = bar0.write_u32(reg, val);
                        writes += 1;
                    }
                    off += 9;
                }
                0x6E => {
                    // NV_REG: reg(u32) + mask(u32) + val(u32)
                    let reg = rd32(rom, off + 1);
                    let mask = rd32(rom, off + 5);
                    let val = rd32(rom, off + 9);
                    if execute && reg < 0x0100_0000 {
                        let cur = bar0.read_u32(reg).unwrap_or(0);
                        let _ = bar0.write_u32(reg, (cur & mask) | val);
                        writes += 1;
                    }
                    off += 13;
                }
                0x58 | 0x91 => {
                    // ZM_REG_SEQUENCE / ZM_REG_GROUP
                    let base = rd32(rom, off + 1);
                    let count = rd08(rom, off + 5) as usize;
                    off += 6;
                    for i in 0..count {
                        if off + 4 > rom.len() {
                            break;
                        }
                        let val = rd32(rom, off);
                        let reg = base + (i as u32) * 4;
                        if execute && reg < 0x0100_0000 {
                            let _ = bar0.write_u32(reg, val);
                            writes += 1;
                        }
                        off += 4;
                    }
                }
                0x77 => {
                    // ZM_REG16
                    let reg = rd32(rom, off + 1);
                    let val = rd16(rom, off + 5) as u32;
                    if execute && reg < 0x0100_0000 {
                        let _ = bar0.write_u32(reg, val);
                        writes += 1;
                    }
                    off += 7;
                }
                0x47 => {
                    // ANDN_REG
                    let reg = rd32(rom, off + 1);
                    let mask = rd32(rom, off + 5);
                    if execute && reg < 0x0100_0000 {
                        let cur = bar0.read_u32(reg).unwrap_or(0);
                        let _ = bar0.write_u32(reg, cur & !mask);
                        writes += 1;
                    }
                    off += 9;
                }
                0x48 => {
                    // OR_REG
                    let reg = rd32(rom, off + 1);
                    let val = rd32(rom, off + 5);
                    if execute && reg < 0x0100_0000 {
                        let cur = bar0.read_u32(reg).unwrap_or(0);
                        let _ = bar0.write_u32(reg, cur | val);
                        writes += 1;
                    }
                    off += 9;
                }
                0x74 | 0x57 => {
                    // TIME / LTIME
                    let usec = rd16(rom, off + 1) as u64;
                    if execute && usec > 0 {
                        std::thread::sleep(std::time::Duration::from_micros(usec.min(100_000)));
                    }
                    off += 3;
                }
                0x75 => {
                    // CONDITION
                    let cond = rd08(rom, off + 1);
                    if cond_table != 0 {
                        let e = cond_table + (cond as usize) * 12;
                        if e + 12 <= rom.len() {
                            let reg = rd32(rom, e);
                            let mask = rd32(rom, e + 4);
                            let val = rd32(rom, e + 8);
                            if reg != 0 {
                                let actual = bar0.read_u32(reg).unwrap_or(0);
                                if (actual & mask) != val {
                                    execute = false;
                                }
                            }
                        }
                    }
                    off += 2;
                }
                0x56 => {
                    // CONDITION_TIME
                    let cond = rd08(rom, off + 1);
                    let retries = rd08(rom, off + 2).max(1);
                    let delay = rd16(rom, off + 3) as u64;
                    if cond_table != 0 {
                        let e = cond_table + (cond as usize) * 12;
                        let mut met = false;
                        for _ in 0..retries {
                            if e + 12 <= rom.len() {
                                let reg = rd32(rom, e);
                                let mask = rd32(rom, e + 4);
                                let val = rd32(rom, e + 8);
                                let actual = bar0.read_u32(reg).unwrap_or(0);
                                if (actual & mask) == val {
                                    met = true;
                                    break;
                                }
                            }
                            std::thread::sleep(std::time::Duration::from_micros(delay));
                        }
                        if !met {
                            execute = false;
                        }
                    }
                    off += 5;
                }
                0x73 => {
                    off += 3;
                } // STRAP_CONDITION (skip)
                0x6D => {
                    // RAM_CONDITION
                    let mask = rd08(rom, off + 1);
                    let val = rd08(rom, off + 2);
                    let strap = bar0.read_u32(0x101000).unwrap_or(0) as u8;
                    if (strap & mask) != val {
                        execute = false;
                    }
                    off += 3;
                }
                0x33 => {
                    off += 2;
                } // REPEAT
                0x36 => {
                    off += 1;
                } // END_REPEAT
                0x5C => {
                    // JUMP
                    let target = rd16(rom, off + 1) as usize;
                    off = if target > 0 && target < rom.len() {
                        target
                    } else {
                        0
                    };
                }
                0x5B => {
                    off += 3;
                } // SUB_DIRECT (skip for simplicity)
                0x6B => {
                    off += 2;
                } // SUB (skip)
                0x76 | 0x39 => {
                    off += 2;
                } // IO_CONDITION / IO_FLAG_CONDITION
                0x3A => {
                    let sz = rd08(rom, off + 2) as usize;
                    off += 3 + sz;
                }
                // PLL opcodes (skip)
                0x79 | 0x4B => {
                    off += 9;
                }
                0x34 | 0x4A => {
                    let c = rd08(rom, off + 9) as usize;
                    off += 10 + c * 4;
                }
                0x59 => {
                    off += 13;
                }
                // I/O and GPIO (skip)
                0x69 => {
                    off += 5;
                }
                0x32 => {
                    let c = rd08(rom, off + 7) as usize;
                    off += 8 + c * 4;
                }
                0x37 => {
                    off += 11;
                }
                0x3B | 0x3C => {
                    off += 5;
                }
                0x49 => {
                    let c = rd08(rom, off + 7) as usize;
                    off += 8 + c * 2;
                }
                0x4C => {
                    off += 7;
                }
                0x4D => {
                    off += 6;
                }
                0x4E => {
                    let c = rd08(rom, off + 4) as usize;
                    off += 5 + c;
                }
                0x4F => {
                    off += 9;
                }
                0x50 => {
                    let c = rd08(rom, off + 3) as usize;
                    off += 4 + c * 2;
                }
                0x51 => {
                    off += 7;
                }
                0x52 => {
                    off += 4;
                }
                0x53 => {
                    off += 3;
                }
                0x54 => {
                    let c = rd08(rom, off + 1) as usize;
                    off += 2 + c * 2;
                }
                0x5A => {
                    off += 9;
                } // ZM_REG_INDIRECT
                0x5E => {
                    off += 6;
                }
                0x5F => {
                    off += 22;
                }
                0x62 => {
                    off += 5;
                }
                0x78 => {
                    off += 6;
                }
                0x87 => {
                    off += 5 + 4 * 4;
                } // simplified RAM_RESTRICT
                0x8F => {
                    let c = rd08(rom, off + 5) as usize;
                    off += 6 + c * 4 * 4;
                }
                0x90 => {
                    off += 9;
                }
                0x96 => {
                    off += 11;
                }
                0x97 => {
                    // ZM_MASK_ADD
                    let reg = rd32(rom, off + 1);
                    let mask = rd32(rom, off + 5);
                    let add = rd08(rom, off + 9) as u32;
                    if execute && reg < 0x0100_0000 {
                        let cur = bar0.read_u32(reg).unwrap_or(0);
                        let _ = bar0.write_u32(reg, (cur & mask) + add);
                        writes += 1;
                    }
                    off += 11;
                }
                0x98 => {
                    off += 8;
                }
                0x99 => {
                    let c = rd08(rom, off + 5) as usize;
                    off += 6 + c;
                }
                0x9A => {
                    off += 9;
                }
                0xA9 => {
                    let c = rd08(rom, off + 1) as usize;
                    off += 2 + c * 2;
                }
                // No-ops
                0x63 | 0x66..=0x68 | 0x8C..=0x8E | 0x92 | 0xAA => {
                    off += 1;
                }
                0x65 => {
                    off += 3;
                }
                0x6F => {
                    off += 2;
                }
                _ => {
                    eprintln!("    Unknown opcode {op:#04x} at {off:#06x}, stopping script");
                    off = 0;
                }
            }
        }
        eprintln!("    Script {script_idx}: {ops} ops, {writes} writes");
        total_ops += ops;
        total_writes += writes;
        total_scripts += 1;
        script_idx += 1;
        if script_idx > 50 {
            break;
        }
    }
    (total_scripts, total_ops, total_writes)
}
/// Path to the nvidia-470 cold→warm diff JSON relative to the workspace data dir.
pub const NVIDIA470_DIFF: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../data/k80/nvidia470-captures/nvidia470_cold_warm_diff.json"
);

/// Apply register writes from nvidia-470 cold→warm diff to BAR0.
/// Skips PMC_ENABLE, PRI-fault sentinel values, and addresses above BAR0 range.
pub fn apply_nvidia470_recipe(bar0: &mut impl RegisterAccess) -> (usize, usize) {
    let data = std::fs::read_to_string(NVIDIA470_DIFF)
        .unwrap_or_else(|e| panic!("Cannot read nvidia-470 diff: {e}"));

    let json: serde_json::Value =
        serde_json::from_str(&data).expect("Failed to parse nvidia-470 diff JSON");

    let skip = [0x200u32, 0x204]; // PMC_ENABLE, PMC_SPOON
    let mut writes = 0usize;
    let mut skipped = 0usize;

    fn apply_section(
        section: &serde_json::Value,
        bar0: &mut dyn RegisterAccess,
        skip: &[u32],
        writes: &mut usize,
        skipped: &mut usize,
        is_changed: bool,
    ) {
        if let Some(obj) = section.as_object() {
            for (_domain, regs) in obj {
                if let Some(regs_obj) = regs.as_object() {
                    for (addr_s, val_entry) in regs_obj {
                        let addr = u32::from_str_radix(addr_s.trim_start_matches("0x"), 16)
                            .unwrap_or(u32::MAX);
                        if addr >= 0x0100_0000 || skip.contains(&addr) {
                            *skipped += 1;
                            continue;
                        }

                        let val_str = if is_changed {
                            val_entry
                                .get("warm")
                                .and_then(|v| v.as_str())
                                .unwrap_or("0x0")
                        } else {
                            val_entry.as_str().unwrap_or("0x0")
                        };
                        let val =
                            u32::from_str_radix(val_str.trim_start_matches("0x"), 16).unwrap_or(0);

                        if is_pri_fault(val) {
                            *skipped += 1;
                            continue;
                        }

                        let _ = bar0.write_u32(addr, val);
                        *writes += 1;
                    }
                }
            }
        }
    }

    if let Some(added) = json.get("added") {
        apply_section(added, bar0, &skip, &mut writes, &mut skipped, false);
    }
    if let Some(changed) = json.get("changed") {
        apply_section(changed, bar0, &skip, &mut writes, &mut skipped, true);
    }

    (writes, skipped)
}
