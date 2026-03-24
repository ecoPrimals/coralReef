// SPDX-License-Identifier: AGPL-3.0-only
//! MMU Oracle — capture full page table chain from any driver state.
//!
//! Usage:
//!   bench_mmu_oracle capture <BDF> [--output <file.json>] [--max-channels N]
//!   bench_mmu_oracle diff <left.json> <right.json>

fn main() {
    let args: Vec<String> = std::env::args().collect();
    if args.len() < 3 {
        eprintln!("Usage:");
        eprintln!(
            "  {} capture <BDF> [--output <file.json>] [--max-channels N]",
            args[0]
        );
        eprintln!("  {} diff <left.json> <right.json>", args[0]);
        std::process::exit(1);
    }

    match args[1].as_str() {
        "capture" => cmd_capture(&args[2..]),
        "diff" => cmd_diff(&args[2..]),
        other => {
            eprintln!("Unknown command: {other}");
            std::process::exit(1);
        }
    }
}

fn cmd_capture(args: &[String]) {
    use coral_driver::vfio::channel::mmu_oracle;

    let bdf = &args[0];
    let mut output: Option<String> = None;
    let mut max_channels: usize = 0;

    let mut i = 1;
    while i < args.len() {
        match args[i].as_str() {
            "--output" | "-o" => {
                i += 1;
                output = Some(args[i].clone());
            }
            "--max-channels" => {
                i += 1;
                max_channels = args[i].parse().unwrap_or(0);
            }
            _ => {
                eprintln!("Unknown flag: {}", args[i]);
                std::process::exit(1);
            }
        }
        i += 1;
    }

    let driver = mmu_oracle::detect_driver(bdf);
    eprintln!("Capturing MMU state from {bdf} (driver: {driver})...");

    let dump = match mmu_oracle::capture_page_tables(bdf, max_channels) {
        Ok(d) => d,
        Err(e) => {
            eprintln!("Capture failed: {e}");
            std::process::exit(1);
        }
    };

    let channel_count = dump.channels.len();
    let total_pts: usize = dump.channels.iter().map(|c| c.page_tables.len()).sum();
    let total_ptes: usize = dump
        .channels
        .iter()
        .flat_map(|c| c.page_tables.iter())
        .map(|pt| pt.entries.len())
        .sum();
    eprintln!("Captured {channel_count} channels, {total_pts} page tables, {total_ptes} PTEs");

    let er = &dump.engine_registers;
    eprintln!(
        "PMU CPUCTL={:#010x} FECS CPUCTL={:#010x} SEC2 CPUCTL={:#010x}",
        er.pmu.get("PMU_FALCON_CPUCTL").unwrap_or(&0),
        er.fecs.get("FECS_FALCON_CPUCTL").unwrap_or(&0),
        er.sec2.get("SEC2_FALCON_CPUCTL").unwrap_or(&0),
    );

    let json = serde_json::to_string_pretty(&dump).expect("serialize");

    match output {
        Some(path) => {
            std::fs::write(&path, &json).expect("write output");
            eprintln!("Written to {path}");
        }
        None => println!("{json}"),
    }
}

fn cmd_diff(args: &[String]) {
    use coral_driver::vfio::channel::mmu_oracle;

    if args.len() < 2 {
        eprintln!("Usage: diff <left.json> <right.json>");
        std::process::exit(1);
    }

    let left_json = std::fs::read_to_string(&args[0]).expect("read left");
    let right_json = std::fs::read_to_string(&args[1]).expect("read right");

    let left: mmu_oracle::PageTableDump = serde_json::from_str(&left_json).expect("parse left");
    let right: mmu_oracle::PageTableDump = serde_json::from_str(&right_json).expect("parse right");

    let diff = mmu_oracle::diff_page_tables(&left, &right);
    mmu_oracle::print_diff_report(&diff);

    let json = serde_json::to_string_pretty(&diff).expect("serialize diff");
    let diff_path = "oracle_diff.json";
    std::fs::write(diff_path, &json).expect("write diff");
    eprintln!("\nStructured diff written to {diff_path}");
}
