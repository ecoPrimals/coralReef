// SPDX-License-Identifier: AGPL-3.0-only

//! Bridge between GlowPlug's software [`Ring`] and coral-driver's hardware
//! [`Sec2Queues`], providing tracked, persistent falcon conversation.
//!
//! The bridge shadows every hardware CMDQ submission with a software ring
//! entry, matches hardware MSGQ responses back to the originating ring
//! entry, and persists ring state to Ember via [`RingMeta`](coral_ember::RingMeta).
//!
//! ```text
//!  GlowPlug Ring (software)          SEC2 CMDQ/MSGQ (hardware)
//!  ─────────────────────             ───────────────────────────
//!  submit(payload) ──┐                    ┌── dmem_write + head_advance
//!                    └──► Sec2Bridge ────►│   poke_irq
//!  consume(entry)  ◄─┐                    │
//!                    └──◄ poll_msgq ◄─────┘── SEC2 response in DMEM
//! ```

use coral_driver::nv::vfio_compute::acr_boot::sec2_queue::{
    FalconId, Sec2Message, Sec2QueueError, Sec2Queues,
};
use coral_driver::vfio::device::MappedBar;

use crate::ember::EmberClient;
use crate::ring::{Ring, RingPayload};

/// A tracked CMDQ command in flight: maps hardware seq to ring entry.
#[derive(Debug)]
struct InFlightCmd {
    hw_seq: u8,
    ring_fence: u64,
}

/// Bridges GlowPlug Ring tracking to hardware SEC2 CMDQ/MSGQ.
pub struct Sec2Bridge {
    queues: Sec2Queues,
    ring: Ring,
    in_flight: Vec<InFlightCmd>,
    bdf: String,
}

impl Sec2Bridge {
    /// Create a bridge from discovered SEC2 queues.
    ///
    /// The ring is named `"sec2"` with capacity 16 (CMDQ is small).
    pub fn new(queues: Sec2Queues, bdf: impl Into<String>) -> Self {
        Self {
            queues,
            ring: Ring::new("sec2", 16),
            in_flight: Vec::new(),
            bdf: bdf.into(),
        }
    }

    /// Restore ring fence from Ember's persisted RingMeta (immortality).
    pub fn restore_from_ember(&mut self, ember: &EmberClient) {
        match ember.ring_meta_get(&self.bdf) {
            Ok(meta) => {
                for entry in &meta.rings {
                    if entry.name == "sec2" {
                        self.ring.restore_fence(entry.last_fence);
                        tracing::info!(
                            bdf = %self.bdf,
                            fence = entry.last_fence,
                            "sec2 ring fence restored from ember"
                        );
                        return;
                    }
                }
                tracing::debug!(bdf = %self.bdf, "no sec2 ring in ember RingMeta");
            }
            Err(e) => {
                tracing::debug!(bdf = %self.bdf, "ember ring_meta_get failed: {e}");
            }
        }
    }

    /// Persist current ring fence to Ember's RingMeta.
    pub fn persist_to_ember(&self, ember: &EmberClient) {
        let meta = coral_ember::RingMeta {
            mailboxes: vec![],
            rings: vec![coral_ember::RingMetaEntry {
                name: "sec2".into(),
                capacity: 16,
                last_fence: self.ring.current_fence(),
            }],
            version: 1,
        };
        if let Err(e) = ember.ring_meta_set(&self.bdf, &meta) {
            tracing::warn!(bdf = %self.bdf, "ember ring_meta_set failed: {e}");
        }
    }

    /// Send a BOOTSTRAP_FALCON command, tracked through the software ring.
    ///
    /// Returns the ring fence value for correlation.
    pub fn bootstrap_falcon(
        &mut self,
        bar0: &MappedBar,
        falcon_id: FalconId,
    ) -> Result<u64, Sec2QueueError> {
        let method = match falcon_id {
            FalconId::Fecs => "sec2.bootstrap_falcon.fecs",
            FalconId::Gpccs => "sec2.bootstrap_falcon.gpccs",
        };

        let hw_seq = self.queues.cmd_bootstrap_falcon(bar0, falcon_id)?;

        let payload = RingPayload {
            method: method.into(),
            data: vec![hw_seq],
        };
        let (_entry_id, fence) = self
            .ring
            .submit(payload)
            .map_err(|_| Sec2QueueError::CmdqFull)?;

        self.in_flight.push(InFlightCmd {
            hw_seq,
            ring_fence: fence,
        });

        tracing::info!(
            bdf = %self.bdf,
            falcon = ?falcon_id,
            hw_seq,
            fence,
            "bootstrap_falcon submitted (ring + hardware)"
        );

        Ok(fence)
    }

    /// Poll for hardware responses and match them to ring entries.
    ///
    /// Returns any messages received, consuming the corresponding ring entries.
    pub fn poll_responses(&mut self, bar0: &MappedBar) -> Vec<Sec2Message> {
        let mut messages = Vec::new();

        while let Some(msg) = self.queues.recv(bar0) {
            if let Some(idx) = self.in_flight.iter().position(|c| c.hw_seq == msg.seq_id) {
                let cmd = self.in_flight.remove(idx);
                let _consumed = self.ring.consume_through_fence(cmd.ring_fence);
                tracing::info!(
                    bdf = %self.bdf,
                    hw_seq = msg.seq_id,
                    fence = cmd.ring_fence,
                    unit = msg.unit_id,
                    "SEC2 response matched to ring entry"
                );
            } else {
                tracing::debug!(
                    bdf = %self.bdf,
                    hw_seq = msg.seq_id,
                    "SEC2 response with no matching ring entry (unsolicited/init)"
                );
            }
            messages.push(msg);
        }

        messages
    }

    /// Number of commands in flight (submitted but no response yet).
    pub fn in_flight_count(&self) -> usize {
        self.in_flight.len()
    }

    /// Access the underlying ring for diagnostics.
    pub fn ring(&self) -> &Ring {
        &self.ring
    }

    /// Access the underlying queues for diagnostics.
    pub fn queues(&self) -> &Sec2Queues {
        &self.queues
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use coral_driver::nv::vfio_compute::acr_boot::sec2_queue::Sec2QueueInfo;

    fn make_test_queues() -> Sec2Queues {
        Sec2Queues::from_known(Sec2QueueInfo {
            cmdq_offset: 0x1000,
            cmdq_size: 256,
            msgq_offset: 0x1100,
            msgq_size: 256,
            os_debug_entry: 0,
        })
    }

    #[test]
    fn new_creates_sec2_ring_with_capacity_16() {
        let bridge = Sec2Bridge::new(make_test_queues(), "0000:3b:00.0");
        assert_eq!(bridge.ring().name(), "sec2");
        assert_eq!(bridge.ring().available(), 16);
    }

    #[test]
    fn new_starts_with_zero_in_flight() {
        let bridge = Sec2Bridge::new(make_test_queues(), "0000:3b:00.0");
        assert_eq!(bridge.in_flight_count(), 0);
    }

    #[test]
    fn ring_stats_reflect_empty_state() {
        let bridge = Sec2Bridge::new(make_test_queues(), "test");
        assert!(bridge.ring().is_empty());
        assert_eq!(bridge.ring().pending_count(), 0);
        assert_eq!(bridge.ring().current_fence(), 0);
    }

    #[test]
    fn bdf_is_stored() {
        let bridge = Sec2Bridge::new(make_test_queues(), "0000:01:00.0");
        assert_eq!(bridge.bdf, "0000:01:00.0");
    }
}
