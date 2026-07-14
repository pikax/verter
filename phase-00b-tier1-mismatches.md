# Phase 0b — Tier-1 mismatches deferred to a later phase

Per §0p.A.0 author-first discipline + §0p.A.4 case 2, the worker
authored a hand-derived `expected SnapshotView` for each Class A
property fixture, then ran the harness to compare against Verter's
current output. Two of the seven brief-listed property fixtures
produce output that does NOT match the rule-correct expected. Per
the brief ("do NOT capture Verter's broken output as the baseline"),
those fixtures are DEFERRED — exactly the same mechanism Phase 0a
used for its 5 utility-type fixtures (per Phase 0a's
`phase-00-tier1-mismatches.md`).

## Deferred fixture 1 — `fixture_slots_typed`

**Rule citation:** Verter macros §slots
(`./.claude/skills/component-meta`) — "defineSlots<T> must surface
every key of T as a slot, with bindings extracted from each slot
function's first parameter".

**SFC source:**

```vue
<script setup lang="ts">
defineSlots<{
  default(props: { item: string }): any;
  named(props: { row: number }): any;
}>();
</script>
<template><div /></template>
```

**Rule-correct expected (programmatic SnapshotView form):**

```rust
SnapshotView {
    component_name: "C".to_string(),
    slots: vec![
        SlotView {
            name: "default".to_string(),
            payload_signature: "{ item: string }".to_string(),
        },
        SlotView {
            name: "named".to_string(),
            payload_signature: "{ row: number }".to_string(),
        },
    ],
    // props/events/models/exposed/fallthrough empty/None.
}
```

**Verter's current actual:**

```json
{
  "slots": [
    { "name": "default", "payload_signature": "{ item: /*unknown*/ semanticMiss }" },
    { "name": "named",   "payload_signature": "{ row: /*unknown*/ semanticMiss }" }
  ]
}
```

**Root cause:** the binding type inside the slot function's
parameter object literal (`item: string`, `row: number`) is not
resolved through the `defineSlots<T>` macro path. The slot NAME is
extracted, the binding NAME is extracted, but the binding's
`TypeExpr` lowers to `Unknown { raw: "semanticMiss" }` instead of
`Primitive(String)` / `Primitive(Number)`.

**Owner phase:** later phase that reaches the slot binding type
resolution path. The fixture should be authored as Class A with the
above rule-correct expected once the resolver fix lands.

### Rule-correct expected (machine-readable per §5.B.5.1 r15)

Hand-authored rule-correct `SnapshotView` for `fixture_slots_typed`,
derived from Verter macros §slots (`./.claude/skills/component-meta`):
"defineSlots<T> must surface every key of T as a slot, with bindings
extracted from each slot function's first parameter". The Phase 5j
§5.B.5.1 rule-correctness gate test
(`deferred_fixture_fixture_slots_typed_byte_equal_to_rule_correct_expected`
in `crates/verter_session/tests/correctness/deferred_fixtures_rule_correct.rs`)
asserts byte-equality between this block and Verter's
post-Phase-5j-fix output. Discrimination: pre-fix Verter produces
`{ item: /*unknown*/ semanticMiss }` / `{ row: /*unknown*/ semanticMiss }`
for the slot payload signatures; post-fix Verter produces
`{ item: string }` / `{ row: number }` because
`ProjectSemanticDispatch::project_slot_binding_member` descends
through the slot's `Function.params[0].ty` to the binding Object.
The `SnapshotView`'s slot list sorts alphabetically by name, so
`default` precedes `named`; bindings within each slot's
`payload_signature` are also sorted.

```json
{
  "fixture_id": "fixture_slots_typed",
  "expected": {
    "component_name": "C",
    "props": [],
    "events": [],
    "slots": [
      {
        "name": "default",
        "payload_signature": "{ item: string }"
      },
      {
        "name": "named",
        "payload_signature": "{ row: number }"
      }
    ],
    "models": [],
    "exposed": [],
    "fallthrough": null,
    "flags": {
      "async_setup": false,
      "has_inherit_attrs_false": false
    }
  }
}
```

## Deferred fixture 2 — `fixture_models`

**Rule citation:** Verter macros §model
(`./.claude/skills/component-meta`) — "defineModel<T>() exposes a
model entry per call, with name from the optional first string
argument (or 'modelValue' default) and type from the type
parameter T".

**SFC source:**

```vue
<script setup lang="ts">
defineModel<string>();
defineModel<number>('count');
</script>
<template><div /></template>
```

**Rule-correct expected (programmatic SnapshotView form):**

```rust
SnapshotView {
    component_name: "C".to_string(),
    models: vec![
        ModelView { name: "count".to_string(),      type_signature: "number".to_string() },
        ModelView { name: "modelValue".to_string(), type_signature: "string".to_string() },
    ],
    // Verter additionally surfaces matching props + update:<name>
    // events per its documented "defineModel ALSO emits prop+event"
    // contract; the rule-correct expected for THAT shape can be
    // re-derived once the type resolution defect below is fixed.
}
```

**Verter's current actual:**

```json
{
  "props": [
    { "name": "count",      "type_signature": "/*unknown*/ semanticMiss", ... },
    { "name": "modelValue", "type_signature": "/*unknown*/ semanticMiss", ... }
  ],
  "events": [
    { "name": "update:count",      "params_signature": "[value: number | undefined]" },
    { "name": "update:modelValue", "params_signature": "[value: string | undefined]" }
  ],
  "models": [
    { "name": "count",      "type_signature": "/*unknown*/ semanticMiss" },
    { "name": "modelValue", "type_signature": "/*unknown*/ semanticMiss" }
  ]
}
```

**Root cause:** the `defineModel<T>()` type parameter T is captured
as `AnalyzedPropField::type_annotation` (see
`crates/verter_semantic/src/analysis/macros.rs::extract_define_model_type`)
but the macro's downstream lowering fails to resolve T from text to a
proper `TypeExpr::Primitive(...)`. Note that the `update:<name>`
event payload IS resolved (as `[value: string | undefined]`),
suggesting the resolver knows T's type — but the model entry's
`type_expr` is filled from a different code path that doesn't
re-use that resolution.

**Owner phase:** later phase that reaches `defineModel<T>` type
resolution. The fixture should be authored as Class A with the
above rule-correct expected once the resolver fix lands.

### Rule-correct expected (machine-readable per §5.B.5.1 r15)

Hand-authored rule-correct `SnapshotView` for `fixture_models`,
derived from Verter macros §model (`./.claude/skills/component-meta`):
"defineModel<T>() exposes a model entry per call, with name from the
optional first string argument (or 'modelValue' default) and type
from the type parameter T". Vue's documented `defineModel<T>()`
contract additionally:
- emits a corresponding `<model_name>` prop. The NATIVE snapshot keeps
  the prop type BARE `T` plus the typed flags `required: false`,
  `has_default: false` when the model is optional (default — no
  `{ required: true }` option) and not defaulted; the `T | undefined`
  optional-model display is a compat/Volar-interop projection derived
  from `required` in
  `packages/component-meta/src/compat/checker.ts`, not native truth.
- emits an `update:<model_name>` event whose display payload tuple is
  `[value: T | undefined]` for an optional, undefaulted model.

The `SnapshotView` projection sorts every collection alphabetically by
name, so `count` precedes `modelValue` in props/models/events. The
Phase 5j §5.B.5.1 rule-correctness gate test
(`deferred_fixture_fixture_models_byte_equal_to_rule_correct_expected`
in `crates/verter_session/tests/correctness/deferred_fixtures_rule_correct.rs`)
asserts byte-equality between this block and Verter's post-Phase-5j-fix
output. Discrimination: pre-fix Verter produces
`/*unknown*/ semanticMiss` for the model `type_expr` and the prop
`type_signature`; post-fix Verter produces the BARE `string` /
`number` for BOTH the model `type_expr` and the prop
`type_signature` (the native snapshot renders the published bare
carrier), because the `expand_field_expr` closure
routes `DefineModel` macros through a direct lower+raise of the
macro's `parsed_type_argument` rather than the path-projection arm
that always missed for primitive-leaf type arguments.

```json
{
  "fixture_id": "fixture_models",
  "expected": {
    "component_name": "C",
    "props": [
      {
        "name": "count",
        "type_signature": "number",
        "required": false,
        "has_default": false,
        "default_signature": null,
        "doc": null
      },
      {
        "name": "modelValue",
        "type_signature": "string",
        "required": false,
        "has_default": false,
        "default_signature": null,
        "doc": null
      }
    ],
    "events": [
      {
        "name": "update:count",
        "params_signature": "[value: number | undefined]"
      },
      {
        "name": "update:modelValue",
        "params_signature": "[value: string | undefined]"
      }
    ],
    "slots": [],
    "models": [
      {
        "name": "count",
        "type_signature": "number"
      },
      {
        "name": "modelValue",
        "type_signature": "string"
      }
    ],
    "exposed": [],
    "fallthrough": null,
    "flags": {
      "async_setup": false,
      "has_inherit_attrs_false": false
    }
  }
}
```

## Discriminating-test impact

Two of the 12 §0p.A.5 discriminating-test rows reference these
fixtures:

- `MutationKind::SlotDropped` on `fixture_slots_typed` — SKIPS
- `MutationKind::SlotPayloadChanged` on `fixture_slots_typed` — SKIPS
- `MutationKind::ModelDropped` on `fixture_models` — SKIPS

The skips are mechanical (the §0p.A.5 r5/M6 fix uses
`fixtures::FIXTURES.iter().find(|f| f.id == case.fixture_id)` and
short-circuits on `None`), so the parametric test still PASSES — it
just covers fewer rows than the brief-target 12. The rows for the 2
deferred fixtures will become live when those fixtures are authored
in a later phase.

After Phase 0b lands, 9 of the 12 rows run + pass:

- `mapped_pick_two_keys` — PropExtraKey, PropTypeChanged (Phase 0a)
- `mapped_omit_two_keys` — PropMissingKey (Phase 0a)
- `fixture_props_with_defaults` — PropDefaultDropped (Phase 0b)
- `mapped_required` — PropRequiredFlipped (Phase 0a)
- `fixture_events_typed` — EventSignatureChanged (Phase 0b)
- `fixture_exposed_methods` — ExposedDropped (Phase 0b)
- `fixture_fallthrough_inherit` — FallthroughInheritFlipped (Phase 0b)
- `fixture_fallthrough_root_inherit` — FallthroughSurfaceChanged (Phase 0b)

The 3 skipped rows (`SlotDropped`, `SlotPayloadChanged`,
`ModelDropped`) are tracked here for the follow-up phase.
