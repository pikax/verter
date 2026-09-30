//! A runaway allocation, alone or handed to a grandchild, is stopped at the
//! cap, and nothing in the tree survives the kill.

use std::time::Duration;

use super::support::{
    exits_within, kill_pid, published_pid, scratch, str_field, supervise, supervise_in, u64_field,
    CAP_MB, CHUNK_MB, EXPECTED_CONTAINMENT,
};

const MIB: u64 = 1024 * 1024;

fn cap_limits() -> Vec<String> {
    vec![
        "--mem-mb".to_owned(),
        CAP_MB.to_string(),
        "--timeout-ms".to_owned(),
        "60000".to_owned(),
    ]
}

fn as_strs(list: &[String]) -> Vec<&str> {
    list.iter().map(String::as_str).collect()
}

/// The peak of a run killed at the cap: at least the kill trigger less two
/// allocation steps. On Windows the commit the tree was granted never exceeds
/// the cap (the fixture reports what it held when the kernel refused it); the
/// job's peak charge also counts the refused request, so it may sit above the
/// cap by at most that request plus allocator overhead. A Linux cgroup's peak
/// never exceeds its `memory.max`.
fn assert_peak_near_cap(run: &super::support::Run) {
    let result = &run.result;
    let cap = CAP_MB * MIB;
    let peak = u64_field(result, "peakBytes").expect("a measured peak");
    let trigger = u64_field(result, "killTriggerBytes").expect("killTriggerBytes");
    eprintln!(
        "cap {cap} trigger {trigger} peak {peak} ({} MiB) metric {:?} overshoot {:?}",
        peak / MIB,
        str_field(result, "peakMetric"),
        result["observedOvershootBytes"]
    );
    assert!(
        peak + 2 * CHUNK_MB * MIB >= trigger,
        "peak {peak} is not near the kill trigger {trigger}: {result}"
    );
    if cfg!(windows) {
        let stdout = std::fs::read_to_string(run.dir.join("result.stdout.log")).unwrap();
        let granted: u64 = stdout
            .lines()
            .find_map(|line| line.strip_prefix("allocation refused after "))
            .and_then(|rest| rest.strip_suffix(" bytes"))
            .unwrap_or_else(|| panic!("the kernel never refused an allocation: {stdout}"))
            .parse()
            .unwrap();
        assert!(
            granted <= cap,
            "granted {granted} exceeds the hard cap {cap}"
        );
        assert!(
            peak <= cap + CHUNK_MB * MIB + 16 * MIB,
            "the peak charge {peak} exceeds the cap by more than one refused request"
        );
        assert_eq!(u64_field(result, "overshootBoundBytes"), Some(0));
    } else if EXPECTED_CONTAINMENT == "hard" {
        // The cgroup's usage never exceeds `memory.max`: the kernel reclaims
        // or OOM-kills instead of charging past it.
        assert!(peak <= cap, "a hard cap was exceeded: peak {peak} > {cap}");
        assert_eq!(u64_field(result, "overshootBoundBytes"), Some(0));
    } else {
        assert!(result["overshootBoundBytes"].is_null());
        assert!(peak < 2 * cap, "sampled overshoot ran away: {peak}");
    }
}

#[test]
#[cfg_attr(
    target_os = "linux",
    ignore = "needs a delegated cgroup v2 subtree (systemd-run --user --scope -p Delegate=yes)"
)]
fn an_allocation_explosion_is_killed_at_the_cap() {
    let limits = cap_limits();
    let run = supervise(
        "alloc",
        &as_strs(&limits),
        &["alloc", &CHUNK_MB.to_string()],
    );
    assert_eq!(run.code, Some(137), "stderr: {}", run.stderr);
    assert_eq!(str_field(&run.result, "killedBy"), Some("memory"));
    assert_eq!(run.result["errors"], serde_json::json!([]));
    assert_peak_near_cap(&run);
    let wall = run.result["wallMs"].as_f64().unwrap();
    assert!(wall < 30_000.0, "the kill came from the cap, not the clock");
}

#[test]
#[cfg_attr(
    target_os = "linux",
    ignore = "needs a delegated cgroup v2 subtree (systemd-run --user --scope -p Delegate=yes)"
)]
fn a_grandchild_allocation_is_contained_and_no_process_survives() {
    let dir = scratch("grandchild-alloc");
    let pidfile = dir.join("grandchild.pid");
    let pidfile_text = pidfile.to_string_lossy().into_owned();
    let limits = cap_limits();
    let chunk = CHUNK_MB.to_string();
    let run = supervise_in(
        &dir,
        &as_strs(&limits),
        &["spawn-wait", &pidfile_text, "alloc", &chunk],
        &[],
    );
    let grandchild = published_pid(&pidfile);
    let gone = exits_within(grandchild, Duration::from_secs(5));
    if !gone {
        kill_pid(grandchild);
    }
    assert!(
        gone,
        "the allocating grandchild {grandchild} survived the kill"
    );
    assert_eq!(run.code, Some(137), "stderr: {}", run.stderr);
    assert_eq!(str_field(&run.result, "killedBy"), Some("memory"));
    if cfg!(windows) {
        assert!(u64_field(&run.result, "processCount").is_some_and(|count| count >= 2));
    }
    assert_peak_near_cap(&run);
}

/// A grandchild that asks to leave the job is refused, so it stays inside
/// the cap and dies with the tree.
#[cfg(windows)]
#[test]
fn a_grandchild_cannot_break_away_from_the_job() {
    let dir = scratch("breakaway");
    let pidfile = dir.join("grandchild.pid");
    let pidfile_text = pidfile.to_string_lossy().into_owned();
    let run = supervise_in(
        &dir,
        &["--mem-mb", "256", "--timeout-ms", "1500"],
        &["spawn-breakaway", &pidfile_text, "sleep", "60000"],
        &[],
    );
    let grandchild = published_pid(&pidfile);
    let gone = exits_within(grandchild, Duration::from_secs(5));
    if !gone {
        kill_pid(grandchild);
    }
    let stdout = std::fs::read_to_string(dir.join("result.stdout.log")).unwrap();
    assert!(stdout.contains("breakaway refused"), "{stdout}");
    assert!(gone, "a breakaway grandchild {grandchild} survived");
    assert_eq!(run.code, Some(124), "stderr: {}", run.stderr);
    assert_eq!(str_field(&run.result, "killedBy"), Some("timeout"));
}

/// A grandchild that starts a new session leaves the process group. The
/// cgroup still holds it on Linux; on macOS the sampler detects the escape
/// and fails the run closed.
#[cfg(unix)]
#[test]
#[cfg_attr(
    target_os = "linux",
    ignore = "needs a delegated cgroup v2 subtree (systemd-run --user --scope -p Delegate=yes)"
)]
fn a_grandchild_that_leaves_the_process_group_does_not_escape() {
    let dir = scratch("setsid");
    let pidfile = dir.join("grandchild.pid");
    let pidfile_text = pidfile.to_string_lossy().into_owned();
    let run = supervise_in(
        &dir,
        &["--mem-mb", "256", "--timeout-ms", "3000"],
        &["spawn-setsid", &pidfile_text, "sleep", "60000"],
        &[],
    );
    // On macOS the escape is detected, and the tree killed, as soon as the
    // grandchild leaves the group: possibly before the fixture publishes its
    // pid. The report names it instead.
    let grandchild = if cfg!(target_os = "macos") {
        reported_escapee(&run.result)
    } else {
        published_pid(&pidfile)
    };
    let gone = exits_within(grandchild, Duration::from_secs(5));
    if !gone {
        kill_pid(grandchild);
    }
    assert!(gone, "the new-session grandchild {grandchild} survived");
    if cfg!(target_os = "macos") {
        assert_eq!(run.code, Some(125), "stderr: {}", run.stderr);
        assert_eq!(str_field(&run.result, "killedBy"), Some("supervisor-error"));
    } else {
        assert_eq!(run.code, Some(124), "stderr: {}", run.stderr);
        assert_eq!(str_field(&run.result, "killedBy"), Some("timeout"));
    }
}

/// The pid of the descendant the result reports as having left the group.
#[cfg(unix)]
fn reported_escapee(result: &serde_json::Value) -> u32 {
    result["errors"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|error| error.as_str())
        .find_map(|error| {
            error
                .strip_prefix("descendant ")?
                .split_once(" left the process group")?
                .0
                .parse()
                .ok()
        })
        .unwrap_or_else(|| panic!("no escape reported: {result}"))
}
