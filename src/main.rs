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
    const OUTCOMES_DOC: &'static str = include_str!("../OUTCOMES.md");
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
