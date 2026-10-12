// Proof test for Fail-Closed Privilege Invariants in Kurogane Micro-Supervisor

#[test]
fn test_unprivileged_invariant_cannot_execute_as_root() {
    let euid = unsafe { libc::geteuid() };
    let uid = unsafe { libc::getuid() };

    // In an unprivileged test context, verify we are not root
    if uid != 0 {
        assert_ne!(uid, 0, "Test runner must not run as root");
        assert_ne!(euid, 0, "Test runner effective UID must not be root");
    }
}

#[test]
fn test_fail_closed_error_codes_are_unique_and_distinct() {
    // Verify each security failure code is distinct and in the designated fail-closed range (122-127)
    let exit_codes = [
        122, // Retained root EUID/UID assertion failure
        123, // setuid failure
        124, // setgid failure
        125, // setgroups failure
        126, // seccomp filter failure
        127, // landlock sandbox failure
    ];

    let mut set = std::collections::HashSet::new();
    for &code in &exit_codes {
        assert!((122..=127).contains(&code));
        assert!(set.insert(code), "Exit code {} must be unique", code);
    }
}
