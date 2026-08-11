# A2C implementation stop finding

STATE: BLOCKED

The binding Revision 11 no-regression construction gate cannot be satisfied
simultaneously with the required retained A2C payload on the locked base
`70ea4c01bea870e9684a66f229230808aeb64235`.

The specification requires the pre-change construction baseline, an upper
slowdown bound of `max(3%, 2 × measured noise floor)`, and these retained
facts for every demanded skeleton. Every measured baseline noise floor yields
an allowed bound of 3%.

| Shape | Baseline median | Implemented median | Slowdown |
|---|---:|---:|---:|
| flat 1,024 | 353.41 µs | 398.18 µs | 12.67% |
| nested 256 | 178.26 µs | 345.75 µs | 93.95% |
| switch 256 | 146.46 µs | 173.42 µs | 18.41% |
| 64 targets | 2.2711 µs | 19.213 µs | 746.0% |
| 65 targets | 2.2849 µs | 19.735 µs | 763.7% |

The 64-target implemented shape retains 10,616 fact bytes: the mandated
72-byte inline `FunctionCompletionFacts`, 193 × 48-byte statement facts,
64 × 20-byte targets, plus the two Arc slice allocations. Query access remains
one field read with zero AST walks, zero allocations, and zero requested bytes.
The 65-target shape correctly publishes
`Unknown(TargetCapacityExceeded)` and retains 10,760 fact bytes.

Raw measurements:

- `command-proofs/baseline-completion-facts.txt`
- `command-proofs/performance-stop-final.txt`

The implementation reached all 24 semantic fact tests and all four public
cold/warm non-interference tests green before this mandatory performance stop.
No commit, gate run, mutation claim, or acceptance recommendation is made.
The implementation worktree is restored to the exact clean base.
