// SPDX-License-Identifier: AGPL-3.0-or-later

//! Unit tests for pure encoding helpers in [`super`] (`GpuGen`, `gpfifo_entry`, USERD offsets).

use super::*;

use super::super::uvm::{
    ADA_COMPUTE_A, AMPERE_CHANNEL_GPFIFO_A, AMPERE_COMPUTE_A, AMPERE_COMPUTE_B,
    BLACKWELL_CHANNEL_GPFIFO_B, BLACKWELL_COMPUTE_A, BLACKWELL_COMPUTE_B, HOPPER_COMPUTE_A,
    NvGpuDevice, NvUvmDevice, RmClient, VOLTA_CHANNEL_GPFIFO_A, VOLTA_COMPUTE_A,
};

/// Low 42 bits of a GPFIFO entry hold the push-buffer VA with bits `[1:0]` cleared (`gpfifo_entry`).
const GPFIFO_VA_MASK: u64 = (1_u64 << 42) - 4;

/// Bits `42..64` of the packed `u64` store `length_dwords`; only 22 bits survive `(length_dwords as u64) << 42`.
const GPFIFO_LENGTH_SHIFT: u32 = 42;

#[inline]
const fn gpfifo_length_field_mask() -> u64 {
    let usable_bits = u64::BITS - GPFIFO_LENGTH_SHIFT;
    (1_u64 << usable_bits) - 1
}

#[test]
fn userd_gp_offsets_match_volta_ramuserd_layout() {
    assert_eq!(USERD_GP_PUT_OFFSET, 0x8C);
    assert_eq!(USERD_GP_GET_OFFSET, 0x88);
    assert_eq!(USERD_GP_PUT_OFFSET, 35 * 4);
    assert_eq!(USERD_GP_GET_OFFSET, 34 * 4);
}

#[test]
fn gpu_gen_from_sm_boundary_values() {
    assert_eq!(GpuGen::from_sm(0), GpuGen::Volta);
    assert_eq!(GpuGen::from_sm(70), GpuGen::Volta);

    assert_eq!(GpuGen::from_sm(74), GpuGen::Volta);
    assert_eq!(GpuGen::from_sm(75), GpuGen::Turing);
    assert_eq!(GpuGen::from_sm(76), GpuGen::Volta);

    assert_eq!(GpuGen::from_sm(79), GpuGen::Volta);
    assert_eq!(GpuGen::from_sm(80), GpuGen::AmpereA);
    assert_eq!(GpuGen::from_sm(81), GpuGen::AmpereB);
    assert_eq!(GpuGen::from_sm(88), GpuGen::AmpereB);

    assert_eq!(GpuGen::from_sm(89), GpuGen::Ada);
    assert_eq!(GpuGen::from_sm(90), GpuGen::Hopper);

    assert_eq!(GpuGen::from_sm(99), GpuGen::Volta);
    assert_eq!(GpuGen::from_sm(100), GpuGen::BlackwellA);
    assert_eq!(GpuGen::from_sm(101), GpuGen::Volta);
    assert_eq!(GpuGen::from_sm(119), GpuGen::Volta);

    assert_eq!(GpuGen::from_sm(120), GpuGen::BlackwellB);
    assert_eq!(GpuGen::from_sm(121), GpuGen::BlackwellB);
    assert_eq!(GpuGen::from_sm(u32::MAX), GpuGen::BlackwellB);
}

#[test]
fn gpu_gen_channel_class_matches_generation_matrix() {
    assert_eq!(GpuGen::Volta.channel_class(), VOLTA_CHANNEL_GPFIFO_A);
    assert_eq!(GpuGen::Turing.channel_class(), VOLTA_CHANNEL_GPFIFO_A);

    assert_eq!(GpuGen::AmpereA.channel_class(), AMPERE_CHANNEL_GPFIFO_A);
    assert_eq!(GpuGen::AmpereB.channel_class(), AMPERE_CHANNEL_GPFIFO_A);
    assert_eq!(GpuGen::Ada.channel_class(), AMPERE_CHANNEL_GPFIFO_A);
    assert_eq!(GpuGen::Hopper.channel_class(), AMPERE_CHANNEL_GPFIFO_A);

    assert_eq!(
        GpuGen::BlackwellA.channel_class(),
        BLACKWELL_CHANNEL_GPFIFO_B
    );
    assert_eq!(
        GpuGen::BlackwellB.channel_class(),
        BLACKWELL_CHANNEL_GPFIFO_B
    );
}

#[test]
fn gpu_gen_compute_class_matches_generation_matrix() {
    assert_eq!(GpuGen::Volta.compute_class(), VOLTA_COMPUTE_A);
    assert_eq!(GpuGen::Turing.compute_class(), VOLTA_COMPUTE_A);

    assert_eq!(GpuGen::AmpereA.compute_class(), AMPERE_COMPUTE_A);
    assert_eq!(GpuGen::AmpereB.compute_class(), AMPERE_COMPUTE_B);
    assert_eq!(GpuGen::Ada.compute_class(), ADA_COMPUTE_A);
    assert_eq!(GpuGen::Hopper.compute_class(), HOPPER_COMPUTE_A);

    assert_eq!(GpuGen::BlackwellA.compute_class(), BLACKWELL_COMPUTE_A);
    assert_eq!(GpuGen::BlackwellB.compute_class(), BLACKWELL_COMPUTE_B);
}

#[test]
fn gpu_gen_from_sm_selects_known_channel_and_compute_classes() {
    assert_eq!(GpuGen::from_sm(70).channel_class(), VOLTA_CHANNEL_GPFIFO_A);
    assert_eq!(GpuGen::from_sm(70).compute_class(), VOLTA_COMPUTE_A);

    assert_eq!(GpuGen::from_sm(75).channel_class(), VOLTA_CHANNEL_GPFIFO_A);
    assert_eq!(GpuGen::from_sm(75).compute_class(), VOLTA_COMPUTE_A);

    assert_eq!(GpuGen::from_sm(80).channel_class(), AMPERE_CHANNEL_GPFIFO_A);
    assert_eq!(GpuGen::from_sm(80).compute_class(), AMPERE_COMPUTE_A);

    assert_eq!(GpuGen::from_sm(86).channel_class(), AMPERE_CHANNEL_GPFIFO_A);
    assert_eq!(GpuGen::from_sm(86).compute_class(), AMPERE_COMPUTE_B);

    assert_eq!(GpuGen::from_sm(89).channel_class(), AMPERE_CHANNEL_GPFIFO_A);
    assert_eq!(GpuGen::from_sm(89).compute_class(), ADA_COMPUTE_A);

    assert_eq!(GpuGen::from_sm(90).channel_class(), AMPERE_CHANNEL_GPFIFO_A);
    assert_eq!(GpuGen::from_sm(90).compute_class(), HOPPER_COMPUTE_A);

    assert_eq!(
        GpuGen::from_sm(100).channel_class(),
        BLACKWELL_CHANNEL_GPFIFO_B
    );
    assert_eq!(GpuGen::from_sm(100).compute_class(), BLACKWELL_COMPUTE_A);

    assert_eq!(
        GpuGen::from_sm(120).channel_class(),
        BLACKWELL_CHANNEL_GPFIFO_B
    );
    assert_eq!(GpuGen::from_sm(120).compute_class(), BLACKWELL_COMPUTE_B);
}

#[test]
fn gpfifo_entry_packs_va_into_low_bits_and_length_at_bit_42() {
    let push_buf_va = 0x0000_0001_2345_6F00_u64;
    let length_dwords = 0x1337_u32;
    let entry = gpfifo_entry(push_buf_va, length_dwords);

    let va_part = entry & GPFIFO_VA_MASK;
    assert_eq!(va_part, push_buf_va & !3);

    let len_part = entry >> 42;
    assert_eq!(len_part, u64::from(length_dwords));

    let dw0 = entry as u32;
    assert_eq!(dw0, push_buf_va as u32);
    let dw1 = (entry >> 32) as u32;
    let addr_hi_9 = (dw1 & 0x1FF) as u64;
    let decoded_va = u64::from(dw0) | (addr_hi_9 << 32);
    assert_eq!(decoded_va, push_buf_va & !3);
    let decoded_len = (dw1 >> 10) & 0x1F_FFFF;
    assert_eq!(decoded_len, length_dwords);
}

#[test]
fn gpfifo_entry_masks_va_to_four_byte_alignment() {
    let misaligned = 0x1_0000_1003_u64;
    let entry = gpfifo_entry(misaligned, 1);
    assert_eq!(entry & GPFIFO_VA_MASK, misaligned & !3);
    assert_eq!(entry & 3, 0);
    assert_eq!(entry >> 42, 1);
}

#[test]
fn gpfifo_entry_zero_va_and_zero_length() {
    let entry = gpfifo_entry(0, 0);
    assert_eq!(entry, 0);
}

#[test]
fn gpfifo_entry_max_u32_length_truncates_to_length_field_width() {
    let max_len = u32::MAX;
    let entry = gpfifo_entry(0, max_len);
    assert_eq!(entry & GPFIFO_VA_MASK, 0);
    let stored = entry >> GPFIFO_LENGTH_SHIFT;
    let expected = (max_len as u64) & gpfifo_length_field_mask();
    assert_eq!(stored, expected);
    assert_eq!(stored, 0x003F_FFFF);
}

#[test]
fn gpfifo_entry_length_dwords_round_trip_for_all_bits_in_length_field() {
    // VA must satisfy `(va & !3) < (1 << 42)` or the length OR overlaps high address bits.
    let va = 0x0000_003F_FFFF_F000_u64;
    let len = 0x003F_FFFF_u32;
    let entry = gpfifo_entry(va, len);
    assert_eq!(entry >> GPFIFO_LENGTH_SHIFT, u64::from(len));
    assert_eq!(entry & GPFIFO_VA_MASK, va & !3);
}

#[test]
fn page_alignment() {
    assert_eq!(page_align(1), 4096);
    assert_eq!(page_align(4096), 4096);
    assert_eq!(page_align(4097), 8192);
    assert_eq!(page_align(0), 0);
}

fn detect_sm_version() -> u32 {
    std::process::Command::new("nvidia-smi")
        .args(["--query-gpu=compute_cap", "--format=csv,noheader"])
        .output()
        .ok()
        .and_then(|out| {
            let s = String::from_utf8_lossy(&out.stdout);
            let parts: Vec<&str> = s.trim().split('.').collect();
            if parts.len() == 2 {
                let major: u32 = parts[0].parse().ok()?;
                let minor: u32 = parts[1].parse().ok()?;
                Some(major * 10 + minor)
            } else {
                None
            }
        })
        .unwrap_or(86)
}

#[test]
#[ignore = "requires proprietary nvidia driver loaded"]
fn uvm_compute_device_open() {
    let sm = detect_sm_version();
    let device = NvUvmComputeDevice::open(0, sm).expect("UVM compute device");
    assert!(device.is_open());
}

#[test]
#[ignore = "requires proprietary nvidia driver loaded"]
fn uvm_compute_alloc_free() {
    let sm = detect_sm_version();
    let mut device = NvUvmComputeDevice::open(0, sm).expect("UVM compute device");
    let handle = device.alloc(4096, MemoryDomain::Gtt).expect("buffer alloc");
    device.free(handle).expect("buffer free");
}

#[test]
#[ignore = "requires proprietary nvidia driver loaded"]
fn uvm_map_memory_single_context() {
    let mut client = RmClient::new().expect("RM root client");
    let uvm = NvUvmDevice::open().expect("open UVM");
    let gpu = NvGpuDevice::open(0).expect("open GPU");
    gpu.register_fd(client.ctl_fd()).expect("register GPU fd");
    uvm.initialize().expect("UVM_INITIALIZE");

    let h_device = client.alloc_device(gpu.index()).expect("RM device");
    let h_subdevice = client.alloc_subdevice(h_device).expect("RM subdevice");
    let _uuid = client
        .register_gpu_with_uvm(h_subdevice, &uvm)
        .expect("register UVM");

    // On Blackwell (580.x), only one rm_map_memory context per nvidiactl fd.
    // Verify the combined-allocation strategy from open() works.
    let h_mem = h_device + 0x5000;
    let combined_size = USERD_SIZE + GPFIFO_SIZE;
    client
        .alloc_system_memory(h_device, h_mem, combined_size)
        .expect("alloc combined");
    let addr = client
        .rm_map_memory(h_device, h_mem, 0, combined_size)
        .expect("rm_map_memory combined");
    assert!(addr != 0);

    let userd_ptr = addr as *mut u32;
    let gpfifo_ptr = (addr + USERD_SIZE) as *mut u32;
    // SAFETY: addr is a valid kernel mmap'd address from `rm_map_memory`
    // (asserted non-null above). `userd_ptr` and `gpfifo_ptr` are within the
    // mapped range (`USERD_SIZE` + `GPFIFO_SIZE`). Volatile reads/writes
    // match the GPU-visible mapping semantics.
    unsafe {
        crate::mmio::VolatilePtr::new(userd_ptr).write(0xDEAD_BEEF);
        crate::mmio::VolatilePtr::new(gpfifo_ptr).write(0xCAFE_BABE);
        assert_eq!(crate::mmio::VolatilePtr::new(userd_ptr).read(), 0xDEAD_BEEF);
        assert_eq!(
            crate::mmio::VolatilePtr::new(gpfifo_ptr).read(),
            0xCAFE_BABE
        );
    }

    client
        .rm_unmap_memory(h_device, h_mem, addr)
        .expect("unmap");
}
