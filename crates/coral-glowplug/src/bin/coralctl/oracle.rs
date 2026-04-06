// SPDX-License-Identifier: AGPL-3.0-or-later
//! MMU oracle: RPC capture, local VFIO capture, and diff.

use coral_driver::vfio::channel::mmu_oracle;

use crate::rpc::{check_rpc_error, rpc_call};

use serde_json::json;

pub(crate) fn oracle_capture_rpc(
    socket: &str,
    bdf: &str,
    output: Option<&str>,
    max_channels: usize,
) {
    let response = rpc_call(
        socket,
        "device.oracle_capture",
        json!({"bdf": bdf, "max_channels": max_channels}),
    );
    check_rpc_error(&response);

    if let Some(result) = response.get("result") {
        let channel_count = result
            .get("channels")
            .and_then(|c| c.as_array())
            .map_or(0, |a| a.len());
        let total_pts: usize = result
            .get("channels")
            .and_then(|c| c.as_array())
            .map(|chs| {
                chs.iter()
                    .filter_map(|ch| ch.get("page_tables").and_then(|p| p.as_array()))
                    .map(|pts| pts.len())
                    .sum()
            })
            .unwrap_or(0);
        let driver = result.get("driver").and_then(|v| v.as_str()).unwrap_or("?");
        eprintln!("Captured {channel_count} channels, {total_pts} page tables (driver: {driver})");

        let json = serde_json::to_string_pretty(result).expect("serialize");
        match output {
            Some(path) => {
                std::fs::write(path, &json).expect("write output");
                eprintln!("Written to {path}");
            }
            None => println!("{json}"),
        }
    }
}

pub(crate) fn oracle_capture_local(bdf: &str, output: Option<&str>, max_channels: usize) {
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
            std::fs::write(path, &json).expect("write output");
            eprintln!("Written to {path}");
        }
        None => println!("{json}"),
    }
}

pub(crate) fn oracle_diff(left_path: &str, right_path: &str) {
    let left_json = std::fs::read_to_string(left_path).expect("read left");
    let right_json = std::fs::read_to_string(right_path).expect("read right");

    let left: mmu_oracle::PageTableDump = serde_json::from_str(&left_json).expect("parse left");
    let right: mmu_oracle::PageTableDump = serde_json::from_str(&right_json).expect("parse right");

    let diff = mmu_oracle::diff_page_tables(&left, &right);
    mmu_oracle::print_diff_report(&diff);

    let diff_json = serde_json::to_string_pretty(&diff).expect("serialize diff");
    println!("\n{diff_json}");
}
