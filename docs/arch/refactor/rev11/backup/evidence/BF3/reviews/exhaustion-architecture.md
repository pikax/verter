VERDICT: BLOCKING

## A. Per-claim verdict

| Claim | Verdict | Evidence |
|---|---|---|
| C1 — runtime discrimination | PASS | The planted test mutates rendered output while preserving mountability at [svelte_official_conformance_gate.rs:1285](crates/verter_session/src/compile/map_equality_tests/svelte_official_conformance_gate.rs:1285). Both it and the live gate use `compare_mounted_render` at [line 1714](crates/verter_session/src/compile/map_equality_tests/svelte_official_conformance_gate.rs:1714) and [line 1805](crates/verter_session/src/compile/map_equality_tests/svelte_official_conformance_gate.rs:1805). Scoped command printed `running 20 tests`; `17 passed; 3 ignored`. |
| C2 — census identity | PASS | Rows name function items and derive identity using `type_name::<F>()`: [suite_census.rs:51](crates/verter_session/src/framework/suite_census.rs:51), [suite_census.rs:71](crates/verter_session/src/framework/suite_census.rs:71). The outside anchor is [script_facts_tests.rs:29](crates/verter_session/src/framework/script_facts_tests.rs:29). My independent retarget plant had marker count `0→1→0`, changed the hash, produced `error[E0425]: cannot find value ...ARCH_REVIEW_CENSUS_PLANT`, and restored SHA-256 `5ab7e8c13410256550c29a7d6b51c374602d47c401f34955d9a05ae4a4bf7745`. Suite runs printed `running 24` (`22 passed; 2 ignored`), `running 11` (`10 passed; 1 ignored`), and `running 21` (`20 passed; 1 ignored`). Documentation has a minor inaccurate diagnostic count; see ARCH-003. |
| C3 — invocation attribution | PASS | The independent observer applies each non-alias export through an apply-counting `Proxy`: [transport_route_equivalence_tests.rs:1715](crates/verter_session/src/framework/transport_route_equivalence_tests.rs:1715), [line 1760](crates/verter_session/src/framework/transport_route_equivalence_tests.rs:1760). Executed exports require successful application, positive count, plugin-key equality, and both carrier decisions at [lines 1913–1955](crates/verter_session/src/framework/transport_route_equivalence_tests.rs:1913). The authoritative transport command printed `running 21 tests`; `20 passed; 1 ignored`. |
| C4 — bundler aliases | BLOCKING | The five groups are genuinely driven at [transport_route_equivalence_tests.rs:2894](crates/verter_session/src/framework/transport_route_equivalence_tests.rs:2894), [2984](crates/verter_session/src/framework/transport_route_equivalence_tests.rs:2984), [3061](crates/verter_session/src/framework/transport_route_equivalence_tests.rs:3061), [3163](crates/verter_session/src/framework/transport_route_equivalence_tests.rs:3163), and [3275](crates/verter_session/src/framework/transport_route_equivalence_tests.rs:3275); recompile is honestly PARTIAL at [framework_product_surface_inventory.json:8](crates/verter_session/src/framework/framework_product_surface_inventory.json:8). However, all probe processes share one fixture directory. The prescribed serial run passed, but the same scoped test without forced serialization printed `running 21 tests` then `FAILED. 19 passed; 1 failed; 1 ignored`, with `ENOTEMPTY` on `.verter-probe-recompile`. Eight parallel standalone probes also yielded one top-level-success JSON containing `vueRecompileLane.outcome:"error"` and `ENOENT ... Child.vue`. See ARCH-001. |
| C5 — batch atomicity | PASS | Reachable classes and lanes are enumerated at [svelte_batch_route_tests.rs:708](crates/verter_session/src/framework/svelte_batch_route_tests.rs:708) and [line 982](crates/verter_session/src/framework/svelte_batch_route_tests.rs:982). Class entry is proven before product absence at [lines 851–954](crates/verter_session/src/framework/svelte_batch_route_tests.rs:851). Success/warning controls begin at [line 1079](crates/verter_session/src/framework/svelte_batch_route_tests.rs:1079); the residual test explicitly claims only a search at [line 1188](crates/verter_session/src/framework/svelte_batch_route_tests.rs:1188). Scoped command printed `running 11 tests`; `10 passed; 1 ignored`. |
| C6 — escalation | BLOCKING | The ratified AT-2 row is byte-unchanged and the new note explicitly retains `NOT-EVIDENCED`: [dispositions.md:29](docs/arch/refactor/rev11/evidence/BF3/dispositions.md:29), [dispositions.md:35](docs/arch/refactor/rev11/evidence/BF3/dispositions.md:35). `cmp` of the base/head row printed `at2_row_cmp=0`. Substantively the escalation does what it claims, but both newly tracked consult files embed a developer-home path. The repository guard printed `running 1 test`, then failed with two violations. See ARCH-002. |

Commands used:

```text
CARGO_BUILD_JOBS=4 cargo test -p verter_session --lib --features bf2-authoritative svelte_official_conformance -- --test-threads=1
CARGO_BUILD_JOBS=4 cargo test -p verter_session --lib --features transport-authoritative transport_route_equivalence -- --test-threads=1
CARGO_BUILD_JOBS=4 cargo test -p verter_session --lib framework_product_surface -- --test-threads=1
CARGO_BUILD_JOBS=4 cargo test -p verter_session --lib svelte_batch_route -- --test-threads=1
node packages/unplugin/scripts/probe-bundler-route.mjs
```

The standalone probe exited 0 with `"fresh":true`.

## B. Exit-criteria enumeration

### Owned scope and required procedure

| # | Criterion | Status | Citation |
|---|---|---|---|
| 1 | Build and run the Svelte authoritative counterpart. | UNCHANGED-BY-DELTA | Scoped run: `running 20 tests`; `17 passed; 3 ignored`. Charter: [BF3.md:19](docs/arch/refactor/rev11/charters/BF3.md:19). |
| 2 | Exercise the six pinned client cells over every named axis and retain the refusal boundary. | UNCHANGED-BY-DELTA | Existing gate executed in the same 20-test run; the delta changes runtime comparison factoring, not the cell inventory or refusal boundary. [BF3.md:21](docs/arch/refactor/rev11/charters/BF3.md:21). |
| 3 | Independently planted defect plus green control for every axis. | SATISFIED-BY-DELTA | New runtime plant and before/after controls: [svelte_official_conformance_gate.rs:1285](crates/verter_session/src/compile/map_equality_tests/svelte_official_conformance_gate.rs:1285); test passed in the 20-test run. |
| 4 | Exhaust every retained product and route; aliases require route identity/publication. | NOT-EVIDENCED | Recompile remains expressly PARTIAL: [framework_product_surface_inventory.json:8](crates/verter_session/src/framework/framework_product_surface_inventory.json:8). Its probe is also non-hermetic under concurrency. |
| 5 | Classify mismatches before assigning ownership. | UNCHANGED-BY-DELTA | Existing ratified classifications remain at [dispositions.md:21](docs/arch/refactor/rev11/evidence/BF3/dispositions.md:21); post-ratification bundler classifications remain at [line 186](docs/arch/refactor/rev11/evidence/BF3/dispositions.md:186). |
| 6 | Precise independently discriminating regression for every genuine defect. | NOT-EVIDENCED | The delta itself records AT-2 item 6 as not evidenced: [dispositions.md:42](docs/arch/refactor/rev11/evidence/BF3/dispositions.md:42), [test-invocations.md:588](docs/arch/refactor/rev11/evidence/BF3/test-invocations.md:588). |
| 7 | Record correction block and acceptance ID; introduce no guard/refusal/withholding/retraction/removal ID. | UNCHANGED-BY-DELTA | Owners and acceptance IDs remain in [dispositions.md:23](docs/arch/refactor/rev11/evidence/BF3/dispositions.md:23). Diff inspection found no production mechanism or correction. |

### Every sentence of “Required exits”

| # | Sentence | Status | Citation |
|---|---|---|---|
| 1 | “The full retained inventory has actual results.” | NOT-EVIDENCED | Recompile is explicitly only partially driven: [framework_product_surface_inventory.json:8](crates/verter_session/src/framework/framework_product_surface_inventory.json:8). |
| 2 | “`UNPROVEN` records an open proof gap and cannot count as exhaustion.” | SATISFIED-BY-DELTA | The residual is recorded as `UNKNOWN, not closed`: [dispositions.md:97](docs/arch/refactor/rev11/evidence/BF3/dispositions.md:97). |
| 3 | Every genuine failure has exact evidence, regression, classification, owner, and acceptance ID; no guard/removal ID. | NOT-EVIDENCED | AT-2 lacks the required independently discriminating regression: [dispositions.md:42](docs/arch/refactor/rev11/evidence/BF3/dispositions.md:42). |
| 4 | `FC-ATOMIC-001` is non-vacuous for success and genuine refusal. | NOT-EVIDENCED | Reachable batch classes are well tested, but the ratified AT-2 refusal claim remains unsupported, and existing AT-1 records a real mixed result: [dispositions.md:28](docs/arch/refactor/rev11/evidence/BF3/dispositions.md:28). |
| 5 | Route-parity tests, mutation controls, and owner regressions replace cold-path/guard tests. | NOT-EVIDENCED | New parity tests exist, but the probe can collide and return an errored lane under top-level success; recompile also remains unattributable. [probe-bundler-route.mjs:512](packages/unplugin/scripts/probe-bundler-route.mjs:512). |
| 6 | If no genuine failure exists, only per-failure clauses are vacuous; all other exits remain mandatory. | UNCHANGED-BY-DELTA | The delta does not claim otherwise and explicitly retains the AT-2/inventory gaps: [dispositions.md:147](docs/arch/refactor/rev11/evidence/BF3/dispositions.md:147). |
| 7 | BF3 closes only after AMD-009 ratification and BA0/BS0/BCSS0/BRT0 exist as mandatory predecessors. | UNCHANGED-BY-DELTA | Ratification is recorded at [amd009-ratification-packet.md:1](docs/arch/refactor/rev11/evidence/BF3/amd009-ratification-packet.md:1); predecessor blocks remain locked at [program-state.toml:362](docs/arch/architecture-lock/ledger/program-state.toml:362). |
| 8 | B2/B3 stay locked until all six predecessors are accepted. | UNCHANGED-BY-DELTA | B2 and B3 remain `LOCKED`, with the six predecessors named: [program-state.toml:447](docs/arch/architecture-lock/ledger/program-state.toml:447), [program-state.toml:468](docs/arch/architecture-lock/ledger/program-state.toml:468). |

## C. Findings

### ARCH-001 — P2

- File: [probe-bundler-route.mjs:512](packages/unplugin/scripts/probe-bundler-route.mjs:512)
- Wrong: every probe invocation deletes, creates, writes, and finally deletes the same `.verter-probe-recompile` directory at [lines 517–558](packages/unplugin/scripts/probe-bundler-route.mjs:517). Concurrent tests race, producing `ENOENT`/`ENOTEMPTY`; one probe can still exit 0 with `"fresh":true` while recording the recompile lane as `"error"`.
- Fix: create a unique repository-local fixture with `mkdtemp`, validate its parent/prefix in Rust, and clean only that invocation’s directory.

### ARCH-002 — P2

- Files: [at2-disposition-prompt.md:4](docs/arch/refactor/rev11/evidence/BF3/at2-disposition-prompt.md:4), [at2-disposition-ruling.md:17](docs/arch/refactor/rev11/evidence/BF3/at2-disposition-ruling.md:17)
- Wrong: new tracked evidence embeds a developer home path. The existing fail-closed guard at [tracked_paths_no_machine_roots.rs:428](crates/verter_session/tests/cases/tracked_paths_no_machine_roots.rs:428) fails with two violations.
- Fix: regenerate the prompt and verbatim ruling using repository-relative citations, or stop claiming literal verbatim preservation and normalize every path.

### ARCH-003 — P3

- File: [landing-record.md:678](docs/arch/refactor/rev11/evidence/BF3/landing-record.md:678)
- Wrong: it says removing all four module declarations yields seven `E0433` sites across four modules. The detailed evidence says that removal yields one outside-anchor error; seven sites across four modules is the result of removing only `suite_census`.
- Fix: distinguish the two mutations and report their actual diagnostics separately.

## Constraint audit

- No `packages/*/src/**` changes.
- Changed Rust `src` files are test modules behind existing `#[cfg(test)]` registration.
- No production guard, refusal, withholding, retraction, runtime tracker, known-divergence list, or compiler correction found.
- No stub tests found.
- Census binding is compiler/type-system enforced, not source-name scanning.
- `git diff --check` passed.
- Commit-subject search found no prohibited plan/programme vocabulary.
- Worktree was restored clean; `.verter-probe-recompile` is absent.

## OUT-OF-DELTA

- OD-001: The ratified AT-2 factual claim still has no demonstrated reachable instance and item 6 remains open. This predates the reviewed delta; the delta correctly records but does not resolve it. [dispositions.md:29](docs/arch/refactor/rev11/evidence/BF3/dispositions.md:29), [dispositions.md:97](docs/arch/refactor/rev11/evidence/BF3/dispositions.md:97).
