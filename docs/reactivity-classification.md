# Reactivity Classification

Verter statically classifies every top-level binding in a Vue SFC's `<script setup>` block into a `ReactivityKind`. This classification drives LSP features (hover, completions, diagnostics) and template codegen optimizations without needing a full type checker at analysis time.

## ReactivityKind Enum

Defined in `crates/verter_analysis/src/types.rs`.

| Variant | Meaning | Example | `.value` needed? |
|---------|---------|---------|-------------------|
| `None` | Not reactive. Const literals, functions, classes, plain const bindings. | `const x = 42` | No |
| `Ref` | Ref-like wrapper. Holds a single reactive value. | `const count = ref(0)` | Yes |
| `Computed` | Computed ref. Derived reactive value, read-only. | `const double = computed(() => count.value * 2)` | Yes |
| `Reactive` | Reactive object. Properties are accessed directly. | `const state = reactive({ count: 0 })` | No |
| `MaybeRef` | Composable return. May or may not be a ref, depending on the composable's implementation. | `const data = useFetch('/api')` | Unknown |
| `Mutable` | `let` binding. Reassignable, but not inherently reactive. | `let name = 'hello'` | No |

`None` is the default. The enum is serializable (`serde`) and used across the FFI boundary.

## Classification Strategy

Classification runs during static analysis in `crates/verter_analysis/src/analysis.rs`, gated by the `AnalysisScope::REACTIVITY` flag.

### classify_reactivity_kind()

The core function takes a `VueApiClassification` (from import analysis) and the callee name, then maps to `ReactivityKind`:

```
VueApiClassification     ->  ReactivityKind
─────────────────────────────────────────────
Ref, ShallowRef,         ->  Ref
CustomRef, ToRef
Computed                 ->  Computed
Reactive, ShallowReactive ->  Reactive
(no Vue API match)       ->  MaybeRef  (if callee starts with `use` + uppercase)
(no Vue API match)       ->  None      (otherwise)
```

### Declaration-level rules

Before `classify_reactivity_kind` runs, the analyzer checks the declaration form:

- **`let` / `var` binding** -- immediately classified as `Mutable`, regardless of initializer.
- **`const` with a call expression** -- callee is looked up in the import map. If the import source is `"vue"`, the `VueApiClassification` determines the kind. Otherwise, the `useXxx` naming convention check applies.
- **Function declaration, class declaration** -- always `None`.
- **`const` with a literal, reference, or unrecognized expression** -- always `None`.

### Composable detection (useXxx convention)

When the callee has no `VueApiClassification` (not from `vue`), the analyzer checks whether the function name matches the Vue composable convention:

1. Starts with `use`
2. Has more than 3 characters
3. The 4th character is uppercase (e.g., `useCounter`, `useFetch`)

If all three conditions hold, the binding gets `ReactivityKind::MaybeRef`. This is a heuristic -- composables may return refs, reactive objects, plain values, or structured objects. The `MaybeRef` kind signals this ambiguity.

## LSP Usage

The LSP features in `crates/verter_lsp/src/features/` consume `ReactivityKind` from the `AnalyzedBinding` struct.

### Hover (`hover.rs`)

Shows reactivity info in hover tooltips:

| ReactivityKind | Hover text |
|----------------|------------|
| `None` | (no annotation, or "reactive" if legacy `is_reactive` flag set) |
| `Ref` | *(ref -- needs `.value`)* |
| `Computed` | *(computed -- needs `.value`, read-only)* |
| `Reactive` | *(reactive -- direct property access)* |
| `MaybeRef` | *(maybe ref -- may need `.value`)* |
| `Mutable` | *(mutable -- reassignable)* |

### Completion (`completion.rs`)

Adds a reactivity tag to completion item details:

| ReactivityKind | Tag |
|----------------|-----|
| `Ref` | `ref` |
| `Computed` | `computed` |
| `Reactive` | `reactive` |
| `MaybeRef` | `maybe-ref` |
| `Mutable` | `mutable` |
| `None` | (none, or `reactive` if legacy flag) |

### Diagnostics (planned)

Can warn about `.value` access on non-ref bindings and missing `.value` on ref bindings.

## Optimizer Usage

The `AnalysisScope::BUILD_OPTIMIZED` preset enables reactivity classification during build. The template codegen and cross-file analysis use this for:

- **Static binding detection**: `None`-classified bindings don't need reactivity tracking in the template. The codegen can hoist them or skip `renderEffect` wrapping.
- **Prop constness (CROSS_PROP_CONST)**: When a parent passes a `None`-classified binding as a prop, and all call sites agree, the prop is classified as constant and skips dynamic tracking.
- **Ref/Computed in template expressions**: Bindings with `Ref` or `Computed` kind trigger `renderEffect` wrapping in Vapor mode codegen, since accessing `.value` creates a reactive dependency.

## Type Resolution Fallback Chain

For non-SFC files (composables, utility modules), exported function return types are analyzed when `AnalysisScope::FUNC_RETURNS` is set. The fallback chain for determining `ReturnReactivity`:

1. **Explicit TS annotation from AST** (cheapest) -- If the function has a return type annotation (e.g., `: Ref<number>`), `classify_return_type_annotation()` checks for known Vue type wrappers (`Ref<`, `ShallowRef<`, `ComputedRef<`, `Reactive<`, `ShallowReactive<`).

2. **Body walk (heuristic fallback)** -- If no annotation exists, `classify_return_reactivity_from_body()` walks all return statements (respecting function boundaries, so nested functions/arrows are not confused with the outer return). Each return expression is classified by checking if the returned value is a call to a known Vue API. Multiple return paths must agree; if they disagree, the result is `Unknown`.

3. **Type provider (TSGO)** (planned) -- The `ScriptTypeEnhancements` struct has placeholder fields for resolved types from an external type checker. When connected, this would provide the definitive answer, overriding heuristic analysis.

The `ReturnReactivity` enum maps to `ReactivityKind` when composable info is built:

| ReturnReactivity | ReactivityKind |
|------------------|----------------|
| `Ref` | `Ref` |
| `Reactive` | `Reactive` |
| `Plain` | `None` |
| `ObjectWithReactiveFields(...)` | (per-field classification in `ComposableReturn::Object`) |
| `Unknown` | `None` |

## Composable Return Shape

For composables (`useXxx` functions), the analyzer also determines the return shape (`ComposableReturn`):

- **`Single(ReactivityKind)`** -- Returns a single value (ref, reactive, or plain).
- **`Object(Vec<ComposableReturnField>)`** -- Returns a destructurable object. Each field has its own `ReactivityKind` and a flag for whether it's a function.
- **`Tuple(Vec<ReactivityKind>)`** -- Returns a tuple-like array.
- **`Unknown`** -- Cannot determine the return shape.

This powers future LSP features like auto-destructuring suggestions and per-field `.value` guidance.

## Relevant Source Files

| File | Role |
|------|------|
| `crates/verter_analysis/src/types.rs` | `ReactivityKind`, `AnalyzedBinding`, `ReturnReactivity`, `ComposableReturn` |
| `crates/verter_analysis/src/analysis.rs` | `classify_reactivity_kind()`, `classify_return_type_annotation()`, `classify_return_reactivity_from_body()` |
| `crates/verter_analysis/src/classify.rs` | `VueApiClassification` enum, `classify_vue_api()`, `is_reactivity_api()` |
| `crates/verter_analysis/src/scope.rs` | `AnalysisScope` flags (`REACTIVITY`, `FUNC_RETURNS`) |
| `crates/verter_lsp/src/features/hover.rs` | Hover tooltips using `ReactivityKind` |
| `crates/verter_lsp/src/features/completion.rs` | Completion items with reactivity tags |
