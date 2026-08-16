# Disposition ruling

BF3 cannot be recommended for acceptance now. RT-1 is a blocking public-route defect, TR-1 is a transport-contract defect, and the reported batch partial publication is a separate atomicity item. All immediate correction blocks must gate B2/B3.

The governing basis is that BF3’s inventory expressly includes batch, host, NAPI, and WASM routes, while route equality covers typed status and requested artifacts—not merely the absence of leaked bytes. See [scope-consult-ruling.md](docs/arch/refactor/rev11/evidence/BF3/scope-consult-ruling.md:39), [AMD-005](docs/arch/refactor/rev11/amendments/AMD-005-framework-compiler-conformance-rescope.md:208), and [C4.md](docs/arch/refactor/rev11/charters/C4.md:5).

## RT-1

Disposition: **DEFER**, to a new immediate common-layer block `BRT0` (“public compiler route and transport identity correction”).

RT-1 is one root-cause defect, not two:

- `CompileBatchInput` omits framework/source-language identity.
- `compile_many` consequently upserts every input as Vue.
- Vue-shaped success and the disappearance of Svelte refusals are two consequences of that single wrong-carrier selection defect.

It is not BS0-owned: the Svelte compiler is never selected. It is not BA0-owned: choosing the wrong carrier precedes publication atomicity. It is a session/public-route request-construction defect spanning the Rust batch DTO, NAPI input, TypeScript declaration, grouping/cache identity, and both batch lanes.

Because this route returns successful output for the wrong framework, `BRT0` must be an immediate predecessor of both B2 and B3. Waiting for B3’s eventual canonical request or C4’s later route-equivalence proof would permit known wrong public success to survive past BF3.

Acceptance item: `BF3-RT-1-BATCH-CARRIER-PARITY`, mapped primarily to `FC-ROUTES-001` and secondarily to `FC-CAPABILITY-001`. It must prove through the public batch surface:

- explicit `.svelte` carrier selection on both host-backed and runtime-render lanes;
- client output agrees with the single-file Svelte route and contains no Vue markers;
- server and advanced-rune cases return the same typed refusal as the single-file route;
- refused entries contain no code, map, language, diagnostics product, or other partial artifact;
- Vue controls remain Vue;
- mixed Vue/Svelte batches preserve per-input carrier identity through deduplication and cache lookup.

Resolution gate: `BRT0` acceptance, no later than BF3 plan close and necessarily before B2/B3 dispatch.

### Batch partial publication

A genuinely refused batch item that nevertheless carries a product is not a second half of RT-1. It is a separate atomicity violation:

| id | finding | class | disposition | owner |
|---|---|---|---|---|
| AT-2 | batch entry publishes a product together with a genuine typed refusal | atomicity violation | DEFER | BA0, distinct acceptance item |

Acceptance item: `BF3-AT-2-BATCH-REFUSAL-ATOMICITY`, mapped to `FC-ATOMIC-001`.

The regression must expose the typed refusal and leaked product in the same entry. Merely showing Vue output from the wrongly selected carrier does not establish AT-2, because that internal request never reached a Svelte refusal. BA0 must close AT-2 if the independently identified refusal-plus-product observation is reproducible.

## TR-1

Disposition: **DEFER**, also to `BRT0`, as a distinct acceptance item.

This is a transport-contract defect under the current portable-host API. NAPI deliberately maps `MissingVirtualNode` to `null`, while WASM maps the same host outcome through the throwing error path. The public typings expose that difference: native is nullable and WASM is not. The transports therefore do not preserve the same typed status, and a caller cannot switch transport without changing control flow.

“No product leaked” satisfies atomicity but does not satisfy route equivalence. The acceptable contract is one canonical missing-product outcome serialized consistently by both transports. Either uniform nullable absence or uniform typed failure could be valid; choosing between them is product-contract ratification, not something an implementer may decide implicitly. The current split is not acceptable merely because both paths withhold the product.

Acceptance item: `BF3-TR-1-MISSING-PRODUCT-PARITY`, mapped to `FC-ROUTES-001`. It must use the identical typed query against native and WASM and assert equivalent observable status, payload, and typings for:

- structurally absent nodes;
- nodes unavailable because the requested product was refused;
- a successful control.

Resolution gate: `BRT0` acceptance, no later than BF3 plan close and before B2/B3 dispatch.

## Consolidated table

| id | corrected class | disposition | durable owner | resolution gate and acceptance item |
|---|---|---|---|---|
| SV-1 | Svelte compiler defect | DEFER | BS0 | BS0 acceptance before BF3 plan close/B2/B3; `BF3-SV-1-EACH-FLAGS` → `FC-SVELTE-001`. Correct gate. |
| SV-2 | Svelte compiler gap | DEFER | BS0 | Same gate; `BF3-SV-2-PROPS-INSTANCE` → `FC-SVELTE-001`. Correct gate. |
| SV-3 | Svelte compiler mapping defect | DEFER | BS0 | Same gate; `BF3-SV-3-CLIENT-MAP-SCRIPT` → `FC-SVELTE-001`. Correct gate only if it asserts authored script-declaration provenance, not merely map validity. |
| SV-4 | Svelte session-projector defect | DEFER | BS0, distinct acceptance item | Same gate; `BF3-SV-4-PROPS-SURFACE` → `FC-TS-001` plus the Svelte pack. Correct as a distinct item; compiler-output tests alone are insufficient. |
| RT-1 | public batch route/carrier-selection defect | DEFER | BRT0 | BRT0 acceptance before BF3 plan close/B2/B3; `BF3-RT-1-BATCH-CARRIER-PARITY` → `FC-ROUTES-001`. |
| AT-1 | atomicity violation | DEFER | BA0 | BA0 acceptance before BF3 plan close/B2/B3; `BF3-AT-1-COMBINED-REFUSAL-ATOMICITY` → `FC-ATOMIC-001`. Correct gate. |
| AT-2 | per-entry batch atomicity violation | DEFER | BA0, distinct acceptance item | Same gate; `BF3-AT-2-BATCH-REFUSAL-ATOMICITY` → `FC-ATOMIC-001`. Required if the genuine refusal-plus-product observation reproduces. |
| CSS-1 | CSS option/product-contract defect | DEFER | BCSS0 | BCSS0 acceptance before BF3 plan close/B2/B3; `BF3-CSS-1-STANDALONE-SOURCEMAP` → `FC-OPTIONS-001`, also asserting no silently ignored public option. Correct gate. |
| TR-1 | portable transport-contract defect | DEFER | BRT0, distinct acceptance item | BRT0 acceptance before BF3 plan close/B2/B3; `BF3-TR-1-MISSING-PRODUCT-PARITY` → `FC-ROUTES-001`. |
| RA-1 | parse-derived route-assembly artifact | **REJECT as defect** | — | Correct. `list_virtual_files` is profile-free discovery of addressable parse-derived node kinds, not a successful-materialization or publication claim. Reopen only if a product/cache artifact leaks or the documented contract promises successful retrieval. |
| RA-2 | unreachable latent concern | **REJECT as defect** | — | Correct. `has_runtime_surface` answers whether any runtime artifact exists; styles legitimately count. A refusal publishing CSS would first violate atomic publication. With no reachable such state, the predicate is not independently defective. |

## Acceptance consequence

The shortest path to an acceptance recommendation is:

Implementers can:

- record durable debt rows for every `DEFER`, including the ruling reference, owner, resolution gate, and acceptance item above;
- add the discriminating public-boundary regressions;
- implement and verify BS0, BRT0, BA0, and BCSS0;
- complete BF3’s remaining inventory and demonstrate that RA-1/RA-2 leak no product;
- produce acceptance evidence for all named items.

Only maintainers can:

- ratify the BF3 rescope amendment;
- authorize `BRT0` and the other immediate correction charters;
- amend the DAG so BS0, BRT0, BA0, and BCSS0 are required predecessors of B2/B3;
- ratify the canonical missing-product transport shape;
- accept those correction blocks and finally BF3.

Until those corrections are accepted and the DAG/charter changes are ratified, BF3 must remain unrecommended and B2/B3 must remain locked.
