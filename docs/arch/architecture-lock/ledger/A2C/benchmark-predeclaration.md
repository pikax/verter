# A2C construction-latency gate predeclaration

Recorded before candidate benchmark compilation or timing collection.

- baseline: `70ea4c01bea870e9684a66f229230808aeb64235`
- candidate: `04048a9471f1c13e81cda075fa27a6c35b59a842`
- candidate tree: `5ca44c8e58d79cf040bbd066a25c44323cf0e10c`
- build: Cargo `bench` profile, the repository benchmark `completion_facts`
- shapes: flat 1,024; nested 256; switch 256; 64 simultaneously live targets; 65 simultaneously live targets
- measured samples: 40 baseline/candidate pairs after 5 discarded warmup pairs
- interleaving: pair 1 baseline then candidate, pair 2 candidate then baseline, alternating thereafter
- stable control: the benchmark executable's `stable_control()` result is retained for every invocation; a separate optimized calibration executes the identical 256-step control 50,000 times per sample
- noise calibration: 40 samples after 5 warmups; no outlier deletion; noise floor is the bootstrap 95% relative half-width of the calibration median, using 100,000 resamples and deterministic seed `0xA2C20260811`
- gate: `max(3%, 2 × measured noise floor)`; the measured noise value and resulting gate are frozen before shape sampling
- shape statistic: median of the 40 paired slowdowns, where paired slowdown is `(candidate_ns / baseline_ns - 1) × 100`
- uncertainty: percentile bootstrap 95% CI of the paired-slowdown median, 100,000 resamples, deterministic seed `0xA2C04048`
- verdict: a shape passes only when the upper endpoint of that CI is at or below the frozen gate; every shape must pass
- outliers: retained; no discretionary deletion
- stop rule: any shape above the gate is a finding and stops remaining evidence work without source edits or gate recalibration

