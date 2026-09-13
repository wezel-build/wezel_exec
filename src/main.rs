use std::collections::HashMap;
use std::process;

use anyhow::{Result, anyhow};
use forager_sdk::{Forager, ForagerPluginOutput};
use schemars::JsonSchema;
use serde::Deserialize;

mod measurement;

#[derive(Deserialize, JsonSchema)]
struct ExecInputs {
    /// Shell command to run.
    cmd: String,
    /// Extra environment variables.
    #[serde(default)]
    env: HashMap<String, String>,
    /// Working directory override.
    cwd: Option<String>,
}

struct Exec;

impl Forager for Exec {
    const NAME: &'static str = "exec";
    const DESCRIPTION: &'static str =
        "Executes a shell command and measures time and resource usage";
    const OUTCOMES_DOC: &'static str = r#"Every successful command emits these outcomes on Linux and macOS. No root access
or external profiling tool is required.

| Outcome | Unit | Meaning |
| --- | --- | --- |
| `wall_time_ms` | milliseconds | Elapsed time from immediately before command launch until its exit is collected, including waiting. |
| `user_time_ms` | milliseconds | CPU time spent executing in user mode. |
| `system_time_ms` | milliseconds | CPU time spent executing in kernel mode. |
| `max_rss_bytes` | bytes | Largest individual process peak resident memory reported by the OS. |
| `minor_page_faults` | count | Page faults serviced without I/O. |
| `major_page_faults` | count | Page faults requiring I/O. |
| `voluntary_context_switches` | count | Context switches caused by yielding or blocking. |
| `involuntary_context_switches` | count | Context switches caused by preemption. |

Time values retain fractional milliseconds. Memory and count values are integers.
All outcomes have unit metadata, use the default lower-is-better direction, and
have no tags. Fault and context-switch counts are diagnostic signals; a reduction
alone does not establish that a build improved.

CPU time and resource counters include the shell and descendants whose usage is
propagated by their parents waiting for them. Commands must wait for their work to
finish; detached processes and work performed by an existing build daemon are not
fully accounted for. CPU time can exceed wall time when work runs in parallel.

`max_rss_bytes` is **not the combined peak memory of a parallel build**. It is the
largest individual peak reported through the process hierarchy. OS accounting
semantics differ, so compare measurements on the same platform and testbed.

A command that cannot start, exits unsuccessfully, or is terminated by a signal
fails the step and produces no outcome report.
"#;
    type Inputs = ExecInputs;

    fn run(inputs: ExecInputs) -> Result<Vec<ForagerPluginOutput>> {
        let mut command = process::Command::new("sh");
        command.arg("-c").arg(&inputs.cmd);
        for (k, v) in &inputs.env {
            command.env(k, v);
        }
        if let Some(dir) = &inputs.cwd {
            command.current_dir(dir);
        }
        let measurement = measurement::run(command)?;
        if !measurement.status.success() {
            return Err(anyhow!("command exited with {}", measurement.status));
        }
        Ok(measurement.into_outcomes())
    }
}

forager_sdk::forager_main!(Exec);
