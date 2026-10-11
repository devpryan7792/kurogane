use nix::sys::time::TimeSpec;
use nix::sys::timerfd::{ClockId, Expiration, TimerFd, TimerFlags, TimerSetTimeFlags};
use std::os::unix::io::{AsFd, AsRawFd, BorrowedFd, RawFd};
use std::time::Duration;

use crate::KuroganeError;

/// Non-blocking monotonic timerfd utility for PID 1 heartbeats, watchdog, and polling ticks.
pub struct MonotonicTimer {
    tfd: TimerFd,
}

impl MonotonicTimer {
    /// Creates a new non-blocking monotonic timerfd.
    pub fn new() -> Result<Self, KuroganeError> {
        let tfd = TimerFd::new(
            ClockId::CLOCK_MONOTONIC,
            TimerFlags::TFD_NONBLOCK | TimerFlags::TFD_CLOEXEC,
        )
        .map_err(|_| KuroganeError::HardwareError("Failed to create timerfd"))?;
        Ok(Self { tfd })
    }

    /// Arms the timer for a specified duration (one-shot).
    pub fn arm_oneshot(&mut self, duration: Duration) -> Result<(), KuroganeError> {
        let time_spec = TimeSpec::from(duration);
        let expiration = Expiration::OneShot(time_spec);
        self.tfd
            .set(expiration, TimerSetTimeFlags::empty())
            .map_err(|_| KuroganeError::HardwareError("Failed to arm timerfd"))?;
        Ok(())
    }

    /// Arms the timer as a periodic tick with an initial delay and recurring interval.
    pub fn arm_periodic(&mut self, interval: Duration) -> Result<(), KuroganeError> {
        let interval_spec = TimeSpec::from(interval);
        let expiration = Expiration::Interval(interval_spec);
        self.tfd
            .set(expiration, TimerSetTimeFlags::empty())
            .map_err(|_| KuroganeError::HardwareError("Failed to arm periodic timerfd"))?;
        Ok(())
    }

    /// Acknowledges / clears the timer if expired. Returns true if expired, false if not yet.
    pub fn ack(&mut self) -> Result<bool, KuroganeError> {
        match self.tfd.wait() {
            Ok(_) => Ok(true),
            Err(nix::errno::Errno::EAGAIN) => Ok(false),
            Err(_) => Err(KuroganeError::HardwareError("Failed to read timerfd")),
        }
    }

    /// Returns the raw file descriptor.
    pub fn as_raw_fd(&self) -> RawFd {
        self.tfd.as_fd().as_raw_fd()
    }

    /// Borrows the descriptor for epoll registration.
    pub fn as_borrowed_fd(&self) -> BorrowedFd<'_> {
        self.tfd.as_fd()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_timer_creation_and_ack() {
        let mut timer = MonotonicTimer::new().expect("create timer");
        // No expirations initially
        assert!(!timer.ack().unwrap());

        // Arm 10ms one-shot
        timer
            .arm_oneshot(Duration::from_millis(10))
            .expect("arm timer");
        std::thread::sleep(Duration::from_millis(25));

        // After sleeping, ack should report true
        let expired = timer.ack().expect("ack timer");
        assert!(expired);
    }
}
