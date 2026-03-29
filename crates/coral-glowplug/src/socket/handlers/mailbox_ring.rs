// SPDX-License-Identifier: AGPL-3.0-only
//! JSON-RPC handlers for mailbox and ring operations.
//!
//! Exposes the posted-command mailbox and multi-ring systems to ecosystem
//! primals (notably hotSpring) for GPU firmware probing and hardware testing.
//!
//! ## Semantic methods
//!
//! | Method               | Description                                      |
//! |----------------------|--------------------------------------------------|
//! | `mailbox.create`     | Create a named mailbox on a device                |
//! | `mailbox.post`       | Post a command to a device's mailbox              |
//! | `mailbox.poll`       | Poll a posted command's completion status          |
//! | `mailbox.complete`   | Mark a command as complete (test/simulation use)   |
//! | `mailbox.drain`      | Drain completed entries from a mailbox             |
//! | `mailbox.stats`      | Get mailbox statistics for a device                |
//! | `ring.create`        | Create a named ring on a device                   |
//! | `ring.submit`        | Submit an entry to a device's ring                |
//! | `ring.consume`       | Consume the next pending ring entry                |
//! | `ring.fence`         | Consume entries through a fence value              |
//! | `ring.peek`          | Peek at the next pending entry without consuming   |
//! | `ring.stats`         | Get ring statistics for a device                  |

use std::sync::Arc;
use std::time::Duration;

use coral_glowplug::error::RpcError;
use coral_glowplug::mailbox::{Mailbox, PostedCommand};
use coral_glowplug::ring::{Ring, RingPayload};

use super::validate_bdf;

/// Dispatch a `mailbox.*` or `ring.*` method.
pub(crate) fn dispatch(
    method: &str,
    params: &serde_json::Value,
    devices: &mut [coral_glowplug::device::DeviceSlot],
) -> Result<serde_json::Value, RpcError> {
    match method {
        "mailbox.create" => handle_mailbox_create(params, devices),
        "mailbox.post" => handle_mailbox_post(params, devices),
        "mailbox.poll" => handle_mailbox_poll(params, devices),
        "mailbox.complete" => handle_mailbox_complete(params, devices),
        "mailbox.drain" => handle_mailbox_drain(params, devices),
        "mailbox.stats" => handle_mailbox_stats(params, devices),
        "ring.create" => handle_ring_create(params, devices),
        "ring.submit" => handle_ring_submit(params, devices),
        "ring.consume" => handle_ring_consume(params, devices),
        "ring.fence" => handle_ring_fence(params, devices),
        "ring.peek" => handle_ring_peek(params, devices),
        "ring.stats" => handle_ring_stats(params, devices),
        other => Err(RpcError::method_not_found(other)),
    }
}

fn find_device_mut<'a>(
    bdf: &str,
    devices: &'a mut [coral_glowplug::device::DeviceSlot],
) -> Result<&'a mut coral_glowplug::device::DeviceSlot, RpcError> {
    let bdf = validate_bdf(bdf)?;
    devices
        .iter_mut()
        .find(|d| d.bdf.as_ref() == bdf)
        .ok_or_else(|| {
            coral_glowplug::error::DeviceError::NotManaged {
                bdf: Arc::from(bdf),
            }
            .into()
        })
}

fn require_bdf(params: &serde_json::Value) -> Result<&str, RpcError> {
    params
        .get("bdf")
        .and_then(serde_json::Value::as_str)
        .ok_or_else(|| RpcError::invalid_params("missing 'bdf' parameter"))
}

fn require_str<'a>(params: &'a serde_json::Value, key: &str) -> Result<&'a str, RpcError> {
    params
        .get(key)
        .and_then(serde_json::Value::as_str)
        .ok_or_else(|| RpcError::invalid_params(format!("missing '{key}' parameter")))
}

fn require_u64(params: &serde_json::Value, key: &str) -> Result<u64, RpcError> {
    params
        .get(key)
        .and_then(serde_json::Value::as_u64)
        .ok_or_else(|| RpcError::invalid_params(format!("missing '{key}' parameter")))
}

// --- Mailbox handlers ---

fn handle_mailbox_create(
    params: &serde_json::Value,
    devices: &mut [coral_glowplug::device::DeviceSlot],
) -> Result<serde_json::Value, RpcError> {
    let bdf = require_bdf(params)?;
    let engine = require_str(params, "engine")?;
    let capacity = params
        .get("capacity")
        .and_then(serde_json::Value::as_u64)
        .unwrap_or(16) as usize;

    let dev = find_device_mut(bdf, devices)?;

    if dev.mailboxes.get(engine).is_some() {
        return Err(RpcError::invalid_params(format!(
            "mailbox '{engine}' already exists on {bdf}"
        )));
    }

    dev.mailboxes.add(Mailbox::new(engine, capacity));

    Ok(serde_json::json!({
        "bdf": dev.bdf.as_ref(),
        "engine": engine,
        "capacity": capacity,
    }))
}

fn handle_mailbox_post(
    params: &serde_json::Value,
    devices: &mut [coral_glowplug::device::DeviceSlot],
) -> Result<serde_json::Value, RpcError> {
    let bdf = require_bdf(params)?;
    let engine = require_str(params, "engine")?;
    let register = require_u64(params, "register")? as u32;
    let command = require_u64(params, "command")? as u32;
    let status_register = require_u64(params, "status_register")? as u32;
    let expected_status = params
        .get("expected_status")
        .and_then(serde_json::Value::as_u64)
        .unwrap_or(0) as u32;
    let status_mask = params
        .get("status_mask")
        .and_then(serde_json::Value::as_u64)
        .unwrap_or(0xFFFF_FFFF) as u32;
    let timeout_ms = params
        .get("timeout_ms")
        .and_then(serde_json::Value::as_u64)
        .unwrap_or(5000);

    let dev = find_device_mut(bdf, devices)?;
    let mb = dev
        .mailboxes
        .get_mut(engine)
        .ok_or_else(|| RpcError::invalid_params(format!("no mailbox '{engine}' on {bdf}")))?;

    let cmd = PostedCommand {
        register,
        command,
        status_register,
        expected_status,
        status_mask,
        timeout: Duration::from_millis(timeout_ms),
    };

    let seq = mb
        .post(cmd)
        .map_err(|e| RpcError::device_error(e.to_string()))?;

    Ok(serde_json::json!({
        "seq": seq.raw(),
        "engine": engine,
    }))
}

fn handle_mailbox_poll(
    params: &serde_json::Value,
    devices: &mut [coral_glowplug::device::DeviceSlot],
) -> Result<serde_json::Value, RpcError> {
    let bdf = require_bdf(params)?;
    let engine = require_str(params, "engine")?;
    let seq_raw = require_u64(params, "seq")?;

    let dev = find_device_mut(bdf, devices)?;
    let mb = dev
        .mailboxes
        .get(engine)
        .ok_or_else(|| RpcError::invalid_params(format!("no mailbox '{engine}' on {bdf}")))?;

    let seq = coral_glowplug::mailbox::Sequence::from_raw(seq_raw);
    let result = mb
        .poll(seq)
        .map_err(|e| RpcError::device_error(e.to_string()))?;

    Ok(serde_json::json!({
        "seq": seq_raw,
        "state": format!("{:?}", result.state),
        "elapsed_ms": result.elapsed.as_millis() as u64,
    }))
}

fn handle_mailbox_complete(
    params: &serde_json::Value,
    devices: &mut [coral_glowplug::device::DeviceSlot],
) -> Result<serde_json::Value, RpcError> {
    let bdf = require_bdf(params)?;
    let engine = require_str(params, "engine")?;
    let seq_raw = require_u64(params, "seq")?;
    let status = require_u64(params, "status")? as u32;

    let dev = find_device_mut(bdf, devices)?;
    let mb = dev
        .mailboxes
        .get_mut(engine)
        .ok_or_else(|| RpcError::invalid_params(format!("no mailbox '{engine}' on {bdf}")))?;

    mb.complete(coral_glowplug::mailbox::Sequence::from_raw(seq_raw), status);

    Ok(serde_json::json!({ "ok": true }))
}

fn handle_mailbox_drain(
    params: &serde_json::Value,
    devices: &mut [coral_glowplug::device::DeviceSlot],
) -> Result<serde_json::Value, RpcError> {
    let bdf = require_bdf(params)?;
    let engine = require_str(params, "engine")?;

    let dev = find_device_mut(bdf, devices)?;
    let mb = dev
        .mailboxes
        .get_mut(engine)
        .ok_or_else(|| RpcError::invalid_params(format!("no mailbox '{engine}' on {bdf}")))?;

    mb.expire_stale();
    let completed = mb.drain_completed();

    let entries: Vec<serde_json::Value> = completed
        .iter()
        .map(|c| {
            serde_json::json!({
                "seq": c.seq.raw(),
                "state": format!("{:?}", c.state),
                "latency_ms": c.latency.as_millis() as u64,
            })
        })
        .collect();

    Ok(serde_json::json!({
        "engine": engine,
        "drained": entries.len(),
        "entries": entries,
    }))
}

fn handle_mailbox_stats(
    params: &serde_json::Value,
    devices: &mut [coral_glowplug::device::DeviceSlot],
) -> Result<serde_json::Value, RpcError> {
    let bdf = require_bdf(params)?;
    let dev = find_device_mut(bdf, devices)?;
    let stats = dev.mailboxes.all_stats();
    serde_json::to_value(stats).map_err(|e| RpcError::internal(e.to_string()))
}

// --- Ring handlers ---

fn handle_ring_create(
    params: &serde_json::Value,
    devices: &mut [coral_glowplug::device::DeviceSlot],
) -> Result<serde_json::Value, RpcError> {
    let bdf = require_bdf(params)?;
    let name = require_str(params, "name")?;
    let capacity = params
        .get("capacity")
        .and_then(serde_json::Value::as_u64)
        .unwrap_or(64) as usize;

    let dev = find_device_mut(bdf, devices)?;

    if dev.rings.get(name).is_some() {
        return Err(RpcError::invalid_params(format!(
            "ring '{name}' already exists on {bdf}"
        )));
    }

    dev.rings.add(Ring::new(name, capacity));

    Ok(serde_json::json!({
        "bdf": dev.bdf.as_ref(),
        "ring": name,
        "capacity": capacity,
    }))
}

fn handle_ring_submit(
    params: &serde_json::Value,
    devices: &mut [coral_glowplug::device::DeviceSlot],
) -> Result<serde_json::Value, RpcError> {
    let bdf = require_bdf(params)?;
    let ring_name = require_str(params, "ring")?;
    let method = require_str(params, "method")?;
    let data = params
        .get("data")
        .and_then(serde_json::Value::as_str)
        .unwrap_or("");

    let dev = find_device_mut(bdf, devices)?;
    let ring = dev
        .rings
        .get_mut(ring_name)
        .ok_or_else(|| RpcError::invalid_params(format!("no ring '{ring_name}' on {bdf}")))?;

    let payload = RingPayload {
        method: method.to_string(),
        data: data.as_bytes().to_vec(),
    };

    let (id, fence) = ring
        .submit(payload)
        .map_err(|e| RpcError::device_error(e.to_string()))?;

    Ok(serde_json::json!({
        "id": id.raw(),
        "fence": fence,
        "ring": ring_name,
    }))
}

fn handle_ring_consume(
    params: &serde_json::Value,
    devices: &mut [coral_glowplug::device::DeviceSlot],
) -> Result<serde_json::Value, RpcError> {
    let bdf = require_bdf(params)?;
    let ring_name = require_str(params, "ring")?;

    let dev = find_device_mut(bdf, devices)?;
    let ring = dev
        .rings
        .get_mut(ring_name)
        .ok_or_else(|| RpcError::invalid_params(format!("no ring '{ring_name}' on {bdf}")))?;

    let entry = ring
        .consume()
        .map_err(|e| RpcError::device_error(e.to_string()))?;

    Ok(serde_json::json!({
        "id": entry.id.raw(),
        "fence": entry.fence,
        "method": entry.payload.method,
        "latency_ms": entry.latency().as_millis() as u64,
    }))
}

fn handle_ring_fence(
    params: &serde_json::Value,
    devices: &mut [coral_glowplug::device::DeviceSlot],
) -> Result<serde_json::Value, RpcError> {
    let bdf = require_bdf(params)?;
    let ring_name = require_str(params, "ring")?;
    let fence = require_u64(params, "fence")?;

    let dev = find_device_mut(bdf, devices)?;
    let ring = dev
        .rings
        .get_mut(ring_name)
        .ok_or_else(|| RpcError::invalid_params(format!("no ring '{ring_name}' on {bdf}")))?;

    let consumed = ring.consume_through_fence(fence);

    let entries: Vec<serde_json::Value> = consumed
        .iter()
        .map(|e| {
            serde_json::json!({
                "id": e.id.raw(),
                "fence": e.fence,
                "method": e.payload.method,
                "latency_ms": e.latency().as_millis() as u64,
            })
        })
        .collect();

    Ok(serde_json::json!({
        "ring": ring_name,
        "consumed": entries.len(),
        "entries": entries,
    }))
}

fn handle_ring_peek(
    params: &serde_json::Value,
    devices: &mut [coral_glowplug::device::DeviceSlot],
) -> Result<serde_json::Value, RpcError> {
    let bdf = require_bdf(params)?;
    let ring_name = require_str(params, "ring")?;

    let dev = find_device_mut(bdf, devices)?;
    let ring = dev
        .rings
        .get(ring_name)
        .ok_or_else(|| RpcError::invalid_params(format!("no ring '{ring_name}' on {bdf}")))?;

    match ring.peek() {
        Some(entry) => Ok(serde_json::json!({
            "id": entry.id.raw(),
            "fence": entry.fence,
            "method": entry.payload.method,
            "pending_ms": entry.latency().as_millis() as u64,
        })),
        None => Ok(serde_json::json!({ "empty": true })),
    }
}

fn handle_ring_stats(
    params: &serde_json::Value,
    devices: &mut [coral_glowplug::device::DeviceSlot],
) -> Result<serde_json::Value, RpcError> {
    let bdf = require_bdf(params)?;
    let dev = find_device_mut(bdf, devices)?;
    let stats = dev.rings.all_stats();
    serde_json::to_value(stats).map_err(|e| RpcError::internal(e.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use coral_glowplug::device::DeviceSlot;

    fn test_device(bdf: &str) -> DeviceSlot {
        DeviceSlot::new(super::super::test_device_config(bdf))
    }

    #[test]
    fn mailbox_create_and_stats() {
        let mut devices = vec![test_device("0000:01:00.0")];
        let params = serde_json::json!({
            "bdf": "0000:01:00.0",
            "engine": "fecs",
            "capacity": 8,
        });
        let result = dispatch("mailbox.create", &params, &mut devices).expect("create mailbox");
        assert_eq!(result["engine"], "fecs");
        assert_eq!(result["capacity"], 8);

        let stats_params = serde_json::json!({ "bdf": "0000:01:00.0" });
        let stats = dispatch("mailbox.stats", &stats_params, &mut devices).expect("stats");
        let arr = stats.as_array().expect("stats is array");
        assert_eq!(arr.len(), 1);
        assert_eq!(arr[0]["name"], "fecs");
    }

    #[test]
    fn mailbox_create_duplicate_fails() {
        let mut devices = vec![test_device("0000:01:00.0")];
        let params = serde_json::json!({
            "bdf": "0000:01:00.0",
            "engine": "fecs",
        });
        dispatch("mailbox.create", &params, &mut devices).expect("first create");
        let err = dispatch("mailbox.create", &params, &mut devices).expect_err("duplicate");
        assert!(err.message.contains("already exists"));
    }

    #[test]
    fn mailbox_post_poll_complete_drain_lifecycle() {
        let mut devices = vec![test_device("0000:01:00.0")];
        let bdf = "0000:01:00.0";

        dispatch(
            "mailbox.create",
            &serde_json::json!({ "bdf": bdf, "engine": "fecs" }),
            &mut devices,
        )
        .expect("create");

        let post_result = dispatch(
            "mailbox.post",
            &serde_json::json!({
                "bdf": bdf,
                "engine": "fecs",
                "register": 0x0040_9800_u64,
                "command": 1,
                "status_register": 0x0040_9804_u64,
                "timeout_ms": 5000,
            }),
            &mut devices,
        )
        .expect("post");
        let seq = post_result["seq"].as_u64().expect("seq");

        let poll_result = dispatch(
            "mailbox.poll",
            &serde_json::json!({ "bdf": bdf, "engine": "fecs", "seq": seq }),
            &mut devices,
        )
        .expect("poll");
        assert!(
            poll_result["state"]
                .as_str()
                .expect("state")
                .contains("Posted")
        );

        dispatch(
            "mailbox.complete",
            &serde_json::json!({ "bdf": bdf, "engine": "fecs", "seq": seq, "status": 1 }),
            &mut devices,
        )
        .expect("complete");

        let drain_result = dispatch(
            "mailbox.drain",
            &serde_json::json!({ "bdf": bdf, "engine": "fecs" }),
            &mut devices,
        )
        .expect("drain");
        assert_eq!(drain_result["drained"], 1);
    }

    #[test]
    fn ring_create_submit_consume_lifecycle() {
        let mut devices = vec![test_device("0000:01:00.0")];
        let bdf = "0000:01:00.0";

        dispatch(
            "ring.create",
            &serde_json::json!({ "bdf": bdf, "name": "gpfifo", "capacity": 8 }),
            &mut devices,
        )
        .expect("create ring");

        let submit_result = dispatch(
            "ring.submit",
            &serde_json::json!({
                "bdf": bdf,
                "ring": "gpfifo",
                "method": "sec2.boot",
                "data": "deadbeef",
            }),
            &mut devices,
        )
        .expect("submit");
        let fence = submit_result["fence"].as_u64().expect("fence");
        assert!(fence > 0);

        let peek_result = dispatch(
            "ring.peek",
            &serde_json::json!({ "bdf": bdf, "ring": "gpfifo" }),
            &mut devices,
        )
        .expect("peek");
        assert_eq!(peek_result["method"], "sec2.boot");

        let consume_result = dispatch(
            "ring.consume",
            &serde_json::json!({ "bdf": bdf, "ring": "gpfifo" }),
            &mut devices,
        )
        .expect("consume");
        assert_eq!(consume_result["method"], "sec2.boot");
    }

    #[test]
    fn ring_fence_consumes_through_value() {
        let mut devices = vec![test_device("0000:01:00.0")];
        let bdf = "0000:01:00.0";

        dispatch(
            "ring.create",
            &serde_json::json!({ "bdf": bdf, "name": "ce0" }),
            &mut devices,
        )
        .expect("create");

        for i in 0..3 {
            dispatch(
                "ring.submit",
                &serde_json::json!({
                    "bdf": bdf,
                    "ring": "ce0",
                    "method": format!("cmd.{i}"),
                }),
                &mut devices,
            )
            .expect("submit");
        }

        let stats = dispatch(
            "ring.stats",
            &serde_json::json!({ "bdf": bdf }),
            &mut devices,
        )
        .expect("stats");
        assert_eq!(stats[0]["pending"], 3);

        let fence_result = dispatch(
            "ring.fence",
            &serde_json::json!({ "bdf": bdf, "ring": "ce0", "fence": 2 }),
            &mut devices,
        )
        .expect("fence");
        assert_eq!(fence_result["consumed"], 2);
    }

    #[test]
    fn ring_create_duplicate_fails() {
        let mut devices = vec![test_device("0000:01:00.0")];
        let params = serde_json::json!({
            "bdf": "0000:01:00.0",
            "name": "gpfifo",
        });
        dispatch("ring.create", &params, &mut devices).expect("first create");
        let err = dispatch("ring.create", &params, &mut devices).expect_err("duplicate");
        assert!(err.message.contains("already exists"));
    }

    #[test]
    fn missing_bdf_returns_invalid_params() {
        let mut devices = vec![test_device("0000:01:00.0")];
        let err =
            dispatch("mailbox.stats", &serde_json::json!({}), &mut devices).expect_err("no bdf");
        assert!(err.message.contains("bdf"));
    }

    #[test]
    fn unknown_device_returns_not_managed() {
        let mut devices = vec![test_device("0000:01:00.0")];
        let err = dispatch(
            "mailbox.stats",
            &serde_json::json!({ "bdf": "0000:99:00.0" }),
            &mut devices,
        )
        .expect_err("not managed");
        assert!(err.message.contains("not managed"));
    }

    #[test]
    fn ring_peek_empty_ring() {
        let mut devices = vec![test_device("0000:01:00.0")];
        dispatch(
            "ring.create",
            &serde_json::json!({ "bdf": "0000:01:00.0", "name": "empty" }),
            &mut devices,
        )
        .expect("create");
        let result = dispatch(
            "ring.peek",
            &serde_json::json!({ "bdf": "0000:01:00.0", "ring": "empty" }),
            &mut devices,
        )
        .expect("peek");
        assert_eq!(result["empty"], true);
    }

    #[test]
    fn consume_empty_ring_returns_error() {
        let mut devices = vec![test_device("0000:01:00.0")];
        dispatch(
            "ring.create",
            &serde_json::json!({ "bdf": "0000:01:00.0", "name": "empty" }),
            &mut devices,
        )
        .expect("create");
        let err = dispatch(
            "ring.consume",
            &serde_json::json!({ "bdf": "0000:01:00.0", "ring": "empty" }),
            &mut devices,
        )
        .expect_err("empty ring");
        assert!(err.message.contains("no pending"));
    }
}
