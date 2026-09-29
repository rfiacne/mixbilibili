use crate::i18n::t;
use crate::INTERRUPTED;
use anyhow::{Context, Result};
use std::io;
use std::process::{Child, ExitStatus};
use std::sync::atomic::Ordering;
use std::time::Duration;

pub(super) const FFMPEG_TIMEOUT: Duration = Duration::from_secs(300);

/// First poll delay. Short enough that a fast `-c copy` merge (tens of ms) is
/// noticed almost immediately instead of waiting out a fixed interval.
const POLL_INITIAL: Duration = Duration::from_millis(1);
/// Upper bound for the poll delay. Still responsive to Ctrl+C and accurate
/// enough for a 300s timeout, while keeping long ffmpeg runs from burning CPU
/// on wakeups.
const POLL_MAX: Duration = Duration::from_millis(50);

/// Why waiting for a child stopped.
///
/// Kept apart because the caller reacts differently: an interrupt leaves the pair
/// pending for `--resume`, a timeout is a merge failure.
enum WaitOutcome {
    /// The child exited on its own.
    Exited(ExitStatus),
    /// Ctrl+C was pressed. The child has been killed and reaped.
    Interrupted,
    /// The child outlived `timeout` and is still running, so the caller must
    /// clean it up.
    TimedOut,
}

/// Extension trait for `std::process::Child` that adds timeout-aware waiting.
trait ChildExt {
    fn wait_with_timeout(
        &mut self,
        timeout: Duration,
        initial_interval: Duration,
    ) -> io::Result<WaitOutcome>;
}

impl ChildExt for Child {
    /// Waits for the child, polling with exponential backoff from
    /// `initial_interval` up to [`POLL_MAX`].
    ///
    /// A fixed poll interval would add up to one full interval of latency to
    /// *every* merge: with `-c copy` on small files ffmpeg finishes in ~30ms,
    /// so a 100ms interval made the poll delay dominate the actual work. Backoff
    /// keeps short merges fast and long ones cheap.
    fn wait_with_timeout(
        &mut self,
        timeout: Duration,
        initial_interval: Duration,
    ) -> io::Result<WaitOutcome> {
        let start = std::time::Instant::now();
        let mut interval = initial_interval.min(POLL_MAX);
        loop {
            match self.try_wait() {
                Ok(Some(status)) => return Ok(WaitOutcome::Exited(status)),
                Ok(None) => {
                    // Check for interrupt signal and kill child if interrupted
                    if INTERRUPTED.load(Ordering::SeqCst) {
                        let _ = self.kill();
                        let _ = self.wait();
                        return Ok(WaitOutcome::Interrupted);
                    }
                    if start.elapsed() >= timeout {
                        return Ok(WaitOutcome::TimedOut);
                    }
                    std::thread::sleep(interval);
                    interval = next_poll_delay(interval, initial_interval);
                }
                Err(e) => return Err(e),
            }
        }
    }
}

/// Next poll delay in the backoff schedule: double the current delay, clamped
/// into `[max(initial, POLL_INITIAL), POLL_MAX]`.
///
/// The lower clamp matters when a caller passes `Duration::ZERO` as the initial
/// interval — `sleep(0)` returns immediately, which would spin the CPU hot.
fn next_poll_delay(current: Duration, initial: Duration) -> Duration {
    (current * 2).clamp(initial.max(POLL_INITIAL), POLL_MAX)
}

/// Runner for a single ffmpeg invocation: spawn `cmd`, wait up to `timeout`,
/// return the exit status. Injectable so merge logic is testable without ffmpeg.
///
/// `Send + Sync` because a batch shares one runner across rayon workers.
pub(super) type Runner<'a> =
    dyn Fn(&mut std::process::Command, Duration) -> Result<ExitStatus> + Send + Sync + 'a;

/// Runs `cmd`, waiting up to `timeout` for it to exit.
///
/// `Ok(None)` means Ctrl+C killed the child, which is *not* a merge failure: the
/// caller records that pair as interrupted so it stays pending for `--resume`.
/// A child that outlives the timeout is an error.
pub(super) fn run_with_timeout(
    cmd: &mut std::process::Command,
    timeout: Duration,
) -> Result<Option<ExitStatus>> {
    // SIGCHLD reset is now done once in main.rs run() before rayon pool creation

    let mut child = cmd.spawn().context(t("failed_to_spawn"))?;

    match child.wait_with_timeout(timeout, POLL_INITIAL) {
        Ok(WaitOutcome::Exited(status)) => Ok(Some(status)),
        Ok(WaitOutcome::Interrupted) => Ok(None),
        Ok(WaitOutcome::TimedOut) => {
            let _ = child.kill();
            let _ = child.wait();
            anyhow::bail!("{}", t("timed_out"));
        }
        Err(e) => Err(e).context(t("failed_to_wait")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_wait_timeout_normal_completion() {
        // Use cross-platform sleep command
        #[cfg(unix)]
        let mut child = std::process::Command::new("sleep")
            .arg("0.1")
            .spawn()
            .expect("sleep command should be available");

        #[cfg(windows)]
        let mut child = std::process::Command::new("timeout")
            .arg("/T")
            .arg("1")
            .arg("/NOBREAK")
            .spawn()
            .expect("timeout command should be available");

        let outcome = child
            .wait_with_timeout(Duration::from_secs(5), POLL_INITIAL)
            .expect("wait should succeed");
        assert!(matches!(outcome, WaitOutcome::Exited(_)));
    }

    #[test]
    fn test_wait_timeout_exceeded() {
        #[cfg(unix)]
        let mut child = std::process::Command::new("sleep")
            .arg("10")
            .spawn()
            .expect("sleep command should be available");

        #[cfg(windows)]
        let mut child = std::process::Command::new("timeout")
            .arg("/T")
            .arg("10")
            .arg("/NOBREAK")
            .spawn()
            .expect("timeout command should be available");

        let outcome = child
            .wait_with_timeout(Duration::from_millis(50), POLL_INITIAL)
            .expect("wait should succeed");
        // Distinct from an interrupt: only a timeout counts as a merge failure.
        assert!(matches!(outcome, WaitOutcome::TimedOut));
    }

    #[test]
    fn test_wait_timeout_already_finished() {
        // Very short sleep that finishes almost immediately
        #[cfg(unix)]
        let mut child = std::process::Command::new("sleep")
            .arg("0.01")
            .spawn()
            .expect("sleep command should be available");

        #[cfg(windows)]
        let mut child = std::process::Command::new("timeout")
            .arg("/T")
            .arg("1")
            .arg("/NOBREAK")
            .spawn()
            .expect("timeout command should be available");

        // Wait a bit for it to finish
        std::thread::sleep(Duration::from_millis(150));

        let outcome = child
            .wait_with_timeout(Duration::from_secs(5), POLL_INITIAL)
            .expect("wait should succeed");
        assert!(matches!(outcome, WaitOutcome::Exited(_)));
    }

    #[test]
    fn test_backoff_doubles_from_initial() {
        assert_eq!(
            next_poll_delay(POLL_INITIAL, POLL_INITIAL),
            Duration::from_millis(2)
        );
        assert_eq!(
            next_poll_delay(Duration::from_millis(8), POLL_INITIAL),
            Duration::from_millis(16)
        );
    }

    #[test]
    fn test_backoff_clamps_at_max() {
        // Never grows past POLL_MAX, so long ffmpeg runs stay cheap to poll.
        assert_eq!(next_poll_delay(POLL_MAX, POLL_INITIAL), POLL_MAX);
        assert_eq!(
            next_poll_delay(Duration::from_millis(40), POLL_INITIAL),
            POLL_MAX
        );
    }

    #[test]
    fn test_backoff_never_returns_zero_delay() {
        // A ZERO delay would make sleep() a no-op and spin the CPU hot.
        assert_eq!(
            next_poll_delay(Duration::ZERO, Duration::ZERO),
            POLL_INITIAL
        );
        assert_ne!(
            next_poll_delay(Duration::ZERO, Duration::ZERO),
            Duration::ZERO
        );
    }

    #[test]
    fn test_backoff_schedule_reaches_max_quickly() {
        // Cumulative sleep before hitting the cap must stay well under the old
        // fixed 100ms interval, otherwise short merges still pay that latency.
        let mut interval = POLL_INITIAL;
        let mut cumulative = Duration::ZERO;
        while interval < POLL_MAX {
            cumulative += interval;
            interval = next_poll_delay(interval, POLL_INITIAL);
        }
        assert!(
            cumulative < Duration::from_millis(100),
            "backoff ramp took {cumulative:?} before reaching POLL_MAX"
        );
    }

    #[test]
    fn test_fast_child_detected_promptly() {
        // Regression guard for the fixed-interval poll: a child that exits
        // almost immediately must not wait out a full poll interval.
        #[cfg(unix)]
        let mut child = std::process::Command::new("sleep")
            .arg("0.02")
            .spawn()
            .expect("sleep command should be available");

        #[cfg(windows)]
        let mut child = std::process::Command::new("timeout")
            .arg("/T")
            .arg("1")
            .arg("/NOBREAK")
            .spawn()
            .expect("timeout command should be available");

        let started = std::time::Instant::now();
        let outcome = child
            .wait_with_timeout(Duration::from_secs(5), POLL_INITIAL)
            .expect("wait should succeed");
        let elapsed = started.elapsed();

        assert!(
            matches!(outcome, WaitOutcome::Exited(_)),
            "child should have exited"
        );
        assert!(
            elapsed < Duration::from_millis(150),
            "fast child took {elapsed:?} to detect; poll backoff may be too slow"
        );
    }
}
