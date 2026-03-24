// SPDX-License-Identifier: AGPL-3.0-only

//! NVIDIA firmware blob parsing for the SEC2→ACR boot chain.
//!
//! Covers `nvfw_bin_hdr`, HS bootloader descriptors, and Nouveau-compatible
//! parsing of `acr/bl.bin` and `acr/ucode_load.bin`.

use std::fmt;

use crate::error::{DriverError, DriverResult};

// ── Firmware structures ──────────────────────────────────────────────

/// NVIDIA firmware binary header (`nvfw_bin_hdr`).
///
/// All NVIDIA firmware blobs (bl.bin, ucode_load.bin) start with this header.
/// Magic = 0x10DE (NVIDIA vendor ID).
///
/// Layout:
///   `0x00` bin_magic      (u32) — always 0x000010DE
///   `0x04` bin_ver        (u32) — header version
///   `0x08` bin_size       (u32) — total file size
///   `0x0C` header_offset  (u32) — offset to type-specific header
///   `0x10` data_offset    (u32) — offset to code/data payload
///   `0x14` data_size      (u32) — size of payload
#[derive(Debug, Clone)]
pub struct NvFwBinHeader {
    /// Firmware binary header: NVIDIA vendor magic (`0x0000_10DE`).
    pub bin_magic: u32,
    /// Firmware binary header: `nvfw_bin_hdr` version.
    pub bin_ver: u32,
    /// Size in bytes of the entire firmware file (including this header).
    pub bin_size: u32,
    /// Byte offset from file start to the chip-specific sub-header.
    pub header_offset: u32,
    /// Byte offset from file start to the code/data payload.
    pub data_offset: u32,
    /// Size in bytes of the payload at `data_offset`.
    pub data_size: u32,
}

impl NvFwBinHeader {
    /// Expected `nvfw_bin_hdr` magic value (`0x0000_10DE`).
    pub const MAGIC: u32 = 0x0000_10DE;

    /// Parses an `nvfw_bin_hdr` from the start of a firmware blob.
    pub fn parse(data: &[u8]) -> DriverResult<Self> {
        if data.len() < 24 {
            return Err(DriverError::DeviceNotFound(
                "firmware file too small for nvfw_bin_hdr".into(),
            ));
        }
        let r = |off: usize| {
            u32::from_le_bytes([data[off], data[off + 1], data[off + 2], data[off + 3]])
        };
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
                format!(
                    "bad nvfw_bin_hdr magic: {:#010x} (expected {:#010x})",
                    hdr.bin_magic,
                    Self::MAGIC
                )
                .into(),
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
            self.bin_magic,
            self.bin_ver,
            self.bin_size,
            self.header_offset,
            self.data_offset,
            self.data_size
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
    /// Parses HS BL descriptor and `nvfw_bin_hdr` from a full `bl.bin`-style image.
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
        self.raw
            .iter()
            .take(64)
            .enumerate()
            .map(|(i, b)| {
                if i > 0 && i % 16 == 0 {
                    format!("\n    {b:02x}")
                } else if i > 0 && i % 4 == 0 {
                    format!("  {b:02x}")
                } else {
                    format!("{b:02x}")
                }
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
    /// Raw `acr/bl.bin` image (ACR secure bootloader blob).
    pub acr_bl_raw: Vec<u8>,
    /// Parsed `nvfw_bin_hdr` and HS sub-header for the ACR bootloader.
    pub acr_bl_parsed: HsBlDescriptor,
    /// Raw `acr/ucode_load.bin` image (ACR ucode payload for the HS loader).
    pub acr_ucode_raw: Vec<u8>,
    /// Parsed headers for `ucode_load.bin`.
    pub acr_ucode_parsed: HsBlDescriptor,
    /// `sec2/desc.bin` — SEC2 descriptor blob for the boot chain.
    pub sec2_desc: Vec<u8>,
    /// `sec2/image.bin` — SEC2 firmware image bytes.
    pub sec2_image: Vec<u8>,
    /// `sec2/sig.bin` — SEC2 signature blob.
    pub sec2_sig: Vec<u8>,
    /// `gr/fecs_bl.bin` — FECS bootloader section.
    pub fecs_bl: Vec<u8>,
    /// `gr/fecs_inst.bin` — FECS IMEM (instruction) image.
    pub fecs_inst: Vec<u8>,
    /// `gr/fecs_data.bin` — FECS DMEM (data) image.
    pub fecs_data: Vec<u8>,
    /// `gr/fecs_sig.bin` — FECS signature blob for WPR/LSF.
    pub fecs_sig: Vec<u8>,
    /// `gr/gpccs_bl.bin` — GPCCS bootloader section.
    pub gpccs_bl: Vec<u8>,
    /// `gr/gpccs_inst.bin` — GPCCS IMEM image.
    pub gpccs_inst: Vec<u8>,
    /// `gr/gpccs_data.bin` — GPCCS DMEM image.
    pub gpccs_data: Vec<u8>,
    /// `gr/gpccs_sig.bin` — GPCCS signature blob for WPR/LSF.
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
            self.acr_bl_raw.len(),
            self.acr_ucode_raw.len(),
            self.sec2_desc.len(),
            self.sec2_image.len(),
            self.sec2_sig.len(),
            self.fecs_bl.len(),
            self.fecs_inst.len(),
            self.fecs_data.len(),
            self.fecs_sig.len(),
            self.gpccs_bl.len(),
            self.gpccs_inst.len(),
            self.gpccs_data.len(),
            self.gpccs_sig.len(),
        )
    }
}

pub(crate) mod dma_idx {
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

/// Parsed HS header from `ucode_load.bin` sub-header.
///
/// Located at `bin_hdr.header_offset` in the file. Contains offsets
/// to signature data and the HS load header within the data payload.
///
/// Layout matches `struct nvfw_hs_header` from Nouveau.
#[derive(Debug, Clone)]
pub struct HsHeader {
    /// File offset to debug (non-production) signature data.
    pub sig_dbg_offset: u32,
    /// Size in bytes of the debug signature region.
    pub sig_dbg_size: u32,
    /// File offset to production signature data.
    pub sig_prod_offset: u32,
    /// Size in bytes of the production signature region.
    pub sig_prod_size: u32,
    /// Indirect patch field: file offset of the u32 patch destination pointer (NVIDIA `0x10de` magic path).
    pub patch_loc: u32,
    /// Indirect patch field: file offset of the u32 signature adjustment (NVIDIA `0x10de` magic path).
    pub patch_sig: u32,
    /// File offset to `nvfw_hs_load_header` (not always relative to data payload).
    pub hdr_offset: u32,
    /// Size in bytes of the HS load header structure.
    pub hdr_size: u32,
}

impl HsHeader {
    /// Parses `nvfw_hs_header` from the `ucode_load.bin` sub-header bytes.
    pub fn parse(data: &[u8]) -> DriverResult<Self> {
        if data.len() < 32 {
            return Err(DriverError::DeviceNotFound("HS header too small".into()));
        }
        let r = |off: usize| {
            u32::from_le_bytes([data[off], data[off + 1], data[off + 2], data[off + 3]])
        };
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
    /// Byte offset to non-secure code within the HS payload.
    pub non_sec_code_off: u32,
    /// Size in bytes of non-secure code.
    pub non_sec_code_size: u32,
    /// DMA base / offset for the HS data segment (loader relocates using this field).
    pub data_dma_base: u32,
    /// Size in bytes of the HS data segment.
    pub data_size: u32,
    /// Number of HS application slots in the following table.
    pub num_apps: u32,
    /// Per-app `(code_offset, code_size)` pairs from the HS load header.
    pub apps: Vec<(u32, u32)>,
}

impl HsLoadHeader {
    /// Parses `nvfw_hs_load_header` from bytes at `HsHeader::hdr_offset` in the file.
    pub fn parse(data: &[u8]) -> DriverResult<Self> {
        if data.len() < 20 {
            return Err(DriverError::DeviceNotFound(
                "HS load header too small".into(),
            ));
        }
        let r = |off: usize| {
            u32::from_le_bytes([data[off], data[off + 1], data[off + 2], data[off + 3]])
        };
        let num_apps = r(16);
        let mut apps = Vec::new();
        let base = 20;
        for i in 0..num_apps as usize {
            let code_off = if base + i * 4 + 4 <= data.len() {
                r(base + i * 4)
            } else {
                0
            };
            let size_idx = num_apps as usize + i;
            let code_size = if base + size_idx * 4 + 4 <= data.len() {
                r(base + size_idx * 4)
            } else {
                0
            };
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
    /// Magic/tag the HS secure loader expects at BL entry (`nvfw_hs_bl_desc`).
    pub bl_start_tag: u32,
    /// DMEM offset where the bootloader writes `flcn_bl_dmem_desc` for the HS loader.
    pub bl_desc_dmem_load_off: u32,
    /// Byte offset to BL code within the BL file payload.
    pub bl_code_off: u32,
    /// Size in bytes of BL code.
    pub bl_code_size: u32,
    /// Size in bytes of the DMEM descriptor (`flcn_bl_dmem_desc_*`).
    pub bl_desc_size: u32,
    /// Byte offset to BL data within the BL file payload.
    pub bl_data_off: u32,
    /// Size in bytes of BL data.
    pub bl_data_size: u32,
}

impl HsBlDesc {
    /// Parses `nvfw_hs_bl_desc` from the `acr/bl.bin` sub-header region.
    pub fn parse(data: &[u8]) -> DriverResult<Self> {
        if data.len() < 28 {
            return Err(DriverError::DeviceNotFound("BL desc too small".into()));
        }
        let r = |off: usize| {
            u32::from_le_bytes([data[off], data[off + 1], data[off + 2], data[off + 3]])
        };
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
            self.bl_start_tag,
            self.bl_desc_dmem_load_off,
            self.bl_code_off,
            self.bl_code_size,
            self.bl_desc_size,
            self.bl_data_off,
            self.bl_data_size,
        )
    }
}

/// Fully parsed ACR firmware ready for the boot chain.
#[derive(Debug)]
pub struct ParsedAcrFirmware {
    /// Parsed HS BL descriptor from `acr/bl.bin`.
    pub bl_desc: HsBlDesc,
    /// Extracted ACR bootloader code bytes (IMEM image for the HS path).
    pub bl_code: Vec<u8>,
    /// Parsed `nvfw_hs_header` from the ucode sub-header.
    pub hs_header: HsHeader,
    /// Parsed `nvfw_hs_load_header` (section layout for HS load).
    pub load_header: HsLoadHeader,
    /// ACR ucode payload bytes (production signature may be patched in-place).
    pub acr_payload: Vec<u8>,
}

impl ParsedAcrFirmware {
    /// Parse bl.bin and ucode_load.bin into structured form.
    pub fn parse(fw: &AcrFirmwareSet) -> DriverResult<Self> {
        let _bl_bin_hdr = &fw.acr_bl_parsed.bin_hdr;
        let bl_sub = &fw.acr_bl_parsed.raw;
        let bl_desc = HsBlDesc::parse(bl_sub)?;

        let bl_payload = fw.acr_bl_parsed.payload(&fw.acr_bl_raw);
        let code_end = (bl_desc.bl_code_off + bl_desc.bl_code_size) as usize;
        let bl_code = if code_end <= bl_payload.len() {
            bl_payload[bl_desc.bl_code_off as usize..code_end].to_vec()
        } else {
            bl_payload.to_vec()
        };

        let _ucode_bin_hdr = &fw.acr_ucode_parsed.bin_hdr;
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
                format!(
                    "HS load header offset {load_hdr_off:#x} beyond file size {}",
                    fw.acr_ucode_raw.len()
                )
                .into(),
            ));
        };
        let load_header = HsLoadHeader::parse(load_hdr_data)?;

        // Patch production signature into ACR payload image.
        // Nouveau: nvkm_falcon_fw_ctor_hs → nvkm_falcon_fw_sign → nvkm_falcon_fw_patch.
        // For 0x10de magic: patch_loc and patch_sig are INDIRECT (file offsets to u32 values).
        let file = &fw.acr_ucode_raw;
        let rd_u32 = |off: usize| -> u32 {
            if off + 4 <= file.len() {
                u32::from_le_bytes([file[off], file[off + 1], file[off + 2], file[off + 3]])
            } else {
                0
            }
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
                sig_patch_loc,
                sig_src,
                sig_size,
                "Patched production signature into ACR payload"
            );
        } else {
            tracing::warn!(
                sig_patch_loc,
                sig_src,
                sig_size,
                file_len = file.len(),
                payload_len = acr_payload.len(),
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
