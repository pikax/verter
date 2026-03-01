# Phase 0: Inter-Crate Dependency Contracts & Architecture Review

## Dependency Graph Accuracy

**Declared vs. Actual — ALL ACCURATE**

```
verter_core                    (standalone — OXC only)
verter_analysis                (standalone — OXC only)
  ↓
verter_diagnostics             (analysis → rules → diagnostics)
  ↓
verter_actions                 (diagnostics + analysis → code actions)

verter_host                    (core + analysis)
  ↓
verter_ffi                     (host + diagnostics → serializable types)
verter_lsp                     (host + analysis + diagnostics + actions)
verter_wasm                    (host + ffi + analysis + diagnostics)
verter_napi                    (host + ffi + core)
```

No circular dependencies detected. All dependencies flow downward in a clean DAG.

---

## API Surface Cleanliness Assessment

### verter_core — VERY CLEAN
- Entry points: `compile::compile()` — takes source → `CompileResult`
- Internal pipeline modules (`ast`, `script`, `style`, `template`, `tsx`) are feature-gated behind `bench` flag
- Production users only see high-level compile API
- Outputs use `Arc<str>` for code (lazy-clone friendly), `String` for diagnostics

### verter_analysis — VERY CLEAN
- 40+ exported functions/types via explicit `pub use` statements
- Key boundary types: `ScriptAnalysisSnapshot`, `TemplateAnalysisSnapshot`, `StyleBlockAnalysis`, `AnalyzedBinding`, `AnalyzedImport`
- No internal leakage

### verter_diagnostics — VERY CLEAN
- `Linter::lint()` takes analysis snapshots → `DiagnosticSet`
- Three linting variants: `lint()`, `lint_with_source()`, `lint_with_cross_file()`
- Detection-only, no mutation. No dependency on host or FFI

### verter_actions — VERY CLEAN
- `ActionEngine::fixes_for()` takes `LintDiagnostic` + `ActionContext` → `Vec<CodeAction>`
- Minimal dependencies: only `verter_diagnostics` and `verter_analysis`
- Can be used standalone

### verter_host — CLEAN WITH INTENTIONAL COMPLEXITY
- Public API: `VerterHost::new(config)`, `upsert()`, `resolve()`, `get_virtual_file()`
- Uses `Arc<str>` heavily in responses for zero-copy sharing
- Internal modules (`cache`, `deps`, `hash`, `parse`, `compile`, `upsert`) all private

### verter_ffi — EXCELLENT ABSTRACTION LAYER
- Single source of truth for serialization contract between NAPI and WASM
- All FFI types use `#[serde(rename_all = "camelCase")]` + flat structs
- Framework-agnostic conversion functions used by both NAPI and WASM

### verter_lsp — CLEAN COMMAND DISPATCHER
- Dispatches LSP messages to feature handlers
- Each feature is independent; diagnostics fetched from `VerterHost`
- Converts between LSP positions and internal byte offsets via `LineIndex`

### verter_wasm / verter_napi — THIN BINDING LAYERS
- Wrap `VerterHost`, delegate all operations
- Use `verter_ffi` types for serialization
- Panic handling prevents crashes

---

## Type Ownership Patterns

- **Host → consumer**: Compile results are `Arc<str>` (shared, cheap clone)
- **Consumer → FFI → JS**: Serialization to `String` (serde handles conversion)
- **JS → FFI → Rust**: Input as `String`, converted to `Arc<str>` only if cached
- Minimal copying; lazy Arc clones only when needed

## Feature Gate Usage

- **verter_core `bench`**: Exposes internal modules for benchmarks; production modules are private
- **verter_host `host_metrics`**: Optional atomic counters for LSP debugging; zero overhead when off
- **verter_wasm `console_error_panic_hook`**: Dev-only panic → JS error conversion

## Architectural Concerns

1. **Diagnostics vs. Actions Decoupling — HEALTHY**: Actions depends on diagnostics, not vice versa
2. **LSP Multi-Concern Consolidation — ACCEPTABLE**: Consumer crate naturally depends on all lower layers
3. **FFI Conversion Logic — EXCELLENT**: Single source of truth for type mapping
4. **Type Closure — NO LEAKAGE**: External dependencies (OXC, serde) are hidden at boundaries
5. **Cross-File Type Resolution — CLEAN API**: Explicit FFI boundary, no hidden state

## Dependency Health Summary

| Metric | Status | Notes |
|--------|--------|-------|
| Circular dependencies | None | Clean DAG |
| API surface leakage | Minimal | Internal modules feature-gated or private |
| Type ownership | Correct | Arc<str> for shared code, String at FFI boundary |
| Feature gates | Well-used | bench, host_metrics, console_error_panic_hook |
| FFI abstraction | Excellent | Single source of truth (verter_ffi) |
| Consumer coupling | Appropriate | LSP depends on all layers; layers don't know about LSP |
| Trait boundaries | Clean | LintRule, LintVisitor, ActionProvider well-scoped |

## Architecture Grade: A

The inter-crate dependency structure is well-designed. The linter → diagnostics+actions split was an excellent architectural choice achieving clean separation of concerns, reusability, testability, and consumer flexibility.
