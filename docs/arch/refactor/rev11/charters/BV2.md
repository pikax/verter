# BV2 — Vue post-BV1 runtime-codegen invariant and regression correction

**Status:** DRAFT — authored for maintainer review; no AMD ratifies it yet. **Class:** subsystem
(`program-dag.toml`). **Predecessors:** `BV1`, `BS1` (both `ACCEPTED`/`IN_PROGRESS` per the ledger at
authoring time — see `docs/arch/architecture-lock/ledger/program-state.toml`).

## Context

A beta.4 regression intake (maintainer-ratified) found a production VDOM template-codegen panic —
`overwrite_segmented precondition violated at [0,N): ReplacedContentSplit { offset: 0 }` at
`template/code_gen/types.rs:712` — on a valid, previously-succeeding Vue SFC shape, plus a second,
previously unknown sibling defect in the SSR backend discovered during the same root-cause and
architecture-ruling pass. Both are regressions against published beta.3, both are release blockers, and
both are placed by a ratified architecture ruling in one bounded post-BV1 correction block: **BV2**.
This charter turns that ruling into an executable charter. It does not reopen the ruling's REPAIR or
PLACEMENT decisions — see "Rulings applied" below.

## Established root cause

Two independent producers write overlapping subranges of the VDOM template header when a leading
comment precedes a single, statically-classed root element under `comments: false` (i.e. production
build, comments disabled):

1. `visit_comment` (`template/code_gen/vdom/mod.rs:1741`), walker-ordered before `leave_template`
   (walker order: `template/code_gen/walker.rs:98,106`), calls `process_comment`
   (`template/code_gen/vdom/comment.rs:22-28`), which unconditionally emits a plain
   `overwrite(comment.start, comment.end, "")` into the `overwrites` channel when comments are disabled.
2. `leave_template` (`template/code_gen/vdom/mod.rs:822`), needing to carry hoisted-static-class anchors
   for its single-root block-root path, emits `overwrite_or_root_prefix_segmented(tag_open.start,
   child.start, ...)` (`vdom/mod.rs:1072-1077`) into the `segmented_overwrites` channel — a range that
   structurally contains producer 1's range, because disabled comments are dropped from child
   bookkeeping (`vdom/mod.rs:709`) without their span being excluded from the claimed prefix.

`CodeGenOutput::apply_to` flushes `overwrites` before `segmented_overwrites` unconditionally
(`types.rs:690-706`, explicitly documented as requiring the two channels to be disjoint). By the time the
segmented overwrite runs, its target is no longer one untouched `Original` chunk, and
`try_overwrite_segmented`'s strict single-`Original`-chunk precondition (`code_transform/segmented.rs:102`
— correctly protecting against a silently wrong source map) fires and panics. Both conditions (a static
class taking the segmented path; a leading comment already splitting the chunk) are individually
necessary and jointly sufficient. A reproducing test and full instrumented trace exist; a repeat pass
during architecture ruling additionally found the SSR sibling below. Neither `Vapor` nor
`code_transform`/`CodeGenOutput::apply_to` itself is implicated (see Rulings applied).

## Owned scope

BV2 owns, and is the sole owner of:

1. **The VDOM root-prefix duplicate-ownership repair.** Eliminate the two-producers-one-range conflict
   locally, in the VDOM backend, by giving the final VDOM root-prefix owner (`leave_template`'s
   root-prefix producer) sole ownership of any disabled-comment removal wholly contained by the range it
   is about to atomically claim: `visit_comment` records a pending removal intent instead of emitting an
   overwrite when comments are disabled; `leave_template`, after it has determined the final effective
   root and exact prefix range, absorbs every pending removal wholly contained by that range (the
   segmented prefix's synthetic content already elides those bytes) and emits an ordinary deletion only
   for a disabled comment left unclaimed (interior/trailing, outside any root-prefix replacement). This
   must not be conditional on the static-class/segmented branch specifically — the root-prefix owner
   subsumes contained comments under its ordinary unmapped prefix replacements too, restoring
   cross-channel disjointness unconditionally. `process_comment`'s disabled-comment branch becomes
   preservation-only for the owner's own claimed range.
2. **The SSR comment-only sibling collision.** SSR does not reproduce the reported plain-overwrite/
   segmented-prefix collision for a nonempty root, but shares the same defect *shape*: comments do not
   count toward `effective_count` (`template/code_gen/ssr/mod.rs:473`), a disabled comment enqueues its
   own segmented deletion (`ssr/mod.rs` comment handling, near `:6523`), and the zero-effective-root
   branch enqueues a whole-template segmented replacement (`ssr/mod.rs:4289`). BV2 fixes this
   backend-locally, in the SSR backend's own comment/root-counting code, using the same duplicate-ownership
   elimination principle as the VDOM repair — **not** by reordering `CodeGenOutput::apply_to` or touching
   `code_transform/` (see Forbidden outcomes).
3. Production VDOM (and SSR) behavior for every failing source shape in the acceptance matrix below.
4. Source-map and provenance preservation across the repair: the strict `try_overwrite_segmented`
   single-chunk precondition and the existing `overwrites`-then-`segmented_overwrites` flush order in
   `CodeGenOutput::apply_to` are retained exactly as they stand — the repair changes *what* the VDOM/SSR
   backends enqueue, never the transform/flush contract itself.
5. Focused regression tests (VDOM- and SSR-local) plus, where the repair's chunk-sequence shape is
   sufficiently well characterized, a `code_transform::segmented` unit-level replay of the captured chunk
   state, alongside the existing `segmented_tests.rs:291`-class rejection test, which must remain green
   unmodified in substance.
6. Relevant Vue `3.6.0-rc.3` official-core conformance reruns (the locked oracle BV1 established) for
   every affected cell, plus BV1's and BV0's existing comment/class/hoisting suites, which must stay green.
7. Direct/native invocation proof: the repair is proven through the same direct-route (`StandaloneCompiler`
   one-shot) and native `compileMany` invocation paths the regression was reported through, not only
   through a synthetic unit-level harness.
8. A benchmark confirmation run (`pikax/vue-benchmarks`, the same suite the regression intake used) as
   **secondary** evidence only: it demonstrates the production VDOM cells that previously panicked now
   complete, but it is never the acceptance oracle — the locked Vue RC.3 conformance pack and the matrix
   below are.

BV2 does **not** own: restoring the removed whole-block overwrite fallback; any `code_transform/` or
`CodeGenOutput::apply_to` change (see Rulings applied §2); Vapor (audited, unaffected — Vapor omits
disabled comments from its own private assembly and emits one whole-template segmented replacement with
no independent comment overwrite, `vapor/comment.rs:14`, `vapor/mod.rs:3352`); Svelte (segmented-overwrite
authority already excludes Svelte, `types.rs:18`; no shared-transform change is made, so no dedicated
Svelte regression suite beyond normal gates is required); unrelated TypeScript or component-meta repairs;
B5's or B6's scope (both explicitly forbid Vue framework semantic repair — `B5.md:5`); or any new public
option, product, or waiver/known-divergence/tracker artifact.

## Forbidden outcomes

Per the maintainer's hard constraints, none of the following is an acceptable BV2 result, regardless of
whether it makes the panic stop:

- Restoring the removed whole-block overwrite fallback (it reconstructs whole-block provenance and can
  emit silently incorrect source mappings).
- Catching/swallowing the panic, or returning partially valid/partial output.
- Converting the invariant violation into an apparently successful compile (e.g. silently coercing the
  segmented overwrite to a lossy plain overwrite).
- Disabling comment removal or static-class hoisting globally, or for any class of input broader than the
  exact duplicate-ownership conflict.
- Any benchmark-marker, fixture-name, or known-failure special case or allowlist.
- Any lowering of source-map correctness.
- Landed enforcement of the fixed invariant that is a name-keyed source scanner rather than structural
  (type-state, sealed traits, visibility/`E0603`, or a closed enum matched exhaustively).

## Acceptance matrix (regression-intake §5.4, reproduced in full)

BV2's required exit is proven across the full cross of these axes, not the single reported cell:

| Axis | Values |
|---|---|
| First template child | comment / none |
| Comment position | first / later (interior or trailing) |
| Root class | static / none / dynamic / static+dynamic |
| Build mode | production / development |
| Backend | VDOM / Vapor / SSR |
| Source maps | off / on |
| Invocation | direct route (`StandaloneCompiler`) / native `compileMany` |
| Style | none / style block |
| Comment shape | short / long / whitespace-only variants |
| Script | Options API / `<script setup>` |
| Template body | interpolation / static text / directive-free minimum |

For every cell in this matrix: no panic; no partial publication; the generated JavaScript parses; the
module links against the intended Vue runtime contract; the intended runtime behavior is produced;
source-map mappings remain accurate; the existing comment/class/hoisting suites (BV0/BV1) stay green;
the locked Vue `3.6.0-rc.3` conformance pack stays green; and the external benchmark's production VDOM
cells complete (secondary evidence). Vue RC.4 output is **not** the normative oracle for any cell — the
locked RC.3 pack is.

## Required exits

`FC-VUE-001`, applicable BV1-owned comment/class/hoisting/mapping cells, and the acceptance matrix above
all pass with no blocked case and no new known-divergence. Specifically:

- The VDOM root-prefix duplicate-ownership conflict (§Owned scope 1) and the SSR comment-only sibling
  (§Owned scope 2) are both closed, backend-locally, with the strict `try_overwrite_segmented`
  single-`Original`-chunk precondition and the `overwrites`-then-`segmented_overwrites` flush order in
  `CodeGenOutput::apply_to` unchanged.
- Every cell of the acceptance matrix produces the required outcomes above.
- Locked Vue `3.6.0-rc.3` official-core conformance stays green; every removed BF3/BV0/BV1 guard for this
  exact defect class stays removed (no reintroduced refusal or tracked divergence).
- `pikax/vue-benchmarks`'s production VDOM cells that previously panicked complete cleanly (secondary
  evidence, recorded but not gating).
- Vapor and Svelte suites are unaffected and stay green under normal gates — no dedicated new Svelte
  regression suite is required, because no shared `code_transform`/`CodeGenOutput::apply_to` change is
  made.

## Structural confinement

Every invariant BV2 introduces is enforced structurally — type-state, sealed traits, visibility/`E0603`,
or a closed enum matched exhaustively — never a name-keyed source scanner. Concretely: "disabled comment
inside the claimed root-prefix range" is a fact the root-prefix owner computes and consumes at the same
call site that already computes the claimed range (child-record/prefix-range geometry it already builds),
not a second pass over source text or identifiers; `process_comment`'s preservation-only disabled-comment
path is a removed code branch (a compile-time absence), not a runtime flag checked by a scanner.

Be precise about what counts as structural. An "exhaustive test-double implementation" is not by itself a
structural proof: trait default methods can be omitted from a double and it still compiles, a bare-`T`
return still type-checks, and a type alias is type-identical to its target — an implementation that merely
compiles against a trait proves the trait was implemented, not that the removed duplicate-ownership branch
cannot reappear. The structural proof here is narrower and load-bearing: the disabled-comment mutation
branch is deleted from `process_comment` (not merely bypassed by a flag), so no code path exists that can
re-emit the duplicate overwrite; a reviewer verifies its absence by reading the function body, not by
running a scanner or a double.

## Review

Three independent mandates on one candidate SHA and tree (`governance.md` §1): conformance (charter, diff,
the acceptance matrix, and whether the SSR sibling correction is genuinely backend-local, not a global
reorder); architecture (whether `code_transform/`/`CodeGenOutput::apply_to` were left untouched as
required, and whether the deleted disabled-comment branch is a genuine structural closure rather than a
conditional bypass); adversarial/performance (mutation-tested regression coverage for both the VDOM and
SSR fixes, and confirmation the repair introduces no new allocation or pass on the existing hot codegen
path).

## Abort/rescope

Stop for: a discovered third backend (beyond VDOM/SSR) sharing the same duplicate-ownership shape that
this charter's owned scope does not name; evidence that the SSR sibling cannot be closed backend-locally
without touching `code_transform/`/`CodeGenOutput::apply_to` (that would reopen the architecture ruling,
not license a local substitution); a regression discovered in the locked Vue RC.3 conformance pack that
BV2's owned scope cannot explain and correct; or pressure to widen the fix beyond the exact duplicate
region conflict (e.g. globally disabling comment removal or static-class hoisting). A discovery at this
bar reopens the ruling itself, not a quiet local substitution for its REPAIR or PLACEMENT decision.

## Rulings applied

Binding ruling: the ratified BV2 repair-and-placement architecture ruling (Codex xhigh, 2026-08-20),
following the maintainer's beta.4 regression-intake directive and a bounded, read-only root-cause
investigation. Do not relitigate here; preserve the ruling's and root-cause investigation's evidentiary
records into `docs/arch/refactor/rev11/evidence/BV2/` at implementation time (the pattern established by
prior debt rows referencing scratch-only transcripts).

1. **Repair class — decided.** Eliminate duplicate region ownership locally (defer disabled-comment
   removal, let the final VDOM root-prefix owner absorb contained removals); reordering, narrowing,
   insertion-conversion, and original-chunk-carrying were all evaluated and rejected as inferior — they
   encode the conflict instead of removing it.
2. **`CodeGenOutput::apply_to`'s flush order — not the defect.** The channel model and flush order stay
   unchanged; the VDOM/SSR *producers* violated the documented disjointness contract, not the transform
   layer. No `code_transform/` change is authorized by this charter.
3. **Blast radius — audited.** Vapor is unaffected (no independent comment overwrite exists in its path).
   SSR has a distinct sibling collision, assigned to BV2 in this same charter, fixed backend-locally.
   Svelte needs no new regression suite because no shared transform changed.
4. **Placement — decided.** BV2 is the sole owner, predecessors `BV1` and `BS1`, with `B5`'s DAG
   predecessor changed from `{BV1, BS1}` to `BV2` (`program-dag.toml`). BV1's accepted record is
   historical and is not rewritten. No accepted ADR changes; no final program outcome changes.
