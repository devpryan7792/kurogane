//! Hardware & DRM Modesetting Subsystem
//! Interrogates multi-GPU DRM render nodes, detects physical vendors,
//! and automatically falls back to Pixman CPU software rendering (WLR_RENDERER=pixman)
//! if hardware modesetting or firmware blobs are unavailable.

use crate::KuroganeError;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

/// Graphics rendering strategy selected by the hardware arbitrator.
#[derive(Debug, PartialEq, Eq, Clone)]
pub enum RendererChoice {
    HardwareGpu(PathBuf),
    PixmanCpuFallback,
}

/// Known hardware GPU vendors detected via PCI / sysfs vendor IDs.
#[derive(Debug, PartialEq, Eq, Clone)]
pub enum GpuVendor {
    Intel,
    Amd,
    Nvidia,
    VirtIo,
    Unknown(String),
}

pub struct DrmManager;

impl DrmManager {
    /// Checks `/proc/cmdline` for explicit software rendering overrides.
    pub fn check_cmdline_force_software() -> bool {
        if let Ok(cmdline) = fs::read_to_string("/proc/cmdline") {
            for token in cmdline.split_whitespace() {
                if token == "cartilage_software_render=1"
                    || token == "kurogane_render=pixman"
                    || token == "force_software_render"
                    || token == "WLR_RENDERER=pixman"
                {
                    return true;
                }
            }
        }
        false
    }

    /// Identifies GPU vendor from `/sys/class/drm/cardX/device/vendor`.
    pub fn identify_vendor(card_name: &str) -> GpuVendor {
        let vendor_path = format!("/sys/class/drm/{}/device/vendor", card_name);
        if let Ok(vendor_hex) = fs::read_to_string(&vendor_path) {
            let trimmed = vendor_hex.trim().to_lowercase();
            if trimmed.contains("0x8086") {
                return GpuVendor::Intel;
            } else if trimmed.contains("0x1002") {
                return GpuVendor::Amd;
            } else if trimmed.contains("0x10de") {
                return GpuVendor::Nvidia;
            } else if trimmed.contains("0x1af4") {
                return GpuVendor::VirtIo;
            } else {
                return GpuVendor::Unknown(trimmed);
            }
        }
        GpuVendor::Unknown("undetected".to_string())
    }

    /// Scans available DRM cards and interrogates modesetting availability.
    pub fn interrogate_drm() -> RendererChoice {
        // 1. Check for explicit software rendering request
        if Self::check_cmdline_force_software() {
            println!("\x1b[1;33m[hardware]\x1b[0m Explicit software render override detected on kernel cmdline.");
            return RendererChoice::PixmanCpuFallback;
        }

        // 2. Scan primary and secondary DRM cards
        let candidates = ["/dev/dri/card0", "/dev/dri/card1", "/dev/dri/card2"];

        for path_str in candidates {
            let card_path = Path::new(path_str);
            if card_path.exists() {
                // Verify card can be opened for read/write modesetting
                if let Ok(_file) = fs::OpenOptions::new()
                    .read(true)
                    .write(true)
                    .open(card_path)
                {
                    if let Some(card_name) = card_path.file_name().and_then(|n| n.to_str()) {
                        let vendor = Self::identify_vendor(card_name);
                        println!(
                            "\x1b[1;32m[hardware]\x1b[0m Hardware DRM detected: {:?} ({:?})",
                            card_path, vendor
                        );
                    }
                    return RendererChoice::HardwareGpu(card_path.to_path_buf());
                }
            }
        }

        // 3. Fallback to CPU Software Rendering (Pixman)
        println!(
            "\x1b[1;33m[hardware] [WARN]\x1b[0m No usable hardware DRM nodes found. Engaging automatic Pixman CPU software fallback."
        );
        RendererChoice::PixmanCpuFallback
    }

    /// Applies compositor environment variables matching the selected renderer.
    pub fn apply_renderer_env(choice: &RendererChoice) {
        match choice {
            RendererChoice::HardwareGpu(dev) => {
                std::env::set_var("WLR_RENDERER", "auto");
                std::env::set_var("WLR_DRM_DEVICES", dev.to_str().unwrap_or("/dev/dri/card0"));
                std::env::remove_var("LIBGL_ALWAYS_SOFTWARE");
                println!(
                    "\x1b[1;32m[hardware]\x1b[0m Compositor configured with hardware DRM acceleration: {:?}",
                    dev
                );
            }
            RendererChoice::PixmanCpuFallback => {
                std::env::set_var("WLR_RENDERER", "pixman");
                std::env::set_var("LIBGL_ALWAYS_SOFTWARE", "1");
                std::env::set_var("GALLIUM_DRIVER", "llvmpipe");
                std::env::set_var("WLR_RENDER_NO_DIRECT_SCANOUT", "1");
                println!(
                    "\x1b[1;33m[hardware]\x1b[0m Compositor configured with Pixman CPU software rasterization (WLR_RENDERER=pixman)."
                );
            }
        }
    }

    /// Initializes `seatd` daemon and input arbitration.
    pub fn init_seat() -> Result<(), KuroganeError> {
        let seat_sock = Path::new("/run/seatd.sock");
        if !seat_sock.exists() && unsafe { libc::geteuid() == 0 } {
            let _ = Command::new("seatd")
                .arg("-u")
                .arg("1000")
                .arg("-g")
                .arg("1000")
                .spawn();
        }
        std::env::set_var("SEATD_VTBOUND", "0");
        Ok(())
    }

    /// Native sysfs uevent coldplug trigger: walks /sys/bus and /sys/class triggering
    /// synthetic kernel "add" uevents without spawning unmanaged external daemons (zero systemd-udevd).
    pub fn trigger_sysfs_coldplug() {
        if unsafe { libc::geteuid() != 0 } {
            return;
        }

        fn trigger_dir(base: &Path) {
            if let Ok(entries) = fs::read_dir(base) {
                for entry in entries.flatten() {
                    let p = entry.path();
                    if p.is_dir() {
                        let uevent_file = p.join("uevent");
                        if uevent_file.exists() {
                            let _ = fs::write(&uevent_file, b"add\n");
                        }
                    }
                }
            }
        }

        let sys_bus = Path::new("/sys/bus");
        if sys_bus.is_dir() {
            if let Ok(busses) = fs::read_dir(sys_bus) {
                for bus in busses.flatten() {
                    let dev_dir = bus.path().join("devices");
                    if dev_dir.is_dir() {
                        trigger_dir(&dev_dir);
                    }
                }
            }
        }

        let sys_class = Path::new("/sys/class");
        if sys_class.is_dir() {
            trigger_dir(sys_class);
        }
    }

    /// Primary entry point for State 3: Hardware Arbitration.
    pub fn arbitrate_hardware() -> Result<RendererChoice, KuroganeError> {
        // Coldplug hardware devices natively via kernel sysfs ABI (zero daemon dependencies)
        Self::trigger_sysfs_coldplug();

        // Interrogate DRM & select rendering strategy
        let choice = Self::interrogate_drm();
        Self::apply_renderer_env(&choice);

        // Input seat arbitration
        Self::init_seat()?;

        Ok(choice)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_renderer_choice_env_application() {
        // Test Pixman fallback environment setup
        DrmManager::apply_renderer_env(&RendererChoice::PixmanCpuFallback);
        assert_eq!(std::env::var("WLR_RENDERER").unwrap(), "pixman");
        assert_eq!(std::env::var("LIBGL_ALWAYS_SOFTWARE").unwrap(), "1");

        // Test Hardware GPU environment setup
        let fake_card = PathBuf::from("/dev/dri/card0");
        DrmManager::apply_renderer_env(&RendererChoice::HardwareGpu(fake_card));
        assert_eq!(std::env::var("WLR_RENDERER").unwrap(), "auto");
        assert!(std::env::var("LIBGL_ALWAYS_SOFTWARE").is_err());
    }

    #[test]
    fn test_drm_interrogation_safety() {
        // Must never panic on any host system
        let choice = DrmManager::interrogate_drm();
        println!("Interrogated renderer choice: {:?}", choice);
    }

    #[test]
    fn test_trigger_sysfs_coldplug_safety() {
        // Must execute cleanly without panicking
        DrmManager::trigger_sysfs_coldplug();
    }
}
