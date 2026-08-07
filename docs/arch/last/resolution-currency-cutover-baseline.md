# Resolution-currency cutover — frozen Block 0A baseline

Companion to [`resolution-currency-cutover-plan.md`](resolution-currency-cutover-plan.md)
(architect-v3 Part C, Block 0A) and
[`resolution-currency-cutover-errata.md`](resolution-currency-cutover-errata.md), both
committed alongside this record. This record freezes the production-harness baseline
that Block 6 compares against (PERF-2 `R_query`, CORPUS-1, DET-1).

## Identity

| Item | Value |
|---|---|
| Baseline commit | `2de3b2d076d72ea84932e23f8d801906429c6646` (clean tree; measurement run from a worktree at this exact commit) |
| Release binary | `packages/native/dist/verter-native.darwin-arm64.node`, sha256 `7de1524725c1333783ca50c36a07e810fba0c43e7f947d1d21fa79604ef56c3f` (built by `pnpm --filter @verter/native build` = `napi build --release`) |
| Corpus | nuxt-ui @ `0a1803d3361dab9a0ebe1bc2097e8bfa283f0c3c` **with a DIRTY working tree** — see "Corpus state" below. 180 discovered `.vue` components under `src/runtime/components` (that subtree IS clean against `0a1803d3`); generated `.nuxt/` tsconfigs present (`tsconfig.app.json`, `tsconfig.shared.json` — required by the worker's `configureWorkspaceProjects`) |
| Machine | Apple M3, 8 cores, 24 GiB, macOS 26.6 (arm64), Darwin 25.6.0 |
| Toolchain | rustc 1.97.1 (8bab26f4f 2026-07-14), cargo 1.97.1, Node v20.20.2, pnpm 10.22.0 |

### Corpus state (Block 6 must reproduce THIS state, not a clean `0a1803d3`)

`git status --short` in the corpus checkout at measurement time:

```
 M docs/nuxt.config.ts
 M package.json
 M playgrounds/nuxt/nuxt.config.ts
 D pnpm-lock.yaml
 M test/nuxt/nuxt.config.ts
?? .npmrc
```

`src/runtime/components` is clean (0 modified paths) with all 180 `.vue` files, so the
measured component set equals `0a1803d3`'s; the dirty paths are bench-setup residue
(`bench:meta:ui:setup`) outside the measured subtree, plus the generated (untracked,
ignored) `.nuxt/` directory whose tsconfigs the harness reads. The final Block 6 run
must use this same corpus checkout state.

## Driver configuration

```
node scripts/benchmark/trace-component-corpus.mjs \
  --ui-root=<worktree>/.integration-tests/repos/nuxt-ui \
  --output-dir=<worktree>/tmp/corpus-baseline/<run> \
  --timeout-ms=30000
```

- Fixed driver concurrency of one (hardcoded; no CLI override exists).
- No `--no-trace` (not a supported flag; audit/footprint capture is unconditional in
  `_audit-component.ts` via `{ auditEnabled: true, footprintCapture: true }`).
- Environment: driver-owned per-child env only (`FORCE_COLOR=0`,
  `VERTER_COMPONENT_META_AUDIT_PATH`, `VERTER_COMPONENT_META_ANALYSIS_PATH`,
  `VERTER_COMPONENT_META_RESULT_PATH`); no `VERTER_*`/`RUST_*` overrides were set in the
  parent environment. LIMITATION: the parent env was not snapshotted into
  `summary.json`, so ambient-override absence is asserted, not machine-verifiable after
  the fact. A future freeze should snapshot the filtered parent env into the summary.

### HARD PRECONDITION for the Block 6 final run — `--ui-root` / child-uiRoot identity

The parent driver uses `--ui-root` for component DISCOVERY only. Each child worker
independently derives its own `uiRoot` as
`getDefaultUiRoot(import.meta.dirname)` = `<repo>/.integration-tests/repos/nuxt-ui`
(`packages/benchmark/src/trace-component-resolver.ts:26`) and uses THAT for the
workspace root and tsconfig chain. **The final run must use a layout where the parent's
resolved `--ui-root` string equals the child's `getDefaultUiRoot()` result.** A mismatch
is silent: the parent enumerates corpus A while every child measures corpus B. This
baseline satisfied the precondition via a worktree-level `.integration-tests` symlink to
the primary checkout's `.integration-tests` (verified: both resolve the identical
string). Reproduce with the same layout or run from a checkout that physically contains
the corpus.

## Sweeps

One warm-up sweep + five measured sweeps, same configuration, run back-to-back
(2026-08-04 13:06–13:43 local). `Q_run = sum(query_ms_from_stdout over status=="ok")`;
`wall_ms` is diagnostic only (includes per-child Node/tsx/NAPI/process floor).

| Run | ok | failed | Q_run (ms) | wall total (ms, diagnostic) |
|---|---|---|---|---|
| warmup (excluded) | 179/180 | 1 | 330,783 | 355,787 |
| sweep-1 | 179/180 | 1 | 332,285 | 357,351 |
| sweep-2 | 179/180 | 1 | 337,146 | 362,727 |
| sweep-3 | 179/180 | 1 | 343,919 | 370,012 |
| sweep-4 | 179/180 | 1 | 343,638 | 369,729 |
| sweep-5 | 179/180 | 1 | 340,134 | 366,029 |

**Median baseline `Q_run` (measured sweeps) = 340,134 ms.** Spread max/min = 3.5%.

PERF-2 at Block 6: `R_query = median(final Q_run) / 340,134`, measured on this same
machine, same corpus state, same driver configuration and layout precondition.

## Known baseline failures (expected at tip)

- `src/runtime/components/Avatar.vue` — `crash` in every run (exit 1, no signal;
  `query_ms_from_stdout` is null, so Avatar contributes nothing to Q_run).
  First Verter-owned panic frame, from the child stderr:
  `crates/verter_session/src/resolver_core/resolver_context.rs:864:13 — "Architectural
  violation: bare-host prepared_value_decl called from production; construct
  HostResolverContext at the request entry"`, surfaced through
  `session.getComponentMetaWithAudit(canonical)` (`_audit-component.ts:217`) as NAPI
  `GenericFailure`. This is the plan's tracked Avatar defect (Block 5C); the frame is
  one of the nine bare-host panic arms (Block 5A). Recorded here as evidence, not fixed
  in 0A.
- Audit-validator duration-budget failures in every run (authored specs, wall-time
  bound, machine-dependent): `Alert.vue` total 3,666–3,842 ms across the five measured
  sweeps (warmup 3,575 ms) vs a 1,500 ms cap; `AvatarGroup.vue` total 2,395–2,505 ms
  (warmup 2,380 ms) vs 1,500 ms. Not correctness violations; recorded as baseline
  state.

Every sweep's failure set is exactly `{Avatar.vue}`; ok/failed counts identical across
all six runs.

## Determinism characterization (DET-1 at Block 0)

### Method — token-aware compare of the raw `analysis` artifacts (the DET-1 oracle)

The DET-1 comparison surface is the raw per-component `analysis/**.json` (full
`ComponentMetaAnalysis`, carrying complete structured type descriptors), deep-compared
pairwise. String values matching `/^[A-Za-z0-9_-]{43}$/` are per-process opaque
identities (audit tokens) and are CORRELATED, not ignored: the lockstep walk builds a
per-file A↔B token bijection — recorded for every token pair, including identical
ones, BEFORE the equality short-circuit — and any functionality violation (one A-token
mapped to two different B-tokens) or injectivity violation (two A-tokens mapped to one
B-token) is a SEMANTIC diff. That is what makes token-REFERENCE rewiring (reparenting,
sibling reordering, identity permutation) visible even though every position still
holds a token-shaped value; a consistent whole-file re-mint maps old→new uniformly and
stays a token diff. Tool:
[`scripts/benchmark/det1-analysis-diff.mjs`](../../../scripts/benchmark/det1-analysis-diff.mjs).

**Plan deviation, dispositioned ADOPT-NOW:** plan Part C Block 0A instructs "Compare
the already-normalized per-component artifacts … The normalizer already sorts names and
excludes timings, request IDs, and counters; any remaining divergence is semantic."
That premise is false in both directions on this tree: the normalizer ALSO collapses
every structured `TypeDescriptor` to `"[object Object]"` (so a real semantic
divergence in any non-primitive type is invisible — see the disclosed-blindness section
below), and the raw surface carries by-design per-process token identity (so not every
remaining raw divergence is semantic). This record therefore supersedes the plan's
comparison instruction with the token-aware `analysis` oracle above; the normalized
comparison is demoted to a supplementary check. Adopted here as the Block 0A/DET-1
method change; PERF-2 and every other Block 0A instruction are unaffected.

Result across all five pairs (warmup↔sweep-1, sweep-1↔sweep-{2,3,4,5}), 179 files per
pair, run through the comparator's gate contract (`--expect-files=179`): **61,016 token
diffs per pair, 0 token-bijection violations, 0 semantic diffs, verdict PASS (exit 0)
in every pair.** Zero divergence in `props`, `events`, `slots`, `models`, `exposed`,
`typeRegistry`, `resolution`, `resolutionStatus`, `fallthroughSurface`. The token
diffs are confined to the audit-token fields (identical field inventory in every pair;
counts from sweep-1↔sweep-2), split by role:

| Role | Field | Token diffs |
|---|---|---|
| mint identity | `sourceSpaceToken` [^sst] | 33,871 |
| mint identity | `attributeToken` | 6,695 |
| mint identity | `nodeToken` | 6,337 |
| mint identity | `blockToken` | 533 |
| mint identity | `artifactToken` | 179 |
| **mint subtotal** | | **47,615** |
| reference edge | `parentNodeToken` | 5,682 |
| reference edge | `childNodeTokens` | 5,682 |
| reference edge | `duplicateOf` | 1,382 |
| reference edge | `markup_root_tokens` | 655 |
| **edge subtotal** | | **13,401** |

[^sst]: `sourceSpaceToken` is a per-file CONSTANT, so its row is one token
    counted many times, not 33,871 independent identities. Census over this
    baseline's own sweep-1 `analysis` tree: **33,871 occurrences across 179
    files carry exactly 179 distinct values — one per file, and zero files
    hold more than one.** The row is therefore 179 re-mints observed 33,871
    times; the raw count is an occurrence count and must not be read as a
    measure of how much identity churn occurred. The other mint rows are not
    known to share this property and are not covered by this note.

The by-design-nondeterminism statement is scoped to the MINT identities (47,615):
fresh opaque ids minted per process, exactly what the earlier `artifact_token`
observation reported — that observation and this record measured different surfaces
and both are true. The 13,401 REFERENCE-edge diffs are not covered by that statement;
they are checked structurally by the bijection, which reported 0 violations on every
real pair — the edges re-minted consistently, with no rewiring.

**Disposition, restated on this method:** the conditional finding "normalized-output
nondeterminism, if observed" (plan finding table, CHARACTERIZE) is **REJECT (not
observed)** — six same-config runs are semantically identical for all 179 successful
components under the discriminating oracle below. No further investigation triggered.

**DET-1 at Block 6 is gated on THIS method** (token-aware `analysis` compare via
`det1-analysis-diff.mjs`, oracle re-proven by `det1-oracle-control.mjs`): two identical
final configurations must produce **0 semantic diffs AND 0 missing files AND
`compared == --expect-files` (180 at Block 6 — CORPUS-1 requires every component,
including Avatar)**, with the tool's exit status as the verdict (0 = pass; non-zero =
fail; a comparison over an empty or truncated tree exits non-zero and FAILS). The
normalized-`results` byte-compare below is a supplementary check only.

### Proven-discriminating oracle (positive + negative control)

The oracle is proven, not assumed. Control battery
([`scripts/benchmark/det1-oracle-control.mjs`](../../../scripts/benchmark/det1-oracle-control.mjs)),
each plant applied to a copy of the real sweep-1 `Alert.vue` analysis artifact and
proven landed in the input before the comparison runs (a non-landed or skipped plant
FAILS the control; exit non-zero on any failure). Each semantic plant must also FAIL
the comparator's exit-code gate, and the negative control must PASS it:

| Plant | Expected | Observed |
|---|---|---|
| Complex prop type: `AvatarProps` ref renamed to a different ref, `kind: "ref"` unchanged | semantic diff | DETECTED (`/props/4/type/name`), exit 1 |
| `typeRegistry` member rename | semantic diff | DETECTED (`/typeRegistry/0/name`), exit 1 |
| Removed slot | semantic diff | DETECTED (`/slots` array-length), exit 1 |
| REWIRE: markup node reparented (`parentNodeToken` repointed) | semantic diff | DETECTED (2 bijection violations), exit 1 |
| REWIRE: sibling order flipped in `childNodeTokens` | semantic diff | DETECTED (14 bijection violations), exit 1 |
| REWIRE: two nodes' `nodeToken` identities permuted | semantic diff | DETECTED (4 bijection violations), exit 1 |
| REWIRE: `markup_root_tokens` reordered | semantic diff | DETECTED (18 bijection violations), exit 1 |
| REWIRE: `duplicateOf` repointed to a different existing token | semantic diff | DETECTED (2 bijection violations), exit 1 — on each of Accordion, Table, Alert |
| NEGATIVE: token replaced by another 43-char token (all occurrences) | token diff only | 0 semantic, 1 token, exit 0 — correctly ignored |

The `duplicateOf` row was previously recorded as an untested residual because an
earlier selector failed to land a real repoint. It is no longer a residual: a
landed repoint (the first `duplicateOf` token slot re-pointed to a different token
already present in the same artifact, proven landed by a whole-document
inequality check before the comparison ran) is DETECTED on all three components
carrying enough slots to plant it — Accordion (22 `duplicateOf` slots), Table (39)
and Alert (6) — each reporting 2 bijection violations and exit 1. The violation
count depends on which token pair the plant selects and is not itself a contract;
the contract is "non-zero semantic diffs, exit 1".

### Comparator gate hardening (applied after the original freeze)

`compared == --expect-files` is a COUNT check and was not a distinctness check.
Two false-positive routes were closed in
[`det1-analysis-diff.mjs`](../../../scripts/benchmark/det1-analysis-diff.mjs):

- **Self-comparison.** `det1-analysis-diff.mjs <dir> <dir>` previously exited 0
  with `token_diffs: 0`, `semantic_diffs: 0`, `compared == expected` — a full PASS
  proving nothing. The two run directories, and separately their `analysis/`
  subtrees, are now rejected with exit 2 when they realpath to the same location.
- **Count padding by link.** `walk()` did not dedupe, so a symlinked or hard-linked
  duplicate subdirectory inflated the file list; a tree of 90 real components plus
  one symlinked duplicate subdir reported `compared: 180` and exit 0 against
  `--expect-files=180`. `walk()` now dedupes by realpath, so the same fixture
  reports the real count and FAILS the gate.

Both routes were reproduced against the pre-fix script and re-run against the
fixed one; an honest two-tree comparison still passes, a real semantic diff still
fails, and `det1-oracle-control.mjs` still reports `ORACLE DISCRIMINATES` (exit 0)
on this baseline's own sweep-1 `Alert.vue` artifact.

### Disclosed blindness of the normalized-`results` surface (superseded method)

The originally recorded DET-1 comparison (deep compare of `results/**.json`, the
`normalizeComponentMetaArtifact` output) **cannot see the type dimension**:
`normalizeProps` (`packages/benchmark/src/meta-ui-core.ts:195`) passes `type` through
`normalizeNullableString` (`:350`) = `String(value).trim()`, collapsing every structured
`TypeDescriptor` to the literal string `"[object Object]"`.

Census over this baseline's own sweep-1 `results` tree (lanes `props`/`events`/`slots`/
`exposed`/`models`, 3,298 published members): **2,492 members carry `"[object Object]"`,
806 carry `null`, 0 carry any other type string — two distinct type values
corpus-wide** (per-lane: all 2,455 props and all 37 exposed members are
`"[object Object]"`; all 190 events and 616 slots are `null`). The only surviving type
signal is `propsJsonSchema`, and the predicate matters: 2,277/2,455 props (92.7%) have
a `propsJsonSchema` entry PRESENT, but only 1,306/2,455 (53.2%) have an entry carrying
a `type` key — and that key encodes JSON-schema primitives only, never a named or
structural type. Events, slots, exposed and models have no schema surface at all.

Plant battery on the real sweep-1 `Alert.vue` payload through the full
`normalizeComponentMetaArtifact` path (each plant proven landed in the analysis input
before normalization; battery preserved at the session scratchpad
`det1-positive-control.ts`, results reproduced on this baseline's artifacts):

| Plant | Detected by `results` compare? |
|---|---|
| Primitive prop type change (`title` string→number) | YES (via `propsJsonSchema`) |
| Complex prop type: `AvatarProps` ref renamed to a different ref (kind unchanged) | **NO — blind** |
| Complex descriptor mutated in place | **NO — blind** |
| Removed prop | YES |
| Removed slot | YES |
| Removed event | YES |
| Requiredness flip | YES |

So the `results` surface detects member add/remove, requiredness, defaults,
descriptions, tags and PRIMITIVE prop types, and **cannot detect a change to any
non-primitive type**. Its earlier "0 divergent of 179" was a true observation on a
blind surface.

### Supplementary check — normalized-`results` byte-compare

Retained as a supplementary (not gating) check: all six runs' `results` trees are
byte-identical — 179 files each, verified by
`diff -rq <runA>/results <runB>/results` returning empty for warmup vs each measured
sweep, which by transitivity covers all 15 run pairs.

## Raw artifacts

Per-run outputs (summary.json, per-component audit/analysis/results/stdout/stderr) under
the measurement worktree at `tmp/corpus-baseline/{warmup,sweep-1..5}/` (untracked;
retained for the duration of the cutover). The `Q_run`, determinism, census, and hash
numbers above are derived exclusively from those files.
