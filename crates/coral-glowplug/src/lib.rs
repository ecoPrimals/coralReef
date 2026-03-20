// SPDX-License-Identifier: AGPL-3.0-only
#![warn(missing_docs)]
#![deny(unsafe_code)]
//! coral-glowplug library — shared types for the sovereign PCIe device lifecycle broker.
//!
//! Re-exports [`DeviceSlot`](device::DeviceSlot), [`Personality`](personality::Personality),
//! [`DeviceError`](error::DeviceError), [`Config`](config::Config),
//! [`EmberClient`](ember::EmberClient), and sysfs helpers for consumption
//! by ecosystem crates.

pub mod config;
pub mod device;
pub mod ember;
pub mod error;
pub mod health;
pub mod pci_ids;
pub mod personality;
pub mod sysfs;
