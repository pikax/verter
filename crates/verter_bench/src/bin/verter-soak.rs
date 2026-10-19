//! Run and verify one mode of the long-churn soak workload.
//!
//! ```text
//! verter-soak --mode normal|pressure [--tranches N] --out <dir>
//! verter-soak --mode normal|pressure [--tranches N] --out <dir> --verify-only
//! verter-soak --mode normal|pressure [--tranches N] --digest
//! verter-soak --controls --out <dir>
//! ```
//!
//! A run writes `<dir>/soak-summary.json`, then reads it back and judges it;
//! the verdict (and every failure) is written to `<dir>/soak-verdict.json`.
//! `--tranches` defaults to the full manifest's 100. `--digest` prints the
//! manifest's digest and distinct query count and runs nothing. `--controls`
//! runs every negative control (a planted defect in a one-tranche smoke run)
//! under `<dir>/<control>`, writes `<dir>/soak-controls.json`, and passes
//! only when each control was applied and failed the run for its own reason.
//! Exit status: 0 passed, 1 failed, 2 usage or run error.

use std::path::PathBuf;
use std::process::ExitCode;

use verter_bench::soak::controls;
use verter_bench::soak::manifest::{Manifest, Mode, FULL_TRANCHES};
use verter_bench::soak::run::{run, RunOptions};
use verter_bench::soak::verify::verify;

const USAGE: &str =
    "usage: verter-soak --mode normal|pressure [--tranches N] (--out <dir> [--verify-only] | --digest)\n       verter-soak --controls --out <dir>";

struct Args {
    controls: bool,
    mode: Mode,
    tranches: u32,
    out: Option<PathBuf>,
    verify_only: bool,
    digest: bool,
}

fn parse() -> Result<Args, String> {
    let mut mode = None;
    let mut controls = false;
    let mut tranches = FULL_TRANCHES;
    let mut out = None;
    let mut verify_only = false;
    let mut digest = false;
    let mut argv = std::env::args().skip(1);
    while let Some(arg) = argv.next() {
        let mut value = || argv.next().ok_or_else(|| format!("{arg} needs a value"));
        match arg.as_str() {
            "--mode" => mode = Some(value()?.parse::<Mode>()?),
            "--tranches" => {
                tranches = value()?
                    .parse()
                    .ok()
                    .filter(|n| *n > 0)
                    .ok_or("--tranches takes a positive count")?
            }
            "--out" => out = Some(PathBuf::from(value()?)),
            "--verify-only" => verify_only = true,
            "--digest" => digest = true,
            "--controls" => controls = true,
            other => return Err(format!("unknown argument {other}")),
        }
    }
    if controls {
        if out.is_none() {
            return Err("--controls needs --out".into());
        }
    } else if mode.is_none() {
        return Err("--mode is required".into());
    }
    if !digest && out.is_none() {
        return Err("--out is required unless --digest".into());
    }
    Ok(Args {
        controls,
        mode: mode.unwrap_or(Mode::Normal),
        tranches,
        out,
        verify_only,
        digest,
    })
}

fn run_controls(out: &std::path::Path) -> ExitCode {
    let mut reports = Vec::new();
    for control in controls::all() {
        match controls::execute(&control, &out.join(control.name)) {
            Ok(report) => reports.push(report),
            Err(err) => {
                eprintln!("verter-soak: control {}: {err}", control.name);
                return ExitCode::from(2);
            }
        }
    }
    let passed = reports.iter().all(|r| r.bit);
    let verdict = serde_json::json!({
        "verdict": if passed { "passed" } else { "failed" },
        "controls": reports,
    });
    let text = serde_json::to_string_pretty(&verdict).expect("a verdict serialises");
    if let Err(err) =
        verter_bench::semantic_perf::disk::write(out.join("soak-controls.json"), &text)
    {
        eprintln!("verter-soak: write the controls verdict: {err}");
        return ExitCode::from(2);
    }
    println!("{text}");
    if passed {
        ExitCode::SUCCESS
    } else {
        ExitCode::FAILURE
    }
}

/// Stack of the thread the run executes on: deep editor fixtures nest
/// resolution past what a default main-thread stack holds in a debug build.
const RUN_STACK_BYTES: usize = 64 * 1024 * 1024;

fn main() -> ExitCode {
    std::thread::Builder::new()
        .name("verter-soak".into())
        .stack_size(RUN_STACK_BYTES)
        .spawn(soak_main)
        .expect("spawn the soak thread")
        .join()
        .unwrap_or_else(|_| ExitCode::from(2))
}

fn soak_main() -> ExitCode {
    let args = match parse() {
        Ok(args) => args,
        Err(err) => {
            eprintln!("verter-soak: {err}\n{USAGE}");
            return ExitCode::from(2);
        }
    };
    if args.controls {
        return run_controls(&args.out.expect("checked by parse"));
    }
    if args.digest {
        let manifest = Manifest::generate(args.mode, args.tranches);
        println!(
            "{{\"mode\":\"{}\",\"tranches\":{},\"actions\":{},\"distinctQueries\":{},\"digest\":\"{}\"}}",
            args.mode.as_str(),
            args.tranches,
            manifest.actions.len(),
            manifest.distinct_queries(),
            manifest.digest()
        );
        return ExitCode::SUCCESS;
    }
    let out = args.out.expect("checked by parse");
    if !args.verify_only {
        let options = RunOptions {
            mode: args.mode,
            tranches: args.tranches,
            out_dir: out.clone(),
            mutation: None,
        };
        if let Err(err) = run(&options) {
            eprintln!("verter-soak: {err}");
            return ExitCode::from(2);
        }
    }
    let failures = verify(args.mode, args.tranches, &out);
    let verdict = serde_json::json!({
        "mode": args.mode.as_str(),
        "tranches": args.tranches,
        "verdict": if failures.is_empty() { "passed" } else { "failed" },
        "failures": failures,
    });
    let text = serde_json::to_string_pretty(&verdict).expect("a verdict serialises");
    if let Err(err) = verter_bench::semantic_perf::disk::write(out.join("soak-verdict.json"), &text)
    {
        eprintln!("verter-soak: write the verdict: {err}");
        return ExitCode::from(2);
    }
    println!("{text}");
    if failures.is_empty() {
        ExitCode::SUCCESS
    } else {
        ExitCode::FAILURE
    }
}
