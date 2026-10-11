pub mod event_loop;
pub mod hardware;
pub mod sandbox;
pub mod seccomp;
pub mod signals;
pub mod storage;
pub mod timer;
pub mod wayland_engine;
pub mod wayland_proxy;
pub mod wayland_wire;

/// Strict error types for Kurogane PID 1 operations.
#[derive(Debug)]
pub enum KuroganeError {
    VfsMountError(&'static str, nix::errno::Errno),
    CgroupError(&'static str, std::io::Error),
    HardwareError(&'static str),
    WaylandProxyError(&'static str),
    TeardownError(&'static str),
    VerityVerificationError(String),
    MemfdSealError(&'static str),
    WireFramingError(&'static str),
    SandboxError(&'static str),
    StorageError(String),
}

impl PartialEq for KuroganeError {
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (Self::VfsMountError(s1, e1), Self::VfsMountError(s2, e2)) => s1 == s2 && e1 == e2,
            (Self::CgroupError(s1, e1), Self::CgroupError(s2, e2)) => {
                s1 == s2 && e1.kind() == e2.kind()
            }
            (Self::HardwareError(s1), Self::HardwareError(s2)) => s1 == s2,
            (Self::WaylandProxyError(s1), Self::WaylandProxyError(s2)) => s1 == s2,
            (Self::TeardownError(s1), Self::TeardownError(s2)) => s1 == s2,
            (Self::VerityVerificationError(s1), Self::VerityVerificationError(s2)) => s1 == s2,
            (Self::MemfdSealError(s1), Self::MemfdSealError(s2)) => s1 == s2,
            (Self::WireFramingError(s1), Self::WireFramingError(s2)) => s1 == s2,
            (Self::SandboxError(s1), Self::SandboxError(s2)) => s1 == s2,
            (Self::StorageError(s1), Self::StorageError(s2)) => s1 == s2,
            _ => false,
        }
    }
}

/// Unique session identifier for concurrent Wayland client connections.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct SessionId(pub u64);

/// Structured metrics report generated upon supervisor teardown.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TeardownReport {
    pub syncfs_clean: bool,
    pub unmount_clean: bool,
    pub reaped_zombies: usize,
}

/// Declarative sandboxing policy configuration for appliances.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SandboxConfig {
    pub uid: u32,
    pub gid: u32,
    pub allowed_rw_paths: Vec<String>,
    pub memory_limit_mb: u64,
}

/// Cryptographic dm-verity block verification configuration.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VerityConfig {
    pub data_device: std::path::PathBuf,
    pub hash_device: std::path::PathBuf,
    pub root_hash: String,
    pub data_block_size: u32,
    pub hash_block_size: u32,
    pub data_blocks: u64,
    pub salt: String,
}

impl VerityConfig {
    pub fn new<P: Into<std::path::PathBuf>>(data_device: P, root_hash: impl Into<String>) -> Self {
        let data_path = data_device.into();
        let hash_path = data_path.clone();
        Self {
            data_device: data_path,
            hash_device: hash_path,
            root_hash: root_hash.into(),
            data_block_size: 4096,
            hash_block_size: 4096,
            data_blocks: 0,
            salt: String::new(),
        }
    }
}

/// Policy for verifying and enforcing memory seals on shared buffers transferred via SCM_RIGHTS.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BufferSealPolicy {
    /// Reject any memfd buffer missing F_SEAL_SHRINK | F_SEAL_GROW
    RejectUnsealed,
    /// Automatically apply F_SEAL_SHRINK | F_SEAL_GROW if not already sealed
    AutoApplySeals,
    /// Pass-through without enforcement (unrestricted mode)
    Disabled,
}

/// The deterministic state machine for the Micro-Supervisor.
#[derive(Debug, PartialEq)]
pub enum SupervisorState {
    VfsAssembly,
    CgroupInit,
    HardwareArbitration,
    AudioInit,
    CartridgeMount,
    Execution,
    Recovery(KuroganeError),
}

impl SupervisorState {
    /// Deterministic transition step for state machine.
    /// On success, advances to next state in sequence.
    /// On error, transitions immediately to Recovery carrying the KuroganeError.
    pub fn step(self, result: Result<(), KuroganeError>) -> SupervisorState {
        match result {
            Err(e) => SupervisorState::Recovery(e),
            Ok(()) => match self {
                SupervisorState::VfsAssembly => SupervisorState::CgroupInit,
                SupervisorState::CgroupInit => SupervisorState::HardwareArbitration,
                SupervisorState::HardwareArbitration => SupervisorState::AudioInit,
                SupervisorState::AudioInit => SupervisorState::CartridgeMount,
                SupervisorState::CartridgeMount => SupervisorState::Execution,
                SupervisorState::Execution => SupervisorState::Execution,
                SupervisorState::Recovery(_) => self,
            },
        }
    }
}

/// Configuration baked into the appliance EROFS image at /etc/cartilage/
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SupervisorConfig {
    pub entrypoint: String,
    pub compositor: String,
    pub args: Vec<String>,
    pub mode: String,
    pub quota: String,
    pub acceleration: String,
}

impl Default for SupervisorConfig {
    fn default() -> Self {
        Self {
            entrypoint: "/usr/bin/foot".to_string(),
            compositor: "/usr/bin/cage".to_string(),
            args: Vec::new(),
            mode: "kiosk".to_string(),
            quota: "256M".to_string(),
            acceleration: "auto".to_string(),
        }
    }
}

impl SupervisorConfig {
    /// Loads configuration from an /etc/cartilage/ root directory.
    pub fn load_from_dir<P: AsRef<std::path::Path>>(dir: P) -> Self {
        let dir = dir.as_ref();
        let read_or = |file: &str, default: &str| -> String {
            std::fs::read_to_string(dir.join(file))
                .map(|s| s.trim().to_string())
                .unwrap_or_else(|_| default.to_string())
        };

        let entrypoint = read_or("entrypoint", "/usr/bin/foot");
        let compositor = read_or("compositor", "/usr/bin/cage");
        let mode = read_or("mode", "kiosk");
        let quota = read_or("quota", "256M");
        let acceleration = read_or("acceleration", "auto");

        let args = if let Ok(content) = std::fs::read_to_string(dir.join("args")) {
            content
                .lines()
                .map(|l| l.trim().to_string())
                .filter(|l| !l.is_empty())
                .collect()
        } else {
            Vec::new()
        };

        Self {
            entrypoint,
            compositor,
            args,
            mode,
            quota,
            acceleration,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn test_supervisor_config_defaults_and_parsing() {
        let default_cfg = SupervisorConfig::default();
        assert_eq!(default_cfg.entrypoint, "/usr/bin/foot");
        assert_eq!(default_cfg.compositor, "/usr/bin/cage");

        let temp_dir = std::env::temp_dir().join("kurogane_test_cfg");
        let _ = fs::remove_dir_all(&temp_dir);
        fs::create_dir_all(&temp_dir).expect("create temp dir");

        fs::write(temp_dir.join("entrypoint"), "/usr/bin/chromium\n").unwrap();
        fs::write(temp_dir.join("compositor"), "/usr/bin/sway\n").unwrap();
        fs::write(temp_dir.join("args"), "--no-sandbox\n--kiosk\n").unwrap();

        let parsed = SupervisorConfig::load_from_dir(&temp_dir);
        assert_eq!(parsed.entrypoint, "/usr/bin/chromium");
        assert_eq!(parsed.compositor, "/usr/bin/sway");
        assert_eq!(parsed.args, vec!["--no-sandbox", "--kiosk"]);

        let _ = fs::remove_dir_all(&temp_dir);
    }
}
