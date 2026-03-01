# Reactivity Classification

::: warning Experimental
Verter is experimental software at v0.0.1-alpha.3. APIs may change without notice.
:::

Verter statically classifies every top-level binding in a Vue SFC's `<script setup>` block into a `ReactivityKind`. This classification drives LSP features (hover, completions, diagnostics) and template codegen optimizations without needing a full type checker.

## ReactivityKind Enum

| Kind | Description | Example |
|------|-------------|---------|
| `Ref` | Single-value reactive container (.value access) | `const x = ref(0)` |
| `ShallowRef` | Same as Ref, shallow reactivity | `const x = shallowRef(obj)` |
| `Reactive` | Deeply reactive proxy (no .value needed) | `const state = reactive({})` |
| `Computed` | Derived reactive value (.value, readonly) | `const doubled = computed(() => x.value * 2)` |
| `ComputedWritable` | Writable computed (.value, get/set) | `computed({ get, set })` |
| `TemplateRef` | Template element reference (.value) | `const el = useTemplateRef('el')` |
| `Composable` | Composable function return | `const { data } = useFetch()` |
| `Literal` | Non-reactive constant | `const PI = 3.14` |
| `Prop` | Component prop (reactive, readonly) | `const props = defineProps<{}>()` |

## How It Works

1. **Static analysis**: The analyzer examines function calls (`ref()`, `reactive()`, `computed()`, etc.) and import sources
2. **No type checker needed**: Classification is based on call patterns and import analysis
3. **Drives LSP features**: Hover tooltips show reactivity kind, completions auto-add `.value`, binding colors reflect kind

## IDE Features Powered by Classification

- **Binding colors**: Different colors for ref (blue), reactive (green), computed (yellow), literal (gray)
- **Hover tooltips**: Show `ReactivityKind` alongside type information
- **Auto .value**: Completions automatically add `.value` for Ref-like bindings
- **Diagnostics**: `no-ref-as-operand` rule uses classification to detect missing `.value`
