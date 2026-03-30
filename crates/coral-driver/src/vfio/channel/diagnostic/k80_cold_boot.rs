// SPDX-License-Identifier: AGPL-3.0-only
//! K80 Cold Boot Orchestrator — sovereign boot from fully cold GK210.
//!
//! The Tesla K80 (GK210 Kepler) has no firmware security: FECS and GPCCS
//! accept unsigned firmware via PIO upload. The only gate is that PLLs and
//! clocks must be configured before falcon CPUs can run.
//!
//! This module orchestrates the full cold boot path:
//! 1. **Clock init** — replay PLL/PCLOCK registers from the BIOS recipe
//! 2. **Full DEVINIT** — replay remaining engine initialization registers
//! 3. **FECS PIO boot** — upload firmware and start FECS/GPCCS
//! 4. **Firmware probe** — capture a [`FirmwareSnapshot`] to verify
//!
//! The BIOS recipe is captured from an nvidia470 VFIO passthrough VM session
//! where the GPU completed full BIOS initialization. We replay those register
//! values to bring a bare-metal cold GPU to the same state.

use std::path::Path;

use crate::DriverError;
use crate::vfio::device::MappedBar;

use super::boot_follower::RecipeStep;
use super::firmware_probe::{self, FirmwareSnapshot};
use super::replay;

/// BIOS recipe JSON format (as captured by the VM pass-through extraction tool).
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
struct BiosRecipe {
    #[serde(rename = "type")]
    recipe_type: String,
    source: String,
    description: String,
    total_writes: usize,
    writes: Vec<BiosWrite>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
struct BiosWrite {
    offset: usize,
    value: u64,
    region: String,
}

/// Domain classification for K80 BIOS recipe writes, with replay priority.
///
/// Priority ordering ensures clock/PLL registers are written first (they must
/// be stable before any falcon CPU can execute), followed by infrastructure
/// (PMC, PBUS, PTIMER), then memory controller, then PFIFO/PBDMA.
///
/// Offsets are classified by BAR0 address range first, falling back to the
/// JSON `region` tag for addresses that don't match a known range.
fn classify_k80_domain(offset: usize, region: &str) -> (&'static str, u32) {
    match offset {
        0x136000..0x137000 => ("ROOT_PLL", 0),
        0x137000..0x138000 => ("PCLOCK", 1),
        0x130000..0x136000 => ("CLK", 2),
        0x000000..0x001000 => ("PMC", 3),
        0x122000..0x123000 => ("PRI_MASTER", 4),
        0x001000..0x002000 => ("PBUS", 5),
        0x009000..0x00A000 => ("PTIMER", 6),
        0x020000..0x024000 => ("PTOP", 7),
        0x100000..0x101000 => ("PFB", 10),
        0x9A0000..0x9B0000 => ("FBPA", 15),
        0x17E000..0x190000 => ("LTC", 16),
        0x10A000..0x10C000 => ("PMU", 20),
        0x002000..0x004000 => ("PFIFO", 25),
        0x040000..0x0A0000 => ("PBDMA", 26),
        0x400000..0x420000 => ("PGRAPH", 30),
        0x800000..0x900000 => ("PCCSR", 35),
        0x700000..0x710000 => ("PRAMIN", 40),
        _ => match region {
            "PCLOCK" => ("PCLOCK", 1),
            "PMC" => ("PMC", 3),
            "PTIMER" => ("PTIMER", 6),
            "PFB" => ("PFB", 10),
            "PFIFO" => ("PFIFO", 25),
            "PGRAPH" => ("PGRAPH", 30),
            "PCOPY" => ("PCOPY", 27),
            "PBUS" => ("PBUS", 5),
            "PROM" => ("PROM", 50),
            "PPCI" => ("PPCI", 8),
            _ => ("UNKNOWN", 99),
        },
    }
}

/// Load a GK210 BIOS recipe and convert to prioritized RecipeSteps.
pub fn load_bios_recipe(path: &Path) -> Result<Vec<RecipeStep>, DriverError> {
    let content = std::fs::read_to_string(path).map_err(|e| {
        DriverError::DeviceNotFound(std::borrow::Cow::Owned(format!(
            "cannot read BIOS recipe {}: {e}",
            path.display()
        )))
    })?;

    let recipe: BiosRecipe = serde_json::from_str(&content).map_err(|e| {
        DriverError::DeviceNotFound(std::borrow::Cow::Owned(format!(
            "cannot parse BIOS recipe {}: {e}",
            path.display()
        )))
    })?;

    let mut steps: Vec<RecipeStep> = recipe
        .writes
        .iter()
        .map(|w| {
            let (domain, priority) = classify_k80_domain(w.offset, &w.region);
            RecipeStep {
                domain: domain.to_string(),
                offset: w.offset,
                value: w.value as u32,
                priority,
            }
        })
        .collect();

    steps.sort_by_key(|s| (s.priority, s.offset));

    tracing::info!(
        total = steps.len(),
        clock = steps.iter().filter(|s| s.priority <= 2).count(),
        "loaded K80 BIOS recipe"
    );

    Ok(steps)
}

/// Extract only the clock/PLL registers from a recipe (priority 0-2).
pub fn filter_clock_registers(steps: &[RecipeStep]) -> Vec<RecipeStep> {
    steps.iter().filter(|s| s.priority <= 2).cloned().collect()
}

/// Configuration for which register domains to include in cold boot replay.
#[derive(Debug, Clone)]
pub struct ColdBootConfig {
    /// Include PGRAPH registers (priority 30, offsets 0x400000..0x420000).
    /// Required for GR engine + FECS execution.
    pub include_pgraph: bool,
    /// Include PCCSR registers (priority 35, offsets 0x800000..0x900000).
    /// Channel/context status registers.
    pub include_pccsr: bool,
    /// Include PRAMIN registers (priority 40, offsets 0x700000..0x710000).
    /// Instance memory / RAMIN window.
    pub include_pramin: bool,
}

impl Default for ColdBootConfig {
    fn default() -> Self {
        Self {
            include_pgraph: false,
            include_pccsr: false,
            include_pramin: false,
        }
    }
}

impl ColdBootConfig {
    /// Full recipe replay — all domains including PGRAPH, PCCSR, PRAMIN.
    pub fn full() -> Self {
        Self {
            include_pgraph: true,
            include_pccsr: true,
            include_pramin: true,
        }
    }

    fn max_priority(&self) -> u32 {
        if self.include_pramin {
            100
        } else if self.include_pccsr {
            40
        } else if self.include_pgraph {
            35
        } else {
            30
        }
    }
}

/// Result of a K80 cold boot attempt.
#[derive(Debug)]
pub struct K80ColdBootResult {
    /// Clock replay result
    pub clock_replay: replay::ReplayResult,
    /// Full DEVINIT replay result (if clock replay succeeded)
    pub devinit_replay: Option<replay::ReplayResult>,
    /// PGRAPH replay result (if PGRAPH was included)
    pub pgraph_replay: Option<replay::ReplayResult>,
    /// Firmware snapshot after boot attempt
    pub firmware_snapshot: FirmwareSnapshot,
    /// Whether FECS appears to be running
    pub fecs_running: bool,
    /// Step-by-step log
    pub log: Vec<String>,
}

/// Execute the full K80 cold boot sequence on a mapped BAR0.
///
/// Phases:
/// 1. Pre-boot firmware probe (capture cold state)
/// 2. Clock init (ROOT_PLL, PCLOCK, CLK domains only)
/// 3. Infrastructure DEVINIT (PMC through PBDMA, priority 3..29)
/// 4. PGRAPH init (priority 30, if config.include_pgraph)
/// 5. FECS/GPCCS PIO boot (if firmware blobs provided)
/// 6. Post-boot firmware probe
pub fn cold_boot(
    bar0: &MappedBar,
    recipe_path: &Path,
    config: &ColdBootConfig,
    fecs_code: Option<&[u8]>,
    fecs_data: Option<&[u8]>,
    gpccs_code: Option<&[u8]>,
    gpccs_data: Option<&[u8]>,
) -> Result<K80ColdBootResult, DriverError> {
    let mut log = Vec::new();

    // Phase 0: Pre-boot firmware probe
    let pre_snap = firmware_probe::capture_firmware_snapshot(bar0, "k80-cold-pre-boot");
    firmware_probe::log_firmware_summary(&pre_snap);
    log.push(format!(
        "pre-boot: BOOT0={:#010x} arch={}",
        pre_snap.boot0, pre_snap.architecture
    ));

    // Verify this is actually a Kepler GPU
    let chip = (pre_snap.boot0 >> 20) & 0x1FF;
    if !(0x0E0..=0x0EF).contains(&chip) {
        return Err(DriverError::DeviceNotFound(std::borrow::Cow::Owned(
            format!(
                "K80 cold boot requires Kepler GPU, got chip={chip:#05x} ({})",
                pre_snap.architecture
            ),
        )));
    }

    // Phase 1: Load recipe and replay clocks first
    let full_recipe = load_bios_recipe(recipe_path)?;
    let clock_steps = filter_clock_registers(&full_recipe);
    log.push(format!(
        "clock init: {} registers (ROOT_PLL + PCLOCK + CLK)",
        clock_steps.len()
    ));

    let clock_result = replay::apply_recipe_to_bar0(bar0, &clock_steps)?;
    log.push(format!(
        "clock replay: applied={} failed={} ptimer_ticking={}",
        clock_result.applied, clock_result.failed, clock_result.ptimer_ticking
    ));

    if !clock_result.ptimer_ticking {
        log.push("WARNING: PTIMER not ticking after clock init — FECS may not start".into());
    }

    // Phase 2: Infrastructure DEVINIT (PMC through PBDMA, priority 3..29)
    let devinit_steps: Vec<RecipeStep> = full_recipe
        .iter()
        .filter(|s| s.priority > 2 && s.priority < 30)
        .cloned()
        .collect();
    log.push(format!(
        "devinit: {} registers (PMC through PBDMA)",
        devinit_steps.len()
    ));

    let devinit_result = replay::apply_recipe_to_bar0(bar0, &devinit_steps)?;
    log.push(format!(
        "devinit replay: applied={} failed={}",
        devinit_result.applied, devinit_result.failed
    ));

    // Phase 2b: Extended domains (PGRAPH, PCCSR, PRAMIN) per config
    let max_prio = config.max_priority();
    let pgraph_replay = if max_prio > 30 || config.include_pgraph {
        let extended_steps: Vec<RecipeStep> = full_recipe
            .iter()
            .filter(|s| {
                s.priority >= 30
                    && s.priority < max_prio
                    && match s.priority {
                        30 => config.include_pgraph,
                        35 => config.include_pccsr,
                        40 => config.include_pramin,
                        _ => true,
                    }
            })
            .cloned()
            .collect();
        if extended_steps.is_empty() {
            None
        } else {
            let domains: Vec<&str> = {
                let mut d = Vec::new();
                if config.include_pgraph {
                    d.push("PGRAPH");
                }
                if config.include_pccsr {
                    d.push("PCCSR");
                }
                if config.include_pramin {
                    d.push("PRAMIN");
                }
                d
            };
            log.push(format!(
                "extended init: {} registers ({})",
                extended_steps.len(),
                domains.join("+")
            ));
            let result = replay::apply_recipe_to_bar0(bar0, &extended_steps)?;
            log.push(format!(
                "extended replay: applied={} failed={}",
                result.applied, result.failed
            ));
            Some(result)
        }
    } else {
        log.push("PGRAPH/PCCSR/PRAMIN: skipped per config".into());
        None
    };

    // Phase 3: FECS/GPCCS PIO boot (if firmware provided)
    let mut fecs_running = false;
    if let (Some(fc), Some(fd), Some(gc), Some(gd)) = (fecs_code, fecs_data, gpccs_code, gpccs_data)
    {
        log.push(format!(
            "FECS PIO boot: fecs_code={}B fecs_data={}B gpccs_code={}B gpccs_data={}B",
            fc.len(),
            fd.len(),
            gc.len(),
            gd.len()
        ));

        use crate::gsp::RegisterAccess;
        struct Bar0Adapter<'a>(&'a MappedBar);

        impl RegisterAccess for Bar0Adapter<'_> {
            fn read_u32(&self, offset: u32) -> Result<u32, crate::gsp::ApplyError> {
                self.0
                    .read_u32(offset as usize)
                    .map_err(|e| crate::gsp::ApplyError::MmioFailed {
                        offset,
                        detail: e.to_string(),
                    })
            }
            fn write_u32(&mut self, offset: u32, value: u32) -> Result<(), crate::gsp::ApplyError> {
                self.0.write_u32(offset as usize, value).map_err(|e| {
                    crate::gsp::ApplyError::MmioFailed {
                        offset,
                        detail: e.to_string(),
                    }
                })
            }
        }

        let mut adapter = Bar0Adapter(bar0);

        // Enable GR engine in PMC if not already
        let pmc_en = bar0.read_u32(0x200).unwrap_or(0);
        if pmc_en & (1 << 12) == 0 {
            log.push("enabling GR engine in PMC_ENABLE (bit 12)".into());
            let _ = bar0.write_u32(0x200, pmc_en | (1 << 12));
            std::thread::sleep(std::time::Duration::from_millis(10));
        }

        // PMC unk260 toggle (nouveau does this around falcon load)
        if let Err(e) = crate::nv::kepler_falcon::pmc_unk260(&mut adapter, true) {
            log.push(format!("pmc_unk260(true) failed: {e}"));
        }

        match crate::nv::kepler_falcon::boot_fecs_gpccs(
            &mut adapter,
            fc,
            fd,
            gc,
            gd,
            std::time::Duration::from_secs(5),
        ) {
            Ok(()) => {
                fecs_running = true;
                log.push("FECS boot SUCCESS — firmware responded".into());
            }
            Err(e) => {
                log.push(format!("FECS boot FAILED: {e}"));
            }
        }

        if let Err(e) = crate::nv::kepler_falcon::pmc_unk260(&mut adapter, false) {
            log.push(format!("pmc_unk260(false) failed: {e}"));
        }
    } else {
        log.push("FECS PIO boot skipped — no firmware blobs provided".into());
    }

    // Phase 4: Post-boot firmware probe
    let post_snap = firmware_probe::capture_firmware_snapshot(bar0, "k80-cold-post-boot");
    firmware_probe::log_firmware_summary(&post_snap);

    let diffs = firmware_probe::diff_snapshots(&pre_snap, &post_snap);
    log.push(format!("firmware diff: {} registers changed", diffs.len()));
    for (path, old, new) in &diffs {
        log.push(format!("  {path}: {old} -> {new}"));
    }

    Ok(K80ColdBootResult {
        clock_replay: clock_result,
        devinit_replay: Some(devinit_result),
        pgraph_replay,
        firmware_snapshot: post_snap,
        fecs_running,
        log,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── Domain classification ───────────────────────────────────────────

    #[test]
    fn classify_root_pll_by_offset() {
        let (domain, priority) = classify_k80_domain(0x136400, "");
        assert_eq!(domain, "ROOT_PLL");
        assert_eq!(priority, 0);
    }

    #[test]
    fn classify_pclock_by_offset() {
        let (domain, priority) = classify_k80_domain(0x137020, "");
        assert_eq!(domain, "PCLOCK");
        assert_eq!(priority, 1);
    }

    #[test]
    fn classify_clk_by_offset() {
        let (domain, priority) = classify_k80_domain(0x132000, "");
        assert_eq!(domain, "CLK");
        assert_eq!(priority, 2);
    }

    #[test]
    fn classify_pmc_by_offset() {
        let (domain, priority) = classify_k80_domain(0x000200, "");
        assert_eq!(domain, "PMC");
        assert_eq!(priority, 3);
    }

    #[test]
    fn classify_pfifo_by_offset() {
        let (domain, priority) = classify_k80_domain(0x002504, "");
        assert_eq!(domain, "PFIFO");
        assert_eq!(priority, 25);
    }

    #[test]
    fn classify_pgraph_by_offset() {
        let (domain, priority) = classify_k80_domain(0x400100, "");
        assert_eq!(domain, "PGRAPH");
        assert_eq!(priority, 30);
    }

    #[test]
    fn classify_fallback_to_region_tag() {
        let (domain, _) = classify_k80_domain(0xFFF000, "PCLOCK");
        assert_eq!(domain, "PCLOCK");
    }

    #[test]
    fn classify_unknown_offset_and_region() {
        let (domain, priority) = classify_k80_domain(0xFFF000, "MYSTERY");
        assert_eq!(domain, "UNKNOWN");
        assert_eq!(priority, 99);
    }

    #[test]
    fn clock_domains_have_lowest_priority() {
        let clock_priority: Vec<u32> = [
            classify_k80_domain(0x136000, "").1,
            classify_k80_domain(0x137000, "").1,
            classify_k80_domain(0x130000, "").1,
        ]
        .to_vec();

        let non_clock_priority = classify_k80_domain(0x000200, "").1;

        for p in &clock_priority {
            assert!(
                *p < non_clock_priority,
                "clock priority {p} should be less than PMC priority {non_clock_priority}"
            );
        }
    }

    // ── Recipe loading and filtering ────────────────────────────────────

    #[test]
    fn bios_recipe_json_deserializes() {
        let json = r#"{
            "type": "gk210_bios_init_recipe",
            "source": "test",
            "description": "test recipe",
            "total_writes": 3,
            "writes": [
                {"offset": 137000, "value": 65536, "region": "PCLOCK"},
                {"offset": 512, "value": 4096, "region": "PMC"},
                {"offset": 1267712, "value": 255, "region": "ROOT_PLL"}
            ]
        }"#;
        let recipe: BiosRecipe = serde_json::from_str(json).expect("deserialize");
        assert_eq!(recipe.writes.len(), 3);
        assert_eq!(recipe.recipe_type, "gk210_bios_init_recipe");
    }

    #[test]
    fn filter_clock_registers_extracts_priority_0_through_2() {
        let steps = vec![
            RecipeStep {
                domain: "ROOT_PLL".to_string(),
                offset: 0x136400,
                value: 0xFF,
                priority: 0,
            },
            RecipeStep {
                domain: "PCLOCK".to_string(),
                offset: 0x137020,
                value: 0x10000,
                priority: 1,
            },
            RecipeStep {
                domain: "CLK".to_string(),
                offset: 0x132000,
                value: 0x01,
                priority: 2,
            },
            RecipeStep {
                domain: "PMC".to_string(),
                offset: 0x000200,
                value: 0x1100,
                priority: 3,
            },
            RecipeStep {
                domain: "PFIFO".to_string(),
                offset: 0x002504,
                value: 0x01,
                priority: 25,
            },
        ];

        let clocks = filter_clock_registers(&steps);
        assert_eq!(clocks.len(), 3);
        assert!(clocks.iter().all(|s| s.priority <= 2));
    }

    #[test]
    fn filter_clock_registers_empty_on_no_clocks() {
        let steps = vec![RecipeStep {
            domain: "PMC".to_string(),
            offset: 0x200,
            value: 0x1100,
            priority: 3,
        }];
        assert!(filter_clock_registers(&steps).is_empty());
    }
}
