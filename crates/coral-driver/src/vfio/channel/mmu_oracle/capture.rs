// SPDX-License-Identifier: AGPL-3.0-or-later
//! Page table and engine register capture (BAR0 / PRAMIN).

use std::collections::BTreeMap;
use std::ptr::NonNull;

use serde::{Deserialize, Serialize};

use crate::error::ChannelError;

use super::super::registers::{misc, pccsr, ramin};

const PRAMIN_OFFSET: usize = 0x0070_0000;

// ─── BAR0 accessor ──────────────────────────────────────────────────────────

/// Read-write mmap of BAR0 for oracle page table walking.
pub(crate) struct Bar0Rw {
    ptr: NonNull<u8>,
    size: usize,
    _file: Option<std::fs::File>,
    owned: bool,
}

impl Bar0Rw {
    pub fn open(bdf: &str) -> Result<Self, ChannelError> {
        let path = crate::linux_paths::sysfs_pci_device_file(bdf, "resource0");
        let file = std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .open(&path)
            .map_err(|e| ChannelError::resource_io("open", path.clone(), e))?;

        let size = 16 * 1024 * 1024; // 16 MiB BAR0
        // SAFETY: mmap of PCI BAR0 sysfs resource with R/W for PRAMIN window sliding.
        let raw = unsafe {
            rustix::mm::mmap(
                std::ptr::null_mut(),
                size,
                rustix::mm::ProtFlags::READ | rustix::mm::ProtFlags::WRITE,
                rustix::mm::MapFlags::SHARED,
                &file,
                0,
            )
        }
        .map_err(|e| ChannelError::Bar0Mmap {
            path: path.clone(),
            source: e,
        })?;

        let ptr = NonNull::new(raw.cast::<u8>()).ok_or(ChannelError::Bar0MmapNull { path })?;

        Ok(Self {
            ptr,
            size,
            _file: Some(file),
            owned: true,
        })
    }

    /// Wrap an existing VFIO MappedBar pointer for oracle capture.
    ///
    /// The resulting `Bar0Rw` does NOT unmap on drop — the caller owns the mapping.
    ///
    /// # Safety
    /// The caller must ensure the pointer remains valid for the lifetime of
    /// this `Bar0Rw` and the underlying mapping is at least `size` bytes.
    pub(crate) unsafe fn from_raw(ptr: *mut u8, size: usize) -> Result<Self, ChannelError> {
        let ptr = NonNull::new(ptr).ok_or(ChannelError::Bar0ExternalNull)?;
        Ok(Self {
            ptr,
            size,
            _file: None,
            owned: false,
        })
    }

    pub fn read_u32(&self, offset: usize) -> u32 {
        if offset + 4 > self.size {
            return 0xDEAD_DEAD;
        }
        // SAFETY: bounds checked, volatile for MMIO.
        unsafe { std::ptr::read_volatile(self.ptr.as_ptr().add(offset).cast::<u32>()) }
    }

    /// Read a 32-bit MMIO register, returning an error for out-of-bounds access.
    ///
    /// Prefer this over [`read_u32`] in new code (PMU probing, etc.) where
    /// sentinel ambiguity is unacceptable.
    pub fn try_read_u32(&self, offset: usize) -> Result<u32, ChannelError> {
        if offset + 4 > self.size {
            return Err(ChannelError::Bar0ReadOutOfBounds {
                offset,
                map_size: self.size,
            });
        }
        // SAFETY: bounds checked, volatile for MMIO.
        Ok(unsafe { std::ptr::read_volatile(self.ptr.as_ptr().add(offset).cast::<u32>()) })
    }

    pub fn write_u32(&self, offset: usize, val: u32) {
        if offset + 4 > self.size {
            return;
        }
        // SAFETY: bounds checked, volatile for MMIO.
        unsafe {
            std::ptr::write_volatile(self.ptr.as_ptr().add(offset).cast::<u32>(), val);
        }
    }

    /// Write a 32-bit MMIO register, returning an error for out-of-bounds access.
    pub fn try_write_u32(&self, offset: usize, val: u32) -> Result<(), ChannelError> {
        if offset + 4 > self.size {
            return Err(ChannelError::Bar0WriteOutOfBounds {
                offset,
                map_size: self.size,
            });
        }
        // SAFETY: bounds checked, volatile for MMIO.
        unsafe {
            std::ptr::write_volatile(self.ptr.as_ptr().add(offset).cast::<u32>(), val);
        }
        Ok(())
    }

    fn read_pramin_u64(&self, offset_in_window: usize) -> u64 {
        let lo = self.read_u32(PRAMIN_OFFSET + offset_in_window) as u64;
        let hi = self.read_u32(PRAMIN_OFFSET + offset_in_window + 4) as u64;
        lo | (hi << 32)
    }

    fn read_pramin_u32(&self, offset_in_window: usize) -> u32 {
        self.read_u32(PRAMIN_OFFSET + offset_in_window)
    }

    fn set_window(&self, vram_page: u64) {
        let window_val = (vram_page >> 16) as u32;
        self.write_u32(misc::BAR0_WINDOW, window_val);
        let _ = self.read_u32(misc::BAR0_WINDOW);
    }

    pub fn read_vram_u32(&self, vram_addr: u64) -> u32 {
        let page = vram_addr & !0xF_FFFF;
        let offset = (vram_addr & 0xF_FFFF) as usize;
        self.set_window(page);
        self.read_pramin_u32(offset)
    }

    pub fn read_vram_u64(&self, vram_addr: u64) -> u64 {
        let page = vram_addr & !0xF_FFFF;
        let offset = (vram_addr & 0xF_FFFF) as usize;
        self.set_window(page);
        self.read_pramin_u64(offset)
    }
}

impl Drop for Bar0Rw {
    fn drop(&mut self) {
        if self.owned {
            // SAFETY: unmapping the region mapped in open().
            unsafe {
                let _ = rustix::mm::munmap(self.ptr.as_ptr().cast(), self.size);
            }
        }
    }
}

// ─── Data structures ────────────────────────────────────────────────────────

/// Decode a VRAM physical address from a V2 PDE.
/// Encoding: `(phys >> 4) | flags`, so `addr = (entry & ~0xF) << 4`.
pub fn decode_entry_addr(entry: u64) -> u64 {
    (entry & !0xF) << 4
}

/// Decode aperture and flags from a V2 PDE/PTE.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EntryFlags {
    pub valid: bool,
    pub aperture: u8,
    pub aperture_name: String,
    pub vol: bool,
}

impl EntryFlags {
    pub fn decode(entry: u64) -> Self {
        let aperture = ((entry >> 1) & 3) as u8;
        Self {
            valid: (entry & 1) != 0,
            aperture,
            aperture_name: match aperture {
                0 => "INVALID".into(),
                1 => "VRAM".into(),
                2 => "SYS_COH".into(),
                3 => "SYS_NCOH".into(),
                _ => "?".into(),
            },
            vol: ((entry >> 3) & 1) != 0,
        }
    }
}

/// A single page directory or page table entry.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PageEntry {
    pub index: u32,
    pub raw: u64,
    pub decoded_addr: u64,
    pub flags: EntryFlags,
}

/// A page directory level (PD3, PD2, PD1, PD0).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PageDirectory {
    pub level: String,
    pub vram_addr: u64,
    pub entries: Vec<PageEntry>,
}

/// PD0 dual entry (small + large PDE per slot).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Pd0Entry {
    pub index: u32,
    pub small: PageEntry,
    pub large: PageEntry,
}

/// A page table (512 entries of 8 bytes each).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PageTable {
    pub vram_addr: u64,
    pub pd0_index: u32,
    pub entries: Vec<PageEntry>,
}

/// Channel instance block fields.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InstanceBlock {
    pub vram_addr: u64,
    pub pdb_lo: u32,
    pub pdb_hi: u32,
    pub pd3_vram_addr: u64,
    pub ramfc_userd_lo: u32,
    pub ramfc_userd_hi: u32,
    pub ramfc_gp_base_lo: u32,
    pub ramfc_gp_base_hi: u32,
    pub sc0_pdb_lo: u32,
    pub sc0_pdb_hi: u32,
    pub addr_limit_lo: u32,
    pub addr_limit_hi: u32,
}

/// Captured channel from PCCSR scan.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChannelInfo {
    pub channel_id: u32,
    pub pccsr_inst_raw: u32,
    pub pccsr_channel_raw: u32,
    pub enabled: bool,
    pub instance_block: InstanceBlock,
}

/// Engine/falcon register state snapshot.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EngineRegisters {
    pub pfifo: BTreeMap<String, u32>,
    pub pmu: BTreeMap<String, u32>,
    pub fecs: BTreeMap<String, u32>,
    pub gpccs: BTreeMap<String, u32>,
    pub sec2: BTreeMap<String, u32>,
    pub mmu: BTreeMap<String, u32>,
    pub misc: BTreeMap<String, u32>,
}

/// Full page table dump with engine register state.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PageTableDump {
    pub bdf: String,
    pub driver: String,
    pub boot0: u32,
    pub timestamp: String,
    pub channels: Vec<ChannelCapture>,
    pub engine_registers: EngineRegisters,
}

/// Full capture of a single channel's page table chain.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChannelCapture {
    pub info: ChannelInfo,
    pub pd3: PageDirectory,
    pub pd2_dirs: Vec<PageDirectory>,
    pub pd1_dirs: Vec<PageDirectory>,
    pub pd0_dirs: Vec<Pd0Directory>,
    pub page_tables: Vec<PageTable>,
}

/// A PD0-level directory with dual entries.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Pd0Directory {
    pub vram_addr: u64,
    pub entries: Vec<Pd0Entry>,
}

// ─── Capture logic ──────────────────────────────────────────────────────────

/// Detect the currently bound driver for a BDF.
pub fn detect_driver(bdf: &str) -> String {
    let link = format!("/sys/bus/pci/devices/{bdf}/driver");
    match std::fs::read_link(&link) {
        Ok(p) => p
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| "unknown".into()),
        Err(_) => "unbound".into(),
    }
}

fn scan_channels(bar0: &Bar0Rw) -> Vec<(u32, u32, u32)> {
    let mut channels = Vec::new();
    for id in 0..512u32 {
        let inst_reg = bar0.read_u32(pccsr::inst(id));
        if inst_reg == 0 || inst_reg == 0xFFFF_FFFF || inst_reg == 0xBADF_1000 {
            continue;
        }
        let chan_reg = bar0.read_u32(pccsr::channel(id));
        channels.push((id, inst_reg, chan_reg));
    }
    channels
}

fn read_instance_block(bar0: &Bar0Rw, inst_vram_addr: u64) -> InstanceBlock {
    let pdb_lo = bar0.read_vram_u32(inst_vram_addr + ramin::PAGE_DIR_BASE_LO as u64);
    let pdb_hi = bar0.read_vram_u32(inst_vram_addr + ramin::PAGE_DIR_BASE_HI as u64);
    let pd3_vram_addr = (pdb_lo as u64 & 0xFFFF_F000) | ((pdb_hi as u64) << 32);

    InstanceBlock {
        vram_addr: inst_vram_addr,
        pdb_lo,
        pdb_hi,
        pd3_vram_addr,
        ramfc_userd_lo: bar0.read_vram_u32(inst_vram_addr + 0x008),
        ramfc_userd_hi: bar0.read_vram_u32(inst_vram_addr + 0x00C),
        ramfc_gp_base_lo: bar0.read_vram_u32(inst_vram_addr + 0x010),
        ramfc_gp_base_hi: bar0.read_vram_u32(inst_vram_addr + 0x014),
        sc0_pdb_lo: bar0.read_vram_u32(inst_vram_addr + ramin::SC0_PAGE_DIR_BASE_LO as u64),
        sc0_pdb_hi: bar0.read_vram_u32(inst_vram_addr + ramin::SC0_PAGE_DIR_BASE_HI as u64),
        addr_limit_lo: bar0.read_vram_u32(inst_vram_addr + ramin::ADDR_LIMIT_LO as u64),
        addr_limit_hi: bar0.read_vram_u32(inst_vram_addr + ramin::ADDR_LIMIT_HI as u64),
    }
}

fn read_pd_entries(bar0: &Bar0Rw, pd_vram_addr: u64, max_entries: u32) -> Vec<PageEntry> {
    let mut entries = Vec::new();
    for i in 0..max_entries {
        let raw = bar0.read_vram_u64(pd_vram_addr + (i as u64) * 8);
        if raw == 0 {
            continue;
        }
        entries.push(PageEntry {
            index: i,
            raw,
            decoded_addr: decode_entry_addr(raw),
            flags: EntryFlags::decode(raw),
        });
    }
    entries
}

fn read_pd0_entries(bar0: &Bar0Rw, pd0_vram_addr: u64, max_entries: u32) -> Vec<Pd0Entry> {
    let mut entries = Vec::new();
    for i in 0..max_entries {
        let base = pd0_vram_addr + (i as u64) * 16;
        let small_raw = bar0.read_vram_u64(base);
        let large_raw = bar0.read_vram_u64(base + 8);
        if small_raw == 0 && large_raw == 0 {
            continue;
        }
        entries.push(Pd0Entry {
            index: i,
            small: PageEntry {
                index: i,
                raw: small_raw,
                decoded_addr: decode_entry_addr(small_raw),
                flags: EntryFlags::decode(small_raw),
            },
            large: PageEntry {
                index: i,
                raw: large_raw,
                decoded_addr: decode_entry_addr(large_raw),
                flags: EntryFlags::decode(large_raw),
            },
        });
    }
    entries
}

fn read_pt_entries(bar0: &Bar0Rw, pt_vram_addr: u64) -> Vec<PageEntry> {
    let mut entries = Vec::new();
    for i in 0..512u32 {
        let raw = bar0.read_vram_u64(pt_vram_addr + (i as u64) * 8);
        if raw == 0 {
            continue;
        }
        entries.push(PageEntry {
            index: i,
            raw,
            decoded_addr: decode_entry_addr(raw),
            flags: EntryFlags::decode(raw),
        });
    }
    entries
}

/// Walk the full page table chain for one channel, capturing all non-zero entries.
fn walk_channel_page_tables(bar0: &Bar0Rw, info: &ChannelInfo) -> ChannelCapture {
    let pd3_addr = info.instance_block.pd3_vram_addr;

    // PD3: up to 16 entries (covers 512 TB VA space, but most GPUs use fewer)
    let pd3_entries = read_pd_entries(bar0, pd3_addr, 16);
    let pd3 = PageDirectory {
        level: "PD3".into(),
        vram_addr: pd3_addr,
        entries: pd3_entries.clone(),
    };

    let mut pd2_dirs = Vec::new();
    let mut pd1_dirs = Vec::new();
    let mut pd0_dirs = Vec::new();
    let mut page_tables = Vec::new();

    // Walk PD2 for each populated PD3 entry
    for pd3_e in &pd3_entries {
        let pd2_addr = pd3_e.decoded_addr;
        if pd2_addr == 0 || pd3_e.flags.aperture == 0 {
            continue;
        }
        let pd2_entries = read_pd_entries(bar0, pd2_addr, 512);
        pd2_dirs.push(PageDirectory {
            level: format!("PD2[from PD3[{}]]", pd3_e.index),
            vram_addr: pd2_addr,
            entries: pd2_entries.clone(),
        });

        // Walk PD1 for each populated PD2 entry
        for pd2_e in &pd2_entries {
            let pd1_addr = pd2_e.decoded_addr;
            if pd1_addr == 0 || pd2_e.flags.aperture == 0 {
                continue;
            }
            let pd1_entries = read_pd_entries(bar0, pd1_addr, 512);
            pd1_dirs.push(PageDirectory {
                level: format!("PD1[from PD2[{}]]", pd2_e.index),
                vram_addr: pd1_addr,
                entries: pd1_entries.clone(),
            });

            // Walk PD0 for each populated PD1 entry
            for pd1_e in &pd1_entries {
                let pd0_addr = pd1_e.decoded_addr;
                if pd0_addr == 0 || pd1_e.flags.aperture == 0 {
                    continue;
                }
                let pd0_entries = read_pd0_entries(bar0, pd0_addr, 512);
                pd0_dirs.push(Pd0Directory {
                    vram_addr: pd0_addr,
                    entries: pd0_entries.clone(),
                });

                // Walk PT for each populated PD0 small PDE
                for pd0_e in &pd0_entries {
                    let pt_addr = pd0_e.small.decoded_addr;
                    if pt_addr == 0 || pd0_e.small.flags.aperture == 0 {
                        continue;
                    }
                    let pt_entries = read_pt_entries(bar0, pt_addr);
                    if !pt_entries.is_empty() {
                        page_tables.push(PageTable {
                            vram_addr: pt_addr,
                            pd0_index: pd0_e.index,
                            entries: pt_entries,
                        });
                    }
                }
            }
        }
    }

    ChannelCapture {
        info: info.clone(),
        pd3,
        pd2_dirs,
        pd1_dirs,
        pd0_dirs,
        page_tables,
    }
}

/// Capture engine register state for cross-driver comparison.
fn capture_engine_registers(bar0: &Bar0Rw) -> EngineRegisters {
    let mut pfifo = BTreeMap::new();
    let mut pmu = BTreeMap::new();
    let mut fecs = BTreeMap::new();
    let mut gpccs = BTreeMap::new();
    let mut sec2 = BTreeMap::new();
    let mut mmu = BTreeMap::new();
    let mut misc_regs = BTreeMap::new();

    // PFIFO
    for &(off, name) in &[
        (0x002100u32, "PFIFO_INTR"),
        (0x002140, "PFIFO_INTR_EN"),
        (0x002200, "PFIFO_ENABLE"),
        (0x002204, "PFIFO_SCHED_EN"),
        (0x002208, "PFIFO_CONTROL"),
        (0x002254, "PFIFO_SCHED_STATUS"),
        (0x002270, "PFIFO_RUNLIST_BASE"),
        (0x002274, "PFIFO_RUNLIST_SUBMIT"),
        (0x002634, "PFIFO_PBDMA_MAP"),
    ] {
        pfifo.insert(name.into(), bar0.read_u32(off as usize));
    }

    // PBDMA 0 state
    for &(off, name) in &[
        (0x040000u32, "PBDMA0_GP_BASE_LO"),
        (0x040004, "PBDMA0_GP_BASE_HI"),
        (0x040008, "PBDMA0_GP_FETCH"),
        (0x04000C, "PBDMA0_GP_GET"),
        (0x040010, "PBDMA0_GP_PUT"),
        (0x040014, "PBDMA0_GP_ENTRY0"),
        (0x040018, "PBDMA0_GP_ENTRY1"),
        (0x040044, "PBDMA0_STATUS"),
        (0x040048, "PBDMA0_CHANNEL"),
        (0x04004C, "PBDMA0_SIGNATURE"),
        (0x040054, "PBDMA0_USERD_LO"),
        (0x040058, "PBDMA0_USERD_HI"),
        (0x040080, "PBDMA0_TARGET"),
        (0x0400B0, "PBDMA0_INTR"),
        (0x0400C0, "PBDMA0_HCE_CTRL"),
        (0x040100, "PBDMA0_METHOD0"),
    ] {
        pfifo.insert(name.into(), bar0.read_u32(off as usize));
    }

    // PMU Falcon
    for &(off, name) in &[
        (0x10A000u32, "PMU_FALCON_IRQSSET"),
        (0x10A004, "PMU_FALCON_IRQSCLR"),
        (0x10A008, "PMU_FALCON_IRQSTAT"),
        (0x10A010, "PMU_FALCON_IRQMSET"),
        (0x10A014, "PMU_FALCON_IRQMCLR"),
        (0x10A040, "PMU_FALCON_MAILBOX0"),
        (0x10A044, "PMU_FALCON_MAILBOX1"),
        (0x10A080, "PMU_FALCON_OS"),
        (0x10A100, "PMU_FALCON_CPUCTL"),
        (0x10A104, "PMU_FALCON_BOOTVEC"),
        (0x10A108, "PMU_FALCON_HWCFG"),
        (0x10A10C, "PMU_FALCON_DMACTL"),
        (0x10A110, "PMU_FALCON_ENGCTL"),
        (0x10A118, "PMU_FALCON_CURCTX"),
        (0x10A11C, "PMU_FALCON_NXTCTX"),
        (0x10A4C0, "PMU_QUEUE_HEAD0"),
        (0x10A4C4, "PMU_QUEUE_HEAD1"),
        (0x10A4C8, "PMU_QUEUE_TAIL0"),
        (0x10A4CC, "PMU_QUEUE_TAIL1"),
    ] {
        pmu.insert(name.into(), bar0.read_u32(off as usize));
    }

    // FECS (GR engine Falcon — Front End Command Scheduler)
    for &(off, name) in &[
        (0x409800u32, "FECS_FALCON_OS"),
        (0x409840, "FECS_FALCON_MAILBOX0"),
        (0x409844, "FECS_FALCON_MAILBOX1"),
        (0x409900, "FECS_FALCON_CPUCTL"),
        (0x409904, "FECS_FALCON_BOOTVEC"),
        (0x409908, "FECS_FALCON_HWCFG"),
        (0x409918, "FECS_FALCON_CURCTX"),
        (0x40991C, "FECS_FALCON_NXTCTX"),
        (0x409A00, "FECS_FALCON_IRQSSET"),
        (0x409A04, "FECS_FALCON_IRQSCLR"),
        (0x409A08, "FECS_FALCON_IRQSTAT"),
        (0x409A10, "FECS_FALCON_IRQMSET"),
        (0x409B00, "FECS_CTX_STATE"),
        (0x409B04, "FECS_CTX_CONTROL"),
        (0x409C18, "FECS_FECS_ENGINE_STATUS"),
    ] {
        fecs.insert(name.into(), bar0.read_u32(off as usize));
    }

    // GPCCS (GPC Command Scheduler — per-GPC Falcon)
    for &(off, name) in &[
        (0x502800u32, "GPCCS_FALCON_OS"),
        (0x502840, "GPCCS_FALCON_MAILBOX0"),
        (0x502844, "GPCCS_FALCON_MAILBOX1"),
        (0x502900, "GPCCS_FALCON_CPUCTL"),
        (0x502904, "GPCCS_FALCON_BOOTVEC"),
        (0x502908, "GPCCS_FALCON_HWCFG"),
    ] {
        gpccs.insert(name.into(), bar0.read_u32(off as usize));
    }

    // SEC2 Falcon
    for &(off, name) in &[
        (0x840000u32, "SEC2_FALCON_IRQSSET"),
        (0x840004, "SEC2_FALCON_IRQSCLR"),
        (0x840008, "SEC2_FALCON_IRQSTAT"),
        (0x840040, "SEC2_FALCON_MAILBOX0"),
        (0x840044, "SEC2_FALCON_MAILBOX1"),
        (0x840080, "SEC2_FALCON_OS"),
        (0x840100, "SEC2_FALCON_CPUCTL"),
        (0x840104, "SEC2_FALCON_BOOTVEC"),
        (0x840108, "SEC2_FALCON_HWCFG"),
    ] {
        sec2.insert(name.into(), bar0.read_u32(off as usize));
    }

    // MMU
    for &(off, name) in &[
        (0x100C80u32, "PFB_MMU_CTRL"),
        (0x100C84, "PFB_MMU_INVALIDATE_PDB"),
        (0x100CB8, "PFB_MMU_INVALIDATE"),
        (0x100E10, "PFB_PRI_MMU_FAULT_STATUS"),
        (0x100E14, "PFB_PRI_MMU_FAULT_ADDR_LO"),
        (0x100E18, "PFB_PRI_MMU_FAULT_ADDR_HI"),
        (0x100E1C, "PFB_PRI_MMU_FAULT_INFO"),
        (0x104A20, "HUBTLB_ERR"),
    ] {
        mmu.insert(name.into(), bar0.read_u32(off as usize));
    }

    // Misc / PMC
    for &(off, name) in &[
        (0x000000u32, "BOOT0"),
        (0x000004, "BOOT1"),
        (0x000100, "PMC_INTR"),
        (0x000140, "PMC_INTR_EN"),
        (0x000200, "PMC_ENABLE"),
        (0x000204, "PMC_ENABLE_1"),
        (0x001700, "BAR0_WINDOW"),
        (0x120058, "PRIV_RING_INTR_STATUS"),
        (0x12004C, "PRIV_RING_COMMAND"),
    ] {
        misc_regs.insert(name.into(), bar0.read_u32(off as usize));
    }

    EngineRegisters {
        pfifo,
        pmu,
        fecs,
        gpccs,
        sec2,
        mmu,
        misc: misc_regs,
    }
}

/// Capture the full page table state from a GPU at the given BDF.
///
/// Works regardless of which driver is currently bound (nouveau, nvidia,
/// vfio-pci, or unbound with BAR0 still accessible). The capture includes:
/// - All active channels found in PCCSR (0-511)
/// - For each channel: full PD3→PD2→PD1→PD0→PT walk (all non-zero entries)
/// - Engine register state (PFIFO, PMU, FECS, GPCCS, SEC2, MMU)
///
/// Set `max_channels` to limit how many channels are walked (0 = all found).
/// Capture using an existing VFIO `MappedBar` — no sysfs resource0 open needed.
///
/// Used by the glowplug daemon to perform oracle captures on VFIO-bound devices
/// through the daemon's existing bar0 mapping, avoiding the sysfs mmap that
/// hangs when vfio-pci owns the device.
pub fn capture_page_tables_via_mapped_bar(
    bdf: &str,
    mapped_bar: &crate::vfio::device::MappedBar,
    max_channels: usize,
) -> Result<PageTableDump, ChannelError> {
    // SAFETY: `mapped_bar` is a live VFIO BAR0 mapping; `base_ptr`/`size` describe the
    // full mapped region for the borrow of `mapped_bar`, satisfying `Bar0Rw::from_raw`.
    let bar0 = unsafe { Bar0Rw::from_raw(mapped_bar.base_ptr(), mapped_bar.size())? };
    capture_page_tables_inner(bdf, &bar0, max_channels)
}

/// A `Send`-safe handle to a BAR0 mapping for use across thread boundaries.
///
/// Wraps a raw pointer + size so it can be moved into `spawn_blocking` tasks.
/// The caller must ensure the underlying mapping outlives this handle.
///
/// ## Thread safety (`Send`)
///
/// The underlying BAR0 window is process-global MMIO: volatile reads are defined
/// for any thread once the mapping is established. This handle does **not**
/// implement [`Sync`]: sharing `&Bar0Handle` across threads without external
/// synchronization would duplicate the same unsafety as sharing raw pointers.
/// Use one handle per task or wrap externally if shared access is required.
pub struct Bar0Handle {
    ptr: *mut u8,
    size: usize,
}

impl std::fmt::Debug for Bar0Handle {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Bar0Handle")
            .field("size", &self.size)
            .field("ptr_nonnull", &!self.ptr.is_null())
            .finish()
    }
}

// SAFETY: Matches the `Send` rationale in the [`Bar0Handle`] docs; the caller
// must keep the mapping alive.
unsafe impl Send for Bar0Handle {}

impl Bar0Handle {
    /// Create a handle from a `MappedBar` reference.
    ///
    /// The handle borrows the mapping's address — the `MappedBar` (and its
    /// owning `VfioHolder`) must outlive any task using this handle.
    pub fn from_mapped_bar(bar: &crate::vfio::device::MappedBar) -> Self {
        Self {
            ptr: bar.base_ptr(),
            size: bar.size(),
        }
    }

    /// Perform an oracle page table capture using this BAR0 mapping.
    pub fn capture_page_tables(
        &self,
        bdf: &str,
        max_channels: usize,
    ) -> Result<PageTableDump, ChannelError> {
        // SAFETY: `Bar0Handle` is only constructed from a live `MappedBar`; the caller
        // keeps that mapping alive, so `ptr`/`size` remain valid for `Bar0Rw::from_raw`.
        let bar0 = unsafe { Bar0Rw::from_raw(self.ptr, self.size)? };
        capture_page_tables_inner(bdf, &bar0, max_channels)
    }

    /// Read a 32-bit BAR0 register with proper error handling.
    pub fn try_read_u32(&self, offset: usize) -> Result<u32, ChannelError> {
        // SAFETY: Same invariants as `capture_page_tables`: underlying BAR0 mapping outlives
        // this handle, so `ptr`/`size` satisfy `Bar0Rw::from_raw`.
        let bar0 = unsafe { Bar0Rw::from_raw(self.ptr, self.size)? };
        bar0.try_read_u32(offset)
    }

    /// Write a 32-bit BAR0 register with proper error handling.
    pub fn try_write_u32(&self, offset: usize, val: u32) -> Result<(), ChannelError> {
        // SAFETY: Same invariants as `capture_page_tables`: underlying BAR0 mapping outlives
        // this handle, so `ptr`/`size` satisfy `Bar0Rw::from_raw`.
        let bar0 = unsafe { Bar0Rw::from_raw(self.ptr, self.size)? };
        bar0.try_write_u32(offset, val)
    }

    /// BAR0 mapping size in bytes.
    #[must_use]
    pub const fn size(&self) -> usize {
        self.size
    }
}

pub fn capture_page_tables(bdf: &str, max_channels: usize) -> Result<PageTableDump, ChannelError> {
    let bar0 = Bar0Rw::open(bdf)?;
    capture_page_tables_inner(bdf, &bar0, max_channels)
}

fn capture_page_tables_inner(
    bdf: &str,
    bar0: &Bar0Rw,
    max_channels: usize,
) -> Result<PageTableDump, ChannelError> {
    let driver = detect_driver(bdf);

    let boot0 = bar0.read_u32(misc::BOOT0);
    if boot0 == 0xFFFF_FFFF {
        return Err(ChannelError::Bar0ReadsAllOnes);
    }

    let saved_window = bar0.read_u32(misc::BAR0_WINDOW);
    let raw_channels = scan_channels(bar0);

    let limit = if max_channels == 0 {
        raw_channels.len()
    } else {
        max_channels.min(raw_channels.len())
    };

    let mut channels = Vec::new();
    for &(id, inst_reg, chan_reg) in raw_channels.iter().take(limit) {
        let inst_ptr_shifted = inst_reg & 0x0FFF_FFFF;
        let inst_vram_addr = (inst_ptr_shifted as u64) << 12;
        let enabled = (chan_reg & 1) != 0;

        let instance_block = read_instance_block(bar0, inst_vram_addr);
        let info = ChannelInfo {
            channel_id: id,
            pccsr_inst_raw: inst_reg,
            pccsr_channel_raw: chan_reg,
            enabled,
            instance_block,
        };

        let capture = walk_channel_page_tables(bar0, &info);
        channels.push(capture);
    }

    let engine_registers = capture_engine_registers(bar0);

    // Restore BAR0 window
    bar0.set_window((saved_window as u64) << 16);

    let timestamp = {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default();
        format!("{}s", now.as_secs())
    };

    Ok(PageTableDump {
        bdf: bdf.to_string(),
        driver,
        boot0,
        timestamp,
        channels,
        engine_registers,
    })
}
