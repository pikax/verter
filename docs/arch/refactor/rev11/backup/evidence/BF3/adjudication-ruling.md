# Architectural ruling

BF3 does not currently satisfy either `FC-ATOMIC-001` or inventory exhaustion. It may eventually close as a completed audit with D1–D5 still under correction, but only after the missing proofs are completed and a ratified amendment makes every immediate correction block a predecessor of B2 and B3.

## 1. `FC-ATOMIC-001` does not currently hold

Atomicity is scoped to one canonical compiler request and its artifact set—not globally to every product ever requested for the same component. The contract defines a compiler artifact set as “one typed all-or-nothing result containing exactly the requested products” and defines fail-closed as non-success before publication with no partial artifact (`docs/arch/refactor/rev11/contracts/framework-compiler-boundary.md:26`, `:39`). Its atomicity clause requires success to publish all and only the requested set and refusal to publish none (`docs/arch/refactor/rev11/contracts/framework-compiler-boundary.md:55-58`).

Therefore:

- A distinct IDE-only or PublicApi-only request may succeed even when a separate runtime request for the same component refuses.
- A combined request may not return runtime refusal while publishing IDE/PublicApi artifacts from that same transaction.
- Sharing preparation, analysis, or cached subplans does not merge or relax those publication boundaries.

The current carrier is not modeled as independent typed product requests. `want_ide` explicitly requests IDE output “in the same pass” (`crates/verter_compiler/src/framework_common/carrier_compiler.rs:384-386`), the resulting bundle contains both runtime and TSX products (`crates/verter_compiler/src/framework_common/carrier_compiler.rs:602-619`), and the Svelte carrier writes TSX after the runtime refusal outcome (`crates/verter_compiler/src/svelte/carrier.rs:517-528`). That is a mixed-success result, not two independently committed transactions. Consequently, the IDE publication alone is enough to fail `FC-ATOMIC-001`.

PublicApi has the same ruling, but its classification depends on request identity:

- If it is committed from that refused compile transaction, it is another violation.
- If the host issues a distinct PublicApi request with a distinct request/result identity, it is valid even for the same source component.

“Same component” is not the atomicity unit. The reviewer checks the canonical request token, requested-product set, typed terminal result, and every cache/publication committed under that token.

The correct architecture is to eliminate the ambiguous mixed-outcome bundle: IDE and PublicApi availability must be expressed as independent product requests and independently committed results. An intentionally combined multi-product request remains all-or-nothing. Shared computation may be reused, but publication identity may not be blurred for caching convenience.

BF3 must not implement this change. The settled ruling expressly forbids BF3 from adding an artifact-withholding path (`docs/arch/refactor/rev11/evidence/BF3/scope-consult-ruling.md:3`). An immediate common compiler/session atomicity correction block—call it `BA0`—must own the legacy carrier/request/result separation and permanent public-boundary tests. B3 and B4 remain the final owners of canonical requests and atomic publication; B3 already claims the requested-product collection (`docs/arch/refactor/rev11/charters/B3.md:5-9`), and B4 is explicitly the atomic-publication block (`docs/arch/refactor/rev11/program-dag.toml:106-109`).

## 2. `UNPROVEN` cannot satisfy “exhausted”

The settled rescope already answers this normatively: every retained reachable-success cell must have an actual result (`docs/arch/refactor/rev11/evidence/BF3/scope-consult-ruling.md:54`). `UNPROVEN` is an honest blocked disposition, but it is not exhaustion.

### PublicApi/TSC/declaration

The TypeScript observations are presently nondiscriminating. The mechanism promises real TypeScript observation of exports, structural types, and diagnostics (`packages/framework-conformance-harness/src/typescript-observe.mjs:1-15`), but its program is constructed solely from the supplied artifact map (`packages/framework-conformance-harness/src/typescript-observe.mjs:293-318`). Without resolvable framework declarations, equality at `any` proves nothing.

Closing this family requires:

- the exact pinned Vue/Svelte declaration and dependency closure to be resolvable by the existing TypeScript host;
- candidate and reference observations under the identical TypeScript version, options, framework closure, and module-resolution environment;
- module-resolution failure to fail the authoritative observation instead of degrading silently;
- a planted control proving that correct and empty/wrong prop surfaces produce different observations;
- semantic assertions over props, events, exports, bindings, diagnostics, and declaration-only behavior—not merely byte agreement or symbol presence.

The premise that this necessarily “builds a new oracle” is wrong. Supplying the exact framework type environment is provisioning the observation domain. TypeScript remains the observer; the pinned framework declarations supply the meaning of the imports.

This is conformance-harness work and is inside reshaped BF3’s exhaustion responsibility. It is not production TypeScript-product correction. If dispatch authority still prohibits that harness work, BF3 remains blocked until a separately authorized proof-closure block completes and is made a downstream predecessor.

### Svelte `compile_many`

It must be executed. Route is part of capability-cell identity, and “same semantic cells” does not establish batch equivalence. Closure requires successful and refused Svelte inputs through `compile_many`, per-item comparison with the corresponding single-file route, stable ordering, no cross-item contamination, and proof of the batch failure/partial-result contract. Source citations showing delegation are useful route-identity evidence, not an executed result.

### NAPI and WASM

Reachable public transports must be built and invoked. They need not repeat the full semantic matrix when structural delegation proves the same typed request, but each transport must execute representative success and refusal/optional-product cases proving option conversion, serialization, artifact presence/absence, and route equivalence. The need for `napi build --release` or `wasm-pack` is an execution prerequisite, not grounds for `UNPROVEN`.

The forbidden name-keyed scanner is not required. Repository policy expressly prohibits landing such guards (`CLAUDE.md:471`). Exhaustion is a claim about the pinned current tree; independent export enumeration plus executed known routes can close it. Future completeness should eventually derive bindings from a structural registry, but absence of that future drift guard does not excuse an unexecuted current route.

An honest `UNPROVEN` record identifies the exact claim, the missing discriminating observation, why existing evidence cannot decide it, its owner, and a falsifiable closure condition—and it blocks acceptance. A gap dressed as `UNPROVEN` uses nondiscriminating equality such as `any == any`, calls a batch or transport route “the same” without executing its boundary, or lets explanatory prose count as an actual result.

## 3. Ownership, acceptance, and the B2/B3 gate

### Defect ownership

- **D1 — Svelte each flags:** immediate `BS0`, implementation owner `verter_compiler`’s Svelte client block planner. It currently sets `EACH_ITEM_REACTIVE` from signal-kind classification (`crates/verter_compiler/src/svelte/runtime/client_block_plan.rs:156-162`), while the pinned constant confirms that bit is `1` (`packages/framework-conformance-harness/.oracle-checkouts/svelte/packages/svelte/src/constants.js:1`).

- **D2 — false `$props()` refusal:** immediate `BS0`, implementation owner the Svelte client semantic/lowering surface. The current classifier explicitly rejects any non-declaration instance-script prop reference (`crates/verter_compiler/src/svelte/runtime/client_surface_script.rs:47-60`). Correction means supporting the accepted official behavior, not merely deleting the check or changing the diagnostic.

- **D3 — runtime map provenance:** immediate `BS0`, implementation owner the Svelte runtime emitter/map builder. The builder can map only ranges explicitly deposited into `self.mappings`; intervening emitted text is intentionally emitted without provenance (`crates/verter_compiler/src/svelte/runtime/output.rs:121-196`). The correction must carry authored script declarations into the mapping plan.

- **D4 — untyped published props:** immediate `BS0` at the program level, but its implementation owner is specifically the session-side Svelte PublicApi projector—not the Svelte runtime backend. Its current missing-fact fallback is `{}` (`crates/verter_session/src/framework/api_projectors/svelte.rs:280-286`), which then feeds the emitted `Component<Props,…>` declaration (`crates/verter_session/src/framework/api_projectors/svelte.rs:310-317`, `:389-419`). `BS0` must contain a separately identified projector acceptance item for this defect.

- **D5 — standalone CSS sourcemap:** a separate immediate `BCSS0`, owned by `verter_compiler::css` and its standalone NAPI product boundary. Both processing branches hard-code `source_map: None` (`crates/verter_compiler/src/css/mod.rs:107-110`, `:143-145`). It does not belong to `BS0`, BS1, or a host-route owner. A true `sourcemap: true` request must produce a correct map for both passthrough and transformed CSS paths.

BS1 remains the broad post-B4 Svelte conformance train; it already owns final Svelte runtime, maps, PublicApi/TSC/declaration, and tooling products (`docs/arch/refactor/rev11/charters/BS1.md:8-21`). It is too late to own these immediate shipping corrections because it currently follows B4 (`docs/arch/refactor/rev11/program-dag.toml:118-121`).

### BF3 acceptance

Under the current DAG, BF3 cannot be accepted. B2 and B3 currently depend only on BV0 and BF3 (`docs/arch/refactor/rev11/program-dag.toml:94-103`), so accepting BF3 now would release them while D1–D5 and the atomicity violation remain live.

After Question 2’s proof gaps are closed, BF3 may be accepted as a completed audit with D1–D5 not yet corrected only if a ratified amendment first creates the immediate correction blocks and makes them mandatory B2/B3 predecessors. That is exactly the settled rule: audit closure after complete disposition is allowed only when every resulting correction block gates B2/B3 (`docs/arch/refactor/rev11/evidence/BF3/scope-consult-ruling.md:57`).

The amended DAG region must be equivalent to:

```text
BF3 -> {BA0, BS0, BCSS0}
{BV0, BF3, BA0, BS0, BCSS0} -> {B2, B3}
```

Acceptance identities must bind:

- `BA0`: request/result separation and `FC-ATOMIC-001`;
- `BS0-D1` through `BS0-D4`: actual corrections plus permanent discriminating regressions;
- `BCSS0-D5`: real standalone CSS maps through the Rust and NAPI boundaries.

The Vue precedent is the correct governance template. AMD-006 created an immediate correction block and inserted it before B2/B3 rather than waiting for the post-B4 owner (`docs/arch/refactor/rev11/amendments/AMD-006-vue-known-defect-correction.md:27-52`). The only necessary difference is decomposition by root cause: one immediate Svelte correction block, one common atomicity block, and one standalone CSS block.

No further architectural consult is required. Maintainer ratification is required only for the formal amendment that supersedes BF3’s stale guard/retraction charter language, introduces the new charters and acceptance IDs, and changes the locked DAG. The settled record already states that normative program-text changes require such ratification (`docs/arch/refactor/rev11/evidence/BF3/scope-memo.md:71`).
