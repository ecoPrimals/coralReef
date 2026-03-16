// SPDX-License-Identifier: AGPL-3.0-only
//! DeviceSlot — persistent ownership of a PCIe device.
//!
//! Each slot manages one GPU/accelerator from boot to shutdown.
//! It tracks the current driver personality, power state, VRAM
//! health, and provides the VFIO fd for toadStool consumers.

use crate::config::DeviceConfig;
use std::collections::BTreeMap;

#[derive(Debug, Clone, PartialEq)]
pub enum Personality {
    Vfio { group_id: u32 },
    Nouveau { drm_card: Option<String> },
    Amdgpu { drm_card: Option<String> },
    #[allow(dead_code)]
    NvidiaProprietary,
    Unbound,
}

impl std::fmt::Display for Personality {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Vfio { group_id } => write!(f, "vfio (group {group_id})"),
            Self::Nouveau { drm_card } => {
                write!(f, "nouveau")?;
                if let Some(card) = drm_card {
                    write!(f, " ({card})")?;
                }
                Ok(())
            }
            Self::Amdgpu { drm_card } => {
                write!(f, "amdgpu")?;
                if let Some(card) = drm_card {
                    write!(f, " ({card})")?;
                }
                Ok(())
            }
            Self::NvidiaProprietary => write!(f, "nvidia"),
            Self::Unbound => write!(f, "unbound"),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum PowerState {
    D0,
    D3Hot,
    D3Cold,
    Unknown,
}

impl std::fmt::Display for PowerState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::D0 => write!(f, "D0"),
            Self::D3Hot => write!(f, "D3hot"),
            Self::D3Cold => write!(f, "D3cold"),
            Self::Unknown => write!(f, "unknown"),
        }
    }
}

#[derive(Debug, Clone)]
pub struct DeviceHealth {
    pub vram_alive: bool,
    pub boot0: u32,
    pub pmc_enable: u32,
    pub power: PowerState,
    #[allow(dead_code)]
    pub pci_link_width: Option<u8>,
    pub domains_alive: usize,
    pub domains_faulted: usize,
}

pub struct DeviceSlot {
    pub config: DeviceConfig,
    pub bdf: String,
    pub personality: Personality,
    pub health: DeviceHealth,
    #[allow(dead_code)]
    pub vendor_id: u16,
    #[allow(dead_code)]
    pub device_id: u16,
    pub chip_name: String,
    vfio_device: Option<coral_driver::nv::RawVfioDevice>,
    register_snapshot: BTreeMap<usize, u32>,
}

impl DeviceSlot {
    pub fn new(config: DeviceConfig) -> Self {
        let bdf = config.bdf.clone();
        let (vendor_id, device_id) = read_pci_ids(&bdf);
        let chip_name = identify_chip(vendor_id, device_id);

        Self {
            config,
            bdf,
            personality: Personality::Unbound,
            health: DeviceHealth {
                vram_alive: false,
                boot0: 0,
                pmc_enable: 0,
                power: PowerState::Unknown,
                pci_link_width: None,
                domains_alive: 0,
                domains_faulted: 0,
            },
            vendor_id,
            device_id,
            chip_name,
            vfio_device: None,
            register_snapshot: BTreeMap::new(),
        }
    }

    pub fn has_vfio(&self) -> bool {
        self.vfio_device.is_some()
    }

    /// Bind device to the configured boot personality and take ownership.
    pub fn activate(&mut self) -> Result<(), String> {
        let target = self.config.boot_personality.clone();
        tracing::info!(bdf = %self.bdf, personality = %target, "activating device");

        self.refresh_power_state();

        match target.as_str() {
            "vfio" => self.bind_vfio()?,
            "nouveau" => self.bind_driver("nouveau")?,
            "amdgpu" => self.bind_driver("amdgpu")?,
            other => return Err(format!("unknown personality: {other}")),
        }

        self.check_health();
        tracing::info!(
            bdf = %self.bdf,
            personality = %self.personality,
            chip = %self.chip_name,
            vram = self.health.vram_alive,
            power = %self.health.power,
            "device activated"
        );

        Ok(())
    }

    /// Hot-swap to a new driver personality.
    pub fn swap(&mut self, target: &str) -> Result<(), String> {
        tracing::info!(bdf = %self.bdf, from = %self.personality, to = %target, "swapping personality");

        // Step 1: snapshot current state
        if self.has_vfio() {
            self.snapshot_registers();
        }

        // Step 2: release current personality
        self.release()?;

        // Step 3: bind new personality
        match target {
            "vfio" | "vfio-pci" => self.bind_vfio()?,
            "nouveau" => self.bind_driver("nouveau")?,
            "amdgpu" => self.bind_driver("amdgpu")?,
            "unbound" => { /* already unbound from release() */ }
            other => return Err(format!("unknown personality: {other}")),
        }

        // Step 4: verify health
        self.check_health();
        tracing::info!(
            bdf = %self.bdf,
            personality = %self.personality,
            vram = self.health.vram_alive,
            "swap complete"
        );

        Ok(())
    }

    /// Release current personality (unbind driver, drop VFIO fd).
    ///
    /// CRITICAL: we must *close* the VFIO fd (not leak it) before unbind.
    /// Leaking prevents the kernel from releasing the VFIO group, which
    /// blocks sysfs unbind indefinitely. The PM reset from fd close is
    /// accepted — the state vault snapshot preserves register state for
    /// restoration after re-binding.
    fn release(&mut self) -> Result<(), String> {
        // Drop VFIO device — this closes the fd and triggers PM reset,
        // but frees the VFIO group so unbind can proceed.
        drop(self.vfio_device.take());

        // Pin power before unbind to prevent D3 transition
        sysfs_write(&format!("/sys/bus/pci/devices/{}/power/control", self.bdf), "on");

        // Unbind from current driver
        let drv_path = format!("/sys/bus/pci/devices/{}/driver/unbind", self.bdf);
        sysfs_write(&drv_path, &self.bdf);
        std::thread::sleep(std::time::Duration::from_millis(500));

        // Keep power pinned
        sysfs_write(&format!("/sys/bus/pci/devices/{}/power/control", self.bdf), "on");
        self.personality = Personality::Unbound;
        Ok(())
    }

    fn bind_vfio(&mut self) -> Result<(), String> {
        // Set driver override
        sysfs_write(
            &format!("/sys/bus/pci/devices/{}/driver_override", self.bdf),
            "vfio-pci",
        );

        // Bind
        sysfs_write("/sys/bus/pci/drivers/vfio-pci/bind", &self.bdf);
        std::thread::sleep(std::time::Duration::from_millis(500));

        // Pin D0
        sysfs_write(&format!("/sys/bus/pci/devices/{}/power/control", self.bdf), "on");
        sysfs_write(&format!("/sys/bus/pci/devices/{}/d3cold_allowed", self.bdf), "0");

        // Open VFIO device
        let group_id = read_iommu_group(&self.bdf);
        match coral_driver::nv::RawVfioDevice::open(&self.bdf) {
            Ok(dev) => {
                self.vfio_device = Some(dev);
                self.personality = Personality::Vfio { group_id };
                Ok(())
            }
            Err(e) => {
                self.personality = Personality::Vfio { group_id };
                Err(format!("VFIO open failed: {e}"))
            }
        }
    }

    fn bind_driver(&mut self, driver: &str) -> Result<(), String> {
        sysfs_write(
            &format!("/sys/bus/pci/devices/{}/driver_override", self.bdf),
            "",
        );
        sysfs_write(
            &format!("/sys/bus/pci/drivers/{driver}/bind"),
            &self.bdf,
        );
        std::thread::sleep(std::time::Duration::from_secs(3));

        sysfs_write(&format!("/sys/bus/pci/devices/{}/power/control", self.bdf), "on");

        let drm_card = find_drm_card(&self.bdf);
        self.personality = match driver {
            "nouveau" => Personality::Nouveau { drm_card },
            "amdgpu" => Personality::Amdgpu { drm_card },
            _ => Personality::Unbound,
        };
        Ok(())
    }

    /// Take a snapshot of key registers (for state preservation across swaps).
    fn snapshot_registers(&mut self) {
        let Some(dev) = &self.vfio_device else { return };
        self.register_snapshot.clear();

        let offsets: &[usize] = &[
            0x000000, 0x000200, 0x000204, // BOOT0, PMC_ENABLE, PMC_DEV_ENABLE
            0x002004, 0x002100, 0x002200, // PFIFO
            0x100000, 0x100800, 0x100C80, // PFB, FBHUB, PFB_NISO
            0x10A000, 0x10A040, 0x10A044, // PMU FALCON
            0x137000, 0x137050, 0x137100, // PCLOCK, NVPLL, MEMPLL
            0x9A0000, 0x17E200, 0x300000, // FBPA0, LTC0, PROM
        ];

        for &off in offsets {
            if let Ok(val) = dev.bar0.read_u32(off) {
                self.register_snapshot.insert(off, val);
            }
        }
        tracing::debug!(bdf = %self.bdf, regs = self.register_snapshot.len(), "snapshot taken");
    }

    /// Check device health by probing key registers.
    pub fn check_health(&mut self) {
        self.refresh_power_state();

        if !self.has_vfio() {
            // Can't probe without VFIO — just report power state
            self.health.vram_alive = false;
            self.health.domains_alive = 0;
            self.health.domains_faulted = 0;
            return;
        }

        let dev = self.vfio_device.as_ref().unwrap();
        let r = |off: usize| dev.bar0.read_u32(off).unwrap_or(0xDEAD_DEAD);

        self.health.boot0 = r(0x000000);
        self.health.pmc_enable = r(0x000200);

        // VRAM test via PRAMIN sentinel
        let pramin_val = r(0x700000);
        self.health.vram_alive = pramin_val != 0xDEAD_DEAD
            && (pramin_val >> 16) != 0xBAD0
            && pramin_val != 0xFFFF_FFFF;

        // Domain health
        let domains: &[(usize, &str)] = &[
            (0x000200, "PMC"), (0x002004, "PFIFO"), (0x100000, "PFB"),
            (0x100800, "FBHUB"), (0x10A000, "PMU"), (0x17E200, "LTC0"),
            (0x9A0000, "FBPA0"), (0x137050, "NVPLL"), (0x700000, "PRAMIN"),
        ];

        let mut alive = 0;
        let mut faulted = 0;
        for &(off, _) in domains {
            let val = r(off);
            if val == 0xDEAD_DEAD || val == 0xFFFF_FFFF {
                faulted += 1;
            } else if (val >> 16) == 0xBADF || (val >> 16) == 0xBAD0 || (val >> 16) == 0xBAD1 {
                faulted += 1;
            } else {
                alive += 1;
            }
        }
        self.health.domains_alive = alive;
        self.health.domains_faulted = faulted;
    }

    fn refresh_power_state(&mut self) {
        let path = format!("/sys/bus/pci/devices/{}/power_state", self.bdf);
        self.health.power = match std::fs::read_to_string(&path) {
            Ok(s) => match s.trim() {
                "D0" => PowerState::D0,
                "D3hot" => PowerState::D3Hot,
                "D3cold" => PowerState::D3Cold,
                _ => PowerState::Unknown,
            },
            Err(_) => PowerState::Unknown,
        };
    }
}

// ── Sysfs Helpers ────────────────────────────────────────────────────────

pub(crate) fn sysfs_write(path: &str, value: &str) {
    // Try direct write first (if udev rules set permissions)
    if std::fs::write(path, value).is_ok() {
        return;
    }
    // Fall back to sudo tee (passwordless via sudoers)
    let _ = std::process::Command::new("sudo")
        .args(["-n", "/usr/bin/tee", path])
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .and_then(|mut child| {
            use std::io::Write;
            if let Some(stdin) = child.stdin.as_mut() {
                stdin.write_all(value.as_bytes())?;
            }
            child.wait()
        });
}

fn read_pci_ids(bdf: &str) -> (u16, u16) {
    let vendor = std::fs::read_to_string(format!("/sys/bus/pci/devices/{bdf}/vendor"))
        .ok()
        .and_then(|s| u16::from_str_radix(s.trim().trim_start_matches("0x"), 16).ok())
        .unwrap_or(0);
    let device = std::fs::read_to_string(format!("/sys/bus/pci/devices/{bdf}/device"))
        .ok()
        .and_then(|s| u16::from_str_radix(s.trim().trim_start_matches("0x"), 16).ok())
        .unwrap_or(0);
    (vendor, device)
}

fn read_iommu_group(bdf: &str) -> u32 {
    std::fs::read_link(format!("/sys/bus/pci/devices/{bdf}/iommu_group"))
        .ok()
        .and_then(|p| p.file_name()?.to_str()?.parse().ok())
        .unwrap_or(0)
}

fn identify_chip(vendor: u16, device: u16) -> String {
    match (vendor, device) {
        (0x10de, 0x1d81) => "GV100 (Titan V)".into(),
        (0x10de, 0x1db1) => "GV100GL (V100)".into(),
        (0x10de, 0x2204) => "GA102 (RTX 3090)".into(),
        (0x10de, 0x2d05) => "GB206 (RTX 5060)".into(),
        (0x1002, 0x66a0) => "Vega 20 (MI50)".into(),
        (0x1002, 0x66a1) => "Vega 20 (MI60)".into(),
        (v, d) => format!("{v:#06x}:{d:#06x}"),
    }
}

fn find_drm_card(bdf: &str) -> Option<String> {
    let drm_dir = format!("/sys/bus/pci/devices/{bdf}/drm");
    let entries = std::fs::read_dir(&drm_dir).ok()?;
    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().to_string();
        if name.starts_with("card") {
            return Some(format!("/dev/dri/{name}"));
        }
    }
    None
}
