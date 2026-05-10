# Compat → TypeDescriptor heuristic mapping (W0.9 audit)

Source: `packages/component-meta/src/compat/checker.ts` at HEAD `d8f296fe`.
Drives W0.6's scope: every row with `YES` in "Missing IR variant?" is a
`@verter/type-ir` schema addition for W0.6.

## Existing `TypeDescriptor` kind enumeration

The `TypeDescriptor` discriminated union in `packages/type-ir/src/type-ir.ts`
currently exposes the following `kind` values (verified at HEAD `d8f296fe`):

- `primitive` (with `PrimitiveName` ∈ { string, number, boolean, symbol, bigint, any, unknown, void, never, null, undefined, object })
- `literal` (string / number / boolean literal)
- `union`
- `intersection`
- `array`
- `tuple`
- `object` (properties + optional indexSignatures, callSignatures, constructSignatures)
- `function` (parameters, returnType, optional typeParameters)
- `typeParameter` (name, optional constraint, optional default)
- `enum`
- `ref` (name + optional typeArguments)
- `recursiveRef` (name + typeArguments + conditionalContext)
- `unknown` (carries verbatim `rawType: string`)

There is **no** kind for indexed-access types (`T['K']`), no kind for
conditional types (`T extends U ? X : Y`), no kind for mapped types
(`{ [K in keyof T]: ... }`), no kind for `keyof` operator, no kind for
`typeof`, and no kind for template-literal types. Indexed-access shapes
currently land as `kind: "unknown"` carrying raw text.

The Verter native `verter_type_expr::TypeExpr` enum does carry richer
variants (notably `TypeExpr::IndexedAccess`), but the JS-side `@verter/type-ir`
DTO is the strictly narrower projection consumed by the compat layer. The
"Missing IR variant?" column is judged against what is exposed in
`@verter/type-ir`, not what the Rust enum can express.

## Audit method

For each function in the W7 rewrite scope (plan §4.8 + §8 list) the table
records:

1. **Function** — the name as it appears in `checker.ts`.
2. **Input** — the type the function accepts. Functions that already accept a
   `TypeDescriptor` are display formatters; functions that accept `string`
   are heuristic text-shape sniffers / splitters / normalisers.
3. **TypeDescriptor equivalent** — the structural predicate (or constructor)
   that replaces the text logic in W7. For pure display formatters the
   column records "display formatter — no structural replacement required".
4. **Missing IR variant?** — `YES` if the structural predicate cannot be
   expressed against the kind set above and an addition to `@verter/type-ir`
   is required (feeds W0.6); `NO` if the existing kinds cover the predicate.

If a YES row's missing variant has already been called out in plan §11 item 4
(i.e. `IndexedAccessType`), the row references it explicitly.

## Splitters

| Function | Input | TypeDescriptor equivalent | Missing IR variant? |
|----------|-------|---------------------------|---------------------|
| `splitTopLevelTypeOperator(s, op)` | `type: string`, `operator: "|" \| "&"` | Caller picks `t.kind === "union" ? t.types : [t]` for `"|"`, `t.kind === "intersection" ? t.types : [t]` for `"&"`. The function itself disappears. | NO |
| `splitTopLevelTypeUnion(s)` | `type: string` | `t.kind === "union" ? t.types : [t]` | NO |
| `splitTopLevelTypeIntersection(s)` | `type: string` | `t.kind === "intersection" ? t.types : [t]` | NO |
| `splitTopLevelObjectMembers(source)` | `source: string` (object body) | Walking `ObjectType.properties` (plus `indexSignatures`, `callSignatures`, `constructSignatures`) directly. | NO |
| `splitTopLevelCommaList(source)` | `source: string` | For function-parameter splits: walk `FunctionType.parameters`. For tuple-element splits: walk `TupleType.elements`. The character-level splitter has no remaining use in a typed pipeline. | NO |

## Shape-detection heuristics

| Function | Input | TypeDescriptor equivalent | Missing IR variant? |
|----------|-------|---------------------------|---------------------|
| `looksLikeBareTypeReference(type)` | `type: string` (ident regex) | `t.kind === "ref" && (!t.typeArguments \|\| t.typeArguments.length === 0)`. The dotted-namespace form (`A.B.C`) is preserved on `RefType.name` verbatim by the lowering. | NO |
| `looksLikeIndexedAccessType(type)` | `type: string` (regex `Foo[…]`) | `t.kind === "indexedAccess"` (per plan §11 item 4). | **YES — needs `IndexedAccessType` (plan §11 item 4)** |
| `looksLikeSlotsHelperRawType(rawType)` | `rawType: string \| undefined` (regex `…["slots"]` / `…['slots']` suffix) | `t.kind === "indexedAccess" && t.indexType.kind === "literal" && t.indexType.value === "slots"`. The "ends with `["slots"]`" recognition is structurally the indexed-access tail. | **YES — needs `IndexedAccessType` (same variant as above)** |
| `looksLikeUiHelperRawType(rawType)` | `rawType: string \| undefined` (regex `…["ui"]` / `…['ui']` suffix) | `t.kind === "indexedAccess" && t.indexType.kind === "literal" && t.indexType.value === "ui"`. Same dependency on indexed-access. | **YES — needs `IndexedAccessType` (same variant as above)** |
| `looksLikeStringCompatibleType(type)` | `type: string` | Walk descriptor for any branch satisfying `kind === "primitive" && (name === "string" \|\| name === "any")` OR `kind === "literal" && typeof value === "string"` OR a string-brand intersection arm (`(string & {})`, structurally `kind === "intersection" && one arm primitive("string") && other arm object({}))`. The "any" / string-literal / string-primitive branches are covered today; the `(string & {})` brand pattern is structurally an intersection of `primitive("string")` with `object({ properties: [] })`, which the existing kinds already express. | NO |
| `looksLikeEventNameParameter(param)` | `param: string` (regex `name: "literal"` parameter shape) | Walk the function-signature parameter: `param.type.kind === "literal" && typeof param.type.value === "string"`. The structural test is "parameter type is a string literal type". | NO |

## Normalisers

| Function | Input | TypeDescriptor equivalent | Missing IR variant? |
|----------|-------|---------------------------|---------------------|
| `normalizeTypeString(type)` | `type: string` | Display-only normaliser (`Array<T>` → `T[]`; single-quote → double-quote). In a typed pipeline the formatter `typeDescriptorToCompatDisplay` emits `T[]` for `array` kind directly, and emits double-quoted strings for `literal` kind directly via `JSON.stringify`. The function disappears. | NO |
| `normalizeOptionalCompatTypeText(type, required)` | `type: string`, `required: boolean` | When required: emit display of `descriptor`. When optional: union-join `descriptor` with `primitive("undefined")` if not already present. Test for "already includes `undefined`" is `t.kind === "union" && t.types.some(x => x.kind === "primitive" && x.name === "undefined")`. | NO |
| `normalizeCompatEventFunctionType(functionType)` | `functionType: string` (regex `((…) => …)` → `(…): …`) | Pure display reformatting of a `function` descriptor. `compatFunctionTypeToString` emits the desired `(params): returnType` form directly from `FunctionType`. The text-rewrite function disappears. | NO |
| `normalizeCompatObjectLiteralTypeText(typeText)` | `typeText: string` | Display-only re-rendering of an object literal type. `compatObjectTypeToString` emits the canonical `{ a: A; b: B; }` form from `ObjectType` directly. | NO |
| `normalizeCompatUnionArrayPart(part)` | `part: string` (`Array<T>` → `T[]`) | `array` kind already encodes the structural shape; the formatter chooses `T[]` rendering. Display-only — no structural replacement. | NO |
| `normalizeCompatSchemaLeaf(part)` | `part: string` (specific text values `(string & {})` → `string & {}`, `null` passthrough) | This is a leaf-text canonicaliser used inside the brand-union schema builder (`buildCompatStringBrandUnionPropMeta`). The structural inputs are `kind === "intersection"` arms of `primitive("string") & object({})`, plus `primitive("null")` and `literal` strings — the W7 rewrite produces these schema leaves directly from descriptors (no text canonicalisation step). | NO |
| `stripTopLevelUndefinedFromTypeString(type)` | `type: string` | `t.kind === "union" ? union(t.types.filter(x => !(x.kind === "primitive" && x.name === "undefined"))) : t`. | NO |
| `stripSingleOuterParens(type)` | `type: string` | Display-only paren stripping. The typed walk never has spurious outer parens: union/intersection/function/object kinds emit their canonical printed form. | NO |

## Extractors / formatters

| Function | Input | TypeDescriptor equivalent | Missing IR variant? |
|----------|-------|---------------------------|---------------------|
| `extractCompatSlotsFieldNames(type)` | `type: TypeDescriptor` | Already typed: walks `ObjectType.properties` (after `unwrapComponentSlotsDescriptor` peels a `ComponentSlots<…>` ref). No source-text manipulation. | NO |
| `extractCompatUiBindingFieldNames(type)` | `type: TypeDescriptor` | Already typed: filters `ObjectType.properties` whose `type.kind === "function"` and returns their names. No source-text manipulation. | NO |
| `extractEventTupleType(rawSignature)` | `rawSignature: string \| undefined` | Structural: when emit signature is a function (`kind === "function"`), drop the leading `event: "name"` parameter (`param.type.kind === "literal" && typeof value === "string"`) and emit the remaining parameters as a tuple type (`tuple(remaining.map(p => p.type))`). When emit signature is already a tuple, return as-is. The text-only path that scans `((…) => …)` strings goes away. | NO |
| `extractFunctionParameterSource(signature)` | `signature: string` | Walk `FunctionType.parameters` directly. The string-source slicer has no role in a typed pipeline. | NO |
| `formatCompatRawObjectType(body)` | `body: string` | Display formatter — emits `{ <body>; }` (with a trailing semicolon for index-signature bodies). In a typed pipeline `compatObjectTypeToString` emits the canonical form; the function disappears. | NO |
| `compatRawTypeLooksLossy(rawType)` | `rawType: string` | Heuristic: detects markdown fences (` ``` `), ellipses, comment fences, bare `object`. In the typed pipeline the only structural counterpart is `t.kind === "primitive" && t.name === "object"` (the bare-`object` case). The other markers (markdown / ellipsis / comments) are **display-text artefacts that cannot appear inside a typed `TypeDescriptor`** — once the rewrite consumes `prop.type` (typed) for semantic decisions, lossy-display detection is obsolete by construction. The function disappears as a semantic gate. | NO |
| `compatFunctionTypeToString(descriptor, …)` | `TypeDescriptor (kind=function)` | **Display formatter — no structural replacement required.** Already typed; called from `typeDescriptorToCompatDisplay`. Survives W7 unchanged. | NO |
| `compatObjectTypeToString(descriptor, …)` | `TypeDescriptor (kind=object)` | **Display formatter — no structural replacement required.** Already typed. Survives W7 unchanged. | NO |
| `compatTypeParameterToString(descriptor, …)` | `TypeDescriptor (kind=typeParameter)` | **Display formatter — no structural replacement required.** Already typed. Survives W7 unchanged. | NO |
| `compatSlotBindingTypeText(binding, typeRegistry)` | `binding: SlotMeta["bindings"][number]`, registry | Currently dispatches on `looksLikeUiHelperRawType(binding.rawType)` (text). Structural replacement: dispatch on `binding.type.kind === "indexedAccess" && index === literal("ui")` (per `looksLikeUiHelperRawType` row above). Otherwise call `typeDescriptorToCompatDisplay(binding.type, typeRegistry)` directly. | **YES — same `IndexedAccessType` dependency** |

## Repair / preference

| Function | Input | TypeDescriptor equivalent | Missing IR variant? |
|----------|-------|---------------------------|---------------------|
| `repairOpaqueCompatSchemaFromRawType(schema, rawType)` | schema, `rawType: string` | The "opaque object" detector is structural already (`schema.kind === "object" && Object.keys(schema.schema).length === 0`); the repair path then **reparses `rawType`** to recover structural detail. In the typed pipeline the descriptor `prop.type` IS the structural detail, so the repair-from-text path is replaced by "if descriptor has structure, render it; otherwise return the opaque schema unchanged". The function disappears as a semantic gate. | NO |
| `buildCompatSchemaFromRawType(rawType)` | `rawType: string` | Recursive text-shape parser that detects union → intersection → object literal in raw text and emits `PropertyMetaSchema`. Structural replacement: walk `TypeDescriptor` via existing `typeDescriptorToSchema` (already exists in the layer above this file), which dispatches on `kind` natively. The function and its three call sites disappear. | NO |
| `buildCompatObjectSchemaFromRawType(rawType)` | `rawType: string` | Sub-routine of above. Walks an object-literal body via `splitTopLevelObjectMembers` + a regex member matcher. Structural replacement: walk `ObjectType.properties` directly. Disappears. | NO |
| `buildCompatIntersectionArmSchema(rawType)` | `rawType: string` | Sub-routine that decides per-arm shape from text. Structural: dispatch on each `IntersectionType.types[i].kind`. Disappears. | NO |
| `preferredCompatPropTypeText(prop, registry)` | `prop: PropMeta` (uses `prop.rawType` + `prop.type`) | Currently picks between rendered `prop.type` text and `prop.rawType` text by running `compatRawTypeLooksLossy`, `shouldPreferRawAliasForExpandedDescriptor`, `shouldPreferDescriptorForProp`. Per plan §3.5, every semantic decision drives off `prop.type`; `rawType` survives as display passthrough only. The selection collapses to `typeDescriptorToCompatDisplay(prop.type, registry)` modulo the descriptor-display preference rules retained for parity. | NO |
| `preferredCompatTypeText(rawType, descriptor, registry)` | `rawType: string \| undefined`, `descriptor: TypeDescriptor` | Same as above, narrower signature. Same conclusion: collapses to display of `descriptor`. | NO |
| `shouldPreferRawSchemaType(rawType, currentType)` | both `string` | Heuristic comparing two rendered strings. In the typed pipeline the "do we have generic args" / "is it indexed access" / "is it a bare ref" tests become `descriptor.kind === "ref" && descriptor.typeArguments?.length > 0` / `descriptor.kind === "indexedAccess"` / `descriptor.kind === "ref"`. Indexed-access dependency. | **YES — `IndexedAccessType` dependency** |
| `shouldPreferRawAliasForExpandedDescriptor(rawType, descriptor)` | `rawType: string`, `descriptor: TypeDescriptor` | The `rawType` arg is checked via `looksLikeBareTypeReference`. Structural rephrasing: "if the prop's annotation expression was a bare `Ref` (carry that on `prop.type` / a sibling typed annotation, not on `rawType`) and the expanded descriptor is a literal-only union, prefer the alias text." The first check is structural (`kind === "ref" && no typeArguments`); the rest of the body is already structural. Once `prop.type` carries the annotation-shape (W0.3 invariant: `type_expr` is the syntactic shape, NOT pre-expanded), the function reads `prop.type.kind === "ref"` directly. | NO |
| `applyRawTypeDisplayHintsToSchema(schema, rawType)` | schema, `rawType: string` | Dispatches into the inner walker. Display-only refinement; in the typed pipeline the schema walker can route descriptor → schema directly without consulting raw text. Disappears. | NO |
| `applyRawTypeDisplayHintsToSchemaInner(schema, rawType)` | schema, `rawType: string` | Walks schema arms in lock-step with raw-text union/intersection arms split by `splitTopLevelTypeUnion` / `splitTopLevelTypeIntersection`. Structural replacement: walk schema arms in lock-step with `descriptor.types[i]`. Disappears (the lock-step walk happens at the typed-schema construction site, not as a post-hoc text refinement). | NO |
| `evaluateDefault(val)` (the `rawType` branch) | `val: string \| undefined` | Pattern-matches default-value expression text (`() => ({})`, `() => []`, single-quoted string literal). **Out of scope per plan §11 trailer** — this is expression text, not type text. A `TODO(typed-default-values)` is left in place, with `no_role_inference_from_name_suffix` allowlist exemption in W0.4. NOT flagged as a missing IR variant. | NO (out of scope) |

## Summary

- **Total functions audited:** 40 (every function from the W0.9 prompt's §4.8 + §8 list, plus `buildCompatIntersectionArmSchema` as a sub-routine of `buildCompatSchemaFromRawType` included for completeness).
- **Functions where the existing `@verter/type-ir` kinds already cover the predicate:** 35.
- **Functions where the structural predicate requires a new IR variant:** 5 (`looksLikeIndexedAccessType`, `looksLikeSlotsHelperRawType`, `looksLikeUiHelperRawType`, `compatSlotBindingTypeText`, `shouldPreferRawSchemaType` — all five depend on the same single missing variant).
- **Functions where the work is purely display-only (no semantic decision; no IR variant):** 7 (`compatFunctionTypeToString`, `compatObjectTypeToString`, `compatTypeParameterToString`, `formatCompatRawObjectType`, `normalizeTypeString`, `normalizeCompatObjectLiteralTypeText`, `stripSingleOuterParens`). These survive in W7 unchanged.
- **Functions out of scope per plan §11 trailer:** 1 (`evaluateDefault` — expression-text parsing, follow-up plan).

### Specific new variants required for `@verter/type-ir`

Exactly **one** new kind discriminator is required to unblock W7:

- **`IndexedAccessType`** (plan §11 item 4) — represents `T['K']` / `T[K]` shapes. Needed by:
  - `looksLikeIndexedAccessType` (direct kind match on `t.kind === "indexedAccess"`).
  - `looksLikeSlotsHelperRawType` (matches `T['slots']` tail).
  - `looksLikeUiHelperRawType` (matches `T['ui']` tail).
  - `compatSlotBindingTypeText` (dispatches via the `looksLikeUiHelperRawType` predicate).
  - `shouldPreferRawSchemaType` (the `looksLikeIndexedAccessType` branch in its body).

  Suggested shape (W0.6 owns the precise definition):

  ```ts
  export interface IndexedAccessType {
    kind: "indexedAccess";
    objectType: TypeDescriptor;
    indexType: TypeDescriptor;
  }
  ```

  Lowering: native `verter_type_expr::TypeExpr::IndexedAccess { object, index }`
  → `{ kind: "indexedAccess", objectType: lower(object), indexType: lower(index) }`.
  No `recursiveRef` interaction is required at this stage — index-type shapes
  used in practice in compat (`T['slots']`, `T['ui']`, `T['type']`) all resolve
  through `literal` index types.

No other variants are flagged. Conditional types, mapped types, `keyof`,
`typeof`, and template-literal types do not appear in any heuristic in the
W7 scope: their structural shapes either resolve to `ref` / `union` /
`object` / `unknown` before reaching the compat layer (because the
resolver materialises them upstream) or are handled as opaque `unknown`
when the resolver cannot resolve them. They remain candidates for
future IR additions but are NOT required to unblock W7 against the
checker.ts heuristic surface enumerated above.

### W7 dispatch gating

W7 cannot dispatch until W0.6 covers the YES rows. Concretely: after W0.6
adds `IndexedAccessType`, every `looksLike*` and `extract*` and `prefer*`
branch above has a structural replacement, and `prop.rawType` can be
removed from every semantic-decision branch in `checker.ts` (plan §4.8 H.3).

If W0.6 chooses the alternative (plan §11 item 4 STOP path) and collapses
indexed-access on the native side before reaching JS, every YES row
above instead becomes a NO — the descriptor reaching JS is whatever the
collapsed form produced (e.g. an inlined member type), and the `looksLike*`
helpers that key off the indexed-access shape lose their reason to exist.
Either resolution unblocks W7.
