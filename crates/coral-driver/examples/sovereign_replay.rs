// SPDX-License-Identifier: AGPL-3.0-only
//! Sovereign Replay — staged register-level GPU init from captured nouveau recipe.
//!
//! Usage:
//!   cargo run --example sovereign_replay --features vfio --release -- \
//!       --bdf 0000:03:00.0 \
//!       --recipe /path/to/nouveau_recipe.json \
//!       [--reference /path/to/warm_bar0.json] \
//!       [--stage PMC_ENGINE_GATING,CLOCK_PLL,...] \
//!       [--dry-run]
//!
//! Loads a StagedRecipe JSON (produced by extract-recipe.py or boot_follower.rs),
//! opens the GPU via VFIO, and replays register writes stage by stage. After each
//! stage, reads back key registers and optionally diffs against a nouveau reference
//! BAR0 snapshot.
//!
//! # Safety Warning
//!
//! This example opens VFIO **directly**, bypassing ember's fork-isolated
//! safety layer. A BAR0 write to an uninitialized GPU can cause a PCIe
//! bus hang that freezes the entire system. In production, always route
//! through `coralctl sovereign init` which uses ember's staged pipeline.

use coral_driver::vfio::device::{MappedBar, VfioDevice};
use coral_driver::vfio::channel::diagnostic::boot_follower::{
    InitStage, StagedRecipe, StageBlock,
};

use std::collections::BTreeMap;
use std::path::PathBuf;

const PTIMER_TIME_0: usize = 0x9400;
const PTIMER_TIME_1: usize = 0x9410;
const PMC_BOOT_0: usize = 0x0;
const PMC_ENABLE: usize = 0x200;

/// Per-stage validation probe — registers to read after each stage is replayed.
struct StageProbe {
    stage: InitStage,
    probes: &'static [(usize, &'static str)],
}

const STAGE_PROBES: &[StageProbe] = &[
    StageProbe {
        stage: InitStage::PmcEngineGating,
        probes: &[
            (PMC_BOOT_0, "PMC_BOOT_0"),
            (PMC_ENABLE, "PMC_ENABLE"),
            (0x000204, "PMC_ENABLE_HI"),
        ],
    },
    StageProbe {
        stage: InitStage::ClockPll,
        probes: &[
            (0x137000, "PCLOCK_0"),
            (0x137004, "PCLOCK_1"),
            (0x136000, "ROOT_PLL_0"),
        ],
    },
    StageProbe {
        stage: InitStage::PriTopology,
        probes: &[
            (0x022430, "PTOP_DEVICE_INFO_0"),
            (0x022434, "PTOP_DEVICE_INFO_1"),
            (0x120100, "PRI_RING_INTR"),
        ],
    },
    StageProbe {
        stage: InitStage::PfbMemory,
        probes: &[
            (0x100000, "PFB_CFG0"),
            (0x100C10, "PFB_NISO_FLUSH_SYSMEM_ADDR"),
            (0x100800, "FBHUB_CFG"),
        ],
    },
    StageProbe {
        stage: InitStage::FalconBoot,
        probes: &[
            (0x409100, "FECS_CPUCTL"),
            (0x409240, "FECS_SCTL"),
            (0x41A100, "GPCCS_CPUCTL"),
            (0x41A240, "GPCCS_SCTL"),
            (0x840100, "SEC2_CPUCTL"),
            (0x10A100, "PMU_CPUCTL"),
        ],
    },
    StageProbe {
        stage: InitStage::GrEngineInit,
        probes: &[
            (0x400100, "PGRAPH_INTR"),
            (0x400108, "PGRAPH_FECS_INTR"),
            (0x400700, "GR_STATUS"),
        ],
    },
    StageProbe {
        stage: InitStage::PfifoChannel,
        probes: &[
            (0x002100, "PFIFO_INTR_0"),
            (0x002140, "PFIFO_INTR_EN_0"),
            (0x040100, "PBDMA0_INTR"),
        ],
    },
];

struct StageResult {
    stage: InitStage,
    writes_applied: usize,
    writes_failed: usize,
    probe_values: Vec<(usize, &'static str, u32)>,
    reference_mismatches: Vec<(usize, &'static str, u32, u32)>,
}

/// Reference BAR0 snapshot loaded from warm_bar0.json.
fn load_reference(path: &PathBuf) -> Result<BTreeMap<usize, u32>, String> {
    let content = std::fs::read_to_string(path)
        .map_err(|e| format!("cannot read reference {}: {e}", path.display()))?;

    let raw: serde_json::Value =
        serde_json::from_str(&content).map_err(|e| format!("invalid JSON: {e}"))?;

    let mut regs = BTreeMap::new();

    if let Some(regions) = raw.get("regions").and_then(|r| r.as_object()) {
        for (_region_name, region_regs) in regions {
            if let Some(obj) = region_regs.as_object() {
                for (offset_str, val_str) in obj {
                    let offset = usize::from_str_radix(
                        offset_str.trim_start_matches("0x"),
                        16,
                    )
                    .unwrap_or(0);
                    let val = u32::from_str_radix(
                        val_str.as_str().unwrap_or("0").trim_start_matches("0x"),
                        16,
                    )
                    .unwrap_or(0);
                    regs.insert(offset, val);
                }
            }
        }
    }

    Ok(regs)
}

fn replay_stage(
    bar0: &MappedBar,
    block: &StageBlock,
    reference: Option<&BTreeMap<usize, u32>>,
    dry_run: bool,
) -> StageResult {
    let mut applied = 0usize;
    let mut failed = 0usize;

    if !dry_run {
        for w in &block.writes {
            match bar0.write_u32(w.offset, w.value) {
                Ok(()) => applied += 1,
                Err(e) => {
                    eprintln!(
                        "  WARN: write {:#08x} = {:#010x} failed: {e}",
                        w.offset, w.value
                    );
                    failed += 1;
                }
            }
        }
    } else {
        applied = block.writes.len();
    }

    let probe = STAGE_PROBES
        .iter()
        .find(|p| p.stage == block.stage);

    let mut probe_values = Vec::new();
    let mut reference_mismatches = Vec::new();

    if let Some(probe) = probe {
        for &(offset, name) in probe.probes {
            let val = if dry_run {
                0xDEAD_BEEF
            } else {
                bar0.read_u32(offset).unwrap_or(0xDEAD_DEAD)
            };
            probe_values.push((offset, name, val));

            if let Some(ref_map) = reference {
                if let Some(&ref_val) = ref_map.get(&offset) {
                    if val != ref_val && !dry_run {
                        reference_mismatches.push((offset, name, val, ref_val));
                    }
                }
            }
        }
    }

    StageResult {
        stage: block.stage,
        writes_applied: applied,
        writes_failed: failed,
        probe_values,
        reference_mismatches,
    }
}

fn print_stage_result(result: &StageResult) {
    let status = if result.writes_failed > 0 {
        "WARN"
    } else if !result.reference_mismatches.is_empty() {
        "DIFF"
    } else {
        "OK"
    };

    println!(
        "  [{status}] {}: {} applied, {} failed",
        result.stage, result.writes_applied, result.writes_failed,
    );

    for &(offset, name, val) in &result.probe_values {
        println!("    {name:30} [{offset:#08x}] = {val:#010x}");
    }

    for &(offset, name, actual, expected) in &result.reference_mismatches {
        println!(
            "    MISMATCH {name} [{offset:#08x}]: got {actual:#010x}, ref {expected:#010x}",
        );
    }
}

fn check_gpu_alive(bar0: &MappedBar) -> (u32, bool) {
    let boot0 = bar0.read_u32(PMC_BOOT_0).unwrap_or(0xFFFF_FFFF);

    let t0_a = bar0.read_u32(PTIMER_TIME_0).unwrap_or(0);
    let t1_a = bar0.read_u32(PTIMER_TIME_1).unwrap_or(0);
    for _ in 0..1000 {
        let _ = bar0.read_u32(PMC_BOOT_0);
    }
    let t0_b = bar0.read_u32(PTIMER_TIME_0).unwrap_or(0);
    let t1_b = bar0.read_u32(PTIMER_TIME_1).unwrap_or(0);

    let ticking = t0_a != t0_b || t1_a != t1_b;
    (boot0, ticking)
}

fn parse_stages(arg: &str) -> Vec<InitStage> {
    arg.split(',')
        .filter_map(|s| {
            let s = s.trim();
            InitStage::all_ordered()
                .iter()
                .find(|st| st.as_str() == s)
                .copied()
        })
        .collect()
}

fn main() {
    eprintln!("WARNING: This example opens VFIO directly, bypassing ember's safety layer.");
    eprintln!("WARNING: BAR0 writes on a cold/partial GPU can freeze the entire system.");
    eprintln!("WARNING: Use `coralctl sovereign init` for safe, staged initialization.\n");

    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .init();

    let args: Vec<String> = std::env::args().collect();

    let mut bdf = String::new();
    let mut recipe_path = PathBuf::new();
    let mut reference_path: Option<PathBuf> = None;
    let mut stage_filter: Option<Vec<InitStage>> = None;
    let mut dry_run = false;

    let mut i = 1;
    while i < args.len() {
        match args[i].as_str() {
            "--bdf" => {
                bdf = args.get(i + 1).cloned().unwrap_or_default();
                i += 2;
            }
            "--recipe" => {
                recipe_path = PathBuf::from(args.get(i + 1).cloned().unwrap_or_default());
                i += 2;
            }
            "--reference" => {
                reference_path =
                    Some(PathBuf::from(args.get(i + 1).cloned().unwrap_or_default()));
                i += 2;
            }
            "--stage" => {
                stage_filter = Some(parse_stages(
                    args.get(i + 1).map_or("", String::as_str),
                ));
                i += 2;
            }
            "--dry-run" => {
                dry_run = true;
                i += 1;
            }
            other => {
                eprintln!("Unknown argument: {other}");
                std::process::exit(1);
            }
        }
    }

    if bdf.is_empty() || recipe_path.as_os_str().is_empty() {
        eprintln!("Usage: sovereign_replay --bdf <BDF> --recipe <path> [--reference <path>] [--stage S1,S2,...] [--dry-run]");
        std::process::exit(1);
    }

    println!("=== Sovereign Replay ===");
    println!("BDF:       {bdf}");
    println!("Recipe:    {}", recipe_path.display());
    if let Some(ref rp) = reference_path {
        println!("Reference: {}", rp.display());
    }
    if dry_run {
        println!("Mode:      DRY RUN (no MMIO writes)");
    }
    println!();

    let recipe = StagedRecipe::load(&recipe_path).unwrap_or_else(|e| {
        eprintln!("Failed to load recipe: {e}");
        std::process::exit(1);
    });

    println!(
        "Recipe: {} total writes across {} stages (GPU: {}, driver: {})",
        recipe.total_writes,
        recipe.stages.len(),
        recipe.gpu,
        recipe.driver,
    );
    for block in &recipe.stages {
        println!("  {:20} {} writes", block.stage.as_str(), block.writes.len());
    }
    println!();

    let reference = reference_path.as_ref().map(|p| {
        load_reference(p).unwrap_or_else(|e| {
            eprintln!("Failed to load reference: {e}");
            std::process::exit(1);
        })
    });

    if !dry_run {
        println!("Opening VFIO device {bdf}...");
        let device = VfioDevice::open(&bdf).unwrap_or_else(|e| {
            eprintln!("Failed to open VFIO device: {e}");
            std::process::exit(1);
        });

        let bar0 = device.map_bar(0).unwrap_or_else(|e| {
            eprintln!("Failed to map BAR0: {e}");
            std::process::exit(1);
        });

        let (boot0, ticking) = check_gpu_alive(&bar0);
        println!(
            "Pre-replay: PMC_BOOT_0={boot0:#010x}, PTIMER ticking={ticking}"
        );
        println!();

        println!("--- Replaying stages ---");
        let mut total_applied = 0usize;
        let mut total_failed = 0usize;
        let mut total_mismatches = 0usize;

        for block in &recipe.stages {
            if let Some(ref filter) = stage_filter {
                if !filter.contains(&block.stage) {
                    println!("  [SKIP] {}", block.stage);
                    continue;
                }
            }

            let result = replay_stage(&bar0, block, reference.as_ref(), false);
            total_applied += result.writes_applied;
            total_failed += result.writes_failed;
            total_mismatches += result.reference_mismatches.len();
            print_stage_result(&result);
        }

        println!();
        let (boot0, ticking) = check_gpu_alive(&bar0);
        println!(
            "Post-replay: PMC_BOOT_0={boot0:#010x}, PTIMER ticking={ticking}"
        );
        println!(
            "Totals: {total_applied} applied, {total_failed} failed, {total_mismatches} mismatches vs reference"
        );

        if boot0 == 0xFFFF_FFFF {
            println!("FATAL: GPU unresponsive after replay");
            std::process::exit(2);
        }
        if !ticking {
            println!("WARNING: PTIMER not ticking after replay");
        }
    } else {
        println!("--- Dry run: stage analysis ---");
        for block in &recipe.stages {
            if let Some(ref filter) = stage_filter {
                if !filter.contains(&block.stage) {
                    println!("  [SKIP] {}", block.stage);
                    continue;
                }
            }
            println!(
                "  {:20} {} writes (offsets {:#08x}..{:#08x})",
                block.stage.as_str(),
                block.writes.len(),
                block.writes.first().map_or(0, |w| w.offset),
                block.writes.last().map_or(0, |w| w.offset),
            );
        }
    }

    println!("\n=== Sovereign Replay Complete ===");
}
