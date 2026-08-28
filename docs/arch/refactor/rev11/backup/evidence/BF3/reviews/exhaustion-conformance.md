VERDICT: BLOCKING

## A. Per-claim verdict

- C1 — PASS. The mutation test plants mounted-but-wrong markup and asserts mount success plus render divergence ([svelte_official_conformance_gate.rs:1274](crates/verter_session/src/compile/map_equality_tests/svelte_official_conformance_gate.rs:1274)); both it and the live gate call `compare_mounted_render` ([line 1714](crates/verter_session/src/compile/map_equality_tests/svelte_official_conformance_gate.rs:1714), [line 1805](crates/verter_session/src/compile/map_equality_tests/svelte_official_conformance_gate.rs:1805)). Scoped run: `running 20 tests`; `17 passed; 3 ignored`, including both runtime tests green.

- C2 — PASS. Rows derive identity from referenced function items via `type_name::<F>()` ([suite_census.rs:62](crates/verter_session/src/framework/suite_census.rs:62)); the unrelated anchor is executable ([script_facts_tests.rs:29](crates/verter_session/src/framework/script_facts_tests.rs:29)); GI-21 records the residual ([gate-integrity-ledger.md:43](docs/arch/gate-integrity-ledger.md:43)). Independent plant retargeting the batch row to the product witness: marker `0→1`, SHA `5ab7…→3601…`; `running 11 tests` went RED with two identity/registration failures. Restore returned SHA `5ab7…`; rerun: `10 passed; 1 ignored`.

- C3 — PASS. The test-owned script wraps the selected callable with an apply-counting `Proxy` ([transport_route_equivalence_tests.rs:1715](crates/verter_session/src/framework/transport_route_equivalence_tests.rs:1715)); executed exports require non-zero application, equal plugin keys, and both include decisions ([line 1924](crates/verter_session/src/framework/transport_route_equivalence_tests.rs:1924)). Independent observer-lie plant: marker `0→1`, SHA `02cd…→b522…`; `running 1 test` failed because the independently observed Svelte include decision disagreed. Restore returned SHA `02cd…`; rerun passed.

- C4 — BLOCKING. Serial functionality exists: the five tests are at [lines 2894–3275](crates/verter_session/src/framework/transport_route_equivalence_tests.rs:2894), and the transport suite reported `running 21 tests; 20 passed; 1 ignored`. Recompile is honestly marked partial ([inventory:8](crates/verter_session/src/framework/framework_product_surface_inventory.json:8)). However, its fixed shared fixture introduces the reproducible race in F1 below.

- C5 — PASS. The seven reachable class/lane rows are explicit ([svelte_batch_route_tests.rs:982](crates/verter_session/src/framework/svelte_batch_route_tests.rs:982)); class entry precedes product assertions ([line 857](crates/verter_session/src/framework/svelte_batch_route_tests.rs:857)); success/warning controls and search-only probe are at [1079](crates/verter_session/src/framework/svelte_batch_route_tests.rs:1079) and [1214](crates/verter_session/src/framework/svelte_batch_route_tests.rs:1214). Independent plant replaced the compile-failure input with a warning-only input: marker `0→1`, SHA `4e1c…→ab0d…`; `running 1 test` went RED with “reported no error at all, so this row is not measuring a failing entry.” Restore returned SHA `4e1c…`; rerun passed. Full suite: `running 11`; `10 passed; 1 ignored`.

- C6 — PASS. The memo explicitly leaves item 6 not evidenced and requests maintainer action ([at2-deviation-memo.md:68](docs/arch/refactor/rev11/evidence/BF3/at2-deviation-memo.md:68), [line 117](docs/arch/refactor/rev11/evidence/BF3/at2-deviation-memo.md:117)); the verbatim ruling is present ([at2-disposition-ruling.md:9](docs/arch/refactor/rev11/evidence/BF3/at2-disposition-ruling.md:9)). A direct extraction/diff of the ratified table at base versus HEAD printed `table_diff_exit=0`.

## B. Exit-criteria enumeration

### Owned scope and required procedure

1. Build/run Svelte counterpart — UNCHANGED-BY-DELTA. Current scoped run genuinely executed 20 tests and passed 17/ignored 3; the six-cell gate already existed ([gate:264](crates/verter_session/src/compile/map_equality_tests/svelte_official_conformance_gate.rs:264)).

2. Exact six client cells/all axes; record existing server refusal — UNCHANGED-BY-DELTA. Client and server counts are asserted as six ([gate:264](crates/verter_session/src/compile/map_equality_tests/svelte_official_conformance_gate.rs:264), [gate:516](crates/verter_session/src/compile/map_equality_tests/svelte_official_conformance_gate.rs:516)); scoped run passed them.

3. Independent plant and green control for every axis — SATISFIED-BY-DELTA. Runtime now has its own mounted-wrong-render plant and before/after controls ([gate:1284](crates/verter_session/src/compile/map_equality_tests/svelte_official_conformance_gate.rs:1284)); scoped run passed it and the existing five-axis mutation test.

4. Exhaust every retained product/public-default route — NOT-EVIDENCED. Recompile remains only “PARTIALLY DRIVEN”; its write is expressly unattributable ([inventory:8](crates/verter_session/src/framework/framework_product_surface_inventory.json:8)). F1 also makes the new probe nondeterministic under concurrency.

5. Classify each observed mismatch before ownership — UNCHANGED-BY-DELTA. Existing classifications and owners remain in the ratified table ([dispositions.md:21](docs/arch/refactor/rev11/evidence/BF3/dispositions.md:21)).

6. Independently discriminating regression for every genuine defect — NOT-EVIDENCED. The current governing AT-2 row remains ratified, but the delta concedes it has no demonstrated instance and item 6 stays not evidenced ([dispositions.md:29](docs/arch/refactor/rev11/evidence/BF3/dispositions.md:29), [line 42](docs/arch/refactor/rev11/evidence/BF3/dispositions.md:42)).

7. Record correction block/acceptance ID; no guard or removal ID — UNCHANGED-BY-DELTA. The ratified rows retain owners and acceptance IDs ([dispositions.md:23](docs/arch/refactor/rev11/evidence/BF3/dispositions.md:23)); table bytes are unchanged.

### Every Required exits sentence

1. “The full retained inventory has actual results.” — NOT-EVIDENCED. Recompile remains partial/unattributable ([inventory:8](crates/verter_session/src/framework/framework_product_surface_inventory.json:8)).

2. “`UNPROVEN` records an open proof gap and cannot count as exhaustion.” — SATISFIED-BY-DELTA. The residual is recorded as `UNKNOWN`, and acceptance is withheld ([at2-deviation-memo.md:61](docs/arch/refactor/rev11/evidence/BF3/at2-deviation-memo.md:61), [line 117](docs/arch/refactor/rev11/evidence/BF3/at2-deviation-memo.md:117)).

3. “Every genuine failure has exact … evidence … regression … owner … acceptance/test ID; no guard or removal ID exists.” — NOT-EVIDENCED. AT-2 remains a ratified genuine-defect row without an evidenced instance or discriminating regression ([dispositions.md:29](docs/arch/refactor/rev11/evidence/BF3/dispositions.md:29)).

4. “`FC-ATOMIC-001` remains non-vacuous …” — SATISFIED-BY-DELTA. Reachable failures, ordinary success, warning-only success, and search-only stale controls are all driven; batch run was `10 passed; 1 ignored` ([svelte_batch_route_tests.rs:982](crates/verter_session/src/framework/svelte_batch_route_tests.rs:982)).

5. “Route-parity tests, harness mutation controls, and correction-owner regressions replace cold-path and guard tests.” — SATISFIED-BY-DELTA. The delta adds actual host-product comparisons ([transport_route_equivalence_tests.rs:2839](crates/verter_session/src/framework/transport_route_equivalence_tests.rs:2839)) and the runtime discriminator; no production guard was added.

6. “If no genuine failure exists …” — UNCHANGED-BY-DELTA. The antecedent is false: multiple genuine DEFER rows remain ([dispositions.md:23](docs/arch/refactor/rev11/evidence/BF3/dispositions.md:23)).

7. “BF3 may close … only after AMD-009 … and BA0/BS0/BCSS0/BRT0 exist …” — UNCHANGED-BY-DELTA. Those predecessor edges exist ([program-dag.toml:81](docs/arch/refactor/rev11/program-dag.toml:81)); BF3 remains `BLOCKED` ([program-state.toml:341](docs/arch/architecture-lock/ledger/program-state.toml:341)).

8. “B2 and B3 stay locked until … all accepted.” — UNCHANGED-BY-DELTA. Both remain `LOCKED` ([program-state.toml:446](docs/arch/architecture-lock/ledger/program-state.toml:446), [line 467](docs/arch/architecture-lock/ledger/program-state.toml:467)).

## C. Findings

- F1 — P2 — [probe-bundler-route.mjs:512](packages/unplugin/scripts/probe-bundler-route.mjs:512). Every probe uses and recursively deletes the same `.verter-probe-recompile` directory before and after execution ([lines 517–558](packages/unplugin/scripts/probe-bundler-route.mjs:517)). Six simultaneous permitted probe invocations all exited 0 and reported `fresh:true`, but two recorded recompile `outcome:"error"` with `ENOENT` for `Parent.vue`/`Child.vue`; an earlier overlap left the fixture directory behind. This makes C4 flaky under ordinary concurrent test execution and allows the standalone probe to exit 0 despite a failed driven lane. Fix: allocate a unique per-invocation repository-local directory with `mkdtemp`, remove only that directory, and fail the probe if a required case records `outcome:"error"`.

OUT-OF-DELTA:

- O1 — P3 — [svelte_official_conformance_gate.rs:1812](crates/verter_session/src/compile/map_equality_tests/svelte_official_conformance_gate.rs:1812). A mounted wrong render increments both `compared` and `divergences`, then the diagnostic reports `compared + divergences`, double-counting it ([line 1831](crates/verter_session/src/compile/map_equality_tests/svelte_official_conformance_gate.rs:1831)). The same defect exists at `b980c6688`; it was not introduced here. Fix with a separate attempted/mounted counter.

Constraint checks: `packages_src_changed=0`; all seven changed crate-source paths are test-only modules; source/commit plan/programme scans returned no matches; `git diff --check` and `CARGO_BUILD_JOBS=4 cargo fmt --all --check` exited 0. All mutations were restored to their pristine hashes; worktree is clean and the probe fixture is absent.

## D. Verdict

BLOCKING because C4 introduces F1, and charter procedure items 4 and 6 plus Required-exit sentences 1 and 3 remain `NOT-EVIDENCED`.
