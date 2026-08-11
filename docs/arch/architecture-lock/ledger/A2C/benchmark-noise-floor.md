# A2C frozen measured noise floor

This value was computed and frozen before candidate shape sampling.

- control samples: 40 measured after 5 warmups
- repetitions per sample: 50,000 of the identical 256-step `stable_control`
- median: 231.202 ns
- bootstrap 95% CI: [230.590 ns, 231.969 ns]
- measured noise floor: 0.331745%
- twice noise floor: 0.663489%
- frozen latency gate: `max(3%, 0.663489%) = 3.000000%`
- raw calibration: `command-proofs/latency/00-control-calibration.txt`
- raw analysis: `command-proofs/latency/03-control-analysis-final.txt`

