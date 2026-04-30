// SPDX-License-Identifier: AGPL-3.0-or-later
//! Map BAR0 for a VFIO-held device (shared by sovereign, devinit, mmio handlers).

use std::collections::HashMap;
use std::sync::{Arc, RwLock};

use coral_driver::vfio::device::{DmaBackend, MappedBar};

use crate::error::HeldBar0Error;
use crate::hold::HeldDevice;

fn with_held_device<R>(
    held: &Arc<RwLock<HashMap<String, HeldDevice>>>,
    bdf: &str,
    f: impl FnOnce(&HeldDevice) -> Result<R, HeldBar0Error>,
) -> Result<R, HeldBar0Error> {
    let map = held.read().map_err(|_| HeldBar0Error::LockPoisoned)?;
    let dev = map.get(bdf).ok_or_else(|| HeldBar0Error::NotHeld {
        bdf: bdf.to_string(),
    })?;
    f(dev)
}

/// Map BAR0 for `bdf` if it exists in ember's held-device table.
///
/// RW lock poison yields [`HeldBar0Error::LockPoisoned`].
pub(crate) fn map_held_bar0(
    held: &Arc<RwLock<HashMap<String, HeldDevice>>>,
    bdf: &str,
) -> Result<MappedBar, HeldBar0Error> {
    with_held_device(held, bdf, |dev| {
        dev.device
            .map_bar(0)
            .map_err(|e| HeldBar0Error::Bar0MapFailed {
                bdf: bdf.to_string(),
                source: e,
            })
    })
}

/// Map BAR0 and sample the device's DMA backend without a second lock acquisition.
pub(crate) fn map_held_bar0_with_dma_backend(
    held: &Arc<RwLock<HashMap<String, HeldDevice>>>,
    bdf: &str,
) -> Result<(MappedBar, DmaBackend), HeldBar0Error> {
    with_held_device(held, bdf, |dev| {
        let dma = dev.device.dma_backend();
        let bar0 = dev
            .device
            .map_bar(0)
            .map_err(|e| HeldBar0Error::Bar0MapFailed {
                bdf: bdf.to_string(),
                source: e,
            })?;
        Ok((bar0, dma))
    })
}
