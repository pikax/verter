//! When containment or telemetry cannot be established the child never
//! runs; when either is lost mid-run the tree is killed. Missing telemetry is
//! `null`, never zero.

use std::process::Stdio;

use super::support::{fixture, scratch, str_field, supervise_in, supervisor};

const LIMITS: &[&str] = &["--mem-mb", "256", "--timeout-ms", "30000"];

fn errors(result: &serde_json::Value) -> Vec<String> {
    result["errors"]
        .as_array()
        .expect("errors array")
        .iter()
        .map(|error| error.as_str().unwrap().to_owned())
        .collect()
}

/// A refused run: exit 125, the program never ran, and nothing reads as a
/// measurement.
fn assert_refused(run: &super::support::Run, marker: &std::path::Path, reason: &str) {
    assert_eq!(run.code, Some(125), "stderr: {}", run.stderr);
    assert!(
        !marker.exists(),
        "the program ran although launch was refused"
    );
    let result = &run.result;
    assert_eq!(result["launched"], false);
    assert!(result["exitCode"].is_null());
    assert!(result["wallMs"].is_null());
    assert!(
        result["peakBytes"].is_null(),
        "missing telemetry must be null, never zero: {result}"
    );
    let errors = errors(result);
    assert!(
        errors.iter().any(|error| error.contains(reason)),
        "errors {errors:?} do not mention {reason:?}"
    );
    assert!(run.stderr.contains(reason), "stderr: {}", run.stderr);
}

#[test]
#[cfg_attr(
    target_os = "linux",
    ignore = "needs a delegated cgroup v2 subtree (systemd-run --user --scope -p Delegate=yes)"
)]
fn a_program_that_cannot_be_spawned_is_reported_not_run() {
    let dir = scratch("spawn-failure");
    let missing = dir.join("no-such-program-verter");
    let output = std::process::Command::new(supervisor())
        .arg("run")
        .args(LIMITS)
        .args(super::support::platform_flags())
        .arg("--out")
        .arg(dir.join("result.json"))
        .arg("--")
        .arg(&missing)
        .stdin(Stdio::null())
        .output()
        .unwrap();
    let run = super::support::finish(&dir, output.status.code(), &output.stderr);
    assert_refused(&run, &dir.join("never-created"), "spawn");
}

#[test]
#[cfg_attr(
    target_os = "linux",
    ignore = "needs a delegated cgroup v2 subtree (systemd-run --user --scope -p Delegate=yes)"
)]
fn a_failed_telemetry_probe_refuses_to_launch() {
    let dir = scratch("telemetry-refusal");
    let marker = dir.join("ran");
    let marker_text = marker.to_string_lossy().into_owned();
    let run = supervise_in(
        &dir,
        LIMITS,
        &["touch", &marker_text],
        &[(verter_supervise::FAULT_ENV, "telemetry")],
    );
    assert_refused(&run, &marker, "telemetry");
}

#[test]
fn a_failure_to_establish_containment_refuses_to_launch() {
    let dir = scratch("containment-refusal");
    let marker = dir.join("ran");
    let marker_text = marker.to_string_lossy().into_owned();
    let run = supervise_in(
        &dir,
        LIMITS,
        &["touch", &marker_text],
        &[(verter_supervise::FAULT_ENV, "containment")],
    );
    assert_refused(&run, &marker, "containment");
}

#[test]
#[cfg_attr(
    target_os = "linux",
    ignore = "needs a delegated cgroup v2 subtree (systemd-run --user --scope -p Delegate=yes)"
)]
fn telemetry_lost_mid_run_kills_the_tree() {
    let dir = scratch("telemetry-midrun");
    let run = supervise_in(
        &dir,
        &[
            "--mem-mb",
            "256",
            "--timeout-ms",
            "30000",
            "--sample-ms",
            "10",
        ],
        &["sleep", "20000"],
        &[(verter_supervise::FAULT_ENV, "telemetry-midrun")],
    );
    assert_eq!(run.code, Some(125), "stderr: {}", run.stderr);
    assert_eq!(str_field(&run.result, "killedBy"), Some("supervisor-error"));
    assert_eq!(run.result["launched"], true);
    assert!(run.result["wallMs"].as_f64().unwrap() < 10_000.0);
    assert!(errors(&run.result)
        .iter()
        .any(|error| error.contains("telemetry")));
}

#[test]
fn a_usage_error_launches_nothing() {
    let dir = scratch("usage");
    let marker = dir.join("ran");
    let output = std::process::Command::new(supervisor())
        .args(["run", "--mem-mb", "0", "--timeout-ms", "1000", "--out"])
        .arg(dir.join("result.json"))
        .arg("--")
        .arg(fixture())
        .arg("touch")
        .arg(&marker)
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(125));
    assert!(!marker.exists());
    assert!(String::from_utf8_lossy(&output.stderr).contains("--mem-mb must be a positive integer"));
}

/// Without explicit consent the sampled macOS backend refuses: sampling is
/// never selected silently.
#[cfg(target_os = "macos")]
#[test]
fn the_sampled_backend_refuses_without_consent() {
    let dir = scratch("no-consent");
    let marker = dir.join("ran");
    let output = std::process::Command::new(supervisor())
        .arg("run")
        .args(LIMITS)
        .arg("--out")
        .arg(dir.join("result.json"))
        .arg("--")
        .arg(fixture())
        .arg("touch")
        .arg(&marker)
        .output()
        .unwrap();
    let run = super::support::finish(&dir, output.status.code(), &output.stderr);
    assert_refused(&run, &marker, "--allow-sampled");
}

/// On Linux without a delegated cgroup v2 subtree the supervisor refuses;
/// with one it launches under a hard cap. Either way it never runs the
/// program uncontained.
#[cfg(target_os = "linux")]
#[test]
fn linux_launches_only_inside_a_cgroup() {
    let dir = scratch("linux-cgroup");
    let marker = dir.join("ran");
    let marker_text = marker.to_string_lossy().into_owned();
    let run = supervise_in(&dir, LIMITS, &["touch", &marker_text], &[]);
    if run.code == Some(125) {
        assert_refused(&run, &marker, "cgroup");
    } else {
        assert_eq!(run.code, Some(0), "stderr: {}", run.stderr);
        assert!(marker.exists());
        assert_eq!(str_field(&run.result, "containment"), Some("hard"));
        assert_eq!(str_field(&run.result, "backend"), Some("linux-cgroup-v2"));
    }
}
