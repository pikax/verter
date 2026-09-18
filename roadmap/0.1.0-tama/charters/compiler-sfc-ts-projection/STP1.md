<!-- unified-charter-v2
id=STP1
name=Feature inventory, pinned engines, and discriminating probe harness
predecessors=STP0
phase=compiler
train=compiler.sfc-ts-projection
product=vue_typescript_tooling
kind=proof
semantic_role=delivery
class=compiler
owner=compiler.sfc-ts-projection:harness
conflict_domains=vertical_manifest
resource_class=ts-heavy
gate_profile=targeted-domain
review_profile=architecture-3
dispatchable=true
optional=false
release_gating=none
gh_milestone=0.0.4
external_requirements=
charter=charters/compiler-sfc-ts-projection/STP1.md
size=M
max_production_loc=0
max_production_files=0
max_related_packages=0
rescope_loc=1500
rescope_files=12
rescope_unrelated_packages=3
implementation_effort_min=high
implementation_effort_default=high
review_effort_min=high
review_effort_default=high
verification_effort_min=high
verification_effort_default=high
confirmation_effort_min=high
confirmation_effort_default=high
-->

# STP1 - Feature inventory, pinned engines, and discriminating probe harness

> **Status: pending implementation.** This charter is part of final plan revision 1, not a record of completed engine or product qualification.

## 1. Independently acceptable outcome

Deliver **Feature inventory, pinned engines, and discriminating probe harness** as one reviewable boundary. The acceptance product is `CurrentFeatureInventory`, `ProbeManifest v1`, `EngineMatrix`, `ProjectionProbeRunner`, `PerformanceMethodology`. A failed mandatory case blocks completion; rescoping may split work but may not reduce this obligation.

## 2. Current owner, final owner and architectural role

Current authority lives in the listed existing compiler/provider/LSO surfaces. Final responsibility stays with `compiler.sfc-ts-projection:harness` through the existing ProjectionBackend/CodeTransform/CertifiedTypeEngineBinding/LSO division. This node has role `delivery` and kind `proof`. It creates no parallel framework resolver, checker, CSS or mapping authority.

## 3. Concrete surfaces and named products

**Existing owner surfaces to read and, only where needed, modify:**

- `scripts`

- `tests`

- `packages/types/src/components/components.ts`


**Planned implementation/test homes:**

- `scripts/sfc-projection/verify-node.mjs`

- `scripts/sfc-projection/manifest.schema.json`

- `scripts/sfc-projection/verify-node.test.mjs`

- `tests/sfc-projection/manifest.json`

- `tests/sfc-projection/STP1/`

- `tests/sfc-projection/STP1/manifest.json`


The planned filenames are new homes, not claims that those files already exist. Resolve the current exact owner symbols at dispatch; retain source ownership across a move. Only this node's product population may be mutated in shared existing modules.

**Produces:** `CurrentFeatureInventory`; `ProbeManifest v1`; `EngineMatrix`; `ProjectionProbeRunner`; `PerformanceMethodology`. These are named logical contracts. Existing names remain upstream-owned; a new concrete signature must match the STP8 accepted ABI and the product catalog.

## 4. Exact predecessor contracts

- **STP0:** ProjectionPolicy v1; CurrentFeatureObligation schema; InferenceParticipation policy; ProjectionOwnershipMap. Its trusted implemented-ledger state is the readiness input; evidence artifacts are not a separate scheduling service.


## 5. Binding architecture and scope

Read `contracts/sfc-typescript-projection.md`, `contracts/sfc-projection-instancetype.md`, `contracts/sfc-projection-implementation-protocol.md` and the relevant predecessor charters. TypeScript owns type answers. Vue generated default exports remain constructor-shaped and preserve `InstanceType<typeof Comp>`. One use has one specialization transaction; mapping geometry, semantic origin and edit spelling stay separate. All shipped valid features are RequiredCurrent.

## 6. Internal implementation work packages

### STP1.1 - Establish the source contract


Enumerate current Vue compiler, IDE, public-types, style, and CLI fixtures plus both selected benchmark manifests. Record input commits, selected package/binary provenance, tsconfigs, framework versions, file inclusion and runtime/compiler feature flags. Inventory includes InstanceType, normal script, Options API, macros, custom/global/async components, JSX/TSX, refs, directives, preprocessing, recovery and editor operations.


**Discriminating cases:** `STP1-inventory`, `STP1-types`. Start with the failing or characterized baseline and keep the corresponding clean/control twin.


### STP1.2 - Implement the owned product


Create one runner at scripts/sfc-projection/verify-node.mjs. It accepts --node, --engine, --require-all and --json; resolves stable row IDs; rejects zero selected cases and unexpected diagnostics; compares exact types/targets/edits and compiles clean twins. Author expected failures as harness data, not suppression directives injected into customer source.


**Discriminating cases:** `STP1-zero-selection`, `STP1-provenance`. Start with the failing or characterized baseline and keep the corresponding clean/control twin.


### STP1.3 - Close boundary integration


Pin a stock JS TypeScript engine and an exact mapper-capable native TypeScript build, with the supported Vue version matrix. Define equivalent-work measurements and acceptance budgets before candidate timing using existing performance/MEM0 policy. Framework reference defects and contradictory policies stay visible in versioned lanes; no denominator shrinking or fixture-sensitive options.


**Discriminating cases:** `STP1-clean-twin`, `STP1-harness`. Start with the failing or characterized baseline and keep the corresponding clean/control twin.


The work packages are steps toward one acceptance result. They are not independently dispatchable subnodes and do not authorize unrelated neighbor work.

## 7. Identity, invalidation, completeness and publication

Use established source lineage and the exact input snapshot. Keep logical ComponentUseId separate from revision-qualified observations. Any inferred use input change invalidates the associated specialization; unchanged checking text cannot justify stale source mappings. Incomplete/configuration-missing/cancelled observations cannot warm complete caches. Publication and edits use the existing provider/LSO snapshot authority. For a pure proof or contract change, test these where the fixture crosses that boundary rather than adding production state.

## 8. Migration and consumer convergence

This is a proof/qualification node. It may add fixture and harness integration inside its test homes, but it must not implement a missing production semantic feature. Counterexamples reopen the owning implementation or STP8 representation choice. Until that owner is corrected this node stays incomplete.


## 9. Exact retirement obligations

No standalone legacy route is retired by this node. Record the exact displaced call sites/helpers as they migrate and assign final removal to STP58 (Vue) or STS15 (Svelte). Existing shared runtime/native-analysis/style authorities are not deletion targets.

## 10. Forbidden designs

No native type-answer substitution, callable Vue SFC replacement, broad any constructor, vacuous never success, duplicate script-body checker, fixed-N overload claim, invented source location, feature-mask diagnostic suppression, incomplete rename application, per-fixture options, hidden old-route answer, or complete capability claim backed only by a dormant product. Author-written any and framework-legal open domains are preserved rather than disguised.

## 11. Acceptance IDs and discriminating proof

The following are mandatory case specifications. `accept` means the scenario must succeed; `reject` means the proposed implementation/mutation must be rejected for the stated reason, not merely produce an unrelated diagnostic. These seeds expand to executable fixtures through STP1; they do not replace the inherited complete suites.


| Case | Scenario / regression | Required disposition |
| --- | --- | --- |

| `STP1-inventory` | Remove one imported current fixture from selected coverage | reject |

| `STP1-zero-selection` | Pass a node filter selecting zero cases | reject |

| `STP1-clean-twin` | Candidate reports an unrelated generated error on a clean twin | reject |

| `STP1-types` | A type equality assertion is accidentally satisfied by any or vacuous never | reject |

| `STP1-provenance` | A different executable is substituted under the same engine label | reject |

| `STP1-harness` | One real positive and one deliberately broken anchored negative execute on each admitted engine | accept |


**STP1-AC1 - completeness:** all cases and applicable RequiredCurrent rows selected; no zero-test pass. **STP1-AC2 - observability:** diagnostics/types/source targets and representative hover, definition/references and edit participation for touched semantics. **STP1-AC3 - state safety:** incremental=fresh and stale/cancelled publication controls for owned state. **STP1-AC4 - bounded work:** no hidden duplicate parse/check/emit/helper expansion or unbounded retention in touched hot paths. A not-applicable axis needs an explicit untouched-owner rationale; required features cannot be waived.

## 12. Performance evidence

Use the STP1 predeclared methodology and existing performance policy. Capture generated text/helper size and engine/public-consumer cost where this node changes them. Measure equivalent work and retain failed/noisy measurements. Do not lower check strength or alter the denominator to meet a target. Contract-only changes need no invented timing claim.

## 13. Scope budgets and mandatory rescope

Metadata ceiling: `M`, 0 production LOC, 0 production files, 0 related packages; rescope above 1500 LOC / 12 files / 3 unrelated packages. Test/fixture work must remain reviewable even where production budget is zero. A proof node may implement its owned test runner/fixtures, not the product it is meant to qualify.

Abort and amend before mutation if a second independently acceptable feature, an unrelated ABI migration, a new semantic authority or a required-case waiver is needed. A discovered compiler limitation is a design issue to solve upstream, not a supported-feature deletion.

## 14. Targeted verification and review

After STP1 implements the runner, execute:

```sh
node scripts/sfc-projection/verify-node.mjs --node STP1 --engine all --require-all --json
```


STP1 bootstraps and self-tests this command itself; the harness must reject absent/empty manifests and record the selected executable.

Use owning Rust/TypeScript tests and the existing `targeted-domain` gate profile. Reviews follow `architecture-3` (independent reviewers under the existing policy). Canonical full gates are owned by CI/orchestration, not repeated locally by every implementer. Commands in this section are future implementation obligations, not claimed executions in the planning session.


## 15. Successors and externally visible promises

Successors may consume only the accepted products above after the existing ledger records completion. No proof report expands public support beyond its covered manifest. Full Vue typing/public declarations/IDE is STP59; full Svelte typing/IDE is STS15. Runtime compilation and future Astro/MDX/Lit support retain their separate roadmap claims.

## 16. Completion evidence

Record actual source revision, input/framework/engine pins, selected case IDs, commands and raw outcomes in `tests/sfc-projection/evidence/STP1/` or the existing CI artifact owner. Include changed symbols, exact deletion population, review findings and why applicable negative tests discriminate the behavior. Never mark a pending test case executed by copying this charter into an evidence file.

## 17. Roadmap consistency

This node is part of the single final STP/STS plan, not the previous VTP/VTC suggestions. Keep TOML, portable definition, inline/mirrored charter and pending state aligned. Receiving amendments are explicitly listed in section 13 of `contracts/sfc-typescript-projection.md`. Do not add reverse edges to TCM4, BR0 or other ancestors; run the combined graph validator after every amendment.
