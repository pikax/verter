# Fact Matrix — Cross-consumer × fact-kind

Cross-consumer × fact-kind matrix slices for every cache-bearing
consumer in `verter_session`. Block 1.H landed the first 25 slices
for the Family B/C/D caches; Block 1.8 landed the second 25 for the
Family A caches and added the completeness arch guard.

## Layout

Each file `<consumer>_<fact_kind>.rs` characterises one matrix cell:
the producer for `<consumer>` and a representative fixture / artefact
that exercises (or deterministically doesn't exercise) the
`<fact_kind>` observation.

The arch guard
`tests/cross_consumer_fact_matrix_complete.rs` enforces that every
consumer × fact-kind cell on the 10 × 5 grid has a slice file on
disk under this directory.

## Caches (10)

Block 1.H (counter-delta discriminator on producer tracer installs):

- `materialize_structure` — `MaterializeStructureDb`
- `ref_cycle` — `RefCycleResultDb`
- `memo_entry` — `MemoEntry` (`SemanticGraphStore::execute_cooperative`)
- `app_config_proof` — `AppConfigNoOverrideProofDb`
- `owner_import_surface` — `OwnerImportSurfaceDb`

Block 1.8 (substrate-validation / fact-tracer fan-out discriminator):

- `compile_tier` — `CompileSlot.fact_dep_signature`
- `component_meta` — `ComponentMetaResultEntry.read_set_signature`
- `fallthrough` — `CachedFallthroughEntry.fact_versions`
- `route_surface` — `BarrelRouteSurface.fact_dep_signature` /
  `EffectiveExportSetEntry.fact_dep_signature`
- `slot_binding_graph` — request fact-tracer fan-out via the
  helper (`emit_slot_binding_graph_dispatch_facts`)

## Fact-kinds (5)

- `member_presence` — Family A path-precise member presence facts
- `member` — Family A path-precise member body facts
- `import_ref` — Parse-domain import-reference facts
- `route_surface` — RouteDb-owned route-surface facts
- `module_augmentation_index_shape` — module-augmentation index facts

## Discrimination rule

Each slice is a `#[test]` that either:

1. Asserts the producer's tracer DOES observe a fact of the given
   kind for the representative fixture (Block 1.H counter-delta
   discriminator), OR
2. Asserts the consumer's `fact_dep_signature` / `read_set_signature`
   substrate can carry that fact-kind end-to-end under the permissive
   view's per-domain dispatcher (Block 1.8 substrate-validation
   discriminator), OR
3. Asserts the fan-out substrate carries the fact-kind into every
   active tracer scope (Block 1.8 slot_binding_graph variant), OR
4. Asserts the producer's tracer NEVER observes that fact-kind
   (documented degenerate cells — the producer architecturally
   cannot produce that fact-kind).

No empty bodies. No `assert!(true)`.

## Block 1.8 completeness arch guard

`tests/cross_consumer_fact_matrix_complete.rs` walks the 10 × 5 grid
and fails when any cell's slice file is missing. Adding a new
cache-bearing consumer therefore obligates adding its 5 fact-kind
slices; otherwise the arch guard fires at workspace test time.
