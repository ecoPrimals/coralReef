// SPDX-License-Identifier: AGPL-3.0-or-later
//! AMD shader binary format detection and metadata extraction.
//!
//! AMD compute shaders are delivered as ELF objects (similar to NVIDIA cubins)
//! containing machine code for the AMDGPU ISA. The ELF contains:
//!
//! - `.text` section: RDNA/GCN instruction stream
//! - `.rodata` section: read-only constant data
//! - `.note` sections: AMDGPU metadata (ISA version, resource usage)
//! - Symbol table: kernel entry points
//!
//! # AMDGPU ISA naming
//!
//! The ISA triple follows: `amdgcn-amd-amdhsa--gfxNNNN` where NNNN is the
//! GFX IP version (e.g. `gfx1030` for RDNA2 Navi 21, `gfx1100` for RDNA3).
//!
//! # Usage
//!
//! ```ignore
//! if is_amdgpu_elf(shader_bytes) {
//!     let meta = parse_amdgpu_metadata(shader_bytes);
//!     // Use meta.gfx_version, meta.sgpr_count, etc. for dispatch config
//! }
//! ```

use crate::error::DriverResult;

const ELF_MAGIC: [u8; 4] = [0x7f, b'E', b'L', b'F'];

/// ELF class (64-bit).
const ELFCLASS64: u8 = 2;
/// ELF data encoding (little-endian).
const ELFDATA2LSB: u8 = 1;
/// ELF OS/ABI for AMDGPU HSA.
const ELFOSABI_AMDGPU_HSA: u8 = 64;
/// ELF machine type for AMDGPU.
const EM_AMDGPU: u16 = 224;

/// AMDGPU ELF note type for ISA version.
const NT_AMDGPU_HSA_ISA: u32 = 3;
/// AMDGPU ELF note type for metadata (YAML/msgpack format).
const NT_AMDGPU_METADATA: u32 = 32;
/// Note name for AMDGPU notes.
const AMDGPU_NOTE_NAME: &[u8] = b"AMDGPU\0";
/// Alternative note vendor name.
const AMD_NOTE_NAME: &[u8] = b"AMD\0";

/// Check whether a byte slice is an AMDGPU ELF binary.
///
/// Validates: ELF magic, 64-bit class, little-endian, and either AMDGPU
/// OS/ABI (64) or AMDGPU machine type (224).
#[must_use]
pub fn is_amdgpu_elf(data: &[u8]) -> bool {
    if data.len() < 20 {
        return false;
    }
    if data[..4] != ELF_MAGIC {
        return false;
    }
    if data[4] != ELFCLASS64 || data[5] != ELFDATA2LSB {
        return false;
    }
    data[7] == ELFOSABI_AMDGPU_HSA || u16::from_le_bytes([data[18], data[19]]) == EM_AMDGPU
}

/// Extracted metadata from an AMDGPU shader ELF.
#[derive(Debug, Clone, Default)]
pub struct AmdgpuShaderMeta {
    /// GFX IP version (e.g. `1030` for gfx1030/RDNA2, `1100` for gfx1100/RDNA3).
    pub gfx_version: u32,
    /// Number of SGPRs used by the kernel.
    pub sgpr_count: u32,
    /// Number of VGPRs used by the kernel.
    pub vgpr_count: u32,
    /// Shared memory (LDS) size in bytes.
    pub lds_size_bytes: u32,
    /// Workgroup size from kernel metadata `[x, y, z]`.
    pub workgroup_size: [u32; 3],
    /// Spill stack size in bytes (scratch memory per thread).
    pub scratch_size: u32,
    /// Kernel code entry byte offset within `.text`.
    pub code_entry_offset: u64,
    /// Whether the binary was recognized as a valid AMDGPU ELF.
    pub valid: bool,
}

/// Parse an AMDGPU ELF to extract shader metadata.
///
/// Scans ELF section headers for `.note` sections containing ISA version
/// and resource usage information. Returns default metadata if parsing fails
/// (the dispatch path can still attempt raw submission).
#[must_use]
pub fn parse_amdgpu_metadata(data: &[u8]) -> AmdgpuShaderMeta {
    let mut meta = AmdgpuShaderMeta::default();

    if !is_amdgpu_elf(data) {
        return meta;
    }
    meta.valid = true;

    if data.len() < 64 {
        return meta;
    }

    // ELF header fields (64-bit LE)
    let e_flags = u32::from_le_bytes(data[0x30..0x34].try_into().unwrap_or_default());
    meta.gfx_version = extract_gfx_version_from_flags(e_flags);

    let e_shoff = u64::from_le_bytes(data[0x28..0x30].try_into().unwrap_or_default()) as usize;
    let e_shentsize = u16::from_le_bytes(data[0x3A..0x3C].try_into().unwrap_or_default()) as usize;
    let e_shnum = u16::from_le_bytes(data[0x3C..0x3E].try_into().unwrap_or_default()) as usize;

    if e_shoff == 0 || e_shentsize < 64 || e_shnum == 0 {
        return meta;
    }

    // Scan section headers for NOTE sections (type = 7)
    for i in 0..e_shnum {
        let sh_start = e_shoff + i * e_shentsize;
        if sh_start + e_shentsize > data.len() {
            break;
        }
        let sh = &data[sh_start..sh_start + e_shentsize];

        let sh_type = u32::from_le_bytes(sh[4..8].try_into().unwrap_or_default());
        if sh_type != 7 {
            // SHT_NOTE
            continue;
        }

        let sh_offset = u64::from_le_bytes(sh[24..32].try_into().unwrap_or_default()) as usize;
        let sh_size = u64::from_le_bytes(sh[32..40].try_into().unwrap_or_default()) as usize;

        parse_notes(data, sh_offset, sh_size, &mut meta);
    }

    meta
}

fn parse_notes(data: &[u8], offset: usize, size: usize, meta: &mut AmdgpuShaderMeta) {
    let end = (offset + size).min(data.len());
    let mut pos = offset;

    while pos + 12 <= end {
        let namesz = u32::from_le_bytes(data[pos..pos + 4].try_into().unwrap_or_default()) as usize;
        let descsz =
            u32::from_le_bytes(data[pos + 4..pos + 8].try_into().unwrap_or_default()) as usize;
        let note_type = u32::from_le_bytes(data[pos + 8..pos + 12].try_into().unwrap_or_default());

        pos += 12;
        let name_end = pos + align4(namesz);
        let desc_start = name_end;
        let desc_end = desc_start + align4(descsz);

        if desc_end > end {
            break;
        }

        let name = &data[pos..pos + namesz.min(data.len() - pos)];
        let is_amdgpu = name.starts_with(AMDGPU_NOTE_NAME)
            || name.starts_with(AMD_NOTE_NAME)
            || name.starts_with(b"AMDGPU");

        if is_amdgpu {
            match note_type {
                NT_AMDGPU_HSA_ISA if descsz >= 12 => {
                    let major = u32::from_le_bytes(
                        data[desc_start..desc_start + 4]
                            .try_into()
                            .unwrap_or_default(),
                    );
                    let minor = u32::from_le_bytes(
                        data[desc_start + 4..desc_start + 8]
                            .try_into()
                            .unwrap_or_default(),
                    );
                    let stepping = u32::from_le_bytes(
                        data[desc_start + 8..desc_start + 12]
                            .try_into()
                            .unwrap_or_default(),
                    );
                    if meta.gfx_version == 0 {
                        meta.gfx_version = major * 100 + minor * 10 + stepping;
                    }
                }
                NT_AMDGPU_METADATA => {
                    parse_metadata_blob(&data[desc_start..desc_start + descsz], meta);
                }
                _ => {}
            }
        }

        pos = desc_end;
    }
}

/// Extract GFX version from ELF e_flags.
///
/// The lower 8 bits of `e_flags` hold the `EF_AMDGPU_MACH` enum value.
/// This maps each enum to the `gfxNNNN` IP version.
fn extract_gfx_version_from_flags(e_flags: u32) -> u32 {
    match e_flags & 0xFF {
        0x28 => 900,  // gfx900  (Vega 10)
        0x29 => 902,  // gfx902  (Vega 12)
        0x2C => 906,  // gfx906  (Vega 20 / MI50)
        0x2F => 908,  // gfx908  (CDNA1 / MI100)
        0x30 => 909,  // gfx909  (placeholder)
        0x31 => 90,   // gfx90a  (CDNA2 / MI200)
        0x32 => 940,  // gfx940
        0x33 => 1010, // gfx1010 (Navi 10)
        0x34 => 1011, // gfx1011 (Navi 12)
        0x35 => 1012, // gfx1012 (Navi 14)
        0x36 => 1030, // gfx1030 (Navi 21 / RDNA2)
        0x37 => 1031, // gfx1031 (Navi 22)
        0x38 => 1032, // gfx1032 (Navi 23)
        0x39 => 1033, // gfx1033 (Navi 24)
        0x3E => 1034, // gfx1034
        0x3F => 1035, // gfx1035
        0x40 => 1036, // gfx1036
        0x41 => 1100, // gfx1100 (Navi 31 / RDNA3)
        0x42 => 1101, // gfx1101 (Navi 32)
        0x43 => 1102, // gfx1102 (Navi 33)
        0x44 => 1103, // gfx1103 (Phoenix)
        0x46 => 1150, // gfx1150 (Strix Point)
        0x47 => 1151, // gfx1151
        0x48 => 1200, // gfx1200 (Navi 48 / RDNA4)
        0x49 => 1201, // gfx1201
        _ => 0,
    }
}

/// Attempt to parse AMDGPU metadata (simplified — handles common fields).
///
/// The full metadata format is msgpack in newer toolchains; older ones used YAML.
/// We extract a minimal set of fields for dispatch configuration.
fn parse_metadata_blob(data: &[u8], meta: &mut AmdgpuShaderMeta) {
    // Look for common msgpack/YAML patterns for resource usage.
    // Full msgpack parsing would require a dependency; we scan for known
    // binary patterns that encode SGPR/VGPR counts.
    if let Some(pos) = find_pattern(data, b".sgpr_count")
        && let Some(val) = read_u32_after_pattern(data, pos + 11)
    {
        meta.sgpr_count = val;
    }
    if let Some(pos) = find_pattern(data, b".vgpr_count")
        && let Some(val) = read_u32_after_pattern(data, pos + 11)
    {
        meta.vgpr_count = val;
    }
    if let Some(pos) = find_pattern(data, b".group_segment_fixed_size")
        && let Some(val) = read_u32_after_pattern(data, pos + 25)
    {
        meta.lds_size_bytes = val;
    }
    if let Some(pos) = find_pattern(data, b".private_segment_fixed_size")
        && let Some(val) = read_u32_after_pattern(data, pos + 27)
    {
        meta.scratch_size = val;
    }
}

fn find_pattern(data: &[u8], pattern: &[u8]) -> Option<usize> {
    data.windows(pattern.len()).position(|w| w == pattern)
}

fn read_u32_after_pattern(data: &[u8], pos: usize) -> Option<u32> {
    // Skip whitespace/separators and try to parse a number
    let mut i = pos;
    while i < data.len() && (data[i] == b':' || data[i] == b' ' || data[i] == b'\n') {
        i += 1;
    }
    // Try reading as a decimal ASCII number
    let mut val = 0u32;
    let mut found = false;
    while i < data.len() && data[i].is_ascii_digit() {
        val = val.wrapping_mul(10).wrapping_add(u32::from(data[i] - b'0'));
        found = true;
        i += 1;
    }
    found.then_some(val)
}

const fn align4(n: usize) -> usize {
    (n + 3) & !3
}

/// Map a GFX version to the human-readable ISA name.
#[must_use]
pub fn gfx_version_name(version: u32) -> &'static str {
    match version {
        900..=909 => "GCN5 (Vega)",
        1010..=1019 => "RDNA1 (Navi 10)",
        1030..=1039 => "RDNA2 (Navi 21-24)",
        1100..=1109 => "RDNA3 (Navi 31-33)",
        1150..=1159 => "RDNA3.5 (Strix Point)",
        1200..=1209 => "RDNA4 (Navi 48-44)",
        _ => "unknown AMDGPU ISA",
    }
}

/// Build a `ShaderInfo` from parsed AMDGPU metadata, suitable for dispatch.
pub fn shader_info_from_meta(
    meta: &AmdgpuShaderMeta,
    default_workgroup: [u32; 3],
) -> crate::ShaderInfo {
    crate::ShaderInfo {
        gpr_count: meta.vgpr_count.max(meta.sgpr_count),
        shared_mem_bytes: meta.lds_size_bytes,
        barrier_count: 0,
        workgroup: if meta.workgroup_size == [0, 0, 0] {
            default_workgroup
        } else {
            meta.workgroup_size
        },
        wave_size: 32,
        local_mem_bytes: if meta.scratch_size > 0 {
            Some(meta.scratch_size)
        } else {
            None
        },
    }
}

/// Validate that a shader binary's GFX version is compatible with the device.
///
/// Returns `Ok(())` if compatible, or a descriptive error if the binary
/// targets a different ISA family.
pub fn validate_gfx_compat(binary_gfx: u32, device_gfx_major: u8) -> DriverResult<()> {
    let binary_major = (binary_gfx / 100) as u8;
    if binary_major != 0 && binary_major != device_gfx_major {
        return Err(crate::error::DriverError::DispatchFailed(
            format!(
                "AMDGPU ISA mismatch: binary targets gfx{binary_gfx} ({}) \
                 but device is GFX{device_gfx_major}",
                gfx_version_name(binary_gfx),
            )
            .into(),
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detect_non_elf() {
        assert!(!is_amdgpu_elf(&[]));
        assert!(!is_amdgpu_elf(&[0; 4]));
        assert!(!is_amdgpu_elf(b"not an elf"));
    }

    #[test]
    fn detect_nvidia_elf_not_amdgpu() {
        let mut fake_elf = vec![0u8; 64];
        fake_elf[..4].copy_from_slice(&ELF_MAGIC);
        fake_elf[4] = ELFCLASS64;
        fake_elf[5] = ELFDATA2LSB;
        fake_elf[7] = 0; // not AMDGPU ABI
        fake_elf[18] = 0xBE; // EM_CUDA = 0xBE
        fake_elf[19] = 0x00;
        assert!(!is_amdgpu_elf(&fake_elf));
    }

    #[test]
    fn detect_amdgpu_by_osabi() {
        let mut fake_elf = vec![0u8; 64];
        fake_elf[..4].copy_from_slice(&ELF_MAGIC);
        fake_elf[4] = ELFCLASS64;
        fake_elf[5] = ELFDATA2LSB;
        fake_elf[7] = ELFOSABI_AMDGPU_HSA;
        assert!(is_amdgpu_elf(&fake_elf));
    }

    #[test]
    fn detect_amdgpu_by_machine() {
        let mut fake_elf = vec![0u8; 64];
        fake_elf[..4].copy_from_slice(&ELF_MAGIC);
        fake_elf[4] = ELFCLASS64;
        fake_elf[5] = ELFDATA2LSB;
        fake_elf[18..20].copy_from_slice(&EM_AMDGPU.to_le_bytes());
        assert!(is_amdgpu_elf(&fake_elf));
    }

    #[test]
    fn gfx_version_from_flags() {
        assert_eq!(extract_gfx_version_from_flags(0x36), 1030); // gfx1030
        assert_eq!(extract_gfx_version_from_flags(0x41), 1100); // gfx1100
        assert_eq!(extract_gfx_version_from_flags(0x28), 900); // gfx900
        assert_eq!(extract_gfx_version_from_flags(0x48), 1200); // gfx1200
        assert_eq!(extract_gfx_version_from_flags(0xFF), 0); // unknown
    }

    #[test]
    fn gfx_version_names() {
        assert_eq!(gfx_version_name(1030), "RDNA2 (Navi 21-24)");
        assert_eq!(gfx_version_name(1100), "RDNA3 (Navi 31-33)");
        assert_eq!(gfx_version_name(1200), "RDNA4 (Navi 48-44)");
        assert_eq!(gfx_version_name(9999), "unknown AMDGPU ISA");
    }

    #[test]
    fn parse_empty_returns_invalid() {
        let meta = parse_amdgpu_metadata(&[]);
        assert!(!meta.valid);
    }

    #[test]
    fn validate_compat_same_major() {
        assert!(validate_gfx_compat(1030, 10).is_ok());
        assert!(validate_gfx_compat(1100, 11).is_ok());
    }

    #[test]
    fn validate_compat_mismatch() {
        let err = validate_gfx_compat(1030, 11);
        assert!(err.is_err());
        let msg = format!("{:?}", err.unwrap_err());
        assert!(msg.contains("ISA mismatch"));
    }

    #[test]
    fn validate_compat_unknown_binary_passes() {
        assert!(validate_gfx_compat(0, 10).is_ok());
    }

    #[test]
    fn shader_info_from_meta_defaults() {
        let meta = AmdgpuShaderMeta {
            vgpr_count: 32,
            sgpr_count: 16,
            lds_size_bytes: 4096,
            ..Default::default()
        };
        let info = shader_info_from_meta(&meta, [64, 1, 1]);
        assert_eq!(info.gpr_count, 32);
        assert_eq!(info.shared_mem_bytes, 4096);
        assert_eq!(info.workgroup, [64, 1, 1]);
    }
}
