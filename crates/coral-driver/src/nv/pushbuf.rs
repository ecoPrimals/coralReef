// SPDX-License-Identifier: AGPL-3.0-only
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

/// Re-exported compute class constants from the canonical UVM definitions.
pub mod class {
    pub use super::super::uvm::{AMPERE_COMPUTE_A, TURING_COMPUTE_A, VOLTA_COMPUTE_A};
}

/// NVIDIA compute class method registers (offsets in bytes).
pub mod method {
    /// Set the target object (compute class).
    pub const SET_OBJECT: u32 = 0x0000;
    /// Invalidate instruction and data caches.
    pub const INVALIDATE_SHADER_CACHES: u32 = 0x0088;
    /// Set shared memory window (upper 32 bits).
    pub const SET_SHADER_LOCAL_MEMORY_WINDOW_A: u32 = 0x077C;
    /// Set shared memory window (lower 32 bits).
    pub const SET_SHADER_LOCAL_MEMORY_WINDOW_B: u32 = 0x0780;
    /// Launch compute: QMD address (upper 32 bits).
    pub const SEND_PCAS_A: u32 = 0x0D00;
    /// Launch compute: QMD address (lower 32 bits).
    pub const SEND_SIGNALING_PCAS_B: u32 = 0x0D04;
}

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
            words: Vec::with_capacity(64),
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

    /// Build a compute dispatch push buffer for Volta+ (SM70+).
    ///
    /// Sets up the compute class, invalidates caches, and launches
    /// via `SEND_PCAS_A`/`B` with the QMD address.
    #[must_use]
    pub fn compute_dispatch(compute_class: u32, qmd_addr: u64, local_mem_window: u64) -> Self {
        let mut pb = Self::new();
        let sub = 0_u32;

        pb.push_1(sub, method::SET_OBJECT, compute_class);

        pb.push_1(
            sub,
            method::INVALIDATE_SHADER_CACHES,
            0x11, // instruction + data caches
        );

        #[expect(
            clippy::cast_possible_truncation,
            reason = "deliberate split into 32-bit halves"
        )]
        {
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

            pb.push_1(sub, method::SEND_PCAS_A, (qmd_addr >> 32) as u32);
            pb.push_1(sub, method::SEND_SIGNALING_PCAS_B, qmd_addr as u32);
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
    #[must_use]
    pub fn gr_context_init(compute_class: u32, method_entries: &[(u32, u32)]) -> Self {
        let mut pb = Self::new();
        let sub = 0_u32;

        pb.push_1(sub, method::SET_OBJECT, compute_class);

        for &(addr, value) in method_entries {
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
        // groundSpring V95: SET_OBJECT (method=0, count=1)
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
        // INVALIDATE_SHADER_CACHES = 0x0088
        // method>>2 = 0x22, count=1, subchan=0
        let hdr = mthd_incr(0, 0x0088, 1);
        let method_field = hdr & 0x1FFF;
        let count_field = (hdr >> 16) & 0x1FFF;
        let type_field = hdr >> 29;
        assert_eq!(type_field, 1);
        assert_eq!(count_field, 1);
        assert_eq!(method_field, 0x0088 >> 2);
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
    fn pushbuf_compute_dispatch_structure() {
        let pb = PushBuf::compute_dispatch(class::VOLTA_COMPUTE_A, 0x1_0000_0000, 0xFF00_0000);
        let words = pb.as_words();
        // Should have: SET_OBJECT(hdr,data), INVALIDATE(hdr,data),
        // LOCAL_MEM_A(hdr,data), LOCAL_MEM_B(hdr,data),
        // SEND_PCAS_A(hdr,data), SEND_PCAS_B(hdr,data) = 12 words
        assert_eq!(words.len(), 12);

        // First pair: SET_OBJECT
        assert_eq!(words[0], mthd_incr(0, method::SET_OBJECT, 1));
        assert_eq!(words[1], class::VOLTA_COMPUTE_A);
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
    #[expect(
        clippy::similar_names,
        reason = "pcas_a and pcas_b are the actual GPU register names"
    )]
    fn pushbuf_compute_dispatch_qmd_addr_split() {
        let qmd_addr = 0x80_1234_5678_9ABC_u64;
        let pb = PushBuf::compute_dispatch(class::VOLTA_COMPUTE_A, qmd_addr, 0xFF00_0000);
        let words = pb.as_words();
        let send_pcas_a_idx = words
            .iter()
            .position(|&w| w == mthd_incr(0, method::SEND_PCAS_A, 1))
            .unwrap();
        assert_eq!(words[send_pcas_a_idx + 1], (qmd_addr >> 32) as u32);
        let send_pcas_b_idx = words
            .iter()
            .position(|&w| w == mthd_incr(0, method::SEND_SIGNALING_PCAS_B, 1))
            .unwrap();
        assert_eq!(words[send_pcas_b_idx + 1], qmd_addr as u32);
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
}
