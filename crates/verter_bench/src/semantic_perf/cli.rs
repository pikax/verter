//! Command line of the semantic benchmark's Verter probe binaries.
//!
//! ```text
//! semantic_perf_probe run --job <job.json> --out <result.json>
//! semantic_perf_probe stats --pid <pid>
//! semantic_perf_probe identity
//! ```
//!
//! `run` answers one job and writes its record (and a `<out>.phase` marker
//! naming the phase running); the record file is the
//! only result channel (stdout carries nothing the harness parses). `stats`
//! prints one [`super::process_stats::ProcessStats`] reading of another
//! process as JSON — the tsc driver reads the tsc API server through it, so
//! both tools are measured by the same code. `identity` prints what this
//! binary was built as.

use std::process::ExitCode;

use serde::Serialize;

use super::{process_stats, run_job, AllocHooks, Job, Sink};

const USAGE: &str = "usage:
  semantic_perf_probe run --job <job.json> --out <result.json>
  semantic_perf_probe stats --pid <pid>
  semantic_perf_probe identity";

fn flag<'a>(args: &'a [String], name: &str) -> Option<&'a str> {
    args.iter()
        .position(|arg| arg == name)
        .and_then(|index| args.get(index + 1))
        .map(String::as_str)
}

/// What the binary was compiled as.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct Identity {
    tool: &'static str,
    crate_version: &'static str,
    instrumented: bool,
    debug_assertions: bool,
    target_os: &'static str,
    target_arch: &'static str,
    /// The hardware's architecture, whatever this process runs as.
    native_arch: Option<&'static str>,
    job_schema: u32,
    result_schema: u32,
}

/// Entry point shared by the plain and the instrumented binary.
pub fn main(alloc: Option<AllocHooks>) -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.first().map(String::as_str) {
        Some("run") => {
            let (Some(job_path), Some(out_path)) = (flag(&args, "--job"), flag(&args, "--out"))
            else {
                eprintln!("{USAGE}");
                return ExitCode::from(2);
            };
            let job: Job = match std::fs::read_to_string(job_path)
                .map_err(|err| err.to_string())
                .and_then(|text| serde_json::from_str(&text).map_err(|err| err.to_string()))
            {
                Ok(job) => job,
                Err(err) => {
                    eprintln!("semantic_perf_probe: read the job {job_path}: {err}");
                    return ExitCode::from(2);
                }
            };
            let sink = Sink::new(std::path::Path::new(out_path));
            match run_job(&job, alloc, &sink) {
                Ok(_) => ExitCode::SUCCESS,
                Err(err) => {
                    eprintln!("semantic_perf_probe: {err}");
                    ExitCode::from(3)
                }
            }
        }
        Some("stats") => {
            let Some(pid) = flag(&args, "--pid").and_then(|pid| pid.parse::<u32>().ok()) else {
                eprintln!("{USAGE}");
                return ExitCode::from(2);
            };
            match process_stats::for_pid(pid) {
                Ok(stats) => {
                    println!(
                        "{}",
                        serde_json::to_string(&stats).expect("stats serialise")
                    );
                    ExitCode::SUCCESS
                }
                Err(err) => {
                    eprintln!("semantic_perf_probe: {err}");
                    ExitCode::from(1)
                }
            }
        }
        Some("identity") => {
            let identity = Identity {
                tool: "verter",
                crate_version: env!("CARGO_PKG_VERSION"),
                instrumented: alloc.is_some(),
                debug_assertions: cfg!(debug_assertions),
                target_os: std::env::consts::OS,
                target_arch: std::env::consts::ARCH,
                native_arch: process_stats::native_arch(),
                job_schema: super::JOB_SCHEMA,
                result_schema: super::RESULT_SCHEMA,
            };
            println!(
                "{}",
                serde_json::to_string(&identity).expect("identity serialises")
            );
            ExitCode::SUCCESS
        }
        _ => {
            eprintln!("{USAGE}");
            ExitCode::from(2)
        }
    }
}
