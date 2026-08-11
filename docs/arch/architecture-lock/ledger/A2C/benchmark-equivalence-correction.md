# A2C benchmark-equivalence correction

The retained baseline executable and the committed candidate executable do not implement the same sampling method: the former reports per-operation averages with fractional nanoseconds, while the latter times one operation and reports the Windows timer's integer result. Their raw run is retained at `command-proofs/latency/06-interleaved-40-pairs.txt` but is not gate evidence.

Before collecting valid shape samples, both exact SHAs are therefore rebuilt from read-only Git archives with one byte-identical benchmark source. The source preserves the repository fixtures, stable control, build mode, and measured boundary (`build_function_body_skeleton` over an already-parsed body), and repeats each construction enough times to report per-operation averages above timer granularity. Both lanes use identical repetition counts and warmup. The baseline and candidate are both rerun; no result from the mismatched run is reused.

The predeclared statistic, 40-pair alternating order, no-outlier policy, bootstrap rule, measured 0.331745% noise floor, and frozen 3.000000% gate are unchanged.

