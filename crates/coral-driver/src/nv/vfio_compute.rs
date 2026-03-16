// SPDX-License-Identifier: AGPL-3.0-only
//! NVIDIA VFIO compute device — direct BAR0/DMA dispatch without kernel driver.
//!
//! Implements [`ComputeDevice`] using the VFIO subsystem:
//! - BAR0 MMIO for register access (GR init, GPFIFO doorbell)
//! - DMA buffers for shader code, QMD, push buffers, and user data
//! - Direct GPFIFO submission via BAR0 USERD write
//!
//! # Prerequisites (provided by toadStool)
//!
//! - GPU bound to `vfio-pci`
//! - IOMMU enabled and configured
//! - User has `/dev/vfio/*` permissions
//!
//! # Architecture
//!
//! ```text
//! NvVfioComputeDevice
//!   ├─ VfioDevice       (container + group + device fd)
//!   ├─ MappedBar (BAR0) (MMIO register access)
//!   ├─ DmaBuffer pool   (IOMMU-mapped host memory for GPU)
//!   │   ├─ GPFIFO ring  (command entries)
//!   │   ├─ USERD page   (doorbell for GPFIFO put pointer)
//!   │   └─ user buffers (shader, QMD, data)
//!   └─ pushbuf + QMD    (reuses coral-driver's existing builders)
//! ```

use crate::error::{DriverError, DriverResult};
use crate::gsp::{self, GrFirmwareBlobs, GrInitSequence};
use crate::vfio::channel::{VfioChannel, ramuserd};
use crate::vfio::device::{MappedBar, VfioDevice};
use crate::vfio::dma::DmaBuffer;
use crate::{BufferHandle, ComputeDevice, DispatchDims, MemoryDomain, ShaderInfo};

use super::pushbuf::PushBuf;
use super::qmd;

use std::borrow::Cow;
use std::collections::HashMap;

/// BAR0 register offsets for NVIDIA GPU.
mod bar0_reg {
    /// Boot0 register — chip identification.
    pub const BOOT0: usize = 0x0000_0000;
}

/// Sync timeout — 5 seconds matches the nouveau/UVM paths.
const SYNC_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(5);

/// Spin-loop sleep interval for GPFIFO polling.
const POLL_INTERVAL: std::time::Duration = std::time::Duration::from_micros(10);

/// GPFIFO configuration constants.
mod gpfifo {
    /// Number of GPFIFO entries (must be power of 2).
    pub const ENTRIES: usize = 128;
    /// Size of each GPFIFO entry in bytes.
    pub const ENTRY_SIZE: usize = 8;
    /// Total GPFIFO ring size in bytes.
    pub const RING_SIZE: usize = ENTRIES * ENTRY_SIZE;

    /// Encode a GPFIFO indirect-buffer entry (NVB06F GP_ENTRY format).
    ///
    /// DW0 (GP_ENTRY0):
    ///   `[1:0]`  = TYPE (0 = PB_SEGMENT)
    ///   `[31:2]` = VA[31:2] (address bits at natural positions)
    ///
    /// DW1 (GP_ENTRY1):
    ///   `[7:0]`  = VA[39:32]
    ///   `[9:8]`  = PRIV (0)
    ///   `[30:10]` = LENGTH in dwords
    ///   `[31]`   = SYNC (0)
    pub fn encode_entry(gpu_addr: u64, len_bytes: u32) -> u64 {
        let lo = gpu_addr & 0xFFFF_FFFC; // VA with TYPE=0 (PB_SEG)
        let hi_addr = (gpu_addr >> 32) & 0xFF;
        let len_dwords = u64::from(len_bytes / 4);
        let hi = hi_addr | (len_dwords << 10);
        lo | (hi << 32)
    }
}

/// DMA-backed GPU buffer tracked by the VFIO device.
struct VfioBuffer {
    dma: DmaBuffer,
    size: u64,
}

/// NVIDIA compute device via VFIO — direct BAR0 + DMA dispatch.
///
/// This is the sovereign compute path where coralReef owns the entire
/// hardware interaction: no kernel GPU driver (nouveau/nvidia) needed,
/// only the VFIO-IOMMU plumbing (provided by toadStool).
pub struct NvVfioComputeDevice {
    #[expect(dead_code, reason = "kept alive for fd lifecycle; used by DmaBuffer")]
    device: VfioDevice,
    bar0: MappedBar,
    sm_version: u32,
    compute_class: u32,
    gpfifo_ring: DmaBuffer,
    gpfifo_put: u32,
    userd: DmaBuffer,
    channel: VfioChannel,
    next_handle: u32,
    next_iova: u64,
    container_fd: std::os::fd::RawFd,
    buffers: HashMap<u32, VfioBuffer>,
    inflight: Vec<BufferHandle>,
}

/// GR engine diagnostic status from BAR0 registers.
#[derive(Debug)]
pub struct GrEngineStatus {
    /// `NV_PGRAPH_STATUS` — GR busy/idle flags.
    pub pgraph_status: u32,
    /// `NV_PGRAPH_FECS_FALCON_CPUCTL` — FECS CPU control (bit 5 = halted).
    pub fecs_cpuctl: u32,
    /// `NV_PGRAPH_FECS_FALCON_MAILBOX0` — FECS status/error code.
    pub fecs_mailbox0: u32,
    /// `NV_PGRAPH_FECS_FALCON_MAILBOX1` — FECS extended status.
    pub fecs_mailbox1: u32,
    /// `NV_PGRAPH_FECS_FALCON_HWCFG` — FECS hardware config.
    pub fecs_hwcfg: u32,
    /// `NV_PGRAPH_GPCCS_FALCON_CPUCTL` — GPCCS CPU control.
    pub gpccs_cpuctl: u32,
    /// `NV_PMC_ENABLE` — master engine enable mask.
    pub pmc_enable: u32,
    /// `NV_PFIFO_ENABLE` — FIFO engine enable.
    pub pfifo_enable: u32,
}

impl GrEngineStatus {
    /// Whether the FECS falcon is halted (not running firmware).
    #[must_use]
    pub fn fecs_halted(&self) -> bool {
        self.fecs_cpuctl & 0x20 != 0 || self.fecs_cpuctl == 0xDEAD_DEAD
    }

    /// Whether the GR engine bit is enabled in PMC.
    #[must_use]
    pub fn gr_enabled(&self) -> bool {
        self.pmc_enable & (1 << 12) != 0
    }
}

impl std::fmt::Display for GrEngineStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "GR: pmc={:#010x} pfifo={:#010x} pgraph={:#010x} fecs_cpu={:#010x} fecs_mb0={:#010x} fecs_mb1={:#010x} fecs_hw={:#010x} gpccs={:#010x} [fecs_halted={} gr_en={}]",
            self.pmc_enable,
            self.pfifo_enable,
            self.pgraph_status,
            self.fecs_cpuctl,
            self.fecs_mailbox0,
            self.fecs_mailbox1,
            self.fecs_hwcfg,
            self.gpccs_cpuctl,
            self.fecs_halted(),
            self.gr_enabled()
        )
    }
}

/// IOVA base for user DMA allocations — above GPFIFO/USERD.
const USER_IOVA_BASE: u64 = 0x10_0000;

/// GPFIFO ring IOVA.
const GPFIFO_IOVA: u64 = 0x1000;

/// USERD page IOVA.
const USERD_IOVA: u64 = 0x2000;

/// Local memory window address for Volta+ (SM >= 70).
const LOCAL_MEM_WINDOW_VOLTA: u64 = 0xFF00_0000_0000_0000;

/// Local memory window address for pre-Volta (SM < 70).
const LOCAL_MEM_WINDOW_LEGACY: u64 = 0xFF00_0000;

/// Raw VFIO device handle for diagnostic/experimental access to BAR0
/// without creating a PFIFO channel.
pub struct RawVfioDevice {
    #[expect(dead_code, reason = "kept alive for fd lifecycle")]
    device: VfioDevice,
    /// BAR0 MMIO mapping for register access.
    pub bar0: MappedBar,
    /// VFIO container fd for DMA buffer allocation.
    pub container_fd: std::os::fd::RawFd,
    /// GPFIFO ring DMA buffer (pre-allocated at standard IOVA).
    pub gpfifo_ring: DmaBuffer,
    /// USERD page DMA buffer (pre-allocated at standard IOVA).
    pub userd: DmaBuffer,
}

impl RawVfioDevice {
    /// Open a VFIO device with BAR0 mapped and GPFIFO/USERD DMA buffers
    /// allocated, but WITHOUT creating a PFIFO channel. Used by the
    /// diagnostic experiment matrix.
    pub fn open(bdf: &str) -> DriverResult<Self> {
        // Force D0 before VFIO open — vfio-pci probe puts the device in D3hot,
        // but HBM2 training is preserved. This restores BAR0 access.
        if let Err(e) = crate::vfio::channel::devinit::force_pci_d0(bdf) {
            tracing::warn!(bdf, error = %e, "force_pci_d0 failed (may already be in D0)");
        }
        let device = VfioDevice::open(bdf)?;
        let container_fd = device.container_fd();
        let bar0 = device.map_bar(0)?;
        let gpfifo_ring = DmaBuffer::new(container_fd, gpfifo::RING_SIZE, GPFIFO_IOVA)?;
        let userd = DmaBuffer::new(container_fd, 4096, USERD_IOVA)?;
        Ok(Self {
            device,
            bar0,
            container_fd,
            gpfifo_ring,
            userd,
        })
    }

    /// GPFIFO ring IOVA.
    pub const fn gpfifo_iova() -> u64 {
        GPFIFO_IOVA
    }

    /// Number of GPFIFO entries.
    pub const fn gpfifo_entries() -> u32 {
        gpfifo::ENTRIES as u32
    }

    /// USERD page IOVA.
    pub const fn userd_iova() -> u64 {
        USERD_IOVA
    }

    /// Leak the underlying VFIO fds to prevent kernel PM reset on drop.
    ///
    /// Preserves HBM2 training state — call when `CORALREEF_PRESERVE_STATE`
    /// is set so the GPU stays warm across test runs.
    pub fn leak(self) {
        std::mem::forget(self);
    }
}

/// Map SM version to chip codename for firmware lookup.
const fn sm_to_chip(sm: u32) -> &'static str {
    match sm {
        50..=52 => "gm200",
        60..=62 => "gp100",
        70 => "gv100",
        75 => "tu102",
        80 => "ga100",
        86..=87 => "ga102",
        89 => "ad102",
        _ => "gv100",
    }
}

impl NvVfioComputeDevice {
    /// Open an NVIDIA GPU via VFIO and prepare for compute dispatch.
    ///
    /// Performs sovereign GR engine initialization from firmware blobs,
    /// creates a PFIFO channel, and submits FECS context init methods —
    /// the full sequence needed for compute shader dispatch without any
    /// kernel GPU driver.
    ///
    /// # Errors
    ///
    /// Returns error if VFIO open, BAR0 mapping, or DMA allocation fails.
    pub fn open(bdf: &str, sm_version: u32, compute_class: u32) -> DriverResult<Self> {
        let device = VfioDevice::open(bdf)?;
        let container_fd = device.container_fd();

        let bar0 = device.map_bar(0)?;

        let chip_id = bar0.read_u32(bar0_reg::BOOT0)?;
        tracing::info!(
            bdf,
            chip_id = format_args!("{chip_id:#010x}"),
            sm_version,
            "VFIO GPU opened via BAR0"
        );

        // Phase 1: Sovereign BAR0 GR init — enable engines + apply PGRAPH
        // register writes from firmware blobs before channel creation.
        Self::apply_gr_bar0_init(&bar0, sm_version);

        let gpfifo_ring = DmaBuffer::new(container_fd, gpfifo::RING_SIZE, GPFIFO_IOVA)?;
        let userd = DmaBuffer::new(container_fd, 4096, USERD_IOVA)?;

        #[expect(
            clippy::cast_possible_truncation,
            reason = "GPFIFO entries constant always fits u32"
        )]
        let channel = VfioChannel::create(
            container_fd,
            &bar0,
            GPFIFO_IOVA,
            gpfifo::ENTRIES as u32,
            USERD_IOVA,
            0, // channel ID 0
        )?;

        let mut dev = Self {
            device,
            bar0,
            sm_version,
            compute_class,
            gpfifo_ring,
            gpfifo_put: 0,
            userd,
            channel,
            next_handle: 1,
            next_iova: USER_IOVA_BASE,
            container_fd,
            buffers: HashMap::new(),
            inflight: Vec::new(),
        };

        // Phase 2: FECS channel init — submit GR context methods via GPFIFO
        // so the compute engine is ready for shader dispatch.
        dev.apply_fecs_channel_init();

        Ok(dev)
    }

    /// Apply BAR0 GR init writes from NVIDIA firmware blobs.
    ///
    /// Parses `sw_bundle_init.bin` etc. from `/lib/firmware/nvidia/{chip}/gr/`,
    /// builds the init sequence, then applies the BAR0-targeted writes
    /// (PMC engine enable, FIFO enable, PGRAPH register programming).
    fn apply_gr_bar0_init(bar0: &MappedBar, sm_version: u32) {
        let chip = sm_to_chip(sm_version);
        let blobs = match GrFirmwareBlobs::parse(chip) {
            Ok(b) => b,
            Err(e) => {
                tracing::warn!(chip, error = %e, "GR firmware not available — skipping BAR0 GR init");
                return;
            }
        };

        let seq = if sm_version == 70 {
            GrInitSequence::for_gv100(&blobs)
        } else {
            GrInitSequence::from_blobs(&blobs)
        };

        let (bar0_writes, fecs_entries) = gsp::split_for_application(&seq);

        tracing::info!(
            chip,
            bar0_writes = bar0_writes.len(),
            fecs_entries = fecs_entries.len(),
            total = seq.len(),
            "sovereign VFIO GR init: applying {} BAR0 register writes",
            bar0_writes.len()
        );

        // Only apply writes with 4-byte-aligned offsets that fit within BAR0.
        let bar0_size = bar0.size() as u32;
        let writes: Vec<(u32, u32)> = bar0_writes
            .iter()
            .filter(|w| {
                if w.offset % 4 != 0 {
                    tracing::debug!(
                        chip,
                        offset = format!("{:#010x}", w.offset),
                        "skipping non-aligned BAR0 write"
                    );
                    return false;
                }
                if w.offset + 4 > bar0_size {
                    tracing::debug!(
                        chip,
                        offset = format!("{:#010x}", w.offset),
                        bar0_size = format!("{bar0_size:#010x}"),
                        "skipping out-of-range BAR0 write"
                    );
                    return false;
                }
                true
            })
            .map(|w| (w.offset, w.value))
            .collect();

        let (applied, failed) = bar0.apply_gr_bar0_writes(&writes);

        if failed > 0 {
            tracing::warn!(chip, applied, failed, "BAR0 GR init had write failures");
        } else {
            tracing::info!(chip, applied, "BAR0 GR init complete");
        }

        // Brief settle after engine enable writes.
        for w in &bar0_writes {
            if w.delay_us > 0 {
                std::thread::sleep(std::time::Duration::from_micros(u64::from(w.delay_us)));
            }
        }
    }

    /// Submit FECS channel init methods via GPFIFO after channel creation.
    ///
    /// Builds a push buffer containing the GR context setup methods
    /// from `sw_bundle_init.bin` / `sw_method_init.bin` (entries with
    /// offsets <= 0x7FFC that are submittable as channel methods).
    fn apply_fecs_channel_init(&mut self) {
        let chip = sm_to_chip(self.sm_version);
        let blobs = match GrFirmwareBlobs::parse(chip) {
            Ok(b) => b,
            Err(e) => {
                tracing::debug!(chip, error = %e, "firmware not available — skipping FECS init");
                return;
            }
        };

        let seq = if self.sm_version == 70 {
            GrInitSequence::for_gv100(&blobs)
        } else {
            GrInitSequence::from_blobs(&blobs)
        };

        let (_bar0, fecs) = gsp::split_for_application(&seq);

        let channel_methods: Vec<(u32, u32)> = fecs
            .iter()
            .filter(|w| {
                matches!(
                    w.category,
                    gsp::RegCategory::BundleInit | gsp::RegCategory::MethodInit
                )
            })
            .map(|w| (w.offset, w.value))
            .collect();

        if channel_methods.is_empty() {
            tracing::debug!(chip, "no FECS channel methods to submit");
            return;
        }

        tracing::info!(
            chip,
            entries = channel_methods.len(),
            "submitting FECS channel methods via GPFIFO"
        );

        let pb = PushBuf::gr_context_init(self.compute_class, &channel_methods);
        let pb_bytes = pb.as_bytes();

        let pb_result = (|| -> DriverResult<()> {
            let (pb_handle, pb_iova) = self.alloc_dma(pb_bytes.len())?;
            self.upload(pb_handle, 0, pb_bytes)?;

            let pb_size = u32::try_from(pb_bytes.len())
                .map_err(|_| DriverError::platform_overflow("FECS pushbuf size fits u32"))?;

            self.submit_pushbuf(pb_iova, pb_size)?;

            // Wait for FECS init to complete before returning the device
            // as ready for compute dispatch.
            self.poll_gpfifo_completion()?;

            let _ = self.free(pb_handle);
            Ok(())
        })();

        match pb_result {
            Ok(()) => tracing::info!(chip, "FECS channel init complete — GR engine ready"),
            Err(e) => {
                tracing::warn!(chip, error = %e, "FECS channel init failed (expected on cold VFIO — GR engine requires falcon firmware)")
            }
        }
    }

    /// Diagnostic: read GR engine status registers to verify readiness.
    ///
    /// Returns a summary of the GR engine state. A fully initialized
    /// GR engine has FECS falcon running and PGRAPH status idle.
    pub fn gr_engine_status(&self) -> GrEngineStatus {
        let r = |off: usize| self.bar0.read_u32(off).unwrap_or(0xDEAD_DEAD);

        GrEngineStatus {
            pgraph_status: r(0x0040_0700),
            fecs_cpuctl: r(0x0040_9100),
            fecs_mailbox0: r(0x0040_9130),
            fecs_mailbox1: r(0x0040_9134),
            fecs_hwcfg: r(0x0040_9800),
            gpccs_cpuctl: r(0x0041_a100),
            pmc_enable: r(0x0000_0200),
            pfifo_enable: r(0x0000_2504),
        }
    }

    /// Allocate a DMA buffer and assign it a new IOVA.
    fn alloc_dma(&mut self, size: usize) -> DriverResult<(BufferHandle, u64)> {
        let aligned = size.div_ceil(4096) * 4096;
        let iova = self.next_iova;
        self.next_iova += aligned as u64;

        let dma = DmaBuffer::new(self.container_fd, size, iova)?;
        let handle_id = self.next_handle;
        self.next_handle += 1;

        let handle = BufferHandle(handle_id);
        self.buffers.insert(
            handle_id,
            VfioBuffer {
                dma,
                size: size as u64,
            },
        );

        Ok((handle, iova))
    }

    /// Submit a push buffer via GPFIFO.
    ///
    /// Writes a GPFIFO entry pointing to the given IOVA/size, updates
    /// GP_PUT in the USERD page at the correct Volta RAMUSERD offset,
    /// then notifies the GPU via the USERMODE doorbell register.
    ///
    /// Uses volatile writes + memory fences to ensure the GPU sees the
    /// GPFIFO entry and GP_PUT before the doorbell MMIO write.
    fn submit_pushbuf(&mut self, pb_iova: u64, pb_size: u32) -> DriverResult<()> {
        let entry = gpfifo::encode_entry(pb_iova, pb_size);

        let slot = (self.gpfifo_put as usize) % gpfifo::ENTRIES;
        let offset = slot * gpfifo::ENTRY_SIZE;

        // Volatile write GPFIFO entry to DMA ring.
        let ring_ptr = self.gpfifo_ring.vaddr().cast_mut();
        // SAFETY: gpfifo_ring.vaddr() is valid from DmaBuffer::new; offset is
        // bounds-checked by slot % ENTRIES; volatile required for GPU DMA visibility.
        unsafe {
            std::ptr::write_volatile(ring_ptr.add(offset).cast::<u64>(), entry);
        }

        self.gpfifo_put = self.gpfifo_put.wrapping_add(1);

        // Volatile write GP_PUT to USERD at Volta RAMUSERD offset 0x8C.
        let userd_ptr = self.userd.vaddr().cast_mut();
        // SAFETY: userd.vaddr() is valid from DmaBuffer::new; ramuserd::GP_PUT
        // (0x8C) is within the 4096-byte USERD page; volatile required for GPU DMA.
        unsafe {
            std::ptr::write_volatile(
                userd_ptr.add(ramuserd::GP_PUT).cast::<u32>(),
                self.gpfifo_put,
            );
        }

        // Full memory fence to ensure DMA writes are globally visible
        // before the MMIO doorbell write crosses the PCIe bus.
        std::sync::atomic::fence(std::sync::atomic::Ordering::SeqCst);

        // Notify the GPU via NV_USERMODE_NOTIFY_CHANNEL_PENDING.
        self.bar0
            .write_u32(VfioChannel::doorbell_offset(), self.channel.id())
            .map_err(|e| DriverError::SubmitFailed(Cow::Owned(format!("doorbell: {e}"))))?;

        Ok(())
    }

    /// Poll USERD GP_GET until it catches up to GP_PUT, indicating
    /// the GPU has consumed all submitted GPFIFO entries.
    ///
    /// The GPU writes GP_GET back to the USERD DMA page at the Volta
    /// RAMUSERD offset (0x88). We poll this with a spin-loop + sleep,
    /// matching the UVM path's `poll_gpfifo_completion()` pattern.
    fn poll_gpfifo_completion(&self) -> DriverResult<()> {
        if self.gpfifo_put == 0 {
            return Ok(());
        }

        let deadline = std::time::Instant::now() + SYNC_TIMEOUT;
        let userd_ptr = self.userd.vaddr();

        loop {
            // SAFETY: userd DMA page is valid for the lifetime of the device;
            // GP_GET at Volta RAMUSERD offset 0x88 is a u32 written by the
            // GPU via IOMMU DMA. Volatile read required for async GPU writes.
            let gp_get =
                unsafe { std::ptr::read_volatile(userd_ptr.add(ramuserd::GP_GET).cast::<u32>()) };

            if gp_get >= self.gpfifo_put {
                return Ok(());
            }

            if std::time::Instant::now() > deadline {
                // SAFETY: same as GP_GET — userd DMA page valid; GP_PUT within bounds.
                let gp_put_val = unsafe {
                    std::ptr::read_volatile(userd_ptr.add(ramuserd::GP_PUT).cast::<u32>())
                };
                let r = |reg: usize| self.bar0.read_u32(reg).unwrap_or(0xDEAD);
                let pfifo_intr = r(0x2100);
                let pccsr_chan0 = r(0x80_0004);
                let pbdma_intr: [u32; 4] = [
                    r(0x40108),
                    r(0x40108 + 0x2000),
                    r(0x40108 + 0x4000),
                    r(0x40108 + 0x6000),
                ];
                let pbdma_hce: [u32; 4] = [
                    r(0x40148),
                    r(0x40148 + 0x2000),
                    r(0x40148 + 0x4000),
                    r(0x40148 + 0x6000),
                ];
                let pbdma_idle: [u32; 4] = [r(0x3080), r(0x3084), r(0x3088), r(0x308C)];
                let pbdma_runl_map: [u32; 4] = [r(0x2390), r(0x2394), r(0x2398), r(0x239C)];
                let mmu_fault_status = r(0x0010_0A2C);
                let mmu_hubtlb_err = r(0x0010_4A20);
                let priv_ring_intr = r(0x0001_2070);
                tracing::error!(
                    gp_get,
                    gp_put = gp_put_val,
                    expected = self.gpfifo_put,
                    channel_id = self.channel.id(),
                    pfifo_intr = format!("{pfifo_intr:#010x}"),
                    pccsr_chan0 = format!("{pccsr_chan0:#010x}"),
                    pbdma_0_intr = format!("{:#010x}", pbdma_intr[0]),
                    pbdma_0_hce = format!("{:#010x}", pbdma_hce[0]),
                    pbdma_0_idle = format!("{:#010x}", pbdma_idle[0]),
                    pbdma_1_intr = format!("{:#010x}", pbdma_intr[1]),
                    pbdma_1_hce = format!("{:#010x}", pbdma_hce[1]),
                    pbdma_1_idle = format!("{:#010x}", pbdma_idle[1]),
                    pbdma_2_intr = format!("{:#010x}", pbdma_intr[2]),
                    pbdma_2_hce = format!("{:#010x}", pbdma_hce[2]),
                    pbdma_2_idle = format!("{:#010x}", pbdma_idle[2]),
                    pbdma_3_intr = format!("{:#010x}", pbdma_intr[3]),
                    pbdma_3_hce = format!("{:#010x}", pbdma_hce[3]),
                    pbdma_3_idle = format!("{:#010x}", pbdma_idle[3]),
                    pbdma_runl_map_0 = format!("{:#010x}", pbdma_runl_map[0]),
                    pbdma_runl_map_1 = format!("{:#010x}", pbdma_runl_map[1]),
                    pbdma_runl_map_2 = format!("{:#010x}", pbdma_runl_map[2]),
                    pbdma_runl_map_3 = format!("{:#010x}", pbdma_runl_map[3]),
                    mmu_fault_status = format!("{mmu_fault_status:#010x}"),
                    mmu_hubtlb_err = format!("{mmu_hubtlb_err:#010x}"),
                    priv_ring_intr = format!("{priv_ring_intr:#010x}"),
                    "Fence timeout: GPFIFO completion did not complete within timeout"
                );
                return Err(DriverError::FenceTimeout {
                    ms: SYNC_TIMEOUT.as_millis() as u64,
                });
            }

            std::hint::spin_loop();
            std::thread::sleep(POLL_INTERVAL);
        }
    }

    /// Inner dispatch — builds QMD + pushbuf, submits via GPFIFO.
    fn dispatch_inner(
        &mut self,
        shader: &[u8],
        buffers: &[BufferHandle],
        dims: DispatchDims,
        info: &ShaderInfo,
        temps: &mut Vec<BufferHandle>,
    ) -> DriverResult<()> {
        let (shader_handle, shader_iova) = self.alloc_dma(shader.len())?;
        temps.push(shader_handle);
        self.upload(shader_handle, 0, shader)?;

        // Build CBUF descriptor for group 0 (same layout as NvDevice).
        let desc_entry_size = 16_usize;
        let desc_buf_size = desc_entry_size * buffers.len().max(1);
        let (desc_handle, desc_iova) = self.alloc_dma(desc_buf_size)?;
        temps.push(desc_handle);

        let mut desc_data = vec![0u8; desc_buf_size];
        for (i, bh) in buffers.iter().enumerate() {
            if let Some(buf) = self.buffers.get(&bh.0) {
                let va = buf.dma.iova();
                let sz = u32::try_from(buf.size).unwrap_or(u32::MAX);
                let off = i * 8;
                #[expect(
                    clippy::cast_possible_truncation,
                    reason = "deliberate split into 32-bit halves"
                )]
                {
                    desc_data[off..off + 4].copy_from_slice(&(va as u32).to_le_bytes());
                    desc_data[off + 4..off + 8].copy_from_slice(&((va >> 32) as u32).to_le_bytes());
                }
                let sz_off = off + 8;
                if sz_off + 4 <= desc_data.len() {
                    desc_data[sz_off..sz_off + 4].copy_from_slice(&sz.to_le_bytes());
                }
            }
        }
        self.upload(desc_handle, 0, &desc_data)?;

        let cbufs = vec![qmd::CbufBinding {
            index: 0,
            addr: desc_iova,
            size: u32::try_from(desc_buf_size).unwrap_or(u32::MAX),
        }];

        let qmd_params = qmd::QmdParams {
            shader_va: shader_iova,
            grid: dims,
            workgroup: info.workgroup,
            gpr_count: info.gpr_count.max(4),
            shared_mem_bytes: info.shared_mem_bytes,
            barrier_count: info.barrier_count,
            cbufs,
        };
        let qmd_words = qmd::build_qmd_for_sm(self.sm_version, &qmd_params);
        let qmd_bytes: &[u8] = bytemuck::cast_slice(&qmd_words);

        let (qmd_handle, qmd_iova) = self.alloc_dma(qmd_bytes.len())?;
        temps.push(qmd_handle);
        self.upload(qmd_handle, 0, qmd_bytes)?;

        let local_mem_window = if self.sm_version >= 70 {
            LOCAL_MEM_WINDOW_VOLTA
        } else {
            LOCAL_MEM_WINDOW_LEGACY
        };
        let pb = PushBuf::compute_dispatch(self.compute_class, qmd_iova, local_mem_window);
        let pb_bytes = pb.as_bytes();

        let (pb_handle, pb_iova) = self.alloc_dma(pb_bytes.len())?;
        temps.push(pb_handle);
        self.upload(pb_handle, 0, pb_bytes)?;

        let pb_size = u32::try_from(pb_bytes.len())
            .map_err(|_| DriverError::platform_overflow("pushbuf size fits in u32"))?;
        self.submit_pushbuf(pb_iova, pb_size)?;

        Ok(())
    }
}

impl ComputeDevice for NvVfioComputeDevice {
    fn alloc(&mut self, size: u64, _domain: MemoryDomain) -> DriverResult<BufferHandle> {
        let size_usize = usize::try_from(size).map_err(|_| DriverError::AllocFailed {
            size,
            domain: _domain,
        })?;
        let (handle, _iova) = self.alloc_dma(size_usize)?;
        Ok(handle)
    }

    fn free(&mut self, handle: BufferHandle) -> DriverResult<()> {
        self.buffers
            .remove(&handle.0)
            .ok_or(DriverError::BufferNotFound(handle))?;
        Ok(())
    }

    fn upload(&mut self, handle: BufferHandle, offset: u64, data: &[u8]) -> DriverResult<()> {
        let buf = self
            .buffers
            .get_mut(&handle.0)
            .ok_or(DriverError::BufferNotFound(handle))?;
        let off = offset as usize;
        let slice = buf.dma.as_mut_slice();
        if off + data.len() > slice.len() {
            return Err(DriverError::SubmitFailed(
                "upload exceeds buffer bounds".into(),
            ));
        }
        slice[off..off + data.len()].copy_from_slice(data);
        Ok(())
    }

    fn readback(&self, handle: BufferHandle, offset: u64, len: usize) -> DriverResult<Vec<u8>> {
        let buf = self
            .buffers
            .get(&handle.0)
            .ok_or(DriverError::BufferNotFound(handle))?;
        let off = offset as usize;
        let slice = buf.dma.as_slice();
        if off + len > slice.len() {
            return Err(DriverError::SubmitFailed(
                "readback exceeds buffer bounds".into(),
            ));
        }
        Ok(slice[off..off + len].to_vec())
    }

    fn dispatch(
        &mut self,
        shader: &[u8],
        buffers: &[BufferHandle],
        dims: DispatchDims,
        info: &ShaderInfo,
    ) -> DriverResult<()> {
        let mut temps: Vec<BufferHandle> = Vec::with_capacity(4);
        let result = self.dispatch_inner(shader, buffers, dims, info, &mut temps);
        if result.is_ok() {
            self.inflight.extend(temps);
        } else {
            for h in temps {
                let _ = self.free(h);
            }
        }
        result
    }

    fn sync(&mut self) -> DriverResult<()> {
        self.poll_gpfifo_completion()?;
        let inflight = std::mem::take(&mut self.inflight);
        for handle in inflight {
            let _ = self.free(handle);
        }
        Ok(())
    }
}

impl Drop for NvVfioComputeDevice {
    fn drop(&mut self) {
        let inflight = std::mem::take(&mut self.inflight);
        for h in inflight {
            let _ = self.free(h);
        }
        let handles: Vec<BufferHandle> = self.buffers.keys().map(|k| BufferHandle(*k)).collect();
        for h in handles {
            let _ = self.free(h);
        }
    }
}

impl std::fmt::Debug for NvVfioComputeDevice {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("NvVfioComputeDevice")
            .field("sm_version", &self.sm_version)
            .field("compute_class", &self.compute_class)
            .field("buffers", &self.buffers.len())
            .field("gpfifo_put", &self.gpfifo_put)
            .finish_non_exhaustive()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gpfifo_entry_encoding() {
        let addr = 0x1000_u64;
        let size = 64_u32; // 16 dwords
        let entry = gpfifo::encode_entry(addr, size);
        // DW0: VA[31:2] at natural positions, TYPE=0
        let dw0 = entry as u32;
        assert_eq!(dw0, 0x1000, "DW0 = addr with type=0");
        // DW1: LENGTH in [30:10], VA[39:32] in [7:0]
        let dw1 = (entry >> 32) as u32;
        let len_field = (dw1 >> 10) & 0x1F_FFFF;
        assert_eq!(len_field, 16, "length = 16 dwords");
        // Recover full address
        let recovered = (dw0 as u64 & 0xFFFF_FFFC) | ((dw1 as u64 & 0xFF) << 32);
        assert_eq!(recovered, addr);
    }

    #[test]
    fn gpfifo_entry_zero() {
        let entry = gpfifo::encode_entry(0, 0);
        assert_eq!(entry, 0);
    }

    #[test]
    fn gpfifo_ring_size() {
        assert_eq!(gpfifo::RING_SIZE, 128 * 8);
    }

    #[test]
    fn gpfifo_entry_large_addr() {
        let addr = 0x10_0000_0000_u64;
        let size = 256_u32; // 64 dwords
        let entry = gpfifo::encode_entry(addr, size);
        let dw0 = entry as u32;
        let dw1 = (entry >> 32) as u32;
        let recovered = (dw0 as u64 & 0xFFFF_FFFC) | ((dw1 as u64 & 0xFF) << 32);
        assert_eq!(recovered, addr);
        let len_field = (dw1 >> 10) & 0x1F_FFFF;
        assert_eq!(len_field, 64, "length = 64 dwords");
    }

    #[test]
    fn iova_constants_non_overlapping() {
        const { assert!(GPFIFO_IOVA < USERD_IOVA) };
        const { assert!(USERD_IOVA + 4096 <= USER_IOVA_BASE) };
    }

    #[test]
    fn open_nonexistent_device() {
        let result = NvVfioComputeDevice::open("9999:99:99.9", 86, 0xC6C0);
        assert!(result.is_err());
    }

    #[test]
    fn local_mem_window_volta() {
        assert_eq!(LOCAL_MEM_WINDOW_VOLTA, 0xFF00_0000_0000_0000);
    }

    #[test]
    fn local_mem_window_legacy() {
        assert_eq!(LOCAL_MEM_WINDOW_LEGACY, 0xFF00_0000);
    }
}
