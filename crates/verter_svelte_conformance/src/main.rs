//! `verter_svelte_conformance` CLI: the canonical Svelte CSS-scoping
//! conformance plan and the committed fixture corpus built from it.
//!
//! - `emit-plan [--format json]` — print the machine-readable plan JSON on
//!   stdout (the source of truth the Node golden façade consumes).
//! - `write` — regenerate `corpus/fixtures/` plus the two review artifacts
//!   at the crate-owned corpus root. Idempotent.
//! - `check` — reconcile the committed conformance corpus against a freshly
//!   built plan; non-zero exit plus a drift report on any mismatch.
//!
//! Output is deterministic: no timestamps, no environment reads beyond the
//! argument list.

use std::process::ExitCode;

use verter_svelte_conformance::generate::{
    build_plan, check_corpus, default_corpus_root, plan_json, write_corpus,
};

const USAGE: &str = "usage: verter_svelte_conformance <emit-plan [--format json] | write | check>";

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Command {
    EmitPlan,
    Write,
    Check,
}

fn parse_command(args: &[String]) -> Result<Command, String> {
    let args: Vec<&str> = args.iter().map(String::as_str).collect();
    match args.as_slice() {
        ["emit-plan"] | ["emit-plan", "--format", "json"] => Ok(Command::EmitPlan),
        ["emit-plan", "--format", other] => Err(format!(
            "unsupported emit-plan format {other:?} (only \"json\")\n{USAGE}"
        )),
        ["write"] => Ok(Command::Write),
        ["check"] => Ok(Command::Check),
        [] => Err(format!("missing subcommand\n{USAGE}")),
        other => Err(format!("unrecognized arguments {other:?}\n{USAGE}")),
    }
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let command = match parse_command(&args) {
        Ok(command) => command,
        Err(message) => {
            eprintln!("{message}");
            return ExitCode::from(2);
        }
    };
    match command {
        Command::EmitPlan => {
            print!("{}", plan_json(&build_plan()));
            ExitCode::SUCCESS
        }
        Command::Write => match write_corpus(&default_corpus_root()) {
            Ok(report) => {
                println!(
                    "wrote {} fixtures under corpus/fixtures/ plus \
                     coverage-index.json and coverage-summary.md",
                    report.fixtures_written
                );
                ExitCode::SUCCESS
            }
            Err(error) => {
                eprintln!("write failed: {error}");
                ExitCode::FAILURE
            }
        },
        Command::Check => match check_corpus(&default_corpus_root()) {
            Ok(()) => {
                println!("the conformance corpus matches the manifest");
                ExitCode::SUCCESS
            }
            Err(drifts) => {
                for drift in &drifts {
                    eprintln!("{}", drift.render());
                }
                eprintln!(
                    "{} drift finding(s); regenerate with \
                     `cargo run -p verter_svelte_conformance -- write`",
                    drifts.len()
                );
                ExitCode::FAILURE
            }
        },
    }
}

#[cfg(test)]
mod tests {
    use super::{parse_command, Command};

    fn args(list: &[&str]) -> Vec<String> {
        list.iter().map(|s| (*s).to_string()).collect()
    }

    #[test]
    fn parses_the_closed_command_vocabulary() {
        assert_eq!(parse_command(&args(&["emit-plan"])), Ok(Command::EmitPlan));
        assert_eq!(
            parse_command(&args(&["emit-plan", "--format", "json"])),
            Ok(Command::EmitPlan)
        );
        assert_eq!(parse_command(&args(&["write"])), Ok(Command::Write));
        assert_eq!(parse_command(&args(&["check"])), Ok(Command::Check));
    }

    #[test]
    fn rejects_malformed_invocations() {
        assert!(parse_command(&args(&[])).is_err());
        assert!(parse_command(&args(&["emit-plan", "--format", "yaml"])).is_err());
        assert!(parse_command(&args(&["emit-plan", "--format"])).is_err());
        assert!(parse_command(&args(&["frobnicate"])).is_err());
        assert!(parse_command(&args(&["write", "--force"])).is_err());
        assert!(parse_command(&args(&["check", "extra"])).is_err());
    }
}
