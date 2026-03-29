// SPDX-License-Identifier: AGPL-3.0-only
//! `experiment sweep` RPC orchestration: personality matrix, timing tables, ember journal summary.

use crate::rpc::rpc_call;

use serde_json::json;

use super::ember_socket;

/// Default personalities to sweep when none specified.
const DEFAULT_SWEEP_PERSONALITIES: &[&str] = &["nouveau", "amdgpu", "nvidia-open", "xe", "i915"];

struct SweepResult {
    bdf: String,
    personality: String,
    _iteration: u32,
    success: bool,
    total_ms: u64,
    bind_ms: u64,
    unbind_ms: u64,
    trace_path: Option<String>,
    insights: usize,
    error: Option<String>,
}

fn sweep_single_card(
    socket: &str,
    bdf: &str,
    targets: &[&str],
    return_to: &str,
    trace: bool,
    repeat: u32,
) -> Vec<SweepResult> {
    let total_ops = targets.len() as u32 * repeat;
    let mut results: Vec<SweepResult> = Vec::new();
    let mut step = 0u32;

    for target in targets {
        for iter in 0..repeat {
            step += 1;
            let iter_label = if repeat > 1 {
                format!("{target} (iter {}/{})", iter + 1, repeat)
            } else {
                target.to_string()
            };
            println!("\n[{step}/{total_ops}] {bdf} -> {iter_label}");

            let swap_resp = rpc_call(
                socket,
                "device.swap",
                json!({
                    "bdf": bdf,
                    "target": target,
                    "trace": trace,
                }),
            );

            if let Some(error) = swap_resp.get("error") {
                let msg = error
                    .get("message")
                    .and_then(|v| v.as_str())
                    .unwrap_or("unknown error");
                println!("  FAILED: {msg}");
                results.push(SweepResult {
                    bdf: bdf.to_string(),
                    personality: target.to_string(),
                    _iteration: iter,
                    success: false,
                    total_ms: 0,
                    bind_ms: 0,
                    unbind_ms: 0,
                    trace_path: None,
                    insights: 0,
                    error: Some(msg.to_string()),
                });
            } else if let Some(result) = swap_resp.get("result") {
                let obs = result
                    .get("observation")
                    .and_then(|v| if v.is_null() { None } else { Some(v) });
                let total_ms = obs
                    .and_then(|o| o.get("total_ms"))
                    .and_then(|v| v.as_u64())
                    .unwrap_or(0);
                let bind_ms = obs
                    .and_then(|o| o.get("bind_ms"))
                    .and_then(|v| v.as_u64())
                    .unwrap_or(0);
                let unbind_ms = obs
                    .and_then(|o| o.get("unbind_ms"))
                    .and_then(|v| v.as_u64())
                    .unwrap_or(0);
                let trace_path = obs
                    .and_then(|o| o.get("trace_path"))
                    .and_then(|v| v.as_str())
                    .map(|s| s.to_string());
                let insights = result
                    .get("insights")
                    .and_then(|v| v.as_array())
                    .map(|a| a.len())
                    .unwrap_or(0);
                let vram = result
                    .get("vram_alive")
                    .and_then(|v| v.as_bool())
                    .unwrap_or(false);

                println!("  OK: {iter_label} ({total_ms}ms, bind={bind_ms}ms, vram={vram})");
                if let Some(ref tp) = trace_path {
                    println!("  Trace: {tp}");
                }
                if insights > 0 {
                    println!("  Observer insights: {insights}");
                }

                results.push(SweepResult {
                    bdf: bdf.to_string(),
                    personality: target.to_string(),
                    _iteration: iter,
                    success: true,
                    total_ms,
                    bind_ms,
                    unbind_ms,
                    trace_path,
                    insights,
                    error: None,
                });
            }

            if *target != return_to {
                print!("  Returning to {return_to}...");
                let ret_resp = rpc_call(
                    socket,
                    "device.swap",
                    json!({
                        "bdf": bdf,
                        "target": return_to,
                    }),
                );
                if ret_resp.get("error").is_some() {
                    println!(" FAILED (experiment may be in inconsistent state)");
                    return results;
                }
                println!(" ok");
            }
        }
    }

    results
}

/// Compute per-personality aggregates (avg, min, max, stddev) from successful results.
struct PersonalityAggregate {
    personality: String,
    bdf: String,
    count: u32,
    avg_total_ms: u64,
    min_total_ms: u64,
    max_total_ms: u64,
    stddev_total_ms: f64,
    avg_bind_ms: u64,
    avg_unbind_ms: u64,
    fail_count: u32,
}

fn aggregate_results(results: &[SweepResult]) -> Vec<PersonalityAggregate> {
    use std::collections::BTreeMap;
    let mut groups: BTreeMap<(String, String), Vec<&SweepResult>> = BTreeMap::new();
    for r in results {
        groups
            .entry((r.bdf.clone(), r.personality.clone()))
            .or_default()
            .push(r);
    }

    groups
        .into_iter()
        .map(|((bdf, personality), runs)| {
            let ok_runs: Vec<_> = runs.iter().filter(|r| r.success).collect();
            let fail_count = runs.len() as u32 - ok_runs.len() as u32;
            let count = ok_runs.len() as u32;
            if count == 0 {
                return PersonalityAggregate {
                    personality,
                    bdf,
                    count: 0,
                    avg_total_ms: 0,
                    min_total_ms: 0,
                    max_total_ms: 0,
                    stddev_total_ms: 0.0,
                    avg_bind_ms: 0,
                    avg_unbind_ms: 0,
                    fail_count,
                };
            }
            let totals: Vec<u64> = ok_runs.iter().map(|r| r.total_ms).collect();
            let sum: u64 = totals.iter().sum();
            let avg = sum / count as u64;
            let min = *totals.iter().min().unwrap_or(&0);
            let max = *totals.iter().max().unwrap_or(&0);
            let mean_f = sum as f64 / count as f64;
            let variance = totals
                .iter()
                .map(|&t| {
                    let d = t as f64 - mean_f;
                    d * d
                })
                .sum::<f64>()
                / count as f64;
            let stddev = variance.sqrt();

            let bind_sum: u64 = ok_runs.iter().map(|r| r.bind_ms).sum();
            let unbind_sum: u64 = ok_runs.iter().map(|r| r.unbind_ms).sum();

            PersonalityAggregate {
                personality,
                bdf,
                count,
                avg_total_ms: avg,
                min_total_ms: min,
                max_total_ms: max,
                stddev_total_ms: stddev,
                avg_bind_ms: bind_sum / count as u64,
                avg_unbind_ms: unbind_sum / count as u64,
                fail_count,
            }
        })
        .collect()
}

fn print_results_table(results: &[SweepResult], repeat: u32) {
    if repeat > 1 {
        let aggs = aggregate_results(results);
        println!(
            "{:<14} {:<16} {:>5} {:>5} {:>10} {:>10} {:>10} {:>8} {:>10}",
            "BDF", "PERSONALITY", "OK", "FAIL", "AVG_MS", "MIN_MS", "MAX_MS", "STDDEV", "BIND_MS"
        );
        println!("{}", "-".repeat(100));
        for a in &aggs {
            println!(
                "{:<14} {:<16} {:>5} {:>5} {:>8}ms {:>8}ms {:>8}ms {:>7.1} {:>8}ms",
                a.bdf,
                a.personality,
                a.count,
                a.fail_count,
                a.avg_total_ms,
                a.min_total_ms,
                a.max_total_ms,
                a.stddev_total_ms,
                a.avg_bind_ms,
            );
        }
    } else {
        println!(
            "{:<14} {:<16} {:>6} {:>10} {:>10} {:>10} {:>8} TRACE/ERROR",
            "BDF", "PERSONALITY", "STATUS", "TOTAL_MS", "BIND_MS", "UNBIND_MS", "INSIGHTS",
        );
        println!("{}", "-".repeat(100));
        for r in results {
            let status = if r.success { "OK" } else { "FAIL" };
            let trail = if let Some(ref e) = r.error {
                e.clone()
            } else if let Some(ref t) = r.trace_path {
                t.clone()
            } else {
                String::new()
            };
            println!(
                "{:<14} {:<16} {:>6} {:>8}ms {:>8}ms {:>8}ms {:>8} {}",
                r.bdf, r.personality, status, r.total_ms, r.bind_ms, r.unbind_ms, r.insights, trail
            );
        }
    }
}

fn print_cross_card_comparison(results: &[SweepResult]) {
    use std::collections::BTreeMap;
    let bdfs: Vec<String> = {
        let mut seen = Vec::new();
        for r in results {
            if !seen.contains(&r.bdf) {
                seen.push(r.bdf.clone());
            }
        }
        seen
    };
    if bdfs.len() < 2 {
        return;
    }

    let aggs = aggregate_results(results);
    let mut by_personality: BTreeMap<String, Vec<&PersonalityAggregate>> = BTreeMap::new();
    for a in &aggs {
        by_personality
            .entry(a.personality.clone())
            .or_default()
            .push(a);
    }

    println!("\n{}", "=".repeat(100));
    println!("Cross-Card Comparison");
    println!("{}", "=".repeat(100));

    for (personality, card_aggs) in &by_personality {
        if card_aggs.len() < 2 {
            continue;
        }
        println!("\n  {personality}:");
        for a in card_aggs {
            println!(
                "    {:<14}  avg={:>7}ms  bind={:>7}ms  unbind={:>7}ms  (n={})",
                a.bdf, a.avg_total_ms, a.avg_bind_ms, a.avg_unbind_ms, a.count,
            );
        }
        let ok_aggs: Vec<&&PersonalityAggregate> =
            card_aggs.iter().filter(|a| a.count > 0).collect();
        if ok_aggs.len() >= 2 {
            let totals: Vec<u64> = ok_aggs.iter().map(|a| a.avg_total_ms).collect();
            let min_t = *totals
                .iter()
                .min()
                .expect("ok_aggs.len() >= 2 with count > 0 yields non-empty totals for min");
            let max_t = *totals
                .iter()
                .max()
                .expect("ok_aggs.len() >= 2 with count > 0 yields non-empty totals for max");
            let delta = max_t.saturating_sub(min_t);
            let pct = if min_t > 0 {
                delta as f64 / min_t as f64 * 100.0
            } else {
                0.0
            };
            println!("    variance: {delta}ms ({pct:.1}%)");
        }
    }
}

fn print_journal_summary(bdf: &str) {
    let ember = ember_socket();
    let params = json!({"bdf": bdf});
    let response = rpc_call(&ember, "ember.journal.stats", params);

    if response.get("error").is_some() {
        println!("  (journal stats unavailable for {bdf})");
        return;
    }

    if let Some(result) = response.get("result") {
        let total_swaps = result
            .get("total_swaps")
            .and_then(|v| v.as_u64())
            .unwrap_or(0);
        let total_resets = result
            .get("total_resets")
            .and_then(|v| v.as_u64())
            .unwrap_or(0);
        let total_boots = result
            .get("total_boot_attempts")
            .and_then(|v| v.as_u64())
            .unwrap_or(0);

        println!(
            "  {bdf}: {total_swaps} swaps, {total_resets} resets, {total_boots} boot attempts"
        );

        if let Some(personalities) = result.get("personality_stats").and_then(|v| v.as_array()) {
            for p in personalities {
                let name = p.get("personality").and_then(|v| v.as_str()).unwrap_or("?");
                let count = p.get("swap_count").and_then(|v| v.as_u64()).unwrap_or(0);
                let avg_total = p.get("avg_total_ms").and_then(|v| v.as_u64()).unwrap_or(0);
                let avg_bind = p.get("avg_bind_ms").and_then(|v| v.as_u64()).unwrap_or(0);
                println!(
                    "    {name:<16} n={count:<4} avg_total={avg_total}ms  avg_bind={avg_bind}ms"
                );
            }
        }
    }
}

pub(crate) fn rpc_experiment_sweep(
    socket: &str,
    bdf_arg: &str,
    personalities: Option<&str>,
    return_to: &str,
    trace: bool,
    repeat: u32,
) {
    let bdfs: Vec<&str> = bdf_arg.split(',').map(|s| s.trim()).collect();
    let targets: Vec<&str> = match personalities {
        Some(p) => p.split(',').map(|s| s.trim()).collect(),
        None => DEFAULT_SWEEP_PERSONALITIES.to_vec(),
    };
    let repeat = repeat.max(1);

    println!("Experiment Sweep");
    println!("Cards: {}", bdfs.join(", "));
    println!("Personalities: {}", targets.join(", "));
    println!("Repeat: {repeat}x  |  Return-to: {return_to}  |  Trace: {trace}");
    println!("{}", "=".repeat(100));

    let mut all_results: Vec<SweepResult> = Vec::new();

    for bdf in &bdfs {
        if bdfs.len() > 1 {
            println!("\n>>> Card: {bdf}");
        }
        let card_results = sweep_single_card(socket, bdf, &targets, return_to, trace, repeat);
        all_results.extend(card_results);
    }

    // Per-card results table
    println!("\n{}", "=".repeat(100));
    println!("Experiment Results");
    println!("{}", "=".repeat(100));
    print_results_table(&all_results, repeat);

    let ok_count = all_results.iter().filter(|r| r.success).count();
    let fail_count = all_results.len() - ok_count;
    println!("{}", "-".repeat(100));
    println!(
        "Summary: {ok_count} succeeded, {fail_count} failed out of {} operations",
        all_results.len()
    );

    // Cross-card comparison (only when multiple BDFs)
    if bdfs.len() > 1 {
        print_cross_card_comparison(&all_results);
    }

    // Auto journal summary
    println!("\n{}", "=".repeat(100));
    println!("Journal Summary (all-time aggregates from ember)");
    println!("{}", "-".repeat(100));
    for bdf in &bdfs {
        print_journal_summary(bdf);
    }
}
