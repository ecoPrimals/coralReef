// SPDX-License-Identifier: AGPL-3.0-only
//! Helper functions for RM client operations: UUID parsing and raw ioctl.

use crate::error::{DriverError, DriverResult};

/// Parse a GID (either binary with 0x04 header or ASCII "GPU-XXXX-...")
/// into a 16-byte `NvProcessorUuid`.
pub(super) fn parse_gid_to_uuid(gid: &[u8]) -> DriverResult<[u8; 16]> {
    if gid.len() >= 16 && gid[0] == 0x04 {
        let mut uuid = [0u8; 16];
        uuid.copy_from_slice(&gid[..16]);
        return Ok(uuid);
    }

    let s = std::str::from_utf8(gid)
        .map_err(|_| DriverError::SubmitFailed("GID is neither binary nor valid ASCII".into()))?;

    let hex: String = s
        .trim_start_matches("GPU-")
        .trim_end_matches('\0')
        .chars()
        .filter(|c| c.is_ascii_hexdigit())
        .collect();

    if hex.len() < 32 {
        return Err(DriverError::SubmitFailed(
            format!("GID hex too short: {} chars from {s:?}", hex.len()).into(),
        ));
    }

    let mut uuid = [0u8; 16];
    for (i, chunk) in hex.as_bytes().chunks(2).take(16).enumerate() {
        let hi = hex_nibble(chunk[0]);
        let lo = hex_nibble(chunk[1]);
        uuid[i] = (hi << 4) | lo;
    }
    Ok(uuid)
}

fn hex_nibble(b: u8) -> u8 {
    match b {
        b'0'..=b'9' => b - b'0',
        b'a'..=b'f' => 10 + b - b'a',
        b'A'..=b'F' => 10 + b - b'A',
        _ => 0,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_binary_gid_with_0x04_header() {
        let mut gid = [0u8; 16];
        gid[0] = 0x04;
        for i in 1..16 {
            gid[i] = i as u8;
        }
        let uuid = parse_gid_to_uuid(&gid).expect("binary GID should parse");
        assert_eq!(uuid, gid);
    }

    #[test]
    fn parse_binary_gid_longer_than_16_bytes() {
        let mut gid = [0u8; 32];
        gid[0] = 0x04;
        for i in 1..16 {
            gid[i] = (0xA0 + i) as u8;
        }
        let uuid = parse_gid_to_uuid(&gid).expect("long binary GID should parse");
        assert_eq!(uuid[0], 0x04);
        assert_eq!(uuid[1], 0xA1);
    }

    #[test]
    fn parse_ascii_gpu_uuid() {
        let ascii = b"GPU-12345678-abcd-ef01-2345-67890abcdef0";
        let uuid = parse_gid_to_uuid(ascii).expect("ASCII GID should parse");
        assert_eq!(uuid[0], 0x12);
        assert_eq!(uuid[1], 0x34);
        assert_eq!(uuid[2], 0x56);
        assert_eq!(uuid[3], 0x78);
        assert_eq!(uuid[4], 0xAB);
        assert_eq!(uuid[5], 0xCD);
    }

    #[test]
    fn parse_ascii_uuid_with_null_terminator() {
        let ascii = b"GPU-AABBCCDD-1122-3344-5566-778899001122\0\0";
        let uuid = parse_gid_to_uuid(ascii).expect("null-terminated GID should parse");
        assert_eq!(uuid[0], 0xAA);
        assert_eq!(uuid[1], 0xBB);
    }

    #[test]
    fn parse_ascii_too_short_fails() {
        let ascii = b"GPU-1234";
        let result = parse_gid_to_uuid(ascii);
        assert!(result.is_err());
    }

    #[test]
    fn parse_invalid_utf8_fails() {
        let invalid = [0xFF, 0xFE, 0xFD, 0xFC];
        let result = parse_gid_to_uuid(&invalid);
        assert!(result.is_err());
    }

    #[test]
    fn hex_nibble_digits() {
        for d in b'0'..=b'9' {
            assert_eq!(hex_nibble(d), d - b'0');
        }
    }

    #[test]
    fn hex_nibble_lower() {
        for (i, c) in (b'a'..=b'f').enumerate() {
            assert_eq!(hex_nibble(c), 10 + i as u8);
        }
    }

    #[test]
    fn hex_nibble_upper() {
        for (i, c) in (b'A'..=b'F').enumerate() {
            assert_eq!(hex_nibble(c), 10 + i as u8);
        }
    }

    #[test]
    fn hex_nibble_invalid_returns_zero() {
        assert_eq!(hex_nibble(b'G'), 0);
        assert_eq!(hex_nibble(b' '), 0);
        assert_eq!(hex_nibble(b'-'), 0);
    }
}

/// Raw ioctl via C FFI, bypassing `rustix::ioctl::Ioctl` (mishandles `NV_ESC_RM_*`).
///
/// # Safety
///
/// `fd` must be valid; `params` must be `#[repr(C)]` and the sole mutable reference.
pub(super) unsafe fn raw_nv_ioctl<T>(fd: i32, ioctl_nr: u64, params: &mut T) -> i32 {
    // SAFETY: ioctl is the kernel ABI; caller guarantees fd valid, params repr(C) and sole ref.
    unsafe extern "C" {
        fn ioctl(fd: i32, request: u64, ...) -> i32;
    }
    // SAFETY: from_mut(params) is valid for ioctl duration; kernel reads/writes synchronously.
    unsafe { ioctl(fd, ioctl_nr, std::ptr::from_mut(params)) }
}
