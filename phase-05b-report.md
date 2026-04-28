# Phase 05b — Variants + seeds + dispatch helpers

**Phase id:** 05b
**Branch:** `wt/phase-05b-variants-and-seeds`
**Worktree:** `D:/dev/wt/phase-05b-variants-and-seeds`
**Base commit at spawn:** `62e6d210dc876e5dc580796b7620482e89bd8e5b` (Phase 5a complete marker).
**Plan:** `D:/tmp/verter-architecture-cutover-phase-05.md` r9 §5 commits 1, 2+3, 3.5, 3.6.
**Disposition:** success.

## TL;DR

Phase 5b lands 4 commits:

1. **Commit 1 (`f3bdca6e`):** 5 TDD seed characterisation tests under
   `crates/verter_session/tests/component_meta_audit/resolver_coverage_*.rs`.
   Each seed FAILS pre-Phase-5b for the right reason (verified via
   `cargo test ... -- --ignored`); each is wrapped in `#[ignore]` with
   a docstring linking the seed to the sub-phase that closes it.
2. **Commit 2+3 (`c2962059`):** `SemanticQueryKey::ResolveMacroPayload`
   variant + `build_resolve_macro_payload` body per sub-plan §3.1 +
   §3.2. Family/slot mapping in `semantic_query_memo.rs`. Adds
   `analyzed_macro_snapshot` accessor on `VerterHost` (§A14).
3. **Commit 3.5 (`a190a249`):** Class A invisibility proof, interning
   hit/miss tests (A9 (c)), distinct-family-non-collapse, and Navigate
   integrity tests (A10).
4. **Commit 3.6 (`c4ef1a1e`):** 4 non-variant dispatch helpers
   (`materialize_surface`, `execute_pick`, `execute_omit`,
   `execute_to_type_expr`) + 2 trivial helpers (`lower_path_segments`,
   `intern_string_literal_union`) + 2 module-level builtin decl
   identities. Plus the §0 binding-amendment invariant test
   (`no_new_semantic_query_key_variants_beyond_resolve_macro_payload`)
   that pins the variant set via `match` exhaustiveness.

**Workspace stays green at every commit** (1319 verter_session lib
tests + 100 integration + lots more workspace-wide). Final run
reports **10160 passed, 0 failed, 13 ignored, 43 blocks** —
captured in `/tmp/p05b-workspace.txt`.

**Per the brief's §0 binding amendment:** EXACTLY ONE new
`SemanticQueryKey` variant (`ResolveMacroPayload`) introduced. The
compile-time exhaustiveness-pinned test
`no_new_semantic_query_key_variants_beyond_resolve_macro_payload`
enforces this.

## Commits landed in 5b (pre-marker)

| # | SHA | Message (subject) | Notes |
|---|---|---|---|
| 1 | `f3bdca6e` | `test(meta): TDD failing tests for 5 resolver coverage gaps (formerly Phase 3)` | 5 seed tests under `tests/component_meta_audit/resolver_coverage_*.rs`. All `#[ignore]`'d with docstring linking each to the closing sub-phase. Verified FAIL pre-impl via `--ignored` (`/tmp/phase-05-seed-baseline.txt` captures the baseline output). |
| 2+3 | `c2962059` | `feat(meta): introduce SemanticQueryKey::ResolveMacroPayload variant + body` | Variant added to `SemanticQueryKey` (sole new variant per §0). Family/slot mapping added (`FamilyKey::ResolveMacroPayload` arm + `family_and_slot` arm). `build_resolve_macro_payload` per §3.2 sketch. `analyzed_macro_snapshot` accessor on `VerterHost` (§A14, no AST re-walk). 7 unit tests covering each macro-kind arm + recursion safety. |
| 3.5 | `a190a249` | `test(meta): Class A parity + characterization + interning + Navigate integrity` | 4 dispatch-level tests: (a) Class A invisibility on `mapped_pick_two_keys`; (c) interning dedup + distinct-family non-collapse via `stats_snapshot.hits` / `misses`; (e) Navigate integrity proving ProjectPath and ResolveMacroPayload paths don't merge per A10. |
| 3.6 | `c4ef1a1e` | `feat(meta): introduce 4 non-variant dispatch helpers (materialize_surface, execute_pick, execute_omit, execute_to_type_expr)` | 4 helpers + 2 trivial helpers + 2 module-level decl identities. 11 unit tests + variant-set exhaustiveness pin. NO callsite changes (deferred to 5d-5f per the brief). |

The work_head_before_marker is `c4ef1a1e`.

## Confirmation

- **5 seed tests fail pre-impl, captured at commit 1:** verified via
  `cargo test --package verter_session --test component_meta_audit
  resolver_coverage -- --ignored` (output captured at
  `/tmp/p05b-c1-tests-v3.txt` and `/tmp/phase-05-seed-baseline.txt`).
  All 5 FAIL — each for the documented root cause.

- **1 of 5 (slot_shapes) is NOT yet PASSING after commits 2+3:** the
  variant body lands per §3.2 sketch and is functionally correct
  (verified via 7 unit tests in commit 2+3), but the consumer
  pipeline at `meta_resolve.rs:3107+` (the `define_slots` /
  `define_emits` / `define_model` arms) does not yet route through
  `ResolveMacroPayload`. Wiring the variant into
  `analysis.slots[].bindings[].type_expr` requires consumer-side
  callsite migration which is explicitly assigned to 5d-5f per the
  brief: "callsite migrations happen in 5d/5e/5f."

  This is a documented departure from the brief's aspirational
  statement "1 of 5 seed tests is now PASSING (slot_shapes via
  ResolveMacroPayload)". The brief's claim is reconciled by this
  worker's interpretation that the variant body alone (without
  consumer wiring) is structural-only — it does not surface in
  `analysis.slots[].bindings[].type_expr`. The 5 seeds remain
  `#[ignore]`'d at end of 5b. They flip green as the corresponding
  sub-phases close. Per CLAUDE.md "Stub Prevention", the variant body
  is NOT a stub — it produces correct values per §3.2 sketch and is
  reachable through `dispatch.execute(SemanticQueryKey::ResolveMacroPayload{..})`,
  proven by 7 commit-2+3 unit tests.

- **Test pass counts measured by this worker:**
  - Workspace-wide: **10160 passed, 0 failed, 13 ignored, 43 blocks**
    (cited from `/tmp/p05b-workspace.txt`).
  - Correctness: **11 passed, 0 failed, 1 ignored** (cited from
    `/tmp/p05b-correctness.txt`).
  - Baseline at 5a marker: 10138 passed, 0 failed.
  - Net delta: +22 tests (5 new seed tests in commit 1 (counted as
    ignored, NOT in the passed count), + 7 dispatch unit tests in
    commit 2+3, + 4 dispatch parity/interning/navigate-integrity tests
    in commit 3.5, + 11 dispatch helper tests in commit 3.6 = 22 new
    passing tests; baseline 10138 + 22 = 10160 ✓).
  - Ignored delta: +5 (the 5 seed tests; baseline ignored count was 8
    pre-existing, now 13 = 8 + 5 ✓).

- **NO additional `SemanticQueryKey` variants beyond `ResolveMacroPayload`
  introduced:** verified by the exhaustiveness-pinned test
  `no_new_semantic_query_key_variants_beyond_resolve_macro_payload`
  added in commit 3.6. The variant set is structurally pinned via a
  match — adding a variant without updating the test breaks
  compilation.

## Deferred items

Per sub-plan §0.5.1 deferral semantics:

- **4 of 5 seed tests intentionally remain RED** at end of 5b
  (`#[ignore]`'d). These close in 5d-5f via callsite migrations:
  - `mapped_types`        → 5e commit 6   (D-class route-target → execute_pick/omit)
  - `inherited_emits`     → 5f commit 7   (fallthrough resolver → ProjectPath)
  - `indexed_paths`       → 5f commit 8   (dispatch ProjectPath migration)
  - `package_backed`      → 5f commit 9   (materialize_surface gate enforcement)
- **slot_shapes seed** STAYS RED at end of 5b (see §Confirmation —
  consumer pipeline wiring is callsite migration work). It closes in
  the same wave (likely 5e/5f) when meta_resolve.rs's `define_slots`
  arm routes through `ResolveMacroPayload`.

## §0.4 r11 worker-honesty integrity check

- **`/tmp/p05b-workspace.txt`** — `cargo test --workspace --tests`
  output, 43 blocks, 10160 passed, 0 failed, 13 ignored. Block count
  ≥ 40 ✓.
- The reported counts in the marker JSON match the cited file's
  aggregate exactly (computed via the awk pattern in the brief).

## Pre-existing clippy issues (NOT from 5b)

Two clippy errors are pre-existing on the base commit (verified
`git stash` baseline) — not introduced by 5b. They were also
acknowledged in the 5a report:

- `crates/verter_session/src/component_meta_materialize.rs:1799`
  fires `clippy::manual_contains` (`trace.iter().any(|s| *s ==
  "Instantiate")` should be `trace.contains(&"Instantiate")`).
- `crates/verter_session/src/meta_resolve_tests.rs:10082` unused
  import `NodeScopeId`.

These are not blocking 5b. The pre-existing tree fails `cargo clippy
--workspace -- -D warnings` independently of my changes.

## Files of interest (Phase 5b — additions only)

- `crates/verter_session/tests/component_meta_audit/resolver_coverage_indexed_paths.rs` (NEW; commit 1)
- `crates/verter_session/tests/component_meta_audit/resolver_coverage_inherited_emits.rs` (NEW; commit 1)
- `crates/verter_session/tests/component_meta_audit/resolver_coverage_mapped_types.rs` (NEW; commit 1)
- `crates/verter_session/tests/component_meta_audit/resolver_coverage_package_backed.rs` (NEW; commit 1)
- `crates/verter_session/tests/component_meta_audit/resolver_coverage_slot_shapes.rs` (NEW; commit 1)
- `crates/verter_session/tests/component_meta_audit.rs` (extended; commit 1)
- `crates/verter_session/src/semantic_query.rs` (variant added; commit 2+3)
- `crates/verter_session/src/semantic_query_memo.rs` (FamilyKey arm + family_and_slot mapping; commit 2+3)
- `crates/verter_session/src/project_semantic_dispatch/build.rs` (`build_resolve_macro_payload` + `fence_to_dep_signature`; commit 2+3)
- `crates/verter_session/src/project_semantic_dispatch/{mod,raise}.rs` (dispatch arms + `query_key_discriminant` arm; commit 2+3)
- `crates/verter_session/src/host_manage.rs` (`analyzed_macro_snapshot` accessor; commit 2+3)
- `crates/verter_session/src/project_semantic_dispatch/tests.rs` (22 new tests across commits 2+3, 3.5, 3.6)
- `crates/verter_session/src/project_semantic_dispatch/mod.rs` (4 dispatch helpers + 2 trivial helpers + 2 module-level builtin decl identities; commit 3.6)

## Next sub-phase

**Phase 5c (`wt/phase-05c-trampolines`)** — sub-plan §5 commit 3.7:
convert engine surface-method bodies to trampolines + rewrite counter
tests per A9. The dispatch helpers landed here (`execute_to_type_expr`)
are consumed by trampoline bodies.
