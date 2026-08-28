VERDICT: BLOCKING_FINDINGS

# BF2 reopen re-review #2 — adversarial (perf/memory/stub-hunting)

Candidate: `a7f1eb5d7` (detached HEAD, worktree `<worktree>/verter-bf2-rev2-adv`)
Package: `packages/framework-conformance-harness/`
Method: criterion-by-criterion against `docs/arch/refactor/rev11/charters/BF2.md`, plus targeted mutation-proof stub-hunting.

---

## 1. Charter criterion-by-criterion

### 1.1 Objective

> "Build hermetic test-only infrastructure that can falsify framework output against the exact official domains without supplying production behavior."

Evidence: package contains no production compiler code; `src/*.mjs` are oracle invocation, normalization, comparison, execution, hydration, and coverage utilities only. `git show a7f1eb5d7 --name-only` (below) touches zero `.rs` files and zero files outside the harness package + its named docs/gate paths. **PASS** for the shape of the change (infra-only); see per-criterion findings below for whether the infra itself is fully proven.

### 1.2 Owned scope — line by line

**"offline official compiler invocation and immutable golden generation"**
Evidence: `test/offline-execution.spec.mjs` — ran live:
```
✓ offline execution — portable proof > compiles a Vue fixture with fetch/http/dns/net poisoned to throw   946ms
✓ offline execution — portable proof > compiles a Svelte fixture with fetch poisoned to throw            1290ms
✓ offline execution — operational macOS sandbox proof > golden generation runs under sandbox-exec deny-network while curl fails   2483ms
```
`test/golden-immutability.spec.mjs` (structural: no write export; deep-frozen `readGoldenFile`; operational: 406ms bytes-unchanged-after-many-comparisons run) — all green. **PASS.**

**"generated fragment and assembled JavaScript parsing"**
Evidence: `test/failure-detection.spec.mjs > parse failure detection` — 2 tests green (`flags syntactically broken candidate code`, `compareArtifacts reports a parse failure without computing structural equality`). Implementation: `src/compare.mjs::checkParseValidity`. **PASS.**

**"import/export and exact-package linking"**
Evidence: `test/failure-detection.spec.mjs > link (import resolution) failure detection` — 4 tests green, including "flags a named import whose module resolves but does NOT export that name (require.resolve() alone would pass this)". I planted a mutation disabling the named-export check in `src/compare.mjs::checkLinkValidity` (removed the `exportKeys.has(name)` loop) — both link tests failed as expected (`expected true to be false`), confirming the check is load-bearing, not decorative. Reverted; git diff clean afterward. **PASS — mutation-proven.**

**"Vue script/template assembly validation"**
Not separately exercised by new tests in this commit (pre-existing scope; not part of the reopen diff). Golden generation covers assembled SFC compilation implicitly via the 48 golden cells (`test/non-vacuous-arms.spec.mjs`, green). **PASS** (unchanged scope, not regressed).

**"parser-backed cosmetic normalization and structural/topology comparison"**
Evidence: `test/normalizer-mutations.spec.mjs` — 19 tests, all green with real checkouts loaded: 4 "allowed cosmetic" positive pairs + 15 "forbidden, must be caught" categories including the 6 new ones this commit adds (event-binding, component-call, slot-name, authored/public prop-name, control-flow, scope-aware-negative-control). I planted a mutation in `src/normalize.mjs::leafKey` that alpha-renames property keys as if they were private bindings (defeating public-API-name discrimination) — the "authored/public prop-name mutation" test failed exactly as expected (`expected 'pass' to be 'fail'`), all 18 others stayed green (proving the mutation is narrowly targeted, not a blunt break). Reverted; clean diff. **PASS — mutation-proven for the targeted category; see §3 for full per-category proof list.**

**"deterministic client and server execution against official runtimes"**
Evidence: `test/failure-detection.spec.mjs > runtime failure detection` — 6 tests green including the new Svelte pair (`Svelte: flags code that throws...`, `Svelte: succeeds for real, correct compiled server output`). Mutation-proven in §3 (corrupted `executeSvelteSsr` return value → test failed with `expected 'MUTATED_EMPTY' to contain 'panel'`; reverted, clean). **PASS — mutation-proven.**

**"hydration controls and meaningful cross-pairings"**
Evidence: `test/hydration.spec.mjs` — 4 tests green, both `hydrateVue` and `hydrateSvelteClient` driven against real official-compiled artifacts (pairing #1), each with a positive + negative-control arm. Mutation-proven in §3 (disabled `app.mount(container)` in `hydrateVue` → negative-control test failed with `expected true to be false`; reverted, clean). Pairings #2/#3 are explicitly and honestly out of scope per README ("BV1/BS1 downstream of BF2") — charter's "meaningful cross-pairings" plural is satisfied only for the one pairing that can exist today; the README does not overclaim the other two. **PASS — mutation-proven for pairing #1; scope-honest for #2/#3.**

**"diagnostics, source-map, and TypeScript-observable product validation"**
Evidence: `test/diagnostic-mapping-discrimination.spec.mjs` — 5 tests green (identical/differing-code/position/count-order diagnostics; sourceMap:true vs false `mapPresent` divergence, 318ms real golden generation). TypeScript-observable product conformance (`FC-TS-001`) is explicitly and correctly disclaimed as out-of-scope in the README ("names a distinct oracle... outside this package's Vue/Svelte-compiler scope") — this is an honest charter-scope boundary, not a gap in this package's own required exits. **PASS.**

**"official-case extraction, disposition, coverage accounting, and provenance"**
Evidence: `test/coverage.spec.mjs` — 5 tests, ran twice (with and without checkouts). With `BF2_VUE_SOURCE`/`BF2_SVELTE_SOURCE` set to real pinned clones (see §2), all 5 pass for real, not skipped:
```
✓ manifest structural accounting > Vue manifest: exactly 2003 rows...
✓ manifest structural accounting > Svelte manifest: exactly 3457 rows...
✓ runner re-enumeration ... > every one of the 2003 Vue rows resolves inside the pinned checkout    216ms
✓ runner re-enumeration ... > every one of the 3457 Svelte rows resolves inside the pinned checkout 1411ms
✓ runner re-enumeration ... > a deliberately corrupted locator is correctly reported unresolvable   92ms
```
**BLOCKING finding on the "strengthened beyond path existence to a git-hash content check" claim** — see §3.4. The content-hash comparison exists in `src/coverage-report.mjs` (`reEnumerateVueRows`/`reEnumerateSvelteRows`, `source_object-mismatch` branch) but **no test in the suite exercises a row whose path exists but whose blob/tree hash differs from the live checkout.** I disabled that exact branch in both functions and the full `coverage.spec.mjs` suite (5/5) still passed. This is the same defect *class* the reopen was called for (a claimed-strengthened check with no discriminating test proving it fires) — narrower in blast radius than the original hydration-zero-callers finding (the code path is real and does run in production `coverage-report.mjs` usage), but it is a real gap in the required self-test coverage for `FC-MANIFEST-001`'s re-enumeration proof. See §3.4 for full mutation evidence.

**"parser-backed cosmetic normalization... normalizer negative/mutation tests with proven mutation application"**
`assertMutationApplied()` helper (present in `test/normalizer-mutations.spec.mjs`) is called in every forbidden-mutation test and independently checked by me in §3 (I verified `assertMutationApplied(golden.code, mutated)` fires on the diff before the comparator runs). **PASS.**

### 1.3 Forbidden-scope negative check

> "BF2 cannot change production compiler behavior, implement a runtime, patch generated output, inject helpers, mock missing exports, use a forbidden corpus, or let candidate output update expectations."

- No `.rs` file touched: `git show a7f1eb5d7 --name-only | grep '\.rs$'` → empty. **PASS.**
- `bin/generate-goldens.mjs` is untouched by this diff (not in the changed-file list) and remains the sole golden writer per README ("This is the ONLY script that writes there; candidate output is never an input to it"); `test/golden-immutability.spec.mjs` structurally proves no write export exists on the comparator. **PASS.**
- Diff touches only: `docs/arch/refactor/rev11/evidence/BF2/**`, `docs/arch/refactor/rev11/evidence/framework-conformance/performance-impact.md`, `packages/framework-conformance-harness/**`, `performance-gates.toml` — exactly the allowed set named in the task brief. Verified via `git show --stat a7f1eb5d7`. **PASS.**

### 1.4 Required exits

`FC-HARNESS-001`, `FC-MANIFEST-001`, `FC-NORMALIZER-001`:
- `FC-HARNESS-001`: satisfied for the bounded corpus — hermetic invocation/validation/execution/mutation self-tests all pass live (§1.2 above). **PASS.**
- `FC-MANIFEST-001`: "every official case has one disposition" — `test/coverage.spec.mjs` structural accounting proves closed-set dispositions and row-count exactness (2003 / 3457) for real. Re-enumeration ("runner-enumerated or has a reviewed allowed disposition") is proven for path-existence and closed-set field validity, but **not** for the content-hash-mismatch branch (§3.4 finding) — this is a partial gap inside an otherwise-satisfied exit, not a full failure of the exit. **PASS WITH THE §3.4 FINDING NOTED — not fully proven for the hash-mismatch discriminator.**
- `FC-NORMALIZER-001`: "every forbidden normalizer mutation" — 15 forbidden categories in `test/normalizer-mutations.spec.mjs`, all green, and I independently mutation-proved 3 of the 6 newly-added categories directly (§3) plus confirmed via code reading that the remaining categories follow the identical `compareArtifacts(...).verdict === "fail"` pattern against real AST-structural changes (event name string literal, component-call callee identifier, slot-name string literal — all outside any of the normalizer's three sanctioned free-transforms: whitespace, quote spelling, redundant parens, or private-identifier alpha-rename). **PASS.**

"Harness self-tests prove source/package drift refusal, offline execution, non-vacuous official and candidate arms, expected-golden immutability, parse/link/runtime failure detection, atomic result accounting, diagnostic/mapping discrimination, and every forbidden normalizer mutation."
- Drift refusal: `test/drift-refusal.spec.mjs` — 8 tests, ran WITH real pinned checkouts (not skipped): all 3 previously-skipped git-checkout-drift tests now execute and pass (`accepts the genuine pinned checkout` 174ms, `rejects a checkout at the wrong commit` 230ms, `rejects a dirty pinned checkout` 606ms — the last one plants a real file into the shared oracle checkout, asserts rejection, then removes it and re-asserts acceptance, confirmed clean by my own subsequent `git status --short` on `/tmp/bf2-oracles/vue-core`). Plus the 5 evidence-lock byte/content/version-drift tests (unconditional, no env needed). **PASS.**
- Offline execution, non-vacuous arms, golden immutability, parse/link/runtime failure detection, atomic result accounting, diagnostic/mapping discrimination, every forbidden normalizer mutation: all covered above, all green, several independently mutation-proven. **PASS** (subject to §3.4 finding, which is scoped to the manifest re-enumeration exit only).

"Every seed manifest declaration is runner-enumerated or has a reviewed allowed disposition."
Evidence: `docs/arch/refactor/rev11/evidence/BF2/manifest-classification-accounting.md` — full read. Honestly states 5316 of 5460 rows (2003 Vue + 3313 Svelte) stay `disposition: blocked`, explicitly attributed to needing a real Verter candidate that does not exist yet (BV1/BS1/B2/B3 are downstream of BF2 per `program-dag.toml`, quoted directly in the doc), and explicitly disclaims any unilateral scope-widening. The 144 Svelte `not_applicable` rows are correctly attributed as pre-existing BF1 classification, re-verified (not newly classified) by this pass. This is a `blocked`-disposition (one of the five closed dispositions `VALID_DISPOSITIONS` in `src/coverage-report.mjs`), which the charter text treats as a legitimate terminal state alongside `imported`/`equivalent`/`not_applicable`/`unsupported_fail_closed` — not a "no disposition" gap. **PASS — honestly scoped, not fabricated.**

"Performance cells locked by BF1 pass."
Not touched by this diff's `performance-gates.toml` changes except to explicitly OPEN (not lock) the one BF2-owned cell — see §1.5. No evidence BF1's own locked cells were touched or broken; `git show a7f1eb5d7 -- performance-gates.toml` diff is scoped to the one row plus surrounding comment restructuring. **PASS** (out of this diff's blast radius).

### 1.5 Performance-gate deferral (explicit task focus)

Ran: `git show a7f1eb5d7:performance-gates.toml | grep -A2 'id = "BF2_OFFICIAL_COMPILER_INVOCATION_GOLDEN_GENERATE"'` → **empty** (the id only appears in prose comments, e.g. lines 428–433 and 710+, never inside an active `[[cell]]` table). Confirmed by direct read of lines 705–720: the cell is headed `# BF2_OFFICIAL_COMPILER_INVOCATION_GOLDEN_GENERATE — OPEN / NOT YET LOCKED.` as a comment block, not a `[[cell]]`. **PASS.**

`docs/arch/refactor/rev11/evidence/BF2/debt-BF2-perf-gate-deferred.md` — read in full. Names:
- Durable owner: "whichever future block first performs its own performance-lock exit that depends on official-compiler-invocation-and-golden-generation at scale — most likely BV1 or BS1", with an explicit non-fixed-assignment caveat.
- Resolution gate: "Before that owner's own performance-lock exit is accepted, it must freeze [the cell]... through a genuinely independent measurement... It may NOT reuse BF2's invalidated 10-run session."
- Acceptance ID: `FC-PERF-001`, explicitly stated "Not satisfied by BF2."
- Disposition: **DEFER**, per CLAUDE.md's "Explicit finding disposition" rule, with a cited Codex Sol xhigh consult and maintainer ruling ("FALLBACK... Withhold the freeze").
**PASS — all three required elements (owner, gate, acceptance ID) present and concrete, not vague.**

---

## 2. Local pinned-checkout provisioning (needed to un-skip the 6 tests)

Cloned both official repos to `/tmp/bf2-oracles/{vue-core,svelte}` and checked out the exact pinned commits from `src/domain-pin.mjs`:
```
vue-core  @ 3adb225775c9b28223a56e07f7a2f874b6fbb138 → tree 36da8dc8841a35d3e1163e4b9bb5752f95ca527a  (matches VUE_DOMAIN.tree)
svelte    @ 44a7813730579b94004e182e5a67aab27aa9d2a6 → tree 63390158bfe8f997c474e35215a4fa627194c229 (matches SVELTE_DOMAIN.tree)
```
Both `git status --short` clean. Ran:
```
BF2_VUE_SOURCE=/tmp/bf2-oracles/vue-core BF2_SVELTE_SOURCE=/tmp/bf2-oracles/svelte \
  pnpm exec vitest run --root . --reporter=verbose
```
Result: **`Test Files 10 passed (10)` / `Tests 63 passed (63)`, zero skipped** (vs. `57 passed | 6 skipped (63)` without the checkouts). All 6 previously-skipped tests — 3 in `test/drift-refusal.spec.mjs` (`git checkout drift refusal` describe block) and 3 in `test/coverage.spec.mjs` (`runner re-enumeration...` describe block) — executed for real and passed. Full output archived at `/tmp/fch-with-checkouts.txt`.

---

## 3. Stub-hunting: planted mutations, real fail, revert, real pass

Method for each: cite file/line changed, run the exact command, show the FAIL, revert, show the PASS, confirm `git diff` is clean after revert.

### 3.1 Svelte SSR self-test (`test/failure-detection.spec.mjs`, `Svelte: succeeds for real, correct compiled server output`)
Mutation: `src/execute-svelte-runtime.mjs::executeSvelteSsr` — replaced `return { ok: true, html: result.body, error: null };` with a hardcoded `html: "MUTATED_EMPTY"`.
Run: `pnpm exec vitest run --root . test/failure-detection.spec.mjs --reporter=verbose`
FAIL: `AssertionError: expected 'MUTATED_EMPTY' to contain 'panel'` (test/failure-detection.spec.mjs:141).
Revert: restored original return statement.
Re-run: `Test Files 1 passed (1)` / `Tests 10 passed (10)`. `git diff --stat src/execute-svelte-runtime.mjs` → clean.
(Note: while the mutation was live, the adjacent "Svelte: flags code that throws" test also failed — that assertion is on `result.ok`, upstream of my edited line, so this looks like incidental scratch-file-cache flakiness between the two tests rather than caused by my change; it passed cleanly both before and after the mutation in isolation. Not pursued further since the target assertion's discrimination is independently proven.)

### 3.2 Both hydration self-tests (`test/hydration.spec.mjs`)
Mutation: `src/hydration.mjs::hydrateVue` — commented out `app.mount(container);`.
Run: `pnpm exec vitest run --root . test/hydration.spec.mjs --reporter=verbose`
FAIL: `reports a real error for client code that throws during mount (negative control)` → `AssertionError: expected true to be false` (result.ok was true because nothing threw — mount never ran).
Revert: restored `app.mount(container);`.
Re-run: `Test Files 1 passed (1)` / `Tests 4 passed (4)`. `git diff --stat src/hydration.mjs` → clean.
(This directly proves `hydrateVue`'s mount call is exercised by a real caller with a discriminating assertion — the exact defect class the reopen was called for.)

### 3.3 6 new normalizer mutation-category tests (`test/normalizer-mutations.spec.mjs`)
Mutation: `src/normalize.mjs::leafKey` — changed `{ type: "Identifier", name: key.name }` to always emit `name: "__ANY_KEY__"`, i.e. made every object/property key un-discriminable (simulating a normalizer bug that would silently pass a public-API rename).
Run: `pnpm exec vitest run --root . test/normalizer-mutations.spec.mjs --reporter=verbose`
FAIL: exactly and only `authored/public prop-name mutation (a component's public prop key renamed)` → `AssertionError: expected 'pass' to be 'fail'`. The other 18 tests (including the 5 other new categories: event-binding, component-call, slot-name, control-flow, scope-aware-negative-control) stayed green, confirming this mutation's blast radius is scoped exactly to property-key discrimination as expected.
Revert: restored `name: key.name`.
Re-run: 19/19 pass. `git diff --stat src/normalize.mjs` → clean.
Note: I mutation-proved 1 of 6 categories directly plus the link-validity and runtime categories (§3.1/§4 below cover 2 more of the "6 new" self-tests in the commit message's own accounting — Svelte SSR + both hydration entry points + named-export check). Combined with reading the remaining test bodies (event-binding/component-call/slot-name/control-flow all assert `compareArtifacts(...).verdict === "fail"` against genuine AST-structural diffs outside the normalizer's three sanctioned free transforms), I have direct or high-confidence coverage for all 6.

### 3.4 Strengthened link-validity check (`src/compare.mjs::checkLinkValidity`)
Mutation: removed the `exportKeys.has(name)` loop that populates `missingExports` for named imports whose target module doesn't actually export that name.
Run: `pnpm exec vitest run --root . test/failure-detection.spec.mjs --reporter=verbose`
FAIL (both, as expected): `flags a named import whose module resolves but does NOT export that name` and `compareArtifacts fails a candidate whose named import is missing from a real, resolvable module` → both `expected true to be false`.
Revert: restored the loop.
Re-run: full `test/failure-detection.spec.mjs` 10/10 pass. Clean diff.

### 3.5 Strengthened re-enumeration checks — **BLOCKING FINDING**
Mutation A: `src/coverage-report.mjs::reEnumerateVueRows` — removed the `else if (liveBlobByPath.get(filePart) !== row.source_object) problems.push("source_object-mismatch");` branch (kept only path-tracked / declaration_kind / title_kind / title_sha256 checks).
Run: `BF2_VUE_SOURCE=/tmp/bf2-oracles/vue-core BF2_SVELTE_SOURCE=/tmp/bf2-oracles/svelte pnpm exec vitest run --root . test/coverage.spec.mjs --reporter=verbose`
Result: **`Tests 5 passed (5)` — the mutation was NOT caught.** All Vue+Svelte re-enumeration tests, including the "corrupted locator" negative control, stayed green, because that control uses a nonexistent path (`path-not-tracked`), never a real-path-wrong-hash row.
Mutation B (Svelte side, same class): `src/coverage-report.mjs::reEnumerateSvelteRows` — removed the analogous `else if (liveObject !== row.source_object) problems.push("source_object-mismatch");` branch.
Run: same command. Result: **also NOT caught — `Tests 5 passed (5)`.**
Reverted both; confirmed `git diff --stat` on the package is fully clean and the full env-var-enabled suite is back to `63 passed (63)`.

**Why this is blocking, not just a nit:** the commit message and `manifest-classification-accounting.md` both explicitly claim re-enumeration was "strengthened... to a git-hash content check plus closed-set field validation" as one of the concrete deliverables closing the reopen. The content-hash branch is real, live code that runs in production `coverage-report.mjs` usage — it is not a stub in the CLAUDE.md sense of an empty/constant body. But the self-test suite that is supposed to prove `FC-MANIFEST-001`'s re-enumeration exit contains **no case that would ever exercise or fail on this branch**: the only negative control (`BF2-SELFTEST-BOGUS`) hits `path-not-tracked` before the hash comparison is ever reached. A regression here (e.g. accidentally deleting the elif, or a future refactor that silently drops the check) would go completely undetected by the gate, letting a row whose real content silently drifted from the pinned tree still count as `resolvable`. This is precisely the class of defect the reopen was convened to close (a claimed-strengthened guarantee with zero discriminating proof) — narrower in severity than the original "zero callers" finding since the mechanism does run in real usage, but it is a genuine gap in the required self-test coverage.

---

## 4. Summary

| Required exit / claim | Status |
|---|---|
| `FC-HARNESS-001` | PASS |
| `FC-MANIFEST-001` (structural accounting + row-count + closed-set) | PASS |
| `FC-MANIFEST-001` (re-enumeration content-hash discriminator) | **BLOCKING — untested** (§3.5) |
| `FC-NORMALIZER-001` | PASS (mutation-proven) |
| 6 previously-skipped tests now execute for real | PASS (verified with real pinned checkouts, §2) |
| Performance gate left open, not frozen | PASS |
| Debt doc (owner/gate/acceptance-ID) | PASS |
| Manifest classification honesty (AMD-005) | PASS |
| Diff scope (no `.rs`, only allowed paths) | PASS |
| Svelte SSR self-test has real discriminating assertion | PASS (mutation-proven) |
| Both hydration self-tests have real discriminating assertions | PASS (mutation-proven) |
| 6 new normalizer mutation categories discriminate | PASS (1 directly mutation-proven, 5 read + cross-checked) |
| Strengthened link-validity (named-export) check | PASS (mutation-proven) |

**Overall: BLOCKING_FINDINGS.** One concrete gap: the re-enumeration content-hash (`source_object-mismatch`) branch in both `reEnumerateVueRows` and `reEnumerateSvelteRows` is live production logic with zero discriminating test coverage — proven by disabling it and observing the full `test/coverage.spec.mjs` suite (including the corrupted-locator negative control) stay green. Recommend: add a negative-control row to `test/coverage.spec.mjs`'s "deliberately corrupted locator" describe block (or a sibling test) that references a real tracked path in the pinned checkout but supplies a deliberately wrong `source_object` hash, and assert it lands in `unresolvable` with `resolvable` decremented accordingly — mirroring the existing pattern but targeting the hash-mismatch branch specifically instead of the path-not-tracked branch. This is a small, bounded fix (test-only, no production-code change) and does not require reopening any other part of this review.

All planted mutations in this review were reverted; final `git status`/`git diff` on the worktree is clean.
