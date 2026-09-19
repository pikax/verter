# ARH2 evidence cases

The sole owning interface is `node tests/architecture-health/ARH2/verify.mjs` (verify) plus `node --test tests/architecture-health/ARH2/arh2.test.mjs` (dirty twins), wired into `test:scripts` and into the dedicated CI `architecture-health` lane (gated on `tests/architecture-health/**` and `crates/**`; that lane also runs `verify.mjs --provenance` on a full-history checkout to prove the pinned candidate is a real ancestor commit). The verifier joins the shipped ARH0/ARH1 predecessor products (`../ARH0/products/*`, `../ARH1/products/*`) and the live tree only, and RE-DERIVES the predecessor populations on every execution by running the shipped ARH0 and ARH1 `validate()` implementations (imported from their verifiers, never re-implemented here); the program DAG is database-owned by the TAMA controller, so owner/heir ids are checked structurally only and no DAG copy is read from this tree.

Behavioral evidence: every pinned lane is a real `cargo nextest run` command over a live workspace member crate, and every witness is a real `#[test]`/`#[tokio::test]` function of a real file whose compiled nextest id (from `mod` / `#[path]` declarations, never a filesystem `src::` fragment) contains the filter. The architecture-health CI lane is Node-only and does not spawn cargo; the lanes remain the targeted-domain gate commands. Their outcome is pass/fail only — cost belongs to the measurements product, never to a pin.

## ARH2-ratification (accept)

Clean products validate: both products pin the same 40-hex candidate basis, the manifest records exactly the cases the verifier implements with disposition reject and the `clean products` accept twin, and the verify/test commands are the canonical strings derived from the verifier's own location.

## ARH2-ratification (reject)

- `manifest-case-drift` / `manifest-product-drift` / `manifest-command-drift`: dirty twins add `ARH2-phantom`, drop `ARH2-separation`, and point verify at `scripts/affected-tests.mjs` (an existing script that is not this verifier).
- `candidate-basis-drift`: both products must pin the same 40-hex commit (dirty twin pins `0000…`); the CI lane's `--provenance` run additionally proves the pinned commit is an ancestor of HEAD.

## ARH2-population (reject) — AC1 live re-derivation

- `predecessor-drift`: the shipped ARH0/ARH1 verifiers run inside this one on every execution; a drifted inventory, importer or consumer population fails here together with the drifted predecessor.
- `hotspot-population-drift`: the characterized hotspots are exactly the ARH1 hotspot contracts, which are exactly the ARH0 god-module candidates, both directions (dirty twin drops the `build.rs` hotspot).
- `responsibility-dropped` / `responsibility-invented`: every ARH1 authority responsibility of every hotspot carries exactly one characterization row, and no characterization row claims a responsibility ARH1 does not carry (dirty twins drop `batch coordination` and invent `template codegen`).
- `population-count-drift`: committed population counts equal the live derivations — ARH0 inventory crate/package rows, ARH1 hotspot contracts, ARH0 god-module candidates (dirty twin bumps `arh0Inventory.packages`).
- `structural-population-drift` / `structural-file-missing` / `structural-loc-drift`: one structural row per characterized hotspot whose `fileLoc` equals the live line count with Rust `lines()` semantics (dirty twin increments `semantic_query.rs` by one line).

## ARH2-deletion (reject) — AC1 executed deletion (ARH1-CUT-1)

- `deletion-not-executed`: the deleted path must be absent from the tree (dirty twin retargets `deletedPath` at `crates/verter_scheduler`, which exists).
- `deletion-inventory-row-stale` / `deletion-debt-row-stale` / `deletion-cutover-row-stale`: no shipped product may still bind the deleted path in a path-typed field (checked against the refreshed ARH0/ARH1 products).
- `deletion-debt-unknown` / `deletion-cutover-unknown` / `deletion-key-mismatch` / `deletion-owner-mismatch`: the deletion binds a real ARH0 debt key and a real ARH1 cutover row whose executing owner is ARH2 and whose satisfied debt matches (dirty twins retarget `satisfies` at `ARH0-DEBT-99` and `cutoverRow` at `ARH1-CUT-2`).
- `deletion-refresh-path-missing` / `deletion-rationale-missing`: the same-change refresh list binds real files and the rationale is non-empty.

## ARH2-characterization (reject) — AC2 discriminating pins

- `hotspot-missing` / `hotspot-crate-unknown` / `hotspot-without-pins`: every characterized hotspot exists, names a live workspace member crate (glob members expanded against `Cargo.toml`-carrying directories) and carries at least one pin.
- `pin-command-not-canonical` / `pin-filter-malformed` / `pin-without-witnesses`: a pin's command is exactly `cargo nextest run -p <crate> <filter>` with a non-flag filter (dirty twin drops `run`).
- `witness-file-missing` / `witness-test-missing` / `witness-not-a-test`: a witness file exists, declares the named function, and a `#[test]`/`#[tokio::test]` attribute sits within the few lines above the declaration (dirty twins name a phantom test and a non-test function).
- `pin-filter-selects-nothing`: the filter is a substring of the compiled nextest id from `mod`/`#[path]` (dirty twins swap the filter to `unrelated_module` and to filesystem `src::dag_tests`, which selects no compiled module).
- `route-characterization-cardinality` / `route-characterization-invented`: every ARH1 cutover row owned by a narrowing heir (ARH3/ARH4) is characterized exactly once before its heir narrows it, and no invented route rides along (dirty twins drop `ARH1-CUT-3` and duplicate `ARH1-CUT-4`).
- `route-path-mismatch` / `route-owner-mismatch` / `route-without-pins`: a route row binds its register row's candidate path and heir.
- `route-surface-drift` / `route-surface-not-live`: the narrowed surface is characterized as it exists BEFORE narrowing — the ARH1-CUT-2 field items join the ARH1 field narrowing population exactly and each is still a live `pub` field, the live `pub fn test_*` population of `scheduler.rs` joins the ARH1 test-configuration rows, and ARH1-CUT-4 restates the bulk retained types, retained assoc items and consumer population exactly (dirty twins add a fourth field name, drop `HashValue`, drop a bulk consumer, and replace the CUT-4 surface with `{kind:"invented"}`).
- `ac3-concern-missing` / `ac3-concern-without-evidence` / `ac3-concern-invented`: every charter AC3 concern (fresh-versus-incremental equivalence, edit/revert, cancellation, stale/partial rejection, deterministic ordering under perturbed discovery or scheduling) carries existing named evidence and nothing else rides the block (dirty twins drop `edit/revert` and point cancellation evidence at a missing file).

## ARH2-separation (reject) — AC5 methodology-bound measurements

- `dimension-cardinality` / `dimension-invented` / `dimension-without-separation` / `dimension-without-measures`: the four charter dimensions (production behavior, clean/warm build time, test cost, application latency) appear exactly once each, every one declaring what it measures and separates from (dirty twin drops `application-latency`).
- `dimension-commits-wall-clock`: a dimension row may never carry a wall-clock/RSS/speedup number — measured numbers live in gate receipts under the ratified methodology (dirty twin adds `wallNs`).
- `mechanism-binding-missing` / `dimension-mechanism-kind-mismatch`: every dimension has a nonempty mechanisms array of the required kind (dirty twins delete or empty all mechanisms, and bind test-cost to BF2 gate cells).
- `gate-cell-unknown` / `gate-cell-wrong-operation`: every gate-cell mechanism is a locked cell of `performance-gates.toml`, and application-latency must cite a host/session cell (dirty twins cite `B6_INVENTED_CELL` and `B6_COMPILER_ROUTE_OVERHEAD`).
- `dimension-cache-state-incomplete`: clean/warm build time binds cargo `--timings` recipes for both cache states (dirty twin drops the warm leg).
- `behavior-lane-unbound` / `behavior-lane-invented` / `test-cost-lane-unbound` / `test-cost-lane-invented`: the behavior and test-cost lane lists are exactly the characterization product's pinned command set, both directions (dirty twins splice out `stable_key_tests` and append a phantom lane).
- `runner-class-unbound`: the number policy equals the locked `[runner]` class of `performance-gates.toml` (dirty twins delete it or suffix-forge it).
- `threshold-block-missing` / `threshold-guard-missing` / `threshold-guard-unknown` / `threshold-ceiling-mismatch`: the ratified god-module threshold binds the existing `god_module_size_budget` guard and its live `DEFAULT_MAX_LINES` ceiling (ARH0-DEBT-5's precondition for ARH12; dirty twin detaches the ceiling).
- `threshold-basis-drift`: the over-threshold production-file count (same classification as the guard: `.rs` under `crates/*/src`, test fixtures excluded) and the hotspot membership are re-derived live on every run (dirty twin records 32 instead of the live 33).
