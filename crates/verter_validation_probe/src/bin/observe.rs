//! Capture the non-gating observation artifact for one lane.
//!
//! The capture drives every selected case cold in a fresh driver and warm
//! through a warmup plus the warm samples, so its duration scales with the
//! corpus it is pointed at. It is a lane driver bounded by its job's timeout,
//! not a test held to a per-test deadline.
//!
//! The artifact is validated against the manifests it was captured from
//! ([`ObservationArtifact::validate`], inside the capture), accepted as a
//! live capture ([`ObservationArtifact::check_live_capture`]), and written
//! to `target/validation-probe/observations.json`. Any refusal exits
//! non-zero; no recorded number ever decides the exit.
//!
//! [`ObservationArtifact::validate`]: verter_validation_probe::observe::ObservationArtifact::validate
//! [`ObservationArtifact::check_live_capture`]: verter_validation_probe::observe::ObservationArtifact::check_live_capture

use std::process::ExitCode;

use verter_validation_probe::observe;
use verter_validation_probe::runner::PhaseDeadlines;
use verter_validation_probe::summary::Lane;

const USAGE: &str = "usage: validation-probe-observe --lane <smoke|main>";

fn main() -> ExitCode {
    let mut lane = None;
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--lane" => match args.next().as_deref() {
                Some("smoke") => lane = Some(Lane::Smoke),
                Some("main") => lane = Some(Lane::Main),
                _ => return fail(&format!("--lane needs `smoke` or `main`\n{USAGE}")),
            },
            "--help" | "-h" => {
                println!("{USAGE}");
                return ExitCode::SUCCESS;
            }
            other => return fail(&format!("unknown argument `{other}`\n{USAGE}")),
        }
    }
    let Some(lane) = lane else {
        return fail(USAGE);
    };

    let artifact = match observe::capture(lane, PhaseDeadlines::default()) {
        Ok(artifact) => artifact,
        Err(error) => return fail(&format!("observation capture failed: {error}")),
    };
    if let Err(error) = artifact.check_live_capture() {
        return fail(&error.to_string());
    }
    match observe::write_observations(&artifact) {
        Ok(path) => {
            println!(
                "{} rows of {} written to {}",
                artifact.rows.len(),
                artifact.artifact_id,
                path.display()
            );
            ExitCode::SUCCESS
        }
        Err(error) => fail(&format!("writing observations: {error}")),
    }
}

fn fail(message: &str) -> ExitCode {
    eprintln!("validation-probe-observe: {message}");
    ExitCode::FAILURE
}
