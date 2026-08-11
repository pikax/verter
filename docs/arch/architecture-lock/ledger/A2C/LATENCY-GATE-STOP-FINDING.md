# A2C latency-gate stop finding

STATE: BLOCKED

Candidate `04048a9471f1c13e81cda075fa27a6c35b59a842`, tree `5ca44c8e58d79cf040bbd066a25c44323cf0e10c`, fails the mandatory construction-latency gate against baseline `70ea4c01bea870e9684a66f229230808aeb64235`.

The frozen noise floor is 0.331745% and the frozen gate is 3.000000%. Forty interleaved pairs show gate failures for nested 256, switch 256, 64 live targets, and 65 live targets. The target-boundary cells are decisive: median slowdowns are 72.024907% and 78.337124%, with lower 95% confidence bounds of 70.712500% and 77.120663%.

No threshold recalibration, retained-layout change, lazy construction, source edit, mutation campaign, zero-cost measurement campaign, or full gate run is authorized by this result. The candidate worktree remains byte-unchanged and clean.

See `latency-benchmark-record.md` and `command-proofs/latency/`.

