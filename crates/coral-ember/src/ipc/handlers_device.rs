// SPDX-License-Identifier: AGPL-3.0-only
//! JSON-RPC handlers for device, swap, reset, ring metadata, and status.

use std::collections::{HashMap, HashSet};
use std::io::Write;
use std::os::unix::net::UnixStream;
use std::sync::{Arc, RwLock};

use crate::error::{EmberIpcError, SwapError};
use crate::hold::HeldDevice;
use crate::journal::{Journal, JournalEntry};
use crate::swap;
use crate::sysfs;

use super::fd::send_with_fds;
use super::helpers::{finish_managed_bdf_early, require_managed_bdf, try_reset_methods};
use super::jsonrpc::{make_jsonrpc_ok, write_jsonrpc_error, write_jsonrpc_ok};

#[deprecated(note = "use MMIO gateway RPCs instead of raw fd sharing")]
pub(crate) fn vfio_fds(
    stream: &mut UnixStream,
    _held: &Arc<RwLock<HashMap<String, HeldDevice>>>,
    _managed_bdfs: &HashSet<String>,
    id: serde_json::Value,
    _params: &serde_json::Value,
) -> Result<(), EmberIpcError> {
    tracing::warn!("ember.vfio_fds called — fd sharing is deprecated, use MMIO gateway RPCs");
    write_jsonrpc_error(
        stream,
        id,
        -32600,
        "fd sharing deprecated — use MMIO gateway RPCs (ember.mmio.*, ember.pramin.*, ember.falcon.*)",
    )
    .map_err(EmberIpcError::from)
}

/// `ember.vfio_fds` over TCP (`SCM_RIGHTS` requires a Unix domain socket).
pub(crate) fn vfio_fds_unavailable(
    stream: &mut impl Write,
    id: serde_json::Value,
) -> Result<(), EmberIpcError> {
    write_jsonrpc_error(
        stream,
        id,
        -32603,
        "ember.vfio_fds requires Unix socket transport (SCM_RIGHTS)",
    )
    .map_err(EmberIpcError::from)
}

/// `ember.checkpoint_fds` — send ALL VFIO fds for ALL held devices via SCM_RIGHTS.
///
/// Response JSON contains a manifest of devices and their fd layout so the
/// receiver (glowplug's FdVault) knows how to reconstruct each device.
/// The fds are concatenated in BDF-sorted order; the manifest specifies
/// the count per device.
pub(crate) fn checkpoint_fds(
    stream: &mut UnixStream,
    held: &Arc<RwLock<HashMap<String, HeldDevice>>>,
    id: serde_json::Value,
) -> Result<(), EmberIpcError> {
    let map = held.read().map_err(|_| EmberIpcError::LockPoisoned)?;

    let mut manifest = Vec::new();
    let mut all_fds = Vec::new();

    let mut sorted_bdfs: Vec<&String> = map.keys().collect();
    sorted_bdfs.sort();

    for bdf in sorted_bdfs {
        let dev = &map[bdf];
        let fds = dev.device.sendable_fds();
        let kind = dev.device.backend_kind();

        let mut entry = serde_json::json!({
            "bdf": bdf,
            "num_fds": fds.len(),
        });

        match kind {
            coral_driver::vfio::VfioBackendKind::Legacy => {
                entry["backend"] = serde_json::json!("legacy");
            }
            coral_driver::vfio::VfioBackendKind::Iommufd { ioas_id } => {
                entry["backend"] = serde_json::json!("iommufd");
                entry["ioas_id"] = serde_json::json!(ioas_id);
            }
        }

        manifest.push(entry);
        all_fds.extend(fds);
    }

    let total_fds = all_fds.len();
    let num_devices = manifest.len();

    let result = serde_json::json!({
        "devices": manifest,
        "total_fds": total_fds,
    });

    let resp = make_jsonrpc_ok(id, result);
    let resp_bytes = format!(
        "{}\n",
        serde_json::to_string(&resp)
            .map_err(|e| EmberIpcError::JsonSerialize(format!("serialize: {e}")))?
    );

    // Send while lock is still held so fds remain valid
    send_with_fds(&*stream, resp_bytes.as_bytes(), &all_fds).map_err(EmberIpcError::from)?;

    drop(all_fds);
    drop(map);

    tracing::info!(
        num_devices,
        total_fds,
        "checkpoint: sent all VFIO fds to caller"
    );
    Ok(())
}

pub(crate) fn list(
    stream: &mut impl Write,
    held: &Arc<RwLock<HashMap<String, HeldDevice>>>,
    id: serde_json::Value,
) -> Result<(), EmberIpcError> {
    let map = held.read().map_err(|_| EmberIpcError::LockPoisoned)?;
    let devices: Vec<String> = map.keys().cloned().collect();
    drop(map);
    write_jsonrpc_ok(stream, id, serde_json::json!({"devices": devices}))
        .map_err(EmberIpcError::from)
}

pub(crate) fn release(
    stream: &mut impl Write,
    held: &Arc<RwLock<HashMap<String, HeldDevice>>>,
    managed_bdfs: &HashSet<String>,
    id: serde_json::Value,
    params: &serde_json::Value,
) -> Result<(), EmberIpcError> {
    let bdf = params
        .get("bdf")
        .and_then(|v| v.as_str())
        .ok_or(EmberIpcError::InvalidRequest("missing 'bdf' parameter"))?;
    finish_managed_bdf_early(require_managed_bdf(bdf, managed_bdfs, stream, &id))?;
    let mut map = held.write().map_err(|_| EmberIpcError::LockPoisoned)?;
    match map.remove(bdf) {
        Some(device) => {
            drop(device);
            tracing::info!(bdf, "ember released VFIO fds for swap");
            drop(map);
            write_jsonrpc_ok(stream, id, serde_json::json!({"bdf": bdf}))
                .map_err(EmberIpcError::from)
        }
        None => {
            drop(map);
            write_jsonrpc_error(
                stream,
                id,
                -32000,
                &format!("device {bdf} not held by ember"),
            )
            .map_err(EmberIpcError::from)
        }
    }
}

pub(crate) fn reacquire(
    stream: &mut impl Write,
    held: &Arc<RwLock<HashMap<String, HeldDevice>>>,
    managed_bdfs: &HashSet<String>,
    id: serde_json::Value,
    params: &serde_json::Value,
) -> Result<(), EmberIpcError> {
    let bdf = params
        .get("bdf")
        .and_then(|v| v.as_str())
        .ok_or(EmberIpcError::InvalidRequest("missing 'bdf' parameter"))?;
    finish_managed_bdf_early(require_managed_bdf(bdf, managed_bdfs, stream, &id))?;
    if sysfs::is_d3cold(bdf) {
        write_jsonrpc_error(
            stream,
            id,
            -32000,
            &format!("{bdf} is D3cold — cannot reacquire"),
        )
        .map_err(EmberIpcError::from)?;
        return Ok(());
    }
    let mut map = held.write().map_err(|_| EmberIpcError::LockPoisoned)?;
    if map.contains_key(bdf) {
        tracing::warn!(bdf, "device already held — skipping reacquire");
        drop(map);
        write_jsonrpc_ok(stream, id, serde_json::json!({"bdf": bdf})).map_err(EmberIpcError::from)
    } else {
        match coral_driver::vfio::VfioDevice::open(bdf) {
            Ok(device) => {
                // Bus master stays OFF — ember never does DMA.
                let req_eventfd = crate::arm_req_irq(&device, bdf);
                tracing::info!(
                    bdf,
                    backend = ?device.backend_kind(),
                    device_fd = device.device_fd(),
                    req_armed = req_eventfd.is_some(),
                    "VFIO device reacquired by ember after swap"
                );
                map.insert(
                    bdf.to_string(),
                    HeldDevice {
                        bdf: bdf.to_string(),
                        device,
                        bar0: None,
                        ring_meta: crate::hold::RingMeta::default(),
                        req_eventfd,
                        experiment_dirty: false,
                        needs_warm_cycle: false,
                        dma_prepare_state: None,
                        mmio_fault_count: 0,
                        health: crate::hold::DeviceHealth::Alive,
                        pcie_armor: Some(crate::pcie_armor::PcieArmor::arm(&bdf)),
                    },
                );
                drop(map);
                write_jsonrpc_ok(stream, id, serde_json::json!({"bdf": bdf}))
                    .map_err(EmberIpcError::from)
            }
            Err(e) => {
                drop(map);
                tracing::error!(bdf, error = %e, "failed to reacquire VFIO device");
                write_jsonrpc_error(stream, id, -32000, &format!("reacquire failed: {e}"))
                    .map_err(EmberIpcError::from)
            }
        }
    }
}

/// Perform a full GPU warm cycle: release → nouveau bind/unbind → reacquire.
///
/// Clears PRAMIN degradation and `needs_warm_cycle` flag. Used by glowplug
/// when ember is still alive, or by experiments that detect cold VRAM.
pub(crate) fn warm_cycle(
    stream: &mut impl Write,
    held: &Arc<RwLock<HashMap<String, HeldDevice>>>,
    managed_bdfs: &HashSet<String>,
    id: serde_json::Value,
    params: &serde_json::Value,
) -> Result<(), EmberIpcError> {
    let bdf = params
        .get("bdf")
        .and_then(|v| v.as_str())
        .ok_or(EmberIpcError::InvalidRequest("missing 'bdf' parameter"))?;
    finish_managed_bdf_early(require_managed_bdf(bdf, managed_bdfs, stream, &id))?;

    tracing::info!(bdf, "ember.warm_cycle: releasing device");

    // 1. Release
    {
        let mut map = held.write().map_err(|_| EmberIpcError::LockPoisoned)?;
        if map.remove(bdf).is_none() {
            tracing::warn!(bdf, "ember.warm_cycle: device not held (proceeding anyway)");
        }
    }

    // 2. Unbind current driver
    let device_path = format!("/sys/bus/pci/devices/{bdf}");
    let driver_link = format!("{device_path}/driver");
    if let Ok(link) = std::fs::read_link(&driver_link) {
        if let Some(driver_name) = link.file_name().and_then(|n| n.to_str()) {
            let unbind = format!("/sys/bus/pci/drivers/{driver_name}/unbind");
            let _ = std::fs::write(&unbind, bdf);
            std::thread::sleep(std::time::Duration::from_millis(500));
        }
    }

    // 3. Bind to nouveau for memory controller retrain
    let override_path = format!("{device_path}/driver_override");
    if let Err(e) = std::fs::write(&override_path, "nouveau") {
        tracing::error!(bdf, error = %e, "ember.warm_cycle: failed to set nouveau override");
        return write_jsonrpc_error(stream, id, -32000, &format!("warm cycle failed: {e}"))
            .map_err(EmberIpcError::from);
    }
    let _ = std::fs::write("/sys/bus/pci/drivers/nouveau/bind", bdf);
    std::thread::sleep(std::time::Duration::from_secs(3));

    // 4. Unbind nouveau
    let _ = std::fs::write("/sys/bus/pci/drivers/nouveau/unbind", bdf);
    std::thread::sleep(std::time::Duration::from_millis(500));

    // 5. Restore vfio-pci override
    if let Err(e) = std::fs::write(&override_path, "vfio-pci") {
        tracing::error!(bdf, error = %e, "ember.warm_cycle: failed to restore vfio-pci override");
        return write_jsonrpc_error(stream, id, -32000, &format!("warm cycle restore failed: {e}"))
            .map_err(EmberIpcError::from);
    }

    // 6. Reacquire
    match coral_driver::vfio::VfioDevice::open(bdf) {
        Ok(device) => {
            let req_eventfd = crate::arm_req_irq(&device, bdf);
            tracing::info!(bdf, "ember.warm_cycle: device reacquired (fresh)");
            let mut map = held.write().map_err(|_| EmberIpcError::LockPoisoned)?;
            map.insert(
                bdf.to_string(),
                HeldDevice {
                    bdf: bdf.to_string(),
                    device,
                    bar0: None,
                    ring_meta: crate::hold::RingMeta::default(),
                    req_eventfd,
                    experiment_dirty: false,
                    needs_warm_cycle: false,
                    dma_prepare_state: None,
                    mmio_fault_count: 0,
                    health: crate::hold::DeviceHealth::Alive,
                    pcie_armor: Some(crate::pcie_armor::PcieArmor::arm(bdf)),
                },
            );
            drop(map);
            write_jsonrpc_ok(stream, id, serde_json::json!({"bdf": bdf, "warm_cycle": "ok"}))
                .map_err(EmberIpcError::from)
        }
        Err(e) => {
            tracing::error!(bdf, error = %e, "ember.warm_cycle: reacquire failed");
            write_jsonrpc_error(stream, id, -32000, &format!("warm cycle reacquire failed: {e}"))
                .map_err(EmberIpcError::from)
        }
    }
}

pub(crate) fn swap(
    stream: &mut impl Write,
    held: &Arc<RwLock<HashMap<String, HeldDevice>>>,
    managed_bdfs: &HashSet<String>,
    id: serde_json::Value,
    params: &serde_json::Value,
    journal: Option<&Arc<Journal>>,
) -> Result<(), EmberIpcError> {
    let bdf = params
        .get("bdf")
        .and_then(|v| v.as_str())
        .ok_or(EmberIpcError::InvalidRequest("missing 'bdf' parameter"))?;
    let target = params
        .get("target")
        .and_then(|v| v.as_str())
        .ok_or(EmberIpcError::InvalidRequest("missing 'target' parameter"))?;
    let enable_trace = params
        .get("trace")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    finish_managed_bdf_early(require_managed_bdf(bdf, managed_bdfs, stream, &id))?;
    if sysfs::is_d3cold(bdf) {
        write_jsonrpc_error(
            stream,
            id,
            -32000,
            &format!("{bdf} is D3cold — cannot swap"),
        )
        .map_err(EmberIpcError::from)?;
        return Ok(());
    }
    let mut map = held.write().map_err(|_| EmberIpcError::LockPoisoned)?;
    match swap::handle_swap_device_with_journal(bdf, target, &mut map, enable_trace, journal) {
        Ok(obs) => {
            drop(map);
            if let Some(j) = journal
                && let Err(e) = j.append(&JournalEntry::Swap(obs.clone()))
            {
                tracing::warn!(error = %e, "journal append failed for swap");
            }
            let obs_json = serde_json::to_value(&obs).unwrap_or_else(|e| {
                serde_json::json!({"bdf": bdf, "to_personality": obs.to_personality, "error": e.to_string()})
            });
            write_jsonrpc_ok(stream, id, obs_json).map_err(EmberIpcError::from)
        }
        Err(e) => {
            drop(map);
            write_jsonrpc_error(stream, id, -32000, &e.to_string()).map_err(EmberIpcError::from)
        }
    }
}

pub(crate) fn device_reset(
    stream: &mut impl Write,
    managed_bdfs: &HashSet<String>,
    id: serde_json::Value,
    params: &serde_json::Value,
    journal: Option<&Arc<Journal>>,
) -> Result<(), EmberIpcError> {
    let bdf = params
        .get("bdf")
        .and_then(|v| v.as_str())
        .ok_or(EmberIpcError::InvalidRequest("missing 'bdf' parameter"))?;
    let method = params
        .get("method")
        .and_then(|v| v.as_str())
        .unwrap_or("auto");
    finish_managed_bdf_early(require_managed_bdf(bdf, managed_bdfs, stream, &id))?;
    if sysfs::is_d3cold(bdf) {
        write_jsonrpc_error(
            stream,
            id,
            -32000,
            &format!("{bdf} is D3cold — cannot reset"),
        )
        .map_err(EmberIpcError::from)?;
        return Ok(());
    }

    let lifecycle = crate::vendor_lifecycle::detect_lifecycle(bdf);
    let methods = lifecycle.available_reset_methods();
    tracing::info!(
        bdf,
        method,
        available = ?methods,
        "ember.device_reset: starting"
    );

    let reset_start = std::time::Instant::now();
    let result: Result<(), SwapError> = match method {
        "sbr" => sysfs::pci_device_reset(bdf).map_err(Into::into),
        "bridge-sbr" => sysfs::pci_bridge_reset(bdf).map_err(Into::into),
        "remove-rescan" => sysfs::pci_remove_rescan(bdf).map_err(Into::into),
        "auto" => try_reset_methods(bdf, &methods).map_err(Into::into),
        other => Err(SwapError::InvalidResetMethod(format!(
            "{other} (use 'auto', 'sbr', 'bridge-sbr', 'remove-rescan')"
        ))),
    };
    let duration_ms = reset_start.elapsed().as_millis() as u64;

    if let Some(j) = journal {
        let obs = crate::observation::ResetObservation {
            bdf: bdf.to_string(),
            method: method.to_string(),
            success: result.is_ok(),
            error: result.as_ref().err().map(ToString::to_string),
            timestamp_epoch_ms: crate::observation::epoch_ms(),
            duration_ms,
        };
        if let Err(e) = j.append(&JournalEntry::Reset(obs)) {
            tracing::warn!(error = %e, "journal append failed for reset");
        }
    }

    match result {
        Ok(()) => {
            tracing::info!(bdf, method, duration_ms, "PCI device reset complete");
            write_jsonrpc_ok(
                stream,
                id,
                serde_json::json!({
                    "bdf": bdf,
                    "reset": true,
                    "method": method,
                    "duration_ms": duration_ms,
                }),
            )
            .map_err(EmberIpcError::from)
        }
        Err(e) => {
            tracing::error!(bdf, method, error = %e, duration_ms, "PCI device reset failed");
            write_jsonrpc_error(stream, id, -32000, &format!("reset failed: {e}"))
                .map_err(EmberIpcError::from)
        }
    }
}

pub(crate) fn status(
    stream: &mut impl Write,
    held: &Arc<RwLock<HashMap<String, HeldDevice>>>,
    id: serde_json::Value,
    started_at: std::time::Instant,
) -> Result<(), EmberIpcError> {
    let map = held.read().map_err(|_| EmberIpcError::LockPoisoned)?;
    let devices: Vec<String> = map.keys().cloned().collect();
    drop(map);
    write_jsonrpc_ok(
        stream,
        id,
        serde_json::json!({
            "devices": devices,
            "uptime_secs": started_at.elapsed().as_secs(),
        }),
    )
    .map_err(EmberIpcError::from)
}

pub(crate) fn ring_meta_get(
    stream: &mut impl Write,
    held: &Arc<RwLock<HashMap<String, HeldDevice>>>,
    id: serde_json::Value,
    params: &serde_json::Value,
) -> Result<(), EmberIpcError> {
    let bdf = params
        .get("bdf")
        .and_then(|v| v.as_str())
        .ok_or(EmberIpcError::InvalidRequest("missing 'bdf' parameter"))?;
    let map = held.read().map_err(|_| EmberIpcError::LockPoisoned)?;
    if let Some(device) = map.get(bdf) {
        let meta_json = serde_json::to_value(&device.ring_meta).unwrap_or_default();
        drop(map);
        write_jsonrpc_ok(stream, id, meta_json).map_err(EmberIpcError::from)
    } else {
        drop(map);
        write_jsonrpc_error(stream, id, -32000, &format!("{bdf}: not held by ember"))
            .map_err(EmberIpcError::from)
    }
}

pub(crate) fn ring_meta_set(
    stream: &mut impl Write,
    held: &Arc<RwLock<HashMap<String, HeldDevice>>>,
    id: serde_json::Value,
    params: &serde_json::Value,
) -> Result<(), EmberIpcError> {
    let bdf = params
        .get("bdf")
        .and_then(|v| v.as_str())
        .ok_or(EmberIpcError::InvalidRequest("missing 'bdf' parameter"))?;
    let meta_val = params
        .get("ring_meta")
        .ok_or(EmberIpcError::InvalidRequest(
            "missing 'ring_meta' parameter",
        ))?;
    let meta: crate::hold::RingMeta = serde_json::from_value(meta_val.clone())
        .map_err(|e| EmberIpcError::JsonSerialize(format!("invalid ring_meta: {e}")))?;
    let mut map = held.write().map_err(|_| EmberIpcError::LockPoisoned)?;
    if let Some(device) = map.get_mut(bdf) {
        device.ring_meta = meta;
        drop(map);
        write_jsonrpc_ok(stream, id, serde_json::json!({"ok": true})).map_err(EmberIpcError::from)
    } else {
        drop(map);
        write_jsonrpc_error(stream, id, -32000, &format!("{bdf}: not held by ember"))
            .map_err(EmberIpcError::from)
    }
}

/// Safely prepare a device for DMA experiments.
///
/// Maps BAR0 server-side, runs the centralized quiesce sequence
/// (PFIFO reset, scheduler stop, blind PRI ACK), masks AER, and
/// optionally enables bus mastering. Stores the DMA state for later cleanup.
///
/// Params: `{bdf, bus_master?}`
///   - `bus_master`: `true` to enable PCIe bus mastering (default: `false`).
///     Only set `true` when the experiment needs the GPU to DMA to system
///     memory. For PHYS_VID falcon experiments (internal VRAM access),
///     keep bus master off — this prevents the GPU from issuing PCIe DMA
///     that can lock up the root complex if page tables are misconfigured.
pub(crate) fn prepare_dma(
    stream: &mut impl Write,
    held: &Arc<RwLock<HashMap<String, HeldDevice>>>,
    id: serde_json::Value,
    params: &serde_json::Value,
) -> Result<(), EmberIpcError> {
    let bdf = params
        .get("bdf")
        .and_then(|v| v.as_str())
        .ok_or(EmberIpcError::InvalidRequest("missing 'bdf' parameter"))?;
    let enable_bus_master = params
        .get("bus_master")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);

    let mut map = held.write().map_err(|_| EmberIpcError::LockPoisoned)?;
    let dev = match map.get_mut(bdf) {
        Some(d) => d,
        None => {
            drop(map);
            return write_jsonrpc_error(
                stream,
                id,
                -32000,
                &format!("{bdf}: not held by ember"),
            )
            .map_err(EmberIpcError::from);
        }
    };

    if dev.dma_prepare_state.is_some() {
        drop(map);
        return write_jsonrpc_error(
            stream,
            id,
            -32001,
            &format!("{bdf}: DMA already prepared (call ember.cleanup_dma first)"),
        )
        .map_err(EmberIpcError::from);
    }

    if dev.bar0.is_none() {
        match dev.device.map_bar(0) {
            Ok(b) => { dev.bar0 = Some(b); }
            Err(e) => {
                drop(map);
                return write_jsonrpc_error(
                    stream,
                    id,
                    -32000,
                    &format!("{bdf}: BAR0 map failed: {e}"),
                )
                .map_err(EmberIpcError::from);
            }
        }
    }

    // prepare_dma does BAR0 writes (PMC reset, PFIFO quiesce, PRI ring ACK)
    // that can stall. Wrap in fork isolation so a stuck BAR0 kills the child,
    // not ember.
    let bar0 = dev.bar0.as_ref().unwrap();
    let bar0_ptr = bar0.base_ptr() as usize;
    let bar0_size = bar0.size();
    let bdf_owned = bdf.to_string();

    tracing::info!(bdf, "ember.prepare_dma: launching fork-isolated child");

    let fork_result = crate::isolation::fork_isolated_mmio(
        &bdf_owned,
        crate::isolation::OperationTier::EngineReset.timeout(),
        |pipe_fd| {
            use super::handlers_mmio::write_json_to_pipe_fd;
            #[allow(unsafe_code)]
            let child_bar0 = unsafe {
                coral_driver::vfio::device::MappedBar::from_raw(bar0_ptr as *mut u8, bar0_size)
            };
            let result = coral_driver::vfio::device::dma_safety::prepare_dma_bar0_only(&child_bar0);
            match result {
                Ok((pmc_before, pmc_after)) => {
                    write_json_to_pipe_fd(
                        pipe_fd,
                        serde_json::json!({
                            "ok": true,
                            "pmc_before": pmc_before,
                            "pmc_after": pmc_after,
                        }),
                    );
                }
                Err(e) => {
                    write_json_to_pipe_fd(
                        pipe_fd,
                        serde_json::json!({"ok": false, "error": format!("{e}")}),
                    );
                }
            }
            std::mem::forget(child_bar0);
        },
    );

    match fork_result {
        crate::isolation::ForkResult::Ok(pipe_data) => {
            let parsed: serde_json::Value = serde_json::from_slice(&pipe_data)
                .unwrap_or(serde_json::json!({"ok": false, "error": "pipe parse failed"}));
            if parsed.get("ok").and_then(|v| v.as_bool()).unwrap_or(false) {
                let pmc_before = parsed.get("pmc_before").and_then(|v| v.as_u64()).unwrap_or(0) as u32;
                let pmc_after = parsed.get("pmc_after").and_then(|v| v.as_u64()).unwrap_or(0) as u32;

                // PCI-level operations (AER mask, optional bus master) run in parent
                // since they use VFIO ioctls, not BAR0 MMIO.
                let aer_state = match dev.device.mask_aer() {
                    Ok(aer) => Some(aer),
                    Err(e) => {
                        tracing::warn!(bdf = bdf_owned, error = %e, "AER mask failed (non-fatal)");
                        None
                    }
                };
                if enable_bus_master {
                    if let Err(e) = dev.device.enable_bus_master() {
                        drop(map);
                        return write_jsonrpc_error(
                            stream, id, -32000,
                            &format!("{bdf_owned}: bus_master enable failed: {e}"),
                        ).map_err(EmberIpcError::from);
                    }
                    tracing::info!(bdf = bdf_owned, "bus master ENABLED (experiment requested it)");
                } else {
                    tracing::info!(bdf = bdf_owned, "bus master stays OFF (PHYS_VID safe mode)");
                }

                dev.dma_prepare_state = Some(coral_driver::vfio::device::dma_safety::DmaPrepareState {
                    aer_state, pmc_before, pmc_after,
                });
                dev.experiment_dirty = true;
                drop(map);
                tracing::info!(bdf = bdf_owned, "ember.prepare_dma: complete (fork-isolated)");
                write_jsonrpc_ok(
                    stream, id,
                    serde_json::json!({
                        "bdf": bdf_owned,
                        "ok": true,
                        "pmc_before": format!("{pmc_before:#010x}"),
                        "pmc_after": format!("{pmc_after:#010x}"),
                    }),
                ).map_err(EmberIpcError::from)
            } else {
                let err_msg = parsed.get("error").and_then(|v| v.as_str()).unwrap_or("unknown");
                drop(map);
                tracing::error!(bdf = bdf_owned, "ember.prepare_dma: child reported failure: {err_msg}");
                write_jsonrpc_error(stream, id, -32000, &format!("prepare_dma: {err_msg}"))
                    .map_err(EmberIpcError::from)
            }
        }

        crate::isolation::ForkResult::Timeout => {
            tracing::error!(bdf = bdf_owned, "prepare_dma: fork child TIMED OUT — device faulted");
            dev.emergency_quiesce();
            drop(map);
            crate::hold::check_voluntary_death(held);
            write_jsonrpc_error(
                stream, id, -32099,
                &format!("{bdf_owned}: prepare_dma timed out — GPU bus-reset, device faulted."),
            ).map_err(EmberIpcError::from)
        }

        crate::isolation::ForkResult::ChildFailed { status } => {
            tracing::error!(bdf = bdf_owned, status, "prepare_dma: fork child failed");
            dev.emergency_quiesce();
            drop(map);
            crate::hold::check_voluntary_death(held);
            write_jsonrpc_error(
                stream, id, -32098,
                &format!("{bdf_owned}: prepare_dma child failed (status={status})."),
            ).map_err(EmberIpcError::from)
        }

        crate::isolation::ForkResult::ForkFailed(e) | crate::isolation::ForkResult::PipeFailed(e) => {
            drop(map);
            write_jsonrpc_error(
                stream, id, -32000,
                &format!("{bdf_owned}: prepare_dma fork/pipe failed: {e}"),
            ).map_err(EmberIpcError::from)
        }
    }
}

/// Clean up after a DMA experiment — disable bus master, restore AER masks,
/// and decontaminate the GPU if the experiment modified hardware state.
///
/// GPU decontamination (PRI drain + SEC2 PMC reset) is critical: experiments
/// that start falcon CPUs leave the GPU's internal PRI ring in a dirty state.
/// Without decontamination, the next experiment's PRAMIN writes can trigger
/// an unrecoverable PCIe flow-control stall that locks the entire system.
pub(crate) fn cleanup_dma(
    stream: &mut impl Write,
    held: &Arc<RwLock<HashMap<String, HeldDevice>>>,
    id: serde_json::Value,
    params: &serde_json::Value,
) -> Result<(), EmberIpcError> {
    let bdf = params
        .get("bdf")
        .and_then(|v| v.as_str())
        .ok_or(EmberIpcError::InvalidRequest("missing 'bdf' parameter"))?;

    let mut map = held.write().map_err(|_| EmberIpcError::LockPoisoned)?;
    let dev = match map.get_mut(bdf) {
        Some(d) => d,
        None => {
            drop(map);
            return write_jsonrpc_error(
                stream,
                id,
                -32000,
                &format!("{bdf}: not held by ember"),
            )
            .map_err(EmberIpcError::from);
        }
    };

    let state = match dev.dma_prepare_state.take() {
        Some(s) => s,
        None => {
            drop(map);
            return write_jsonrpc_error(
                stream,
                id,
                -32001,
                &format!("{bdf}: no DMA prepare state (call ember.prepare_dma first)"),
            )
            .map_err(EmberIpcError::from);
        }
    };

    let was_dirty = dev.experiment_dirty;
    let bdf_owned = bdf.to_string();

    // Decontaminate GPU before restoring DMA state. If the experiment started
    // a falcon CPU, the GPU's PRI ring may have pending errors that would
    // cause the next experiment's PRAMIN writes to stall catastrophically.
    let mut decontaminate_note = String::new();
    if was_dirty {
        if let Some(ref bar0) = dev.bar0 {
            let bar0_ptr = bar0.base_ptr() as usize;
            let bar0_size = bar0.size();
            drop(map);

            tracing::info!(bdf = bdf_owned, "cleanup_dma: GPU is dirty — decontaminating");
            let decon_result = crate::isolation::decontaminate_gpu(
                &bdf_owned, bar0_ptr, bar0_size,
            );

            decontaminate_note = match decon_result {
                crate::isolation::DecontaminateResult::Clean => {
                    "gpu_soft_reset_clean".to_string()
                }
                crate::isolation::DecontaminateResult::SbrTriggered => {
                    tracing::warn!(bdf = bdf_owned, "GPU soft reset failed — SBR escalation fired");
                    "gpu_sbr_escalation".to_string()
                }
                crate::isolation::DecontaminateResult::StillDirty => {
                    tracing::warn!(bdf = bdf_owned, "GPU decontamination failed");
                    "gpu_decontamination_failed".to_string()
                }
            };

            map = held.write().map_err(|_| EmberIpcError::LockPoisoned)?;
            if let Some(d) = map.get_mut(&*bdf_owned) {
                d.experiment_dirty = false;
            }
        } else {
            dev.experiment_dirty = false;
        }
    }

    let result =
        coral_driver::vfio::device::dma_safety::cleanup_dma(
            &map.get(&*bdf_owned)
                .ok_or(EmberIpcError::InvalidRequest("device gone"))?
                .device,
            &state,
        );
    drop(map);

    match result {
        Ok(()) => {
            tracing::info!(bdf = bdf_owned, decontaminate = %decontaminate_note, "ember.cleanup_dma: complete");
            write_jsonrpc_ok(
                stream,
                id,
                serde_json::json!({
                    "bdf": bdf_owned,
                    "ok": true,
                    "decontaminated": was_dirty,
                    "decontaminate_result": decontaminate_note,
                }),
            )
            .map_err(EmberIpcError::from)
        }
        Err(e) => {
            tracing::error!(bdf = bdf_owned, error = %e, "ember.cleanup_dma: failed");
            write_jsonrpc_error(stream, id, -32000, &format!("cleanup_dma: {e}"))
                .map_err(EmberIpcError::from)
        }
    }
}
