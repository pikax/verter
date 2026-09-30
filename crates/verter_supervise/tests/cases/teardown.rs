//! Every way a run ends tears the whole tree down: the child exiting, the
//! deadline, cancellation of the supervisor, and the supervisor dying.

use std::time::{Duration, Instant};

use super::support::{
    command, exits_within, finish, kill_pid, published_pid, scratch, str_field, supervise,
    supervise_in, u64_field, wait_with_limit,
};

/// Assert every process in `tree` is gone, killing any survivor first so a
/// failing case leaks nothing.
fn assert_gone(tree: &[(u32, &str)]) {
    let survivors: Vec<&(u32, &str)> = tree
        .iter()
        .filter(|(pid, _)| !exits_within(*pid, Duration::from_secs(5)))
        .collect();
    for (pid, _) in &survivors {
        kill_pid(*pid);
    }
    assert!(survivors.is_empty(), "survived teardown: {survivors:?}");
}

#[test]
#[cfg_attr(
    target_os = "linux",
    ignore = "needs a delegated cgroup v2 subtree (systemd-run --user --scope -p Delegate=yes)"
)]
fn the_deadline_kills_the_tree_with_exit_124() {
    let started = Instant::now();
    let run = supervise(
        "timeout",
        &["--mem-mb", "256", "--timeout-ms", "700"],
        &["sleep", "60000"],
    );
    let elapsed = started.elapsed();
    assert_eq!(run.code, Some(124), "stderr: {}", run.stderr);
    assert_eq!(str_field(&run.result, "killedBy"), Some("timeout"));
    let wall = run.result["wallMs"].as_f64().unwrap();
    assert!((700.0..10_000.0).contains(&wall), "wallMs {wall}");
    assert!(elapsed < Duration::from_secs(20), "took {elapsed:?}");
    assert!(u64_field(&run.result, "peakBytes").is_some());
}

#[test]
#[cfg_attr(
    target_os = "linux",
    ignore = "needs a delegated cgroup v2 subtree (systemd-run --user --scope -p Delegate=yes)"
)]
fn descendants_left_behind_by_an_exiting_child_are_killed() {
    let dir = scratch("orphan");
    let pidfile = dir.join("grandchild.pid");
    let pidfile_text = pidfile.to_string_lossy().into_owned();
    let run = supervise_in(
        &dir,
        &["--mem-mb", "256", "--timeout-ms", "30000"],
        &["spawn-exit", &pidfile_text, "sleep", "60000"],
        &[],
    );
    let grandchild = published_pid(&pidfile);
    assert_gone(&[(grandchild, "the orphaned grandchild")]);
    assert_eq!(run.code, Some(0), "stderr: {}", run.stderr);
    assert!(run.result["killedBy"].is_null());
    assert!(
        u64_field(&run.result, "descendantsKilled").is_some_and(|killed| killed >= 1),
        "{}",
        run.result
    );
}

/// Start a supervised `spawn-wait … sleep` tree and return the supervisor,
/// its scratch directory and the child and grandchild pids once the tree is up.
fn running_tree(name: &str) -> (std::process::Child, std::path::PathBuf, u32, u32) {
    let dir = scratch(name);
    let pidfile = dir.join("grandchild.pid");
    let pidfile_text = pidfile.to_string_lossy().into_owned();
    let mut command = command(
        &dir,
        &["--mem-mb", "256", "--timeout-ms", "60000"],
        &["spawn-wait", &pidfile_text, "sleep", "60000"],
    );
    new_process_group(&mut command);
    let child = command.spawn().expect("launch verter-supervise");
    let grandchild = published_pid(&pidfile);
    let parent = published_pid(&pidfile.with_extension("parent"));
    (child, dir, parent, grandchild)
}

#[cfg(windows)]
fn new_process_group(command: &mut std::process::Command) {
    use std::os::windows::process::CommandExt;
    const CREATE_NEW_PROCESS_GROUP: u32 = 0x0000_0200;
    command.creation_flags(CREATE_NEW_PROCESS_GROUP);
}

#[cfg(unix)]
fn new_process_group(command: &mut std::process::Command) {
    use std::os::unix::process::CommandExt;
    command.process_group(0);
}

/// Deliver the platform's interactive cancellation to the supervisor.
#[cfg(windows)]
fn cancel(supervisor: &std::process::Child) {
    use windows_sys::Win32::System::Console::{GenerateConsoleCtrlEvent, CTRL_BREAK_EVENT};
    // SAFETY: plain Win32 call; the supervisor leads its own process group.
    let sent = unsafe { GenerateConsoleCtrlEvent(CTRL_BREAK_EVENT, supervisor.id()) };
    assert_ne!(
        sent,
        0,
        "GenerateConsoleCtrlEvent failed: {}",
        std::io::Error::last_os_error()
    );
}

#[cfg(unix)]
fn cancel(supervisor: &std::process::Child) {
    // SAFETY: sending a signal has no memory-safety preconditions.
    unsafe {
        libc::kill(supervisor.id() as libc::pid_t, libc::SIGTERM);
    }
}

#[test]
#[cfg_attr(
    target_os = "linux",
    ignore = "needs a delegated cgroup v2 subtree (systemd-run --user --scope -p Delegate=yes)"
)]
fn cancelling_the_supervisor_kills_the_tree() {
    let (mut supervisor, dir, child, grandchild) = running_tree("cancel");
    cancel(&supervisor);
    let code = wait_with_limit(&mut supervisor, Duration::from_secs(20));
    assert_gone(&[
        (child, "the child of a cancelled run"),
        (grandchild, "the grandchild of a cancelled run"),
    ]);
    let run = finish(&dir, code, b"");
    assert_eq!(run.code, Some(130));
    assert_eq!(str_field(&run.result, "killedBy"), Some("cancel"));
}

#[test]
#[cfg_attr(
    target_os = "linux",
    ignore = "needs a delegated cgroup v2 subtree (systemd-run --user --scope -p Delegate=yes)"
)]
fn a_killed_supervisor_takes_the_tree_with_it() {
    let (mut supervisor, dir, child, grandchild) = running_tree("crash");
    // TerminateProcess on Windows, SIGKILL on Unix: no handler runs.
    supervisor.kill().expect("kill the supervisor");
    supervisor.wait().expect("reap the supervisor");
    assert_gone(&[
        (child, "the child of a killed supervisor"),
        (grandchild, "the grandchild of a killed supervisor"),
    ]);
    assert!(
        !dir.join("result.json").exists(),
        "a killed supervisor writes no result; a missing result is the caller's failure signal"
    );
}

/// The supervisor dies after the child exists but before it is released or
/// registered anywhere. The child (suspended on Windows and macOS, already
/// running on Linux) must not outlive it: the job (Windows) or the watchdog
/// that spawned it (Unix) tears it down.
#[test]
#[cfg_attr(
    target_os = "linux",
    ignore = "needs a delegated cgroup v2 subtree (systemd-run --user --scope -p Delegate=yes)"
)]
fn a_supervisor_dying_between_spawn_and_release_leaves_no_orphan() {
    let dir = scratch("die-after-spawn");
    let pidfile = dir.join("grandchild.pid");
    let pidfile_text = pidfile.to_string_lossy().into_owned();
    let run = supervise_in_raw(
        &dir,
        &["--mem-mb", "256", "--timeout-ms", "60000"],
        &["spawn-wait", &pidfile_text, "sleep", "60000"],
        &[(verter_supervise::FAULT_ENV, "die-after-spawn")],
    );
    let child: u32 = run
        .1
        .lines()
        .find_map(|line| line.strip_prefix("verter-supervise: fault: dying after spawning pid "))
        .unwrap_or_else(|| panic!("the fault never fired; stderr: {}", run.1))
        .trim()
        .parse()
        .unwrap();
    // Give a running (Linux) child the chance to start its grandchild.
    std::thread::sleep(Duration::from_millis(300));
    let mut tree = vec![(
        child,
        "the child of a supervisor that died after spawning it",
    )];
    if let Ok(text) = std::fs::read_to_string(&pidfile) {
        tree.push((
            text.trim().parse().unwrap(),
            "the grandchild of a supervisor that died after spawning its parent",
        ));
    }
    assert_gone(&tree);
    assert_ne!(run.0, Some(0));
    assert!(!dir.join("result.json").exists());
}

/// Run the supervisor to completion and return its exit code and stderr,
/// without requiring a result document.
fn supervise_in_raw(
    dir: &std::path::Path,
    limits: &[&str],
    fixture_args: &[&str],
    env: &[(&str, &str)],
) -> (Option<i32>, String) {
    let mut command = command(dir, limits, fixture_args);
    for (key, value) in env {
        command.env(key, value);
    }
    let output = command.output().expect("launch verter-supervise");
    (
        output.status.code(),
        String::from_utf8_lossy(&output.stderr).into_owned(),
    )
}
