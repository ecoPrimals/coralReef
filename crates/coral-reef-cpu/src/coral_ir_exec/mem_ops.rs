// SPDX-License-Identifier: AGPL-3.0-only
//! Memory read/write helpers for the `CoralIR` interpreter.
//!
//! Supports three memory regions:
//! - **Global buffers** — binding data, addressed via synthetic `BUFFER_STRIDE`
//! - **Shared memory** — per-workgroup `var<workgroup>` storage
//! - **Atomics** — read-modify-write operations on both regions

use coral_reef::codegen::ir::AtomOp;

/// Spacing between synthetic buffer base addresses.
pub const BUFFER_STRIDE: u32 = 0x10_0000;

/// Decode a synthetic address into (buffer index, byte offset).
pub const fn decode_addr(addr: usize) -> (usize, usize) {
    let stride = BUFFER_STRIDE as usize;
    (addr / stride, addr % stride)
}

pub fn read_u32_from_buffers(buffers: &[Vec<u8>], addr: usize) -> u32 {
    let (buf_idx, byte_off) = decode_addr(addr);
    if let Some(buf) = buffers.get(buf_idx) {
        if byte_off + 4 <= buf.len() {
            return u32::from_le_bytes([
                buf[byte_off],
                buf[byte_off + 1],
                buf[byte_off + 2],
                buf[byte_off + 3],
            ]);
        }
    }
    0
}

pub fn write_u32_to_buffers(buffers: &mut [Vec<u8>], addr: usize, val: u32) {
    let (buf_idx, byte_off) = decode_addr(addr);
    if let Some(buf) = buffers.get_mut(buf_idx) {
        if byte_off + 4 <= buf.len() {
            buf[byte_off..byte_off + 4].copy_from_slice(&val.to_le_bytes());
        }
    }
}

pub fn read_u32_from_shared(shared: &[u8], offset: usize) -> u32 {
    if offset + 4 <= shared.len() {
        u32::from_le_bytes([
            shared[offset],
            shared[offset + 1],
            shared[offset + 2],
            shared[offset + 3],
        ])
    } else {
        0
    }
}

pub fn write_u32_to_shared(shared: &mut [u8], offset: usize, val: u32) {
    if offset + 4 <= shared.len() {
        shared[offset..offset + 4].copy_from_slice(&val.to_le_bytes());
    }
}

#[expect(
    clippy::cast_possible_wrap,
    reason = "atomic min/max operates on signed i32"
)]
#[expect(
    clippy::cast_sign_loss,
    reason = "result reinterpreted as u32 bit pattern for memory"
)]
pub fn eval_atomic(op: AtomOp, current: u32, data: u32) -> u32 {
    match op {
        AtomOp::Add => current.wrapping_add(data),
        AtomOp::Min => (current as i32).min(data as i32) as u32,
        AtomOp::Max => (current as i32).max(data as i32) as u32,
        AtomOp::And => current & data,
        AtomOp::Or => current | data,
        AtomOp::Xor => current ^ data,
        AtomOp::Exch => data,
        AtomOp::Inc => {
            if current >= data {
                0
            } else {
                current + 1
            }
        }
        AtomOp::Dec => {
            if current == 0 || current > data {
                data
            } else {
                current - 1
            }
        }
        AtomOp::CmpExch(_) => current,
    }
}
