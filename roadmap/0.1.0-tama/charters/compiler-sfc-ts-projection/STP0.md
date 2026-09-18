<!-- unified-charter-v2
id=STP0
name=Projection constitution and current-feature preservation contract
predecessors=ORC0,TCM0R,CCA1J,B4R0
phase=compiler
train=compiler.sfc-ts-projection
product=vue_typescript_tooling
kind=lock
semantic_role=delivery
class=compiler
owner=compiler.sfc-ts-projection:contract
conflict_domains=capability_catalog
resource_class=docs-light
gate_profile=docs-domain
review_profile=architecture-3
implementation_effort_min=high
implementation_effort_default=high
review_effort_min=high
review_effort_default=high
verification_effort_min=high
verification_effort_default=high
confirmation_effort_min=high
confirmation_effort_default=high
size=M
dispatchable=true
optional=false
release_gating=none
external_requirements=
charter=charters/compiler-sfc-ts-projection/STP0.md
max_production_loc=0
max_production_files=0
max_related_packages=0
rescope_loc=1500
rescope_files=12
rescope_unrelated_packages=3
-->

# STP0 - Projection constitution and current-feature preservation contract

> **Status: pending implementation.** This charter is part of final plan revision 1, not a record of completed engine or product qualification.

## 1. Independently acceptable outcome

Deliver **Projection constitution and current-feature preservation contract** as one reviewable boundary. The acceptance product is `ProjectionPolicy v1`, `CurrentFeatureObligation schema`, `InferenceParticipation policy`, `ProjectionOwnershipMap`. A failed mandatory case blocks completion; rescoping may split work but may not reduce this obligation.

## 2. Current owner, final owner and architectural role

Current authority lives in the listed existing compiler/provider/LSO surfaces. Final responsibility stays with `compiler.sfc-ts-projection:contract` through the existing ProjectionBackend/CodeTransform/CertifiedTypeEngineBinding/LSO division. This node has role `delivery` and kind `lock`. It creates no parallel framework resolver, checker, CSS or mapping authority.

## 3. Concrete surfaces and named products

**Existing owner surfaces to read and, only where needed, modify:**

- `roadmap/0.1.0-tama/contracts`

- `crates/verter_compiler/src/framework_common/vue_projection_backend.rs`


**Planned implementation/test homes:**

- `roadmap/0.1.0-tama/contracts/sfc-typescript-projection.md`

- `tests/sfc-projection/STP0/`

- `tests/sfc-projection/STP0/manifest.json`


The planned filenames are new homes, not claims that those files already exist. Resolve the current exact owner symbols at dispatch; retain source ownership across a move. Only this node's product population may be mutated in shared existing modules.

**Produces:** `ProjectionPolicy v1`; `CurrentFeatureObligation schema`; `InferenceParticipation policy`; `ProjectionOwnershipMap`. These are named logical contracts. Existing names remain upstream-owned; a new concrete signature must match the STP8 accepted ABI and the product catalog.

## 4. Exact predecessor contracts

- **ORC0:** Trusted implementation-ledger cutover; consume that owner's existing accepted API and source/identity contract, not a duplicate implementation. Its trusted implemented-ledger state is the readiness input; evidence artifacts are not a separate scheduling service.

- **TCM0R:** TypeScript dual-plane architecture and observation-identity rescope; consume that owner's existing accepted API and source/identity contract, not a duplicate implementation. Its trusted implemented-ledger state is the readiness input; evidence artifacts are not a separate scheduling service.

- **CCA1J:** IDE projection route convergence; consume that owner's existing accepted API and source/identity contract, not a duplicate implementation. Its trusted implemented-ledger state is the readiness input; evidence artifacts are not a separate scheduling service.

- **B4R0:** Stable SourceUnitId lineage repair; consume that owner's existing accepted API and source/identity contract, not a duplicate implementation. Its trusted implemented-ledger state is the readiness input; evidence artifacts are not a separate scheduling service.


## 5. Binding architecture and scope

Read `contracts/sfc-typescript-projection.md`, `contracts/sfc-projection-instancetype.md`, `contracts/sfc-projection-implementation-protocol.md` and the relevant predecessor charters. TypeScript owns type answers. Vue generated default exports remain constructor-shaped and preserve `InstanceType<typeof Comp>`. One use has one specialization transaction; mapping geometry, semantic origin and edit spelling stay separate. All shipped valid features are RequiredCurrent.

## 6. Internal implementation work packages

### STP0.1 - Establish the source contract


Ratify the constructor-shaped Vue default export, TypeScript-only type authority, one specialization transaction per use, and source/operation/edit provenance separation. Keep the existing five compiler authorities; a plan is a compiler-owned obligation description, not another semantic registry.


**Discriminating cases:** `STP0-authority`, `STP0-ratification`. Start with the failing or characterized baseline and keep the corresponding clean/control twin.


### STP0.2 - Implement the owned product


Make all shipped valid Vue features RequiredCurrent. Preserve intended semantics, not known unsound behavior: correcting a bug requires a source-backed oracle and a clean/dirty pair, not removing the row. Fix Vue-correct typed fallthrough as the primary policy and retain the original benchmark score in a separate unmodified lane.


**Discriminating cases:** `STP0-required-current`. Start with the failing or characterized baseline and keep the corresponding clean/control twin.


### STP0.3 - Close boundary integration


Define channel participation as syntax/framework policy: authored-signature, contextual-consumer, validation-only, observation-only, or coupled. Do not compute type-parameter variance or inference results using native TypeInfo. Freeze the absence of a public callable replacement and of a synthetic runtime property used solely to hold checker metadata.


**Discriminating cases:** `STP0-policy`. Start with the failing or characterized baseline and keep the corresponding clean/control twin.


The work packages are steps toward one acceptance result. They are not independently dispatchable subnodes and do not authorize unrelated neighbor work.

## 7. Identity, invalidation, completeness and publication

Use established source lineage and the exact input snapshot. Keep logical ComponentUseId separate from revision-qualified observations. Any inferred use input change invalidates the associated specialization; unchanged checking text cannot justify stale source mappings. Incomplete/configuration-missing/cancelled observations cannot warm complete caches. Publication and edits use the existing provider/LSO snapshot authority. For a pure proof or contract change, test these where the fixture crosses that boundary rather than adding production state.

## 8. Migration and consumer convergence

Ratify a precise contract and required row manifest; do not pre-certify later code. No runtime activation or new public helper shape is authorized here without the predecessor evidence explicitly required by this node.


## 9. Exact retirement obligations

No standalone legacy route is retired by this node. Record the exact displaced call sites/helpers as they migrate and assign final removal to STP58 (Vue) or STS15 (Svelte). Existing shared runtime/native-analysis/style authorities are not deletion targets.

## 10. Forbidden designs

No native type-answer substitution, callable Vue SFC replacement, broad any constructor, vacuous never success, duplicate script-body checker, fixed-N overload claim, invented source location, feature-mask diagnostic suppression, incomplete rename application, per-fixture options, hidden old-route answer, or complete capability claim backed only by a dormant product. Author-written any and framework-legal open domains are preserved rather than disguised.

## 11. Acceptance IDs and discriminating proof

The following are mandatory case specifications. `accept` means the scenario must succeed; `reject` means the proposed implementation/mutation must be rejected for the stated reason, not merely produce an unrelated diagnostic. These seeds expand to executable fixtures through STP1; they do not replace the inherited complete suites.


| Case | Scenario / regression | Required disposition |
| --- | --- | --- |

| `STP0-authority` | A proposal routes a prop through native assignability or creates a second mapping owner | reject |

| `STP0-required-current` | A shipped valid feature or InstanceType row is removed, optionalized, or marked external | reject |

| `STP0-policy` | Default inheritAttrs and explicit true differ without an actual runtime distinction | reject |

| `STP0-ratification` | Contract names current/final owners, all mandatory rows, and receiving amendments | accept |


**STP0-AC1 - completeness:** all cases and applicable RequiredCurrent rows selected; no zero-test pass. **STP0-AC2 - observability:** diagnostics/types/source targets and representative hover, definition/references and edit participation for touched semantics. **STP0-AC3 - state safety:** incremental=fresh and stale/cancelled publication controls for owned state. **STP0-AC4 - bounded work:** no hidden duplicate parse/check/emit/helper expansion or unbounded retention in touched hot paths. A not-applicable axis needs an explicit untouched-owner rationale; required features cannot be waived.

## 12. Performance evidence

Use the STP1 predeclared methodology and existing performance policy. Capture generated text/helper size and engine/public-consumer cost where this node changes them. Measure equivalent work and retain failed/noisy measurements. Do not lower check strength or alter the denominator to meet a target. Contract-only changes need no invented timing claim.

## 13. Scope budgets and mandatory rescope

Metadata ceiling: `M`, 0 production LOC, 0 production files, 0 related packages; rescope above 1500 LOC / 12 files / 3 unrelated packages. Test/fixture work must remain reviewable even where production budget is zero. A proof node may implement its owned test runner/fixtures, not the product it is meant to qualify.

Abort and amend before mutation if a second independently acceptable feature, an unrelated ABI migration, a new semantic authority or a required-case waiver is needed. A discovered compiler limitation is a design issue to solve upstream, not a supported-feature deletion.

## 14. Targeted verification and review

Run the static roadmap validation and requirement/case coverage checks, then review all named predecessor evidence. A lock cannot use a documentation-only test to assert runtime correctness.

Use owning Rust/TypeScript tests and the existing `docs-domain` gate profile. Reviews follow `architecture-3` (independent reviewers under the existing policy). Canonical full gates are owned by CI/orchestration, not repeated locally by every implementer. Commands in this section are future implementation obligations, not claimed executions in the planning session.


## 15. Successors and externally visible promises

Successors may consume only the accepted products above after the existing ledger records completion. No proof report expands public support beyond its covered manifest. Full Vue typing/public declarations/IDE is STP59; full Svelte typing/IDE is STS15. Runtime compilation and future Astro/MDX/Lit support retain their separate roadmap claims.

## 16. Completion evidence

Record actual source revision, input/framework/engine pins, selected case IDs, commands and raw outcomes in `tests/sfc-projection/evidence/STP0/` or the existing CI artifact owner. Include changed symbols, exact deletion population, review findings and why applicable negative tests discriminate the behavior. Never mark a pending test case executed by copying this charter into an evidence file.

## 17. Roadmap consistency

This node is part of the single final STP/STS plan, not the previous VTP/VTC suggestions. Keep TOML, portable definition, inline/mirrored charter and pending state aligned. Receiving amendments are explicitly listed in section 13 of `contracts/sfc-typescript-projection.md`. Do not add reverse edges to TCM4, BR0 or other ancestors; run the combined graph validator after every amendment.
