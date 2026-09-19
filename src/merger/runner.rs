use crate::i18n::t;
use crate::INTERRUPTED;
use anyhow::{Context, Result};
use std::io;
use std::process::{Child, ExitStatus};
use std::sync::atomic::Ordering;
use std::time::Duration;

pub(super) const FFMPEG_TIMEOUT: Duration = Duration::from_secs(300);
const POLL_INTERVAL: Duration = Duration::from_millis(100);

/// Extension trait for `std::process::Child` that adds timeout-aware waiting.
trait ChildExt {
    fn wait_with_timeout(
        &mut self,
        timeout: Duration,
        poll_interval: Duration,
    ) -> io::Result<Option<ExitStatus>>;
}

impl ChildExt for Child {
    fn wait_with_timeout(
        &mut self,
        timeout: Duration,
        poll_interval: Duration,
    ) -> io::Result<Option<ExitStatus>> {
        let start = std::time::Instant::now();
        loop {
            match self.try_wait() {
                Ok(Some(status)) => return Ok(Some(status)),
                Ok(None) => {
                    // Check for interrupt signal and kill child if interrupted
                    if INTERRUPTED.load(Ordering::SeqCst) {
                        let _ = self.kill();
                        let _ = self.wait();
                        return Ok(None);
                    }
                    if start.elapsed() >= timeout {
                        return Ok(None);
                    }
                    std::thread::sleep(poll_interval);
                }
                Err(e) => return Err(e),
            }
        }
    }
}

/// Runner for a single ffmpeg invocation: spawn `cmd`, wait up to `timeout`,
/// return the exit status. Injectable so merge logic is testable without ffmpeg.
pub(super) type Runner<'a> =
    dyn Fn(&mut std::process::Command, Duration) -> Result<ExitStatus> + 'a;

pub(super) fn run_with_timeout(
    cmd: &mut std::process::Command,
    timeout: Duration,
) -> Result<ExitStatus> {
    // SIGCHLD reset is now done once in main.rs run() before rayon pool creation

    let mut child = cmd.spawn().context(t("failed_to_spawn"))?;

    match child.wait_with_timeout(timeout, POLL_INTERVAL) {
        Ok(Some(status)) => Ok(status),
        Ok(None) => {
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

        let result = child.wait_with_timeout(Duration::from_secs(5), POLL_INTERVAL);
        assert!(result.is_ok());
        let status = result.unwrap();
        assert!(status.is_some());
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

        let result = child.wait_with_timeout(Duration::from_millis(50), POLL_INTERVAL);
        assert!(result.is_ok());
        // Should return None indicating timeout
        let status = result.unwrap();
        assert!(status.is_none());
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

        let result = child.wait_with_timeout(Duration::from_secs(5), POLL_INTERVAL);
        assert!(result.is_ok());
        assert!(result.unwrap().is_some());
    }
}
