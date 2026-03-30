// SPDX-License-Identifier: AGPL-3.0-only
//! `coralctl onboard` — hardware onboarding protocol.
//!
//! Runs the firmware probe on a VFIO-bound GPU and produces a structured
//! onboarding report: identity, firmware census, boot path recommendation,
//! and protocol probe results.

use crate::rpc::{check_rpc_error, rpc_call};
use serde::Serialize;
use std::collections::BTreeMap;

/// Structured onboarding report for a GPU under VFIO control.
///
/// Captures identity, firmware census, boot path recommendation, and
/// dispatch readiness in a single JSON-serializable document.
#[derive(Serialize)]
struct OnboardReport {
    bdf: String,
    identity: Identity,
    firmware_census: FirmwareCensus,
    boot_path: BootPathRecommendation,
    dispatch_readiness: DispatchReadiness,
}

/// GPU identity decoded from BOOT0 and health RPC.
#[derive(Serialize)]
struct Identity {
    boot0: String,
    architecture: String,
    chip_id: String,
    pmc_enable: String,
    vram_alive: bool,
    power: String,
}

/// State snapshot for a single falcon engine.
#[derive(Serialize)]
struct FalconCensus {
    name: String,
    cpuctl: String,
    /// CPU stopped / idle (CPUCTL bit 5).
    stopped: bool,
    /// Firmware halted — HALT instruction (CPUCTL bit 4).
    halted: bool,
    sctl: String,
    security_mode: String,
    reachable: bool,
}

/// Firmware census across all observable falcon engines.
#[derive(Serialize)]
struct FirmwareCensus {
    fecs: FalconCensus,
    gpccs: FalconCensus,
    pmu: FalconCensus,
    engines_powered: u32,
    pbdma_count: u32,
}

/// Boot path recommendation based on architecture and security mode.
#[derive(Serialize)]
struct BootPathRecommendation {
    recommended: String,
    reason: String,
    security_level: String,
    requires_reagent: bool,
    reagent: Option<String>,
}

/// Dispatch readiness assessment with concrete blockers.
#[derive(Serialize)]
struct DispatchReadiness {
    pfifo_alive: bool,
    fecs_running: bool,
    gr_enabled: bool,
    ready: bool,
    blockers: Vec<String>,
}

/// Decode GPU architecture name and chip ID from BOOT0.
///
/// Returns `(arch_name, chip_id)` where `chip_id` is `(boot0 >> 20) & 0x1FF`.
fn decode_architecture(boot0: u32) -> (&'static str, u32) {
    let chip = (boot0 >> 20) & 0x1FF;
    let name = match chip {
        0x0E0..=0x0EF => "Kepler",
        0x100..=0x10F => "Maxwell",
        0x120..=0x13F => "Pascal",
        0x140..=0x14F => "Volta",
        0x160..=0x16F => "Turing",
        0x170..=0x17F => "Ampere",
        0x190..=0x19F => "Ada",
        0x1B0..=0x1BF => "Blackwell",
        _ => "Unknown",
    };
    (name, chip)
}

/// Decode falcon security mode from SCTL register bits 12-13.
fn decode_security(sctl: u32) -> &'static str {
    match (sctl >> 12) & 3 {
        0 => "NS (no security)",
        1 => "LS (light security)",
        2 => "HS (high security)",
        3 => "HS+ (locked)",
        _ => "unknown",
    }
}

/// Recommend the sovereign boot path based on architecture and FECS security level.
///
/// Each GPU generation has a different firmware boot mechanism:
/// - **Kepler**: PIO direct (no security)
/// - **Maxwell/Pascal**: nouveau warm handoff (ACR-signed via SEC2)
/// - **Volta**: nouveau warm handoff + livepatch (ACR+FWSEC, WPR2)
/// - **Turing+**: nvidia proprietary GSP
fn recommend_boot_path(arch: &str, fecs_sctl: u32) -> BootPathRecommendation {
    let sec = (fecs_sctl >> 12) & 3;

    match arch {
        "Kepler" => BootPathRecommendation {
            recommended: "PIO direct".to_string(),
            reason: "No firmware security — FECS accepts unsigned code via host PIO upload"
                .to_string(),
            security_level: decode_security(fecs_sctl).to_string(),
            requires_reagent: false,
            reagent: None,
        },
        "Maxwell" | "Pascal" => BootPathRecommendation {
            recommended: "nouveau warm handoff".to_string(),
            reason: "ACR-signed firmware required — nouveau loads via SEC2 ACR chain".to_string(),
            security_level: decode_security(fecs_sctl).to_string(),
            requires_reagent: true,
            reagent: Some("nouveau".to_string()),
        },
        "Volta" => {
            if sec >= 2 {
                BootPathRecommendation {
                    recommended: "nouveau warm handoff (livepatch)".to_string(),
                    reason: "FECS in HS mode via ACR+FWSEC — use nouveau reagent with livepatch to preserve state".to_string(),
                    security_level: decode_security(fecs_sctl).to_string(),
                    requires_reagent: true,
                    reagent: Some("nouveau + livepatch".to_string()),
                }
            } else {
                BootPathRecommendation {
                    recommended: "nouveau warm handoff".to_string(),
                    reason: "WPR2 locked, ACR chain — nouveau reagent required".to_string(),
                    security_level: decode_security(fecs_sctl).to_string(),
                    requires_reagent: true,
                    reagent: Some("nouveau".to_string()),
                }
            }
        }
        "Turing" | "Ampere" | "Ada" | "Blackwell" => BootPathRecommendation {
            recommended: "nvidia proprietary (GSP)".to_string(),
            reason: "GSP-based firmware management — entire driver runs on GPU".to_string(),
            security_level: decode_security(fecs_sctl).to_string(),
            requires_reagent: true,
            reagent: Some("nvidia proprietary".to_string()),
        },
        _ => BootPathRecommendation {
            recommended: "unknown".to_string(),
            reason: format!("Unrecognized architecture: {arch}"),
            security_level: decode_security(fecs_sctl).to_string(),
            requires_reagent: false,
            reagent: None,
        },
    }
}

pub(crate) fn run_onboard(socket: &str, bdf: &str, output: Option<&str>) {
    eprintln!("Onboarding GPU at {bdf}...");

    // Step 1: Health check (identity + firmware)
    let health_resp = rpc_call(socket, "device.health", serde_json::json!({ "bdf": bdf }));
    check_rpc_error(&health_resp);
    let health = match health_resp.get("result") {
        Some(r) => r,
        None => {
            eprintln!("no result from device.health");
            return;
        }
    };

    // Step 2: Full register probe
    let probe_resp = rpc_call(
        socket,
        "device.register_dump",
        serde_json::json!({ "bdf": bdf }),
    );
    check_rpc_error(&probe_resp);
    let regs: BTreeMap<String, u64> = probe_resp
        .get("result")
        .and_then(|r| serde_json::from_value(r.clone()).ok())
        .unwrap_or_default();

    let get_reg = |key: &str| -> u32 {
        regs.get(key)
            .or_else(|| regs.get(&format!("0x{key}")))
            .copied()
            .unwrap_or(0) as u32
    };

    // Parse identity
    let boot0 = health["boot0"].as_u64().unwrap_or(0) as u32;
    let (arch, chip) = decode_architecture(boot0);
    let pmc_enable = health["pmc_enable"].as_u64().unwrap_or(0) as u32;

    // Parse falcon state from health response (using new firmware fields)
    let fecs_cpuctl = health["fecs_cpuctl"].as_u64().unwrap_or(0) as u32;
    let fecs_sctl = health["fecs_sctl"].as_u64().unwrap_or(0) as u32;
    // New RPC keys: fecs_stopped (bit 5), fecs_halted (bit 4). Legacy misnamed them fecs_halted / fecs_hreset.
    let fecs_stopped = if health.get("fecs_stopped").is_some() {
        health["fecs_stopped"].as_bool().unwrap_or(false)
    } else {
        health["fecs_halted"].as_bool().unwrap_or(false)
    };
    let fecs_halted_fw = if health.get("fecs_stopped").is_some() {
        health["fecs_halted"].as_bool().unwrap_or(false)
    } else {
        health["fecs_hreset"].as_bool().unwrap_or(false)
    };
    let gpccs_cpuctl = health["gpccs_cpuctl"].as_u64().unwrap_or(0) as u32;

    // Try to read additional falcon registers from probe dump
    let pmu_cpuctl = get_reg("0x0010a100");
    let pmu_sctl = get_reg("0x0010a240");

    let falcon_reachable = |cpuctl: u32| -> bool {
        cpuctl != 0xBADF_1201 && cpuctl != 0xDEAD_DEAD && cpuctl != 0xFFFF_FFFF
    };

    let firmware_census = FirmwareCensus {
        fecs: FalconCensus {
            name: "FECS".to_string(),
            cpuctl: format!("{fecs_cpuctl:#010x}"),
            stopped: fecs_stopped,
            halted: fecs_halted_fw,
            sctl: format!("{fecs_sctl:#010x}"),
            security_mode: decode_security(fecs_sctl).to_string(),
            reachable: falcon_reachable(fecs_cpuctl),
        },
        gpccs: {
            let gpccs_sctl = get_reg("0x0041a240");
            FalconCensus {
                name: "GPCCS".to_string(),
                cpuctl: format!("{gpccs_cpuctl:#010x}"),
                stopped: gpccs_cpuctl & (1 << 5) != 0,
                halted: gpccs_cpuctl & (1 << 4) != 0,
                sctl: format!("{gpccs_sctl:#010x}"),
                security_mode: decode_security(gpccs_sctl).to_string(),
                reachable: falcon_reachable(gpccs_cpuctl),
            }
        },
        pmu: FalconCensus {
            name: "PMU".to_string(),
            cpuctl: format!("{pmu_cpuctl:#010x}"),
            stopped: pmu_cpuctl & (1 << 5) != 0,
            halted: pmu_cpuctl & (1 << 4) != 0,
            sctl: format!("{pmu_sctl:#010x}"),
            security_mode: decode_security(pmu_sctl).to_string(),
            reachable: falcon_reachable(pmu_cpuctl),
        },
        engines_powered: pmc_enable.count_ones(),
        pbdma_count: {
            let pbdma_map = get_reg("0x00002004");
            pbdma_map.count_ones()
        },
    };

    let boot_path = recommend_boot_path(arch, fecs_sctl);

    // Dispatch readiness
    let pfifo_alive = get_reg("0x00002504") != 0 || get_reg("0x00002200") != 0;
    let fecs_running = falcon_reachable(fecs_cpuctl) && !fecs_stopped && !fecs_halted_fw;
    let gr_enabled = pmc_enable & (1 << 12) != 0;

    let mut blockers = Vec::new();
    if !pfifo_alive {
        blockers.push("PFIFO not alive".to_string());
    }
    if !fecs_running {
        blockers.push(format!(
            "FECS not running (cpuctl={fecs_cpuctl:#010x} stopped={fecs_stopped} halted={fecs_halted_fw})"
        ));
    }
    if !gr_enabled {
        blockers.push("GR engine not enabled in PMC".to_string());
    }
    if !health["vram_alive"].as_bool().unwrap_or(false) {
        blockers.push("VRAM not accessible".to_string());
    }

    let report = OnboardReport {
        bdf: bdf.to_string(),
        identity: Identity {
            boot0: format!("{boot0:#010x}"),
            architecture: format!("{arch} (chip={chip:#05x})"),
            chip_id: format!("{chip:#05x}"),
            pmc_enable: format!("{pmc_enable:#010x}"),
            vram_alive: health["vram_alive"].as_bool().unwrap_or(false),
            power: health["power"].as_str().unwrap_or("unknown").to_string(),
        },
        firmware_census,
        boot_path,
        dispatch_readiness: DispatchReadiness {
            pfifo_alive,
            fecs_running,
            gr_enabled,
            ready: blockers.is_empty(),
            blockers,
        },
    };

    let json = serde_json::to_string_pretty(&report).expect("serialize onboard report");

    match output {
        Some(path) => {
            if let Err(e) = std::fs::write(path, &json) {
                eprintln!("error writing {path}: {e}");
                std::process::exit(1);
            }
            eprintln!("Onboard report written to {path}");
        }
        None => {
            println!("{json}");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── Architecture decode ─────────────────────────────────────────────

    #[test]
    fn decode_kepler() {
        let (arch, chip) = decode_architecture(0x0EA0_00A1);
        assert_eq!(arch, "Kepler");
        assert_eq!(chip, 0x0EA);
    }

    #[test]
    fn decode_maxwell() {
        let (arch, chip) = decode_architecture(0x1000_00A1);
        assert_eq!(arch, "Maxwell");
        assert!((0x100..=0x10F).contains(&chip));
    }

    #[test]
    fn decode_pascal() {
        let (arch, chip) = decode_architecture(0x1320_00A1);
        assert_eq!(arch, "Pascal");
        assert_eq!(chip, 0x132);
    }

    #[test]
    fn decode_volta() {
        let (arch, chip) = decode_architecture(0x1400_00A1);
        assert_eq!(arch, "Volta");
        assert_eq!(chip, 0x140);
    }

    #[test]
    fn decode_turing() {
        let (arch, chip) = decode_architecture(0x1640_00A1);
        assert_eq!(arch, "Turing");
        assert_eq!(chip, 0x164);
    }

    #[test]
    fn decode_ampere() {
        let (arch, chip) = decode_architecture(0x1700_00A1);
        assert_eq!(arch, "Ampere");
        assert_eq!(chip, 0x170);
    }

    #[test]
    fn decode_ada() {
        let (arch, chip) = decode_architecture(0x1900_00A1);
        assert_eq!(arch, "Ada");
        assert_eq!(chip, 0x190);
    }

    #[test]
    fn decode_blackwell() {
        let (arch, chip) = decode_architecture(0x1B60_00A1);
        assert_eq!(arch, "Blackwell");
        assert_eq!(chip, 0x1B6);
    }

    #[test]
    fn decode_unknown() {
        let (arch, _) = decode_architecture(0xFFFF_FFFF);
        assert_eq!(arch, "Unknown");
    }

    // ── Security decode ─────────────────────────────────────────────────

    #[test]
    fn security_ns() {
        assert_eq!(decode_security(0x0000), "NS (no security)");
    }

    #[test]
    fn security_ls() {
        assert_eq!(decode_security(0x1000), "LS (light security)");
    }

    #[test]
    fn security_hs() {
        assert_eq!(decode_security(0x2000), "HS (high security)");
    }

    #[test]
    fn security_hs_plus() {
        assert_eq!(decode_security(0x3000), "HS+ (locked)");
    }

    // ── Boot path recommendation ────────────────────────────────────────

    #[test]
    fn kepler_recommends_pio_direct() {
        let bp = recommend_boot_path("Kepler", 0x0000);
        assert_eq!(bp.recommended, "PIO direct");
        assert!(!bp.requires_reagent);
        assert!(bp.reagent.is_none());
    }

    #[test]
    fn maxwell_recommends_nouveau_warm_handoff() {
        let bp = recommend_boot_path("Maxwell", 0x1000);
        assert!(bp.recommended.contains("nouveau"));
        assert!(bp.requires_reagent);
        assert_eq!(bp.reagent.as_deref(), Some("nouveau"));
    }

    #[test]
    fn pascal_recommends_nouveau_warm_handoff() {
        let bp = recommend_boot_path("Pascal", 0x2000);
        assert!(bp.recommended.contains("nouveau"));
        assert!(bp.requires_reagent);
    }

    #[test]
    fn volta_hs_recommends_livepatch() {
        let bp = recommend_boot_path("Volta", 0x2000);
        assert!(bp.recommended.contains("livepatch"));
        assert!(bp.requires_reagent);
        assert!(bp.reagent.as_deref().unwrap().contains("livepatch"));
    }

    #[test]
    fn volta_ns_recommends_plain_nouveau() {
        let bp = recommend_boot_path("Volta", 0x0000);
        assert!(bp.recommended.contains("nouveau"));
        assert!(!bp.recommended.contains("livepatch"));
    }

    #[test]
    fn turing_recommends_gsp() {
        let bp = recommend_boot_path("Turing", 0x2000);
        assert!(bp.recommended.contains("GSP"));
        assert!(bp.requires_reagent);
    }

    #[test]
    fn ampere_recommends_gsp() {
        let bp = recommend_boot_path("Ampere", 0x2000);
        assert!(bp.recommended.contains("nvidia"));
    }

    #[test]
    fn ada_recommends_gsp() {
        let bp = recommend_boot_path("Ada", 0x2000);
        assert!(bp.recommended.contains("GSP"));
    }

    #[test]
    fn blackwell_recommends_gsp() {
        let bp = recommend_boot_path("Blackwell", 0x2000);
        assert!(bp.recommended.contains("GSP"));
    }

    #[test]
    fn unknown_arch_returns_unknown() {
        let bp = recommend_boot_path("Mystery", 0x0000);
        assert_eq!(bp.recommended, "unknown");
        assert!(bp.reason.contains("Mystery"));
    }

    // ── Falcon reachability ─────────────────────────────────────────────

    #[test]
    fn badf_is_unreachable() {
        let falcon_reachable = |cpuctl: u32| -> bool {
            cpuctl != 0xBADF_1201 && cpuctl != 0xDEAD_DEAD && cpuctl != 0xFFFF_FFFF
        };
        assert!(!falcon_reachable(0xBADF_1201));
        assert!(!falcon_reachable(0xDEAD_DEAD));
        assert!(!falcon_reachable(0xFFFF_FFFF));
        assert!(falcon_reachable(0x0000_0030));
        assert!(falcon_reachable(0));
    }
}
