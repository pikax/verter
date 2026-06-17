# Native Svelte Compiler — Program Plan (source `.svelte` → JS importing `svelte/internal/client`)

Verter gains a NATIVE Svelte 5 RUNTIME compiler over the existing shared carrier substrate — never
a parallel pipeline, never a runtime of its own. **A compiler converts a `.svelte` source file into
a `.js` module — nothing more. The runtime IS Svelte.** The emitted module imports the real Svelte
runtime directly from `svelte/internal/client` (and `svelte/internal/server` for SSR), exactly as
Verter's Vue compiler emits `.vue` → JS that imports `vue`. Verter implements NO Svelte reactive
runtime and ships NO `@verter/svelte-runtime` facade package; the bundler/browser/`svelte` package
provides `svelte/internal/*`.

This document synthesizes the codex architect's ratified verdict (xhigh, gpt-5.5; confidence: high;
no disagreement with the framing) against the live tree on branch
`block/svelte-compiler-plan` (base `feat/framework-adapters-clean`). All file references were
verified against that tree. Every reconciliation is recorded in the Decisions Log (§8).

The ground-truth code citations below were RE-VERIFIED against `feat/framework-adapters-clean`
@ `3f5dc431` AFTER the lsp-perf integration landed. That integration added an upstream
framework-NEUTRAL parse lifecycle (`FrameworkParseArtifact` / `parse_carrier_counted` /
`ensure_indexed_ready_serve`) that AIDS Block 1 — the carrier-neutral `snapshot.framework_parse`
artifact is already threaded into `compile_entry`, currently opened only via `vue_parse` — but it did
NOT pre-land any runtime-codegen carrier work. Block 1's scope, deliverables, and "before" state are
therefore unchanged; the only effect on this plan is shifted line citations (≈ +170 lines).

---

## 1. Context

### 1.1 Why this change

Verter is a Vue compiler + LSP that converts SFCs to valid TSX (IDE type checking) and optimized
render functions, plus typeinfo/component-meta surfaces through ONE shared type-resolution engine.
The framework-adapter substrate (`docs/arch/multi-framework-adapters-plan.md`) already landed the
carrier seam, the registry, language routing, the two-pass script-fact seam, and the Svelte IDE TSX
projection.

**The Svelte IDE-TSX path is already complete and is NOT what is missing** (D5/§1.1 correction —
the earlier "IDE-TSX is missing" framing was wrong). Verified against the live tree:
`crates/verter_compiler/src/svelte/ide/projector/mod.rs::project_svelte_ide(source, &ParsedSvelte,
filename, skip_source_map)` generates valid TSX from the parsed Svelte AST;
`crates/verter_compiler/src/svelte/carrier.rs::SvelteCarrierCompiler::compile_ide()` gates on
`parsed_svelte(artifact)` and invokes the projector; its `eval_source()` blanks everything except the
instance + module script regions at their raw byte offsets (position-exact). Parse →
`eval_source` → projection ALREADY lands IDE-TSX for `.svelte`.

What is ACTUALLY missing is the SECOND codegen path — a RUNTIME compiler that emits an executable
`.js` module — and the live `compile_entry()` runtime path is ORPHANED for Svelte. This is the real
P0 (the "onclick throws" symptom). Verified at
`crates/verter_session/src/host_resolve/virtual_file_pipeline.rs::compile_entry()` (begins ≈ line
1873): it parses through the Vue adapter and then UNCONDITIONALLY calls `compile_from_parsed` /
`compile_sfc` (Vue's SFC compiler, the direct calls at ≈ lines 2030-2032) followed by
`assemble_main_module` (≈ line 2068, defined in `crates/verter_session/src/compile.rs:54`) — there is
NO framework dispatch. A `.svelte` file routed here is handed to the Vue
compiler and produces broken runtime output. The fix is the carrier-routed runtime path (Block 1),
NOT touching the working IDE-TSX projection.

The owner directive is a HARD CONSTRAINT that supersedes any prior facade-style design and the
substrate plan's Invariant 4 ("runtime codegen for non-Vue frameworks is out of scope"):

- Emit a `.js` module importing DIRECTLY from `svelte/internal/client`
  (+ `svelte/internal/disclose-version`, `svelte/internal/flags/async` as the pinned official
  compiler emits them), MATCHING the OFFICIAL Svelte 5 compiler output.
- NO `@verter/svelte-runtime` facade. NO Verter reactive runtime. The `svelte` npm package provides
  the runtime. (Confirmed: no such package exists in the tree today; this plan ensures none is ever
  built — §8 D-1, §9.)
- The OFFICIAL Svelte 5 compiler output is the CONFORMANCE TARGET (client and SSR).
- ANALYSIS + LINT work FIRST-CLASS for Svelte to Vue parity.
- Same-or-better quality + performance than Verter's Vue compiler.

### 1.2 The conformance target (the bar)

This input:

```svelte
<script> let name = $state('world'); let count = $state(0); </script>
<h1>Hello {name}!</h1>
<input bind:value={name} />
<button onclick={() => count += 1}>clicks: {count}</button>
```

must compile to output equivalent to the official Svelte 5 client output:

```js
import 'svelte/internal/disclose-version';
import * as $ from 'svelte/internal/client';
var root = $.from_html(`<h1> </h1> <input/> <button> </button>`, 1);
export default function App($$anchor) {
  let name = $.state('world'); let count = $.state(0);
  var fragment = root();
  var h1 = $.first_child(fragment); var text = $.child(h1); $.reset(h1);
  var input = $.sibling(h1, 2); $.remove_input_defaults(input);
  var button = $.sibling(input, 2); var text_1 = $.child(button); $.reset(button);
  $.template_effect(() => { $.set_text(text, `Hello ${$.get(name) ?? ''}!`); $.set_text(text_1, `clicks: ${$.get(count) ?? ''}`); });
  $.bind_value(input, () => $.get(name), ($$value) => $.set(name, $$value));
  $.delegated('click', button, () => $.set(count, $.get(count) + 1));
  $.append($$anchor, fragment);
}
$.delegate(['click']);
```

**Component-function name (verified empirically).** The exported function name is DERIVED from the
component filename, not hardcoded. `App.svelte` → `export default function App($$anchor)`; an unnamed
input (no `filename` passed to the compiler) → `export default function _unknown_($$anchor)`. Verified:
`compile(src, { generate:'client' })` with no `filename` emits `_unknown_`, while
`filename:'App.svelte'` emits `App`. The runtime path derives the function identifier from the
component filename (filename stem → a JS-identifier-sanitized name; `_unknown_` when no filename is
available) — see Block 4. An explicit `name` compile option OVERRIDES this derivation
(`2-analyze/index.js`: `options.name ?? component_name`) for both backends — verified
`name:'CustomName'` → `export default function CustomName` (§3.8); the Block 4 naming step takes the
resolved `name` from the Block 5m compile-options resolver.

The output-parity bar (§5, D-3): **FUNCTIONAL / behavioral equivalence on the pinned runtime, plus
structural / helper-family / call-topology parity with the official compiler.** Byte identity is NOT
the bar anywhere — Verter is not a formatting clone of the official compiler. "Equivalent" above and
throughout means: the emitted module imports the same `svelte/internal/*` helper families, makes the
same reactivity / hydration / CSR-vs-SSR decisions, and produces the same observable DOM behavior when
executed against the pinned runtime. Verified by the jsdom behavioral harness (§5/§7-block) on
`svelte@5.56.3` plus helper-topology goldens (structure + helper-call sequence, NOT bytes).

### 1.3 Ground truth (verified against the live tree)

- **Vue runtime codegen** (`crates/verter_compiler/src/template/code_gen/{vdom,vapor,ssr,shared}/`):
  `CodeGenMode {Vdom, Vapor, Ssr}` (`template/code_gen/mod.rs`); entry `generate_template()`
  dispatches to `VdomCodeGen` / `VaporCodeGen` / `SsrCodeGen`, each walking the SHARED template AST
  via `walker::walk_template()`, writing DEFERRED ops to a shared `CodeGenOutput`, then
  `CodeGenOutput::apply_to(&mut CodeTransform)`. VDOM emits `import {...} from "vue"`; VAPOR (the
  closest structural analogue to Svelte 5 client output — DOM-direct, template-cloned,
  effect-wrapped: `_template(...)`, `_child`, `_next`, `_setText`, `_renderEffect`,
  `_delegateEvents`, `_on`) emits from `"vue"`; SSR emits from `"vue"` + `"vue/server-renderer"`.
  Helper imports are tracked as BITFLAGS (`shared/helpers.rs`, `to_imports()` stable order).
- **`CompileTarget`** is BITFLAGS (`crates/verter_compiler/src/compile/types.rs`):
  `STYLE | SCRIPT | TEMPLATE | TSX | TSC | TEMPLATE_DATA`; presets `BUNDLER = STYLE|SCRIPT|TEMPLATE`,
  `IDE = TSX`, `ANALYSIS/META = SCRIPT|TEMPLATE_DATA`.
- **`VerterCompileResult`** (`compile/types.rs`): Vue-shaped — `script`, `template { code, imports,
  ssr_imports, source_map }`, `styles`, `tsx`, `template_data`. `runtime_module_name` threads
  through (default `"vue"`) via `resolve_runtime_module_name()`. Public entries `compile_sfc` /
  `compile_from_parsed` (`compile/mod.rs`).
- **Host module assembly** (`crates/verter_session/src/compile.rs::assemble_main_module`) is
  VUE-SHAPED: it emits style imports, template runtime imports, then a `_sfc_main` object with an
  attached `.render` / `.ssrRender`, then HMR. The official Svelte module is DIFFERENT (module-scope
  `var root = $.from_html(...)`, a single `export default function App($$anchor){...}`, module-scope
  `$.delegate([...])`) — so Svelte assembly is NOT this `_sfc_main` shape.
- **CarrierCompiler seam** (`crates/verter_compiler/src/framework_common/carrier_compiler.rs`): the
  trait owns `parse` / `eval_source` / `compile_ide` / `template_data` ONLY — NO runtime-codegen op.
  Vue impl `vue_bridge::VueCarrierCompiler`; Svelte impl `svelte/carrier.rs::SvelteCarrierCompiler`
  (parse/eval/IDE-TSX/template_data only). Registry `framework_common/registry.rs::
  CarrierCompilerRegistry::built_in()`. **The host RUNTIME path is NOT routed through the carrier
  today**: `crates/verter_session/src/host_resolve/virtual_file_pipeline.rs::compile_entry()` calls
  `compile_from_parsed` / `compile_sfc` directly (a hardcoded Vue call, ≈ lines 2030-2032), then
  `assemble_main_module` (≈ line 2068).
- **Shared Svelte parser** (`crates/verter_compiler/src/svelte/parser/`, ~2045 lines core):
  `parse_svelte(source) -> ParsedSvelte` (lossless, span-based, infallible). AST (`template_ast.rs`):
  `SvelteNode {Text, Comment, Interpolation, Element, Block, Tag}`; `SvelteElement {name, kind, attributes, children, open_span, close_span}`; `SvelteSpecialKind`; `SvelteAttribute {Plain, Spread, Directive(SvelteDirectiveKind)}`; `SvelteBlock {If, Each, Await, Key, Snippet}`; `SvelteTag {Render, Html, Const, …}`. Expression interiors are SPANS; runes are expression content recognized at the IDE layer via `is_rune()` (`ide/store_scan.rs`).
- **Svelte IDE path** (`svelte/ide/project_svelte_ide()`) emits TSX for type checking and MUST REMAIN
  UNTOUCHED (the two-codegen-paths rule).
- **Lint/diagnostics** (`crates/verter_diagnostics/`): `Linter`, `LintRule` trait, `RuleRegistry::
  builtin()` (~150 rules). Rules are HARDCODED Vue (Vue `TemplateAnalysisSnapshot` directives,
  `ScriptAnalysisSnapshot.macros`). NO framework abstraction; lint is NOT surfaced to the LSP.
- **Framework-adapter substrate** (`crates/verter_session/src/framework/`): `FrameworkAdapterRegistry`,
  `FrameworkAdapterDescriptor`; Svelte descriptor exists. Two-pass script-fact seam:
  `verter_semantic::analysis::framework_facts::svelte::SvelteScriptProvider` (captures runes: `$props`
  type, `$bindable` members, `Snippet` imports, legacy `export let`, `createEventDispatcher<E>()`);
  `verter_session::typeinfo::framework_surface::svelte_exec::resolve_svelte_surface()` resolves through
  the shared engine → `MacroSurfaceDtos`. `ComponentMetaAnalysis` is framework-agnostic; its INPUT
  (`ComponentMetaInput`) is Vue-macro-shaped.

---

## 2. Program-Level Invariants (hard; violating any is a STOP)

1. **No Svelte runtime, no facade.** Verter emits source → JS that imports `svelte/internal/client`
   / `svelte/internal/server` directly. No `@verter/svelte-runtime` package, no Verter reactive
   runtime, no facade indirection. The `svelte` npm package is the runtime authority. (D-1.)
2. **Official Svelte 5 compiler output is the conformance target**, pinned to the EXACT version
   **`svelte@5.56.3`** (the current lockfile version; verified `compiler.VERSION === '5.56.3'`).
   `svelte/internal/*` is intentionally internal, so the compiler is oracle-tested against THIS exact
   version — never a floating "latest". Bumping the pin is an explicit, reviewed oracle re-pin
   (re-pin → regenerate goldens → review the golden diff as the oracle delta; D-2, §5.3, §9). A
   CI lockfile-drift guard asserts the `svelte` lockfile version equals the pinned oracle constant.
3. **Two separate codegen paths.** The Svelte RUNTIME path is a SECOND path that consumes the SAME
   `ParsedSvelte` AST as the IDE path but is physically separate. `svelte/ide/project_svelte_ide()`
   stays UNTOUCHED; modifying one path must not affect the other (CLAUDE.md two-codegen-paths rule).
4. **Exactly ONE type-resolution engine.** Svelte analysis/component-meta resolve through
   `SemanticQueryKey` → `ProjectSemanticDispatch::execute` → `SemanticGraphStore` (five modes) via the
   existing `resolve_svelte_surface()` rail. No per-framework resolver, per-surface walker, or
   re-parse-and-resolve.
5. **CodeTransform is the sole mutation mechanism** for all generated output. Synthesized Svelte
   output emits through a `SvelteRuntimeOutput` accumulator that lowers into `CodeTransform` mapped
   insertions; no post-hoc string munging of `build_string()` output (sourcemap integrity).
6. **Typed-IR / typed-AST only.** Rune detection, binding-table construction, and the script-rune
   transform are AST-based (OXC for script/expression spans, the typed `ParsedSvelte` for the
   template). No regex on type/source text, no identifier-suffix classification, no
   synthesize-then-reparse.
7. **Vue compiler parse/codegen BEHAVIOR untouched.** No edits to Vue parser/codegen semantics in
   `verter_parser` / `verter_compiler`. Mechanical re-export updates and dispatch rehoming in
   `verter_session` are in scope and pinned by byte-identity characterization suites.
8. **Hermetic vendored fixtures** in all non-gated tests; the official-`svelte`-compiler oracle is
   feature-gated (`external-corpus` / a dedicated `svelte-oracle` gate). The default canonical run
   uses vendored goldens.
9. **Every new CRITICAL rule lands with a registered guard** (the R6 meta-guard at
   `crates/verter_session/tests/g_misc0/critical_rules_have_guards.rs`).

---

## 3. Codegen Architecture

### 3.1 A Svelte-specific runtime IR (not a Vapor reuse)

Svelte needs its OWN runtime lowering IR. Reusing Vapor's IR would leak Vue semantics — Vapor's
helper set, component shape, event API, binding model, and render ownership all differ from Svelte.
What is REUSED is Vapor's proven PATTERNS, not its IR: counter allocators, helper bitflags,
single-pass template walking, template-string synthesis, node-path planning, arena allocation, and
deferred `CodeTransform` application.

Add a sibling runtime compiler under `crates/verter_compiler/src/svelte/runtime/`:

| Module        | Responsibility                                                            |
| ------------- | ------------------------------------------------------------------------- |
| `client.rs`   | Svelte client backend (the `svelte/internal/client` emission).            |
| `server.rs`   | Svelte SSR backend (the `svelte/internal/server` emission).               |
| `ir.rs`       | Svelte-specific node-template + reactive-op IR.                           |
| `html.rs`     | Static HTML serialization + placeholder (whitespace/anchor) generation.   |
| `expr.rs`     | OXC-backed expression / rune rewriting. A `$.state`-family read → `$.get`, signal write → `$.set`/`$.update`; a bare-`$.proxy` value (no `$.state` wrapper) reads/writes as a PLAIN member access (`o.a`/`o.a++`) — see §3.3. |
| `helpers.rs`  | Svelte `$.` helper flags + the delegated-event set.                       |
| `sourcemap.rs`| Mapped generated-expression emission helpers.                            |

The client backend lowers `ParsedSvelte` into four regions:

1. **Module imports** — `import 'svelte/internal/disclose-version';` (unless `discloseVersion: false` —
   §3.8),
   `import 'svelte/internal/flags/legacy';` (when the component is NOT in runes mode — i.e. the explicit
   `runes` option is unset/`true`-less AND no runes are detected; the explicit `runes` option overrides
   inference — Block 5i, §3.2.1) and `import 'svelte/internal/flags/async';` (only when Svelte's experimental async flag is
   on — Block 5j, §3.2.2), `import * as $ from 'svelte/internal/client';`.
2. **Module hoists** — `var root = $.from_html(\`...\`, flag);` (the static template serialized once;
   the trailing flag is the official template flag).
3. **Component body** — `export default function App($$anchor) { ...instance script + node walk +
   effects + binds + events + $.append... }`.
4. **Module epilogue** — `$.delegate(['click', ...]);` when delegated events exist.

### 3.2 Official-output `$.` helper-call matrix (empirically derived from `svelte@5.56.3`)

This matrix is DERIVED from real `svelte@5.56.3` compiler output (`compile(src, {generate:'client'})`
and `{generate:'server'}`), NOT invented. It is the lowering-decision authority for Blocks 3-5/8;
the oracle (Block 2) regenerates it mechanically from the pinned compiler (§5.3). Every helper named
here was observed in actual compiler output. **Helper-name corrections from the empirical run** (the
review's guesses were partly wrong): `$effect` → **`$.user_effect`** (NOT `$.effect`); `$effect.pre`
→ `$.user_pre_effect`; `$effect.root` → `$.effect_root`; spread → `$.attribute_effect` (NOT
`$.spread_attributes`); `$props()` destructure → `$.prop` / `$.rest_props` / direct `$$props.x`;
component call is a DIRECT `Child($$anchor, {…})` invocation (NOT `$.component`, which is reserved for
`<svelte:component>`); a STATIC `{@render row(n)}` is a direct snippet call while a DYNAMIC
`{@render expr?.()}` uses `$.snippet`.

**Matrix scope — REPRESENTATIVE + oracle-regenerated, NOT a hand-maintained exhaustive enumeration.**
This matrix is a REPRESENTATIVE artifact mechanically REGENERATED by the oracle (Block 2, §5.3) from the
pinned `svelte@5.56.3` compiler — it is not, and is not intended to be, a hand-maintained exhaustive
table of every helper / every server form / every AST-context variant / every dev-mode shape. By design,
the deep per-surface detail is finalized at the OWNING block's STEP-0 (each block's STEP-0 regenerates and
PINS that block's exhaustive helper set, server/SSR forms, AST-context sensitivity, and dev-mode goldens
against the pinned compiler — §10). The KNOWN deeper cases that land at block-start (intentionally
block-scoped deliverables, NOT silent omissions) are:

- **Async-rune context-sensitivity → Block 5j.** The same async rune lowers differently by AST context:
  `$state.eager(n)` inline → `$.eager(() => n)` client / plain `n` server; `$effect.pending()` →
  `$.eager($.pending)` client / `0` server. 5j STEP-0 pins the per-context async helper set.
- **Special-element SSR / dynamic-title variants → Blocks 5f / 8.** `<svelte:head>` with a dynamic title →
  `$.deferred_template_effect`, static metadata → `$.template_effect`, SSR `$$renderer.title(…)`;
  `<svelte:boundary>` failed → `$$renderer.boundary`, pending-only markers. 5f/8 STEP-0 pin the
  special-element SSR matrix.
- **Dev-mode SSR module-shape → Block 5k.** `App.render` stub, `$$renderer.component` wrap, element-location
  helpers, dev `$inspect` server behavior. 5k STEP-0 pins the dev-mode SSR shape (the dev-mode axis,
  §3.2.3).

These are converted from "underspecified" into EXPLICITLY block-start-scoped — the matrix carries the
representative cell; the owning block's STEP-0 finalizes the exhaustive set.

**The full empirical `$state` classification rule (verified against `svelte@5.56.3`).** A `$state`
initializer lowers along TWO orthogonal axes — (a) reactivity of the BINDING itself (is it read/written
reactively?) and (b) the initializer's VALUE shape (object/array vs primitive). The lowering is the
cross-product, NOT a single `$.state` decision:

- **Non-reactive binding** (never read or written reactively in instance/template) → PLAIN `let x = …;`
  (no `$.state`, no `$.proxy`), regardless of value shape. Verified: `let n = $state(0)` read only once
  statically → `let n = 0;`.
- **Reactive primitive** → `let n = $.state(0);`; reads `$.get(n)`, writes `$.set(n, …)` /
  `$.update(n)` (`$.update(n, -1)` for `--`). Verified.
- **Object/array, deep-mutated but NEVER reassigned** → `let o = $.proxy({ … });` — a PLAIN `let` bound
  to `$.proxy(…)`, with NO `$.state` wrapper. The binding is not a signal; reads are plain member access
  (`o.a`), deep writes are plain (`o.a++`). Verified: `let o = $state({a:1})` (only `o.a` read/mutated) →
  `let o = $.proxy({ a: 1 });`; `let items = $state([])` → `let items = $.proxy([]);`.
- **Object/array, REASSIGNED (the binding itself is reactive)** → `let o = $.state($.proxy({ … }));` —
  `$.state` COMPOSED over `$.proxy`. Reads are `$.get(o).a`; reassign is `$.set(o, { … }, true)` (the
  trailing `true` flags a proxied-value set). Verified.
- **`$state.raw(x)` reassigned** → `let o = $.state({ … });` — `$.state` WITHOUT `$.proxy` (raw opts out
  of deep proxying). Reads `$.get(o).a`. Verified.

The transform must decide `$.state` / `$.proxy` / `$.state($.proxy(…))` / plain on the binding-table
reactivity classification AND the initializer value shape, not on the rune's presence alone. The
proxy-vs-signal read distinction is load-bearing for §3.3 / Block 4 (see the read-rewrite note there).

#### Runes (instance/module script)

| Surface | Official client helper(s) | Server (if different) | Verter block | Disposition |
| --- | --- | --- | --- | --- |
| `$state(prim)` (reactive primitive) | `$.state(x)`, reads `$.get`, writes `$.set` / `$.update` | plain `let` (no helper; SSR is non-reactive) | 4 | in-scope |
| `$state(obj/arr)` (deep-mutated, NEVER reassigned) | `let o = $.proxy({…})` — plain `let` bound to `$.proxy`, NO `$.state`; reads are plain member access `o.a`, deep writes plain `o.a++` | plain `let` | 4 | in-scope |
| `$state(obj/arr)` (REASSIGNED — binding reactive) | `let o = $.state($.proxy({…}))` — `$.state` composed over `$.proxy`; reads `$.get(o).a`, reassign `$.set(o, {…}, true)` | plain `let` | 4 | in-scope |
| `$state(x)` (never reactively read) | plain `let x = …` (no `$.state`, no `$.proxy`) | plain `let` | 4 | in-scope |
| `$state.raw(x)` (reassigned) | `let o = $.state({…})` — `$.state` WITHOUT `$.proxy` (raw opts out of deep proxy); reads `$.get(o)` | plain `let` | 5g | in-scope |
| `$state.raw(x)` (non-reactive) | plain `let x = …` (no proxy) | plain `let` | 5g | in-scope |
| `$state.snapshot(x)` | `$.snapshot(x)` | `$.snapshot` | 5g | in-scope |
| `$state.eager(fn)` | `$.eager(fn)` (the visited arg, wrapped in a thunk) | `() => …` (passthrough; non-reactive in SSR) | 5j (async-gated, §3.2.2) | in-scope (flag-gated) |
| `$derived(e)` / `$derived.by(fn)` | `$.derived(() => e)` / `$.derived(fn)`, read `$.get` | `$.derived(() => e)` / `$.derived(fn)` — the server visitor DOES emit `$.derived` (NOT a plain value); reads are a plain call. Async `$derived` (flag-ON) emits `await $.async_derived(() => …)` (§3.2.2). | 4 | in-scope |
| `$effect(fn)` | **`$.user_effect(fn)`** | omitted (effects don't run in SSR) | 4 | in-scope |
| `$effect.pre(fn)` | `$.user_pre_effect(fn)` | omitted | 5g | in-scope |
| `$effect.root(fn)` | `$.effect_root(fn)` | omitted | 5g | in-scope |
| `$effect.tracking()` | `$.effect_tracking()` | `false` (SSR is never inside a tracking context) | 5g | in-scope |
| `$effect.pending()` | `$.eager($.pending)` (an eager read of the pending-async count) | `0` (no pending async in sync SSR — server `CallExpression` visitor returns `b.literal(0)`, e.g. `$.escape(0)`) | 5j (async-gated, §3.2.2) | in-scope (flag-gated) |
| `$props()` destructure | `$.prop($$props,'name',flags,default)`, plain reads `$$props.foo` | `let { … } = $$props` (runes SSR uses native destructure defaults — NOT `$.fallback`; the legacy `export let` SSR path uses `$.fallback` + `$.bind_props` — §3.2.1) | 4 | in-scope |
| `$props()` rest | `$.rest_props($$props, rest_excludes)` (+ module `var rest_excludes = new Set([…])`) | — | 5g | in-scope |
| `$props.id()` | `$.props_id()` | `$.props_id($$renderer)` (the SSR form threads `$$renderer`; parity, not a typo) | 5g | in-scope |
| `$bindable(default)` (runes) | `$.prop($$props,'name', flags|bindable, default)` (a flag on `$.prop`, not a standalone helper) | NATIVE destructure default + `$.bind_props` — verified: `let { value = 'x' } = $$props;` … `$.bind_props($$props, { value })`. NOT `$.fallback` (that helper is the LEGACY `export let x = default` SSR path ONLY — §3.2.1) | 5g | in-scope |
| `$inspect(…)` | production: emitted as no-op (matches official); dev-mode: `$.inspect([...], fn, true)` | omitted | 5g (prod no-op) / dev-mode axis (§3.2.3) | in-scope (prod) |
| `$inspect(…).with(fn)` | production no-op; dev-mode: `$.inspect([...], (...$$args) => fn(...$$args))` (the `.with` callback wraps the inspector) | omitted | 5g (prod no-op) / dev-mode axis (§3.2.3) | in-scope (prod) |
| `$inspect.trace()` | production no-op; dev-mode: wraps the enclosing function body in `$.trace(() => label, () => { … })` (+ `import 'svelte/internal/flags/tracing'`) | omitted (SSR no-op) | 5g (prod no-op) / dev-mode axis (§3.2.3) | in-scope (prod) |
| `$host()` + `<svelte:options customElement>` | `$$props.$$host` + module `customElements.define(name, $.create_custom_element(Cmp, props, [], [], { mode }))` (+ `$.push`/`$.pop($$exports)` with `get/set` accessors) | n/a | 5h | in-scope |
| read of a `$.state` signal | `$.get(name)` | direct read | 4 | in-scope |
| read of a bare-`$.proxy` value (no `$.state` wrapper) | plain member read `o.a` (NOT `$.get(o).a`) | direct read | 4 | in-scope |
| write `count = v` / `+=` (signal) | `$.set(count, …)` (`$.set(o, v, true)` for a proxied reassign) | n/a | 4 | in-scope |
| write `count++` / `--` (signal) | `$.update(count)` (`$.update(count, -1)` for `--`) | n/a | 4 | in-scope |
| deep write of a bare-`$.proxy` value (`o.a++`) | plain `o.a++` (the proxy traps the mutation; no `$.set`/`$.update`) | n/a | 4 | in-scope |

#### Static template + DOM walk

| Surface | Official client helper(s) | Server | Verter block | Disposition |
| --- | --- | --- | --- | --- |
| static skeleton | hoisted `var root = $.from_html(\`…\`, flag)` (the trailing flag is the FRAGMENT flag — emitted as `1` for ANY multi-root template, NOT a top-level node count; verified `1` for both a 2-root and a 3-root template, and ABSENT for a single-root template) | `$$renderer.push(\`…\`)` (no template hoist) | 4 | in-scope |
| zero-element / block-only root | `var fragment = $.comment(); var node = $.first_child(fragment)` | comment markers `<!--[…-->` | 4 | in-scope |
| DOM refs | `$.first_child(fragment)`, `$.child(node[, true])`, `$.sibling(node[, N])`, `$.reset(node)`, `$.next()`, `$.text([init])` | n/a | 4 | in-scope |
| mount | `$.append($$anchor, fragment\|node)` | `$$renderer.push(…)` | 4 | in-scope |
| input normalizers | `$.remove_input_defaults(input)`, `$.remove_textarea_child(textarea)` | n/a | 4/5c | in-scope |

#### Reactive text / attributes / class / style

| Surface | Official client helper(s) | Server | Verter block | Disposition |
| --- | --- | --- | --- | --- |
| reactive text | grouped `$.template_effect(() => $.set_text(text, …))` | inline `${$.escape(v)}` in a `$$renderer.push(\`…\`)` | 4 | in-scope |
| static interpolation | `node.nodeValue = '…'` / `el.textContent = '…'` (no effect) | inline literal | 4 | in-scope |
| dynamic attr | `$.set_attribute(el, 'id', v)` | `${$.attr('id', v)}` (boolean attrs: `$.attr('checked', v, true)`) | 5a | in-scope |
| boolean DOM prop | direct `input.disabled = v` (no helper) | `$.attr('disabled', v, true)` | 5a | in-scope |
| `class:`/`class={…}` | `$.set_class(el, flag, baseClass, prevClasses, attrs, {name: cond, …})` | `${$.attr_class($.clsx(base), void 0, {name: cond, …})}` | 5a | in-scope |
| `style:`/`style={…}` | `$.set_style(el, prevStyles, attrs, {prop: v, '--x': '1'})` | `${$.attr_style(value, {prop: v, …})}` | 5a | in-scope |
| spread `{...x}` | `$.attribute_effect(el, () => ({...a, ...b}))` (+ `$.rest_props` for `{...rest}`) | `${$.attributes({ ...a, ...b })}` (the SSR spread helper is `$.attributes`, NOT `$.attr`) | 5b | in-scope |
| `{@html h}` | `$.html(node, () => h)` (anchored in a `$.comment()` fragment) | `${$.html(h)}` | 5b | in-scope |

#### Bindings (the `bind_*` family — full observed set)

| `bind:` target | Official client helper | Verter block | Disposition |
| --- | --- | --- | --- |
| `bind:value` (input) | `$.bind_value(el, () => get, ($$v) => set)` | 4 | in-scope |
| `bind:value` (textarea) | `$.bind_value` (+ `$.remove_textarea_child`) | 5c | in-scope |
| `bind:value` (select) | `$.bind_select_value(el, get, set)` | 5c | in-scope |
| `bind:checked` | `$.bind_checked(el, get, set)` | 5c | in-scope |
| `bind:group` | `$.bind_group(binding_group, [], el, get, set)` (+ `const binding_group = []` declared INSIDE the component function — verified component-fn-scoped, NOT module scope; module scope would share the binding group's state across every component instance, a correctness bug) | 5c | in-scope |
| `bind:this` (element) | `$.bind_this(el, ($$v) => set, () => get)` | 4 | in-scope |
| `bind:this` (component) | `$.bind_this(Child(…), set, get)` | 5c | in-scope |
| `bind:currentTime` / `bind:paused` / media | `$.bind_current_time` / `$.bind_paused` / … | 5c | in-scope |
| `bind:clientWidth` / dimensions | `$.bind_element_size(el, 'clientWidth', set)` | 5c | in-scope |
| `bind:innerHTML` (contenteditable) | `$.bind_content_editable('innerHTML', el, get, set)` | 5c | in-scope |
| `bind:open` (details) and other DOM properties | `$.bind_property('open', 'toggle', el, set, get)` | 5c | in-scope |
| `bind:innerWidth` (svelte:window) | `$.bind_window_size('innerWidth', set)` | 5c | in-scope |
| component `bind:prop` | getter/setter pair on the component props object (`get value()/set value($$v)`) | 5c | in-scope |

#### Events

| Surface | Official client helper(s) | Verter block | Disposition |
| --- | --- | --- | --- |
| delegated event (`onclick`, `onkeydown`, …) | `$.delegated('click', el, handler)` + ONE module-scope `$.delegate(['click', …])` (ordered set) | 4 | in-scope |
| non-delegated event (`onfocus`/`onblur`/window) | `$.event('focus', el\|$.window\|$.document.body, handler)` | 5d | in-scope |
| legacy `on:click\|preventDefault` modifier | `$.event('click', el, $.preventDefault(h))` (modifier wrappers `$.preventDefault`/`$.stopPropagation`/…) | 5d | in-scope (legacy) |

#### Store auto-subscriptions (`$store`)

The `$store` auto-subscription (a `$`-prefixed read/write of a `svelte/store` value) is a real official
feature with RUNTIME lowering — it works in BOTH runes and legacy components (verified). The store setup
+ teardown is COMPONENT-FUNCTION-scoped (the `$$stores` accessor table + `setup_stores` + the `$$cleanup`
call all live inside `export default function App(…)`), NOT module scope.

| Surface | Official client helper(s) | Server | Verter block | Disposition |
| --- | --- | --- | --- | --- |
| store read `$count` | accessor `const $count = () => $.store_get(count, '$count', $$stores)`, reads call `$count()` | `$.store_get($$store_subs ??= {}, '$count', count)` (with `var $$store_subs;` declared at body top) | 5i | in-scope |
| store write `$count = v` / `$count += 1` | `$.store_set(count, …)` (`$.update_store(c, $c())` for a `++`/compound on a module-scope store) | `$.store_set(…)` / `$.update_store($$store_subs ??= {}, '$c', c)` | 5i | in-scope |
| store setup + teardown | top-of-body `const [$$stores, $$cleanup] = $.setup_stores();` + a trailing `$$cleanup();` at the end of the component function (paired with `$.push`/`$.pop`) | `var $$store_subs;` at body top + a trailing `if ($$store_subs) $.unsubscribe_stores($$store_subs);` (no `setup_stores`/`$$cleanup` — the server uses the lazy `$$store_subs` object) | 5i | in-scope |

#### Blocks / components / snippets / special elements

| Surface | Official client helper(s) | Server | Verter block | Disposition |
| --- | --- | --- | --- | --- |
| `{#if}/{:else if}/{:else}` | `$.if(node, ($$render) => { if (c) $$render(branch); else … })` | `if/else` + `$$renderer.push('<!--[0-->')` (true) / `'<!--[-1-->'` (else) + closing `<!--]-->` | 5e | in-scope |
| `{#each}` keyed | `$.each(node, FLAG, () => items, (item) => key, ($$anchor, item) => {…})` — the `item` binding is a SIGNAL: reads inside the block are `$.get(item)` (e.g. `$.set_text(text, $.get(item).x)`, `() => ($.get(item).x++)`), NOT inert plain locals (§3.3) | `<!--[-->` + `const each_array = $.ensure_array_like(items)` + `for` loop pushing items + `<!--]-->` | 5e | in-scope |
| `{#each}` unkeyed | `$.each(node, FLAG, () => items, $.index, ($$anchor, item, idx) => {…})` — `item` binding is a `$.get` signal as in the keyed row (§3.3) | `$.ensure_array_like` + `for` (as keyed) | 5e | in-scope |
| `{#each}…{:else}` | `$.each(node, FLAG, …, eachFn, elseFn)` | `for` else-branch | 5e | in-scope |
| `{#await}` | `$.await(node, () => p, pendingFn, thenFn, catchFn)` — the `{:then x}` / `{:catch e}` bindings are SIGNAL reads (`$.get(x)`) inside their branch, like the each binding (§3.3) | `$.await($$renderer, promise, pendingFn, thenFn[, catchFn])` (the server emits the SAME `$.await` helper threading `$$renderer`, NOT a sync resolve) + a trailing `$$renderer.push('<!--]-->')` marker | 5e | in-scope |
| `{#key}` | `$.key(node, () => k, ($$anchor) => {…})` | re-render | 5e | in-scope |
| `{@const x = …}` (runes mode) | block-local `const x = $.derived(() => …)` (a derived memo over the block scope); reads `$.get(x)`. (Legacy non-runes mode emits `$.derived_safe_equal(() => …)` instead — §3.2.1.) | plain `const x = …` | 5e | in-scope |
| `{const x = …}` / `{let x = …}` (5.56 declaration tag — a DISTINCT AST node `DeclarationTag`, NOT `{@const}`) | a PLAIN block-local declaration `const x = …` / `let x = …` (an INERT local binding, NO `$.derived` memo, NO `$.get` read); the declarator may itself carry runes (`{let a = $state(0), b = $derived(a * 2)}` registers state transformers like an instance-script decl) and may be async (drives an async-declaration thunk) | plain `const x = …` / `let x = …` | 5e | in-scope |
| `{@debug a, b}` | `$.template_effect(() => { console.log({ a: $.snapshot(a), b: $.snapshot(b) }); debugger; })` (a reactive effect logging snapshots + `debugger`) | `console.log({ a, b }); debugger;` (sync, no effect) | 5e | in-scope |
| `{#snippet}` def | module/local arrow `const name = ($$anchor, p = $.noop) => {…}` | `function name($$renderer, p) {…}` | 5f | in-scope |
| `{@render name(args)}` static | DIRECT call `name($$anchor, () => arg)` | direct call `name($$renderer, arg)` | 5f | in-scope |
| `{@render expr?.()}` dynamic | `$.snippet(node, () => expr ?? $.noop)` | `$.snippet` | 5f | in-scope |
| `{@attach expr}` (5.29 attachment) | `$.attach(el, () => expr)` (the attachment fn runs as an effect on the element) | omitted (attachment effects don't run in SSR) | 5f | in-scope |
| component (static) | DIRECT `Child($$anchor, { foo: p, $$events: {…} })` | DIRECT `Child($$renderer, { foo: p, children: ($$renderer) => {…}, $$slots: {…} })` | 5f | in-scope |
| `<svelte:component this={C}>` | `$.component(node, () => C, ($$anchor, $$c) => $$c($$anchor, {}))` | dynamic call | 5f | in-scope |
| `<svelte:element this={tag}>` | `$.element(node, () => tag, false)` | dynamic tag | 5f | in-scope |
| `<svelte:self>` (recursive self) | DIRECT recursive call to the component's OWN function `Name(node, {…})` — no dedicated helper (self-recursion) | recursive `Name($$renderer, {…})` | 5f | in-scope |
| `<svelte:fragment slot="x">` (transparent slot group) | NO dedicated helper — lowers into a `$$slots: { x: ($$anchor, $$slotProps) => {…} }` entry on the enclosing component call | `$$slots: { x: ($$renderer) => {…} }` entry | 5f | in-scope |
| `<svelte:head>` | `$.head('<hash>', ($$anchor) => {…})` (the head content rendered into the document head, keyed by a stable hash) | `$.head('<hash>', $$renderer, ($$renderer) => {…})` (e.g. `$$renderer.title(…)`) | 5f | in-scope |
| `<svelte:document>` | `$.event(name, $.document, handler)` for events; binds via `$.document` (e.g. `$.bind_*(… $.document …)`) | n/a (document-scoped effects don't run in SSR) | 5f | in-scope |
| `<svelte:boundary>` | `$.boundary(node, { failed, pending }, ($$anchor) => {…})` (error/pending snippets passed as a props object) | `$$renderer.boundary({ failed }, ($$renderer) => {…})` | 5f | in-scope |
| `<svelte:window>` / `<svelte:body>` | `$.event(…, $.window\|$.document.body, …)`, `$.bind_window_size(…)` | n/a | 5f | in-scope |
| transition: / in: / out: | `$.transition(FLAG, el, () => fn[, getParams])` (FLAG: 1=in, 2=out, 3=both) | n/a | 5f | in-scope |
| `animate:` | `$.animation(el, () => fn, getParams)` | n/a | 5f | in-scope |
| `use:` action | `$.action(el, ($$node[, arg]) => fn?.($$node[, arg])[, () => arg])` | n/a | 5f | in-scope |

#### Module-shape, imports, flags

| Surface | Official client output | Server |
| --- | --- | --- |
| disclose-version import | `import 'svelte/internal/disclose-version';` (always, client) | absent |
| client runtime import | `import * as $ from 'svelte/internal/client';` | `import * as $ from 'svelte/internal/server';` |
| legacy-mode flag | `import 'svelte/internal/flags/legacy';` (emitted iff the component is NOT in runes mode — `analysis.runes = runes_option ?? inferred-from-rune-presence`, where `runes_option` is the explicit `<svelte:options runes>` or compiler `runes` option; the flag is emitted iff `!analysis.runes`. The explicit option OVERRIDES inference — `<svelte:options runes>` suppresses the flag with zero runes, and `runes={false}` forces it even WITH runes; §3.2.1) | absent |
| async flag | `import 'svelte/internal/flags/async';` (emitted only when experimental async is used — §3.2.2) | `import 'svelte/internal/flags/async';` — the server ALSO emits the async flag when experimental async is on (verified: a flag-ON `generate:'server'` compile emits it ahead of `import * as $ from 'svelte/internal/server';`); absent when the flag is off |
| component fn | `export default function Name($$anchor[, $$props]) {…}` | `export default function Name($$renderer[, $$props]) {…}` |
| `$.push`/`$.pop` | wraps the body ONLY for specific cases (`$.push($$props, runes_bool)` … `$.pop()`): a bindable prop (`$bindable`), a custom element (`<svelte:options customElement>`/`$host`), or a legacy-reactive / store-subscription context. NOT emitted for a simple `$props()` destructure / rest or a simple legacy `export let` (verified: both consume `$$props` yet emit NO `$.push`/`$.pop`) | n/a |
| delegated-event registry | module-scope `$.delegate(['click', …])` | n/a |

#### `<style>` compilation + CSS scoping (representative — full contract in §3.7)

| Surface | Official client output | Server | Verter block | Disposition |
| --- | --- | --- | --- | --- |
| scoped-class injection | matched element gains `class="svelte-<hash>"` in the `$.from_html` template | matched element gains `class="svelte-<hash>"` in the `$$renderer.push` HTML (SAME hash) | CSS-scoping (5l) | in-scope |
| `css: 'external'` (default) | NO style helper; CSS returned as a SEPARATE artifact `compile().css = { code: 'p.svelte-<hash>{…}', map, hasGlobal }` (rides `RuntimeCompileOutput.styles`) | same separate CSS artifact; no inline style helper | CSS-scoping (5l) | in-scope |
| `css: 'injected'` | module-scope `const $$css = { hash, code }` + body `$.append_styles($$anchor, $$css)`; `compile().css === null` (NO separate artifact) | module-scope `const $$css = { hash, code }` + body `$$renderer.global.css.add($$css)`; `compile().css === null` (NO separate artifact) | CSS-scoping (5l) | in-scope |

#### 3.2.1 Legacy-mode (non-runes `.svelte`) disposition

**Legacy-flag trigger (verified empirically + against the pinned source).** Runes-vs-legacy mode is
selected as `analysis.runes = runes_option ?? (inferred-from-rune-presence)`
(`2-analyze/index.js:351` resolves `runes_option`, `:454-456` is the `??` fallback), where
`runes_option` is the EXPLICIT `runes` compile option OR a `<svelte:options runes={true|false}>`
attribute (`read/options.js:31-33`, merged in `compiler/index.js`). The `import
'svelte/internal/flags/legacy'` import is emitted iff `!analysis.runes`
(`transform-client.js:547-548`). The explicit `runes` option therefore takes PRIORITY over inference:
- When the `runes` option is UNSET, mode is INFERRED from rune presence — a component with NO runes
  is legacy (flag emitted); ANY rune (e.g. one `$state`) makes it runes mode (flag suppressed). The
  inference fallback is rune-ABSENCE, not the presence of a specific legacy construct: verified, a
  runes-free component using ONLY `{#snippet}` + `{@render}` (no `export let`, no `$:`) STILL emits the
  legacy flag.
- When the `runes` option is SET, it OVERRIDES inference. Verified empirically against `5.56.3`:
  `<svelte:options runes={true}/><h1>hi</h1>` (and the compiler `runes: true` option) emits NO legacy
  flag DESPITE zero runes; `<svelte:options runes={false}/>` (and `runes: false`) FORCES the legacy
  flag even WITH a `$state` rune present.

The legacy constructs below (`export let`, reactive `$:`, `<slot>`, `createEventDispatcher`) are the
COMMON contents of an inferred-legacy component, not the flag trigger. Observed lowering:

- `export let name = 'world'` → client: `let name = $.prop($$props, 'name', 8, 'world')` (a function-call
  accessor `name()`), NOT a `$state` signal. SSR (`generate:'server'`): a default-valued legacy prop
  emits `let name = $.fallback($$props['name'], 'world')` (a default-prop fallback helper) and the
  component body ends with `$.bind_props($$props, { name, … })` to write back two-way bindings — both
  helpers are SSR-legacy-path specific and do NOT appear on the runes `$props()` SSR destructure.
- reactive `$: doubled = a * 2` → `$.mutable_source()` + `$.legacy_pre_effect(deps, fn)` +
  `$.legacy_pre_effect_reset()`; reactive statements use `$.deep_read_state`.
- `<slot name="x" />` → `$.slot(node, $$props, 'x', {}, fallbackFn)`.
- `createEventDispatcher()` → preserved import + `$.init()` in the body; dispatch is a plain call.

**Program disposition:** legacy non-runes mode is a REAL official-compiler feature, so for near
drop-in parity it is **IN-SCOPE**, sequenced as the dedicated legacy block (Block 5i, §10) AFTER the
runes client/SSR breadth lands (it depends on the binding-table and block infrastructure being
proven on runes first). Until Block 5i lands, a detected legacy component returns `CompileUnsupported`
with a precise diagnostic; that diagnostic is removed when 5i lands. The legacy helper set above is
the empirical target for 5i. Legacy is NOT a silent deferral — it has a block and a deletion entry
(§11).

#### 3.2.2 Async-gating disposition

Svelte's experimental async (async `$derived`/`{#await}`, plus the async-gated runes `$state.eager` →
`$.eager(fn)` and `$effect.pending` → `$.eager($.pending)`, under Svelte's own experimental flag) emits
`import 'svelte/internal/flags/async';`. Confirmed empirically: at `5.56.3` the official compiler did
NOT emit the async flag for the synchronous §1.2 corpus (so the §1.2 example correctly carries no
`flags/async` import).

**Disposition:** experimental async stays GATED behind Svelte's own experimental flag, MIRRORING
official — it is NOT dropped. When the flag is on, Verter emits `import 'svelte/internal/flags/async'`
and the async block helpers exactly as the official compiler does; when off, Verter emits neither
(feature parity with official's own gating). The flag-on async lowering is the dedicated async block
(Block 5j, §10). Until 5j lands, a component that requires the flag-on path returns
`CompileUnsupported`; that diagnostic is removed when 5j lands. Because this mirrors an OFFICIAL
experimental gate (not a Verter scope cut), it is the one feature that may stay flag-gated and still
count as drop-in parity (§9.1).

#### 3.2.3 Dev-mode codegen axis

The official compiler emits DIFFERENT output in dev mode (`dev: true`): validation wrappers,
`$.add_locations`, `$.inspect([...], fn)` for `$inspect` / `$inspect().with`, `$.trace(…)` for
`$inspect.trace` (+ `import 'svelte/internal/flags/tracing'`), and other instrumentation that becomes a
no-op or is absent in production. For TRUE drop-in parity, dev-mode codegen is its own axis and is
ACKNOWLEDGED here rather than silently dropped:

- **Production codegen is in-scope first** (the blocks below). Production output must match official
  production output, including the `$inspect` production no-op (Block 5g).
- **Dev-mode codegen is a named follow-on block (Block 5k, §10).** It mirrors official `dev: true`
  output (validation wrappers, `$.add_locations`, dev-mode `$.inspect` / `$.trace`). It is NOT a production
  feature gap — it is a separate output mode the official compiler itself gates on `dev`. It is
  in-scope (it has a block) but sequenced after production parity, since production is the drop-in
  baseline.

### 3.3 Script-rune transform

AST-based (OXC), never regex. The transform builds a BINDING TABLE from the instance + module
scripts:

- Rune variables: `$state`, `$derived`, `$props` destructures, `$bindable` members.
- **Signal vs bare-proxy classification (the §3.2 `$state` rule).** Each `$state` binding is classified
  into one of {plain `let`, `$.state` signal, bare `$.proxy`, `$.state($.proxy(…))`} from its reactivity
  + value shape. This classification drives read/write rewriting: a `$.state`-family binding read is
  rewritten to `$.get(name)` and a write to `$.set`/`$.update`; a BARE-`$.proxy` binding (object/array
  state that is deep-mutated but never reassigned, so it is NOT a signal) is read/written as a PLAIN
  member access (`o.a`, `o.a++`) and is NEVER wrapped in `$.get`/`$.set`. Mis-rewriting a bare-proxy
  read as `$.get(o).a` is a defect; Block 4 lands a discriminating test that FAILS against a
  proxy-blind rewriter (treating every `$state` as a signal) and PASSES against the
  classification-aware one.
- Mutable vs readonly signals (readonly `$derived` mutation is a lint error, §6 / Block 11).
- Props and bindable props.
- Lexical locals introduced by blocks/snippets/each/await (so generated reads/writes do not
  mis-rewrite a shadowing local).
- **Each/await-introduced bindings are SIGNAL reads, not inert plain locals (verified).** A keyed
  `{#each}` (and `{#await … then x}`) binding is itself a signal: the official output reads it as
  `$.get(item)`, e.g. `$.template_effect(() => $.set_text(text, $.get(item).x))` and
  `() => ($.get(item).x++)`. The binding table must mark each/await-introduced bindings as `$.get`
  signal reads so the scope-aware rewriter rewrites their reads/writes through `$.get`/`$.set` (and
  this interacts with the shadowing rule below — a shadowing local of the SAME name in an inner scope
  is NOT this signal and must not be rewritten). See the 5e rows in §3.2.
- Shadowing / error cases.

`CodeTransform` carries all source-derived script movement and rewriting (instance script hoisted
into the component body, rune calls rewritten in place). Synthesized output (the node walk, template
effects, bind table, event registration) emits through the `SvelteRuntimeOutput` accumulator that
lowers into `CodeTransform` MAPPED insertions, so generated references inside `$.get(...)`,
`$.set(...)`, template-effect bodies, and event handlers map back to the original expression spans
(`{name}`, `count += 1`) — token-precise sourcemaps (§5).

**PRIMARY Block-4 risk — scope / shadowing (D5).** The single highest-risk concern in the script-rune
transform is mis-rewriting an identifier that SHADOWS a rune binding. A rune signal `name` read as
`$.get(name)` must NOT be rewritten when a `{#each items as name}`, a snippet parameter
`{#snippet row(name)}`, an `{#await p then name}`, a `{@const name = …}`, a function parameter, or a
nested-block lexical `let name` introduces a same-named local that shadows it in that scope. The
binding table is therefore SCOPE-AWARE: it tracks the lexical scope chain (instance/module script,
each/await/snippet/key block bodies, arrow/function bodies, `{@const}` introductions) and a read/write
is rewritten to a `$.get`/`$.set` ONLY when the nearest binding in scope is the rune declaration, not
a shadowing local. Conversely, a `bind:value={x}` whose `x` is a shadowing local must bind the local,
not the outer signal. This is the leading correctness hazard; Block 4 lands with discriminating
shadowing tests (each-as shadow, snippet-param shadow, await-then shadow, `{@const}` shadow,
nested-fn-param shadow) BEFORE the rewrite logic, each FAILING against a scope-blind rewriter and
PASSING against the scope-aware one.

### 3.4 Two-codegen-paths fit

The runtime path consumes the SAME `parse_svelte` → `ParsedSvelte` AST that the IDE path consumes;
the two are physically separate (`svelte/runtime/` vs `svelte/ide/`). The IDE TSX projection is
untouched. This mirrors Vue's VDOM/Vapor (runtime) vs IDE split.

### 3.5 SSR

Svelte SSR is a SEPARATE backend (`svelte/runtime/server.rs`) targeting official
`svelte/internal/server` output. It SHARES expression lowering and some template analysis with the
client backend but does NOT share the client DOM IR blindly — it is the Svelte equivalent of
`CodeGenMode::Ssr`, not a Vue `CodeGenMode` extension. SSR gets its own golden + behavioral
string-render harness. SSR is first-class and WORKING, not a thin afterthought — see the dedicated
SSR block in §10 and the empirically-derived SSR helper set in the §3.2 server columns.

### 3.6 Static optimization stance (conformance-first; pre-lowering-IR only)

Verter keeps the analysis facts that power Vue's optimizations (e.g. prop-watch elision) for Svelte
diagnostics and future optimization, but the BASELINE Svelte compiler does NOT change runtime call
topology. The Vue "elide the prop watch" analogy is mostly NOT meaningful in Svelte 5: the official
compiler already lowers reactive reads fine-grained and already elides non-reactive machinery
(unused/non-reactive `$state` → plain `let`, static text, template hoisting, event delegation — all
observed in the §3.2 probe). The right framing is **semantics-level static optimization on top of an
official-compatible lowering contract**, not "prop-watch elision".

**The line (hard rule).** Verter may optimize the program BEFORE runtime lowering, on the typed /
template IR; it must NOT invent a competing Svelte runtime protocol AFTER lowering. The
`svelte/internal/*` protocol is internal, version-coupled, and co-designed with the runtime —
functional equivalence does NOT license fabricating alternate helper protocols. Structural
conformance comes FIRST; optimization beyond official is a separately-gated block (§10) that lands
ONLY after conformance is proven, inside the constrained envelope below.

**SAFE envelope (optimize BEFORE lowering, same decision applied to client AND SSR).** The
optimization runs on the SAME pre-lowering IR that feeds both backends, so client and server stay in
agreement. Safe classes — remove / fold only semantically-dead or compile-time-constant code:

| Optimization class | Safe? | Fail-closed trigger |
| --- | --- | --- |
| Literal constant folding | safe | — |
| Static text / static attribute folding | safe | — |
| Unreachable-branch removal (statically-false `{#if}`) | safe | branch condition not a compile-time constant |
| Dead-template-fragment removal | safe | fragment referenced by any binding/effect/ref |
| Unused-generated-helper cleanup | safe | helper reachable from any live emission |
| Duplicate-static-text merging | safe | — |
| Compile-time env folding | safe ONLY when env is an explicit cache dimension | env value not a declared cache dim |
| Closed-world cross-file constant propagation (private app-only components) | safe WITH extra proof | spread / dynamic-component / public export / unknown import / side effect |

**UNSAFE (never in scope, baseline or optimization block).** Changing live
`$.state`/`$.derived`/`$.effect`/`$.prop`/`$.template_effect` topology for observable values; skipping
lifecycle/effect/helper calls unless the whole branch is unreachable; hand-rolling DOM in place of the
Svelte helper protocols around hydration / bindings / actions / transitions / snippets / blocks /
components / delegated events; client-only optimizations SSR doesn't share; call-site prop-reactivity
specialization for exported / library components; cross-file component specialization. Every
optimization FAILS CLOSED on dynamic spread, unknown import, external component, side effects, or
unsupported syntax — falling back to the conformant lowering, never to a guessed simplification.

The narrow, separately-gated **Optimization block** (§10) carries the classification table above, the
gating infrastructure, the fail-closed rules, and the acceptance gate; it is sequenced strictly AFTER
the conformance blocks and the conformance/hydration gates are green.

### 3.7 `<style>` compilation + CSS scoping (first-class, empirically derived from `svelte@5.56.3`)

The `<style>` block is a first-class owned feature, NOT a styles-passthrough. The official compiler
SCOPES component CSS by injecting a stable `svelte-<hash>` class into both the matched template HTML and
the emitted CSS, and returns the compiled CSS as a SEPARATE artifact on the compile result. Verified
against `compile('<style>p{color:red}</style><p>hi</p>')`:

- **Scoped-class injection (both backends).** The matched template element gains a `svelte-<hash>` class
  in the serialized HTML — client `var root = $.from_html(\`<p class="svelte-n50uah">hi</p>\`)` and
  server `$$renderer.push(\`<p class="svelte-n50uah">hi</p>\`)`. The hash is the SAME across client and
  server for the same source.
- **CSS hash.** The `svelte-<hash>` class is derived from the component CSS via the official compiler's
  hashing algorithm; hash PARITY with the pinned compiler is part of the conformance bar (the CSS-scoping
  block's STEP-0 pins the hash algorithm against `svelte@5.56.3` goldens). The scoped selector is emitted
  as `p.svelte-<hash>{color:red}`.
- **`css` mode option — external (default) vs injected.** The mode toggle is symmetric across backends:
  `external` always produces a separate `compile().css` artifact (no injection); `injected` always emits
  the CSS inline via a backend-specific helper and sets `compile().css === null` (NO separate artifact).
  The source gate is `analysis.css.ast && !analysis.inject_styles` (`3-transform/index.js:48-51`) — when
  `inject_styles` is set (`css: 'injected'`), `compile().css` is `null` on BOTH backends.
  - **`css: 'external'` (default).** `compile().css` is `{ code: 'p.svelte-<hash>{…}', map, hasGlobal }`
    — a SEPARATE CSS artifact with its own source map, on BOTH backends. The client/server JS contains
    no inline style helper (the host/bundler is responsible for emitting/linking the stylesheet).
    Verified: client AND server emit the scoped class in the markup (`$.from_html` / `$$renderer.push`)
    and return the artifact; neither emits an injection helper.
  - **`css: 'injected'` — client.** The client JS emits a module-scope `const $$css = { hash:
    'svelte-<hash>', code: 'p.svelte-<hash>{…}' }` and the component body calls
    `$.append_styles($$anchor, $$css)`; `compile().css === null` (the CSS lives inline in the JS).
    Verified.
  - **`css: 'injected'` — server.** The server JS emits the SAME module-scope `const $$css = { hash,
    code }` and the component body calls `$$renderer.global.css.add($$css)`
    (`server/transform-server.js:305-310`); `compile().css === null` (no separate artifact). Verified.
    There is NO server CSS artifact in injected mode — the server delivers the CSS through the renderer's
    global CSS set, not a separate `compile().css` object.
- **CSS artifact on `RuntimeCompileOutput`.** The `styles` slot of the neutral `RuntimeCompileOutput`
  (§4.1) rides EXTERNAL mode only: when `css: 'external'`, the compiled CSS (`{ code, map, hash,
  has_global }`) populates `styles`, so the carrier-routed runtime path hands the CSS artifact to the
  host alongside the JS body. Under `css: 'injected'` there is NO separate artifact (`compile().css ===
  null` on both backends), so `styles` is empty; the CSS is delivered INLINE in the JS body — the client
  emits the `$$css` constant + `$.append_styles($$anchor, $$css)`, the server emits the `$$css` constant
  + `$$renderer.global.css.add($$css)` — mirroring the official toggle.

The CSS-scoping block (§10) owns hash-algorithm parity, scoped-class injection into the serialized HTML,
the `css` mode (external-vs-injected) toggle, the injected path (`$.append_styles` on client /
`$$renderer.global.css.add` on server), and surfacing the external CSS artifact on `RuntimeCompileOutput`. It mirrors how Vue's style flow reaches the bundler/unplugin (the
Vue `query.type === "style"` virtual-file sub-request path — §10.1): the Svelte CSS artifact flows to the
bundler/unplugin as a style virtual file analogously.

### 3.8 `<svelte:options>` + compile-option axis (first-class, empirically derived from `svelte@5.56.3`)

The official compiler's OUTPUT is shaped by a full axis of compile options — the public `CompileOptions`
/ `ModuleCompileOptions` (the authoritative type `svelte/types/index.d.ts`, `CompileOptions` ~line 1037,
`ModuleCompileOptions` ~line 1180) AND the inline `<svelte:options>` overrides (the AST `SvelteOptions`
interface ~line 1236, parsed by `read/options.js`). The `<svelte:options>` attribute set is narrower than
the full `CompileOptions` — `read/options.js` accepts exactly `runes`, `tag` (DEPRECATED → hard compile
error, NOT a fold), `customElement`, `namespace`, `css` (only `'injected'`), `immutable`,
`preserveWhitespace`, `accessors` — and these inline values OVERRIDE the compile options for the same
keys. (`tag` is NOT a real attribute key — its `read/options.js` `case 'tag'` arm only calls
`e.svelte_options_deprecated_tag(attribute)`, which `throw`s an `InternalCompileError` via the `never`-
returning `e()` helper, so `<svelte:options tag="…">` ERRORS at parse time; it does not fold into
`customElement`. Verified at `5.56.3`: `compile('<svelte:options tag="my-el"/><h1>hi</h1>')` throws
`svelte_options_deprecated_tag` — `"tag" option is deprecated — use "customElement" instead`. Block 5m's
`<svelte:options>` reader must reproduce this error path, NOT a fold.) This axis is its OWN feature family (Block 5m, §10), NOT a per-template scatter: a single options
resolver folds compile options ∪ `<svelte:options>` overrides into the runtime backends. Each
output-affecting option is enumerated below with its empirically-confirmed effect (verified against
`compile(src, { generate, ...option })` at `5.56.3`); the block's STEP-0 pins the exhaustive per-option
goldens (the §3.2 matrix-scope note: exhaustive goldens defer to block-start).

**Output-affecting options (each verified against `5.56.3`):**

| Option | Source(s) | Representative client effect | Server (if different) | Block |
| --- | --- | --- | --- | --- |
| `name?: string` | compile only | OVERRIDES the §1.2/Block 4 filename-derived component name — `2-analyze/index.js` resolves `module.scope.generate(options.name ?? component_name)`, so the resolved name is the exported component-function identifier. Verified: `compile('<h1>hi</h1>', {filename:'App.svelte', name:'CustomName'})` → `export default function CustomName(…)` for BOTH client AND server (vs filename-derived `App` with no `name`). | same name on the server backend's `.render`/export shape | 4 (component naming — §1.2) + 5m (the compile-options resolver feeds `name` to the Block 4 naming step) |
| `runes?: boolean` | compile + `<svelte:options runes>` | mode selection — `analysis.runes = runes_option ?? inferred`; suppresses/forces `import 'svelte/internal/flags/legacy'` (§3.2.1/H1). Verified: `runes={true}` → NO legacy flag with zero runes; `runes={false}` → legacy flag WITH a `$state`. | same mode gate | 4 (mode plumbing) / 5i (legacy lowering) |
| `namespace?: 'html'\|'svg'\|'mathml'` | compile + `<svelte:options namespace>` | template root helper — `$.from_html` (default) / `$.from_svg` / `$.from_mathml` (`transform-template/index.js`). Verified: `namespace:'svg'` → `$.from_svg`, `'mathml'` → `$.from_mathml`. | SSR template-string serialization (namespace-correct markup) | 5m |
| `fragments?: 'html'\|'tree'` | compile only | clone strategy — `$.from_html` (default) vs `$.from_tree` (CSP-safe one-element-at-a-time). Verified: `fragments:'tree'` → `$.from_tree`, no `$.from_html`. | n/a (no template hoist on SSR) | 5m |
| `preserveWhitespace?: boolean` | compile + `<svelte:options preserveWhitespace>` | serialized-template whitespace — default collapses (`<div>a    b</div>`), `true` keeps it raw (`<div>  a    b  </div>`). Verified. | same (server template string) | 5m |
| `preserveComments?: boolean` | compile only | comment retention — default strips HTML comments from the template, `true` keeps `<!-- … -->`. Verified. | same | 5m |
| `accessors?: boolean` | compile + `<svelte:options accessors>` | LEGACY only (deprecated/no-op in runes) — `true` wraps the body in `$.push`/`$.pop($$exports)` and emits `get x()/set x($$v)` prop accessors; `false` emits neither. Verified on a legacy `export let`: `$.pop($$exports)` + getters/setters present iff `accessors:true`. Always `true` under `customElement`. | n/a | 5m (gated on 5i legacy) |
| `immutable?: boolean` | compile + `<svelte:options immutable>` | LEGACY only (deprecated/no-op in runes) — flips the legacy prop flag: `$.prop($$props, 'x', 8, …)` (default) → `$.prop($$props, 'x', 9, …)` (immutable bit set). Verified. | (legacy SSR prop path) | 5m (gated on 5i legacy) |
| `customElement?: boolean \| {tag, shadow, props, extend}` | compile + `<svelte:options customElement>` | `customElements.define(name, $.create_custom_element(Cmp, props, [], [], { mode }))` + `$.push`/`$.pop($$exports)` accessors (`$host()` surfaces as `$$props.$$host`). Verified: define + `$.create_custom_element` emitted. Forces `css:'injected'`. | n/a | 5h (already owns this — confirmed stays) |
| `css?: 'injected'\|'external'\|fn` | compile + `<svelte:options css='injected'>` (inline only allows `'injected'`) | `'external'` (default) → no style helper, CSS on `compile().css`; `'injected'` → module `const $$css = { hash, code }` + body `$.append_styles($$anchor, $$css)`, `compile().css === null` (no separate artifact). Verified. | `'external'` → same separate `compile().css` artifact; `'injected'` → module `const $$css = { hash, code }` + body `$$renderer.global.css.add($$css)`, `compile().css === null` (no separate artifact). Verified. | 5l (already owns this — confirmed stays) |
| `cssHash?: ({hash, css, name, filename}) => string` | compile only | OUTPUT-AFFECTING — overrides the scoped-class name in BOTH the serialized HTML and the CSS artifact. Verified: default `svelte-n50uah` → custom `myhash-16e8uch`. Hash parity (default algorithm) is the 5l conformance bar; a custom `cssHash` threads through 5l. | same (same scoped class both backends) | 5l (CSS-scoping) |
| `discloseVersion?: boolean` | compile only | `import 'svelte/internal/disclose-version'` emission — default `true` emits it (client), `false` suppresses it. Verified. | absent on server regardless | 5m |
| `compatibility?: { componentApi?: 4 \| 5 }` | compile only | `componentApi: 4` → client wraps the default export so it instantiates as a Svelte-4 class (`createClassComponent` + `$.push`/`$.pop`); server emits an object with a `.render(...)` method (`transform-client.js` / `transform-server.js`). Verified: `createClassComponent` + `$.pop` (client) / `.render` (server). Default `5` emits neither. | `.render(...)` method shape | 5m |
| `hmr?: boolean` | compile only | HMR wrapper — `true` (with `dev:true`) emits `$.hmr(...)` + `import.meta.hot` accept plumbing; `false` emits neither. Verified. | n/a (client-only) | 5m |
| `dev?: boolean` | compile (`ModuleCompileOptions`) | the dev-mode codegen axis — validation wrappers, `$.add_locations`, dev `$.inspect`/`$.trace` (§3.2.3). | dev SSR module-shape (§3.2/Block 5k) | 5k (dev-mode axis) |
| `generate?: 'client'\|'server'\|false` | compile (`ModuleCompileOptions`) | selects the client vs server backend (the §3.5 split); `false` emits nothing (analysis/warnings only). | the SSR backend (§3.5, Block 8) | 4 (client) / 8 (server) |
| `experimental.async?: boolean` | compile (`ModuleCompileOptions`) | the experimental-async gate — `import 'svelte/internal/flags/async'` + async helpers when ON (§3.2.2). | server ALSO emits `flags/async` when ON | 5j (async) |

**Inert / non-output-affecting options (resolved but do NOT change the emitted JS/CSS topology — handled by the existing source-map / diagnostic plumbing, NOT a feature block):**

- `sourcemap?: object | string` — an initial (preprocessor) source map merged into the final output map; affects ONLY the map, not the code (rides §6 sourcemap hardening / the existing source-map policy).
- `outputFilename` / `cssOutputFilename` — source-map `file`/`sources` naming only.
- `filename` — debugging hints + the component-function export name derivation (§1.2) + the default `cssHash` input; threads through §4.1 `RuntimeCompileOptions.filename`, not a 5m surface.
- `rootDir` — filename sanitization for the source map; no code effect.
- `warningFilter` — diagnostics filtering only (no code effect; relates to §6 lint).
- `modernAst?: boolean` — affects ONLY the returned `.ast` shape, not the emitted JS/CSS. Verified: emitted JS is byte-identical with/without `modernAst`. Not a runtime-codegen concern.

**Exhaustive field accounting (axis closed at `5.56.3`).** Every field of the three authoritative interfaces in `svelte/types/index.d.ts` is classified above — none remain unaccounted:

- `CompileOptions` (the `extends ModuleCompileOptions` superset, ~line 1024) — 18 own fields, ALL classified: output-affecting → `name`, `customElement`, `accessors`, `namespace`, `immutable`, `css`, `cssHash`, `preserveComments`, `preserveWhitespace`, `fragments`, `runes`, `discloseVersion`, `compatibility.componentApi`, `hmr` (14); inert → `sourcemap`, `outputFilename`, `cssOutputFilename`, `modernAst` (4).
- `ModuleCompileOptions` (~line 1159) — 6 own fields, ALL classified: output-affecting → `dev` (5k), `generate` (4/8), `experimental.async` (5j) (3); inert → `filename`, `rootDir`, `warningFilter` (3). `experimental` has exactly one member at `5.56.3` (`async`); no other experimental sub-field exists.
- AST `SvelteOptions` (the inline `<svelte:options>` shape, ~line 1236) — `start`/`end`/`attributes` are AST plumbing (positions + raw attribute list, no code effect); the real option keys `runes`, `immutable`, `accessors`, `preserveWhitespace`, `namespace`, `css` (only `'injected'`), `customElement` are each a subset of the `CompileOptions` keys above and OVERRIDE them. The `read/options.js` parser ALSO recognizes the `tag` attribute key, but ONLY to ERROR (`svelte_options_deprecated_tag`) — it never reaches `SvelteOptions` as a field (correctly absent from the interface). No `<svelte:options>` key is unaccounted.

The only output-affecting field previously missing from this table was `name`; it is now listed. No other unlisted field changes the emitted JS/CSS topology at `5.56.3`.

**Block placement.** The output-affecting template/mode/module-shape options that lack a dedicated home —
`namespace`, `fragments`, `preserveWhitespace`, `preserveComments`, `discloseVersion`,
`compatibility.componentApi`, `hmr`, plus the legacy-gated `accessors`/`immutable` — land in a dedicated
**compile-options block (Block 5m, §10)** with a single options resolver folding compile options ∪
`<svelte:options>` overrides. The options already owned by their feature block stay there: `runes` (mode
plumbing in Block 4, legacy lowering in 5i), `css`/`cssHash` (CSS-scoping, 5l), `customElement` (5h),
`dev` (dev-mode, 5k), `experimental.async` (5j), `generate` (client backend Block 4 / server backend
Block 8). The relevant carrier-threaded options surface on §4.1 `RuntimeCompileOptions`.

---

## 4. The `compile_entry` / CarrierCompiler Foundation (Block 1)

The runtime path needs the carrier seam to own runtime codegen — the framing-independent foundation.
Today `compile_entry()` calls `compile_sfc` / `compile_from_parsed` directly (a hardcoded Vue path)
and `assemble_main_module` assumes the Vue `_sfc_main` shape.

### 4.1 Extend the CarrierCompiler trait

In `crates/verter_compiler/src/framework_common/carrier_compiler.rs`:

- Add `RuntimeCompileOptions` (the neutral subset: filename, production/`dev` flag, source-map flag,
  SSR/`generate` flag, runtime-module override, component id — the runtime analogue of
  `IdeCompileOptions`). It ALSO carries the output-affecting compile options that thread through to the
  Svelte backends (§3.8): `name` (the explicit component-name override — `options.name ?? component_name`,
  §3.8/Block 4), `runes`, `namespace`, `fragments`, `preserveWhitespace`, `preserveComments`,
  `css` mode + `cssHash`, `customElement`, `discloseVersion`, `compatibility.componentApi`, `hmr`,
  `accessors`, `immutable`, and the experimental-async flag — each carrier reads only the options it
  supports (Vue ignores the Svelte-specific ones; a non-output-affecting option like `modernAst` is not
  carried). The carrier resolves compile options ∪ `<svelte:options>` inline overrides (Block 5m).
- Add `RuntimeCompileOutput` — **neutral, NOT `VerterCompileResult`** (which is Vue-shaped). It
  carries:
  - `main` — the framework-owned ESM body code + source map + language.
  - Optional script / template side virtual files.
  - styles / custom blocks.
  - `tsx` (when requested in the same pass).
  - `template_data`.
  - diagnostics / timings / cache tags.
- Add `fn compile_runtime(&self, source, artifact, opts, alloc) -> Result<RuntimeCompileOutput,
  CompileUnsupported>` with a typed unsupported result for runtime targets a carrier cannot yet
  produce.

`RuntimeCompileOutput.main.body_code` is the framework-OWNED ESM body. Vue's carrier returns a
Vue-shaped body (assembled by the Vue bridge); Svelte's carrier returns official-shaped Svelte
output. The session owns only host virtual-file concerns (style virtual imports, custom-block
imports, cache metadata, virtual file IDs).

### 4.2 Module assembly ownership

Do NOT generalize Vue's `_sfc_main` shape across frameworks. The current host
`assemble_main_module` becomes Vue-only (conceptually `assemble_vue_main_module`, owned by the Vue
bridge). Each carrier owns its complete module emission. This keeps `compile_entry()` a ROUTER, not a
framework assembler.

### 4.3 Route `compile_entry` through the registry

`crates/verter_session/src/host_resolve/virtual_file_pipeline.rs::compile_entry()` routes through
`CarrierCompilerRegistry` (by resolved carrier language) instead of the direct `compile_sfc` /
`compile_from_parsed` call. Vue becomes a REGISTERED runtime carrier — not a privileged path. Vue
parse/codegen BEHAVIOR is untouched; only dispatch and assembly OWNERSHIP move.

### 4.4 Vue byte-identity characterization (Block 1 STEP-0)

Before the cutover, capture characterization tests pinning Vue output byte-identical before/after:

- Main module output byte-identical (VDOM, Vapor, SSR).
- Style / custom-block import ordering byte-identical.
- HMR byte-identical.
- Script / template virtual-file outputs byte-identical.
- Template analysis output unchanged.

### 4.5 No dual-path shim survives the Block 1 cutover (D5 guard)

The Block 1 cutover is a CLEAN replacement, not a dual path (CLAUDE.md legacy-deletion rule). After
Block 1, the direct `compile_sfc` / `compile_from_parsed` call in `compile_entry()` MUST be GONE —
replaced wholesale by `CarrierCompilerRegistry` dispatch, with Vue a registered runtime carrier. No
`if framework == Vue { old_path } else { registry }` branch, no feature flag preserving the old
routing, no `assemble_main_module` call left reachable outside the Vue bridge. A named static guard,
**`compile_entry_has_no_direct_compile_sfc_call`**, asserts `virtual_file_pipeline.rs::compile_entry`
contains no direct `compile_sfc`/`compile_from_parsed`/`assemble_main_module` call (routing is
registry-only), and **`no_framework_branch_in_compile_entry`** asserts there is no per-framework
conditional in the runtime router. This pairs with the §4.4 byte-identity suite: Vue output is
identical AND the old path is physically deleted.

---

## 5. Conformance Strategy

Two layers:

### 5.1 Helper-topology golden oracle

Default tests use vendored fixtures + committed goldens. A FEATURE-GATED oracle invokes the pinned
official `svelte` compiler (from the `pnpm` lockfile-pinned version) and diffs the NORMALIZED STRUCTURE
+ helper-call SEQUENCE against Verter's output — helper families used, import set,
reactivity/hydration/CSR-vs-SSR decisions, call topology. The goldens pin STRUCTURE + HELPER TOPOLOGY,
NOT bytes. Byte identity is not the bar anywhere (including the smoke corpus).

### 5.2 Behavioral jsdom

Compile `.svelte` with Verter, EXECUTE the emitted module against the REAL `svelte/internal/client`
runtime, and assert DOM behavior: node creation, text updates, input binding, events, effects,
lifecycle, blocks (`{#if}`/`{#each}`/`{#key}`/`{#await}`), components, snippets.

**Output-parity bar (D-3):** FUNCTIONAL / behavioral equivalence on the pinned runtime + structural /
helper-family / call-topology parity is the bar. Byte identity is NOT required anywhere. Verter is not
a formatting clone of the official compiler; it produces functionally-equivalent modules that import
the same helper families and make the same decisions, verified behaviorally (§7 jsdom + §8 SSR) and
structurally (helper-topology goldens).

SSR gets parallel golden + string-render behavioral harnesses against `svelte/internal/server`.

### 5.3 Version pin, drift guard, and golden regeneration (D3)

The oracle and the runtime harness are anchored to ONE pinned version, **`svelte@5.56.3`**:

- **Pin constant.** A single source-of-truth constant `SVELTE_ORACLE_VERSION = "5.56.3"` (the oracle
  gate's pinned version) is the authority. §1.2/§2/§9 all name this exact version (no "open" decision
  remains — see §9).
- **Lockfile-drift guard (CI requirement).** A guard asserts the resolved `svelte` version in
  `pnpm-lock.yaml` EQUALS `SVELTE_ORACLE_VERSION`. A `svelte` bump that does not also re-pin the
  oracle constant (and regenerate goldens) FAILS the guard — silent drift is impossible. Named guard:
  `svelte_lockfile_matches_oracle_pin`.
- **jsdom harness uses the SAME pin.** The behavioral harness imports `svelte/internal/client` from
  the identical `svelte@5.56.3` resolved in the lockfile (no second floating dependency) so emitted
  modules execute against exactly the oracle version's runtime.
- **Golden REGENERATION procedure (committed script).** `scripts/gen-svelte-goldens.mjs` (mirroring
  the existing `scripts/gen-corpus-audit-tests.mjs` pattern) re-runs the pinned official compiler
  (`generate:'client'` AND `generate:'server'`) over the entire vendored corpus and rewrites ALL
  committed goldens (the NORMALIZED structure + helper-call-sequence form, not raw bytes) + the §3.2
  helper-matrix fixtures in one idempotent command: `node scripts/gen-svelte-goldens.mjs`. A `--check`
  mode (`gen-svelte-goldens.mjs --check`) re-runs the compiler, re-normalizes, and asserts the
  committed goldens equal the fresh normalized output (this guards that the OFFICIAL fixtures track the
  pinned compiler — it is not a Verter-output byte bar), gated as
  `svelte_goldens_in_sync_with_pinned_compiler`. **A version bump is therefore a first-class, reviewed
  delta:** (1) re-pin `SVELTE_ORACLE_VERSION` + bump the `svelte` lockfile entry; (2) run
  `node scripts/gen-svelte-goldens.mjs`; (3) review the golden diff as the oracle delta in the PR.
  The script is the ONLY way goldens change — goldens are never hand-edited.

---

## 6. Analysis + Lint First-Class Plan (Vue parity)

### 6.1 Lint framework abstraction

`verter_diagnostics` needs a framework abstraction, not Vue-shaped adapters baked into every rule.
Add a framework-AWARE lint input:

- Vue path keeps `TemplateAnalysisSnapshot` + `ScriptAnalysisSnapshot`.
- Svelte path consumes `ParsedSvelte`, Svelte template facts, `SvelteScriptFacts`, styles, source.
- Reusable a11y / HTML-conformance / security rules operate on a NEUTRAL HTML snapshot (shared
  across frameworks).
- `LintRule` gains a `target_framework()` gate (default: all frameworks); the `RuleRegistry`
  activates the Vue / Svelte / neutral rule sets per the file's framework.

### 6.2 Svelte-native rules (Vue-parity set)

- Keyed `{#each}` (the Svelte analogue of `require-v-for-key`).
- Invalid `bind:` targets (against the existing bind-contract data).
- `$props` / `$bindable` correctness.
- Readonly `$derived` mutation.
- Mutating props.
- Invalid `{@render}` / snippet usage.
- Unsafe `{@html}`.
- Event-handler validity.
- Svelte a11y parity (reusing the neutral a11y rules over the neutral HTML snapshot).

### 6.3 Component-meta

Do NOT clone the Vue macro pipeline. Generalize `ComponentMetaInput` so Vue supplies macro surfaces
and Svelte supplies `MacroSurfaceDtos` from `resolve_svelte_surface()` plus `ParsedSvelte` template
facts. The output stays `ComponentMetaAnalysis` (already framework-agnostic).

### 6.4 LSP wiring

Wire lint diagnostics into the LSP after parse/type diagnostics, with config-level
`target_framework` gating. (Lint is not surfaced to the LSP today for any framework; this lands the
wiring for both Vue and Svelte.)

---

## 7. Performance Contract (vs Vue)

Same-or-better than the Vue runtime compile:

- **No VDOM render IR** — Svelte emits direct DOM code like Vapor, with LESS Vue-specific binding
  work.
- **One `ParsedSvelte` walk** producing HTML serialization, DOM-path plan, effects, binds, and events.
- **OXC parses only** the script + expression spans that need rewriting (the template skeleton is
  serialized from the typed AST without re-parsing).
- **Helper / import dedup via bitflags** (`helpers.rs`, mirroring `shared/helpers.rs`).
- **Delegated events via a compact ordered set.**
- **Arena-owned strings + small reusable state stacks** (the oxc `Allocator` arena discipline).
- **One batched `CodeTransform` application** (deferred-op batching).

**Numeric perf-gate threshold (D5, replaces the vague "same-or-better").** The gate is a regression
fence, not an absolute number: for each named baseline fixture, Svelte runtime compile mean wall-time
(warm, single-file, over N≥50 runs) and the heap-delta (`heapUsedMB`) must each be **≤ 1.10×** the
behavior-equivalent Vue Vapor compile of the same logical component (i.e. no worse than 10% slower and
no more than 10% larger heap delta than Vapor). Target is `≤ 1.00×` (same-or-better); `> 1.10×` is a
gate FAILURE that blocks the block. The metrics are EXACTLY the ones the existing `@verter/benchmark`
harness already reports (it has no allocation-counter): mean ms / ops-per-sec wall-time and
`heapUsedMB` (a `process.memoryUsage().heapUsed` before/after delta, in MB — `packages/benchmark/src/index.ts`
+ `utils/stats.ts`).

- **Named baseline fixtures** (one Vapor equivalent each, vendored): `bench_hello` (the §1.2 example),
  `bench_list` (a 100-row keyed `{#each}` over `$state` items), `bench_form` (5 `bind:` inputs +
  events), `bench_conditional` (nested `{#if}`/`{#each}`), `bench_component` (parent rendering 3
  child components with props + binds).
- **Metric (the existing `@verter/benchmark` metrics — no new harness):** primary = mean compile
  wall-time (ms) per fixture (and the derived ops-per-sec), secondary = `heapUsedMB` heap-delta in MB
  (the `process.memoryUsage().heapUsed` before/after delta the bench already records). Both reported
  per the CLAUDE.md bench rule (cache mode, batch shape, thread count, hit/fallback counts). The bench
  package has NO allocation-counter; if per-allocation instrumentation is later wanted it is an
  explicit named deliverable of the perf-CI block (Block 12), not assumed to exist.
- The gate compares Verter-Svelte vs Verter-Vapor of the SAME logical component — never Verter-Svelte
  vs the official Svelte compiler (different toolchains).
- **The gate runs INCREMENTALLY, not only once at Block 12.** Block 12 stands up the job + the baseline
  fixtures, but the ≤1.10× regression fence is evaluated at EACH feature-family sub-block landing that
  touches the compile hot path (5a-5k), not deferred to a single end-of-program run. A regression
  introduced in (e.g.) 5e is therefore caught when 5e lands, not discovered much later at Block 12.
  Each such sub-block's deliverables include re-running the perf gate over the baseline set plus any
  fixture the sub-block adds.

---

## 8. Decisions Log

| ID  | Decision                                                                                                 |
| --- | -------------------------------------------------------------------------------------------------------- |
| D-1 | **Corrected framing accepted.** Source `.svelte` → JS module importing `svelte/internal/client` / `svelte/internal/server`. NO Svelte runtime, NO `@verter/svelte-runtime` facade is built. Supersedes any prior facade design. (Confirmed: no facade scaffolding exists in the tree today.) |
| D-2 | **Official Svelte 5 compiler output is the conformance target, pinned to `svelte@5.56.3`.** `svelte/internal/*` is intentionally internal; the compiler is oracle-tested against this exact lockfile version, never floating "latest". A bump is re-pin → regenerate goldens (`scripts/gen-svelte-goldens.mjs`) → review the golden diff (§5.3). Lockfile-drift guard `svelte_lockfile_matches_oracle_pin`. |
| D-3 | **Output-parity bar = FUNCTIONAL / behavioral equivalence + structural / helper-family / call-topology parity, verified on the pinned runtime.** Byte identity is NOT the bar anywhere (smoke corpus included). Verter emits functionally-equivalent modules importing the same `svelte/internal/*` helper families with the same reactivity/hydration/CSR-vs-SSR decisions; not a formatting clone of the official compiler. |
| D-4 | **Svelte gets its own runtime IR** (`svelte/runtime/ir.rs`), NOT a Vapor IR reuse. Vapor PATTERNS (counters, bitflags, single-pass walk, template-string synthesis, arenas, deferred CodeTransform) are reused; Vapor SEMANTICS are not. |
| D-5 | **`RuntimeCompileOutput` is neutral, not `VerterCompileResult`** (which is Vue-shaped). Each carrier owns its complete framework-shaped ESM body; the session owns only host virtual-file concerns. |
| D-6 | **`assemble_main_module` becomes Vue-only** (owned by the Vue bridge); `compile_entry()` is a router. No generalized `_sfc_main` shape across frameworks. |
| D-7 | **`docs/arch/multi-framework-adapters-plan.md` Invariant 4** ("runtime codegen for non-Vue frameworks is out of scope") is SUPERSEDED by this program for Svelte and must be updated to reference this plan. |
| D-8 | **Lint gains a framework abstraction** (`target_framework()` gate + neutral/Vue/Svelte rule sets) rather than Vue-shaped adapters per rule. |
| D-9 | **Static optimization is conformance-first and pre-lowering only** (§3.6). The baseline compiler never changes runtime call topology; a narrow separately-gated Optimization block (after conformance) applies only safe, fail-closed, pre-lowering-IR optimizations shared by client AND SSR. Verter may optimize the program before lowering; it must NOT invent a competing `svelte/internal/*` protocol after lowering. |
| D-10 | **SSR is first-class and WORKING** — a dedicated block (server backend + server goldens from real `generate:'server'` output + string-render behavioral harness) plus a CSR/SSR hydration round-trip gate (SSR render → client hydrate → assert no mismatch + interactivity). SSR shares the pre-lowering IR with client so optimizations apply identically. |
| D-11 | **Near drop-in feature parity with the official Svelte 5 compiler** — every real official-compiler feature is in-scope with a block (§9.1 in-scope matrix). The ONLY justified exclusions are deeper-than-official optimizations (D-9) and surfaces official `5.56.3` does not emit; experimental async stays gated, mirroring official's own gate (§3.2.2). No silent feature deferrals. |
| D-12 | **Concrete `@verter/unplugin` public API** (§10.1): named exports `VerterVue` / `VerterSvelte` / `Verter({ lang })` + subpaths `@verter/unplugin/vue` and `@verter/unplugin/sveltejs`, all thin wrappers over ONE shared transform core dispatched by `lang`/detection. Additive — the existing default entry is preserved (`VerterVue` re-exposes today's default behavior). |
| D-13 | **First-class perf-comparison CI** (§7, §10.2): a real CI job modeled on the existing Vue `@verter/benchmark` (`packages/benchmark/`) + `.github/workflows/benchmark.yml` — a Svelte compile bench (Verter-Svelte vs the official Svelte compiler, and vs Verter-Vapor for the §7 numeric gate) with named baseline fixtures, the §7 metric + ≤1.10× regression gate, and a `/benchmark`-style workflow job. Not prose — a wired CI job (Block 12). |

---

## 9. Open / Owner-Existential Decisions

All round-2 owner directives are resolved in the plan body (parity bar = functional equivalence, D-3;
SSR first-class + working, D-10/§8-block; near drop-in feature parity, D-11/§9.1; optimization
conformance-first, D-9/§3.6; concrete unplugin API, D-12/§10.1). No owner-existential decision blocks
execution. The remaining items below are sequencing/operational, not scope-existential:

- **Optimization block sequencing** (§3.6 / §10): the narrow Optimization block lands only after the
  conformance + hydration gates are green; its go-ahead is a normal block gate, not an owner-blocking
  decision.
- **Dev-mode and legacy block ordering** (§3.2.1 / §3.2.3 / §10): legacy (5i), async (5j), and
  dev-mode (5k) are in-scope blocks sequenced after the production runes client/SSR baseline; their
  relative ordering is an implementation-sequencing call.

### 9.1 In-scope feature matrix vs the official compiler (near drop-in parity)

The bar is near drop-in parity with the official Svelte 5 compiler (D-11). EVERY real official-compiler
feature is IN-SCOPE and assigned to a block. A surface that is not yet implemented returns
`CompileUnsupported` with a precise diagnostic until its block lands (then the diagnostic is deleted,
§11) — it is never silently mis-compiled and never a permanent gap. There are NO silent deferrals.

**Complete in-scope matrix (official feature → block):**

| Official feature | Official lowering (per §3.2 probe) | In-scope block |
| --- | --- | --- |
| Runes core (`$state`/`$state.raw`/`$state.snapshot`) | `$.state` / `$.proxy` / `$.state($.proxy(…))` / plain `let` / `$.snapshot` (full classification in §3.2) | 4, 5g |
| `$state.eager` (async-gated) | `$.eager(fn)` (mirrors official's experimental-async gate) | 5j |
| `$derived` / `$derived.by` | `$.derived` | 4 |
| `$effect` / `$effect.pre` / `$effect.root` | `$.user_effect`/`$.user_pre_effect`/`$.effect_root` | 4, 5g |
| `$effect.tracking` | `$.effect_tracking()` (client) / `false` (SSR) | 5g |
| `$effect.pending` (async-gated) | `$.eager($.pending)` (mirrors official's experimental-async gate) | 5j |
| `$props()` destructure / rest / `$props.id()` | `$.prop`/`$.rest_props`/`$.props_id()` (SSR `$.props_id($$renderer)`) | 4, 5g |
| `$bindable` | `$.prop(…, flags\|bindable, …)` | 5g |
| `$inspect` / `$inspect().with` / `$inspect.trace` (production) | production no-op (matches official) | 5g |
| `$inspect` / `$inspect().with` / `$inspect.trace` / validation (dev mode) | `$.inspect`/`$.trace`/`$.add_locations`/validation wrappers | 5k (dev-mode axis, §3.2.3) |
| `$host()` + `<svelte:options customElement>` | `$$props.$$host` + `customElements.define(…, $.create_custom_element(…))` + `$.push`/`$.pop($$exports)` | 5h |
| Static template + DOM walk | `$.from_html`/`$.first_child`/`$.child`/`$.sibling`/`$.reset`/`$.append` | 4 |
| Reactive text | `$.template_effect` + `$.set_text` | 4 |
| Dynamic attrs / boolean DOM props | `$.set_attribute` / direct DOM prop | 5a |
| `class:`/`class={}`, `style:`/`style={}` | `$.set_class` / `$.set_style` | 5a |
| Spreads `{...x}`, `{@html}` | `$.attribute_effect`/`$.rest_props`, `$.html` | 5b |
| Full `bind:*` family | `$.bind_*` (§3.2 bindings table) | 4 (`bind:value`/`bind:this`), 5c (breadth) |
| Delegated + non-delegated events, legacy modifiers | `$.delegated`/`$.delegate`, `$.event`, `$.preventDefault`/… | 4 (delegated), 5d (breadth) |
| Blocks `{#if}`/`{#each}`/`{#await}`/`{#key}` | `$.if`/`$.each`/`$.await`/`$.key` (each/await bindings are `$.get` signals, §3.3) | 5e |
| `{@const}` / `{@debug}` | `{@const}` → `$.derived(() => …)` (runes mode) / `$.derived_safe_equal` (legacy, §3.2.1); `{@debug}` → `$.template_effect(() => { console.log({…$.snapshot}); debugger; })` | 5e |
| `{const …}` / `{let …}` declaration tags (5.56, `DeclarationTag` — distinct from `{@const}`) | a plain inert block-local `const`/`let` declaration (NO `$.derived` memo); declarators may carry runes / be async | 5e |
| Components (static + `{@render}` snippets), `<svelte:component>`, `<svelte:element>`, `{@attach}` | direct `Child($$anchor, {…})` / `$.component` / `$.snippet` / `$.element` / `$.attach` | 5f |
| Special elements `<svelte:head>`/`<svelte:document>`/`<svelte:boundary>`/`<svelte:self>`/`<svelte:fragment>`/`<svelte:window>`/`<svelte:body>` | `$.head` / `$.event(…, $.document, …)` / `$.boundary` / recursive self-call / `$$slots` entry / `$.event`+`$.bind_window_size` (per the §3.2 special-elements rows) | 5f |
| Transitions / actions / animations | `$.transition` / `$.action` / `$.animation` | 5f |
| SSR (`generate:'server'`) | `$$renderer.push`/`$.escape`/`$.attr`/`$.attr_class`/`$.clsx`/`$.attr_style`/`$.ensure_array_like` + comment markers | 8 |
| Legacy non-runes (`export let`, `$:`, `<slot>`, `createEventDispatcher`) | `$.prop`/`$.legacy_pre_effect`/`$.slot`/`$.init` (§3.2.1); SSR adds `$.fallback` (default-prop fallback) + `$.bind_props` (write-back) | 5i |
| Store auto-subscriptions (`$store`) | client `$.store_get`/`$.store_set`/`$.update_store` + `$.setup_stores`/`$$cleanup`; SSR `$.store_get`/`$.store_set`/`$.update_store` + `$.unsubscribe_stores` (component-fn-scoped — §3.2 store rows) | 5i |
| `<style>` compilation + CSS scoping (+ `css` mode / `cssHash`) | `svelte-<hash>` scoped class in template HTML + separate `compile().css` artifact (`css: 'external'`, both backends) / `const $$css` + body helper — `$.append_styles($$anchor, $$css)` client, `$$renderer.global.css.add($$css)` server — with `compile().css === null` (`css: 'injected'`, both backends); `cssHash` overrides the scoped-class name — §3.7/§3.8 | 5l |
| `<svelte:options>` + compile-option axis | `name` (component-function name override — `options.name ?? component_name`, both backends) / `runes` (mode/legacy flag) / `namespace` (`$.from_html`/`$.from_svg`/`$.from_mathml`) / `fragments` (`$.from_html` vs `$.from_tree`) / `preserveWhitespace` / `preserveComments` / `accessors` (`$.push`/`$.pop($$exports)` + getters-setters, legacy) / `immutable` (prop flag, legacy) / `discloseVersion` (disclose-version import) / `compatibility.componentApi` (`createClassComponent` client / `.render` server) / `hmr` (`$.hmr` + `import.meta.hot`); the inline `<svelte:options tag>` key is a DEPRECATED hard error (`svelte_options_deprecated_tag`), reproduced as an error, NOT folded — §3.8 | 5m (`name`→4 naming, `runes`→4/5i, `css`/`cssHash`→5l, `customElement`→5h, `dev`→5k, `generate`→4/8, async→5j) |
| Experimental async (flag ON) | `import 'svelte/internal/flags/async'` + async block helpers | 5j (mirrors official's own gate) |

**Justified exclusions (the only things NOT made an in-scope feature block):**

- **Deeper-than-official static optimizations** — a separately-gated Optimization block (§3.6 / D-9),
  not a parity feature; it is conformance-first and never changes runtime topology.
- **Experimental async with the flag OFF** — the official compiler itself emits nothing here, so
  matching it means emitting nothing (5j supplies the flag-ON path that mirrors official).
- **Any helper the official `5.56.3` compiler does not emit** — by definition not a parity surface;
  if a future pinned version emits it, the oracle re-pin (§5.3) surfaces it as a golden delta.

---

## 10. Block Decomposition

Numbered blocks; STEP-0 = a discovery/spike step that must complete before the block's
implementation. Block 1 is the framing-independent foundation. Every block lands with concrete
deliverables + verification; a feature block deletes its surface's `CompileUnsupported` diagnostic
when it lands (§11).

**STEP-0 finalizes that block's exhaustive depth (see the §3.2 matrix-scope note).** The §3.2 matrix is
a REPRESENTATIVE, oracle-regenerated artifact (Block 2, §5.3) — it is not a hand-maintained exhaustive
enumeration. Each block's STEP-0 regenerates and PINS THAT block's exhaustive helper set, server/SSR
forms, AST-context sensitivity, and dev-mode goldens against the pinned compiler. The known deeper cases
that land at the owning block's STEP-0 are named in §3.2: async-rune context-sensitivity → 5j;
special-element SSR / dynamic-title variants → 5f / 8; dev-mode SSR module-shape → 5k.

Block 5 is SPLIT by runtime feature family (D4) — the §3.2 matrix families are the split axis. Each
sub-block (5a-5m) lands with its OWN vendored goldens, jsdom behavioral cases, and `CompileUnsupported`
deletion. The runes-completion (5g), custom-element (5h), legacy + store-subscription (5i), async (5j),
dev-mode (5k), CSS-scoping (5l), and the `<svelte:options>`/compile-option axis (5m) sub-blocks close
the §9.1 feature matrix (E6 near drop-in parity).

**Grow-relationship between the feature sub-blocks and the cross-cutting blocks (Block 7 / 8 / 12).**
Blocks 7 (jsdom), 8 (SSR), and 12 (perf CI) are STANDING blocks, not one-shot. Their dependency column
lists their INITIAL landing dependency (`4, 5a-5f` — the breadth needed to stand the harness up), but
each later feature sub-block (5g-5m) ADDS its own behavioral/SSR/perf cases AT ITS OWN landing: 5g-5m
do NOT re-enter Blocks 7/8/12 as a separate pass — instead, each sub-block's deliverables INCLUDE the
matching jsdom case (Block 7 harness), the SSR golden + string-render case (Block 8 harness), and any
new perf fixture (Block 12 set) for the surfaces it introduces, landed alongside the sub-block. The
harnesses themselves (the runners, the fixtures dir, the CI job) are stood up at 7/8/12; the per-feature
cases grow into them as 5g-5m land. This makes the perf gate INCREMENTAL (see §7 / below).

The Integration block (I, D2) makes the emitted JS reachable through the bundler / playground. SSR
(Block 8) + the CSR/SSR hydration round-trip gate (Block 9) are first-class (D-10). The Optimization
block (Block 14) is the narrow, conformance-gated optimizer (D-9, §3.6).

| Block | Scope | Depends on | STEP-0 |
| ----- | ----- | ---------- | ------ |
| **1** | **Carrier runtime cutover + module seam.** Add `compile_runtime` / `RuntimeCompileOptions` / `RuntimeCompileOutput` to `CarrierCompiler`; make `assemble_main_module` Vue-only (Vue-bridge-owned); route `compile_entry()` through `CarrierCompilerRegistry`. Vue becomes a registered runtime carrier. NO dual path survives (§4.5 guards). | — | **Yes** — Vue byte-identity characterization suite (§4.4) pinned BEFORE the cutover. |
| **2** | **Svelte oracle harness.** Vendored golden corpus + feature-gated official-compiler oracle (normalized structure + helper-call-topology diff, NOT bytes). Includes `scripts/gen-svelte-goldens.mjs` + the drift/sync guards (§5.3). | 1 | **Yes** — capture the pinned official-compiler corpus (client). |
| **3** | **Svelte runtime IR spike.** Design `svelte/runtime/ir.rs` + the DOM-path plan + the helper/delegated-event model, anchored to the §3.2 matrix. The IR is the SHARED pre-lowering surface feeding client AND server (§3.6 optimization envelope). | 2 | **Yes** — confirm the §3.2 classification against the pinned output. |
| **4** | **Client MVP.** Scripts + `$state`/`$.get`/`$.set`/`$.update` + the §3.2 `$state`/`$.proxy` classification + interpolation + `$.template_effect` + `bind:value` + `bind:this` + delegated `onclick` — emits the §1.2 official example. Includes the component export-name derivation (filename stem → JS-identifier-sanitized name; `_unknown_` when no filename — §1.2), with the explicit `name` compile option overriding it (`options.name ?? component_name`, both backends — §3.8; the resolved `name` arrives from the 5m compile-options resolver). PRIMARY risk: scope/shadowing (§3.3). | 3 | — (covered by Block 3 STEP-0). |
| **5a** | **Attributes + class/style.** Dynamic attrs (`$.set_attribute` / boolean DOM props), `class:`/`class={}` (`$.set_class`), `style:`/`style={}` (`$.set_style`). | 4 | — |
| **5b** | **Spreads + `{@html}`.** `$.attribute_effect`, `$.rest_props` + `rest_excludes`, `$.html`. | 4 | — |
| **5c** | **Bindings breadth.** The full `$.bind_*` family beyond `bind:value`/`bind:this` (checked, group, select, media, dimensions, contenteditable, property, window-size, component binds). | 4 | — |
| **5d** | **Events breadth.** Non-delegated `$.event`, window/body events, legacy modifier wrappers (`$.preventDefault` etc.). | 4 | — |
| **5e** | **Control-flow blocks + `{@const}` + `{const …}`/`{let …}` declaration tags.** `$.if`, `$.each` (keyed/unkeyed/else, `$.index`), `$.await`, `$.key`, `{@const}` (runes mode → `$.derived(() => …)`; legacy non-runes mode → `$.derived_safe_equal(() => …)` — §3.2.1/§3.2/§9.1), and the plain-binding `DeclarationTag` lowering for `{const …}`/`{let …}` (distinct from `{@const}` — no `$.derived` memo; declarators may carry runes / be async — §3.2). | 4 | — |
| **5f** | **Components, snippets, special elements, transitions/actions/animations.** Direct component calls + `$.component`, snippet defs / `$.snippet` / direct render, `{@attach}` (`$.attach`), `$.element`, the full special-element set — `<svelte:head>` (`$.head`), `<svelte:document>` (`$.event(…, $.document, …)`), `<svelte:boundary>` (`$.boundary`), `<svelte:self>` (recursive self-call), `<svelte:fragment>` (`$$slots` entry), `<svelte:window>`/`<svelte:body>` — and `$.transition` / `$.action` / `$.animation` (per the §3.2 special-elements rows). | 4, 5c, 5e | — |
| **5g** | **Runes completion (production).** `$state.raw`/`$state.snapshot` (`$.snapshot`), `$effect.pre`/`$effect.root` (`$.user_pre_effect`/`$.effect_root`), `$effect.tracking` (`$.effect_tracking()` client / `false` SSR), `$props()` rest (`$.rest_props` + `rest_excludes`) / `$props.id()` (`$.props_id()` client / `$.props_id($$renderer)` SSR), `$bindable` (`$.prop` bindable flag), `$inspect` / `$inspect().with` / `$inspect.trace` production no-op. (`$derived.by` lands with `$derived` in Block 4 — §3.2/§9.1; `$state.eager` / `$effect.pending` are async-gated → 5j; the dev-mode `$.inspect`/`$.trace` forms → 5k.) Closes the production runes rows of §9.1. | 4 | — |
| **5h** | **Custom elements / `$host()`.** `<svelte:options customElement>` + `$host()` → `$$props.$$host` + module `customElements.define(name, $.create_custom_element(Cmp, props, [], [], { mode }))` + `$.push`/`$.pop($$exports)` getter/setter accessors. | 4, 5f | — |
| **5i** | **Legacy non-runes mode + store auto-subscriptions.** `export let` (`$.prop` accessor), reactive `$:` (`$.mutable_source`/`$.legacy_pre_effect`/`$.legacy_pre_effect_reset`/`$.deep_read_state`), `<slot>` (`$.slot`), `createEventDispatcher` (`$.init`) — §3.2.1; emits `import 'svelte/internal/flags/legacy'`. ALSO: store auto-subscriptions `$store` (the legacy-store reactivity contract; works in BOTH runes and legacy components) — client `$.store_get`/`$.store_set`/`$.update_store` + top-of-body `const [$$stores, $$cleanup] = $.setup_stores()` and trailing `$$cleanup()`; server `var $$store_subs;` + `$.store_get($$store_subs ??= {}, …)` and trailing `if ($$store_subs) $.unsubscribe_stores($$store_subs)` (component-fn-scoped, NOT module — §3.2 store-subscription rows). | 4, 5e, 5f | **Yes** — capture the official legacy-mode + store corpus. |
| **5j** | **Experimental async (flag ON).** Mirrors official's experimental gate: emit `import 'svelte/internal/flags/async'` + async `$derived`/`{#await}` helpers ONLY when Svelte's experimental flag is on (§3.2.2). Includes the async-gated runes `$state.eager` (`$.eager(fn)`) and `$effect.pending` (`$.eager($.pending)`). | 4, 5e | **Yes** — capture the flag-ON official corpus. |
| **5k** | **Dev-mode codegen (`dev: true`).** Mirrors official dev output: validation wrappers, `$.add_locations`, dev-mode `$.inspect` / `$inspect().with` / `$inspect.trace` (`$.trace` + `flags/tracing`) (§3.2.3). Production output (5g) is the baseline; this is the dev axis. | 4, 5g | **Yes** — capture the official `dev: true` corpus. |
| **5l** | **`<style>` compilation + CSS scoping (§3.7).** CSS-hash-algorithm parity with `svelte@5.56.3`; `svelte-<hash>` scoped-class injection into the serialized template HTML (client `$.from_html` AND server `$$renderer.push`, same hash); the `css` mode toggle (symmetric across backends, source-gated by `analysis.css.ast && !analysis.inject_styles`) — `external` (default) returns the CSS as a SEPARATE artifact on `RuntimeCompileOutput.styles` with no inline helper (both backends), `injected` emits a module-scope `const $$css = { hash, code }` + body helper (`$.append_styles($$anchor, $$css)` on client, `$$renderer.global.css.add($$css)` on server) and a `null` `compile().css` (NO separate artifact, both backends); the external CSS artifact reaches the bundler/unplugin as a style virtual file mirroring Vue's `query.type === "style"` flow (§10.1, Block I). | 4 | **Yes** — capture the official CSS corpus (`scripts/gen-svelte-goldens.mjs` CSS pass): scoped HTML + `css.code` + hash + the `external`-vs-`injected` JS shapes for BOTH client (`$.append_styles`) and server (`$$renderer.global.css.add`). |
| **5m** | **`<svelte:options>` + compile-option axis (§3.8).** A single options resolver folding compile options ∪ `<svelte:options>` inline overrides into the runtime backends, owning the output-affecting options without a dedicated home: `namespace` (`$.from_html`/`$.from_svg`/`$.from_mathml`), `fragments` (`$.from_html` vs `$.from_tree`), `preserveWhitespace`, `preserveComments`, `discloseVersion` (disclose-version import toggle), `compatibility.componentApi` (Svelte-4 `createClassComponent` client / `.render` server), `hmr` (`$.hmr` + `import.meta.hot`), and the legacy-gated `accessors` (`$.push`/`$.pop($$exports)` + prop getters-setters) / `immutable` (prop flag). The resolver also resolves `name` (`options.name ?? component_name`) and feeds it to the Block 4 component-naming step. The `<svelte:options>` reader reproduces the DEPRECATED `tag` HARD ERROR (`svelte_options_deprecated_tag`), NOT a fold. `name` mode-plumbing feeds Block 4 naming; `runes` mode-plumbing rides Block 4 / legacy lowering 5i; `css`/`cssHash` ride 5l; `customElement` rides 5h; `dev` rides 5k; `generate` rides 4/8; experimental async rides 5j. | 4, 5i, 5l | **Yes** — capture the per-option official corpus (name/namespace/fragments/whitespace/comments/discloseVersion/componentApi/hmr/accessors/immutable goldens, client + server) + the `<svelte:options tag>` error fixture. |
| **I** | **Integration so emitted JS is reachable (D2).** `@verter/unplugin` API (§10.1: `VerterVue` / `VerterSvelte` / `Verter({ lang })` + `/vue` and `/sveltejs` subpaths, `.svelte` in the filter); `.svelte` in `default_known_dependency_extensions` (NAPI + WASM); playground Svelte preview (import map for `svelte/internal/client` + `svelte/internal/server`); NAPI/host routing confirmation; docs. | 4 | — |
| **6** | **Sourcemap hardening + generated-JS syntax validation.** Token-precise maps for rune reads/writes + template-effect bodies; OXC-parse every generated module. | 4, 5a-5f | — |
| **7** | **jsdom behavioral harness (client).** Execute emitted modules against the real pinned `svelte/internal/client`; assert DOM behavior across the breadth set. STANDING block: stood up at `4, 5a-5f`; each later sub-block (5g-5m) lands its own jsdom cases into this harness at its own landing. | 4, 5a-5f (initial); 5g-5m add cases at their own landing | — |
| **8** | **SSR backend (first-class, D-10).** `svelte/runtime/server.rs` → `svelte/internal/server` output (the §3.2 server columns: `$$renderer.push`, `$.escape`, `$.attr`, `$.attr_class`/`$.clsx`, `$.attr_style`, `$.ensure_array_like`, comment markers); shares the pre-lowering IR with client (§3.6). Deliverables: server goldens regenerated from real `generate:'server'` output; a server-render BEHAVIORAL harness (render to string, assert HTML + `$.escape` escaping + `<!--[…-->`/`<!--]-->` comment markers). STANDING block: stood up at `3, 5a-5f`; each later sub-block (5g-5m) lands its own SSR golden + string-render case into this harness at its own landing. | 3, 5a-5f (initial); 5g-5m add cases at their own landing | **Yes** — capture the official SSR output corpus (`scripts/gen-svelte-goldens.mjs` server pass). |
| **9** | **CSR/SSR hydration round-trip gate (first-class, D-10).** Client output must hydrate the SSR output from the SAME compiler. Render the component server-side (Block 8) → mount the client module with `$.hydrate` over the SSR HTML → assert NO hydration mismatch + post-hydration interactivity (events fire, bindings update). This is an acceptance gate, not an aspiration: "SSR works" = this gate is green. | 7, 8 | — |
| **10** | **Svelte analysis / component-meta.** Generalize `ComponentMetaInput`; Svelte component-meta from `resolve_svelte_surface()` + `ParsedSvelte`. | 1 | — |
| **11** | **Diagnostics / lint abstraction + LSP wiring.** `target_framework()` gate, neutral/Vue/Svelte rule sets, Svelte-native rules, LSP lint wiring. | 10 | **Yes** — rule taxonomy + config migration. |
| **12** | **Perf-comparison CI (D-13 / §7, §10.2).** Svelte compiler perf-comparison CI job modeled on the existing Vue `@verter/benchmark` (`packages/benchmark/`) + `.github/workflows/benchmark.yml`. Named baseline fixtures, the §7 metric + ≤1.10× numeric gate, the wired workflow. STANDING block: the job + the §7 baseline-fixture set are stood up at `4, 5a-5f, 7`; the gate then runs INCREMENTALLY — each later feature-family sub-block (5g-5m) that touches the hot path adds its perf fixture and the gate runs at THAT sub-block's landing, so a regression is caught at the introducing block, not deferred to the end (§7). | 4, 5a-5f, 7 (initial); the gate runs incrementally per later sub-block landing | — |
| **13** | **Legacy deletions, docs, guards, final verification.** | all conformance/feature/SSR/hydration blocks | — |
| **14** | **Optimization block (narrow, conformance-gated, D-9 / §3.6).** Optimization classification table + gating infrastructure + the first safe pre-lowering-IR optimizations, applied to client AND SSR. Lands ONLY after Blocks 7+8+9 (conformance + hydration) are green. Acceptance gate per §3.6 (all conformance tests pass, hydration round-trip passes, every optimization has a red/green semantic-dead + optimized==non-optimized==official behavioral proof, fails closed on dynamic/unknown/external/side-effect). | 7, 8, 9 | **Yes** — confirm conformance + hydration gates green; pick the first safe-class set. |

### 10.1 Integration block (Block I — D2): make emitted runtime JS reachable

The runtime compiler is useless until the emitted `svelte/internal/client`-importing JS flows through
the bundler and the playground. All sites verified against the live tree:

- **`@verter/unplugin` `.svelte` transform.** Today the plugin is `.vue`-only:
  `packages/unplugin/src/index.ts::createFilter()` defaults to `(f) => f.endsWith(".vue")` and
  `transformInclude()` uses it (`parseVueRequest`). Extend the default filter to also match
  `.svelte`, route a `.svelte` file's transform to the Svelte runtime carrier output, and handle
  Svelte style/virtual-file sub-requests analogously to the Vue `query.type === "style"` path. The
  scoped CSS artifact (`RuntimeCompileOutput.styles` — the `svelte-<hash>` compiled CSS from Block 5l,
  §3.7) flows to the bundler as a style virtual file via this same `query.type === "style"` sub-request,
  mirroring Vue's style flow (the scoped class is already injected into the emitted HTML by the runtime
  backend; the `css: 'injected'` mode instead inlines the CSS into the JS body — `$.append_styles` on
  client, `$$renderer.global.css.add` on server — and emits no separate `styles` artifact, so the
  style-virtual-file flow is `css: 'external'` only).
  The emitted module's `svelte/internal/client` import is left for the bundler to resolve from the
  installed `svelte` package.
- **`@verter/unplugin` public API (D-12) — explicit deliverables.** Current exports shape (verified
  in `packages/unplugin/package.json` + `src/index.ts`): a SINGLE default export
  `createUnplugin(unpluginFactory)` plus per-bundler subpaths (`./vite`, `./rollup`, `./webpack`, …);
  there is NO `VerterVue` / `VerterSvelte` / `Verter` named export and NO `/vue` or `/sveltejs`
  framework subpath today. The ADDITIVE change (does NOT break the existing default entry — `VerterVue`
  re-exposes today's default behavior):
  - **Shared transform-core refactor.** Extract today's `unpluginFactory` body into ONE shared
    transform core parameterized by a resolved framework (`vue` | `sveltejs`). The framework is chosen
    by an explicit `lang` option or by per-file detection (`.vue` → vue, `.svelte` → sveltejs). The
    core owns filter, transform, virtual-file/style sub-requests, and HMR; the named exports + subpaths
    are THIN wrappers over it.
  - **Named exports.** `VerterVue` (framework pinned to `vue`), `VerterSvelte` (framework pinned to
    `sveltejs`), and `Verter({ lang: 'vue' | 'sveltejs' | 'auto' })` — `lang: 'auto'` (the default)
    detects per file from the extension; `lang: 'vue'` / `lang: 'sveltejs'` pin the framework. All
    three resolve to the shared core with the framework fixed accordingly.
  - **Package `exports` subpaths.** Add `@verter/unplugin/vue` and `@verter/unplugin/sveltejs` (each a
    thin wrapper file re-exporting the corresponding pinned factory, mirroring the existing per-bundler
    subpath wrapper pattern) to `package.json` `exports`, alongside the preserved default `.` entry.
  - **Backward-compat.** The existing default export and per-bundler subpaths keep their current
    behavior; the new named exports / framework subpaths are purely additive.
- **WASM + NAPI dependency extensions.** Add `".svelte"` to `default_known_dependency_extensions()`
  in BOTH `crates/verter_wasm/src/lib.rs` and `crates/verter_napi/src/lib.rs` (verified: the function
  is identical in both and currently lists `"", .ts, .tsx, .js, .jsx, .mts, .mjs, .cts, .cjs, .vue`
  — `.svelte` is absent). Dependency resolution will not see `.svelte` imports until this lands.
- **NAPI surface.** Verified: there is NO standalone `compile_sfc`/runtime-compile NAPI export; the
  NAPI host (`crates/verter_napi/src/lib.rs::NapiVerterHost`) funnels all compilation through
  `upsert()` → host compile → `getVirtualFile()`. So a Svelte sibling NAPI entry is NOT needed — once
  Block 1 routes `compile_entry()` through the registry, `getVirtualFile()` returns Svelte runtime JS
  with no NAPI signature change. (The only NAPI change is the `.svelte` extension above.)
- **Playground Svelte preview.** Verified Vue-hardwired: `packages/playground/src/core/importMap.ts::
  getDefaultImportMap()` only maps `vue` / `vue/server-renderer`, and
  `packages/playground/src/output/Preview.vue` mounts via `window.Vue` + `createApp()`. Complete the
  deferred Svelte preview: add `svelte` / `svelte/internal/client` (+ `svelte/internal/disclose-version`,
  and `svelte/internal/server` for SSR) entries to the import map (CDN ESM at the pinned `5.56.3`),
  and a Svelte mount branch in the preview srcdoc that imports the emitted module's `default` export
  and instantiates it with `$.mount`/`$.hydrate` rather than Vue `createApp()`. The import map uses
  the SAME pinned version as the oracle/jsdom harness (§5.3).
- **Docs touch-point.** Update `docs/arch/multi-framework-adapters-plan.md` (Invariant 4 supersession,
  D-7) and add a Svelte usage note to the user-facing unplugin / playground docs.

### 10.2 Perf-comparison CI block (Block 12 — D-13): first-class compiler-performance gate

Verter already runs a first-class Vue compiler perf-comparison CI. Verified against the live tree:
the bench package is `@verter/benchmark` (`packages/benchmark/`, with `src/`, `baselines/`,
`audit-specs/`); the wired workflow is `.github/workflows/benchmark.yml` (triggered on a `/benchmark`
PR comment and `workflow_dispatch`, builds `@verter/native`, runs the bench, uploads a
`benchmark-results` artifact, and posts a "Benchmark Results" comment back to the PR). The Svelte
perf job is MODELED on this — the same package, the same workflow shape — not a new harness.

- **Deliverable.** A Svelte compile bench in `@verter/benchmark` (a new `src/` suite reusing the
  package's existing runner + baseline/audit-spec machinery) plus a wired `/benchmark`-style job in
  `.github/workflows/benchmark.yml` (extend the existing matrix/steps; do NOT fork a second workflow).
- **What it compares.** Two comparisons, both first-class:
  1. **Verter-Svelte vs Verter-Vapor** of the SAME logical component — the §7 NUMERIC regression gate
     (≤ 1.10× mean wall-time and ≤ 1.10× `heapUsedMB` heap-delta vs the Vapor equivalent; target
     ≤ 1.00×). This is the gate that BLOCKS the block on regression.
  2. **Verter-Svelte vs the official Svelte compiler** (`svelte@5.56.3`, the §5.3 pin) — an INFORMATIONAL
     wall-time comparison reported in the PR comment (different toolchains, so NOT a pass/fail gate;
     it tracks how Verter's native compile compares to the reference compiler over time).
- **Named baseline fixtures** (the §7 set, one Vapor equivalent each, vendored in
  `packages/benchmark/baselines/`): `bench_hello`, `bench_list`, `bench_form`, `bench_conditional`,
  `bench_component`.
- **Metric (reconciled with §7 — the existing `@verter/benchmark` metrics).** Primary = mean compile
  wall-time (ms) per fixture over N ≥ 50 warm single-file runs (and the derived ops-per-sec);
  secondary = `heapUsedMB` heap-delta in MB (the `process.memoryUsage().heapUsed` before/after delta
  the bench already records — `packages/benchmark/src/index.ts` / `utils/stats.ts`). There is NO
  allocation-counting harness in `@verter/benchmark`; this gate reuses the EXISTING metrics rather than
  inventing one (adding allocation instrumentation would be an explicit named Block-12 deliverable, not
  an assumed harness). Reported per the CLAUDE.md bench rule (cache mode, source-map policy, batch
  shape, thread count, hit/fallback counts). The ≤ 1.10× threshold is the §7 gate verbatim — a single
  source of truth, not a second number.
- **CI integration.** The job runs in `.github/workflows/benchmark.yml` alongside the Vue bench, posts
  the Svelte-vs-Vapor gate result (pass/fail) AND the Svelte-vs-official informational delta to the
  PR comment, and uploads the Svelte results into the same `benchmark-results` artifact. A
  `> 1.10×` Vapor-relative regression FAILS the job (Block 12 gate).

---

## 11. Legacy Deletions

- Any `@verter/svelte-runtime` facade scaffolding, should any be present (none today — this program
  ensures none is built; D-1, §9).
- The "runtime codegen for non-Vue frameworks is out of scope" invariant in
  `docs/arch/multi-framework-adapters-plan.md` (Invariant 4) — superseded for Svelte (D-7); update it
  to reference this plan.
- Hardcoded `compile_sfc` / `compile_from_parsed` routing in
  `crates/verter_session/src/host_resolve/virtual_file_pipeline.rs::compile_entry()` — replaced by
  registry dispatch (Block 1).
- Generic use of `assemble_main_module` as if all frameworks are Vue-shaped — becomes Vue-only,
  Vue-bridge-owned (Block 1, D-6).
- Any dual-path / framework-branch / feature-flag in `compile_entry()` after Block 1 — forbidden by
  the §4.5 guards (`compile_entry_has_no_direct_compile_sfc_call`, `no_framework_branch_in_compile_entry`).
- The `.vue`-only default filter in `packages/unplugin/src/index.ts::createFilter()` — replaced by a
  filter matching `.vue` AND `.svelte` (Block I, D2).
- Svelte runtime typed-unsupported (`CompileUnsupported`) diagnostics — removed for each surface once
  the runtime codegen for it lands. Every in-scope block deletes ITS surface's diagnostic at its own
  landing: Blocks 4, 5a, 5b, 5c, 5d, 5e (control-flow blocks, `{@const}`, AND the `{const …}`/`{let …}`
  declaration-tag surface), 5f, 5g (runes completion), 5h (`$host`/custom-element), 5i
  (legacy non-runes AND store auto-subscriptions `$store`), 5j (experimental async flag-ON), 5k
  (dev-mode), 5l (`<style>`/CSS scoping), 5m (the `<svelte:options>`/compile-option axis — §3.8:
  `namespace`/`fragments`/`preserveWhitespace`/`preserveComments`/`discloseVersion`/
  `compatibility.componentApi`/`hmr`/`accessors`/`immutable`), and 8 (SSR). All of 5g-5m are
  IN-SCOPE blocks in THIS program — there is no "remains until a follow-up" deferral: the follow-up IS
  the in-scope block (§9.1). The only surfaces with NO diagnostic to delete are the official compiler's
  own non-emissions (experimental async with the flag OFF — §3.2.2/§9.1) and deeper-than-official
  optimizations (Block 14, conformance-gated — D-9). After every in-scope block lands, no
  `CompileUnsupported` path for an official `5.56.3` feature remains.

---

## 12. Verification

Canonical Rust gate (per CLAUDE.md):

- `cargo nextest run --workspace` (completeness — runs the `verter_session` integration suite).
- `cargo test -p verter_session --tests` (shared-process surface).
- `cargo clippy --workspace -- -D warnings`.
- `cargo fmt --all --check`.

Plus:

- `pnpm test` (TS packages, incl. `@verter/unplugin` `.svelte` transform + playground preview).
- `pnpm install --frozen-lockfile` (lockfile in sync).
- Svelte golden oracle (feature-gated; client + SSR), pinned to `svelte@5.56.3`.
- Golden sync + lockfile-drift guards (§5.3): `svelte_goldens_in_sync_with_pinned_compiler`,
  `svelte_lockfile_matches_oracle_pin`; `scripts/gen-svelte-goldens.mjs --check` clean.
- jsdom behavioral runtime harness (client) against the pinned `svelte/internal/client@5.56.3`
  runtime; SSR string-render harness (Block 8) against the pinned `svelte/internal/server@5.56.3`
  SERVER runtime (NOT the client runtime).
- CSR/SSR hydration round-trip gate (Block 9, D-10): SSR-render the component (server runtime) → mount
  the client module over the SSR HTML with `$.hydrate` → assert NO hydration mismatch + post-hydration
  interactivity (events fire, bindings update). "SSR works" = this gate is green.
- OXC parse-validation for every generated JS module.
- Sourcemap token tests for rune reads/writes + template-effect expressions; scope/shadowing
  discriminating tests (§3.3) green.
- Vue byte-identity characterization suite (Block 1) green before AND after the cutover.
- No-dual-path guards (§4.5): `compile_entry_has_no_direct_compile_sfc_call`,
  `no_framework_branch_in_compile_entry`.
- `.svelte` present in `default_known_dependency_extensions` (WASM + NAPI); unplugin compiles a
  `.svelte` fixture end-to-end; playground Svelte preview loads against the pinned runtime (Block I).
- Numeric perf gate (§7), run INCREMENTALLY per feature-family sub-block landing (not only at Block
  12): each named baseline fixture ≤ 1.10× its Vue Vapor equivalent on mean wall-time and `heapUsedMB`
  heap-delta (the existing `@verter/benchmark` metrics — no allocation-counter).
- Each new CRITICAL rule registered in `CRITICAL_RULE_GUARDS` (R6 meta-guard green).
