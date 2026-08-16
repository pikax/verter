# BV0A landing record

Base: `f15fa5376` (`program/architecture-lock` tip immediately before this landing). Candidate
lands as one squashed commit, `7b017b955`.

## Arc

1. **AMD-007** (RATIFIED, `acabec8fa`) first chartered BV0A: an interim Vue-only assembled-module
   source map, validated by matching oracle-violation attribution between an assembled run and
   per-fragment standalone runs.
2. **AMD-008** (RATIFIED, ratification-bundle `a1f6523ce`) redefined BV0A's acceptance boundary
   after twelve review rounds found the attribution mechanism structurally insufficient (round 3)
   and prose alone unable to settle the composition algebra at implementation-grade precision
   (rounds 4–11, each closing named edge cases and finding the next layer). Replacement: exact
   ORDERED equality of the complete decoded map artifact against an independent, input-only
   JavaScript reference, governed by a two-layer specification — a frozen semantic layer (layer 1)
   and a literal vector coverage set (layer 2) — rather than oracle-violation matching. Round 11
   deferred one residual finding (layer-1 gate authority — completeness, non-retroactive
   chronology, maintainer adoption) as tracked debt `FC-VUE-003`, owned by BV0A's own acceptance
   review; round 12 (governance, targeted) closed two final wording findings on that debt record.
3. **Layer-1 freeze**: `packages/framework-conformance-harness/spec/assembled-map-composition-layer1.md`
   — the pre-assembly DTO schema, the exhaustive `UncomposableInputMap` validation order/taxonomy,
   the chaining/transform algebra for both authorized rewrites, and the assembler write/boundary
   manifest. Seven review rounds, twenty-one independent blind dispatches, findings narrowing
   13 → 8 → 7 → 5 → 3 → 2 → 0. Adopted at commit `6317cadd5`, blob
   `0ea47424acfbd4913e11f16156baa597216c84fb`
   ([`layer1-freeze-adoption.md`](layer1-freeze-adoption.md)). A narrow post-freeze addendum
   (`DECISION` D-8, the `U8.1` fragment-attribution rule) received its own independent review and
   adoption at commit `a52f3021c`, blob `085139c5267136ed0c2fa39d78ad48168c6e0e76`
   ([`layer1-d8-adoption.md`](layer1-d8-adoption.md)).
4. **Independent JavaScript reference** (`packages/framework-conformance-harness/src/assembled-map-*.mjs`):
   written from frozen layer 1 alone, with no dependency on Rust composition/rewrite/map-emission
   code — the property that closes common-mode error between the two implementations.
5. **Production Rust fix** (`crates/verter_session/src/compile/{map_input,map_compose,map_json,map_tests}.rs`,
   `crates/verter_compiler/src/code_transform/{source_map.rs,chain.rs}`): the genuine production
   assembler now returns a typed code-plus-map result, composing script and template output maps
   through the two authorized `CodeTransform` rewrites, reproducing `Chunk::Overwritten` token
   geometry and equal-coordinate wire order.
6. **Layer-2 vector suite**: 75 hand-derived vectors (23 positive, 52 fail-closed) covering rewrite
   geometry, boundary segments (`BR-3`), table composition, sourceless boundaries, astral/CRLF text,
   and the full fail-closed taxonomy. Two dedicated standalone review rounds (conformance,
   architecture, adversarial) beyond the whole-package spot-checks — round 1 found real
   artifact-completeness gaps (missing derivations, an overstated `knownGaps: []`), closed by a fix
   pass adding five new vectors (V19–V23); round 2 (conformance recheck) found and fixed one
   remaining derivation-prose defect in V23. `layer2-readiness-record.md` records content
   completeness and that both implementations reproduce the complete inventory.
7. **Historical code-byte baseline**: `crates/verter_session/src/compile/map_equality_tests/bf2_seed_matrix_code_baseline.json`
   repinned from a genuinely historical pre-BV0A assembler run (not a self-referential candidate
   hash), per `historical-baseline-provenance.md`.
8. **`FC-VUE-003` resolution**: all three resolution-gate checks (layer-1 completeness,
   non-retroactive chronology, maintainer adoption) independently re-verified MET across three
   review rounds; recorded in `debt-layer1-gate-authority.md`.

## This session

Picked up after a prior track-orchestrator session died mid-task, having landed a real fix
(`6ba1b2778`: an ignore-list upper-bound check misplaced at validation step 1.15 instead of the
table-bounds step 1.23/`U6.3`) plus doc corrections, but never running the promised re-review.

1. **Round 4** (Codex xhigh, read-only, fresh worktree at `6ba1b2778`): confirmed all six round-3
   findings genuinely closed. Found two new blocking defects:
   - `read_ignore_list`'s cross-spelling agreement check compared entries after narrowing to `u64`,
     which saturates for binary64 values beyond `u64::MAX` — two genuinely different values (e.g.
     `2^64` and `2^65`) collided and were wrongly reported as agreeing, diverging from the JS
     reference.
   - The vector-suite runners' "count asserted against inventory" claim was not actually enforced:
     the JS spec hand-enumerated named test blocks with no coverage assertion; the Rust
     `vector_inventory.rs` compared against hardcoded literals (`23`/`52`/`75`).
2. **Fix cycle**: both defects fixed test-first (`8ff8ae7fd`, `caae109bc`). Ignore-list entries now
   carry full `f64` identity through the type check, agreement check, and table-bounds check,
   narrowing to `u32` only once the bound is proven. Both vector-suite runners now derive the
   expected inventory from the loaded suite arrays and assert exact executed-id parity. Both fixes
   proved RED-then-GREEN against genuine mutation plants (JS: a synthetic unexercised vector; Rust:
   a driver mutation silently skipping one vector), reverted and reconfirmed clean.
3. **Round 5** (Codex xhigh, read-only): confirmed both fixes genuinely correct against layer 1 and
   the JS reference. Found one non-executable doc-comment inconsistency (`vector_inventory.rs`
   still stated a hardcoded "75 total" next to its own "no count hardcoded" claim). Fixed directly
   (`09e0b1f76`).
4. **Full canonical gate** (`node scripts/gate.mjs`) on `work/bv0a-integration`'s tip: SURFACE 1
   failed — 22 evidence files under `docs/arch/refactor/rev11/` carried absolute macOS
   home-directory path markers. Root cause: the branch had forked from `program/architecture-lock` before
   an unrelated commit (`f15fa5376`, landed by a separate session) sanitized those exact files on
   the mainline. Confirmed via `git merge-base --is-ancestor` and direct diff inspection — not a
   BV0A defect.
5. **Reconciliation**: built a fresh landing branch from the current `program/architecture-lock`
   tip and `git merge --squash work/bv0a-integration` — merged with zero conflicts (git correctly
   recognized the branch never independently touched those files beyond what the sanitization
   commit already fixed). Committed as one clean, plan-vocabulary-free commit.

## Final gate evidence (reconciled candidate, `7b017b955`)

- `node scripts/gate.mjs` (`--build-jobs 4 --test-threads 4 --memory-limit 12GiB`): PASS, all three
  surfaces green — Surface 1 (nextest, process isolation): 24262/24262 passed, 581 skipped.
  Surface 2 (in-process `verter_session` libtests): 3/3 suites clean, 0 tolerated failures.
  Surface 3 (shipped `no-debug-assertions` cfg, `verter_session` + `verter_scheduler`): 8587/8587
  passed. Zero tolerated failures anywhere; freshness byte-pin ran genuinely (tooling present).
- `cargo clippy --workspace --all-targets -- -D warnings`: clean.
- `cargo fmt --all --check`: clean (exit 0).
- `cargo test -p verter_session --lib compile::`: 73/73 (independently re-run by the program
  orchestrator).
- `pnpm test`: fails in any FRESH worktree on `typeinfo` (3 tests) and
  `framework-conformance-harness` (`closure-drift.spec.mjs`, `mapping-oracle-composition.spec.mjs`)
  — proven pre-existing and unrelated to this candidate: a control worktree built from unmodified
  `program/architecture-lock` with the identical fresh-build sequence reproduces the same failure
  set byte-for-byte; the long-lived, pre-built main checkout passes both suites fully clean
  (typeinfo 31/31, framework-conformance-harness 411/411). Root cause is environment/network/store-
  cache-dependent one-time setup state (e.g. `closure-drift.spec.mjs`'s real, scripts-disabled,
  network-denied install-tree test), not this candidate's code.

## Disclosed, bounded residuals (not blocking)

- BV0A's own scope explicitly excludes BF2's non-clean MAPPING verdict from its gate — residual
  fragment-emitter mapping defects remain BV0's acceptance responsibility, per AMD-008 §3's
  reinforcement of BV0's literal full-oracle-clean exit.
- The layer-2 vector suite's `knownGaps` discloses three residual write-manifest sites (W-05 SSR
  template imports, W-13 the `ssrRender` attachment distinct from its already-reached sibling
  W-13′, W-16′ the webpack HMR block distinct from its already-reached sibling W-16) unreached by
  any vector — judged an acceptable narrow, disclosed residual by the dedicated layer-2 review
  round, not a false completeness claim.
- `pnpm test`'s fresh-worktree-only failures above are an environment-setup gap in how a throwaway
  worktree is prepared for the JS test suite, not a BV0A code defect; not remediated in this
  landing because remediation is orthogonal to this charter's scope.
