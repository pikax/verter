VERDICT: PASS

# BF2 Revision-11 conformance harness — architecture review, round 7

## Executive result

Candidate `8cdafe329` closes all four findings in this targeted re-review. I independently re-ran my three round-6 counterexamples against the production loaders/runtime on the new candidate:

- the same-process Svelte compiler poison is now refused with `PackageDriftError` / `realized-content-drift` before the poisoned module evaluates;
- the Svelte ESM `svelte/compiler` subpath tear is now refused with `PackageDriftError` / `oracle-entry-unresolvable`, while the package root, CJS compiler entry, and all 22 package manifests remain present and no content manifest exists or is recorded; and
- the `{label}` text-root component hydrated onto `"WRONG MARKERLESS TEXT"` now returns `mismatched: true`, while the official marked control remains false.

The round-6 AF-4/A6-1 test-adequacy finding is also closed. The new held-lock schedule passes unmutated and, with `acquireRealizeLock` unconditionally bypassed in a disposable exact-candidate checkout, fails at the designated held-lock assertion in 5/5 runs.

The source diff introduces no compiler/runtime substitute or second production semantic engine. It changes only `domain-pin.mjs`, `oracle-install.mjs`, and the existing Svelte hydration observer. The new exports-map walker is a fail-closed, resolution-only preflight over the immutable pinned package shapes; it never returns a module namespace or supplies linking behavior. Actual CJS/ESM loading remains Node's existing loader path. I separately compared all six declared load rows to real `require.resolve` / `import.meta.resolve` results under their actual conditions; all six select the files the gate checks.

The exact package suite passes 226/226. Rows 1 and 4 are PASS. The other 14 settled rows show no regression.

## 1. Candidate binding, scope, and authority

Binding was verified, not inferred:

| Item | Required | Observed |
|---|---|---|
| Baseline | `19cce22c8` | exact |
| Candidate HEAD | `8cdafe329` | `8cdafe329ecdd23d0bb9239f1148baa90d455935` |
| Candidate tree | `5feddbb9ae31d9733eed276cda9902830c93164d` | exact |
| BF2 charter SHA-256 | `1f99cf7eda1a955ada751f075799dabc8c8ab1defda19b20375f7ca09aa5963b` | exact |

`git diff --check 19cce22c8..HEAD` is clean. Across the full range, all 153 changed paths are under `packages/framework-conformance-harness/`; zero paths are outside the package and zero are Rust files. The required production-source diff:

```text
git diff 19cce22c8..HEAD -- packages/framework-conformance-harness/src/

packages/framework-conformance-harness/src/domain-pin.mjs
packages/framework-conformance-harness/src/hydration.mjs
packages/framework-conformance-harness/src/oracle-install.mjs
```

The larger path count is regenerated golden/provenance data plus package-local tests and the optional package-local timeout fix; production behavior outside the harness is untouched.

Governing authority applied:

- BF2 objective: “Build hermetic test-only infrastructure that can falsify framework output against the exact official domains without supplying production behavior.”
- BF2 required exit: harness self-tests must “prove source/package drift refusal”.
- Official-core oracle contract: “The harness rejects any source SHA/tree, package version, integrity, or transitive closure mismatch before generating expectations or running candidate output.”
- Hydration contract: “Assertions cover initial DOM/markers, successful hydration without replacement” and “A hydration mismatch is semantic and cannot be normalized away.”
- Repository architecture: one shared engine per concern; test-only infrastructure must not supply production semantics.
- Mandatory review method: a green test name is insufficient; each changed decision needs a unique, applied white-box kill that drives its designated test RED.

## 2. B1 — same-process memo bypass: CLOSED

### Prior finding, verbatim

> “The content and torn-tree gates are bypassed after the first same-process validation.”

> “`ensureOracleDomain` memoizes a successful validation and returns the memo before either new pass-6 gate. In one process, mutate the installed compiler after the first validation and a later production `importOracleModule` evaluates that changed compiler. This directly violates Fix 1's ‘both checks must run and throw BEFORE any oracle compiler code’ requirement.”

### Fix and complete path inventory

At `src/oracle-install.mjs:555-618`, `ensureOracleDomain` now has exactly two successful returns:

1. memo return at lines 558-561, preceded by both `assertRecordedContentIntact` and `assertOracleEntrypointsResolvable` against `memo.installDir`;
2. cold return at line 618, preceded by the content gate at line 579 and the entry gate at line 597.

There is no third successful exit. The surrounding paths are also ordered correctly:

- `oracleRequire` calls `ensureOracleDomain` at line 630 before consulting/using its cached `createRequire`;
- `importOracleModule` calls it at line 653 before its namespace memo return at line 656;
- `oracleScratchDir` calls it at line 675 before creating the scratch directory;
- `oracleLinkBaseDir` returns only `ensureOracleDomain(...).installDir` at line 683;
- direct generator/provenance callers call `ensureOracleDomain` itself and therefore take one of the two audited returns.

Thus the namespace/module caches cannot bypass the live checks: their cache decisions occur only after `ensureOracleDomain` returns. The memo still owns only the expensive realization/full-closure/digest result; it is not a validity oracle for the live tree.

### Exact committed test and why it proves the criterion

`test/closure-drift.spec.mjs:614-675`, “a payload mutated AFTER a successful load in the SAME process is REFUSED on the next load”, imports the real `src/oracle-install.mjs` once with a cache-busting URL solely to bind a private installs root and a fresh module-local memo. In that same Vitest process it:

1. successfully loads the real Vue compiler through `oracleRequire` and Vue through `importOracleModule`;
2. proves the content manifest is armed;
3. mutates the actual compiler payload with package metadata and manifest untouched;
4. calls both production loader APIs again in that same module instance; and
5. requires both to throw the real `PackageDriftError` with layer `realized-content-drift`.

This directly covers “after a successful load”, “same process”, “next load”, both exported loader forms, and refusal by the content gate rather than by a downstream raw module error.

The three targeted files passed together:

```text
pnpm --filter @verter/framework-conformance-harness exec vitest run \
  test/closure-drift.spec.mjs test/hydration.spec.mjs \
  test/oracle-install-realization.spec.mjs --fileParallelism=false

Test Files  3 passed (3)
Tests       27 passed (27)
```

### Independent re-attempt of my round-6 probe

Full source is stored as `.agent-run/review7-evidence/probe-b1-same-process.mjs`. Its critical sequence is:

```js
const first = ensureOracleDomain("svelte");
appendFileSync(
  join(first.installDir, "node_modules/svelte/src/compiler/index.js"),
  '\nconsole.log("BF2_R7_MEMO_POISON_EVALUATED");\n',
);
const loaded = await importOracleModule("svelte", "svelte/compiler");
```

Observed result against `8cdafe329`:

```json
{
  "primed": true,
  "payloadChanged": true,
  "contentManifestPresent": true,
  "contentManifestUnchanged": true,
  "load": {
    "ok": false,
    "name": "PackageDriftError",
    "layer": "realized-content-drift"
  }
}
```

The poison's evaluation marker never printed. This is the opposite of round 6, where the same shape printed the marker and returned Svelte version 5.56.8.

### White-box kill

In a detached worktree at exact HEAD, I uniquely planted the pre-fix behavior by deleting both memo-branch gate calls and leaving the unconditional `return memo`. The marker occurred once and `git diff --check` passed. The designated test went RED:

```text
AssertionError: expected null to be an instance of PackageDriftError
Test Files  1 failed (1)
Tests       1 failed | 17 skipped (18)
```

After restoring an empty source diff, the four designated tests passed together. B1 is closed.

## 3. B2 — Svelte compiler subpath tear: CLOSED

### Prior finding, verbatim

> “The torn-tree gate validates `svelte`, while production loads `svelte/compiler`.”

> “The torn-tree check resolves each direct package name, not each oracle entry actually loaded. Svelte's package-root entry remains resolvable after `svelte/compiler` is deleted, so the gate accepts and records an installation that its production loader cannot load.”

### Fix, inventory, and architecture

`src/domain-pin.mjs:46-63,78-93` adds one frozen `oracleLoadSpecifiers` registry containing the current production load inventory and loader semantics:

| Domain | Specifier | Semantics |
|---|---|---|
| Vue | `@vue/compiler-sfc` | require |
| Vue | `vue` | import |
| Vue | `@vue/server-renderer` | import |
| Svelte | `svelte/compiler` | import |
| Svelte | `svelte/server` | import |
| Svelte | `svelte` | import + `browser` condition |

I inventoried every literal `oracleRequire` / `importOracleModule` production callsite plus the Svelte hydration runner's bare browser-conditioned root import. The registry matches them all; duplicate Vue-root use is intentionally one row. No production load specifier is omitted.

`src/oracle-install.mjs:312-375` resolves exact-subpath export targets under the immutable pinned manifests, and `assertOracleEntrypointsResolvable` at lines 400-447 retains the broad direct-package root loop and additionally checks every registry row. Require rows use Node's `req.resolve`. Import rows are a resolution-only, fail-closed preflight: missing manifest, missing exact subpath, unmatched condition branch, missing target, or non-file target refuses. No oracle code is evaluated by the gate.

This does not introduce a second loader or runtime. The preflight never supplies a resolved module to production; `oracleRequire` still invokes Node `require`, and `importOracleModule` still writes an importer inside the isolated install and invokes Node ESM import. Unsupported future exports shapes refuse rather than emulate. For the exact immutable current domains, independent real-Node parity produced:

```text
@vue/compiler-sfc require     -> @vue/compiler-sfc/dist/compiler-sfc.cjs.js
vue import                    -> vue/index.mjs
@vue/server-renderer import   -> @vue/server-renderer/index.js
svelte/compiler import        -> svelte/src/compiler/index.js
svelte/server import          -> svelte/src/server/index.js
svelte import + browser       -> svelte/src/index-client.js
```

All six real targets matched the pinned target selected by the preflight and existed as files.

### Exact committed test and why it proves the criterion

`test/closure-drift.spec.mjs:561-612` primes a private Svelte install, then deletes only `node_modules/svelte/src/compiler`, leaving:

- `svelte/package.json` present;
- the package-root server entry present;
- the divergent CJS compiler bundle present;
- every manifest intact; and
- no content manifest.

It then drives the production `compileSvelteFixture` path and requires a non-zero process, `PackageDriftError`, layer `oracle-entry-unresolvable`, no `ERR_MODULE_NOT_FOUND`, no compiler artifact marker, and no newly-recorded content manifest. These assertions distinguish gate refusal from both the prior false acceptance/re-baselining and a raw downstream loader crash.

### Independent re-attempt of my round-6 probe

Full source is `.agent-run/review7-evidence/probe-b2-svelte-subpath.mjs`. It copied the valid pinned install, removed only `src/compiler`, counted manifests before/after, invoked `ensureOracleDomain`, then invoked the real `importOracleModule` path:

```json
{
  "plant": {
    "compilerEntryPresent": false,
    "packageRootEntryPresent": true,
    "cjsCompilerEntryPresent": true,
    "packageManifestsBefore": 22,
    "packageManifestsAfter": 22,
    "contentManifestPresentBeforeGate": false
  },
  "gate": {
    "accepted": false,
    "name": "PackageDriftError",
    "layer": "oracle-entry-unresolvable",
    "rawModuleNotFound": false
  },
  "productionLoad": {
    "accepted": false,
    "name": "PackageDriftError",
    "layer": "oracle-entry-unresolvable",
    "rawModuleNotFound": false
  },
  "contentManifestRecorded": false
}
```

### White-box kill

I uniquely replaced the actual-load loop with an empty iterable, leaving only the pre-fix direct-package root loop. The Svelte tear test went RED because the gate accepted and the real loader emitted raw `ERR_MODULE_NOT_FOUND`:

```text
AssertionError: expected ... to contain 'PackageDriftError'
Received: Error [ERR_MODULE_NOT_FOUND]: Cannot find module .../svelte/src/compiler/index.js
```

Restoring the exact source returned the designated test to green. B2 is closed.

## 4. B3 — markerless text-root hydration: CLOSED

### Prior finding, verbatim

> “Text-only Svelte roots can lose hydration markers and be reported clean.”

> “Svelte hydration's new structural comparison erases comments and treats a text-only server root as having vacuously reused its server nodes. Markerless, wrong text is discarded and rebuilt, but the harness reports `mismatched: false`. This is a structural hydration-marker loss, distinct from the explicitly accepted dynamic-text-only limitation.”

### Fix and architecture

`src/hydration.mjs:185-196` now snapshots every initial server-root child node, including text and comment nodes, rather than filtering to elements. The existing reuse decision therefore becomes non-vacuous for a non-empty text/comment-only root. A genuinely empty root remains the only vacuous case.

The comment-erasing fresh-render serialization remains intentionally unchanged: a fresh client mount lacks SSR boundary markers that correct hydration retains, so including comments in that independent comparison would make the official marked positive control false-positive. Marker survival is instead observed through the widened node-identity signal. This extends the existing three-signal observer; it does not create a DOM parser, hydration implementation, or runtime substitute. The actual client remains the pinned Svelte compiler/runtime output.

### Exact committed test and why it proves the criterion

`test/hydration.spec.mjs:148-211` compiles the exact text-root source `<script>let { label } = $props();</script>{label}` for both server and client. It first proves the official marker-bearing render hydrates with `mismatched: false`; it then requires `mismatched: true` for:

1. the exact markerless wrong-text class from B3; and
2. a torn opening-boundary-only input.

The test proves the markerless plant contains no comment marker, uses real pinned compilation, observes successful Svelte recovery (`ok: true`), and distinguishes that recovery from clean hydration.

### Independent re-attempt of my round-6 probe

Full source is `.agent-run/review7-evidence/probe-b3-text-root-hydration.mjs`. Result:

```json
{
  "officialSsr": "<!--[--><!---->hello<!--]-->",
  "control": {
    "ok": true,
    "mismatched": false,
    "finalHtml": "<!--[--><!---->hello<!--]-->"
  },
  "markerless": {
    "input": "WRONG MARKERLESS TEXT",
    "inputHadHydrationMarkers": false,
    "result": {
      "ok": true,
      "mismatched": true,
      "finalHtml": "hello"
    }
  },
  "dynamicOnly": {
    "input": "<!--[--><!---->WRONG DYNAMIC TEXT<!--]-->",
    "result": {
      "ok": true,
      "mismatched": false,
      "finalHtml": "<!--[--><!---->hello<!--]-->"
    }
  }
}
```

I also exercised opening-only, missing-close, missing-anchor, missing-open, empty-boundary, and garbled-open variants. Every marker-loss/torn variant returned `mismatched: true`; only the intact-marker dynamic-text-only case remained false, exactly matching the settled limitation.

### White-box kill

I uniquely restored the element-only filter on `initialServerNodes`. The exact new test went RED at the markerless assertion:

```text
AssertionError: expected false to be true
test/hydration.spec.mjs:197
```

After restoration, the designated test passed. B3 is closed.

## 5. A6-1 / AF-4 — racy lock test: CLOSED

### Prior findings, verbatim

Round-5 AF-4 as quoted in the round-6 adversarial report:

> “The designated test for fix 3 detects total removal of the lock in only 9/15 planted runs (~60 %); unmutated 10/10 pass.”

Round-6 A6-1:

> “Round-5 AF-4 is unaddressed, unmentioned and undispositioned in pass 6, and independently reproduces: the designated concurrency test passes 3 of 15 runs with the exclusion mechanism entirely removed.”

### Fix and test adequacy

Production `acquireRealizeLock` is unchanged. The new regression discriminator is `test/oracle-install-realization.spec.mjs:121-207`. The test itself pre-creates the exact `<installs>/svelte.lock` directory, spawns one real production `ensureOracleDomain("svelte")` child, and after a ten-second checkpoint requires:

- the child has not exited;
- no `.stage-*` exists;
- no final Svelte tree exists; and
- no digest has printed.

It then releases the lock and requires that same child to finish successfully, reproduce a fully validated lock-closure digest, and leave no lock/stage residue. The original two-racer convergence test remains only as a positive secondary check.

This schedule is deterministic in the relevant direction: a correct `mkdirSync(lockPath)` test-and-set cannot pass the held lock; a bypass can immediately create a stage/final tree or finish, all of which violate the checkpoint. The test is gated with `it.skip` when the offline cache is absent, so an unprovisioned environment cannot be reported as a pass. In my target and full-suite runs the cache was present and the test executed.

### White-box kill, five independent runs

In the detached candidate checkout I uniquely planted:

```js
const lockPath = path.join(ORACLE_INSTALLS_ROOT, `${framework}.lock`);
return lockPath; // exclusion test-and-set unreachable
```

The marker count was exactly one and the diff was clean. I ran the new held-lock test five times. All 5/5 reached the designated assertion and failed identically after the checkpoint:

```text
AssertionError: expected true to be false
test/oracle-install-realization.spec.mjs:180

AF4_KILL_RED_COUNT=5/5
AF4_DESIGNATED_ASSERTION_HITS=5/5
```

After restoring an empty source diff, the four designated tests passed. AF-4/A6-1 is closed.

## 6. Architecture audit

### No memo/return bypass remains

All `ensureOracleDomain` returns and all exported consumers were inventoried in §2. The seemingly relevant namespace memo at `oracle-install.mjs:656` is downstream of the unconditional `ensureOracleDomain` call at line 653 and therefore cannot bypass either live gate. The require cache is likewise downstream. No other source file accesses `ensured`, `requires`, or `importedNamespaces`.

The cold path runs content refusal before validation/realization and entry refusal after a validated tree exists. The memo path runs both against the memo's live `installDir`. Entry refusal precedes manifest recording, so a B2-shaped torn tree with no content record cannot become a new baseline.

### No second/parallel engine

The range adds no source module and touches no Verter compiler/runtime. The three changes remain in existing owners:

- immutable domain/load metadata in `domain-pin.mjs`;
- official-install validation/loading in `oracle-install.mjs`;
- observation of pinned Svelte hydration in `hydration.mjs`.

The import-target helper is not used to link or evaluate output; it is an independent refusal preflight. Node still performs every actual module load. Its scope is the six exact immutable export rows, and real-Node resolution parity was independently checked for all six. Unknown/missing shapes fail closed.

The hydration change widens one existing identity observation from elements to all initial children. It does not recreate Svelte hydration, parse HTML textually, patch output, or synthesize markers.

### Scope correctness

Pass 7 does not alter production Verter behavior or any settled row's mechanism. The new tests are additive. AF-4 changes only test scheduling. Optional timeout edits affect only three child-process test timeouts. Regenerated goldens are expected because golden provenance binds harness source bytes.

## 7. Row dispositions

| Row | Verdict | Basis |
|---|---|---|
| 1 | PASS | B1 same-process gate ordering closes; B2 actual entry inventory closes; B3 marker-loss observation closes; AF-4 deterministic discriminator closes. Direct counterexamples now refuse/detect, all kills discriminate, and official positive controls remain clean. |
| 4 | PASS | Both live gates execute before every successful loader return; all actual current load rows are checked; Svelte compiler-subpath tear refuses without a content manifest and cannot re-baseline. |
| 2, 3, 5-16 | PASS (no regression) | Settled in round 6; exact full package suite executes and passes 226/226. |

## 8. Full regression suite

An initial invocation without the pinned source-checkout environment executed 218 tests and skipped 8; I did not count that as regression evidence. I then bound the already-provisioned exact Vue/Svelte source checkouts, offline npm cache, and a private installs root and reran the requested command:

```text
pnpm --filter @verter/framework-conformance-harness test

Test Files  19 passed (19)
Tests       226 passed (226)
```

The Node 20 process emitted unrelated workspace package engine/platform warnings before Vitest; the harness run itself completed normally with zero skips and zero failures.

## 9. Settled limitations, explicitly rechecked

Neither settled limitation changed status:

- The content manifest remains locally recorded rather than lock-anchored. A combined tree+manifest tamper can disarm that content gate specifically; the independent entry and lock-anchored layers remain. B1 did not broaden or hide this limitation.
- With intact Svelte markers and only a dynamic text value changed, Svelte repairs the text without an observable mismatch signal. My direct probe still returns `mismatched: false` for that exact intact-marker case. This remains distinct from marker loss: markerless and every additional torn/garbled marker variant I exercised now returns true.

These are the two previously recorded honest limitations, not new findings.

## 10. Evidence inventory and SHA-256

All evidence is under `.agent-run/review7-evidence/`.

| Evidence | SHA-256 |
|---|---|
| `binding-scope-callers.log` | `d4fdaeb12270b974175bb24180daab02ad7d51e0f999f34f2e509019351ee3d0` |
| `targeted-tests.log` | `edb4544c88747a89bed8c8bdfb4517410251677f1da42652cf368effe63fb2f6` |
| `full-suite.log` | `bcb9c1d2167044bfdd1d26461667e027cea50da37b3f5d6f9c14e27911a481eb` |
| `probe-b1-same-process.mjs` | `c5b27da02af351ba9608bdaa3ea8cb6b555e64f8e17b6a857ef2ca3c720f1e39` |
| `probe-b1-same-process.log` | `a788e8d19bd1d31fb04a27cf0c169f7b04e2973e47239e3a846aaae44911e627` |
| `probe-b2-svelte-subpath.mjs` | `66a625af88fd3b79a9b37516777831821fae5a8a5823fb7c5280021c6e1b3130` |
| `probe-b2-svelte-subpath.log` | `50895232ecc07a89c487e524d8727dca69ccceaaa51afb67437e741b585e18db` |
| `probe-b3-text-root-hydration.mjs` | `3c284ec1d4d8fcf591c5fb3a35587cdaa8f1aea01fac238bfb6d9c19d0cdd606` |
| `probe-b3-text-root-hydration.log` | `aa439782118d3de492fd60bce0ea20305bbd1df784e4578ce3e3d1c8a89f82b2` |
| `probe-b3-marker-variants.mjs` | `10201f2d465bd1d8d3a5759c2fc974f69c875b23d16d92cf5a2e6fa7cd37bea8` |
| `probe-b3-marker-variants.log` | `73df266e489969a0a1e68fd0602c16862d729f3e8abd3bfa198de9287885a8cb` |
| `probe-entry-resolution-parity.mjs` | `42ffd11c0d7dc5240309d5a86b721f0ac7c41b9482b9342817334bc2b65c6f58` |
| `probe-entry-resolution-parity.log` | `790296fc2ff49e607a80161eb5fb113b4456589934b4adc0e6661e6a86bced22` |
| `kill-b1-red.log` | `c4aae10f341b5d0bfa5917cb296ad654487f17897ab1f4d5d807ae392dc5d061` |
| `kill-b2-red.log` | `b5ea68f212a7843ef7c1502a62606a632f1e104054ac173d26ec4fdbd94fa528` |
| `kill-b3-red.log` | `7e1d3c37d6e4aa2734348b9bea2a4fa19c82a22cdc1d951b87f07bfc1221d005` |
| `kill-af4-plant-proof.log` | `70e70d2a6a79aa9ce58228a3c2634bdac0c60ff23e0bae662bdd6e1858bfee4b` |
| `kill-af4-five-red.log` | `3a5c26cf64ea5e10edb9d7904cb0bc38113fa47f18179725bd88792965b06b3d` |
| `kills-restored-green.log` | `c0d1299d1041da8b790bb987af5123b0efc0d53d70082ce907d2aec2e2806e17` |

Final candidate `git status --short` is empty; no candidate source was edited during review.
