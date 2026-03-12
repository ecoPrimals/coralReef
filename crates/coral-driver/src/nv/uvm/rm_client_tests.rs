// SPDX-License-Identifier: AGPL-3.0-only
//! Tests for [`RmClient`] and RM protocol integration.

use super::super::*;
use super::*;

#[test]
#[ignore = "requires proprietary nvidia driver loaded"]
fn rm_client_alloc() {
    let client = RmClient::new().expect("RM root client allocation");
    assert!(client.handle() != 0);
}

/// Open an RM client + GPU device (common setup for hardware tests).
fn hw_client_and_device() -> (RmClient, NvGpuDevice, u32) {
    let gpu = NvGpuDevice::open(0).expect("open GPU");
    let mut client = RmClient::new().expect("RM root client");
    gpu.register_fd(client.ctl_fd()).expect("register GPU fd");
    let h_device = client.alloc_device(gpu.index()).expect("RM device");
    (client, gpu, h_device)
}

#[test]
#[ignore = "requires proprietary nvidia driver loaded"]
fn rm_client_alloc_device() {
    let (_client, _gpu, _h_device) = hw_client_and_device();
}

#[test]
#[ignore = "requires proprietary nvidia driver loaded"]
fn rm_client_alloc_subdevice() {
    let (mut client, _gpu, h_device) = hw_client_and_device();
    let _h_subdevice = client.alloc_subdevice(h_device).expect("RM subdevice");
}

#[test]
#[ignore = "requires proprietary nvidia driver loaded"]
fn uvm_register_gpu() {
    let (mut client, _gpu, h_device) = hw_client_and_device();
    let h_subdevice = client.alloc_subdevice(h_device).expect("RM subdevice");
    let uvm = NvUvmDevice::open().expect("open uvm");
    uvm.initialize().expect("UVM_INITIALIZE");
    let _uuid = client
        .register_gpu_with_uvm(h_subdevice, &uvm)
        .expect("GPU registration with UVM");
}

#[test]
#[ignore = "requires proprietary nvidia driver loaded"]
fn uvm_alloc_vaspace() {
    let (mut client, _gpu, h_device) = hw_client_and_device();
    let _h_vaspace = client.alloc_vaspace(h_device).expect("VA space");
}

#[test]
#[ignore = "requires proprietary nvidia driver loaded"]
fn uvm_alloc_channel() {
    let (mut client, _gpu, h_device) = hw_client_and_device();
    let _h_subdevice = client.alloc_subdevice(h_device).expect("RM subdevice");
    let h_vaspace = client.alloc_vaspace(h_device).expect("VA space");
    let h_changrp = client
        .alloc_channel_group(h_device, h_vaspace)
        .expect("Channel group");

    let gpfifo_entries: u32 = 512;
    let gpfifo_size = u64::from(gpfifo_entries) * 8;
    let h_gpfifo_mem = h_device + 0x5000;
    let h_userd_mem = h_device + 0x5001;
    let h_virt_mem = h_device + 0x5002;
    client
        .alloc_system_memory(h_device, h_gpfifo_mem, gpfifo_size)
        .expect("GPFIFO");
    client
        .alloc_system_memory(h_device, h_userd_mem, 4096)
        .expect("USERD");
    client
        .alloc_virtual_memory(h_device, h_virt_mem, h_vaspace)
        .expect("virtual memory");

    let gpfifo_gpu_va = client
        .rm_map_memory_dma(h_device, h_virt_mem, h_gpfifo_mem, 0, gpfifo_size)
        .expect("GPFIFO DMA map");

    let _h_channel = client
        .alloc_gpfifo_channel(
            h_changrp,
            h_userd_mem,
            gpfifo_gpu_va,
            gpfifo_entries,
            AMPERE_CHANNEL_GPFIFO_A,
        )
        .expect("GPFIFO channel");
}

#[test]
#[ignore = "requires proprietary nvidia driver loaded"]
fn uvm_compute_bind() {
    let (mut client, _gpu, h_device) = hw_client_and_device();
    let _h_subdevice = client.alloc_subdevice(h_device).expect("RM subdevice");
    let h_vaspace = client.alloc_vaspace(h_device).expect("VA space");
    let h_changrp = client
        .alloc_channel_group(h_device, h_vaspace)
        .expect("Channel group");

    let gpfifo_entries: u32 = 512;
    let gpfifo_size = u64::from(gpfifo_entries) * 8;
    let h_gpfifo_mem = h_device + 0x5000;
    let h_userd_mem = h_device + 0x5001;
    let h_virt_mem = h_device + 0x5002;
    client
        .alloc_system_memory(h_device, h_gpfifo_mem, gpfifo_size)
        .expect("GPFIFO");
    client
        .alloc_system_memory(h_device, h_userd_mem, 4096)
        .expect("USERD");
    client
        .alloc_virtual_memory(h_device, h_virt_mem, h_vaspace)
        .expect("virtual memory");

    let gpfifo_gpu_va = client
        .rm_map_memory_dma(h_device, h_virt_mem, h_gpfifo_mem, 0, gpfifo_size)
        .expect("GPFIFO DMA map");

    let h_channel = client
        .alloc_gpfifo_channel(
            h_changrp,
            h_userd_mem,
            gpfifo_gpu_va,
            gpfifo_entries,
            AMPERE_CHANNEL_GPFIFO_A,
        )
        .expect("GPFIFO channel");

    let _h_compute = client
        .alloc_compute_engine(h_channel, AMPERE_COMPUTE_B)
        .expect("Compute engine bind");
}

#[test]
#[ignore = "requires proprietary nvidia driver loaded"]
fn rm_protocol_observer_captures_session() {
    use crate::gsp::rm_observer::LoggingObserver;

    let gpu = NvGpuDevice::open(0).expect("open GPU");
    let mut client = RmClient::new().expect("RM root client");
    gpu.register_fd(client.ctl_fd()).expect("register GPU fd");
    client.attach_observer(Box::new(LoggingObserver::for_gpu("ga102", 86)));

    let h_device = client.alloc_device(gpu.index()).expect("RM device");
    let h_subdevice = client.alloc_subdevice(h_device).expect("RM subdevice");

    let uvm = NvUvmDevice::open().expect("open uvm");
    uvm.initialize().expect("UVM_INITIALIZE");
    let _uuid = client
        .register_gpu_with_uvm(h_subdevice, &uvm)
        .expect("GPU registration");

    let h_vaspace = client.alloc_vaspace(h_device).expect("VA space");
    let h_changrp = client
        .alloc_channel_group(h_device, h_vaspace)
        .expect("Channel group");

    let gpfifo_entries: u32 = 512;
    let gpfifo_size = u64::from(gpfifo_entries) * 8;
    let h_gpfifo_mem = h_device + 0x5000;
    let h_userd_mem = h_device + 0x5001;
    let h_virt_mem = h_device + 0x5002;
    client
        .alloc_system_memory(h_device, h_gpfifo_mem, gpfifo_size)
        .expect("GPFIFO mem");
    client
        .alloc_system_memory(h_device, h_userd_mem, 4096)
        .expect("USERD mem");
    client
        .alloc_virtual_memory(h_device, h_virt_mem, h_vaspace)
        .expect("virtual memory");

    let gpfifo_gpu_va = client
        .rm_map_memory_dma(h_device, h_virt_mem, h_gpfifo_mem, 0, gpfifo_size)
        .expect("GPFIFO DMA map");

    let h_channel = client
        .alloc_gpfifo_channel(
            h_changrp,
            h_userd_mem,
            gpfifo_gpu_va,
            gpfifo_entries,
            AMPERE_CHANNEL_GPFIFO_A,
        )
        .expect("GPFIFO channel");

    let _h_compute = client
        .alloc_compute_engine(h_channel, AMPERE_COMPUTE_B)
        .expect("Compute engine");

    let obs = client.detach_observer().expect("observer was attached");
    let logging_obs: Box<LoggingObserver> = obs.into_any().downcast().expect("is LoggingObserver");
    let log = logging_obs.into_log();

    eprintln!("\n=== RM Protocol Log for GA102 (RTX 3090) ===");
    eprintln!("Total operations: {}", log.len());
    eprintln!("Successful alloc classes:");
    for class in &log.successful_classes() {
        eprintln!("  0x{class:04X}");
    }
    eprintln!(
        "\nAllocation recipe ({} steps):",
        log.allocation_recipe().len()
    );
    for step in &log.allocation_recipe() {
        eprintln!(
            "  class=0x{:04X} typed={} params_size={}",
            step.class, step.has_params, step.params_size
        );
    }

    assert!(
        log.len() >= 8,
        "should capture RM operations after observer attached"
    );
    let recipe = log.allocation_recipe();
    assert!(
        recipe.len() >= 7,
        "should have successful allocs (root alloc precedes observer)"
    );
    assert_eq!(
        recipe[0].class, NV01_DEVICE_0,
        "first observed alloc is DEVICE"
    );
}

#[test]
#[ignore = "requires proprietary nvidia driver loaded"]
fn uvm_external_memory_mapping() {
    let gpu = NvGpuDevice::open(0).expect("open GPU");
    let mut client = RmClient::new().expect("RM root client");
    gpu.register_fd(client.ctl_fd()).expect("register GPU fd");
    let uvm = NvUvmDevice::open().expect("open UVM");
    uvm.initialize().expect("UVM_INITIALIZE");

    let h_device = client.alloc_device(gpu.index()).expect("RM device");
    let h_subdevice = client.alloc_subdevice(h_device).expect("RM subdevice");
    client
        .register_gpu_with_uvm(h_subdevice, &uvm)
        .expect("GPU registered with UVM");

    let h_mem = h_device + 0x7000;
    client
        .alloc_system_memory(h_device, h_mem, 4096)
        .expect("RM system memory");

    let cpu_addr = client
        .rm_map_memory(h_device, h_mem, 0, 4096)
        .expect("RM_MAP_MEMORY");

    let ptr = cpu_addr as *mut u32;
    // SAFETY: `cpu_addr` is a valid CPU mapping returned by `rm_map_memory`
    // for a 4096-byte system memory allocation. Writing and reading a u32
    // at offset 0 is within bounds and properly aligned.
    unsafe {
        std::ptr::write_volatile(ptr, 0xDEAD_BEEF);
        let readback = std::ptr::read_volatile(ptr);
        assert_eq!(readback, 0xDEAD_BEEF);
    }

    client
        .rm_unmap_memory(h_device, h_mem, cpu_addr)
        .expect("RM_UNMAP_MEMORY");
    client.free_object(h_device, h_mem).expect("free memory");
}
