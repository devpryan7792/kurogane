use nix::sys::signal::{self, SigSet, SigmaskHow, Signal};
use nix::sys::signalfd::{SfdFlags, SignalFd};
use nix::unistd::Pid;
use std::os::unix::io::AsRawFd;

use crate::KuroganeError;

/// High-level event produced by Unix signals delivered to PID 1.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SignalEvent {
    /// A supervised child process exited or was terminated.
    ChildExit(Pid, i32),
    /// Host requested system poweroff (SIGPWR or SIGINT).
    PowerOff,
    /// Host requested system reboot (SIGTERM).
    Reboot,
    /// Unhandled or ignored signal.
    Ignored(i32),
}

/// Wrapper around Linux signalfd(2) providing a non-blocking signal stream for PID 1.
pub struct SignalManager {
    sfd: SignalFd,
    _mask: SigSet,
}

impl SignalManager {
    /// Sets up the signalfd mask, blocking signals from standard delivery so they can be read.
    pub fn new() -> Result<Self, KuroganeError> {
        let mut mask = SigSet::empty();
        mask.add(Signal::SIGCHLD);
        mask.add(Signal::SIGTERM);
        mask.add(Signal::SIGINT);
        mask.add(Signal::SIGPWR);

        // Block standard POSIX signal handling so signals queue into signalfd instead
        signal::sigprocmask(SigmaskHow::SIG_BLOCK, Some(&mask), None)
            .map_err(|_| KuroganeError::HardwareError("Failed to apply sigprocmask"))?;

        // Create non-blocking signalfd
        let sfd = SignalFd::with_flags(&mask, SfdFlags::SFD_NONBLOCK | SfdFlags::SFD_CLOEXEC)
            .map_err(|_| KuroganeError::HardwareError("Failed to create signalfd"))?;

        Ok(Self { sfd, _mask: mask })
    }

    /// Read the next signal from the signalfd descriptor, if available.
    pub fn read_signal(&mut self) -> Result<Option<SignalEvent>, KuroganeError> {
        match self.sfd.read_signal() {
            Ok(Some(siginfo)) => {
                let signo = siginfo.ssi_signo as i32;
                let event = match Signal::try_from(signo) {
                    Ok(Signal::SIGCHLD) => {
                        let pid = Pid::from_raw(siginfo.ssi_pid as i32);
                        let status = siginfo.ssi_status;
                        SignalEvent::ChildExit(pid, status)
                    }
                    Ok(Signal::SIGTERM) => SignalEvent::Reboot,
                    Ok(Signal::SIGINT) | Ok(Signal::SIGPWR) => SignalEvent::PowerOff,
                    _ => SignalEvent::Ignored(signo),
                };
                Ok(Some(event))
            }
            Ok(None) => Ok(None),
            Err(e) => {
                // EAGAIN / EWOULDBLOCK means no signals queued
                if e == nix::errno::Errno::EAGAIN || e == nix::errno::Errno::EWOULDBLOCK {
                    Ok(None)
                } else {
                    Err(KuroganeError::HardwareError("Error reading signalfd"))
                }
            }
        }
    }

    /// Returns the raw file descriptor for epoll registration.
    pub fn as_raw_fd(&self) -> std::os::unix::io::RawFd {
        self.sfd.as_raw_fd()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_signal_manager_creation_and_polling() {
        let mut manager = SignalManager::new().expect("create SignalManager");
        // No signals should be queued initially
        let signal = manager.read_signal().expect("read non-blocking signal");
        assert_eq!(signal, None);
        assert!(manager.as_raw_fd() >= 0);
    }
}
