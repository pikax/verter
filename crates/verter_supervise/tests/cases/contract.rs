//! The result document and exit-code contract of an ordinary run.

use super::support::{
    fixture, scratch, str_field, supervise, supervise_in, u64_field, EXPECTED_BACKEND,
    EXPECTED_CONTAINMENT,
};

const LIMITS: &[&str] = &["--mem-mb", "512", "--timeout-ms", "30000"];

#[test]
#[cfg_attr(
    target_os = "linux",
    ignore = "needs a delegated cgroup v2 subtree (systemd-run --user --scope -p Delegate=yes)"
)]
fn the_child_exit_code_passes_through_with_a_complete_result() {
    let run = supervise("exit-code", LIMITS, &["exit", "7"]);
    assert_eq!(run.code, Some(7), "stderr: {}", run.stderr);
    let result = &run.result;
    assert_eq!(u64_field(result, "schema"), Some(1));
    assert_eq!(result["exitCode"], 7);
    assert!(result["killedBy"].is_null());
    assert!(result["signal"].is_null());
    assert_eq!(result["launched"], true);
    assert_eq!(str_field(result, "backend"), Some(EXPECTED_BACKEND));
    assert_eq!(str_field(result, "containment"), Some(EXPECTED_CONTAINMENT));
    assert_eq!(
        str_field(result, "program"),
        Some(fixture().to_string_lossy().as_ref())
    );
    assert_eq!(result["args"], serde_json::json!(["exit", "7"]));
    assert!(str_field(result, "startedAt").is_some_and(|at| at.ends_with('Z')));
    assert!(result["wallMs"].as_f64().is_some_and(|ms| ms >= 0.0));
    assert!(
        u64_field(result, "peakBytes").is_some_and(|peak| peak > 0),
        "a run that executed has a measured, non-zero peak: {result}"
    );
    assert!(str_field(result, "peakMetric").is_some());
    assert_eq!(u64_field(result, "memLimitBytes"), Some(512 * 1024 * 1024));
    assert_eq!(result["errors"], serde_json::json!([]));

    let zero = supervise("exit-zero", LIMITS, &["exit", "0"]);
    assert_eq!(zero.code, Some(0));
    assert_eq!(zero.result["exitCode"], 0);
}

#[test]
#[cfg_attr(
    target_os = "linux",
    ignore = "needs a delegated cgroup v2 subtree (systemd-run --user --scope -p Delegate=yes)"
)]
fn stdout_and_stderr_land_in_files_next_to_the_result() {
    let run = supervise(
        "stdio",
        LIMITS,
        &["stdio", "to-stdout\nline two", "to-stderr"],
    );
    assert_eq!(run.code, Some(0), "stderr: {}", run.stderr);
    let stdout_path = str_field(&run.result, "stdoutPath").expect("stdoutPath");
    let stderr_path = str_field(&run.result, "stderrPath").expect("stderrPath");
    assert_eq!(
        std::path::Path::new(stdout_path),
        run.dir.join("result.stdout.log")
    );
    assert_eq!(
        std::path::Path::new(stderr_path),
        run.dir.join("result.stderr.log")
    );
    assert_eq!(
        std::fs::read_to_string(stdout_path).unwrap(),
        "to-stdout\nline two"
    );
    assert_eq!(std::fs::read_to_string(stderr_path).unwrap(), "to-stderr");
}

#[test]
#[cfg_attr(
    target_os = "linux",
    ignore = "needs a delegated cgroup v2 subtree (systemd-run --user --scope -p Delegate=yes)"
)]
fn cwd_and_env_reach_the_child_and_the_fault_hook_does_not() {
    let dir = scratch("env");
    let work = dir.join("work dir");
    std::fs::create_dir_all(&work).unwrap();
    let mut limits: Vec<&str> = LIMITS.to_vec();
    let work_text = work.to_string_lossy().into_owned();
    limits.extend(["--cwd", &work_text, "--env", "SUPERVISE_PROBE=a=b"]);
    let run = supervise_in(&dir, &limits, &["env", "SUPERVISE_PROBE"], &[]);
    assert_eq!(run.code, Some(0), "stderr: {}", run.stderr);
    let stdout = std::fs::read_to_string(dir.join("result.stdout.log")).unwrap();
    assert!(stdout.contains("SUPERVISE_PROBE=a=b"), "{stdout}");
    let cwd = stdout
        .lines()
        .find_map(|line| line.strip_prefix("cwd="))
        .expect("cwd line");
    assert_eq!(
        std::fs::canonicalize(cwd).unwrap(),
        std::fs::canonicalize(&work).unwrap()
    );

    let hook = supervise_in(
        &dir,
        LIMITS,
        &["env", verter_supervise::FAULT_ENV],
        &[(verter_supervise::FAULT_ENV, "unknown-fault")],
    );
    assert_eq!(hook.code, Some(0), "stderr: {}", hook.stderr);
    let stdout = std::fs::read_to_string(dir.join("result.stdout.log")).unwrap();
    assert!(
        stdout.contains(&format!("{}=<unset>", verter_supervise::FAULT_ENV)),
        "the fault hook must never reach the child: {stdout}"
    );
}
