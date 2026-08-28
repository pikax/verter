# BV0A context packet

**Base:** `f15fa5376` (`program/architecture-lock` tip immediately before this landing).
**Candidate:** `7b017b955e7d16bdaf58f21a8f63a247043f3572`, tree
`20372516e639c57eb7bd86de8d27798e341ada9a`.
**Charter:** `docs/arch/refactor/rev11/charters/BV0A.md`, as amended by
[AMD-008](../../amendments/AMD-008-bv0a-assembly-neutral-exit.md) (RATIFIED, ratification-bundle
`a1f6523ce752db969e19e073cb0b21c5a038e9a1`, reviewed-package `d75a6f79f34736d0347de69470a367b43d0bbeb7`,
tree `54f34b50b108582aed54975f2eb174881aa6ba92`). AMD-008 itself amends the predecessor
[AMD-007](../../amendments/AMD-007-assembled-module-source-map-interim.md) (RATIFIED, landed at
`acabec8fa`).

## What this block does

Replaces the bare-`String` return of `verter_session::compile::assemble_vue_main_module` with one
typed result coupling the assembled Vue main-module code to a genuinely composed source map, so
BV0's 36-cell BF2 seed matrix can perform its required mapping validation. Composition runs the
existing script and template output maps through two real, sequential `CodeTransform` rewrite
passes (`__sfc__` → `_sfc_main`, then `export default _sfc_main;\n` removal), each driving both
code and map. BF2's accepted authored-source oracle is unchanged and unreopened.

## Owned scope (per AMD-008 §2, redefining BV0A's acceptance boundary)

Acceptance is exact ORDERED equality of the complete decoded map artifact — code, map fields, and
segment sequence — against an independently computed, input-only JavaScript reference, under real
`CodeTransform` rewrite semantics, governed by a two-layer specification: a frozen semantic layer
(layer 1) reviewed and frozen BEFORE either implementation was written against it, and a literal
vector coverage set (layer 2) completed and frozen at BV0A's acceptance. Oracle-violation
attribution — AMD-007's original mechanism, found insufficient across rounds 3–11 of AMD-008's own
ratification review — is deleted entirely; BF2's oracle still runs once per cell and its non-clean
MAPPING verdict is explicitly excluded from BV0A's own gate (residual fragment-emitter defects stay
BV0's).

## What triggered this landing session

The prior track-orchestrator session (agent `a7fa42c2dc61f567c`) had landed a real correctness fix
(`6ba1b2778` — an ignore-list upper-bound check misplaced at the wrong validation stage) plus
several doc corrections found by its round-3 conformance re-review, but the process died before
dispatching the promised re-review of that fix. This session: re-verified round 3's fix and doc
corrections directly against source; dispatched a fresh conformance round (round 4) which found two
further real defects the fix itself introduced (detailed in `landing-record.md`); dispatched a fix
cycle and a closing round 5; ran the full canonical gate; found and reconciled a stale-fork
machine-path regression against the current `program/architecture-lock` tip; and landed.

## Scope discipline

Touches only `crates/verter_compiler/src/code_transform/{mod.rs,source_map.rs,chain.rs,chain_tests.rs}`,
`crates/verter_session/src/{compile.rs,compile/*,lib.rs,host_resolve/virtual_file_pipeline.rs}` and
their test suites, `crates/verter_session/Cargo.toml`, `crates/verter_workspace/tool-output-allowlist.toml`,
`packages/framework-conformance-harness/{bin,src,test,spec,vectors}/assembled-map-*`, and this
evidence tree. No B3/B4/BV1/B5 authority introduced, no universal IR, no Svelte path, no change to
BF2's oracle or invocation, no identifier rename (`__sfc__`/`_sfc_main` both investigated and
rejected per AMD-008 owned-scope item 5).

## Review process

Full history — the AMD-008 ratification's twelve review rounds, the layer-1 freeze's seven rounds,
the layer-2 vector suite's two dedicated rounds, and this session's three additional rounds — is in
`landing-record.md` in this directory.

## Verification (program orchestrator, independent)

- `node scripts/gate.mjs` (capped `--build-jobs 4 --test-threads 4 --memory-limit 12GiB`), full
  three-surface run on the reconciled candidate tree: PASS — Surface 1 24262/24262, Surface 2 3/3
  suites clean, Surface 3 (shipped `no-debug-assertions` cfg) 8587/8587, zero tolerated failures.
- `cargo clippy --workspace --all-targets -- -D warnings`: clean.
- `cargo fmt --all --check`: clean.
- Targeted re-run: `cargo test -p verter_session --lib compile::` — 73/73.
- `pnpm test` failures in a fresh worktree (`typeinfo`, `framework-conformance-harness`
  closure-drift/mapping-oracle-composition) independently proven pre-existing and unrelated to this
  candidate via a control worktree built from unmodified `program/architecture-lock` with the
  identical fresh-build sequence (byte-identical failure set) and a clean pass (typeinfo 31/31,
  framework-conformance-harness 411/411) on the long-lived, pre-built main checkout.
- Ancestry, commit content, and machine-path-marker absence confirmed directly via `git` (not taken
  from the track orchestrator's prose report).
- Commit message hygiene: clean (no plan vocabulary in the landed `feat(core)` commit; block/amendment
  IDs are fine in this evidence tree itself, per governance convention for evidence documents).
