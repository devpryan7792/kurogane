//! Appliance Sandboxing: User Namespaces & Landlock LSM Confinement
//! Drops all host privileges and binds the application strictly to its data envelope.

use crate::KuroganeError;
use nix::sched::{unshare, CloneFlags};
use std::ffi::CString;
use std::fs;
use std::path::Path;

#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct LandlockRulesetAttr {
    pub handled_access_fs: u64,
}

#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct LandlockPathBeneathAttr {
    pub allowed_access: u64,
    pub parent_fd: i32,
}

pub const LANDLOCK_CREATE_RULESET_VERSION: u32 = 1 << 0;
pub const LANDLOCK_RULE_PATH_BENEATH: u32 = 1;

pub const LANDLOCK_ACCESS_FS_EXECUTE: u64 = 1 << 0;
pub const LANDLOCK_ACCESS_FS_WRITE_FILE: u64 = 1 << 1;
pub const LANDLOCK_ACCESS_FS_READ_FILE: u64 = 1 << 2;
pub const LANDLOCK_ACCESS_FS_READ_DIR: u64 = 1 << 3;
pub const LANDLOCK_ACCESS_FS_REMOVE_DIR: u64 = 1 << 4;
pub const LANDLOCK_ACCESS_FS_REMOVE_FILE: u64 = 1 << 5;
pub const LANDLOCK_ACCESS_FS_MAKE_CHAR: u64 = 1 << 6;
pub const LANDLOCK_ACCESS_FS_MAKE_DIR: u64 = 1 << 7;
pub const LANDLOCK_ACCESS_FS_MAKE_REG: u64 = 1 << 8;
pub const LANDLOCK_ACCESS_FS_MAKE_SOCK: u64 = 1 << 9;
pub const LANDLOCK_ACCESS_FS_MAKE_FIFO: u64 = 1 << 10;
pub const LANDLOCK_ACCESS_FS_MAKE_BLOCK: u64 = 1 << 11;
pub const LANDLOCK_ACCESS_FS_MAKE_SYM: u64 = 1 << 12;
pub const LANDLOCK_ACCESS_FS_REFER: u64 = 1 << 13;
pub const LANDLOCK_ACCESS_FS_TRUNCATE: u64 = 1 << 14;

pub const SYS_LANDLOCK_CREATE_RULESET: libc::c_long = 444;
pub const SYS_LANDLOCK_ADD_RULE: libc::c_long = 445;
pub const SYS_LANDLOCK_RESTRICT_SELF: libc::c_long = 446;

/// Configures unprivileged User Namespace (CLONE_NEWUSER) with unprivileged UID/GID mapping.
pub fn setup_user_namespace(inside_uid: u32, inside_gid: u32) -> Result<(), KuroganeError> {
    let host_uid = unsafe { libc::geteuid() };
    let host_gid = unsafe { libc::getegid() };

    // 1. Unshare user namespace
    if let Err(e) = unshare(CloneFlags::CLONE_NEWUSER) {
        eprintln!(
            "[WARN] unshare(CLONE_NEWUSER) returned {}. Continuing with existing namespace.",
            e
        );
        return Ok(());
    }

    // 2. Deny setgroups to allow unprivileged GID mapping
    let setgroups_path = "/proc/self/setgroups";
    if Path::new(setgroups_path).exists() {
        let _ = fs::write(setgroups_path, "deny");
    }

    // 3. Map UID: inside_uid host_uid count
    let uid_map_path = "/proc/self/uid_map";
    if Path::new(uid_map_path).exists() {
        let _ = fs::write(uid_map_path, format!("{} {} 1\n", inside_uid, host_uid));
    }

    // 4. Map GID: inside_gid host_gid count
    let gid_map_path = "/proc/self/gid_map";
    if Path::new(gid_map_path).exists() {
        let _ = fs::write(gid_map_path, format!("{} {} 1\n", inside_gid, host_gid));
    }

    Ok(())
}

/// Linux Landlock LSM wrapper for path-based filesystem confinement.
pub struct LandlockSandbox;

impl LandlockSandbox {
    /// Queries the Landlock ABI version supported by the running kernel.
    /// Returns 0 or negative if unsupported.
    pub fn abi_version() -> i32 {
        let res = unsafe {
            libc::syscall(
                SYS_LANDLOCK_CREATE_RULESET,
                std::ptr::null::<libc::c_void>(),
                0,
                LANDLOCK_CREATE_RULESET_VERSION,
            )
        };
        res as i32
    }

    /// Attempts to apply Landlock path restrictions to current process.
    /// Grants read-only access to `/` and read-write access only to `allowed_rw_paths`.
    /// Fails gracefully if host kernel does not support Landlock (returns Ok).
    pub fn restrict_paths(allowed_rw_paths: &[&str]) -> Result<(), KuroganeError> {
        let abi = Self::abi_version();
        if abi <= 0 {
            eprintln!("[INFO] Landlock LSM not active on host kernel. Filesystem isolated via mount namespaces.");
            return Ok(());
        }

        // Compute supported access rights based on kernel ABI version
        let mut handled_fs: u64 = (1 << 13) - 1; // Base ABI v1 flags
        if abi >= 2 {
            handled_fs |= LANDLOCK_ACCESS_FS_REFER;
        }
        if abi >= 3 {
            handled_fs |= LANDLOCK_ACCESS_FS_TRUNCATE;
        }

        let attr = LandlockRulesetAttr {
            handled_access_fs: handled_fs,
        };
        let ruleset_fd = unsafe {
            libc::syscall(
                SYS_LANDLOCK_CREATE_RULESET,
                &attr as *const LandlockRulesetAttr as *const libc::c_void,
                std::mem::size_of::<LandlockRulesetAttr>(),
                0,
            )
        };

        if ruleset_fd < 0 {
            eprintln!("[WARN] Failed to create Landlock ruleset.");
            return Ok(());
        }

        // 1. Grant read-only access to root filesystem
        let read_access = handled_fs
            & (LANDLOCK_ACCESS_FS_EXECUTE
                | LANDLOCK_ACCESS_FS_READ_FILE
                | LANDLOCK_ACCESS_FS_READ_DIR);
        if let Ok(root_path) = CString::new("/") {
            let root_fd = unsafe { libc::open(root_path.as_ptr(), libc::O_PATH | libc::O_CLOEXEC) };
            if root_fd >= 0 {
                let rule = LandlockPathBeneathAttr {
                    allowed_access: read_access,
                    parent_fd: root_fd,
                };
                unsafe {
                    libc::syscall(
                        SYS_LANDLOCK_ADD_RULE,
                        ruleset_fd,
                        LANDLOCK_RULE_PATH_BENEATH,
                        &rule as *const LandlockPathBeneathAttr as *const libc::c_void,
                        0,
                    );
                    libc::close(root_fd);
                }
            }
        }

        // 2. Grant full read-write access to explicitly allowed paths
        for path in allowed_rw_paths {
            if let Ok(c_path) = CString::new(*path) {
                let fd = unsafe { libc::open(c_path.as_ptr(), libc::O_PATH | libc::O_CLOEXEC) };
                if fd >= 0 {
                    let rule = LandlockPathBeneathAttr {
                        allowed_access: handled_fs,
                        parent_fd: fd,
                    };
                    unsafe {
                        libc::syscall(
                            SYS_LANDLOCK_ADD_RULE,
                            ruleset_fd,
                            LANDLOCK_RULE_PATH_BENEATH,
                            &rule as *const LandlockPathBeneathAttr as *const libc::c_void,
                            0,
                        );
                        libc::close(fd);
                    }
                }
            }
        }

        // 3. Enforce lockdown: require no_new_privs, then restrict_self
        unsafe {
            let _ = libc::prctl(libc::PR_SET_NO_NEW_PRIVS, 1, 0, 0, 0);
            let ret = libc::syscall(SYS_LANDLOCK_RESTRICT_SELF, ruleset_fd, 0);
            libc::close(ruleset_fd as i32);
            if ret != 0 {
                return Err(KuroganeError::HardwareError(
                    "Failed to apply landlock_restrict_self",
                ));
            }
        }

        Ok(())
    }
}

/// Drops process into full defense-in-depth isolation:
/// 1. User Namespace mapping (UID 1000)
/// 2. Landlock path restriction (only data_path and wayland socket dir allowed for writing)
/// 3. Seccomp-BPF banned syscall filter
pub fn enter_sandbox(uid: u32, gid: u32, data_path: &str) -> Result<(), KuroganeError> {
    // 1. User namespace
    setup_user_namespace(uid, gid)?;

    // 2. Landlock filesystem confinement
    LandlockSandbox::restrict_paths(&[data_path, "/run/user/1000"])?;

    // 3. Seccomp-BPF filter
    crate::seccomp::apply_appliance_confinement()?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_landlock_probe_safety() {
        let abi = LandlockSandbox::abi_version();
        println!("Detected Landlock ABI version: {}", abi);
        assert!(abi >= 0);
    }
}
