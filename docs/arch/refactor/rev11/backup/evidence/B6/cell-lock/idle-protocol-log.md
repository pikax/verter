# Idle-protocol log — B6_COMPILER_ROUTE_OVERHEAD calibration not started

The pre-measure registration refuses to run unless 1-minute load `< 2.00`
and no foreign `cargo` / `cargo-nextest` / `rustc` / `gate.mjs` exists.
This file is the evidence that the protocol was checked, not skipped.

| local time | loadavg 1/5/15 | cargo/rustc/nextest count | notes |
|---|---|---:|---|
| 12:19 | 13.77 / 12.64 / 20.88 | n/a | first check, before harness |
| 12:35 | 9.61 / 15.44 / 18.11 | cargo present | after first release build |
| 12:39 | 8.10 / 12.03 / 15.94 | rustc 207% | start of wait loop |
| 12:48 | 8.71 / 11.18 / 13.75 | rustc 47–283% | |
| 12:57 | 32.08 / 22.77 / 17.52 | 5 rustc | |
| 13:06 | 24.05 / 29.68 / 23.43 | rustc 95% | |
| 13:14 | 23.99 / 17.43 / 18.68 | 5 rustc 50–225% | |
| 13:23 | 24.70 / 21.12 / 19.81 | 4 rustc + nextest | |
| 13:32 | 40.63 / 29.27 / 24.74 | 4 rustc + tsserver + node | peak |
| 13:41 | 8.55 / 18.19 / 22.54 | **0** | first cargo-zero; load still 8.55 |
| 13:49 | 6.68 / 9.87 / 16.12 | rustc 97% | burst resumed |
| 13:52 | 2.94 / 7.34 / 14.21 | **0** | closest to protocol; tens of seconds |
| 13:53 | 5.09 / 6.96 / 13.24 | cargo=1 | window gone |
| 13:55 | 4.32 / 5.96 / 12.17 | 2 rustc 99% | |
| 14:04 | 19.93 / 10.51 / 10.95 | 11 cargo/rustc | |
| 14:12 | 10.30 / 13.07 / 12.21 | 1 nextest | |
| 14:21 | 7.19 / 11.23 / 12.00 | 1 nextest | |
| 14:30 | 9.50 / 10.53 / 11.22 | **0** | cargo-zero; load 9.50 |
| 14:31 | 10.52 / 10.86 / 11.27 | rustc 247% | window gone |
| 14:35 | 18.42 / 13.96 / 12.45 | 16 cargo/rustc | |
| 14:44 | 16.11 / 15.47 / 13.82 | 11 cargo/rustc | stop waiting; do not fabricate |

`lowpowermode` was `0` whenever checked. AC power assumed (A6 runner class).

No calibration TSV, no holdout TSV, no derived relative bound.

## Attempt 2 — 2026-08-23 17:07–18:03 local

Second attempt, after the harness, the control benchmark and the cell renderer
were all built and verified. Waiting was automated rather than eyeballed: a
driver polled every 5 s and would have started the calibration within five
seconds of the protocol holding. The whole two-session protocol takes about
15 s of machine time (control 3.3 s + 30 cold invocations ≈ 0.4 s + control
3.3 s, twice), so the window needed was very short. It never appeared.

| | value |
|---|---:|
| samples (5 s apart) | 641 |
| span | 17:07:18 → 18:02:17 |
| minimum 1-minute load observed | 3.84 |
| maximum 1-minute load observed | 74.34 |
| maximum concurrent foreign `cargo`/`rustc`/`cargo-nextest` | 22 |
| samples with **zero** foreign build processes | 4 |
| sessions started | 0 |

Load *rose* across the wait rather than falling — about 5 at 17:05, about 36
by 18:02 — because several other agents entered heavy Rust build phases on this
shared host. `pmset` reported `lowpowermode 0` on AC power throughout, so power
policy was never the blocker; contention was. No orphaned build processes were
found (`ppid == 1` check), so nothing could be legitimately reclaimed: every
`cargo`/`rustc` process belonged to a live agent doing real work.

The protocol was not relaxed to fit the machine. Lowering the load ceiling, or
measuring anyway and noting the load, would have produced a coefficient of
variation inflated by other agents' compilers, and the relative bound derived
from it — `max(3.0000, 2 × CV)` — would have been *looser* than the truth. A
gate that every future block is judged against must not be widened by whoever
happened to be compiling at the time.

## Attempt 3 — 2026-08-23 18:28–18:53 local — SUCCEEDED

The maintainer authorised draining the host and the coordinator stopped the
concurrent agents. The first drain still left the floor at 1-minute load 2.39:
an adversarial review leg (`adversarial-review.sh b6-r7`, supervisor PID 56821,
`grok` child PID 56967) was bursting `cargo-nextest` -> `cargo` -> `rustc` every
30-40 s from worktree `verter-adv-b6-r7`. Attribution
was not guessed: 67 of 67 compiler observations sampled over five minutes, across
16 distinct PIDs, resolved by `lsof` to that one worktree. The bursts re-inflated
the average before it could decay, so waiting could never have worked. The first
sweep missed it because it killed by resolved working directory, and only the
`grok` child carried the worktree cwd -- its supervisor's cwd was a plain shell
path, so the worker died and the thing that respawns it did not.

Once that chain was killed parent-first, load decayed monotonically with nothing
left to generate bursts, crossed 2.00 at 18:53:09, and the waiting driver fired
within five seconds. Both sessions completed back to back.

| | calibration | holdout |
|---|---:|---:|
| 1-minute load at session start | 1.98 | 1.83 |
| foreign compilers | 0 | 0 |
| `lowpowermode` | 0 | 0 |
| control median, start -> end | 81.35 -> 82.01 ms | 81.68 -> 82.41 ms |
| control drift (ceiling 3.0%) | 0.8113% | 0.8937% |

Before this window the per-invocation guard checked only the load average, so a
burst starting mid-session would not have voided it -- a live contamination path
given a 30-40 s burst cadence against a ~15 s session, not a theoretical one.
Every measured step now re-checks load **and** foreign compilers, and voids the
session on either. That is a tightening; it can only make a session harder to
pass.

## Outcome

`B6_COMPILER_ROUTE_OVERHEAD` is locked into repo-root `performance-gates.toml`,
which now carries 4 cells and 53 metrics. The Implementation Lock Record gained
its section 13 gate-file extension register, with this cell as row E-2 and BF2's
earlier unregistered extension disclosed as row E-1.

Both absolute budgets were registered before any of this ran and are unchanged by
it. The measured medians sit roughly 55x inside the wall budget, which is the
expected shape for a product budget rather than a fit.
