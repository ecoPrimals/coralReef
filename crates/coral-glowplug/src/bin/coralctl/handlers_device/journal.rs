// SPDX-License-Identifier: AGPL-3.0-only
//! Journal query and stats handlers — talks directly to coral-ember via IPC.

use crate::rpc::{check_rpc_error, rpc_call};
use serde_json::json;

/// Resolve the ember socket path for direct journal access.
///
/// Delegates to `coral_ember::ember_socket_path()` so the resolution logic
/// (env var override → `XDG_RUNTIME_DIR` → tempdir fallback) is centralised.
pub(super) fn ember_socket() -> String {
    coral_ember::ember_socket_path()
}

pub(crate) fn rpc_journal_query(
    _glowplug_socket: &str,
    bdf: Option<String>,
    kind: Option<String>,
    personality: Option<String>,
    limit: usize,
) {
    let mut params = json!({});
    if let Some(ref b) = bdf {
        params["bdf"] = json!(b);
    }
    if let Some(ref k) = kind {
        params["kind"] = json!(k);
    }
    if let Some(ref p) = personality {
        params["personality"] = json!(p);
    }
    params["limit"] = json!(limit);

    let response = rpc_call(&ember_socket(), "ember.journal.query", params);
    check_rpc_error(&response);

    if let Some(result) = response.get("result") {
        let entries = result
            .get("entries")
            .and_then(|v| v.as_array())
            .cloned()
            .unwrap_or_default();

        if entries.is_empty() {
            println!("No journal entries found.");
            return;
        }

        println!("{} journal entries:", entries.len());
        println!("{}", "-".repeat(80));

        for entry in &entries {
            let kind = entry.get("kind").and_then(|v| v.as_str()).unwrap_or("?");
            let bdf = entry.get("bdf").and_then(|v| v.as_str()).unwrap_or("?");
            let ts = entry
                .get("timestamp_epoch_ms")
                .and_then(|v| v.as_u64())
                .unwrap_or(0);

            match kind {
                "Swap" => {
                    let to = entry
                        .get("to_personality")
                        .and_then(|v| v.as_str())
                        .unwrap_or("?");
                    let from = entry
                        .get("from_personality")
                        .and_then(|v| v.as_str())
                        .unwrap_or("none");
                    let total_ms = entry
                        .get("timing")
                        .and_then(|t| t.get("total_ms"))
                        .and_then(|v| v.as_u64())
                        .unwrap_or(0);
                    let trace = entry
                        .get("trace_path")
                        .and_then(|v| v.as_str())
                        .unwrap_or("");
                    print!("[{ts}] SWAP {bdf}: {from} → {to} ({total_ms}ms)");
                    if !trace.is_empty() {
                        print!(" trace={trace}");
                    }
                    println!();
                }
                "Reset" => {
                    let method = entry.get("method").and_then(|v| v.as_str()).unwrap_or("?");
                    let success = entry
                        .get("success")
                        .and_then(|v| v.as_bool())
                        .unwrap_or(false);
                    let dur = entry
                        .get("duration_ms")
                        .and_then(|v| v.as_u64())
                        .unwrap_or(0);
                    let status = if success { "OK" } else { "FAIL" };
                    println!("[{ts}] RESET {bdf}: {method} {status} ({dur}ms)");
                }
                "BootAttempt" => {
                    let strategy = entry
                        .get("strategy")
                        .and_then(|v| v.as_str())
                        .unwrap_or("?");
                    let success = entry
                        .get("success")
                        .and_then(|v| v.as_bool())
                        .unwrap_or(false);
                    let sec2 = entry.get("sec2_exci").and_then(|v| v.as_u64()).unwrap_or(0);
                    let status = if success { "OK" } else { "FAIL" };
                    println!("[{ts}] BOOT {bdf}: {strategy} {status} (sec2_exci=0x{sec2:08x})");
                }
                _ => {
                    println!("[{ts}] {kind} {bdf}");
                }
            }
        }
    }
}

pub(crate) fn rpc_journal_stats(_glowplug_socket: &str, bdf: Option<String>) {
    let params = match bdf {
        Some(ref b) => json!({"bdf": b}),
        None => json!({}),
    };

    let response = rpc_call(&ember_socket(), "ember.journal.stats", params);
    check_rpc_error(&response);

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

        println!("Journal Statistics");
        println!("{}", "=".repeat(60));
        println!(
            "Total: {} swaps, {} resets, {} boot attempts",
            total_swaps, total_resets, total_boots
        );

        if let Some(personalities) = result.get("personality_stats").and_then(|v| v.as_array())
            && !personalities.is_empty()
        {
            println!("\nPersonality Swap Timing:");
            println!(
                "  {:<16} {:>6} {:>10} {:>10} {:>10}",
                "PERSONALITY", "COUNT", "AVG_TOTAL", "AVG_BIND", "AVG_UNBIND"
            );
            for p in personalities {
                let name = p.get("personality").and_then(|v| v.as_str()).unwrap_or("?");
                let count = p.get("swap_count").and_then(|v| v.as_u64()).unwrap_or(0);
                let avg_total = p.get("avg_total_ms").and_then(|v| v.as_u64()).unwrap_or(0);
                let avg_bind = p.get("avg_bind_ms").and_then(|v| v.as_u64()).unwrap_or(0);
                let avg_unbind = p.get("avg_unbind_ms").and_then(|v| v.as_u64()).unwrap_or(0);
                println!(
                    "  {:<16} {:>6} {:>8}ms {:>8}ms {:>8}ms",
                    name, count, avg_total, avg_bind, avg_unbind
                );
            }
        }

        if let Some(resets) = result.get("reset_method_stats").and_then(|v| v.as_array())
            && !resets.is_empty()
        {
            println!("\nReset Method Stats:");
            println!(
                "  {:<16} {:>8} {:>8} {:>10} {:>10}",
                "METHOD", "ATTEMPTS", "SUCCESS", "RATE", "AVG_MS"
            );
            for r in resets {
                let method = r.get("method").and_then(|v| v.as_str()).unwrap_or("?");
                let attempts = r.get("attempts").and_then(|v| v.as_u64()).unwrap_or(0);
                let successes = r.get("successes").and_then(|v| v.as_u64()).unwrap_or(0);
                let rate = r
                    .get("success_rate")
                    .and_then(|v| v.as_f64())
                    .unwrap_or(0.0);
                let avg_ms = r
                    .get("avg_duration_ms")
                    .and_then(|v| v.as_u64())
                    .unwrap_or(0);
                println!(
                    "  {:<16} {:>8} {:>8} {:>9.0}% {:>8}ms",
                    method,
                    attempts,
                    successes,
                    rate * 100.0,
                    avg_ms
                );
            }
        }
    }
}
