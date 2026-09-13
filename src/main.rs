use std::collections::HashMap;
use std::process;

use anyhow::{Context, Result, anyhow};
use forager_sdk::{Forager, ForagerPluginOutput};
use schemars::JsonSchema;
use serde::Deserialize;

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
    const DESCRIPTION: &'static str = "Executes a shell command; produces no outcomes";
    const OUTCOMES_DOC: &'static str = "This forager emits no outcomes.";
    type Inputs = ExecInputs;

    fn run(inputs: ExecInputs) -> Result<Vec<ForagerPluginOutput>> {
        let mut child = process::Command::new("sh");
        child.arg("-c").arg(&inputs.cmd);
        for (k, v) in &inputs.env {
            child.env(k, v);
        }
        if let Some(dir) = &inputs.cwd {
            child.current_dir(dir);
        }
        let status = child.status().context("failed to spawn command")?;
        if !status.success() {
            return Err(anyhow!("command exited with {status}"));
        }
        Ok(vec![])
    }
}

fn main() {
    if let Err(error) = forager_sdk::run::<Exec>() {
        eprintln!("{}: {error:#}", env!("CARGO_BIN_NAME"));
        std::process::exit(1);
    }
}
