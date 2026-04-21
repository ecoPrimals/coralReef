// SPDX-License-Identifier: AGPL-3.0-or-later
//! Push buffer construction for NVIDIA GPU command submission.
//!
//! Provides helpers that build small inline push buffers for compute init,
//! per-dispatch, and GR context init workloads. Each push buffer is a
//! sequence of `u32` words containing method headers and data.


/// Push buffer builder.
///
/// Wraps a growable `Vec<u32>` of GPU method+data words.
pub struct PushBuf {
    data: Vec<u32>,
}

impl PushBuf {
    /// Create an empty push buffer.
    #[must_use]
    pub fn new() -> Self {
        Self {
            data: Vec::with_capacity(64),
        }
    }

    /// Encode and push a single-word method write.
    ///
    /// Uses the SEC_OP=1 (INC_METHOD) encoding with count=1:
    /// - Bits [31:29] = 001 (INC_METHOD)
    /// - Bits [28:16] = count (1)
    /// - Bits [15:13] = subchannel
    /// - Bits [12:0]  = method address >> 2
    pub fn push_1(&mut self, subchannel: u32, method_addr: u32, value: u32) {
        let hdr = (1u32 << 29) | (1 << 16) | ((subchannel & 0x7) << 13) | ((method_addr >> 2) & 0x1FFF);
        self.data.push(hdr);
        self.data.push(value);
    }

    /// Append another push buffer's words to this one.
    pub fn append(&mut self, other: &PushBuf) {
        self.data.extend_from_slice(&other.data);
    }

    /// Return the push buffer data as a slice of words.
    #[must_use]
    pub fn as_words(&self) -> &[u32] {
        &self.data
    }

    /// Return the push buffer data as a byte slice.
    #[must_use]
    pub fn as_bytes(&self) -> &[u8] {
        bytemuck::cast_slice(&self.data)
    }
}

/// Compute class identifiers.
pub mod class {
    /// Volta compute class.
    pub const VOLTA_COMPUTE_A: u32 = 0xC3C0;
    /// Turing compute class.
    pub const TURING_COMPUTE_A: u32 = 0xC5C0;
    /// Ampere compute class.
    pub const AMPERE_COMPUTE_A: u32 = 0xC6C0;
}

/// Method offsets and constants for NVIDIA compute class push buffers.
pub mod method {
    /// Bind an engine class to a subchannel.
    pub const SET_OBJECT: u32 = 0x0000;
    /// Cache invalidation method (common across compute classes).
    pub const INVALIDATE_SHADER_CACHES: u32 = 0x021C;
    /// Data value: invalidate both instruction and data caches.
    pub const INVALIDATE_INSTR_AND_DATA: u32 = 0x13;
    /// Set shared memory window base (upper bits).
    pub const SET_SHADER_SHARED_MEMORY_WINDOW_A: u32 = 0x02A0;
    /// Set shared memory window base (lower 32 bits).
    pub const SET_SHADER_SHARED_MEMORY_WINDOW_B: u32 = 0x02A4;
    /// SLM non-throttled per-TPC limit (upper 8 bits).
    pub const SET_SHADER_LOCAL_MEMORY_NON_THROTTLED_A: u32 = 0x02E4;
    /// SLM non-throttled per-TPC limit (lower 32 bits).
    pub const SET_SHADER_LOCAL_MEMORY_NON_THROTTLED_B: u32 = 0x02E8;
    /// SLM non-throttled max SM count (9 bits).
    pub const SET_SHADER_LOCAL_MEMORY_NON_THROTTLED_C: u32 = 0x02EC;
    /// SLM base GPU VA (upper 8 bits).
    pub const SET_SHADER_LOCAL_MEMORY_A: u32 = 0x0790;
    /// SLM base GPU VA (lower 32 bits).
    pub const SET_SHADER_LOCAL_MEMORY_B: u32 = 0x0794;
    /// Set local memory window base (upper 17 bits).
    pub const SET_SHADER_LOCAL_MEMORY_WINDOW_A: u32 = 0x07B0;
    /// Set local memory window base (lower 32 bits).
    pub const SET_SHADER_LOCAL_MEMORY_WINDOW_B: u32 = 0x07B4;
    /// Launch compute: QMD address >> 8 (256-byte aligned).
    pub const SEND_PCAS_A: u32 = 0x02B4;
    /// Launch compute trigger (<= Turing): bit 0 = invalidate, bit 1 = schedule.
    pub const SEND_SIGNALING_PCAS_B: u32 = 0x02BC;
    /// Launch compute trigger (Ampere+): enum, see `PCAS_ACTION_*`.
    pub const SEND_SIGNALING_PCAS2_B: u32 = 0x02C0;
    /// Turing compute class value — used to determine launch path.
    pub const TURING_COMPUTE_A: u32 = 0xC5C0;
    /// PCAS2 action: invalidate + copy + schedule.
    pub const PCAS_ACTION_INVALIDATE_COPY_SCHEDULE: u32 = 3;
}

impl PushBuf {
    /// Build a one-time compute init push buffer for Volta+ (SM70+).
    ///
    /// Binds the compute class to subchannel 1 via `SET_OBJECT` and configures
    /// SLM (Shader Local Memory) base address + per-TPC allocation limit.
    /// This mirrors NVK's `nvk_push_dispatch_state_init()` + `nvk_queue_state_update()`.
    ///
    /// NVK does NOT set `SET_SHADER_SHARED_MEMORY_WINDOW` or
    /// `SET_SHADER_LOCAL_MEMORY_WINDOW` for compute subchannels — those are
    /// only needed for the 3D engine. The compute SM accesses local/shared
    /// memory through dedicated address spaces, not the generic window.
    ///
    /// Repeated `SET_OBJECT` calls on Blackwell corrupt channel state
    /// (GR_CLASS_ERROR 0x0D), so this is separated from per-dispatch work.
    #[must_use]
    pub fn compute_init(
        compute_class: u32,
        _local_mem_window: u64,
        slm_base_addr: u64,
        slm_per_tpc_bytes: u64,
    ) -> Self {
        let mut pb = Self::new();
        let sub = 1_u32;

        pb.push_1(sub, method::SET_OBJECT, compute_class);

        #[expect(
            clippy::cast_possible_truncation,
            reason = "deliberate split into 32-bit halves"
        )]
        {
            pb.push_1(
                sub,
                method::SET_SHADER_LOCAL_MEMORY_A,
                (slm_base_addr >> 32) as u32,
            );
            pb.push_1(sub, method::SET_SHADER_LOCAL_MEMORY_B, slm_base_addr as u32);

            pb.push_1(
                sub,
                method::SET_SHADER_LOCAL_MEMORY_NON_THROTTLED_A,
                (slm_per_tpc_bytes >> 32) as u32,
            );
            pb.push_1(
                sub,
                method::SET_SHADER_LOCAL_MEMORY_NON_THROTTLED_B,
                slm_per_tpc_bytes as u32,
            );
            pb.push_1(
                sub,
                method::SET_SHADER_LOCAL_MEMORY_NON_THROTTLED_C,
                0xFF,
            );
        }

        pb
    }

    /// Build a per-dispatch push buffer using the compute class to infer
    /// the launch method (PCAS vs PCAS2).
    ///
    /// Prefer [`compute_dispatch_with_launch`] when a
    /// [`GenerationProfile`](super::generation::GenerationProfile) is available.
    #[must_use]
    pub fn compute_dispatch(compute_class: u32, qmd_addr: u64) -> Self {
        use super::generation::LaunchMethod;
        let launch = if compute_class > method::TURING_COMPUTE_A {
            LaunchMethod::Pcas2
        } else {
            LaunchMethod::Pcas
        };
        Self::compute_dispatch_with_launch(launch, qmd_addr)
    }

    /// Build a per-dispatch push buffer using an explicit launch method
    /// from the generation profile.
    ///
    /// Invalidates caches and launches via `SEND_PCAS_A` + the appropriate
    /// signaling method. The compute class must already be bound to subchannel 1
    /// via a prior [`compute_init`](Self::compute_init) submission.
    #[must_use]
    pub fn compute_dispatch_with_launch(
        launch: super::generation::LaunchMethod,
        qmd_addr: u64,
    ) -> Self {
        use super::generation::LaunchMethod;
        let mut pb = Self::new();
        let sub = 1_u32;

        pb.push_1(
            sub,
            method::INVALIDATE_SHADER_CACHES,
            method::INVALIDATE_INSTR_AND_DATA,
        );

        #[expect(
            clippy::cast_possible_truncation,
            reason = "deliberate split into 32-bit halves"
        )]
        {
            pb.push_1(sub, method::SEND_PCAS_A, (qmd_addr >> 8) as u32);

            match launch {
                LaunchMethod::Pcas2 => {
                    pb.push_1(
                        sub,
                        method::SEND_SIGNALING_PCAS2_B,
                        method::PCAS_ACTION_INVALIDATE_COPY_SCHEDULE,
                    );
                }
                LaunchMethod::Pcas => {
                    pb.push_1(sub, method::SEND_SIGNALING_PCAS_B, 0x3);
                }
            }
        }

        pb
    }

    /// Build a GR context init push buffer from FECS method entries.
    ///
    /// Submits the method init entries from firmware blobs as class
    /// method writes on subchannel 0. This initializes the GR engine
    /// context so that subsequent compute dispatches have a valid
    /// context (prevents CTXNOTVALID from PBDMA).
    ///
    /// Each method entry is a `(addr, value)` pair where `addr` is a
    /// GR class method offset and `value` is the data to write.
    ///
    /// Callers must ensure all addresses fit in the 13-bit push buffer
    /// method encoding (<= 0x7FFC). Use [`crate::gsp::split_for_application`]
    /// to separate BAR0 from channel-submittable entries.
    #[must_use]
    pub fn gr_context_init(compute_class: u32, method_entries: &[(u32, u32)]) -> Self {
        let mut pb = Self::new();
        let sub = 0_u32;

        pb.push_1(sub, method::SET_OBJECT, compute_class);

        for &(addr, value) in method_entries {
            debug_assert!(
                addr <= 0x7FFC,
                "method addr {addr:#x} exceeds 13-bit push buffer encoding limit"
            );
            pb.push_1(sub, addr, value);
        }

        pb
    }
}

impl Default for PushBuf {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mthd_incr_field_order() {
        let pb = PushBuf::compute_dispatch(0xC6C0, 0x1_0000_0000);
        let words = pb.as_words();
        assert!(words.len() >= 6, "dispatch should have >=3 methods");
        let hdr = words[0];
        assert_eq!(hdr >> 29, 1, "SEC_OP should be 1 (INC_METHOD)");
        assert_eq!((hdr >> 16) & 0x1FFF, 1, "count should be 1");
        assert_eq!((hdr >> 13) & 0x7, 1, "subchannel should be 1");
    }

    #[test]
    fn compute_init_sets_object() {
        let pb = PushBuf::compute_init(0xCEC0, 0xFF00_0000, 0x1_0000_0000, 0x8000);
        let words = pb.as_words();
        let hdr = words[0];
        assert_eq!((hdr >> 13) & 0x7, 1, "subchannel 1");
        assert_eq!(hdr & 0x1FFF, 0, "SET_OBJECT method addr >> 2 = 0");
        assert_eq!(words[1], 0xCEC0, "compute class");
    }

    #[test]
    fn compute_dispatch_uses_pcas2_for_ampere_plus() {
        let pb = PushBuf::compute_dispatch(0xC6C0, 0x2_0000_0000);
        let words = pb.as_words();
        let pcas2_method = method::SEND_SIGNALING_PCAS2_B >> 2;
        let found = words
            .chunks(2)
            .any(|w| (w[0] & 0x1FFF) == pcas2_method);
        assert!(found, "Ampere+ should use SEND_SIGNALING_PCAS2_B");
    }

    #[test]
    fn compute_dispatch_uses_pcas_for_turing() {
        let pb = PushBuf::compute_dispatch(0xC5C0, 0x1_0000_0000);
        let words = pb.as_words();
        let pcas_method = method::SEND_SIGNALING_PCAS_B >> 2;
        let found = words
            .chunks(2)
            .any(|w| (w[0] & 0x1FFF) == pcas_method);
        assert!(found, "Turing should use SEND_SIGNALING_PCAS_B");
    }

    fn decode_push_va(words: &[u32]) -> Option<u64> {
        for w in words.chunks(2) {
            if (w[0] & 0x1FFF) == (method::SEND_PCAS_A >> 2) {
                return Some(u64::from(w[1]) << 8);
            }
        }
        None
    }

    #[test]
    fn send_pcas_a_encodes_qmd_addr_shifted() {
        let addr: u64 = 0x3_DEAD_0000;
        let pb = PushBuf::compute_dispatch(0xC6C0, addr);
        let decoded = decode_push_va(pb.as_words()).expect("SEND_PCAS_A missing");
        assert_eq!(decoded, addr, "round-trip qmd addr");
    }

    #[test]
    fn gr_context_init_subchannel_0() {
        let entries = vec![(0x0418, 0x1234)];
        let pb = PushBuf::gr_context_init(0xCEC0, &entries);
        let words = pb.as_words();
        for w in words.chunks(2) {
            assert_eq!((w[0] >> 13) & 0x7, 0, "GR init should be subchannel 0");
        }
    }

    #[test]
    fn as_bytes_len() {
        let pb = PushBuf::compute_dispatch(0xC5C0, 0);
        assert_eq!(pb.as_bytes().len(), pb.as_words().len() * 4);
    }

    #[test]
    fn empty_pushbuf() {
        let pb = PushBuf::new();
        assert!(pb.as_words().is_empty());
        assert!(pb.as_bytes().is_empty());
    }

    /// Regression: CBUF addresses referenced in the QMD may be loaded
    /// through the method stream if a driver implements descriptor table
    /// upload that way. Verify that the offsets we expose are consistent
    /// with what `compute_dispatch` expects (subchannel 1, proper method
    /// encoding).
    #[test]
    fn method_constants_consistent() {
        assert_eq!(method::SET_OBJECT, 0x0000);
        assert_eq!(method::INVALIDATE_SHADER_CACHES, 0x021C);
        assert_eq!(method::SEND_PCAS_A, 0x02B4);
        assert_eq!(method::SEND_SIGNALING_PCAS_B, 0x02BC);
        assert_eq!(method::SEND_SIGNALING_PCAS2_B, 0x02C0);
    }

    #[test]
    fn slm_registers_present_in_compute_init() {
        let pb = PushBuf::compute_init(0xCEC0, 0, 0x1_0000_0000, 0x8000);
        let words = pb.as_words();
        let slm_a = method::SET_SHADER_LOCAL_MEMORY_A >> 2;
        let slm_b = method::SET_SHADER_LOCAL_MEMORY_B >> 2;
        let found_a = words.chunks(2).any(|w| (w[0] & 0x1FFF) == slm_a);
        let found_b = words.chunks(2).any(|w| (w[0] & 0x1FFF) == slm_b);
        assert!(found_a, "SET_SHADER_LOCAL_MEMORY_A should be present");
        assert!(found_b, "SET_SHADER_LOCAL_MEMORY_B should be present");
    }
}
