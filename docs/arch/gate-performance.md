# Gate performance

Working notes for `node scripts/gate.mjs`, the canonical Rust gate. See
[`docs/arch/refactor/rev11/rulings/MAINTAINER-DIRECTIVE-GATE-PERFORMANCE-BLOCK.md`](refactor/rev11/rulings/MAINTAINER-DIRECTIVE-GATE-PERFORMANCE-BLOCK.md),
[`MAINTAINER-DIRECTIVE-ONE-BUILD-ONE-RUN.md`](refactor/rev11/rulings/MAINTAINER-DIRECTIVE-ONE-BUILD-ONE-RUN.md),
and [`MAINTAINER-DIRECTIVE-SINGLE-TEST-UNIVERSE.md`](refactor/rev11/rulings/MAINTAINER-DIRECTIVE-SINGLE-TEST-UNIVERSE.md)
for the ratified plan this document reports evidence against.

## Memory ceiling — measured, not assumed (step 1)

`parsePosixProcessTableRss` used to sum RSS by POSIX process-GROUP membership,
which undercounted catastrophically (nextest reassigns each executing test to
its own fresh process group, so a pgid-based sum saw only the
`cargo`/`cargo-nextest` wrapper — "43 MiB across 1 process(es)" on a real
full-workspace run). Fixed to a parent-pid TREE walk. That fix is not
report-only: `parsePosixProcessTableRss` feeds the memory-ceiling watchdog
(`gate-internals.mjs` `runContainedStep`), so it arms the ceiling for the
first time — a ceiling that had never realistically been reachable before.

A dedicated `node scripts/gate.mjs --memory-limit 18GiB` run on a 24 GiB host
(default ceiling `max(512MiB, 50% RAM)` = 12 GiB) measured peak RSS per phase
with the corrected sampler:

| phase                        | peak RSS  | processes\* |
|-------------------------------|-----------|-------------|
| archive build (dev profile)   | 4.87 GiB  | —           |
| SURFACE 1 (nextest run)       | 1.88 GiB  | —           |
| SURFACE 2 (in-process)        | 1.54 GiB  | —           |
| archive build (shipped-cfg)   | 5.50 GiB  | —           |
| SURFACE 3 (nextest run)       | 1.02 GiB  | —           |

\* The per-sample process count reported alongside peak RSS was, at
measurement time, a second bug in the same telemetry: `runContainedStep`
overwrote its process-count field on every watchdog tick rather than
capturing the count AT the sample that set the new peak, so a build's true
mid-run peak (several concurrent rustc) could be reported paired with a much
later, lower-parallelism sample's count. Fixed by tracking a dedicated
`peakRssProcessCount`, updated only when a sample ties or extends the running
max — see the `peakRssProcessCount` doc comment in `gate-internals.mjs` and
the discriminating scripted-sampler regression test in
`gate-memory-selftest.mjs`.

**Ceiling decision:** the measured maximum across every phase is **5.50 GiB**
(shipped-cfg archive build), 45.8% of the 12 GiB default — **6.5 GiB of
headroom**. The default ceiling is correct as-is and was NOT raised; arming
the watchdog does not break a default (no `--memory-limit`) run on this class
of host. This machine's default is itself `50% of physical RAM`, so a much
smaller host (roughly <11 GiB RAM) would compute a default ceiling below the
measured absolute peak — noted for awareness, not acted on here, since no
such host is in scope for this measurement.

The gate run that produced this table also surfaced one pre-existing,
unrelated failure: `verter_lsp sync_coordinator::tests::hanging_provider_diagnostics_do_not_starve_verter_owned_batch`,
owned by a different block.

## Surface-replay reduction (step 2)

The directive named two files (`architecture_guards.rs`,
`output_projector_residual_guards.rs`, ~250 tests / ~40,000 lines) as the
source-scanning inventory compiled into `verter_session`'s consolidated test
binary and replayed under all three surfaces for no behavioral reason (a
source scan has no shared-process failure mode and no `debug_assertions`
sensitivity). The real surface, measured on the tree, was twelve files.
Verified per file (not by filename) rather than assumed:

| file | verdict | why |
|---|---|---|
| `architecture_guards.rs` | **kept in `verter_session`** | one `#[test]` (`foundations_guards::external_corpus_paths_not_present_outside_gated_tests`) is bound by name into `typeinfo_ignored_test_manifest.rs`'s `live_guard!` registry, which requires the bound test to run in the SAME binary the canonical gates execute; the file's `foundations_guards`/`w5f_test_archaeology`/`general_test_archaeology`/`packages_ts_archaeology` submodules also share internal helper functions across that boundary. Relocating it needs a scoped design decision (repoint the one binding, or prove the "same binary" invariant can relax), not a same-session mechanical move. |
| `output_projector_residual_guards.rs` | moved | pure scan plus a check against `verter_session`'s public API (sealed `TypeExpr`/`NoTypeExpr` capability fence) — never runs it |
| `whole_env_consumer_graph_native_inventory.rs` | moved | pure `syn`/`walkdir` scan of `verter_session`'s `src/` |
| `residual_type_expr_body_reader_inventory.rs` | moved | pure `syn`/`walkdir` scan |
| `handle_capable_consumer_guards.rs` | moved | pure `syn`/`walkdir` scan (reads sibling `verter_session` test files that stayed behind, e.g. `architecture_guards.rs`) |
| `tracked_paths_are_portable.rs` | moved | scan plus a check against `verter_session`'s public API (`framework::descriptor` registry) — never runs it |
| `scanners_replacement.rs` | moved | pure JSON/schema scan |
| `tracked_paths_no_machine_roots.rs` | moved | pure `git ls-files` scan |
| `framework_known_bug_manifest.rs` | moved | pure `syn` scan |
| `svelte_typecheck_gate.rs` | kept | drives a real `tsc.js` subprocess — not a scan |
| `vue_macro_tsc_typecheck_gate.rs` | kept | drives a real `tsc.js` subprocess — not a scan |
| `defect_b_corpus_prevention_gate.rs` | kept, N/A | `#![cfg(feature = "external-corpus")]` — already excluded from the default gate entirely; not compiled into any of the three surfaces to begin with |

The eight movable files relocated to a new crate, `verter_source_policy_gate`
(`crates/verter_source_policy_gate/tests/cases/`), with its own single
`tests/main.rs` (Anti-Binary-Growth-compliant — no allowlist entry needed).
`cargo nextest run --workspace` (Surface 1) still runs its 180 tests once, as
before; Surface 2 (`verter_session`-only shared-process) and Surface 3 (the
five-package filterset above) select by PACKAGE, so this crate's tests are
structurally invisible to both regardless of what it depends on — no filter
change was needed on either surface to achieve the exclusion. Two of the
eight (`tracked_paths_are_portable.rs`, `output_projector_residual_guards.rs`)
turned out to depend on `verter_session`'s public API partway through the
file (not visible from the file's own header `use` block) rather than being
pure disk scans; that dependency does not affect the exclusion, since it is
a library dependency, not a package-selection criterion.

Four of the eight scan `verter_session`'s own `src/` tree, so their
`crate_root()` helper — previously `CARGO_MANIFEST_DIR`, correct only because
`CARGO_MANIFEST_DIR` used to BE `verter_session`'s own directory — was
re-anchored to `workspace_root().join("crates/verter_session")` explicitly.
`output_projector_residual_guards.rs` additionally has one genuine
self-reference (reading its own source for a doc/list-parity check), which
resolves against the new crate's own `CARGO_MANIFEST_DIR` via a second,
distinct `own_crate_root()` helper — the two must not be conflated.

Verified: `cargo test -p verter_source_policy_gate` (180/180 passing,
non-vacuous — several tests assert the production scan found real production
matches, not an empty/mocked tree), plus
`cargo test -p verter_session --test main -- cases::typeinfo_ignored_test_manifest cases::g_misc0::critical_rules_have_guards cases::architecture_guards cases::integration_test_layout_guard`
(318/319, 1 pre-existing `#[ignore]`, 0 failed) — covering the R6 registry
scanner (finds the moved guard names at their new path), the `live_guard!`
binary-identity binding (still resolves — `architecture_guards.rs` stayed
put), and the anti-binary-growth layout guard (the new crate needs no
allowlist entry).

The non-blocking oversize-source-line advisory (scan of `crates/*/src` for
files over 1,500 lines) used to run synchronously before the mutex
acquisition, ahead of the archive build. It is now kicked off on a deferred
microtask and joined in the outer `finally` — after `runGate`/`runPrepare`
returns — so its cost — a fast synchronous scan, but still nonzero —
overlaps the multi-hundred-second archive build instead of serializing ahead
of it. On a gate path that reaches a terminal verdict the advisory line
therefore prints just after `VERDICT:`. It does not universally follow a
verdict: `--prepare` prints `PREPARED_NOT_GATE` instead, and several early
`runGate` returns emit no `VERDICT:` line at all.

Not done in this step (later blocks, per the ratified plan): removing
Surface 2 as a blanket rerun, removing the second whole-workspace archive,
or moving `architecture_guards.rs`. The deletion bar is unchanged — a surface
is removed only after a seeded-defect mutation proves its replacement
catches what the surface existed for.

## Removing Surface 2 and Surface 3 (step 3)

Implements the SINGLE-TEST-UNIVERSE directive. Surface 2 (the direct
in-process `verter_session` libtest replay) is deleted entirely — its
shared-process rationale now lives inside Surface 1's ONE archive/run as
`verter_session/tests/cases/shared_process_contract.rs`, ordinary `#[test]`
functions that perform many operations sequentially in the one process
nextest already gives each test. Surface 3 (a second whole-workspace archive
under `no-debug-assertions`, plus a 5-package/15,454-test filtered nextest
run) is deleted and replaced with a small shipped-cfg guard:
`cargo check --workspace --all-targets --profile no-debug-assertions`
(compile-only) followed by `cargo nextest run -p verter_shipped_cfg_contract
--cargo-profile no-debug-assertions` (package-scoped, not `--workspace` —
never a second whole-workspace archive).

**Seeded-defect proof, Surface 2 replacement.** Temporarily wrapped
`VerterHost::get_component_meta` with a process-global `HashMap` keyed only
on canonical id, ignoring host identity — exactly the class Surface 2 existed
to catch. `cargo test -p verter_session --test main shared_process_contract::`
went from 8/8 passing to 4/8 failing:
`create_use_drop_recreate_hosts_stay_independent`,
`multiple_hosts_coexist_in_one_process_without_cross_contamination`,
`cache_and_registry_invalidation_survives_many_repeated_edits`,
`failure_then_recovery_in_one_host_and_process` — each failing with the
leaked host's stale prop names instead of the querying host's own content.
Seed reverted, all 8 pass again.

**Seeded-defect proof, Surface 3 replacement.** Temporarily split
`get_component_meta` into a `#[cfg(debug_assertions)]` correct arm and a
`#[cfg(not(debug_assertions))]` arm that drops the last resolved prop — the
same observable hazard a `debug_assert!(mutating_call())` produces (correct
under `dev`, silently different under the shipped configuration).

At the time of this proof `verter_shipped_cfg_contract` carried 8 tests
total: 6 behavioral tests under `mod shipped_cfg_behaviour` (profile-
agnostic — expected to pass under EITHER profile) plus 2 profile-sanity
canaries under `mod profile_contract`
(`debug_assertions_are_off_under_this_profile`,
`overflow_checks_are_off_under_this_profile` — DESIGNED to fail under `dev`
and pass only under `no-debug-assertions`, by construction, seed or no
seed). An unfiltered `cargo test -p verter_shipped_cfg_contract` therefore
cannot report "6/6" under `dev` — the two canaries run too and fail
regardless. The actual commands, filtered to isolate the 6 behavioral tests
so the seed's effect is unambiguous:

- `cargo test -p verter_shipped_cfg_contract shipped_cfg_behaviour::` under
  `dev`: stayed 6/6 passing (the seed is inert under `dev` — the
  `#[cfg(debug_assertions)]` arm is what compiles in, so the correct value is
  still returned — proving Surface 1 alone cannot see this class).
- `cargo test -p verter_shipped_cfg_contract --profile no-debug-assertions
  shipped_cfg_behaviour::` (bare `cargo test`'s custom-profile flag is
  `--profile`, not nextest's `--cargo-profile`): went to 6 FAILED / 0 passed
  (the `#[cfg(not(debug_assertions))]` arm compiles in instead and drops the
  prop).
- Unfiltered `cargo nextest run -p verter_shipped_cfg_contract
  --cargo-profile no-debug-assertions` (all 8 tests, matching what the live
  guard actually runs): 2 passed (the two canaries, which pass under this
  profile by construction) / 6 failed (the seeded behavioral tests).

Seed reverted: unfiltered `cargo nextest run -p verter_shipped_cfg_contract
--cargo-profile no-debug-assertions` back to 8/8 passing.

**Structural-elimination audit** (the three SINGLE-TEST-UNIVERSE
prerequisites for retiring the guard entirely): every
`debug_assert!`/`debug_assert_eq!`/`debug_assert_ne!` call in `crates/*/src`
carrying a method/function call was enumerated (134 call-bearing sites out
of 274 total) and inspected — none perform semantic work; every hit is a
pure observation (`is_none`, `is_empty`, `len`, `matches!`, `is_char_boundary`,
etc.). Every production `#[cfg(debug_assertions)]` block
(`resolver_core/runtime_values.rs`, `parse.rs`, `host_manage/eval_env.rs`) is
a non-breaking oracle cross-check whose comparison result never feeds the
returned value. Explicit overflow-safe arithmetic (`checked_*`/`wrapping_*`/
`saturating_*`) was **not** audited workspace-wide — out of scope for this
block. Two of three prerequisites verified holding; the third is unverified
and clearly not holding project-wide, so the guard stays per the directive.

**Measured, this branch, both runs with `node scripts/gate.mjs
--memory-limit 16GiB` on a heavily loaded shared host** (several unrelated
concurrent `cargo`/`rust-lock.sh` jobs from other agents were running
throughout both measurements — Surface 1's absolute wall-clock is noisy
against the pre-step-3 497s baseline for that reason):

| phase | before (this branch) | after |
|---|---|---|
| dev archive build | ~443s cold / 65s warm | 10s (warm, unchanged mechanism) |
| SURFACE 1 | 497s, 24777 tests | 712-925s, 24785-24793 tests (contended host; test count differs only by the 8 new `shared_process_contract` tests) |
| SURFACE 2 | 3 suites in-process (sequential, on top of the above) | **removed** |
| SURFACE 3 | 221s test run + a second whole-workspace archive build | **removed** |
| shipped-cfg guard | n/a | 100s check + 68s run = 168s (cold, first commit) → 6s + 5s = 11s (warm, second run) |

The unambiguous, contention-independent win is the shipped-cfg guard number:
what used to require a full second `cargo nextest archive --workspace`
compile (~443s cold / 65s warm) plus a 221s/15,454-test run now costs 11s
warm — no second whole-workspace compile at all, ever. Final verdict both
runs: `VERDICT: PASS` (first run, pre-existing-in-this-branch failures fixed
in the same commit; second run clean).
