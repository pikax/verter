# BV0 — Adversarial review (candidate `c40a1ca96`, base `b64358705`)

Charter: `docs/arch/refactor/rev11/charters/BV0.md`
Reviewer posture: assume every test is weak until a real red/green plant proves otherwise.
Worktree: `<worktree>/verter-review-bv0-adv`, left byte-identical to start
(`git status --porcelain` empty, `HEAD == c40a1ca96b73cf9b723fd7209516ea0462deaad2`).

---

## 0. Environment note that materially changed the review

On a fresh checkout the seed matrix's **link** and **runtime** JS axes SKIP, because
`.oracle-installs/` and `.oracle-npm-cache/` are gitignored and require a one-time network
provisioning step. `js_axis_reasons` (official_seed_matrix.rs:466-486) treats `skipped` as
informational only — never a failure. So on any unprovisioned machine all 36 cells pass with
2 of 4 axes never executing.

I provisioned it (`node packages/framework-conformance-harness/scripts/provision-oracle-npm-cache.mjs`)
and verified all four axes genuinely run:

```
{"parse":{"status":"ran","ok":true},"mapping":{"status":"ran","ok":true},
 "link":{"status":"ran","ok":true},"runtime":{"status":"ran","ok":true}}
```

The three `vapor_runtime_behavior` cases also genuinely execute (no `SKIP` line with `--nocapture`)
against the real pinned `vue.runtime-with-vapor.esm-browser.js` in jsdom.

**Everything below was measured with the oracle genuinely installed.**

---

## 1. Mutation log (plant → red → restore → green)

Every mutation was applied with a Python patcher that `assert`s the target string occurs
**exactly once** before writing, and was verified present by `grep` afterwards — so a plant that
failed to apply could not be silently read as "the code is correct". Every mutation was restored
and the suite re-run green.

| # | Production fix reverted | File | Test(s) expected to guard it | Result |
|---|---|---|---|---|
| M1 | `bind_expose`/`emit_bare_expose_call` → pre-fix (`has_expose` / `false`) | `script/process.rs:470-471` | `non_inline_no_define_expose_binds_and_emits_bare_expose_call` | **RED** ✔ |
| M2 | Restore `if is_ssr { plain return }` around `__isScriptSetup` | `script/process.rs` `build_setup_wrapper_end` | `ssr_setup_return_carries_script_setup_marker` | **RED** ✔ |
| M3 | Re-add `names.sort()` (alphabetical `__returned__`) | `script/process.rs:953` | `build_returned_preserves_declaration_order_not_alphabetical` | **RED** ✔ |
| M4 | `is_vapor && is_ssr` → `is_vapor` | `script/process.rs` `build_setup_wrapper_end` | `build_wrapper_end_vapor_non_ssr_no_vapor_flag` | **RED** ✔ (but see BLOCKING-1 — it guards the *wrong* behavior) |
| M5 | Drop `|| element.tag_type.is_slot_outlet()` from `is_slot_parent` | `vdom/mod.rs` | `slot_outlet_fallback_static_element_marked_slot_cached`, `..._two_static_elements_grouped_not_double_wrapped` | **RED** ✔ (2 tests) |
| M5b | Drop the `-1 /* CACHED */` close on cached static text | `vdom/slots.rs:1754` | `slot_outlet_fallback_static_text_uses_cache_with_cached_flag` | **RED** ✔ |
| M6 | `for_item_needs_own_block` → `el.v_for.is_some()` | `vdom/mod.rs` | `v_for_constant_source_emits_stable_fragment_without_item_block` | **RED** ✔ |
| M7 | Drop the anchor argument from `_setInsertionState(nP, nA)` | `vapor/block_plan/emit.rs:313` | seed-matrix vapor cells / `vapor_runtime_behavior::*` | **RED structurally, GREEN behaviorally** — see NON-BLOCKING-1 |

Detail:

- **M1** — 1 unit test red; **24 of 36 seed-matrix cells red**. The 12 green cells are all `slots`,
  whose fixture has no `<script>` block at all (verified) — correct, not a coverage hole. Both
  negative controls (`inline_mode_no_define_expose_omits_bind_and_bare_expose_call`,
  `non_inline_with_authored_define_expose_output_is_byte_identical`) stayed green.
- **M3** — 1 of 8 `build_returned_*` tests red; the other 7 correctly insensitive.
- **M6** — 1 of 133 `v_for_*` tests red; the negative controls
  (`v_for_ref_source_keeps_keyed_fragment_and_item_block`,
  `v_for_let_source_without_key_keeps_unkeyed_fragment`,
  `v_for_const_member_expression_source_stays_keyed`) stayed green.
- **M5/M5b** — the two halves of the slot fix are *independently* guarded; neither test covers both.
  The author's own note on `slot_outlet_fallback_static_element_marked_slot_cached` honestly states it
  does not discriminate alone and names the companion test that does. That is accurate.

### Extra plant: the SSR runtime axis itself

To check the runtime oracle isn't decorative I fed three SSR candidates through
`bin/check-candidate.mjs` (plants proven applied by counting occurrences before running):

| variant | runtime axis |
|---|---|
| golden verbatim | `ran`, ok |
| `$setup.` → `_ctx.` | `ran`, **FAILED** — `<ul><!--[--><!--]--></ul>` vs golden's three `<li>` |
| `<p>zero</p>` → `<p>ZERO</p>` | `ran`, **FAILED** — HTML diverges |

The runtime axis genuinely executes and discriminates. It also **empirically confirms** the
`__isScriptSetup` reasoning: with the marker present, `_ctx.<setupBinding>` does *not* resolve
(the pinned runtime warns `Property "count" was accessed during render but is not defined on
instance`). Official rc.3's `ssrRender` reads `$setup.count`, and BV0 correspondingly deleted
`is_ssr` from `BindingResolver`. That change is coherent and correct.

---

## 2. Tests that pass regardless of the fix

No `assert!(true)`, `assert_eq!(1,1)`, `|| true`, or empty `#[test] fn …() {}` bodies anywhere in
the diff's test surface (33 217-line test diff scanned). `known_divergences_file_is_well_formed`
actively forbids the auto-generated `TODO:` note, and no ledger entry carries one.
`candidate-axes.spec.mjs` (14 tests, 64 assertions) is a genuine one-mutation-one-axis
discrimination suite; it passes.

One real gap rather than a tautology: `candidate-axes.spec.mjs` covers parse and mapping
discrimination only. There is no committed link/runtime-axis mutation control (it cannot run
without the oracle install). I supplied that control manually above.

---

## 3. Cells passing for a coincidental / wrong reason

This is where the review breaks.

### The Vapor golden's script half was never compiled as Vapor

`packages/framework-conformance-harness/src/invoke-vue-oracle.mjs:127`:

```js
const compiled = compileScript(descriptor, { id: filename, inlineTemplate: false, sourceMap });
```

`vapor` and `ssr` are passed to `compileTemplate` (lines 150, 168) but **never to `compileScript`**.
Official rc.3 derives them there as `vapor = sfc.vapor || options.vapor` and
`ssr = options.templateOptions?.ssr` (`compiler-sfc.cjs.js:15385-15386`). So every "official"
vapor golden in `goldens/records/` had its script half compiled as a **non-vapor, non-SSR**
component.

Proven directly against the pinned oracle, on the harness's own `fixtures/vue/basic-interpolation.vue`:

```
compileScript(descriptor, {})                    -> __vapor: false  defineVaporComponent: false   <- what the harness calls
compileScript(descriptor, {vapor:true})          -> __vapor: true   defineVaporComponent: false   <- what a real vapor build calls
```

and on a TS `<script setup lang="ts">`: `{vapor:true}` → `defineVaporComponent: true`.

`assembleNonInline({ scriptCode, renderCode, ssr })` (line 289) destructures only `ssr` and drops
`vapor`, so nothing restores the marker at assembly either.

Corroboration from inside this repo: **every** rc.3 corpus vapor golden carries `__vapor: true`
(e.g. `crates/verter_vue_conformance/corpus/goldens/3.6.0-rc.3/vapor/conditionals/if-else-if-else.js`),
while **no** harness vapor golden does. Two golden generators, same official version, opposite
answers.

### Verter's production code was then changed to match the defective golden

`crates/verter_compiler/src/script/process.rs` — three sites changed `if options.is_vapor` to
`if options.is_vapor && options.is_ssr`, justified by:

> "Official only emits this marker for SSR (`ssr && vapor` — see `ScriptCodeGenOptions::is_ssr`)"

That quotes only `compiler-sfc.cjs.js:15731`, which is inside the **`if (ctx.isTS)`** branch. The
`else` (JS) branch, four lines later at **15736**, is:

```js
} else {
    if (vapor) runtimeOptions += `\n  __vapor: true,`;
```

— unconditional for vapor. And in the TS branch official substitutes
`defineVaporComponent` (`vapor && !ssr`), which Verter does not emit at all (`grep defineVaporComponent
crates/` → no hits). So the accompanying claim "a component's own wrapper is otherwise identical
between the VDOM and Vapor backends" is false on both branches.

`__vapor` is load-bearing in the pinned runtime, not a cosmetic tag —
`isVaporComponent` is literally `return type.__vapor` (line 7412), and it gates VDOM↔Vapor
interop mounting (3933, 6779, 15362, 15381) and vapor-app component classification
(`if (isInteropEnabled && appContext.vapor && !component.__vapor)`, 13275).

**Why the matrix cannot see this:** the runtime axis mounts candidate *and* golden through
`mountVueVapor` and compares DOM. Both now lack `__vapor`, so both behave identically. Structural
comparison compares candidate against the same defective golden. Two bugs cancelling — exactly the
class this review was asked to hunt. And the new unit test
`build_wrapper_end_vapor_non_ssr_no_vapor_flag`, added by this commit, **locks the defect in**.

### Vapor nested block-plan and SSR `__isScriptSetup`, cross-checked against real behavior

Both check out on the merits:

- **SSR `__isScriptSetup`**: verified against the live pinned server renderer (§1 extra plant).
  Marker + `$setup.*` routing is the official pairing; `_ctx.*` routing genuinely breaks under the
  marker, and BV0 removed `_ctx.*` routing in the same change. Coherent.
- **Vapor nested block plan**: the emitted module and live DOM are correct —
  `_setInsertionState(n6, n7)` produces
  `<div class="root"><p>yes</p><!--if--><ul><li>a</li><li>b</li><li>c</li><!--for--></ul></div>`,
  source order preserved, branch swap leaves no stale node, list reorder moves rather than
  duplicates. Confirmed by dumping the real module + per-step HTML from the jsdom driver.

---

## 4. Undisclosed conformance regression, attributed

The seed corpus's tracked-divergence ledger grew from 361 to 694 reasons. To separate the
`3.6.0-rc.1 → rc.3` oracle re-pin (also in this commit) from BV0's code, I ran the corpus with the
**base compiler** against the **candidate's rc.3 goldens**:

| tree | cells | divergence reasons |
|---|---|---|
| base compiler @ rc.1 goldens | 85 | 361 |
| base compiler @ rc.3 goldens | 85 | 376 |
| **candidate compiler @ rc.3 goldens** | 84 | **694** |

The re-pin costs +15. **BV0's code costs +318 (+85%).** 48 cells worse, 17 better, 17 unchanged;
2 cells that PASSED now diverge (`components/dynamic-multi-root` / `elements-text/multi-root`,
VDOM non-inline, import-lowering order); 3 newly pass.

VDOM improved broadly. **Every worst regression is `vapor non-inline`**, and they are semantic,
not cosmetic:

- `v-on/modifiers` 6→28 — *"Verter-only: `createInvoker`; official-only: `child`, `next`,
  `withModifiers`, `withVaporModifiers`"*, plus `missing imported helper 'withModifiers'`.
  Event modifiers are not routed through the official modifier helpers.
- `conditionals/if-else-if-else` 3→22 — Verter bakes `"<p>Done:  "` into the static template where
  official emits `"<p> "` plus `txt`/`setText`/`renderEffect`. Interpolated text folded into static
  markup.
- `components/child-comp` 7→22, `v-bind/prop-attr-modifiers` 2→18, `v-on/key-modifiers` 5→20,
  `v-model/{checkbox,input,select}` 2→15 / 1→13 / 1→13, `v-bind/static-dynamic` 1→12.

The commit message describes only corrections and says nothing about this.

---

## 5. Charter-exit items that are not met

- *"The isolated oracle install is present so link checks genuinely execute."* — Nothing in this
  commit makes that true for CI or a fresh clone; the axes skip silently and the cell still passes.
  The commit message's "All 36 cells pass across every axis" holds only after manual provisioning.
- *"No Vue tracking, backlog, waiver, or retraction artifact remains."* — See BLOCKING-2. Also, new
  comments added by this commit cite `docs/arch/future/vue-vdom-parity-backlog.md` D6 as a live
  tracked divergence, and the 84-cell `known-divergences.json` waiver ledger grew 85%.

---

## 6. Required verification runs (capped, targeted)

`CARGO_BUILD_JOBS=3`, `--test-threads=3`, on the clean candidate tree, oracle provisioned:

```
cargo test -p verter_vue_conformance --test main -- --test-threads=3
  test result: ok. 47 passed; 0 failed; 0 ignored   (36 matrix cells, all 4 axes live)

cargo test -p verter_compiler -- --test-threads=3
  test result: ok. 6001 passed; 0 failed; 5 ignored
  test result: ok.  496 passed; 0 failed; 0 ignored
  (0 FAILED lines in the full log)
```

Both clean. `git status --porcelain` empty afterwards.

> Process note: during the base-compiler attribution experiment (§4) a
> `git checkout <base> -- crates/…` staged base content into the index, so a subsequent
> `git checkout -- …` restored *from the index* rather than from HEAD. Caught it when the
> "final" run reported 40 failures, fixed with `git reset --hard HEAD`, and re-ran both suites
> from a verified-clean tree — the numbers above are the clean-tree ones. All mutation results in
> §1 predate that experiment and are unaffected.

---

## Findings

### BLOCKING

**1. `__vapor: true` is now dropped from every non-SSR Vapor build, to match a golden that was
never compiled as Vapor.**
`invoke-vue-oracle.mjs:127` omits `vapor`/`ssr` from `compileScript`, so the harness's vapor
goldens are not official vapor output (proven: `{vapor:true}` → `__vapor: true`, `{}` → nothing, on
the harness's own fixture with the pinned rc.3 compiler). `script/process.rs` ×3 was changed to
`is_vapor && is_ssr` on a justification that quotes only the TS branch (`compiler-sfc.cjs.js:15731`)
while the JS branch (15736) is `if (vapor)` unconditional; the repo's own rc.3 corpus vapor goldens
all carry the marker. `__vapor` is `isVaporComponent` in the runtime and gates VDOM↔Vapor interop.
The new test `build_wrapper_end_vapor_non_ssr_no_vapor_flag` locks the defect in, and the seed
matrix cannot detect it because candidate and golden are wrong identically.
*Fix:* pass `vapor` (and `templateOptions.ssr`) to `compileScript`, regenerate the vapor goldens,
restore `if (is_vapor)` for the JS branch, and decide `defineVaporComponent` for TS separately.

**2. `docs/arch/ssr-noninline-shape-divergence.md` — edited by this commit — asserts the opposite of
the code this commit ships.**
It still states the SSR component "does **not** set `__isScriptSetup`", that "every binding routes
through `_ctx.*` … never `$setup.*`", and cites a guard "(`__isScriptSetup` present client / absent
SSR)" that this commit inverted. Its own exit criterion — *"the marker returns, bindings route
through `$setup.*`, and this document is deleted in the same change"* — was **met by this commit**,
yet the document was rewritten to re-assert the divergence rather than deleted. Directly against
the charter's "No Vue tracking, backlog, waiver, or retraction artifact remains."

**3. An 85% growth in tracked Vue conformance divergences, attributable to BV0's code and
undisclosed.** Base@rc.3 376 → candidate@rc.3 694 reasons; 48 cells worse, 2 previously-passing
cells now divergent; every worst regression is Vapor and semantic (missing
`withModifiers`/`withVaporModifiers`, interpolated text folded into static template markup, missing
`txt`/`setText`). Needs either a fix or an explicit ratified disposition — silently regenerating the
waiver ledger is not one.

### NON-BLOCKING

**1. The behavioral Vapor suite does not discriminate boundary DOM position.** Plant M7 (drop the
`_setInsertionState` anchor) renders
`<div class="root"><!----><ul>…</ul><p>yes</p><!--if--></div>` — the `v-if` block lands *after* its
sibling `<ul>` instead of before it — and all three `vapor_runtime_behavior` tests stay green.
`initial.starts_with("<div class=\"root\">") && initial.contains("<ul>")` is satisfied by a
wrongly-ordered tree. The structural comparator did catch M7 (4 cells red), so the production fix is
guarded — but the suite's own headline claim ("a generated `_next(n9)` lands on the node a block is
inserted before") is not what it tests. Assert full expected HTML, or the sibling order explicitly.

**2. Link/runtime axes silently skip on any unprovisioned checkout** and skips are non-failing, so
"36/36 across every axis" is not reproducible from a clone. Either provision in CI, or make a skip
fail when an env flag marks the run as gate-authoritative.

**3. Stale comment** `// $setup dot notation (SSR uses _ctx.x like VDOM)` —
`template/code_gen/ssr/tests.rs:2129`, contradicted by the two tests directly beneath it.

VERDICT: BLOCKING

1. `__vapor: true` removed from every non-SSR Vapor build to match a harness golden whose script half was compiled with `vapor` unset (`invoke-vue-oracle.mjs:127`); the justifying comment reads only official's TS branch while the JS branch emits it unconditionally; locked in by a new test the matrix cannot contradict because candidate and golden share the defect.
2. `docs/arch/ssr-noninline-shape-divergence.md`, edited in this same commit, asserts SSR omits `__isScriptSetup` and routes `_ctx.*` — both removed by this commit; its stated exit criterion was met and the document should have been deleted, not re-asserted (charter: no Vue tracking/waiver artifact remains).
3. Undisclosed +85% growth in tracked Vue conformance divergences attributable to BV0's code (376 → 694 with the oracle re-pin factored out), concentrated in Vapor and semantic in nature (missing `withModifiers`/`withVaporModifiers`, interpolation folded into static template markup), including 2 previously-passing cells.
