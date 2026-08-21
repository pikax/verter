# vue-benchmarks CSS probe findings — J1 input

**Status:** evidence, not a decision. Produced 2026-08-21 by an investigation
against `pikax/vue-benchmarks`, reproduced directly through the built native
binary (`processStyle` + `compileMany`), the same public API the benchmark
harness calls.

**Why this matters to J1:** 9 of 16 CSS plants fail, and every root cause lives
in `crates/verter_compiler/src/css/{mod,scoped,prepass,types}.rs` — the
lightningcss-based `process_style` authority the CSS directive marks for
deletion. J1 replaces that authority, so J1 either fixes these or reproduces
them.

**The load-bearing conclusion:** none of the six genuine defect classes are
acceptance criteria in the ratified plan today. The ruling's J1 floor names
feature *categories* (scoped/deep/global/slotted/keyframe, v-bind analysis); it
does not name these edge cases. Two of them already have Rust unit tests over
the exact input shape that fail to discriminate — `scoped.rs:686` feeds
`.item:is(.a, .b)` through and asserts only that the prefix is present, never
that the argument list stays unscoped. Today's suite is not a safety net for
these bugs when the new engine lands.

Highest severity is `v-bind`: the JS-registered key bakes in `--`, and Vue's
`useCssVars` prepends `--` again, so the custom property becomes
`----hash-color` and the binding never applies in a real browser. It is silently
broken at runtime, not cosmetic.

---

## Investigation: Why Verter fails the vue-benchmarks CSS probes

**Method:** Cloned `pikax/vue-benchmarks` to `/tmp` (read-only, outside the Verter checkout). Traced the CSS validation path to `scripts/lib/style-feature-gates.mjs` (`STYLE_FEATURE_CASES`, 16 plants) invoked from `scripts/lib/surfaces/compile.mjs::computeStyleCorrectnessGates`, which is the mandatory gate behind the README's `⚠` on "Verter compileMany + processStyle (render + CSS)". Reproduced the exact harness call (`compileMany` runtime-render + `verterNative.processStyle`) against Verter's already-built `packages/native/dist/verter-native.darwin-arm64.node` (built 20:39, no relevant source changes since — commit log checked) and ran the harness's own `assertStyleFeature` against the real output. No build was needed; no gate/cargo run was performed.

**Result: 9/16 plants fail**, all traced to concrete `file:line` defects, all living in `crates/verter_compiler/src/css/{mod,scoped,prepass,types}.rs` — the `lightningcss`-based `process_style` authority that `MAINTAINER-DIRECTIVE-CSS-CLEAN-CUTOVER.md:354` names for wholesale deletion.

### Table

| Probe | Fails because | Root cause | Category | Fixed by which J block |
|---|---|---|---|---|
| `deep` | `.deep-host [data-v-x] .deep-target` — space before the scope attribute (Vue: `.deep-host[data-v-x] .deep-target`, no space). Selector no longer matches any real DOM — attribute is required on a *separate descendant* of `.deep-host`, which doesn't exist. | `crates/verter_compiler/src/css/prepass.rs:249-296` (`transform_deep`) — inserts `DEEP_MARKER` as a leading token regardless of whether `:deep(` was preceded by a combinator space; only works when `:deep(` is directly suffixed to the preceding compound (confirmed: `deep-compound` passes because there's no space in that source). | (a) genuine defect | Structurally yes (J1 floor names "Native Vue scoped/deep/global/slotted/keyframe transforms" per ruling §5) — **but not fixed by the plan as currently drafted**: no charter case names this edge (`:deep()` as its own space-separated segment vs. suffix) |
| `global-mixed-local` | Verter keeps `.local-host` / `.local-tail` around the unwrapped `:global()` content; real Vue's `:global()` **discards everything outside the parens** in that compound selector (verified against live `@vue/compiler-sfc`: `.foo :global(.bar) .baz` → `.bar{...}`, prefix/suffix dropped entirely). | `crates/verter_compiler/src/css/scoped.rs:98-126` (`transform_global`) — string-splices out only the `:global(` wrapper tokens, preserves surrounding selector text verbatim | (a) genuine defect | Same as above — named in J1 floor, not in any drafted case |
| `is-selector-list` | Verter injects `[data-v-x]` *inside* the `:is()` argument list (`.beta[data-v-x]:hover`), narrowing the match. Real Vue only scopes the host, leaving `:is()` members unscoped. | `crates/verter_compiler/src/css/scoped.rs:144-198` (`add_scope_to_selector`'s combinator scan) — splits on bare space/`>`/`+`/`~` bytes with **no parenthesis-depth tracking**, so the space inside `:is(.alpha, .beta:hover)` is misread as a descendant combinator, producing a second "last segment" that gets scoped too | (a) genuine defect | Same — J1 floor names `:is`/`:where`? No — ruling §5's floor list only says "deep/global/slotted/keyframe", **`:is()`/`:where()` are not named in the J1 minimum floor at all** |
| `where-selector-list` | Same mechanism as `is-selector-list` (`:where()` uses the identical unparenthesized split). | Same as above | (a) genuine defect | Same — **not named anywhere in the ratified floor** |
| `scoped-keyframes` | `@keyframes fade` is emitted completely unrenamed; Vue renames to `fade-{scopeId}` and rewrites `animation`/`animation-name` to match. Not a partial bug — the rename pass **does not exist**. | No renaming logic anywhere in `crates/verter_compiler/src/css/{walk,scoped,prepass,mod}.rs`; `walk.rs:109-118` explicitly *skips* keyframe body selectors from scope-injection (correct) but nothing renames the `@keyframes` identifier or its references | (a) genuine defect (missing feature) | J1 floor explicitly names "keyframe" — the *concept* is scoped, but no drafted case yet requires the identifier-uniqueing behavior itself |
| `v-bind`, `v-bind-multiple`, `v-bind-quoted` | JS registers `"--{hash}-color"` (prefix baked in); Vue's real `useCssVars` runtime always prepends `--` itself, so the DOM custom property actually becomes `----hash-color` — **the binding never applies at runtime in a real browser**, confirmed against live `@vue/compiler-sfc` (`"v70215e88"`, no `--`). | `crates/verter_compiler/src/css/types.rs:33-43` (`generate_var_name`) bakes `--{scope}-{expr}` and the identical string is reused verbatim as the JS object key at `crates/verter_compiler/src/script/css_vars.rs:60-62` — one name serves two roles that must differ | (a) genuine defect, highest severity (silently broken at runtime, not just cosmetic) | J1 floor names "Native authored `v-bind()` analysis/transform where proven safe" — covers it in principle, no drafted case yet |
| `slotted-compound` | `.slot-child:hover[data-v-x-s]` vs Vue's `.slot-child[data-v-x-s]:hover` — attribute placed after the pseudo-class instead of before. | `crates/verter_compiler/src/css/prepass.rs:357` (`transform_slotted`) always appends the marker at the end of `inner`, regardless of trailing pseudo-classes | (c) probe encodes byte-order as the check, but CSS compound-selector matching is order-independent for simple selectors — `.a:hover[b]` and `.a[b]:hover` match an identical element set. Under the project's own Compiled-Output Conformance doctrine (behavioral/structural parity, not raw bytes) this is arguably **not** a behavioral defect, though it is a real deviation from Vue's exact output | Ambiguous — depends whether J1's acceptance bar is byte-parity or behavioral-parity for CSS (directive text leans toward "preserve authored bytes," doesn't explicitly rule on Verter-vs-Vue *rewritten* byte order) |

7 probes pass cleanly: `scoped`, `slotted`, `global`, `css-modules`, `deep-compound`, `media-scoped`, `supports-scoped`.

**Bonus finding (not a probe failure, but corroborating evidence):** the `media-scoped` case passes the probe but Verter silently rewrites `@media (min-width:1px)` → `@media (width >= 1px)` — lightningcss's modern-syntax normalization altering authored bytes, exactly the behavior `MAINTAINER-DIRECTIVE-CSS-CLEAN-CUTOVER.md:152-170` ("No CSS normalization or printing") forbids and Track J1 removes by deleting lightningcss. Confirms the defects above aren't isolated — the whole legacy pipeline actively rewrites authored CSS beyond scoping.

**Corroborating case:** `style-preprocessor-gates.mjs`'s `scss-deep-media-scoped` case compiles SCSS externally (correctly, via JS `sass` — matching the ratified external-preprocessor boundary) then feeds plain CSS containing `&:hover :deep(.pre-scss-external)` into the same `processStyle` path. The `&:hover` → `.pre-scss-deep:hover` expansion puts a space before `:deep(`, so this SCSS-gate case hits the exact same `deep` root cause. Not independently re-run; same code path, same bug.

### The gap list — what's still missing after J1–J4 as currently ratified

None of the 6 genuine-defect categories (`deep`-with-space, `:global()` context-discarding, `:is()`/`:where()` non-recursion, keyframe renaming, v-bind double-dash) are locked in as acceptance criteria today — `docs/arch/refactor/rev11/charters/J1.template.md` is a boilerplate template with zero CSS-specific content; no J1/J4 charter has been drafted yet. The ratified ruling's J1 "minimum floor" (§5) names the *feature categories* (scoped/deep/global/slotted/keyframe transforms, v-bind analysis) but not these specific edge cases, and the *existing* Rust test suite already had the right input to catch two of them and didn't (`scoped.rs:686` `test_is_pseudo_class` uses `.item:is(.a, .b)` — the exact shape that leaks scoping into the argument list — but only asserts the prefix is present, never that the argument list stays unscoped; `walk.rs`'s keyframes tests check selectors *inside* `@keyframes` aren't scoped, never that the keyframe *name itself* gets renamed). That's a live instance of the "non-discriminating test" pattern — the plan should not assume today's Rust unit tests are a safety net for these five bugs when the new engine lands.

Concretely, absent explicit charter action, expect **all of the following to reappear** in the `StyleSyntaxIr`-based rewrite unless named explicitly:

1. **`:deep()` used as its own space-separated selector segment** (vs. suffixed directly to a compound) must attach the scope attribute to the *preceding* compound with no combinator space — needs a discriminating test distinct from the compound form.
2. **`:global()` composed with surrounding local selector segments** must discard everything outside the parens (matches Vue's real, non-obvious behavior) — needs an explicit test; today's only test (`scoped.rs:370`) covers the no-surrounding-context case only.
3. **`:is()`/`:where()` argument lists must never receive scope-attribute injection** — needs a test asserting the *absence* of `[data-v-...]` inside the parens, not just presence before them.
4. **Scoped `@keyframes` identifier uniqueing** (rename + rewrite `animation`/`animation-name` references) is a feature that must be built from scratch, not migrated — it doesn't exist today. This needs to be an explicit J1 (or J4) deliverable, not assumed inherited.
5. **`v-bind()` CSS-variable JS registration key must never include the `--` prefix**, even though the emitted CSS `var()` reference does — needs an explicit cross-check test (CSS var name vs. JS-registered key, checking the *absence* of double-prefixing), because this is exactly the kind of bug that's invisible unless you check both artifacts together (which is what the vue-benchmarks probe does and the current Rust unit tests don't).
6. **`:global()`/`:is()`/`:where()` are not named anywhere in the ratified J1 floor text at all** — recommend the charter draft explicitly add them alongside deep/global/slotted/keyframe, since they're a distinct forwarding-selector semantic class that the current code already gets wrong.

Recommend these six become explicit, source-cited acceptance criteria in the J1 charter (with discriminating tests modeled directly on `STYLE_FEATURE_CASES`, which is a good-quality, freely reusable probe suite) rather than left implicit under "scoped selectors" / "deep" as general category names.

### Could not determine / would need more work

- Whether `slotted-compound`'s token-order deviation is considered in-contract or out-of-contract under the CSS directive is a **judgment call for the maintainer/architect**, not something the evidence settles — I've given the behavioral-equivalence argument for treating it as low-severity, but the directive's general "preserve bytes" language could be read either way for Verter's own rewritten output (as opposed to *authored* bytes, which aren't at stake here since scoping always rewrites this selector).
- Did not exhaustively run the full `style-preprocessor-gates.mjs` suite (SCSS/Sass/Less/Stylus × scoped/deep/v-bind/CSS-modules matrix) — traced one corroborating case analytically instead of executing, to avoid an unnecessary `sass`-dependent Node run; the underlying root causes are already pinned from the smaller suite and would need to be re-verified only if the maintainer wants exact per-case confirmation across preprocessors.
- Did not run the actual `pnpm confirm`/`pnpm bench` CI harness end-to-end (would require full fixture generation + potentially a cargo/native rebuild under this machine's single-gate-at-a-time constraint) — relied on a targeted direct repro against the existing native binary, which is evidentially equivalent for these findings since it uses the identical public API surface (`processStyle`, `compileMany`) the harness calls.