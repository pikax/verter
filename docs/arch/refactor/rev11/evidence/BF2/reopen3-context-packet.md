# BF2 reopen #3 — pre-dispatch context packet

Authored BEFORE any implementation change in this pass, as the first commit on
`work/bf2-reopen3-fix`. This is the pre-dispatch record of scope and intent for
this reopen — not a post-hoc summary.

## Why this reopen exists

BV0's round-1 adversarial review (`docs/arch/refactor/rev11/evidence/vue-known-defect-correction/reviews/round1-adversarial.md`,
Finding 1) found that `packages/framework-conformance-harness/src/invoke-vue-oracle.mjs`
never passes `vapor` or `templateOptions.ssr` to the official `compileScript` call, so
every harness-generated Vapor golden's script half was compiled by the real official
compiler as **non-Vapor, non-SSR** output. BV0 then changed Verter's own production code
to match those defective goldens — deleting the load-bearing `__vapor: true` marker from
real non-SSR Vapor builds (this gates VDOM↔Vapor runtime interop) — and added a new unit
test that locks the wrong behavior in. A Codex xhigh scoping ruling
(`docs/arch/refactor/rev11/evidence/vue-known-defect-correction/reviews/round1-reopen-scoping-ruling.md`)
holds this is BF2-owned (BF2 owns "offline official compiler invocation and immutable
golden generation"), requiring a formal BF2 reopen with a fresh 3-mandate review before
BV0 may be rebased and re-evaluated. This is BF2 reopen #3 (see ledger notes for reopens
#1 and #2, and `evidence/BF2/second-reopen-ruling.md` for the reopen #2 precedent this
pass follows procedurally).

## Scope of this pass (ratified by the reopen-scoping ruling)

This pass repairs the harness's oracle-invocation defect and regenerates every official
golden it corrupted. It does **not** touch `crates/verter_compiler/src` (compiler
production code) and does **not** touch any Svelte file — those are BV0's and a separate
Svelte block's territory respectively.

### Item A — carry the complete semantic profile into every official phase

`compileVueFixture` in `invoke-vue-oracle.mjs` currently derives `ssr`/`vapor` booleans
from `options.backend` and passes them to `compileTemplate` only. Official
`@vue/compiler-sfc` (`compiler-sfc.cjs.js`, pinned rc.3, realized at
`/tmp/bv0-oracle-installs/vue` from this session — reprovision if stale/absent via
`BF2_ORACLE_NPM_CACHE=/tmp/bv0-oracle-npm-cache node scripts/provision-oracle-npm-cache.mjs`
then realize the install through the harness's own `ensureOracleDomain`) derives them at
`compileScript` as `vapor = sfc.vapor || options.vapor` and
`ssr = options.templateOptions?.ssr` (verified directly against the pinned dist in the
round-1 adversarial review, `compiler-sfc.cjs.js:15385-15386`). Fix:

- Pass `vapor` (boolean) and `templateOptions: { ssr }` into the `compileScript(...)`
  call alongside the existing `id`/`inlineTemplate`/`sourceMap` options.
- Verify parse, `compileScript`, `compileTemplate`, assembly, golden provenance, and the
  runtime harness all consume the SAME requested `{ backend, sourceMap, isProd }` axes —
  audit for any other call site in the harness (`bin/generate-goldens.mjs`,
  `bin/check-candidate.mjs`, `src/execute-vue-vapor.mjs` / `execute-vapor-runtime.mjs` /
  runtime executors, hydration, TS observation) that independently derives `vapor`/`ssr`
  and could silently diverge from what was actually requested.
- Do not merely add `vapor: true` at one call site — this is a class fix across the
  whole option-propagation chain, not a single-line patch.

### Item B — regenerate every affected official golden

Because `templateOptions.ssr` was ALSO never passed to `compileScript` (a second omission,
distinct from the `vapor` omission), scope is not limited to the Vapor backend: any
SSR/non-inline script-bearing golden whose `compileScript` output depends on `ssr` being
visible at script-compile time is also potentially affected (official routes
`ssr`-visibility to script-side binding metadata differently once `templateOptions.ssr`
is set — verify empirically per-fixture rather than assuming no change). Regenerate the
full golden corpus (`bin/generate-goldens.mjs`) and diff every changed file for a
plausible root cause (script-half compiled with the correct backend/ssr flag now visible)
before accepting the diff — do not blindly accept "changed" as "correct" without spot
inspection against the pinned dist source.

### Item C — independent JS and TS controls

Add controls, independent of the existing seed-matrix consumer suites, that directly
assert the FIXED harness behavior at the harness level (not the compiler-consumer level):

- A JS `<script setup>` fixture compiled with `{ backend: "vapor", isProd: false }`
  produces a script half carrying `__vapor: true` (official's unconditional JS-branch
  behavior — `compiler-sfc.cjs.js:15736`, `if (vapor) runtimeOptions += '\n  __vapor:
  true,'`).
- A TS `<script setup lang="ts">` fixture compiled the same way produces
  `defineVaporComponent(...)` wrapping (official's TS-branch behavior,
  `compiler-sfc.cjs.js:15731`, `vapor && !ssr`).
- A VDOM-backend compile of the same fixtures carries neither marker (negative control).
- An SSR-backend compile's script half reflects `templateOptions.ssr` visibility
  correctly (whatever the verified correct official behavior is — confirm against the
  pinned dist, do not assume).
- Runtime interop: mount a component compiled with the FIXED harness path through
  `mountVueVapor`/the pinned runtime and assert `isVaporComponent`/interop gating
  actually observes the marker (a behavioral check, not just a structural string check on
  emitted source) — this is the check that would have caught the original defect, since a
  structural comparison of candidate-vs-defective-golden could not.

These controls are HARNESS-level (they test `invoke-vue-oracle.mjs` output directly, not
Verter's compiler), live under `packages/framework-conformance-harness/test/`, and must
be written so they FAIL against the pre-fix harness and PASS against the fixed harness
(verify this by temporarily reverting the fix and confirming red, per Stub Prevention).

### Item D — source-map acceptance axis restored

The round-1 architecture review (Finding 4) found `compare.mjs`'s mapping-comparison axis
was silently narrowed from candidate-vs-official structural comparison to
candidate-self-consistency only, justified by an unresolved harness artifact
(`reAnchorMapLines` blank-line padding in the golden generator). BV0's charter (item 3 of
owned scope) expects "source-map differences after harness artifacts are removed" — i.e.
strip the harness artifact, then resume the real candidate-vs-official comparison. Default
ruling per the reopen-scoping consult (Q4.1): identify and normalize out the
`reAnchorMapLines` artifact, then restore genuine candidate-vs-official mapping
comparison (decoded VLQ segments, source/line/column equivalence up to the normalized
artifact) as the acceptance axis, replacing (or supplementing, if both add signal) the
current self-consistency-only check. If full byte/segment equivalence turns out to be
unsound for a DIFFERENT structural reason discovered during this work, do not silently
re-narrow again — STOP and report the specific new reason for a ruling, do not decide it
unilaterally.

### Item E — reusable authoritative/fail-closed harness mode (enables BV0's fail-closed obligation)

BV0 owns making ITS OWN seed-matrix acceptance check (`official_seed_matrix.rs`) fail
closed when `link`/`runtime` axes report `skipped` — that specific Rust-side change is
BV0's, not this pass's. But the reopen-scoping ruling (Q3, item 5) notes that "changing
generic `check-candidate.mjs` skip semantics is BF2-owned" and invites BF2 to add a
reusable authoritative/fail-closed harness mode if desired. Add one: an explicit flag or
env var that `check-candidate.mjs` / the harness respects to turn a `skipped` axis into a
hard non-zero exit / thrown error instead of an informational skip, so a consumer (BV0's
Rust suite, or CI) can opt into "prove every axis genuinely ran" without the harness
changing its default (skip-with-reason) behavior for ordinary local development. Keep the
default behavior unchanged; this is additive.

### Item F — rerun locked BF2 performance/provenance gates

Changing `compileScript`'s invocation options and regenerating a large golden domain
invalidates the corresponding generation measurements/digests recorded for BF2's locked
performance cells in `performance-gates.toml`. Rerun the affected measurement/provenance
tooling and reattest — thresholds themselves stay LOCKED (this is re-attestation of the
same thresholds against new invocation behavior, never recalibration of the threshold
values). If a rerun genuinely cannot pass the existing locked threshold, STOP and report —
do not loosen the threshold to make it pass.

## Non-goals / explicit exclusions for this pass

- No file under `crates/verter_compiler/src` is touched (that is BV0's territory).
- No Svelte file (corpus, production `src/svelte/runtime`, `package.json` pins) is
  touched — the Svelte oracle migration is explicitly a separate, not-yet-authorized
  block per the reopen-scoping ruling (Q2). Do not bundle it in here either.
- `official_seed_matrix.rs` and any other `.rs` file under `crates/verter_vue_conformance`
  is BV0's to change for its own fail-closed acceptance behavior — this pass only adds
  the reusable harness-level primitive (Item E), it does not wire BV0's Rust consumer to
  use it.
- No new known-divergence waiver row, tracker, or retraction artifact of any kind.
- Existing `performance-gates.toml` threshold VALUES are not changed, only re-attested.

## Verification plan

- `pnpm --filter @verter/framework-conformance-harness test` — full harness self-test
  suite, including the new Item C controls, run WITH the oracle genuinely provisioned
  (`BF2_ORACLE_NPM_CACHE=/tmp/bv0-oracle-npm-cache BF2_ORACLE_INSTALLS=/tmp/bv0-oracle-installs`,
  reprovisioning first if those paths are stale/absent) so link/runtime-dependent
  self-tests genuinely execute, not skip.
- Regenerated golden diff reviewed for plausibility (spot-check several changed files
  against the pinned dist behavior, not just "count changed").
- Performance/provenance gate rerun evidence attached to the final report.
- `git status` clean in the worktree before commit, all changes scoped to
  `packages/framework-conformance-harness/` (source, tests, scripts), regenerated golden
  corpora under Vue-only paths, and this evidence document plus
  `packages/framework-conformance-harness/README.md` if wording needs correction.
