// SPDX-License-Identifier: AGPL-3.0-only
//! Sovereign SEC2/ACR falcon boot chain — the gateway to FECS.
//!
//! Three strategies for getting FECS/GPCCS running on GV100:
//!
//! 1. **EMEM boot** (cold VFIO, HS-locked SEC2): Write signed ACR bootloader
//!    into SEC2 EMEM, PMC-reset SEC2, ROM boots from EMEM, ACR loads FECS.
//!
//! 2. **Direct IMEM boot** (post-driver-reset, HS-cleared SEC2): Load ACR
//!    firmware directly into SEC2 IMEM/DMEM, set BOOTVEC, start CPU.
//!
//! 3. **Warm handoff** (nouveau oracle): nouveau boots everything, GlowPlug
//!    swaps to VFIO preserving state.
//!
//! Both EMEM and IMEM paths need a WPR (Write-Protected Region) in DMA memory
//! containing the FECS/GPCCS firmware images for ACR to load.
//!
//! ## Architecture
//!
//! ```text
//! Host builds WPR in DMA memory
//!   → SEC2 boots (via EMEM or IMEM path)
//!     → SEC2 runs ACR firmware
//!       → ACR reads WPR, verifies LS images
//!         → ACR DMA-loads FECS firmware into FECS IMEM
//!           → ACR releases FECS HRESET
//!             → FECS starts, signals mailbox0
//!               → GR engine ready for dispatch
//! ```

use std::fmt;

use crate::error::{DriverError, DriverResult};
use crate::vfio::channel::registers::falcon;
use crate::vfio::device::{DmaBackend, MappedBar};
use crate::vfio::memory::{MemoryRegion, PraminRegion};

// ── Firmware structures ──────────────────────────────────────────────

/// NVIDIA firmware binary header (`nvfw_bin_hdr`).
///
/// All NVIDIA firmware blobs (bl.bin, ucode_load.bin) start with this header.
/// Magic = 0x10DE (NVIDIA vendor ID).
///
/// Layout:
///   [0x00] bin_magic      (u32) — always 0x000010DE
///   [0x04] bin_ver        (u32) — header version
///   [0x08] bin_size       (u32) — total file size
///   [0x0C] header_offset  (u32) — offset to type-specific header
///   [0x10] data_offset    (u32) — offset to code/data payload
///   [0x14] data_size      (u32) — size of payload
#[derive(Debug, Clone)]
pub struct NvFwBinHeader {
    pub bin_magic: u32,
    pub bin_ver: u32,
    pub bin_size: u32,
    pub header_offset: u32,
    pub data_offset: u32,
    pub data_size: u32,
}

impl NvFwBinHeader {
    pub const MAGIC: u32 = 0x0000_10DE;

    pub fn parse(data: &[u8]) -> DriverResult<Self> {
        if data.len() < 24 {
            return Err(DriverError::DeviceNotFound(
                "firmware file too small for nvfw_bin_hdr".into(),
            ));
        }
        let r = |off: usize| u32::from_le_bytes([data[off], data[off+1], data[off+2], data[off+3]]);
        let hdr = Self {
            bin_magic: r(0),
            bin_ver: r(4),
            bin_size: r(8),
            header_offset: r(12),
            data_offset: r(16),
            data_size: r(20),
        };
        if hdr.bin_magic != Self::MAGIC {
            return Err(DriverError::DeviceNotFound(
                format!("bad nvfw_bin_hdr magic: {:#010x} (expected {:#010x})", hdr.bin_magic, Self::MAGIC).into(),
            ));
        }
        Ok(hdr)
    }
}

impl fmt::Display for NvFwBinHeader {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "nvfw_bin_hdr: magic={:#x} ver={} size={:#x} hdr_off={:#x} data_off={:#x} data_size={:#x}",
            self.bin_magic, self.bin_ver, self.bin_size,
            self.header_offset, self.data_offset, self.data_size
        )
    }
}

/// Parsed HS (Heavy Secure) header from a firmware blob.
///
/// Found at `bin_hdr.header_offset` in bl.bin files. Contains the
/// bootloader descriptor that the falcon ROM uses.
///
/// The exact layout varies by generation; this covers the gp102/gv100 format.
/// Key fields extracted at known offsets within the sub-header.
#[derive(Debug, Clone)]
pub struct HsBlDescriptor {
    /// Raw bytes of the sub-header for diagnostic dump.
    pub raw: Vec<u8>,
    /// Bin header referencing this descriptor.
    pub bin_hdr: NvFwBinHeader,
}

impl HsBlDescriptor {
    pub fn parse(data: &[u8]) -> DriverResult<Self> {
        let bin_hdr = NvFwBinHeader::parse(data)?;
        let hdr_off = bin_hdr.header_offset as usize;
        let hdr_end = (hdr_off + 256).min(data.len());
        let raw = data[hdr_off..hdr_end].to_vec();
        Ok(Self { raw, bin_hdr })
    }

    /// The code+data payload slice within the original file.
    pub fn payload<'a>(&self, file_data: &'a [u8]) -> &'a [u8] {
        let off = self.bin_hdr.data_offset as usize;
        let size = self.bin_hdr.data_size as usize;
        let end = (off + size).min(file_data.len());
        &file_data[off..end]
    }

    /// Hex dump of the sub-header for analysis.
    pub fn header_hex(&self) -> String {
        self.raw.iter()
            .take(64)
            .enumerate()
            .map(|(i, b)| {
                if i > 0 && i % 16 == 0 { format!("\n    {b:02x}") }
                else if i > 0 && i % 4 == 0 { format!("  {b:02x}") }
                else { format!("{b:02x}") }
            })
            .collect()
    }
}

impl fmt::Display for HsBlDescriptor {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(f, "  {}", self.bin_hdr)?;
        write!(f, "  sub-header (first 64B): {}", self.header_hex())
    }
}

/// All firmware needed for the SEC2→ACR→FECS boot chain.
#[derive(Debug)]
pub struct AcrFirmwareSet {
    pub acr_bl_raw: Vec<u8>,
    pub acr_bl_parsed: HsBlDescriptor,
    pub acr_ucode_raw: Vec<u8>,
    pub acr_ucode_parsed: HsBlDescriptor,
    pub sec2_desc: Vec<u8>,
    pub sec2_image: Vec<u8>,
    pub sec2_sig: Vec<u8>,
    pub fecs_bl: Vec<u8>,
    pub fecs_inst: Vec<u8>,
    pub fecs_data: Vec<u8>,
    pub fecs_sig: Vec<u8>,
    pub gpccs_bl: Vec<u8>,
    pub gpccs_inst: Vec<u8>,
    pub gpccs_data: Vec<u8>,
    pub gpccs_sig: Vec<u8>,
}

impl AcrFirmwareSet {
    /// Load all firmware files for the full ACR boot chain.
    pub fn load(chip: &str) -> DriverResult<Self> {
        let base = format!("/lib/firmware/nvidia/{chip}");
        let read = |subpath: &str| -> DriverResult<Vec<u8>> {
            let path = format!("{base}/{subpath}");
            std::fs::read(&path)
                .map_err(|e| DriverError::DeviceNotFound(format!("{path}: {e}").into()))
        };

        let acr_bl_raw = read("acr/bl.bin")?;
        let acr_bl_parsed = HsBlDescriptor::parse(&acr_bl_raw)?;
        let acr_ucode_raw = read("acr/ucode_load.bin")?;
        let acr_ucode_parsed = HsBlDescriptor::parse(&acr_ucode_raw)?;

        Ok(Self {
            acr_bl_raw,
            acr_bl_parsed,
            acr_ucode_raw,
            acr_ucode_parsed,
            sec2_desc: read("sec2/desc.bin")?,
            sec2_image: read("sec2/image.bin")?,
            sec2_sig: read("sec2/sig.bin")?,
            fecs_bl: read("gr/fecs_bl.bin")?,
            fecs_inst: read("gr/fecs_inst.bin")?,
            fecs_data: read("gr/fecs_data.bin")?,
            fecs_sig: read("gr/fecs_sig.bin")?,
            gpccs_bl: read("gr/gpccs_bl.bin")?,
            gpccs_inst: read("gr/gpccs_inst.bin")?,
            gpccs_data: read("gr/gpccs_data.bin")?,
            gpccs_sig: read("gr/gpccs_sig.bin")?,
        })
    }

    /// Summary of loaded firmware sizes for diagnostics.
    pub fn summary(&self) -> String {
        format!(
            "ACR FW: bl={}B ucode={}B | SEC2: desc={}B image={}B sig={}B | \
             FECS: bl={}B inst={}B data={}B sig={}B | \
             GPCCS: bl={}B inst={}B data={}B sig={}B",
            self.acr_bl_raw.len(), self.acr_ucode_raw.len(),
            self.sec2_desc.len(), self.sec2_image.len(), self.sec2_sig.len(),
            self.fecs_bl.len(), self.fecs_inst.len(), self.fecs_data.len(), self.fecs_sig.len(),
            self.gpccs_bl.len(), self.gpccs_inst.len(), self.gpccs_data.len(), self.gpccs_sig.len(),
        )
    }
}

// ── SEC2 state probing ────────────────────────────────────────────────

/// Classified SEC2 falcon state.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Sec2State {
    /// HS-locked (BIOS POST state): SCTL bit 0 set, IMEM write-protected.
    HsLocked,
    /// Clean reset (post-driver unbind): IMEM/DMEM writable, SCTL bit 0 clear.
    CleanReset,
    /// Already running (mailbox active, firmware loaded).
    Running,
    /// Powered off or clock-gated (registers return PRI error).
    Inaccessible,
}

/// Detailed SEC2 probe result.
#[derive(Debug, Clone)]
pub struct Sec2Probe {
    pub cpuctl: u32,
    pub sctl: u32,
    pub bootvec: u32,
    pub hwcfg: u32,
    pub mailbox0: u32,
    pub mailbox1: u32,
    pub state: Sec2State,
}

impl Sec2Probe {
    pub fn capture(bar0: &MappedBar) -> Self {
        let base = falcon::SEC2_BASE;
        let r = |off: usize| bar0.read_u32(base + off).unwrap_or(0xBADF_DEAD);

        let cpuctl = r(falcon::CPUCTL);
        let sctl = r(falcon::SCTL);
        let bootvec = r(falcon::BOOTVEC);
        let hwcfg = r(falcon::HWCFG);
        let mailbox0 = r(falcon::MAILBOX0);
        let mailbox1 = r(falcon::MAILBOX1);

        let state = classify_sec2(cpuctl, sctl, mailbox0);

        Self { cpuctl, sctl, bootvec, hwcfg, mailbox0, mailbox1, state }
    }
}

impl fmt::Display for Sec2Probe {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "SEC2 @ {:#010x}: {:?} cpuctl={:#010x} sctl={:#010x} bootvec={:#010x} \
             hwcfg={:#010x} mb0={:#010x} mb1={:#010x}",
            falcon::SEC2_BASE, self.state,
            self.cpuctl, self.sctl, self.bootvec, self.hwcfg,
            self.mailbox0, self.mailbox1
        )
    }
}

fn classify_sec2(cpuctl: u32, sctl: u32, mailbox0: u32) -> Sec2State {
    use crate::vfio::channel::registers::pri;
    if pri::is_pri_error(cpuctl) || cpuctl == 0xBADF_DEAD {
        return Sec2State::Inaccessible;
    }
    if mailbox0 != 0 && (cpuctl & falcon::CPUCTL_HALTED == 0) {
        return Sec2State::Running;
    }
    if sctl & 1 != 0 {
        Sec2State::HsLocked
    } else {
        Sec2State::CleanReset
    }
}

// ── SEC2 EMEM interface ──────────────────────────────────────────────

/// Write data to SEC2 EMEM via PIO (always writable, even in HS lockdown).
///
/// nouveau `gp102_flcn_pio_emem_wr_init`: BIT(24) only for write mode.
/// Auto-increment is implicit in the EMEM port hardware.
pub fn sec2_emem_write(bar0: &MappedBar, offset: u32, data: &[u8]) {
    let base = falcon::SEC2_BASE;
    let w = |off: usize, val: u32| { let _ = bar0.write_u32(base + off, val); };

    // BIT(24) = write mode (nouveau: gp102_flcn_pio_emem_wr_init)
    w(falcon::EMEMC0, (1 << 24) | offset);

    for chunk in data.chunks(4) {
        let word = match chunk.len() {
            4 => u32::from_le_bytes([chunk[0], chunk[1], chunk[2], chunk[3]]),
            3 => u32::from_le_bytes([chunk[0], chunk[1], chunk[2], 0]),
            2 => u32::from_le_bytes([chunk[0], chunk[1], 0, 0]),
            1 => u32::from_le_bytes([chunk[0], 0, 0, 0]),
            _ => 0,
        };
        w(falcon::EMEMD0, word);
    }
}

/// Read back data from SEC2 EMEM via PIO.
///
/// nouveau `gp102_flcn_pio_emem_rd_init`: BIT(25) only for read mode.
pub fn sec2_emem_read(bar0: &MappedBar, offset: u32, len: usize) -> Vec<u32> {
    let base = falcon::SEC2_BASE;
    let w = |off: usize, val: u32| { let _ = bar0.write_u32(base + off, val); };
    let r = |off: usize| bar0.read_u32(base + off).unwrap_or(0xDEAD_DEAD);

    // BIT(25) = read mode (nouveau: gp102_flcn_pio_emem_rd_init)
    w(falcon::EMEMC0, (1 << 25) | offset);

    let word_count = len.div_ceil(4);
    (0..word_count).map(|_| r(falcon::EMEMD0)).collect()
}

/// Verify EMEM write by reading back and comparing.
pub fn sec2_emem_verify(bar0: &MappedBar, offset: u32, data: &[u8]) -> bool {
    let readback = sec2_emem_read(bar0, offset, data.len());
    for (i, chunk) in data.chunks(4).enumerate() {
        let expected = match chunk.len() {
            4 => u32::from_le_bytes([chunk[0], chunk[1], chunk[2], chunk[3]]),
            3 => u32::from_le_bytes([chunk[0], chunk[1], chunk[2], 0]),
            2 => u32::from_le_bytes([chunk[0], chunk[1], 0, 0]),
            1 => u32::from_le_bytes([chunk[0], 0, 0, 0]),
            _ => 0,
        };
        if i >= readback.len() || readback[i] != expected {
            tracing::error!(
                offset, word = i,
                expected = format!("{expected:#010x}"),
                got = format!("{:#010x}", readback.get(i).copied().unwrap_or(0xDEAD)),
                "EMEM verify mismatch"
            );
            return false;
        }
    }
    true
}

// ── SEC2 falcon-level reset ──────────────────────────────────────

/// Full engine reset: PMC-level disable/enable + falcon-local 0x3C0 reset.
///
/// Nouveau's `gp102_flcn_reset_eng()` does a PMC engine reset FIRST,
/// then the falcon-local 0x3C0 toggle. Without the PMC reset, the falcon
/// may not enter proper HRESET state and CPUCTL_STARTCPU has no effect.
///
/// For SEC2 on GV100: PMC_ENABLE (0x200) bit 22 = SEC2 engine.
/// Reset a Falcon microcontroller, matching Nouveau's `gm200_flcn_enable` sequence.
///
/// Order is critical — Nouveau does:
///   1. Falcon-local reset via `+0x3C0` pulse
///   2. PMC engine enable (for SEC2: ensures engine clock is running)
///   3. Scrub wait: poll `+0x10C` until bits [2:1] clear
///   4. Write GPU BOOT_0 chip ID to `+0x084`
///
/// Previous versions of this code did PMC disable+enable BEFORE the 0x3C0 pulse,
/// which is the wrong order and may explain why SEC2 auto-started its ROM before
/// we could upload firmware.
pub fn falcon_engine_reset(bar0: &MappedBar, base: usize) -> DriverResult<()> {
    let r = |off: usize| bar0.read_u32(base + off).unwrap_or(0xDEAD);
    let w = |off: usize, val: u32| -> DriverResult<()> {
        bar0.write_u32(base + off, val).map_err(|e| {
            DriverError::SubmitFailed(format!("falcon reset {off:#x}: {e}").into())
        })
    };

    // Step 1: Falcon-local engine reset via 0x3C0 (gp102_flcn_reset_eng).
    w(0x3C0, 0x01)?;

    // RACE Window A: Write 0x668 DURING reset (falcon logic disabled)
    if base == falcon::SEC2_BASE {
        let iv = FALCON_INST_VRAM >> 12;
        let _ = bar0.write_u32(base + SEC2_FLCN_BIND_INST, iv);
        let rba = bar0.read_u32(base + SEC2_FLCN_BIND_INST).unwrap_or(0xDEAD);
        tracing::info!(rb = format!("{rba:#010x}"), "0x668 during reset");
    }

    std::thread::sleep(std::time::Duration::from_micros(10));
    w(0x3C0, 0x00)?;

    // RACE Window B: Write 0x668 immediately after reset clear (before PMC)
    if base == falcon::SEC2_BASE {
        let iv = FALCON_INST_VRAM >> 12;
        let _ = bar0.write_u32(base + SEC2_FLCN_BIND_INST, iv);
        let rbb = bar0.read_u32(base + SEC2_FLCN_BIND_INST).unwrap_or(0xDEAD);
        tracing::info!(rb = format!("{rbb:#010x}"), "0x668 post-reset-clear");
    }

    // Step 2: PMC engine enable (gm200_flcn_enable).
    if base == falcon::SEC2_BASE {
        pmc_enable_sec2(bar0)?;

        // RACE Window C: Write 0x668 IMMEDIATELY after PMC enable (no reads first)
        let iv = FALCON_INST_VRAM >> 12;
        let _ = bar0.write_u32(base + SEC2_FLCN_BIND_INST, iv);
        // Also immediately write DMACTL
        let _ = bar0.write_u32(base + falcon::DMACTL, 0x07);
        let rbc = bar0.read_u32(base + SEC2_FLCN_BIND_INST).unwrap_or(0xDEAD);
        let dmactl_c = bar0.read_u32(base + falcon::DMACTL).unwrap_or(0xDEAD);
        tracing::info!(
            bind = format!("{rbc:#010x}"),
            dmactl = format!("{dmactl_c:#010x}"),
            "0x668 race post-PMC-enable"
        );
    }

    // Step 4: Dummy mailbox0 mask (Nouveau: gm200_flcn_reset_wait_mem_scrubbing).
    let _ = bar0.read_u32(base + falcon::MAILBOX0);

    // Step 5: Wait for memory scrubbing (0x10C bits [2:1] = 0).
    let timeout = std::time::Duration::from_millis(100);
    let start = std::time::Instant::now();
    loop {
        let scrub = r(0x10C);
        if scrub & 0x06 == 0 {
            tracing::info!(
                scrub = format!("{scrub:#010x}"),
                elapsed_us = start.elapsed().as_micros(),
                "falcon memory scrub complete"
            );
            break;
        }
        if start.elapsed() > timeout {
            tracing::warn!(scrub = format!("{scrub:#010x}"), "falcon memory scrub timeout");
            break;
        }
        std::thread::sleep(std::time::Duration::from_micros(100));
    }

    // Step 6: Write BOOT_0 chip ID to falcon 0x084 (per nouveau gm200_flcn_enable).
    let boot0 = bar0.read_u32(0x000).unwrap_or(0);
    w(0x084, boot0)?;

    // Step 7: Wait for CPUCTL_HALTED — the ROM scrubs IMEM/DMEM then HALTs.
    // Nouveau: nvkm_falcon_v1_wait_for_halt(). This is CRITICAL: registers
    // like 0x668 (instance block binding) can only be written while HALTED.
    let halt_start = std::time::Instant::now();
    let halt_timeout = std::time::Duration::from_millis(500);
    let mut halted = false;
    loop {
        let cpuctl = r(falcon::CPUCTL);
        if cpuctl & falcon::CPUCTL_HALTED != 0 {
            halted = true;
            tracing::info!(
                cpuctl = format!("{cpuctl:#010x}"),
                elapsed_us = halt_start.elapsed().as_micros(),
                "falcon HALTED after scrub"
            );
            break;
        }
        if halt_start.elapsed() > halt_timeout {
            let sctl = r(falcon::SCTL);
            let pc = r(0x030);
            tracing::warn!(
                cpuctl = format!("{cpuctl:#010x}"),
                sctl = format!("{sctl:#010x}"),
                pc = format!("{pc:#010x}"),
                "falcon did NOT halt after scrub (500ms timeout)"
            );
            break;
        }
        std::thread::sleep(std::time::Duration::from_micros(100));
    }

    let cpuctl = r(falcon::CPUCTL);
    let sctl = r(falcon::SCTL);
    tracing::info!(
        cpuctl = format!("{cpuctl:#010x}"),
        sctl = format!("{sctl:#010x}"),
        halted,
        alias_en = cpuctl & (1 << 6) != 0,
        "falcon state after reset"
    );

    Ok(())
}

// ── VRAM Instance Block for Falcon DMA ────────────────────────────────
// VRAM addresses for the page table chain (below our ACR/WPR region)
const FALCON_INST_VRAM: u32 = 0x10000;
const FALCON_PD3_VRAM: u32 = 0x11000;
const FALCON_PD2_VRAM: u32 = 0x12000;
const FALCON_PD1_VRAM: u32 = 0x13000;
const FALCON_PD0_VRAM: u32 = 0x14000;
const FALCON_PT0_VRAM: u32 = 0x15000;

fn encode_vram_pde(vram_addr: u64) -> u64 {
    const APER_VRAM: u64 = 1 << 1; // bits[2:1] = 1 = VRAM
    (vram_addr >> 4) | APER_VRAM
}

fn encode_vram_pd0_pde(vram_addr: u64) -> u64 {
    const SPT_PRESENT: u64 = 1 << 4;
    encode_vram_pde(vram_addr) | SPT_PRESENT
}

fn encode_vram_pte(vram_phys: u64) -> u64 {
    const VALID: u64 = 1; // bit[0] = VALID, bits[2:1] = 0 = VRAM aperture
    (vram_phys >> 4) | VALID
}

/// Encode a PTE pointing to system memory (SYS_MEM_COH) for the hybrid VRAM
/// page table approach. VRAM PDEs walk the page table chain in VRAM, but leaf
/// PTEs point to IOMMU-mapped system memory where the ACR/WPR data lives.
fn encode_sysmem_pte(iova: u64) -> u64 {
    const FLAGS: u64 = 1 | (2 << 1) | (1 << 3); // VALID + SYS_MEM_COH + VOL
    (iova >> 4) | FLAGS
}

/// Build a minimal VRAM-based instance block with identity-mapped page tables.
/// Returns true if successful. Maps first 2MB of VRAM so falcon DMA can
/// access VRAM addresses 0x0..0x200000 (covers our ACR payload + WPR).
pub fn build_vram_falcon_inst_block(bar0: &MappedBar) -> bool {
    let wv = |vram_addr: u32, offset: usize, val: u32| -> bool {
        match PraminRegion::new(bar0, vram_addr, offset + 4) {
            Ok(mut region) => region.write_u32(offset, val).is_ok(),
            Err(_) => false,
        }
    };
    let wv64 = |vram_addr: u32, offset: usize, val: u64| -> bool {
        let lo = (val & 0xFFFF_FFFF) as u32;
        let hi = (val >> 32) as u32;
        wv(vram_addr, offset, lo) && wv(vram_addr, offset + 4, hi)
    };

    // PD3[0] → PD2
    if !wv64(FALCON_PD3_VRAM, 0, encode_vram_pde(FALCON_PD2_VRAM as u64)) {
        return false;
    }
    // PD2[0] → PD1
    if !wv64(FALCON_PD2_VRAM, 0, encode_vram_pde(FALCON_PD1_VRAM as u64)) {
        return false;
    }
    // PD1[0] → PD0
    if !wv64(FALCON_PD1_VRAM, 0, encode_vram_pde(FALCON_PD0_VRAM as u64)) {
        return false;
    }
    // PD0[0] → PT0 (dual PDE format: small PDE at bytes 0-7)
    if !wv64(FALCON_PD0_VRAM, 0, encode_vram_pd0_pde(FALCON_PT0_VRAM as u64)) {
        return false;
    }

    // PT0: identity-map 512 small pages (4KiB each = 2MiB total)
    for i in 1u64..512 {
        let phys = i * 4096;
        let pte = encode_vram_pte(phys);
        if !wv64(FALCON_PT0_VRAM, (i as usize) * 8, pte) {
            return false;
        }
    }

    // Instance block: PAGE_DIR_BASE at RAMIN offset 0x200
    let pdb_lo: u32 = ((FALCON_PD3_VRAM >> 12) << 12)
        | (1 << 11) // BIG_PAGE_SIZE = 64KiB
        | (1 << 10) // USE_VER2_PT_FORMAT
        ;           // target bits[1:0] = 0 = VRAM, VOL=0
    if !wv(FALCON_INST_VRAM, 0x200, pdb_lo) { return false; }
    if !wv(FALCON_INST_VRAM, 0x204, 0) { return false; }

    // VA limit = 128TB
    if !wv(FALCON_INST_VRAM, 0x208, 0xFFFF_FFFF) { return false; }
    if !wv(FALCON_INST_VRAM, 0x20C, 0x0001_FFFF) { return false; }

    // Verify: read back key entries
    let rv = |vram_addr: u32, offset: usize| -> u32 {
        PraminRegion::new(bar0, vram_addr, offset + 4)
            .ok()
            .and_then(|r| r.read_u32(offset).ok())
            .unwrap_or(0xDEAD)
    };
    let pdb_rb = rv(FALCON_INST_VRAM, 0x200);
    let pd3_0 = rv(FALCON_PD3_VRAM, 0);
    let pd3_4 = rv(FALCON_PD3_VRAM, 4);
    let pt0_112_lo = rv(FALCON_PT0_VRAM, 112 * 8);
    let pt0_112_hi = rv(FALCON_PT0_VRAM, 112 * 8 + 4);
    let pt0_1_lo = rv(FALCON_PT0_VRAM, 1 * 8);
    let pt0_1_hi = rv(FALCON_PT0_VRAM, 1 * 8 + 4);

    tracing::info!(
        pdb_lo = format!("{pdb_lo:#010x}"),
        pdb_rb = format!("{pdb_rb:#010x}"),
        pd3 = format!("{pd3_0:#010x}:{pd3_4:#010x}"),
        pt112 = format!("{pt0_112_lo:#010x}:{pt0_112_hi:#010x}"),
        pt1 = format!("{pt0_1_lo:#010x}:{pt0_1_hi:#010x}"),
        "VRAM falcon instance block built"
    );
    true
}

/// PMC enable for SEC2 engine (Nouveau: `nvkm_mc_enable`).
///
/// This is called AFTER the falcon-local 0x3C0 reset to re-enable the
/// engine clock. Nouveau's `gm200_flcn_enable` does this as step 2,
/// after `reset_eng` and before `reset_wait_mem_scrubbing`.
///
/// Only ENABLES the engine — does not disable first. A full PMC
/// disable+enable cycle is a separate, more invasive operation.
fn pmc_enable_sec2(bar0: &MappedBar) -> DriverResult<()> {
    let pmc_enable: usize = 0x200;

    let sec2_bit = find_sec2_pmc_bit(bar0).unwrap_or(22);
    let sec2_mask = 1u32 << sec2_bit;

    let val = bar0.read_u32(pmc_enable).unwrap_or(0);
    let already_enabled = val & sec2_mask != 0;
    tracing::info!(
        pmc_enable = format!("{val:#010x}"),
        sec2_bit,
        already_enabled,
        "PMC SEC2 enable (post-reset)"
    );

    if !already_enabled {
        bar0.write_u32(pmc_enable, val | sec2_mask).map_err(|e| {
            DriverError::SubmitFailed(format!("PMC enable SEC2: {e}").into())
        })?;
        let _ = bar0.read_u32(pmc_enable); // read barrier
        std::thread::sleep(std::time::Duration::from_micros(20));
    }

    Ok(())
}

/// Full PMC disable+enable cycle for SEC2 (more invasive than `pmc_enable_sec2`).
/// Used by strategies that need a complete engine power cycle.
fn pmc_reset_sec2(bar0: &MappedBar) -> DriverResult<()> {
    let pmc_enable: usize = 0x200;

    let sec2_bit = find_sec2_pmc_bit(bar0).unwrap_or(22);
    let sec2_mask = 1u32 << sec2_bit;

    let val = bar0.read_u32(pmc_enable).unwrap_or(0);
    tracing::info!(
        pmc_enable = format!("{val:#010x}"),
        sec2_bit,
        sec2_mask = format!("{sec2_mask:#010x}"),
        sec2_enabled = val & sec2_mask != 0,
        "PMC SEC2 reset: disabling engine"
    );

    bar0.write_u32(pmc_enable, val & !sec2_mask).map_err(|e| {
        DriverError::SubmitFailed(format!("PMC disable SEC2: {e}").into())
    })?;
    let _ = bar0.read_u32(pmc_enable);
    std::thread::sleep(std::time::Duration::from_micros(20));

    bar0.write_u32(pmc_enable, val | sec2_mask).map_err(|e| {
        DriverError::SubmitFailed(format!("PMC enable SEC2: {e}").into())
    })?;
    let _ = bar0.read_u32(pmc_enable);
    std::thread::sleep(std::time::Duration::from_micros(20));

    let after = bar0.read_u32(pmc_enable).unwrap_or(0);
    tracing::info!(
        pmc_after = format!("{after:#010x}"),
        sec2_enabled = after & sec2_mask != 0,
        "PMC SEC2 reset: engine re-enabled"
    );

    Ok(())
}

/// Scan PTOP table at 0x22700 to find SEC2's PMC reset/enable bits.
///
/// GV100 PTOP uses multi-entry sequences:
///   - type 0b01: engine definition (bits [11:2] = engine type, bits [15:12] = instance)
///   - type 0b10: fault info
///   - type 0b11: reset/enable info (bits [20:16] = reset bit, bits [25:21] = enable bit)
/// SEC2 = engine type 0x15 (decimal 21).
fn find_sec2_pmc_bit(bar0: &MappedBar) -> Option<u32> {
    let mut found_sec2 = false;
    let mut reset_bit = None;
    let mut enable_bit = None;

    for idx in 0..64u32 {
        let entry = bar0.read_u32(0x22700 + idx as usize * 4).unwrap_or(0);
        if entry == 0 || entry == 0xFFFFFFFF {
            if found_sec2 && (reset_bit.is_some() || enable_bit.is_some()) {
                break;
            }
            found_sec2 = false;
            continue;
        }

        let entry_type = entry & 0x3;

        if entry_type == 1 {
            // Engine definition entry
            let engine_type = (entry >> 2) & 0x3FF;
            if engine_type == 0x15 {
                found_sec2 = true;
                let instance = (entry >> 12) & 0xF;
                tracing::info!(
                    ptop_idx = idx,
                    entry = format!("{entry:#010x}"),
                    engine_type = format!("{engine_type:#x}"),
                    instance,
                    "PTOP: Found SEC2 engine entry"
                );
            } else if found_sec2 {
                break; // next engine, stop
            }
        } else if entry_type == 3 && found_sec2 {
            // Reset/enable info entry (follows engine def)
            let has_reset = entry & (1 << 14) != 0;
            let has_enable = entry & (1 << 15) != 0;
            let r_bit = (entry >> 16) & 0x1F;
            let e_bit = (entry >> 21) & 0x1F;
            tracing::info!(
                ptop_idx = idx,
                entry = format!("{entry:#010x}"),
                has_reset,
                reset_bit = r_bit,
                has_enable,
                enable_bit = e_bit,
                "PTOP: SEC2 reset/enable info"
            );
            if has_reset {
                reset_bit = Some(r_bit);
            }
            if has_enable {
                enable_bit = Some(e_bit);
            }
        } else if entry_type == 2 && found_sec2 {
            // Fault info entry
            let fault_id = (entry >> 2) & 0x1FF;
            tracing::info!(
                ptop_idx = idx,
                entry = format!("{entry:#010x}"),
                fault_id,
                "PTOP: SEC2 fault info"
            );
        }
    }

    // Dump ALL PTOP entries for debugging
    tracing::info!("PTOP table dump (entries 0-31):");
    for idx in 0..32u32 {
        let entry = bar0.read_u32(0x22700 + idx as usize * 4).unwrap_or(0);
        if entry != 0 && entry != 0xFFFFFFFF {
            let etype = entry & 0x3;
            tracing::info!(
                idx,
                entry = format!("{entry:#010x}"),
                etype,
                "PTOP[{idx}]"
            );
        }
    }

    // Prefer enable_bit, fall back to reset_bit
    let result = enable_bit.or(reset_bit);
    if result.is_none() {
        tracing::warn!("SEC2 PMC bit not found in PTOP, using fallback bit 22");
    }
    result
}

/// Reset SEC2 falcon specifically.
pub fn reset_sec2(bar0: &MappedBar) -> DriverResult<()> {
    falcon_engine_reset(bar0, falcon::SEC2_BASE)
}

/// Issue STARTCPU to a falcon, using CPUCTL_ALIAS if ALIAS_EN (bit 6) is set.
///
/// Matches Nouveau's `nvkm_falcon_v1_start`:
/// ```c
/// u32 reg = nvkm_falcon_rd32(falcon, 0x100);
/// if (reg & BIT(6))
///     nvkm_falcon_wr32(falcon, 0x130, 0x2);
/// else
///     nvkm_falcon_wr32(falcon, 0x100, 0x2);
/// ```
fn falcon_start_cpu(bar0: &MappedBar, base: usize) {
    let cpuctl = bar0.read_u32(base + falcon::CPUCTL).unwrap_or(0);
    let alias_en = cpuctl & (1 << 6) != 0;
    if alias_en {
        tracing::info!("ALIAS_EN set, using CPUCTL_ALIAS (0x130) for STARTCPU");
        let _ = bar0.write_u32(base + falcon::CPUCTL_ALIAS, falcon::CPUCTL_STARTCPU);
    } else {
        tracing::info!("ALIAS_EN clear, using CPUCTL (0x100) for STARTCPU");
        let _ = bar0.write_u32(base + falcon::CPUCTL, falcon::CPUCTL_STARTCPU);
    }
}

/// Prepare a falcon for no-instance-block DMA (physical mode).
///
/// Matches Nouveau's `gm200_flcn_fw_load` for the non-instance path:
/// ```c
/// nvkm_falcon_mask(falcon, 0x624, 0x00000080, 0x00000080);
/// nvkm_falcon_wr32(falcon, 0x10c, 0x00000000);
/// ```
fn falcon_prepare_physical_dma(bar0: &MappedBar, base: usize) {
    let cur = bar0.read_u32(base + 0x624).unwrap_or(0);
    let _ = bar0.write_u32(base + 0x624, cur | 0x80);
    let _ = bar0.write_u32(base + 0x10C, 0);
}

// ── Boot attempts ────────────────────────────────────────────────────

/// Result of a boot chain attempt.
#[derive(Debug)]
pub struct AcrBootResult {
    pub strategy: &'static str,
    pub sec2_before: Sec2Probe,
    pub sec2_after: Sec2Probe,
    pub fecs_cpuctl_after: u32,
    pub fecs_mailbox0_after: u32,
    pub gpccs_cpuctl_after: u32,
    pub success: bool,
    pub notes: Vec<String>,
}

impl fmt::Display for AcrBootResult {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(f, "╔══ ACR Boot: {} ═══════════════════════════════════╗", self.strategy)?;
        writeln!(f, "  success: {}", self.success)?;
        writeln!(f, "  SEC2 before: {}", self.sec2_before)?;
        writeln!(f, "  SEC2 after:  {}", self.sec2_after)?;
        writeln!(
            f,
            "  FECS after:  cpuctl={:#010x} mailbox0={:#010x}",
            self.fecs_cpuctl_after, self.fecs_mailbox0_after
        )?;
        writeln!(
            f,
            "  GPCCS after: cpuctl={:#010x}",
            self.gpccs_cpuctl_after
        )?;
        for note in &self.notes {
            writeln!(f, "  note: {note}")?;
        }
        write!(f, "╚═══════════════════════════════════════════════════════╝")
    }
}

/// Attempt to command the (potentially still-running) SEC2 ACR firmware
/// to re-bootstrap FECS via the mailbox command interface.
///
/// After Nouveau boots, SEC2 runs the ACR firmware which enters an idle
/// loop waiting for commands. If SEC2 survived VFIO binding, we can send
/// it a `BOOTSTRAP_FALCON(FECS)` command without resetting anything.
///
/// Nouveau's ACR protocol (gv100_acr.c):
///   1. Host writes ACR_CMD_BOOTSTRAP_FALCON to MAILBOX0 or FALCON_MTHD
///   2. Host writes falcon ID (FECS=1) as parameter
///   3. SEC2 processes command, loads FECS firmware from WPR
///   4. SEC2 releases FECS from HRESET
///   5. SEC2 writes completion status
pub fn attempt_acr_mailbox_command(bar0: &MappedBar) -> AcrBootResult {
    let mut notes = Vec::new();
    let sec2_before = Sec2Probe::capture(bar0);
    let base = falcon::SEC2_BASE;
    let r = |off: usize| bar0.read_u32(base + off).unwrap_or(0xDEAD);
    let w = |off: usize, val: u32| { let _ = bar0.write_u32(base + off, val); };

    // Check if SEC2 appears to be running (from Nouveau's boot)
    let cpuctl = r(falcon::CPUCTL);
    let mb0 = r(falcon::MAILBOX0);
    let mb1 = r(falcon::MAILBOX1);
    notes.push(format!(
        "SEC2 pre-command: cpuctl={cpuctl:#010x} mb0={mb0:#010x} mb1={mb1:#010x}"
    ));

    // Try multiple ACR command approaches:

    // Approach 1: Direct MAILBOX0 command
    // Some ACR firmwares accept commands via MAILBOX0:
    //   MAILBOX0 = command_id, MAILBOX1 = parameter
    const ACR_CMD_BOOTSTRAP_FALCON: u32 = 1;
    const FALCON_ID_FECS: u32 = falcon_id::FECS;   // 2
    const FALCON_ID_GPCCS: u32 = falcon_id::GPCCS; // 3

    w(falcon::MAILBOX1, FALCON_ID_FECS);
    w(falcon::MAILBOX0, ACR_CMD_BOOTSTRAP_FALCON);
    std::thread::sleep(std::time::Duration::from_millis(200));
    let mb0_after = r(falcon::MAILBOX0);
    let mb1_after = r(falcon::MAILBOX1);
    notes.push(format!(
        "After BOOTSTRAP_FALCON(FECS): mb0={mb0_after:#010x} mb1={mb1_after:#010x}"
    ));

    // Approach 2: Dump SEC2 DMEM to find active command queue structures.
    // The ACR firmware uses CMDQ/MSGQ in DMEM for host communication.
    // Read first 4KB of DMEM to find all non-zero data.
    {
        let hwcfg = r(falcon::HWCFG);
        let dmem_sz = falcon::dmem_size_bytes(hwcfg);
        let read_sz = (dmem_sz as usize).min(4096);
        let sec2_dmem = sec2_dmem_read(bar0, 0, read_sz);
        let mut ranges = Vec::new();
        let mut in_nonzero = false;
        let mut start = 0;
        for (i, &word) in sec2_dmem.iter().enumerate() {
            if word != 0 && word != 0xDEAD_DEAD {
                if !in_nonzero {
                    start = i;
                    in_nonzero = true;
                }
            } else if in_nonzero {
                ranges.push(format!(
                    "[{:#05x}..{:#05x}]",
                    start * 4, i * 4
                ));
                in_nonzero = false;
            }
        }
        if in_nonzero {
            ranges.push(format!(
                "[{:#05x}..{:#05x}]",
                start * 4, sec2_dmem.len() * 4
            ));
        }
        notes.push(format!(
            "SEC2 DMEM size={dmem_sz}B, non-zero ranges: {}",
            if ranges.is_empty() { "NONE".to_string() } else { ranges.join(", ") }
        ));

        // Dump first 128 bytes in detail for analysis
        let mut detail = Vec::new();
        for (i, &word) in sec2_dmem.iter().take(32).enumerate() {
            if word != 0 {
                detail.push(format!("[{:#05x}]={word:#010x}", i * 4));
            }
        }
        if !detail.is_empty() {
            notes.push(format!("SEC2 DMEM[0..128]: {}", detail.join(" ")));
        }

        // Also dump around common queue descriptor offsets (0x100-0x200)
        let mut queue_detail = Vec::new();
        for (i, &word) in sec2_dmem.iter().skip(64).take(64).enumerate() {
            if word != 0 && word != 0xDEAD_DEAD {
                queue_detail.push(format!("[{:#05x}]={word:#010x}", (i + 64) * 4));
            }
        }
        if !queue_detail.is_empty() {
            notes.push(format!("SEC2 DMEM[0x100..0x200]: {}", queue_detail.join(" ")));
        }
    }

    // Approach 3: Try GPCCS too
    w(falcon::MAILBOX1, FALCON_ID_GPCCS);
    w(falcon::MAILBOX0, ACR_CMD_BOOTSTRAP_FALCON);
    std::thread::sleep(std::time::Duration::from_millis(200));
    let mb0_gpccs = r(falcon::MAILBOX0);
    notes.push(format!(
        "After BOOTSTRAP_FALCON(GPCCS): mb0={mb0_gpccs:#010x}"
    ));

    // Approach 4: Try interrupt/doorbell to wake SEC2
    // Write to SEC2 interrupt register to signal new command
    let sec2_irq_set: usize = 0x000; // INTR_SET at falcon base + 0x000 (varies)
    w(sec2_irq_set, 1);
    std::thread::sleep(std::time::Duration::from_millis(100));

    // Check final state
    let sec2_after = Sec2Probe::capture(bar0);
    let fecs_r = |off: usize| bar0.read_u32(falcon::FECS_BASE + off).unwrap_or(0xDEAD);
    let fecs_cpuctl_after = fecs_r(falcon::CPUCTL);
    let fecs_mailbox0_after = fecs_r(falcon::MAILBOX0);
    let gpccs_cpuctl_after = bar0
        .read_u32(falcon::GPCCS_BASE + falcon::CPUCTL)
        .unwrap_or(0xDEAD);
    notes.push(format!(
        "FECS: cpuctl={fecs_cpuctl_after:#010x} mb0={fecs_mailbox0_after:#010x}"
    ));
    notes.push(format!(
        "GPCCS: cpuctl={gpccs_cpuctl_after:#010x}"
    ));

    let success = fecs_cpuctl_after & falcon::CPUCTL_HRESET == 0
        && fecs_mailbox0_after != 0;

    AcrBootResult {
        strategy: "ACR mailbox command (live SEC2)",
        sec2_before,
        sec2_after,
        fecs_cpuctl_after,
        fecs_mailbox0_after,
        gpccs_cpuctl_after,
        success,
        notes,
    }
}

/// Direct FECS boot — bypass SEC2/ACR entirely.
///
/// FECS is in HRESET (cpuctl=0x10) and accepts STARTCPU. Since SEC2 cannot
/// be reset in VFIO, we bypass the ACR chain and load FECS firmware directly:
///   1. Upload fecs_inst.bin (raw code) into FECS IMEM via PIO
///   2. Upload fecs_data.bin (raw data) into FECS DMEM via PIO
///   3. Do the same for GPCCS
///   4. Set BOOTVEC=0, STARTCPU to release FECS from HRESET
///
/// This works if the GR engine doesn't enforce ACR authentication after FLR.
pub fn attempt_direct_fecs_boot(bar0: &MappedBar, fw: &AcrFirmwareSet) -> AcrBootResult {
    let mut notes = Vec::new();
    let sec2_before = Sec2Probe::capture(bar0);

    let fecs_base = falcon::FECS_BASE;
    let gpccs_base = falcon::GPCCS_BASE;
    let fr = |base: usize, off: usize| bar0.read_u32(base + off).unwrap_or(0xDEAD);
    let fw_ = |base: usize, off: usize, val: u32| { let _ = bar0.write_u32(base + off, val); };

    // Check FECS state — must be in HRESET for STARTCPU to work
    let fecs_cpuctl = fr(fecs_base, falcon::CPUCTL);
    let fecs_hwcfg = fr(fecs_base, falcon::HWCFG);
    let gpccs_cpuctl = fr(gpccs_base, falcon::CPUCTL);
    let gpccs_hwcfg = fr(gpccs_base, falcon::HWCFG);
    notes.push(format!(
        "FECS: cpuctl={fecs_cpuctl:#010x} hwcfg={fecs_hwcfg:#010x} \
         imem={}B dmem={}B",
        falcon::imem_size_bytes(fecs_hwcfg), falcon::dmem_size_bytes(fecs_hwcfg)
    ));
    notes.push(format!(
        "GPCCS: cpuctl={gpccs_cpuctl:#010x} hwcfg={gpccs_hwcfg:#010x}"
    ));

    if fecs_cpuctl & falcon::CPUCTL_HRESET == 0 {
        notes.push("FECS is NOT in HRESET — cannot use STARTCPU".to_string());
        return make_fail_result("Direct FECS boot: not in HRESET", sec2_before, bar0, notes);
    }

    // Upload FECS firmware
    // fecs_inst.bin is raw code (no bin_hdr), load at IMEM offset 0
    notes.push(format!("Uploading fecs_inst ({} bytes) to FECS IMEM@0", fw.fecs_inst.len()));
    falcon_imem_upload_nouveau(bar0, fecs_base, 0, &fw.fecs_inst, 0);

    // Verify first 16 bytes of IMEM upload
    fw_(fecs_base, falcon::IMEMC, 0x0200_0000); // read mode, addr=0
    let mut readback = [0u32; 4];
    for word in &mut readback {
        *word = fr(fecs_base, falcon::IMEMD);
    }
    let expected = &fw.fecs_inst[..16.min(fw.fecs_inst.len())];
    let readback_bytes: Vec<u8> = readback.iter().flat_map(|w| w.to_le_bytes()).collect();
    let imem_match = readback_bytes[..expected.len()] == *expected;
    notes.push(format!(
        "FECS IMEM verify: match={imem_match} read={:02x?}",
        &readback_bytes[..expected.len()]
    ));

    // fecs_data.bin is raw data, load at DMEM offset 0
    notes.push(format!("Uploading fecs_data ({} bytes) to FECS DMEM@0", fw.fecs_data.len()));
    falcon_dmem_upload(bar0, fecs_base, 0, &fw.fecs_data);

    // Also do GPCCS if it's in HRESET
    if gpccs_cpuctl & falcon::CPUCTL_HRESET != 0 {
        notes.push(format!("Uploading gpccs_inst ({} bytes) to GPCCS IMEM@0", fw.gpccs_inst.len()));
        falcon_imem_upload_nouveau(bar0, gpccs_base, 0, &fw.gpccs_inst, 0);
        notes.push(format!("Uploading gpccs_data ({} bytes) to GPCCS DMEM@0", fw.gpccs_data.len()));
        falcon_dmem_upload(bar0, gpccs_base, 0, &fw.gpccs_data);
    }

    // Boot GPCCS first (FECS expects GPCCS to be running)
    if gpccs_cpuctl & falcon::CPUCTL_HRESET != 0 {
        fw_(gpccs_base, falcon::MAILBOX0, 0);
        fw_(gpccs_base, falcon::MAILBOX1, 0);
        fw_(gpccs_base, falcon::BOOTVEC, 0);
        fw_(gpccs_base, falcon::CPUCTL, falcon::CPUCTL_STARTCPU);
        std::thread::sleep(std::time::Duration::from_millis(50));
        let gpccs_after_cpuctl = fr(gpccs_base, falcon::CPUCTL);
        let gpccs_after_mb0 = fr(gpccs_base, falcon::MAILBOX0);
        notes.push(format!(
            "GPCCS after STARTCPU: cpuctl={gpccs_after_cpuctl:#010x} mb0={gpccs_after_mb0:#010x}"
        ));
    }

    // Boot FECS
    fw_(fecs_base, falcon::MAILBOX0, 0);
    fw_(fecs_base, falcon::MAILBOX1, 0);
    fw_(fecs_base, falcon::BOOTVEC, 0);
    notes.push("FECS: BOOTVEC=0, issuing STARTCPU".to_string());
    fw_(fecs_base, falcon::CPUCTL, falcon::CPUCTL_STARTCPU);

    // Poll for FECS to respond
    let timeout = std::time::Duration::from_secs(3);
    let start = std::time::Instant::now();
    loop {
        std::thread::sleep(std::time::Duration::from_millis(10));
        let cpuctl = fr(fecs_base, falcon::CPUCTL);
        let mb0 = fr(fecs_base, falcon::MAILBOX0);
        let mb1 = fr(fecs_base, falcon::MAILBOX1);

        let hreset = cpuctl & falcon::CPUCTL_HRESET != 0;
        let halted = cpuctl & falcon::CPUCTL_HALTED != 0;

        if mb0 != 0 || halted || !hreset {
            notes.push(format!(
                "FECS responded: cpuctl={cpuctl:#010x} mb0={mb0:#010x} mb1={mb1:#010x} ({}ms)",
                start.elapsed().as_millis()
            ));
            break;
        }
        if start.elapsed() > timeout {
            notes.push(format!(
                "FECS timeout: cpuctl={cpuctl:#010x} mb0={mb0:#010x} mb1={mb1:#010x}"
            ));
            break;
        }
    }

    let sec2_after = Sec2Probe::capture(bar0);
    let fecs_cpuctl_after = fr(fecs_base, falcon::CPUCTL);
    let fecs_mailbox0_after = fr(fecs_base, falcon::MAILBOX0);
    let gpccs_cpuctl_after = fr(gpccs_base, falcon::CPUCTL);

    let success = fecs_cpuctl_after & falcon::CPUCTL_HRESET == 0
        && fecs_mailbox0_after != 0;

    AcrBootResult {
        strategy: "Direct FECS boot (bypass ACR)",
        sec2_before,
        sec2_after,
        fecs_cpuctl_after,
        fecs_mailbox0_after,
        gpccs_cpuctl_after,
        success,
        notes,
    }
}

/// Attempt 081a: Direct HRESET release experiments.
///
/// Tries several low-cost approaches before committing to the full ACR chain:
/// 1. Direct write to FECS CPUCTL to clear HRESET bit
/// 2. PMC GR engine reset toggle
/// 3. SEC2 EMEM probe (verify accessibility)
pub fn attempt_direct_hreset(bar0: &MappedBar) -> AcrBootResult {
    let mut notes = Vec::new();
    let sec2_before = Sec2Probe::capture(bar0);
    notes.push(format!("SEC2 initial state: {:?}", sec2_before.state));

    let fecs_r = |off: usize| bar0.read_u32(falcon::FECS_BASE + off).unwrap_or(0xDEAD);

    // Experiment 1: Try direct CPUCTL write to clear HRESET
    let fecs_cpuctl_before = fecs_r(falcon::CPUCTL);
    notes.push(format!("FECS cpuctl before: {fecs_cpuctl_before:#010x}"));

    if fecs_cpuctl_before & falcon::CPUCTL_HRESET != 0 {
        // Try writing 0 to CPUCTL (clear all bits including HRESET)
        let _ = bar0.write_u32(falcon::FECS_BASE + falcon::CPUCTL, 0);
        std::thread::sleep(std::time::Duration::from_millis(5));
        let after = fecs_r(falcon::CPUCTL);
        notes.push(format!("FECS cpuctl after direct clear: {after:#010x}"));

        if after & falcon::CPUCTL_HRESET == 0 {
            notes.push("Direct HRESET clear SUCCEEDED".to_string());
        } else {
            notes.push("Direct HRESET clear failed (expected — ACR-managed)".to_string());
        }
    }

    // Experiment 2: PMC GR engine reset toggle (bit 12)
    let pmc_enable: usize = 0x200;
    let pmc = bar0.read_u32(pmc_enable).unwrap_or(0);
    let gr_bit: u32 = 1 << 12;
    notes.push(format!("PMC before GR toggle: {pmc:#010x}"));

    let _ = bar0.write_u32(pmc_enable, pmc & !gr_bit);
    std::thread::sleep(std::time::Duration::from_millis(5));
    let _ = bar0.write_u32(pmc_enable, pmc | gr_bit);
    std::thread::sleep(std::time::Duration::from_millis(10));

    let fecs_after_pmc = fecs_r(falcon::CPUCTL);
    notes.push(format!("FECS cpuctl after PMC GR toggle: {fecs_after_pmc:#010x}"));

    // Experiment 3: SEC2 EMEM accessibility test
    let test_pattern: [u8; 4] = [0xDE, 0xAD, 0xBE, 0xEF];
    sec2_emem_write(bar0, 0, &test_pattern);
    let readback = sec2_emem_read(bar0, 0, 4);
    let expected_word: u32 = 0xEFBE_ADDE;
    let emem_ok = readback.first().copied() == Some(expected_word);
    notes.push(format!(
        "SEC2 EMEM write/read: wrote={:#010x} read={:#010x} match={}",
        expected_word,
        readback.first().copied().unwrap_or(0),
        emem_ok
    ));

    let sec2_after = Sec2Probe::capture(bar0);
    let fecs_cpuctl_after = fecs_r(falcon::CPUCTL);
    let fecs_mailbox0_after = fecs_r(falcon::MAILBOX0);
    let gpccs_cpuctl_after = bar0
        .read_u32(falcon::GPCCS_BASE + falcon::CPUCTL)
        .unwrap_or(0xDEAD);

    let success = fecs_cpuctl_after & falcon::CPUCTL_HRESET == 0
        && fecs_mailbox0_after != 0;

    AcrBootResult {
        strategy: "081a: direct HRESET experiments",
        sec2_before,
        sec2_after,
        fecs_cpuctl_after,
        fecs_mailbox0_after,
        gpccs_cpuctl_after,
        success,
        notes,
    }
}

/// Attempt EMEM-based SEC2 boot with signed ACR bootloader.
///
/// SEC2 has an internal ROM that runs after any reset. The ROM checks EMEM
/// for a signed bootloader. We try two approaches:
///
/// A) Write FULL bl.bin to EMEM, then engine reset → ROM finds it during init
/// B) Engine reset first, then write FULL bl.bin → ROM might be polling EMEM
///
/// The full file (with nvfw_bin_hdr + signature) is loaded, not just the payload,
/// since the ROM needs the signature for verification.
pub fn attempt_emem_boot(bar0: &MappedBar, fw: &AcrFirmwareSet) -> AcrBootResult {
    let mut notes = Vec::new();
    let sec2_before = Sec2Probe::capture(bar0);
    let base = falcon::SEC2_BASE;
    let r = |off: usize| bar0.read_u32(base + off).unwrap_or(0xDEAD);

    notes.push(format!("SEC2 state: {:?}", sec2_before.state));

    // We'll try both the full file and just the payload
    let bl_full = &fw.acr_bl_raw;
    let bl_payload = fw.acr_bl_parsed.payload(&fw.acr_bl_raw);
    notes.push(format!(
        "ACR BL: total={}B payload={}B data_off={:#x}",
        bl_full.len(), bl_payload.len(), fw.acr_bl_parsed.bin_hdr.data_offset
    ));

    // First: engine reset and dump DMEM to see what ROM initializes
    tracing::info!("EMEM: Resetting SEC2 and dumping post-ROM DMEM");
    if let Err(e) = falcon_engine_reset(bar0, base) {
        notes.push(format!("Pre-dump reset failed: {e}"));
    }
    std::thread::sleep(std::time::Duration::from_millis(100));
    let tracepc_rom = r(0x030);
    notes.push(format!("ROM idle PC: {tracepc_rom:#010x}"));

    // Dump DMEM after ROM init (first 256 bytes)
    let post_rom_dmem = sec2_dmem_read(bar0, 0, 256);
    let mut rom_data = Vec::new();
    for (i, &w) in post_rom_dmem.iter().enumerate() {
        if w != 0 && w != 0xDEAD_DEAD {
            rom_data.push(format!("[{:#05x}]={w:#010x}", i * 4));
        }
    }
    notes.push(format!(
        "DMEM after ROM: {}",
        if rom_data.is_empty() { "all zeros".to_string() } else { rom_data.join(" ") }
    ));

    // Also dump EMEM after ROM (ROM might have cleared it)
    let post_rom_emem = sec2_emem_read(bar0, 0, 64);
    let emem_nonzero: Vec<String> = post_rom_emem.iter().enumerate()
        .filter(|&(_, w)| *w != 0 && *w != 0xDEAD_DEAD)
        .map(|(i, w)| format!("[{:#05x}]={w:#010x}", i * 4))
        .collect();
    notes.push(format!(
        "EMEM after ROM: {}",
        if emem_nonzero.is_empty() { "all zeros".to_string() } else { emem_nonzero.join(" ") }
    ));

    // ── Approach A: Write full bl.bin to EMEM offset 0, then reset ──
    tracing::info!("EMEM approach A: full bl.bin@0 → reset");
    sec2_emem_write(bar0, 0, bl_full);
    if let Err(e) = falcon_engine_reset(bar0, base) {
        notes.push(format!("A: reset failed: {e}"));
    }
    std::thread::sleep(std::time::Duration::from_millis(200));
    let cpuctl_a = r(falcon::CPUCTL);
    let mb0_a = r(falcon::MAILBOX0);
    let tracepc_a = r(0x030);
    notes.push(format!(
        "A (full@0): cpuctl={cpuctl_a:#010x} mb0={mb0_a:#010x} pc={tracepc_a:#010x}"
    ));

    // ── Approach B: Write payload to EMEM offset 0, then reset ──
    tracing::info!("EMEM approach B: payload@0 → reset");
    sec2_emem_write(bar0, 0, bl_payload);
    if let Err(e) = falcon_engine_reset(bar0, base) {
        notes.push(format!("B: reset failed: {e}"));
    }
    std::thread::sleep(std::time::Duration::from_millis(200));
    let cpuctl_b = r(falcon::CPUCTL);
    let tracepc_b = r(0x030);
    notes.push(format!(
        "B (payload@0): cpuctl={cpuctl_b:#010x} pc={tracepc_b:#010x}"
    ));

    // ── Approach C: Write full bl.bin to EMEM offset 0x200 (data_offset) ──
    tracing::info!("EMEM approach C: full bl.bin@0x200 → reset");
    // Clear EMEM first
    let zeros = vec![0u8; bl_full.len() + 0x200];
    sec2_emem_write(bar0, 0, &zeros);
    sec2_emem_write(bar0, 0x200, bl_full);
    if let Err(e) = falcon_engine_reset(bar0, base) {
        notes.push(format!("C: reset failed: {e}"));
    }
    std::thread::sleep(std::time::Duration::from_millis(200));
    let cpuctl_c = r(falcon::CPUCTL);
    let tracepc_c = r(0x030);
    notes.push(format!(
        "C (full@0x200): cpuctl={cpuctl_c:#010x} pc={tracepc_c:#010x}"
    ));

    // ── Approach D: Write BL desc to DMEM + payload to IMEM via PIO ──
    // After engine reset, ROM runs and enters idle. Then we halt? No...
    // Actually, let's try BOOTVEC=0xAC4 (ROM idle PC) + write to MAILBOX
    // to signal the ROM. Some ROMs check MAILBOX for commands.
    tracing::info!("Approach D: signal ROM via MAILBOX");
    if let Err(e) = falcon_engine_reset(bar0, base) {
        notes.push(format!("D: reset failed: {e}"));
    }
    std::thread::sleep(std::time::Duration::from_millis(50));
    // Write "boot command" to MAILBOX0 (ROM might check this)
    let _ = bar0.write_u32(base + falcon::MAILBOX0, 0x1);
    std::thread::sleep(std::time::Duration::from_millis(200));
    let mb0_d = r(falcon::MAILBOX0);
    let tracepc_d = r(0x030);
    notes.push(format!(
        "D (mailbox signal): mb0={mb0_d:#010x} pc={tracepc_d:#010x} (changed={})",
        mb0_d != 0x1
    ));

    let sec2_after = Sec2Probe::capture(bar0);
    let fecs_r = |off: usize| bar0.read_u32(falcon::FECS_BASE + off).unwrap_or(0xDEAD);
    let fecs_cpuctl_after = fecs_r(falcon::CPUCTL);
    let fecs_mailbox0_after = fecs_r(falcon::MAILBOX0);
    let gpccs_cpuctl_after = bar0
        .read_u32(falcon::GPCCS_BASE + falcon::CPUCTL)
        .unwrap_or(0xDEAD);

    notes.push(format!("FECS: cpuctl={fecs_cpuctl_after:#010x} mb0={fecs_mailbox0_after:#010x}"));

    let success = fecs_cpuctl_after & falcon::CPUCTL_HRESET == 0
        && fecs_mailbox0_after != 0;

    AcrBootResult {
        strategy: "EMEM-based SEC2 boot",
        sec2_before,
        sec2_after,
        fecs_cpuctl_after,
        fecs_mailbox0_after,
        gpccs_cpuctl_after,
        success,
        notes,
    }
}

/// Attempt nouveau-style SEC2 boot: falcon reset + IMEM code + EMEM descriptor.
///
/// Matches nouveau's `gm200_flcn_fw_load()` + `gm200_flcn_fw_boot()`:
/// 1. Reset SEC2 falcon (engine reset via 0x3C0)
/// 2. Load BL CODE into IMEM at (code_limit - boot_size) with tag = start_tag
/// 3. Load BL DATA descriptor into EMEM at offset 0
/// 4. Set BOOTVEC = start_tag << 8 = 0xFD00
/// 5. Write mailbox0 = 0xcafebeef
/// 6. CPUCTL = 0x02 (STARTCPU)
/// 7. Poll for halt (CPUCTL & HRESET set)
pub fn attempt_nouveau_boot(bar0: &MappedBar, fw: &AcrFirmwareSet) -> AcrBootResult {
    let mut notes = Vec::new();
    let sec2_before = Sec2Probe::capture(bar0);
    notes.push(format!("SEC2 state: {:?}", sec2_before.state));

    let base = falcon::SEC2_BASE;
    let r = |off: usize| bar0.read_u32(base + off).unwrap_or(0xDEAD);
    let w = |off: usize, val: u32| { let _ = bar0.write_u32(base + off, val); };

    // Step 1: Falcon engine reset (nouveau: gp102_flcn_reset_eng).
    tracing::info!("Resetting SEC2 falcon via engine reset (0x3C0)");
    if let Err(e) = falcon_engine_reset(bar0, base) {
        notes.push(format!("Engine reset failed: {e}"));
    } else {
        let cpuctl_after_reset = r(falcon::CPUCTL);
        let sctl_after_reset = r(falcon::SCTL);
        notes.push(format!(
            "After engine reset: cpuctl={cpuctl_after_reset:#010x} sctl={sctl_after_reset:#010x}"
        ));
    }

    // Parse BL descriptor from the sub-header at bin_hdr.header_offset.
    let bl_hdr = &fw.acr_bl_parsed;
    let sub_hdr = &bl_hdr.raw;
    let bl_start_tag = if sub_hdr.len() >= 4 {
        u32::from_le_bytes([sub_hdr[0], sub_hdr[1], sub_hdr[2], sub_hdr[3]])
    } else {
        0xFD
    };
    let bl_code_off = if sub_hdr.len() >= 12 {
        u32::from_le_bytes([sub_hdr[8], sub_hdr[9], sub_hdr[10], sub_hdr[11]])
    } else {
        0
    };
    let bl_code_size = if sub_hdr.len() >= 16 {
        u32::from_le_bytes([sub_hdr[12], sub_hdr[13], sub_hdr[14], sub_hdr[15]])
    } else {
        0x200
    };
    let bl_data_off = if sub_hdr.len() >= 20 {
        u32::from_le_bytes([sub_hdr[16], sub_hdr[17], sub_hdr[18], sub_hdr[19]])
    } else {
        0x200
    };
    let bl_data_size = if sub_hdr.len() >= 24 {
        u32::from_le_bytes([sub_hdr[20], sub_hdr[21], sub_hdr[22], sub_hdr[23]])
    } else {
        0x100
    };

    let boot_addr = bl_start_tag << 8;
    notes.push(format!(
        "BL desc: start_tag={bl_start_tag:#x} boot_addr={boot_addr:#x} \
         code=[{bl_code_off:#x}+{bl_code_size:#x}] data=[{bl_data_off:#x}+{bl_data_size:#x}]"
    ));

    // Extract code and data from the payload.
    let payload = bl_hdr.payload(&fw.acr_bl_raw);
    let code_end = (bl_code_off + bl_code_size) as usize;
    let data_end = (bl_data_off + bl_data_size) as usize;

    let bl_code = if code_end <= payload.len() {
        &payload[bl_code_off as usize..code_end]
    } else {
        payload
    };
    let bl_data = if data_end <= payload.len() {
        &payload[bl_data_off as usize..data_end]
    } else {
        &[]
    };

    // Step 2: Load BL code into IMEM at (code_limit - boot_size).
    // SEC2 HWCFG gives code_limit; on GV100 SEC2 it's 64KB = 0x10000.
    let hwcfg = r(falcon::HWCFG);
    let code_limit = falcon::imem_size_bytes(hwcfg);
    let imem_addr = code_limit.saturating_sub(bl_code.len() as u32);
    let imem_tag = boot_addr >> 8;

    notes.push(format!(
        "Loading BL code: {} bytes to IMEM@{imem_addr:#x} tag={imem_tag:#x} (code_limit={code_limit:#x})",
        bl_code.len()
    ));

    // Use the tagged IMEM upload matching nouveau's gm200_flcn_pio_imem_wr.
    let imemc_val = (1u32 << 24) | imem_addr;
    w(falcon::IMEMC, imemc_val);

    // Write tag for the first 256-byte page.
    w(falcon::IMEMT, imem_tag);
    for (i, chunk) in bl_code.chunks(4).enumerate() {
        let byte_off = (i * 4) as u32;
        // Set tag for each new 256-byte page boundary.
        if byte_off > 0 && byte_off & 0xFF == 0 {
            w(falcon::IMEMT, imem_tag + (byte_off >> 8));
        }
        let word = match chunk.len() {
            4 => u32::from_le_bytes([chunk[0], chunk[1], chunk[2], chunk[3]]),
            3 => u32::from_le_bytes([chunk[0], chunk[1], chunk[2], 0]),
            2 => u32::from_le_bytes([chunk[0], chunk[1], 0, 0]),
            1 => u32::from_le_bytes([chunk[0], 0, 0, 0]),
            _ => 0,
        };
        w(falcon::IMEMD, word);
    }

    // Step 3: Load BL data (descriptor) into EMEM at offset 0.
    // This tells the BL where to find the main ACR firmware via DMA.
    // For now, we load the raw BL data section — DMA addresses will be 0,
    // so the BL will try to DMA and fail, but we should see execution.
    if !bl_data.is_empty() {
        notes.push(format!("Loading BL data: {} bytes to EMEM@0", bl_data.len()));
        sec2_emem_write(bar0, 0, bl_data);
    }

    // Step 3b: Set up physical DMA mode (Nouveau: gm200_flcn_fw_load non-instance path).
    falcon_prepare_physical_dma(bar0, base);

    // Step 4-6: Boot sequence (nouveau: gm200_flcn_fw_boot).
    w(falcon::MAILBOX0, 0xcafe_beef_u32);
    w(falcon::BOOTVEC, boot_addr);
    let cpuctl_pre = r(falcon::CPUCTL);
    let alias_en = cpuctl_pre & (1 << 6) != 0;
    notes.push(format!(
        "BOOTVEC={boot_addr:#x} mailbox0=0xcafebeef cpuctl={cpuctl_pre:#010x} alias_en={alias_en}, issuing STARTCPU"
    ));
    falcon_start_cpu(bar0, base);

    // Step 7: Poll for halt (nouveau waits for CPUCTL & 0x10).
    let timeout = std::time::Duration::from_secs(2);
    let start = std::time::Instant::now();
    loop {
        std::thread::sleep(std::time::Duration::from_millis(5));
        let cpuctl = r(falcon::CPUCTL);
        let mb0 = r(falcon::MAILBOX0);
        let mb1 = r(falcon::MAILBOX1);

        // Nouveau waits for HRESET bit to be set (falcon halted).
        if cpuctl & falcon::CPUCTL_HRESET != 0 && cpuctl != sec2_before.cpuctl {
            notes.push(format!(
                "SEC2 halted: cpuctl={cpuctl:#010x} mb0={mb0:#010x} mb1={mb1:#010x} ({}ms)",
                start.elapsed().as_millis()
            ));
            break;
        }
        if mb0 != 0xcafe_beef && mb0 != 0 {
            notes.push(format!(
                "SEC2 mailbox changed: cpuctl={cpuctl:#010x} mb0={mb0:#010x} ({}ms)",
                start.elapsed().as_millis()
            ));
            break;
        }
        if start.elapsed() > timeout {
            notes.push(format!(
                "SEC2 timeout: cpuctl={cpuctl:#010x} mb0={mb0:#010x} mb1={mb1:#010x}"
            ));
            break;
        }
    }

    // Read TRACEPC for debugging (write indices 0-4 to EXCI, read TRACEPC).
    let exci = r(falcon::EXCI);
    let tidx_count = (exci >> 16) & 0xFF;
    let mut tracepc = Vec::new();
    for sp in 0..tidx_count.min(8) {
        w(falcon::EXCI, sp);
        tracepc.push(r(falcon::TRACEPC));
    }
    notes.push(format!(
        "EXCI={exci:#010x} TRACEPC({tidx_count}): {:?}",
        tracepc.iter().map(|v| format!("{v:#010x}")).collect::<Vec<_>>()
    ));

    let sec2_after = Sec2Probe::capture(bar0);
    let fecs_r = |off: usize| bar0.read_u32(falcon::FECS_BASE + off).unwrap_or(0xDEAD);
    let fecs_cpuctl_after = fecs_r(falcon::CPUCTL);
    let fecs_mailbox0_after = fecs_r(falcon::MAILBOX0);
    let gpccs_cpuctl_after = bar0
        .read_u32(falcon::GPCCS_BASE + falcon::CPUCTL)
        .unwrap_or(0xDEAD);

    let success = fecs_cpuctl_after & falcon::CPUCTL_HRESET == 0
        && fecs_mailbox0_after != 0;

    AcrBootResult {
        strategy: "nouveau-style IMEM+EMEM SEC2 boot",
        sec2_before,
        sec2_after,
        fecs_cpuctl_after,
        fecs_mailbox0_after,
        gpccs_cpuctl_after,
        success,
        notes,
    }
}

// ── Firmware sub-header parsing (Nouveau-compatible) ─────────────────

/// Falcon DMA context indices (from Nouveau `nvkm/falcon/priv.h`).
///
/// SEC2 uses a different DMA index than generic falcons: context 6 maps
/// through the falcon's instance block page tables. Context 0 is physical
/// DMA (no translation). Using the wrong index causes the BL to read from
/// VRAM instead of system memory.
mod dma_idx {
    /// Physical DMA, no MMU translation.
    #[expect(dead_code, reason = "kept for non-instance-block paths")]
    pub const PHYS: u32 = 0;
    /// Virtual DMA via instance block — `FALCON_DMAIDX_VIRT` in Nouveau.
    /// Used in flcn_bl_dmem_desc_v2.ctx_dma for SEC2 ACR boot.
    pub const VIRT: u32 = 4;
    /// Physical DMA to system memory — `FALCON_DMAIDX_PHYS_SYS` in Nouveau.
    #[expect(dead_code, reason = "kept for alternative DMA paths")]
    pub const PHYS_SYS: u32 = 6;
}

/// IOVA for ACR firmware DMA buffer — must be within the channel's 2MB
/// identity-mapped page table range (PT0 covers IOVAs 0x1000..0x1FF000).
/// Placed at 1.5MB to avoid conflicts with channel infrastructure and GPFIFO.
const ACR_IOVA_BASE: u64 = 0x18_0000;

/// SEC2 falcon instance block binding register (from Nouveau `gp102_sec2_flcn_bind_inst`).
const SEC2_FLCN_BIND_INST: usize = 0x668;

/// Parsed HS header from `ucode_load.bin` sub-header.
///
/// Located at `bin_hdr.header_offset` in the file. Contains offsets
/// to signature data and the HS load header within the data payload.
///
/// Layout matches `struct nvfw_hs_header` from Nouveau.
#[derive(Debug, Clone)]
pub struct HsHeader {
    pub sig_dbg_offset: u32,
    pub sig_dbg_size: u32,
    pub sig_prod_offset: u32,
    pub sig_prod_size: u32,
    pub patch_loc: u32,
    pub patch_sig: u32,
    pub hdr_offset: u32,
    pub hdr_size: u32,
}

impl HsHeader {
    pub fn parse(data: &[u8]) -> DriverResult<Self> {
        if data.len() < 32 {
            return Err(DriverError::DeviceNotFound("HS header too small".into()));
        }
        let r = |off: usize| u32::from_le_bytes([data[off], data[off+1], data[off+2], data[off+3]]);
        Ok(Self {
            sig_dbg_offset: r(0),
            sig_dbg_size: r(4),
            sig_prod_offset: r(8),
            sig_prod_size: r(12),
            patch_loc: r(16),
            patch_sig: r(20),
            hdr_offset: r(24),
            hdr_size: r(28),
        })
    }
}

/// Parsed HS load header — code/data layout within the ACR payload.
///
/// Located at `hs_header.hdr_offset` relative to the DATA payload.
/// Tells the BL how the ACR firmware sections are organized.
///
/// Layout matches `struct nvfw_hs_load_header` from Nouveau.
#[derive(Debug, Clone)]
pub struct HsLoadHeader {
    pub non_sec_code_off: u32,
    pub non_sec_code_size: u32,
    pub data_dma_base: u32,
    pub data_size: u32,
    pub num_apps: u32,
    /// (code_off, code_size) per app.
    pub apps: Vec<(u32, u32)>,
}

impl HsLoadHeader {
    pub fn parse(data: &[u8]) -> DriverResult<Self> {
        if data.len() < 20 {
            return Err(DriverError::DeviceNotFound("HS load header too small".into()));
        }
        let r = |off: usize| u32::from_le_bytes([data[off], data[off+1], data[off+2], data[off+3]]);
        let num_apps = r(16);
        let mut apps = Vec::new();
        let base = 20;
        for i in 0..num_apps as usize {
            let code_off = if base + i * 4 + 4 <= data.len() { r(base + i * 4) } else { 0 };
            let size_idx = num_apps as usize + i;
            let code_size = if base + size_idx * 4 + 4 <= data.len() { r(base + size_idx * 4) } else { 0 };
            apps.push((code_off, code_size));
        }
        Ok(Self {
            non_sec_code_off: r(0),
            non_sec_code_size: r(4),
            data_dma_base: r(8),
            data_size: r(12),
            num_apps,
            apps,
        })
    }
}

/// Parsed BL sub-header from `acr/bl.bin`.
///
/// Located at `bin_hdr.header_offset` in the file.
/// Matches `struct nvfw_hs_bl_desc` from Nouveau.
#[derive(Debug, Clone)]
pub struct HsBlDesc {
    pub bl_start_tag: u32,
    pub bl_desc_dmem_load_off: u32,
    pub bl_code_off: u32,
    pub bl_code_size: u32,
    pub bl_desc_size: u32,
    pub bl_data_off: u32,
    pub bl_data_size: u32,
}

impl HsBlDesc {
    pub fn parse(data: &[u8]) -> DriverResult<Self> {
        if data.len() < 28 {
            return Err(DriverError::DeviceNotFound("BL desc too small".into()));
        }
        let r = |off: usize| u32::from_le_bytes([data[off], data[off+1], data[off+2], data[off+3]]);
        Ok(Self {
            bl_start_tag: r(0),
            bl_desc_dmem_load_off: r(4),
            bl_code_off: r(8),
            bl_code_size: r(12),
            bl_desc_size: r(16),
            bl_data_off: r(20),
            bl_data_size: r(24),
        })
    }
}

impl fmt::Display for HsBlDesc {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "BL: tag={:#x} dmem_off={:#x} code=[{:#x}+{:#x}] desc_sz={:#x} data=[{:#x}+{:#x}]",
            self.bl_start_tag, self.bl_desc_dmem_load_off,
            self.bl_code_off, self.bl_code_size, self.bl_desc_size,
            self.bl_data_off, self.bl_data_size,
        )
    }
}

/// Fully parsed ACR firmware ready for the boot chain.
#[derive(Debug)]
pub struct ParsedAcrFirmware {
    pub bl_desc: HsBlDesc,
    pub bl_code: Vec<u8>,
    pub hs_header: HsHeader,
    pub load_header: HsLoadHeader,
    pub acr_payload: Vec<u8>,
}

impl ParsedAcrFirmware {
    /// Parse bl.bin and ucode_load.bin into structured form.
    pub fn parse(fw: &AcrFirmwareSet) -> DriverResult<Self> {
        let bl_bin_hdr = &fw.acr_bl_parsed.bin_hdr;
        let bl_sub = &fw.acr_bl_parsed.raw;
        let bl_desc = HsBlDesc::parse(bl_sub)?;

        let bl_payload = fw.acr_bl_parsed.payload(&fw.acr_bl_raw);
        let code_end = (bl_desc.bl_code_off + bl_desc.bl_code_size) as usize;
        let bl_code = if code_end <= bl_payload.len() {
            bl_payload[bl_desc.bl_code_off as usize..code_end].to_vec()
        } else {
            bl_payload.to_vec()
        };

        let ucode_bin_hdr = &fw.acr_ucode_parsed.bin_hdr;
        let ucode_sub = &fw.acr_ucode_parsed.raw;
        let hs_header = HsHeader::parse(ucode_sub)?;

        let mut acr_payload = fw.acr_ucode_parsed.payload(&fw.acr_ucode_raw).to_vec();

        // hdr_offset in nvfw_hs_header is FILE-relative (like sig_dbg_offset,
        // sig_prod_offset, etc.), NOT relative to the data payload. For GV100:
        //   header_offset=0x100, data_offset=0x200, hdr_offset=0x148
        // The load header sits within the sub-header region of the file.
        let load_hdr_off = hs_header.hdr_offset as usize;
        let load_hdr_data = if load_hdr_off < fw.acr_ucode_raw.len() {
            &fw.acr_ucode_raw[load_hdr_off..]
        } else {
            return Err(DriverError::DeviceNotFound(
                format!("HS load header offset {load_hdr_off:#x} beyond file size {}", fw.acr_ucode_raw.len()).into(),
            ));
        };
        let load_header = HsLoadHeader::parse(load_hdr_data)?;

        // Patch production signature into ACR payload image.
        // Nouveau: nvkm_falcon_fw_ctor_hs → nvkm_falcon_fw_sign → nvkm_falcon_fw_patch.
        // For 0x10de magic: patch_loc and patch_sig are INDIRECT (file offsets to u32 values).
        let file = &fw.acr_ucode_raw;
        let rd_u32 = |off: usize| -> u32 {
            if off + 4 <= file.len() {
                u32::from_le_bytes([file[off], file[off+1], file[off+2], file[off+3]])
            } else { 0 }
        };

        let sig_patch_loc = rd_u32(hs_header.patch_loc as usize) as usize;
        let sig_adj = rd_u32(hs_header.patch_sig as usize) as usize;
        let sig_src = hs_header.sig_prod_offset as usize + sig_adj;
        let sig_size = hs_header.sig_prod_size as usize;

        if sig_size > 0
            && sig_src + sig_size <= file.len()
            && sig_patch_loc + sig_size <= acr_payload.len()
        {
            acr_payload[sig_patch_loc..sig_patch_loc + sig_size]
                .copy_from_slice(&file[sig_src..sig_src + sig_size]);
            tracing::info!(
                sig_patch_loc, sig_src, sig_size,
                "Patched production signature into ACR payload"
            );
        } else {
            tracing::warn!(
                sig_patch_loc, sig_src, sig_size,
                file_len = file.len(), payload_len = acr_payload.len(),
                "Could not patch signature — offsets out of range"
            );
        }

        tracing::info!(
            bl_code_len = bl_code.len(),
            acr_payload_len = acr_payload.len(),
            ?bl_desc,
            non_sec_code_off = load_header.non_sec_code_off,
            non_sec_code_size = load_header.non_sec_code_size,
            data_off = load_header.data_dma_base,
            data_size = load_header.data_size,
            num_apps = load_header.num_apps,
            "Parsed ACR firmware"
        );

        Ok(Self {
            bl_desc,
            bl_code,
            hs_header,
            load_header,
            acr_payload,
        })
    }
}

// ── ACR DMA chain boot (080b/080c) ──────────────────────────────────

use crate::vfio::dma::DmaBuffer;

/// DMA buffers allocated for the ACR boot chain.
pub struct AcrDmaContext {
    pub acr_ucode: DmaBuffer,
}

/// Build flcn_bl_dmem_desc_v1 for SEC2 BL — tells BL where to find ACR firmware.
///
/// Layout matches `struct flcn_bl_dmem_desc_v1` from Nouveau's `nvfw/flcn.h`.
/// Total size: 4+4+4+4 + 4+4+4+4 + 4 + 8 + 4+4+4+4+4 + 8 + 4 = 76 bytes (packed).
fn build_bl_dmem_desc(
    code_dma_base: u64,
    data_dma_base: u64,
    parsed: &ParsedAcrFirmware,
) -> Vec<u8> {
    let mut desc = vec![0u8; 76];
    let w32 = |buf: &mut [u8], off: usize, val: u32| {
        buf[off..off+4].copy_from_slice(&val.to_le_bytes());
    };
    let w64 = |buf: &mut [u8], off: usize, val: u64| {
        buf[off..off+8].copy_from_slice(&val.to_le_bytes());
    };

    // reserved[4] at 0..16: zeroes
    // signature[4] at 16..32: zeroes
    // ctx_dma at 32: SEC2 uses DMA index 6 (instance-block-translated),
    // NOT index 0 (physical DMA). This is critical — wrong index causes BL
    // to read from VRAM instead of system memory.
    w32(&mut desc, 32, dma_idx::VIRT);
    // code_dma_base at 36 (u64, packed)
    w64(&mut desc, 36, code_dma_base);
    // non_sec_code_off at 44
    w32(&mut desc, 44, parsed.load_header.non_sec_code_off);
    // non_sec_code_size at 48
    w32(&mut desc, 48, parsed.load_header.non_sec_code_size);
    // sec_code_off at 52 (first app code offset)
    let sec_off = parsed.load_header.apps.first().map(|a| a.0).unwrap_or(0);
    w32(&mut desc, 52, sec_off);
    // sec_code_size at 56 (first app code size)
    let sec_size = parsed.load_header.apps.first().map(|a| a.1).unwrap_or(0);
    w32(&mut desc, 56, sec_size);
    // code_entry_point at 60
    w32(&mut desc, 60, 0);
    // data_dma_base at 64 (u64, packed)
    w64(&mut desc, 64, data_dma_base);
    // data_size at 72
    w32(&mut desc, 72, parsed.load_header.data_size);

    desc
}

/// Patch the ACR descriptor within the ACR payload's data section.
///
/// The data section of `ucode_load.bin` contains a `flcn_acr_desc_v1` that
/// must be patched with WPR region addresses before loading. For GP102/GV100:
///
/// `flcn_acr_desc_v1` layout (from Nouveau `nvfw/acr.h`):
///   0x000: reserved_dmem[0x200]  (512 bytes)
///   0x200: signatures[4]          (16 bytes)
///   0x210: wpr_region_id          (u32)
///   0x214: wpr_offset             (u32)
///   0x218: mmu_memory_range       (u32)
///   0x21C: regions.no_regions     (u32)
///   0x220: region_props[0].start_addr  (u32, addr >> 8)
///   0x224: region_props[0].end_addr    (u32, addr >> 8)
///   0x228: region_props[0].region_id   (u32)
///   0x22C: region_props[0].read_mask   (u32)
///   0x230: region_props[0].write_mask  (u32)
///   0x234: region_props[0].client_mask (u32)
///   0x238: region_props[0].shadow_mem_start_addr (u32, addr >> 8)
///   0x23C: region_props[1]  (28 bytes, left zeroed)
///   0x258: ucode_blob_size  (u32)
///   0x260: ucode_blob_base  (u64, 8-byte aligned)
fn patch_acr_desc(payload: &mut [u8], data_off: usize, wpr_start: u64, wpr_end: u64) {
    let needed = data_off + 0x268;
    if needed > payload.len() {
        tracing::warn!(
            data_off, payload_len = payload.len(), needed,
            "ACR data section too small for v1 descriptor patch"
        );
        return;
    }
    let w32 = |buf: &mut [u8], off: usize, val: u32| {
        buf[off..off+4].copy_from_slice(&val.to_le_bytes());
    };
    let w64 = |buf: &mut [u8], off: usize, val: u64| {
        buf[off..off+8].copy_from_slice(&val.to_le_bytes());
    };
    let base = data_off;

    w32(payload, base + 0x210, 1);                              // wpr_region_id
    w32(payload, base + 0x21C, 2);                              // no_regions
    w32(payload, base + 0x220, (wpr_start >> 8) as u32);        // region[0].start_addr
    w32(payload, base + 0x224, (wpr_end >> 8) as u32);          // region[0].end_addr
    w32(payload, base + 0x228, 1);                              // region[0].region_id
    w32(payload, base + 0x22C, 0xF);                            // region[0].read_mask
    w32(payload, base + 0x230, 0xC);                            // region[0].write_mask
    w32(payload, base + 0x234, 0x2);                            // region[0].client_mask
    w32(payload, base + 0x238, (wpr_start >> 8) as u32);        // region[0].shadow_mem_start

    // ucode_blob_base/size: point ACR at the entire WPR region
    let wpr_size = wpr_end - wpr_start;
    w32(payload, base + 0x258, wpr_size as u32);                // ucode_blob_size
    w64(payload, base + 0x260, wpr_start);                      // ucode_blob_base
}

/// Build a complete WPR (Write-Protected Region) containing FECS and GPCCS
/// firmware for ACR verification and bootstrap.
///
/// Layout matches Nouveau's `gp102_acr_wpr_layout` + `gp102_acr_wpr_build`:
///   [0..264]     wpr_header_v1 array (11 max entries × 24B)
///   [264..512]   padding (ALIGN to 256)
///   [512..768]   shared sub-WPR headers (0x100 bytes, zeros)
///   [768..]      per-falcon: LSB header (240B) → image (4K-aligned) → BLD (256B)
///
/// Returns `(wpr_data, wpr_size)` — the serialized WPR and its total size.
pub fn build_wpr(fw: &AcrFirmwareSet, wpr_vram_base: u64) -> Vec<u8> {
    let align = |v: usize, a: usize| (v + a - 1) & !(a - 1);
    let w32 = |buf: &mut Vec<u8>, off: usize, val: u32| {
        if off + 4 > buf.len() { buf.resize(off + 4, 0); }
        buf[off..off+4].copy_from_slice(&val.to_le_bytes());
    };

    // Build per-falcon images: bl_bytes + inst_bytes + data_bytes
    let fecs_img = [fw.fecs_bl.as_slice(), fw.fecs_inst.as_slice(), fw.fecs_data.as_slice()].concat();
    let gpccs_img = [fw.gpccs_bl.as_slice(), fw.gpccs_inst.as_slice(), fw.gpccs_data.as_slice()].concat();

    // Phase 1: compute layout offsets (gp102_acr_wpr_layout)
    let mut wpr: usize = 0;

    // Header table: 11 entries × 24 bytes, aligned to 256
    wpr += 11 * 24;
    wpr = align(wpr, 256);
    // Shared sub-WPR headers
    wpr += 0x100;

    // FECS
    wpr = align(wpr, 256);
    let fecs_lsb_off = wpr;
    wpr += 240; // sizeof(lsb_header_v1)

    wpr = align(wpr, 4096);
    let fecs_img_off = wpr;
    wpr += fecs_img.len();

    wpr = align(wpr, 256);
    let fecs_bld_off = wpr;
    wpr += 256; // ALIGN(sizeof(flcn_bl_dmem_desc_v2), 256)

    // GPCCS
    wpr = align(wpr, 256);
    let gpccs_lsb_off = wpr;
    wpr += 240;

    wpr = align(wpr, 4096);
    let gpccs_img_off = wpr;
    wpr += gpccs_img.len();

    wpr = align(wpr, 256);
    let gpccs_bld_off = wpr;
    wpr += 256;

    let wpr_total = wpr;
    let mut buf = vec![0u8; wpr_total];

    // Phase 2: write WPR headers (gp102_acr_wpr_build)
    // FECS header at offset 0
    w32(&mut buf, 0, falcon_id::FECS);           // falcon_id
    w32(&mut buf, 4, fecs_lsb_off as u32);       // lsb_offset
    w32(&mut buf, 8, falcon_id::SEC2);            // bootstrap_owner
    w32(&mut buf, 12, 0);                         // lazy_bootstrap = FALSE (auto-boot)
    w32(&mut buf, 16, 0);                         // bin_version
    w32(&mut buf, 20, 1);                         // status = WPR_HEADER_V1_STATUS_COPY

    // GPCCS header at offset 24
    w32(&mut buf, 24, falcon_id::GPCCS);
    w32(&mut buf, 28, gpccs_lsb_off as u32);
    w32(&mut buf, 32, falcon_id::SEC2);
    w32(&mut buf, 36, 0);                         // lazy_bootstrap = FALSE
    w32(&mut buf, 40, 0);                         // bin_version
    w32(&mut buf, 44, 1);                         // status = COPY

    // Sentinel at offset 48
    w32(&mut buf, 48, falcon_id::INVALID);

    // Phase 3: write LSB headers + images + BLDs
    // Helper: write LSB header for a falcon
    let write_lsb = |buf: &mut Vec<u8>, lsb_off: usize, img_off: usize, bld_off: usize,
                      sig: &[u8], bl_size: usize, inst_size: usize, data_size: usize,
                      fid: u32| {
        // Copy signature (up to 192 bytes from sig file)
        let sig_len = sig.len().min(192);
        buf[lsb_off..lsb_off + sig_len].copy_from_slice(&sig[..sig_len]);
        // Populate lsf_ucode_desc_v1 metadata fields within the signature area.
        // The sig file only contains crypto keys (bytes 0-63). Fields at 64+
        // must be filled by the host (Nouveau does this in build_bld_desc).
        if sig_len < 112 {
            buf.resize(buf.len().max(lsb_off + 112), 0);
        }
        let s = lsb_off;
        // b_prd_present (offset 64) — already set in sig file
        // b_dbg_present (offset 68) — already set in sig file
        buf[s + 72..s + 76].copy_from_slice(&fid.to_le_bytes());          // falcon_id
        buf[s + 76..s + 80].copy_from_slice(&1u32.to_le_bytes());         // bsupported = 1
        // status (offset 80) — keep from sig file
        // elf_section_names_idx (offset 84) — 0
        buf[s + 88..s + 92].copy_from_slice(&(bl_size as u32).to_le_bytes());  // app_resident_code_off
        buf[s + 92..s + 96].copy_from_slice(&(inst_size as u32).to_le_bytes()); // app_resident_code_size
        buf[s + 96..s + 100].copy_from_slice(&((bl_size + inst_size) as u32).to_le_bytes()); // app_resident_data_off
        buf[s + 100..s + 104].copy_from_slice(&(data_size as u32).to_le_bytes()); // app_resident_data_size

        // LSB tail starts at lsb_off + 192
        let t = lsb_off + 192;
        let img_size = bl_size + inst_size + data_size;
        // ucode_off: offset of image relative to WPR base
        buf[t..t+4].copy_from_slice(&(img_off as u32).to_le_bytes());
        // ucode_size
        buf[t+4..t+8].copy_from_slice(&(img_size as u32).to_le_bytes());
        // data_size
        buf[t+8..t+12].copy_from_slice(&(data_size as u32).to_le_bytes());
        // bl_code_size
        buf[t+12..t+16].copy_from_slice(&(bl_size as u32).to_le_bytes());
        // bl_imem_off = 0 (BL loads at IMEM 0)
        buf[t+16..t+20].copy_from_slice(&0u32.to_le_bytes());
        // bl_data_off = bld_off relative to WPR
        buf[t+20..t+24].copy_from_slice(&(bld_off as u32).to_le_bytes());
        // bl_data_size = 256
        buf[t+24..t+28].copy_from_slice(&256u32.to_le_bytes());
        // app_code_off = bl_size (app code follows BL in image)
        buf[t+28..t+32].copy_from_slice(&(bl_size as u32).to_le_bytes());
        // app_code_size = inst_size
        buf[t+32..t+36].copy_from_slice(&(inst_size as u32).to_le_bytes());
        // app_data_off = bl_size + inst_size
        buf[t+36..t+40].copy_from_slice(&((bl_size + inst_size) as u32).to_le_bytes());
        // app_data_size = data_size
        buf[t+40..t+44].copy_from_slice(&(data_size as u32).to_le_bytes());
        // flags = 0
        buf[t+44..t+48].copy_from_slice(&0u32.to_le_bytes());
    };

    // Write FECS LSB + image + BLD
    write_lsb(
        &mut buf, fecs_lsb_off, fecs_img_off, fecs_bld_off,
        &fw.fecs_sig, fw.fecs_bl.len(), fw.fecs_inst.len(), fw.fecs_data.len(),
        falcon_id::FECS,
    );
    buf[fecs_img_off..fecs_img_off + fecs_img.len()].copy_from_slice(&fecs_img);

    // FECS BLD (flcn_bl_dmem_desc_v2): point code/data at WPR-relative addresses
    let fecs_code_dma = wpr_vram_base + fecs_img_off as u64;
    let fecs_data_dma = wpr_vram_base + fecs_img_off as u64 + fw.fecs_bl.len() as u64 + fw.fecs_inst.len() as u64;
    write_bl_dmem_desc_v2(
        &mut buf, fecs_bld_off, fecs_code_dma, fecs_data_dma,
        0, fw.fecs_bl.len() as u32,
        fw.fecs_bl.len() as u32, fw.fecs_inst.len() as u32,
        0, fw.fecs_data.len() as u32,
    );

    // Write GPCCS LSB + image + BLD
    write_lsb(
        &mut buf, gpccs_lsb_off, gpccs_img_off, gpccs_bld_off,
        &fw.gpccs_sig, fw.gpccs_bl.len(), fw.gpccs_inst.len(), fw.gpccs_data.len(),
        falcon_id::GPCCS,
    );
    buf[gpccs_img_off..gpccs_img_off + gpccs_img.len()].copy_from_slice(&gpccs_img);

    let gpccs_code_dma = wpr_vram_base + gpccs_img_off as u64;
    let gpccs_data_dma = wpr_vram_base + gpccs_img_off as u64 + fw.gpccs_bl.len() as u64 + fw.gpccs_inst.len() as u64;
    write_bl_dmem_desc_v2(
        &mut buf, gpccs_bld_off, gpccs_code_dma, gpccs_data_dma,
        0, fw.gpccs_bl.len() as u32,
        fw.gpccs_bl.len() as u32, fw.gpccs_inst.len() as u32,
        0, fw.gpccs_data.len() as u32,
    );

    tracing::info!(
        wpr_size = wpr_total,
        fecs_lsb = fecs_lsb_off,
        fecs_img = fecs_img_off,
        fecs_img_size = fecs_img.len(),
        fecs_bld = fecs_bld_off,
        gpccs_lsb = gpccs_lsb_off,
        gpccs_img = gpccs_img_off,
        gpccs_img_size = gpccs_img.len(),
        gpccs_bld = gpccs_bld_off,
        "WPR layout"
    );

    buf
}

/// Write a `flcn_bl_dmem_desc_v2` (84 bytes packed, padded to 256) into `buf` at `off`.
fn write_bl_dmem_desc_v2(
    buf: &mut [u8], off: usize,
    code_dma_base: u64, data_dma_base: u64,
    non_sec_code_off: u32, non_sec_code_size: u32,
    sec_code_off: u32, sec_code_size: u32,
    code_entry_point: u32, data_size: u32,
) {
    let w32 = |buf: &mut [u8], o: usize, v: u32| buf[o..o+4].copy_from_slice(&v.to_le_bytes());
    let w64 = |buf: &mut [u8], o: usize, v: u64| buf[o..o+8].copy_from_slice(&v.to_le_bytes());

    // reserved[4] at 0..16: zeros
    // signature[4] at 16..32: zeros
    w32(buf, off + 32, 0);                        // ctx_dma = 0
    w64(buf, off + 36, code_dma_base);
    w32(buf, off + 44, non_sec_code_off);
    w32(buf, off + 48, non_sec_code_size);
    w32(buf, off + 52, sec_code_off);
    w32(buf, off + 56, sec_code_size);
    w32(buf, off + 60, code_entry_point);
    w64(buf, off + 64, data_dma_base);
    w32(buf, off + 72, data_size);
    // argc=0, argv=0 at 76, 80
}

/// Upload code to falcon IMEM matching Nouveau's per-256B-chunk protocol.
///
/// Nouveau re-initializes IMEMC for each 256-byte page. This is critical —
/// auto-increment may not cross page boundaries on all falcon versions.
fn falcon_imem_upload_nouveau(
    bar0: &MappedBar,
    base: usize,
    imem_addr: u32,
    data: &[u8],
    start_tag: u32,
) {
    let w = |off: usize, val: u32| { let _ = bar0.write_u32(base + off, val); };

    for (chunk_idx, chunk) in data.chunks(256).enumerate() {
        let chunk_addr = imem_addr + (chunk_idx as u32) * 256;
        let chunk_tag = start_tag + chunk_idx as u32;

        // Re-init IMEMC for this page (Nouveau: gm200_flcn_pio_imem_wr_init)
        // BIT(24) = auto-increment within page. No secure flag (sec=false in Nouveau).
        w(falcon::IMEMC, (1u32 << 24) | chunk_addr);

        // Set tag for this page (Nouveau: gm200_flcn_pio_imem_wr)
        w(falcon::IMEMT, chunk_tag as u32);

        // Write data words
        for word_chunk in chunk.chunks(4) {
            let word = match word_chunk.len() {
                4 => u32::from_le_bytes([word_chunk[0], word_chunk[1], word_chunk[2], word_chunk[3]]),
                3 => u32::from_le_bytes([word_chunk[0], word_chunk[1], word_chunk[2], 0]),
                2 => u32::from_le_bytes([word_chunk[0], word_chunk[1], 0, 0]),
                1 => u32::from_le_bytes([word_chunk[0], 0, 0, 0]),
                _ => 0,
            };
            w(falcon::IMEMD, word);
        }

        // Pad remainder of 256-byte page with zeroes
        let written = (chunk.len().div_ceil(4)) * 4;
        let remainder = written & 0xFF;
        if remainder != 0 {
            let pad_words = (256 - remainder) / 4;
            for _ in 0..pad_words {
                w(falcon::IMEMD, 0);
            }
        }
    }
}

/// Upload data to falcon DMEM matching Nouveau's protocol.
/// Read SEC2 DMEM contents via PIO.
fn sec2_dmem_read(bar0: &MappedBar, offset: u32, len: usize) -> Vec<u32> {
    let base = falcon::SEC2_BASE;
    let w = |off: usize, val: u32| { let _ = bar0.write_u32(base + off, val); };
    let r = |off: usize| bar0.read_u32(base + off).unwrap_or(0xDEAD_DEAD);

    // DMEMC: BIT(25) = read mode with auto-increment
    w(falcon::DMEMC, (1u32 << 25) | offset);

    let words = (len + 3) / 4;
    let mut result = Vec::with_capacity(words);
    for _ in 0..words {
        result.push(r(falcon::DMEMD));
    }
    result
}

fn falcon_dmem_upload(bar0: &MappedBar, base: usize, dmem_addr: u32, data: &[u8]) {
    let w = |off: usize, val: u32| { let _ = bar0.write_u32(base + off, val); };

    // DMEMC: BIT(24) = write mode with auto-increment
    w(falcon::DMEMC, (1u32 << 24) | dmem_addr);

    for chunk in data.chunks(4) {
        let word = match chunk.len() {
            4 => u32::from_le_bytes([chunk[0], chunk[1], chunk[2], chunk[3]]),
            3 => u32::from_le_bytes([chunk[0], chunk[1], chunk[2], 0]),
            2 => u32::from_le_bytes([chunk[0], chunk[1], 0, 0]),
            1 => u32::from_le_bytes([chunk[0], 0, 0, 0]),
            _ => 0,
        };
        w(falcon::DMEMD, word);
    }
}

/// Full ACR chain boot: DMA-backed SEC2 → ACR → FECS/GPCCS.
///
/// Implements the complete Nouveau-compatible boot chain:
/// 1. Parse firmware blobs (bl.bin, ucode_load.bin)
/// 2. Allocate DMA buffer for ACR firmware payload
/// 3. Patch ACR descriptor with WPR addresses (placeholder for now)
/// VRAM-based ACR boot: write the ACR payload into VRAM via PRAMIN, then
/// have the BL DMA-load it from VRAM using physical addressing.
///
/// Insight: the falcon's physical DMA mode (0x624 | 0x80) reads from GPU
/// VRAM, not from system memory. Previous strategies failed because the BL
/// tried to `xcld` from an IOVA (system memory address) that the falcon
/// couldn't reach. By placing the ACR payload in VRAM and pointing the BL
/// descriptor at VRAM addresses, the DMA path stays entirely on-GPU.
pub fn attempt_vram_acr_boot(bar0: &MappedBar, fw: &AcrFirmwareSet) -> AcrBootResult {
    use crate::vfio::memory::{MemoryRegion, PraminRegion};

    let mut notes = Vec::new();
    let sec2_before = Sec2Probe::capture(bar0);
    let base = falcon::SEC2_BASE;
    let r = |off: usize| bar0.read_u32(base + off).unwrap_or(0xDEAD);
    let w = |off: usize, val: u32| { let _ = bar0.write_u32(base + off, val); };

    notes.push(format!("SEC2 state: {:?}", sec2_before.state));

    // ── Step 1: Parse firmware ──
    let parsed = match ParsedAcrFirmware::parse(fw) {
        Ok(p) => p,
        Err(e) => {
            notes.push(format!("Firmware parse failed: {e}"));
            return make_fail_result("VRAM ACR: parse failed", sec2_before, bar0, notes);
        }
    };

    let acr_payload = &parsed.acr_payload;
    let payload_size = acr_payload.len();
    notes.push(format!(
        "ACR payload: {}B non_sec=[{:#x}+{:#x}] data=[{:#x}+{:#x}]",
        payload_size,
        parsed.load_header.non_sec_code_off, parsed.load_header.non_sec_code_size,
        parsed.load_header.data_dma_base, parsed.load_header.data_size,
    ));

    // ── Step 2: Write ACR payload to VRAM via PRAMIN ──
    // Use VRAM offset 0x50000 — above diagnostic areas, within a single 64KB window.
    let vram_base: u32 = 0x0005_0000;
    let vram_sentinel_ok;

    // First verify VRAM is accessible with a sentinel test
    match PraminRegion::new(bar0, vram_base, 8) {
        Ok(mut region) => {
            let sentinel = 0xACB0_0700_u32;
            if let Err(e) = region.write_u32(0, sentinel) {
                notes.push(format!("VRAM sentinel write failed: {e}"));
                vram_sentinel_ok = false;
            } else {
                let rb = region.read_u32(0).unwrap_or(0);
                vram_sentinel_ok = rb == sentinel;
                notes.push(format!(
                    "VRAM@{vram_base:#x} sentinel: wrote={sentinel:#010x} read={rb:#010x} ok={vram_sentinel_ok}"
                ));
            }
        }
        Err(e) => {
            notes.push(format!("PRAMIN region create failed: {e}"));
            vram_sentinel_ok = false;
        }
    }

    if !vram_sentinel_ok {
        notes.push("VRAM not accessible — cannot use VRAM ACR path".to_string());
        return make_fail_result("VRAM ACR: VRAM inaccessible", sec2_before, bar0, notes);
    }

    // ── Step 2a: Build WPR with FECS/GPCCS firmware ──
    let wpr_vram_base: u32 = 0x0007_0000; // WPR starts at VRAM 0x70000
    let wpr_data = build_wpr(fw, wpr_vram_base as u64);
    let wpr_end = wpr_vram_base as u64 + wpr_data.len() as u64;
    notes.push(format!(
        "WPR: {}B at VRAM@{wpr_vram_base:#x}..{wpr_end:#x}",
        wpr_data.len()
    ));

    // ── Step 2b: Patch ACR descriptor in payload with WPR bounds ──
    let mut payload_patched = acr_payload.to_vec();
    let data_off = parsed.load_header.data_dma_base as usize;
    patch_acr_desc(
        &mut payload_patched, data_off,
        wpr_vram_base as u64, wpr_end,
    );
    notes.push(format!(
        "ACR desc patched: wpr_start={wpr_vram_base:#x} wpr_end={wpr_end:#x} at data_off={data_off:#x}"
    ));

    // ── Step 2c: Write ACR payload to VRAM ──
    let write_to_vram = |bar0: &MappedBar, vram_addr: u32, data: &[u8], notes: &mut Vec<String>| -> bool {
        let mut off = 0usize;
        while off < data.len() {
            let chunk_vram = vram_addr + off as u32;
            let chunk_size = (data.len() - off).min(0xC000);
            match PraminRegion::new(bar0, chunk_vram, chunk_size) {
                Ok(mut region) => {
                    for word_off in (0..chunk_size).step_by(4) {
                        let src = off + word_off;
                        if src >= data.len() { break; }
                        let end = (src + 4).min(data.len());
                        let mut bytes = [0u8; 4];
                        bytes[..end - src].copy_from_slice(&data[src..end]);
                        let val = u32::from_le_bytes(bytes);
                        if region.write_u32(word_off, val).is_err() {
                            notes.push(format!("VRAM write failed at {chunk_vram:#x}+{word_off:#x}"));
                            return false;
                        }
                    }
                    off += chunk_size;
                }
                Err(e) => {
                    notes.push(format!("PRAMIN at VRAM@{chunk_vram:#x}: {e}"));
                    return false;
                }
            }
        }
        true
    };

    if !write_to_vram(bar0, vram_base, &payload_patched, &mut notes) {
        return make_fail_result("VRAM ACR: payload write failed", sec2_before, bar0, notes);
    }
    notes.push(format!("ACR payload: {}B → VRAM@{vram_base:#x}", payload_patched.len()));

    // ── Step 2d: Write WPR to VRAM ──
    if !write_to_vram(bar0, wpr_vram_base, &wpr_data, &mut notes) {
        return make_fail_result("VRAM ACR: WPR write failed", sec2_before, bar0, notes);
    }
    notes.push(format!("WPR: {}B → VRAM@{wpr_vram_base:#x}", wpr_data.len()));

    // Verify ACR payload in VRAM
    if let Ok(region) = PraminRegion::new(bar0, vram_base, 16) {
        let v0 = region.read_u32(0).unwrap_or(0);
        let e0 = u32::from_le_bytes([payload_patched[0], payload_patched[1], payload_patched[2], payload_patched[3]]);
        notes.push(format!("VRAM ACR verify: [{v0:#010x}] expected [{e0:#010x}] ok={}", v0 == e0));
    }

    // ── Step 2e: Build VRAM instance block for falcon DMA (before reset) ──
    let inst_ok = build_vram_falcon_inst_block(bar0);

    // Verify page tables by reading back critical entries
    let rv = |vram_addr: u32, offset: usize| -> u32 {
        PraminRegion::new(bar0, vram_addr, offset + 4)
            .ok()
            .and_then(|r| r.read_u32(offset).ok())
            .unwrap_or(0xDEAD)
    };
    let pdb = rv(FALCON_INST_VRAM, 0x200);
    let pdb_hi = rv(FALCON_INST_VRAM, 0x204);
    let pt112_lo = rv(FALCON_PT0_VRAM, 112 * 8);
    let pt112_hi = rv(FALCON_PT0_VRAM, 112 * 8 + 4);
    notes.push(format!(
        "VRAM inst: built={inst_ok} PDB@0x200={pdb:#010x}:{pdb_hi:#010x} PT[112]={pt112_lo:#010x}:{pt112_hi:#010x}"
    ));

    // ── Step 3: Inline engine reset with 0x668 race timing ──
    let inst_bind_val = FALCON_INST_VRAM >> 12;

    // 3a: Falcon-local reset pulse
    w(0x3C0, 0x01);
    // RACE A: Write 0x668 during reset (falcon logic disabled)
    w(SEC2_FLCN_BIND_INST, inst_bind_val);
    let rba = r(SEC2_FLCN_BIND_INST);
    let sctl_a = r(falcon::SCTL);
    std::thread::sleep(std::time::Duration::from_micros(10));

    w(0x3C0, 0x00);
    // RACE B: Write 0x668 immediately after reset clear
    w(SEC2_FLCN_BIND_INST, inst_bind_val);
    let rbb = r(SEC2_FLCN_BIND_INST);
    let sctl_b = r(falcon::SCTL);

    // 3b: PMC enable
    let _ = pmc_enable_sec2(bar0);

    // RACE C: Write 0x668 immediately after PMC enable (no other ops)
    w(SEC2_FLCN_BIND_INST, inst_bind_val);
    w(falcon::DMACTL, 0x07);
    let rbc = r(SEC2_FLCN_BIND_INST);
    let sctl_c = r(falcon::SCTL);
    let dmactl_c = r(falcon::DMACTL);

    // 3c: Mailbox mask + scrub wait
    let _ = bar0.read_u32(base + falcon::MAILBOX0);
    let scrub_start = std::time::Instant::now();
    loop {
        let scrub = r(falcon::DMACTL);
        if scrub & 0x06 == 0 { break; }
        if scrub_start.elapsed() > std::time::Duration::from_millis(100) { break; }
        std::thread::sleep(std::time::Duration::from_micros(100));
    }

    // RACE D: Write 0x668 immediately after scrub completes
    w(SEC2_FLCN_BIND_INST, inst_bind_val);
    let rbd = r(SEC2_FLCN_BIND_INST);
    let sctl_d = r(falcon::SCTL);

    // 3d: Write BOOT_0
    let boot0 = bar0.read_u32(0x000).unwrap_or(0);
    w(0x084, boot0);

    notes.push(format!(
        "0x668 race: A(reset)={rba:#x}/sctl={sctl_a:#x} B(clear)={rbb:#x}/sctl={sctl_b:#x} C(pmc)={rbc:#x}/sctl={sctl_c:#x}/dma={dmactl_c:#x} D(scrub)={rbd:#x}/sctl={sctl_d:#x}"
    ));

    let bind_ok = rba != 0 || rbb != 0 || rbc != 0 || rbd != 0;
    if bind_ok {
        notes.push("0x668 ACCEPTED at one of the race windows!".to_string());
    }

    // ── Step 4: Configure DMA — match Nouveau's 0x624 = 0x190 ──
    // Nouveau shows 0x624=0x190 (bits 4,7,8). Our previous value was 0x090 (bits 4,7).
    // Bit 8 (0x100) likely enables instance block DMA path alongside physical DMA.
    let fbif_before = r(0x624);
    w(0x624, 0x190); // Match Nouveau: physical DMA + instance block DMA
    let fbif_after = r(0x624);
    notes.push(format!(
        "0x624: was={fbif_before:#010x} wrote=0x190 now={fbif_after:#010x}"
    ));

    w(falcon::DMACTL, 0x07);
    let dmactl_after = r(falcon::DMACTL);
    notes.push(format!("DMACTL: wrote=0x07 after={dmactl_after:#010x}"));

    // ── Step 5: Load BL code → IMEM ──
    let hwcfg = r(falcon::HWCFG);
    let code_limit = falcon::imem_size_bytes(hwcfg);
    let boot_size = ((parsed.bl_desc.bl_code_off + parsed.bl_desc.bl_code_size + 0xFF) & !0xFF) as u32;
    let imem_addr = code_limit.saturating_sub(boot_size);
    let start_tag = parsed.bl_desc.bl_start_tag;
    let boot_addr = start_tag << 8;

    falcon_imem_upload_nouveau(bar0, base, imem_addr, &parsed.bl_code, start_tag);
    notes.push(format!(
        "BL code: {}B → IMEM@{imem_addr:#x} tag={start_tag:#x} boot_addr={boot_addr:#x}",
        parsed.bl_code.len()
    ));

    // ── Step 6: Pre-load ACR data section + BL descriptor to DMEM ──
    // The BL's data xcld (from VRAM) may fail. To ensure the ACR finds its
    // flcn_acr_desc_v1 in DMEM, we pre-load the patched data section first,
    // then overlay the BL descriptor on top. The BL descriptor (76 bytes)
    // falls entirely within the reserved_dmem[512] area that the ACR ignores.
    let data_section = &payload_patched[parsed.load_header.data_dma_base as usize..];
    falcon_dmem_upload(bar0, base, 0, data_section);
    notes.push(format!(
        "ACR data pre-loaded: {}B → DMEM@0 (includes patched desc at 0x210+)",
        data_section.len()
    ));

    let code_dma_base = vram_base as u64;
    let data_dma_base = vram_base as u64 + parsed.load_header.data_dma_base as u64;
    let bl_desc = build_bl_dmem_desc(code_dma_base, data_dma_base, &parsed);
    // Overlay BL descriptor at DMEM@0 (first 76 bytes, within reserved_dmem)
    falcon_dmem_upload(bar0, base, 0, &bl_desc);
    notes.push(format!(
        "BL desc overlay: {}B → DMEM@0 (code={code_dma_base:#x} data={data_dma_base:#x})",
        bl_desc.len()
    ));

    // ── Step 7: Boot SEC2 ──
    w(falcon::MAILBOX0, 0);
    w(falcon::MAILBOX1, 0);
    w(falcon::BOOTVEC, boot_addr);
    let cpuctl_pre = r(falcon::CPUCTL);
    notes.push(format!(
        "BOOTVEC={boot_addr:#x} cpuctl={cpuctl_pre:#010x}, issuing STARTCPU"
    ));
    falcon_start_cpu(bar0, base);

    // ── Step 7b: Aggressively write DMACTL and 0x668 during BL execution ──
    // The BL runs for a few microseconds. During this window, the HS
    // security context might be different, allowing register writes.
    let mut dmactl_best = 0u32;
    let mut bind_best = 0u32;
    for _ in 0..100 {
        w(falcon::DMACTL, 0x07);
        w(SEC2_FLCN_BIND_INST, inst_bind_val);
        let dc = r(falcon::DMACTL);
        let bi = r(SEC2_FLCN_BIND_INST);
        if dc > dmactl_best { dmactl_best = dc; }
        if bi > bind_best { bind_best = bi; }
        std::thread::sleep(std::time::Duration::from_micros(10));
    }
    notes.push(format!(
        "Post-boot DMACTL race: best={dmactl_best:#010x} bind_best={bind_best:#010x}"
    ));

    // ── Step 8: Poll with PC sampling ──
    let timeout = std::time::Duration::from_secs(5);
    let start_time = std::time::Instant::now();
    let mut pc_samples = Vec::new();
    let mut last_pc = 0u32;
    // Phase 1: Aggressive tracing (100μs intervals) to catch execution flow
    let mut all_pcs: Vec<u32> = Vec::new();
    for _ in 0..500 {
        let pc = r(0x030);
        if all_pcs.last() != Some(&pc) {
            all_pcs.push(pc);
        }
        std::thread::sleep(std::time::Duration::from_micros(100));
        if start_time.elapsed().as_millis() > 50 { break; }
    }

    // Phase 2: Normal settling (5ms intervals)
    let mut settled_count = 0u32;
    loop {
        std::thread::sleep(std::time::Duration::from_millis(5));
        let cpuctl = r(falcon::CPUCTL);
        let mb0 = r(falcon::MAILBOX0);
        let mb1 = r(falcon::MAILBOX1);
        let pc = r(0x030);

        if pc != last_pc {
            pc_samples.push(format!("{:#06x}@{}ms", pc, start_time.elapsed().as_millis()));
            last_pc = pc;
            settled_count = 0;
        } else {
            settled_count += 1;
        }

        let halted = cpuctl & falcon::CPUCTL_HALTED != 0;
        let hreset_back = cpuctl & falcon::CPUCTL_HRESET != 0;

        if mb0 != 0 || halted || hreset_back {
            notes.push(format!(
                "SEC2 response: cpuctl={cpuctl:#010x} mb0={mb0:#010x} mb1={mb1:#010x} pc={pc:#010x} ({}ms)",
                start_time.elapsed().as_millis()
            ));
            break;
        }
        if settled_count > 200 {
            notes.push(format!(
                "SEC2 settled at pc={pc:#010x} cpuctl={cpuctl:#010x} mb0={mb0:#010x} mb1={mb1:#010x} ({}ms)",
                start_time.elapsed().as_millis()
            ));
            break;
        }
        if start_time.elapsed() > timeout {
            notes.push(format!(
                "SEC2 timeout: cpuctl={cpuctl:#010x} mb0={mb0:#010x} mb1={mb1:#010x} pc={pc:#010x}"
            ));
            break;
        }
    }
    if !all_pcs.is_empty() {
        let pc_trace: Vec<String> = all_pcs.iter().map(|p| format!("{p:#06x}")).collect();
        notes.push(format!("Fast PC trace (100μs): [{}]", pc_trace.join(" → ")));
    }
    if !pc_samples.is_empty() {
        notes.push(format!("PC progression: [{}]", pc_samples.join(", ")));
    }

    // ── DMEM diagnostic: read ACR-relevant regions ──
    // First 256B: BL descriptor area
    let dmem_lo = sec2_dmem_read(bar0, 0, 256);
    let lo_vals: Vec<String> = dmem_lo.iter().enumerate()
        .filter(|&(_, &w)| w != 0 && w != 0xDEAD_DEAD)
        .map(|(i, &w)| format!("[{:#05x}]={w:#010x}", i * 4))
        .collect();
    if !lo_vals.is_empty() {
        notes.push(format!("DMEM[0..0x100]: {}", lo_vals.join(" ")));
    }
    // ACR descriptor region: 0x200-0x270 (signatures + wpr_region_id + regions)
    let dmem_acr = sec2_dmem_read(bar0, 0x200, 0x70);
    let acr_vals: Vec<String> = dmem_acr.iter().enumerate()
        .filter(|&(_, &w)| w != 0 && w != 0xDEAD_DEAD)
        .map(|(i, &w)| format!("[{:#05x}]={w:#010x}", 0x200 + i * 4))
        .collect();
    notes.push(format!("DMEM[0x200..0x270]: {}", if acr_vals.is_empty() { "ALL ZERO".to_string() } else { acr_vals.join(" ") }));

    // DMA config and transfer registers after ACR settles
    let dma_624 = r(0x624);
    let dma_10c = r(falcon::DMACTL);
    let dma_668 = r(SEC2_FLCN_BIND_INST);
    let dmatrfbase = r(0x110);
    let dmatrfmoffs = r(0x114);
    let dmatrfcmd = r(0x118);
    let dmatrffboffs = r(0x11C);
    notes.push(format!(
        "DMA: 0x624={dma_624:#010x} DMACTL={dma_10c:#010x} 0x668={dma_668:#010x}"
    ));
    notes.push(format!(
        "DMA xfer: base={dmatrfbase:#010x} moffs={dmatrfmoffs:#010x} cmd={dmatrfcmd:#010x} fboffs={dmatrffboffs:#010x}"
    ));
    // Check EXCI (exception info) for any trapped errors
    let exci = r(falcon::EXCI);
    let tracepc2 = r(0x034); // TRACEPC[1] — previous PC sample
    let tracepc3 = r(0x038); // TRACEPC[2]
    let tracepc4 = r(0x03C); // TRACEPC[3]
    notes.push(format!(
        "Diag: EXCI={exci:#010x} TRACEPC[0..3]=[{:#06x}, {:#06x}, {:#06x}, {:#06x}]",
        r(0x030), tracepc2, tracepc3, tracepc4
    ));

    // Check WPR header status in VRAM — did ACR modify it?
    // wpr_header_v1[0].status is at WPR+20, [1].status at WPR+44
    if let Ok(region) = PraminRegion::new(bar0, wpr_vram_base, 64) {
        let fecs_fid = region.read_u32(0).unwrap_or(0);
        let fecs_status = region.read_u32(20).unwrap_or(0);
        let gpccs_fid = region.read_u32(24).unwrap_or(0);
        let gpccs_status = region.read_u32(44).unwrap_or(0);
        let sentinel = region.read_u32(48).unwrap_or(0);
        let status_name = |s: u32| match s {
            0 => "NONE", 1 => "COPY", 2 => "CODE_FAIL", 3 => "DATA_FAIL",
            4 => "DONE", 5 => "SKIPPED", 6 => "READY", 7 => "REVOKE_FAIL",
            _ => "UNKNOWN"
        };
        notes.push(format!(
            "WPR hdrs: FECS(id={fecs_fid}) status={fecs_status}({}), GPCCS(id={gpccs_fid}) status={gpccs_status}({}), sentinel={sentinel:#x}",
            status_name(fecs_status), status_name(gpccs_status)
        ));
    }

    // Read DMEM@0xB00 (non-zero region seen in all runs)
    let dmem_b00 = sec2_dmem_read(bar0, 0xB00, 0x20);
    let b00_vals: Vec<String> = dmem_b00.iter().enumerate()
        .map(|(i, &w)| format!("[{:#05x}]={w:#010x}", 0xB00 + i * 4))
        .collect();
    notes.push(format!("DMEM[0xB00..0xB20]: {}", b00_vals.join(" ")));

    // Also read DMEM around 0xD00-0xE00 for potential error codes
    let dmem_hi = sec2_dmem_read(bar0, 0xD00, 0x100);
    let hi_vals: Vec<String> = dmem_hi.iter().enumerate()
        .filter(|&(_, &w)| w != 0 && w != 0xDEAD_DEAD)
        .map(|(i, &w)| format!("[{:#05x}]={w:#010x}", 0xD00 + i * 4))
        .collect();
    if !hi_vals.is_empty() {
        notes.push(format!("DMEM[0xD00..0xE00]: {}", hi_vals.join(" ")));
    }

    // ── Step 9: Send BOOTSTRAP_FALCON commands ──
    // If ACR is running (not back in ROM), send bootstrap commands via mailbox.
    let sec2_pc = r(0x030);
    let sec2_in_acr = sec2_pc > 0x100 && sec2_pc < 0x3000; // ACR code range

    if sec2_in_acr {
        notes.push(format!("ACR appears active at pc={sec2_pc:#x}"));

        // Scan full 64KB DMEM for non-zero regions (find queue structures)
        let hwcfg = r(falcon::HWCFG);
        let dmem_total = falcon::dmem_size_bytes(hwcfg) as usize;
        let scan_size = dmem_total.min(0x10000);
        let mut nz_ranges = Vec::new();
        let mut in_nz = false;
        let mut nz_start = 0;
        // Read in 4KB chunks to avoid timeout
        for chunk_base in (0..scan_size).step_by(4096) {
            let chunk = sec2_dmem_read(bar0, chunk_base as u32, 4096);
            for (i, &word) in chunk.iter().enumerate() {
                let addr = chunk_base + i * 4;
                if word != 0 && word != 0xDEAD_DEAD {
                    if !in_nz { nz_start = addr; in_nz = true; }
                } else if in_nz {
                    nz_ranges.push(format!("{nz_start:#06x}..{addr:#06x}"));
                    in_nz = false;
                }
            }
        }
        if in_nz { nz_ranges.push(format!("{nz_start:#06x}..{scan_size:#06x}")); }
        notes.push(format!(
            "DMEM[0..{scan_size:#x}] non-zero: [{}]",
            nz_ranges.join(", ")
        ));

        // Read interesting high DMEM regions (potential queue headers)
        for &region_start in &[0x1000u32, 0x2000, 0x4000, 0x8000] {
            let sample = sec2_dmem_read(bar0, region_start, 32);
            let has_data = sample.iter().any(|&w| w != 0 && w != 0xDEAD_DEAD);
            if has_data {
                let vals: Vec<String> = sample.iter().enumerate()
                    .filter(|&(_, &w)| w != 0 && w != 0xDEAD_DEAD)
                    .map(|(i, &w)| format!("[{:#06x}]={w:#010x}", region_start as usize + i * 4))
                    .collect();
                notes.push(format!("DMEM@{region_start:#x}: {}", vals.join(" ")));
            }
        }

        // Try BOOTSTRAP via all interrupt methods
        w(falcon::MAILBOX1, falcon_id::FECS);
        w(falcon::MAILBOX0, 1);
        // Method 1: IRQSSET bit 4 (ext interrupt to falcon)
        w(0x000, 0x10);
        std::thread::sleep(std::time::Duration::from_millis(200));
        // Method 2: IRQSSET bit 0-3 (other interrupt sources)
        w(0x000, 0x01);
        std::thread::sleep(std::time::Duration::from_millis(200));

        let pc_post = r(0x030);
        let fecs_cpuctl_post = bar0.read_u32(falcon::FECS_BASE + falcon::CPUCTL).unwrap_or(0);
        notes.push(format!(
            "After IRQ attempts: pc={pc_post:#010x} FECS cpuctl={fecs_cpuctl_post:#010x}"
        ));

        // Check if interrupts changed PC
        if pc_post != sec2_pc {
            notes.push(format!("PC MOVED after IRQ: {sec2_pc:#x} → {pc_post:#x}"));
        }
    } else {
        notes.push(format!("ACR not in code range (pc={sec2_pc:#x}), skipping bootstrap commands"));
    }

    // Final FECS/GPCCS state
    let sec2_after = Sec2Probe::capture(bar0);
    let fecs_r = |off: usize| bar0.read_u32(falcon::FECS_BASE + off).unwrap_or(0xDEAD);
    let fecs_cpuctl_after = fecs_r(falcon::CPUCTL);
    let fecs_mailbox0_after = fecs_r(falcon::MAILBOX0);
    let gpccs_cpuctl_after = bar0
        .read_u32(falcon::GPCCS_BASE + falcon::CPUCTL)
        .unwrap_or(0xDEAD);
    notes.push(format!(
        "Final: FECS cpuctl={fecs_cpuctl_after:#010x} mb0={fecs_mailbox0_after:#010x} GPCCS cpuctl={gpccs_cpuctl_after:#010x}"
    ));

    let success = fecs_cpuctl_after & falcon::CPUCTL_HRESET == 0
        && fecs_mailbox0_after != 0;

    AcrBootResult {
        strategy: "VRAM-based ACR boot (PRAMIN DMA)",
        sec2_before,
        sec2_after,
        fecs_cpuctl_after,
        fecs_mailbox0_after,
        gpccs_cpuctl_after,
        success,
        notes,
    }
}

// ── System-memory ACR boot (Exp 083) ────────────────────────────────

/// IOVA layout for system-memory ACR boot.
/// Placed at 0x40000+ to avoid channel infrastructure (0x1000..0xB000)
/// and FECS init push buffer (0x100000). All within first 2 MiB for
/// single PT0 coverage.
mod sysmem_iova {
    pub const INST:    u64 = 0x4_0000; // SEC2 instance block (4 KiB)
    pub const PD3:     u64 = 0x4_1000;
    pub const PD2:     u64 = 0x4_2000;
    pub const PD1:     u64 = 0x4_3000;
    pub const PD0:     u64 = 0x4_4000;
    pub const PT0:     u64 = 0x4_5000;
    pub const ACR:     u64 = 0x4_6000; // ACR payload (up to 32 KiB)
    pub const WPR:     u64 = 0x4_E000; // WPR region (up to 128 KiB)
}

/// System-memory ACR boot: all DMA buffers in IOMMU-mapped host memory.
///
/// This matches Nouveau's actual architecture on GV100: the WPR, instance
/// block, and page tables all live in system memory. The falcon's MMU
/// translates GPU VAs → IOVAs, which the system IOMMU resolves to host
/// physical pages.
///
/// Key differences from the VRAM path:
///  - Instance block at system memory IOVA (not VRAM offset)
///  - Page table entries use SYS_MEM_COH aperture
///  - 0x668 binding uses SYS_MEM_COH target (bits[29:28] = 2)
///  - WPR and ACR payload in DMA buffers (not PRAMIN)
pub fn attempt_sysmem_acr_boot(
    bar0: &MappedBar,
    fw: &AcrFirmwareSet,
    container: DmaBackend,
) -> AcrBootResult {
    use crate::vfio::DmaBuffer;

    let mut notes = Vec::new();
    let sec2_before = Sec2Probe::capture(bar0);
    let base = falcon::SEC2_BASE;
    let r = |off: usize| bar0.read_u32(base + off).unwrap_or(0xDEAD);
    let w = |off: usize, val: u32| { let _ = bar0.write_u32(base + off, val); };

    notes.push(format!("SEC2 state: {:?}", sec2_before.state));

    // ── Step 1: Parse firmware ──
    let parsed = match ParsedAcrFirmware::parse(fw) {
        Ok(p) => p,
        Err(e) => {
            notes.push(format!("Firmware parse failed: {e}"));
            return make_fail_result("SysMem ACR: parse failed", sec2_before, bar0, notes);
        }
    };
    notes.push(format!(
        "ACR payload: {}B non_sec=[{:#x}+{:#x}] data=[{:#x}+{:#x}]",
        parsed.acr_payload.len(),
        parsed.load_header.non_sec_code_off, parsed.load_header.non_sec_code_size,
        parsed.load_header.data_dma_base, parsed.load_header.data_size,
    ));

    // ── Step 2: Allocate DMA buffers ──
    // Each buffer gets its own IOMMU mapping at a distinct IOVA.
    let mut inst_dma = match DmaBuffer::new(container.clone(), 4096, sysmem_iova::INST) {
        Ok(b) => b,
        Err(e) => {
            notes.push(format!("DMA alloc inst failed: {e}"));
            return make_fail_result("SysMem ACR: DMA alloc failed", sec2_before, bar0, notes);
        }
    };
    let mut pd3_dma = match DmaBuffer::new(container.clone(), 4096, sysmem_iova::PD3) {
        Ok(b) => b,
        Err(e) => {
            notes.push(format!("DMA alloc PD3 failed: {e}"));
            return make_fail_result("SysMem ACR: DMA alloc failed", sec2_before, bar0, notes);
        }
    };
    let mut pd2_dma = match DmaBuffer::new(container.clone(), 4096, sysmem_iova::PD2) {
        Ok(b) => b,
        Err(e) => {
            notes.push(format!("DMA alloc PD2 failed: {e}"));
            return make_fail_result("SysMem ACR: DMA alloc failed", sec2_before, bar0, notes);
        }
    };
    let mut pd1_dma = match DmaBuffer::new(container.clone(), 4096, sysmem_iova::PD1) {
        Ok(b) => b,
        Err(e) => {
            notes.push(format!("DMA alloc PD1 failed: {e}"));
            return make_fail_result("SysMem ACR: DMA alloc failed", sec2_before, bar0, notes);
        }
    };
    let mut pd0_dma = match DmaBuffer::new(container.clone(), 4096, sysmem_iova::PD0) {
        Ok(b) => b,
        Err(e) => {
            notes.push(format!("DMA alloc PD0 failed: {e}"));
            return make_fail_result("SysMem ACR: DMA alloc failed", sec2_before, bar0, notes);
        }
    };
    let mut pt0_dma = match DmaBuffer::new(container.clone(), 4096, sysmem_iova::PT0) {
        Ok(b) => b,
        Err(e) => {
            notes.push(format!("DMA alloc PT0 failed: {e}"));
            return make_fail_result("SysMem ACR: DMA alloc failed", sec2_before, bar0, notes);
        }
    };

    let acr_payload_size = parsed.acr_payload.len().div_ceil(4096) * 4096;
    let mut acr_dma = match DmaBuffer::new(container.clone(), acr_payload_size.max(4096), sysmem_iova::ACR) {
        Ok(b) => b,
        Err(e) => {
            notes.push(format!("DMA alloc ACR payload failed: {e}"));
            return make_fail_result("SysMem ACR: DMA alloc failed", sec2_before, bar0, notes);
        }
    };

    // WPR: build first to know the size, then allocate
    let wpr_base_iova = sysmem_iova::WPR;
    let wpr_data = build_wpr(fw, wpr_base_iova);
    let wpr_end_iova = wpr_base_iova + wpr_data.len() as u64;
    let wpr_buf_size = wpr_data.len().div_ceil(4096) * 4096;
    let mut wpr_dma = match DmaBuffer::new(container.clone(), wpr_buf_size.max(4096), wpr_base_iova) {
        Ok(b) => b,
        Err(e) => {
            notes.push(format!("DMA alloc WPR failed: {e}"));
            return make_fail_result("SysMem ACR: DMA alloc failed", sec2_before, bar0, notes);
        }
    };
    notes.push(format!(
        "DMA buffers: inst={:#x} PD3={:#x} ACR={:#x}({acr_payload_size:#x}) WPR={:#x}({wpr_buf_size:#x})",
        sysmem_iova::INST, sysmem_iova::PD3, sysmem_iova::ACR, sysmem_iova::WPR
    ));

    // ── Step 3: Populate WPR + patch ACR descriptor ──
    wpr_dma.as_mut_slice()[..wpr_data.len()].copy_from_slice(&wpr_data);
    notes.push(format!(
        "WPR: {}B at IOVA {wpr_base_iova:#x}..{wpr_end_iova:#x}",
        wpr_data.len()
    ));

    let mut payload_patched = parsed.acr_payload.to_vec();
    let data_off = parsed.load_header.data_dma_base as usize;
    patch_acr_desc(&mut payload_patched, data_off, wpr_base_iova, wpr_end_iova);
    acr_dma.as_mut_slice()[..payload_patched.len()].copy_from_slice(&payload_patched);
    notes.push(format!(
        "ACR desc patched: wpr=[{wpr_base_iova:#x}..{wpr_end_iova:#x}] data_off={data_off:#x}"
    ));

    // ── Step 4: Populate page tables (identity map first 2 MiB) ──
    // GP100 V2 MMU PDE: (addr >> 4) | aperture_flags
    //   aperture bits[2:1]: 2=SYS_MEM_COH, VOL=bit3
    // GP100 V2 MMU PTE: (addr >> 4) | VALID(0) | aperture(2:1) | VOL(3)
    let sysmem_pde = |iova: u64| -> u64 {
        const FLAGS: u64 = (2 << 1) | (1 << 3); // SYS_MEM_COH + VOL
        (iova >> 4) | FLAGS
    };
    let sysmem_pd0_pde = |iova: u64| -> u64 {
        sysmem_pde(iova) | (1 << 4) // SPT_PRESENT
    };
    let sysmem_pte = |phys: u64| -> u64 {
        const FLAGS: u64 = 1 | (2 << 1) | (1 << 3); // VALID + SYS_MEM_COH + VOL
        (phys >> 4) | FLAGS
    };
    let w32_le = |buf: &mut [u8], off: usize, val: u32| {
        buf[off..off + 4].copy_from_slice(&val.to_le_bytes());
    };

    // PD3[0] → PD2
    let pde3 = sysmem_pde(sysmem_iova::PD2);
    pd3_dma.as_mut_slice()[0..8].copy_from_slice(&pde3.to_le_bytes());

    // PD2[0] → PD1
    let pde2 = sysmem_pde(sysmem_iova::PD1);
    pd2_dma.as_mut_slice()[0..8].copy_from_slice(&pde2.to_le_bytes());

    // PD1[0] → PD0
    let pde1 = sysmem_pde(sysmem_iova::PD0);
    pd1_dma.as_mut_slice()[0..8].copy_from_slice(&pde1.to_le_bytes());

    // PD0[0] → PT0 (dual entry: small PDE with SPT_PRESENT)
    let pde0 = sysmem_pd0_pde(sysmem_iova::PT0);
    pd0_dma.as_mut_slice()[0..8].copy_from_slice(&pde0.to_le_bytes());

    // PT0: identity-map pages 1..512 (4 KiB each = 2 MiB total)
    // Page 0 left unmapped as null guard.
    let pt = pt0_dma.as_mut_slice();
    for i in 1..512usize {
        let phys = (i as u64) * 4096;
        let pte = sysmem_pte(phys);
        let off = i * 8;
        pt[off..off + 8].copy_from_slice(&pte.to_le_bytes());
    }

    notes.push("Page tables: identity-mapped VA 0..2MiB → IOVA (SYS_MEM_COH)".to_string());

    // ── Step 5: Populate SEC2 instance block ──
    // Minimal: just PDB at offset 0x200 + subcontext 0 PDB at 0x2A0.
    // Aperture = SYS_MEM_COH (2), USE_VER2_PT_FORMAT, BIG_PAGE_SIZE = 64K.
    {
        let inst = inst_dma.as_mut_slice();
        let pd3_iova = sysmem_iova::PD3;
        const APER_COH: u32 = 2; // SYS_MEM_COHERENT in bits[1:0]
        let pdb_lo: u32 = ((pd3_iova >> 12) as u32) << 12
            | (1 << 11)  // BIG_PAGE_SIZE = 64 KiB
            | (1 << 10)  // USE_VER2_PT_FORMAT
            | (1 << 2)   // valid
            | APER_COH;
        let pdb_hi: u32 = (pd3_iova >> 32) as u32;

        w32_le(inst, 0x200, pdb_lo);
        w32_le(inst, 0x204, pdb_hi);

        // VA limit: 128 TB
        w32_le(inst, 0x208, 0xFFFF_FFFF);
        w32_le(inst, 0x20C, 0x0001_FFFF);

        // Subcontext 0 PDB (same as main)
        w32_le(inst, 0x290, 1);   // SC_PDB_VALID
        w32_le(inst, 0x2A0, pdb_lo);
        w32_le(inst, 0x2A4, pdb_hi);

        notes.push(format!(
            "Instance block: PDB_LO={pdb_lo:#010x} PDB_HI={pdb_hi:#010x} at IOVA {:#x}",
            sysmem_iova::INST
        ));
    }

    // ── Step 6: Full Nouveau-style SEC2 reset (gm200_flcn_disable + gm200_flcn_enable) ──
    // Phase A: gm200_flcn_disable
    w(0x048, r(0x048) & !0x03);          // clear ITFEN bits[1:0]
    w(0x014, 0xFFFF_FFFF);               // clear all interrupts (IRQMCLR)
    // PMC disable SEC2 engine
    {
        let pmc_enable: usize = 0x200;
        let sec2_bit = find_sec2_pmc_bit(bar0).unwrap_or(22);
        let sec2_mask = 1u32 << sec2_bit;
        let val = bar0.read_u32(pmc_enable).unwrap_or(0);
        if val & sec2_mask != 0 {
            let _ = bar0.write_u32(pmc_enable, val & !sec2_mask);
            let _ = bar0.read_u32(pmc_enable); // flush
            std::thread::sleep(std::time::Duration::from_micros(20));
        }
    }
    // Falcon-local reset (reset_eng)
    w(0x3C0, 0x01);
    std::thread::sleep(std::time::Duration::from_micros(10));
    w(0x3C0, 0x00);

    // Phase B: gm200_flcn_enable
    if let Err(e) = pmc_enable_sec2(bar0) {
        notes.push(format!("PMC enable failed: {e}"));
    }
    // Wait for mem scrubbing (gm200_flcn_reset_wait_mem_scrubbing)
    let _ = bar0.read_u32(base + falcon::MAILBOX0); // dummy read
    let scrub_start = std::time::Instant::now();
    loop {
        let scrub = r(falcon::DMACTL);
        if scrub & 0x06 == 0 { break; }
        if scrub_start.elapsed() > std::time::Duration::from_millis(100) {
            notes.push(format!("Scrub timeout: DMACTL={scrub:#010x}"));
            break;
        }
        std::thread::sleep(std::time::Duration::from_micros(100));
    }

    let boot0 = bar0.read_u32(0x000).unwrap_or(0);
    w(0x084, boot0);

    let cpuctl_post = r(falcon::CPUCTL);
    let sctl_post = r(falcon::SCTL);
    notes.push(format!(
        "Post-reset: cpuctl={cpuctl_post:#010x} sctl={sctl_post:#010x}"
    ));

    // ── Step 7: Bind instance block (exact Nouveau gm200_flcn_fw_load sequence) ──
    // Nouveau does NOT touch 0x624 or DMACTL in the instance-block path.
    // Instead it: enable ITFEN → bind_inst → poll bind_stat → clear IRQ → set IRQ mask → poll idle.

    // 7a. Enable interrupt/transfer: mask(0x048, 0x01, 0x01)
    let itfen = r(0x048);
    w(0x048, (itfen & !0x01) | 0x01);
    notes.push(format!("ITFEN: {itfen:#010x} → {:#010x}", r(0x048)));

    // 7b. gp102_sec2_flcn_bind_inst: SYS_MEM_COH target
    const SYS_MEM_COH_TARGET: u32 = 2;
    let inst_bind_val = ((sysmem_iova::INST >> 12) as u32) | (SYS_MEM_COH_TARGET << 28);
    w(SEC2_FLCN_BIND_INST, inst_bind_val);
    notes.push(format!("0x668: wrote={inst_bind_val:#010x} (SYS_MEM_COH)"));

    // 7c. Poll bind_stat (0x0dc bits[14:12]) until == 5 (bind complete)
    let bind_start = std::time::Instant::now();
    let mut bind_ok = false;
    loop {
        let stat = (r(0x0dc) & 0x7000) >> 12;
        if stat == 5 { bind_ok = true; break; }
        if bind_start.elapsed() > std::time::Duration::from_millis(10) { break; }
        std::thread::sleep(std::time::Duration::from_micros(10));
    }
    notes.push(format!("bind_stat→5: {} (0x0dc={:#010x})", if bind_ok { "OK" } else { "TIMEOUT" }, r(0x0dc)));

    // 7d. Clear DMA interrupt + set IRQ mask: mask(0x004, 0x08, 0x08), mask(0x058, 0x02, 0x02)
    let irqs = r(0x004);
    w(0x004, (irqs & !0x08) | 0x08);
    let irqm = r(0x058);
    w(0x058, (irqm & !0x02) | 0x02);

    // 7e. Poll bind_stat until == 0 (idle)
    let idle_start = std::time::Instant::now();
    let mut idle_ok = false;
    loop {
        let stat = (r(0x0dc) & 0x7000) >> 12;
        if stat == 0 { idle_ok = true; break; }
        if idle_start.elapsed() > std::time::Duration::from_millis(10) { break; }
        std::thread::sleep(std::time::Duration::from_micros(10));
    }
    notes.push(format!("bind_stat→0: {} (0x0dc={:#010x})", if idle_ok { "OK" } else { "TIMEOUT" }, r(0x0dc)));

    // ── Step 8: Load BL code → IMEM ──
    let hwcfg = r(falcon::HWCFG);
    let code_limit = falcon::imem_size_bytes(hwcfg);
    let boot_size = ((parsed.bl_desc.bl_code_off + parsed.bl_desc.bl_code_size + 0xFF) & !0xFF) as u32;
    let imem_addr = code_limit.saturating_sub(boot_size);
    let start_tag = parsed.bl_desc.bl_start_tag;
    let boot_addr = start_tag << 8;

    falcon_imem_upload_nouveau(bar0, base, imem_addr, &parsed.bl_code, start_tag);
    notes.push(format!(
        "BL code: {}B → IMEM@{imem_addr:#x} tag={start_tag:#x} boot={boot_addr:#x}",
        parsed.bl_code.len()
    ));

    // ── Step 10: Pre-load ACR data to DMEM + BL descriptor overlay ──
    // The BL's DMA xcld may succeed now (system memory path), but we also
    // pre-load the data section as insurance.
    let code_dma_base = sysmem_iova::ACR;
    let data_dma_base = sysmem_iova::ACR + data_off as u64;

    let data_section = &payload_patched[data_off..];
    falcon_dmem_upload(bar0, base, 0, data_section);
    notes.push(format!(
        "ACR data pre-loaded: {}B → DMEM@0",
        data_section.len()
    ));

    let bl_desc = build_bl_dmem_desc(code_dma_base, data_dma_base, &parsed);
    falcon_dmem_upload(bar0, base, 0, &bl_desc);
    notes.push(format!(
        "BL desc: code={code_dma_base:#x} data={data_dma_base:#x}"
    ));

    // ── Step 10: Boot SEC2 ──
    // Nouveau writes 0xcafebeef to MAILBOX0 before start; expects 0x10 on success.
    w(falcon::MAILBOX0, 0xcafe_beef_u32);
    w(falcon::MAILBOX1, 0);
    w(falcon::BOOTVEC, boot_addr);
    notes.push(format!("BOOTVEC={boot_addr:#x} mb0=0xcafebeef, issuing STARTCPU"));
    falcon_start_cpu(bar0, base);

    // ── Step 12: Poll with PC sampling ──
    let timeout = std::time::Duration::from_secs(5);
    let start_time = std::time::Instant::now();
    let mut pc_samples = Vec::new();
    let mut last_pc = 0u32;

    let mut all_pcs: Vec<u32> = Vec::new();
    for _ in 0..500 {
        let pc = r(0x030);
        if all_pcs.last() != Some(&pc) {
            all_pcs.push(pc);
        }
        std::thread::sleep(std::time::Duration::from_micros(100));
        if start_time.elapsed().as_millis() > 50 { break; }
    }

    let mut settled_count = 0u32;
    loop {
        std::thread::sleep(std::time::Duration::from_millis(5));
        let cpuctl = r(falcon::CPUCTL);
        let mb0 = r(falcon::MAILBOX0);
        let mb1 = r(falcon::MAILBOX1);
        let pc = r(0x030);

        if pc != last_pc {
            pc_samples.push(format!("{:#06x}@{}ms", pc, start_time.elapsed().as_millis()));
            last_pc = pc;
            settled_count = 0;
        } else {
            settled_count += 1;
        }

        let halted = cpuctl & falcon::CPUCTL_HALTED != 0;
        let hreset_back = cpuctl & falcon::CPUCTL_HRESET != 0;

        if mb0 != 0 || halted || hreset_back {
            notes.push(format!(
                "SEC2 response: cpuctl={cpuctl:#010x} mb0={mb0:#010x} mb1={mb1:#010x} pc={pc:#010x} ({}ms)",
                start_time.elapsed().as_millis()
            ));
            break;
        }
        if settled_count > 200 {
            notes.push(format!(
                "SEC2 settled: cpuctl={cpuctl:#010x} mb0={mb0:#010x} pc={pc:#010x} ({}ms)",
                start_time.elapsed().as_millis()
            ));
            break;
        }
        if start_time.elapsed() > timeout {
            notes.push(format!(
                "SEC2 timeout: cpuctl={cpuctl:#010x} mb0={mb0:#010x} pc={pc:#010x}"
            ));
            break;
        }
    }

    if !all_pcs.is_empty() {
        let pc_trace: Vec<String> = all_pcs.iter().map(|p| format!("{p:#06x}")).collect();
        notes.push(format!("Fast PC trace: [{}]", pc_trace.join(" → ")));
    }
    if !pc_samples.is_empty() {
        notes.push(format!("PC progression: [{}]", pc_samples.join(", ")));
    }

    // ── Step 13: Diagnostics ──
    let exci = r(falcon::EXCI);
    let tracepc = [r(0x030), r(0x034), r(0x038), r(0x03C)];
    notes.push(format!(
        "Diag: EXCI={exci:#010x} TRACEPC=[{:#06x}, {:#06x}, {:#06x}, {:#06x}]",
        tracepc[0], tracepc[1], tracepc[2], tracepc[3]
    ));

    let dma_624 = r(0x624);
    let dma_10c = r(falcon::DMACTL);
    let dma_668 = r(SEC2_FLCN_BIND_INST);
    let dmatrfbase = r(0x110);
    let dmatrfmoffs = r(0x114);
    let dmatrfcmd = r(0x118);
    let dmatrffboffs = r(0x11C);
    notes.push(format!(
        "DMA: 0x624={dma_624:#010x} DMACTL={dma_10c:#010x} 0x668={dma_668:#010x}"
    ));
    notes.push(format!(
        "DMA xfer: base={dmatrfbase:#010x} moffs={dmatrfmoffs:#010x} cmd={dmatrfcmd:#010x} fboffs={dmatrffboffs:#010x}"
    ));

    // DMEM diagnostic: ACR descriptor region
    let dmem_acr = sec2_dmem_read(bar0, 0x200, 0x70);
    let acr_vals: Vec<String> = dmem_acr.iter().enumerate()
        .filter(|&(_, &w)| w != 0 && w != 0xDEAD_DEAD)
        .map(|(i, &w)| format!("[{:#05x}]={w:#010x}", 0x200 + i * 4))
        .collect();
    notes.push(format!(
        "DMEM[0x200..0x270]: {}",
        if acr_vals.is_empty() { "ALL ZERO".to_string() } else { acr_vals.join(" ") }
    ));

    // Check WPR header status in DMA buffer — did ACR modify it?
    {
        let wpr_slice = wpr_dma.as_mut_slice();
        let fecs_status = u32::from_le_bytes([wpr_slice[20], wpr_slice[21], wpr_slice[22], wpr_slice[23]]);
        let gpccs_status = u32::from_le_bytes([wpr_slice[44], wpr_slice[45], wpr_slice[46], wpr_slice[47]]);
        notes.push(format!(
            "WPR headers: FECS status={fecs_status} GPCCS status={gpccs_status} (0=none, 1=copy, 0xFF=done)"
        ));
    }

    // ── Capture final state ──
    let sec2_after = Sec2Probe::capture(bar0);
    let fecs_cpuctl_after = bar0
        .read_u32(falcon::FECS_BASE + falcon::CPUCTL)
        .unwrap_or(0xDEAD);
    let fecs_mailbox0_after = bar0
        .read_u32(falcon::FECS_BASE + falcon::MAILBOX0)
        .unwrap_or(0);
    let gpccs_cpuctl_after = bar0
        .read_u32(falcon::GPCCS_BASE + falcon::CPUCTL)
        .unwrap_or(0xDEAD);
    notes.push(format!(
        "Final: FECS cpuctl={fecs_cpuctl_after:#010x} GPCCS cpuctl={gpccs_cpuctl_after:#010x}"
    ));

    let success = fecs_cpuctl_after & falcon::CPUCTL_HRESET == 0
        && fecs_mailbox0_after != 0;

    AcrBootResult {
        strategy: "System-memory ACR boot (IOMMU DMA)",
        sec2_before,
        sec2_after,
        fecs_cpuctl_after,
        fecs_mailbox0_after,
        gpccs_cpuctl_after,
        success,
        notes,
    }
}

/// Hybrid ACR boot: VRAM instance block + page tables, system memory data.
///
/// This matches Nouveau's exact architecture on GV100:
///  - Instance block: VRAM (falcon can always reach VRAM)
///  - Page directory chain (PD3→PD2→PD1→PD0): VRAM, VRAM-aperture PDEs
///  - PT0 entries: SYS_MEM_COH aperture → IOMMU-mapped DMA buffers
///  - ACR payload + WPR: system memory DMA buffers
///
/// The falcon's 0x668 binding uses VRAM target (no IOMMU needed for initial
/// lookup). The GPU MMU walks VRAM page tables. Only leaf PTEs cross to
/// system memory via the IOMMU.
pub fn attempt_hybrid_acr_boot(
    bar0: &MappedBar,
    fw: &AcrFirmwareSet,
    container: DmaBackend,
) -> AcrBootResult {
    use crate::vfio::DmaBuffer;

    let mut notes = Vec::new();
    let sec2_before = Sec2Probe::capture(bar0);
    let base = falcon::SEC2_BASE;
    let r = |off: usize| bar0.read_u32(base + off).unwrap_or(0xDEAD);
    let w = |off: usize, val: u32| { let _ = bar0.write_u32(base + off, val); };

    notes.push(format!("SEC2 state: {:?}", sec2_before.state));

    // ── Step 1: Parse firmware ──
    let parsed = match ParsedAcrFirmware::parse(fw) {
        Ok(p) => p,
        Err(e) => {
            notes.push(format!("Firmware parse failed: {e}"));
            return make_fail_result("Hybrid ACR: parse failed", sec2_before, bar0, notes);
        }
    };
    notes.push(format!(
        "ACR payload: {}B data_off={:#x}",
        parsed.acr_payload.len(), parsed.load_header.data_dma_base
    ));

    // ── Step 2: Allocate DMA buffers for ACR payload + WPR ──
    let acr_iova = sysmem_iova::ACR;
    let acr_payload_size = parsed.acr_payload.len().div_ceil(4096) * 4096;
    let mut acr_dma = match DmaBuffer::new(container.clone(), acr_payload_size.max(4096), acr_iova) {
        Ok(b) => b,
        Err(e) => {
            notes.push(format!("DMA alloc ACR failed: {e}"));
            return make_fail_result("Hybrid ACR: DMA alloc failed", sec2_before, bar0, notes);
        }
    };

    let wpr_iova = sysmem_iova::WPR;
    let wpr_data = build_wpr(fw, wpr_iova);
    let wpr_end = wpr_iova + wpr_data.len() as u64;
    let wpr_buf_size = wpr_data.len().div_ceil(4096) * 4096;
    let mut wpr_dma = match DmaBuffer::new(container.clone(), wpr_buf_size.max(4096), wpr_iova) {
        Ok(b) => b,
        Err(e) => {
            notes.push(format!("DMA alloc WPR failed: {e}"));
            return make_fail_result("Hybrid ACR: DMA alloc failed", sec2_before, bar0, notes);
        }
    };
    notes.push(format!(
        "DMA: ACR@{acr_iova:#x}({acr_payload_size:#x}) WPR@{wpr_iova:#x}({wpr_buf_size:#x})"
    ));

    // ── Step 3: Populate WPR + patch ACR descriptor ──
    wpr_dma.as_mut_slice()[..wpr_data.len()].copy_from_slice(&wpr_data);

    let mut payload_patched = parsed.acr_payload.to_vec();
    let data_off = parsed.load_header.data_dma_base as usize;
    patch_acr_desc(&mut payload_patched, data_off, wpr_iova, wpr_end);
    acr_dma.as_mut_slice()[..payload_patched.len()].copy_from_slice(&payload_patched);
    notes.push(format!(
        "WPR: {}B [{wpr_iova:#x}..{wpr_end:#x}] desc patched",
        wpr_data.len()
    ));

    // ── Step 4: Build VRAM page tables (hybrid: VRAM PDEs + sysmem PTEs) ──
    // Reuse VRAM addresses for PD chain (falcon can always DMA from VRAM).
    // PT0 entries point to sysmem IOVAs via SYS_MEM_COH aperture.
    let wv = |vram_addr: u32, offset: usize, val: u32| -> bool {
        match PraminRegion::new(bar0, vram_addr, offset + 4) {
            Ok(mut region) => region.write_u32(offset, val).is_ok(),
            Err(_) => false,
        }
    };
    let wv64 = |vram_addr: u32, offset: usize, val: u64| -> bool {
        let lo = (val & 0xFFFF_FFFF) as u32;
        let hi = (val >> 32) as u32;
        wv(vram_addr, offset, lo) && wv(vram_addr, offset + 4, hi)
    };

    // PD chain: all VRAM aperture
    let pt_ok = wv64(FALCON_PD3_VRAM, 0, encode_vram_pde(FALCON_PD2_VRAM as u64))
        && wv64(FALCON_PD2_VRAM, 0, encode_vram_pde(FALCON_PD1_VRAM as u64))
        && wv64(FALCON_PD1_VRAM, 0, encode_vram_pde(FALCON_PD0_VRAM as u64))
        && wv64(FALCON_PD0_VRAM, 0, encode_vram_pd0_pde(FALCON_PT0_VRAM as u64));

    if !pt_ok {
        notes.push("VRAM page directory chain write failed".to_string());
        return make_fail_result("Hybrid ACR: VRAM PD failed", sec2_before, bar0, notes);
    }

    // PT0: hybrid mapping
    // Pages 0x40..0x6E (IOVAs 0x40000..0x6E000) → SYS_MEM_COH PTEs
    // All other pages → identity-map to VRAM (for BL/ACR internal use)
    let acr_page_start = (acr_iova / 4096) as u64;
    let wpr_page_end = ((wpr_iova + wpr_buf_size as u64).div_ceil(4096)) as u64;
    let mut pt_fail = false;
    for i in 1u64..512 {
        let pte = if i >= acr_page_start && i < wpr_page_end {
            encode_sysmem_pte(i * 4096)
        } else {
            encode_vram_pte(i * 4096)
        };
        if !wv64(FALCON_PT0_VRAM, (i as usize) * 8, pte) {
            pt_fail = true;
            break;
        }
    }
    if pt_fail {
        notes.push("VRAM PT0 write failed".to_string());
        return make_fail_result("Hybrid ACR: VRAM PT failed", sec2_before, bar0, notes);
    }
    notes.push(format!(
        "VRAM page tables: PD chain in VRAM, PT0 sysmem pages {acr_page_start}..{wpr_page_end}"
    ));

    // Instance block: PDB in VRAM, pointing to PD3 in VRAM
    let pdb_lo: u32 = ((FALCON_PD3_VRAM >> 12) << 12)
        | (1 << 11) // BIG_PAGE_SIZE = 64KiB
        | (1 << 10) // USE_VER2_PT_FORMAT
        ;           // bits[1:0] = 0 = VRAM aperture, VOL=0
    if !wv(FALCON_INST_VRAM, 0x200, pdb_lo)
        || !wv(FALCON_INST_VRAM, 0x204, 0)
        || !wv(FALCON_INST_VRAM, 0x208, 0xFFFF_FFFF)
        || !wv(FALCON_INST_VRAM, 0x20C, 0x0001_FFFF)
    {
        notes.push("VRAM instance block write failed".to_string());
        return make_fail_result("Hybrid ACR: inst write failed", sec2_before, bar0, notes);
    }
    notes.push(format!(
        "Instance block: VRAM@{:#x} PDB_LO={pdb_lo:#010x} (VRAM aperture)",
        FALCON_INST_VRAM
    ));

    // ── Step 5: Full Nouveau-style SEC2 reset (gm200_flcn_disable + gm200_flcn_enable) ──
    // Phase A: gm200_flcn_disable
    w(0x048, r(0x048) & !0x03);          // clear ITFEN bits[1:0]
    w(0x014, 0xFFFF_FFFF);               // clear all interrupts
    {
        let pmc_enable: usize = 0x200;
        let sec2_bit = find_sec2_pmc_bit(bar0).unwrap_or(22);
        let sec2_mask = 1u32 << sec2_bit;
        let val = bar0.read_u32(pmc_enable).unwrap_or(0);
        if val & sec2_mask != 0 {
            let _ = bar0.write_u32(pmc_enable, val & !sec2_mask);
            let _ = bar0.read_u32(pmc_enable);
            std::thread::sleep(std::time::Duration::from_micros(20));
        }
    }
    w(0x3C0, 0x01);
    std::thread::sleep(std::time::Duration::from_micros(10));
    w(0x3C0, 0x00);

    // Phase B: gm200_flcn_enable
    if let Err(e) = pmc_enable_sec2(bar0) {
        notes.push(format!("PMC enable failed: {e}"));
    }
    let _ = bar0.read_u32(base + falcon::MAILBOX0);
    let scrub_start = std::time::Instant::now();
    loop {
        let scrub = r(falcon::DMACTL);
        if scrub & 0x06 == 0 { break; }
        if scrub_start.elapsed() > std::time::Duration::from_millis(100) {
            notes.push(format!("Scrub timeout: DMACTL={scrub:#010x}"));
            break;
        }
        std::thread::sleep(std::time::Duration::from_micros(100));
    }

    let boot0 = bar0.read_u32(0x000).unwrap_or(0);
    w(0x084, boot0);

    let cpuctl_post = r(falcon::CPUCTL);
    let sctl_post = r(falcon::SCTL);
    notes.push(format!("Post-reset: cpuctl={cpuctl_post:#010x} sctl={sctl_post:#010x}"));

    // ── Step 6: Bind instance block (exact Nouveau gm200_flcn_fw_load sequence) ──

    // 6a. Enable interrupt/transfer: mask(0x048, 0x01, 0x01)
    let itfen = r(0x048);
    w(0x048, (itfen & !0x01) | 0x01);
    notes.push(format!("ITFEN: {itfen:#010x} → {:#010x}", r(0x048)));

    // 6b. gp102_sec2_flcn_bind_inst: VRAM target (bits[29:28] = 0)
    let inst_bind_val = (FALCON_INST_VRAM >> 12) as u32;
    w(SEC2_FLCN_BIND_INST, inst_bind_val);
    notes.push(format!("0x668: wrote={inst_bind_val:#010x} (VRAM target)"));

    // 6c. Poll bind_stat (0x0dc bits[14:12]) until == 5 (bind complete)
    let bind_start = std::time::Instant::now();
    let mut bind_ok = false;
    loop {
        let stat = (r(0x0dc) & 0x7000) >> 12;
        if stat == 5 { bind_ok = true; break; }
        if bind_start.elapsed() > std::time::Duration::from_millis(10) { break; }
        std::thread::sleep(std::time::Duration::from_micros(10));
    }
    notes.push(format!("bind_stat→5: {} (0x0dc={:#010x})", if bind_ok { "OK" } else { "TIMEOUT" }, r(0x0dc)));

    // 6d. Clear DMA interrupt + set IRQ mask
    let irqs = r(0x004);
    w(0x004, (irqs & !0x08) | 0x08);
    let irqm = r(0x058);
    w(0x058, (irqm & !0x02) | 0x02);

    // 6e. Poll bind_stat until == 0 (idle)
    let idle_start = std::time::Instant::now();
    let mut idle_ok = false;
    loop {
        let stat = (r(0x0dc) & 0x7000) >> 12;
        if stat == 0 { idle_ok = true; break; }
        if idle_start.elapsed() > std::time::Duration::from_millis(10) { break; }
        std::thread::sleep(std::time::Duration::from_micros(10));
    }
    notes.push(format!("bind_stat→0: {} (0x0dc={:#010x})", if idle_ok { "OK" } else { "TIMEOUT" }, r(0x0dc)));

    // ── Step 7: Load BL code → IMEM ──
    let hwcfg = r(falcon::HWCFG);
    let code_limit = falcon::imem_size_bytes(hwcfg);
    let boot_size = ((parsed.bl_desc.bl_code_off + parsed.bl_desc.bl_code_size + 0xFF) & !0xFF) as u32;
    let imem_addr = code_limit.saturating_sub(boot_size);
    let start_tag = parsed.bl_desc.bl_start_tag;
    let boot_addr = start_tag << 8;

    falcon_imem_upload_nouveau(bar0, base, imem_addr, &parsed.bl_code, start_tag);
    notes.push(format!(
        "BL code: {}B → IMEM@{imem_addr:#x} boot={boot_addr:#x}",
        parsed.bl_code.len()
    ));

    // ── Step 9: Pre-load ACR data + BL descriptor ──
    let code_dma_base = acr_iova;
    let data_dma_base = acr_iova + data_off as u64;

    let data_section = &payload_patched[data_off..];
    falcon_dmem_upload(bar0, base, 0, data_section);
    let bl_desc = build_bl_dmem_desc(code_dma_base, data_dma_base, &parsed);
    falcon_dmem_upload(bar0, base, 0, &bl_desc);
    notes.push(format!(
        "BL desc: code={code_dma_base:#x} data={data_dma_base:#x}"
    ));

    // ── Step 10: Boot SEC2 ──
    w(falcon::MAILBOX0, 0xcafe_beef_u32);
    w(falcon::MAILBOX1, 0);
    w(falcon::BOOTVEC, boot_addr);
    notes.push(format!("BOOTVEC={boot_addr:#x} mb0=0xcafebeef, issuing STARTCPU"));
    falcon_start_cpu(bar0, base);

    // ── Step 11: Poll with PC sampling ──
    let timeout = std::time::Duration::from_secs(5);
    let start_time = std::time::Instant::now();
    let mut pc_samples = Vec::new();
    let mut last_pc = 0u32;

    let mut all_pcs: Vec<u32> = Vec::new();
    for _ in 0..500 {
        let pc = r(0x030);
        if all_pcs.last() != Some(&pc) {
            all_pcs.push(pc);
        }
        std::thread::sleep(std::time::Duration::from_micros(100));
        if start_time.elapsed().as_millis() > 50 { break; }
    }

    let mut settled_count = 0u32;
    loop {
        std::thread::sleep(std::time::Duration::from_millis(5));
        let cpuctl = r(falcon::CPUCTL);
        let mb0 = r(falcon::MAILBOX0);
        let mb1 = r(falcon::MAILBOX1);
        let pc = r(0x030);

        if pc != last_pc {
            pc_samples.push(format!("{:#06x}@{}ms", pc, start_time.elapsed().as_millis()));
            last_pc = pc;
            settled_count = 0;
        } else {
            settled_count += 1;
        }

        let halted = cpuctl & falcon::CPUCTL_HALTED != 0;
        let hreset_back = cpuctl & falcon::CPUCTL_HRESET != 0;

        if mb0 != 0 || halted || hreset_back {
            notes.push(format!(
                "SEC2 response: cpuctl={cpuctl:#010x} mb0={mb0:#010x} mb1={mb1:#010x} pc={pc:#010x} ({}ms)",
                start_time.elapsed().as_millis()
            ));
            break;
        }
        if settled_count > 200 {
            notes.push(format!(
                "SEC2 settled: cpuctl={cpuctl:#010x} mb0={mb0:#010x} pc={pc:#010x} ({}ms)",
                start_time.elapsed().as_millis()
            ));
            break;
        }
        if start_time.elapsed() > timeout {
            notes.push(format!(
                "SEC2 timeout: cpuctl={cpuctl:#010x} mb0={mb0:#010x} pc={pc:#010x}"
            ));
            break;
        }
    }

    if !all_pcs.is_empty() {
        let pc_trace: Vec<String> = all_pcs.iter().map(|p| format!("{p:#06x}")).collect();
        notes.push(format!("Fast PC trace: [{}]", pc_trace.join(" → ")));
    }
    if !pc_samples.is_empty() {
        notes.push(format!("PC progression: [{}]", pc_samples.join(", ")));
    }

    // ── Diagnostics ──
    let exci = r(falcon::EXCI);
    let tracepc = [r(0x030), r(0x034), r(0x038), r(0x03C)];
    notes.push(format!(
        "Diag: EXCI={exci:#010x} TRACEPC=[{:#06x}, {:#06x}, {:#06x}, {:#06x}]",
        tracepc[0], tracepc[1], tracepc[2], tracepc[3]
    ));

    let dmatrfcmd = r(0x118);
    notes.push(format!(
        "DMA: 0x624={:#010x} DMACTL={:#010x} dma_cmd={dmatrfcmd:#010x}",
        r(0x624), r(falcon::DMACTL)
    ));

    // GPU MMU fault check — see if falcon DMA triggered a page fault
    use crate::vfio::channel::registers::mmu;
    let fault_status = bar0.read_u32(mmu::FAULT_STATUS).unwrap_or(0);
    let fault_addr_lo = bar0.read_u32(mmu::FAULT_ADDR_LO).unwrap_or(0);
    let fault_addr_hi = bar0.read_u32(mmu::FAULT_ADDR_HI).unwrap_or(0);
    let fault_inst_lo = bar0.read_u32(mmu::FAULT_INST_LO).unwrap_or(0);
    let fault_inst_hi = bar0.read_u32(mmu::FAULT_INST_HI).unwrap_or(0);
    if fault_status != 0 {
        notes.push(format!(
            "MMU FAULT: status={fault_status:#010x} addr={fault_addr_hi:#010x}_{fault_addr_lo:#010x} inst={fault_inst_hi:#010x}_{fault_inst_lo:#010x}"
        ));
    } else {
        notes.push("MMU fault: none pending".to_string());
    }

    // DMEM dump: first 0x100 bytes (BL desc + ACR state) + 0x200-0x270 (ACR desc)
    let dmem_lo = sec2_dmem_read(bar0, 0, 256);
    let lo_vals: Vec<String> = dmem_lo.iter().enumerate()
        .filter(|&(_, &w)| w != 0 && w != 0xDEAD_DEAD)
        .map(|(i, &w)| format!("[{:#05x}]={w:#010x}", i * 4))
        .collect();
    if !lo_vals.is_empty() {
        notes.push(format!("DMEM[0..0x100]: {}", lo_vals.join(" ")));
    }
    let dmem_acr = sec2_dmem_read(bar0, 0x200, 0x70);
    let acr_vals: Vec<String> = dmem_acr.iter().enumerate()
        .filter(|&(_, &w)| w != 0 && w != 0xDEAD_DEAD)
        .map(|(i, &w)| format!("[{:#05x}]={w:#010x}", 0x200 + i * 4))
        .collect();
    notes.push(format!(
        "DMEM[0x200..0x270]: {}",
        if acr_vals.is_empty() { "ALL ZERO".to_string() } else { acr_vals.join(" ") }
    ));

    // Check WPR header status in DMA buffer
    {
        let wpr_slice = wpr_dma.as_mut_slice();
        let fecs_status = u32::from_le_bytes([wpr_slice[20], wpr_slice[21], wpr_slice[22], wpr_slice[23]]);
        let gpccs_status = u32::from_le_bytes([wpr_slice[44], wpr_slice[45], wpr_slice[46], wpr_slice[47]]);
        notes.push(format!(
            "WPR: FECS status={fecs_status} GPCCS status={gpccs_status} (1=copy, 0xFF=done)"
        ));
    }

    let sec2_after = Sec2Probe::capture(bar0);
    let fecs_cpuctl_after = bar0
        .read_u32(falcon::FECS_BASE + falcon::CPUCTL)
        .unwrap_or(0xDEAD);
    let fecs_mailbox0_after = bar0
        .read_u32(falcon::FECS_BASE + falcon::MAILBOX0)
        .unwrap_or(0);
    let gpccs_cpuctl_after = bar0
        .read_u32(falcon::GPCCS_BASE + falcon::CPUCTL)
        .unwrap_or(0xDEAD);
    notes.push(format!(
        "Final: FECS cpuctl={fecs_cpuctl_after:#010x} GPCCS cpuctl={gpccs_cpuctl_after:#010x}"
    ));

    let success = fecs_cpuctl_after & falcon::CPUCTL_HRESET == 0
        && fecs_mailbox0_after != 0;

    AcrBootResult {
        strategy: "Hybrid ACR boot (VRAM pages + sysmem data)",
        sec2_before,
        sec2_after,
        fecs_cpuctl_after,
        fecs_mailbox0_after,
        gpccs_cpuctl_after,
        success,
        notes,
    }
}

/// 4. Engine-reset SEC2 falcon
/// 5. Configure physical DMA mode (register 0x624 + DMACTL)
/// 6. Load BL code → IMEM (per-page IMEMC init, matching Nouveau)
/// 7. Build flcn_bl_dmem_desc_v1 → DMEM (with DMA addresses)
/// 8. BOOTVEC + STARTCPU → poll for HRESET + mailbox check
pub fn attempt_acr_chain(
    bar0: &MappedBar,
    fw: &AcrFirmwareSet,
    container: DmaBackend,
) -> AcrBootResult {
    let mut notes = Vec::new();
    let sec2_before = Sec2Probe::capture(bar0);
    notes.push(format!("SEC2 state: {:?}", sec2_before.state));

    let base = falcon::SEC2_BASE;
    let r = |off: usize| bar0.read_u32(base + off).unwrap_or(0xDEAD);
    let w = |off: usize, val: u32| { let _ = bar0.write_u32(base + off, val); };

    // ── Step 1: Parse firmware ──
    let parsed = match ParsedAcrFirmware::parse(fw) {
        Ok(p) => p,
        Err(e) => {
            notes.push(format!("Firmware parse failed: {e}"));
            return make_fail_result("ACR chain: parse failed", sec2_before, bar0, notes);
        }
    };
    notes.push(format!("{}", parsed.bl_desc));
    notes.push(format!(
        "ACR ucode: {}B, non_sec=[{:#x}+{:#x}] sec=[{:#x}+{:#x}] data=[{:#x}+{:#x}]",
        parsed.acr_payload.len(),
        parsed.load_header.non_sec_code_off, parsed.load_header.non_sec_code_size,
        parsed.load_header.apps.first().map(|a| a.0).unwrap_or(0),
        parsed.load_header.apps.first().map(|a| a.1).unwrap_or(0),
        parsed.load_header.data_dma_base, parsed.load_header.data_size,
    ));

    // ── Step 2: Allocate DMA for ACR firmware payload ──
    let acr_payload_size = parsed.acr_payload.len();
    let acr_iova = ACR_IOVA_BASE;
    let mut acr_dma = match DmaBuffer::new(container.clone(), acr_payload_size, acr_iova) {
        Ok(buf) => buf,
        Err(e) => {
            notes.push(format!("DMA alloc failed for ACR payload: {e}"));
            return make_fail_result("ACR chain: DMA alloc failed", sec2_before, bar0, notes);
        }
    };
    notes.push(format!("ACR payload DMA: iova={acr_iova:#x} size={acr_payload_size:#x}"));

    // Copy ACR payload into DMA buffer (with optional WPR patching)
    let mut payload_copy = parsed.acr_payload.clone();

    // Patch ACR descriptor with placeholder WPR (all zeros = no WPR yet)
    // For the initial boot test, WPR patching is deferred — the BL should
    // still DMA-load the ACR ucode successfully and we can observe the behavior.
    let data_off = parsed.load_header.data_dma_base as usize;
    if data_off + 0x24 <= payload_copy.len() {
        notes.push(format!("ACR desc at data_off={data_off:#x} (placeholder WPR)"));
    }

    acr_dma.as_mut_slice()[..payload_copy.len()].copy_from_slice(&payload_copy);

    let code_dma_base = acr_iova;
    let data_dma_base = acr_iova + data_off as u64;
    notes.push(format!("DMA addrs: code={code_dma_base:#x} data={data_dma_base:#x}"));

    // ── Step 3: Engine-reset SEC2 ──
    tracing::info!("Engine-resetting SEC2 for ACR chain boot");
    if let Err(e) = falcon_engine_reset(bar0, base) {
        notes.push(format!("Engine reset failed: {e}"));
    } else {
        let cpuctl_post = r(falcon::CPUCTL);
        notes.push(format!("Post-reset cpuctl={cpuctl_post:#010x}"));
    }

    // ── Step 4: Configure SEC2 DMA ──
    // Try all known methods to enable falcon DMA for system memory access.
    // Nouveau uses two different approaches depending on falcon generation:
    //
    // A) Instance block binding (gp102_sec2_flcn_bind_inst):
    //    Register 0x668 (SEC2-specific) or 0x480 (generic falcon)
    //    Value: (pd3_iova >> 12) | (target << 28), target=3 for SYS_MEM_COH
    //    Then DMACTL=0x02 (enable IMEM DMA through instance block)
    //
    // B) Physical DMA mode (gm200_flcn_fw_boot, no instance block):
    //    Register 0x624 |= 0x80 (enable physical addressing)
    //    DMACTL=0 (no instance block translation)
    //
    // We try both: bind instance block first, then set physical DMA as fallback.
    use crate::vfio::channel::registers::PD3_IOVA;
    const SYS_MEM_COH_TARGET: u64 = 3;
    let inst_val = ((PD3_IOVA >> 12) | (SYS_MEM_COH_TARGET << 28)) as u32;

    // Method A: Instance block binding (both SEC2-specific and generic registers)
    w(SEC2_FLCN_BIND_INST, inst_val); // 0x668 — gp102+ SEC2
    w(0x480, inst_val); // 0x480 — generic gm200+ falcon bind_inst
    w(falcon::DMACTL, 0x02);
    let r668 = r(SEC2_FLCN_BIND_INST);
    let r480 = r(0x480);
    let dmactl_a = r(falcon::DMACTL);
    notes.push(format!(
        "DMA config: 0x668={r668:#010x} 0x480={r480:#010x} DMACTL={dmactl_a:#010x} (wrote {inst_val:#010x})"
    ));

    // Method B: Also enable physical DMA as fallback (does not conflict with inst block)
    w(0x624, r(0x624) | 0x80);
    notes.push(format!(
        "Physical DMA fallback: 0x624={:#010x}",
        r(0x624)
    ));

    // ── Step 5: Load BL code → IMEM ──
    let hwcfg = r(falcon::HWCFG);
    let code_limit = falcon::imem_size_bytes(hwcfg);
    let boot_size = ((parsed.bl_desc.bl_code_off + parsed.bl_desc.bl_code_size + 0xFF) & !0xFF) as u32;
    let imem_addr = code_limit.saturating_sub(boot_size);
    let start_tag = parsed.bl_desc.bl_start_tag;
    let boot_addr = start_tag << 8;

    notes.push(format!(
        "IMEM: code_limit={code_limit:#x} boot_size={boot_size:#x} addr={imem_addr:#x} tag={start_tag:#x} boot_addr={boot_addr:#x}"
    ));

    falcon_imem_upload_nouveau(bar0, base, imem_addr, &parsed.bl_code, start_tag);

    // ── Step 6: Build BL descriptor → DMEM ──
    let bl_desc_bytes = build_bl_dmem_desc(code_dma_base, data_dma_base, &parsed);
    notes.push(format!(
        "BL DMEM desc: {} bytes → DMEM@0 (ctx_dma=UCODE via instance block)",
        bl_desc_bytes.len()
    ));
    falcon_dmem_upload(bar0, base, 0, &bl_desc_bytes);

    // ── Step 7: Boot SEC2 ──
    w(falcon::MAILBOX0, 0);
    w(falcon::MAILBOX1, 0);
    w(falcon::BOOTVEC, boot_addr);
    notes.push(format!("BOOTVEC={boot_addr:#x}, issuing STARTCPU"));
    w(falcon::CPUCTL, falcon::CPUCTL_STARTCPU);

    // ── Step 8: Poll for completion ──
    // Nouveau waits for CPUCTL bit 4 (HRESET) to be re-asserted = falcon halted.
    let timeout = std::time::Duration::from_secs(3);
    let start = std::time::Instant::now();
    let mut last_cpuctl = 0u32;
    loop {
        std::thread::sleep(std::time::Duration::from_millis(10));
        let cpuctl = r(falcon::CPUCTL);
        let mb0 = r(falcon::MAILBOX0);
        let mb1 = r(falcon::MAILBOX1);
        last_cpuctl = cpuctl;

        // Success: falcon halted (HRESET re-asserted) and mailbox indicates completion
        let halted = cpuctl & falcon::CPUCTL_HALTED != 0;
        let hreset_back = cpuctl & falcon::CPUCTL_HRESET != 0;

        if hreset_back && cpuctl != sec2_before.cpuctl {
            notes.push(format!(
                "SEC2 halted: cpuctl={cpuctl:#010x} mb0={mb0:#010x} mb1={mb1:#010x} ({}ms)",
                start.elapsed().as_millis()
            ));
            break;
        }
        if mb0 != 0 {
            notes.push(format!(
                "SEC2 mailbox: cpuctl={cpuctl:#010x} mb0={mb0:#010x} mb1={mb1:#010x} ({}ms)",
                start.elapsed().as_millis()
            ));
            break;
        }
        if halted {
            notes.push(format!(
                "SEC2 halted (no mailbox): cpuctl={cpuctl:#010x} ({}ms)",
                start.elapsed().as_millis()
            ));
            break;
        }
        if start.elapsed() > timeout {
            notes.push(format!(
                "SEC2 timeout: cpuctl={cpuctl:#010x} mb0={mb0:#010x} mb1={mb1:#010x}"
            ));
            break;
        }
    }

    // Diagnostics: EXCI + TRACEPC
    let exci = r(falcon::EXCI);
    let tidx_count = (exci >> 16) & 0xFF;
    let mut tracepc = Vec::new();
    for sp in 0..tidx_count.min(8) {
        w(falcon::EXCI, sp);
        tracepc.push(r(falcon::TRACEPC));
    }
    notes.push(format!(
        "EXCI={exci:#010x} TRACEPC({tidx_count}): {:?}",
        tracepc.iter().map(|v| format!("{v:#010x}")).collect::<Vec<_>>()
    ));

    let sec2_after = Sec2Probe::capture(bar0);
    let fecs_r = |off: usize| bar0.read_u32(falcon::FECS_BASE + off).unwrap_or(0xDEAD);
    let fecs_cpuctl_after = fecs_r(falcon::CPUCTL);
    let fecs_mailbox0_after = fecs_r(falcon::MAILBOX0);
    let gpccs_cpuctl_after = bar0
        .read_u32(falcon::GPCCS_BASE + falcon::CPUCTL)
        .unwrap_or(0xDEAD);

    let success = fecs_cpuctl_after & falcon::CPUCTL_HRESET == 0
        && fecs_mailbox0_after != 0;

    // DMA buffer is dropped here, unmapping it.
    drop(acr_dma);

    AcrBootResult {
        strategy: "ACR chain: DMA-backed SEC2 boot",
        sec2_before,
        sec2_after,
        fecs_cpuctl_after,
        fecs_mailbox0_after,
        gpccs_cpuctl_after,
        success,
        notes,
    }
}

/// Direct ACR firmware load — bypasses the bootloader's DMA transfer.
///
/// Instead of: BL → (DMA) → ACR code → (DMA) → FECS
/// We do:      Host PIO → ACR code in IMEM/DMEM → Start SEC2
///
/// This eliminates the DMA dependency for loading ACR into SEC2, though
/// the ACR firmware itself will still need DMA to load FECS from a WPR.
/// Useful as a diagnostic to determine if the DMA is the sole blocker.
pub fn attempt_direct_acr_load(
    bar0: &MappedBar,
    fw: &AcrFirmwareSet,
) -> AcrBootResult {
    let mut notes = Vec::new();
    let sec2_before = Sec2Probe::capture(bar0);
    let base = falcon::SEC2_BASE;
    let r = |off: usize| bar0.read_u32(base + off).unwrap_or(0xDEAD);
    let w = |off: usize, val: u32| { let _ = bar0.write_u32(base + off, val); };

    // ── Canary test: load tiny program that writes 0xBEEF to MAILBOX0 ──
    // Try multiple falcon ISA encodings since the correct one depends on
    // the falcon version (v5/v6 on GV100 SEC2).
    //
    // Encoding A: fuc5 16-bit immediate MOV (b0/b1 prefix)
    //   mov b16 $r0 0xbeef; mov b16 $r1 0x0040; iowr I[$r1] $r0; exit
    const CANARY_V5_16: &[u8] = &[
        0xb0, 0xef, 0xbe,       // mov b16 $r0 0xbeef
        0xb1, 0x40, 0x00,       // mov b16 $r1 0x0040
        0xf6, 0x10, 0x00,       // iowr I[$r1] $r0
        0xf8, 0x02,             // exit
    ];
    // Encoding B: fuc5 32-bit immediate MOV (f0/f1 prefix)
    const CANARY_V5_32: &[u8] = &[
        0xf0, 0xef, 0xbe, 0x00, 0x00, // mov b32 $r0 0x0000beef
        0xf1, 0x40, 0x00, 0x00, 0x00, // mov b32 $r1 0x00000040
        0xf6, 0x10, 0x00,             // iowr I[$r1] $r0
        0xf8, 0x02,                   // exit
    ];
    // Encoding C: original (may be fuc0/fuc3 format)
    const CANARY_ORIG: &[u8] = &[
        0x80, 0xef, 0xbe, 0x00,
        0x01, 0x40,
        0xf6, 0x10, 0x00,
        0xf8, 0x02,
    ];

    let canaries: &[(&str, &[u8])] = &[
        ("v5_16bit", CANARY_V5_16),
        ("v5_32bit", CANARY_V5_32),
        ("original", CANARY_ORIG),
    ];

    // Try each canary encoding with engine reset + IMEM upload + STARTCPU.
    // Also try CPUCTL_ALIAS (0x130) for starting.
    for (name, code) in canaries {
        if let Err(e) = falcon_engine_reset(bar0, base) {
            notes.push(format!("CANARY {name}: reset failed: {e}"));
            continue;
        }
        let tracepc_pre = r(0x030);
        w(falcon::CPUCTL, falcon::CPUCTL_IINVAL);
        std::thread::sleep(std::time::Duration::from_millis(1));
        falcon_imem_upload_nouveau(bar0, base, 0, code, 0);
        w(falcon::MAILBOX0, 0);
        w(falcon::MAILBOX1, 0);
        w(falcon::BOOTVEC, 0);
        let cpuctl_pre = r(falcon::CPUCTL);

        // Try both CPUCTL and CPUCTL_ALIAS for STARTCPU
        w(falcon::CPUCTL, falcon::CPUCTL_STARTCPU);
        w(falcon::CPUCTL_ALIAS, falcon::CPUCTL_STARTCPU);
        std::thread::sleep(std::time::Duration::from_millis(100));

        let cpuctl_post = r(falcon::CPUCTL);
        let tracepc_post = r(0x030);
        let mb0 = r(falcon::MAILBOX0);
        let ok = mb0 == 0xBEEF;
        notes.push(format!(
            "CANARY {name}: pre_cpuctl={cpuctl_pre:#010x} post={cpuctl_post:#010x} \
             pc_pre={tracepc_pre:#010x} pc_post={tracepc_post:#010x} mb0={mb0:#010x} ok={ok}"
        ));
        if ok {
            notes.push(format!("*** CANARY {name} SUCCEEDED — falcon CAN execute code! ***"));
            break;
        }
    }

    // Method B: HALT the running ROM, then upload + restart.
    // cpuctl=0 means the ROM is running. If we can HALT it (set bit 5),
    // then upload code and STARTCPU to restart with our code.
    w(falcon::CPUCTL, falcon::CPUCTL_HALTED);
    std::thread::sleep(std::time::Duration::from_millis(1));
    let cpuctl_after_halt = r(falcon::CPUCTL);
    notes.push(format!(
        "CANARY B: halt attempt: cpuctl={cpuctl_after_halt:#010x} (bit5={})",
        cpuctl_after_halt & falcon::CPUCTL_HALTED != 0
    ));

    // Also try writing to CPUCTL_ALIAS to halt
    w(falcon::CPUCTL_ALIAS, falcon::CPUCTL_HALTED);
    std::thread::sleep(std::time::Duration::from_millis(1));
    let alias_after_halt = r(falcon::CPUCTL_ALIAS);
    let cpuctl_after_alias_halt = r(falcon::CPUCTL);
    notes.push(format!(
        "CANARY B: alias halt: alias={alias_after_halt:#010x} cpuctl={cpuctl_after_alias_halt:#010x}"
    ));

    // If halted, try to upload and start
    if cpuctl_after_halt & falcon::CPUCTL_HALTED != 0
        || cpuctl_after_alias_halt & falcon::CPUCTL_HALTED != 0
    {
        w(falcon::CPUCTL, falcon::CPUCTL_IINVAL);
        std::thread::sleep(std::time::Duration::from_millis(1));
        falcon_imem_upload_nouveau(bar0, base, 0, CANARY_V5_16, 0);
        w(falcon::MAILBOX0, 0);
        w(falcon::BOOTVEC, 0);
        w(falcon::CPUCTL, falcon::CPUCTL_STARTCPU);
        std::thread::sleep(std::time::Duration::from_millis(100));
        let canary_b_mb0 = r(falcon::MAILBOX0);
        notes.push(format!(
            "CANARY B (halt+start): mb0={canary_b_mb0:#010x} ok={}",
            canary_b_mb0 == 0xBEEF
        ));
    }

    // Method C: Try SCTL register (0x240) — security control might
    // allow halting or state changes.
    let sctl = r(0x240);
    notes.push(format!("SEC2 SCTL: {sctl:#010x}"));
    // Try writing 0 to SCTL to clear security state
    w(0x240, 0);
    std::thread::sleep(std::time::Duration::from_millis(1));
    let sctl_after = r(0x240);
    notes.push(format!("SEC2 SCTL after clear: {sctl_after:#010x}"));

    // Method D: Check EXCI (exception info) and TRACEPC for signs of life
    let exci = r(0x01C);
    let tracepc0 = r(0x030);
    let tracepc1 = r(0x034);
    notes.push(format!(
        "SEC2 EXCI={exci:#010x} TRACEPC[0]={tracepc0:#010x} TRACEPC[1]={tracepc1:#010x}"
    ));

    let parsed = match ParsedAcrFirmware::parse(fw) {
        Ok(p) => p,
        Err(e) => {
            notes.push(format!("Firmware parse failed: {e}"));
            return make_fail_result("Direct ACR: parse failed", sec2_before, bar0, notes);
        }
    };

    // Engine-reset SEC2 for ACR load
    if let Err(e) = falcon_engine_reset(bar0, base) {
        notes.push(format!("ACR reset failed: {e}"));
    }

    // Invalidate IMEM tags
    w(falcon::CPUCTL, falcon::CPUCTL_IINVAL);
    std::thread::sleep(std::time::Duration::from_millis(1));

    // Configure physical DMA mode (Nouveau: gm200_flcn_fw_load non-instance path)
    falcon_prepare_physical_dma(bar0, base);

    // Upload non_sec code to IMEM starting at offset 0, tags starting at 0
    let non_sec_off = parsed.load_header.non_sec_code_off as usize;
    let non_sec_size = parsed.load_header.non_sec_code_size as usize;
    let non_sec_end = (non_sec_off + non_sec_size).min(parsed.acr_payload.len());
    let non_sec_code = &parsed.acr_payload[non_sec_off..non_sec_end];
    falcon_imem_upload_nouveau(bar0, base, 0, non_sec_code, 0);
    notes.push(format!("IMEM: non_sec [{non_sec_off:#x}..{non_sec_end:#x}] → IMEM@0 tag=0"));

    // Upload sec code to IMEM at non_sec_size offset
    if let Some(&(sec_off, sec_size)) = parsed.load_header.apps.first() {
        let sec_off = sec_off as usize;
        let sec_end = (sec_off + sec_size as usize).min(parsed.acr_payload.len());
        let sec_code = &parsed.acr_payload[sec_off..sec_end];
        let imem_addr = non_sec_size as u32;
        let start_tag = (non_sec_size / 256) as u32;
        falcon_imem_upload_nouveau(bar0, base, imem_addr, sec_code, start_tag);
        notes.push(format!(
            "IMEM: sec [{sec_off:#x}..{sec_end:#x}] → IMEM@{imem_addr:#x} tag={start_tag:#x}"
        ));
    }

    // Verify IMEM upload by reading back first 16 bytes
    w(falcon::IMEMC, 0x0200_0000); // read mode, addr=0
    let mut readback = [0u32; 4];
    for word in &mut readback {
        *word = r(falcon::IMEMD);
    }
    let expected = &non_sec_code[..16.min(non_sec_code.len())];
    let readback_bytes: Vec<u8> = readback.iter().flat_map(|w| w.to_le_bytes()).collect();
    let imem_match = readback_bytes[..expected.len()] == *expected;
    notes.push(format!(
        "IMEM verify: read={:02x?} expected={:02x?} match={imem_match}",
        &readback_bytes[..expected.len()], expected
    ));

    // Upload data section to DMEM at offset 0
    let data_off = parsed.load_header.data_dma_base as usize;
    let data_size = parsed.load_header.data_size as usize;
    let data_end = (data_off + data_size).min(parsed.acr_payload.len());
    if data_off < parsed.acr_payload.len() {
        let data = &parsed.acr_payload[data_off..data_end];
        falcon_dmem_upload(bar0, base, 0, data);
        notes.push(format!("DMEM: data [{data_off:#x}..{data_end:#x}] → DMEM@0"));
    }

    // Boot SEC2 at the non_sec entry point (offset 0)
    w(falcon::MAILBOX0, 0);
    w(falcon::MAILBOX1, 0);
    w(falcon::BOOTVEC, 0);
    let cpuctl_pre_start = r(falcon::CPUCTL);
    let alias_en = cpuctl_pre_start & (1 << 6) != 0;
    notes.push(format!(
        "Pre-start cpuctl={cpuctl_pre_start:#010x} alias_en={alias_en}, BOOTVEC=0x0, issuing STARTCPU"
    ));
    falcon_start_cpu(bar0, base);

    // Quick PC sampling (capture falcon state at very short intervals)
    let mut pc_samples = Vec::new();
    for _ in 0..5 {
        std::thread::sleep(std::time::Duration::from_millis(1));
        let cpuctl = r(falcon::CPUCTL);
        let mb0 = r(falcon::MAILBOX0);
        pc_samples.push(format!("cpuctl={cpuctl:#010x} mb0={mb0:#010x}"));
    }
    notes.push(format!("PC samples (1ms intervals): {:?}", pc_samples));

    // Poll for completion
    let timeout = std::time::Duration::from_secs(3);
    let start = std::time::Instant::now();
    loop {
        std::thread::sleep(std::time::Duration::from_millis(10));
        let cpuctl = r(falcon::CPUCTL);
        let mb0 = r(falcon::MAILBOX0);
        let mb1 = r(falcon::MAILBOX1);

        let halted = cpuctl & falcon::CPUCTL_HALTED != 0;
        let hreset_back = cpuctl & falcon::CPUCTL_HRESET != 0;

        if mb0 != 0 || halted || hreset_back {
            notes.push(format!(
                "SEC2 stopped: cpuctl={cpuctl:#010x} mb0={mb0:#010x} mb1={mb1:#010x} ({}ms)",
                start.elapsed().as_millis()
            ));
            break;
        }
        if start.elapsed() > timeout {
            notes.push(format!(
                "SEC2 timeout: cpuctl={cpuctl:#010x} mb0={mb0:#010x} mb1={mb1:#010x}"
            ));
            break;
        }
    }

    // Diagnostics
    let exci = r(falcon::EXCI);
    let tidx_count = (exci >> 16) & 0xFF;
    let mut tracepc = Vec::new();
    for sp in 0..tidx_count.min(8) {
        w(falcon::EXCI, sp);
        tracepc.push(r(falcon::TRACEPC));
    }
    notes.push(format!(
        "EXCI={exci:#010x} TRACEPC({tidx_count}): {:?}",
        tracepc.iter().map(|v| format!("{v:#010x}")).collect::<Vec<_>>()
    ));

    let sec2_after = Sec2Probe::capture(bar0);
    let fecs_r = |off: usize| bar0.read_u32(falcon::FECS_BASE + off).unwrap_or(0xDEAD);
    let fecs_cpuctl_after = fecs_r(falcon::CPUCTL);
    let fecs_mailbox0_after = fecs_r(falcon::MAILBOX0);
    let gpccs_cpuctl_after = bar0
        .read_u32(falcon::GPCCS_BASE + falcon::CPUCTL)
        .unwrap_or(0xDEAD);

    let success = fecs_cpuctl_after & falcon::CPUCTL_HRESET == 0
        && fecs_mailbox0_after != 0;

    AcrBootResult {
        strategy: "Direct ACR IMEM load (no BL DMA)",
        sec2_before,
        sec2_after,
        fecs_cpuctl_after,
        fecs_mailbox0_after,
        gpccs_cpuctl_after,
        success,
        notes,
    }
}

fn make_fail_result(
    strategy: &'static str,
    sec2_before: Sec2Probe,
    bar0: &MappedBar,
    notes: Vec<String>,
) -> AcrBootResult {
    let sec2_after = Sec2Probe::capture(bar0);
    let fecs_r = |off: usize| bar0.read_u32(falcon::FECS_BASE + off).unwrap_or(0xDEAD);
    AcrBootResult {
        strategy,
        sec2_before,
        sec2_after,
        fecs_cpuctl_after: fecs_r(falcon::CPUCTL),
        fecs_mailbox0_after: fecs_r(falcon::MAILBOX0),
        gpccs_cpuctl_after: bar0.read_u32(falcon::GPCCS_BASE + falcon::CPUCTL).unwrap_or(0xDEAD),
        success: false,
        notes,
    }
}

// ── WPR construction (080b) ──────────────────────────────────────────

/// Falcon ID constants used in WPR LSF (Lazy Secure Falcon) descriptors.
/// From Nouveau's `nvkm_acr_lsf_id` enum.
pub mod falcon_id {
    pub const PMU: u32 = 0;
    pub const FECS: u32 = 2;
    pub const GPCCS: u32 = 3;
    pub const SEC2: u32 = 7;
    pub const INVALID: u32 = 0xFFFF_FFFF;
}

// ── Falcon Boot Solver (top-level orchestrator) ──────────────────────

/// Classified FECS state.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FecsState {
    Running,
    InHreset,
    Halted,
    Inaccessible,
}

/// Probe all falcon states relevant to boot strategy selection.
#[derive(Debug)]
pub struct FalconProbe {
    pub fecs_cpuctl: u32,
    pub fecs_mailbox0: u32,
    pub fecs_hwcfg: u32,
    pub gpccs_cpuctl: u32,
    pub sec2: Sec2Probe,
    pub fecs_state: FecsState,
}

impl FalconProbe {
    pub fn capture(bar0: &MappedBar) -> Self {
        let fecs_r = |off: usize| bar0.read_u32(falcon::FECS_BASE + off).unwrap_or(0xDEAD_DEAD);
        let fecs_cpuctl = fecs_r(falcon::CPUCTL);
        let fecs_mailbox0 = fecs_r(falcon::MAILBOX0);
        let fecs_hwcfg = fecs_r(falcon::HWCFG);
        let gpccs_cpuctl = bar0
            .read_u32(falcon::GPCCS_BASE + falcon::CPUCTL)
            .unwrap_or(0xDEAD_DEAD);
        let sec2 = Sec2Probe::capture(bar0);

        let fecs_state = if crate::vfio::channel::registers::pri::is_pri_error(fecs_cpuctl) {
            FecsState::Inaccessible
        } else if fecs_mailbox0 != 0 && fecs_cpuctl & falcon::CPUCTL_HRESET == 0 {
            FecsState::Running
        } else if fecs_cpuctl & falcon::CPUCTL_HRESET != 0 {
            FecsState::InHreset
        } else {
            FecsState::Halted
        };

        Self {
            fecs_cpuctl,
            fecs_mailbox0,
            fecs_hwcfg,
            gpccs_cpuctl,
            sec2,
            fecs_state,
        }
    }
}

impl fmt::Display for FalconProbe {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(f, "Falcon Probe:")?;
        writeln!(
            f,
            "  FECS: {:?} cpuctl={:#010x} mb0={:#010x} hwcfg={:#010x}",
            self.fecs_state, self.fecs_cpuctl, self.fecs_mailbox0, self.fecs_hwcfg
        )?;
        writeln!(f, "  GPCCS: cpuctl={:#010x}", self.gpccs_cpuctl)?;
        write!(f, "  {}", self.sec2)
    }
}

/// Boot strategy selected by the solver.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BootStrategy {
    /// FECS is already running — no boot needed.
    AlreadyRunning,
    /// Direct HRESET experiments (low cost, may not work).
    DirectHreset,
    /// SEC2 EMEM-based ACR boot (works on HS-locked falcon).
    EmemBoot,
    /// SEC2 IMEM-based ACR boot (works on clean-reset falcon).
    ImemBoot,
    /// All strategies exhausted.
    NoViablePath,
}

/// The Falcon Boot Solver — probes GPU state and selects the best
/// strategy for getting FECS running.
pub struct FalconBootSolver;

impl FalconBootSolver {
    /// Probe and attempt to boot FECS using the best available strategy.
    ///
    /// Strategy ordering prioritizes the most faithful Nouveau reproduction:
    ///   0. Already running (free)
    ///   1. Nouveau-style SEC2 boot (corrected reset + IMEM/EMEM + ALIAS_EN)
    ///   2. VRAM-based ACR boot (PRAMIN → VRAM → falcon DMA)
    ///   3. System-memory ACR boot (IOMMU DMA — matches Nouveau arch)
    ///   4. Direct FECS boot (bypass ACR — if FECS in HRESET)
    ///   5. ACR mailbox command (if SEC2 still has live Nouveau ACR)
    ///   6. Direct HRESET experiments
    ///   7. Direct ACR IMEM load (canary test + full ACR firmware)
    ///   8. Full ACR chain with DMA (legacy physical addressing)
    ///   9. EMEM-based boot fallback
    pub fn boot(
        bar0: &MappedBar,
        chip: &str,
        container: Option<DmaBackend>,
    ) -> DriverResult<Vec<AcrBootResult>> {
        let mut results = Vec::new();
        let probe = FalconProbe::capture(bar0);
        tracing::info!("{probe}");

        // Strategy 0: Already running
        if probe.fecs_state == FecsState::Running {
            tracing::info!("FECS already running — no boot needed");
            return Ok(results);
        }

        let fw = match AcrFirmwareSet::load(chip) {
            Ok(fw) => {
                tracing::info!("{}", fw.summary());
                fw
            }
            Err(e) => {
                tracing::error!("Failed to load firmware: {e}");
                return Ok(results);
            }
        };

        // ── Strategy 1: Nouveau-style SEC2 boot ──
        // Most faithful reproduction of Nouveau's gm200_flcn_enable +
        // gm200_flcn_fw_load + gm200_flcn_fw_boot. Uses corrected reset
        // sequence (0x3C0 → PMC enable → scrub → BOOT_0), physical DMA prep,
        // and ALIAS_EN-aware STARTCPU.
        tracing::info!("Strategy 1: Nouveau-style SEC2 boot (corrected sequence)...");
        let nouveau_result = attempt_nouveau_boot(bar0, &fw);
        tracing::info!("{nouveau_result}");
        let nouveau_success = nouveau_result.success;
        results.push(nouveau_result);
        if nouveau_success {
            return Ok(results);
        }

        // ── Strategy 2: VRAM-based ACR boot ──
        // Write ACR payload to VRAM via PRAMIN, then have the BL
        // DMA-load from VRAM addresses (physical DMA stays on-GPU).
        tracing::info!("Strategy 2: VRAM-based ACR boot (PRAMIN→VRAM→falcon DMA)...");
        let vram_result = attempt_vram_acr_boot(bar0, &fw);
        tracing::info!("{vram_result}");
        let vram_success = vram_result.success;
        results.push(vram_result);
        if vram_success {
            return Ok(results);
        }

        // ── Strategy 3: System-memory ACR boot (Exp 083) ──
        // Matches Nouveau's actual architecture: WPR, instance block, and
        // page tables all in IOMMU-mapped system memory DMA buffers.
        if let Some(ref dma_backend) = container {
            tracing::info!("Strategy 3: System-memory ACR boot (IOMMU DMA)...");
            let sysmem_result = attempt_sysmem_acr_boot(bar0, &fw, dma_backend.clone());
            tracing::info!("{sysmem_result}");
            let sysmem_success = sysmem_result.success;
            results.push(sysmem_result);
            if sysmem_success {
                return Ok(results);
            }
        } else {
            tracing::info!("No DMA backend — skipping system-memory ACR boot");
        }

        // ── Strategy 3b: Hybrid ACR boot (VRAM pages + sysmem data) ──
        if let Some(ref dma_backend) = container {
            tracing::info!("Strategy 3b: Hybrid ACR boot (VRAM pages + sysmem data)...");
            let hybrid_result = attempt_hybrid_acr_boot(bar0, &fw, dma_backend.clone());
            tracing::info!("{hybrid_result}");
            let hybrid_success = hybrid_result.success;
            results.push(hybrid_result);
            if hybrid_success {
                return Ok(results);
            }
        }

        // ── Strategy 4: Direct FECS boot (bypass ACR) ──
        if probe.fecs_state == FecsState::InHreset {
            tracing::info!("Strategy 4: Direct FECS boot (bypass ACR)...");
            let fecs_result = attempt_direct_fecs_boot(bar0, &fw);
            tracing::info!("{fecs_result}");
            let fecs_success = fecs_result.success;
            results.push(fecs_result);
            if fecs_success {
                return Ok(results);
            }
        }

        // ── Strategy 5: ACR mailbox command ──
        tracing::info!("Strategy 5: ACR mailbox command (live SEC2)...");
        let mailbox_result = attempt_acr_mailbox_command(bar0);
        tracing::info!("{mailbox_result}");
        let mailbox_success = mailbox_result.success;
        results.push(mailbox_result);
        if mailbox_success {
            return Ok(results);
        }

        // ── Strategy 6: Direct HRESET experiments ──
        tracing::info!("Strategy 6: Direct HRESET experiments...");
        let direct_result = attempt_direct_hreset(bar0);
        tracing::info!("{direct_result}");
        let direct_success = direct_result.success;
        results.push(direct_result);
        if direct_success {
            return Ok(results);
        }

        // ── Strategy 7: Direct ACR IMEM load ──
        tracing::info!("Strategy 7: Direct ACR IMEM load (canary + firmware)...");
        let direct_acr_result = attempt_direct_acr_load(bar0, &fw);
        tracing::info!("{direct_acr_result}");
        let direct_acr_success = direct_acr_result.success;
        results.push(direct_acr_result);
        if direct_acr_success {
            return Ok(results);
        }

        // ── Strategy 8: Full ACR chain with DMA (legacy — physical addressing) ──
        if let Some(dma_backend) = container {
            tracing::info!("Strategy 8: Full ACR chain boot (DMA-backed, legacy)...");
            let chain_result = attempt_acr_chain(bar0, &fw, dma_backend);
            tracing::info!("{chain_result}");
            let chain_success = chain_result.success;
            results.push(chain_result);
            if chain_success {
                return Ok(results);
            }
        } else {
            tracing::info!("No DMA backend — skipping ACR chain");
        }

        // ── Strategy 9: EMEM-based boot fallback ──
        tracing::info!("Strategy 9: EMEM-based SEC2 boot (fallback)...");
        let emem_result = attempt_emem_boot(bar0, &fw);
        tracing::info!("{emem_result}");
        results.push(emem_result);

        Ok(results)
    }
}
