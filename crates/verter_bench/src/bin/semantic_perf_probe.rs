//! The Verter arm of `scripts/benchmark/semantic-perf.mjs`: a release
//! executable linking the production `verter_session` library (never a test
//! build) that answers one benchmark job. See
//! [`verter_bench::semantic_perf`] for the job and the record. It installs the
//! allocator the shipped binaries install (`verter_alloc`), so its timings
//! measure the heap users run.

use std::process::ExitCode;

#[global_allocator]
static GLOBAL: verter_alloc::Allocator = verter_alloc::ALLOCATOR;

fn main() -> ExitCode {
    verter_alloc::configure();
    verter_bench::semantic_perf::cli::main(None)
}

#[cfg(test)]
mod tests {
    /// The probe measures the heap the shipped binaries run on.
    #[test]
    fn the_shipped_allocator_serves_this_probe() {
        assert_eq!(
            verter_alloc::mimalloc_serves_global_allocations(),
            verter_alloc::SELECTED == verter_alloc::AllocatorKind::Mimalloc
        );
    }
}
