# Fact Matrix — Block 1.H

Cross-consumer × fact-kind matrix slices for the Family B/C/D
caches wired by Block 1.H.

## Layout

Each file `<cache>_<fact_kind>.rs` characterises one matrix cell:
the producer for `<cache>` and a representative fixture that
exercises (or deterministically doesn't exercise) the
`<fact_kind>` observation.

## Caches (5)

- `materialize_structure` — `MaterializeStructureDb`
- `ref_cycle` — `RefCycleResultDb`
- `memo_entry` — `MemoEntry` (`SemanticGraphStore::execute_cooperative`)
- `app_config_proof` — `AppConfigNoOverrideProofDb`
- `owner_import_surface` — `OwnerImportSurfaceDb`

## Fact-kinds (5)

- `member_presence` — Family A path-precise member presence facts
- `member` — Family A path-precise member body facts
- `import_ref` — Parse-domain import-reference facts
- `route_surface` — RouteDb-owned route-surface facts
- `module_augmentation_index_shape` — module-augmentation index facts

## Block 1.8 follow-up

The Block 1.8 `REQUIRED_CONSUMERS` arch guard currently lists:
`compile_tier`, `component_meta`, `fallthrough`, `ref_cycle`,
`materialise`, `route_surface`, `slot_binding_graph`.

The new Block 1.H caches map as:
- `materialize_structure` → `materialise` (already in list)
- `ref_cycle` → `ref_cycle` (already)
- `memo_entry` → NEW — TODO(1.8): add to `REQUIRED_CONSUMERS`
- `app_config_proof` → NEW — TODO(1.8): add to `REQUIRED_CONSUMERS`
- `owner_import_surface` → NEW — TODO(1.8): add to `REQUIRED_CONSUMERS`

## Discrimination rule

Each slice is a `#[test]` that either:

1. Asserts the producer's tracer DOES observe a fact of the given
   kind for the representative fixture, OR
2. Asserts the producer's tracer NEVER observes that fact-kind
   (documented degenerate cells — the producer architecturally
   cannot produce that fact-kind).

No empty bodies. No `assert!(true)`.
