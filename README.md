# wezel_exec

Run a shell command and measure its time and resource usage in a Wezel experiment.
Supports Linux and macOS without root access.

```toml
[step.exec.build]
cmd = "cargo build --release"
summary.build-time = { outcome = "wall_time_ms" }
summary.cpu-time = { outcome = "user_time_ms" }
summary.peak-memory = { outcome = "max_rss_bytes" }
```

`cmd` is executed with `sh -c`. Optional `env` entries extend or override the
inherited environment; optional `cwd` selects the working directory. Standard
input, output, and error are inherited.

See [the outcome reference](OUTCOMES.md) for all eight metrics and their accounting
scope. The same reference is exposed by `wezel_exec --schema` for editor support.

For direct invocation, set `FORAGER_INPUTS` to a JSON input file and `FORAGER_OUT`
to the output report path. For example, the input file can contain:

```json
{"cmd":"cargo build --release","env":{"CARGO_INCREMENTAL":"0"}}
```

Build and validate with:

```sh
cargo fmt --all -- --check
cargo clippy --all-targets -- -D warnings
cargo test
```
