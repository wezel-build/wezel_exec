#![cfg(any(target_os = "linux", target_os = "macos"))]

use std::fs;
use std::hint::black_box;
use std::io::Write;
use std::process::{Command, Output, Stdio};
use std::time::Duration;

use serde_json::{Value, json};
use tempfile::TempDir;

const METRICS: [(&str, &str); 8] = [
    ("wall_time_ms", "milliseconds"),
    ("user_time_ms", "milliseconds"),
    ("system_time_ms", "milliseconds"),
    ("max_rss_bytes", "bytes"),
    ("minor_page_faults", "count"),
    ("major_page_faults", "count"),
    ("voluntary_context_switches", "count"),
    ("involuntary_context_switches", "count"),
];

struct Run {
    directory: TempDir,
    output: Output,
}

impl Run {
    fn report(&self) -> Value {
        assert!(
            self.output.status.success(),
            "forager failed: {}",
            String::from_utf8_lossy(&self.output.stderr)
        );
        let report: Value = serde_json::from_slice(
            &fs::read(self.directory.path().join("out.json")).expect("report was written"),
        )
        .expect("valid report JSON");
        let outcomes = report["outcomes"].as_array().expect("SDK output envelope");
        assert_eq!(outcomes.len(), METRICS.len());
        for (name, unit) in METRICS {
            let outcome = outcomes
                .iter()
                .find(|outcome| outcome["name"] == name)
                .unwrap_or_else(|| panic!("missing {name}: {report}"));
            assert_eq!(outcome["unit"], unit, "unit for {name}");
            assert_eq!(
                outcome["direction"].as_str().unwrap_or("lower-is-better"),
                "lower-is-better",
                "direction for {name}"
            );
            let value = outcome["value"].as_f64().expect("numeric metric");
            assert!(value.is_finite() && value >= 0.0, "{name}: {value}");
        }
        report
    }
}

fn run(inputs: Value, stdin: &[u8]) -> Run {
    let directory = tempfile::tempdir().unwrap();
    let inputs_path = directory.path().join("inputs.json");
    fs::write(&inputs_path, serde_json::to_vec(&inputs).unwrap()).unwrap();
    let mut child = Command::new(env!("CARGO_BIN_EXE_wezel_exec"))
        .env("FORAGER_INPUTS", inputs_path)
        .env("FORAGER_OUT", directory.path().join("out.json"))
        .env("WEZEL_EXEC_TEST_INHERITED", "inherited")
        .env("WEZEL_EXEC_TEST_OVERRIDE", "original")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    child.stdin.take().unwrap().write_all(stdin).unwrap();
    Run {
        directory,
        output: child.wait_with_output().unwrap(),
    }
}

fn metric(report: &Value, name: &str) -> f64 {
    report["outcomes"]
        .as_array()
        .unwrap()
        .iter()
        .find(|outcome| outcome["name"] == name)
        .unwrap()["value"]
        .as_f64()
        .unwrap()
}

#[test]
fn schema_describes_inputs_and_all_measurements() {
    let output = Command::new(env!("CARGO_BIN_EXE_wezel_exec"))
        .arg("--schema")
        .env_remove("FORAGER_INPUTS")
        .env_remove("FORAGER_OUT")
        .output()
        .unwrap();
    assert!(output.status.success());
    let schema: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(schema["name"], "exec");
    assert!(
        schema["inputs"]["required"]
            .as_array()
            .unwrap()
            .contains(&json!("cmd"))
    );
    for input in ["cmd", "env", "cwd"] {
        assert!(schema["inputs"]["properties"].get(input).is_some());
    }
    for (name, _) in METRICS {
        assert!(
            schema["outcomes_doc"].as_str().unwrap().contains(name),
            "schema does not document {name}"
        );
    }
}

#[test]
fn preserves_working_directory_environment_and_standard_streams() {
    let cwd = tempfile::tempdir().unwrap();
    fs::write(cwd.path().join("marker"), "present").unwrap();
    let result = run(
        json!({
            "cmd": "test -f marker || exit 9; IFS= read -r input; printf '%s/%s/%s' \"$WEZEL_EXEC_TEST_INHERITED\" \"$WEZEL_EXEC_TEST_OVERRIDE\" \"$input\"; printf 'stderr preserved' >&2",
            "cwd": cwd.path(),
            "env": { "WEZEL_EXEC_TEST_OVERRIDE": "overridden" }
        }),
        b"input preserved\n",
    );
    result.report();
    assert_eq!(
        result.output.stdout,
        b"inherited/overridden/input preserved"
    );
    assert_eq!(result.output.stderr, b"stderr preserved");
}

#[test]
fn accounts_for_a_waited_child_and_elapsed_waiting() {
    let result = run(
        json!({
            // The trailing exit keeps sh alive while it waits for the helper,
            // so this exercises descendant accounting as well as direct exec.
            "cmd": "\"$WEZEL_EXEC_TEST_HELPER\" --ignored --exact workload_helper --nocapture; exit $?",
            "env": { "WEZEL_EXEC_TEST_HELPER": std::env::current_exe().unwrap() }
        }),
        b"",
    );
    let report = result.report();
    // Wide bounds tolerate scheduling delays and platform accounting precision.
    assert!(metric(&report, "wall_time_ms") >= 150.0, "{report}");
    assert!(
        metric(&report, "user_time_ms") + metric(&report, "system_time_ms") >= 30.0,
        "{report}"
    );
    let rss = metric(&report, "max_rss_bytes");
    assert!(rss >= (24 * 1024 * 1024) as f64, "{report}");
    // Also detects accidentally treating macOS's byte value as Linux's KiB.
    assert!(rss < (1024 * 1024 * 1024) as f64, "{report}");
}

#[test]
fn failures_do_not_write_successful_reports() {
    let absent = tempfile::tempdir().unwrap();
    for inputs in [
        json!({ "cmd": "exit 7" }),
        json!({ "cmd": "kill -TERM $$" }),
        json!({ "cmd": "true", "cwd": absent.path().join("does-not-exist") }),
    ] {
        let result = run(inputs.clone(), b"");
        assert!(!result.output.status.success(), "{inputs}");
        assert!(!result.output.stderr.is_empty(), "missing error: {inputs}");
        assert!(
            !result.directory.path().join("out.json").exists(),
            "failed command wrote a report: {inputs}"
        );
    }
}

fn process_cpu_time() -> Duration {
    let mut time = std::mem::MaybeUninit::<libc::timespec>::uninit();
    // SAFETY: clock_gettime initializes the valid output pointer on success.
    assert_eq!(
        unsafe { libc::clock_gettime(libc::CLOCK_PROCESS_CPUTIME_ID, time.as_mut_ptr()) },
        0
    );
    // SAFETY: the successful call above initialized both fields.
    let time = unsafe { time.assume_init() };
    Duration::new(
        time.tv_sec.try_into().unwrap(),
        time.tv_nsec.try_into().unwrap(),
    )
}

#[test]
#[ignore = "invoked as a measured subprocess by accounts_for_a_waited_child_and_elapsed_waiting"]
fn workload_helper() {
    let mut allocation = vec![0_u8; 32 * 1024 * 1024];
    // Touch every page to commit the allocation without relying on allocator
    // zero filling or the optimizer retaining unused memory.
    for offset in (0..allocation.len()).step_by(4096) {
        allocation[offset] = 1;
    }
    let start = process_cpu_time();
    let mut value = 1_u64;
    while process_cpu_time() - start < Duration::from_millis(40) {
        for _ in 0..10_000 {
            value = black_box(value.wrapping_mul(6364136223846793005).wrapping_add(1));
        }
    }
    std::thread::sleep(Duration::from_millis(150));
    black_box(&allocation);
    black_box(value);
}
