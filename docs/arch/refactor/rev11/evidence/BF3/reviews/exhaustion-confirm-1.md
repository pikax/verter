VERDICT: BLOCKING
tokens used
194 381
## Fixes

- **X1 — BLOCKING.** Private fixtures work: [probe-bundler-route.mjs:536](packages/unplugin/scripts/probe-bundler-route.mjs:536) uses `mkdtemp`, and [line 581](packages/unplugin/scripts/probe-bundler-route.mjs:581) removes only that leaf. Eight concurrent probes each printed `exit=0 ... erroredCases=[] recompile=buildStarted`; `remaining_fixture_children=0`. The non-serialized transport suite printed `running 21 tests` and `20 passed; 0 failed; 1 ignored`.

  The required-lane RED plant printed `erroredCases=["REVIEW_X1_ERRORED_LANE"]`, `exit=1`; the Rust consumer executed `running 1 test` and failed with `the probe drove 1 required lane(s) into an error`.

  Blocking defect: the new exit contract says every driven lane is covered ([lines 11–13](packages/unplugin/scripts/probe-bundler-route.mjs:11), [737–750](packages/unplugin/scripts/probe-bundler-route.mjs:737)), but `erroredCases` scans only `results.cases`, excluding `results.exportCases`, whose driver can return `outcome:"error"` at [lines 719–725](packages/unplugin/scripts/probe-bundler-route.mjs:719). Proven plant:

  `loaded=true fresh=true erroredCases=[] exportCase.VerterVue.outcome=error message=REVIEW_EXPORT_CASE_ERROR`  
  `exit=0`

  The Rust export test independently caught that mutation, but the standalone process contract remains false.

- **X2 — PASS.** The invalid cross-process inference is gone; the test explicitly disclaims it at [transport_route_equivalence_tests.rs:3278](crates/verter_session/src/framework/transport_route_equivalence_tests.rs:3278) and compares both products at [line 3400](crates/verter_session/src/framework/transport_route_equivalence_tests.rs:3400). Correct wording and closure condition appear in [inventory:8](crates/verter_session/src/framework/framework_product_surface_inventory.json:8), [test-invocations.md:484](docs/arch/refactor/rev11/evidence/BF3/test-invocations.md:484), and [landing-record.md:620](docs/arch/refactor/rev11/evidence/BF3/landing-record.md:620).

  Non-vacuity plant appended one marker to the observed child module. Command:

  `CARGO_BUILD_JOBS=4 cargo test -p verter_session --lib --features transport-authoritative the_bundler_pre_compile_lane_publishes_the_hosts_products_for_a_real_project -- --test-threads=1 --nocapture`

  Exact RED: `running 1 test`; `the bundler route's published bytes differ from the host route's`. After restoration: `1 passed; 0 failed`.

- **X3 — PASS.** Both files have zero machine-root hits; normalization is disclosed at [at2-disposition-ruling.md:7](docs/arch/refactor/rev11/evidence/BF3/at2-disposition-ruling.md:7). Guard command:

  `CARGO_BUILD_JOBS=4 cargo test -p verter_session --test main tracked_paths_no_machine_roots -- --test-threads=1`

  Exact result: `running 5 tests`; `5 passed; 0 failed`.

- **X4 — PASS.** The seven-versus-one diagnostics are correctly separated at [landing-record.md:697](docs/arch/refactor/rev11/evidence/BF3/landing-record.md:697); current source contains exactly the cited seven consumers. Status now says two criteria remain at [line 750](docs/arch/refactor/rev11/evidence/BF3/landing-record.md:750). Deleted-constant comments are corrected at [suite_census.rs:133](crates/verter_session/src/framework/suite_census.rs:133), and the three unreachable facts agree at [svelte_batch_route_tests.rs:708](crates/verter_session/src/framework/svelte_batch_route_tests.rs:708) and [dispositions.md:125](docs/arch/refactor/rev11/evidence/BF3/dispositions.md:125).

  Batch command printed `running 11 tests`; `10 passed; 0 failed; 1 ignored`.

- **X5 — PASS.** The ignore reason now states the invariant at [transport_route_equivalence_tests.rs:2585](crates/verter_session/src/framework/transport_route_equivalence_tests.rs:2585); the old identifier is absent from crate source. `framework_known_bug_ledger_bijection` printed `running 2 tests`; `2 passed; 0 failed`.

- **X6 — PASS.** All three reports are committed, and [reviews/README.md:10](docs/arch/refactor/rev11/evidence/BF3/reviews/README.md:10) records seat/mandate/verdict while [lines 26–57](docs/arch/refactor/rev11/evidence/BF3/reviews/README.md:26) records dispositions. `git show --stat e21ea5eca` reported `4 files changed, 267 insertions(+)`.

## Specific answers

- **Is X2 still non-vacuous?** Yes. The byte-divergence mutation made the exact named test RED for a genuine product mismatch.
- **Were assertions weakened?** Yes, intentionally:
  - The invalid cross-file `changed_files` assertion was removed.
  - Exact equality to the old fixed fixture root was broadened to stable parent + `recompile-` prefix + one-level leaf at [lines 3320–3337](crates/verter_session/src/framework/transport_route_equivalence_tests.rs:3320).
  - Product byte/map parity was not weakened.
- **Any new overstatement?** Yes: X1’s “every lane” exit-status claim excludes errored export cases, as the exit-0 plant proves.

## OUT-OF-DELTA

`no_phase_archaeology_in_production_code` currently fails on [suite_census.rs:40](crates/verter_session/src/framework/suite_census.rs:40): `residual is carried as a debt row`. `git blame` attributes it to `f716f30340`, an ancestor of `6bb18e771`; not actioned.

All plants were restored: script worktree/HEAD blob both `181bea645bc418704a0fdb5aeb4a884f9415745a`, all markers zero, fixture children zero, worktree clean.

VERDICT: BLOCKING
