# Vue VDOM Parity — Post-Merge Backlog

> Logged per maintainer directive (2026-07-19): stop the pre-merge fix-loop; record these errors to be fixed **after** the merge. The Vue conformance-goldens suite (`crates/verter_vue_conformance`) + adversarial reviews surfaced these. They are TRACKED here and (behaviorally) in `corpus/known-divergences.json`.
>
> Scope note: the maintainer chose "fix common-pattern bugs, track the rest" then revised to "log all, fix post-merge." So NONE of the below were fixed pre-merge; this is the actionable post-merge list. Each item names the official 3.6.0-rc.1 behavior, Verter's wrong behavior, location, and fix approach.

## Legend
- **REGRESSION** = introduced/exposed by the defineComponent-gate + inline-template work (93fc14423 / 97033f2b8 / 72714af27 / 36be9089a / e877e448d). Fix first post-merge — these are newly-broken cases.
- **PRE-EXISTING** = a parity gap Verter always had; the conformance surfaced it.
- **GUARD-DEBT** = architecture-guard violation from the conformance crate itself (gate-breaking, not behavioral).

---

## A. REGRESSIONS (from the gate + inline work) — fix first

### D1 — `defineOptions` referencing a setup-local ref → runtime ReferenceError  [BLOCKER]
`macros.rs:294-312`, `process.rs:626-641`. The gate-fix (36be9089a) captures `defineOptions`'s raw argument and hoists it OUTSIDE `setup()`. If the argument references a `<script setup>` local (e.g. `defineOptions({ name: someLocalRef })`), the hoisted reference hits a TDZ/ReferenceError at runtime.
- Official: emits a COMPILE ERROR — "defineOptions() in <script setup> cannot reference locally declared variables". Non-local refs (imports, module scope, literals) are valid.
- Fix: detect a setup-local reference in the defineOptions arg and emit the matching compile error instead of broken JS.

### D2 — inline template `ref="el"` emits a static string, not the setup ref binding  [BLOCKER]
VDOM element/hoist path. In inline mode, `<div ref="el">` with `const el = ref()` in setup should compile to the official ref-binding form (`ref_key: "el", ref: el`) so the ref object receives the element. Verter emits a static `"el"` string → the ref never populates.
- Fix: in inline mode resolve a template ref to the setup ref binding per official `compileScript({inlineTemplate:true})`. Determine whether non-inline is also affected (likely inline-only regression).

### D4 — inline setup context missing `attrs`/`slots`  [HIGH]
`process.rs:672-687`. Official inline `setup(__props, { expose, attrs: $attrs, slots: $slots, emit })` injects `attrs`/`slots`; Verter's inline setup omits them, so `$attrs`/`$slots` usage breaks in inline mode.
- Fix: inject `attrs`/`slots` into the inline setup destructure matching official's condition (always vs on-use).

### D6 — `__returned__` over-elides unused imports (false "matches official" claim)  [MEDIUM]
`process.rs:753-791`. `build_returned_object` + `template_used_vars` elides UNUSED setup imports from `__returned__`; official 3.6.0-rc.1 INCLUDES them (`get unusedHelper() { return unusedHelper }` getters). Not a leak (over-elision). The gate-fix's comment/claim that elision "matches official" is FALSE.
- Fix: either (a) match official (include all setup bindings incl. unused imports as getters) — production-faithful; or (b) keep the over-elision as an intentional documented divergence and correct the false claim. Prefer (a).

---

## B. PRE-EXISTING VDOM parity gaps — fix post-merge

### D3 — reactive props destructure not implemented  [BLOCKER, common pattern]
`const { foo = 1, bar: renamed, ...rest } = defineProps<...>()` — Verter does NOT route destructure DEFAULTS through `_mergeDefaults` into the runtime props options, nor rewrite references to `__props.foo` (reactive). Wrong for EVERY destructure-default SFC (a ubiquitous Vue 3.5+ pattern). Related: script keeps `console.log(foo)` on destructured locals (stale/non-reactive) vs official `__props.foo`.
- Official: defaults → `_mergeDefaults(<props>, { foo: 1 })`; references → `__props.foo`; aliases → `__props.bar`; rest → `_createPropsRestProxy(__props, ["foo"])`. Mutually exclusive with `withDefaults`.
- Fix: extract destructure defaults → `_mergeDefaults`; add binding-metadata marking each destructured prop as `props`-reactive so the existing setup/template reference-rewrite emits `__props.x`; handle alias + rest. Mode-1 runtime, inline + non-inline. A full implementation brief exists at (job tmp) `vue-d3-props-destructure-brief.txt`.
- NOTE: check whether the IDE/TSX (mode-2) path also lacks reactive-props-destructure TYPE handling — likely a separate item.

### Nested / computed props destructure not structurally rejected  [MEDIUM, structural-validation gap]
`const { x: { y } } = defineProps(...)` (nested pattern) and `const { [k]: y } = defineProps(...)` (computed key) — official `processPropsDestructure` REJECTS both with structural compile errors (`defineProps() destructure does not support nested patterns.` / `defineProps() destructure cannot use computed key.`; `@vue/compiler-sfc` 3.6.0-rc.1, `processPropsDestructure`). Verter ACCEPTS these illegal patterns (no structural validation). This is a false NEGATIVE distinct from D3: D3 is the reactive-runtime `_mergeDefaults`/`__props` lowering; this is INPUT validation. Surfaced during the macro scope-check reconciliation, which faithfully mirrors official's DETECTION/gating (peel wrappers before `isCallOf`; `propsDestructureDecl` = `!isWithDefaults && declId.type === "ObjectPattern"`) but not official's `processPropsDestructure` per-property structural checks.
- Fix: in the props-destructure walk (`macro_type_diagnostics.rs`), emit the official messages when an `ObjectProperty` value is a nested pattern (a non-Identifier `AssignmentPattern.left` or a non-Identifier / non-AssignmentPattern `ObjectProperty.value`) or the key is computed / unresolvable, matching `processPropsDestructure`'s `ctx.error(...)` arms. Diagnostic-only; Mode-1.

### D5 — event handlers flagged as dynamic PROPS  [MEDIUM]
Stable method refs / simple handlers emit `_hoisted_1 = ["onClick"]` + `9 /* TEXT, PROPS */`; official emits TEXT-only patch flags + `_cache[0] || (_cache[0] = $event => …)` where applicable (no `onClick` in dynamic-props keys arrays). Runtime usually works; optimizer/patch topology diverges. Same family as the tracked `PROPS vs NEED_HYDRATION` conformance cells.
- Fix: cache stable handlers via `_cache`; don't route them through dynamic PROPS keys.

### v-text — expression loss  [surfaced during V1b]
`v-text` binding expression is dropped/mishandled (a real pre-existing bug the inline path faithfully inherits). Affects both inline + non-inline. Needs the exact repro isolated from the conformance cell that surfaced it.

### `ref_for: true` missing for refs inside `v-for`  [PRE-EXISTING, whole VDOM element path]
Official emits `ref_for: true` on the props object for ANY ref (static or dynamic) inside a `v-for` scope (the runtime collects ref ARRAYS per iteration). Verter emits it nowhere — including our D2 inline `ref_key`/`ref` fix, which is still incomplete in lists: a `ref="el"` inside `v-for` binds only the LAST element instead of an array.
- Official: `transformElement` sets `ref_for: true` (with `ref_key`/`ref` for inline bindings) whenever `hasRef && inVFor`.
- Fix: thread the v-for scope flag into the element props emission and add `ref_for: true` (both inline and non-inline ref shapes).

### Ref patch-flag taxonomy — base flag wrong  [PRE-EXISTING]
`compute_patch_flags` (`props.rs`) maps `ref` → NEED_HYDRATION (32), but official marks ANY ref-bearing element NEED_PATCH (512) so the diffing traversal updates the ref. Our D2 fix only ORs 512 for the NEW dynamic shapes (inline `ref_key`/`ref` binding, dynamic `:ref`); the static hoisted-string case still gets 32. Runtime ref updates on stable ref elements can be skipped/mis-patched.
- Fix: `has_ref → PATCH_NEED_PATCH` for every ref-bearing element (static hoisted-string case included), not just the dynamic shapes.

### Inline `$props` / `$emit` routing  [PRE-EXISTING — broader inline-setup-context gap]
Official inline injects `const $props = __props` (template `$props` use) and `emit: $emit` destructure (template `$emit` use, when no defineEmits) / `const $emit = __emit` (with defineEmits), and template `$props`/`$emit` references resolve to those. Verter's D4 fixed only `attrs`/`$slots`; inline `$props`/`$emit` references still route `_ctx.$props` / `_ctx.$emit`. Likely works at runtime (instance proxy) but structurally divergent from official inline.
- Fix: extend the D4 `buildDestructureElements` port to the `$props`/`$emit` builtins (on-use), with the resolver emitting bare `$props`/`$emit` in inline mode.

### PascalCase `<Component :is>` not treated as a dynamic component  [PRE-EXISTING, LOW]
Official's `isComponentTag` is `tag === "component" || tag === "Component"` — BOTH the lowercase `component` and the PascalCase `Component` tag are dynamic-component hosts. Verter's VDOM dynamic-component detection (`is_dynamic_component_tag` / `resolve_dynamic_component`, `crates/verter_compiler/src/template/code_gen/vdom/component.rs`) accepts only lowercase `"component"`, so `<Component :is="x">` compiles as a regular component named `Component` carrying an `:is` prop instead of `_resolveDynamicComponent(x)` + an open block. Pre-existing — NOT introduced by the C1 dynamic-component-block fix (0f4415c23); surfaced by the grok adversarial review of C1. Verter SSR already accepts both spellings, so this is a VDOM-only gap. Low severity: uncommon SFC spelling, and the miss is consistent on both resolve + blockify (no resolve/block disagreement — just not full official parity).
- Fix: accept both `"component"` and `"Component"` in the VDOM dynamic-component detection (mirror official `isComponentTag`); the C1 block-forcing then follows automatically. Add a discriminating test (`<Component :is>` → `_resolveDynamicComponent` + block) with a negative assertion.

---

## C. Low / infra

### D7 — host/FFI cannot set `inline`; `result.inline` misleading on IDE  [LOW]
`CompileProfile.inline` exists (`types.rs:1238`) and the conformance harness uses it, but `FfiCompileProfile`/`HostCompileProfile` don't map it (`verter_ffi/.../input.rs:69-128`; `packages/native/host-types.ts:71-96`) — only `isProduction` is exposed, so a host can't force inline in dev. Also `IDE` + `inline: Some(true)` sets `result.inline = true` while emitting only TSX (flag not gated on "runtime inline actually happened").
- Fix: map `inline` through the FFI/host profiles; gate `result.inline` on runtime-inline actually occurring.

### Valueless-`:is` / static-`is` multi-root block emission lacks dedicated tests  [LOW, test coverage]
The C1 fix (0f4415c23: dynamic `<component :is>` always opens a block) covers three forms via shared logic (`is_dynamic_component_tag` mirrors `resolve_dynamic_component`): bound `:is="expr"` / `v-bind:is`, valueless `:is` (Vue 3.4 shorthand), and static `is="Foo"`. Only the bound `:is="expr"` MULTI-ROOT case has a dedicated block-emission test; the valueless-`:is` and static-`is` multi-root forms are transitively covered (same code path) but NOT explicitly pinned.
- Fix: add dedicated tests asserting that a valueless-`:is` and a static-`is` dynamic component in a MULTI-ROOT context each emit `(_openBlock(), _createBlock(_resolveDynamicComponent(...)))` and NOT a bare `_createVNode(_resolveDynamicComponent(...))`. Low priority — completeness, not a correctness gap.

---

## D. Architecture-guard debt (GATE-BREAKING, from the conformance crate) — needed before the branch's gate is green

These trip `node scripts/gate.mjs` (the conformance crate is entirely new — absent at base 6f816df83, so these are ours). They are NOT behavioral; they break the gate:
- `canon.rs` is 1785 lines → file-size/extraction guard. Split into modules.
- `assemble_vue_main_module` made `pub` (Slice-2b real-assembler coupling) → verter_session pub-surface snapshot drift + `lib.rs` line ceiling. Update the snapshot / trim, or reconsider the coupling.
- `std::fs` in `verter_vue_conformance/src/lib.rs` → std::fs guard. Route through an allowed API or exempt the test/conformance crate (with justification).
- `generator_smoke.rs` archaeology → no-phase-archaeology guard. Rescrub.

External (NOT ours — the LSP branch / base): 9 `verter_lsp` real_provider + 3 `verter_relay_shim` + 2 `verter_lsp` std::fs failures pre-exist on the base.

---

## Verified-SOLID (do NOT re-open — adversarially confirmed correct)
- Companion default-export merge (36be9089a): non-literal `__default__` rebind, `defineComponent(...)` companion, expose/model, TS spread order, companion + `defineOptions` — all match official.
- Inline binding resolution (simple): ref `.value`, `__props.x`, computed, v-for shadowing, method+interpolation.
- Helper dedup / single vue import line; inline + companion + defineOptions composition.
- `filter_setup_return` removal (no unused-import leak; compiler-owned elision still runs — the divergence is D6 over-elision, opposite direction).
- Dual-corpus integrity: 29 inline goldens are real official `inlineTemplate:true`; the 7 PASS cells are genuine; the 22 tracked divergences are real (not false-greens).
- Render-arity (97033f2b8): no counterexample found.
- Mode isolation: IDE/TSX (mode-2) + TSC (mode-3) unaffected by the gate/inline changes.

### `export const {…} = defineProps` in `<script setup>` not rejected  [PRE-EXISTING, lint surface]
Official rejects a `<script setup>` `export const { x } = defineProps(...)` with "cannot contain ES module exports"; Verter leaves the export. Separate `no-export-in-script-setup` lint surface (round-9 finding) — not part of the macro scope-check reconciliation. Pre-existing.

### Non-top-level macro calls skip scope-check  [PRE-EXISTING]
`if (true) { defineProps(...) }` / a macro call nested below the top level — both Verter and official's scope-walk are top-level only, but prop-extraction differences predate the peel/gate work (round-9 finding). Pre-existing; documented for completeness.
