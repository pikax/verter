# Phase 4B — Consumer-Surface Inventory (sub-task 4B.0)

**Status:** Authoritative inventory committed BEFORE policy code.
**Branch:** refactor/semantic-db-overhaul
**Pre-state:** post-624b14d2 (deletion of `rematerialize_public_component_meta_types` +
`choose_less_symbolic_component_meta_type_expr`).

This document drives the rule set in `apply_component_meta_resolution_policy`. The
4-rule list in the plan (D-Q13) is EXPECTED, not authoritative — the rules below
are derived from per-consumer rg evidence.

## 1. Adapter pass-through (`packages/component-meta/src/adapters/*.ts`)

Each adapter consumes `TypeDescriptor` (the JS-projection of `TypeExpr` produced
by the dispatch layer). The adapter's `case "ref":` arm and `case "object":` arm
diverge sharply.

### 1.1 `adapters/zod.ts`

| Site | Arm | Output | Reads body? |
|---|---|---|---|
| `:46-50` | `"object"` (string-form) | `"z.object({})"` | No |
| `:83-115` | `"object"` (full body) | `z.object({ ...properties })` w/ index sigs | YES — walks `type.properties` |
| `:113-116` | `"ref"` | `"z.unknown()"` | No |
| `:213-217` | `"object"` (deserialize fast) | `z.object({})` | No |
| `:247-282` | `"object"` (deserialize full) | `z.object({...})` | YES |
| `:280-282` | `"ref"` | `z.unknown()` | No |

**Verdict:** adapters/zod consumes Object schemas; "ref" produces opaque
`z.unknown()`. Adapter wants Object for project-local non-Props. Adapter has no
project-aware registry walking.

### 1.2 `adapters/json-schema.ts`

| Site | Arm | Output | Reads body? |
|---|---|---|---|
| `:73-95` | `"object"` (full) | `{ type: "object", properties: {...}, required: [...] }` | YES — walks `type.properties` |
| `:107-110` | `"ref"` | `{ description: type.name }` | No |
| `:166-170` | `"object"` (string-form) | `{ type: "object" }` | No |

**Verdict:** json-schema produces concrete schemas for Object, opaque
description-only stub for Ref. Adapter wants Object for project-local non-Props.

### 1.3 `adapters/histoire.ts`

| Site | Arm | Output |
|---|---|---|
| `:89-90` | `"object"` | `{}` |
| `:92-93` | `"ref"` | `undefined` |

**Verdict:** histoire returns empty object placeholder for Object, undefined
(skipped) for Ref. Distinct outputs; adapters DO see the difference.

### 1.4 `adapters/storybook.ts`

| Site | Arm | Output |
|---|---|---|
| `:105-106` | `"object"` (control) | `{ type: "object" }` |
| `:133-134` | `"object"` (typeName) | `"object"` |
| `:139-142` | `"ref"` (typeName) | `type.name` |
| `:160-161` | `"object"` (summary) | `"object"` |
| `:166-169` | `"ref"` (summary) | `${type.name}<...>` |

**Verdict:** storybook has different outputs for Object vs Ref but is the most
forgiving adapter — it formats both shapes successfully.

## 2. Compat-layer mapping (`packages/component-meta/src/compat/*.ts`)

The compat layer translates `TypeDescriptor` to vue-component-meta's
`PropertyMetaSchema` shape (`kind: "object" | "ref" | "literal" | "enum"`).

### 2.1 `compat/schema.ts:155, :209, :213, :411, :452`

| Site | Behavior |
|---|---|
| `:89-91` | Builds `{ kind: "object", type: name, schema: {...} }` for Object descriptors |
| `:155` | Same — recursive descent |
| `:209, :213` | **Ref descriptors fall through to `{ kind: "object", type: name, schema: {} }` placeholder** |

**Verdict:** the compat schema builder does NOT differentiate at output —
everything becomes `{ kind: "object", schema }`. **But** for Ref descriptors with
no Object body, `schema` is `{}` (empty placeholder). Downstream consumers that
walk `schema.schema` see no properties.

### 2.2 `compat/checker.ts:2201-2216` (PREFERRED public path)

```ts
const declared = host.getDeclaredComponentMeta(canonical);
if (declared) { return declared; /* fast path */ }
const full = host.getComponentMeta(canonical); /* fallback */
```

**Verdict:** compat prefers `getDeclaredComponentMeta`. If the policy pass is
not wired into `get_declared_component_meta_payload`, the regression survives.

### 2.3 `compat/checker.spec.ts:2682, :2797`

Test fixtures explicitly expect `{ kind: "object", type: "ChipProps", schema: {} }`
(EMPTY schema for Props-suffix imports). **Compat WANTS symbolic Ref shape for
*Props imports** so vue-component-meta sees a "named opaque" entry instead of
inlined member properties.

## 3. Native consumers (LSP, MCP, NAPI)

```bash
$ rg "TypeExpr::Ref\b|TypeExpr::Object\b" crates/verter_lsp crates/verter_mcp crates/verter_napi
# No matches.
```

**Verdict:** native consumers (LSP, MCP, NAPI) do NOT case-match on
TypeExpr::Ref vs TypeExpr::Object. They consume the FFI-projected
`TypeDescriptor` only after the JS adapters and compat layer translate it. No
direct contract obligation at the Rust boundary.

## 4. Integration-test fixtures

### 4.1 `nuxt-ui-e2e/docs/app/components/content/ComponentPropsSchema.vue`

```ts
function getSchemaProps(schema: PropertyMeta['schema']): any {
  if (!schema || typeof schema === 'string' || !schema.schema) return [];
  if (schema.kind === 'object') {
    return Object.values(schema.schema).filter(...)
  }
  return (Array.isArray(schema.schema) ? schema.schema : Object.values(schema.schema)).flatMap(getSchemaProps);
}
```

**Verdict:** fixture explicitly tests `schema.kind === 'object'` and walks
`schema.schema` (the property bag). When prop type is symbolic Ref, the compat
layer projects it to `{ kind: "object", schema: {} }` — empty schema bag —
`Object.values(schema.schema)` is `[]` — fixture renders no prop rows. **This is
the production-facing regression closed by Phase 4B.**

### 4.2 `nuxt-ui-e2e/docs/app/components/content/ComponentProps.vue`

Same pattern — reads `prop.schema.schema` member-bag.

## 5. Pre-vs-Post-624b14d2 behavioral diff per consumer category

| Imported type category | Pre-624b14d2 (rematerialize ON) | Post-624b14d2 (rematerialize OFF) | Required by adapters |
|---|---|---|---|
| `ImportedUser` (project-local non-Props interface) | `Object{id: number, ...}` | `Ref(ImportedUser)` | **Object** |
| `Status` (project-local non-Props alias `'idle' \| 'busy'`) | `Union[Literal,Literal]` | `Ref(Status)` | **Object** (or Union — preserves shape) |
| `ButtonProps[]` (Props-suffix array) | `Array { Ref(ButtonProps) }` | `Array { Ref(ButtonProps) }` (same — pre-deletion preserved Props uniformly) | symbolic Ref |
| `Pick<ProgressProps, 'color'>` (Props-suffix utility wrap) | symbolic (untouched) | structural Object/Union | symbolic Ref |
| `Omit<ButtonProps, K>` (Props-suffix utility wrap) | symbolic (untouched) | structural Object/Union | symbolic Ref |
| `AvatarProps` (bare Props-suffix import) | symbolic Ref | symbolic Ref | symbolic Ref |
| `Button['ui']` (member-path-on-Props) | symbolic IndexedAccess | symbolic IndexedAccess | symbolic IndexedAccess |
| `T from /node_modules/...` (package-backed) | symbolic Ref | structural | symbolic Ref |

## 6. Derived rule set

The rules below close the diff in §5 and are **authoritative** for `apply_component_meta_resolution_policy`. The pass mutates `ComponentMetaAnalysis` IN-PLACE on the dispatch result; each rule fires per `TypeExpr`-by-`TypeExpr` walk.

### Rule 1 — Declaration-provenance: keep package-backed Refs symbolic.

For `TypeExpr::Ref { name, type_arguments }` where the registry meta entry's
`canonical_source` contains `/node_modules/`: keep the Ref as-is (do NOT chase
into package-internal types). Recurse into type_arguments.

**Inputs:** `&[ResolvedTypeRegistryMeta]` (declaration.canonical_source).
**Pure:** YES (no host).

### Rule 2 — Member-path-on-Props: keep IndexedAccess<*Props, ...> symbolic.

For `TypeExpr::IndexedAccess { object, index }` where the object is a Ref
whose name ends with `"Props"` (or transitively in Union/Intersection): keep
the IndexedAccess as-is. Recurse into other arms.

**Inputs:** structural (TypeExpr only).
**Pure:** YES.

### Rule 3 — Project-local non-Props: chase Ref to registry body.

For `TypeExpr::Ref { name, type_arguments: [] }` where:
- the registry meta's `canonical_source` is set AND does not contain `/node_modules/`
- the name does NOT end with `"Props"`
- the resolved type registry contains an entry named `name` whose `type_expr`
  is a non-Ref structural shape (Object, Union, Intersection, Array, Tuple,
  Function, Primitive, Literal, etc.)

→ replace with the registry entry's `type_expr.clone()`.

**Inputs:** `resolved_type_registry: &[ResolvedTypeAnalysis]`,
            `resolved_type_registry_meta: &[ResolvedTypeRegistryMeta]`.
**Pure:** YES.

### Rule 4 — Props-suffix bare alias: keep symbolic.

For `TypeExpr::Ref { name, type_arguments: [...] }` where `name` ends with
`"Props"`: keep symbolic. Recurse into type_arguments only on Rule 1's check
(which gates the recursion to non-Props arms).

**Inputs:** structural (name suffix).
**Pure:** YES.

### Rule 5 — Recurse into compound shapes.

For Array, Tuple, Union, Intersection, Object, Function, IndexedAccess (when
not member-path-on-Props), Conditional, Mapped, KeyOf: recurse into each child
TypeExpr, applying rules 1-4 at each leaf.

**Inputs:** structural.
**Pure:** YES.

## 7. Per-field rewrite contract (sub-task 4B.0.2)

| Field | Action | Why |
|---|---|---|
| `props[i].type_expr` | rewrite via policy_apply | primary public surface |
| `events[i].parameters[j].ty` | rewrite | symmetric |
| `events[i].return_type` | rewrite | symmetric |
| `slots[i].bindings[j].type_expr` | rewrite | symmetric |
| `models[i].type_expr` | rewrite | symmetric |
| `exposed[i].type_expr` | rewrite | symmetric |
| `accepted_props[i].type_expr` | re-derive after props mutation | derived |
| `accepted_events[i].parameters` | re-derive after events mutation | derived |
| `public_instance` | **recompute** via `populate_public_instance_sidecar` | derived from props/events/exposed/models |
| `fallthrough_surface` | leave as-is | host-populated; not a TypeExpr |
| `type_registry[i].type_expr` | leave as-is | this is the SOURCE of Rule 3's body — rewriting it would lose the resolved body |

**Key invariant:** `populate_public_instance_sidecar` MUST run after primary
fields mutate. The deleted `rematerialize_public_component_meta_types` rebuilt
this sidecar — the new pass MUST preserve this behavior or break public
instance schema.

## 8. Provenance plumbing audit (sub-task 4B.0.1)

All 5 rules are PURE over `(resolution.resolved_type_registry, resolution.resolved_type_registry_meta)`:

- Rule 1 reads `registry_meta[i].declaration.canonical_source` — present on
  `ResolvedTypeRegistryMeta`.
- Rule 2 reads `TypeExpr::IndexedAccess.object` and Ref name suffix — purely
  structural.
- Rule 3 reads both `registry_meta[i].declaration.canonical_source` AND
  `registry[i].type_expr` for the body lookup.
- Rule 4 reads Ref name suffix only.
- Rule 5 is structural.

**No `&VerterHost` parameter required.** The policy pass signature drops the
host:

```rust
pub fn apply_component_meta_resolution_policy(
    analysis: &mut ComponentMetaAnalysis,
    type_registry: &[ResolvedTypeAnalysis],
    type_registry_meta: &[ResolvedTypeRegistryMeta],
);
```

If 4B.0 had revealed a rule needing host data not in resolved state, that
would be STOP CONDITION — propose a `PolicyExtraction` mutable-carrier
extension. **Inventory confirms no such rule exists.**

## 9. Test contracts to revert (sub-task 4B.3)

Listed in plan revision 11.3 §4B.3:

1. `evaluate_types_invalidates_cached_results_when_dependency_changes`:
   `props["user"].type_expr = TypeExpr::Object { properties: [{name: "id", ...}] }` (Rule 3).
2. `get_component_meta_resolves_imported_helper_aliases_without_dep_env_merge`
   (rename `_post_outcome3` suffix back):
   `props["status"].type_expr = TypeExpr::Union[Literal("idle"), Literal("busy")]` (Rule 3).
3. `public_component_meta_keeps_utility_wrapped_imported_refs_symbolic`:
   - `actions: Array { Ref(ButtonProps) }` (Rule 4 — Props uniform symbolic).
   - `close: Union[..., Omit<ButtonProps, K>]` (Rule 4 — utility on *Props symbolic).
   - `progress: Union[..., Pick<ProgressProps, K>]` (Rule 4).
4. `resolve_component_meta_keeps_imported_slot_param_member_paths_symbolic_in_registry`:
   `slot_binding.type_expr` stays `TypeExpr::IndexedAccess { object: Ref(Button), index: Literal("ui") }` (Rule 2).

`step7_rematerialize_function_deleted_post_outcome3` test STAYS — the
rematerialize function does not come back; the policy lives in a new
`apply_component_meta_resolution_policy` function that operates on the resolved
state, not on a separate query-engine walk.

## 10. Conclusion

**5 rules** (not 4 as in D-Q13's expected list — the structural recursion is
called out as Rule 5 for clarity). All rules are PURE. Wired at a single
shared helper consumed by all 3 public extraction paths. No STOP CONDITION 8
hit (consumer contracts beyond the 4 reverted tests are all served by these
rules).
