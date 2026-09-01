//! Step the station's clock forward from the messages it receives.
//!
//! The Pis run offline: no NTP, and (without a battery on J5) an RTC that
//! forgets across power-off, so a station boots believing it is still
//! whenever it was last unplugged. That is worse than cosmetic: dash-chat
//! refuses a reply whose timestamp is not later than its target
//! (`ReplyError::TimestampNotLater`), so a bot whose clock lags the players'
//! phones can never thread its acks onto the deliveries they answer.
//!
//! The players' phones DO know the time (cellular). Every op they author is
//! stamped with it, so the fix is the same trick as NTP with the phones as
//! the time source: whenever an op arrives from ahead of local time, step the
//! system clock up to it. Forward only — a phone with a slow clock is ignored
//! rather than allowed to drag the station back, and the largest timestamp
//! seen wins, which keeps the bot's own sends later than everything it
//! answers.
//!
//! Setting the clock needs `CAP_SYS_TIME` (granted to the bot units in
//! nix/larp-bot.nix). Anywhere else — dev runs, tests — the syscall fails
//! with EPERM and is logged once at info, not treated as an error: on a
//! machine with a real clock there is nothing to fix anyway.

use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use tracing::{info, warn};

/// Ops this far ahead of local time (or less) are treated as ordinary skew
/// between healthy clocks and never trigger a step.
const TOLERANCE: Duration = Duration::from_secs(2);

/// Step the system clock forward to `op_micros` (µs since the UNIX epoch —
/// p2panda's `Timestamp` unit) if that lies more than [`TOLERANCE`] ahead of
/// local time. Returns whether the clock was stepped.
pub fn step_clock_forward(op_micros: u64) -> bool {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or(Duration::ZERO);
    let target = Duration::from_micros(op_micros);
    if !should_step(now, target) {
        return false;
    }
    let stepped = set_system_clock(target);
    if stepped {
        info!(
            behind_secs = (target - now).as_secs(),
            "local clock lagged a received message — stepped it forward"
        );
    }
    stepped
}

/// The step decision, separated from the syscall so tests can cover it
/// without ever being able to set a (privileged) host's real clock.
fn should_step(now: Duration, target: Duration) -> bool {
    target > now + TOLERANCE
}

fn set_system_clock(target: Duration) -> bool {
    let ts = libc::timespec {
        tv_sec: target.as_secs() as libc::time_t,
        tv_nsec: u64::from(target.subsec_nanos()) as libc::c_long,
    };
    // SAFETY: plain syscall on a valid, initialized timespec.
    let rc = unsafe { libc::clock_settime(libc::CLOCK_REALTIME, &ts) };
    if rc == 0 {
        return true;
    }
    // Expected wherever the bot runs without CAP_SYS_TIME (dev runs, tests).
    // Logged once, then quiet: every scan of the same chat would repeat it.
    static REPORTED: AtomicBool = AtomicBool::new(false);
    if !REPORTED.swap(true, Ordering::Relaxed) {
        let err = std::io::Error::last_os_error();
        warn!(
            %err,
            "local clock lags received messages but cannot be stepped \
             (no CAP_SYS_TIME?) — replies will not thread until the clock is fixed"
        );
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Past timestamps and skew within tolerance never step; only a
    /// timestamp beyond the tolerance does. Pure decision only — the test
    /// must never be able to set a (privileged) host's real clock.
    #[test]
    fn steps_only_beyond_the_forward_tolerance() {
        let now = Duration::from_secs(1_000_000);
        assert!(!should_step(now, Duration::ZERO));
        assert!(!should_step(now, now - Duration::from_secs(60)));
        assert!(!should_step(now, now + TOLERANCE));
        assert!(should_step(now, now + TOLERANCE + Duration::from_secs(1)));
    }

    /// The public entry point with a past timestamp: refuses without ever
    /// reaching the syscall.
    #[test]
    fn past_timestamp_is_ignored() {
        assert!(!step_clock_forward(0));
    }
}
