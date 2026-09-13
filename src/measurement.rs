use std::io;
use std::mem::MaybeUninit;
use std::os::unix::process::ExitStatusExt;
use std::process::{Child, Command, ExitStatus};
use std::time::{Duration, Instant};

use anyhow::{Context, Result};
use forager_sdk::{ForagerPluginOutput, Unit};

#[cfg(target_os = "linux")]
const RSS_UNIT_BYTES: u64 = 1024;
#[cfg(target_os = "macos")]
const RSS_UNIT_BYTES: u64 = 1;
#[cfg(not(any(target_os = "linux", target_os = "macos")))]
compile_error!("wezel_exec resource measurement supports Linux and macOS");

pub struct Measurement {
    pub status: ExitStatus,
    wall_time: Duration,
    usage: libc::rusage,
}

pub fn run(mut command: Command) -> Result<Measurement> {
    let start = Instant::now();
    let child = command.spawn().context("failed to spawn command")?;
    let (status, usage) = wait(child).context("failed to wait for command")?;
    Ok(Measurement {
        status,
        wall_time: start.elapsed(),
        usage,
    })
}

/// Consume the child so no caller can wait on or signal its PID after we reap it.
fn wait(mut child: Child) -> io::Result<(ExitStatus, libc::rusage)> {
    // Match Child::wait if the command ever uses piped stdin.
    drop(child.stdin.take());
    let pid = child.id() as libc::pid_t;
    let mut status = 0;
    let mut usage = MaybeUninit::<libc::rusage>::uninit();

    loop {
        // SAFETY: both output pointers are valid, and this function is the sole
        // waiter for this child. Do not call Child::wait/try_wait before wait4:
        // they would reap the child and discard its resource usage.
        let result = unsafe { libc::wait4(pid, &mut status, 0, usage.as_mut_ptr()) };
        if result == -1 {
            let error = io::Error::last_os_error();
            if error.kind() == io::ErrorKind::Interrupted {
                continue;
            }
            return Err(error);
        }
        // Traced children can report a stop even without WUNTRACED.
        if libc::WIFEXITED(status) || libc::WIFSIGNALED(status) {
            // SAFETY: a successful terminating wait4 initialized the rusage.
            let usage = unsafe { usage.assume_init() };
            return Ok((ExitStatus::from_raw(status), usage));
        }
    }
}

impl Measurement {
    pub fn into_outcomes(self) -> Vec<ForagerPluginOutput> {
        let usage = self.usage;
        vec![
            outcome(
                "wall_time_ms",
                self.wall_time.as_secs_f64() * 1000.0,
                Unit::Milliseconds,
            ),
            outcome(
                "user_time_ms",
                milliseconds(usage.ru_utime),
                Unit::Milliseconds,
            ),
            outcome(
                "system_time_ms",
                milliseconds(usage.ru_stime),
                Unit::Milliseconds,
            ),
            outcome(
                "max_rss_bytes",
                usage.ru_maxrss as u64 * RSS_UNIT_BYTES,
                Unit::Bytes,
            ),
            outcome("minor_page_faults", usage.ru_minflt as u64, Unit::Count),
            outcome("major_page_faults", usage.ru_majflt as u64, Unit::Count),
            outcome(
                "voluntary_context_switches",
                usage.ru_nvcsw as u64,
                Unit::Count,
            ),
            outcome(
                "involuntary_context_switches",
                usage.ru_nivcsw as u64,
                Unit::Count,
            ),
        ]
    }
}

fn milliseconds(time: libc::timeval) -> f64 {
    time.tv_sec as f64 * 1000.0 + time.tv_usec as f64 / 1000.0
}

fn outcome(name: &str, value: impl Into<serde_json::Value>, unit: Unit) -> ForagerPluginOutput {
    ForagerPluginOutput {
        name: name.into(),
        value: value.into(),
        unit: Some(unit),
        ..Default::default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cpu_time_preserves_submillisecond_precision() {
        assert_eq!(
            milliseconds(libc::timeval {
                tv_sec: 2,
                tv_usec: 345_678,
            }),
            2345.678,
        );
    }
}
