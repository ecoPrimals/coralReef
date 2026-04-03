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

use std::borrow::Cow;
use std::collections::HashMap;
use std::path::Path;

use crate::DriverError;
use crate::vfio::device::MappedBar;

use super::boot_follower::{DomainMap, KeplerDomainMap, RecipeStep};
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

/// Domain classification for K80 BIOS recipe writes — delegates to
/// [`KeplerDomainMap`] which owns the canonical Kepler address table.
fn classify_k80_domain(offset: usize, region: &str) -> (&'static str, u32) {
    KeplerDomainMap.classify(offset, region)
}

/// Load a wrapped recipe (object with a `"recipe"` array field).
///
/// Handles hex-string offsets/values (`"0x000160"`) in addition to numeric values.
fn load_wrapped_recipe(val: &serde_json::Value) -> Result<Vec<RecipeStep>, DriverError> {
    let recipe_array = val
        .get("recipe")
        .or_else(|| val.get("steps"))
        .and_then(|v| v.as_array())
        .ok_or_else(|| {
            DriverError::DeviceNotFound(Cow::Borrowed("missing 'recipe' or 'steps' array"))
        })?;

    let mut steps = Vec::with_capacity(recipe_array.len());
    for entry in recipe_array {
        let domain = entry
            .get("domain")
            .and_then(|v| v.as_str())
            .unwrap_or("UNKNOWN")
            .to_string();
        let offset = parse_hex_or_int(entry.get("offset")).unwrap_or(0);
        let value = parse_hex_or_int(entry.get("value")).unwrap_or(0) as u32;
        let priority = entry
            .get("priority")
            .and_then(|v| v.as_u64())
            .unwrap_or(99) as u32;
        steps.push(RecipeStep {
            domain,
            offset,
            value,
            priority,
        });
    }

    tracing::info!(
        steps = steps.len(),
        "loaded wrapped recipe"
    );
    Ok(steps)
}

fn parse_hex_or_int(val: Option<&serde_json::Value>) -> Option<usize> {
    let v = val?;
    if let Some(n) = v.as_u64() {
        return Some(n as usize);
    }
    let s = v.as_str()?;
    let s = s.strip_prefix("0x").or_else(|| s.strip_prefix("0X")).unwrap_or(s);
    usize::from_str_radix(s, 16).ok()
}

/// Load a GK210 BIOS recipe and convert to prioritized RecipeSteps.
pub fn load_bios_recipe(path: &Path) -> Result<Vec<RecipeStep>, DriverError> {
    let content = std::fs::read_to_string(path).map_err(|e| {
        DriverError::DeviceNotFound(Cow::Owned(format!(
            "cannot read BIOS recipe {}: {e}",
            path.display()
        )))
    })?;

    let recipe: BiosRecipe = serde_json::from_str(&content).map_err(|e| {
        DriverError::DeviceNotFound(Cow::Owned(format!(
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

// ── Reagent capture format adapters ─────────────────────────────────────
//
// agentReagents produces two JSON capture formats from VM passthrough sessions:
//
// 1. **Snapshot** (`cold_bar0.json`, `warm_bar0.json`):
//    `{ "bdf": "...", "regions": { "CLK": { "0x130000": "0x98010000", ... }, ... } }`
//
// 2. **Diff** (`nvidia470_cold_warm_diff.json`):
//    `{ "added": { "CLK": { "0x130000": "0x98010000" } },
//       "changed": { "PBUS": { "0x001100": { "cold": "0x0e", "warm": "0x0c" } } },
//       "removed": { ... }, "summary": { ... } }`
//
// Both use hex-string keys/values and nested domain grouping, unlike BiosRecipe
// which uses decimal numbers and a flat writes array.

/// Reagent BAR0 snapshot format (as captured by `snapshot-bar0.py` in reagent VMs).
#[derive(Debug, Clone, serde::Deserialize)]
struct ReagentSnapshot {
    #[allow(unused)]
    bdf: Option<String>,
    regions: HashMap<String, HashMap<String, String>>,
}

/// Reagent cold/warm diff format (produced by diffing cold vs warm snapshots).
#[derive(Debug, Clone, serde::Deserialize)]
struct ReagentDiff {
    #[serde(default)]
    added: HashMap<String, HashMap<String, String>>,
    #[serde(default)]
    changed: HashMap<String, HashMap<String, serde_json::Value>>,
}

fn parse_hex(s: &str) -> Option<u64> {
    let s = s.trim();
    if let Some(hex) = s.strip_prefix("0x").or_else(|| s.strip_prefix("0X")) {
        u64::from_str_radix(hex, 16).ok()
    } else {
        s.parse::<u64>().ok()
    }
}

/// Load a reagent diff JSON and convert to prioritized RecipeSteps.
///
/// Extracts writes from both `added` (new registers in warm state) and
/// `changed` (registers whose value differs, using the `warm` value).
/// Applies `classify_k80_domain` for priority ordering.
pub fn load_reagent_diff(path: &Path) -> Result<Vec<RecipeStep>, DriverError> {
    let content = std::fs::read_to_string(path).map_err(|e| {
        DriverError::DeviceNotFound(Cow::Owned(format!(
            "cannot read reagent diff {}: {e}",
            path.display()
        )))
    })?;

    let diff: ReagentDiff = serde_json::from_str(&content).map_err(|e| {
        DriverError::DeviceNotFound(Cow::Owned(format!(
            "cannot parse reagent diff {}: {e}",
            path.display()
        )))
    })?;

    let mut steps = Vec::new();

    for (domain, registers) in &diff.added {
        for (offset_hex, value_hex) in registers {
            let offset = match parse_hex(offset_hex) {
                Some(v) => v as usize,
                None => continue,
            };
            let value = match parse_hex(value_hex) {
                Some(v) => v as u32,
                None => continue,
            };
            let (classified_domain, priority) = classify_k80_domain(offset, domain);
            steps.push(RecipeStep {
                domain: classified_domain.to_string(),
                offset,
                value,
                priority,
            });
        }
    }

    for (domain, registers) in &diff.changed {
        for (offset_hex, val) in registers {
            let offset = match parse_hex(offset_hex) {
                Some(v) => v as usize,
                None => continue,
            };
            // `changed` entries are `{ "cold": "0x...", "warm": "0x..." }` — use warm value
            let warm_hex = match val.get("warm").and_then(|v| v.as_str()) {
                Some(s) => s,
                None => continue,
            };
            let value = match parse_hex(warm_hex) {
                Some(v) => v as u32,
                None => continue,
            };
            let (classified_domain, priority) = classify_k80_domain(offset, domain);
            steps.push(RecipeStep {
                domain: classified_domain.to_string(),
                offset,
                value,
                priority,
            });
        }
    }

    steps.sort_by_key(|s| (s.priority, s.offset));

    tracing::info!(
        total = steps.len(),
        clock = steps.iter().filter(|s| s.priority <= 2).count(),
        "loaded K80 reagent diff recipe"
    );

    Ok(steps)
}

/// Load a reagent BAR0 snapshot JSON and convert to prioritized RecipeSteps.
///
/// Every register in the snapshot becomes a write step, classified by domain
/// and sorted by priority. Useful for replaying a full warm-state snapshot
/// onto a cold card.
pub fn load_reagent_snapshot(path: &Path) -> Result<Vec<RecipeStep>, DriverError> {
    let content = std::fs::read_to_string(path).map_err(|e| {
        DriverError::DeviceNotFound(Cow::Owned(format!(
            "cannot read reagent snapshot {}: {e}",
            path.display()
        )))
    })?;

    let snap: ReagentSnapshot = serde_json::from_str(&content).map_err(|e| {
        DriverError::DeviceNotFound(Cow::Owned(format!(
            "cannot parse reagent snapshot {}: {e}",
            path.display()
        )))
    })?;

    let mut steps = Vec::new();

    for (domain, registers) in &snap.regions {
        for (offset_hex, value_hex) in registers {
            let offset = match parse_hex(offset_hex) {
                Some(v) => v as usize,
                None => continue,
            };
            let value = match parse_hex(value_hex) {
                Some(v) => v as u32,
                None => continue,
            };
            let (classified_domain, priority) = classify_k80_domain(offset, domain);
            steps.push(RecipeStep {
                domain: classified_domain.to_string(),
                offset,
                value,
                priority,
            });
        }
    }

    steps.sort_by_key(|s| (s.priority, s.offset));

    tracing::info!(
        total = steps.len(),
        clock = steps.iter().filter(|s| s.priority <= 2).count(),
        "loaded K80 reagent snapshot recipe"
    );

    Ok(steps)
}

/// Detected recipe format based on JSON structure.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RecipeFormat {
    /// `BiosRecipe` format: `{ "type": "...", "writes": [...] }`
    BiosRecipe,
    /// Reagent diff format: `{ "added": {...}, "changed": {...} }`
    ReagentDiff,
    /// Reagent snapshot format: `{ "bdf": "...", "regions": {...} }`
    ReagentSnapshot,
    /// Flat `Vec<RecipeStep>` format: `[{ "domain": "...", "offset": ... }]`
    FlatRecipe,
    /// Wrapped format: `{ "recipe": [...], "source": "...", ... }` (mmiotrace distilled)
    WrappedRecipe,
}

/// Returns `true` if a register value is a PRI fault sentinel captured from
/// cold/dead hardware. These leak into recipes when the capture was taken from
/// an un-POST'd GPU and must be stripped before replay.
pub fn is_pri_fault_value(value: u32) -> bool {
    use crate::vfio::channel::registers::pri;
    pri::is_pri_error(value)
}

/// Strip PRI fault values from a loaded recipe. Logs the count of removed
/// entries so the operator knows how much capture garbage was present.
pub fn filter_pri_faults(steps: Vec<RecipeStep>) -> Vec<RecipeStep> {
    let before = steps.len();
    let clean: Vec<RecipeStep> = steps
        .into_iter()
        .filter(|s| !is_pri_fault_value(s.value))
        .collect();
    let removed = before - clean.len();
    if removed > 0 {
        tracing::warn!(
            removed,
            remaining = clean.len(),
            "recipe: stripped PRI fault values from capture"
        );
    }
    clean
}

/// Auto-detect recipe format from a JSON file and load as RecipeSteps.
///
/// Probes the JSON structure to determine which format the file uses,
/// then delegates to the appropriate loader. PRI fault values (0xBADx_xxxx)
/// are automatically stripped from all formats — they are capture artifacts
/// from cold hardware that would poison the replay.
pub fn load_recipe_auto(path: &Path) -> Result<Vec<RecipeStep>, DriverError> {
    let content = std::fs::read_to_string(path).map_err(|e| {
        DriverError::DeviceNotFound(Cow::Owned(format!(
            "cannot read recipe {}: {e}",
            path.display()
        )))
    })?;

    let val: serde_json::Value = serde_json::from_str(&content).map_err(|e| {
        DriverError::DeviceNotFound(Cow::Owned(format!(
            "cannot parse recipe JSON {}: {e}",
            path.display()
        )))
    })?;

    let format = detect_recipe_format(&val);
    tracing::info!(format = ?format, path = %path.display(), "auto-detected recipe format");

    let steps = match format {
        RecipeFormat::BiosRecipe => load_bios_recipe(path),
        RecipeFormat::ReagentDiff => load_reagent_diff(path),
        RecipeFormat::ReagentSnapshot => load_reagent_snapshot(path),
        RecipeFormat::FlatRecipe => replay::load_recipe(path),
        RecipeFormat::WrappedRecipe => load_wrapped_recipe(&val),
    }?;

    Ok(filter_pri_faults(steps))
}

fn detect_recipe_format(val: &serde_json::Value) -> RecipeFormat {
    if val.is_array() {
        return RecipeFormat::FlatRecipe;
    }
    if val.get("type").is_some() && val.get("writes").is_some() {
        return RecipeFormat::BiosRecipe;
    }
    if val.get("added").is_some() || val.get("changed").is_some() {
        return RecipeFormat::ReagentDiff;
    }
    if val.get("regions").is_some() {
        return RecipeFormat::ReagentSnapshot;
    }
    if val.get("recipe").is_some_and(|v| v.is_array()) {
        return RecipeFormat::WrappedRecipe;
    }
    if val.get("steps").is_some_and(|v| v.is_array()) {
        return RecipeFormat::WrappedRecipe;
    }
    RecipeFormat::FlatRecipe
}

/// Configuration for which register domains to include in cold boot replay.
#[derive(Debug, Clone, Default)]
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

    // Verify this is actually a Kepler GPU (GK1xx = 0xE0..0xFF: GK104=0xE4, GK110=0xF0, GK210=0xF2)
    let chip = (pre_snap.boot0 >> 20) & 0x1FF;
    if !(0x0E0..=0x0FF).contains(&chip) {
        return Err(DriverError::DeviceNotFound(Cow::Owned(format!(
            "K80 cold boot requires Kepler GPU, got chip={chip:#05x} ({})",
            pre_snap.architecture
        ))));
    }

    // Phase 1: Load recipe (auto-detects BiosRecipe, reagent diff, or snapshot format)
    // PRI fault values are automatically stripped by load_recipe_auto().
    let full_recipe = load_recipe_auto(recipe_path)?;
    let clock_steps = filter_clock_registers(&full_recipe);
    log.push(format!(
        "clock init: {} registers (ROOT_PLL + PCLOCK + CLK)",
        clock_steps.len()
    ));

    let pll_hooks = replay::KeplerPllHooks::default();
    let clock_result = replay::apply_recipe_phased(bar0, &clock_steps, &pll_hooks)?;
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
    fn reagent_diff_json_deserializes() {
        let json = r#"{
            "added": {
                "CLK": {
                    "0x130000": "0x98010000",
                    "0x130004": "0x00011001"
                },
                "PTIMER": {}
            },
            "changed": {
                "PBUS": {
                    "0x001100": { "cold": "0x0000000e", "warm": "0x0000000c" }
                }
            },
            "summary": { "CLK": { "added": 2 } }
        }"#;
        let diff: ReagentDiff = serde_json::from_str(json).expect("deserialize");
        assert_eq!(diff.added.get("CLK").unwrap().len(), 2);
        assert_eq!(diff.changed.get("PBUS").unwrap().len(), 1);
    }

    #[test]
    fn reagent_diff_to_recipe_steps() {
        let json = r#"{
            "added": {
                "CLK": { "0x130000": "0x98010000" },
                "PMC": { "0x000200": "0x11001100" }
            },
            "changed": {
                "PBUS": {
                    "0x001100": { "cold": "0x0e", "warm": "0x0c" }
                }
            }
        }"#;
        let tmp = std::env::temp_dir().join("test_reagent_diff.json");
        std::fs::write(&tmp, json).unwrap();
        let steps = load_reagent_diff(&tmp).unwrap();
        std::fs::remove_file(&tmp).ok();

        assert_eq!(steps.len(), 3);
        // Clock domain should sort first (priority 2)
        assert_eq!(steps[0].domain, "CLK");
        assert_eq!(steps[0].offset, 0x130000);
        assert_eq!(steps[0].value, 0x98010000);
        // PMC next (priority 3)
        assert_eq!(steps[1].domain, "PMC");
        // PBUS (priority 5), warm value
        assert_eq!(steps[2].domain, "PBUS");
        assert_eq!(steps[2].value, 0x0c);
    }

    #[test]
    fn reagent_snapshot_json_deserializes() {
        let json = r#"{
            "bdf": "0000:05:00.0",
            "regions": {
                "CLK": {
                    "0x130000": "0xbadf3000",
                    "0x130004": "0xbadf3000"
                }
            }
        }"#;
        let snap: ReagentSnapshot = serde_json::from_str(json).expect("deserialize");
        assert_eq!(snap.bdf.as_deref(), Some("0000:05:00.0"));
        assert_eq!(snap.regions.get("CLK").unwrap().len(), 2);
    }

    #[test]
    fn reagent_snapshot_to_recipe_steps() {
        let json = r#"{
            "bdf": "0000:05:00.0",
            "regions": {
                "CLK": { "0x130000": "0x98010000" },
                "PMC": { "0x000200": "0x11001100" }
            }
        }"#;
        let tmp = std::env::temp_dir().join("test_reagent_snapshot.json");
        std::fs::write(&tmp, json).unwrap();
        let steps = load_reagent_snapshot(&tmp).unwrap();
        std::fs::remove_file(&tmp).ok();

        assert_eq!(steps.len(), 2);
        assert_eq!(steps[0].domain, "CLK");
        assert_eq!(steps[1].domain, "PMC");
    }

    #[test]
    fn parse_hex_values() {
        assert_eq!(parse_hex("0x130000"), Some(0x130000));
        assert_eq!(parse_hex("0X0C"), Some(0x0c));
        assert_eq!(parse_hex("0xbadf3000"), Some(0xbadf3000));
        assert_eq!(parse_hex("65536"), Some(65536));
        assert_eq!(parse_hex(""), None);
    }

    #[test]
    fn detect_format_bios_recipe() {
        let json: serde_json::Value = serde_json::from_str(
            r#"{"type": "gk210", "source": "test", "writes": [], "description": "", "total_writes": 0}"#
        ).unwrap();
        assert_eq!(detect_recipe_format(&json), RecipeFormat::BiosRecipe);
    }

    #[test]
    fn detect_format_reagent_diff() {
        let json: serde_json::Value =
            serde_json::from_str(r#"{"added": {}, "changed": {}}"#).unwrap();
        assert_eq!(detect_recipe_format(&json), RecipeFormat::ReagentDiff);
    }

    #[test]
    fn detect_format_reagent_snapshot() {
        let json: serde_json::Value =
            serde_json::from_str(r#"{"bdf": "0:0:0.0", "regions": {}}"#).unwrap();
        assert_eq!(detect_recipe_format(&json), RecipeFormat::ReagentSnapshot);
    }

    #[test]
    fn detect_format_flat_recipe() {
        let json: serde_json::Value = serde_json::from_str(r#"[]"#).unwrap();
        assert_eq!(detect_recipe_format(&json), RecipeFormat::FlatRecipe);
    }

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

    // ── PRI fault filtering ─────────────────────────────────────────────

    #[test]
    fn is_pri_fault_detects_bad_values() {
        assert!(is_pri_fault_value(0xBADF_1234));
        assert!(is_pri_fault_value(0xBAD0_DA1F));
        assert!(is_pri_fault_value(0xBAD1_0000));
        assert!(!is_pri_fault_value(0x9801_0000));
        assert!(!is_pri_fault_value(0x0000_0000));
        assert!(!is_pri_fault_value(0xFFFF_FFFF));
    }

    #[test]
    fn filter_pri_faults_strips_bad_values() {
        let steps = vec![
            RecipeStep {
                domain: "ROOT_PLL".to_string(),
                offset: 0x136400,
                value: 0x0001_0000,
                priority: 0,
            },
            RecipeStep {
                domain: "PTIMER".to_string(),
                offset: 0x009400,
                value: 0xBAD0_DA1F,
                priority: 6,
            },
            RecipeStep {
                domain: "CLK".to_string(),
                offset: 0x130000,
                value: 0x9801_0000,
                priority: 2,
            },
            RecipeStep {
                domain: "PMC".to_string(),
                offset: 0x000200,
                value: 0xBADF_5040,
                priority: 3,
            },
        ];
        let clean = filter_pri_faults(steps);
        assert_eq!(clean.len(), 2);
        assert_eq!(clean[0].domain, "ROOT_PLL");
        assert_eq!(clean[1].domain, "CLK");
    }

    #[test]
    fn filter_pri_faults_preserves_clean_recipe() {
        let steps = vec![
            RecipeStep {
                domain: "ROOT_PLL".to_string(),
                offset: 0x136400,
                value: 0x0001_0000,
                priority: 0,
            },
            RecipeStep {
                domain: "CLK".to_string(),
                offset: 0x130000,
                value: 0x9801_0000,
                priority: 2,
            },
        ];
        let clean = filter_pri_faults(steps);
        assert_eq!(clean.len(), 2);
    }
}
