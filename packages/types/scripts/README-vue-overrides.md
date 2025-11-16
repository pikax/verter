# Vue Macro Override Generator

Generates enhanced TypeScript macro overrides for Vue 3 `<script setup>` compiler macros (`defineProps`, `withDefaults`, `defineEmits`). Each override:

- Preserves Vue's original JSDoc documentation
- Renames the macro with a configurable suffix / prefix (currently `_Box` suffix)
- Intersects the original macro return type with a hidden unique metadata key: `& { [UniqueKey]?: <DerivedType> }`
- Derives `<DerivedType>` using deterministic rules based on the macro signature
- Minimizes internal Vue type inlining (only what is actually referenced)
- Conditionally imports or defines helper utility types (`UnionToIntersection`, `IfAny`) only when needed

## Current Output File

```
packages/types/src/vue/vue.macros.ts
```

This replaces the previous `vue.ts` file. The old file is deleted automatically when regenerating.

## Generation

```bash
pnpm generate:vue
```

Or directly:

```bash
node scripts/generate-vue-overrides.mjs
```

## Return Type Derivation Rules

For each function overload, the base type used in the intersection is computed as:

1. If the macro has parameters:
  - Single parameter: use its type directly.
  - Multiple parameters: use a tuple of all parameter types.
2. Else, if it has type parameters:
  - Single type parameter: use that type.
  - Multiple type parameters: use a tuple of those types.
3. If neither applies: fallback to `any` (rare / defensive).

The UniqueKey value (`[UniqueKey]?: ...`) is:

- For an array base type (e.g. `EE[]`): `Base & [Base]` (embedding both the array and a single-element tuple of it)
- For a tuple (multiple params or generics): the tuple itself
- For a single non-array type: the type itself

## Example Transformations

| Original Vue Macro | Override Name | Original Return | Derived Intersection |
|--------------------|---------------|-----------------|----------------------|
| `defineEmits<EE>(emitOptions: EE[])` | `defineEmits_Box` | `EE[]` | `EE[] & { [UniqueKey]?: EE[] & [EE[]] }` |
| `defineEmits<E extends EmitsOptions>(emitOptions: E)` | `defineEmits_Box` | `E` | `E & { [UniqueKey]?: E }` |
| `defineEmits<T extends ComponentTypeEmits>()` | `defineEmits_Box` | `T` | `T & { [UniqueKey]?: T }` |
| `withDefaults(props, defaults)` | `withDefaults_Box` | `[DefineProps, Defaults]` | `[DefineProps, Defaults] & { [UniqueKey]?: [DefineProps, Defaults] }` |
| `defineProps<PropNames extends string>(props: PropNames[])` | `defineProps_Box` | `PropNames[]` | `PropNames[] & { [UniqueKey]?: PropNames[] & [PropNames[]] }` |

## Helper Type Handling

- `UnionToIntersection` is imported from `../helpers/helpers` only if referenced by an inlined internal type or a macro signature.
- `IfAny` is locally re-declared only if actually required (referenced inside included internal Vue utility types).
- All other Vue exported types are qualified as `import("vue").TypeName` unless they are detected helper types outside the root `vue` export surface.

## Internal Type Inlining Strategy

Only internal Vue types that are directly (or transitively) referenced by the selected macro signatures are inlined. Their dependencies are traversed recursively. This keeps the generated surface lean (e.g. currently only `InferDefault`, `InferDefaults`, `NativeType`).

## Naming Configuration

Edit in `generate-vue-overrides.mjs`:

```ts
const NAME_APPEND = "_Box"; // suffix
const NAME_PREPEND = "";    // prefix
```

Both are applied when constructing the new macro name: `NAME_PREPEND + original + NAME_APPEND`.

## Extending Overrides

Modify the list:

```ts
const FUNCTIONS_TO_OVERRIDE = ["defineProps", "withDefaults", "defineEmits"]; // add more here
```

The script automatically discovers overloads across all relevant Vue type declaration files.

## Regeneration & Safety

The generated file includes a header warning. Do **not** edit `vue.macros.ts` manually—rerun the generator instead.

## Why Intersection + UniqueKey?

Embedding `UniqueKey` allows downstream tooling to carry opaque metadata alongside the original return type without polluting the developer-facing API surface or triggering structural type mismatches.

## Troubleshooting

- If a helper type unexpectedly appears qualified as `import("vue").X`, ensure it is either exported directly by `vue` or add logic to treat it as a helper.
- If the file is missing expected JSDoc blocks, verify the extraction logic in `extractFunctionDeclarations()` was not altered.
- Run `pnpm generate:vue` after upgrading Vue to refresh overload discovery.

## License

Auto-generated output and this script follow the repository's root license.

