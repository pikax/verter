//! `verter-supervise`: launch a command under OS-backed process-tree memory
//! containment and a deadline, tear the whole tree down on every exit path,
//! and report truthful telemetry.
//!
//! Backends, strongest first:
//!
//! - Windows (`windows-job-object`, hard): the child is created suspended and
//!   already inside a job object with a job-wide committed-memory limit,
//!   kill-on-close and no breakaway, and only then resumed. The kernel refuses
//!   commit past the cap; the limit notification kills the tree; the
//!   supervisor's death closes the job and kills the tree.
//! - Linux (`linux-cgroup-v2`, hard): a dedicated cgroup v2 with `memory.max`,
//!   `memory.swap.max=0` and `memory.oom.group=1`, joined by the child before
//!   `exec`. Without a delegated cgroup the supervisor refuses.
//! - macOS (`macos-phys-footprint`, sampled): the child is spawned suspended
//!   into its own process group; the supervisor samples the summed
//!   `phys_footprint` of every tracked descendant and has the group killed
//!   on a breach. Sampling proves no overshoot bound, so this backend runs
//!   only with `--allow-sampled`.
//!
//! On Unix a watchdog process (see `sentinel`) owns the workload: it spawns
//! it, observes its exit, tears the tree down and reaps it, and does so on its
//! own when the supervisor dies. Nothing is ever killed by a cached process
//! id: Linux kills through the cgroup's `cgroup.kill`, macOS through the
//! process group of an unreaped leader.
//!
//! Fail closed: when containment or telemetry cannot be established the child
//! never runs (exit 125, `launched: false`); when either is lost mid-run the
//! tree is killed (`killedBy: "supervisor-error"`, exit 125).

pub mod cli;
pub mod report;

#[cfg(target_os = "linux")]
mod linux;
#[cfg(target_os = "macos")]
mod macos;
#[cfg(unix)]
mod sentinel;
#[cfg(unix)]
mod unix;
#[cfg(windows)]
mod windows;

use std::ffi::OsString;
use std::fs::File;

use cli::RunSpec;
use report::Report;

/// The environment variable that injects a supervisor fault, for the tests
/// that prove each fail-closed path. It is never passed to the child.
///
/// - `containment`: establishing containment fails.
/// - `telemetry`: the pre-launch telemetry probe fails.
/// - `telemetry-midrun`: the third telemetry observation fails.
/// - `die-after-spawn`: the supervisor dies, as a crash would, right after
///   the child exists and before it is released (it prints the child's pid).
pub const FAULT_ENV: &str = "VERTER_SUPERVISE_FAULT";

/// An injected fault (see [`FAULT_ENV`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Fault {
    Containment,
    Telemetry,
    TelemetryMidrun,
    DieAfterSpawn,
}

impl Fault {
    fn from_env() -> Option<Fault> {
        match std::env::var(FAULT_ENV).ok()?.as_str() {
            "containment" => Some(Fault::Containment),
            "telemetry" => Some(Fault::Telemetry),
            "telemetry-midrun" => Some(Fault::TelemetryMidrun),
            "die-after-spawn" => Some(Fault::DieAfterSpawn),
            _ => None,
        }
    }
}

/// Which observation an injected mid-run telemetry fault fails.
pub(crate) const MIDRUN_FAULT_OBSERVATION: u64 = 3;

/// The child's stdout and stderr files.
/// On Unix they only prove the paths are writable before launch: the watchdog
/// reopens them by path for the child it spawns.
#[cfg_attr(unix, allow(dead_code))]
pub(crate) struct Outputs {
    pub stdout: File,
    pub stderr: File,
}

/// Everything a backend needs to run one child.
pub(crate) struct Launch<'a> {
    pub spec: &'a RunSpec,
    #[cfg_attr(unix, allow(dead_code))]
    pub outputs: Outputs,
    pub fault: Option<Fault>,
}

/// The backend this build runs, as named in `result.json`.
pub const BACKEND: &str = if cfg!(windows) {
    "windows-job-object"
} else if cfg!(target_os = "linux") {
    "linux-cgroup-v2"
} else if cfg!(target_os = "macos") {
    "macos-phys-footprint"
} else {
    "unsupported"
};

/// Run the CLI with the process's arguments and return the exit code.
pub fn main_entry() -> i32 {
    let args: Vec<OsString> = std::env::args_os().skip(1).collect();
    #[cfg(unix)]
    if args
        .first()
        .is_some_and(|arg| arg == sentinel::SENTINEL_ARG)
    {
        return sentinel::run();
    }
    match cli::parse(args) {
        Ok(spec) => run(&spec),
        Err(error) => {
            eprintln!("verter-supervise: {}", error.message);
            report::EXIT_SUPERVISOR_ERROR
        }
    }
}

/// Supervise one run described by `spec`, write its result document, and
/// return the supervisor's exit code.
pub fn run(spec: &RunSpec) -> i32 {
    let mut report = Report::new(spec, BACKEND);
    let fault = Fault::from_env();
    match open_outputs(spec) {
        Ok(outputs) => {
            let launch = Launch {
                spec,
                outputs,
                fault,
            };
            supervise(launch, &mut report);
        }
        Err(error) => report
            .errors
            .push(format!("cannot create the child's output files: {error}")),
    }
    report.settle_overshoot();
    let code = report.exit_code();
    for error in &report.errors {
        eprintln!("verter-supervise: {error}");
    }
    if let Err(error) = report.write(&spec.out) {
        eprintln!(
            "verter-supervise: cannot write {}: {error}",
            spec.out.display()
        );
        return report::EXIT_SUPERVISOR_ERROR;
    }
    code
}

fn open_outputs(spec: &RunSpec) -> std::io::Result<Outputs> {
    if let Some(parent) = spec
        .out
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
    {
        std::fs::create_dir_all(parent)?;
    }
    Ok(Outputs {
        stdout: File::create(spec.stdout_path())?,
        stderr: File::create(spec.stderr_path())?,
    })
}

#[cfg(windows)]
fn supervise(launch: Launch<'_>, report: &mut Report) {
    windows::supervise(launch, report);
}

#[cfg(target_os = "linux")]
fn supervise(launch: Launch<'_>, report: &mut Report) {
    linux::supervise(launch, report);
}

#[cfg(target_os = "macos")]
fn supervise(launch: Launch<'_>, report: &mut Report) {
    macos::supervise(launch, report);
}

#[cfg(not(any(windows, target_os = "linux", target_os = "macos")))]
fn supervise(_launch: Launch<'_>, report: &mut Report) {
    report.errors.push(
        "this platform has no process-tree containment backend; refusing to launch".to_owned(),
    );
}
