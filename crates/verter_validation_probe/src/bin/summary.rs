//! Read the lane's summary artifact and publish or dispose of it.
//!
//! Two modes, deliberately separate:
//!
//! * `--markdown` renders the compact human summary for the job page. It is a
//!   publication step and never decides the job.
//! * `--dispose` is the lane's DISPOSITION: it exits non-zero when any gate
//!   cell evaluated to anything but a gate pass. It runs AFTER publication, so
//!   the summary and its artifact exist for the run that turned red.
//!
//! Both modes validate the document first, and validation checks the presence
//! of every required counter against the raw JSON. A summary missing a counter
//! is refused rather than read as a zero — which is what keeps "the lane
//! reported no regressions" from meaning "the lane forgot to count them".

use std::process::ExitCode;

use verter_validation_probe::summary::{Disposition, Summary};

const USAGE: &str = "usage: validation-probe-summary [--markdown|--dispose] [--summary <path>]";

fn main() -> ExitCode {
    let mut markdown = false;
    let mut dispose = false;
    let mut path = default_summary_path();
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--markdown" => markdown = true,
            "--dispose" => dispose = true,
            "--summary" => match args.next() {
                Some(value) => path = value.into(),
                None => return fail("--summary needs a path"),
            },
            "--help" | "-h" => {
                println!("{USAGE}");
                return ExitCode::SUCCESS;
            }
            other => return fail(&format!("unknown argument `{other}`\n{USAGE}")),
        }
    }
    if markdown == dispose {
        return fail(&format!("exactly one of --markdown or --dispose\n{USAGE}"));
    }

    let text = match verter_validation_probe::disk::read_text(&path) {
        Ok(text) => text,
        Err(error) => return fail(&format!("the summary artifact could not be read: {error}")),
    };
    let summary = match Summary::from_json_str(&text) {
        Ok(summary) => summary,
        Err(error) => return fail(&format!("{}: {error}", path.display())),
    };

    if markdown {
        print!("{}", summary.to_markdown());
        return ExitCode::SUCCESS;
    }

    match summary.disposition() {
        Disposition::Clean => {
            println!(
                "validation probe: {} attempted, {} passed, no gate regression",
                summary.totals.attempted, summary.totals.passed
            );
            ExitCode::SUCCESS
        }
        Disposition::GateRegressed { count } => {
            eprintln!(
                "validation probe: {count} gate cell(s) did not evaluate to a gate pass; \
                 the lane fails."
            );
            for framework in &summary.frameworks {
                for case in &framework.cases {
                    for cell in &case.cells {
                        if cell.evaluation.blocks() {
                            eprintln!(
                                "  {} [{}]: expected {:?}, observed {:?}",
                                cell.probe_id,
                                cell.dimension,
                                cell.expected_class,
                                cell.observed_class
                            );
                        }
                    }
                }
            }
            ExitCode::from(1)
        }
    }
}

fn fail(message: &str) -> ExitCode {
    eprintln!("{message}");
    ExitCode::from(2)
}

fn default_summary_path() -> std::path::PathBuf {
    verter_validation_probe::corpus::workspace_root()
        .join("target")
        .join("validation-probe")
        .join("summary.json")
}
