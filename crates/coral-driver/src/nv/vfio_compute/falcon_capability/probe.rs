// SPDX-License-Identifier: AGPL-3.0-or-later

use crate::error::{DriverError, DriverResult};
use crate::vfio::channel::registers::falcon;
use crate::vfio::device::MappedBar;

use super::DEAD_READ;
use super::types::{CpuCtlLayout, FalconCapabilities, FalconVersion, PioLayout, SecurityMode};

const TEST_PATTERN: u32 = 0xCAFE_BABE;

/// Probe a single falcon instance and discover its capabilities.
///
/// Safe to call on any falcon (FECS, GPCCS, SEC2, PMU) — the probe reads
/// HWCFG to determine version, then performs non-destructive PIO validation
/// if the falcon appears accessible (not in DEAD state).
pub fn probe_falcon(bar0: &MappedBar, name: &str, base: usize) -> DriverResult<FalconCapabilities> {
    let r = |off: usize| bar0.read_u32(base + off).unwrap_or(DEAD_READ);

    let hwcfg_raw = r(falcon::HWCFG);
    if hwcfg_raw == DEAD_READ {
        return Err(DriverError::DeviceNotFound(
            format!("{name} @ {base:#08x}: HWCFG reads as DEAD — falcon not accessible").into(),
        ));
    }

    let mut anomalies = Vec::new();

    let version = detect_version(hwcfg_raw, &mut anomalies);
    let cpuctl = detect_cpuctl_layout(&version);
    let security = decode_security_mode(r(falcon::SCTL), &mut anomalies);
    let sctl_raw = r(falcon::SCTL);
    let imem_size = falcon::imem_size_bytes(hwcfg_raw);
    let dmem_size = falcon::dmem_size_bytes(hwcfg_raw);
    let requires_signed_fw = hwcfg_raw & falcon::HWCFG_SECURITY_MODE != 0;

    if imem_size == 0 {
        anomalies.push(format!("IMEM size=0 from HWCFG {hwcfg_raw:#010x}"));
    }
    if dmem_size == 0 {
        anomalies.push(format!("DMEM size=0 from HWCFG {hwcfg_raw:#010x}"));
    }

    let pio = probe_pio_layout(bar0, base, dmem_size, &mut anomalies);

    Ok(FalconCapabilities {
        name: name.to_string(),
        base,
        version,
        cpuctl,
        pio,
        security,
        sctl_raw,
        imem_size,
        dmem_size,
        requires_signed_fw,
        anomalies,
    })
}

/// Probe all standard GV100 falcon instances.
pub fn probe_all_falcons(bar0: &MappedBar) -> Vec<FalconCapabilities> {
    let targets = [
        ("FECS", falcon::FECS_BASE),
        ("GPCCS", falcon::GPCCS_BASE),
        ("SEC2", falcon::SEC2_BASE),
        ("PMU", falcon::PMU_BASE),
    ];

    targets
        .iter()
        .filter_map(|&(name, base)| match probe_falcon(bar0, name, base) {
            Ok(caps) => Some(caps),
            Err(e) => {
                tracing::warn!(name, base = format!("{base:#08x}"), error = %e, "falcon probe failed");
                None
            }
        })
        .collect()
}

fn detect_version(hwcfg: u32, anomalies: &mut Vec<String>) -> FalconVersion {
    // Falcon version is encoded in HWCFG bits [27:24] on GM200+ (envytools: CORE_REV).
    // On older falcons, these bits may be zero or encode differently.
    let core_rev = (hwcfg >> 24) & 0xF;

    // Heuristic: falcon v0 has core_rev=0, v1-v5 have nonzero core_rev.
    // GV100 falcons are v4/v5 (core_rev >= 4).
    let major = match core_rev {
        0 => {
            // Could be v0 (pre-GM200) or just unset. Use IMEM size heuristic:
            // v0 falcons typically have smaller IMEM (<=16KB).
            let imem = falcon::imem_size_bytes(hwcfg);
            if imem > 0 && imem <= 16384 {
                0
            } else {
                anomalies.push(format!(
                    "HWCFG core_rev=0 with large IMEM ({imem}B) — defaulting to v4"
                ));
                4
            }
        }
        1..=3 => {
            // GM200 era: falcon v1-v3
            core_rev as u8
        }
        4..=5 => core_rev as u8,
        _ => {
            anomalies.push(format!("unexpected HWCFG core_rev={core_rev}"));
            core_rev as u8
        }
    };

    FalconVersion {
        major,
        hwcfg_raw: hwcfg,
    }
}

fn detect_cpuctl_layout(version: &FalconVersion) -> CpuCtlLayout {
    if version.major >= 4 {
        // Falcon v4+ (GM200+): IINVAL=BIT(0), STARTCPU=BIT(1)
        CpuCtlLayout {
            startcpu: 1 << 1,
            iinval: 1 << 0,
            hreset: 1 << 4,
            halted: 1 << 5,
        }
    } else {
        // Falcon v0-v3: STARTCPU=BIT(0), no separate IINVAL
        CpuCtlLayout {
            startcpu: 1 << 0,
            iinval: 0,
            hreset: 1 << 4,
            halted: 1 << 5,
        }
    }
}

fn decode_security_mode(sctl: u32, anomalies: &mut Vec<String>) -> SecurityMode {
    // envytools SEC_MODE at offset 0x240: bits[13:12] encode security level.
    // 0=NS, 1=LS, 2=HS. Value 3 is undocumented.
    //
    // NVIDIA internal "SCTL" terminology uses a wider field.
    // We decode both the 2-bit SEC_MODE and flag any extra bits.
    let sec_level = (sctl >> 12) & 0x3;
    let mode = match sec_level {
        0 => SecurityMode::NonSecure,
        1 => SecurityMode::LightSecure,
        2 => SecurityMode::HeavySecure,
        _ => {
            anomalies.push(format!(
                "SCTL={sctl:#010x}: sec_level={sec_level} is undocumented (bits[13:12]=0b11)"
            ));
            SecurityMode::Unknown(sec_level)
        }
    };

    // Flag additional set bits outside the known SEC_MODE field
    let known_mask = 0x3000; // bits [13:12]
    let extra = sctl & !known_mask;
    if extra != 0 {
        anomalies.push(format!(
            "SCTL={sctl:#010x}: extra bits outside SEC_MODE: {extra:#010x}"
        ));
    }

    mode
}

/// Candidate bit positions to try for PIO auto-increment.
const WRITE_CANDIDATES: &[(u32, &str)] = &[
    (1 << 24, "BIT(24) — GM200+/nouveau"),
    (1 << 6, "BIT(6) — legacy/envytools v0"),
    (1 << 8, "BIT(8) — speculative"),
];

const READ_CANDIDATES: &[(u32, &str)] = &[
    (1 << 25, "BIT(25) — GM200+/nouveau"),
    (1 << 7, "BIT(7) — legacy/envytools v0"),
];

fn probe_pio_layout(
    bar0: &MappedBar,
    base: usize,
    dmem_size: u32,
    anomalies: &mut Vec<String>,
) -> PioLayout {
    // Default: the GM200+ layout (BIT(24)/BIT(25)/BIT(28)) which is correct
    // for all falcon instances we've validated on GV100.
    let mut layout = PioLayout {
        write_autoinc: 1 << 24,
        read_autoinc: 1 << 25,
        secure_flag: 1 << 28,
        write_validated: false,
        read_validated: false,
    };

    // Only attempt PIO validation if DMEM exists and is large enough for a test word.
    if dmem_size < 8 {
        anomalies.push("DMEM too small for PIO validation".to_string());
        return layout;
    }

    // Use DMEM for probing because it's simpler (no tags, no secure page complexity).
    // Write a test pattern using each candidate bit, then read it back.
    let test_addr: u32 = 0;

    // Save original DMEM[0] before we clobber it.
    let original = read_dmem_word(bar0, base, test_addr, layout.read_autoinc);

    // Try write candidates
    for &(write_bit, label) in WRITE_CANDIDATES {
        write_dmem_word(bar0, base, test_addr, write_bit, TEST_PATTERN);

        // Read back with each read candidate to find the working combo
        for &(read_bit, _) in READ_CANDIDATES {
            let readback = read_dmem_word(bar0, base, test_addr, read_bit);
            if readback == TEST_PATTERN {
                layout.write_autoinc = write_bit;
                layout.read_autoinc = read_bit;
                layout.write_validated = true;
                layout.read_validated = true;

                if write_bit != (1 << 24) {
                    anomalies.push(format!(
                        "PIO write uses {label} ({write_bit:#010x}), not BIT(24)"
                    ));
                }

                // Restore original value
                write_dmem_word(bar0, base, test_addr, write_bit, original);
                return probe_secure_flag(bar0, base, &mut layout, anomalies);
            }
        }
    }

    // None of the candidates produced a valid readback.
    // This can happen if the falcon is in a state that blocks DMEM PIO
    // (e.g., running firmware occupying DMEM, or truly inaccessible).
    anomalies.push(
        "PIO validation failed: no write+read candidate produced correct readback. \
                    Falcon may be running or DMEM inaccessible. Using default GM200+ layout."
            .to_string(),
    );

    layout
}

fn probe_secure_flag(
    bar0: &MappedBar,
    base: usize,
    layout: &mut PioLayout,
    anomalies: &mut Vec<String>,
) -> PioLayout {
    // The secure flag (expected BIT(28)) marks IMEM pages as secure.
    // When read back, secure pages return 0xdead5ec1 instead of actual content.
    // We can detect this by writing to IMEM with the flag and checking the sentinel.
    //
    // This test is only safe if the falcon is halted (not executing from IMEM).
    // For now, we trust the default BIT(28) since it matches nouveau source
    // for all falcon versions we've seen.
    //
    // A full validation would: write IMEM[high_addr] with BIT(28), read back,
    // check for 0xdead5ec1. But IMEM writes clobber firmware, so we skip this
    // on running falcons.
    let cpuctl = bar0.read_u32(base + falcon::CPUCTL).unwrap_or(DEAD_READ);
    let halted_or_hreset = cpuctl & ((1 << 4) | (1 << 5)) != 0;

    if !halted_or_hreset && cpuctl != DEAD_READ {
        anomalies.push(
            "skipped IMEM secure-flag probe: falcon appears running (CPUCTL indicates active)"
                .to_string(),
        );
    }

    *layout
}

fn write_dmem_word(bar0: &MappedBar, base: usize, addr: u32, write_bit: u32, value: u32) {
    let _ = bar0.write_u32(base + falcon::DMEMC, write_bit | addr);
    let _ = bar0.write_u32(base + falcon::DMEMD, value);
}

fn read_dmem_word(bar0: &MappedBar, base: usize, addr: u32, read_bit: u32) -> u32 {
    let _ = bar0.write_u32(base + falcon::DMEMC, read_bit | addr);
    bar0.read_u32(base + falcon::DMEMD).unwrap_or(DEAD_READ)
}

#[cfg(test)]
mod tests {
    use super::{decode_security_mode, detect_cpuctl_layout, detect_version};
    use crate::nv::vfio_compute::falcon_capability::types::{FalconVersion, SecurityMode};

    #[test]
    fn security_mode_decode_ns() {
        let mut a = Vec::new();
        assert_eq!(
            decode_security_mode(0x0000, &mut a),
            SecurityMode::NonSecure
        );
        assert!(a.is_empty());
    }

    #[test]
    fn security_mode_decode_ls() {
        let mut a = Vec::new();
        assert_eq!(
            decode_security_mode(0x1000, &mut a),
            SecurityMode::LightSecure
        );
        assert!(a.is_empty());
    }

    #[test]
    fn security_mode_decode_hs() {
        let mut a = Vec::new();
        assert_eq!(
            decode_security_mode(0x2000, &mut a),
            SecurityMode::HeavySecure
        );
        assert!(a.is_empty());
    }

    #[test]
    fn security_mode_decode_unknown_3() {
        let mut a = Vec::new();
        let mode = decode_security_mode(0x3000, &mut a);
        assert!(matches!(mode, SecurityMode::Unknown(3)));
        assert_eq!(a.len(), 1, "should flag undocumented sec_level=3");
    }

    #[test]
    fn security_mode_extra_bits_flagged() {
        let mut a = Vec::new();
        decode_security_mode(0x3001, &mut a);
        assert!(
            a.len() >= 2,
            "should flag both unknown level and extra bit 0"
        );
    }

    #[test]
    fn cpuctl_layout_v4_plus() {
        let v = FalconVersion {
            major: 4,
            hwcfg_raw: 0,
        };
        let layout = detect_cpuctl_layout(&v);
        assert_eq!(layout.startcpu, 1 << 1);
        assert_eq!(layout.iinval, 1 << 0);
        assert_eq!(layout.hreset, 1 << 4);
        assert_eq!(layout.halted, 1 << 5);
    }

    #[test]
    fn cpuctl_layout_v0() {
        let v = FalconVersion {
            major: 0,
            hwcfg_raw: 0,
        };
        let layout = detect_cpuctl_layout(&v);
        assert_eq!(layout.startcpu, 1 << 0);
        assert_eq!(layout.iinval, 0);
    }

    #[test]
    fn version_detect_high_core_rev() {
        let mut a = Vec::new();
        let v = detect_version(0x0400_0000 | 0x140, &mut a); // core_rev=4, some IMEM
        assert_eq!(v.major, 4);
        assert!(a.is_empty());
    }

    #[test]
    fn version_detect_zero_core_rev_small_imem() {
        let mut a = Vec::new();
        let hwcfg = 0x0000_0040; // 64 * 256 = 16384B IMEM, core_rev=0
        let v = detect_version(hwcfg, &mut a);
        assert_eq!(v.major, 0);
    }
}
