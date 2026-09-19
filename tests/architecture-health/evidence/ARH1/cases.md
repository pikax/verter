# ARH1 evidence cases

The sole owning interface is `node tests/architecture-health/ARH1/verify.mjs` (verify) plus `node --test tests/architecture-health/ARH1/arh1.test.mjs` (dirty twins), wired into `test:scripts`. The verifier joins the shipped ARH0 predecessor products (`../ARH0/products/responsibility-map.json`, `../ARH0/products/debt-register.json`) and the live tree only; the program DAG is database-owned by the TAMA controller, so owner/heir ids are checked structurally (dotted lowercase train ids, uppercase node ids) and no DAG copy is read from this tree.

## ARH1-ratification (accept)

Clean products validate: the five ARH0 god-module candidates each carry exactly one hotspot contract, the declared import direction equals the production import tree measured from the live source (comments stripped; cfg(test) and cfg(all(test, ..)) items stripped so test configuration never counts as production direction; nested `use` groups expanded with prefix inheritance so brace-list items never classify as roots and local `macro_rules!` re-exports name file locals; verter, external and crate-internal roots compared two-way), the layer rules equal the live crate Cargo manifests, constructor anchors and retained/narrowed surface items bind real declarations, state rows name live identifiers with one sole owner that declares the state, narrowing rows carry exact consumer inventories, the semantic_query importer list equals the derived cross-crate population, and every declared route has exactly one cutover disposition.

## ARH1-ratification (reject)

- `manifest-case-drift`: the manifest records a case the verifier does not implement, drops one it does, or lists a case without disposition reject and the `clean products` accept twin (dirty twins add `ARH1-phantom` and drop `ARH1-split`).
- `manifest-product-drift`: a carried product schema missing from the manifest's products list, or a recorded product no file carries.
- `manifest-command-drift`: the manifest verify/test command is not the canonical command string derived from the verifier's own location (dirty twin points verify at `scripts/affected-tests.mjs` — an existing script that is not the ARH1 verifier).
- `obligation-producer-malformed` / `obligation-without-text`: an AC4 producer obligation whose owner id is outside the structural discipline (dirty twin invents `doc2`) or that records no obligation text.
- `ac4-surface-uncovered`: a charter AC4 surface (`VIM/DX`, `host/profile`, `permissions`, `uncertainty`, `migration`) named by no producer obligation and no N/A rationale — the obligations and the rationale are rewritten keyword-free in the dirty twin (dirty twin rewrites every obligation and `ac4Rationale` to "dirty twin").

## ARH1-hotspot-coverage (reject) — AC1 join to the shipped ARH0 inventory

- `hotspot-without-contract` / `contract-without-inventory-row`: the contract population and the ARH0 god-module population are joined both directions (dirty twins delete the `flow_slice_content.rs` row and retarget `semantic_query.rs` at `semantic_query_memo/arena.rs`).
- `responsibility-dropped` / `responsibility-invented`: every ARH0 inventoried responsibility survives under exactly one authority, and no authority row claims a responsibility the inventory does not carry (dirty twins drop `batch coordination` and invent `typescript diagnostics formatting`).
- `authority-double-claimed`, `missing-authority-owner`, `missing-hotspot`: structural discipline of the authority map.

## ARH1-import-direction (reject) — AC1 boundary binding

- `import-drift`: the declared verter/external/crate-internal sets must equal the production import tree of the live source — a declared-but-unmeasured import (dirty twin adds `verter_semantic` to the scheduler file), a measured-but-undeclared one (dirty twin undeclares `verter_language`), an undeclared live crate-internal root (dirty twin undeclares `dag`) and a declared root only test configuration imports (dirty twin re-declares `cache_id`, which only the scheduler's cfg(test) code imports) all fail, as does a measured import with no dependency in the owning crate's manifest.
- Production measurement basis: a synthetic source with `#[cfg(test)] mod` using `crate::VerterHost`, `#[cfg(all(test, not(target_arch = "wasm32")))] mod` using `crate::pool` and `#[cfg(any(test, feature = "test-support"))] mod` using `crate::stage` measures exactly `{dag, stage}` production-internal while the full measurement still sees `VerterHost` and `pool` — the live `use crate::VerterHost` inside `project_semantic_dispatch/build.rs`'s cfg(test) module is therefore measured and must be (and is) excluded by the strip, never silently undeclared.
- `forbidden-import`: a must-not-import root that appears in the measured source (dirty twin adds `verter_language` to the scheduler file's forbidden set while it is still imported).
- `layer-violation` / `layer-rule-contradiction`: layer rules equal the live Cargo manifests — removing a real dependency from a rule's allowed set, or forbidding one the crate depends on, or allowing and forbidding the same root (dirty twins mutate `ARH1-LAYER-1`).
- `missing-importer` / `importer-without-reference`: each declared importer exists and still references the hotspot module token (dirty twin lists `crates/verter_span/src/lib.rs` as a `semantic_query` importer).

## ARH1-constructor (reject)

- `missing-constructor`: a declared constructor that is not a live `pub`/`pub(crate)` fn (dirty twin invents `with_semantic_engine`).
- `constructor-anchor-missing`: a capability anchor absent from the constructor's declaration span — the span stops at the next item-level fn, so the capability talks about the real signature, not the whole file (dirty twin anchors `new` on `Arc<Mutex<Vec<Byte>>>`).
- `constructor-without-rules`, `constructor-rationale-missing`: every constructor carries rules; a constructor-free hotspot records why (dirty twin deletes the flow_return rationale). `test-hook-unmarked` keeps testOnly rows named as test hooks.

## ARH1-state-lifetimes (reject) — AC3 rationale's ownership map

- `bad-lifetime`: a lifetime outside the contract vocabulary {process, session, request, dispatch-transaction, content-version, snapshot-epoch} (dirty twin sets `Scheduler.overlay` to `forever`).
- `state-without-owner` / `state-owner-missing`: every state row names one sole owner, and the owner target (file or module directory) exists.
- `state-not-in-source`: some identifier token of the state must appear in the live hotspot source (dirty twin adds an `xyzzy_entanglement_ledger`).
- `state-owner-without-declaration`: the sole-owner target must itself declare an identifier of the state — an unrelated existing file is not ownership (dirty twin re-points `Scheduler.nodes` at `crates/verter_span/src/lib.rs`, which exists and declares nothing of the scheduler state).

## ARH1-surface (reject)

- `surface-declaration-drift`: the module visibility declaration is pinned verbatim at its declaration site (`pub mod semantic_query;`, `pub(crate) mod flow_slice_content;`, `pub(crate) mod flow_return;`, `pub(crate) mod build;`) — dirty twin rewrites it to `pub(crate) mod semantic_query;`.
- `surface-item-missing`: retained fns/types and narrowed items must be live declarations (dirty twin retains `submit_everything`).
- `retained-item-unconsumed`: for `semantic_query` (flagged `crossCrateRetainedVerifiedByConsumers`) every retained type must be imported by a real cross-crate consumer — derived from use-statement and inline-path syntax, never substring or comment matches; the scheduler's retained surface serves the verter_session population, so its proof is scoped and documented instead of asserted.
- `bad-narrow-target` / `missing-narrow-consumer`: narrowing targets only {`pub(crate)`, `test-configuration`, `feature-gated`} (dirty twin widens `tombstones` to `pub`), and every affected-consumer path exists.
- `narrow-hook-without-reference-forms` / `narrow-consumer-without-reference` / `narrow-consumer-omitted`: each test-hook narrowing row records the qualified reference forms its consumers use (`Scheduler::test_x`, `.test_x(` receiver calls); every recorded consumer must reference one of those forms in comment-stripped code, and every file outside the owning crate referencing those forms must be recorded — an existing file that references a different hook is not a consumer (dirty twin lists `host_construction.rs` under `test_new`), and omitting the live `host_batch_coordinator.rs` caller of `test_new` fails.
- `importer-population-drift`: the declared importer list must equal the complete derived cross-crate population of the module (direct `verter_session::semantic_query` paths, aliased crate roots via `use verter_session as host;`, module imports with local refs) — dropping a real importer (dirty twin drops `verter_ffi/src/convert/typeinfo.rs`) and adding a mirror-documentation mention that imports nothing (dirty twin adds `verter_audit/src/payloads/tags.rs`) both fail.
- `narrowed-item-consumer-unrecorded`: every file importing at least one narrowed (unretained) name must be recorded on the bulk row's consumer migration — dirty twin drops `flow_literal_provenance.rs`, which imports `FlowGap`, `LiteralValue` and `SemanticNodeData` from the library (integration tests compile as separate crates).
- `assoc-item-unretained` / `assoc-item-missing`: assoc items used cross-crate on retained types (qualified `Type::member` on a pub member of the module) must be retained — retaining a type does not retain its methods; dirty twin unlists `PartialReasonSet::PROPAGATED` (used by `verter_ffi`'s `component_meta.rs`), and a listed item that is not a declared pub member fails.

## ARH1-split (reject) — ARH1-AC2 discriminator

- `split-retains-shared-state`: a cohesive module claiming state with mode `shared` — moving methods into files while retaining unrestricted shared state does not satisfy a responsibility boundary (dirty twin gives `flow_products.rs` a shared claim on the flow-return compute frame).
- `state-double-owner`: two modules claiming the same state `sole` (dirty twin has `flow_return_products.rs` claim the mutable product state that `flow_products.rs` owns — the sole-claim must also match the state row's declared sole owner).
- `state-ownership-undeclared`: a claim on a state the hotspot never declared (dirty twin: `Scheduler.secret_vibes` on `dag.rs`).
- `cohesive-responsibility-invented` / `missing-cohesive-module` / `split-rationale-missing`: cohesive responsibilities come from the ARH0 inventory, modules exist, and a hotspot without cohesive modules records why.

## ARH1-cutover (reject) — AC1 route ownership

- `arh0-debt-undecided`: ARH0 debt rows naming ARH1 as decision owner must be satisfied here (dirty twin drops the `packages/core` decision row bound to `ARH0-DEBT-1`).
- `duplicate-satisfies`: one ARH0 debt decision per register — a fresh-id copy of the `packages/core` row with owner `ARH11` and the opposite decision still fails (dirty twin does exactly that).
- `cutover-route-missing`: routes are keyed `(candidatePath, narrow kind)`; every narrowing kind a hotspot declares (field vs fn vs bulk) needs exactly one register row binding it to the executing heir — deleting `ARH1-CUT-2` (field narrowing) fails while its sibling `ARH1-CUT-3` (fn narrowing) stays on the same file, and dropping `ARH1-CUT-4` uncovers the `semantic_query.rs` bulk route.
- `duplicate-cutover-route`: two register rows carrying the same route key (dirty twin copies `ARH1-CUT-4` under a fresh id with a second disposition).
- `cutover-route-invented`: a register row whose route key matches no declared narrowing kind.
- `cutover-without-decision` / `cutover-without-disposition` / `missing-candidate-path` / `deletion-rationale-missing`: structural discipline of the register.
