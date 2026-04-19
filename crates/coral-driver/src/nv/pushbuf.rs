// SPDX-License-Identifier: AGPL-3.0-or-later
//! Kepler+ push buffer construction — correct Type 1/3 header encoding.
//!
//! Push buffer headers on Kepler+ encode GPU method calls:
//!
//! ```text
//! [31:29] = type   [28:16] = count   [15:13] = subchan   [12:0] = method/4
//! ```
//!
//! Type 1 = incrementing method, Type 3 = non-incrementing.
//!
//! Reference: NVK `nv_push.h` line 80 (`NVC0_FIFO_PKHDR_SQ`), confirmed
//! via NVK ioctl trace.

/// Kepler+ Type 1 (INCR) push buffer header.
///
/// Encodes `count` method writes starting at `method`, incrementing by 4
/// after each data word.
#[must_use]
pub const fn mthd_incr(subchan: u32, method: u32, count: u32) -> u32 {
    (0x1 << 29) | (count << 16) | (subchan << 13) | (method >> 2)
}

/// Kepler+ Type 3 (NINC) push buffer header — non-incrementing.
///
/// Sends `count` data words to the same method address (e.g., FIFO data).
#[must_use]
pub const fn mthd_ninc(subchan: u32, method: u32, count: u32) -> u32 {
    (0x3 << 29) | (count << 16) | (subchan << 13) | (method >> 2)
}

/// Kepler+ Type 4 (IMMD) push buffer header — single inline immediate.
///
/// Encodes a single data value directly in the header, no following data word.
#[must_use]
pub const fn mthd_immd(subchan: u32, method: u32, value: u32) -> u32 {
    (0x4 << 29) | (value << 16) | (subchan << 13) | (method >> 2)
}

/// Compute engine class constants — canonical values used across all dispatch
/// paths (nouveau, UVM, VFIO). Sourced from ioctl definitions so this module
/// compiles with any feature combination.
pub mod class {
    pub use super::super::ioctl::{
        NVIF_CLASS_AMPERE_COMPUTE_A as AMPERE_COMPUTE_A,
        NVIF_CLASS_TURING_COMPUTE_A as TURING_COMPUTE_A,
        NVIF_CLASS_VOLTA_COMPUTE_A as VOLTA_COMPUTE_A,
    };
}

/// NVIDIA compute class method registers (offsets in bytes).
///
/// Sourced from `clc3c0.h` (Volta), `clcec0.h` (Blackwell).
/// These are stable across Volta through Blackwell (SM70–SM120+).
pub mod method {
    /// Set the target object (compute class) on the selected subchannel.
    pub const SET_OBJECT: u32 = 0x0000;
    /// Invalidate shader instruction and data caches.
    pub const INVALIDATE_SHADER_CACHES: u32 = 0x021C;
    /// Set shared memory window base (upper 17 bits).
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
    /// Launch compute trigger (Ampere+): bits 3:0 = PCAS_ACTION enum.
    /// NVK uses PCAS_ACTION_INVALIDATE_COPY_SCHEDULE (0x3) for all dispatches.
    pub const SEND_SIGNALING_PCAS2_B: u32 = 0x02C0;

    /// Invalidate instruction + data caches (bits 0 + 4).
    pub const INVALIDATE_INSTR_AND_DATA: u32 = 0x11;

    /// PCAS_ACTION_INVALIDATE_COPY_SCHEDULE — invalidate PCAS state, copy QMD,
    /// and schedule the CTA grid for execution. Used with `SEND_SIGNALING_PCAS2_B`.
    pub const PCAS_ACTION_INVALIDATE_COPY_SCHEDULE: u32 = 0x3;

    /// Turing compute class threshold — classes above this use PCAS2_B.
    pub const TURING_COMPUTE_A: u32 = 0xC5C0;
}

/// Default word capacity for push buffers — sufficient for most
/// single-dispatch command sequences.
const DEFAULT_PUSHBUF_WORDS: usize = 64;

/// Push buffer builder for nouveau command submission.
///
/// Accumulates GPU method calls into a word stream suitable for
/// `DRM_NOUVEAU_GEM_PUSHBUF` submission.
pub struct PushBuf {
    words: Vec<u32>,
}

impl PushBuf {
    /// Create an empty push buffer.
    #[must_use]
    pub fn new() -> Self {
        Self {
            words: Vec::with_capacity(DEFAULT_PUSHBUF_WORDS),
        }
    }

    /// Push a single method+data pair (Type 1, count=1).
    pub fn push_1(&mut self, subchan: u32, method: u32, data: u32) {
        self.words.push(mthd_incr(subchan, method, 1));
        self.words.push(data);
    }

    /// Push a method with multiple incrementing data words.
    #[expect(
        clippy::cast_possible_truncation,
        reason = "push buffer method counts are always small (< 0x1FFF)"
    )]
    pub fn push_n(&mut self, subchan: u32, method: u32, data: &[u32]) {
        if data.is_empty() {
            return;
        }
        let count = data.len() as u32;
        self.words.push(mthd_incr(subchan, method, count));
        self.words.extend_from_slice(data);
    }

    /// Push a method with multiple non-incrementing data words.
    #[expect(
        clippy::cast_possible_truncation,
        reason = "push buffer method counts are always small (< 0x1FFF)"
    )]
    pub fn push_ninc(&mut self, subchan: u32, method: u32, data: &[u32]) {
        if data.is_empty() {
            return;
        }
        let count = data.len() as u32;
        self.words.push(mthd_ninc(subchan, method, count));
        self.words.extend_from_slice(data);
    }

    /// Append another push buffer's contents to this one.
    pub fn append(&mut self, other: &Self) {
        self.words.extend_from_slice(&other.words);
    }

    /// Emit a NOP (zero header).
    pub fn nop(&mut self) {
        self.words.push(0);
    }

    /// Total size in bytes.
    #[must_use]
    pub const fn size_bytes(&self) -> usize {
        self.words.len() * 4
    }

    /// View as raw bytes for upload.
    #[must_use]
    pub fn as_bytes(&self) -> &[u8] {
        bytemuck::cast_slice(&self.words)
    }

    /// View as raw word slice.
    #[must_use]
    pub fn as_words(&self) -> &[u32] {
        &self.words
    }

    /// Build a one-time compute init push buffer for Volta+ (SM70+).
    ///
    /// Binds the compute class to subchannel 1 via `SET_OBJECT` and configures
    /// the shared/local memory windows **and** the SLM (Shader Local Memory)
    /// base address. Must be submitted once after channel setup — NVK does
    /// this in `nvk_push_dispatch_state_init()` + `nvk_queue_state_update()`.
    ///
    /// The SLM registers (`SET_SHADER_LOCAL_MEMORY_A/B` +
    /// `SET_SHADER_LOCAL_MEMORY_NON_THROTTLED_A/B/C`) tell the SM where to
    /// put per-warp scratch/CRS memory. Without them, even an EXIT-only shader
    /// faults with `Invalid Address Space` during warp launch.
    ///
    /// Repeated `SET_OBJECT` calls on Blackwell corrupt channel state
    /// (GR_CLASS_ERROR 0x0D), so this is separated from per-dispatch work.
    #[must_use]
    pub fn compute_init(
        compute_class: u32,
        local_mem_window: u64,
        slm_base_addr: u64,
        slm_per_tpc_bytes: u64,
    ) -> Self {
        let mut pb = Self::new();
        let sub = 1_u32;

        pb.push_1(sub, method::SET_OBJECT, compute_class);

        let uses_pcas2 = compute_class > method::TURING_COMPUTE_A;
        let smem_window: u64 = if uses_pcas2 { 1u64 << 32 } else { 0xFE00_0000 };

        #[expect(
            clippy::cast_possible_truncation,
            reason = "deliberate split into 32-bit halves"
        )]
        {
            pb.push_1(
                sub,
                method::SET_SHADER_SHARED_MEMORY_WINDOW_A,
                (smem_window >> 32) as u32,
            );
            pb.push_1(
                sub,
                method::SET_SHADER_SHARED_MEMORY_WINDOW_B,
                smem_window as u32,
            );

            // SLM base address (where per-warp scratch / CRS lives).
            pb.push_1(
                sub,
                method::SET_SHADER_LOCAL_MEMORY_A,
                (slm_base_addr >> 32) as u32,
            );
            pb.push_1(
                sub,
                method::SET_SHADER_LOCAL_MEMORY_B,
                slm_base_addr as u32,
            );

            // Per-TPC SLM allocation limit — NVK sets this to
            // `bytes_per_warp * max_warps_per_sm * sms_per_tpc`.
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
                0xFF, // all SMs
            );

            // Local memory window (generic address space mapping).
            pb.push_1(
                sub,
                method::SET_SHADER_LOCAL_MEMORY_WINDOW_A,
                (local_mem_window >> 32) as u32,
            );
            pb.push_1(
                sub,
                method::SET_SHADER_LOCAL_MEMORY_WINDOW_B,
                local_mem_window as u32,
            );
        }

        pb
    }

    /// Build a per-dispatch push buffer for Volta+ (SM70+).
    ///
    /// Invalidates caches and launches via `SEND_PCAS_A` + the appropriate
    /// signaling method. The compute class must already be bound to subchannel 1
    /// via a prior [`compute_init`](Self::compute_init) submission.
    ///
    /// - `<= Turing`: `SEND_SIGNALING_PCAS_B` (0x02BC) with invalidate+schedule bits
    /// - `>  Turing`: `SEND_SIGNALING_PCAS2_B` (0x02C0) with `PCAS_ACTION_INVALIDATE_COPY_SCHEDULE`
    ///
    /// `SEND_PCAS_A` takes `qmd_addr >> 8` (QMD must be 256-byte aligned).
    #[must_use]
    pub fn compute_dispatch(compute_class: u32, qmd_addr: u64) -> Self {
        let mut pb = Self::new();
        let sub = 1_u32;

        pb.push_1(
            sub,
            method::INVALIDATE_SHADER_CACHES,
            method::INVALIDATE_INSTR_AND_DATA,
        );

        let uses_pcas2 = compute_class > method::TURING_COMPUTE_A;

        #[expect(
            clippy::cast_possible_truncation,
            reason = "deliberate split into 32-bit halves"
        )]
        {
            pb.push_1(sub, method::SEND_PCAS_A, (qmd_addr >> 8) as u32);

            if uses_pcas2 {
                pb.push_1(
                    sub,
                    method::SEND_SIGNALING_PCAS2_B,
                    method::PCAS_ACTION_INVALIDATE_COPY_SCHEDULE,
                );
            } else {
                pb.push_1(sub, method::SEND_SIGNALING_PCAS_B, 0x3);
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
        // Ecosystem validation V95: SET_OBJECT (method=0, count=1)
        // Correct: 0x20010000 = (1<<29) | (1<<16) | (0<<13) | (0>>2)
        let hdr = mthd_incr(0, 0, 1);
        assert_eq!(hdr, 0x2001_0000, "SET_OBJECT header must be 0x20010000");
    }

    #[test]
    fn mthd_incr_nop() {
        // NOP: method=0, count=0 → 0x20000000
        let hdr = mthd_incr(0, 0, 0);
        assert_eq!(hdr, 0x2000_0000);
    }

    #[test]
    fn mthd_incr_with_method() {
        // INVALIDATE_SHADER_CACHES = 0x021C
        // method>>2 = 0x87, count=1, subchan=0
        let hdr = mthd_incr(0, 0x021C, 1);
        let method_field = hdr & 0x1FFF;
        let count_field = (hdr >> 16) & 0x1FFF;
        let type_field = hdr >> 29;
        assert_eq!(type_field, 1);
        assert_eq!(count_field, 1);
        assert_eq!(method_field, 0x021C >> 2);
    }

    #[test]
    fn mthd_ninc_type_is_3() {
        let hdr = mthd_ninc(0, 0, 1);
        assert_eq!(hdr >> 29, 3);
    }

    #[test]
    fn mthd_immd_type_is_4() {
        let hdr = mthd_immd(0, 0x100, 0x42);
        assert_eq!(hdr >> 29, 4);
        let method_field = hdr & 0x1FFF;
        assert_eq!(method_field, 0x100 >> 2);
    }

    #[test]
    fn compute_init_volta_binds_class_and_windows() {
        let pb = PushBuf::compute_init(class::VOLTA_COMPUTE_A, 0xFF00_0000, 0x1_0000_0000, 0x8000);
        let words = pb.as_words();
        // SET_OBJECT(2) + SMEM_WINDOW_A/B(4) + SLM_A/B(4) +
        // NON_THROTTLED_A/B/C(6) + LOCAL_WINDOW_A/B(4) = 20
        assert_eq!(words.len(), 20);
        assert_eq!(words[0], mthd_incr(1, method::SET_OBJECT, 1));
        assert_eq!(words[1], class::VOLTA_COMPUTE_A);
    }

    #[test]
    fn compute_init_blackwell_binds_class_and_windows() {
        let pb = PushBuf::compute_init(0xCEC0, 0xFF00_0000, 0x1_0000_0000, 0x8000);
        let words = pb.as_words();
        assert_eq!(words.len(), 20);
        assert_eq!(words[0], mthd_incr(1, method::SET_OBJECT, 1));
        assert_eq!(words[1], 0xCEC0);
        // Shared memory window for Blackwell: 1<<32 → A=1, B=0
        assert_eq!(words[3], 0x0000_0001); // SMEM_WINDOW_A upper
        assert_eq!(words[5], 0x0000_0000); // SMEM_WINDOW_B lower
    }

    #[test]
    fn pushbuf_compute_dispatch_volta_uses_pcas_b() {
        let pb = PushBuf::compute_dispatch(class::VOLTA_COMPUTE_A, 0x1_0000_0000);
        let words = pb.as_words();
        // INVALIDATE(2), SEND_PCAS_A(2), SEND_SIGNALING_PCAS_B(2) = 6
        assert_eq!(words.len(), 6);

        // No SET_OBJECT — that belongs in compute_init
        assert!(
            !words.contains(&mthd_incr(1, method::SET_OBJECT, 1)),
            "compute_dispatch must not contain SET_OBJECT"
        );

        let pcas_b_idx = words
            .iter()
            .position(|&w| w == mthd_incr(1, method::SEND_SIGNALING_PCAS_B, 1));
        assert!(pcas_b_idx.is_some(), "Volta must use SEND_SIGNALING_PCAS_B");
    }

    #[test]
    fn pushbuf_compute_dispatch_blackwell_uses_pcas2_b() {
        let pb = PushBuf::compute_dispatch(0xCEC0, 0x1_0000_0000);
        let words = pb.as_words();
        // INVALIDATE(2), SEND_PCAS_A(2), SEND_SIGNALING_PCAS2_B(2) = 6
        assert_eq!(words.len(), 6);

        // No SET_OBJECT — that belongs in compute_init
        assert!(
            !words.contains(&mthd_incr(1, method::SET_OBJECT, 1)),
            "compute_dispatch must not contain SET_OBJECT"
        );

        let pcas2_idx = words
            .iter()
            .position(|&w| w == mthd_incr(1, method::SEND_SIGNALING_PCAS2_B, 1));
        assert!(
            pcas2_idx.is_some(),
            "Blackwell must use SEND_SIGNALING_PCAS2_B"
        );
        assert_eq!(
            words[pcas2_idx.unwrap() + 1],
            method::PCAS_ACTION_INVALIDATE_COPY_SCHEDULE
        );
    }

    #[test]
    fn pushbuf_as_bytes_length() {
        let mut pb = PushBuf::new();
        pb.push_1(0, 0, 0);
        assert_eq!(pb.size_bytes(), 8);
        assert_eq!(pb.as_bytes().len(), 8);
    }

    #[test]
    fn pushbuf_push_n_multiple() {
        let mut pb = PushBuf::new();
        pb.push_n(0, 0x100, &[0xA, 0xB, 0xC]);
        let words = pb.as_words();
        assert_eq!(words.len(), 4); // 1 header + 3 data
        let count_field = (words[0] >> 16) & 0x1FFF;
        assert_eq!(count_field, 3);
    }

    #[test]
    fn pushbuf_push_n_empty_noop() {
        let mut pb = PushBuf::new();
        pb.push_n(0, 0x100, &[]);
        assert!(pb.as_words().is_empty());
    }

    #[test]
    fn pushbuf_push_ninc_empty_noop() {
        let mut pb = PushBuf::new();
        pb.push_ninc(0, 0x100, &[]);
        assert!(pb.as_words().is_empty());
    }

    #[test]
    fn pushbuf_nop_increments_size() {
        let mut pb = PushBuf::new();
        assert_eq!(pb.size_bytes(), 0);
        pb.nop();
        assert_eq!(pb.size_bytes(), 4);
        pb.nop();
        assert_eq!(pb.size_bytes(), 8);
    }

    #[test]
    fn pushbuf_push_ninc_emits_type3() {
        let mut pb = PushBuf::new();
        pb.push_ninc(1, 0x200, &[0xDEAD_BEEF]);
        let words = pb.as_words();
        assert_eq!(words.len(), 2);
        assert_eq!(words[0] >> 29, 3);
        assert_eq!(words[1], 0xDEAD_BEEF);
    }

    #[test]
    #[expect(
        clippy::cast_possible_truncation,
        reason = "deliberate truncation test for 32-bit halves"
    )]
    fn pushbuf_compute_dispatch_qmd_addr_shifted8() {
        let qmd_addr = 0x0000_0001_0000_1000_u64;
        let pb = PushBuf::compute_dispatch(class::VOLTA_COMPUTE_A, qmd_addr);
        let words = pb.as_words();
        let send_pcas_a_idx = words
            .iter()
            .position(|&w| w == mthd_incr(1, method::SEND_PCAS_A, 1))
            .expect("compute_dispatch must emit SEND_PCAS_A");
        assert_eq!(words[send_pcas_a_idx + 1], (qmd_addr >> 8) as u32);
    }

    #[test]
    fn gr_context_init_structure() {
        let methods = vec![(0x0100_u32, 0xAAAA_u32), (0x0200, 0xBBBB), (0x0300, 0xCCCC)];
        let pb = PushBuf::gr_context_init(class::VOLTA_COMPUTE_A, &methods);
        let words = pb.as_words();

        // SET_OBJECT header + data, then 3 * (method header + data) = 8 words
        assert_eq!(words.len(), 8);

        // First pair: SET_OBJECT
        assert_eq!(words[0], mthd_incr(0, method::SET_OBJECT, 1));
        assert_eq!(words[1], class::VOLTA_COMPUTE_A);

        // Method entries submitted as individual push_1 calls
        assert_eq!(words[3], 0xAAAA);
        assert_eq!(words[5], 0xBBBB);
        assert_eq!(words[7], 0xCCCC);
    }

    #[test]
    fn gr_context_init_empty_methods() {
        let pb = PushBuf::gr_context_init(class::VOLTA_COMPUTE_A, &[]);
        let words = pb.as_words();
        // Just SET_OBJECT: header + data = 2 words
        assert_eq!(words.len(), 2);
    }

    #[test]
    fn mthd_immd_value_encoding() {
        // Value must fit in 13 bits (bits 28:16) to avoid overwriting type field
        let hdr = mthd_immd(0, 0x200, 0x42);
        assert_eq!(hdr >> 29, 4);
        assert_eq!((hdr >> 16) & 0x1FFF, 0x42);
        assert_eq!(hdr & 0x1FFF, 0x200 >> 2);
    }

    #[test]
    fn pushbuf_default() {
        let pb = PushBuf::default();
        assert!(pb.as_words().is_empty());
        assert_eq!(pb.size_bytes(), 0);
    }

    #[test]
    fn pushbuf_new_empty() {
        let pb = PushBuf::new();
        assert!(pb.as_words().is_empty());
    }

    #[test]
    fn mthd_ninc_subchan_encoding() {
        let hdr = mthd_ninc(2, 0x100, 1);
        let subchan = (hdr >> 13) & 0x7;
        assert_eq!(subchan, 2);
    }

    #[test]
    fn mthd_incr_count_encoding() {
        let hdr = mthd_incr(0, 0x400, 5);
        let count = (hdr >> 16) & 0x1FFF;
        assert_eq!(count, 5);
    }

    #[test]
    fn mthd_incr_method_shifted_right_by_2() {
        // Method 0x0100 -> method field = 0x40 (0x100 >> 2)
        let hdr = mthd_incr(0, 0x0100, 1);
        assert_eq!(hdr & 0x1FFF, 0x40);
    }

    #[test]
    fn pushbuf_push_ninc_multiple_words() {
        let mut pb = PushBuf::new();
        pb.push_ninc(0, 0x200, &[0x1111, 0x2222, 0x3333]);
        let words = pb.as_words();
        assert_eq!(words.len(), 4);
        assert_eq!(words[0] >> 29, 3);
        assert_eq!((words[0] >> 16) & 0x1FFF, 3);
        assert_eq!(words[1], 0x1111);
        assert_eq!(words[2], 0x2222);
        assert_eq!(words[3], 0x3333);
    }

    #[test]
    fn pushbuf_as_bytes_little_endian() {
        let mut pb = PushBuf::new();
        pb.push_1(0, 0, 0x1234_5678);
        let bytes = pb.as_bytes();
        assert_eq!(bytes.len(), 8);
        assert_eq!(bytes[4], 0x78);
        assert_eq!(bytes[5], 0x56);
        assert_eq!(bytes[6], 0x34);
        assert_eq!(bytes[7], 0x12);
    }

    #[test]
    fn pushbuf_gr_context_init_method_addrs_encoded() {
        let methods = [(0x0004, 0x11), (0x0100, 0x22)]; // 0x0004 and 0x0100 fit in 13 bits
        let pb = PushBuf::gr_context_init(class::VOLTA_COMPUTE_A, &methods);
        let words = pb.as_words();
        assert_eq!(words.len(), 6); // SET_OBJECT + 2 method pairs
        assert_eq!(words[2], mthd_incr(0, 0x0004, 1));
        assert_eq!(words[3], 0x11);
        assert_eq!(words[4], mthd_incr(0, 0x0100, 1));
        assert_eq!(words[5], 0x22);
    }
}
