VERDICT: BLOCKING_FINDINGS

# BF2 conformance harness — ADVERSARIAL review, round 7

**One blocking finding (A7-1), on the harness's own test determinism — not on
any of the four pass-7 mechanisms.** All four round-6 causes (B1, B2, B3,
A6-1/AF-4) are genuinely and independently CLOSED; I reproduced each of my
and the architecture seat's round-6 counterexamples directly against
`8cdafe329` and each now refuses/detects correctly, and all four
plant-red-green cycles discriminate. But the candidate **fails mandated item
2**: under the canonical invocation the suite is **226/226 in only 4 of 8
runs** — the *new* B3 regression test is flaky at ~50%, timing out against
vitest's 5000 ms default, which is the exact failure mode pass 7 recognized
and fixed in two *other* files while leaving it in the test it added itself.

A designated regression test that fails half the time under its own canonical
gate is the same class of defect I filed as blocking in round 6 (A6-1), in the
opposite direction. Under the mandatory method — and item 2's explicit
"expect 226/226" — that is BLOCKING, not an observation.

---

## 0. Candidate binding (verified)

| Item | Required | Observed | ✓ |
|---|---|---|---|
| HEAD | `8cdafe329` | `8cdafe329ecdd23d0bb9239f1148baa90d455935` | ✓ |
| Tree | `5feddbb9ae31d9733eed276cda9902830c93164d` | identical | ✓ |
| Worktree | unmodified | `git status --porcelain` empty at start AND at end | ✓ |

`git rev-parse` re-run after every plant cycle; the candidate was never
edited persistently. Every plant was reverted with `git checkout -- <file>`
and proven byte-identical by SHA-256 (`git stash` deliberately not used — it
is repo-global and would collide with the concurrent review seats).

**Environment provisioning.** This fresh worktree had no `.oracle-npm-cache`,
`.oracle-installs` or `.oracle-checkouts` (all gitignored, not carried by
`git worktree add`). Without them the `runIf` suites SKIP rather than run —
a silent-skip trap. I provisioned by copying the npm cache and checkouts from
`verter-bf2-rev6-adv` and letting the harness realize its own installs
offline from that cache. Result: **zero unexpected prerequisite skips** —
`oracle-install-realization.spec.mjs` runs 3/3 (not `2 skipped`), and the
`cacheReady`-gated arms execute. All three directories are gitignored;
`git status` stayed clean throughout.

---

## 1. Item 3 — the mandatory plant-red-green checklist (all 4 cycles, executed independently)

I did not rely on `verter-bf2-fix2/.agent-run/BF2-PASS7-FIX-REPORT.md`; I did
not read its cycle results before running mine. Each plant below was proven
present, unique, and *new* before the RED run.

### Cycle 1 — B1: memo no longer bypasses the gates

**Plant** (`src/oracle-install.mjs:558-562` → one line):

```diff
 export function ensureOracleDomain(framework) {
   const entry = frameworkEntry(framework);
   const memo = ensured.get(framework);
-  if (memo !== undefined) {
-    assertRecordedContentIntact(entry, framework, memo.installDir);
-    assertOracleEntrypointsResolvable(entry, framework, memo.installDir);
-    return memo;
-  }
+  if (memo !== undefined) return memo; // BF2_ADV7_PLANT_B1: pre-fix unconditional early return, NO gate calls
```

**Plant proven applied and unique:** `grep -c BF2_ADV7_PLANT_B1` = `1`;
`git diff --stat` = `1 insertion(+), 5 deletions(-)`; the printed memo branch
shows zero gate calls remaining.

**RED** — `npx vitest run --root . test/closure-drift.spec.mjs -t "SAME process"`:

```
 FAIL  test/closure-drift.spec.mjs > … > a payload mutated AFTER a successful load in the
       SAME process is REFUSED on the next load (memoization does not bypass the gates)
AssertionError: expected null to be an instance of PackageDriftError
 ❯ test/closure-drift.spec.mjs:660:28
      Tests  1 failed | 17 skipped (18)
```

`requireError === null` — the mutated compiler loaded with no refusal, i.e.
exactly the pre-fix behaviour.

**Revert proven:** `git diff --exit-code` clean, `grep -c` = 0, SHA-256
`2617277af547fd78990e7a87466983abbd409c90471c32609042f5458e069f21`
(identical to the pre-plant capture).

**GREEN — full file:** `Tests 18 passed (18)` — the pass-6 cold-load poison
and torn-tree tests still pass unchanged. ✅ **discriminating**

### Cycle 2 — B2: the entry gate resolves the ACTUAL Svelte load specifier

**Plant** (`src/oracle-install.mjs:423`):

```diff
-  for (const row of entry.domain.oracleLoadSpecifiers) {
+  for (const row of []) { // BF2_ADV7_PLANT_B2: loop emptied — only the pre-fix directPackages root loop runs
```

**Plant proven:** `grep -c` = 1; after the plant the only surviving
`oracleLoadSpecifiers` occurrence in the file is a doc comment (line 385) —
the loop body is genuinely dead.

**RED** — `-t "torn Svelte tree"`:

```
AssertionError: expected 'node:internal/modules/run_main:123\n …' to contain 'PackageDriftError'
+ Error [ERR_MODULE_NOT_FOUND]: Cannot find module
+   '…/svelte/node_modules/svelte/src/compiler/index.js'
+   imported from …/svelte/.bf2-loader/ns-745274d4eb9f0364.mjs
+   code: 'ERR_MODULE_NOT_FOUND'
      Tests  1 failed | 17 skipped (18)
```

This is precisely the checklist's predicted RED **and** round-6 B2's exact
signature: the gate accepts the torn tree and the child dies raw in the
loader instead of refusing cleanly.

**Revert proven:** diff clean, `grep -c` = 0, SHA-256 back to `2617277a…`.
**GREEN — full file:** `Tests 18 passed (18)`; the Vue torn-tree case (covered
by the kept `directPackages` loop) unaffected. ✅ **discriminating**

### Cycle 3 — B3: Svelte reuse signal covers all initial server child nodes

**Independent `hydrateVue` byte-identity check (checklist demands I not trust
the report).** I extracted the `hydrateVue` function body by brace-matching
from both trees and hashed it:

```
pass6 (19cce22c8) hydrateVue: 1768 bytes  sha256 bd073d585d876d4e278460a93da824e88e47b35ea8713675d35d0a65460a130d
cand  (8cdafe329) hydrateVue: 1768 bytes  sha256 bd073d585d876d4e278460a93da824e88e47b35ea8713675d35d0a65460a130d
BYTE-IDENTICAL: True
```

Confirmed — the Vue path is untouched; every hunk in `hydration.mjs` lands in
the doc comment and the Svelte runner source.

**Plant** (`src/hydration.mjs:189`):

```diff
-const initialServerNodes = [...container.childNodes];
+const initialServerNodes = [...container.childNodes].filter((n) => n.nodeType === 1); // BF2_ADV7_PLANT_B3: element-only filter restored
```

**RED** — `-t "TEXT-ROOT"`:

```
AssertionError: expected false to be true // Object.is equality
 ❯ test/hydration.spec.mjs:197:35
    197|     expect(markerless.mismatched).toBe(true);
      Tests  1 failed | 5 skipped (6)
```

`mismatched: false` where `true` is required — the checklist's predicted RED.

**Revert proven:** diff clean, `grep -c` = 0, SHA-256
`9b2484565cf78f4e52746f585ee1433f6b8f1b53dff5ff7687fb2ae19d4836fb`.
**GREEN — FULL `test/hydration.spec.mjs`:** `Tests 6 passed (6)` — both Vue
tests and all existing Svelte element-root tests (wrong-tag-under-markers,
markerless `<div>`, positive control) pass. ✅ **discriminating**

### Cycle 4 — AF-4: the lock-exclusion test is now deterministic

*(This is the item my seat owes extra rigor on — I filed round-6 A6-1.)*

**Plant** (`src/oracle-install.mjs:476`, the same shape as my round-6 plant):

```diff
 function acquireRealizeLock(framework) {
   mkdirSync(ORACLE_INSTALLS_ROOT, { recursive: true });
   const lockPath = path.join(ORACLE_INSTALLS_ROOT, `${framework}.lock`);
+  return lockPath; // BF2_ADV7_PLANT_AF4: unconditional bypass — mkdir test-and-set exclusion removed
+  // eslint-disable-next-line no-unreachable
   const deadline = Date.now() + REALIZE_LOCK_TIMEOUT_MS;
```

**Plant proven:** `grep -c` = 1, and I printed the whole rewritten function to
prove the `mkdirSync(lockPath)` test-and-set sits *after* the unconditional
return and is unreachable. Unconditional (not env-gated) so it cannot fail to
apply in a spawned child.

**All planted runs reported individually** (`-t "HELD realization lock"`):

| Run | Result | Test duration |
|---|---|---|
| 1 | **RED** `AssertionError: expected true to be false` | 10009 ms |
| 2 | **RED** same | 10018 ms |
| 3 | **RED** same | 10014 ms |
| 4 | **RED** same | 10014 ms |
| 5 | **RED** same | 10025 ms |
| 6 | **RED** (detail capture) | — |
| 7 | **RED** (detail capture) | — |
| 8 | **RED** | 10018 ms |
| 9 | **RED** | 10015 ms |
| 10 | **RED** | 10032 ms |

**10 / 10 RED.** The checklist required 5; I ran 10 because this is the item
my round-6 finding was about. Failing assertion, captured in full:

```
AssertionError: expected true to be false // Object.is equality
 ❯ test/oracle-install-realization.spec.mjs:180:24
    180|       expect(finished).toBe(false);
```

`finished === true` — the realizer ran to completion despite the held lock.

**Control arm (unmutated), 5 runs — all GREEN:**

```
CONTROL RUN 1:  Tests  1 passed | 2 skipped (3)
CONTROL RUN 2:  Tests  1 passed | 2 skipped (3)
CONTROL RUN 3:  Tests  1 passed | 2 skipped (3)
CONTROL RUN 4:  Tests  1 passed | 2 skipped (3)
CONTROL RUN 5:  Tests  1 passed | 2 skipped (3)
```

**Revert proven:** diff clean, `grep -c` = 0, SHA-256 back to `2617277a…`.
**GREEN — full file:** `Tests 3 passed (3)` (the kept 2-racer happy path and
the offline fail-closed test both ran — not skipped).

**A6-1 is CLOSED.** Round 6 measured 12 RED / 15 (3 green misses, ~20% miss
rate). Round 7 measures **10 RED / 10 planted, 5 GREEN / 5 control**. The
replacement is a genuine schedule, not a race: the test holds the lock itself
and asserts non-progress over a 10 s window, so the discriminator does not
depend on timing luck. This is the strongest of the four fixes. ✅

---

## 2. Item 1 — rows 1 and 4, re-verified against the round-6 findings

Per the mandatory method I reproduced each round-6 counterexample *myself*
against `8cdafe329`, rather than only checking that the committed tests pass.

### B1 — "content and torn-tree gates are bypassed after the first same-process validation"

> Round 6, verbatim: *"`assertRecordedContentIntact` … and
> `assertOracleEntrypointsResolvable` … Both calls occur before the loader only
> on the non-memo path… The function instead checks the process-global
> `ensured` map first and returns at `:434-436`."*

**Fix cited:** `src/oracle-install.mjs:555-562` — the memo branch now calls
both live gates before returning `memo`; `entry` is hoisted above the memo
lookup so it is available to them.

**My own probe** (same shape as the round-6 architecture probe: private copy
of the validated Svelte install + its unchanged manifest, prime, append to
`node_modules/svelte/src/compiler/index.js`, reload through the real
production loaders in the same process):

```
primed, compiler VERSION = 5.56.8
payloadChanged: true | contentManifestUnchanged: true
importOracleModule: REFUSED layer=realized-content-drift
ensureOracleDomain:  REFUSED layer=realized-content-drift
oracleScratchDir:    REFUSED layer=realized-content-drift
```

Round 6 got `BF2_R6_MEMO_BYPASS_EXECUTED` printed and `load.ok: true`. I get a
refusal on **all three** public entry points and **no execution of the
poisoned payload**. The quantifier holds because `oracleRequire`,
`importOracleModule`, `oracleScratchDir` and `oracleLinkBaseDir` are the
complete set of exported load routes (grep-verified) and every one calls
`ensureOracleDomain` on its first line — notably `importOracleModule` calls it
*before* consulting its own `importedNamespaces` memo, so a warm namespace
cannot skip the gate. **CLOSED.**

### B2 — "the torn-tree gate validates `svelte`, while production loads `svelte/compiler`"

> Round 6, verbatim: *"`assertOracleEntrypointsResolvable` loops
> `Object.keys(entry.domain.directPackages)` … Resolving the package root
> proves `src/index-server.js` exists; it does not prove
> `src/compiler/index.js` exists."*

**Fix cited:** `domain-pin.mjs:59-64` / `:85-95` add `oracleLoadSpecifiers`;
`oracle-install.mjs:423-448` resolves each row under its caller's loader
semantics, with `esmImportTargetFile` (`:344-380`) walking the package's own
exports map under `{node, import, …extraConditions}` in declaration order —
which is Node's own `PACKAGE_TARGET_RESOLVE` order — plus a `statSync`
`isFile()` existence check.

**Caller-inventory check (the "every" quantifier).** Round 6's required
correction was that the inventory be *derived from all production oracle
loader callsites*. I enumerated them independently:

| Callsite | Specifier / loader | In inventory? |
|---|---|---|
| `invoke-vue-oracle.mjs:57` | `oracleRequire("vue", "@vue/compiler-sfc")` | ✓ require |
| `execute-vue-runtime.mjs:43` | `import "vue"` | ✓ import |
| `execute-vue-runtime.mjs:44` | `import "@vue/server-renderer"` | ✓ import |
| `hydration.mjs:89` (`hydrateVue`) | `import "vue"` | ✓ import |
| `invoke-svelte-oracle.mjs:30` | `import "svelte/compiler"` | ✓ import |
| `execute-svelte-runtime.mjs:38` | `import "svelte/server"` | ✓ import |
| `hydration.mjs:164` (runner, `--conditions=browser`) | `import "svelte"` | ✓ import + `browser` |

**Complete — 7 callsites, 6 rows, exact cover.** The `browser` extra
condition is correctly attached to the one row that needs it.

**My own probe** (round-6's exact counterexample: remove only
`src/compiler`, keep the package root entry and every manifest, no content
manifest):

```
defaultEntryPresent(root src/index-server.js): true
pkg manifest intact: true
compilerEntryPresent: false | contentManifestPresent: false
gate: REFUSED layer=oracle-entry-unresolvable subject=svelte/compiler
torn tree recorded as new baseline: false
```

Round 6 got `gate: accepted=true` **and** `contentManifestRecordedByAcceptedGate:
true`. Both halves are now fixed — it refuses, and it does not poison the
baseline. **CLOSED** (see observation O-A7-1 for a same-class residual).

### B3 — "text-only Svelte roots can lose hydration markers and be reported clean"

> Round 6, verbatim: *"With a text-only root, `serverElements.length === 0`
> makes reuse true by definition. Hydration markers are comments, so the
> structural comparison cannot observe that they were missing."*

**Fix cited:** `src/hydration.mjs:189` — `initialServerNodes =
[...container.childNodes]` (no filter), consumed at `:196`. The comment-erasing
`serialize` at `:204` is unchanged but now *documented* as deliberate, with
marker survival explicitly reassigned to the reuse signal.

**My own probe** (round-6's exact component and inputs, plus two extra arms):

```
officialSsr: "<!--[--><!---->hello<!--]-->"
control (official marked)                            -> ok=true mismatched=false
markerless wrong text                                -> ok=true mismatched=true
torn markers                                         -> ok=true mismatched=true
intact-marker dynamic-only (settled O-4 limitation)  -> ok=true mismatched=false
```

Round 6 got `mismatched: false` for the markerless arm. Both marker-loss arms
now report `true`, and the positive control stays `false` — the widened signal
is not trigger-happy. **CLOSED.**

**On the two recorded limitations** (common doc asks me to speak explicitly):
- The **dynamic-text-only** limitation (row 4 of my probe) is **unchanged in
  scope** — intact markers, only the text value differs, still `false`. Pass 7
  did not widen or narrow it. Not a finding.
- The **locally-recorded-manifest** limitation is likewise unchanged. My B1 and
  B2 probes both avoid relying on it (B1 left the manifest untouched and
  verified so by digest; B2 had no manifest at all). Not a finding.

### Row disposition

| Row | Round 6 | Round 7 | Basis |
|---|---|---|---|
| 1 | BLOCKING (B1,B2,B3,A6-1) | **BLOCKING (A7-1 only)** | all four causes closed and independently reproduced; blocked solely on the new test's non-determinism |
| 4 | BLOCKING (B1,B2) | **BLOCKING (A7-1 only)** | B1/B2 closed; same test-determinism defect reaches row 4's designated suite via the shared canonical gate |

---

## 3. Item 2 — full-suite regression check → **FAILS**

Required: `pnpm --filter @verter/framework-conformance-harness test`,
**expect 226/226**.

**8 runs of the canonical invocation on the unmutated candidate:**

| Run | Result |
|---|---|
| baseline | **FAIL** — 225 passed, 1 failed |
| 1 | PASS 226/226 |
| 2 | **FAIL** — 224 passed, 2 failed |
| 3 | PASS 226/226 |
| 4 | **FAIL** — 224 passed, 2 failed |
| 5 | PASS 226/226 |
| 6 | PASS 226/226 |
| 7 | **FAIL** — 225 passed, 1 failed |

**4 FAIL / 8 (50%).** Every failure is the same test, always the same cause.

---

## 4. Findings

| ID | Rows | Severity | Finding |
|---|---|---|---|
| **A7-1** | 1, 4 | **BLOCKING** | Pass 7's own new B3 regression test has no explicit timeout and fails ~50% of runs of the canonical suite invocation by exceeding vitest's 5000 ms default — the exact failure mode pass 7 diagnosed and fixed in two other files in the same commit. |
| O-A7-1 | 1, 4 | Non-blocking observation | `oracleLoadSpecifiers` covers direct loader callsites but not the specifiers *compiled oracle output* imports (`svelte/internal/client`, `svelte/internal/server`); tearing those reproduces B2's signature exactly. |
| O-A7-2 | — | Non-blocking observation | The memo branch re-runs the two live tree gates but not `assertEvidenceStaticPinned`. |

### A7-1 — the designated B3 test is non-deterministic under the canonical gate (BLOCKING)

**What fails.** `test/hydration.spec.mjs:148` — *"reports mismatched: true for
a TEXT-ROOT component hydrated onto markerless wrong server text (negative
control)"*, the designated regression test for B3:

```
 FAIL  test/hydration.spec.mjs > hydrateSvelteClient — official server / official client
       (pairing #1) > reports mismatched: true for a TEXT-ROOT component hydrated onto
       markerless wrong server text (negative control)
Error: Test timed out in 5000ms.
If this is a long-running test, pass a timeout value as the last argument …
 ❯ test/hydration.spec.mjs:148:5
      Tests  1 failed | 225 passed (226)
```

**Root cause — measured, not inferred.** The test is `async` and takes no
timeout argument, so it inherits vitest's 5000 ms default. It performs two
`compileSvelteFixture` calls, one `executeSvelteSsr`, and three
`hydrateSvelteClient` calls — six real child-process spawns. Measured
durations:

| Condition | Test duration |
|---|---|
| isolated (`-t "TEXT-ROOT"`) | **1.02 s** |
| full suite, run A | 6.605 s → timeout |
| full suite, run B | 10.312 s → timeout |
| full suite, run C | 9.102 s → timeout |

Under full-suite parallel-worker contention the test inflates 6–10× and
crosses the 5 s default. In isolation it passed 5/5 (`adv7-hyd-isolation.txt`)
— so this reproduces only under the invocation the review is *required* to
use, which is precisely why a per-file spot check would miss it.

**Why this is blocking rather than an observation.**

1. **It fails mandated item 2 directly.** The candidate does not deliver
   226/226; it delivers it 4 times in 8.
2. **Pass 7 diagnosed this exact failure mode and fixed it everywhere else in
   the same commit.** `test/golden-provenance.spec.mjs` got two `60_000`
   timeouts with the comment *"spawns a full `--check` child; the 5s default
   flakes under parallel worker contention"*, and
   `test/offline-execution.spec.mjs` got the identical treatment. The new
   hydration test — which spawns *six* children, more than either — was left
   on the default. No test in `hydration.spec.mjs` carries an explicit
   timeout, so this is an omission, not a considered choice.
3. **It is the same defect class as round-6 A6-1, which blocked this row.**
   A6-1 was "the designated test is timing-dependent and gives the wrong
   answer 3 times in 15." A7-1 is "the designated test is timing-dependent and
   gives the wrong answer 4 times in 8." The direction differs (false RED
   rather than false GREEN) but the property violated is identical: the
   harness's own regression discriminator must not depend on machine timing.
   Passing this while having blocked A6-1 would be inconsistent.
4. **The consequence is not cosmetic.** This harness is the falsification
   oracle for compiler conformance. A gate that red-flags at random trains
   maintainers to re-run until green — which is exactly how a *real* B3
   regression would be waved through.

**Under the mandatory method:** *"No 'PASS with caveat' is permitted."* The
required behaviour (the suite passes) is violated on observed runs, so this is
BLOCKING FINDINGS, not NOT PROVEN.

**Remedy (not applied — I did not edit the candidate).** Give the test an
explicit timeout in the same style and with the same rationale comment as the
two files pass 7 already fixed, e.g. `}, 60_000); // six real child spawns;
the 5s default flakes under parallel worker contention`. The three
pre-existing Svelte tests in the file spawn children too and should be
audited in the same pass. Mechanism-wise nothing changes — cycle 3 proves the
mechanism itself is correct and discriminating.

### O-A7-1 — the load-specifier inventory stops at direct callsites (non-blocking)

Round-6 B2's required correction was an inventory *derived from all production
oracle loader callsites*. That is delivered exactly (§2, 7/7 cover). But
compiled oracle **output** also imports from inside the install tree, and
those specifiers are not in the inventory. Measured against the real pinned
compiler:

```
client artifact imports: ["svelte/internal/client"]
server artifact imports: ["svelte/internal/server"]
```

Neither is an `oracleLoadSpecifiers` row. Tearing the ESM target of
`./internal/client` (exports map: `{"default":"./src/internal/client/index.js"}`)
while keeping every manifest, with no content manifest, reproduces B2's
signature exactly:

```
target dir present before: true → after: false
GATE VERDICT: ACCEPTED
content manifest recorded by the accepted gate: true
```

and the downstream consequence is the raw loader death the committed Svelte
test explicitly asserts must not happen
(`expect(result.stderr).not.toContain("ERR_MODULE_NOT_FOUND")`):

```
compile ok (compiler subtree intact): true
hydrate ok: false | mismatched: false
Error [ERR_MODULE_NOT_FOUND]: Cannot find module
  '…/svelte/node_modules/svelte/src/internal/client/runtime.js'
  imported from …/svelte/node_modules/svelte/src/index-client.js
```

Note the gate *did* check the `svelte` row and stat'd `src/index-client.js`
successfully — the check is a single-file existence test, not transitive, so
it cannot see that the file's own imports are gone.

**Why non-blocking.** B2's literal required correction is satisfied; this is a
narrower residual at a different specifier class. It is also masked in the
steady state by the layer-4 content-drift gate, which catches any post-
realization tear whenever a manifest exists — the exposure needs the same
no-manifest premise the committed test constructs. I record it rather than
charge it, and recommend either adding the `internal/*` rows or making the
existence check follow the target's static imports one level.

### O-A7-2 — the memo branch does not re-run the static evidence layers (non-blocking)

`ensureOracleDomain`'s memo branch re-runs `assertRecordedContentIntact` and
`assertOracleEntrypointsResolvable`, but not `assertEvidenceStaticPinned`.
Committed-evidence tampering after a prime is therefore not re-detected in the
same process. Round-6 B1 was scoped to the *realized tree*, and the committed
evidence is in-repo rather than in the install tree, so this is outside what
was charged. Recorded for completeness only.

---

## 5. Evidence index

All raw captures are committed alongside this report in `.agent-run/`:

| File | Contents |
|---|---|
| `adv7-full-baseline.txt` | first canonical full-suite run (the 225/226 failure, with the timeout stack) |
| `adv7-full-repeat.txt` | full-suite runs 1–4 (2 PASS, 2 FAIL) |
| `adv7-hyd-isolation.txt` | 5× isolated `hydration.spec.mjs` — 6/6 every time |
| `adv7-af4-planted.txt` | AF-4 planted runs 1–5 and 8–10, all RED |

Plant/revert integrity for every cycle was proven by SHA-256:
`src/oracle-install.mjs` = `2617277af547fd78990e7a87466983abbd409c90471c32609042f5458e069f21`
and `src/hydration.mjs` = `9b2484565cf78f4e52746f585ee1433f6b8f1b53dff5ff7687fb2ae19d4836fb`
before each plant and after each revert. Final `git status --porcelain` empty;
final `git rev-parse HEAD` = `8cdafe329ecdd23d0bb9239f1148baa90d455935`.

---

## 6. Summary

| Round-6 cause | Plant cycle | My independent probe | Status |
|---|---|---|---|
| B1 memo-bypass | RED → GREEN 18/18 | refuses on all 3 entry points, payload never executed | **CLOSED** |
| B2 Svelte subpath | RED (`ERR_MODULE_NOT_FOUND`) → GREEN 18/18 | refuses `svelte/compiler`, no baseline poisoning | **CLOSED** |
| B3 marker loss | RED (`false`≠`true`) → GREEN 6/6 | markerless + torn both `true`, control `false` | **CLOSED** |
| A6-1 / AF-4 flaky test | **10/10 RED**, 5/5 control GREEN | — | **CLOSED** |

Four for four on mechanism. The block is on the harness's own determinism:
**A7-1** — the new B3 test fails 4 runs in 8 of the canonical suite for want of
the explicit timeout pass 7 added to every other slow test it touched. The
remedy is one argument and one comment; the finding is filed at BLOCKING
because item 2's 226/226 requirement is observably not met, and because
passing a ~50%-flaky designated regression test would contradict the round-6
ruling this very seat obtained.
