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

**Owner directive (2026-06-22, EXPLICIT and binding): FORMAT / COSMETIC differences DO NOT MATTER and are NEVER
a finding.** Intra-expression whitespace (`a+b` vs official `a + b`) and behavior-preserving redundant parens
(`(a+b)+c` vs `a+b+c`) are EXPLICITLY ALLOWED to differ from the official output — the downstream minifier
collapses them, so they have zero observable effect. Generated private local identifier spellings are waivable
only when the conformance oracle implements scope-aware alpha-equivalence for non-observable bindings; until
then, generated identifiers are structural and a rename is a finding. The corpus/oracle
compares **behaviorally + structurally** (helper family, call topology, reactivity decisions, DOM topology,
diagnostics), NEVER raw bytes, for these cosmetic categories. Chasing byte-identity on cosmetic formatting is a
PROCESS ERROR: a reviewer, corpus cell, or §1a check that flags a cosmetic-only diff as a blocker is WRONG —
only a BEHAVIORAL / structural divergence blocks. The "byte-faithful" / "byte-parity" wording elsewhere in this
plan (the block deliverables, [[codegen-byte-parity-doctrine]], D-14/D-15) means byte-parity of the NON-COSMETIC
structural content ONLY (helper choice, memoization, reactivity, class/style normalization, valueless-vs-empty
attr shape, reject/diagnostic ordering) — it NEVER requires cosmetic formatting parity. The corpus must compare
expression CONTENT structurally (parse + compare, or token-normalized), so a cosmetic whitespace/paren diff
PASSES as correct (it is correct), while a behavioral/structural diff still FAILS.

**Conformance taxonomy (divergence disposition).** Every observed difference from the pinned official
compiler falls into exactly one bucket:

1. **Official parity (the default).** The pinned official output is the oracle; Verter matches it
   behaviorally + structurally. No annotation needed.
2. **Temporary divergence.** A known, not-yet-implemented gap → a debt-ledger row (a `D-*` entry), and
   the surface stays FAIL-CLOSED until the row is retired. Never a silently-shipping divergent `Main`.
3. **Deliberate final deviation.** A permanent, intentional difference → an explicit accepted-divergence
   record (what differs, why, and the guard that pins it). Rare and reviewed.
4. **Cosmetic formatting.** Waived by the owner directive above — never a finding, never a record.

The "official-can-have-bugs" nuance is admissible ONLY in its narrow form: official remains the DEFAULT
oracle, and rejecting an official behavior requires (a) proof the official behavior is defective, (b) a
fail-closed guard on the affected surface, and (c) an explicit accepted-divergence record per bucket 3.
"We think ours is better" without all three is bucket 2 debt, not a deviation.

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
  trait owns `parse` / `eval_source` / `compile_ide` / `template_data` AND `compile_bundle` (the
  framework-neutral runtime op — there is NO separate `compile_runtime` method). Vue impl
  `vue_bridge::VueCarrierCompiler`; Svelte impl `svelte/carrier.rs::SvelteCarrierCompiler`. Registry
  `framework_common/registry.rs::CarrierCompilerRegistry::built_in()`. The host RUNTIME path IS
  routed through the carrier: `virtual_file_pipeline.rs` calls `compile_bundle` and emits the `Main`
  virtual node from `RuntimeCompileOutput.main.body_code` when `has_runtime_surface()` is true.
  Svelte's `compile_bundle` emits the native `svelte/internal/client` module for the supported runes
  subset (the `svelte/runtime/client.rs` backend) and FAILS CLOSED with a typed
  `UnsupportedSvelteRuntimeSurface` (a precise non-fatal diagnostic) for every unsupported surface.
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
10. **Cosmetic / format differences are EXPLICITLY WAIVED (owner directive, §1.2) — flagging one is a STOP-level
    process error.** Intra-expression whitespace and behavior-preserving redundant parens MAY differ from
    official and are NEVER a finding. Generated private local identifier names MAY differ only when the oracle
    implements scope-aware alpha-equivalence; under the current Svelte client comparator they are structural and
    a rename is a finding.
    The bar is behavioral + structural / helper-topology parity; the corpus compares expression content
    STRUCTURALLY (parse/token-normalized) for cosmetic categories, NEVER raw bytes. Wasting review/fix rounds to
    make cosmetic formatting byte-identical (e.g. an esrap-faithful re-printer for spacing) is forbidden — only
    a BEHAVIORAL / structural divergence (helper choice, memoization, reactivity, class/style normalization,
    attr-shape, diagnostic ordering) blocks.

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
- **Special-element SSR / dynamic-title variants → Blocks 5f-b / 8.** `<svelte:head>` with a dynamic title →
  `$.deferred_template_effect`, static metadata → `$.template_effect`, SSR `$$renderer.title(…)`;
  `<svelte:boundary>` failed → `$$renderer.boundary`, pending-only markers. 5f-b/8 STEP-0 pin the
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
| `$host()` + `<svelte:options customElement>` | `$$props.$$host` + the module epilogue `$.create_custom_element(Cmp, props, [], [], shadowRootInit?, extend?)` — a CONDITIONAL 6-arg shape: arg5 is `{ mode: 'open' }` for the open/default shadow, OMITTED for `shadow:'none'` (spelled `void 0` when an `extend` arg6 follows), and the verbatim object expression for an object shadow; arg6 is present only when `extend` is given. `customElements.define(tag, …)` wraps the create call ONLY for a tagged descriptor (`{}` / compile-option-`true` emit the bare create; `customElement={null}` is a null-SKIP that falls back to the `customElement` compile option — a set compile option still applies; only with no option does the component compile plain). The body frame is FACT-DRIVEN, not blanket: `$.push($$props, true)` + `var $$exports = { get/set … }` + `return $.pop($$exports)` ONLY when prop accessors exist; `$.push`/`$.pop` ALSO fires via the INDEPENDENT `needs_context` fact (it is NOT gated on CE prop accessors — a no-props custom element keeps the plain frame only when nothing else sets `needs_context`) | n/a | 5h | in-scope |
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
| `bind:this` (component) | `$.bind_this(Child(…), set, get)` | 5f-a | in-scope |
| `bind:currentTime` / `bind:paused` / `bind:duration` / `bind:played` | `$.bind_current_time` / `$.bind_paused` / `$.bind_property('duration', 'durationchange', …)` / `$.bind_played(el, set)` | 5c | in-scope |
| `bind:clientWidth` / `bind:clientHeight` / `bind:offsetWidth` / `bind:offsetHeight` | `$.bind_element_size(el, 'clientWidth', set)` | 5c | in-scope |
| `bind:innerHTML` (contenteditable) | `$.bind_content_editable('innerHTML', el, get, set)` | 5c | in-scope |
| `bind:open` (details) | `$.bind_property('open', 'toggle', el, set, get)` | 5c | in-scope |
| `<svelte:window>`/`<svelte:body>`/`<svelte:document>` binds | window size `$.bind_window_size('innerWidth', set)`, window scroll `$.bind_window_scroll('x', get, set)`, window `online`; body `bind:this` + element-size `$.bind_element_size($.document.body, …)` (body has NO `scrollX`/`scrollY` — `<svelte:body bind:scrollX>` is an official compile error); document `bind:this`/`activeElement`/property binds | 5f-b | in-scope |
| component `bind:prop` (+ component function binding `bind:x={get,set}`) | getter/setter pair on the component props object (`get value()/set value($$v)`) | 5f-a | in-scope |

#### Events

| Surface | Official client helper(s) | Verter block | Disposition |
| --- | --- | --- | --- |
| delegated event (`onclick`, `onkeydown`, …) | `$.delegated('click', el, handler)` + ONE module-scope `$.delegate(['click', …])` (ordered set) | 4 | in-scope |
| non-delegated event (`onfocus`/`onblur`) | `$.event('focus', el, handler)` | 5d | in-scope |
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
| `{#each}` keyed | `$.each(node, FLAG, () => items, (item) => key, ($$anchor, item) => {…})` — the `item` binding is a SIGNAL: reads inside the block are `$.get(item)` (e.g. `$.set_text(text, $.get(item).x)`, `() => ($.get(item).x++)`), NOT inert plain locals (§3.3) (the SIGNAL-marking is 5e; the member reactive-TEXT shape `$.get(item).x` itself rides the global Block-4 reactive-text breadth — D-35.) | `<!--[-->` + `const each_array = $.ensure_array_like(items)` + `for` loop pushing items + `<!--]-->` | 5e | in-scope |
| `{#each}` unkeyed | `$.each(node, FLAG, () => items, $.index, ($$anchor, item, idx) => {…})` — `item` binding is a `$.get` signal as in the keyed row (§3.3) | `$.ensure_array_like` + `for` (as keyed) | 5e | in-scope |
| `{#each}…{:else}` | `$.each(node, FLAG, …, eachFn, elseFn)` | `for` else-branch | 5e | in-scope |
| `{#await}` | `$.await(node, () => p, pendingFn, thenFn, catchFn)` — the `{:then x}` / `{:catch e}` bindings are SIGNAL reads (`$.get(x)`) inside their branch, like the each binding (§3.3) | `$.await($$renderer, promise, pendingFn, thenFn[, catchFn])` (the server emits the SAME `$.await` helper threading `$$renderer`, NOT a sync resolve) + a trailing `$$renderer.push('<!--]-->')` marker | 5e | in-scope |
| `{#key}` | `$.key(node, () => k, ($$anchor) => {…})` | re-render | 5e | in-scope |
| `{@const x = …}` (runes mode) | block-local `const x = $.derived(() => …)` (a derived memo over the block scope); reads `$.get(x)`. (Legacy non-runes mode emits `$.derived_safe_equal(() => …)` instead — §3.2.1.) | plain `const x = …` | 5e | in-scope |
| `{const x = …}` / `{let x = …}` (5.56 declaration tag — a DISTINCT AST node `DeclarationTag`, NOT `{@const}`) | a PLAIN block-local declaration `const x = …` / `let x = …` (an INERT local binding, NO `$.derived` memo, NO `$.get` read); the declarator may itself carry runes (`{let a = $state(0), b = $derived(a * 2)}` registers state transformers like an instance-script decl) and may be async (drives an async-declaration thunk) (REGION-ROOT placement is 5e; NESTED-in-element placement requires per-element `BlockStatement` scoping + per-block effect split — a separate nested element-scope codegen axis, fail-closed in 5e, tracked as D-36.) | plain `const x = …` / `let x = …` | 5e | in-scope |
| `{@debug a, b}` | `$.template_effect(() => { console.log({ a: $.snapshot(a), b: $.snapshot(b) }); debugger; })` (a reactive effect logging snapshots + `debugger`) | `console.log({ a, b }); debugger;` (sync, no effect) | 5e | in-scope |
| `{#snippet}` def | module/local arrow `const name = ($$anchor, p = $.noop) => {…}` | `function name($$renderer, p) {…}` | 5f-a | in-scope |
| `{@render name(args)}` static | DIRECT call `name($$anchor, () => arg)` | direct call `name($$renderer, arg)` | 5f-a | in-scope |
| `{@render expr?.()}` dynamic | `$.snippet(node, () => expr ?? $.noop)` | `$.snippet` | 5f-a | in-scope |
| `{@attach expr}` (5.29 attachment) | `$.attach(el, () => expr)` (the attachment fn runs as an effect on the element) | omitted (attachment effects don't run in SSR) | 5f-c | in-scope |
| component (static) | DIRECT `Child($$anchor, { foo: p, $$events: {…} })` | DIRECT `Child($$renderer, { foo: p, children: ($$renderer) => {…}, $$slots: {…} })` | 5f-a | in-scope |
| `<svelte:component this={C}>` | `$.component(node, () => C, ($$anchor, $$c) => $$c($$anchor, {}))` | dynamic call | 5f-a | in-scope |
| `<svelte:element this={tag}>` | `$.element(node, () => tag, false)` | dynamic tag | 5f-b | in-scope |
| `<svelte:self>` (recursive self) | DIRECT recursive call to the component's OWN function `Name(node, {…})` — no dedicated helper (self-recursion) | recursive `Name($$renderer, {…})` | 5f-a | in-scope |
| `<svelte:fragment slot="x">` (transparent slot group) | NO dedicated helper — lowers into a `$$slots: { x: ($$anchor, $$slotProps) => {…} }` entry on the enclosing component call | `$$slots: { x: ($$renderer) => {…} }` entry | 5f-a | in-scope |
| `<svelte:head>` | `$.head('<hash>', ($$anchor) => {…})` (the head content rendered into the document head, keyed by a stable hash) | `$.head('<hash>', $$renderer, ($$renderer) => {…})` (e.g. `$$renderer.title(…)`) | 5f-b | in-scope |
| `<svelte:document>` | `$.event(name, $.document, handler)` for events; binds via `$.document` (e.g. `$.bind_*(… $.document …)`) | n/a (document-scoped effects don't run in SSR) | 5f-b | in-scope |
| `<svelte:boundary>` | `$.boundary(node, { failed, pending }, ($$anchor) => {…})` (error/pending snippets passed as a props object) | `$$renderer.boundary({ failed }, ($$renderer) => {…})` | 5f-b | in-scope |
| `<svelte:window>` / `<svelte:body>` | `$.event(…, $.window\|$.document.body, …)`, `$.bind_window_size(…)` | n/a | 5f-b | in-scope |
| transition: / in: / out: | `$.transition(FLAG, el, () => fn[, getParams])` (FLAG: `TRANSITION_IN`=1, `TRANSITION_OUT`=2, both=3; `\|global` adds `TRANSITION_GLOBAL`=4 → 5/6/7; `\|local` is the default, no +4; the getParams thunk present IFF params) | n/a | 5f-c | in-scope |
| `animate:` | `$.animation(el, () => fn, getParams)` — ALWAYS 3 args (`null` when no params); keyed-each-only (the each FLAGS gain `EACH_IS_ANIMATED`=8); NEVER `$.transition` | n/a | 5f-c | in-scope |
| `use:` action | `$.action(el, ($$node[, arg]) => fn?.($$node[, arg])[, () => arg])` | n/a | 5f-c | in-scope |

#### Module-shape, imports, flags

| Surface | Official client output | Server |
| --- | --- | --- |
| disclose-version import | `import 'svelte/internal/disclose-version';` (always, client) | absent |
| client runtime import | `import * as $ from 'svelte/internal/client';` | `import * as $ from 'svelte/internal/server';` |
| user `.svelte` default import | `import Child from './Child.svelte';` — a DEFAULT import of a `.svelte` component module (the imported local consumed as a component callee OR dynamic component value, e.g. `<svelte:component this={Child}>`), hoisted to module scope immediately AFTER `import * as $`, in source order (the 5f-a component-callee / dynamic-component-value subset via the general `UserImport` carrier; broad named / namespace / side-effect / non-`.svelte` import hoisting is 5s) | same slot (server runtime ns) |
| legacy-mode flag | `import 'svelte/internal/flags/legacy';` (emitted iff the component is NOT in runes mode — `analysis.runes = runes_option ?? inferred-from-rune-presence`, where `runes_option` is the explicit `<svelte:options runes>` or compiler `runes` option; the flag is emitted iff `!analysis.runes`. The explicit option OVERRIDES inference — `<svelte:options runes>` suppresses the flag with zero runes, and `runes={false}` forces it even WITH runes; §3.2.1) | absent |
| async flag | `import 'svelte/internal/flags/async';` (emitted only when experimental async is used — §3.2.2) | `import 'svelte/internal/flags/async';` — the server ALSO emits the async flag when experimental async is on (verified: a flag-ON `generate:'server'` compile emits it ahead of `import * as $ from 'svelte/internal/server';`); absent when the flag is off |
| component fn | `export default function Name($$anchor[, $$props]) {…}` | `export default function Name($$renderer[, $$props]) {…}` |
| `$.push`/`$.pop` | wraps the body ONLY for specific cases (`$.push($$props, runes_bool)` … `$.pop()`): a bindable prop (`$bindable`), a customElement with prop accessors (the FACT-DRIVEN `$$exports` get/set frame), the INDEPENDENT `needs_context` fact (`client_plan.rs` computes it separately from the CE prop accessors — a no-props customElement keeps the plain frame only when nothing else sets `needs_context`), or a legacy-reactive / store-subscription context. NOT emitted for a simple `$props()` destructure / rest or a simple legacy `export let` (verified: both consume `$$props` yet emit NO `$.push`/`$.pop`) | n/a |
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
  is NOT this signal and must not be rewritten). See the 5e rows in §3.2. The 5e-local responsibility is ONLY marking each/await-introduced bindings as `$.get`/`$.set` SIGNALS (so a bare `{item}` read emits `$.get(item)` and a handler write emits the signal-write form); the cited member reactive-TEXT shape `$.get(item).x` inside `{ … }` text is the GLOBAL reactive-text/interpolation breadth axis (member / optional-member / call / binary / conditional interpolation expressions, plus static/non-reactive reads — whether standalone or inside an otherwise-simple mixed run; simple mixed text with bare-signal / no-default-prop interpolations is already supported in element-hosted / block-body text-run placements — a naked component-root text-node region is separately refused by the root-region gate as `RootTextRegion`), owned by Block 4 and tracked as **D-35** — refused identically at top level and in-block by the pre-existing `classify_interpolation_shape` gate until that breadth lands, and NOT a 5e deliverable.
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

**DEBT LEDGER — deferred CSS ANALYSIS-PHASE diagnostics ONLY (owner: CSS-scoping block 5l; upstream
`phases/2-analyze/css`).** The Block-4 official-reject gate parses a `<style>` CSS body via a validation-only
port of upstream's `read/style.js` PARSE-ENTRY control flow (`svelte/runtime/css_reject.rs`, reserved by the
parser's `StyleBodyProbe` at the `read_style` position, BEFORE `style_duplicate`), so a malformed CSS body's
exact PARSE-ENTRY code wins the first-error race over `style_duplicate` (the parity-bar requirement for a
reachable 2nd-`<style>`-body official reject). The PARSE-ENTRY codes this port reproduces —
`css_expected_identifier` / `css_empty_declaration` / `css_selector_invalid` / `expected_token` (required
comment-close + brace tokens) / `unexpected_eof` — are NOT deferred: any `read/style.js` parse-entry code is
a MUST-FIX (a missed/wrong one is a BUG, never ledgerable).

This deferral covers EXACTLY the upstream POST-PARSE CSS ANALYSIS family, which is raised ONLY in
`phases/2-analyze/css/css-analyze.js` (verified — these codes never originate in `read/style.js`):
`css_global_*` (global-block placement / combinator / declaration / modifier rules),
`css_nesting_selector_invalid_placement`, and `css_type_selector_invalid_placement`. Each of these forms
PARSES clean (`parse()` accepts; a clean-CSS 2nd style still reports `style_duplicate`) and is rejected only
later by `compile()`'s analysis phase. They are NOT reachable as a wrong-code parity miss in the §1.2-core
surface today (any top-level `<style>` already fails closed as an unsupported FEATURE, and the parse-entry
codes are the only ones a 2nd-style body race surfaces). They land with the 5l CSS-analysis port. Owner
block: 5l (CSS-scoping). Upstream phase: `phases/2-analyze/css` (analysis), NOT `phases/1-parse/read/style.js`
(parse).

**DEBT LEDGER — exotic raw-block / `lang=` scan corners OUTSIDE the finite lower-case raw-block contract
(owner: `svelte-native-parser-parity`; phase: post-Block-4 parser parity expansion).** The Block-4 finite
contract covers lower-case `<script>` / `<style>` raw blocks and the deterministic `lang=` selection forms
(quoted / unquoted / empty / `ts` / `tsx` / `typescript` / `TS` / no-lang / unrelated-quoted-substring /
rightmost-overriding — the `script_lang` parse-parity axis + the `lang_scan_*` unit tests). Two exotic
corners sit OUTSIDE that finite set, and BOTH FAIL CLOSED (behavioral parity is met; only an exotic exact
code or an UNOBSERVABLE internal grammar choice differs):

- **Quoted-`>` between two `lang=` attributes** (`<script lang=js data-x=">" lang="ts">`). The official
  constructor regex's `[^>]*` prefix cannot cross the quoted `>`, so the OFFICIAL grammar scan selects the
  EARLIER `lang=js`; Verter's attribute-aware byte scan skips the quoted value and selects the later
  `lang="ts"`. This grammar-scan divergence is UNOBSERVABLE end-to-end: the source carries TWO `lang=`
  attributes, so the official compiler AND Verter both reject it with `attribute_duplicate` (oracle-pinned;
  locked by `quoted_gt_between_two_langs_rejects_with_attribute_duplicate_both_directions` at the gate level
  and characterized at the scan level by `lang_scan_quoted_gt_between_two_langs_is_an_out_of_finite_scope_grammar_divergence`).
  The divergent grammar choice never reaches a body parse. Oracle fixtures: both directions reject
  `attribute_duplicate` (a both-reject SAME-code case — strictly stronger than the both-reject-different-code
  bar).
- **Uppercase `<SCRIPT>` / `<STYLE>` raw tags.** The official raw-block recognizer is case-sensitive on the
  lowercase tag name; an uppercase `<SCRIPT>` / `<STYLE>` is NOT a raw block and parses as an ordinary
  element. Verter's §1.2-core raw-block surface is the lower-case form only; an uppercase raw tag is outside
  the finite contract. Both the official compiler and Verter treat it as a non-raw element (fail closed: it
  is not a §1.2-core supported surface), so behavioral parity holds. Oracle fixtures land with this owner.
- **`customElement={EXPR}` with an UNTERMINATED block comment swallowing the close brace**
  (`<svelte:options customElement={1 /* unterminated} />`). Verter's `customElement={…}` inner span is
  delimited by the COMMENT-AWARE brace matcher (`find_matching_brace_in`), whose `/*`-to-EOF skip consumes
  the `}` into the comment, so no closing `}` is found, the span runs to true EOF, and the missing required
  `}` (`eat('}', true)`) yields `expected_token`. The official compiler reaches a `js_parse_error` along its
  acorn `read_expression` path on the same source. BOTH REJECT (no `Main` leak); only the exact code differs,
  and the construct sits OUTSIDE the recognized `customElement` value axes (a value carrying an unterminated
  comment is not one of the enumerated string / object / null / number / identifier / expression forms). An
  exotic, out-of-finite-scope corner: behavioral parity (reject ⇔ reject) holds.

When this block lands, each corner gets an oracle-pinned fixture (official-accept → an unsupported row if
Verter does not compile it; both-reject → the recorded exact code, noting any divergence). Owner:
`svelte-native-parser-parity`. Upstream phase: `phases/1-parse` (the constructor `lang=` regex +
`read/script.js` / `read/style.js` raw-block recognition).

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
| `customElement?: boolean \| {tag, shadow, props, extend}` | compile + `<svelte:options customElement>` | the CONDITIONAL 6-arg `$.create_custom_element(Cmp, props, [], [], shadowRootInit?, extend?)` — arg5 `{ mode: 'open' }` open/default, OMITTED for `shadow:'none'` (`void 0` when an `extend` arg6 follows), verbatim object shadow; arg6 only when `extend` given — wrapped in `customElements.define(tag, …)` ONLY for a tagged descriptor (`{}` / compile-option-`true` create WITHOUT define; `{null}` is the backwards-compat null-SKIP with the `customElement` compile option as fallback). The body frame is FACT-DRIVEN: the `$.push($$props, true)`/`$.pop()` context frame is driven by `needs_context` (a reactive-analysis reason — e.g. an unsafe render callee — OR non-empty `$$exports` accessor exports); `props_param_bound` (`real_props_binder || needs_context`) separately controls `$$props` binding / bare-`$host()` admission — a real props binder alone (rest-only / whole-object `$props()`) does not open the frame; the `$$exports` get/set accessors + `return $.pop($$exports)` only when prop accessors exist (`$host()` surfaces as `$$props.$$host`). Verified: define + `$.create_custom_element` emitted. Forces `css:'injected'`. | n/a | 5h (already owns this — confirmed stays) |
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

### 4.5 No dual-path shim survives the Block 1 routing (D5 guard) — LANDED

The Block 1 routing is a CLEAN replacement, not a dual path (CLAUDE.md legacy-deletion rule). After
Block 1, the direct `compile_sfc` / `compile_from_parsed` / `vue_parse` calls in `compile_entry()`
are GONE — replaced wholesale by `CarrierCompilerRegistry::compile_bundle` dispatch, with Vue a
registered runtime carrier. No `if framework == Vue { old_path } else { registry }` branch, no
feature flag preserving the old routing; `assemble_main_module` was renamed `assemble_vue_main_module`
and consumes the NEUTRAL `RuntimeCompileOutput` (Vue still leaves `main.body_code = None` so the host
assembles `_sfc_main`). The landed AST/`syn` static guard
**`compile_entry_routes_through_carrier_registry_not_hardcoded_vue`**
(`crates/verter_session/tests/svelte_compiler_block1_guards.rs`) asserts `compile_entry` (and its
one-level local helpers) calls none of `compile` / `compile_from_parsed` / `compile_sfc` / `vue_parse`,
AND that `virtual_file_pipeline.rs` imports none of them under any alias / rename / glob — with a
negative self-test proving the guard catches a renamed import. This pairs with the §4.4 byte-identity
suite (`svelte_compiler_block1::vue_runtime_main_is_byte_identical_through_the_carrier` +
`vue_ide_output_is_byte_identical_through_the_carrier`, `include_str!` goldens): Vue output is
identical AND the old path is physically deleted. The §APIDECISION ruling-B IDE-ensure path landed as
`ensure_ide_compiled` (+ the `CompileDemand` enum), pinned by `get_ide_is_a_pure_cached_read_no_compute`
and `ensure_ide_compiled_never_requests_virtual_node_main`.

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
  touches the compile hot path (5a-5k, incl. 5s), not deferred to a single end-of-program run. This
  `5a-5k (incl. 5s)` range is the deliberately-bounded COMPILE-HOT-PATH subset — narrower than the
  all-feature-block range `5g-5n (incl. 5s)`: `5s` (static-import-prelude codegen) sits inside the span
  and IS gated, while `5l` (CSS scoping), `5m` (compile-options), and `5n` (script/module-item
  completion) are intentionally OUTSIDE it (not compile-hot-path codegen; they land after the range). A
  regression introduced in (e.g.) 5e is therefore caught when 5e lands, not discovered much later at Block 12.
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
| D-14 | **The mixed-template const-fold is a decidable TRI-STATE contract, NOT full JS-semantic exactness.** The `build_template_chunk` evaluator (`svelte/runtime/reactive_fold.rs` + `reactive_fold_tristate.rs` + `reactive_fold_globals.rs`) classifies each constant chunk as exactly one of **`Fold(exact)`** (only when traversed exactly as Svelte `Evaluation` INCLUDING its eagerness — both logical operands + both conditional branches evaluated before selecting, template literals stop at the first unknown; every op/global in the checked-in `ExactFold` allow-list; byte-exactly emittable; proven non-throwing) \| **`Live{ledger}`** (a known-but-not-byte-exact value → emit the live `?? ''` expression, ledgered in the checked-in `LIVE_FALLBACK_LEDGER`: BigInt-vs-Number precision compare, huge-finite `ToInt32`/`ToUint32`, `parseInt` radix/non-ASCII-whitespace, lone surrogate, AND every TRANSCENDENTAL `Math.*` — Rust system libm vs V8 `fdlibm` is not provably bit-identical cross-platform, so only `sqrt` + the IEEE-754-mandated/integer/decimal-scan/string/constant set fold exactly) \| **`Refuse`** (a compile-time JS throw official also compile-fails — mixed BigInt+Number arithmetic/bitwise, `/`/`%`/`>>>` BigInt, `+`bigint, `in`/`instanceof` over a primitive, a throwing global, AND eagerness throws like `false && (1n/0n)` → a deterministic `svelte-runtime-unsupported-const-fold-throw` refusal, NEVER live code that would crash at runtime). **Stopping rule: wrong fold is forbidden; a known compile-time throw must refuse; non-throwing exactness gaps live-fall-back only with a ledger reason row.** Gate: the 3-bucket corpus (`fold-exact` / `refuse` / `live-fallback`) generated by `gen-svelte-codegen-corpus.mjs` + the `svelte_client_emit_topology.rs` bucket gates (`refuse_bucket_cells_are_refused_with_const_fold_throw`, `live_fallback_bucket_cells_emit_live_not_the_folded_literal`, `const_fold_buckets_cover_every_required_family_and_eagerness`). |
| D-15 | **ACCEPTED RESOURCE-BOUNDARY CONFORMANCE DIVERGENCE — BigInt const-fold maximum-size limit (uses the D-14 Refuse mechanism, but at a resource boundary where the official compiler may FOLD — distinct from D-14's official-throw refusals; an outside-surface ledgered exception per [[codegen-byte-parity-doctrine]]; NOT a DEFER; no owner; NOT plan-close debt). Cite: codex-architect ACCEPT ruling (2026-06-28).** Verter conservatively REFUSES certain magnitude-growing BigInt const-folds at the `2^30`-bit size boundary: its cheap O(1) bit-bound guards refuse rather than materialize the ~134 MB result to decide exactly, over-refusing in BOTH genuinely-undecidable cases (V8 might throw `RangeError: Maximum BigInt size exceeded`) AND some PROVABLY-foldable cases (V8 would fold, but Verter refuses to avoid emitting the 100 MB+ constant — e.g. unary `-`, which preserves the operand's bit-width, and `*` at the edge of its `x_bits + y_bits >= 2^30` band). **Svelte version:** `5.56.3`. **Boundary (no practical fixture):** an allowed `2^30`-bit operand CAN be written inline (e.g. `1n << ((2n ** 30n) - 1n)`), but only by materializing the ~134 MB value — so there is no practical template/corpus fixture; the resource boundary is instead pinned by guard-level tests against the guard functions at `BIGINT_MAX_BITS` in `reactive_fold_bigint.rs`. **Official output (at the boundary):** V8 FOLDS to the ~134 MB literal when the true result is `2^30` bits, or compile-fails when it is `2^30 + 1`. **Verter output:** `Refuse` (`ConstFoldRefuse::BigIntMaxSizeExceeded`) — a deterministic `svelte-runtime-unsupported-const-fold-throw` refusal, never live code. **Proof:** the cheap O(1) bit-bound guards decide on a result's UPPER bit-bound; well-below-boundary operations FOLD byte-exact matching V8, while at the boundary the conservative guards over-refuse — including a thin band of PROVABLY-safe cases (mul's `x_bits + y_bits >= MAX` refuses the equality band; the unary `+1` ceiling refuses a same-magnitude `-x`) — rather than allocate the ~134 MB value to decide. The guards (in `crates/verter_compiler/src/svelte/runtime/reactive_fold_bigint.rs`) span the full magnitude-growing surface — binary `+` / `-` / `&` / `\|` / `^` / `<<` / `**` / `*` and a negative-count `>>`, plus unary `~` / `-` — implemented as cheap bit-bound checks (`left_shift_exceeds_max` / `pow_exceeds_max` / `mul_exceeds_max` / the add-sub-bitwise ceiling / `unary_exceeds_max`, the last called from `eval_unary` in `reactive_fold.rs`). Some are EXACT and match V8 with no divergence (e.g. `<<` result-bits); the rest conservatively over-refuse at the limit, and the source is the authority for each guard's exact-vs-conservative boundary. Every guard is fail-closed — never wrong output. This is an accepted RESOURCE-BOUNDARY conformance divergence — NOT a D-14 official-throw refusal: Verter deliberately refuses some folds the official compiler would perform, trading exact fidelity on absurd-size constants for bounded compile-time memory. It is fail-closed (never wrong output — only a deterministic refusal, never a wrong value or a runtime crash), it touches none of the non-cosmetic byte-parity categories, and it is an ABSURD-input-only case (a 100 MB+ BigInt literal in a class/style value, never a real template constant) — an outside-surface, fail-closed, ledgered resource-policy exception per [[codegen-byte-parity-doctrine]]. **Future optimization (NOT plan-close debt):** a future arbitrary-precision exact path (or a bounded interval analyzer) at `crates/verter_compiler/src/svelte/runtime/reactive_fold_bigint.rs` could fold the exact-`2^30`-bit boundary V8 folds without the ~134 MB allocation — an OPTIONAL precision improvement, never a required deliverable (the `TODO(follow-up)` marker — a module-level comment at `reactive_fold_bigint.rs:38` — denotes THIS optional optimization, not an open deferral). The accepted conservative Refuse is the PERMANENT contract; this row is an accepted divergence and is explicitly NOT counted among the plan-end open deferrals at close-out. |
| D-17 | **COMPARATOR FEED-FORWARD DEBT — structural-comparator completeness gaps that are not accepted-positive today (owner: the Svelte client conformance-comparator owner; hard trigger: the first change that opens native `<script module>` client emission — at which point `module_import_export` becomes a gated `SUPPORTED_MATRIX` slug and the now-encoded import/export oracle axes move from oracle-only to gated — OR the first change that makes a genuinely TS/module-only declaration form (the only remaining discriminant-only collapse) accepted client-positive; or any change that broadens accepted client output across those surfaces).** Cite: conformance-rule / Svelte-client-emit-conformance-comparator block codex-DEFER ruling (2026-06-25). The AST-structural emit comparator (`conformance_sig` -> `program_sig` / `expr_sig` / `params_sig` / `binding_sig` / `stmt_sig` / `decl_sig` / `function_body_sig`; `module_sig` on `ModuleConformanceSig`) is the oracle wired at both Svelte client gate sites. It now encodes the strict-mode-reachable ordinary-JS statement set (`for` / `for-in` / `for-of` / `while` / `do-while` / `switch` / `try`/`catch`/`finally` / `throw` / `break` / `continue` / `labeled` / `debugger` / `empty` / class declarations; a `WithStatement` arm is also encoded but `with` is INVALID under `SourceType::mjs()` (module/strict), so that arm is unreachable for emitted mjs and DEFENSIVE-ONLY), the module import/export ORACLE family (import `phase` + `with`-clause attributes, the full `ExportNamedDeclaration` surface, `ExportAllDeclaration`, the program `hashbang`), `FunctionBody.directives` AND `Program.directives` (directive prologues), a bounded class skeleton (`class_sig` / `class_element_sig` — `class_sig` encodes `Class.r#type`, decorators, id, super-class, and the runtime-bearing members (method kind/static/computed/async/generator/params/body, property/accessor key+value, static blocks) for `SourceType::mjs()`-parseable emitted classes; `Class.r#type` (`ClassDeclaration` vs `ClassExpression`) is behavior-bearing at `export default` — a declaration binds the name in module scope while an expression does not; TS-only member axes (abstract/accessor-type/index-signature) are stripped pre-emit and ignored), the client-reachable EXPRESSION surface (`ClassExpression` routes through the same `class_sig`; `YieldExpression` — `delegate` + arg; dynamic `ImportExpression` — source/options/phase; `MetaProperty` — `import.meta` / `new.target`; `PrivateInExpression` — `#x in obj`; `Super`), and recursive destructuring binding defaults through `binding_sig` — each PARSEABLE-under-`SourceType::mjs()` axis covered by a discriminator test (the encoded-but-mjs-unreachable arms — the `WithStatement` arm above — are signed DEFENSIVELY, with no planted positive). A stray no-op `EmptyStatement` (`;`) in a statement LIST is filtered by `statements_sig` (printer-dropped cosmetic no-op) AND the comment-anchor index mirrors that filter (`CommentAnchorIndex::normalize_statement_list` indexes real statements by their LOGICAL empty-filtered index and gives a comment attached to a filtered empty a synthetic `empty_gap[<logical>.<empty_ordinal>]:EmptyStatement` anchor — the per-gap empty ordinal keeps a semantic comment moved among CONSECUTIVE filtered empties distinct — applied to the top-level body and, via the `visit_statements` override, every nested statement list), while an `EmptyStatement` in a REQUIRED child position (loop/if/with/labeled body) stays signed via `stmt_sig` (behavior-bearing) and is never filtered — all locked by discriminator tests. **DECORATORS are ENCODED (class-level via `class_sig`, per-member via `class_element_sig`, each through `decorators_sig` -> the paren-transparent `expr_sig`):** OXC parses stage-3 decorators under the comparator's `SourceType::mjs()` parse (verified empirically — `@dec class C {}` / `class C { @dec m() {} }` / `class C { @dec x = 1; }` parse with errors=0, so `conformance_sig` signs a decorated class structurally rather than refusing it), the Svelte runtime strips TS but NOT decorators, and the value-expression refusal set does NOT refuse decorators/classes — so a decorated class/member in a source-preserved `{@html}`/dynamic value is byte-copied to emitted client JS, where the decorator executes and can alter runtime behavior; the prior fail-closed claim was incorrect (decorators ARE parseable under mjs and ARE reachable). A redundant paren in a decorator argument is cosmetic-EQUAL while a different decorator expression/name/argument FAILs — locked by discriminator tests. **The module import/export family is an ORACLE family — NOW ENCODED, NOT a residual.** OFFICIAL Svelte client output CAN contain module-script imports/exports: the committed `matrix/module_import_export` golden carries a `clientModule` with `import {base} from "./base.js"; … export const VERSION = 1;`. The prior "module scripts fail closed" basis was therefore WRONG about the OFFICIAL output domain — the comparator is the gate's ORACLE and must compare ANY official output correctly, including import/export forms, even though native Verter currently REFUSES `<script module>` in this branch (`crates/verter_compiler/src/svelte/runtime/client_surface.rs`, pinned by the fail-closed tests). So these axes are encoded NOW: (1) `ImportDeclaration` `phase` + `with`-clause import attributes (`with_clause_sig` / `import_attribute_sig`); (2) the full `ExportNamedDeclaration` surface — inline declaration / specifier list (`export_specifier_sig`) / re-export source / export-kind / `with`-clause, including specifier-only `export { a as x } from "m"`; (3) `ExportAllDeclaration` (source / namespace rename / export-kind / `with`-clause); (4) the program `hashbang` (`program_sig`). The import/export module family is signed in full (`ImportDeclaration` source/kind/phase/with-clause/specifiers; `ExportNamedDeclaration` declaration/specifiers/source/export-kind/with-clause; `ExportAllDeclaration` exported/source/export-kind/with-clause) and covered by discriminator tests for the parseable-under-`SourceType::mjs()` axes (import-attribute key/value + `with`/`assert` keyword, export specifier local/exported + source + export-all source/namespace, hashbang). The `import type` / `export type` kind is TS-only syntax NOT parseable under the comparator's `SourceType::mjs()` parse (verified — both fail with a parse error), and the `import defer` phase ties to the namespace-import form, so those are signed DEFENSIVELY (fields encoded, no planted positive) — not a reachable gap for the emitted-mjs oracle. Because native still refuses `<script module>`, `module_import_export` is NOT yet a gated `SUPPORTED_MATRIX` slug — it enters the gate only when native module-script emission opens; until then the oracle stays complete without the gate row (adding it now would fail the fail-closed tests). **Remaining structural gaps (honest residual):** genuinely TS/module-only declaration/statement forms not parseable under `SourceType::mjs()` (e.g. `TSTypeAliasDeclaration`, `TSInterfaceDeclaration`, `TSEnumDeclaration`, `TSModuleDeclaration`, `TSImportEqualsDeclaration`, `TSExportAssignment`, including the TS-only inner declarations reached via the `ExportNamedDeclaration` declaration path such as `export type X = …`) still collapse to the discriminant-only `Stmt(discriminant)` / `Decl(discriminant)` fallback — restricted to TS/module-only forms, NOT ordinary control-flow and NOT the import/export family. The expression `other =>` fallback is likewise an explicitly-classified conservative fallback over only the TS-only wrappers / JSX / V8-intrinsic forms — none parseable under `SourceType::mjs()` — NOT a generic remaining-kind catch-all. **Semantic-comment occurrence-path descent is not part of this structural debt:** the anchor is deterministic and collision-resistant over the NORMALIZED comparator view (`CommentAnchorIndex` indexes statements by the same empty-filtered logical view as `statements_sig`, node-types the segments, and gives comments attached to normalized-away empty statements an explicit synthetic `empty_gap[<logical>.<empty_ordinal>]` anchor); `CommentAnchorIndex` walks top-level `Program.directives` and `program.body`, and the generic OXC visitor descends reachable child nodes including `FunctionBody.directives` and every nested statement list (the `visit_statements` override applies the same empty-filtered normalization there). The CONSISTENCY INVARIANT: the comment-anchor occurrence path MUST be computed over the same normalized AST view as the structural signature; any structural normalization (empty-statement filtering, paren transparency, future list rewrites) must be mirrored in anchor indexing OR introduce a synthetic carrier for comments attached to removed nodes. A future anchor collapse inside that normalized view is a comparator bug, not D-17 debt. **Basis:** the comparator is the gate's ORACLE, so it must compare ANY official output correctly — it is NOT scoped to what native Verter currently emits, which is exactly why the module import/export family is encoded ahead of native module-script emission. The only remaining discriminant-only collapse is the TS/module-only declaration/statement set that does not parse under `SourceType::mjs()`; if a source-preserved construct outside that set reached emitted client output and the comparator dropped its axis, two bodies that differ only below the gap would compare equal — a silent structural false-PASS. **Contract:** the owner change that makes any listed residual family accepted-positive must either prove it is refused with explicit fail-closed tests, OR encode that family structurally in the same change, reusing `program_sig` / `expr_sig` / `params_sig` / `binding_sig` / `decl_var_sig` / `statements_sig` / `function_body_sig` and adding discriminator tests for each newly covered family. **FAIL-CLOSED FALLBACK CONTRACT (F3 forward-hardening):** the `expr_sig` `other =>`, `stmt_sig` `other =>`, and `decl_sig` `other =>` wildcard arms FAIL OPEN today — `stmt_sig` / `decl_sig` collapse the residual to a discriminant-only signature (`Stmt(discriminant)` / `Decl(discriminant)`) and `expr_sig` collapses it to a span-stripped Debug string (`Other(strip_debug_noise(Debug))`); both shapes are lossy fail-open fallbacks (NOT explicit structural encoding and NOT a fail-closed refusal). This is correct ONLY because that residual set is exactly the TS-only / JSX / V8-intrinsic / TS-module forms that do NOT parse under the comparator's `SourceType::mjs()` parse (so no ordinary-JS node currently reaches them and there is no current false-PASS). This is forward-fragile: a FUTURE `oxc` upgrade that adds a new ORDINARY-JS (mjs-parseable) expression / statement / declaration node would let that node silently fall through to its lossy fallback (a discriminant-only `Stmt`/`Decl` signature, or a span/noise-stripped `Other` Debug string whose structural completeness is not contractual) — a latent structural false-PASS. The owner of any `oxc` bump that introduces a new mjs-parseable node form in those families MUST make these fallback arms FAIL CLOSED for it (panic on, or route through an explicit residual-use side channel that fails, any `SourceType::mjs()`-parseable conformance input that reaches `Other(` / `Stmt(` / `Decl(` for an ordinary-JS node) rather than fail open — encode the new family structurally in the same change OR fail-closed-assert it, never leave it collapsing to a lossy fallback. This is the MECHANICAL belt-and-suspenders enforcement of D-17's existing encode-or-fail-closed contract for the wildcard arms specifically. |
| D-19 | **SVELTE CLIENT SEMANTIC-COMMENT GOLDEN ORACLE GAP (narrowed; owner: the Svelte golden-pipeline owner — the next change that changes the Svelte golden schema/generator, AND a hard blocker for the first official-positive semantic-comment fixture).** Cite: conformance-rule / Svelte-client-emit-conformance-comparator block codex-DEFER ruling (2026-06-25). The raw Svelte client comparator now ENFORCES in-contract semantic comments on raw module pairs (`conformance_sig`/`comment_sig` + the discrimination guard `svelte_structural_conformance_discriminates_cosmetic_from_behavioral_diffs`), but committed client goldens serialize `clientModule` through `normalizeModuleForComparison`, which DROPS every JS comment. Therefore the supported-fixture and codegen fixture gates cannot yet prove official-positive semantic-comment preservation (an official semantic comment Verter drops compares EQUAL because the golden side was stripped at generation). This is a GOLDEN-DATA ORACLE gap — NOT a comparator-logic gap, and NOT an unreachable-surface claim (semantic comments ARE reachable in accepted author output; the comparator catches them on raw inputs today). **Done when:** (1) hand-vendored AND codegen client goldens carry an official semantic-comment oracle DERIVED FROM THE RAW PINNED SVELTE OUTPUT by the SAME Rust extractor used by `conformance_sig`/`comment_sig` (NO duplicated JS classifier); (2) `emitted_client_topology_matches_official_goldens` AND `emitted_codegen_corpus_matches_official_goldens` compare Verter's RAW emitted semantic-comment signature against that stored official signature; (3) `svelte_goldens_in_sync` rejects a missing/stale field (`deny_unknown_fields`, not optional/default-empty); (4) ≥1 checked-in OFFICIAL-POSITIVE semantic-comment fixture proves non-vacuity by FAILING if the official semantic-comment oracle is empty. **Distinct from D-17:** D-19 is a golden-DATA oracle gap (the comparator enforces semantic comments on raw inputs, but the committed goldens were comment-stripped at generation); D-17 is a structural-comparator completeness gap (the listed structural-comparator gaps). NEITHER is an unreachable-surface claim. D-19 ALSO covers a regex-literal blind spot in `scripts/svelte-golden-lib.mjs::normalizeModuleForComparison` (and its Rust mirror `normalize_module_for_comparison`): the golden-generation normalizer is NOT a JavaScript lexer and may rewrite whitespace or `//` inside code-position regex literals. As of the audited corpus, NO committed Svelte client golden contains a real code-position regex literal, so this is NOT a current conformance false-pass (the comparator-side `RegExpLiteral.raw` axis is correct and in-contract); the invariant is PINNED by `committed_client_goldens_carry_no_code_position_regex_literal` in `svelte_goldens_in_sync.rs`. If official Svelte client output starts emitting code-position regex literals, that guard FAILS — replace the normalizer with a lexer-backed implementation (or stop normalizing code-bearing bytes) before accepting those goldens. |
| D-20 | **COMPARATOR / GUARD HARDENING DEBT (codex-DEFER — two net-new deferrals, NOT an "unreachable" claim).** Two polish items the conformance-comparator REOPEN deliberately deferred per a codex-architect ruling. NEITHER is a reachability claim — both are forward-looking robustness/maintenance items recorded so they are not silently dropped. **B1 — typed `ConformanceSig` (DEFER).** The Svelte emit comparator's signatures (`expr_sig` / `stmt_sig` / `import_specifier_sig` / `comment_sig` / `conformance_sig` in `crates/verter_compiler/tests/svelte_client_emit_topology.rs`) are STRING-form. A typed AST/value representation would remove any theoretical delimiter/collision looseness. _codex DEFER rationale:_ the current string form is test-only and already discriminated by planted positive/negative asserts; a full ~20-fn rewrite is not the best current move; the real risk is future delimiter/collision looseness, not current perf. _Owner/trigger:_ the Svelte conformance comparator owner — trigger when adding the next broad expression/statement axis, sharing the comparator across backends, or observing signature allocation in test profiles. **B3 — Cargo.lock / AST dependency-deny for the re-printer guard (DEFER).** The `no_compiled_output_cosmetic_reprinter_path` guard (`crates/verter_session/tests/architecture_guards.rs`) scans comment-stripped source needles + path scoping + use-path detection + structural TOML manifest parsing; a build-graph (`Cargo.lock` / dependency-deny) layer was considered as a harder gate. _codex DEFER rationale:_ a build-graph deny adds high-maintenance allowlists + role-ambiguous false positives, and `Cargo.lock` does not solve intent/reachability; the existing guard already combines the four discriminating layers. _Owner/trigger:_ the architecture-guard owner — trigger only after a concrete bypass appears or a shared Rust-AST scan substrate can prove lower false-positive cost. **Note on B2 (per-backend positive structural-discriminator coverage):** NOT tracked here — it is already recorded by the existing `Compiled-Output Conformance (CRITICAL)` rule (the CLAUDE.md "positive structural-discriminator guard currently covers Svelte client only" line + the `/compiler-codegen` SKILL.md "Tracked guard gap" note), per the codex ruling that the existing rule records that gap correctly. |
| D-21 | **BINDING HOST-OWNERSHIP REALIGNMENT (FEED-FORWARD, owner: Block 5f-a (component-host binds) + Block 5f-b (special-element binds); codex-DEFER ruling recorded — adjudication 2026-06-26, sub-split with the 5f-a/5f-b/5f-c partition).** A Svelte client binding is owned by the node that HOSTS it, not by the `$.bind_*` helper-name family alone. Block 5c owns ordinary DOM-element bindings on hosts emitted by the intrinsic DOM emitter (adding the `textarea`/`select`/`option` finite-allowlist rows + goldens, which fail closed today), PLUS the shared bind-operator metadata/getter-setter substrate (the IDE-only `svelte/ide/bind_contract.rs` promoted to a shared `svelte/bind_contract` consumed by IDE + runtime). Component-host bindings (component `bind:this`, component `bind:prop`, component function bindings `bind:x={get,set}`) are FEED-FORWARDED to Block **5f-a**, and special-element bindings (`<svelte:window>` size/scroll/online, `<svelte:body>` `bind:this`/element-size, `<svelte:document>` `bind:this`/activeElement/property) to Block **5f-b**, because `IrNode::Component` and renderable `IrNode::Special` fail closed before those blocks and a real component bind also needs the import hoisting; emitting these in 5c would require opening or stubbing the host emitter — a throwaway dual path the plan forbids. The plan previously DOUBLE-ASSIGNED these rows to both 5c and 5f; this row dedups them. 5c MUST add/retain explicit fail-closed tests for the feed-forwarded component + special-element binding surfaces until 5f-a / 5f-b open the hosts. **Done when (5f-a — component binds, LANDED):** Block 5f-a has pinned `svelte@5.56.3` accepted-positive goldens for (a) component `bind:this`, (b) component `bind:prop`, and (c) component function bindings — all consuming 5c's shared bind-operator substrate — AND the corresponding 5c fail-closed refusals for those surfaces are DELETED in the SAME change that lands the goldens (done in 5f-a: `component_bind_this`/`component_bind_prop`/`component_bind_function` goldens + the converted `component_bind_*_fails_closed` tests). **Done when (5f-b — special-element binds, DEFERRED):** Block 5f-b has pinned goldens for (d) supported `<svelte:window>` size/scroll/online binds (`$.bind_online` for `bind:online`), (e) supported `<svelte:body>` binds, and (f) supported `<svelte:document>` binds — PLUS negative coverage for official-invalid host/name pairs (e.g. `<svelte:body bind:scrollX>`) — AND the corresponding 5c fail-closed refusals are DELETED at 5f-b in the SAME change (no coverage gap). |
| D-22 | **TEXTAREA DYNAMIC-CONTENT CHANNEL (DEFER-NEW, owner: a later content-model block).** A `<textarea bind:value={v}>{expr}</textarea>` with DYNAMIC / interpolation content is deferred. Official emits `$.set_value(textarea, expr)` BEFORE the bind — a textarea CONTENT channel distinct from the static-fallback child (which 5c already handles via the `$.remove_textarea_child` prelude, since a baked static-text fallback is cleared at runtime regardless of the bind). 5c admits the static-text fallback form ONLY; a dynamic textarea content child stays fail-closed at the special-content-model gate (`Element { tag: "textarea" }`). Modelling the `$.set_value` content channel (and the reactive `$.template_effect` variant when the content reads a signal) is the content-model layer's job, not the bindings-breadth vertical's. **Done when:** pinned `svelte@5.56.3` goldens for dynamic textarea content (`$.set_value(textarea, …)`, plus the reactive variant) land in that block, AND the 5c dynamic-textarea-content refusal (the negative `textarea_bind_value_with_dynamic_content_child_still_fails_closed`) is DELETED in the SAME change (no coverage gap). |
| D-23 | **STATIC `<optgroup>` / SELECT STATIC CONTENT-MODEL (DEFER-NEW, owner: a later select content-model block).** A `<select bind:value><optgroup>…</optgroup></select>` static `<optgroup>` (and any other static select content-model NOT involving option `value` / `__value`, which is the existing 5a option-value-channel deferral) is deferred. 5c's select host accepts only STATIC `<option>` children (and insignificant whitespace); a static `<optgroup>` (a nested grouping element) is not in 5c's finite select-interior allowlist and stays fail-closed at the special-content-model gate. The static optgroup/select content-model (its skeleton serialization + any reset semantics) is owned by a dedicated select content-model layer. **Done when:** pinned `svelte@5.56.3` goldens for the static `<optgroup>` / select static content-model land in that block, AND the corresponding 5c select-interior refusal is DELETED in the SAME change (no coverage gap). (NOT to be confused with option `value` / `__value` reactive tracking — that is the pre-existing 5a option-value-channel deferral.) |
| D-24 | **MISMATCHED DEFAULT-ATTR + BIND CO-LOCATION (DEFER-NEW, conservative never-wrong narrowing).** A static `defaultValue` / `defaultChecked` co-located with a MISMATCHED two-way bind — `defaultChecked` + `bind:value`, or `defaultValue` + `bind:checked` — is conservatively REFUSED in 5c. Official ACCEPTS the mismatched form (verified `svelte@5.56.3`: `<input defaultChecked bind:value={v}>` emits `input.defaultChecked = true;` + `$.bind_value(...)`; `<input type="checkbox" defaultValue="x" bind:checked={c}>` emits `input.defaultValue = 'x';` + `$.bind_checked(...)` — the default-property write THEN the bind). 5c's `default_attr_has_matching_bind` (`bind_target_names.rs`) pairs `defaultValue`↔`bind:value` and `defaultChecked`↔`bind:checked` ONLY; a mismatched default+bind returns `false` and stays fail-closed at the static-attr allowlist (default-deny — 5c NEVER emits wrong output, it only under-accepts a co-location it has not modelled). **Done when:** the matching-AND-mismatching default-attr/bind co-location is UNIFIED (both pairings emit the official default-property write + the bind), AND the conservative refusal test (`default_checked_with_mismatched_bind_value_fails_closed`) is DELETED in the SAME change (no coverage gap). |
| D-25 | **OUT-OF-CHARTER ORDINARY DOM ELEMENT BINDINGS (DEFER-NEW, owner: Svelte DOM-binding completion owner; hard trigger: before declaring the Svelte client runtime feature-complete / before RC).** Block 5c's ordinary DOM binding charter is exact: textarea/select value, checked, group, media currentTime/paused/duration/played, element-size clientWidth/clientHeight/offsetWidth/offsetHeight, contenteditable innerHTML/innerText/textContent, and details open. Official svelte@5.56.3 also accepts ordinary DOM binds intentionally not emitted in 5c: files, focused, playbackRate, volume, muted, contentRect/contentBoxSize/borderBoxSize/devicePixelContentBoxSize, indeterminate, buffered/seekable/seeking/ended/readyState, and naturalWidth/naturalHeight/videoWidth/videoHeight. These rows must remain explicit shared bind-contract rows with official helper identity preserved and `RuntimeSupport::Unsupported` until owned; absent-row fail-closed and helper-identity erasure are both unacceptable. **Done when:** the owning block flips each row to `RuntimeSupport::Supported` with its official helper/arity/event, adds pinned goldens + behavioral coverage + IDE contract coverage, and deletes the corresponding 5c fail-closed tests in the same change. |
| D-26 | **PLAIN-.svelte TEMPLATE-EXPRESSION JS-GRAMMAR / TS-REJECTION DIAGNOSTIC PARITY (DEFER-NEW, owner: Svelte runtime expression-front-end / parser-parity owner; hard trigger: the first block that centralizes template-expression parsing or broadens accepted template value/expression surfaces, and in any case before declaring Svelte client runtime diagnostic conformance / feature-complete / RC).** Official svelte@5.56.3 parses template/directive/tag expressions in a plain .svelte file as plain JS before directive/bind analysis; TS-only syntax (`as`, `!`, typed params, generic arrows, etc.) must therefore be a source-ordered official parse reject (`expected_token`/`js_parse_error`) across interpolation, events, dynamic/mixed attributes, class/style/html/render, and every bind expression. Today Verter's runtime expression front-end uses TSX+strip for most template expressions and bind uses a scoped mjs function-pair lane; some TS-in-plain forms fail closed via unsupported channels (`Binding`, `ComplexInterpolation`, `NonDelegatedEvent`, `svelte-runtime-expr-parse`) and some non-bind value positions are still accepted/stripped. The bind-target LVALUE nested-TS surface (a TS-only operator ANYWHERE in an accepted `bind:` lvalue spine — `o!.x` / `a[x as T]` / `a[x!]`, and the root `name!` / `name as T`) now FAILS CLOSED STRUCTURALLY via the `BindTargetFact.lvalue_contains_ts` fact (the `Binding` channel), so the bind lvalue is NO LONGER accept-and-stripped; only its exact diagnostic-CODE parity (`expected_token`/`js_parse_error` vs the structural fail-closed) remains deferred here, owned by the shared template-expression parse authority (NOT a bind-only TS code gate). The BARE-INSTANTIATION ambiguity is part of THIS same lvalue surface (round-6 G1 re-ruling, 2026-06-28): a `f<T>` instantiation ROOT and a `arr[g<T>]` instantiation INDEX (each an OXC `TSInstantiationExpression` with NO trailing call) ALSO fail closed today via the SAME `BindTargetFact.lvalue_contains_ts` fact, because svelte@5.56.3 PARSE-REJECTS both as plain JS (`js_parse_error`). SYMMETRICALLY, the call / new / tagged-template form CARRYING TYPE ARGUMENTS (`arr[g<a,b>(c)]` — a `CallExpression`; `new C<T>()` — a `NewExpression`; or a type-argument tagged template — each NOT a bare instantiation) ALSO FAILS CLOSED today via the SAME `BindTargetFact.lvalue_contains_ts` fact (the shared `expression_contains_non_plain_svelte_js` / `StrictOfficialDeltaScan` detector), because the TSX+strip lane would otherwise silently strip the type-arguments and emit a DIVERGENT index (`arr[g<a,b>(c)]` → `arr[g(c)]`), whereas official `svelte@5.56.3` parses the source as plain JS and emits the relational/comma `arr[(g < a, b > c)]` (confirm bar-4 codex F1 ruling, 2026-06-28). The bare-instantiation AND the call/new/tagged-template-type-argument fail-close are both supplied by the shared `expression_contains_non_plain_svelte_js` / `StrictOfficialDeltaScan` detector — a wholesale, default-closed, non-plain-Svelte-JS refusal shared by BOTH bind lanes (the single-lvalue spine and the function-pair element scan) — RETAINED until the shared plain-MJS authority below subsumes it: any TS / non-plain-ECMAScript node anywhere in an accepted bind lvalue fails closed, so the `arr[g<T>]` / nested-TS accept-and-strip fail-open the TSX+strip rewriter would otherwise produce cannot occur. When the shared plain-MJS template-expression authority lands it must ACCEPT the call / new / tagged-template-with-type-arguments form emitting the CORRECT relational/comma form (`arr[g<a,b>(c)]` → `arr[(g < a, b > c)]`), REJECT the bare-instantiation form `f<T>` / `arr[g<T>]` with the exact official `js_parse_error`, and thereby make the bind-specific `expression_contains_non_plain_svelte_js` / `StrictOfficialDeltaScan` detector REDUNDANT. Do not add bind-only exact-code gates. Implement one language-aware expression parse authority that (1) parses plain .svelte expressions with `SourceType::mjs()` before semantic/shape classification, (2) records source-order parse defects with exact pinned official code and arbitrates against existing parser facts, (3) reuses the parsed fact for analysis/rewrite to avoid double parsing/perf regression, (4) keeps `lang="ts"` refusal behavior until the TS block opens, then switches that file-language to TS parsing/strip by design, and (5) adds oracle-pinned negative matrix rows for TS-in-plain across interpolation, event, dynamic/mixed attributes, `{@html}`/tag expressions, class/style directives, and bind lvalue/function-pair/group. **Done when:** no plain-.svelte TS construct is accepted or code-less refused outside audited exotic exceptions, exact official code/order matches `svelte@5.56.3`, and bind-specific TS tests are deleted or reclassified to the shared parse-parity matrix. |
| D-27 | **STANDALONE / INDEPENDENT FORM-DEFAULT PROPERTY ATTRIBUTES (DEFER-NEW, owner: out-of-charter form-default property-attribute emission owner; hard trigger: the block that opens non-bind static/dynamic property-attribute emission, and in any case before declaring the Svelte client runtime feature-complete / RC). Cite: round-5 codex scope ruling (2026-06-27).** A STANDALONE `<input defaultValue="x">` / `<input defaultChecked>` (a `defaultValue` / `defaultChecked` attribute with NO co-located bind) is OUT of block 5c's ordinary-DOM `bind:*` charter: official svelte@5.56.3 ACCEPTS it and emits a PROPERTY write (`input.defaultValue = 'x'` / `input.defaultChecked = true`), which is static/non-static PROPERTY-ATTRIBUTE emission, NOT a two-way binding. 5c accepts a static `defaultValue`/`defaultChecked` ONLY when co-located with its MATCHING bind (`default_attr_has_matching_bind`, `client_surface.rs`); a STANDALONE default has no matching bind and stays fail-closed at the static-attr allowlist (`DynamicAttribute` — default-deny, 5c NEVER emits wrong output, it only under-accepts an out-of-charter property attr). The MISMATCHED default+bind co-location is the related D-24 conservative narrowing; this row + D-24 together are the full independent-form-default-property surface. 5c RETAINS explicit fail-closed tests for the standalone forms (`standalone_default_value_without_bind_still_fails_closed`, `standalone_default_checked_without_bind_still_fails_closed`) and the mismatched form (`default_checked_with_mismatched_bind_value_fails_closed`, D-24). **Done when:** standalone AND mismatched (unified with D-24) `defaultValue`/`defaultChecked` emit the official property writes, suppress `remove_input_defaults` where official does, and the corresponding 5c fail-closed tests are DELETED in the SAME change that lands the accepted-positive behavior (no coverage gap). |
| D-28 | **`bind:group` ACCUMULATOR KEY = RESOLVED-BINDING IDENTITY (DEFER-NEW, owner: the block that opens each / control-flow-scoped `bind:group` binds; not a current defect). Cite: round-5 codex scope ruling (2026-06-27).** Block 5c's `GroupBindKey` is `(ScopeId, keypath)` — the structural target keypath plus the lexical scope id — which correctly separates two DISTINCT top-level targets and shares one `binding_group` accumulator for the SAME target in the SAME scope (the only group surface 5c opens; each / control-flow-scoped binds are themselves fail-closed in 5c, so the exposing scopes do not exist yet). Official grouping is effectively `(keypath, resolved binding objects)` — the RESOLVED binding identity, not the lexical scope id. This is NOT a current defect: the `(ScopeId, keypath)` vs `(keypath, resolved binding)` divergence is only OBSERVABLE once each/control-flow-scoped group binds expose the SAME keypath spelling across DISTINCT resolved binding objects (e.g. `{#each items as item}<input bind:group={item.sel}>`), which 5c does not open. **Done when:** `GroupBindKey` replaces `(ScopeId, keypath)` with the official-equivalent RESOLVED binding identity, with tests proving the SAME keypath spelling in DISTINCT each/control-flow binding objects does NOT share an accumulator, AND the same resolved target still does. |
| D-29 | **BIND NAME / HOST / HOST-ATTRIBUTE DIAGNOSTIC EXACT-CODE PARITY (DEFER-NEW, owner: the shared bind-validation / template-expression diagnostic authority; hard trigger: the block that centralizes bind name/host/host-attribute diagnostics, and in any case before declaring the Svelte client runtime diagnostic conformance / feature-complete / RC). Cite: round-6 codex-DEFER scope ruling (2026-06-27).** Block 5c's official-reject gate runs ONE document/attribute-order bind-validation pass (`official_reject.rs`) that establishes bind NAME / HOST / host-ATTRIBUTE validity FIRST and runs the bind-target SHAPE scans (group-policy → parens → invalid-expression) ONLY for binds official carries to expression validation — an INTRINSIC host with a valid name + host + host-attrs (via the shared `bind_contract` routing + the `host_attr_gate` authority), OR any non-intrinsic (component / special) host, which official validates straight to expression shape. A bind that is name/host/host-attribute INVALID on an intrinsic host — official svelte@5.56.3 rejects it with a NAME / HOST / host-ATTR code BEFORE expression validation (`bind_invalid_name` for `<div bind:foo>`, `bind_invalid_target` for `<div bind:value>`, `attribute_contenteditable_missing` / `attribute_contenteditable_dynamic` for the contenteditable binds, `attribute_invalid_multiple` for a dynamic `<select multiple>`, `attribute_invalid_type` for an invalid `<input type>`) — FAILS CLOSED through the EXISTING unsupported channel (`UnsupportedSvelteRuntimeSurface::Binding`) rather than minting a confidently-WRONG shape `OfficialRejection` code. This is fail-CLOSED (no fail-open); only the EXACT name/host/host-attribute diagnostic code + ordering is deferred. **Done when:** the bind name/host/host-attribute diagnostics — `bind_invalid_name`, `bind_invalid_target`, `attribute_contenteditable_missing`, `attribute_contenteditable_dynamic`, `attribute_invalid_multiple`, and `attribute_invalid_type` — match Svelte's exact code AND ordering through the one shared ordered bind-validation pass, AND the corresponding 5c unsupported-channel fail-closed tests for those name/host/attr cases are converted to exact-code reject tests in the SAME change (no coverage gap). 5c RETAINS the unsupported-channel fail-closed tests for those forms until then. Also covers cross-category bind diagnostic ordering (round-7 codex scope ruling, 2026-06-28): global-reference analysis (`global_reference_invalid`) must preempt bind-shape codes wherever pinned Svelte does; official-supported but runtime-unsupported D-25 ordinary DOM rows must participate in exact target-shape diagnostics without becoming emittable; and D-21 component/special hosts must apply bind-name target policies such as `bind:group` sequence rejection with official code/order when their host validation reaches bind analysis. |
| D-30 | **SVELTE CLIENT BIND/EVENT/GROUP LOOKUP INDEXING (DEFER-NEW, owner: Svelte client plan/emitter performance owner). Cite: round-7 codex scope ruling (2026-06-28).** Current plan/emitter paths retain source-order Vecs and perform repeated `.iter().find` lookups for bind shapes, event shapes, static group values, and dynamic group values. This is correct and fail-closed today, but worst-case scales as template nodes/ops × metadata count. **Done when:** `ClientModulePlan` or the classified surface builds source-order-preserving indexes such as `(NodeId, bind_name) -> ClientBindShape`, event lookup by node/event identity, and `NodeId -> group value`/`GroupDynamicValue`; emission uses O(1) lookups while retaining Vecs only where order is semantically required, with small-template no-regression and large-form benchmark coverage. |
| D-31 | **NON-PRIMITIVE `$state` PROXY DECLARATION LOWERING (DEFER-EXISTING, owner: the §3.2 `$state`/`$.proxy` classification owner — Block 4 per this plan; note that current source comments label this "runes-completion block (5g)", so that source-vs-plan ownership contradiction must be reconciled by the proxy-lowering owner, not by 5c). Cite: codex-architect scope decider ruling (2026-06-28).** Verter currently fails closed for the whole object/array/non-primitive `$state(...)` declaration class at `state_decl_shape`; this gate is load-bearing because the accepted declaration item/emitter path is primitive-`$state` only, and removing it would route `$state({ ... })` / `$state([])` into semantically wrong `$.state(init)` emission without `$.proxy`. This debt covers the shared declaration-lowering path for `BareProxy` and `StateProxy`: supported-item variant(s), proxied init emission, non-literal init rewrite through the shared expression rewriter, end-to-end member read/write/reassign emission, helper/topology accounting, and conformance/behavioral coverage. **Done when:** the class-wide non-primitive `state_decl_shape` refusal is replaced by shared proxy declaration lowering; object/array/proxiable cases match pinned `svelte@5.56.3` for `let x = $.proxy(init)` and `let x = $.state($.proxy(init))`; the object-state member-bind and array-state `<select multiple>` fail-closed tests (`bind_value_object_state_member_fails_closed_at_the_object_state_decl_gate`, `bind_select_value_with_array_state_fails_closed_at_the_array_state_decl_gate`) are converted to positive tests in the SAME change (no coverage gap); and the source comments and plan ownership labels agree. |
| D-32 | **BIND FUNCTION-PAIR ELEMENT STRUCTURED FACTS (DEFER-NEW, owner: the Svelte bind-target typed-fact / function-pair analysis owner; hard trigger: the block that centralizes bind-target typed facts or the shared plain-MJS template-expression authority, and in any case before declaring the Svelte client runtime feature-complete / RC). Cite: codex-architect scope ruling (2026-06-28).** `collect_bind_function_pair_names` (`bind_target_names.rs`) admits a named top-level function declaration referenced by a function-pair `bind:x={get,set}` by reading each pair-element SOURCE slice and running `src.trim()` + `is_plain_identifier(...)` on the source string, rather than reading a parsed-AST fact — a typed-IR-cleanliness defect (the file otherwise relies on the parsed bind fact, not a raw-source scan). This is fail-CLOSED / correct today (it only admits a bare-identifier element) and PRE-EXISTING (introduced by the original 5c land, NOT by the F1/F2 fix; no fix commit touched the file); only the source-string shape check is deferred. **Done when:** `parse_plain_svelte_function_pair` returns structured per-element facts including `source` and `named_identifier: Option<String>` derived from the parsed `SourceType::mjs()` AST; `BindTargetFact` stores that structured pair; `collect_bind_function_pair_names` reads only `named_identifier`; the string `is_plain_identifier` path is removed; and tests cover bare identifiers, parenthesized / non-identifier elements, escaped or non-ASCII valid identifiers, and no extra reparses. |
| D-33 | **AUDITED / HELPER COMPILE CARRIER MIGRATION (DEFER-NEW, owner: the host-session carrier compile/audit owner; hard trigger: the first change that makes the audited/helper compile path support a non-Vue carrier, OR that touches `compile_with_audit_options()` or any audit/helper compile caller, AND in any case before plan close-out). Cite: codex-architect DEFER ruling (2026-06-28).** The audited compile entry `crates/verter_session/src/host_compile_audit.rs::compile_with_audit_options()` still drives the hardcoded Vue SFC compiler (`verter_compiler::compile::compile`) directly — a second Vue runtime-compile path outside the `CarrierCompilerRegistry`. Block-1 STEP-0 explicitly deferred migrating the audit/helper compile callers. The interim is FAIL-CLOSED, not silent-wrong: `compile_with_audit_options` classifies the file and returns a typed `VerterE001` diagnostic for a non-Vue framework carrier (`host_compile_audit.rs:185` — `if language.is_framework_carrier() && !language.is_vue()` — through `:206` `VerterE001`) rather than silently Vue-compiling a `.svelte` file; pinned by guard `crates/verter_session/tests/g_compile/compile_audit_vue_only_guard.rs` (`compile_with_audit_rejects_a_non_vue_carrier_with_a_typed_error`, plus the positive `compile_with_audit_still_compiles_a_vue_carrier`). **Done when:** `compile_with_audit_options` (and any other audit/helper compile caller) routes through `CarrierCompilerRegistry::compile_bundle` so the audited path compiles every registered carrier with per-carrier output; the `TODO(follow-up)` at the `host_compile_audit.rs` callsite is removed; and the fail-closed `compile_with_audit_rejects_a_non_vue_carrier_with_a_typed_error` test is converted to a POSITIVE (an audited `.svelte` compile succeeds through the registry) in the SAME change. The §11 "audit / helper compile callers → carrier registry" carry-forward bullet points to this row. |
| D-34 | **EDITOR SYNTAX HIGHLIGHTING FOR NON-VUE FRAMEWORK FILE EXTENSIONS (DEFER-NEW, cross-framework, owner: the shared VS Code extension / framework-language-contribution owner — keyed by the adapter registry, never a Svelte fork; hard trigger: any TextMate-grammar / `contributes.grammars` / language-contribution work, any Svelte IDE/LSP parity claim, AND in any case before declaring the Svelte IDE/LSP experience feature-complete / RC). Cite: codex-architect DEFER ruling (2026-06-28).** The VS Code extension (`packages/vue-vscode`) ALREADY ships a `.vue` TextMate grammar (`source.vue` / `syntaxes/vue-generated.json` in `packages/vue-vscode/package.json`, with embedded-language injection for `<script>` TS/JS, `<style>` CSS, and template regions), so `.vue` file CONTENT is highlighted — Vue is the parity model. `.svelte` (and any future non-Vue adapter file extension) has NO bundled grammar — its content renders uncoloured (a `.svelte` user relies on a third-party Svelte extension); the absence of a `source.svelte` grammar is pinned by `packages/vue-vscode/src/packageManifestFramework.spec.ts`. This is editor token colouring, NOT file-tree icons. **Done when:** the shared extension ships the grammar(s) / embedded-language injection for each non-Vue adapter (keyed by the adapter registry) so `.svelte` (and future adapter) files are highlighted like `.vue` already is (and like the official Svelte extension), with a regression test (a manifest/tokenization assertion or an e2e colouring check) proving `.svelte` content is highlighted. |
| D-35 | **REACTIVE-TEXT / STATIC-INTERPOLATION COMPLETION (DEFER-EXISTING, owner: Block 4 reactive-text/interpolation owner; hard trigger: any change broadening interpolation acceptance, any removal of the complex/static interpolation fail-closed gate, AND in any case before declaring the Svelte client runtime feature-complete / RC). Cite: codex decider ruling (2026-06-29).** Verter currently accepts only bare signal / no-default-prop identifier interpolation and fails closed for member, optional-member, call, binary, conditional, and static interpolation shapes. This is a GLOBAL reactive-text boundary (the pre-existing `classify_interpolation_shape` gate), NOT a block-body boundary; it applies IDENTICALLY at top level and inside 5e blocks — a block body with member / complex-expression / static reactive text (`{#each items as item}<p>{item.x}</p>{/each}`, `got {item.x}`) is refused, not mis-emitted, exactly as the same shape is at top level. Simple mixed text — simple-ASCII literal chunks plus accepted bare-signal / no-default-prop interpolations (`Hi {name}!`, `a {c} b`, `got {value}`) — IS already supported in element-hosted / block-body text-run placements (inside an element or a 5e block body), emitting the official `` `…${$.get(x) ?? ''}…` `` mixed-run form; what fails closed is a COMPLEX interpolation EXPRESSION (member/optional-member/call/binary/conditional) or a STATIC/non-reactive read, whether standalone or as one chunk of an otherwise-simple mixed run. ONLY a component root whose WHOLE cleaned root sequence is a single text run, OR an empty / comment-only root, is refused by the root-region gate as `RootTextRegion` (`refuse_unsupported_root_region`); a root text run alongside a sibling element or block is a `from_html` fragment and IS supported. This naked-root refusal is a SEPARATE surface, not part of D-35. The 5e-local each/await SIGNAL contract (§3.3) is fully satisfied independently of this. **Done when:** the shared `build_template_chunk`-equivalent evaluator lowers member / complex-expression / static / live text globally; static interpolation folds to the official `textContent`/`nodeValue` topology; D-14 governs the constant-chunk fold/refuse/live-fallback decisions; top-level AND block-body fixtures such as `{item.x}` / `got {item.x}` match pinned `svelte@5.56.3`; and the complex/static interpolation fail-closed tests/comments are converted or deleted in the SAME change. |
| D-36 | **NESTED DECLARATIONTAG ELEMENT-SCOPE CODEGEN (DEFER-EXISTING, owner: Svelte client nested element-scope / per-block reactive-partitioning owner; hard trigger: any change broadening `{const}`/`{let}` DeclarationTag placement beyond region roots, any removal or weakening of the nested DeclarationTag fail-closed gate, any change that emits `BlockStatement`-scoped template effects for nested non-rendering template tags, AND in any case before declaring the Svelte client runtime feature-complete / RC). Cite: codex DECIDER ruling (2026-06-29).** Verter currently matches pinned `svelte@5.56.3` for region-root `{const}`/`{let}` DeclarationTags, block-body-root `{@const}` (plus `{@const}` placement validation — a component-root `{@const}` is rejected), but fails closed for nested `{const}`/`{let}` inside elements with `svelte-runtime-unsupported-block`. Official `svelte@5.56.3` accepts nested DeclarationTags by wrapping the affected element child-walk fragment in a real JavaScript `BlockStatement`, emitting the declaration inside that block, signal-rewriting the initializer in the element's lexical scope, and placing the relevant `$.template_effect` / positional `{@debug}` effects inside that block. This is a LOCAL element-scope reactive-partitioning/codegen axis, NOT ordinary 5e control-flow body lowering and NOT the global interpolation-breadth D-35 axis. A flat hoist, silent drop, or positional no-block shim is forbidden because it breaks lexical shadowing / same-name declarations and fails the AST-structural conformance gate. **Done when:** the client IR/plan represents element-local declaration scopes, including same-name sibling declarations, as ordered scoped fragments/effect groups; the DOM walk can open/close emitted `BlockStatement`s at official child-walk positions; runtime ops are partitioned by innermost declaration block rather than only by `TemplateScopeId`; `$.template_effect` emission is split per declaration block with memoization/deps local to each split; declaration initializers and child expressions use the correct lexical binding/rewrite rules; nested `{@debug}` before/inside/after nested declaration blocks lands in the official position; structural goldens/probes cover nested `let`/`const` in an element, same-name sibling declarations, nesting under each/if/await/key, reactive text/attr/class/style inside and outside the nested block, and debug interleaving; and the nested DeclarationTag fail-closed rows are converted or deleted in the SAME change. |
| D-37 | **UPPERCASE HTML `class` / `style` ATTRIBUTE-NAME CASE-NORMALIZATION (DEFER-NEW, owner: the general 5a class/style attribute-identity + parser directive-classification / static-attribute-serializer case-normalization follow-up — the same 5a/parser layers that own the existing `5a` `KNOWN_DIVERGENCES` rows, NOT the 5f special-region layer; hard trigger: any change to regular-element `class`/`style` attribute identity / emission routing, the static-attribute skeleton serializer, or the directive-prefix classifier, AND in any case before declaring the Svelte client runtime feature-complete / RC / plan close-out. Cite: codex-DEFER terminal disposition (`B-FIX5-CODEX-TERMINAL-OUT.txt` — "LAND once A/B/C tracked") + codex-DEFER mechanism confirm (`B-FIX5-CODEX-MECHANISM-OUT.txt` — ledger-as-home + non-matrix labeling, no generator/corpus regen required).** Three pre-existing GENERAL-path (5a attribute-identity / parser) divergences on EXTREME uppercase HTML `class`/`style` attribute-name edges remain after 5f-b converged the special-region (`<svelte:element>`) mixed-case static `class`/`style` routing: **(A)** a dynamic/mixed uppercase `<div CLASS={k}>` emits Verter `$.set_attribute(div, 'class', k)` where official emits `$.set_class(div, 1, k)`, and the co-located `<div CLASS={c} class:active={x}>` DOUBLE-EMITS a redundant `$.set_attribute` alongside the merged `$.set_class` (client-behavior-correct but cosmetically redundant, present pre-5f-b at `c0fc409ae` and HEAD, NOT a 5f-b regression); **(B)** a static-lone uppercase `<div CLASS="card">` (no directive) FAILS CLOSED where official ACCEPTS and bakes a lowercased `class="card"` into the static skeleton; **(C)** an uppercase directive prefix `<svelte:element STYLE:color={c}>` (and the regular-element equivalent) FAILS CLOSED as an unknown directive where official ACCEPTS and folds it as a GENERIC attribute named `'STYLE:color'`. These belong to the general 5a class/style attribute-identity layer and the parser directive/static-serializer gate, NOT the special-region layer, and are NOT convergeable by a naive `eq_ignore_ascii_case` route: official behavior is CASE-DEPENDENT beyond routing — uppercase `CLASS={k}` → `$.set_class(div, 1, k)` WITHOUT the lowercase path's `$.clsx(k)`, whereas lowercase `class={k}` → `$.set_class(div, 1, $.clsx(k))` WITH `$.clsx`, and uppercase static `CLASS="card"` bakes LOWERCASED — so a naive case-insensitive route would INJECT a WRONG `$.clsx` (a NEW divergence). (A) is fail-close-SAFE-plus-cosmetically-redundant (client-behavior-correct); (B) and (C) are fail-close-safe official-ACCEPTS / Verter-fail-CLOSES divergences and MUST NOT be described as official parity (reject-vs-accept, distinct from the `<svelte:head>` reject-parity). **Done when:** the uppercase `class`/`style` attribute-name family converges with the CORRECT case-dependent emission (uppercase `CLASS={k}` → `$.set_class(div, 1, k)` with NO `$.clsx`; uppercase static `CLASS="card"` → baked lowercased `class="card"`; uppercase `STYLE:color` → generic `'STYLE:color'` attribute fold), the (A) double-emit is removed, each lands its own oracle coverage, and the (B)/(C) fail-closed refusals (incl. `uppercase_style_directive_prefix_is_not_a_style_directive`) are converted to positive goldens in the SAME change; MUST close by plan close-out (empty debt ledger required). |
| D-38 | **COMPONENT `{@attach}` FORWARDING (DEFER-NEW, owner: the 5f-a component-call projection owner; hard trigger: any change to the component props-object member assembly (`project_component_call` / `ComponentMember`), any broadening of component-hosted attribute acceptance, AND in any case before declaring the Svelte client runtime feature-complete / RC).** Official `svelte@5.56.3` ACCEPTS `<Comp {@attach expr} />` and forwards the attachment as a COMPUTED-KEY prop on the component call — `Comp($$anchor, { [$.attachment()]: expr })` (the `$.attachment` / `createAttachmentKey` prop key, NO element helper). Verter 5f-c lowers the attribute-position `{@attach}` to the typed `AttrIr::Attach` on every host, and the component-call projection FAILS IT CLOSED (`svelte-runtime-unsupported-component`) rather than dropping the attachment or mis-emitting an element `$.attach`. This is an official-ACCEPTS / Verter-fail-CLOSES divergence (not reject parity). Guard: the `component_attach` fail-matrix row. **Done when:** the component-call projection emits the `[$.attachment()]: expr` computed-key member (in official member order), a positive `lifecycle/`-corpus golden pins it structurally, and the `component_attach` fail-closed row is converted in the SAME change. |
| D-39 | **`<svelte:element>` + GLOBAL-HOST LIFECYCLE DIRECTIVES (DEFER-NEW, owner: the 5f-b special-region / host-lifecycle follow-up; hard trigger: any change to the `<svelte:element>` callback-body emission or the global-host (`<svelte:window\|document\|body>`) attr gates, any broadening of special-host directive acceptance, AND in any case before declaring the Svelte client runtime feature-complete / RC).** Official `svelte@5.56.3` ACCEPTS lifecycle directives on the dynamic element — `<svelte:element this={t} use:foo>` emits `$.action($$element, ($$node) => foo?.($$node))` inside the element callback (transitions likewise against `$$element`) — and on the global hosts (`<svelte:body use:foo />` emits `$.action($.document.body, …)`). Verter 5f-c keeps BOTH fail-closed (`svelte-runtime-unsupported-component` / `-dynamic-attribute`): the element-lifecycle emission is regular-element-hosted only (the walk's inline init domain + post-event phase), and the special-region hosts need the callback-body / init-body phase threading 5f-b owns. Guards: the `svelte_element_use` fail-matrix row + the `classify_svelte_element` / `classify_special_host` refusal arms. **Done when:** the `<svelte:element>` callback body and the global-host init body emit the four lifecycle helpers against `$$element` / the host globals with the official phase order, positive goldens pin them, and the fail-closed rows/arms are converted in the SAME change. |
| D-40 | **ASYNC / BLOCKER WRAPPING OF LIFECYCLE EXPRESSIONS (DEFER-EXISTING, owner: the experimental-async block owner; hard trigger: any change accepting `await` inside template expressions, any landing of the official `run_after_blockers` machinery, AND in any case before declaring the Svelte client runtime feature-complete / RC).** Official experimental-async wraps lifecycle expressions containing `await` / async blockers through `run_after_blockers` before registering the action/transition/animation/attachment. Verter fails ANY `await` inside a lifecycle expression closed through the shared fallible rewriter (`svelte-runtime-unsupported-experimental-async`) BEFORE the plan exists — never a torn synchronous registration. Guard: the `lifecycle_async_expr` fail-matrix row. **Done when:** the experimental-async block lands the blocker-aware wrapping for lifecycle expressions and the fail-closed row is converted in the SAME change. |
| D-42 | **`slot=` DISPOSITION LANDED: the COMPONENT-OWNER THREE-CLASS disposition PLUS a SNIPPETBLOCK STATIC TEXT-VALUED-ONLY DIRECT-CHILD branch; the remaining refusals are official REJECT PARITY (not deferrals) — EXCEPT the D-43 custom-element-host / native-slotting over-refusal, which is outside this parity claim: any `slot=` shape blocked because its source owner or ancestor is a custom-element host, including hyphenated tags and customized built-ins with `is=`, component-family plain-prop descendants under that host, `<svelte:element slot>` descendants under that host, any custom-element host used as a direct component slot filler, and any slot-bearing child whose nearest owner is `<svelte:element>`.** Official `svelte@5.56.3` ACCEPTS and Verter EMITS: **(Class A)** a STATIC `slot=` on a DIRECT slot-declaring-component child routed into the parent's `$$slots.NAME` — the accepted ELEMENT filler is a supported NON-CUSTOM intrinsic element ONLY (a hyphenated / `is=`-carrying custom-element filler is the D-43 over-refusal: official accepts it, Verter refuses at the custom-element HOST gate before slot routing); a component / `<svelte:component>` / `<svelte:self>` filler ALSO keeps the `slot` prop on its own call, and a `<svelte:element>` filler folds it via `$.attribute_effect($$element, () => ({ slot: '…' }))`; **(Class B)** a `slot` (static OR dynamic/mixed) on a component-family host with NO direct-placement owner at all — neither a direct component child nor a direct `{#snippet}`-body child — as an ordinary plain prop (top level, nested in a supported non-custom element, in a block body, or hoisted out of a slotted `<svelte:fragment>` — a Class-B host under a custom-element ancestor never reaches the plain-prop path: the owner refuses first at the custom-element host gate, the D-43 class); **(SnippetBlock static branch)** a SINGLE STATIC **TEXT-VALUED** `slot=` (official `is_text_attribute`) on a DIRECT `{#snippet}`-body child of a filler-capable host kind — official validates a snippet direct child as component-owned placement (`is_component = true`), so the `slot` stays a plain attr/prop on the host itself: a regular non-custom element bakes it into the `$.from_html` skeleton, a component / `<svelte:component>` / `<svelte:self>` keeps the `{slot: '…'}` prop on its own call, and a `<svelte:element>` folds it via `$.attribute_effect` — snippet direct children are NOT slot fillers, do NOT route into `$$slots`, and do NOT enter the duplicate/default-slot checks (the placement fact is the lowering-recorded `SvelteRuntimeIr::direct_snippet_slot_attr_child_hosts` set, populated at the snippet lowering call site, never inside the `{#await}`-shared child-lowering helper); a dynamic/mixed OR VALUELESS/boolean `slot=` on a direct snippet child REJECTS (oracle `slot_attribute_invalid` — the accepted form must be text-valued; Verter mints the typed `DynamicAttribute` reject; the valueless reject is SNIPPET-SCOPED — the owner-less Class-B valueless `<Inner slot/>` plain prop `{slot: true}` stays an official accept) and snippet-child membership DISABLES the Class-B plain-prop path — a custom-element snippet-static host is the D-43 over-refusal (the slot gate accepts the placement; the custom-element HOST gate then fails closed). The fail-closed remainder is official reject parity, kept permanently: a regular (supported non-custom intrinsic) element `slot=` outside direct-filler and direct-snippet-child placement and any non-direct element inside component/default/slotted-fragment content, PROVIDED the element is NOT a descendant of a custom element (official `slot_attribute_invalid_placement` scopes its reject to exactly "a child of a component or a descendant of a custom element" — the custom-element / `<svelte:element>`-owner acceptance is the D-43 over-refusal, EXCLUDED from this parity claim); a top-level / non-direct `<svelte:element slot>` outside a custom-element owner (same reject, same D-43 exclusion for the custom-element-owner form); a dynamic/mixed `slot` on a DIRECT component-family/element child or a DIRECT `{#snippet}`-body child (`slot_attribute_invalid` — official's static-value rule fires at `owner === parent`, and a `{#snippet}` body IS the owner); a duplicate slot name (`slot_attribute_duplicate`); the explicit-default conflict — official exempts ONLY a regular-element / `<svelte:fragment>` sibling carrying a `slot` attribute, so a component-family `slot="default"` child self-conflicts (`slot_default_duplicate`); `<svelte:boundary slot>` (`svelte_boundary_invalid_attribute`); a top-level `<svelte:head slot>` (`svelte_head_illegal_attribute`) and a component-nested `<svelte:head>` (`svelte_meta_invalid_placement`, refused on the official-reject rail); a standalone `<svelte:fragment slot>` (`svelte_fragment_invalid_placement`). Guards: the `slot_attr_*` reject rows in `svelte_client_fail_matrix.rs`, the per-`SpecialKind` disposition unit proof (`validate_slot_placement_disposition_is_exhaustive_per_host_kind`, including the direct-snippet-child rows), the `{#snippet}`-child slot cluster in `client_tests.rs` (`static_slot_on_*_snippet_child_*` positives, `dynamic_slot_on_direct_snippet_child_fails_closed`, the `valueless_slot_on_*_snippet_child_fails_closed` rejects plus the Class-B scope lock `valueless_slot_on_toplevel_component_class_b_still_accepts`, the fragment/boundary/head snippet-child rejects, `custom_element_static_slot_snippet_child_fails_closed_at_host_gate`), the emission positives + reject tests in `client_tests.rs`, and the `components/slot_filler_*` / `components/slot_prop_*` oracle goldens. |
| D-43 | **CUSTOM-ELEMENT HOST / NATIVE-DOM `slot=` OVER-REFUSAL (DEFER-NEW, owner: future custom-element host support plus native-slotting carve-out; hard trigger: any change to the custom-element host gate, `element_carries_is_attribute`, `validate_slot_placement`, element-family slot acceptance, or `<svelte:element>` capability work, and before Svelte client runtime feature-complete / RC).** Official `svelte@5.56.3` accepts this class: slot-bearing content inside a custom-element host, a custom-element host used as a direct component slot filler, component-family `slot` props under a custom-element host, slot-bearing children whose owner is `<svelte:element>`, and a custom-element host bearing a static `slot=` as a DIRECT `{#snippet}`-body child (official emits the `importNode` clone + `$.set_custom_element_data`; Verter's unified slot gate now ACCEPTS the snippet-static placement and the custom-element HOST gate then fails closed until custom-element host support lands). Verter fail-closes it on two rails. **Rail 1 - custom-element host gate:** any hyphenated element or element carrying `is=` refuses as `svelte-runtime-unsupported-host-custom-element` before attribute or child classification. This covers hyphenated and customized-built-in owners/fillers and all descendants hidden behind that owner. Closure condition: implement custom-element host support, including official `importNode` clone topology, `$.set_custom_element_data`, and sanitized DOM local naming, then lift the host gate. **Rail 2 - `validate_slot_placement` native-slotting carve-out:** when no literal custom-element host blocks traversal, a slot-bearing child owned by `<svelte:element>` reaches the unified slot choke-point and is rejected as `UnsupportedSvelteRuntimeSurface::DynamicAttribute { name: "slot", span: <slot-bearing node span> }`; the diagnostic code is only the end-to-end symptom. Closure condition: add the native-DOM-slotting carve-out for custom-element-descendant and `<svelte:element>`-owner placements, emitting `slot` as a plain DOM attribute, skeleton-baked or via `$.attribute_effect`, without weakening permanent reject-parity placements. Fail-closed proof: generic host-gate rows `custom_element_attr`, `custom_element_static_attr`, `custom_element_no_attr`, `customized_builtin_static_attr`, `generated_static_attr_shapes_land_on_boundary::{static_attr::custom_element, static_attr::customized_builtin_is}`, plus `validate_slot_placement_disposition_is_exhaustive_per_host_kind` and the snippet-static host-gate probe `custom_element_static_slot_snippet_child_fails_closed_at_host_gate` (`client_tests.rs` — pins the host-gate reject identity `host-custom-element`, not the slot gate's `DynamicAttribute`). Representative slot probes, not exhaustive proof: the shapes listed above with their expected diagnostics. **Done when:** rail 1 and rail 2 closure conditions land as needed per sub-shape; the representative fail-closed probes convert to positive goldens in the same change; D-42's exclusion can then be narrowed or removed. |
| D-44 | **CUSTOM-ELEMENT BARE `$host()` DEGENERATE-UNBOUND OFFICIAL OUTPUT (ACCEPTED FAIL-CLOSED UPSTREAM-BUG DIVERGENCE — not plan-close debt; owner: Svelte 5h custom-element `$host` parity owner; LOCAL HARD TRIGGER: re-run the matrix on any change to the `$host` scan, the `client_plan.rs` props-parameter decision, the `needs_context` predicate, or the `$host` rewrite).** Official `svelte@5.56.3` rewrites admitted `$host()` to `$$props.$$host` but emits an unbound `$$props` reference when no independent props-parameter trigger exists (the degenerate-unbound class: bare / alias `const h = $host()` / argument-position `$host()` — a guaranteed runtime `ReferenceError`). Verter must NOT reproduce official's unbound `$$props.$$host` output and must NOT silently repair it with non-official helper topology. Guard: fail closed as `UnsupportedSvelteRuntimeSurface::HostOrCustomElement { surface: "$host", ... }` iff `host_used && !props_param_bound`, where `props_param_bound = real_props_binder || needs_context`; this means no real props binder and no `needs_context` reason (a member on the `$host()` call result — in a handler or a `{@render}` dynamic callee — IS a `needs_context` reason and binds normally). Alias-member forms such as `const h = $host(); h.x` are not binding triggers unless another `needs_context` reason exists. The blanket `|| host_used` silent-repair stays deleted; the degenerate residue refuses through `svelte-runtime-unsupported-host-custom-element`, and positive/negative tests cover the matrix above. **Review/retire when:** an intentional re-pin of the pinned Svelte oracle shows upstream no longer emits the degenerate unbound `$$props.$$host` output for bare/alias/arg `$host()`; the matrix is regenerated and reviewed; Verter converges to the new official accepted/rejected behavior or this row is narrowed/removed; and the tests are updated in the same change. |
| D-45 | **RENDER / HANDLER CALLEE `needs_context` TEMPLATE-LEXICAL-SCOPE BLINDNESS (DEFER-EXISTING — a pre-existing gap surfaced, not introduced, by the custom-element / `$host()` block (5h); implementation owner: the general `needs_context` / template lexical-scope owner; administrative obligation to add this row now = the custom-element / `$host()` block (5h), which surfaced the gap by routing render callees through the same shared `needs_context` predicate the handler path already used; hard trigger: any change to the `needs_context` predicate, the render/handler callee-safety scan, or template lexical-scope shadowing, AND in any case before declaring the Svelte client runtime feature-complete / RC / plan close-out. Cite: codex-DEFER disposition ruling (2026-07-05).** The shared `needs_context` render/handler callee-safety analysis is template-lexical-scope-BLIND: a callee whose name is shadowed by a `{#each}`-alias binding or a `{#snippet}`-parameter binding is treated as an unsafe callee and forces the context frame, so the component body OVER-FRAMES with an extra `$.push($$props, true)` / `$.pop()` pair versus official `svelte@5.56.3`, which recognizes the shadowing local as safe and emits no frame. The divergence is SYMMETRIC across the handler and render-callee paths: the handler path carried this scope-blindness pre-existingly, and the custom-element / `$host()` block made the render-callee path inherit the same shared predicate, so render now over-frames on the identical shadowed shape — not a new render-only regression. Per the conformance taxonomy this extra `$.push`/`$.pop` is a NON-COSMETIC helper / call-topology conformance gap (not cosmetic carrier formatting), so it is a tracked ledger row and NOT merely a note. It is CONTRIVED (zero manifestations across the repo corpus) and FAIL-SAFE (the divergence is an extra context frame ONLY — the inner emit is byte-identical to official, e.g. `$.get(foo)`, never a wrong-binding rewrite and never a missing frame); but fail-safe is NOT fail-closed (Verter emits divergent output, it does not refuse), and this is NOT an accepted final deviation like D-44 (which carries an explicit rationale, guard, and retire trigger) — hence a `DEFER-EXISTING` tracking row rather than an accepted-deviation record. **Done when:** `needs_context` is made scope-aware — a callee shadowed by a `{#each}`-alias or a `{#snippet}`-param binding is recognized as a safe local and does NOT force a frame, while unshadowed framing is preserved unchanged — AND official-parity goldens cover BOTH the shadowed cases (the shadowed handler callee and the shadowed render callee emit no extra frame, matching pinned `svelte@5.56.3`) AND the unshadowed controls (still framing). |

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
| `$host()` + `<svelte:options customElement>` | `$$props.$$host` + the conditional 6-arg `$.create_custom_element(Cmp, props, [], [], shadowRootInit?, extend?)` (define only for a tagged descriptor; the fact-driven `$.push`/`$.pop` context frame driven by `needs_context` — a reactive-analysis reason, e.g. an unsafe render callee, OR non-empty `$$exports` accessor exports; `props_param_bound` only controls `$$props` binding / bare-`$host()` admission, and a real props binder alone does not open the frame — and the `$$exports` accessors + `return $.pop($$exports)` only when prop accessors exist) | 5h |
| Static template + DOM walk | `$.from_html`/`$.first_child`/`$.child`/`$.sibling`/`$.reset`/`$.append` | 4 |
| Reactive text | `$.template_effect` + `$.set_text` | 4 |
| Dynamic attrs / boolean DOM props | `$.set_attribute` / direct DOM prop | 5a |
| `class:`/`class={}`, `style:`/`style={}` | `$.set_class` / `$.set_style` | 5a |
| Spreads `{...x}`, `{@html}` | `$.attribute_effect`/`$.rest_props`, `$.html` | 5b |
| Full `bind:*` family | `$.bind_*` (§3.2 bindings table) | 4 (`bind:value`/element `bind:this`), 5c (ordinary-DOM breadth + shared bind substrate), 5f-a (component binds), 5f-b (special-element binds) |
| Delegated + non-delegated events, legacy modifiers | `$.delegated`/`$.delegate`, `$.event`, `$.preventDefault`/… | 4 (delegated), 5d (regular-element non-delegated + legacy modifiers); special-element global events → 5f-b |
| Blocks `{#if}`/`{#each}`/`{#await}`/`{#key}` | `$.if`/`$.each`/`$.await`/`$.key` (each/await bindings are `$.get` signals, §3.3) | 5e |
| `{@const}` / `{@debug}` | `{@const}` → `$.derived(() => …)` (runes mode) / `$.derived_safe_equal` (legacy, §3.2.1); `{@debug}` → `$.template_effect(() => { console.log({…$.snapshot}); debugger; })` | 5e |
| `{const …}` / `{let …}` declaration tags (5.56, `DeclarationTag` — distinct from `{@const}`) | a plain inert block-local `const`/`let` declaration (NO `$.derived` memo); declarators may carry runes / be async (region-root only in 5e; nested-in-element placement → D-36.) | 5e |
| Components (static + `{@render}` snippets), `<svelte:component>`, `<svelte:element>`, `{@attach}` | direct `Child($$anchor, {…})` / `$.component` / `$.snippet` / `$.element` / `$.attach` | 5f-a (components / `{@render}` / `<svelte:component>`), 5f-b (`<svelte:element>`), 5f-c (`{@attach}`) |
| Special elements `<svelte:head>`/`<svelte:document>`/`<svelte:boundary>`/`<svelte:self>`/`<svelte:fragment>`/`<svelte:window>`/`<svelte:body>` | `$.head` / `$.event(…, $.document, …)` / `$.boundary` / recursive self-call / `$$slots` entry / `$.event`+`$.bind_window_size` (per the §3.2 special-elements rows) | 5f-a (`<svelte:self>` / `<svelte:fragment>`), 5f-b (head/document/boundary/window/body) |
| Transitions / actions / animations | `$.transition` / `$.action` / `$.animation` | 5f-c |
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
special-element SSR / dynamic-title variants → 5f-b / 8; dev-mode SSR module-shape → 5k.

Block 5 is SPLIT by runtime feature family (D4) — the §3.2 matrix families are the split axis. Each
sub-block (5a-5n, incl. 5s) lands with its OWN vendored goldens, jsdom behavioral cases, and
`CompileUnsupported` deletion. The runes-completion (5g), custom-element (5h), static script-import
prelude (5s), legacy + store-subscription (5i), async (5j), dev-mode (5k), CSS-scoping (5l), the
`<svelte:options>`/compile-option axis (5m), and script/module-item completion (5n) sub-blocks close
the §9.1 feature matrix (E6 near drop-in parity).

**Grow-relationship between the feature sub-blocks and the cross-cutting blocks (Block 7 / 8 / 12).**
Blocks 7 (jsdom), 8 (SSR), and 12 (perf CI) are STANDING blocks, not one-shot. Their dependency column
lists their INITIAL landing dependency (`4, 5a-5f-c` — the breadth needed to stand the harness up), but
each later feature sub-block (5g-5n, incl. 5s) ADDS its own behavioral/SSR/perf cases AT ITS OWN landing: 5g-5n (incl. 5s)
do NOT re-enter Blocks 7/8/12 as a separate pass — instead, each sub-block's deliverables INCLUDE the
matching jsdom case (Block 7 harness), the SSR golden + string-render case (Block 8 harness), and any
new perf fixture (Block 12 set) for the surfaces it introduces, landed alongside the sub-block. The
harnesses themselves (the runners, the fixtures dir, the CI job) are stood up at 7/8/12; the per-feature
cases grow into them as 5g-5n (incl. 5s) land. This makes the perf gate INCREMENTAL (see §7 / below).

The Integration block (I, D2) makes the emitted JS reachable through the bundler / playground. SSR
(Block 8) + the CSR/SSR hydration round-trip gate (Block 9) are first-class (D-10). The Optimization
block (Block 14) is the narrow, conformance-gated optimizer (D-9, §3.6).

The Real-world UI corpus block (RC, D-16) vendors `bits-ui` + `shadcn-svelte` + `skeleton` as a feature-gated
external corpus — the **Svelte** real-world-component slice of a broader per-framework matrix. It lands
DEFERRED — after the Svelte compiler is feature-complete (so real components compile rather than mostly-refuse)
and BEFORE the OPTIMIZER (Block 14) and the real-world perf-acceptance extension of the perf-CI (Block 12) — so
the optimizer and the perf gate are validated/benchmarked against real-world cases, not only synthetic fixtures.
RC's benchmark axis EXTENDS the Block-12 harness with real-world baselines; it does NOT replace the §7 synthetic
≤1.10× Svelte-vs-Vapor incremental gate, which already runs at each 5a-5k (incl. 5s) landing. Execution order:
`5a-5h → 5s → 5i-5m → 5n → I → 6/7/8/9 → 10/11 → RC → 12/14 → 13`. INVARIANT: a framework's real-world corpus row (stress +
benchmark + typings) lands only AFTER that framework's required Verter surface is implemented (the
compile/benchmark axes require a runtime compiler; the typings axis requires the adapter + component-meta) and
BEFORE that framework's perf work — a framework is never benchmarked before its compiler exists. RC is
Svelte-only; the **Vue** real-world corpus is owned by a SEPARATE branch / work-stream (already-implemented Vue,
tracked outside this plan), not this plan's RC. The remaining not-yet-implemented frameworks defer their slices
accordingly, each landing WITH its adapter, before that framework's perf work, per the framework-agnostic
roadmap: Astro (accessible-astro-components + Starlight + the withastro cross-framework example), React
(Radix/Ariakit/MUI), Preact (preact + signals), Solid and Lit (selections per the roadmap), and the
cross-framework TanStack generics/typings slices (each riding its owning framework's row).

| Block | Scope | Depends on | STEP-0 |
| ----- | ----- | ---------- | ------ |
| **1** | **Carrier runtime cutover + module seam.** Add `compile_runtime` / `RuntimeCompileOptions` / `RuntimeCompileOutput` to `CarrierCompiler`; make `assemble_main_module` Vue-only (Vue-bridge-owned); route `compile_entry()` through `CarrierCompilerRegistry`. Vue becomes a registered runtime carrier. NO dual path survives (§4.5 guards). | — | **Yes** — Vue byte-identity characterization suite (§4.4) pinned BEFORE the cutover. |
| **2** | **Svelte oracle harness.** Vendored golden corpus + feature-gated official-compiler oracle (normalized structure + helper-call-topology diff, NOT bytes). Includes `scripts/gen-svelte-goldens.mjs` + the drift/sync guards (§5.3). | 1 | **Yes** — capture the pinned official-compiler corpus (client). |
| **3** | **Svelte runtime IR spike.** Design `svelte/runtime/ir.rs` + the DOM-path plan + the helper/delegated-event model, anchored to the §3.2 matrix. The IR is the SHARED pre-lowering surface feeding client AND server (§3.6 optimization envelope). | 2 | **Yes** — confirm the §3.2 classification against the pinned output. |
| **4** | **Client MVP.** Scripts + `$state`/`$.get`/`$.set`/`$.update` + the §3.2 `$state`/`$.proxy` classification + interpolation + `$.template_effect` + `bind:value` + `bind:this` + delegated `onclick` — emits the §1.2 official example. Includes the component export-name derivation (filename stem → JS-identifier-sanitized name; `_unknown_` when no filename — §1.2), with the explicit `name` compile option overriding it (`options.name ?? component_name`, both backends — §3.8; the resolved `name` arrives from the 5m compile-options resolver). PRIMARY risk: scope/shadowing (§3.3). | 3 | — (covered by Block 3 STEP-0). |
| **5a** | **Attributes + class/style.** Dynamic attrs (`$.set_attribute` / boolean DOM props), `class:`/`class={}` (`$.set_class`), `style:`/`style={}` (`$.set_style`). | 4 | — |
| **5b** | **Spreads + `{@html}`.** `$.attribute_effect`, `$.rest_props` + `rest_excludes`, `$.html`. | 4 | — |
| **5c** | **Bindings breadth (ordinary DOM-element hosts) + shared bind-operator substrate.** The `$.bind_*` family on ordinary DOM-element hosts owned by the intrinsic DOM emitter — textarea/select value (incl. `<select multiple>`: the `$.bind_select_value` 3-arg helper wiring + the static-`multiple` host gate; array/object-`$state`-rooted bind targets remain fail-closed at the separate non-primitive-`$state` proxy declaration-lowering boundary recorded in D-31; ADDING the `textarea`/`select`/`option` finite-allowlist rows + goldens in `client_allowlist.rs`, since they fail closed today), checked, group (component-fn-scoped `const binding_group = []`), media (`currentTime`/`paused` dedicated helpers, `duration` via `$.bind_property('duration','durationchange',…)` read-only, `played` via setter-only `$.bind_played`), dimensions (`$.bind_element_size`), contenteditable (`$.bind_content_editable`), generic DOM property (`$.bind_property('open','toggle',…)`), and the general getter/setter target-lvalue lowering for element binds (state signals, plain locals — initialized AND uninitialized, members, the TS-wrapped lvalue BOUNDARY (fails closed; canonical-lvalue-from-TS deferred to the `lang="ts"`-script block, and unreachable today since `<script lang="ts">` is refused entirely upstream), function-binding `bind:x={get,set}` on DOM hosts — including NAMED top-level function declarations referenced by the pair) — PLUS promoting the bind-operator metadata table (today IDE-only `svelte/ide/bind_contract.rs`, generated by `scripts/generate-svelte-bind-contract.mjs`) to a SHARED `svelte/bind_contract` owner consumed by BOTH the IDE projection and the runtime codegen (one authored registry = source of truth; pinned official `svelte@5.56.3` goldens = oracle; NEVER derive routing from runtime exports; static enums, not a hot tag-string splitter). The `$.set(…, true)` should_proxy flag is per-row policy — emitted ONLY where the official row requires it, NEVER on ordinary DOM value/checked/select/group/media/property setters. Component-host + special-element binds are NOT in 5c — component binds are feed-forwarded to 5f-a, special-element binds to 5f-b (see D-21). | 4 | — |
| **5d** | **Events breadth.** Regular DOM-host non-delegated `$.event`, capture-phase, legacy modifier wrappers (`$.preventDefault`/`$.stopPropagation`/… + passive booleans). | 4 | — |
| **5e** | **Control-flow blocks + `{@const}` + `{const …}`/`{let …}` declaration tags.** `$.if`, `$.each` (keyed/unkeyed/else, `$.index`), `$.await`, `$.key`, `{@const}` (runes mode → `$.derived(() => …)`; legacy non-runes mode → `$.derived_safe_equal(() => …)` — §3.2.1/§3.2/§9.1), and the plain-binding `DeclarationTag` lowering for `{const …}`/`{let …}` (distinct from `{@const}` — no `$.derived` memo; declarators may carry runes / be async — §3.2). | 4 | — |
| **5f-a** | **Components, snippets, render, slots, component-specials (the component/snippet vertical).** Direct component calls (`Child($$anchor, {…})`) + `$.component` (`<svelte:component>`), `{#snippet}` defs (module/instance/block-local `const`) + `$.snippet` / direct `{@render}`, component children / named slots / `$$slots` / `let:` slot props, component `bind:this` (`$.bind_this`) / `bind:prop` (getter-setter pair) / function-pair binds (consuming 5c's shared bind-operator substrate), `<svelte:self>` (recursive self-call), and `<svelte:fragment slot>` (a `$$slots` named-slot entry, absorbed at lowering). Owns ONLY the narrow `.svelte` default component-import subset (the imported local consumed as a component callee OR dynamic component value — e.g. `<svelte:component this={Imported}>`) via one general `UserImport` carrier — NOT arbitrary static-import hoisting (that is 5s). Cleanly buildable only as this full closure (component children ARE snippets; named slots ARE `$$slots`). 5f-a depends on 5c for the reusable bind-operator substrate, NOT because 5c shipped component binds. | 4, 5c, 5e | — |
| **5f-b** | **Special hosts + renderable specials.** `<svelte:window>` / `<svelte:document>` / `<svelte:body>` events + binds (size/scroll/property/`bind:this`) + the no-body/init-only emission (NO template / NO clone-root / NO `$.append`), `<svelte:head>` (`$.head`), `<svelte:element this={…}>` (`$.element`), `<svelte:boundary>` (`$.boundary`), the `<svelte:body bind:scrollX>` invalid-pair compile error, the GLOBAL EVENT output (`$.event(…, $.window\|$.document\|$.document.body, …)` via the SAME 5d substrate fed the global-host `EventEmitTarget` variants), the special-element node-gate opening (window/body/document/head/element/boundary acceptance), and the `refuse_unsupported_root_region` no-body/init-only root-region machinery. May reuse 5f-a's generic region-callback builder (for `<svelte:boundary>` body regions) but must NOT depend on component import/slot machinery. | 4, 5c, 5d, 5e | — |
| **5f-c** | **Element lifecycle directives + attachments.** `use:` (`$.action`), `transition:` / `in:` / `out:` (`$.transition`), `animate:` (`$.animation`), and `{@attach}` (`$.attach`). `{@attach}` belongs here, not with components — the IR already models it beside the action/transition runtime ops. | 4, 5e | — |
| **5g** | **Runes completion (production).** `$state.raw`/`$state.snapshot` (`$.snapshot`), `$effect.pre`/`$effect.root` (`$.user_pre_effect`/`$.effect_root`), `$effect.tracking` (`$.effect_tracking()` client / `false` SSR), `$props()` rest (`$.rest_props` + `rest_excludes`) / `$props.id()` (`$.props_id()` client / `$.props_id($$renderer)` SSR), `$bindable` (`$.prop` bindable flag), `$inspect` / `$inspect().with` / `$inspect.trace` production no-op. (`$derived.by` lands with `$derived` in Block 4 — §3.2/§9.1; `$state.eager` / `$effect.pending` are async-gated → 5j; the dev-mode `$.inspect`/`$.trace` forms → 5k.) Closes the production runes rows of §9.1. | 4 | — |
| **5h** | **Custom elements / `$host()`.** `<svelte:options customElement>` + `$host()` → `$$props.$$host` + the module epilogue `$.create_custom_element(Cmp, props, [], [], shadowRootInit?, extend?)` — the CONDITIONAL 6-arg shape (arg5 `{ mode: 'open' }` for the open/default shadow, OMITTED for `shadow:'none'` and spelled `void 0` when an `extend` arg6 follows, the verbatim object expression for an object shadow; arg6 = `extend` only when given) — with `customElements.define(tag, …)` only for a tagged descriptor (`{}` / compile-option-`true` create WITHOUT define; `customElement={null}` is a null-SKIP that falls back to the `customElement` compile option — a set compile option still applies; only with no option does the component compile plain) and the FACT-DRIVEN frame: the `$.push($$props, true)`/`$.pop()` context frame driven by `needs_context` (a reactive-analysis reason — e.g. an unsafe render callee — OR non-empty `$$exports` accessor exports); `props_param_bound` (`real_props_binder || needs_context`) separately controls `$$props` binding / bare-`$host()` admission — a real props binder alone (rest-only / whole-object `$props()`) does not open the frame; the `$$exports` get/set accessors + `return $.pop($$exports)` only when prop accessors exist. | 4, 5f-a | — |
| **5s** | **Static script-import prelude.** Broad top-level static import hoisting beyond 5f-a's narrow `.svelte`-component-default subset: instance non-component imports, named / default / namespace / side-effect imports, import-only `<script module>` items, the official import ordering slots, and imported-local binding registration — all broadening the SAME `UserImport`/`script_prelude` carrier 5f-a introduced (NOT a second import path). Required before 5i because store auto-subscription needs imports like `import { writable } from 'svelte/store'`. | 4, 5f-a | **Yes** — capture the official import-ordering corpus. |
| **5i** | **Legacy non-runes mode + store auto-subscriptions.** `export let` (`$.prop` accessor), reactive `$:` (`$.mutable_source`/`$.legacy_pre_effect`/`$.legacy_pre_effect_reset`/`$.deep_read_state`), `<slot>` (`$.slot`), `createEventDispatcher` (`$.init`) — §3.2.1; emits `import 'svelte/internal/flags/legacy'`. ALSO: store auto-subscriptions `$store` (the legacy-store reactivity contract; works in BOTH runes and legacy components) — client `$.store_get`/`$.store_set`/`$.update_store` + top-of-body `const [$$stores, $$cleanup] = $.setup_stores()` and trailing `$$cleanup()`; server `var $$store_subs;` + `$.store_get($$store_subs ??= {}, …)` and trailing `if ($$store_subs) $.unsubscribe_stores($$store_subs)` (component-fn-scoped, NOT module — §3.2 store-subscription rows). | 4, 5e, 5f-a, 5s | **Yes** — capture the official legacy-mode + store corpus. |
| **5j** | **Experimental async (flag ON).** Mirrors official's experimental gate: emit `import 'svelte/internal/flags/async'` + async `$derived`/`{#await}` helpers ONLY when Svelte's experimental flag is on (§3.2.2). Includes the async-gated runes `$state.eager` (`$.eager(fn)`) and `$effect.pending` (`$.eager($.pending)`). | 4, 5e | **Yes** — capture the flag-ON official corpus. |
| **5k** | **Dev-mode codegen (`dev: true`).** Mirrors official dev output: validation wrappers, `$.add_locations`, dev-mode `$.inspect` / `$inspect().with` / `$inspect.trace` (`$.trace` + `flags/tracing`) (§3.2.3). Production output (5g) is the baseline; this is the dev axis. | 4, 5g | **Yes** — capture the official `dev: true` corpus. |
| **5l** | **`<style>` compilation + CSS scoping (§3.7).** CSS-hash-algorithm parity with `svelte@5.56.3`; `svelte-<hash>` scoped-class injection into the serialized template HTML (client `$.from_html` AND server `$$renderer.push`, same hash); the `css` mode toggle (symmetric across backends, source-gated by `analysis.css.ast && !analysis.inject_styles`) — `external` (default) returns the CSS as a SEPARATE artifact on `RuntimeCompileOutput.styles` with no inline helper (both backends), `injected` emits a module-scope `const $$css = { hash, code }` + body helper (`$.append_styles($$anchor, $$css)` on client, `$$renderer.global.css.add($$css)` on server) and a `null` `compile().css` (NO separate artifact, both backends); the external CSS artifact reaches the bundler/unplugin as a style virtual file mirroring Vue's `query.type === "style"` flow (§10.1, Block I). | 4 | **Yes** — capture the official CSS corpus (`scripts/gen-svelte-goldens.mjs` CSS pass): scoped HTML + `css.code` + hash + the `external`-vs-`injected` JS shapes for BOTH client (`$.append_styles`) and server (`$$renderer.global.css.add`). |
| **5m** | **`<svelte:options>` + compile-option axis (§3.8).** A single options resolver folding compile options ∪ `<svelte:options>` inline overrides into the runtime backends, owning the output-affecting options without a dedicated home: `namespace` (`$.from_html`/`$.from_svg`/`$.from_mathml`), `fragments` (`$.from_html` vs `$.from_tree`), `preserveWhitespace`, `preserveComments`, `discloseVersion` (disclose-version import toggle), `compatibility.componentApi` (Svelte-4 `createClassComponent` client / `.render` server), `hmr` (`$.hmr` + `import.meta.hot`), and the legacy-gated `accessors` (`$.push`/`$.pop($$exports)` + prop getters-setters) / `immutable` (prop flag). The resolver also resolves `name` (`options.name ?? component_name`) and feeds it to the Block 4 component-naming step. The `<svelte:options>` reader reproduces the DEPRECATED `tag` HARD ERROR (`svelte_options_deprecated_tag`), NOT a fold. `name` mode-plumbing feeds Block 4 naming; `runes` mode-plumbing rides Block 4 / legacy lowering 5i; `css`/`cssHash` ride 5l; `customElement` rides 5h; `dev` rides 5k; `generate` rides 4/8; experimental async rides 5j. | 4, 5i, 5l | **Yes** — capture the per-option official corpus (name/namespace/fragments/whitespace/comments/discloseVersion/componentApi/hmr/accessors/immutable goldens, client + server) + the `<svelte:options tag>` error fixture. |
| **5n** | **Script / module-item completion.** The non-import top-level module/instance script items still fail-closed after 5s: arbitrary `<script module>` statements + exports, arbitrary instance-script statements beyond the supported rune/state/`bind:this`-local/function-pair allowlist (functions, classes, enums, plain locals read in non-interpolation positions, etc.; legacy reactive `$:` labels are owned by 5i, NOT 5n) — the broad script-item lowering + ordering. | 4, 5m | **Yes** — capture the official module/instance script-item corpus. |
| **I** | **Integration so emitted JS is reachable (D2).** `@verter/unplugin` API (§10.1: `VerterVue` / `VerterSvelte` / `Verter({ lang })` + `/vue` and `/sveltejs` subpaths, `.svelte` in the filter); `.svelte` in `default_known_dependency_extensions` (NAPI + WASM); playground Svelte preview (import map for `svelte/internal/client` + `svelte/internal/server`); NAPI/host routing confirmation; docs. | 4 | — |
| **6** | **Sourcemap hardening + generated-JS syntax validation.** Token-precise maps for rune reads/writes + template-effect bodies; OXC-parse every generated module. | 4, 5a-5f-c | — |
| **7** | **jsdom behavioral harness (client).** Execute emitted modules against the real pinned `svelte/internal/client`; assert DOM behavior across the breadth set. STANDING block: stood up at `4, 5a-5f-c`; each later sub-block (5g-5n, incl. 5s) lands its own jsdom cases into this harness at its own landing. | 4, 5a-5f-c (initial); 5g-5n (incl. 5s) add cases at their own landing | — |
| **8** | **SSR backend (first-class, D-10).** `svelte/runtime/server.rs` → `svelte/internal/server` output (the §3.2 server columns: `$$renderer.push`, `$.escape`, `$.attr`, `$.attr_class`/`$.clsx`, `$.attr_style`, `$.ensure_array_like`, comment markers); shares the pre-lowering IR with client (§3.6). Deliverables: server goldens regenerated from real `generate:'server'` output; a server-render BEHAVIORAL harness (render to string, assert HTML + `$.escape` escaping + `<!--[…-->`/`<!--]-->` comment markers). STANDING block: stood up at `3, 5a-5f-c`; each later sub-block (5g-5n, incl. 5s) lands its own SSR golden + string-render case into this harness at its own landing. | 3, 5a-5f-c (initial); 5g-5n (incl. 5s) add cases at their own landing | **Yes** — capture the official SSR output corpus (`scripts/gen-svelte-goldens.mjs` server pass). |
| **9** | **CSR/SSR hydration round-trip gate (first-class, D-10).** Client output must hydrate the SSR output from the SAME compiler. Render the component server-side (Block 8) → mount the client module with `$.hydrate` over the SSR HTML → assert NO hydration mismatch + post-hydration interactivity (events fire, bindings update). This is an acceptance gate, not an aspiration: "SSR works" = this gate is green. | 7, 8 | — |
| **10** | **Svelte analysis / component-meta.** Generalize `ComponentMetaInput`; Svelte component-meta from `resolve_svelte_surface()` + `ParsedSvelte`. | 1 | — |
| **11** | **Diagnostics / lint abstraction + LSP wiring.** `target_framework()` gate, neutral/Vue/Svelte rule sets, Svelte-native rules, LSP lint wiring. | 10 | **Yes** — rule taxonomy + config migration. |
| **RC** | **Real-world Svelte UI corpus (bits-ui + shadcn-svelte + skeleton) — D-16.** Vendor `huntabyte/bits-ui` + `huntabyte/shadcn-svelte` + `skeletonlabs/skeleton` under `.integration-tests/repos/`, LOCALLY-VENDORED + FEATURE-GATED (`external-corpus`) + EXCLUDED from the default hermetic run (guard `external_corpus_paths_not_present_outside_gated_tests`). Three axes mirroring the Vue/nuxt-ui external-corpus harness: (1) **stress-test** — compile every real `.svelte` through the client AND IDE paths, recording crash / hang / refuse(unsupported) / success + a coverage metric (refuse-rate trends down as feature blocks land; real-world crash/hang detection, cf. the 5a BigInt hang); (2) **benchmark** — perf-compare vs the official `svelte` compiler, feeding the Block-12 §7 perf-fixture set with real-world baselines; (3) **typings** — compile → valid TSX → tsgo type-check → assert component-meta prop/binding/slot/event types. **Svelte-only at RC** (the Vue real-world corpus is owned by a SEPARATE branch / work-stream, not this block): newly vendored `huntabyte/bits-ui` + `huntabyte/shadcn-svelte` + `skeletonlabs/skeleton`, all three axes. RC's benchmark axis EXTENDS the Block-12 §7 perf harness with real-world baselines — it does NOT replace the synthetic ≤1.10× Svelte-vs-Vapor incremental gate that runs at each 5a-5k (incl. 5s) landing; the optimizer (Block 14) and the final perf acceptance consume the real-world fixtures. **INVARIANT**: a framework's corpus row lands only after that framework's required Verter surface is implemented (the compile/benchmark axes require a runtime compiler; the typings axis requires the adapter + component-meta) and before that framework's perf work — never benchmark a framework before its compiler exists; enforced by the planned manifest guard `real_world_corpus_rows_require_registered_framework_surface` (rejects rows for unimplemented frameworks; reuses `external_corpus_paths_not_present_outside_gated_tests` for the ungated-`.integration-tests/repos/` rule). **Deferred** — each lands its own row WITH its adapter, BEFORE that framework's perf work: Astro (accessible-astro-components + Starlight + the withastro cross-framework example), React (Radix/Ariakit/MUI), Preact (preact + signals), Solid + Lit (selections per the framework-agnostic roadmap), and the cross-framework TanStack slices (each riding its owning framework's row). Lands when the Svelte compiler is FEATURE-COMPLETE (so real components compile, not mostly-refuse) and BEFORE the Svelte performance work (Block 14 optimizer + the Block-12 real-world perf acceptance). | 5a-5n (incl. 5s), I, 6, 7, 8, 9, 10, 11 | **Yes** — design the vendoring + per-axis harness (coverage / perf / typings metrics) mirroring `packages/benchmark/` + the nuxt-ui external-corpus pattern, the corpus-row acceptance manifest, and the `real_world_corpus_rows_require_registered_framework_surface` guard. |
| **12** | **Perf-comparison CI (D-13 / §7, §10.2).** Svelte compiler perf-comparison CI job modeled on the existing Vue `@verter/benchmark` (`packages/benchmark/`) + `.github/workflows/benchmark.yml`. Named baseline fixtures, the §7 metric + ≤1.10× numeric gate, the wired workflow. STANDING block: the job + the §7 baseline-fixture set are stood up at `4, 5a-5f-c, 7`; the gate then runs INCREMENTALLY — each later feature-family sub-block (5g-5n, incl. 5s) that touches the hot path adds its perf fixture and the gate runs at THAT sub-block's landing, so a regression is caught at the introducing block, not deferred to the end (§7). | 4, 5a-5f-c, 7 (initial); the gate runs incrementally per later sub-block landing | — |
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
- **Real-world extension (RC, D-16).** The synthetic baseline set above is the INCREMENTAL gate (it runs from
  5a-5k, incl. 5s). The Real-world UI corpus block (RC) later EXTENDS this same `@verter/benchmark` harness with real-world
  Svelte baselines (bits-ui / shadcn-svelte / skeleton),
  feeding the optimizer (Block 14) and the final perf acceptance — it does NOT replace the §7 ≤1.10× synthetic
  Svelte-vs-Vapor regression gate.

### 10.3 Deferred IDE/LSP parity follow-ups (post-compiler, user-directed 2026-06-22)

These concern the Svelte IDE/LSP experience relative to Vue. Item 1 remains an OPEN deferral, tracked in the
Decisions Log as **D-34** (it lands AFTER the compiler is feature-complete — post `5a-5n` (incl. 5s); sequence
around/after Blocks I / 10 / 11 / 13; NOT a codegen block; CROSS-FRAMEWORK, implemented in the shared
adapter / LSP layer keyed by the adapter registry, never as a Svelte fork; lands with the standard gate + a
regression test). Items 2 and 3 have since LANDED — this §10.3 listing (authored 2026-06-22) post-dated their
implementation (2026-06-14 / 2026-06-17); they are recorded below as LANDED for traceability, NOT as open
deferrals.

1. **Editor syntax highlighting (text colours) for non-Vue framework file extensions.** The VS Code extension
   (`packages/vue-vscode`) ALREADY ships a `.vue` TextMate grammar (`source.vue` / `syntaxes/vue-generated.json`
   in `packages/vue-vscode/package.json`, with embedded-language injection for `<script>` TS/JS, `<style>` CSS,
   and template regions), so `.vue` file CONTENT is highlighted — Vue is the parity model. `.svelte` (and any
   future non-Vue adapter file extension) has NO bundled grammar, so its content renders uncoloured (a `.svelte`
   user relies on a third-party Svelte extension). Ship the grammar(s) / embedded-language injection in the shared
   extension keyed by the adapter registry so each non-Vue adapter's files are highlighted like `.vue` already is
   (and like the official Svelte extension). Cross-framework. (This is editor token colouring, NOT file-tree
   icons.) (Tracked in the Decisions Log as **D-34**.)
2. **LANDED — JSX types shim, user install dropped.** The `@verter/svelte-jsx` types shim is host-materialized
   (written once per host version OUTSIDE the user workspace) and located via provider `paths` injection, so a
   `.svelte` user's TS program resolves the shim with NO workspace install. The
   host-materialized resolution (the authoritative LSP/TSGO path) is proven by
   `asset_resolution_without_workspace_svelte_npm_dep_resolves_shim_via_mapping` and
   `production_topology_resolves_via_injected_rows_and_fails_without_them`
   (`crates/verter_lsp/src/svelte_assets.rs`); the ts-plugin path resolves the plugin-bundled copy via normal
   node resolution (`@verter/svelte-jsx` is a bundled `dependency` of `packages/typescript-plugin`). Mechanism
   design: `docs/arch/multi-framework-adapters-plan.md` D-av / D-ay.
3. **LANDED — `.svelte` generated TSX is regenerated on document change.** On a `.svelte` document change the
   generated IDE TSX is recompiled/invalidated and the position-mapper rebuilt through the SHARED carrier-general
   `did_change` → `ensure_ide_compiled` → fresh-`PositionMapper` path plus the eager `sync_tsx` push (Vue and
   Svelte share it — the prior `is_vue()` gate became `is_framework_carrier()`). Regression
   test `did_change_installs_a_fresh_position_mapper_arc` (`crates/verter_lsp/src/documents/mod.rs`) drives a real same-file `.svelte` content edit and asserts a FRESH `PositionMapper` Arc afterward — the observable product of the carrier recompile, RED→GREEN-discriminating for that path (it fails if the `did_change` → `ensure_ide_compiled` recompile/rebuild is skipped, the stale-TSX root cause); the assertion is the mapper rebuild the recompile produces, not a regenerated-TSX byte-diff.

---

## 11. Legacy Deletions

- Any `@verter/svelte-runtime` facade scaffolding, should any be present (none today — this program
  ensures none is built; D-1, §9).
- The "runtime codegen for non-Vue frameworks is out of scope" invariant in
  `docs/arch/multi-framework-adapters-plan.md` (Invariant 4) — superseded for Svelte (D-7); update it
  to reference this plan.
- Hardcoded `compile_sfc` / `compile_from_parsed` / `vue_parse` routing in
  `crates/verter_session/src/host_resolve/virtual_file_pipeline.rs::compile_entry()` — DELETED, replaced
  by `CarrierCompilerRegistry::compile_bundle` dispatch (Block 1, LANDED).
- **CARRY-FORWARD (audit / helper compile callers → carrier registry).** The audited compile entry
  `crates/verter_session/src/host_compile_audit.rs::compile_with_audit_options()` still drives the
  hardcoded Vue SFC compiler (`verter_compiler::compile::compile`) directly — a second Vue runtime-compile
  path outside the carrier registry. STEP-0 explicitly deferred migrating the audit / helper compile
  callers in Block 1, so the full carrier migration (route it through `compile_bundle`, returning per-carrier
  output) is NOT done in Block 1. To close the SILENT-WRONG-OUTPUT risk in the interim, `compile_with_audit`
  is made explicitly VUE-ONLY: it classifies the file and FAILS CLOSED on a non-Vue framework carrier
  (a `.svelte` file) with a typed `VerterE001` diagnostic rather than silently Vue-compiling it (Block 1).
  Follow-on: migrate `compile_with_audit_options` (and any other audit/helper compile caller) to the carrier
  registry so the audited path compiles every registered carrier — tracked as Decisions Log row **D-33**
  (audit / helper compile carrier migration); `TODO(follow-up)` at the `host_compile_audit.rs` callsite.
- Generic `assemble_main_module` consuming the Vue-shaped `VerterCompileResult` — renamed
  `assemble_vue_main_module`, consumes the NEUTRAL `RuntimeCompileOutput` (Block 1, D-6, LANDED).
- Any dual-path / framework-branch / feature-flag in `compile_entry()` after Block 1 — forbidden by
  the §4.5 guard `compile_entry_routes_through_carrier_registry_not_hardcoded_vue` (LANDED).
- The `.vue`-only default filter in `packages/unplugin/src/index.ts::createFilter()` — replaced by a
  filter matching `.vue` AND `.svelte` (Block I, D2).
- Svelte runtime typed-unsupported (`CompileUnsupported`) diagnostics — removed for each surface once
  the runtime codegen for it lands. Every in-scope block deletes ITS surface's diagnostic at its own
  landing: Blocks 4, 5a, 5b, 5c, 5d, 5e (control-flow blocks, `{@const}`, AND the `{const …}`/`{let …}`
  declaration-tag surface), 5f-a (component/snippet/slot vertical), 5f-b (special hosts +
  renderable specials), 5f-c (element lifecycle directives + `{@attach}`), 5g (runes completion), 5h (`$host`/custom-element), 5s (static script-import prelude), 5i
  (legacy non-runes AND store auto-subscriptions `$store`), 5j (experimental async flag-ON), 5k
  (dev-mode), 5l (`<style>`/CSS scoping), 5m (the `<svelte:options>`/compile-option axis — §3.8:
  `namespace`/`fragments`/`preserveWhitespace`/`preserveComments`/`discloseVersion`/
  `compatibility.componentApi`/`hmr`/`accessors`/`immutable`), 5n (script/module-item completion), and 8 (SSR). All of 5g-5n (incl. 5s) are
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
