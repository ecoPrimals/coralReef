// SPDX-License-Identifier: AGPL-3.0-only
//! mmiotrace integration — captures MMIO register writes during driver bind.
//!
//! The kernel's mmiotrace facility records every `ioremap`'d MMIO write/read
//! from any kernel driver. Ember enables it around driver bind events so we
//! can capture the exact register sequence a driver uses during initialization
//! (e.g. nouveau's SEC2 boot, nvidia's GSP firmware load).
//!
//! Trace files are written to `/sys/kernel/debug/tracing/trace` by the kernel;
//! Ember copies the output to a data directory tagged with BDF, driver, and
//! timestamp.
//!
//! Requires: `ReadWritePaths=/sys/kernel/debug/tracing` in the systemd unit
//! and `CAP_SYS_ADMIN` for debugfs access.

use crate::error::{SwapError, TraceError};
use std::path::Path;
use std::time::SystemTime;

const DEFAULT_DEBUGFS_TRACING: &str = "/sys/kernel/debug/tracing";

fn debugfs_tracing_base() -> String {
    std::env::var("CORALREEF_DEBUGFS_TRACING")
        .ok()
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| DEFAULT_DEBUGFS_TRACING.to_string())
}

fn tracer_path() -> String {
    format!("{}/current_tracer", debugfs_tracing_base())
}

fn tracing_on_path() -> String {
    format!("{}/tracing_on", debugfs_tracing_base())
}

fn trace_path() -> String {
    format!("{}/trace", debugfs_tracing_base())
}

fn trace_data_dir() -> String {
    std::env::var("CORALREEF_TRACE_DIR")
        .ok()
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "/var/lib/coralreef/traces".to_string())
}

/// Enable the kernel mmiotrace facility.
///
/// Writes `mmiotrace` to `current_tracer` and `1` to `tracing_on`.
/// Returns `Ok(())` if both writes succeed.
pub fn enable_mmiotrace() -> Result<(), TraceError> {
    tracing::info!("enabling kernel mmiotrace");

    // Clear any previous trace data
    std::fs::write(trace_path(), "").ok();

    std::fs::write(tracer_path(), "mmiotrace").map_err(|e| {
        TraceError::Enable(format!("failed to set current_tracer to mmiotrace: {e}"))
    })?;

    std::fs::write(tracing_on_path(), "1")
        .map_err(|e| TraceError::Enable(format!("failed to enable tracing_on: {e}")))?;

    let verify = std::fs::read_to_string(tracer_path())
        .unwrap_or_default()
        .trim()
        .to_string();
    if verify != "mmiotrace" {
        return Err(TraceError::Enable(format!(
            "tracer verification failed: expected 'mmiotrace', got '{verify}'"
        )));
    }

    tracing::info!("mmiotrace enabled");
    Ok(())
}

/// Disable mmiotrace and restore the nop tracer.
///
/// Ensures `tracing_on` is off and switches `current_tracer` back to `nop`.
/// Idempotent — safe to call even if tracing_on was already stopped.
pub fn disable_mmiotrace() -> Result<(), TraceError> {
    tracing::info!("disabling kernel mmiotrace");

    // Idempotent: stop recording if not already stopped
    let _ = std::fs::write(tracing_on_path(), "0");

    std::fs::write(tracer_path(), "nop")
        .map_err(|e| TraceError::Disable(format!("failed to reset current_tracer to nop: {e}")))?;

    tracing::info!("mmiotrace disabled");
    Ok(())
}

/// Read the current trace buffer and save it to the data directory.
///
/// The output file is named `<BDF>_<driver>_<timestamp>.mmiotrace`.
/// Returns the path to the saved file on success.
pub fn capture_trace(bdf: &str, driver: &str) -> Result<String, TraceError> {
    let data_dir = trace_data_dir();

    if let Some(parent) = Path::new(&data_dir).parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let _ = std::fs::create_dir_all(&data_dir);

    let timestamp = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);

    let safe_bdf = bdf.replace(':', "-");
    let filename = format!("{safe_bdf}_{driver}_{timestamp}.mmiotrace");
    let output_path = format!("{data_dir}/{filename}");

    let trace_data = std::fs::read_to_string(trace_path()).map_err(|e| TraceError::Capture {
        bdf: bdf.to_string(),
        reason: format!("failed to read trace buffer: {e}"),
    })?;

    let line_count = trace_data.lines().count();
    tracing::info!(
        bdf,
        driver,
        lines = line_count,
        path = %output_path,
        "captured mmiotrace"
    );

    std::fs::write(&output_path, &trace_data).map_err(|e| TraceError::Capture {
        bdf: bdf.to_string(),
        reason: format!("failed to write trace to {output_path}: {e}"),
    })?;

    Ok(output_path)
}

/// Full trace lifecycle: enable mmiotrace, run closure, capture, then disable.
///
/// Used by the swap orchestrator to wrap a driver bind with trace capture.
///
/// IMPORTANT: The capture must happen BEFORE switching current_tracer back to
/// "nop", because the kernel clears the ring buffer when the tracer changes.
/// The sequence is: enable → run → stop recording → capture → reset tracer.
pub fn with_mmiotrace<F, T>(bdf: &str, driver: &str, f: F) -> (Result<T, SwapError>, Option<String>)
where
    F: FnOnce() -> Result<T, SwapError>,
{
    if let Err(e) = enable_mmiotrace() {
        tracing::error!(error = %e, "mmiotrace enable failed — proceeding without trace");
        let result = f();
        return (result, None);
    }

    let result = f();

    // Stop recording new events but keep the tracer active so the
    // ring buffer is preserved for reading.
    let _ = std::fs::write(tracing_on_path(), "0");

    // Capture the trace while mmiotrace is still the active tracer —
    // the ring buffer is only valid while current_tracer == "mmiotrace".
    let trace_path = match capture_trace(bdf, driver) {
        Ok(path) => Some(path),
        Err(e) => {
            tracing::error!(error = %e, "trace capture failed");
            None
        }
    };

    // Now safe to switch tracer back to nop (this clears the ring buffer,
    // but we already saved the data).
    if let Err(e) = disable_mmiotrace() {
        tracing::error!(error = %e, "mmiotrace disable failed");
    }

    (result, trace_path)
}

/// Check whether mmiotrace is available on this kernel.
pub fn is_mmiotrace_available() -> bool {
    Path::new(&tracer_path()).exists()
}

/// Read the current tracer name (e.g. "nop", "mmiotrace").
pub fn current_tracer() -> Option<String> {
    std::fs::read_to_string(tracer_path())
        .ok()
        .map(|s| s.trim().to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn trace_data_dir_falls_back_to_default() {
        let dir = trace_data_dir();
        assert!(!dir.is_empty(), "trace_data_dir should never be empty");
    }

    #[test]
    fn is_mmiotrace_available_returns_bool() {
        let _ = is_mmiotrace_available();
    }

    #[test]
    fn current_tracer_returns_option() {
        let _ = current_tracer();
    }
}
