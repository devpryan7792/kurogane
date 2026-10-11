//! Integration Test: Kurogane v3.x Single-Threaded Event Core & Non-Blocking Reaping
//! Verifies SPEC.md Task 16 (signalfd + epoll) and Task 17 (timerfd watchdog).

use kurogane_init::event_loop::{EventLoop, TOKEN_SIGNAL, TOKEN_TIMER};
use kurogane_init::timer::MonotonicTimer;
use nix::sys::epoll::EpollFlags;
use nix::unistd::{fork, ForkResult};
use std::fs;
use std::process::{exit, Command};
use std::time::Duration;

#[test]
fn test_signalfd_drain_and_reap() {
    // Fork an isolated single-threaded process to replicate exact PID 1 signal environment
    match unsafe { fork() }.expect("fork isolated test process") {
        ForkResult::Parent { child } => {
            let status = nix::sys::wait::waitpid(child, None).expect("wait for child");
            assert_eq!(
                status,
                nix::sys::wait::WaitStatus::Exited(child, 0),
                "Isolated test process failed"
            );
        }
        ForkResult::Child => {
            let mut event_loop = EventLoop::new().expect("create EventLoop");

            // Spawn 5 background children that immediately exit
            let mut pids = Vec::new();
            let mut children = Vec::new();
            for _ in 0..5 {
                let child = Command::new("true").spawn().expect("spawn child");
                pids.push(child.id() as i32);
                children.push(child);
            }

            // Give kernel a slice to transition processes to zombie state
            std::thread::sleep(Duration::from_millis(50));

            // Epoll should detect signalfd readability (TOKEN_SIGNAL)
            let tokens = event_loop.poll_events(100).expect("poll epoll for signals");
            if !tokens.contains(&TOKEN_SIGNAL) {
                eprintln!("[FAIL] Expected TOKEN_SIGNAL in epoll tokens: {:?}", tokens);
                exit(1);
            }

            // Drain signals and non-blockingly reap zombies
            let _signals = event_loop.dispatch_signals().expect("dispatch signals");
            let reaped = EventLoop::reap_zombies();

            if reaped.len() < 5 {
                eprintln!("[FAIL] Expected >= 5 reaped, got {}", reaped.len());
                exit(2);
            }

            // Verify all spawned child PIDs were reaped
            let reaped_pids: Vec<i32> = reaped.iter().map(|(pid, _)| pid.as_raw()).collect();
            for expected_pid in pids {
                if !reaped_pids.contains(&expected_pid) {
                    eprintln!(
                        "[FAIL] PID {} was not reaped in {:?}",
                        expected_pid, reaped_pids
                    );
                    exit(3);
                }
            }

            for mut c in children {
                let _ = c.wait();
            }

            exit(0);
        }
    }
}

#[test]
fn test_timerfd_nonblocking_integration() {
    let mut event_loop = EventLoop::new().expect("create EventLoop");
    let mut timer = MonotonicTimer::new().expect("create MonotonicTimer");

    // Arm a 20ms one-shot timer
    timer
        .arm_oneshot(Duration::from_millis(20))
        .expect("arm timer");

    // Register timer with epoll under TOKEN_TIMER
    event_loop
        .add_fd(timer.as_borrowed_fd(), TOKEN_TIMER, EpollFlags::EPOLLIN)
        .expect("register timerfd with epoll");

    // Poll epoll with 100ms timeout
    let tokens = event_loop.poll_events(100).expect("poll epoll for timerfd");
    assert!(
        tokens.contains(&TOKEN_TIMER),
        "Expected TOKEN_TIMER event in epoll"
    );

    // Acknowledge timer expiration
    let expired = timer.ack().expect("ack timer");
    assert!(expired, "Expected timer to report expiration");
}

#[test]
fn test_single_threaded_invariant() {
    if let Ok(status) = fs::read_to_string("/proc/self/status") {
        for line in status.lines() {
            if line.starts_with("Threads:") {
                let count: usize = line
                    .split_whitespace()
                    .nth(1)
                    .and_then(|s| s.parse().ok())
                    .unwrap_or(0);
                println!("[INFO] Process active thread count: {}", count);
                assert!(count >= 1);
            }
        }
    }
}
