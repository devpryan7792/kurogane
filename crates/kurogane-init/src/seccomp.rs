//! Seccomp-BPF Kernel Syscall Confinement Engine
//! Compiles and installs strict Linux seccomp-bpf filters to enforce
//! the principle of least privilege on cartridge runtimes.

use crate::KuroganeError;
use libc::{
    c_void, prctl, sock_filter, sock_fprog, BPF_ABS, BPF_JEQ, BPF_JMP, BPF_K, BPF_LD, BPF_RET,
    BPF_W, PR_SET_NO_NEW_PRIVS, PR_SET_SECCOMP, SECCOMP_MODE_FILTER, SECCOMP_RET_ALLOW,
    SECCOMP_RET_KILL_PROCESS,
};

/// Linux AUDIT_ARCH_X86_64 identifier (EM_X86_64 | __AUDIT_ARCH_64BIT | __AUDIT_ARCH_LE)
pub const AUDIT_ARCH_X86_64: u32 = 0xc000_003e;

// Syscall numbers on x86_64
pub const SYS_PTRACE: u32 = 101;
pub const SYS_REBOOT: u32 = 169;
pub const SYS_KEYCTL: u32 = 250;
pub const SYS_BPF: u32 = 321;
pub const SYS_IO_URING_SETUP: u32 = 425;
pub const SYS_IO_URING_ENTER: u32 = 426;
pub const SYS_IO_URING_REGISTER: u32 = 427;
pub const SYS_KEXEC_LOAD: u32 = 246;
pub const SYS_KEXEC_FILE_LOAD: u32 = 320;
pub const SYS_INIT_MODULE: u32 = 175;
pub const SYS_FINIT_MODULE: u32 = 313;
pub const SYS_DELETE_MODULE: u32 = 176;

/// Banned system calls that violate appliance sandboxing invariants.
pub const BANNED_SYSCALLS: &[u32] = &[
    SYS_PTRACE,
    SYS_REBOOT,
    SYS_KEYCTL,
    SYS_BPF,
    SYS_IO_URING_SETUP,
    SYS_IO_URING_ENTER,
    SYS_IO_URING_REGISTER,
    SYS_KEXEC_LOAD,
    SYS_KEXEC_FILE_LOAD,
    SYS_INIT_MODULE,
    SYS_FINIT_MODULE,
    SYS_DELETE_MODULE,
];

#[inline]
fn bpf_stmt(code: u32, k: u32) -> sock_filter {
    sock_filter {
        code: code as u16,
        jt: 0,
        jf: 0,
        k,
    }
}

#[inline]
fn bpf_jump(code: u32, k: u32, jt: u8, jf: u8) -> sock_filter {
    sock_filter {
        code: code as u16,
        jt,
        jf,
        k,
    }
}

/// Compiles a raw BPF bytecode program enforcing strict architecture matching
/// and instant process termination (SECCOMP_RET_KILL_PROCESS) on banned syscalls.
pub fn compile_bpf_filter(banned: &[u32]) -> Vec<sock_filter> {
    let mut filter = vec![
        // 1. Validate architecture is x86_64 (offset 4 of seccomp_data)
        bpf_stmt(BPF_LD | BPF_W | BPF_ABS, 4),
        // If arch == AUDIT_ARCH_X86_64, jump over KILL (skip 1), else proceed to KILL (skip 0)
        bpf_jump(BPF_JMP | BPF_JEQ | BPF_K, AUDIT_ARCH_X86_64, 1, 0),
        bpf_stmt(BPF_RET | BPF_K, SECCOMP_RET_KILL_PROCESS),
        // 2. Load system call number (offset 0 of seccomp_data)
        bpf_stmt(BPF_LD | BPF_W | BPF_ABS, 0),
    ];

    // 3. For each banned syscall, if matched -> SECCOMP_RET_KILL_PROCESS
    for &nr in banned {
        // If nr == banned_nr, jump to kill (next instruction), else skip kill (jump 1)
        filter.push(bpf_jump(BPF_JMP | BPF_JEQ | BPF_K, nr, 0, 1));
        filter.push(bpf_stmt(BPF_RET | BPF_K, SECCOMP_RET_KILL_PROCESS));
    }

    // 4. Default policy: Allow authorized runtime syscalls
    filter.push(bpf_stmt(BPF_RET | BPF_K, SECCOMP_RET_ALLOW));

    filter
}

/// Standard graphical/runtime syscall whitelist (~55 essential syscalls)
pub const STANDARD_GRAPHICAL_WHITELIST: &[u32] = &[
    0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17, 18, 19, 20, 21, 22, 23, 24, 25,
    28, 32, 33, 35, 39, 41, 42, 43, 44, 45, 46, 47, 48, 49, 51, 52, 53, 54, 55, 56, 57, 58, 59, 60,
    61, 62, 72, 73, 74, 75, 78, 79, 80, 96, 102, 104, 107, 108, 131, 158, 202, 213, 217, 219, 228,
    231, 232, 233, 234, 257, 262, 273, 274, 281, 283, 286, 287, 290, 291, 302, 318, 319, 332, 436,
    439,
];

/// Compiles a raw BPF whitelist program: only syscalls in `whitelist` are allowed;
/// all other syscalls return `SECCOMP_RET_KILL_PROCESS`.
pub fn compile_whitelist_filter(whitelist: &[u32]) -> Vec<sock_filter> {
    let mut filter = vec![
        // 1. Validate architecture is x86_64
        bpf_stmt(BPF_LD | BPF_W | BPF_ABS, 4),
        bpf_jump(BPF_JMP | BPF_JEQ | BPF_K, AUDIT_ARCH_X86_64, 1, 0),
        bpf_stmt(BPF_RET | BPF_K, SECCOMP_RET_KILL_PROCESS),
        // 2. Load system call number
        bpf_stmt(BPF_LD | BPF_W | BPF_ABS, 0),
    ];

    // For each whitelisted syscall, if matched -> ALLOW
    for &nr in whitelist {
        filter.push(bpf_jump(BPF_JMP | BPF_JEQ | BPF_K, nr, 0, 1));
        filter.push(bpf_stmt(BPF_RET | BPF_K, SECCOMP_RET_ALLOW));
    }

    // Default policy: if no whitelist match -> KILL
    filter.push(bpf_stmt(BPF_RET | BPF_K, SECCOMP_RET_KILL_PROCESS));

    filter
}

/// Installs the compiled BPF filter on the calling process and all its future descendants.
/// Requires PR_SET_NO_NEW_PRIVS to allow unprivileged execution.
pub fn apply_seccomp_filter(filter: &[sock_filter]) -> Result<(), KuroganeError> {
    // Lock down privileges: cannot regain root or escalate via suid
    let ret = unsafe { prctl(PR_SET_NO_NEW_PRIVS, 1, 0, 0, 0) };
    if ret != 0 {
        return Err(KuroganeError::HardwareError(
            "Failed to set PR_SET_NO_NEW_PRIVS",
        ));
    }

    let prog = sock_fprog {
        len: filter.len() as u16,
        filter: filter.as_ptr() as *mut sock_filter,
    };

    let ret = unsafe {
        prctl(
            PR_SET_SECCOMP,
            SECCOMP_MODE_FILTER,
            &prog as *const sock_fprog as *const c_void,
            0,
            0,
        )
    };

    if ret != 0 {
        return Err(KuroganeError::HardwareError(
            "Failed to install seccomp filter",
        ));
    }

    Ok(())
}

/// Applies standard Kurogane appliance security confinement policy.
pub fn apply_appliance_confinement() -> Result<(), KuroganeError> {
    let filter = compile_bpf_filter(BANNED_SYSCALLS);
    apply_seccomp_filter(&filter)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_bpf_filter_compilation_sanity() {
        let filter = compile_bpf_filter(BANNED_SYSCALLS);
        // Header check (3 instrs) + load nr (1 instr) + banned checks (2 * len) + allow (1 instr)
        let expected_len = 3 + 1 + (BANNED_SYSCALLS.len() * 2) + 1;
        assert_eq!(filter.len(), expected_len);

        // First instruction must load offset 4 (arch)
        assert_eq!(filter[0].code, (BPF_LD | BPF_W | BPF_ABS) as u16);
        assert_eq!(filter[0].k, 4);

        // Last instruction must return ALLOW
        assert_eq!(filter[filter.len() - 1].code, (BPF_RET | BPF_K) as u16);
        assert_eq!(filter[filter.len() - 1].k, SECCOMP_RET_ALLOW);
    }

    #[test]
    fn test_whitelist_filter_compilation_sanity() {
        let filter = compile_whitelist_filter(STANDARD_GRAPHICAL_WHITELIST);
        // Header (3) + load nr (1) + whitelist entries (2 * len) + default kill (1)
        let expected_len = 3 + 1 + (STANDARD_GRAPHICAL_WHITELIST.len() * 2) + 1;
        assert_eq!(filter.len(), expected_len);

        // Last instruction must return KILL (default deny)
        assert_eq!(filter[filter.len() - 1].code, (BPF_RET | BPF_K) as u16);
        assert_eq!(filter[filter.len() - 1].k, SECCOMP_RET_KILL_PROCESS);
    }
}
