//! Per-request resolver-context port call counts on the semantic hot lanes.
//! See [`verter_bench::semantic_perf::dispatch_profile`].

fn main() -> std::process::ExitCode {
    verter_bench::semantic_perf::dispatch_profile::main()
}
