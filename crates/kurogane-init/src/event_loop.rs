use nix::sys::epoll::{Epoll, EpollCreateFlags, EpollEvent, EpollFlags, EpollTimeout};
use nix::sys::wait::{waitpid, WaitPidFlag, WaitStatus};
use nix::unistd::Pid;
use std::os::unix::io::{AsFd, BorrowedFd, RawFd};

use crate::signals::{SignalEvent, SignalManager};
use crate::KuroganeError;

pub const TOKEN_SIGNAL: u64 = 1;
pub const TOKEN_TIMER: u64 = 2;
pub const TOKEN_WAYLAND_LISTENER: u64 = 3;

/// Distinct epoll event tokens multiplexed in the single-threaded reactor.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EpollToken {
    SignalFd,
    TimerFd,
    WaylandListener,
    WaylandClient(RawFd),
    WaylandCompositor(RawFd),
    Custom(u64),
}

impl EpollToken {
    pub const CLIENT_BASE: u64 = 1000;
    pub const COMPOSITOR_BASE: u64 = 2000;

    pub fn to_u64(self) -> u64 {
        match self {
            EpollToken::SignalFd => TOKEN_SIGNAL,
            EpollToken::TimerFd => TOKEN_TIMER,
            EpollToken::WaylandListener => TOKEN_WAYLAND_LISTENER,
            EpollToken::WaylandClient(fd) => Self::CLIENT_BASE + (fd as u64),
            EpollToken::WaylandCompositor(fd) => Self::COMPOSITOR_BASE + (fd as u64),
            EpollToken::Custom(val) => val,
        }
    }

    pub fn from_u64(val: u64) -> Self {
        match val {
            TOKEN_SIGNAL => EpollToken::SignalFd,
            TOKEN_TIMER => EpollToken::TimerFd,
            TOKEN_WAYLAND_LISTENER => EpollToken::WaylandListener,
            v if v >= Self::CLIENT_BASE && v < Self::COMPOSITOR_BASE => {
                EpollToken::WaylandClient((v - Self::CLIENT_BASE) as RawFd)
            }
            v if v >= Self::COMPOSITOR_BASE && v < Self::COMPOSITOR_BASE + 1000 => {
                EpollToken::WaylandCompositor((v - Self::COMPOSITOR_BASE) as RawFd)
            }
            other => EpollToken::Custom(other),
        }
    }
}

/// Single-threaded, non-blocking event loop orchestrator for Kurogane PID 1.
pub struct EventLoop {
    epoll: Epoll,
    pub signals: SignalManager,
}

impl EventLoop {
    /// Initializes epoll instance and registers signalfd descriptor.
    pub fn new() -> Result<Self, KuroganeError> {
        let epoll = Epoll::new(EpollCreateFlags::EPOLL_CLOEXEC)
            .map_err(|_| KuroganeError::HardwareError("Failed to create epoll instance"))?;

        let signals = SignalManager::new()?;

        let raw_sfd = signals.as_raw_fd();
        let borrowed_sfd = unsafe { BorrowedFd::borrow_raw(raw_sfd) };
        let signal_event = EpollEvent::new(EpollFlags::EPOLLIN, TOKEN_SIGNAL);

        epoll
            .add(borrowed_sfd, signal_event)
            .map_err(|_| KuroganeError::HardwareError("Failed to register signalfd with epoll"))?;

        Ok(Self { epoll, signals })
    }

    /// Register an arbitrary file descriptor into epoll (e.g. timerfd or IPC sockets).
    pub fn add_fd<Fd: AsFd>(
        &self,
        fd: Fd,
        token: u64,
        flags: EpollFlags,
    ) -> Result<(), KuroganeError> {
        let event = EpollEvent::new(flags, token);
        self.epoll
            .add(fd, event)
            .map_err(|_| KuroganeError::HardwareError("Failed to add fd to epoll"))
    }

    /// Register an arbitrary file descriptor into epoll with typed EpollToken.
    pub fn add_token_fd<Fd: AsFd>(
        &self,
        fd: Fd,
        token: EpollToken,
        flags: EpollFlags,
    ) -> Result<(), KuroganeError> {
        self.add_fd(fd, token.to_u64(), flags)
    }

    /// Deregisters a file descriptor from epoll.
    pub fn delete_fd<Fd: AsFd>(&self, fd: Fd) -> Result<(), KuroganeError> {
        self.epoll
            .delete(fd)
            .map_err(|_| KuroganeError::HardwareError("Failed to remove fd from epoll"))
    }

    /// Drains all exited zombie child processes non-blockingly via waitpid(WNOHANG).
    pub fn reap_zombies() -> Vec<(Pid, WaitStatus)> {
        let mut reaped = Vec::new();
        loop {
            match waitpid(Pid::from_raw(-1), Some(WaitPidFlag::WNOHANG)) {
                Ok(status) => match status {
                    WaitStatus::Exited(pid, _) | WaitStatus::Signaled(pid, _, _) => {
                        reaped.push((pid, status));
                    }
                    WaitStatus::StillAlive => break,
                    _ => break,
                },
                Err(nix::errno::Errno::ECHILD) => break,
                Err(_) => break,
            }
        }
        reaped
    }

    /// Waits for the next event with a non-blocking or bounded timeout.
    pub fn poll_events(&mut self, timeout_ms: u16) -> Result<Vec<u64>, KuroganeError> {
        let mut events = [EpollEvent::empty(); 16];
        let timeout = EpollTimeout::from(timeout_ms);

        let n = self
            .epoll
            .wait(&mut events, timeout)
            .map_err(|_| KuroganeError::HardwareError("epoll_wait failed"))?;

        let mut triggered_tokens = Vec::with_capacity(n);
        for event in events.iter().take(n) {
            triggered_tokens.push(event.data());
        }

        Ok(triggered_tokens)
    }

    /// Waits for events and maps triggered tokens into typed EpollTokens.
    pub fn poll_tokens(&mut self, timeout_ms: u16) -> Result<Vec<EpollToken>, KuroganeError> {
        let raw = self.poll_events(timeout_ms)?;
        Ok(raw.into_iter().map(EpollToken::from_u64).collect())
    }

    /// Dispatches signal events if the signalfd token was triggered.
    pub fn dispatch_signals(&mut self) -> Result<Vec<SignalEvent>, KuroganeError> {
        let mut received = Vec::new();
        while let Some(sig) = self.signals.read_signal()? {
            received.push(sig);
        }
        Ok(received)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::process::Command;
    use std::time::Duration;

    #[test]
    fn test_epoll_token_bijective_mapping() {
        let tokens = [
            EpollToken::SignalFd,
            EpollToken::TimerFd,
            EpollToken::WaylandListener,
            EpollToken::WaylandClient(5),
            EpollToken::WaylandClient(42),
            EpollToken::WaylandCompositor(7),
            EpollToken::WaylandCompositor(99),
            EpollToken::Custom(9999),
        ];

        for t in tokens {
            let encoded = t.to_u64();
            let decoded = EpollToken::from_u64(encoded);
            assert_eq!(t, decoded);
        }
    }

    #[test]
    fn test_event_loop_creation_and_polling() {
        let mut event_loop = EventLoop::new().expect("create EventLoop");
        let _ = event_loop.dispatch_signals();
        let _ = EventLoop::reap_zombies();
        // Non-blocking poll with 10ms timeout when idle
        let tokens = event_loop.poll_events(10).expect("poll events");
        assert!(tokens.is_empty() || tokens == vec![TOKEN_SIGNAL]);
    }

    #[test]
    fn test_nonblocking_zombie_reaper_drain() {
        // Spawn 3 short-lived child processes
        let mut child1 = Command::new("true").spawn().expect("spawn true 1");
        let mut child2 = Command::new("true").spawn().expect("spawn true 2");
        let mut child3 = Command::new("true").spawn().expect("spawn true 3");

        // Wait a brief moment for children to terminate
        std::thread::sleep(Duration::from_millis(50));

        // Non-blocking reap should collect all 3
        let reaped = EventLoop::reap_zombies();
        assert!(reaped.len() >= 3);

        let _ = child1.wait();
        let _ = child2.wait();
        let _ = child3.wait();
    }
}
