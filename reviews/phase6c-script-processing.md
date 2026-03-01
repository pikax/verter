# Phase 6c: Script Processing Review

## Overall: WELL-ARCHITECTED — Clean Two-Layer Design with Type Resolution Gaps

Handles all major Vue macros in runtime and type-based variants. CSS v-bind complex expressions and conditional type resolution are the main gaps.

---

## Critical Issues

### C1. `find_matching_brace` Ignores String/Comment Context
**File**: script/macros.rs:549-567

Naive brace counting without handling strings, template literals, or comments. `defineProps({ msg: { default: '{}' } })` would cause incorrect brace matching.

**Impact**: Incorrect binding metadata for runtime defineProps with string-containing defaults. Mitigated by type-based defineProps taking a different path.

### C2. CSS v-bind Complex Expressions Have No Identifier Rewriting
**File**: script/css_vars.rs:62-65

Only simple identifiers are looked up in binding map. `v-bind(count + 1)` where `count` is a ref outputs `(count + 1)` instead of `(count.value + 1)`.

**Impact**: Any non-trivial `v-bind()` CSS expression referencing refs/reactive state produces incorrect runtime values.

---

## High Issues

### H1. `force_js_in_section` May Leak TypeScript Syntax
**File**: script/process.rs:793-808

Fallback on strip failure silently returns original section with TypeScript intact. In force_js mode (Rolldown/tsdown), TS annotations could leak into runtime output.

### H2. Conditional Type Resolution Returns `Unknown`
**File**: resolve_type.rs:1160

`TSConditionalType` → `Unknown`. `defineProps<{ x: T extends string ? string : number }>()` generates `type: null` instead of `type: [String, Number]`.

### H3. Qualified Type Names Only Use Rightmost Part
**File**: resolve_type.rs:1299-1308

`Namespace.Props` → only `Props` looked up. Components using namespaced type references get incorrect/empty prop definitions.

### H4. Specifier-Level Type-Only Imports (Known Gap)
`import { type X }` handling documented as remaining issue for nuxt-ui. Works correctly for explicit `type` keyword; edge case of value-syntax imports in type-only positions handled by runtime text check.

---

## Medium Issues

### M1. `extract_array_prop_names` Uses Naive String Parsing
Doesn't handle template literals (backticks). Rare in practice.

### M2. Companion Types Merge Uses `or_insert` Semantics
External types only inserted if key doesn't exist. If companion block re-exports modified type, unmodified companion version used instead.

### M3. `dedup_props` Only Works When `key_name` Populated
For locally-resolved props where `key_name` is None, dedup is a no-op. Union types with shared property names get duplicate props.

### M4. No Handling of `TSImportType`
`import('path/to/module').Type` returns `Unknown`. No runtime prop declarations for this pattern.

### M5. `component_name` Not Escaped for Single Quotes
File named `it's-a-test.vue` → `__name: 'it's-a-test'` → syntax error.

### M6. Recursion Guard Uses `Vec::contains` (O(n))
Should use `FxHashSet` for deep inheritance hierarchies.

### M7. `defineModel` Options Not Forwarded to Prop Runtime Definition
`defineModel('visible', { type: Boolean, default: false })` → generated model prop is always `visible: {}`. Type and default from options discarded.

**Impact**: Props from defineModel with options lose runtime type validation and defaults.

---

## Low Issues

- L1: Redundant OXC allocator creation per function call
- L2: `push_method_as_arrow` doesn't handle generator methods
- L3: `_useCssVars` callback always emits `_ctx` parameter
- L4: `AnalysisInsights` system in types.rs is defined but unused
- L5: `scope_id` in wrapper not HTML-escaped (safe in practice)

---

## Type Resolution Coverage

| Feature | Status |
|---------|--------|
| Inline type literals | Supported |
| Local type aliases | Supported |
| Local interfaces (with extends) | Supported |
| Union/Intersection types | Supported |
| Function types | Supported |
| `typeof X` | Supported |
| Generic constraints | Supported |
| Companion block types | Supported |
| External/cross-file types | Supported |
| Conditional types | NOT supported (Unknown) |
| Mapped types | Partial (Object, no prop extraction) |
| Indexed access types | NOT supported |
| Template literal types | NOT supported |
| Import types (`import('...')`) | NOT supported |
| Qualified names (`Foo.Bar`) | Partial (rightmost only) |
| Utility types (Pick, Omit) | Object inference only |
| Generic instantiation | NOT supported |

---

## Strengths

1. **Clean separation of concerns**: OXC analysis layer (`utils/oxc/vue/script/`) cleanly separated from codegen (`script/`)
2. **Comprehensive async detection**: Thorough expression walking, correct nested function boundary handling
3. **Correct `_withAsyncContext` transformation**: Matches Vue official compiler behavior
4. **Robust companion script merging**: Type deduplication, proper companion import marking
5. **Defensive external type spans**: `key_name` field pre-populated for cross-file types
6. **Template-used-vars filtering**: Only referenced imports in `__returned__` (tree-shaking)
7. **Correct `_mergeModels` usage**: Avoids brittle string-level insertion
8. **Force-JS mode pipeline**: Thoughtful strip-then-check with memchr fast scanning

---

## Priority Fixes
1. **C2**: CSS v-bind complex expression rewriting (affects runtime correctness)
2. **M7**: defineModel options forwarding (loses runtime defaults)
3. **H2**: Conditional type resolution (at minimum, resolve both branches)
4. **H3**: Qualified type name resolution (use full qualified path)
5. **M5**: Escape component_name for JS string literals
