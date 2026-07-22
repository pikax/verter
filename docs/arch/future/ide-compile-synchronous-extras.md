# IDE compilation runs its extras synchronously on the LSP serve thread

**Status:** deferred architectural refactor. A partial mitigation (a *lighter* IDE
compile) has landed; the full refactor has not.

**Owner ruling that produced this document (2026-07-22):** *"`CompileTarget::IDE` is
meant to have all bells and whistles, but that does not mean we should have them sync.
It seems it might need a proper refactor. Please assess the blast radius if we make it
async and not fully required. If too much, store it in `docs/arch/future/*.md` and change
the IDE to a lighter compilation, making sure you add a comment explaining why and
pointing to the future todo document."*

The blast-radius assessment below concluded the full refactor is **too large for the
current effort**. The lighter compile landed instead, with a pointer to this file from
`CompileTarget::needs_runtime_prop_constructors` in
`crates/verter_compiler/src/compile/types.rs`.

---

## Symptom

Typing in an open SFC blocks every language feature for seconds per keystroke. A user
session captured on 2026-07-22 showed no TypeScript features at all for more than a
minute, then normal behaviour once the carrier reached the external engine.

Measured from the server's own instrumentation in that session:

| event | value |
| --- | --- |
| `DocumentRegistry::did_change ENSURE_COMPILED_DONE elapsed` | 2.357 s, 2.158 s, 1.573 s |
| `HANDLER_EXIT did_change ... elapsed` | 2.378 s, 2.176 s, 2.364 s, all on `ThreadId(2)` |
| `HOST_UPSERT_DONE elapsed` (the part that is genuinely required) | 3.6 ms – 5.1 ms |
| semantic dispatch events in the 5.9 s window | 12,550 (`execute_via_cold_build_helper` + memo hit/miss) |
| gap between one `did_change` exiting and the next entering | 2.5 ms and 2.7 ms |

So >99% of a `did_change` was compile plus Verter-native type resolution, and the next
`did_change` was already buffered in the transport before the previous one finished.

## Mechanism

Three separate couplings stack up.

1. **`did_change` compiles synchronously on the serve thread.**
   `crates/verter_lsp/src/documents/mod.rs` calls `host.ensure_ide_compiled(...)` inline
   in the `did_change` path. `tower-lsp` polls every handler on one thread, so while that
   compile runs no hover, completion, definition, or inlay hint can execute. The captured
   session shows `HANDLER_ENTER inlay_hint` → `no type_provider_context` → exit,
   repeatedly, for the whole blocking window.

2. **The compile entry demanded the heaviest macro bundle unconditionally.**
   `VerterHost::compile_entry` (`crates/verter_session/src/host_resolve/virtual_file_pipeline.rs`)
   asked for `VueMacroCodegenDemand::Runtime` for every Vue carrier, without consulting
   the caller's `CompileTarget`. The runtime bundle classifies one broad runtime
   constructor per public prop (`ClassifyBroadRuntime`), and classifying a member means
   resolving that member's whole type through the shared semantic engine. A component
   with a CSS style object prop therefore expands the entire `csstype` property union:
   in the captured session 5,064 of 5,342 scoped `ResolveDecl` executions were against
   `csstype`, driven by 75 `ClassifyBroadRuntime` executions (26 distinct prop routes ×
   3 compiles). *This is the part that has been fixed.*

3. **`CompileTarget::needs_script()` is true for `TEMPLATE_DATA`.**
   The LSP's interactive profile is `IDE | TEMPLATE_DATA` (`DocumentRegistry::new`).
   `needs_script()` is `SCRIPT | TEMPLATE | TEMPLATE_DATA`, so a TSX-only consumer also
   runs full runtime script codegen — macros, bindings, imports, `CodeTransform`, and the
   runtime `props: {...}` option object — none of which the external TypeScript engine
   ever sees. Template-data extraction needs script *bindings*; it does not need the
   rendered runtime output.

## What actually landed (the lighter compile)

Only the narrowest cut that removes engine work no TypeScript result depends on:

- `CompileTarget::needs_runtime_prop_constructors()` — new, deliberately narrower than
  `needs_script()`: `SCRIPT | TEMPLATE` only.
- `VueMacroCodegenDemand::RuntimeBindingNames` — public binding names and optionality
  without broad-runtime classification.
- `VueMacroCodegenDemand::for_compile_target()` — one target→demand mapping, replacing
  three separate decisions (two duplicated matches plus `compile_entry`'s hardcoded
  `Runtime`).
- `RuntimePropType::Unclassified` — an honest "the demand never asked", distinct from a
  computed semantic `Unknown` and from a degradation, so no consumer infers a
  classification that was never performed and no spurious member-degraded diagnostic is
  emitted.

Measured effect on the semantic engine, one `did_change` on one real component, debug
binary, `verter=debug` filter, counted off the server's own trace output:

| metric | before | after | change |
| --- | --- | --- | --- |
| `execute_via_cold_build_helper` (total semantic dispatches) | 14,394 | 606 | −95.8% |
| ... of which touch `csstype` | 4,736 | 76 | −98.4% |
| ... `ClassifyBroadRuntime` | 1,196 | 0 | −100% |
| `ENSURE_COMPILED_DONE` for that edit | 2.067 s | 350.3 ms | −83.1% |
| server trace volume for that edit | 27.1 MB | 1.04 MB | −96.2% |

Measured effect on latency (corpus F, debug profile, shared machine, A/B back to back
inside one bench-harness lock hold, 6 `did_change` notifications on one real SFC):

| metric | before | after | change |
| --- | --- | --- | --- |
| `ENSURE_COMPILED_DONE` median | 2042.9 ms | 339.7 ms | −83.4% |
| `ENSURE_COMPILED_DONE` sum over 6 edits | 11,978 ms | 1,558 ms | −87.0% |
| `HANDLER_EXIT did_change` median | 2068.7 ms | 385.2 ms | −81.4% |
| client-observed round trip median | 2063 ms | 384 ms | −81.4% |

Corpus-gate non-regression on the same corpus/route, same lock hold: hover p95
263 → 217 ms, hover max 4327 → 872 ms, definition p95 109 → 83 ms, server peak RSS
639.5 → 470.6 MB, requests errored 2 → 0.

The 606 dispatches that remain are the floor this document is about: they are what the
required core plus the still-synchronous extras cost, and they are why the compile is
still 340 ms rather than tens of milliseconds.

## Why the full refactor was deferred — blast radius

Making IDE compilation asynchronous and its extras non-required is not a call-site
change; it changes the artifact contract.

**Consumers that assume a fully-populated IDE artifact, synchronously:**

- `DocumentRegistry::did_change` / `did_open` build the document's `PositionMapper` from
  `get_ide(...).source_map` immediately after `ensure_ide_compiled`. A staged artifact
  means the mapper can be absent or stale, and every position-mapped feature
  (hover, definition, rename, semantic tokens, inlay hints) fails closed until it lands.
  The existing "preserve the old projection when compilation fails" branch is a
  *failure* path, not a *not-yet-ready* path, so it cannot be reused as-is.
- `sync_coordinator` publishes the `.tsx` companion to the external engine from the same
  artifact. A partial artifact must never be published: the engine would type-check a
  half-built carrier and emit wrong diagnostics. That needs an explicit readiness state
  on the artifact, which does not exist today.
- `$/verter/getVirtualFiles` and `$/verter/getAnalysis` read virtual nodes that only a
  full compile produces.
- The bundler/unplugin and `verter-tsc` paths call the same `compile_entry` and require
  the complete result; they are not interactive and must not be made to wait on a
  staged artifact.

**What would need a staged/partial artifact:** `CachedTsx` plus the compile-slot map
would need a per-extra readiness dimension (TSX ready / template data ready / CSS
analysis ready / macro semantics ready) and every reader would need an explicit
"not ready yet" branch. Today those are `Option`s that mean "this target did not ask for
it", which is a different thing and cannot be overloaded without making every consumer's
`None` ambiguous.

**Tests and guards pinning the current shape:** the compile-cache mode classification and
its publish/`ReturnOnly` rules, the artifact-commit generation stamping, the
`vue_macro_codegen` scheduler identity/singleflight tests, and the IDE codegen suites all
assume one atomic artifact per `(canonical, profile)`. `verter_compiler` alone has 5,825
lib tests over that contract.

**Hardest constraint:** the coalescing half cannot be solved inside `did_change` at all.
`tower-lsp` delivers notifications serially on one thread, so the handler cannot see that
a newer version is already buffered — the 2.5 ms inter-arrival above proves the newer
notification existed but was invisible. Superseding a stale compile therefore requires
changing the serve loop, which is the separately-recorded
`lsp-serve-loop-single-thread-head-of-line-blocking` item, not a change to this compile.

## Proposed design (when it is taken on)

1. Split the IDE artifact into a **required core** (parse + TSX + source map — the only
   thing the external TypeScript engine consumes) and **deferred extras** (template data,
   CSS analysis, runtime macro semantics, lens inputs, component-meta).
2. `did_change` produces only the required core inline, and that core must stay in the
   low-hundreds-of-milliseconds range — after the landed fix it already measures
   340 ms median on a real SFC in a debug build.
3. Extras are produced on the scheduler's CPU pool keyed by the same
   `(canonical, content hash)` identity, published on completion, and read through an
   explicit `Pending | Ready | Failed` state rather than `Option`.
4. Consumers of extras degrade honestly: a feature whose extra is `Pending` returns
   framework-native results or defers, never a wrong answer.
5. A superseded version cancels its in-flight extras through the existing
   `CancellationToken` rather than running to completion.

**Falsifiable prediction:** with the split in place, `ENSURE_COMPILED_DONE` inside
`did_change` drops below 50 ms median on the same corpus-F SFC in a debug build (from
340 ms after the landed fix, 2043 ms before it), and `active_handlers` never exceeds 1
for longer than that. Measure with the same A/B method: 6 `did_change` notifications on
one open carrier, reading the server's own `ENSURE_COMPILED_DONE` and
`HANDLER_EXIT did_change` instrumentation.

## What it costs to keep deferring

Every keystroke still blocks the single serve thread for the duration of the required
core plus whatever extras remain wired into it. At 340 ms and typing faster than that,
notifications still queue and each one still runs to completion, so a burst of N
keystrokes still serialises into N × 340 ms of dead time — better than N × 2 s, but the
same shape. The user-visible symptom (features unavailable while typing) is reduced, not
removed.

## Blast radius of leaving it

Nothing breaks. The landed lighter compile is behaviour-preserving for every consumer
that reads what it asked for: runtime and bundler targets still receive fully classified
constructors, and the IDE target receives exactly the binding names it consumes. The risk
of leaving it is only that a future change widens
`needs_runtime_prop_constructors()` back to `needs_script()` — which is why that function
carries a pointer to this document.

## Reproduction

Synthetic, no private corpus required:

1. An SFC with `defineProps<T>()` where `T` is imported and one member is typed by a
   deeply-nested third-party type (`csstype`'s `Properties` is the real-world case; any
   type whose expansion touches thousands of declarations reproduces it).
2. Compile it at `CompileTarget::IDE | CompileTarget::TEMPLATE_DATA` — the LSP's live
   interactive profile.
3. Before the landed fix, one broad-runtime classification runs per public prop and the
   compile walks the whole imported union. After it, zero classifications run and the
   binding names are unchanged.

The regression test is
`crates/verter_session/src/typeinfo/typeinfo_tests/vue_macro_codegen_part2.rs::ide_only_target_takes_binding_names_without_broad_runtime_classification`.
