# BF2 reopen #4 context packet

**Base:** `cf131b4ccf4d6b18c968b5b5c30ee3e74c816ca2` (post reopen #3, accepted).
**Candidate:** `4cd8500a8ec2cf17788b6297ae65aabf869430d7`, tree
`d751e6340d0c3c170da637dee87c79eed3eb26ff`.
**Charter:** `docs/arch/refactor/rev11/charters/BF2.md` (unchanged; owned scope already covers
"parser-backed cosmetic normalization and structural/topology comparison" and "diagnostics,
source-map, and TypeScript-observable product validation" — no charter amendment required for
either reopen trigger below).

## Reopen triggers

1. Two option-propagation defects found by the BV0-relanding scoping consult
   (`evidence/vue-known-defect-correction/reviews/round1-reopen-scoping-ruling.md` Q2/Q4): named
   ESM import-specifier order compared positionally when it has no semantic effect; a script-less
   SFC's `bindingMetadata` passed as a truthy empty object instead of `undefined`, producing a
   wrong render-arity golden.
2. A structurally unsound mapping-conformance oracle, found while drafting AMD-007: BF2's mapping
   axis compared the candidate's source map against the official compiler's own generated map.
   Binding maintainer direction (relayed mid-reopen, full investigation at
   `mapping-oracle-scoping-consult.md` in this directory) established this is unsound by
   construction — once generated JS is allowed to differ cosmetically between compilers (the
   project's own Compiled-Output Conformance rule), two compilers' source maps necessarily describe
   two different generated documents, so byte/segment comparison can reject correct output and
   accept wrong output. Replaced with a self-referential oracle validating a candidate's map
   against its own generated code and the authored SFC content.
3. A related maintainer clarification (`optimization-vs-conformance-investigation.md` in this
   directory) confirmed the general concern only partially — narrow, low-risk over-constraints
   (import-specifier order, an already-fixed-elsewhere unused-import case) were real; no evidence
   Verter's core codegen architecture has been forced worse. Disposed: narrow items ADOPT-NOW
   within this reopen; the general comparator-redesign idea DEFERRED (`FC-VUE-002`, CLAUDE.md
   CRITICAL-rule-level, requires separate maintainer ratification).

## Scope discipline

No production compiler code (`crates/verter_compiler`, `crates/verter_session`) touched. No Svelte
file touched. No file under `crates/verter_vue_conformance/corpus/` touched (only
`packages/framework-conformance-harness/` and its golden records, plus the normalizer contract doc
and this evidence tree). Diff verified by the program orchestrator: `git diff --stat
219579368..4cd8500a8` — 206 files, entirely within the above boundaries.

## Review process

Four independent review rounds (conformance/architecture/adversarial). Full history, findings,
and the final round-4 fixes (independently re-verified by the program orchestrator against
committed source, no fifth agent round dispatched per the review-round cap) are in
`landing-record.md` in this directory.

## Verification (program orchestrator, independent)

- `pnpm test` in `packages/framework-conformance-harness`: 411/411.
- `cargo test -p verter_vue_conformance --test main` (`CARGO_BUILD_JOBS=3 --test-threads=3`): 8/8.
- Commit message hygiene: clean (no plan vocabulary, no AI attribution).
- Tree/SHA identity: confirmed via `git rev-parse` directly, not taken from the implementer's
  prose report (which contained a transcription error in its final hex digits).
