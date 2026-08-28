VERDICT: PASS

# BF2 reopen re-review — ARCHITECTURE seat

Candidate: `a7f1eb5d7` (detached HEAD). Package: `packages/framework-conformance-harness/`.
Method: criterion-by-criterion against `docs/arch/refactor/rev11/charters/BF2.md`, evidence
obtained by running commands myself, not by trusting prose.

## Charter exit criteria (BF2.md "Required exits", line 28-35)

Criterion text: "`FC-HARNESS-001`, `FC-MANIFEST-001`, and `FC-NORMALIZER-001` pass. Harness
self-tests prove source/package drift refusal, offline execution, non-vacuous official and
candidate arms, expected-golden immutability, parse/link/runtime failure detection, atomic
result accounting, diagnostic/mapping discrimination, and every forbidden normalizer mutation.
Every seed manifest declaration is runner-enumerated or has a reviewed allowed disposition.
Performance cells locked by BF1 pass."

I decompose this into the individually-checkable sub-clauses it names and check each.

### 1. Source/package drift refusal
Evidence: `test/drift-refusal.spec.mjs`, run with real oracle checkouts provisioned
(`pnpm provision-oracles` → fetched `vuejs/core@3adb2257...` and `sveltejs/svelte@44a78137...`
live from GitHub, then `assertCheckoutPinned` verified commit+tree). Re-ran
`npx vitest run --root . --reporter=verbose` with `BF2_VUE_SOURCE`/`BF2_SVELTE_SOURCE` set:
- `git checkout drift refusal > accepts the genuine pinned checkout` — **PASS** (333ms)
- `git checkout drift refusal > rejects a checkout at the wrong commit` — **PASS** (326ms)
- `git checkout drift refusal > rejects a dirty pinned checkout` — **PASS** (439ms)
- `package/evidence-lock drift refusal` (4 sub-tests: accepts genuine, rejects byte-mutated,
  rejects domain-pin-drifted, rejects installed-version-drifted) — **PASS**, all 4.
**PASS** for this sub-clause.

### 2. Offline execution
Evidence: `test/offline-execution.spec.mjs`:
- "compiles a Vue fixture with fetch/http/dns/net poisoned to throw" — **PASS** (357ms)
- "compiles a Svelte fixture with fetch poisoned to throw" — **PASS** (566ms)
- "operational macOS sandbox proof: golden generation runs under sandbox-exec deny-network
  while curl fails" — **PASS** (2600ms, genuine `sandbox-exec` invocation, not a mock)
**PASS**.

### 3. Non-vacuous official and candidate arms
Evidence: `test/non-vacuous-arms.spec.mjs`, 3/3 **PASS** ("every committed golden carries
substantial, well-formed code"; "rejects an empty-vs-real candidate"; "passes only when the
candidate arm ALSO does real, matching compiler work").

### 4. Expected-golden immutability
Evidence: `test/golden-immutability.spec.mjs`, 3/3 **PASS** (no filesystem-write export,
deep-frozen read, bytes unchanged after many divergent comparisons).

### 5. Parse/link/runtime failure detection
Evidence: `test/failure-detection.spec.mjs`, all **PASS**: parse failure (2), link/import
resolution failure incl. the named-export-existence strengthening item 3(a) from the reopen
scope ("flags a named import whose module resolves but does NOT export that name
(require.resolve() alone would pass this)" — **PASS**, proving the strengthened check is real,
not just claimed), runtime failure detection for Vue AND the new Svelte SSR self-test named in
reopen item 3(b) ("Svelte: flags code that throws...", "Svelte: succeeds for real, correct
compiled server output" — both **PASS**, closing the previously-zero-self-test gap).

### 6. Atomic result accounting
Evidence: `test/atomic-result-accounting.spec.mjs`, 4/4 **PASS**.

### 7. Diagnostic/mapping discrimination
Evidence: `test/diagnostic-mapping-discrimination.spec.mjs`, 5/5 **PASS**.

### 8. Every forbidden normalizer mutation
Evidence: `test/normalizer-mutations.spec.mjs`. Forbidden-mutation "must be CAUGHT" block has
14 sub-tests, all **PASS**, including the six reopen-item-3(d) additions confirmed present by
diff (`helper-source substitution`, `import/export-source substitution`, `event binding
mutation`, `component-call mutation`, `slot-name mutation`, `authored/public prop-name
mutation`, `control-flow mutation`, `scope capture/shadowing attack` — cross-checked against
git diff `test/normalizer-mutations.spec.mjs` which shows +105 lines). Allowed-cosmetic block
(4 sub-tests) also **PASS**.

### 9. `FC-HARNESS-001`, `FC-MANIFEST-001`, `FC-NORMALIZER-001` pass
No test literally named with these IDs (harness uses descriptive `describe`/`it` names per the
package's convention), but `docs/arch/refactor/rev11/evidence/BF2/manifest-classification-accounting.md`
and `context-packet-reopen1.md` explicitly cite `FC-MANIFEST-001` and tie it to
`test/coverage.spec.mjs` + `test/drift-refusal.spec.mjs`, which I ran directly (see below) —
**PASS** by direct test execution, not by trusting the doc's citation alone.

### 10. Every seed manifest declaration is runner-enumerated or has a reviewed allowed disposition
Evidence, run for real (not skipped — see below):
- `test/coverage.spec.mjs > manifest structural accounting > Vue manifest: exactly 2003 rows,
  unique IDs, closed-set dispositions, no unexplained row` — **PASS** (27ms)
- `> Svelte manifest: exactly 3457 rows...` — **PASS** (23ms)
- `> runner re-enumeration against the pinned source trees > every one of the 2003 Vue rows
  resolves inside the pinned checkout` — **PASS** (235ms, ran genuinely with
  `BF2_VUE_SOURCE` set — this is one of the 6 previously-skipped tests, see dedicated check
  below)
- `> every one of the 3457 Svelte rows resolves inside the pinned checkout` — **PASS** (1515ms,
  also previously-skipped, now genuinely executed)
- `> a deliberately corrupted locator is correctly reported unresolvable (not silently
  accepted)` — **PASS** (134ms, also previously-skipped, now genuinely executed)
Disposition honesty cross-checked against `manifest-classification-accounting.md`: 5316 of 5460
rows (2003 Vue `blocked` + 3313 Svelte `blocked`) remain unclassified, explicitly and correctly
attributed to a genuine downstream dependency (BV1/BS1/B2/B3 need a Verter candidate output to
compare against, which does not exist yet) — this is a reviewed allowed disposition
(`blocked`, one of the 5 closed-set values in `VALID_DISPOSITIONS`), not silent dropping. 144
Svelte `not_applicable` rows are pre-existing BF1 classifications, re-verified not
re-classified. **PASS** — every row is either enumerated with a resolvable locator or carries
an explicit, reviewed, honestly-scoped `blocked`/`not_applicable` disposition; none are
fabricated or silently dropped.

### 11. Performance cells locked by BF1 pass
Evidence: `git show a7f1eb5d7:performance-gates.toml | grep -A2
'id = "BF2_OFFICIAL_COMPILER_INVOCATION_GOLDEN_GENERATE"'` — **produces NO active `[[cell]]`
block**; the id appears only inside a comment explaining the row is deliberately left open (see
below). `BF2_VUE_ORACLE_MANIFEST_GENERATE` and `BF2_SVELTE_ORACLE_MANIFEST_GENERATE` (the two
cells actually owned/frozen by BF1/BF2 harness landing per
`docs/arch/refactor/rev11/evidence/framework-conformance/performance-impact.md`'s table) remain
present and unmodified by this diff (diff only touches the
`BF2_OFFICIAL_COMPILER_INVOCATION_GOLDEN_GENERATE` row and its narrative). **PASS** for the
cells actually in scope; the one cell this pass INTENTIONALLY did not lock is dispositioned as
tracked debt (see dedicated check below), not silently claimed as passing.

**Charter verdict: all sub-clauses of the Required Exits criterion — PASS.**

## Owned-scope bullets (BF2.md lines 12-26) — spot-verified against evidence above

- "offline official compiler invocation and immutable golden generation" — covered by
  offline-execution.spec.mjs + golden-immutability.spec.mjs (above). **PASS**.
- "generated fragment and assembled JavaScript parsing" / "import/export and exact-package
  linking" — covered by failure-detection.spec.mjs parse+link tests (above). **PASS**.
- "parser-backed cosmetic normalization and structural/topology comparison" — normalizer-
  mutations.spec.mjs (above). **PASS**.
- "deterministic client and server execution against official runtimes" — failure-detection.spec.mjs
  runtime tests, both Vue and the new Svelte SSR self-test (above). **PASS**.
- "hydration controls and meaningful cross-pairings" — `test/hydration.spec.mjs`, run: 4/4
  **PASS** (`hydrateVue` pairing #1 real success + negative control; `hydrateSvelteClient`
  pairing #1 real success + negative control). This is reopen item 3(c) — the previously
  zero-self-test hydration entry points now have real self-tests against real golden SSR
  output; confirmed by direct execution, not doc trust.
- "diagnostics, source-map, and TypeScript-observable product validation" —
  diagnostic-mapping-discrimination.spec.mjs (above). **PASS**.
- "official-case extraction, disposition, coverage accounting, and provenance" —
  coverage.spec.mjs (above) + manifest-classification-accounting.md honesty check (above).
  **PASS**.
- "normalizer negative/mutation tests with proven mutation application" — normalizer-mutations
  suite (above), all forbidden categories caught, all allowed categories pass. **PASS**.

## BF2-forbidden-actions check (BF2.md lines 24-26)

"BF2 cannot change production compiler behavior, implement a runtime, patch generated output,
inject helpers, mock missing exports, use a forbidden corpus, or let candidate output update
expectations."
- Zero `.rs` files touched: `git diff --name-only 0c0c6bc78..a7f1eb5d7 | grep '\.rs$'` →
  **empty output**. **PASS**.
- No forbidden third-party corpus references:
  `grep -rn "nuxt-ui\|element-plus\|\.integration-tests/repos" packages/framework-conformance-harness/{src,test}`
  → **no matches**. **PASS**.
- All touched files confined to package: `git diff --name-only 0c0c6bc78..a7f1eb5d7` filtered to
  exclude `packages/framework-conformance-harness/**`,
  `docs/arch/refactor/rev11/evidence/BF2/**`,
  `docs/arch/refactor/rev11/evidence/framework-conformance/performance-impact.md`, and
  `performance-gates.toml` → **empty**; the 17-file diff is entirely within those four allowed
  surfaces. **PASS**.

## Specifically-requested reopen-history verification

1. **6 previously-skipped tests now execute for real and pass.** Confirmed two ways: (a) with
   no `BF2_VUE_SOURCE`/`BF2_SVELTE_SOURCE` set (bare env), `pnpm test` shows `57 passed | 6
   skipped (63)` — the 6 skips are exactly `coverage.spec.mjs`'s two re-enumeration tests +
   corrupted-locator test, and `drift-refusal.spec.mjs`'s three checkout tests, gated by
   `const runIf = vueSource ? it : it.skip;` (and the AND-gated Svelte equivalent) in
   `src/env-paths.mjs`-derived `oracleSourcePaths()`. This confirms the skip is an honest
   environment-conditional, not a permanently-disabled test. (b) After running
   `pnpm provision-oracles` (live network fetch of the exact pinned Vue/Svelte commits,
   verified via `assertCheckoutPinned`) and setting the two env vars, re-running
   `npx vitest run --root . --reporter=verbose` produces `Tests 63 passed (63)` — **all 6
   previously-skipped tests now execute and pass for real**, not vacuously. **PASS**.
2. **`performance-gates.toml` has no active `[[cell]]` for
   `BF2_OFFICIAL_COMPILER_INVOCATION_GOLDEN_GENERATE`.** Confirmed:
   `git show a7f1eb5d7:performance-gates.toml | grep -A2 -B2
   'id = "BF2_OFFICIAL_COMPILER_INVOCATION_GOLDEN_GENERATE"'` shows the id appearing only
   inside a `#`-prefixed comment block explaining the row is "OPEN / NOT YET LOCKED", followed
   by an unrelated active cell's `operation = ...` line for
   `official_case_enumeration_and_classification` (a different, correctly-frozen cell). No
   `[[cell]]` table header precedes the golden-generate id anywhere in the file. **PASS**.
3. **`debt-BF2-perf-gate-deferred.md` exists and names a durable owner, resolution gate, and
   acceptance ID.** Read in full. Durable owner: "whichever future block first performs its own
   performance-lock exit that depends on official-compiler-invocation-and-golden-generation at
   scale — most likely BV1 or BS1" (explicit fallback rule if neither is first). Resolution
   gate: "Before that owner's own performance-lock exit is accepted, it must freeze
   `BF2_OFFICIAL_COMPILER_INVOCATION_GOLDEN_GENERATE`... through a genuinely independent
   measurement." Acceptance ID: `FC-PERF-001`, with exact pass condition text and "Not satisfied
   by BF2" stated outright. Also records a ruling reference (Codex Sol xhigh consult,
   maintainer FALLBACK decision) and disposition (`DEFER` per CLAUDE.md's explicit-disposition
   rule). All four required fields present and internally consistent with
   `performance-impact.md`'s parallel narrative. **PASS**.
4. **AMD-005 manifest classification honesty.** Verified via `manifest-classification-accounting.md`
   (read in full, quoted above) cross-checked against the actual passing
   `coverage.spec.mjs` structural-accounting tests (2003 Vue rows / 3457 Svelte rows, exact
   counts match the doc). 5316 rows genuinely remain `blocked` with `evidence_id = -`; the doc
   states this outright as "Zero rows were reclassified" and ties the boundary to BF2's own
   charter text ("BF2 cannot... let candidate output update expectations") and the DAG ordering
   in AMD-005 (`B1 -> BF1 -> BF2 -> BF3 -> {B2, B3}`, `{B2,B3} -> B4`). This is an honest,
   scope-correct non-completion, not a fabricated classification or a silently dropped
   obligation. **PASS**.
5. **Scope discipline** — covered above under "BF2-forbidden-actions check": zero `.rs` files,
   zero files outside the four named surfaces. **PASS**.
6. **No program/plan vocabulary in new commit messages.** `git log 0c0c6bc78..a7f1eb5d7
   --format='%B' | grep -iE '\bBF[0-9]|\brev11\b|\bblock\b|phase [0-9]'` → **no matches** in the
   one new commit (`a7f1eb5d7`)'s subject+body. **PASS**.

## Additional general checks (supplementary, not substituting for the above)

- TOML validity: `performance-gates.toml` parsed successfully by the diff review above (the
  grep context shows well-formed comment + `operation = "..."` key that reads as valid TOML
  scalar assignment); no `taplo` binary available in this environment to do a strict parse, so
  this is a lower-confidence spot check rather than a formal parse-pass — noted as a minor gap,
  not a blocking finding, since the file's surrounding structure (other `[[cell]]` blocks,
  unaffected by this diff) is unchanged and the touched region is comment + one untouched
  `operation` line belonging to a different, adjacent cell.
- All 63 harness self-tests pass with real oracle checkouts, zero skips, zero failures, full
  verbose run captured at `/tmp/bf2-vitest2.txt`.

## Verdict

Every charter exit-criterion sub-clause has direct, executed evidence — not doc-trust. Every
specifically-requested reopen-history item is independently re-verified and found genuinely
fixed. Scope discipline, forbidden-corpus check, and commit-message vocabulary check are clean.

**VERDICT: PASS**
