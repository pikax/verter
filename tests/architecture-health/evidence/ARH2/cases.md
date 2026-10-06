# ARH2 evidence cases

The sole owning interface is `node tests/architecture-health/ARH2/verify.mjs` (verify) plus `node --test tests/architecture-health/ARH2/arh2.test.mjs` (dirty twins), wired into `test:scripts` and into the dedicated CI `architecture-health` lane (gated on `tests/architecture-health/**`, `crates/**`, `performance-gates.toml` and `scripts/validate-performance-gates.mjs`; no Git history is required). The verifier joins the shipped ARH0/ARH1 predecessor products (`../ARH0/products/*`, `../ARH1/products/*`) and the live tree only, and RE-DERIVES the predecessor populations on every execution by running the shipped ARH0 and ARH1 `validate()` implementations (imported from their verifiers, never re-implemented here); the program DAG is database-owned by the TAMA controller, so owner/heir ids are checked structurally only and no DAG copy is read from this tree.

Behavioral evidence: every pinned lane is a real `cargo nextest run` command over a live workspace member crate, and every witness is a real `#[test]`/`#[tokio::test]` function of a real file whose compiled nextest id (the file's `mod` / `#[path]` path plus that function's enclosing inline module, never a filesystem `src::` fragment or a sibling-inline cross-product) contains the filter. The architecture-health CI lane is Node-only and does not spawn cargo; the lanes remain the targeted-domain gate commands. Their outcome is pass/fail only — cost belongs to the measurements product, never to a pin.

## ARH2-ratification (accept)

Clean products validate: the manifest records exactly the cases the verifier implements with disposition reject and the `clean products` accept twin, and the verify/test commands are the canonical strings derived from the verifier's own location.

Historical source titles and ISO dates are optional descriptive context. Tests exercise the verifier without commit metadata and run its CLI with Git history unavailable. Code, manifest and behavioral obligations remain enforced.

## ARH2-ratification (reject)

- `manifest-case-drift` / `manifest-product-drift` / `manifest-command-drift`: dirty twins add `ARH2-phantom`, drop `ARH2-separation`, and point verify at `scripts/affected-tests.mjs` (an existing script that is not this verifier).
- CI `arch` filter: `performance-gates.toml` and `scripts/validate-performance-gates.mjs` must select the architecture-health lane (ARH2 reads both as measurement-methodology inputs).

## ARH2-population (reject) — AC1 live re-derivation

File size and item counts are not architecture evidence: no line count, size threshold or population count is recorded or compared.


- `predecessor-drift`: the shipped ARH0/ARH1 verifiers run inside this one on every execution; a drifted inventory, importer or consumer population fails here together with the drifted predecessor.
- `hotspot-population-drift`: the characterized hotspots are exactly the ARH1 hotspot contracts, which are exactly the ARH0 god-module candidates, both directions (dirty twin drops the `build.rs` hotspot).
- `responsibility-dropped` / `responsibility-invented`: every ARH1 authority responsibility of every hotspot carries exactly one characterization row, and no characterization row claims a responsibility ARH1 does not carry (dirty twins drop `batch coordination` and invent `template codegen`).

## ARH2-deletion (reject) — AC1 executed deletion (ARH1-CUT-1)

- `deletion-not-executed`: the deleted path must be absent from the tree (dirty twin retargets `deletedPath` at `crates/verter_scheduler`, which exists).
- `deletion-path-unbound`: `deletedPath` equals the ARH0-DEBT-1 `candidate` path with the `(deleted)` marker stripped (dirty twin retargets at `packages/definitely-never-existed`, which is absent but is not the executed deletion).
- `deletion-inventory-row-stale` / `deletion-debt-row-stale` / `deletion-cutover-row-stale`: no shipped product may still bind the deleted path in a path-typed field (checked against the refreshed ARH0/ARH1 products).
- `deletion-debt-unknown` / `deletion-cutover-unknown` / `deletion-key-mismatch` / `deletion-owner-mismatch`: the deletion binds a real ARH0 debt key and a real ARH1 cutover row whose executing owner is ARH2 and whose satisfied debt matches (dirty twins retarget `satisfies` at `ARH0-DEBT-99` and `cutoverRow` at `ARH1-CUT-2`).
- `deletion-refresh-empty` / `deletion-refresh-population-drift` / `deletion-refresh-path-missing` / `deletion-rationale-missing`: the same-change refresh list is exactly the nonempty predecessor refresh population, every path exists, and the rationale is non-empty (dirty twin clears `sameChangeRefresh`).

## ARH2-characterization (reject) — AC2 discriminating pins

- `hotspot-missing` / `hotspot-crate-unknown` / `hotspot-without-pins`: every characterized hotspot exists, names a live workspace member crate (glob members expanded against `Cargo.toml`-carrying directories) and carries at least one pin; a pin whose witnesses compile in another package names that package as its own `crate`, which must be a live member too.
- `pin-command-not-canonical` / `pin-filter-malformed` / `pin-without-witnesses`: a pin's command is exactly `cargo nextest run -p <crate> <filter>` with a non-flag filter (dirty twin drops `run`).
- `witness-file-missing` / `witness-test-missing` / `witness-not-a-test`: a witness file exists, declares the named function, and a `#[test]`/`#[tokio::test]` attribute sits within the few lines above the declaration (dirty twins name a phantom test and a non-test function).
- `witness-ignored` / `witness-cfg-disabled`: a pinned witness must be eligible to execute under the retained nextest recipe. Same-line `#[ignore]` and `#[cfg(any())]` overlays on `vue_script_setup_functions_serve_under_the_instance_owner_only` are the discriminating twins; the canonical recipe has no `--run-ignored`.
- `pin-filter-selects-nothing`: the filter is a substring of the compiled nextest id from the file's `mod`/`#[path]` path plus the witness function's enclosing inline module (dirty twins swap the filter to `unrelated_module`, to filesystem `src::dag_tests`, to sibling inline `scheduler::pool_topology`, and to file-level `scheduler::incarnation_rejects_pre_remove_source_submission` which omits `tests`).
- `route-characterization-cardinality` / `route-characterization-invented`: every ARH1 cutover row owned by a narrowing heir (ARH3/ARH4) is characterized exactly once before its heir narrows it, and no invented route rides along (dirty twins drop `ARH1-CUT-3` and duplicate `ARH1-CUT-4`).
- `route-path-mismatch` / `route-owner-mismatch` / `route-without-pins`: a route row binds its register row's candidate path and heir.
- `route-surface-drift` / `route-surface-not-live`: the narrowed surface is characterized as it exists BEFORE narrowing — the ARH1-CUT-2 field items join the ARH1 field narrowing population exactly and each is still a live `pub` field, the live `pub fn test_*` hook names of `scheduler.rs` equal the ARH1 test-configuration rows by name, and ARH1-CUT-4 restates the bulk retained types, retained assoc items and consumer population exactly (dirty twins add a fourth field name, drop `HashValue`, drop a bulk consumer, and replace the CUT-4 surface with `{kind:"invented"}`).
- `route-required-witness-missing`: the ARH1-CUT-2 scheduler field route pins every required stale-work witness by file and test — incarnation rejection (pre-remove source submission, delayed source republication, delayed failure across remove/reset), removal drain (late admission, unknown-removal history capacity), the retained generation-floor fence and deferred-blocker replacement. A pin may carry more witnesses, never fewer; ARH7's predecessor join fails with it (dirty twins drop each required witness in turn and re-home one to a different file).
- `ac3-concern-missing` / `ac3-concern-without-evidence` / `ac3-concern-invented`: every charter AC3 concern (fresh-versus-incremental equivalence, edit/revert, cancellation, stale/partial rejection, deterministic ordering under perturbed discovery or scheduling) carries existing named evidence and nothing else rides the block (dirty twins drop `edit/revert` and point cancellation evidence at a missing file).

## ARH2-separation (reject) — AC5 methodology-bound measurements

- `dimension-cardinality` / `dimension-invented` / `dimension-without-separation` / `dimension-without-measures`: the four charter dimensions (production behavior, clean/warm build time, test cost, application latency) appear exactly once each, every one declaring what it measures and separates from (dirty twin drops `application-latency`).
- `dimension-commits-wall-clock`: a dimension row may never carry a wall-clock/RSS/speedup number — measured numbers live in gate receipts under the ratified methodology (dirty twin adds `wallNs`).
- `mechanism-binding-missing` / `dimension-mechanism-kind-mismatch`: every dimension has a nonempty mechanisms array of the required kind (dirty twins delete or empty all mechanisms, and bind test-cost to BF2 gate cells).
- `gate-cell-unknown` / `gate-cell-wrong-operation`: every gate-cell mechanism is a locked cell of `performance-gates.toml`, and application-latency must cite a host/session cell (dirty twins cite `B6_INVENTED_CELL` and `B6_COMPILER_ROUTE_OVERHEAD`).
- `cargo-build-recipe-identity-mismatch`: clean and warm recipes measure the same cargo build targets (dirty twin retargets only the warm command at `verter_debug_assert`).
- `dimension-cache-state-incomplete`: clean/warm build time binds cargo `--timings` recipes for both cache states (dirty twin drops the warm leg).
- `cargo-build-clean-prepare-missing` / `cargo-build-clean-prepare-not-clean`: the clean leg must `cargo clean` the timed packages before the `--timings` build. A missing prepare, or a prepare that is itself a `cargo build` (prewarming the target), leaves the alleged clean leg measuring a warm target.
- `cargo-build-warm-prepare-missing` / `cargo-build-warm-prepare-not-build` / `cargo-build-prepare-identity-mismatch`: the warm leg's prepare is a first `cargo build` of those same packages so the timed command is the second build.
- `behavior-lane-unbound` / `behavior-lane-invented` / `test-cost-lane-unbound` / `test-cost-lane-invented`: the behavior and test-cost lane lists are exactly the characterization product's pinned command set, both directions (dirty twins splice out `stable_key_tests` and append a phantom lane).
- `runner-class-unbound`: the number policy equals the locked `[runner]` class of `performance-gates.toml` (dirty twins delete it or suffix-forge it).

The scheduler field route retains generation floors and deferred blockers. Its `scheduler::` lane includes the lifecycle module: incarnation rejection, delayed completion/failure across removal/reset, unknown-removal history capacity and late-admission capacity drain replace the removal-history proof. Reset's external generation fence and deferred-blocker replacement remain required; SKR-RET-FLOORS owns the remaining floor migration. Both behavior and test-cost recipes bind the same expanded lane.
