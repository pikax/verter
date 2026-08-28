# BV0 context packet

**Base:** `3c9d72df9` (`program/architecture-lock` tip immediately before this landing; predecessor
BV0A accepted at `7b017b955e7d16bdaf58f21a8f63a247043f3572`).
**Candidate:** `1c5e8e53fa8e0a963e18554b76069ac8b39df55e`, tree
`7bb1066eac730b58257b867376c080f4c747fc04`.
**Charter:** `docs/arch/refactor/rev11/charters/BV0.md` (Immediate Vue known-defect correction), as
amended by AMD-007 §4 and AMD-008 (predecessor changed to BV0A; mapping-scope clarification; the
literal 36/36 exit reinforcement).

## What this block does

Corrects the genuine Vue script-emitter, VDOM, Vapor, SSR, and source-map defects the exact 36-cell
BF2 seed matrix exposed, one at a time, each with a failing regression test first and independent
verification against the vendored `vuejs/core v3.6.0-rc.3` compiler and runtime. Builds the full-axis
(parse/link/structure/runtime/diagnostics/mapping) gating harness the charter's required procedure
implies but that did not previously exist (`crates/verter_session/src/compile/map_equality_tests/bf2_full_axis_gate.rs`),
reusing BV0A's locked seed-manifest reader and typed assembled-module result. All 36 exact BF2 seed
cells pass genuinely, oracle-provisioned, every applicable axis reporting `ran`.

Along the way, builds a Vapor per-region template-splitting/anchor-wiring capability that did not
exist at all before this landing (the backend previously produced one flattened template regardless
of nesting), a two-phase (transform-prepass/emit) dynamic-id allocation model matching official's own
allocation order, and an additive, authority-gated `CodeTransform` primitive
(`try_overwrite_segmented`) giving VDOM/Vapor/SSR static attributes and interpolations genuine
per-anchor source-map provenance where a whole-span overwrite previously fabricated one.

## Owned scope

Touches script-emitter (`crates/verter_compiler/src/script/**`, `verter_session/src/compile.rs`,
`types.rs`), VDOM/Vapor/SSR template codegen (`crates/verter_compiler/src/template/code_gen/**`), the
shared `CodeTransform` layer (`crates/verter_compiler/src/code_transform/**`), the 32-seed corpus's
tracked-divergence ledger and its golden generator (`packages/vue-conformance-oracle/`,
`crates/verter_vue_conformance/corpus/known-divergences.json`), and the BV0A-owned map-composition
reference/harness where a new `CompileProfile` field needed threading through
(`packages/framework-conformance-harness/{src,test,spec,vectors}/assembled-map-*`). Deletes two
now-stale Vue tracking/backlog documents (`docs/arch/future/vue-vdom-parity-backlog.md`,
`docs/arch/future/vapor-parity-plan.md`) and one stale divergence-doc
(`docs/arch/ssr-noninline-shape-divergence.md`, whose own exit criterion this landing meets). No
Svelte file touched, no B3/B4 authority introduced, no IDE codegen path touched, no BV0A oracle/
invocation/composition-algebra change.

Three backend-private/additive capability extensions were authorized mid-implementation by independent
Codex xhigh architecture-scope consults, each confined to the guardrails the ruling specified (full
text in `landing-record.md`): the Vapor per-region template-splitting capability, the Vapor two-phase
id-allocation split, and the additive `CodeTransform` segmented-overwrite primitive. One residual —
a `<template v-if>` wrapping a nested `v-if` in the Vapor backend is not yet a transparent root
element — is DEFERRED to BV1 under a fourth Codex disposition ruling, recorded as debt row
`FC-VUE-001` (acceptance test:
`crates/verter_session/src/compile/map_equality_tests/nested_v_for_runtime_proof.rs::template_v_if_wrapping_inner_v_if_mounts_and_renders_inner_content`,
currently `#[ignore]`d with a precise reason).

## Unintegrated prior work — investigated, not reused

Two branches predating this session (`backup/pre-bv0-squash`, tree-identical to the reverted, never-
accepted round-1 candidate `backup/bv0-round1-candidate`; and `work/bv0-relanding`, a distinct, later
WIP branch that predates BF2 reopen #4 and BV0A) were investigated for reusable content. Neither was
reused: both predate BV0A's landed assembler rewrite and BF2 reopen #4's harness fixes, a direct
cherry-pick of the most promising isolated commit failed to compile against the current tree, and
the reverted candidate's own tree carried the exact failure modes (self-introduced regressions
absorbed into the divergence ledger, a bundled out-of-charter Svelte migration, a large speculative
Vapor rewrite, a stale contradicted doc, a matched-wrong-golden bug) that this landing was built to
avoid repeating. This implementation is a fresh, independent TDD build against the current
BV0A-accepted tree.

## Review process

Three full independent review rounds (conformance/architecture/adversarial, Codex xhigh + Grok xhigh,
each in a dedicated worktree), each finding real defects the prior round's fixes had not fully
closed; a fourth, narrower fix cycle addressing two issues the track orchestrator's own direct
`node scripts/gate.mjs` run surfaced (neither caught by any review round, since none ran the full
canonical gate); a fifth, narrower fix cycle addressing a JS-side test-fixture gap the track
orchestrator's own direct harness-suite run surfaced. Full arc, every round's findings, and the four
architecture-scope/disposition consult rulings are in `landing-record.md`.

## Verification (track orchestrator, independent)

- `node scripts/gate.mjs --build-jobs 4 --test-threads 4 --memory-limit 12GiB`: run independently
  twice by the track orchestrator on the final candidate tree (once immediately after the last Rust
  fix, once as the final confirming pass) — both PASS, all three surfaces green (Surface 1
  24357/24357, Surface 2 3/3 suites clean, Surface 3 8591/8591), zero tolerated failures.
- `cargo clippy --workspace --all-targets -- -D warnings`: clean.
- `cargo clippy --target wasm32-unknown-unknown -p verter_wasm -- -D warnings`: clean.
- `cargo fmt --all --check`: clean.
- `cargo check --workspace --release`: clean.
- `packages/framework-conformance-harness` (`npx vitest run`): 616/616, independently re-run by the
  track orchestrator (initially found 102 failures from two stale local test fixtures that predated
  a new required schema field; fixed and re-confirmed green).
- Full-axis BF2 seed-matrix gate (`cargo test -p verter_session --lib --features bf2-authoritative`):
  independently re-run by the track orchestrator, 36/36 cells pass, mutation-discrimination control
  green.
- Ancestry, commit content, and squash-diff identity confirmed directly via `git` (squash-merge diff
  matched the pre-squash worktree diff exactly; not taken from the implementer's prose report).
- Commit message hygiene: clean (no plan/block/round vocabulary in the landed `fix(core)` commit;
  evidence-tree references are exempt per governance convention).
