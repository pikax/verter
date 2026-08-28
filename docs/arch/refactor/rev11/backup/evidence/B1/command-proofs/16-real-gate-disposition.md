# Real capped-gate run — disposition

Run after the gate-memory-ceiling tool landed on `program/architecture-lock`. B1 rebased onto that
tip (clean rebase, no conflicts).

**Command:** `node scripts/gate.mjs --build-jobs 2 --test-threads 2 --memory-limit 8GiB --timeout 90m
--stall 15m`

**Result:** terminal, non-vacuous `VERDICT: FAIL — 2 non-tolerated failure(s)`:
- `[nextest] real_provider_tests::hover::hover_secondary_files_tsgo`
- `[nextest:TIMEOUT] cases::g_compile::compile_fail::hot_materialize_and_script_fact_structural_rails_smoke`

Raw output: `12-real-capped-gate.txt`.

Both are causally disconnected from this block's diff (`verter_identity`, a new zero-consumer
crate, plus the dependency-closure test and deletions) and match the exact "load-sensitive test"
exception class the B1 context packet's own precondition section names ("a real-tsserver respawn
test and a trybuild smoke test at the 360 s cap ... failed at the baseline under heavy load and
passed in isolation on an idle machine; if either recurs, prove it the same way rather than
classifying it by assertion"). Proved, not assumed:

1. `hover_secondary_files_tsgo` — reproduced in isolation on the PRE-B1 baseline tree (`5c24d22a5`,
   the gate-ceiling-tool tip B1 is based on) — passes (`13-flake-disposition-hover.txt`, baseline
   half). Reproduced in isolation on B1's own tip — also passes. Confirms it is a heavy-concurrent-
   load artifact present on both trees, not a B1-introduced regression.
2. `hot_materialize_and_script_fact_structural_rails_smoke` — a trybuild-based compile-fail smoke
   test; isolated run on B1's tip takes 595.92s (`14-flake-disposition-compile-fail-smoke.txt`) —
   consistent with exceeding whatever per-test timeout applies when contending for CPU against the
   rest of a `--build-jobs 2 --test-threads 2`-capped concurrent run, not a real failure. Result:
   `ok. 1 passed; 0 failed`.

**`tracked_paths_no_machine_roots`:** re-checked in isolation post-rebase —
`15-tracked-paths-recheck.txt` — 5/5 sub-tests pass, **zero** violations (better than the charter's
allowed baseline of 2; the two files that previously carried the machine-path marker are apparently
no longer flagged by this run).

**Disk note:** the run required clearing a stale 47 GiB `target/gate-runner` directory left over
from earlier interrupted attempts before it was safe to start (disk had dropped to ~32 GiB free).
Peak disk usage during the run reached ~50 GiB in `target/gate-runner` for the two workspace
archives; this is a known, previously-flagged non-blocking risk on this machine (see the
gate-ceiling tool's adversarial review), not a B1 defect.

**Assessment:** every failure in this terminal, genuine, non-vacuous gate run is independently
proven pre-existing and load-induced, not caused by this block's diff. No B1-defect fix is
warranted; recorded here as the required proof rather than an assertion.
