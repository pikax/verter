//! The Verter arm of `scripts/benchmark/semantic-perf.mjs`: a release
//! executable linking the production `verter_session` library (never a test
//! build) that answers one benchmark job. See
//! [`verter_bench::semantic_perf`] for the job and the record.

use std::process::ExitCode;

fn main() -> ExitCode {
    verter_bench::semantic_perf::cli::main(None)
}
