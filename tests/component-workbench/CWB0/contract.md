# Component workbench and design-system contract

This contract defines source-linked component exploration over the existing
[ComponentInfo view](../../../docs/arch/kernel/tif1d-componentinfo-view.md).
It describes the repository at `docs(meta): complete component view roles and
consumer inventory`, 2026-10-12. The title/date locate context; they are not
acceptance evidence. This is contract text, inventory and case tables only,
under the [docs-only rule](../../../docs/arch/kernel/README.md). No executable
support, new metadata extractor, runtime, validator or test is delivered here.

The sole product owner is component-contract exploration and execution
adapters over TypeInfo, runtime inspection and SDK. CWB0 owns this decision;
CWB1–CWB9 own the implementation and delivery below. Accepted predecessor
completion comes from the controller's task state, not source presence or a
repository ledger. The earlier TIF1D deferral ruling retains that acceptance
barrier; no ledger or replacement completion store is created. TIF1D defines
the consumed schema; TIF1 implements it before CWB1 executes.

## Intent and imported authority

A design-system author can inspect a component's declared and accepted
contract, find consumers and authored examples, navigate its declarations and
style/accessibility evidence, and explicitly choose a supported preview.
Missing facts stay missing. A displayed type, generated example or installed
upstream package cannot establish runtime defaults, safe inputs or product
support. Static exploration never imports executable stories or mounts a
component.

Use TypeInfo's typed component selector, OP10 facet demands and result envelope
from [the facade contract](../../../docs/arch/kernel/typeinfo-facade.md).
ComponentInfo retains the owning child observation's QueryIdentity,
InputBasisId, profile, exact release and NodeRef roles; a composed container
does not acquire a fictitious single basis. The catalog, source registry,
index, maps, query scheduler and edit transactions remain their existing
authorities. Workbench selection is presentation state, not a semantic cache
or a replacement project graph.

The [WDX0 evidence classes](../../web-product/WDX0/products/evidence-class-policy.v1.json)
separate static proof, runtime observation and estimates. Existing
[FeatureExposureContract](../../product-experience/DX0/products/feature-exposure-contract.v1.json),
[HostExecutionClass](../../product-experience/DX0/products/host-execution-class.v1.json)
and [ProductReceiptBasis](../../product-experience/DX0/products/product-receipt-basis.v1.json)
govern claims. Their historical ledger/path references do not recreate removed
repository governance; the controller owns readiness. The charter's legacy
`contracts/web-product-expansion-v1.md` companion is absent from this candidate
and dispatched packet. This document grounds the bounded decision in the
frozen charter and available predecessor artifacts; it claims no inspection
or ratification of that unavailable companion.

## Observation and provenance contract

These are requirements on existing contribution/result interfaces, not a new
DTO or another registry. Each contributed item retains:

- The configured project, canonical SourceUnitId and exact export selector,
  framework/profile and exact producer/runtime/adapter release. A basename or
  display name is never identity, including external and framework-discovered
  components with no direct local import.
- Its owning query and child basis, requested facet/role and producer, member
  occurrence/declaration slot, source revision and validated authored map.
  Style blocks retain sealed block identity; ordinal indexes are display only.
- Independent domain membership, member-value and boolean guarantees with
  typed causes, conditional availability and the existing delivery status.
  Complete-empty, partial, pending, unsupported, ambiguous, failed, cancelled
  and stale remain distinct. A missing/undemanded facet proves no absence.
- Authored documentation/expressions, semantic facts, generated suggestions
  and runtime observations as separate evidence. A runtime observation also
  binds source/config/build/engine/host/session and execution grant, inputs and
  scenario. Optional origin/audit capture is explanation, never required type
  authority or a substitute for observation provenance.

Each type-bearing prop, event parameter/return, slot binding/return, model or
exposed member resolves through its exact TypeInfo child/role, including
overloads and inherited effective scope. Terminal display, raw annotations,
emitted declaration text and JSDoc text cannot recover missing meaning.
TypeScript-authoritative demands keep the certified engine binding; an
unavailable provider is unavailable, never a native substitute.

Semantic equality uses the shared TypeInfo/signature ordering contract.
Presentation retains source order where it is meaningful (including event
overloads, tuple positions and story order). Catalog discovery order cannot
change identities or choose a facet. Stable presentation ties use canonical
source/export/profile and producer occurrence identity, not discovery timing.

### Default and control evidence

Default presence, authored default text, an evaluated runtime value, an
example input and a generated suggestion are different facts. `hasDefault:
undefined` means no published boolean, not false. `hasDefault: true` proves
presence only; a factory expression is not its evaluated value. Even complete
type information cannot turn a missing default into `0`, `false`, `''`, `{}`,
`[]` or the first union arm. JSDoc `@default` is authored documentation unless
the owning producer independently certifies it.

Controls consume supported semantic types and their guarantees. An editor
must preserve null/undefined choices, optionality, overload/branch conditions
and user omission. A literal select offers choices without selecting a
default. Unresolved references, deferred operations, functions, recursive or
complex/conditional contracts without a safe editor remain explicit and
disabled or require authored input. No text/object fallback asserts safety.
Type representability is not proof of runtime constraints; updates need the
owning validation/preview result, and invalid or refused updates retain their
causes. Authored examples override suggestions only as example inputs, never
as semantic defaults. CWB1-AC-R1 owns the default counterexample; CWB3C-AC1/AC2
own editors, CWB4/CWB5 own their adapter projections.

## Required capability matrix

Every row is required downstream work, even when its current producer is
partial or unimplemented. Unsupported is an observable result for a specific
profile/demand, not completion of the row. Producer ownership is retained;
the named CWB owner owns only consumption and delivery.

| Capability                      | Existing source/authority and limits                                                                                                                                                                                                                                                    | Receiving owner and acceptance                                  |
| ------------------------------- | --------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | --------------------------------------------------------------- |
| Component catalog and consumers | Workspace/index candidates plus ComponentInfo observations; local, external, reexported and framework-discovered candidates survive. Index membership is not resolved type truth or exhaustive usage when reachability is partial.                                                      | CWB1-AC1/AC2/AC3; workspace/index producers retain discovery    |
| Props and accepted inputs       | `PropMeta`, native facts, accepted/fallthrough branches and structured public contract; retain required/default guarantees, declared/inherited provenance and conditional availability independently.                                                                                   | CWB1-AC1/AC-R1; TIF1 retains semantic view production           |
| Events                          | Event occurrence/signature/parameter/return roles and accepted listeners; preserve duplicate overloads, labels, optional/rest flags, branch and source provenance. An adapter action key is presentation, not event identity.                                                           | CWB1-AC1; CWB4-AC1 and CWB5-AC1 for their projections           |
| Slots/children/yields           | Profile-qualified facet and typed inputs/bindings/returns, requiredness/fallback guarantees; do not impose Vue slots on other profiles or infer meaning from legacy return text.                                                                                                        | CWB1-AC1; CWB3C-AC1 for supported editing                       |
| Models                          | Profile producer's model binding/value and update relationship when published; no name-based reconstruction from props/events. Unsupported model facets remain explicit.                                                                                                                | CWB1-AC1; CWB3C-AC1                                             |
| Exposed API                     | Exposed members and separately demanded public-instance surface, with TypeInfo roles and exactness; browsing never invokes a method.                                                                                                                                                    | CWB1-AC1; CWB3-AC1 for explicitly admitted interactions         |
| CSS variables/classes/parts     | Existing style facet's block token, custom properties, classes, selectors and v-bind links; CSS/Custom Element producers supply parts and export/forwarding semantics. `StyleMeta` has no CSS-parts field today; selector text alone proves neither exported parts nor computed values. | CWB2-AC1; CSS and Custom Element owners retain analysis         |
| Accessibility                   | Link producer-owned static projections and separate runtime observations with dynamic limits; projected accessibility is not verified behavior or conformance.                                                                                                                          | CWB2-AC1/AC2; accessibility/runtime producers retain algorithms |
| Controls                        | Supported TypeInfo types with explicit unsafe/unknown/conditional limits; suggestions, authored args and runtime defaults remain distinct.                                                                                                                                              | CWB3C-AC1/AC2; CWB1-AC-R1 for the default law                   |
| Isolated preview                | Official framework runtime through approved execution service, explicit grant and source/config/build/session receipt, isolation and disposal. Static metadata has no execution permission.                                                                                             | CWB3-AC1/AC2/AC3                                                |
| Storybook                       | Existing `toArgTypes`; admitted story/args contract linked to the same canonical component. Static indexing cannot evaluate stories/config.                                                                                                                                             | CWB4-AC1/AC2/AC4                                                |
| Histoire                        | Existing `toHistoireConfig`, `generateDefaultProps`, `generateVariants`; Histoire-specific story semantics and execution boundary, no Storybook convention substitution.                                                                                                                | CWB5-AC1/AC2/AC4                                                |
| Examples/documentation/tests    | Source-linked authored usages, stories, JSDoc and test assertions; generated snippets stay suggestions until actual mount/interaction evidence exists. Documentation tags do not change types/defaults.                                                                                 | CWB6-AC1/AC2/AC4; DOC1 retains executable documentation proof   |
| Workbench views                 | Lazy component tree, contract, consumers, examples, styles/a11y and preview over one observation owner; keyboard access and clear permission/uncertainty states.                                                                                                                        | CWB7-AC1/AC2/AC4; CWB8-AC1                                      |
| Safety and public product       | Real supported-story/control/interaction and teardown tests, affected/unaffected controls, framework isolation and explicit host limits; adapters alone cannot qualify a workbench.                                                                                                     | CWB8-AC1/AC2/AC3/AC5; CWB9-AC1/AC2/AC4                          |

## Actual consumer inventory and cutover

Paths identify inspected production consumers and their adjacent coverage.
The complete upstream metadata population stays in TIF1D's field/consumer
inventory; this table narrows product-facing consumption without transferring
its semantic cutover to CWB. No separate workbench implementation exists at
this candidate.

| Current route                                                                                                | Evidence and present behavior                                                                                                                                                                                                               | Exactly one surviving boundary owner; disposition                                                                                                                                                                                                                                                                    |
| ------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `packages/component-meta/src/index.ts`, `browser.ts`, `types.ts`, `native-component-meta.ts`                 | Public decoded metadata, native facts, public contract, usage, style and JSDoc; browser entry excludes Node session APIs.                                                                                                                   | TIF1 (`TIF1-AC1/AC2`): view/transport cutover. Retain public projections, retire duplicate semantic authority under its existing inventory. CWB1 consumes the resulting view.                                                                                                                                        |
| `packages/component-meta/src/adapters/storybook.ts::toArgTypes`                                              | Export `@verter/component-meta/storybook`; props/controls and event actions. Complex fallback can produce text/object controls; property/action key collisions and incomplete facts need explicit handling.                                 | CWB4 (`CWB4-AC1/AC2`): own Storybook story/args integration over the shared control contract. Retire unsafe fallback and name-only collision authority; keep lawful presentation and migrate exports/docs together. The underlying metadata-view adoption remains TIF1's C09, not a second CWB resolver.             |
| `packages/component-meta/src/adapters/histoire.ts::{toHistoireConfig,generateDefaultProps,generateVariants}` | Export `@verter/component-meta/histoire`; synthetic primitive/container/first-union values, named "Default", independent of presence facts. Only the first all-literal union prop drives variants; none of this certifies runtime defaults. | CWB5 (`CWB5-AC1/AC2`): own Histoire integration, classify suggestions/authored args separately from producer-backed defaults. Retire unqualified auto-seeding and story semantics inconsistent with admitted Histoire versions; migrate exports/docs together. Underlying metadata-view adoption remains TIF1's C09. |
| `packages/component-meta/src/adapters/json-schema.ts`, `zod.ts`                                              | Validation presentations over metadata descriptors; adjacent specs preserve unknown requiredness. They are not workbench runtime/default producers.                                                                                         | TIF1 (`TIF1-AC2`): retain mechanical validation projections; workbench controls consume their published limitations without private resolving.                                                                                                                                                                       |
| `packages/component-meta/src/compat/native-projection.ts`, `compat/index.ts`, runtime session APIs           | Compatibility/session presentation over one native response, with declared/accepted policies and host lifetime.                                                                                                                             | TIF1 (`TIF1-AC1/AC2/AC-R1`): retain mechanical compatibility and close semantics there, not in adapter code.                                                                                                                                                                                                         |
| `packages/playground/src/core/compiler.ts`, `editor/carrierStore.ts`                                         | Public/declaration carriers and browser source mappings. The `componentMeta` output alias is analysis presentation, not a component explorer query/preview product.                                                                         | TIF1 (`TIF1-AC2/AC-R1`): preserve existing output behavior; CWB7 adds its own shared workbench views without relabeling this route as completed support.                                                                                                                                                             |
| `packages/component-meta/readme.md`                                                                          | Current metadata reference, including adapter availability.                                                                                                                                                                                 | TIF1 (`TIF1-AC2`): own metadata-reference migration; consuming product nodes supply their operation/version/permission evidence.                                                                                                                                                                                     |
| Exported adapter interfaces and co-located specs                                                             | Repository search finds adapter calls only in their specs; no live Storybook/Histoire application consumer or upstream dependency pin is present.                                                                                           | CWB4/CWB5 (`CWB4-AC1/AC4`, `CWB5-AC1/AC4`), each solely for its own adapter interface/spec population. Preserve current assertions until deliberate behavioral cutover; add no fictitious app consumer.                                                                                                              |

Deletion population **in this decision: empty**. It adds a contract boundary
and reviewed inventory, with no production replacement. The table binds
actual later cutovers; it does not invent a retired API or authorize a second
metadata authority. CWB1 builds a lazy catalog over workspace/index sources,
not another recursive project scanner. CWB3 adapts the approved execution
service rather than shipping a framework runtime. CWB7 reuses the shared
selection/view owner rather than a hidden cross-project store. CWB6 owns the
new workbench example/test links, not the current metadata reference or either
adapter's semantic adoption. Each ownership layer above is a distinct
boundary; upstream view production and product integration do not compete.

### Exact versions and source evidence

These are candidate inventory facts, not supported-version declarations.
Dependency integrity pins in `pnpm-lock.yaml` retain their operational purpose
and are not changed here.

| Source                                        | Candidate identity                                                                                           | Meaning and receiving obligation                                                                                                                                                                        |
| --------------------------------------------- | ------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `packages/component-meta/package.json`        | `@verter/component-meta@0.0.1-beta.6`; `./storybook` and `./histoire` exports point to the authored adapters | Verter-owned presentation APIs exist. This supplies no evidence of upstream story execution.                                                                                                            |
| Root `package.json`, `pnpm-lock.yaml`         | Vue/compiler `3.6.0-rc.5`; Svelte `5.56.10`; TypeScript `7.0.2`                                              | Dependency identities for this candidate, not automatic framework/workbench qualification. Profile producer evidence must bind the actual selected release.                                             |
| `packages/playground/package.json`            | Vue `3.6.0-rc.5`; browser TypeScript `6.0.3`                                                                 | Browser editor/compiler identities differ from the root TypeScript toolchain. Portable compilation does not prove Portable OP10 or preview.                                                             |
| Storybook/Histoire package and lock inventory | No upstream Storybook/Histoire dependency or exact version pin                                               | Upstream supported versions are unqualified. CWB4/CWB5 must bind exact admitted package/version/config and authored operation replay before claims; do not infer support from structural TS interfaces. |

New exposure rows use the existing VIM/DX contract and exact catalog profiles,
operation, producer, maturity, required obligation, semantic/inspection rail,
host execution class, executable route and replay case. CWB1–CWB7 each own
their operation's row; CWB9 checks completed artifacts and routes to the WDX
qualification terminal. Registration or this decision cannot promote support.

## Execution, source navigation and lifetime

Static catalog/contract/style/documentation views run with read authority and
do not evaluate project config, component modules, stories or default factories.
Preview, interaction, example execution and config evaluation require an
explicit scoped execution intent/grant through the existing execution/trust
owner. CWB3 uses official admitted framework runtimes; Verter ships no runtime
code. Grants identify project/component/story, host/profile, config/build and
allowed effects. Absence or revocation refuses execution; it cannot trigger a
different host or engine silently.

Preview receipts bind ProductReceiptBasis plus selected story/input and the
actual build/session/runtime. A browser Portable route and an explicit local
native route have separate evidence; a NativeOnly or ExternalOwner operation
cannot be relabeled browser-executable. Observed values carry scenario and
conditions, never universal default or accessibility claims.

Selection change, edit invalidation, cancellation, failure, grant revocation
and host closure dispose leases and preview resources: mounts, observers,
listeners, pending work and execution-service handles. No runtime crosses
component/project/profile sessions. Late responses check the current basis
and selection before display; cancelled/partial wrappers do not warm a
complete semantic cache. Reuse follows each TypeInfo child's existing
read-set/admission law; workbench adds no result cache or polling timer.

Navigation uses producer-authored maps at the exact revision. Missing or stale
maps return explicit unavailability, not regex search, clamped offsets or
ordinal fallback. This contract creates no source edit operation; later edits
must use the existing reversible edit transaction and current map validation.

## Public delivery and acceptance

CWB1–CWB7 document their real operation, permissions, unsupported/partial
states and exact framework/host/adapter versions with DOC1-tested public
examples/reference. CWB4/CWB5 migrate adapter callers, exports and public
examples at cutover; suggestions cannot retain a runtime-default label.
CWB6 separates authored examples from generated ones and binds executed
results to assertions and reproducible inputs. CWB9 verifies the delivered
catalog, contract, controls, preview, source links, stories, consumers,
styles/a11y and keyboard workflows on supported hosts, with uncertainty and
migration notes visible. Metadata snapshots or screenshots alone do not pass.

The [acceptance case table](../evidence/CWB0/cases.md) records existing
coverage and required downstream counterexamples, not execution transcripts.

| Acceptance | Evidence in this decision                                                                                                                                                                                                              |
| ---------- | -------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| CWB0-AC1   | Observation law, full capability matrix, actual consumer inventory and single cutover owner per route. Executable import enforcement belongs to CWB1-AC-R1.                                                                            |
| CWB0-AC2   | Default law and explicit existing Histoire gap; case table distinguishes existing assertions from the missing end-to-end discriminator owned by CWB1-AC-R1. No duplicate or new test here.                                             |
| CWB0-AC3   | No state/query/map or scheduling code changes, so fresh/incremental, edit/revert, cancellation and ordering execution are not applicable here. Requirements remain binding on CWB1/CWB3/CWB3C/CWB4/CWB5/CWB7, with CWB8 qualification. |
| CWB0-AC4   | Operation/profile/host evidence, permissions, provenance and DOC1/public migration obligations are specified above for each producer; CWB9 must verify actual artifacts. This decision makes no product support claim.                 |
| CWB0-AC5   | No hot path, runtime or allocation behavior changes and no new budget or measurement. Additive deletion set is explicit; production cutovers have named owners.                                                                        |

Later cost comparisons follow the standing measurement rule: equivalent
answers and work counts/growth against the baseline, measured on the same
assigned worker/session when compared. Timings/allocation/RSS are evidence,
not machine-specific acceptance thresholds here; unavailable metrics are
reported as unavailable. CWB8 qualifies lazy fan-out and bounded retention
without dropping required facets/components or shrinking the workload.
