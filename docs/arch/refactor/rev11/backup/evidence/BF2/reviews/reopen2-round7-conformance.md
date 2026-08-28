VERDICT: PASS

# BF2 conformance review — round 7 (targeted: rows 1 and 4 + no-regression)

## Executive result

All four round-6 blocking causes are closed on candidate `8cdafe329`, each
verified three independent ways: the designated committed test run by me, my own
reproduction of the round-6 reviewer's counterexample against the new candidate,
and a white-box kill proving the designated test is genuinely wired to the new
decision.

| Round-6 finding | Status on `8cdafe329` | Strongest evidence |
|---|---|---|
| **B1** memo bypasses both gates | **CLOSED** | My same-process poison probe now refuses at `realized-content-drift` on **all four** exported oracle entry points (round 6: poisoned compiler executed). Kill 1 turns the new test red, 17/18 others green. |
| **B2** gate resolves `svelte`, production loads `svelte/compiler` | **CLOSED** | My subpath-tear probe refuses at `oracle-entry-unresolvable` naming `svelte/compiler`, and records **no** content manifest (round 6: accepted **and** recorded). Every one of the 6 declared load specifiers proven load-bearing individually, plus an over-strictness control. |
| **B3** text-root marker loss reported clean | **CLOSED** | Round-6's exact input (`WRONG MARKERLESS TEXT` onto a `{label}` text root) now reports `mismatched: true`; torn and garbled markers too; official marked control stays `false` for both text and element roots. |
| **A6-1 / AF-4** racy exclusion test | **CLOSED** | The new deterministic test is **5/5 RED** under an unconditional `acquireRealizeLock` bypass. I reproduced the defect it replaces in the same run: the old racy test is **5 GREEN / 3 RED in 8 planted runs** on this machine. |

No regression: `pnpm --filter @verter/framework-conformance-harness test` →
**226 passed (226), 0 failed, 0 skipped**, exit 0.

Rows 1 and 4: **PASS**. Rows 2, 3, 5–16: no regression observed (settled, not
re-derived, per the round-7 scope).

I disagree with none of the two recorded limitations and file no blocking
findings. Three non-blocking observations are recorded in §6, one of which
(O-A) narrows the wording of the round-6 O-1 mitigation clause a second time and
should be carried into the ledger text rather than lost.

---

## 1. Candidate binding, scope, and environment

### 1.1 Binding — verified, exact

| Item | Required | Observed | Match |
|---|---|---|---|
| Baseline | `19cce22c8` | `19cce22c8` (used as the diff base throughout) | ✔ |
| Candidate HEAD | `8cdafe329` | `8cdafe329ecdd23d0bb9239f1148baa90d455935` | ✔ |
| Candidate tree | `5feddbb9ae31d9733eed276cda9902830c93164d` | `5feddbb9ae31d9733eed276cda9902830c93164d` | ✔ |
| Charter SHA-256 | `1f99cf7eda1a955ada751f075799dabc8c8ab1defda19b20375f7ca09aa5963b` | `1f99cf7e…5963b docs/arch/refactor/rev11/charters/BF2.md` | ✔ |

Evidence: `.agent-run/conf7-evidence/binding-scope.log`, SHA-256
`0f4ad2f6dc2141da435cf2f911b0b1b46ba072c7090f04a3adc1690e7dce957a`.

### 1.2 Scope of `19cce22c8..8cdafe329`

153 paths, **all** under `packages/framework-conformance-harness/`; paths outside
the owned package: **0**; `.rs` files: **0**. Production source touched:
`src/domain-pin.mjs` (+34), `src/hydration.mjs` (±35), `src/oracle-install.mjs`
(+171/−…). Tests touched: `closure-drift` (+137), `hydration` (+65),
`oracle-install-realization` (+109), plus two pure timeout-argument bumps
(`golden-provenance` +4, `offline-execution` — reindentation and a `60_000`
timeout, **no assertion changed**; I diffed both and confirmed every `expect`
is byte-identical). The remaining 145 paths are regenerated goldens
(`generation: 8 → 12`) — see O-C.

### 1.3 Environment and the "prove execution" preconditions

The worktree arrived with `node_modules` installed but **no** oracle provisioning.
I seeded it from the pinned, read-only round-6 review worktree — the npm cache
(`.oracle-npm-cache`, 5.5 MB) and the pinned upstream checkouts
(`.oracle-checkouts`, 58 MB) — and let the harness realize its own installs from
the cache offline. All three directories are gitignored; `git status --short` is
empty before and after every operation in this review. **I did not edit the
candidate.** All kill mutations ran in a disposable copy under
`.agent-run/kill-clone/` (built with `git archive HEAD`, `diff -r` proven
identical to the candidate `src/` before use, deleted afterwards, and
`git status` re-verified clean).

Zero unexpected skips: the final run reports **0 skipped**. This is worth
stating explicitly because my *first* run, before the pinned checkouts were
seeded, reported 8 skips (`coverage.spec.mjs` ×5, `drift-refusal.spec.mjs` ×3) —
i.e. those 8 are silently absent from any run on an unprovisioned machine. They
are `it.skip`-gated on the checkout's presence, not silently passed, which is
correct; but a reviewer who did not check the skip count would have reported a
"green" suite that never exercised source-tree drift refusal at all.

---

## 2. Method

Per the round-7 common document, rows 2, 3, 5–16 are settled and re-confirmed
only through the full-suite run (§5). Rows 1 and 4 are re-verified at the full
unchanged rigor. For each round-6 finding I record: the exact authority, the
round-6 finding verbatim, the pass-7 mechanism with file/function/line, the
designated committed test with the reason its assertions cover the criterion's
quantifiers, my own execution of it, my own independent counterexample re-run
against the new candidate, and a white-box kill proving the test discriminates.

The white-box kills follow the mandatory discipline: each plant was proven
**unique** (`grep -c` on the marker and a uniqueness assertion on the replaced
text, aborting on a count ≠ 1), proven **new** (marker absent before), proven
**applied** (marker present after, and for kill 4 the whole rewritten function
printed to show the `mkdirSync` test-and-set is unreachable), and proven
**reverted** (`diff -q`/`diff -r` against the candidate, then a full 226/226
re-run of the clone).

---

## 3. Row 1 — `FC-HARNESS-001` passes — **PASS**

### 3.0 Authority

- BF2 charter, exit criteria: "`FC-HARNESS-001` … pass. Harness self-tests prove
  source/package drift refusal, offline execution, non-vacuous official and
  candidate arms, expected-golden immutability, parse/link/runtime failure
  detection, atomic result accounting, diagnostic/mapping discrimination, and
  every forbidden normalizer mutation."
- `ssr-hydration.md`: assertions cover "initial DOM/markers" and "successful
  hydration without replacement"; "A hydration mismatch is semantic and cannot be
  normalized away."
- Round-6 fix-1 requirement, dispatched into this row: "Both checks must run and
  throw BEFORE any oracle compiler code is ever imported/required/evaluated."

### 3.1 B1 — same-process memo bypass

**Round-6 finding, verbatim** (`BF2-REVIEW6-ARCHITECTURE.md` §Executive result 1
/ §B1):

> "`ensureOracleDomain` memoizes a successful validation and returns the memo
> before either new pass-6 gate. In one process, mutate the installed compiler
> after the first validation and a later production `importOracleModule`
> evaluates that changed compiler. This directly violates Fix 1's 'both checks
> must run and throw BEFORE any oracle compiler code' requirement."

**Pass-7 mechanism.** `src/oracle-install.mjs:555-562`. The memo early-return is
no longer unconditional:

```js
export function ensureOracleDomain(framework) {
  const entry = frameworkEntry(framework);
  const memo = ensured.get(framework);
  if (memo !== undefined) {
    assertRecordedContentIntact(entry, framework, memo.installDir);       // :559
    assertOracleEntrypointsResolvable(entry, framework, memo.installDir); // :560
    return memo;
  }
```

**Caller/domain inventory — the quantifier.** "Every load" is the load-bearing
word, so I inventoried it rather than sampling. `grep` over `src/` and `bin/`
finds exactly five functions that can hand out an install path or a module, and
**all five** route through `ensureOracleDomain` as their first statement:
`oracleRequire` (`:630`), `importOracleModule` (`:653`), `oracleScratchDir`
(`:675`), `oracleLinkBaseDir` (`:683`), and `ensureOracleDomain` itself, called
directly by `invoke-vue-oracle.mjs:41`, `invoke-svelte-oracle.mjs:23`, and
`bin/generate-goldens.mjs:71-72`. There is no other route into the install tree.
Because the two gates now precede *every* return of that one function, the
"before any oracle compiler code" quantifier holds over the complete domain, not
one sampled path.

**Designated test.** `test/closure-drift.spec.mjs:614` — "a payload mutated AFTER
a successful load in the SAME process is REFUSED on the next load (memoization
does not bypass the gates)". It covers the criterion's words because it (a) uses
the *real* production loaders, not a helper — `mod.oracleRequire` and
`mod.importOracleModule`; (b) proves the prime genuinely loaded the real compiler
(`expect(typeof compilerSfc.parse).toBe("function")`) so the memo is genuinely
warm; (c) proves the plant is **new** (`not.toContain` before) and **applied**
(`toContain` after); (d) asserts the *typed* refusal
(`toBeInstanceOf(PackageDriftError)` + `details.layer === "realized-content-drift"`),
not merely "it threw"; and (e) asserts it on **both** loader shapes, CJS and ESM.
It avoids the round-6 objection structurally by never spawning a child for the
load half — it imports the production module under a cache-busting query so it
gets a fresh, private memo map in *this* process.

**Run.** `pnpm exec vitest run test/closure-drift.spec.mjs --reporter=verbose` →
**18 passed (18)**, exit 0, the new case at 337 ms.
Evidence `target-closure-drift.log`, SHA-256
`e6cfa734da4838e4c40932184ebe8a15244af706479c01aaae9255c761365a5c`.

**My own counterexample (round-6 probe re-run, widened).** I reproduced the
round-6 probe and strengthened it in two ways: I exercised **all four** exported
entry points rather than one, and I loaded a specifier that had *not* already
been imported (`svelte/server`), because re-importing an already-loaded ESM
specifier returns Node's module cache and would not have re-read the poisoned
bytes anyway — the round-6 probe's shape understated the risk.

```
primedOk: true, preLoaderOk: true
payloadDigestChanged: true, plantProvenApplied: true, contentManifestUnchanged: true
postPlantCalls:
  ensureOracleDomain                 → PackageDriftError / realized-content-drift
  oracleScratchDir                   → PackageDriftError / realized-content-drift
  importOracleModule(svelte/server)  → PackageDriftError / realized-content-drift
  oracleLinkBaseDir                  → PackageDriftError / realized-content-drift
```

`BF2_R7_CONF_MEMO_POISON_EXECUTED` never appears in the output — the poisoned
compiler did not run. Round 6's `BF2_R6_MEMO_BYPASS_EXECUTED` did.
Probe `probes/probe-B1-memo-bypass.mjs`
(`e0edb905b937161cfb266f6de6958fc05c737cf921f325466160f9d9412589a9`),
output `probe-B1-memo-bypass.log`
(`a25a2f180f61c37a9dabf71b94f5a9408f56598acecb252fceff20f03d579e8b`).

**White-box kill 1.** Restored the pre-fix unconditional
`if (memo !== undefined) return memo;` (target text asserted unique before
replacement; marker count 1 after; the two `assertRecordedContentIntact`
occurrences dropped 4 → 3, confirming the removal landed).
Result: **1 failed | 17 passed** — precisely the new same-process test, nothing
else. `kill1-memo-bypass.log`,
`cad1ec808ee03c69bc552e9dab14b9bc435b43590400aeaaa1bc7a0e4699fcaa`.

**A residual I checked and cleared.** The memo path re-runs the two live gates
but not `assertEvidenceStaticPinned`. I checked whether a mid-process tamper of
the committed lock could therefore disarm the content gate, since
`assertRecordedContentIntact` returns early when
`recorded.lockSha256 !== entry.lockSha256`. It cannot: `entry.lockSha256` is
`EVIDENCE_LOCK_DIGESTS.*` (`src/oracle-install.mjs:104,112`) — a frozen
source-level constant in `domain-pin.mjs`, never re-read from disk — so the
"superseded lock" branch is reachable only by a genuine committed-domain
amendment, not by on-disk tampering. No finding.

### 3.2 B3 — text-root hydration marker loss

**Round-6 finding, verbatim:**

> "Svelte hydration's new structural comparison erases comments and treats a
> text-only server root as having vacuously reused its server nodes. Markerless,
> wrong text is discarded and rebuilt, but the harness reports
> `mismatched: false`. This is a structural hydration-marker loss, distinct from
> the explicitly accepted dynamic-text-only limitation."

**Pass-7 mechanism.** `src/hydration.mjs:189,196` — approach (a) of the two the
brief offered. The reuse-identity signal now tracks *every* initial child node:

```js
const initialServerNodes = [...container.childNodes];            // :189 (was .filter(n => n.nodeType === 1))
const serverNodesReused =
  initialServerNodes.length === 0 || initialServerNodes.some((n) => container.contains(n)); // :196
```

The comment-erasing `serialize` is *retained*, with a documented reason I
independently checked and accept: a fresh detached `mount` never emits SSR
boundary markers, so folding comments into the fresh-render comparison would
report every **correct** hydration as mismatched. Marker survival moved to
signal 2 instead, which is the coherent placement — markers are now tracked as
identity, not as content. My control results (below) confirm the retained
erasure is not hiding anything: correct hydrations stay clean and every marker
mutation is caught.

**Designated test.** `test/hydration.spec.mjs:148`. It covers the criterion's
words because it asserts a **positive control first** (the official
marker-bearing SSR of the *same* component/props must stay `mismatched: false`,
with `expect(official.html).toContain("<!--[-->")` proving the control really is
marker-bearing — so the widened signal is proven not trigger-happy before it is
credited for a detection), then two negative arms: fully markerless wrong text
(with `expect(markerlessHtml).not.toContain("<!--")` proving markerlessness) and
torn markers (`<!--[-->WRONG TEXT`). The brief's requirement (i) and (ii) are
each a separate assertion.

**Run.** `pnpm exec vitest run test/hydration.spec.mjs --reporter=verbose` →
**6 passed (6)**, exit 0, the new case at 971 ms.
`target-hydration.log`, `302acc1f282e21ed7cd75c5e83b4c7235d87af548167e7e254ab99a881fff21e`.

**My own counterexample (round-6 probe re-run, extended to 12 cells).** I ran the
round-6 input plus five more shapes, against **both** a text root and an element
root, using the real pinned compiler and runtime:

| case | text-root | element-root | required |
|---|---|---|---|
| A official marked SSR (control) | `false` | `false` | false ✔ |
| B `WRONG MARKERLESS TEXT` — **round-6's exact input** | **`true`** | `true` | true ✔ |
| C torn markers (`<!--[-->WRONG TEXT`) | `true` | `true` | true ✔ |
| D garbled marker payload (`<!--x-->hello<!--y-->`) | `true` | `true` | true ✔ |
| E empty server root | `false` | `false` | vacuous by design ✔ |
| F intact markers, only dynamic text differs | `false` | `false` | settled limitation, unchanged ✔ |

Round 6 recorded `{"markerless": {"result": {"mismatched": false}}}` for cell B.
It is now `true`. `probe-B3-hydration.log`,
`1b44cbe528bbdb78002f256d7396f591c705090f3c01aed3e89019ea4b1a9f7c`.

**My own additional fault injection — partial replacement.** Signal 2 uses
`some`, not `every`, so a *partially* replaced server root is not caught by that
signal. I derived this counterexample independently (round 6 did not raise it)
and tested whether signals 1/3 cover it, using a two-root component
`<div class="a">static</div><span>{label}</span>`:

| injected divergence (markers intact) | result |
|---|---|
| wrong **static** text in the first root | `true` ✔ |
| wrong **tag** on the second root | `true` ✔ |
| **extra** sibling node inside the markers | `true` ✔ |
| **missing** second root inside the markers | `true` ✔ |
| wrong **attribute** value | `true` ✔ |
| control (unmodified official) | `false` ✔ |

No gap: signal 3's structural walk covers everything signal 2's `some` misses.
`probe-partial-replacement.log`,
`53ae0efcdcf59e6593e54419d3ca4bda417572a93861ffaf27e91e0e4570b81d`.

**White-box kill 3.** Restored `.filter((n) => n.nodeType === 1)` on line 189
(target asserted unique). Result: **1 failed | 5 passed** —
`AssertionError: expected false to be true` on exactly the new text-root case,
with both pre-existing element-root negative controls still green. This is
round-6's false-clean reproduced on demand, and it proves the new test is wired
to the widened decision rather than to some incidental effect.
`kill3-hydration-textroot.log`,
`b5b70b5cefe78acb00e2b57e6da179818491e0bbc48eead3dac54efa5543ab99`.

### 3.3 A6-1 / AF-4 — the exclusion test's discrimination

**Round-6 finding, verbatim** (`BF2-REVIEW6-ADVERSARIAL.md` §3, A6-1):

> "Round-5 AF-4 is unaddressed, unmentioned and undispositioned in pass 6, and
> independently reproduces: the designated concurrency test passes **3 of 15 runs
> with the exclusion mechanism entirely removed**."

carrying round-5 AF-4:

> "The designated test for fix 3 detects total removal of the lock in only 9/15
> planted runs (~60 %); unmutated 10/10 pass."

**Pass-7 mechanism.** `test/oracle-install-realization.spec.mjs:122` — a new
deterministic test that replaces timing luck with a schedule: the test itself
creates `<installs>/svelte.lock` (the exact directory `acquireRealizeLock`'s
`mkdirSync` test-and-set creates), spawns a real production realizer, waits a
fixed 10 s, then asserts four independent no-progress facts
(`finished === false`, no `.stage-*` directory, no final tree, no
`REALIZED_DIGEST` on stdout), releases the lock, and requires the *same* child to
then complete with exit 0, a validated closure, a matching digest, and no
residue. The old racy test is retained and explicitly demoted in its own comment
to a happy-path check. The gating is unchanged (`runIf` on the provisioned
cache — skipped, never silently passed).

**Run.** `pnpm exec vitest run test/oracle-install-realization.spec.mjs
--reporter=verbose` → **3 passed (3)**, the new case at 10 523 ms — i.e. it
genuinely spent the hold window rather than short-circuiting.
`target-realization.log`, `5b0b5f5e2047a0465eeba21e7bee8580baa8641236b2872447343aba5df7546a`.

**White-box kill 4 — 5 planted runs, all RED.** I planted the adversarial seat's
own mutation shape (`return lockPath;` immediately after the path is computed,
before the `deadline`/`for(;;)`/`mkdirSync` test-and-set), proved the marker
absent beforehand and unique after, and printed lines 473-486 of the mutated
function to show the `mkdirSync` exclusion is unreachable:

| planted run | result | elapsed |
|---|---|---|
| 1 | **RED** | 10 010 ms |
| 2 | **RED** | 10 008 ms |
| 3 | **RED** | 10 007 ms |
| 4 | **RED** | 10 008 ms |
| 5 | **RED** | 10 009 ms |

Failure reason is the discriminating assertion, not an incidental error:
`test/oracle-install-realization.spec.mjs:180` → `expect(finished).toBe(false)`,
`AssertionError: expected true to be false`. `kill4-run1..5.log`, digests in §7.

**I also reproduced AF-4 itself on this machine, under the identical plant**, to
confirm the finding was real and that the *new* test — not the retained one — is
what now kills it. Eight planted runs of the old two-racer test:

```
run 1 PASS · run 2 FAIL · run 3 PASS · run 4 FAIL · run 5 PASS
run 6 PASS · run 7 FAIL · run 8 PASS      → 5 GREEN / 8 with the mechanism deleted
```

62.5 % miss rate here versus round 6's 20 % and round 5's 40 % — machine- and
load-dependent, exactly as characterized, and worse on this host than on either
prior reviewer's. Against the same plant the new test is 5/5. AF-4 is closed by
replacement, not by silence.

### 3.4 Row 1 verdict

Fix 3 (TypeScript observation) and the remaining row-1 mechanisms were settled
PASS at round 6 and are untouched by this diff; the full suite re-confirms them.
B3 and A6-1 are closed with positive witness, negative witness, my own
counterexample, and a discriminating kill. B1 and B2 (row 1's shared findings)
are closed in §3.1 and §4.1.

**Row 1: PASS.**

---

## 4. Row 4 — source / package / realized-tree drift refusal — **PASS**

### 4.0 Authority

- `official-core-oracles.md`: "The harness rejects any source SHA/tree, package
  version, integrity, or transitive closure mismatch before generating
  expectations or running candidate output."
- Round-6 required correction for B2: "Define one authoritative per-framework
  list of actual oracle module specifiers/entrypoints, shared by the resolver
  gate and loaders, and resolve every item before recording/using the tree… the
  inventory should be derived from all production oracle loader callsites, not
  assumed from the lock's direct-package keys."

### 4.1 B2 — the entry-resolvability gate validated the wrong specifier

**Round-6 finding, verbatim:**

> "The torn-tree check resolves each direct package name, not each oracle entry
> actually loaded. Svelte's package-root entry remains resolvable after
> `svelte/compiler` is deleted, so the gate accepts and records an installation
> that its production loader cannot load."

**Pass-7 mechanism.** Two parts.

1. `src/domain-pin.mjs:59` (`VUE_DOMAIN`) and `:85` (`SVELTE_DOMAIN`) add a
   frozen `oracleLoadSpecifiers` column: `{ specifier, loader, extraConditions? }`
   rows derived from the real callsites and documented with those callsites
   inline.
2. `src/oracle-install.mjs:400` `assertOracleEntrypointsResolvable` keeps the
   direct-package root loop **and adds** a per-row loop at `:423` that resolves
   each declared specifier *under its own caller's loader semantics*: `require`
   rows via `req.resolve`; `import` rows via `esmImportTargetFile` (`:344`), a
   fail-closed exports-map walk under `{node, import, …extraConditions}`
   (`resolveExportsTarget`, `:312`) plus a `statSync().isFile()` existence check.

**The inventory quantifier — verified by inventory, not sampling.** I grepped
`src/` and `bin/` for every `oracleRequire`/`importOracleModule` callsite and
compared against the declared rows:

| real callsite | specifier | loader | in `oracleLoadSpecifiers`? |
|---|---|---|---|
| `invoke-vue-oracle.mjs:57` | `@vue/compiler-sfc` | require | ✔ |
| `execute-vue-runtime.mjs:43`, `hydration.mjs:89` | `vue` | import | ✔ |
| `execute-vue-runtime.mjs:44` | `@vue/server-renderer` | import | ✔ |
| `invoke-svelte-oracle.mjs:30` | `svelte/compiler` | import | ✔ |
| `execute-svelte-runtime.mjs:38` | `svelte/server` | import | ✔ |
| `hydration.mjs` generated runner (`--conditions=browser`) | `svelte` | import + `browser` | ✔ |

Complete: six callsite specifiers, six rows, no omission and no invented row.

**Why the `extraConditions` column is load-bearing, not decoration.** I resolved
each row through Node's own resolver (`import.meta.resolve` in a module written
inside the install tree, spawned with the row's conditions):

```
@vue/compiler-sfc   → node_modules/@vue/compiler-sfc/dist/compiler-sfc.cjs.js
vue                 → node_modules/vue/index.mjs
@vue/server-renderer→ node_modules/@vue/server-renderer/index.js
svelte/compiler     → node_modules/svelte/src/compiler/index.js
svelte/server       → node_modules/svelte/src/server/index.js
svelte (browser)    → node_modules/svelte/src/index-client.js
```

The `browser` row lands on `src/index-client.js`, a **different file** from the
server root `src/index-server.js` — so a gate that ignored conditions would
validate a file the hydration runner never loads, which is the same class of
error as B2 itself. `probe-resolver-fidelity.log`,
`bd6d04809fbae51363ab57f6b0ddb859bfd479af45607f2f49adba297a25b9f9`.

**My own fault injection — every row proven load-bearing, plus an over-strictness
control.** For each of the six rows I deleted **only** the file Node resolves to
(package manifests and everything else intact, content manifest removed so the
structural gate decides alone) and required a refusal naming that specifier;
then a seventh, inverse control:

| tear | gate | layer / named | required | ✓ |
|---|---|---|---|---|
| `@vue/compiler-sfc/dist/compiler-sfc.cjs.js` | refused | `oracle-entry-unresolvable` / `@vue/compiler-sfc` | refuse | ✔ |
| `vue/index.mjs` | refused | … / `vue` | refuse | ✔ |
| `@vue/server-renderer/index.js` | refused | … / `@vue/server-renderer` | refuse | ✔ |
| `svelte/src/compiler/index.js` | refused | … / `svelte/compiler` | refuse | ✔ |
| `svelte/src/server/index.js` | refused | … / `svelte/server` | refuse | ✔ |
| `svelte/src/index-client.js` | refused | … / `svelte` | refuse | ✔ |
| **control:** `svelte/compiler/index.js` (the *require*-branch bundle production never imports) | **accepted** | — | accept | ✔ |

7/7. The control is the important one: it proves the gate resolves the **import**
branch, not the `require` branch — the exact condition-divergence that made
root-resolution meaningless in B2 — and that the new gate is not over-strict.
`probe-gate-target-fidelity.log`,
`4a14aca627db2badcacf4ef5125880e82ee76438fb52148be82d15263e605153`.

**My own counterexample (round-6 probe re-run).** Round 6's exact scenario —
delete `svelte/src/compiler`, keep the package root, every `package.json`, and
the CJS compiler bundle; no content manifest:

```
plantApplied: true, packageJsonIntact: true, packageRootEntryIntact: true,
cjsCompilerBundleIntact: true, contentManifestPresentBefore: false
gate: { accepted: false, PackageDriftError, layer: oracle-entry-unresolvable,
        pkg: "svelte/compiler" }
contentManifestRecordedByGate: false
```

Round 6 recorded `gate: accepted=true`,
`contentManifestRecordedByAcceptedGate: true`, `load: ERR_MODULE_NOT_FOUND`. Both
halves of the round-6 harm — acceptance *and* baseline-recording of a torn tree —
are gone. `probe-B2-svelte-subpath.log`,
`6314db9f746b07f62d0a4ec54a5c080f6e3b5092c9ebe915024376bb449d89c5`.

**Designated test.** `test/closure-drift.spec.mjs:561`. Its assertions cover the
criterion's words: it proves the tear applied (`existsSync` false), proves the
pre-fix acceptance shape still holds (`src/index-server.js` present — i.e. the
package root *would* still resolve, so acceptance under the old gate is not
hypothetical), proves the content manifest is absent so gate 1 cannot be credited
for the refusal, then asserts a **typed** refusal (`PackageDriftError` +
`oracle-entry-unresolvable`), asserts `not.toContain("ERR_MODULE_NOT_FOUND")` —
so the refusal comes from the gate and not from the loader tripping — asserts no
artifact was produced, and asserts the torn tree was **not** recorded as a new
baseline.

**White-box kill 2.** Removed the `oracleLoadSpecifiers` loop entirely (target
asserted unique; module proven to still parse). Result: **1 failed | 17 passed**
— exactly the new Svelte case, with the pre-existing Vue torn-tree case still
green. That asymmetry is itself the proof of B2's premise: the Vue case passes
under the old direct-package inventory because `@vue/compiler-sfc` *is* the load
specifier, and only the Svelte case discriminates.
`kill2-svelte-subpath.log`,
`eb1d7d16c7683a92f5bdd896e232ae28cf0d49ed34ea721bdda341e9564e7eb1`.

### 4.2 B1 in the row-4 frame

The pre-execution ordering the row requires ("before generating expectations or
running candidate output") now holds on the memoized path as well as the cold
one, across the complete five-function caller inventory — §3.1, probe
`probe-B1-memo-bypass.log`, kill 1.

### 4.3 Row 4 verdict

**Row 4: PASS.** Both blocking causes closed, each with a typed refusal, a
proven-applied fault injection of my own, and a kill that turns exactly the
designated test red.

---

## 5. Item 2 — no regression on the other 14 rows

Exact required command, on the bound candidate, with pinned checkouts present:

```
$ pnpm --filter @verter/framework-conformance-harness test

 Test Files  19 passed (19)
      Tests  226 passed (226)
   Duration  14.04s
   Exit      0
```

226 = the round-6 baseline of 222 plus exactly the four new tests (two in
`closure-drift`, one in `hydration`, one in `oracle-install-realization`). **0
failed, 0 skipped** — no unexpected skip, and no previously-passing test lost.
`full-suite.log`, SHA-256
`36512b6900226c048eaecc8bf027114df29b0af993182a3256ef2ac415b56fab`.

Repeatability: 3 consecutive additional green runs of the same command, plus one
`--fileParallelism=false` run (218 passed / 8 skipped, before checkouts were
seeded) and two full 226/226 runs of the kill clone at restored state. Rows 2, 3,
5–16 are not re-derived here, per the round-7 scope; the two rows whose
mechanisms this diff *could* have disturbed but did not — row 3/14 (normalizer)
and the golden-provenance rows — are green in every run, including the
provenance `--check` arms that re-read the regenerated goldens (O-C).

I confirmed the two "bonus" test edits weaken nothing: both are timeout arguments
and reindentation only, with every `expect` byte-identical to `19cce22c8`.

---

## 6. Non-blocking observations

**O-A. The entry-resolvability gate proves the entry *file* exists, not that its
module *graph* resolves — the round-6 O-1 mitigation clause needs narrowing
again.** With the content manifest absent, I tore `svelte/src/internal/server`
(package manifests intact, and all six declared exports targets still present as
files):

```
gateAccepted: true          manifestRecorded: true
svelte/compiler → ERR_MODULE_NOT_FOUND  …/src/internal/server/hydration.js
                                        imported from …/src/compiler/phases/3-transform/server/visitors/shared/utils.js
svelte/server   → ERR_MODULE_NOT_FOUND  …/src/internal/server/index.js
svelte          → ERR_MODULE_NOT_FOUND  …/src/internal/server/context.js
```

That is round-6 B2's sentence — "the gate accepts and records an installation
that its production loader cannot load" — reproduced one level in. I am recording
it rather than filing it, for three reasons, and I want the reasoning on the
record rather than the conclusion alone:

- It is **inside the already-accepted residual window**. Round 6's adversarial
  seat found this same class (O-1: `postcss/*` torn, manifest deleted, gate
  accepts, loader fails mid-evaluation), bounded it, and judged it acceptable.
  My case is intra-package rather than transitive-package, but the same window.
- The **bound still holds, and I re-proved it**: with the manifest armed — the
  steady state after any successful realization — the identical tear is refused
  at `realized-content-drift`. `probe-B2c-armed.log`,
  `d614ff73293e952ffe3f36732518ea9a7cc54cd083524c28191c9e9268c7b813`. Pass 7's B1
  fix in fact *strengthens* this: the refusal now fires on every load, not just
  the process's first.
- It **fails loudly**. Every affected path throws `ERR_MODULE_NOT_FOUND`; no
  wrong conformance verdict can be emitted. B2 was blocking because the gate's
  inventory was systematically wrong for the primary Svelte compiler entry — zero
  coverage of the specifier that every Svelte compile goes through. What remains
  is a depth limitation, not a wrong target.

What should change is wording, not code. Round 6 asked that the ledger's
"gate 2 … still applies" be corrected to "…applies **to direct oracle entry
points**". After pass 7 the accurate phrasing is: *gate 2 applies to direct
oracle entry points and to each declared load specifier's own exports target
file, resolved under that caller's conditions — not to those targets' transitive
module graphs.* If a future block wants the residual closed, the mechanism is a
resolution-only transitive walk from each declared entry, which is a real
design decision and out of this round's scope.

**O-B. The two Svelte hydration tests carry no timeout override and fail on a
cold oracle-install run.** On this worktree's very first suite run — with the
oracle installs being realized from the npm cache inside a worker while 18 other
files contend — `test/hydration.spec.mjs:101` (pre-existing) and `:148` (the new
B3 test) both exceeded vitest's 5 000 ms default at 6 487 ms and 10 458 ms. Every
subsequent warm run is green (3/3 default-parallelism, plus serial, plus two clone
runs), and solo the new test costs 971-1 025 ms, so this is the round-6 O-6
contention class, not a semantic defect. It is worth noting because pass 7
*did* bump timeouts on the other two child-spawning files
(`golden-provenance`, `offline-execution`) to `60_000` for exactly this reason
and left the hydration file — which spawns five child processes in the new test
alone — at the default. The consequence is narrow but real: a CI runner with a
cold `.oracle-installs` will red-flag two tests, one of them the designated
evidence for B3. A `60_000` third argument on those two `it`s would make the
treatment consistent. Not filed as a finding: no assertion is wrong, no
mechanism is unproven, and the fix brief explicitly placed this class out of
scope.

**O-C. Golden churn is provenance-only.** 145 of the 153 changed paths are
goldens; `manifest.json` advances `generation: 8 → 12` (one regeneration per fix
commit). I diffed a sample record pair: the payload fields — `code`, `map`,
`raw.codeSha256`, `normalizer.normalizedDigestSha256`,
`realizedClosureSha256`, `packageLockSha256`, fixture digests — are unchanged;
only `generator.commit`/`generator.tree`/`generator.implementationSha256` differ,
and since those fields are inside the record, the record's own content-address
filename changes with them. So the churn is the content-addressing scheme working
as designed, not an expectation change. Two consequences worth stating: (i)
`generator.worktreeDirty: true` is recorded, as it also was at pass 6 —
unchanged, not a new defect; (ii) the recorded `generator.commit`
(`e7ddedb7ea…`) is a pre-squash WIP commit that does not exist in the squashed
candidate's history. Both were true of the pass-6 candidate that passed round 6,
and the golden-provenance suite (including its `--check` control arm) is green
against the regenerated set, so this is a no-change observation — but a future
block that wants golden provenance to be *reproducible from the landed history*
will have to address the squash/regeneration ordering.

---

## 7. Disposition of the two recorded limitations

Both were re-tested; neither changed status; I do not disagree with either.

**Locally-recorded (not lock-anchored) content digest.** Unchanged in kind. Pass
7 changes its *frequency* (now enforced on every load, per B1) and *widens the
independent backstop* (gate 2 now covers the actual load specifiers, per B2). A
combined tamper of tree + manifest still disarms the content gate specifically;
the entry-resolvability gate and the lock-anchored layers still apply, with the
narrowing in O-A. **Acceptable.**

**Hydration dynamic-text-only limitation.** Verified explicitly unchanged: cell F
of my 12-cell probe (intact markers, only the dynamic text value differs) reports
`mismatched: false` for both a text root and an element root, exactly as before.
This is distinct from the marker-loss class pass 7 closed (cells B/C/D), which
now all report `true`. **Acceptable, unchanged scope.**

---

## 8. Evidence index

All evidence is committed to the worktree under
`.agent-run/conf7-evidence/` (not `/tmp`-only); probe sources are under
`.agent-run/conf7-evidence/probes/`.

| File | SHA-256 |
|---|---|
| `binding-scope.log` | `0f4ad2f6dc2141da435cf2f911b0b1b46ba072c7090f04a3adc1690e7dce957a` |
| `full-suite.log` | `36512b6900226c048eaecc8bf027114df29b0af993182a3256ef2ac415b56fab` |
| `target-closure-drift.log` | `e6cfa734da4838e4c40932184ebe8a15244af706479c01aaae9255c761365a5c` |
| `target-hydration.log` | `302acc1f282e21ed7cd75c5e83b4c7235d87af548167e7e254ab99a881fff21e` |
| `target-realization.log` | `5b0b5f5e2047a0465eeba21e7bee8580baa8641236b2872447343aba5df7546a` |
| `probe-B1-memo-bypass.log` | `a25a2f180f61c37a9dabf71b94f5a9408f56598acecb252fceff20f03d579e8b` |
| `probe-B2-svelte-subpath.log` | `6314db9f746b07f62d0a4ec54a5c080f6e3b5092c9ebe915024376bb449d89c5` |
| `probe-B2b-detail.log` | `018dc45cb32a1796736a1d24f13e68fcd5c42846339d09a7715fb108d4a90373` |
| `probe-B2c-armed.log` | `d614ff73293e952ffe3f36732518ea9a7cc54cd083524c28191c9e9268c7b813` |
| `probe-B3-hydration.log` | `1b44cbe528bbdb78002f256d7396f591c705090f3c01aed3e89019ea4b1a9f7c` |
| `probe-partial-replacement.log` | `53ae0efcdcf59e6593e54419d3ca4bda417572a93861ffaf27e91e0e4570b81d` |
| `probe-resolver-fidelity.log` | `bd6d04809fbae51363ab57f6b0ddb859bfd479af45607f2f49adba297a25b9f9` |
| `probe-gate-target-fidelity.log` | `4a14aca627db2badcacf4ef5125880e82ee76438fb52148be82d15263e605153` |
| `kill1-memo-bypass.log` | `cad1ec808ee03c69bc552e9dab14b9bc435b43590400aeaaa1bc7a0e4699fcaa` |
| `kill2-svelte-subpath.log` | `eb1d7d16c7683a92f5bdd896e232ae28cf0d49ed34ea721bdda341e9564e7eb1` |
| `kill3-hydration-textroot.log` | `b5b70b5cefe78acb00e2b57e6da179818491e0bbc48eead3dac54efa5543ab99` |
| `kill4-run1.log` | `cd68bb974301478df32fbebbe43d05688eafd568190e9669bf1af555c62043fd` |
| `kill4-run2.log` | `2595c1d5202e754eb248dce449fe083d77026904b450a55bf81ab85c33ca0a05` |
| `kill4-run3.log` | `8710b4ee656971a6b4dac582f246605322c636a8c11d0ca0ff7d8f1811d451f3` |
| `kill4-run4.log` | `1c4b7fe4fdb6c57073185524ba306b58eabf0088d18e46921b6a7e01ebf37db1` |
| `kill4-run5.log` | `8f45d61feba840914cd868777fbc577ce4b31b994aede98c6774abbb323d6dc8` |
| `probes/probe-B1-memo-bypass.mjs` | `e0edb905b937161cfb266f6de6958fc05c737cf921f325466160f9d9412589a9` |
| `probes/probe-B2-svelte-subpath.mjs` | `e40bc609007c4aefe168b834d80c1d2937986bfefae6fe6b73889b03d7a0e29c` |
| `probes/probe-B2b-detail.mjs` | `79e81edd25012e71479bb8bd49acab8e39aa4e3ad3c45f8aaa08fcca4409740b` |
| `probes/probe-B2c-armed.mjs` | `a8db784f9e238dc02f00395f5c91d48c020d5102bbc61f616900635ff539ac2b` |
| `probes/probe-B3-hydration.mjs` | `820a0fd14e707c0459ec0fd78a7a1ef2b90a8afa20882e715db43c77edbc01db` |
| `probes/probe-partial-replacement.mjs` | `944918bc6610c2e41aa8f590a2a11d98c102e0b2385861f39ceebe5feb7775d0` |
| `probes/probe-resolver-fidelity.mjs` | `80d12aaa4486b6fe2a94a141fac73f435ad7a84b8f40648a6a793eec93882aa0` |
| `probes/probe-gate-target-fidelity.mjs` | `38f59f3e9651b5328d9e8f1fc6668c745b4e1aa78c8b1976e4a71b0596027e0d` |
| `probes/probe-gate-cost.mjs` | `54e79940b6d75c7fb1e1ebb976d5f362ea0568db2148edc5ef966d1a4e732375` |

Candidate integrity: `git status --short` empty at report time; no candidate file
was edited at any point; the kill clone was deleted after use.

---

## 9. Final disposition

| Target row | Round-6 verdict | Round-7 verdict | Basis |
|---|---|---|---|
| 1 `FC-HARNESS-001` | BLOCKING | **PASS** | B1 closed (5-function caller inventory + 4-entry-point probe + kill); B3 closed (12-cell probe incl. round-6's exact input + 6-cell partial-replacement injection + kill); A6-1/AF-4 closed by deterministic replacement (5/5 planted RED vs. the old test's 5/8 planted GREEN reproduced here) |
| 4 drift refusal | BLOCKING | **PASS** | B2 closed (6/6 declared specifiers proven individually load-bearing + over-strictness control + round-6 probe now refuses and records nothing + kill); B1's pre-execution ordering now holds on the memo path |
| 2, 3, 5–16 | PASS (settled) | **no regression** | 226/226, 0 skipped, exit 0, plus 3 repeat green runs |

**Final verdict: PASS.**
