# BV0 landing record

Base: `3c9d72df9` (`program/architecture-lock` tip, BV0A accepted). Candidate lands as one squashed
commit, `1c5e8e53f`.

## Arc

1. **Investigation.** Two unintegrated prior branches (`backup/pre-bv0-squash` — tree-identical to
   the reverted, never-accepted round-1 candidate; `work/bv0-relanding` — a distinct, later WIP
   branch predating BF2 reopen #4 and BV0A) were read for orientation and diagnostic leads only,
   never cherry-picked or merged. A direct cherry-pick attempt of the most isolated, plausible commit
   from the first branch failed to compile against BV0A's landed assembler-result type change,
   confirming both branches were structurally stale as well as tainted by the reverted candidate's
   review failures.
2. **Fresh implementation**, 13 rounds in one resumed `claude` CLI session, each with a failing
   regression test first, verified against the vendored rc.3 compiler/runtime, honest per-round
   progress reporting (the full defect list is in `context-packet.md`'s companion `git log` and the
   squashed commit's own body). Key structural milestones, each requiring a dedicated Codex xhigh
   architecture-scope consult before implementation (full ruling text below):
   - Round 4: a Vapor-private per-region template-splitting/anchor-wiring capability — the backend
     had none at all; nested v-if/v-for/`<slot>` collapsed into one flattened template. Ruling:
     **ADOPT-NOW**, confined to `template/code_gen/vapor/**`.
   - Round 7: a two-phase (transform-prepass, then emit) dynamic-id allocation split, matching
     official's own two-pass model. Ruling: **ADOPT-NOW**, same confinement, explicitly distinguished
     from a forbidden second file-read/parse/general-IR pass (a backend-private re-walk of the
     already-parsed template AST, not a new traversal architecture).
   - Round 12: an additive, opt-in `CodeTransform` segmented-overwrite primitive
     (`try_overwrite_segmented`, `Chunk::OverwrittenSegmented`) for source-map position-tracking,
     since Vapor/SSR's single-monolithic-overwrite emission strategy was structurally incompatible
     with the existing position-aware primitive. Ruling: **ADOPT-NOW**, preferred over migrating
     Vapor/SSR's emission structure, with a mandatory guardrail list (byte-identical for every
     non-opt-in caller including IDE; a real structural call-site restriction; fail-closed refusal,
     never a silent fallback to plain `overwrite`).
   - Fix round 3 (post review-round-3): a `<template v-if>` wrapping a nested `v-if` needs the
     branch's root-element machinery to recognize a template-wrapped body as owning no template of
     its own. An attempted fix traded a silent content drop for a different runtime error and was
     reverted rather than landed half-working. Disposition ruling: **DEFER-to-BV1**, debt row
     `FC-VUE-001` (below).
3. **Three full independent review rounds** (this program's cap), each in dedicated worktrees,
   conformance + architecture (Codex xhigh) + adversarial (Grok xhigh, explicit default-to-block
   posture), each round finding real residue the prior fix cycle had not fully closed:
   - **Round 1** (candidate `247dc4209`): all three **BLOCKING**. Real findings: the additive
     `SegmentedOverwriteAuthority` guard was not actually a structural restriction (the public
     wrapper didn't require the token, IDE code could reach it via `apply_to`, and refusal silently
     fell back to plain `overwrite` — reintroducing the false provenance the primitive existed to
     remove); a new `PendingNavRequest` type leaked into shared `types.rs` instead of staying
     Vapor-private; SSR mapping placement was recovered by scanning generated output
     (`attrs_obj.find(...)`) instead of being tracked at write time; production comments referenced
     plan/round vocabulary; two `known-divergences.json` waiver rows repeated round-1-of-the-original-
     (reverted)-candidate's exact failure pattern; a historical byte-pin baseline test
     (`assembled_code_bytes_match_the_pinned_baseline`) could never pass again since BV0 intentionally
     changes the bytes it pins; no retained TDD red-first evidence; a nested-`v-for` fix was later
     found (by round 2's adversarial pass) to have corrected only the identifier spelling, not the
     actual scope structure — a real `ReferenceError`.
   - **Round 2** (candidate `ccfe3666c`): all three **BLOCKING**, smaller residue. The authority guard
     and SSR write-time capture were now genuinely fixed (independently confirmed via direct code
     reading, not just running the tests); `PendingNavRequest` was relocated but its visibility was
     never actually narrowed (still `pub(crate)` + re-exported); 14 more stale ledger notes found on a
     full 63-entry audit; `docs/arch/future/vue-vdom-parity-backlog.md` still self-described as an
     open, actionable backlog despite a disclaimer paragraph; more phase-archaeology comments
     (including in files the fix round itself introduced); a false "byte-for-byte unchanged" doc claim.
   - **Round 3** (candidate `97033ede9`): all three **BLOCKING**. Two of three reviewers independently
     compiled and RAN (jsdom-mounted against the pinned rc.3 vapor runtime) a `v-if` nested inside
     another `v-if` and found the inner conditional and its content silently absent from the
     generated module — the same "claimed general, actually only covers the tested shape" failure
     class recurring a second time on the flush-gate mechanism, this time silently dropping content
     instead of throwing. `PendingNavRequest` was still not fixed (confirmed unchanged from round 2).
     29 more ledger-note mismatches found on request of a full, mechanical, re-derive-from-`reasons`
     audit. A second stale tracking document (`docs/arch/future/vapor-parity-plan.md`) found, self-
     describing a DRAFT status, an `OUT-OF-SCOPE` waiver, and a seven-slice backlog, contradicting the
     session's own landed nested-v-for work.
   - **Fix round 3** closed all of round 3's findings: root-caused the `v-if`-in-`v-if` drop precisely
     (the `leave_element` early-flush gate keyed on "does this element have any `v_condition`" instead
     of "is this element the recorded parent of the chain about to be flushed"), fixed it, added
     genuine runtime-execution regression coverage (mount + assert rendered HTML, not string
     `contains()`) for the original shape plus 4 additional independently-constructed shapes,
     narrowed a FAST_REMOVE-flag test that had been silently matching the wrong call's flags; properly
     narrowed `PendingNavRequest` to `pub(in crate::template::code_gen::vapor)` with a permanent
     `trybuild` negative-control proof; rewrote all 29 ledger notes from their own `reasons` arrays and
     added a permanent structural consistency guard to `known_divergences_file_is_well_formed`
     (mutation-tested); deleted `vapor-parity-plan.md`; swept the remaining phase-archaeology
     instances.
4. **Track orchestrator's own direct verification** (post review-round cap, per this program's "at the
   cap, disposition explicitly rather than looping a fourth full review round" convention) found two
   further issues no review round's targeted test runs had surfaced, since none ran the exact
   canonical gate invocation:
   - The full `node scripts/gate.mjs` run failed: two `trybuild` compile-fail fixtures' pinned
     `.stderr` output didn't match the ACTUAL error under the gate's archived-binary execution mode
     (trybuild's own feature-detection walks a `.fingerprint/` directory that doesn't exist next to a
     nextest-archived binary, so the fixture hits a coarser, earlier module-privacy wall than under a
     live `cargo test --features bench` invocation) — a real, disclosed, environment-dependent
     divergence, not a regression; both `.stderr` pins corrected to the archived-gate's actual wall,
     documented in both wrapper tests. Three architecture guards (`no_std_fs_in_semantic_session_paths`,
     `no_std_fs_outside_native_fs_or_allow_list`, `vfs_boundary_is_authoritative`) failed because the
     new runtime-mount proof test used `std::fs::` directly; allowlisted with the same rationale
     pattern as its sibling `bf2_seed_matrix.rs` tool-output entries. While fixing this, a genuine
     pre-existing race (a PID-only temp filename colliding under concurrent test execution once six
     sibling tests shared it) was found and fixed with the same PID+counter pattern already used
     elsewhere.
   - The track orchestrator's own direct `packages/framework-conformance-harness` `npx vitest run`
     found 102 test failures across two local `AssembleInput` fixture-builder helpers that predated
     round 1's `emitSsrModuleRegistration` schema field and were never updated (a genuine gap none of
     the three review rounds' Rust-focused verification caught). Fixed; a full harness-wide sweep for
     the same pattern found no other instance.

## Architecture-scope and disposition consult rulings (verbatim)

**Round 4 — Vapor per-region template-splitting (ADOPT-NOW):**
> The capability is within BV0. Per-region Vapor skeleton splitting and anchor traversal are
> backend-local mechanisms required to correct explicitly owned "fragment and patch topology"
> defects. They introduce neither B3 canonical-request authority nor B4 publication architecture,
> and they do not change a ratified public contract. The prior architecture review already
> established that even a larger Vapor-private plan was not a prohibited universal IR.
>
> Scope guardrails: production changes remain under `template/code_gen/vapor/**`; extend the
> existing enter/leave emitter with private per-region state, no replacement two-pass backend; no new
> `pub(crate)` type consumed outside `vapor/`; do not modify VDOM/SSR/IDE/public APIs; implement
> generic semantics for the demonstrated defects, no speculative coverage expansion; acceptance
> requires the complete 36-cell gate, not merely new unit tests; proportionality is a review tripwire
> (hundreds of lines, not another ~2279-line wholesale backend replacement).

**Round 7 — Vapor two-phase id-allocation split (ADOPT-NOW):**
> A Vapor-private transform prepass over the already-parsed template AST is within BV0's owned
> "fragment and patch topology" correction scope. It is not a second file read, parse, semantic
> engine, public codegen path, or raw-source rescan. The Build Philosophy's no-rescan rule protects
> canonical file processing and cached semantic inventory; it does not prohibit backend-local
> multi-pass lowering over retained AST/IR.
>
> Guardrails: the first pass may only reserve transform-phase dynamic IDs in official rc.3 traversal
> order; the existing emit pass must consume those reservations afterward, no dual allocation model;
> reuse the parsed template AST, no source-text scanning or reparsing; keep state private to
> `vapor/**`; preserve one production Vapor path; discriminating tests for allocation order including
> nested/sibling controls.

**Round 12 — additive `CodeTransform` segmented-overwrite primitive (ADOPT-NOW, prefer the additive
primitive over migrating backend emission structure):**
> No new enabling block is presently required. BV0 explicitly owns missing authored anchors and other
> source-map correctness defects, residual template-emitter mapping defects exposed after BV0A, and
> shared lower-owner corrections serving multiple Vue backends. Its rescope trigger is narrower: B3/B4
> authority or a ratified public-product-contract change — not merely touching a shared internal
> primitive.
>
> Mandatory guardrails: one crate-private, opt-in primitive; no change to existing `overwrite`/
> chunk-walk/source-map behavior for existing callers; existing callers do not acquire the new
> behavior unless explicitly migrated; generated code byte-identical on every axis except intended
> `map1` segments; synthetic prefixes/scaffolding remain unmapped; permitted touch cone
> `template/code_gen/types.rs` + minimum `code_transform/**` + exact opt-in call sites under
> `template/code_gen/{vdom,vapor,ssr}/**`; `ide/**`, LSP/provider mapping, Svelte, BV0A composition,
> BF2 oracle, and B3/B4 must remain untouched; a static call-site guard restricting the operation to
> the authorized Vue runtime emitters; if existing `CodeTransform` semantics must change globally or
> IDE/Svelte callers must be modified, stop with `RESCOPE_REQUIRED`.

**Fix round 3 — `<template v-if>` wrapping a nested `v-if` (DEFER-to-BV1):**
> DEFER-to-BV1 is acceptable. BV0 need not fix this before landing. BV0's scope is explicitly bounded
> to the exact 36-cell matrix and controls for that domain; authoring an adjacent regression does not
> expand the charter. BV1 owns the complete Vapor/Vue pack and requires zero semantic
> known-divergences. The gap was not a previously-correct route regressed by BV0.
>
> Required debt row: disposition/owner `CODEX-DEFER`, durable owner `BV1`; acceptance `FC-VUE-001`,
> test `template_v_if_wrapping_inner_v_if_mounts_and_renders_inner_content`
> (`crates/verter_session/src/compile/map_equality_tests/nested_v_for_runtime_proof.rs`); resolution
> gate before BV1 acceptance and no later than plan close — remove `#[ignore]`, the genuine
> pinned-runtime mount must pass with exact HTML `<div><p>x</p></div>`; ruling reference this
> disposition consult against candidate `f99cd2e00`.

## Final gate evidence (landed candidate, `1c5e8e53f`, tree `7bb1066ea`)

- `node scripts/gate.mjs --build-jobs 4 --test-threads 4 --memory-limit 12GiB`: run independently
  twice by the track orchestrator (once on the pre-squash implementation tree at `57ddae400` before
  the final JS-fixture fix, which was PASS; once as the final confirming pass on the same tree at
  `a5cf30cd1` after the JS fix, which was also PASS). Both: Surface 1 (nextest, process isolation)
  24357/24357 passed, 581 skipped; Surface 2 (in-process `verter_session` libtests) 3/3 suites clean;
  Surface 3 (shipped `no-debug-assertions` cfg, `verter_session` + `verter_scheduler`) 8591/8591
  passed. Zero tolerated failures; freshness byte-pin ran genuinely (tooling present). The squashed
  landing commit's diff is byte-identical to the pre-squash tree's diff (verified via `git diff`
  comparison during the squash-merge), so this evidence carries over directly.
- `cargo clippy --workspace --all-targets -- -D warnings`: clean.
- `cargo clippy --target wasm32-unknown-unknown -p verter_wasm -- -D warnings`: clean.
- `cargo fmt --all --check`: clean.
- `cargo check --workspace --release`: clean (compiles the real release profile; `debug_assert!`
  behavior itself is covered by gate Surface 3, not this check).
- `packages/framework-conformance-harness` (`npx vitest run`): 616/616, independently re-run by the
  track orchestrator after fixing two stale local fixture-builder helpers.
- Targeted re-run: `cargo test -p verter_compiler --lib` 6041/6041; `cargo test -p verter_session
  --lib --features bf2-authoritative -- --test-threads=1` full-axis gate 36/36, mutation-discrimination
  control green, one disclosed `#[ignore]`d test (`FC-VUE-001`).
- Ancestry, squash-diff identity, and commit hygiene confirmed directly via `git` by the track
  orchestrator (not taken from the implementer's prose report). Squash-merge produced the exact
  expected 211-file, +10621/−2598 diff with zero conflicts.
- Commit message hygiene: clean (no plan/block/round vocabulary in the landed `fix(core)` commit).

## Disclosed, bounded residuals (not blocking)

- **`FC-VUE-001`** (see disposition ruling above): a `<template v-if>` wrapping a nested `v-if` is not
  yet a transparent root element in the Vapor backend. Deferred to BV1 with a named, currently-ignored
  acceptance test.
- **v-for destructuring-pattern renaming** (`{ id, name }`, `[a, b]`) is not implemented — only
  bare-identifier v-for renaming is; no BF2 seed fixture exercises the destructured shape. Same class
  as `FC-VUE-001`; not separately debt-rowed since it was not independently found by any review round
  as a must-fix (disclosed by the implementer, not escalated by any reviewer).
- Two `__vapor`-emission call sites (`process_script_only`, `emit_minimal_component`) and the TS
  `ssr && vapor` `defineVaporComponent` branch retain their pre-landing shape; confirmed not exercised
  by any of the 3 BF2 seed fixtures.
- A same-element combined `v-if`+`v-for` panic (`for_scope_depth` underflow) is confirmed
  pre-existing at the BV0A-accepted base (`3c9d72df9`), not introduced or worsened by this landing,
  and not exercised by any BF2 seed fixture.
- `packages/framework-conformance-harness/test/link-surface.spec.mjs`'s intermittent cross-spec-file
  scratch-dir collision (reproduces identically with and without this landing's changes; passes
  reliably in isolation) — pre-existing, unrelated, disclosed in round 1.
- `packages/framework-conformance-harness/spec/assembled-map-composition-layer1.md` carries 3 stale
  `compile.rs:N` code-quote citations that predate this landing (an earlier, unrelated commit
  relocated the cited code into sibling files) — pre-existing, disclosed in round 1, not touched.
- The following-sibling `<!>` anchor-position divergence in Vapor's static-template skeleton
  (`if_then_footer`-class shapes: the anchor lands after a following plain sibling instead of at the
  `v-if`'s own DFS position) is a real structural divergence from official's skeleton, confirmed
  non-crashing (`_child` still resolves the correct insertion point) — investigated, root-caused,
  disclosed in the relevant `merge_into_stack_index` doc comment, not fixed (a larger
  "reserve-the-anchor's-DFS-position-at-chain-creation-time" mechanism, out of this landing's
  critical-path scope; no BF2 seed fixture exercises it).
