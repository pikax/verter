# A2C construction-latency gate record

## Bound identities and method

- baseline: `70ea4c01bea870e9684a66f229230808aeb64235`
- candidate: `04048a9471f1c13e81cda075fa27a6c35b59a842`
- candidate tree: `5ca44c8e58d79cf040bbd066a25c44323cf0e10c`
- harness SHA-256 in both lanes: `f0a6e8dcdc2374fdcbc82598b1dbe79827a6d54ad356a69a6b793f5570e23acd`
- samples: 40 measured baseline/candidate pairs after 5 warmup pairs
- order: alternating baseline/candidate then candidate/baseline
- statistic: median paired slowdown; percentile-bootstrap 95% CI of the median; 100,000 deterministic resamples
- outliers: none removed
- stable-control median slowdown: 0.028084%; bootstrap 95% CI `[-0.092412%, 0.291592%]`

The measured control noise floor was frozen before shape sampling at 0.331745%. Twice that value is 0.663489%, so the gate is `max(3%, 0.663489%) = 3.000000%`.

## Results

| Shape | Median slowdown | Bootstrap 95% CI | Gate | Verdict |
|---|---:|---:|---:|---|
| flat 1,024 | -1.413832% | [-4.610143%, 0.136585%] | 3.000000% | PASS |
| nested 256 | 2.480523% | [-1.168683%, 5.711110%] | 3.000000% | FAIL |
| switch 256 | 3.363167% | [-1.225021%, 5.828231%] | 3.000000% | FAIL |
| 64 live targets | 72.024907% | [70.712500%, 74.081850%] | 3.000000% | FAIL |
| 65 live targets | 78.337124% | [77.120663%, 80.734475%] | 3.000000% | FAIL |

Overall verdict: **FAIL**. The candidate exceeds the frozen gate.

Raw evidence:

- `benchmark-predeclaration.md`
- `benchmark-noise-floor.md`
- `benchmark-equivalence-correction.md`
- `command-proofs/latency/00-control-calibration.txt`
- `command-proofs/latency/03-control-analysis-final.txt`
- `command-proofs/latency/10-equivalent-harness-install-final.txt`
- `command-proofs/latency/11-equivalent-baseline-build.txt`
- `command-proofs/latency/12-equivalent-candidate-build.txt`
- `command-proofs/latency/13-valid-interleaved-40-pairs.txt`
- `command-proofs/latency/14-valid-bootstrap-analysis.txt`
- `command-proofs/latency/15-final-candidate-identity-clean.txt`

