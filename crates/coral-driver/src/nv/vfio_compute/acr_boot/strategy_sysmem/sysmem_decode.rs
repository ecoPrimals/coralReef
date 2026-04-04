// SPDX-License-Identifier: AGPL-3.0-only

//! Pure decode helpers for system-memory ACR boot diagnostics.

/// Minimum byte length required to read FECS (offset 20) and GPCCS (offset 44) status dwords.
pub(super) const WPR_STATUS_DECODE_MIN_LEN: usize = 48;

/// Decode FECS and GPCCS status dwords from a WPR header slice (little-endian).
///
/// Offsets match the WPR layout used in `sysmem_impl::attempt_sysmem_acr_boot_inner`
/// diagnostics: FECS at bytes 20–23, GPCCS at bytes 44–47. Values `1` and `0xFF` indicate
/// copy-in-progress and done respectively in Nouveau-style logging.
///
/// If `wpr_slice` is shorter than 48 bytes, returns `(0, 0)` so callers avoid panicking on
/// truncated buffers.
#[must_use]
pub(super) fn decode_wpr_status(wpr_slice: &[u8]) -> (u32, u32) {
    if wpr_slice.len() < WPR_STATUS_DECODE_MIN_LEN {
        return (0, 0);
    }
    let fecs = u32::from_le_bytes([wpr_slice[20], wpr_slice[21], wpr_slice[22], wpr_slice[23]]);
    let gpccs = u32::from_le_bytes([wpr_slice[44], wpr_slice[45], wpr_slice[46], wpr_slice[47]]);
    (fecs, gpccs)
}

/// Nouveau convention: SEC2 falcon `MAILBOX0` `== 0` after ACR load means success.
#[must_use]
pub(super) fn is_acr_success(mailbox0: u32) -> bool {
    mailbox0 == 0
}

#[cfg(test)]
mod tests {
    use super::{decode_wpr_status, is_acr_success};

    #[test]
    fn decode_wpr_status_all_zeros() {
        let buf = [0_u8; 48];
        assert_eq!(decode_wpr_status(&buf), (0, 0));
    }

    #[test]
    fn decode_wpr_status_fecs_copy_gpccs_done() {
        let mut buf = [0_u8; 48];
        buf[20..24].copy_from_slice(&1_u32.to_le_bytes());
        buf[44..48].copy_from_slice(&0xFF_u32.to_le_bytes());
        assert_eq!(decode_wpr_status(&buf), (1, 0xFF));
    }

    #[test]
    fn decode_wpr_status_known_pattern() {
        let mut buf = [0xAA_u8; 64];
        buf[20..24].copy_from_slice(&0x0102_0304_u32.to_le_bytes());
        buf[44..48].copy_from_slice(&0xAABB_CCDD_u32.to_le_bytes());
        assert_eq!(decode_wpr_status(&buf), (0x0102_0304, 0xAABB_CCDD));
    }

    #[test]
    fn decode_wpr_status_short_slice_returns_zeros() {
        let buf = [0_u8; 16];
        assert_eq!(decode_wpr_status(&buf), (0, 0));
    }

    #[test]
    fn is_acr_success_cases() {
        assert!(is_acr_success(0));
        assert!(!is_acr_success(1));
        assert!(!is_acr_success(0xDEAD));
        assert!(!is_acr_success(0xFFFF_FFFF));
    }
}
