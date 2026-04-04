// SPDX-License-Identifier: AGPL-3.0-only
//! JSON-RPC handlers for per-GPU boot personality and experiment policy.
//!
//! The `ember.policy.*` namespace controls how each GPU is booted, what
//! teardown operations are allowed, and enables experiment matrix sweeps
//! across different boot configurations.

use std::collections::HashMap;
use std::io::Write;
use std::sync::{Arc, RwLock};

use super::jsonrpc::{write_jsonrpc_error, write_jsonrpc_ok};

fn io_err_string(e: std::io::Error) -> String {
    format!("I/O: {e}")
}

/// Boot personality for a GPU — controls sovereign boot behavior.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct BootPolicy {
    /// How to boot: "sovereign" (VBIOS + recipe + ACR), "recipe-only",
    /// "passthrough" (let driver handle boot), "warm" (no reset).
    pub boot_mode: String,
    /// VBIOS DEVINIT behavior: "auto" (check needs_post), "force", "skip".
    pub devinit_mode: String,
    /// ACR strategy selection: "full" (all strategies), "7c-primary"
    /// (sysmem WPR first), "skip" (no ACR boot).
    pub acr_strategy: String,
    /// Teardown policy: "block-all" (NOP all teardown via livepatch),
    /// "selective" (NOP specific functions), "allow" (let driver teardown).
    pub teardown_policy: String,
    /// Whether to perform SBR before sovereign boot.
    pub sbr_before_boot: bool,
}

impl Default for BootPolicy {
    fn default() -> Self {
        Self {
            boot_mode: "sovereign".into(),
            devinit_mode: "auto".into(),
            acr_strategy: "full".into(),
            teardown_policy: "block-all".into(),
            sbr_before_boot: true,
        }
    }
}

/// Per-GPU policy store, keyed by BDF.
pub type PolicyStore = Arc<RwLock<HashMap<String, BootPolicy>>>;

/// Create a new empty policy store.
pub fn new_policy_store() -> PolicyStore {
    Arc::new(RwLock::new(HashMap::new()))
}

/// `ember.policy.get` — retrieve the boot policy for a device.
///
/// Params: `{ "bdf": "0000:03:00.0" }`
/// Returns the current policy or the default if none is set.
pub(crate) fn get(
    stream: &mut impl Write,
    policies: &PolicyStore,
    id: serde_json::Value,
    params: &serde_json::Value,
) -> Result<(), String> {
    let bdf = params
        .get("bdf")
        .and_then(|v| v.as_str())
        .ok_or("missing 'bdf' parameter")?;

    let map = policies.read().map_err(|e| format!("lock poisoned: {e}"))?;
    let policy = map.get(bdf).cloned().unwrap_or_default();

    let result = serde_json::json!({
        "bdf": bdf,
        "policy": policy,
    });

    write_jsonrpc_ok(stream, id, result).map_err(io_err_string)
}

/// `ember.policy.set` — set the boot policy for a device.
///
/// Params: `{ "bdf": "0000:03:00.0", "policy": { ... } }`
pub(crate) fn set(
    stream: &mut impl Write,
    policies: &PolicyStore,
    id: serde_json::Value,
    params: &serde_json::Value,
) -> Result<(), String> {
    let bdf = params
        .get("bdf")
        .and_then(|v| v.as_str())
        .ok_or("missing 'bdf' parameter")?;

    let policy_val = params
        .get("policy")
        .ok_or("missing 'policy' parameter")?;

    let mut policy = {
        let map = policies.read().map_err(|e| format!("lock poisoned: {e}"))?;
        map.get(bdf).cloned().unwrap_or_default()
    };

    if let Some(v) = policy_val.get("boot_mode").and_then(|v| v.as_str()) {
        match v {
            "sovereign" | "recipe-only" | "passthrough" | "warm" => {
                policy.boot_mode = v.into();
            }
            other => {
                write_jsonrpc_error(
                    stream,
                    id,
                    -32602,
                    &format!("invalid boot_mode: {other}"),
                )
                .map_err(io_err_string)?;
                return Ok(());
            }
        }
    }

    if let Some(v) = policy_val.get("devinit_mode").and_then(|v| v.as_str()) {
        match v {
            "auto" | "force" | "skip" => {
                policy.devinit_mode = v.into();
            }
            other => {
                write_jsonrpc_error(
                    stream,
                    id,
                    -32602,
                    &format!("invalid devinit_mode: {other}"),
                )
                .map_err(io_err_string)?;
                return Ok(());
            }
        }
    }

    if let Some(v) = policy_val.get("acr_strategy").and_then(|v| v.as_str()) {
        match v {
            "full" | "7c-primary" | "skip" => {
                policy.acr_strategy = v.into();
            }
            other => {
                write_jsonrpc_error(
                    stream,
                    id,
                    -32602,
                    &format!("invalid acr_strategy: {other}"),
                )
                .map_err(io_err_string)?;
                return Ok(());
            }
        }
    }

    if let Some(v) = policy_val.get("teardown_policy").and_then(|v| v.as_str()) {
        match v {
            "block-all" | "selective" | "allow" => {
                policy.teardown_policy = v.into();
            }
            other => {
                write_jsonrpc_error(
                    stream,
                    id,
                    -32602,
                    &format!("invalid teardown_policy: {other}"),
                )
                .map_err(io_err_string)?;
                return Ok(());
            }
        }
    }

    if let Some(v) = policy_val.get("sbr_before_boot").and_then(|v| v.as_bool()) {
        policy.sbr_before_boot = v;
    }

    let result_policy = policy.clone();
    let mut map = policies.write().map_err(|e| format!("lock poisoned: {e}"))?;
    map.insert(bdf.to_string(), policy);

    let result = serde_json::json!({
        "bdf": bdf,
        "policy": result_policy,
        "status": "updated",
    });

    write_jsonrpc_ok(stream, id, result).map_err(io_err_string)
}

/// `ember.policy.list` — list all per-GPU policies.
pub(crate) fn list(
    stream: &mut impl Write,
    policies: &PolicyStore,
    id: serde_json::Value,
) -> Result<(), String> {
    let map = policies.read().map_err(|e| format!("lock poisoned: {e}"))?;

    let entries: Vec<serde_json::Value> = map
        .iter()
        .map(|(bdf, policy)| {
            serde_json::json!({
                "bdf": bdf,
                "policy": policy,
            })
        })
        .collect();

    let result = serde_json::json!({
        "devices": entries,
        "count": entries.len(),
    });

    write_jsonrpc_ok(stream, id, result).map_err(io_err_string)
}

/// `ember.policy.matrix` — describe the experiment matrix for a device.
///
/// Returns the Cartesian product of policy dimensions that would be swept
/// in a matrix experiment (boot_mode × devinit_mode × acr_strategy × teardown).
pub(crate) fn matrix(
    stream: &mut impl Write,
    id: serde_json::Value,
    params: &serde_json::Value,
) -> Result<(), String> {
    let bdf = params
        .get("bdf")
        .and_then(|v| v.as_str())
        .ok_or("missing 'bdf' parameter")?;

    let boot_modes = params
        .get("boot_modes")
        .and_then(|v| v.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|v| v.as_str().map(String::from))
                .collect::<Vec<_>>()
        })
        .unwrap_or_else(|| vec!["sovereign".into(), "recipe-only".into()]);

    let devinit_modes = params
        .get("devinit_modes")
        .and_then(|v| v.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|v| v.as_str().map(String::from))
                .collect::<Vec<_>>()
        })
        .unwrap_or_else(|| vec!["auto".into(), "force".into(), "skip".into()]);

    let acr_strategies = params
        .get("acr_strategies")
        .and_then(|v| v.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|v| v.as_str().map(String::from))
                .collect::<Vec<_>>()
        })
        .unwrap_or_else(|| vec!["full".into(), "7c-primary".into()]);

    let mut combinations = Vec::new();
    for bm in &boot_modes {
        for dm in &devinit_modes {
            for acr in &acr_strategies {
                combinations.push(serde_json::json!({
                    "boot_mode": bm,
                    "devinit_mode": dm,
                    "acr_strategy": acr,
                }));
            }
        }
    }

    let result = serde_json::json!({
        "bdf": bdf,
        "total_combinations": combinations.len(),
        "dimensions": {
            "boot_modes": boot_modes,
            "devinit_modes": devinit_modes,
            "acr_strategies": acr_strategies,
        },
        "matrix": combinations,
    });

    write_jsonrpc_ok(stream, id, result).map_err(io_err_string)
}
