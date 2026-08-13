# BV0 Conformance Review — candidate `c40a1ca96` vs base `b64358705`

**Charter:** `docs/arch/refactor/rev11/charters/BV0.md` (Immediate Vue known-defect correction)
**Reviewer role:** CONFORMANCE (criterion-by-criterion against "Required exits")
**Worktree:** `/Users/carlosrodrigues/Documents/dev/verter-review-bv0-conf`, HEAD `c40a1ca96`, `git status` clean before and after every run.

> **Environment caveat, stated up front:** a second (adversarial) reviewer agent is operating in this
> same physical worktree and temporarily reverts production code for mutation testing. Every test run
> below was bracketed by a `git status --porcelain` integrity check; all reported runs executed against
> an **unmodified** tree at `c40a1ca96` (`tree_unchanged_during_run=YES`, `still_clean=YES`).

---

## 0. Test execution — what I actually ran

### 0.1 The mandated run (memory-capped, `CARGO_BUILD_JOBS=3 --test-threads=3`)

```
cargo test -p verter_vue_conformance --test main official_seed_matrix -- --test-threads=3
→ test result: ok. 36 passed; 0 failed; 0 ignored; 11 filtered out; finished in 15.58s
```

**This run alone is NOT sufficient evidence for the charter's required axes**, and that is finding **B2**
below. In the worktree's default environment the `link` and `runtime` axes **silently skip**. Proof:

```
$ node bin/check-candidate.mjs --candidate <cell> --out ...      # default worktree env
parse    ran
link     skipped | oracle npm cache not provisioned at .../verter-review-bv0-conf/...
mapping  ran
runtime  skipped | oracle npm cache not provisioned at .../verter-review-bv0-conf/...
```

`official_seed_matrix.rs:459-489` (`js_axis_reasons`) folds `"skipped"` into an informational line, never
a failure; `assert_cell_passes` (l.511) asserts only on `comparison.passed() && !js_failed`, and the
collected `js_reason_lines` are interpolated **only into the failure message**. A fully-skipped run is
therefore byte-indistinguishable from a fully-passing one on stdout.

### 0.2 The decisive re-run — all four axes genuinely executing

I provisioned the isolated oracle out-of-tree (worktree untouched; `.oracle-*` are gitignored):

```
$ BF2_ORACLE_NPM_CACHE=/tmp/bv0-oracle-npm-cache node scripts/provision-oracle-npm-cache.mjs
added 20 packages in 4s → oracle npm cache warmed          (exit 0)

$ ensureOracleDomain("vue")     # offline realization from the committed lock
installDir:            /tmp/bv0-oracle-installs/vue
realizedClosureSha256: a0a58df52c90abdffca1a61f94fe5a7ed2918f586f36e73a5e8d9decd01ce1e7
vue version:           3.6.0-rc.3          ← matches the pinned oracle exactly
```

Re-probe with the oracle wired in — **all four axes flip to `ran`**:

```
parse ran | link ran | mapping ran | runtime ran
```

Then the mandated suite re-run with the oracle active:

```
BF2_ORACLE_NPM_CACHE=... BF2_ORACLE_INSTALLS=... \
cargo test -p verter_vue_conformance --test main official_seed_matrix -- --test-threads=3
→ test result: ok. 36 passed; 0 failed; 0 ignored; 11 filtered out; finished in 26.14s
```

**36/36 pass with parse + exact-package-link + mapping + deterministic runtime/server genuinely
executing** (26.14s vs 15.58s is the link/runtime cost). The runtime axis is a real behavioral oracle,
not a structural proxy (`check-candidate.mjs:242-302`): SSR renders both sides and compares HTML; vdom
hydrates the candidate against the **official** SSR golden's HTML and compares final DOM; vapor mounts
both and compares DOM.

### 0.3 Supporting runs

```
cargo test -p verter_vue_conformance --test main   (oracle active)
→ 47 passed; 0 failed; 0 ignored                  incl. seed_conformance_matches_tracked_dispositions
                                                  incl. 3x vapor_runtime_behavior

cargo test -p verter_compiler --lib
→ 6001 passed; 0 failed; 5 ignored                (tree verified unchanged during run)
```

---

## 1. Required exits, criterion by criterion

| # | Required exit | Verdict |
|---|---|---|
| E1 | All 36 seed cells pass parse / link / structural / runtime / diagnostic / mapping | **MET** (§0.2) |
| E2 | Isolated oracle install present so link checks genuinely execute | **DEFECTIVE** → B2 |
| E3 | Every planted control is detected | **DELEGATED** (adversarial reviewer's assignment; not independently re-run here) |
| E4 | No generated Vue route changed to typed non-success | **MET** (§1.4) |
| E5 | No Vue tracking / backlog / waiver / retraction artifact remains | **NOT MET** → B1 |
| E6 | `__expose` + slot-fallback-cache pass dev/prod, inline/non-inline, VDOM/Vapor/SSR | **MET**, one cosmetic defect → N1 |
| E7 | Locked performance cells within existing thresholds | **MET** (thresholds unchanged; not executed) |

### 1.4 E4 — no typed non-success (MET)

`git diff … -- crates/verter_compiler/src | grep '^+' | grep -E 'CompileDiagnostic|Severity::Error|push_error|…'`
returns **zero** new error-emission sites. No `#[ignore]` was added anywhere in `crates/` (and none
removed). No new waiver/tracker file was added. Deleted files (`vapor/comment.rs`, `vapor/text.rs`) are
genuine legacy deletions folded into the new `vapor/block_plan/` — no shim, no dual path, consistent
with the Legacy Code Deletion rule.

The 17 `*.refuse.json` fixtures that surfaced in a refusal-pattern grep are **pre-existing Svelte**
constant-folding fixtures: `git diff --numstat` shows all 17 are 1-insertion/1-deletion version-string
changes and `--diff-filter=A | grep -c refuse` = **0**. Not a Vue tracking mechanism, not introduced here.

---

## 2. Independent spot-checks of fixed defect classes (diff read against real rc.3 source)

Oracle source read from the realized install at `/tmp/bv0-oracle-installs/vue/node_modules/@vue/*`
(verified `vue@3.6.0-rc.3`), **not** from commit-message claims.

### 2.1 `__expose` binding + implicit call — **FAITHFUL ✓**

Official `@vue/compiler-sfc/dist/compiler-sfc.cjs.js`:
```js
15658: const destructureElements = ctx.hasDefineExposeCall || !inlineMode ? [`expose: __expose`] : [];
15728: if (!ctx.hasDefineExposeCall && !inlineMode) setupPreambleLines.push(`__expose();`);
```
Verter `crates/verter_compiler/src/script/process.rs:470-471`:
```rust
let bind_expose          =  macro_state.has_expose || !options.inline_template;
let emit_bare_expose_call = !macro_state.has_expose && !options.inline_template;
```
Exact predicate-for-predicate correspondence on both the binding and the bare-call condition. Corroborated
independently: the phrase *"official always destructures `expose: __expose` … Verter emits those only when
`defineExpose` is used"* appears in **8+ triage notes at base** and in **zero** notes at candidate — the
defect really was eliminated corpus-wide, not just on the 3 seed fixtures.

### 2.2 Vapor `:key` exclusion from dynamic props — **FAITHFUL ✓**

`vapor/props.rs:157` skips `arg == Some("key")` gated on `skip_key_prop: el.v_for.is_some()`.
Cross-checked against the correct backend oracle — `@vue/compiler-vapor` reserves `key` in the **v-for**
context (`compiler-vapor.cjs.js:137` `prop.name === "bind" && …arg.content === "key" && dirs.includes("key")`;
`:4460` `findProp(node,"key")` inside the v-for transform; `:4463` `wrapTemplate(node,["for","key"])`).
The v-for-scoped gate is correct for vapor. (Note the vdom oracle differs — `compiler-core.cjs.js:4690`
excludes `ref`/`key` unconditionally — but that is a different backend and a different code path; the
candidate's gate is in `vapor/props.rs` only, so this is not a mismatch.)

### 2.3 Slot-fallback static caching + `CACHED` flag — **BEHAVIOR ✓ / rationale wrong** → N1

`vdom/slots.rs` correctly routes `<slot>` fallback children through the cache-aware emitter
(`emit_slot_children_with_cache`) instead of the uncached separators-only path, and attaches the `-1`
CACHED flag. The flag itself is right. But the new prod branch:
```rust
let close = if self.options.is_production { "\", -1))" } else { "\", -1 /* CACHED */))" };
```
is justified by the source comment *"dropping the dev-only comment in production"*. **The real rc.3
goldens contradict that claim** — `vue/slots__vdom__map0__prod1` (isProd=**true**) emits:
```
_cache[0] || (_cache[0] = _createTextVNode("Untitled", -1 /* CACHED */))
```
identical to the prod0 record. Official does **not** drop the comment in production. The comment further
claims to be *"matching the individual-element cache wrapper's own `-1 /* CACHED */` handling in
`element.rs`"* — but `element.rs:2544` emits `", -1 /* CACHED */"` **unconditionally, with no prod branch**.
The cited precedent says the opposite of what the new code does.

This does not fail the gate: `canon/comments.rs:13-25` anchors only semantic comments (PURE, license,
JSDoc, bundler-significant), so `/* CACHED */` is normalized away as cosmetic — permitted by the
Compiled-Output Conformance rule. Recorded as non-blocking **N1**.

---

## 3. Known-wrong-output guard / tracker / waiver scan

No typed refusal, no fixture-identity branch, no official-compiler fallback, and no new allowlist file was
introduced on any Vue path. `official_seed_matrix.rs:16-20` explicitly declines a tracked-divergence
mechanism for its own domain.

**However**, the pre-existing seed-corpus waiver ledger `crates/verter_vue_conformance/corpus/known-divergences.json`
was regenerated (`VERTER_CONFORMANCE_UPDATE=1` path, `seed_conformance.rs:341-372`) and **grew from 361 to
690 recorded divergence reasons**.

I initially read that growth as mass masking. **That reading is wrong and I am recording the correction:**
the dominant cause is a sanctioned oracle migration, not a regression. `seed_conformance.rs:120` and
`tests/common/mod.rs:13` move the corpus goldens `3.6.0-rc.1 → 3.6.0-rc.3`; the rc.1 tree is fully deleted
(only `corpus/goldens/3.6.0-rc.3` remains), matching the one-corpus-per-framework rule. rc.3 changed Vapor
materially (delegation → direct `on` listeners, `defineVaporComponent`, `withVaporModifiers`/`withVaporKeys`),
which is exactly where the growth concentrates. The comparator itself (`src/compare.rs`) is **unchanged** in
this diff, so the growth is not a stricter-oracle artifact either. Triage notes are genuinely maintained —
zero `"TODO: triage this divergence"` placeholders, and several record real wins ("v-if branch key hoisting
is now fixed", "the event-handler dynamic-props-array/PROPS-flag divergence is now fixed"). Three cells were
**removed** because they now pass.

That exoneration does **not** extend to two cells — see B1.

---

## BLOCKING FINDINGS

### B1. Two previously-passing Vue routes regressed, and were absorbed into the waiver ledger under a factually impossible justification

Two cells that had **no entry at base** (and therefore passed — `seed_conformance.rs:385-389` fails any
untracked divergence) acquired new waiver entries at candidate:

- `components/dynamic-multi-root | vdom | non-inline`
- `elements-text/multi-root | vdom | non-inline`

both with the same 2 reasons:
```
[structure] Program[0]/import              — node kind: verter `VariableDeclaration` vs golden `import`
[structure] Program[1]/VariableDeclaration — node kind: verter `import` vs golden `VariableDeclaration`
```

**This is not oracle churn.** I diffed the base rc.1 goldens against the candidate rc.3 goldens for both
cells: **byte-identical** (`IDENTICAL rc.1==rc.3` for both). The oracle did not move for these cells, so the
only changed variable is Verter's own emission. These are BV0-introduced regressions.

**The recorded justification is impossible.** Both entries carry the note:

> *"import-statement lowering order: Verter lowers **the user import** to a VariableDeclaration in a
> different Program-item slot … a template-only SFC has no other script-level statements…"*

Neither fixture has a user import — or any script at all. `grep -c "script\|import"` returns **0** for both
`.vue` files; `elements-text/multi-root.vue` is three static elements in a bare `<template>`. The actual
divergence is that Verter now emits the `_sfc_main` `VariableDeclaration` **before** the `vue` helper import,
where the golden emits the import first.

Charter impact — this is the specific thing BV0's procedure and abort clause forbid:
- Required procedure: *"prove unaffected routes retain their prior successful result contract"* — two vdom
  routes demonstrably did not.
- Abort/rescope: *"Do not substitute a guard, tracker, **waiver**, fixture-specific branch, or silent
  deferral."* A self-introduced regression was dispositioned into the waiver ledger rather than fixed or
  escalated, and the note attached to it would lead a later reader to dismiss it as a benign pre-existing
  shape.

Severity note, stated plainly: ES module imports are hoisted, so the emitted order is very likely
**behaviorally** harmless — this blocks on process and on the false rationale, not on a runtime hazard.
Required disposition: either fix the emission order, or record an explicit `ADOPT-NOW`/`DEFER`/`REJECT`
disposition with a rationale that matches the fixtures.

### B2. The new seed-matrix suite reports 36/36 green when the charter-required link and runtime axes never ran

`official_seed_matrix.rs` is authored by this block, and it cannot distinguish "link passed" from "link
never executed". Skipped axes are informational-only (l.476-480) and their lines reach stdout **only inside
the assertion failure message** (l.511-523). With no oracle provisioned — the default state of a fresh
clone, this worktree, and any CI runner that has not run the one-time provisioning — the suite prints
`36 passed` while `exact-package-link` and `deterministic runtime/server`, two axes the charter names
explicitly, did not run at all. §0.1 vs §0.2 above is the demonstration: same commit, same command, the
only delta is two env vars, and the axis statuses flip `skipped → ran`.

This contradicts the charter exit *"The isolated oracle install is present so link checks genuinely
execute"* and the project's MANDATORY *Verification Must Prove Execution* rule, which requires that
"unexpected prerequisite skips were zero" and states that "exit status 0 alone … is FAIL".

To be fair to the candidate: the *substance* is fine — I provisioned the oracle and all 36 cells pass all
four axes (§0.2), and the harness is honest by construction (skips carry an exact reason and remediation;
a drifted install is a hard refusal, never a skip). The defect is that nothing forces or records
execution. A minimal fix is sufficient: fail (or at minimum emit an unmissable stderr banner and a
non-silent marker) when `link`/`runtime` report `skipped`, so the charter's "genuinely execute" is proven
rather than assumed.

---

## NON-BLOCKING FINDINGS

**N1. `slots.rs` production `/* CACHED */` drop is cosmetically divergent and its rationale is wrong on
both counts.** Official rc.3 emits `-1 /* CACHED */` in production (`slots__vdom__map0__prod1` golden), and
`element.rs:2544` — cited by the comment as the precedent being matched — emits it unconditionally. The
comparator treats it as cosmetic so no test fails, but the code now disagrees with both the oracle and its
own stated model. Suggest dropping the prod branch (emit unconditionally, matching `element.rs`) or
correcting the comment.

**N2. Out-of-charter Svelte scope.** ~2609 files bump the Svelte oracle pin 5.56.3 → 5.56.8 inside a block
whose charter is *"Immediate Vue known-defect correction"*. Mechanically this is benign and I verified it
carefully: of 1390 `svelte_oracle_corpus/goldens` files **1390 are version-string-only and 0 have any
content change**; of 1220 `verter_svelte_conformance` files 1219 are version-only plus one 2-line
`src/model.rs` change. Svelte codegen output is byte-identical under the new pin, and it aligns with the
one-corpus-per-framework rule. Flagged only because it inflates a Vue block's diff from ~40 reviewable
source files to 2076 files, which materially degrades reviewability.

**N3. E3 not independently verified here.** "Every planted control is detected" is assigned to the
adversarial reviewer. I observed that `conformance_discriminator.rs` was correctly updated for rc.3 vapor
semantics (delegation/`$evtclick` planted mutations replaced with `_on(...)`/event-name mutations, with the
expected `DiffDim` adjusted from `Identifier` to `Literal`), but I did not re-run plant-red-green myself.

---

## Summary

The engineering substance is largely sound: the `__expose` fix is a predicate-exact match to rc.3, the
vapor `:key` gate matches `@vue/compiler-vapor`, all 36 seed cells pass every required axis with the oracle
genuinely executing, the wider corpus and 6001 compiler tests are green, no typed refusal or fixture branch
was introduced, and the vapor legacy files were properly deleted rather than shimmed. The large
known-divergence growth is explained by a sanctioned rc.1 → rc.3 migration, not masking — I checked and
withdrew that concern.

Two items block: a self-introduced regression on two previously-green Vue routes waived under a
justification the fixtures contradict (B1), and a new suite that reports green without proving the
charter-required link/runtime axes ran (B2).

VERDICT: BLOCKING
1. B1 — `components/dynamic-multi-root|vdom` and `elements-text/multi-root|vdom` regressed from passing to waived (rc.1/rc.3 goldens byte-identical ⇒ BV0-caused), dispositioned into `known-divergences.json` under a note citing a "user import" that neither script-less fixture contains; violates the required procedure's "prove unaffected routes retain their prior successful result contract" and the abort clause's prohibition on substituting a waiver.
2. B2 — `official_seed_matrix.rs` treats `skipped` link/runtime axes as informational and prints nothing on success, so it reports 36/36 green when the charter-required exact-package-link and deterministic-runtime axes never executed (demonstrated: identical command, axes flip `skipped → ran` on provisioning alone); fails the "isolated oracle install is present so link checks genuinely execute" exit and the MANDATORY Verification-Must-Prove-Execution rule.
