# Add the semantic signature kernel train

Date: 2026-09-17. Authority: the owner supplied the consolidated implementation contract `contracts/signature-kernel.md` (Verter Semantic Signature Kernel, revision 4.1, SHA-256 `0baaf36510c2c0bf345864d4c701d9150ae853638150128d04fb5aa5d90b9539` of the supplied file) and directed that the D12 work continue under its enlarged scope. Implementation dispatch belongs to the TAMA controller.

## Source-grounded reason and bounded scope

D12 (PR #575) landed the async/generator return wrap and the two awaited relations on a substrate that still has no shared signature discovery, an intersection authority whose output order is the arena allocation order, a relation engine that refuses `Subtype`/`StrictSubtype`, strictness that never reaches dispatch from tsconfig, authored-only warm admission for synthesized signatures, and up to three `FlowReturn` executions per generic call. The contract resolves the architecture for one graph-native signature kernel shared by calls, constructors, utilities, runtime Awaited, lib conditional inference, and flow, with mandatory load-, query-, allocation-, cache-history-, and schedule-independent semantic ordering and generated output.

The contract's section 13 asks to "amend the D12 charter explicitly". `APPLICATION.md` freezes charters whose nodes carry an implemented ledger row, so the enlargement is recorded here and in the contract's checkout-binding appendix instead of by rewriting `charters/rev11-flow/D12.md`. D12 keeps its acceptance; V7 migrates its Awaited protocol onto the kernel and must keep D12-AC1..AC4 holding.

The contract's proposal names that do not exist at the checkout (`SemanticMeet`, `stitch_module_augmentations`, a `verter_semantic` `type_expr.rs`) are bound to their live owners in the contract's Appendix B. The oracle is pinned to TypeScript 7.0.2 native (installed); the recorded tsgo `7.0.0-dev.20260526.1` columns are ported by V0; 7.1.0-dev nightlies are out of scope.

## Added nodes

| Node | Independently acceptable outcome | Direct predecessors |
|---|---|---|
| V0 | Evidence lock: contract, oracle and lib digests, corpus port to 7.0.2, new signature corpus, determinism/stable-key/ledger/budget registration | D12 |
| V1 | Effective options, interned semantic contexts with one policy set, compact outcomes, complete result-demand identity, rooted admission | V0 |
| V2 | Signature records, frozen substitutions, provenance, boxcar-backed private storage, epoch read views, lifetime contract | V1 |
| V3 | Complete global contributor snapshots; delete the two `known_canonicals()` lookup scans | V1 |
| V4 | `VerterStableV1` order, carrier-qualified unions, `ReduceIntersection`, graph-native Subtype/StrictSubtype | V2, TA1B |
| V5 | `SignaturesOfType`, one positional model, `ReadSignatureResult` | V2, V3, V4 |
| V6 | Call/construct/utility/callable-classifier/flow-intersection cutover; one substitution | V5 |
| V7 | Runtime and lib Awaited over shared discovery; delete Awaited-private readers | V5, V6 |
| V8 | Differential, determinism matrix, performance, lifetime evidence, legacy deletion, documentation | V7 |
| V9 | Measured storage optimization (conditional, optional) | V8 |

All rows start pending and dispatchable in `rev11.signature-kernel` with the Rev11 foundation milestone; V9 is optional. V0 is initially READY.

## Superseded owners

- `rev11.type-algebra` (TA1A/TA1B, implemented): the `CanonicalMint` builders and the `CompositeCarrierCategory` registry remain the construction-site closure; V4 replaces their arena-ordinal sort and first-wins category window and retires `NormalizeIntersection` in favour of `ReduceIntersection`. TA1B is a recorded predecessor of V4.
- `rev11.flow` D12: predecessor of V0; its awaited relations are consumers of V5/V7.
- `rev11.query-runtime` G2 (pending): unchanged; V2's storage adapter is private to the kernel and does not alter `ProjectTypeStore` ownership.

## Protected active work

| PR | Current local owner | Protection |
|---|---|---|
| #575 | D12 | Base of the train's first candidate; no acceptance change. |
| #585 | C2 | Unchanged; compiler type-info gateway files are outside every V-node boundary. |
| #98 | No matching node | Not adopted; Svelte runtime surfaces stay outside this train. |

## Landing shape

Each node lands as its own candidate. Until #575 merges, the V0 candidate stacks on the D12 branch and later candidates stack on their predecessor's branch; each is retargeted to `main` when its base merges.
