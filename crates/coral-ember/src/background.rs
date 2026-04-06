// SPDX-License-Identifier: AGPL-3.0-or-later
//! Background threads: VFIO REQ IRQ polling and systemd watchdog heartbeats.

use std::collections::HashMap;
use std::sync::{Arc, RwLock};

use crate::hold::HeldDevice;

/// Default watchdog interval in seconds (half a typical `WatchdogSec=30`).
const WATCHDOG_INTERVAL_SECS: u64 = 15;

/// Arm `VFIO_PCI_REQ_ERR_IRQ` (index 4) on a VFIO device.
///
/// When armed, the kernel signals this eventfd instead of printing
/// "No device request channel registered, blocked until released by user".
/// The [`spawn_req_watcher`] thread monitors all active eventfds and
/// auto-releases the VFIO fd before the kernel enters D-state.
pub(crate) fn arm_req_irq(
    device: &coral_driver::vfio::VfioDevice,
    bdf: &str,
) -> Option<std::os::fd::OwnedFd> {
    use coral_driver::vfio::irq::{VfioIrqIndex, arm_irq_eventfd};

    match arm_irq_eventfd(device.device_as_fd(), VfioIrqIndex::Req, 0) {
        Ok(fd) => {
            tracing::info!(bdf, "VFIO REQ IRQ armed — kernel can signal device release");
            Some(fd)
        }
        Err(e) => {
            tracing::warn!(
                bdf,
                error = %e,
                "failed to arm VFIO REQ IRQ (non-fatal — external unbind may D-state)"
            );
            None
        }
    }
}

/// Spawn a background thread that monitors all VFIO device request eventfds.
///
/// When the kernel signals a REQ IRQ (because someone wrote to `driver/unbind`
/// while ember holds the fd), this thread auto-releases the VFIO device from
/// the `held` map. This prevents the kernel from blocking indefinitely in
/// `wait_for_completion()` inside `vfio_unregister_group_dev()`, which is the
/// root cause of D-state cascades on Kepler and other FLR-lacking GPUs.
///
/// The thread rebuilds its poll set each cycle from `try_clone()`'d eventfds,
/// so it remains correct as devices are added/removed from the held map.
pub(crate) fn spawn_req_watcher(held: Arc<RwLock<HashMap<String, HeldDevice>>>) {
    use rustix::event::{PollFd, PollFlags, poll};
    use rustix::time::Timespec;

    std::thread::Builder::new()
        .name("ember-req-watcher".into())
        .spawn(move || {
            loop {
                let (cloned_fds, bdfs): (Vec<std::os::fd::OwnedFd>, Vec<String>) = {
                    let map = match held.read() {
                        Ok(m) => m,
                        Err(_) => break,
                    };
                    let mut fds = Vec::new();
                    let mut names = Vec::new();
                    for (bdf, dev) in map.iter() {
                        if let Some(ref req_fd) = dev.req_eventfd
                            && let Ok(cloned) = req_fd.try_clone()
                        {
                            fds.push(cloned);
                            names.push(bdf.clone());
                        }
                    }
                    (fds, names)
                };

                if cloned_fds.is_empty() {
                    std::thread::sleep(std::time::Duration::from_secs(2));
                    continue;
                }

                let mut poll_fds: Vec<PollFd<'_>> = cloned_fds
                    .iter()
                    .map(|fd| PollFd::new(fd, PollFlags::IN))
                    .collect();

                let timeout = Timespec {
                    tv_sec: 1,
                    tv_nsec: 0,
                };
                match poll(&mut poll_fds, Some(&timeout)) {
                    Ok(n) if n > 0 => {
                        for (i, pfd) in poll_fds.iter().enumerate() {
                            let revents = pfd.revents();
                            if revents.contains(PollFlags::IN) {
                                let bdf = &bdfs[i];
                                tracing::warn!(
                                    bdf,
                                    "VFIO device-release request from kernel — \
                                     auto-releasing VFIO fds to prevent D-state"
                                );

                                let mut buf = [0u8; 8];
                                let _ = rustix::io::read(&cloned_fds[i], &mut buf);

                                match held.try_write() {
                                    Ok(mut map) => {
                                        if let Some(device) = map.remove(bdf) {
                                            drop(device);
                                            tracing::info!(
                                                bdf,
                                                "device auto-released (kernel REQ IRQ)"
                                            );
                                        }
                                    }
                                    Err(_) => {
                                        tracing::warn!(
                                            bdf,
                                            "held lock busy — will retry auto-release \
                                             on next poll cycle"
                                        );
                                    }
                                }
                            }
                        }
                    }
                    _ => {}
                }
            }
            tracing::info!("req-watcher thread exiting");
        })
        .expect("spawn device request watcher thread");
}

/// Spawn a background thread that periodically:
/// 1. Sends `WATCHDOG=1` to systemd (if `NOTIFY_SOCKET` is set).
/// 2. Verifies held VFIO fds are still valid (ring-keeper liveness).
///
/// The thread is daemonic — it dies when the main process exits.
pub(crate) fn spawn_watchdog(held: Arc<RwLock<HashMap<String, HeldDevice>>>) {
    let interval = std::env::var("CORALREEF_EMBER_WATCHDOG_SECS")
        .ok()
        .and_then(|v| v.parse::<u64>().ok())
        .unwrap_or(WATCHDOG_INTERVAL_SECS);

    let notify_path = std::env::var("NOTIFY_SOCKET").ok();

    std::thread::Builder::new()
        .name("ember-watchdog".into())
        .spawn(move || {
            let interval = std::time::Duration::from_secs(interval);
            loop {
                std::thread::sleep(interval);

                let device_count = held.read().map(|map| map.len()).unwrap_or(0);

                if device_count == 0 {
                    tracing::warn!("watchdog: no devices held — ring-keeper degraded");
                }

                if let Some(ref path) = notify_path {
                    let _ = std::os::unix::net::UnixDatagram::unbound()
                        .and_then(|sock| sock.send_to(b"WATCHDOG=1", path));
                }

                tracing::trace!(devices = device_count, "watchdog: heartbeat");
            }
        })
        .expect("spawn ember watchdog thread");
}
