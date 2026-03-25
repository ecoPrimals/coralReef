// SPDX-License-Identifier: AGPL-3.0-only
//! Posted-command mailbox for GPU firmware interaction.
//!
//! Models the mailbox pattern used by GPU firmware (FECS, GPCCS, SEC2):
//! a producer posts a command, the consumer (firmware/hardware) processes it,
//! and the producer polls for completion. This is the primary mechanism
//! hotSpring uses to crack GPU firmware behavior.
//!
//! # Architecture
//!
//! ```text
//! Producer (primal)           Mailbox             Consumer (GPU/firmware)
//!   post(cmd) ──────────► [slot: Posted] ──────►  picks up command
//!   poll(seq) ◄─────────  [slot: Complete(val)]   writes result
//! ```
//!
//! # Multi-mailbox
//!
//! Each GPU engine (FECS, GPCCS, SEC2, PMU) can have its own [`Mailbox`].
//! Use [`MailboxSet`] to manage a fleet of named mailboxes for a device.

use std::collections::VecDeque;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

/// Global monotonic sequence counter shared across all mailboxes.
static GLOBAL_SEQ: AtomicU64 = AtomicU64::new(1);

/// Opaque sequence number returned by [`Mailbox::post`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Sequence(u64);

impl Sequence {
    /// Reconstruct a sequence from a raw wire value (e.g. from JSON-RPC params).
    #[must_use]
    pub fn from_raw(raw: u64) -> Self {
        Self(raw)
    }

    /// Returns the raw numeric value (for serialization / wire format).
    #[must_use]
    pub fn raw(self) -> u64 {
        self.0
    }
}

impl std::fmt::Display for Sequence {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "seq:{}", self.0)
    }
}

/// A command posted to a mailbox.
#[derive(Debug, Clone)]
pub struct PostedCommand {
    /// BAR0 register offset to write the command word.
    pub register: u32,
    /// Command word value.
    pub command: u32,
    /// BAR0 register offset to poll for completion.
    pub status_register: u32,
    /// Expected value (masked) that indicates completion.
    pub expected_status: u32,
    /// Mask applied to the status register read before comparison.
    pub status_mask: u32,
    /// Maximum time to wait for completion before declaring timeout.
    pub timeout: Duration,
}

/// Lifecycle state of a posted command.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SlotState {
    /// Command posted, not yet picked up by consumer.
    Posted,
    /// Consumer acknowledged the command, processing in flight.
    InFlight,
    /// Command completed successfully with the status register value.
    Complete(u32),
    /// Command did not complete within the deadline.
    TimedOut,
    /// Command failed with an error description.
    Failed(String),
}

/// A single mailbox slot tracking one posted command.
#[derive(Debug)]
struct Slot {
    seq: Sequence,
    command: PostedCommand,
    state: SlotState,
    posted_at: Instant,
    completed_at: Option<Instant>,
}

impl Slot {
    /// Wall-clock latency from post to completion (or to now if still pending).
    fn latency(&self) -> Duration {
        self.completed_at
            .unwrap_or_else(Instant::now)
            .duration_since(self.posted_at)
    }
}

/// Result of polling a mailbox slot.
#[derive(Debug, Clone)]
pub struct PollResult {
    /// Sequence of the polled command.
    pub seq: Sequence,
    /// Current state.
    pub state: SlotState,
    /// Elapsed time since the command was posted.
    pub elapsed: Duration,
}

/// Completion record for a drained (finished) command.
#[derive(Debug, Clone)]
pub struct Completion {
    /// Sequence of the completed command.
    pub seq: Sequence,
    /// Terminal state (Complete, TimedOut, or Failed).
    pub state: SlotState,
    /// Time from post to terminal state.
    pub latency: Duration,
    /// The original command that was posted.
    pub command: PostedCommand,
}

/// Posted-command mailbox for a single GPU engine.
///
/// Not `Send`/`Sync` by design — each mailbox is owned by the device
/// slot that manages the corresponding engine. Cross-thread access
/// goes through the [`DeviceSlot`](crate::device::DeviceSlot) lock.
#[derive(Debug)]
pub struct Mailbox {
    name: String,
    slots: VecDeque<Slot>,
    capacity: usize,
    total_posted: u64,
    total_completed: u64,
    total_timed_out: u64,
    total_failed: u64,
}

/// Errors from mailbox operations.
#[derive(Debug, thiserror::Error)]
pub enum MailboxError {
    /// The mailbox is at capacity; drain completed entries first.
    #[error("mailbox '{name}' is full ({capacity} slots)")]
    Full {
        /// Engine name.
        name: String,
        /// Maximum slot count.
        capacity: usize,
    },

    /// No slot with the given sequence exists.
    #[error("sequence {seq} not found in mailbox '{name}'")]
    NotFound {
        /// Engine name.
        name: String,
        /// The missing sequence.
        seq: Sequence,
    },
}

impl Mailbox {
    /// Create a new mailbox for the named engine (e.g. `"fecs"`, `"gpccs"`, `"sec2"`).
    #[must_use]
    pub fn new(name: impl Into<String>, capacity: usize) -> Self {
        Self {
            name: name.into(),
            slots: VecDeque::with_capacity(capacity),
            capacity,
            total_posted: 0,
            total_completed: 0,
            total_timed_out: 0,
            total_failed: 0,
        }
    }

    /// Engine name this mailbox serves.
    #[must_use]
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Number of active (non-terminal) slots.
    #[must_use]
    pub fn active_count(&self) -> usize {
        self.slots
            .iter()
            .filter(|s| matches!(s.state, SlotState::Posted | SlotState::InFlight))
            .count()
    }

    /// Post a command to the mailbox. Returns a [`Sequence`] for tracking.
    ///
    /// # Errors
    ///
    /// Returns [`MailboxError::Full`] if the mailbox is at capacity.
    pub fn post(&mut self, command: PostedCommand) -> Result<Sequence, MailboxError> {
        if self.slots.len() >= self.capacity {
            return Err(MailboxError::Full {
                name: self.name.clone(),
                capacity: self.capacity,
            });
        }

        let seq = Sequence(GLOBAL_SEQ.fetch_add(1, Ordering::Relaxed));
        self.slots.push_back(Slot {
            seq,
            command,
            state: SlotState::Posted,
            posted_at: Instant::now(),
            completed_at: None,
        });
        self.total_posted += 1;
        Ok(seq)
    }

    /// Poll a specific sequence for its current state.
    ///
    /// # Errors
    ///
    /// Returns [`MailboxError::NotFound`] if the sequence has been drained or never existed.
    pub fn poll(&self, seq: Sequence) -> Result<PollResult, MailboxError> {
        let slot =
            self.slots
                .iter()
                .find(|s| s.seq == seq)
                .ok_or_else(|| MailboxError::NotFound {
                    name: self.name.clone(),
                    seq,
                })?;

        Ok(PollResult {
            seq: slot.seq,
            state: slot.state.clone(),
            elapsed: slot.latency(),
        })
    }

    /// Mark a posted/in-flight slot as complete with the given status value.
    ///
    /// Called by the polling loop after reading the hardware status register.
    pub fn complete(&mut self, seq: Sequence, status: u32) {
        if let Some(slot) = self.slots.iter_mut().find(|s| s.seq == seq)
            && matches!(slot.state, SlotState::Posted | SlotState::InFlight)
        {
            slot.state = SlotState::Complete(status);
            slot.completed_at = Some(Instant::now());
            self.total_completed += 1;
        }
    }

    /// Mark a posted/in-flight slot as having transitioned to in-flight.
    pub fn mark_in_flight(&mut self, seq: Sequence) {
        if let Some(slot) = self.slots.iter_mut().find(|s| s.seq == seq)
            && slot.state == SlotState::Posted
        {
            slot.state = SlotState::InFlight;
        }
    }

    /// Mark a slot as timed out.
    pub fn timeout(&mut self, seq: Sequence) {
        if let Some(slot) = self.slots.iter_mut().find(|s| s.seq == seq)
            && matches!(slot.state, SlotState::Posted | SlotState::InFlight)
        {
            slot.state = SlotState::TimedOut;
            slot.completed_at = Some(Instant::now());
            self.total_timed_out += 1;
        }
    }

    /// Mark a slot as failed with a reason.
    pub fn fail(&mut self, seq: Sequence, reason: impl Into<String>) {
        if let Some(slot) = self.slots.iter_mut().find(|s| s.seq == seq)
            && matches!(slot.state, SlotState::Posted | SlotState::InFlight)
        {
            slot.state = SlotState::Failed(reason.into());
            slot.completed_at = Some(Instant::now());
            self.total_failed += 1;
        }
    }

    /// Scan all active slots and time out any that have exceeded their deadline.
    ///
    /// Returns the number of slots that were timed out.
    pub fn expire_stale(&mut self) -> usize {
        let now = Instant::now();
        let mut expired = 0;
        for slot in &mut self.slots {
            if matches!(slot.state, SlotState::Posted | SlotState::InFlight)
                && now.duration_since(slot.posted_at) > slot.command.timeout
            {
                slot.state = SlotState::TimedOut;
                slot.completed_at = Some(now);
                self.total_timed_out += 1;
                expired += 1;
            }
        }
        expired
    }

    /// Drain all terminal (Complete, TimedOut, Failed) slots from the front.
    ///
    /// Returns completed entries in FIFO order, freeing capacity for new posts.
    pub fn drain_completed(&mut self) -> Vec<Completion> {
        let mut completions = Vec::new();
        while let Some(front) = self.slots.front() {
            if matches!(
                front.state,
                SlotState::Complete(_) | SlotState::TimedOut | SlotState::Failed(_)
            ) {
                let slot = self.slots.pop_front().expect("front exists");
                let latency = slot.latency();
                completions.push(Completion {
                    seq: slot.seq,
                    state: slot.state,
                    latency,
                    command: slot.command,
                });
            } else {
                break;
            }
        }
        completions
    }

    /// Summary statistics.
    #[must_use]
    pub fn stats(&self) -> MailboxStats {
        MailboxStats {
            name: self.name.clone(),
            capacity: self.capacity,
            active: self.active_count(),
            total_posted: self.total_posted,
            total_completed: self.total_completed,
            total_timed_out: self.total_timed_out,
            total_failed: self.total_failed,
        }
    }
}

/// Snapshot of mailbox statistics for diagnostics / RPC.
#[derive(Debug, Clone, serde::Serialize)]
pub struct MailboxStats {
    /// Engine name.
    pub name: String,
    /// Maximum concurrent slots.
    pub capacity: usize,
    /// Currently active (Posted or InFlight) slots.
    pub active: usize,
    /// Total commands ever posted.
    pub total_posted: u64,
    /// Total commands completed successfully.
    pub total_completed: u64,
    /// Total commands that timed out.
    pub total_timed_out: u64,
    /// Total commands that failed.
    pub total_failed: u64,
}

/// A fleet of named mailboxes for a single device.
///
/// Typical usage: one mailbox per GPU engine (FECS, GPCCS, SEC2, PMU).
#[derive(Debug)]
pub struct MailboxSet {
    mailboxes: Vec<Mailbox>,
}

impl MailboxSet {
    /// Create an empty set.
    #[must_use]
    pub fn new() -> Self {
        Self {
            mailboxes: Vec::new(),
        }
    }

    /// Add a mailbox for the named engine.
    pub fn add(&mut self, mailbox: Mailbox) {
        self.mailboxes.push(mailbox);
    }

    /// Look up a mailbox by engine name.
    #[must_use]
    pub fn get(&self, name: &str) -> Option<&Mailbox> {
        self.mailboxes.iter().find(|m| m.name == name)
    }

    /// Look up a mailbox by engine name (mutable).
    pub fn get_mut(&mut self, name: &str) -> Option<&mut Mailbox> {
        self.mailboxes.iter_mut().find(|m| m.name == name)
    }

    /// Expire stale commands across all mailboxes. Returns total expired.
    pub fn expire_all_stale(&mut self) -> usize {
        self.mailboxes.iter_mut().map(Mailbox::expire_stale).sum()
    }

    /// Statistics for all mailboxes.
    #[must_use]
    pub fn all_stats(&self) -> Vec<MailboxStats> {
        self.mailboxes.iter().map(Mailbox::stats).collect()
    }

    /// Iterator over all mailboxes.
    pub fn iter(&self) -> impl Iterator<Item = &Mailbox> {
        self.mailboxes.iter()
    }

    /// Mutable iterator over all mailboxes.
    pub fn iter_mut(&mut self) -> impl Iterator<Item = &mut Mailbox> {
        self.mailboxes.iter_mut()
    }
}

impl Default for MailboxSet {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_command(timeout_ms: u64) -> PostedCommand {
        PostedCommand {
            register: 0x0040_9800,
            command: 0x0000_0001,
            status_register: 0x0040_9804,
            expected_status: 0x0000_0001,
            status_mask: 0x0000_00FF,
            timeout: Duration::from_millis(timeout_ms),
        }
    }

    #[test]
    fn post_and_poll_returns_posted_state() {
        let mut mb = Mailbox::new("fecs", 8);
        let seq = mb.post(sample_command(1000)).expect("post succeeds");
        let result = mb.poll(seq).expect("poll succeeds");
        assert_eq!(result.state, SlotState::Posted);
        assert_eq!(mb.active_count(), 1);
    }

    #[test]
    fn complete_transitions_to_complete_state() {
        let mut mb = Mailbox::new("fecs", 8);
        let seq = mb.post(sample_command(1000)).expect("post");
        mb.complete(seq, 0x01);
        let result = mb.poll(seq).expect("poll");
        assert_eq!(result.state, SlotState::Complete(0x01));
        assert_eq!(mb.active_count(), 0);
    }

    #[test]
    fn mark_in_flight_transitions_posted_to_in_flight() {
        let mut mb = Mailbox::new("gpccs", 4);
        let seq = mb.post(sample_command(1000)).expect("post");
        mb.mark_in_flight(seq);
        let result = mb.poll(seq).expect("poll");
        assert_eq!(result.state, SlotState::InFlight);
        assert_eq!(mb.active_count(), 1);
    }

    #[test]
    fn timeout_transitions_to_timed_out() {
        let mut mb = Mailbox::new("sec2", 4);
        let seq = mb.post(sample_command(1000)).expect("post");
        mb.timeout(seq);
        let result = mb.poll(seq).expect("poll");
        assert_eq!(result.state, SlotState::TimedOut);
        assert_eq!(mb.active_count(), 0);
    }

    #[test]
    fn fail_transitions_to_failed() {
        let mut mb = Mailbox::new("pmu", 4);
        let seq = mb.post(sample_command(1000)).expect("post");
        mb.fail(seq, "NACK from firmware");
        let result = mb.poll(seq).expect("poll");
        assert_eq!(result.state, SlotState::Failed("NACK from firmware".into()));
    }

    #[test]
    fn full_mailbox_rejects_post() {
        let mut mb = Mailbox::new("fecs", 2);
        mb.post(sample_command(1000)).expect("post 1");
        mb.post(sample_command(1000)).expect("post 2");
        let err = mb.post(sample_command(1000)).expect_err("full");
        assert!(matches!(err, MailboxError::Full { capacity: 2, .. }));
    }

    #[test]
    fn drain_completed_returns_fifo_order() {
        let mut mb = Mailbox::new("fecs", 8);
        let s1 = mb.post(sample_command(1000)).expect("post 1");
        let s2 = mb.post(sample_command(1000)).expect("post 2");
        let _s3 = mb.post(sample_command(1000)).expect("post 3");

        mb.complete(s1, 0x01);
        mb.complete(s2, 0x02);

        let drained = mb.drain_completed();
        assert_eq!(drained.len(), 2);
        assert_eq!(drained[0].seq, s1);
        assert_eq!(drained[1].seq, s2);
        assert_eq!(mb.active_count(), 1);
    }

    #[test]
    fn drain_stops_at_first_active_slot() {
        let mut mb = Mailbox::new("fecs", 8);
        let s1 = mb.post(sample_command(1000)).expect("post 1");
        let _s2 = mb.post(sample_command(1000)).expect("post 2 (active)");
        let s3 = mb.post(sample_command(1000)).expect("post 3");

        mb.complete(s1, 0x01);
        mb.complete(s3, 0x03);

        let drained = mb.drain_completed();
        assert_eq!(drained.len(), 1, "s2 is still active, blocking s3 drain");
        assert_eq!(drained[0].seq, s1);
    }

    #[test]
    fn expire_stale_times_out_overdue_commands() {
        let mut mb = Mailbox::new("fecs", 8);
        let seq = mb.post(sample_command(0)).expect("post with 0ms timeout");
        std::thread::sleep(Duration::from_millis(1));
        let expired = mb.expire_stale();
        assert_eq!(expired, 1);
        let result = mb.poll(seq).expect("poll");
        assert_eq!(result.state, SlotState::TimedOut);
    }

    #[test]
    fn poll_unknown_sequence_returns_not_found() {
        let mb = Mailbox::new("fecs", 8);
        let err = mb.poll(Sequence(9999)).expect_err("not found");
        assert!(matches!(err, MailboxError::NotFound { .. }));
    }

    #[test]
    fn stats_reflect_operations() {
        let mut mb = Mailbox::new("fecs", 8);
        let s1 = mb.post(sample_command(1000)).expect("post 1");
        let s2 = mb.post(sample_command(1000)).expect("post 2");
        let s3 = mb.post(sample_command(1000)).expect("post 3");

        mb.complete(s1, 0x01);
        mb.timeout(s2);
        mb.fail(s3, "error");

        let stats = mb.stats();
        assert_eq!(stats.total_posted, 3);
        assert_eq!(stats.total_completed, 1);
        assert_eq!(stats.total_timed_out, 1);
        assert_eq!(stats.total_failed, 1);
        assert_eq!(stats.active, 0);
    }

    #[test]
    fn sequence_display_format() {
        let seq = Sequence(42);
        assert_eq!(seq.to_string(), "seq:42");
        assert_eq!(seq.raw(), 42);
    }

    #[test]
    fn mailbox_set_manages_multiple_engines() {
        let mut set = MailboxSet::new();
        set.add(Mailbox::new("fecs", 4));
        set.add(Mailbox::new("gpccs", 4));
        set.add(Mailbox::new("sec2", 4));

        assert!(set.get("fecs").is_some());
        assert!(set.get("gpccs").is_some());
        assert!(set.get("sec2").is_some());
        assert!(set.get("pmu").is_none());

        let fecs = set.get_mut("fecs").expect("fecs exists");
        fecs.post(sample_command(1000)).expect("post to fecs");

        let stats = set.all_stats();
        assert_eq!(stats.len(), 3);
        assert_eq!(stats[0].total_posted, 1);
    }

    #[test]
    fn mailbox_set_expire_all_stale_across_engines() {
        let mut set = MailboxSet::new();
        set.add(Mailbox::new("fecs", 4));
        set.add(Mailbox::new("gpccs", 4));

        set.get_mut("fecs")
            .expect("fecs")
            .post(sample_command(0))
            .expect("post");
        set.get_mut("gpccs")
            .expect("gpccs")
            .post(sample_command(0))
            .expect("post");

        std::thread::sleep(Duration::from_millis(1));
        let expired = set.expire_all_stale();
        assert_eq!(expired, 2);
    }

    #[test]
    fn mailbox_set_default_is_empty() {
        let set = MailboxSet::default();
        assert_eq!(set.all_stats().len(), 0);
    }

    #[test]
    fn complete_is_idempotent_on_terminal_slots() {
        let mut mb = Mailbox::new("fecs", 4);
        let seq = mb.post(sample_command(1000)).expect("post");
        mb.complete(seq, 0x01);
        mb.complete(seq, 0x02);
        let result = mb.poll(seq).expect("poll");
        assert_eq!(
            result.state,
            SlotState::Complete(0x01),
            "second complete must not overwrite"
        );
        assert_eq!(mb.stats().total_completed, 1);
    }

    #[test]
    fn fail_is_idempotent_on_terminal_slots() {
        let mut mb = Mailbox::new("fecs", 4);
        let seq = mb.post(sample_command(1000)).expect("post");
        mb.fail(seq, "first error");
        mb.fail(seq, "second error");
        let result = mb.poll(seq).expect("poll");
        assert_eq!(result.state, SlotState::Failed("first error".into()));
        assert_eq!(mb.stats().total_failed, 1);
    }

    #[test]
    fn drain_frees_capacity_for_new_posts() {
        let mut mb = Mailbox::new("fecs", 2);
        let s1 = mb.post(sample_command(1000)).expect("post 1");
        let s2 = mb.post(sample_command(1000)).expect("post 2");
        assert!(mb.post(sample_command(1000)).is_err(), "full");

        mb.complete(s1, 0x01);
        mb.complete(s2, 0x02);
        mb.drain_completed();

        mb.post(sample_command(1000))
            .expect("capacity freed after drain");
    }

    #[test]
    fn completion_latency_is_positive() {
        let mut mb = Mailbox::new("fecs", 4);
        let seq = mb.post(sample_command(1000)).expect("post");
        std::thread::sleep(Duration::from_millis(1));
        mb.complete(seq, 0x01);
        let drained = mb.drain_completed();
        assert_eq!(drained.len(), 1);
        assert!(
            drained[0].latency >= Duration::from_millis(1),
            "latency should reflect wall-clock time"
        );
    }
}
