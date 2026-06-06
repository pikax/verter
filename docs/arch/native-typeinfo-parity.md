# Native Typeinfo Parity — Full TypeScript-Parity Checker Architecture

> **Sequencing authority:** `docs/arch/semantic-db-overhaul-unified-remaining-plan.md`.
> This document is the **parent architecture** for Verter's full TypeScript-parity
> native typeinfo engine. It owns the engine architecture, the capability map, the
> query/fact authority, the per-block execution framework, and the manifest/cutover
> protocol. It does **not** define an independent phase ladder, a competing
> schedule, or a review-round log.
>
> **Foundation:** `docs/arch/semantic-type-graph-plan-recovered.md` owns the
> graph / wire / cache foundation this architecture builds on.
>
> **Owning blocks:** `U0`–`U15` (the unified plan reserves these block IDs and
> links here). Where this document must express ordering it expresses it as a
> dependency relationship, not a competing stage ladder.
>
> **Native checker (sibling follow-up).** Typeinfo parity (this architecture, the
> `U0`–`U15` blocks) is the **foundation**; the native checker is a **later layer**
> over the **same** resolver (`SemanticQueryKey → ProjectSemanticDispatch::execute →
> SemanticGraphStore`), specified as a sibling plan in
> `docs/arch/native-checker.md`. The typeinfo blocks **reserve** the checker seams —
> the `SemanticQueryValue::DiagnosticAnalysis(CheckResult)` value arm + the `Check*`
> query names (§3), the `ExecutableRegionId` / `ExecutableRegionKind::Function` region
> abstraction, and the `ProgramAnalysisContributor` injection seam (§5) — but do **not**
> build the checker. It is **not** part of the 362-row parity blocks.

The end state is a full native checker-grade typeinfo engine — not a larger
flow-return patch. It has **one resolver**:
`SemanticQueryKey → ProjectSemanticDispatch::execute → SemanticGraphStore`. There
is no OXC query-time resolver, no tsserver/tsgo execution path, no projection
repair path, and no whole-body typecheck path. OXC is the syntax/lowering
front-end only.

**The target is FULL TypeScript-checker-grade type parity** — relation /
assignability, inference, measured variance, control-flow narrowing, conditional /
mapped / template reduction, overload resolution, and the cross-engine recursion —
an **honest multi-person-year scope** (sequencing authority §"Scope"). The **362-row
ledger (PART 2 §10) tracks COVERAGE / wiring, NOT semantic tsc-parity**: 362-green
proves every row is owned + executably proven + wired ("detects un-wired"), it does
**not** prove the engine agrees with `tsc`/`tsgo` on the families' SEMANTICS
("detects wrong"). **Semantic tsc-parity is gated separately by the differential
`tsgo`-parity oracle (§6.3)** — a per-family divergence budget over a TS-conformance
slice + property-generated fixtures, baselined at each hard phase's **rescope gate**
(sequencing authority §3.2). The hardest cores (`U2.RELATION_INFER`, U6 cross-engine
recursion, the native checker, U7) are **RESCOPE-GATE-REQUIRED**: their
algorithm-depth design is produced at a pre-implementation rescope session (planner +
1-Claude+2-codex panel iterating to best-architecture, with a real termination proof
where recursive), NOT specified up-front here. The "route through `Relate` / the one
engine" phrasing throughout is the one-engine WIRING constraint, not a substitute for
those phases' algorithms.

---

## Doc-set index

The architecture is documented as one parent (this file) plus four child
subplans. Each child references back to this parent **and** to the sequencing
authority via the standard subplan header (below). The no-orphan-documents target
**is reached**: the unified plan
(`docs/arch/semantic-db-overhaul-unified-remaining-plan.md`) indexes the parent
**and** all four children — in its §A doc-set index table and at the owning
`U0`/`U2`/`U3`/`U6`/`U8`/`U10`–`U15` blocks — and every subplan links back to the
unified plan and to this parent architecture. The unified plan no longer carries a
stale `/tmp` coverage path; the flow-return coverage detail lives in
`native-flow-return.md` and the §10.4 / §10.4.1 coverage table. That index was a
DELIVERABLE owned by the unified-plan integration step (see "Cross-reference /
doc-update obligations") and is now **done**.

| File | Owns |
|---|---|
| `docs/arch/native-typeinfo-parity.md` | **Parent architecture** (this file): engine architecture, capability map, query/fact authority, the per-block contract template, the two-table manifest ledger, the git/CI landing protocol, the guards index |
| `docs/arch/native-typeinfo-parity-u2-reducers.md` | **U2** child blocks: reducer / relation / utility / indexed / mapped / template / class / enum / module / JSX foundations |
| `docs/arch/native-flow-return.md` | **U6** flow chapter: the per-function `FunctionFlowGraph` + the `ReturnPathPeeker` graph demand planner (two-frontier rule as typed edge classes) and the flow IR |
| `docs/arch/native-typeinfo-parity-cache-export-session.md` | **U3 / U8 / U10 / U11 / U12 / U13**: facts, exporter, DB, session, projections, wire |
| `docs/arch/native-typeinfo-parity-adapters-final-lift.md` | **U14 / U15**: framework adapters, integrations, final lift |

Every subplan begins with this header (the requirement is stated here and is
binding on each child):

```md
Parent architecture: docs/arch/native-typeinfo-parity.md
Sequencing authority: docs/arch/semantic-db-overhaul-unified-remaining-plan.md
Owning U-block(s): ...
Prerequisites: ...
Consumers: ...
Progress ledger: crates/verter_session/tests/typeinfo_ignored_test_manifest.rs
```

The unified plan must index every subplan in its `U0`/`U2`/`U6`/`U15` sections.
Every subplan must link back to the unified plan and the parent architecture.

---

# PART 1 — The Engine Architecture

## 1. Type IR — three layers

Internal type meaning lives in `SemanticNodeData`; boundary / projection carriers
live in `TypeExpr`; published transport values live in `GraphTypeNode`.
`GraphTypeNode` contains **type values only**. Flow facts, contextual-typing
facts, and narrowing facts live in `ProgramAnalysisGraph`, never as published
type nodes. Declaration / environment-mutation facts are an in-process VALUE
domain (`SemanticQueryValue::DeclarationAnalysis`), distinct from the wire: on
the wire, module / global augmentation REMAIN `GraphTypeNode` arms (kinds 23 / 25
— see §1.3). There is no `DeclarationAnalysisGraph` wire message; the proposed
relocation of the augmentation arms off `GraphTypeNode` was **rejected**.

Consequently the query engine's result domain is **typed**, not uniformly
`SemanticNodeId`: each `SemanticQueryKey` resolves to its correct value domain via
the typed `SemanticQueryValue` layer (see §3), so flow / contextual keys return
`ProgramAnalysisGraph` values and augmentation keys return the in-process
`SemanticQueryValue::DeclarationAnalysis` value domain rather than smuggling a
non-type value into `GraphTypeNode` — while the augmentation graph nodes
themselves stay on the wire as `GraphTypeNode` kinds 23 / 25.

### 1.1 `SemanticNodeData` coverage

`SemanticNodeData` must cover: object / member surfaces; unions / intersections;
literals; tuples / rest / labels; type parameters with variance / default / const
metadata; signatures with `SignatureEffect` metadata; overload sets; class
surfaces (including abstract class surfaces — §1.6 — and decorator / auto-accessor
member surfaces — §1.7); enum value / type duality; unique-symbol identities;
mapped / conditional / template / indexed-access shells; merged declarations;
module augmentations; ambient namespaces; and typed degradation / cycle nodes.

`SignatureEffect::{Predicate, Assertion, AssertsCondition}` is metadata on
function signatures, not a standalone published `TypeExpr` / `GraphTypeNode`.
`Awaited`, generator, async-generator, optional-chain, and unique-symbol carriers
are first-class semantic carriers. `FlowSlot` is forbidden outside flow IR.

### 1.2 `NoInfer` is occurrence-local

`NoInfer` is occurrence-local. It is **not** type-parameter declaration metadata.
It is recorded where the `NoInfer<...>` occurrence appears (a wrapped parameter
position / inference site), on `NoInfer` nodes / the signature parameter inference
policy. A signature's parameter inference policy reads the `NoInfer` occurrence to
suppress inference for that position.

`GraphTypeParameter` carries **no** `no_infer` field. The wire tag `9` + the name
`no_infer` are retired and `reserved` on `GraphTypeParameter` and never reused;
off-tree clients keep round-tripping the retired tag as an unknown field. No
replacement `no_infer` field is added — `NoInfer` is carried occurrence-locally.
Pinned by **`no_infer_not_type_parameter_metadata`**.

### 1.3 `GraphTypeNode` purity — whole-class closure

`GraphTypeNode` carries **only** type-value arms. This is closed as a class, not
discovered arm-by-arm. The proto's `GraphTypeNode` `oneof kind` is enumerated and
every non-type-value arm is retired (its tag + name moved into the enclosing
message's `reserved` list at message scope, never reused — proto3 forbids
`reserved` inside an `oneof`) and relocated to its end-state home.

The closed **type-value allowlist** that remains on `GraphTypeNode` — each arm
carrying an explicit type-value classification — is: `primitive` (1), `literal`
(2), `unique_symbol` (3), `union` (4), `intersection` (5), `object` (6), `array`
(7), `tuple` (8), `reference` (9), `alias_instantiation` (10), `type_parameter`
(11), `key_of` (12), `indexed_access` (13), `conditional` (14), `mapped` (15),
`template_literal` (16), `typeof_node` (17), `satisfies_node` (18), `class_node`
(19), `this_type` (20), `merged_declaration` (21), `ambient_module` (22),
`module_augmentation` (23), `ambient_namespace` (24), `global_augmentation` (25),
`infer_node` (29), `enum_node` (30), `opaque` (31), `cycle` (32). The
declaration-surface arms `merged_declaration` / `ambient_module` /
`ambient_namespace` remain because each is explicitly classified as a
value-bearing namespace / object-type surface whose members are queryable type
values (the same object surface `ResolveMergedDeclaration` /
`ResolveAmbientNamespace` projects). `module_augmentation` (23) and
`global_augmentation` (25) likewise REMAIN as retained value-bearing augmentation
arms: the proposed relocation to a `DeclarationAnalysisGraph` side surface was
**rejected**, so these arms are the wire home (see the relocation note below and
`u2-query-value-domain-design.md` §2.2). The in-process
`SemanticQueryValue::DeclarationAnalysis` value domain is the value-side
counterpart, not a wire relocation.

Every arm that does NOT remain a value-bearing arm on `GraphTypeNode` relocates
(the augmentation arms 23/25 are the explicit exception — see their bullet below,
where the relocation was rejected and they stay on the wire):

- **Flow narrowing (tag 26) and contextual type (tag 27)** → `ProgramAnalysisGraph`
  (program-analysis facts). The arms are retired + `reserved` (tags `26`/`27` +
  names `flow_narrowing`/`contextual_type` at message scope), and the facts are
  exposed through `ProgramAnalysisGraph { flow_narrowings, contextual_types }` on
  `TypeInfoGraphPayload.program_analysis`. This is **mandatory-both**: the arms are
  retired **and** the facts remain queryable — `FlowNarrowingAt` / `ContextualTypeAt`
  resolve to `ProgramAnalysisGraph` payload entries, never `GraphTypeNode` arms.
- **Relation proof (tag 28)** → `RelationPayload` / a payload-side proof table. A
  relation proof is a non-type **value** (the assignability judgement's evidence),
  not a type. The arm is retired + `reserved` (tag `28` + name `relation_proof`).
  Public `relate` returns its proof through a dedicated `RelationPayload`; where
  graph payloads need a proof reference, the proof lives on a payload-side proof
  table `TypeInfoGraphPayload.relation_proofs` keyed by an opaque proof id (graph
  nodes carry the proof id, never the proof value itself).
- **Module augmentation (tag 23) and global augmentation (tag 25)** — the proposed
  relocation to a `DeclarationAnalysisGraph` side surface was **rejected** (see
  `/type-resolution` → Merge/augmentation WIRE domain, and
  `u2-query-value-domain-design.md` §2.2). The authoritative landed decision keeps
  these arms as the wire home: `GraphTypeNode` kinds **21–25** REMAIN, and the live
  proto still carries `module_augmentation = 23` / `global_augmentation = 25` as
  `GraphTypeNode` arms — they are NOT retired, NOT `reserved`, and NOT relocated.
  The in-process `SemanticQueryValue::DeclarationAnalysis` value domain is the
  value-side counterpart, not a wire relocation.
- **Diagnostics and diagnostic directives** → `TypeInfoGraphPayload.diagnostics` /
  `TypeInfoGraphPayload.diagnostic_directives` (and off `SemanticTypeGraph`, §1.5).

The class is closed mechanically by the landed taxonomy guard:

- **`node_taxonomy_complete`** (`crates/verter_session/tests/g_block/typeinfo_graph_contract_guards.rs`)
  — scans the proto's `GraphTypeNode` `oneof kind` and pins the EXACT 32-arm set,
  INCLUDING `module_augmentation` (23) and `global_augmentation` (25), plus the
  additive `reserved 33 to 100;` window at message scope. Adding, dropping, or
  renaming any arm fails the assertion, so a future arm change is a deliberate
  schema-version bump, not a silent tag grab. (The earlier-planned split guards
  `graph_type_node_oneof_contains_only_type_value_arms` /
  `graph_type_node_allowlist_arms_have_type_value_classification` were NOT landed;
  the single exact-set `node_taxonomy_complete` assertion subsumes them, and it
  treats the augmentation arms 23/25 as valid live graph state, not as
  retired/relocated.)

With it registered, no future arm needs case-by-case handling: the whole
`GraphTypeNode` taxonomy is
closed by one enumerating assertion plus one classification assertion.

The DTO end-state shape is:

```
TypeInfoGraphPayload {
    graph,                  // the GraphTypeNode type-values / topology surface
                            // (module/global augmentation are GraphTypeNode arms 23/25 — NOT a side surface)
    program_analysis,       // ProgramAnalysisGraph { flow_narrowings, contextual_types }
    diagnostics,
    diagnostic_directives,
    relation_proofs,        // payload-side proof table, referenced by proof id
}
```

### 1.4 Typeinfo wire-surface purity — whole-surface closure

The `GraphTypeNode` oneof closure (§1.3) closes only the `GraphTypeNode` oneof; it
does not cover non-`GraphTypeNode` sites such as `FrameworkSurfacePayload.graph` or
`GraphTypeParameter.no_infer`. To stop discovering contradicting wire sites
site-by-site, one comprehensive deliverable reconciles the **entire** public proto
surface with the moved-concept end-state, under the Typeinfo Wire Contract
(closed-enum discipline + wire-compat + additive-audit + validate-before-execute):

- **(a) Every `SemanticTypeGraph` embedding uses `TypeInfoGraphPayload` / an
  additive side-table, never a retype.** Every public message field typed
  `SemanticTypeGraph` is migrated to carry `TypeInfoGraphPayload` (or its own
  additive side-table exposing the moved-off facts) as the server-populated graph
  surface — the original field retired/`reserved` or kept only behind a registered
  versioned downgrade encoder, its type never changed in place, its field number
  never reused (§1.5).
- **(b) Every field/arm representing a relocated/retired concept is retired +
  `reserved` + relocated.** Diagnostics + diagnostic directives, relation proofs,
  flow narrowing, contextual type, `no_infer` type-parameter metadata, and any
  other relocated/retired concept is, wherever it appears, retired (its tag + name
  in the enclosing message's `reserved` list, never reused) and relocated to its
  end-state home (`ProgramAnalysisGraph` for program-analysis facts, a
  `TypeInfoGraphPayload` side table such as `diagnostics` / `diagnostic_directives`
  / `relation_proofs`, a `RelationPayload`, or an occurrence-local node), or removed
  outright where it has no end-state value (`no_infer`). Module / global
  augmentation are NOT in this set: their relocation off `GraphTypeNode` was
  **rejected** — they REMAIN value-bearing `GraphTypeNode` arms 23 / 25 on the wire
  (§1.3), with `SemanticQueryValue::DeclarationAnalysis` as the in-process
  value-side counterpart (no wire side surface).

The wire-surface-purity class is closed by the landed guards:

- **`node_taxonomy_complete`** (`crates/verter_session/tests/g_block/typeinfo_graph_contract_guards.rs`)
  — pins the EXACT 32-arm `GraphTypeNode` `oneof kind` set (INCLUDING
  `module_augmentation` 23 and `global_augmentation` 25 as live arms) plus the
  additive `reserved 33 to 100;` window. This is the single enumerating assertion
  that closes the `GraphTypeNode` taxonomy; it treats arms 23 / 25 as valid live
  graph state, never as retired/relocated.
- **`all_public_semantic_type_graph_embeddings_are_payload_wrapped`** — the
  whole-class embedding guard (§1.5).

NOTE — NOT LANDED: the earlier-planned denylist guard
`typeinfo_wire_surface_has_no_retired_concept_fields` and the two split
`GraphTypeNode` guards `graph_type_node_oneof_contains_only_type_value_arms` /
`graph_type_node_allowlist_arms_have_type_value_classification` were never landed
(they do not exist in `crates/`). The single exact-set `node_taxonomy_complete`
assertion subsumes them, and it must NOT denylist `module_augmentation` /
`global_augmentation` — those arms are live wire state.

### 1.5 `SemanticTypeGraph` embeddings and the response landing

`SemanticTypeGraph` carries graph topology and type values **only**; diagnostics,
relation proofs, and flow/contextual facts belong to the payload, not the graph.
(Module / global augmentation are value-bearing `GraphTypeNode` arms 23 / 25 and
therefore DO live on the graph — see §1.3; only their in-process VALUE-domain
counterpart `SemanticQueryValue::DeclarationAnalysis` is off-graph, and it is not
a wire surface.)

- **Diagnostics ownership migration.** `SemanticTypeGraph.diagnostics` (tag 9) is
  retired — tag `9` + name `diagnostics` move into `SemanticTypeGraph`'s `reserved`
  list (schema-version bumped), never reused. Diagnostics move to
  `TypeInfoGraphPayload.diagnostics` and directives to
  `TypeInfoGraphPayload.diagnostic_directives`. Pinned by
  **`diagnostics_only_on_typeinfo_graph_payload`**.
- **`TypeInfoGraphPayload` response landing (additive, never retyping field 1).**
  The current success arm of the response is `SemanticTypeGraph graph = 1` directly
  in `TypeInfoGraphResponse`. The new payload lands **additively**: add
  `message TypeInfoGraphPayload`; retire field `1` (move tag `1` + name `graph`
  into `reserved`, OR keep it alive only behind a registered versioned downgrade
  encoder); add `TypeInfoGraphPayload payload = <next free tag>` as the current
  success arm the server populates. Field `1`'s type is never changed in place — it
  stays `SemanticTypeGraph` (reserved or downgrade-only), and the new payload takes
  a fresh tag, not a recycled one. Schema-version gated. Pinned by
  **`typeinfo_graph_response_payload_arm_is_additive_not_retyped`**.
- **All public `SemanticTypeGraph` embeddings migrate.** Every other public wire
  carrier that embeds `SemanticTypeGraph` directly migrates the same way: it carries
  `TypeInfoGraphPayload` (or its own additive side-table exposing the moved-off
  facts) as the server-populated graph surface; the original embedding is
  retired/`reserved` or kept only behind a registered versioned downgrade encoder;
  the type is never changed in place; the field number is never reused. The concrete
  case is `FrameworkSurfacePayload.graph = 4`: tag `4` + name `graph` are
  retired/`reserved` (or downgrade-only), and the `TypeInfoGraphPayload` carrier
  takes the next free tag. Pinned by
  **`framework_surface_payload_graph_payload_is_additive_not_retyped`** and the
  whole-class **`all_public_semantic_type_graph_embeddings_are_payload_wrapped`**
  (which scans every public field typed `SemanticTypeGraph` **except** the canonical
  `TypeInfoGraphPayload.graph` field — exempt by construction, since
  `TypeInfoGraphPayload` is the mandated wrapper — and asserts every other such
  field is `reserved` or downgrade-only with the server-populated graph surface on a
  sibling `TypeInfoGraphPayload`/side-table field, with no field number reused).

### 1.6 Abstract class / abstract construct parity

`AbstractConstruct` and `is_abstract` carriers are carried forward into the Type IR
and the class matrix:

- `SignatureKind::AbstractConstruct` is a first-class signature kind distinct from
  a concrete construct signature. An `abstract new (...) => T` carries
  `AbstractConstruct`; a concrete `new (...) => T` does not.
- `ClassSurface.is_abstract` records whether the class declaration itself is
  `abstract`. Abstract member metadata (which members are declared `abstract`) is
  carried on the class surface alongside the public / nominal member sets.
- Relation and construct-availability rules read these carriers: a concrete
  `new Abstract(...)` is rejected (an abstract construct signature is not callable
  as a concrete constructor), while abstract-base inheritance,
  `InstanceType<abstract new ...>`, and the constructor utilities
  (`ConstructorParameters` / `InstanceType`) operate on the abstract construct
  signature per TS7 semantics.

Matrix rows: abstract-base inheritance; `InstanceType<abstract new ...>`;
constructor-utility behavior on abstract; rejecting concrete `new Abstract`.

### 1.7 TS7 decorators / auto-accessors

Decorators and auto-accessors are **not** modelled as `UnsupportedConstruct` and
**not** recovered through a diagnostic projection; they participate in the class
surface (replacing the prior unsupported-construct ruling).

- **`accessor` is a declared property whose visibility follows its modifiers.**
  Class/member-surface lowering models the `accessor` keyword (an auto-accessor) as
  a declared property on the class surface — not a hidden accessor pair, not a
  diagnostic stub — with its accessibility (`public` / `private` / `protected` /
  `#private`) and `static`/instance modifiers preserved. Its declared type is the
  property's annotated/inferred type. Visibility is **not** blanket-public: only a
  PUBLIC auto-accessor publishes a public property (it enters the public/nominal
  member sets like any public declared property). A `private` / `protected` /
  `#private` auto-accessor participates in the class's nominal / private / protected
  identity (the same brand identities relation reads) and is not a public projection
  member unless TS7 semantics make it visible at the projection site. `static`
  auto-accessors land on the static side.
- **Decorated method return types are preserved.** A decorated method keeps its
  literal-declared (or inferred) return type on the class surface. Indexed-access
  projection on the instance (`InstanceType<typeof C>['method']`,
  `ReturnType<C['method']>`, etc.) sees the declared method signature, including its
  preserved return type / literal-union return; decoration does not erase or widen
  the declared return.
- **Identity-compatible decorator effects are validation, not surface rewriting.**
  A decorator whose effect is identity-compatible with its target (it returns the
  same shape it received) is treated as a validation step through `ResolveCall` /
  `Relate`, **without** rewriting the class surface. The decorator call is resolved
  and relation-checked against the decorator-target contract; if the effect is
  identity-compatible, the class surface is unchanged. The engine does not
  synthesize a new member shape from the decorator's return; it relates the
  decorator against the target and keeps the declared surface.

Four `ClassFeatures` decorator/accessor matrix rows, each with a named guard:

- `decorators.rs::decorators_identity_method_decorator_preserves_return_inference`
  — guard **`decorator_identity_method_preserves_declared_return`**.
- `decorators.rs::decorators_identity_accessor_decorator_publishes_public_property`
  — guard **`accessor_decorator_publishes_public_property`**.
- `decorators.rs::decorators_metadata_reader_describe_return_is_literal_union`
  — guard **`decorated_method_literal_union_return_projects`**.
- `decorators.rs::decorators_accessor_decorator_returning_same_target_publishes_public_property`
  — guard **`accessor_decorator_identity_target_return_keeps_public_property`**.

### 1.8 Declaration-merge order is specified and recorded as facts

When a name has multiple declarations that merge (interface + interface,
namespace + interface, namespace + function, `enum` + namespace, a module + its
augmentations), the merged surface's member ORDER and override resolution are not
incidental — TS has a defined merge order, and the same source under a different
contributor order can produce a different merged surface. The merged-declaration /
augmentation reducers (`ResolveMergedDeclaration` / `ResolveDeclarationAugmentation`,
U2.MODULE_AUGMENTATION) specify this order and record the contributor SEQUENCE as
facts so the merged result is deterministic and the cache validates against it:

- **TS binder order within a file.** Declarations that merge within one file are
  ordered by the TS binder's declaration order (source order, with the binder's
  same-name merge rules). The merged member surface is assembled in that order.
- **Overload-group precedence.** A merged callable's overload group is ordered per TS's
  overload-merge precedence (declared overloads in declaration order; an
  implementation signature is internal-only and never the externally-visible last
  overload — consistent with §7 / the "last visible overload" rule for
  `ReturnType<typeof overloaded>`). The overload-group sequence is part of the recorded
  merge order.
- **Augmentation contributor order across files.** A module/global augmentation's
  contributors (the augmenting files) are ordered deterministically (by the
  declaration-analysis contributor sequence — the same contributor-provenance
  discipline §3 names for merged declarations and global augmentations), so a merged
  augmented surface does not depend on file-visitation nondeterminism.
- **Contributor sequence recorded as FACTS.** The merge does not just compute an order
  at query time — it records the contributor sequence as facts (on the
  declaration-analysis fact surface the merged/augmentation reducers read), so the
  `ReadSetSignature` validates against the exact contributor set + order in effect: a
  new / removed / reordered contributor invalidates the cached merged surface through
  the recorded facts (R6 version rooting on the value, not on a query-identity key).

Guard: **`declaration_merge_records_binder_overload_augmentation_order_as_facts`** (the
merged-declaration / augmentation reducers order contributors by TS binder order +
overload-group precedence + augmentation-contributor sequence and record that sequence
as facts validated by `ReadSetSignature`; a discriminating fixture pins a merged
surface whose member/overload order is order-sensitive and asserts the order matches
the oracle and that adding a contributor invalidates the cached merge through the
recorded facts; owned at `U2.MODULE_AUGMENTATION`).

---

## 2. Query Keys

> **State note:** the query keys below are the **end state**. The U2B.5/6/7 spine keys
> (`ResolveClassSurface`, `ResolveAmbientNamespace`, `ResolveEnum`, `ResolveOverloadSet`,
> `FlowNarrowingAt`, `ContextualTypeAt`) are LANDED in `SemanticQueryKey`; the remaining
> forward-planned U2 keys (`ResolveMergedDeclaration`, `ResolveDeclarationAugmentation`)
> and later/future keys are NOT yet in the enum — this architecture does not imply those
> already exist.

### 2.1 The seven-variant query-key spine (five landed, two forward-planned)

The seven-variant spine is: `ResolveMergedDeclaration`,
`ResolveDeclarationAugmentation`, `ResolveAmbientNamespace`, `ResolveOverloadSet`,
`ResolveEnum`, `FlowNarrowingAt`, `ContextualTypeAt`.

Of these, five are **landed** in `SemanticQueryKey` — `ResolveAmbientNamespace`,
`ResolveOverloadSet`, `ResolveEnum`, `FlowNarrowingAt`, `ContextualTypeAt`. The
remaining two — `ResolveMergedDeclaration` and `ResolveDeclarationAugmentation` —
are **forward-planned** (owned by a not-yet-landed block) and are NOT in the enum
or the generated spec table; they are documented here as the end-state shape, not
as live deliverables.

The forward-planned augmentation key is the **generalized** form. When it lands,
the former `ResolveModuleAugmentation` slot is broadened to
`ResolveDeclarationAugmentation` so module **and** global
declaration-environment-mutation facts share **one** concrete `SemanticQueryKey`
identity, per the one-resolver rule — an existing-slot generalization, not a sixth
spine variant.

### 2.2 `ResolveDeclarationAugmentation` key shape (declaration-environment identity)

The `target` is **env-free** — the env lives on `DeclarationAnalysisContext`, not
duplicated on the target. The `FileArtifactStore`
`AugmentationTargetKey { project_identity, resolve_env_hash, lib_env_hash, population, target }`
has its `{R, L, J}` env dims DERIVED from `DeclarationAnalysisContext` at execution
time (so the augmentation-target env has exactly one source — the context — and
cannot diverge from the query-key env), while its `population: AugmentationPopulation
{Base, Session(overlay-set fingerprint)}` dim is derived from the active SESSION
view (NOT from `DeclarationAnalysisContext`) and provides the Base/Session overlay
isolation:

```rust
ResolveDeclarationAugmentation {
    target: DeclarationAugmentationTarget, // Module(ModuleSpecifier) | Global(GlobalEnvScope) — ENV-FREE; env from context
    context: DeclarationAnalysisContext,
}

enum DeclarationAugmentationTarget {
    Module(ModuleSpecifier),  // ENV-FREE: just the augmented module specifier; AugmentationTargetKey derived from context
    Global(GlobalEnvScope),   // ENV-FREE: a content-free global-environment scope (`declare global` /
                              // `export as namespace` / UMD globals); project-/lib-scoped env from context
}

struct DeclarationAnalysisContext {
    resolve_env_hash: ResolveEnvHash,  // name/import resolution (derives AugmentationTargetKey.resolve_env_hash)  — R
    lib_env_hash: LibEnvHash,          // lib-declared global/ambient surfaces a global augmentation mutates       — L
    project_identity: ProjectIdentity, // project isolation (derives AugmentationTargetKey.project_identity)        — J
    // Exactly the `{R, L, J}` axes that the landed
    // `AugmentationTargetKey { project_identity, resolve_env_hash, lib_env_hash, population, target }`
    // folds (the extra `population` dim is the session-view overlay identity, not a
    // context axis). NO parse_env_hash / type_env_hash KEY DIM, NO project_config_hash (R21),
    // NO content hash / parse_stable_hash, NO fact_dep_signature (R6).
}
```

Both `target` variants map to exactly the in-process `SemanticQueryValue::DeclarationAnalysis`
VALUE domain (the declaration-environment-mutation fact domain — module facts and
global facts), NEVER `SemanticQueryValue::TypeNode`. This is the in-process VALUE
counterpart, distinct from the WIRE: on the wire, module / global augmentation
graph nodes REMAIN `GraphTypeNode` arms 23 / 25 (§1.3) — there is no
`DeclarationAnalysisGraph` wire message (that relocation was rejected). The context
carries the `{R, L, J}` env axes ONLY — exactly the dims the landed
`AugmentationTargetKey` folds (plus its session-view `population` dim). Parse env (and type env) are NOT key dimensions: a parse-option /
parser-version change is reflected through the VALUE-side body read (the cold
compute re-sources the augmenter bodies from live parser output and roots the value
on the contributing files' `FileWholeHash` self-roots), not through the
content-free query-identity key (R6). This matches `u2-query-value-domain-design.md`
§2.2 and `semantic-type-graph-plan-recovered.md`.

Forward-planned guards (land with the generalized key): **`global_augmentation_query_has_declaration_analysis_identity`**
(global declaration-environment-mutation facts are reachable through the generalized
`ResolveDeclarationAugmentation { target: Global(GlobalEnvScope), .. }` key — a
concrete `SemanticQueryKey` identity, not an identity-less side product — resolving
to `SemanticQueryValue::DeclarationAnalysis`); and
**`declaration_augmentation_target_is_env_free_env_comes_from_context`** (the query
`target` is env-free, the `AugmentationTargetKey`'s `{R, L, J}` env dims are derived
from `DeclarationAnalysisContext` at execution time — the `population` dim comes
from the active session view, not the context — the derived target env equals the
context env, and no public constructor can create a target/context env mismatch).

### 2.3 The five added keys

Exactly five variants are added beyond the seven:

| Added key | Lands | Purpose |
|---|---:|---|
| `FlowReturn { function_slot, normalized_type_args, context, demand, input }` | U6 | Demand-sliced return/body flow query |
| `ResolveClassSurface { decl_slot, type_args, side, context: ClassSurfaceContext }` | U2 | Instance/static heritage with generic substitution via the shared dual-space algorithm |
| `ApparentType { base, context: ApparentTypeContext }` | U2 | Primitive/array/constrained-generic apparent member lookup via lib facts |
| `TemplateLiteralReduce { pattern, args, context: TemplateLiteralReduceContext }` | U2 | Template literal distribution, intrinsics, and `infer` splitting |
| `ResolveCall { callee, call_kind, receiver_this, args, explicit_type_args, contextual_result, policy, context }` | U6, with U2 overload key | Reusable call resolution with its own cache identity |

`ApplySignature` and `InferTypeArgs` fold into the U6 flow/call solver plus
`ResolveOverloadSet` and `Relate`. `CallResolve` is **promoted** to the first-class
`ResolveCall` key (not folded). `Widen` folds into a pure `widen_for_position`
helper. Generic `ResolveClass` folds into `ResolveClassSurface`. The name is
`ApparentType`, not `GetApparentType`.

Each added key lands its enum variant **+** its `SemanticQueryKeySpec` row **+**
its dispatch behavior together in the block named in the **Lands** column — never a
spec row ahead of its variant. The three U2-landed keys (`ResolveClassSurface`,
`ApparentType`, `TemplateLiteralReduce`) land at `U2.QUERY_VALUE_DOMAIN`; the
U6-landed `FlowReturn` / `ResolveCall` land their variant + spec row + behavior at
U6 (reusing the slot-identity SHAPE/model U2 finalized, with no cache re-key). They
are NOT pre-registered at U2. The `semantic_query_key_spec_table_equals_enum`
meta-guard is therefore a STANDING per-block invariant — the generated spec table
EXACTLY EQUALS the live enum on every committed tree, validated incrementally after
each block — not a one-shot "U2 proves all five rows" gate.

### 2.4 `ResolveCall` key shape and context soundness

Call resolution is reusable semantic work, not merely a flow helper. Without its
own cache identity, contextual typing, flow return, overload selection, generic
inference, and typeinfo expression evaluation would duplicate work or hide
meaning-affecting inputs inside a body solver. It therefore gets a first-class key.

The key normalizes closed arguments to **type** identities and keeps an
**expression** identity only for context-sensitive arguments (raw `args` / a raw
call-site slot is the wrong identity — pure expression slots destroy reuse, pure
type identities are unsound for context-sensitive arguments):

```rust
ResolveCall {
    callee: SemanticNodeId,
    call_kind: CallKind, // call/new/tagged/optional-call
    receiver_this: Option<SemanticNodeId>,
    args: Arc<[CallArgKey]>,
    explicit_type_args: Option<Arc<[SemanticNodeId]>>,
    contextual_result: ContextualResultKey,
    policy: CallResolutionPolicy,
    context: CallResolutionContext,
}

enum CallArgKey {
    Eager { spread: bool, ty: SemanticNodeId, freshness: FreshnessKey },
    ContextSensitive { spread: bool, expr: ContextSensitiveExprKey },
}
```

`call_kind` and `policy` are first-class key inputs — they appear in the displayed
key, not only in a guard name.

A context-sensitive argument's identity is sound **only** if the key/context
carries every input that can change how that expression resolves. A bare expression
slot is not sound: the same slot resolves differently under different narrowed flow,
generic substitutions, lexical/binder scope, and contextual-typing targets. Those
axes are carried explicitly:

```rust
struct ContextSensitiveExprKey {
    expr_slot: ExprSlotId,
    flow_narrowing: FlowNarrowingKey,        // caller flow / narrowing context
    substitution: SubstitutionCanonicalHash, // substitution context
    binder: BinderScopeId,                   // lexical / binder identity
    contextual_typing: ContextualTypingKey,  // contextual-typing axes for callbacks/fresh literals
}

struct CallResolutionContext {
    parse_env_hash: ParseEnvHash,
    resolve_env_hash: ResolveEnvHash,        // callee/import resolution
    type_env_hash: TypeEnvHash,              // strict / exact-optional / index-access options
    lib_env_hash: LibEnvHash,                // lib-declared apparent/call surfaces
    project_identity: ProjectIdentity,
    substitution: SubstitutionCanonicalHash,
    projection_reduction: ProjectionReductionContext,
    caller_flow_narrowing: FlowNarrowingKey,
    binder: BinderScopeId,
    contextual_typing: ContextualTypingKey,
    // NO project_config_hash (R21), NO content/parse_stable_hash, NO fact_dep_signature (R6).
}
```

(Equivalently these axes may live on `CallResolutionContext`; the requirement is
that they are part of the cache identity.)

Call resolution is also where the `InferenceSession` substrate (§4.2) is most
visible: `ResolveCall` opens one **speculative** `InferenceSession` per overload
candidate, runs applicability + argument-to-parameter inference + fixation + final
substitution inside the session, and publishes ONLY the winning completed
`ResolvedCall` under this key — losing candidates' sessions are discarded without
publishing any entry, fact signature, or backfill. The published value is the final
`ResolvedCall`, never a mutable session or a session-local partial (§4.2 admission
rule).

Guards:
**`resolve_call_key_covers_args_this_contextual_type_overload_policy_and_context`**
and **`resolve_call_same_expr_different_flow_or_substitution_does_not_warm_hit`**.

### 2.5 `FlowReturn` key shape (canonical, one definition)

```rust
FlowReturn {
    function_slot,
    normalized_type_args,
    context: FlowReturnContext,  // env + substitution + projection-reduction + flow policy
    demand: ReturnProjectionDemand,
    input: FlowInputContext,     // contextual callback input signature + relation/call demand mode
}
```

`FlowReturnContext` carries env + substitution + projection-reduction + flow
policy. `ReturnProjectionDemand` is its own key field `demand`, not duplicated in
`context`. `FlowInputContext` (the contextual callback input signature plus the
relation/call demand mode) is its own key field `input`. This canonical key is
identical to the full normalized re-entry identity used by the flow cycle-id space
(`FlowReturnContext + ReturnProjectionDemand + FlowInputContext` — §5): the cache
key and the cycle-re-entry key are the same normalized identity. That flow cycle-id
space is the flow-typed view of the ONE shared `CheckerReentryGraph` (§4.2) that
also spans `ResolveCall`, `ContextualTypeAt`, and `FlowNarrowingAt`, so the
`ResolveCall → FlowReturn → narrowing → ResolveCall` cycle discharges through a
shared re-entry assumption rather than self-awaiting or budget-spinning. Guards:
**`flow_return_key_covers_env_dimensions`** and
**`flow_return_key_covers_input_context_and_projection_demand`** (two `FlowReturn`
queries differing only in projection demand, or only in contextual callback input
signature / relation-call demand mode, produce distinct cache identities and do not
warm-hit).

### 2.6 U2 key shapes — explicit structs with explicit context

`ResolveClassSurface`, `ApparentType`, and `TemplateLiteralReduce` carry their
env/context dimensions on a named per-key `*Context` struct, not on a hidden shared
global context. Each names the **split** env hashes it actually depends on (the
relevant subset of `parse_env_hash` / `resolve_env_hash` / `type_env_hash` /
`lib_env_hash` / `project_identity` — never a bundled `project_config_hash`, per
R21). The substitution axis rides on the KEY's instantiated node fields
(`type_args` / `args`) or is already baked into a `base` node — NOT duplicated in
the `*Context`; the projection rung rides as `mode` on the one surface that has one
(`ClassSurfaceContext`). No `*Context` carries a content hash, `parse_stable_hash`,
or `fact_dep_signature` (these are query-identity keys — version rooting lives on
the cached value, not the key). The slot-intrinsic `T,L,J` dims of
`ResolveClassSurface` come from its `decl_slot: ResolvedDeclSlotIdentity`, so its
`ClassSurfaceContext` carries only the extra `P,R` env plus `mode`; the no-slot
`ApparentType` / `TemplateLiteralReduce` keys carry their env dims directly on the
context:

```rust
ResolveClassSurface {
    decl_slot: ResolvedDeclSlotIdentity,  // slot carries T,L,J (+ symbol_space)
    type_args: Arc<[SemanticNodeId]>,     // heritage generic substitution (instantiated)
    side: ClassSurfaceSide,               // instance / static
    context: ClassSurfaceContext,
}

struct ClassSurfaceContext {
    parse_env_hash: HashValue,            // decorator lowering is parse-env / parser-version sensitive (forward-declared for the deferred decorator reducer)
    resolve_env_hash: HashValue,          // heritage/import resolution
    mode: ProjectionMode,                 // projection rung the composed surface is produced under
}

ApparentType {
    base: SemanticNodeId,                 // already-substituted base node
    context: ApparentTypeContext,
}

struct ApparentTypeContext {
    type_env_hash: HashValue,
    lib_env_hash: HashValue,              // apparent members come from lib wrapper interfaces
    project_identity: u32,
}

TemplateLiteralReduce {
    pattern: Arc<[Arc<str>]>,             // literal quasis
    args: Arc<[SemanticNodeId]>,          // instantiated distribution arguments (substitution rides here)
    context: TemplateLiteralReduceContext,
}

struct TemplateLiteralReduceContext {
    resolve_env_hash: HashValue,          // name/intrinsic resolution
    type_env_hash: HashValue,
    lib_env_hash: HashValue,              // intrinsic (Uppercase/Lowercase/Capitalize/Uncapitalize) facts
    project_identity: u32,
}
```

`ApparentType` omits `parse_env_hash` and `resolve_env_hash` (apparent-member lookup
is a function of the base node + lib/type/project env, not of import resolution or a
parsed body skeleton); `TemplateLiteralReduce` omits `parse_env_hash` (it reduces
already-lowered interned arg nodes); `ResolveClassSurface` carries `parse_env_hash`
(class-surface lowering owns decorators, which are parse-env sensitive — the full
planned `P R T L J` identity, with `P` forward-declared for the deferred
decorator-reading reducer). Guards:
**`resolve_class_surface_key_covers_side_demand_type_args_and_context`** and
**`resolve_class_surface_identity_covers_parse_env_axis`** (asserting
`ClassSurfaceContext` carries `parse_env_hash` and none of the R21/R6 forbidden
fields), **`apparent_type_key_covers_lib_env_demand_and_context`**, and
**`template_literal_reduce_key_covers_context`**.

### 2.7 `Relate` key shape (existing-key upgrade, full relation identity)

`Relate` is the sole assignability authority (§4) and carries the same
cache-soundness discipline. The pre-U2 `Relate` was only `{ source, target }`, which
was not a sound cache identity: the same pair relates differently under a different
relation kind, overload-selection / excess-check policy, source freshness, or
env/substitution context. This block landed that as an explicit existing-key upgrade
(not a new key — the added-key count stays five) to the full identity below:

```rust
Relate {
    source: SemanticNodeId,
    target: SemanticNodeId,
    relation: RelationKind,           // assignable / subtype / identity / comparable / strict-subtype
    policy: RelationPolicy,           // overload-selection / excess-property / variance policy
    source_freshness: FreshnessKey,   // fresh-object-literal excess-check mode vs non-fresh
    inference_context: Option<InferenceContextKey>, // Some for binding-producing relations; None for pure assignability
    context: RelationContext,
}

struct RelationContext {
    resolve_env_hash: ResolveEnvHash,
    type_env_hash: TypeEnvHash,
    lib_env_hash: LibEnvHash,
    project_identity: ProjectIdentity,
    substitution: SubstitutionCanonicalHash, // same hash as flow/call keys
    projection_reduction: ProjectionReductionContext,
    // NO project_config_hash (R21), NO content/parse_stable_hash, NO fact_dep_signature (R6).
}
```

`RelationPolicy` is too coarse for binding-producing relation work: relation that
produces inference bindings (`infer` / `InferBind`, generic call inference,
conditional-type `infer` extraction, contextual generic inference) resolves
differently under a different inference setup even when everything else is
identical. Such a relation runs INSIDE the active `InferenceSession` of the
enclosing `CheckerTransaction` (§4.2): it mutates the session (deposits candidates
into the relevant `InferenceInfo`) and returns **session-local inference deltas**,
which are never globally cacheable partial results. What caches is the completed
result, fingerprinted by the session's projection. `Relate` therefore carries an
`inference_context: Option<InferenceContextKey>` — `Some` (part of identity) for
binding-producing relations, `None` for pure non-binding assignability checks.
`InferenceContextKey` is the **cache-identity projection of the active session** —
the completed-session fingerprint of the inference setup in scope — not a standalone
bag of axes assembled independently of the session:

```rust
struct InferenceContextKey {
    inferable_params: InferableParamSetId,          // which type parameters are open / inferable
    variance_phase: VariancePhase,                  // covariant / contravariant / invariant measurement pass
    candidate_priority: InferenceCandidatePriority, // return-type vs argument vs naked-type-parameter priority
    no_infer_mask: NoInferMask,                     // the occurrence-local NoInfer suppression mask in effect
    const_param_policy: ConstParamPolicy,           // `<const T>` const-ness propagation
    contextual_inference_mode: ContextualInferenceMode, // whether/how a contextual target drives inference
    // NO project_config_hash (R21), NO content/parse_stable_hash, NO fact_dep_signature (R6).
}
```

`NoInferMask` is the occurrence-local `NoInfer` suppression mask (consistent with
§1.2). These axes are exactly the fields the active `InferenceSession` carries
(§4.2): `InferenceContextKey` is their content-free projection onto the cache key,
so two relations under different inference sessions produce distinct identities. The
relation/inference engine is the **sole** owner of this binding work — every
binding-producing relation runs through the `InferenceSession` substrate (§4.2) and
none implements a parallel matcher (§4).

The `RelationBudget` pair memo (§6) is keyed by **this full** `Relate` identity
(`source`, `target`, `relation`, `policy`, `source_freshness`, `inference_context`,
`context`), NOT the bare `(source, target)` pair — the memo cannot false-hit across
relation-kind / policy / freshness / inference-context / env differences. Under this
key, `Relate` produces a public `RelationPayload` (outcome / bindings / proof +
typed `BudgetExceeded` non-admission), not a bare tri-state `RelationResult`; its
value domain is `SemanticQueryValue::Relation(RelationPayload)`, and `RelationPayload`
is exactly where public `relate` returns its proof off the type-values surface.
Only the COMPLETED `RelationPayload` of a completed, deterministic session is
admitted under this key (§4.2 admission rule); the session-local inference deltas a
binding-producing relation deposits are never warm-admitted on their own, and a
cancelled / budget-exceeded / mid-flight session is `ReturnOnly`.

Guards: **`relate_key_covers_relation_kind_policy_freshness_and_context`**,
**`relate_same_nodes_different_relation_kind_policy_or_env_do_not_warm_hit`**, and
**`relate_same_nodes_different_inference_context_do_not_warm_hit`** (binding-producing
identity — pinning that `RelationPolicy` alone is not a sufficient identity).

### 2.8 Remaining `SemanticQueryKey` context shapes — per-key whole-class closure

The remaining variants get explicit R21/R6-clean contexts on the same
split-env / projection discipline. Each carries only the split env hashes it
depends on plus the substitution / flow / contextual axes where applicable; none
carries a content hash, `parse_stable_hash`, or `fact_dep_signature`:

```rust
ResolveMergedDeclaration { decl_slot, type_args, demand: MemberDemand, context: MergedDeclarationContext }
ResolveAmbientNamespace  { namespace_slot, type_args, context: AmbientNamespaceContext }
ResolveOverloadSet       { callee, type_args, context: OverloadSetContext }
ResolveEnum              { enum_slot, context: EnumContext }
FlowNarrowingAt          { point: ProgramPointId, flow: FlowNarrowingKey, context: ProgramAnalysisContext }
ContextualTypeAt         { point: ProgramPointId, contextual: ContextualTypingKey, context: ProgramAnalysisContext }
```

The landed `AmbientNamespaceContext` carries only the extra `{parse_env_hash,
resolve_env_hash}` env plus the projection rung `mode` — its slot-intrinsic `T,L,J`
come from `namespace_slot: ResolvedDeclSlotIdentity` and its substitution axis rides
on the key's `type_args` field, not on the context (`parse_env_hash` is
forward-declared for the deferred body-reading namespace-member reducer — the
skeleton is parse-env sensitive). The planned `MergedDeclarationContext` follows the
same split-env discipline for the merged-declaration reducer. `OverloadSetContext`
and `EnumContext` omit `parse_env_hash` (they read already-lowered signatures / enum
members); `EnumContext` carries no substitution axis (an enum declaration is not
generic). `ProgramAnalysisContext` is
the SHARED program-analysis env context covering env + substitution. The flow /
contextual demand axis is NOT folded into this shared context — it lives as a
PER-VARIANT key field so neither variant carries the other's dead axis:
`FlowNarrowingAt` carries `flow: FlowNarrowingKey`, `ContextualTypeAt` carries
`contextual: ContextualTypingKey` (design §417/418 demand-axes column). The shared
`substitution` axis stays on the context (both variants depend on it):

```rust
struct ProgramAnalysisContext {
    parse_env_hash: ParseEnvHash,           // flow/contextual analysis reads the parsed body skeleton
    resolve_env_hash: ResolveEnvHash,
    type_env_hash: TypeEnvHash,             // strict / exact-optional / index-access options that change narrowing
    lib_env_hash: LibEnvHash,
    project_identity: ProjectIdentity,
    substitution: SubstitutionCanonicalHash, // shared — same hash as the flow/call/relation keys
}
// per-variant demand axes (NOT on the shared context — no dead axis):
//   FlowNarrowingAt { point, flow: FlowNarrowingKey, context }
//   ContextualTypeAt { point, contextual: ContextualTypingKey, context }
```

`FlowNarrowingKey` and `ContextualTypingKey` are content-free SHAPE-only newtypes
over an interned `Arc<[SemanticNodeId]>` set (mirroring `InferableParamSetId`),
forward-declared for the deferred U6 flow / contextual reducers. They are the same
axis identities the
`ResolveCall` context-sensitive-arg identity and `CallResolutionContext` carry —
there is one flow/narrowing axis space and one contextual-typing axis space shared
across the call, flow-return, and program-analysis keys. `ContextualTypeAt` and
`FlowNarrowingAt` are nodes on the ONE shared `CheckerReentryGraph` (§4.2)
alongside `ResolveCall` and `FlowReturn` — each keyed by its full normalized
`ProgramPointId + ProgramAnalysisContext + the per-variant flow / contextual key`
identity (`FlowNarrowingAt` carries `flow: FlowNarrowingKey`, `ContextualTypeAt`
carries `contextual: ContextualTypingKey`) — so a contextual / narrowing
re-entry on the mutual-recursion cycle records the in-flight re-entry assumption
rather than self-awaiting; and contextual-callback inference at such a point runs
inside the active `InferenceSession` (§4.2), not a private callback-inference loop.

Per-key no-cross-context-warm-hit guards. For the LANDED U2B.5/6/7 keys the live guard
names are the shorter `*_do_not_warm_hit` forms:
**`resolve_ambient_namespace_do_not_warm_hit`**,
**`resolve_overload_set_do_not_warm_hit`**,
**`resolve_enum_do_not_warm_hit`**,
**`flow_narrowing_at_do_not_warm_hit`**,
**`contextual_type_at_do_not_warm_hit`**,
**`resolve_class_surface_do_not_warm_hit`**
(plus the per-axis discriminators, e.g. `resolve_ambient_namespace_identity_covers_parse_env_axis`).
The FORWARD-PLANNED U2 keys carry their planned guard names:
**`resolve_merged_declaration_same_site_different_env_or_context_do_not_warm_hit`**
and **`declaration_augmentation_key_same_site_different_env_or_context_do_not_warm_hit`**
(whose env-axis coverage asserts the `{R, L, J}` axes of
`DeclarationAnalysisContext` — `resolve_env_hash`, `lib_env_hash`,
`project_identity` — NOT `parse_env_hash`, which is not a key dim; see §2.2).

### 2.9 Generated `SemanticQueryKeySpec` table over the whole closed enum

A soft "every variant has a context" meta-guard is insufficient: the live enum
carries variants this architecture neither retires nor specifies (`ResolveDecl`,
`Instantiate`, `ProjectMember`, `IndexedAccess`, `KeyOf`, `MappedType`,
`Conditional`, `TypeOf`, `NormalizeUnion`, `NormalizeIntersection`, `ProjectPath`,
`ResolvedNamedType`, `Relate`, `ResolveMacroPayload`), and a "has a context"
assertion leaves implementers to invent contexts / value-domains / admission
behavior for them. To make the closure mechanical, a **generated**
`SemanticQueryKeySpec` table gives, for **every** current and end-state variant,
five fields: (1) lifecycle (`live` / `retired` / `renamed`), (2) context shape (the
named R21/R6-clean `*Context` struct or inline split-env/projection context), (3)
value domain (the `SemanticQueryValue` arm), (4) cross-context guard, and (5)
admission / budget behavior. The table is generated from the canonical key list by a
dedicated `cargo run` generator and checked in (same generated-not-hand-maintained
discipline as the oracle rows, the proof registry, and the row-test wrapper).

| Variant | Lifecycle | Context shape | Value domain | Cross-context guard | Admission / budget |
|---|---|---|---|---|---|
| `ResolveDecl` | live | `ResolveDeclKey` (split env `R T L J`; the key IS the context) | `TypeNode` | — (no dedicated cross-context guard row in the generated spec table) | `Singleflight` |
| `Instantiate` | live | `InstantiateContext` (`projection_reduction: ProjectionReductionContext` + `resolve_env_hash` = `R`); base is the env-bearing content-free `ResolvedDeclSlotIdentity` (slot carries `T,L,J`); `provenance` + `merge_role` stay FAMILY-IDENTITY on `FamilyKey` | `TypeNode` | `instantiate_same_base_different_env_or_context_do_not_warm_hit`, `decl_self_type_or_lib_env_change_produces_distinct_instantiate_key` | `Singleflight`; `ReturnOnly` on overflow/cancel/budget |
| `ProjectMember` | live (canonicalised to length-1 `ProjectPath`) | `ProjectionMode` (env `T L J`) | `TypeNode` | — | `Singleflight` |
| `IndexedAccess` | live | `ProjectionMode` (env `T L J`) | `TypeNode` | — | `Singleflight` (union-key distribution guarded by the planned `KeyspaceBudget` reducer; `ReturnOnly` on overflow) |
| `KeyOf` | live | `ProjectionReductionContext` (env `T L J`; demand axes include `Provenance,MergeRole`) | `TypeNode` | `keyof_queries_differing_only_by_provenance_do_not_warm_hit`, `keyof_and_mapped_type_context_axes_do_not_alias_family_identity` | `Singleflight` (keyspace guarded by the planned `KeyspaceBudget` reducer; `ReturnOnly` on overflow) |
| `MappedType` | live | `ProjectionReductionContext` (env `T L J`; demand axes include `Provenance,MergeRole`) | `TypeNode` | `mapped_type_queries_differing_only_by_merge_role_do_not_warm_hit`, `keyof_and_mapped_type_context_axes_do_not_alias_family_identity` | `Singleflight` (keyspace explosion guarded by the planned `KeyspaceBudget` reducer; `ReturnOnly` on overflow) |
| `Conditional` | live | inline `(check,extends,true_branch,false_branch,distributive)` (env `T L J`) | `TypeNode` | — | `Singleflight` (consumes `Relate` bindings; `ReturnOnly` on budget/cycle) |
| `TypeOf` | live | `ValueRootKey` (env `R T L J`) | `TypeNode` | — | `Singleflight`; `ReturnOnly` on overflow |
| `NormalizeUnion` | live | inline `(members)` (env `T L J`) | `TypeNode` | — | `Singleflight` (large unions guarded by the planned `KeyspaceBudget` reducer; `ReturnOnly` on overflow) |
| `NormalizeIntersection` | live | inline `(members)` (env `T L J`) | `TypeNode` | — | `Singleflight` (keyspace guarded by the planned `KeyspaceBudget` reducer; `ReturnOnly` on overflow) |
| `ProjectPath` | live | `ProjectionReductionContext` (env `T L J`; base + path) | `TypeNode` | — | `Singleflight`; `ReturnOnly` on budget/cycle |
| `ResolvedNamedType` | live (read-dominant macro artifact; `execute` returns `Miss` until written) | `HostResolvedNamedTypeKey` (own env/identity; env `R T L J`) | `TypeNode` | — | `ReadDominantNoExecute` (read-only memo; writes via the `NamedTypeCache` adapter) |
| `Relate` | live (existing-key UPGRADE — `{source,target}` → full identity) | inline `(source,target,relation,policy,source_freshness,inference_context,context)` (env `R T L J`; binding-producing) | `Relation(RelationPayload)` | — | `RelationMemo` (`RelationBudget`; coinductive-SCC discharge; `ReturnOnly` on `Unknown`/cancel/`BudgetExceeded`) |
| `ResolveMacroPayload` | live (Vue-macro payload key, distinct from the typeinfo macro story) | `MacroPayloadContext` (`resolve_env_hash` = `R` + `mode`); owner is the env-bearing content-free `ResolvedDeclSlotIdentity` (slot carries `T,L,J`) | `TypeNode` | `resolve_macro_payload_same_owner_different_env_or_context_do_not_warm_hit` | `Singleflight`; `ReturnOnly` on overflow/cancel |
| `ResolveMergedDeclaration` | planned (U2-MODULE) | `MergedDeclarationContext` | `TypeNode` | `resolve_merged_declaration_same_site_different_env_or_context_do_not_warm_hit` | `Singleflight`; `ReturnOnly` on budget/cycle |
| `ResolveDeclarationAugmentation` | planned (U2-MODULE; generalizes `ResolveModuleAugmentation`) | `DeclarationAnalysisContext` (`{R,L,J}` — NO `parse_env_hash` key dim; parse env enters via the value-side body read only) | `DeclarationAnalysis(DeclarationAnalysisValue)` | `declaration_augmentation_key_same_site_different_env_or_context_do_not_warm_hit` | `Singleflight`; `ReturnOnly` on overflow/cancel |
| `ResolveAmbientNamespace` | live (added — U2B.5; reducer deferred, `execute` returns `Miss`) | `AmbientNamespaceContext` (`{P,R}` incl. `parse_env_hash` + `mode`) | `TypeNode` | `resolve_ambient_namespace_do_not_warm_hit` + `resolve_ambient_namespace_identity_covers_parse_env_axis` / `_mode_axis` | `NonProducingPendingReducer` |
| `ResolveOverloadSet` | live (added — U2B.5; reducer deferred, `execute` returns `Miss`) | `OverloadSetContext` (`{R}`) | `OverloadSet(Arc<[SignatureRef]>)` | `resolve_overload_set_do_not_warm_hit` + `resolve_overload_set_key_covers_context` | `NonProducingPendingReducer` |
| `ResolveEnum` | live (added — U2B.5; reducer deferred, `execute` returns `Miss`) | `EnumContext` (`{R}`) | `TypeNode` | `resolve_enum_do_not_warm_hit` + `resolve_enum_key_covers_context` | `NonProducingPendingReducer` |
| `FlowNarrowingAt` | live (added — U2B.7; flow engine deferred to U6, `execute` returns `Miss`) | `ProgramAnalysisContext` (env `{P,R,T,L,J}` + shared `substitution`) + per-variant `flow: FlowNarrowingKey` | `ProgramAnalysis(ProgramAnalysisValue)` | `flow_narrowing_at_do_not_warm_hit` + `flow_narrowing_at_identity_covers_flow_axis` / `_substitution_axis` + `flow_narrowing_at_key_covers_full_env_and_point` | `NonProducingPendingReducer` |
| `ContextualTypeAt` | live (added — U2B.7; contextual engine deferred to U6, `execute` returns `Miss`) | `ProgramAnalysisContext` (env `{P,R,T,L,J}` + shared `substitution`) + per-variant `contextual: ContextualTypingKey` | `ProgramAnalysis(ProgramAnalysisValue)` | `contextual_type_at_do_not_warm_hit` + `contextual_type_at_identity_covers_contextual_axis` / `_substitution_axis` + `contextual_type_at_key_covers_full_env_and_point` | `NonProducingPendingReducer` |
| `FlowReturn` | planned (U6) | `FlowReturnContext` + `demand`(`ReturnProjectionDemand`) + `input`(`FlowInputContext`) | `FlowReturn(Arc<FlowReturnResult>)` | `flow_return_key_covers_input_context_and_projection_demand` | `FlowSliceBudget`; flow-cycle sentinel `ReturnOnly` |
| `ResolveClassSurface` | live (added — U2B.5; decorator-reading reducer deferred) | `ClassSurfaceContext` (`{P,R}` incl. `parse_env_hash` + `mode`) | `TypeNode` | `resolve_class_surface_do_not_warm_hit` + `resolve_class_surface_key_covers_side_demand_type_args_and_context` + `resolve_class_surface_identity_covers_parse_env_axis` / `_mode_axis` / `_canonicalizes_decl_slot_symbol_space` | `Singleflight`; `ReturnOnly` on budget |
| `ApparentType` | live (added — U2B.6; lib-member-index reducer deferred) | `ApparentTypeContext` (`{T,L,J}`) | `TypeNode` | `apparent_type_do_not_warm_hit` + `apparent_type_key_covers_lib_env_demand_and_context` | `NonProducingPendingReducer` |
| `TemplateLiteralReduce` | live (added — U2B.6; LIVE producer) | `TemplateLiteralReduceContext` (`{R,T,L,J}`) | `TypeNode` | `template_literal_reduce_do_not_warm_hit` + `template_literal_reduce_key_covers_context` | `Singleflight` |
| `ResolveCall` | planned (U6) | `CallResolutionContext` (+ `ContextSensitiveExprKey` per arg) | `ResolvedCall(Arc<ResolvedCallResult>)` | `resolve_call_same_expr_different_flow_or_substitution_does_not_warm_hit` | `CallResolutionBudget`; `ReturnOnly` on `BudgetExceeded` |

In the generated `SemanticQueryKeySpec` table on any committed tree, no live variant
is omitted: every live variant appears with `live` + spec; any variant intended to
retire/rename carries `retired` (+ `reserved` on its wire surface) or `renamed` —
there is no fourth state in the generated table. The table reproduced ABOVE is the
END-STATE view (every block landed); rows for variants not yet present in the live
`SemanticQueryKey` enum are annotated `planned (<owning-block>)` here to show which
block lands each. A `planned` row is NOT present in the generated table — it enters
only when its owning block adds the enum variant (see below).

The class is closed by one meta-guard asserting enum/table EQUALITY:

- **`semantic_query_key_spec_table_equals_enum`** — asserts the generated table's
  variant set EXACTLY EQUALS the closed `SemanticQueryKey` enum's variant set (no
  omissions, no extras — every enum variant has exactly one row, every row names a
  real enum variant or is explicitly `retired`/`renamed`). For every `live` row it
  asserts (1) an explicit named R21/R6-clean key identity (each `*Context` carries
  split env hashes + where applicable `substitution`; the flow / contextual
  set-identity axes ride as per-variant key fields, not on the shared context; none
  of the forbidden fields),
  (2) exactly one value domain (cross-checked with
  `every_semantic_query_key_maps_to_exactly_one_value_domain`), (3) a registered
  no-cross-context-warm-hit guard, and (4) a declared admission/budget behavior;
  `retired` rows are absent from the live enum (wire surface `reserved`); `renamed`
  rows' old name absent / new name present. A variant with no row, a row for a
  non-existent variant, a `live` row missing any field, or a lifecycle mismatch,
  FAILS. This is the mechanical closure of the per-key context-identity class —
  enum/table equality forces a fully-specified spec row for each variant. This
  replaces the soft
  `every_semantic_query_key_has_explicit_context_and_cross_context_warm_hit_guard`.

  The table above is the END-STATE view. Because enum/table EQUALITY is the
  assertion, the guard is a STANDING per-block invariant: a variant's spec row may
  appear ONLY in the same block that adds its enum variant. Each `planned` row's
  lifecycle names its owning block (per the Stage-B sequence):
  `ResolveMergedDeclaration` / `ResolveDeclarationAugmentation` at `U2-MODULE`;
  `ResolveClassSurface` / `ResolveAmbientNamespace` / `ResolveEnum` /
  `ResolveOverloadSet` at `U2B.5`; `ApparentType` / `TemplateLiteralReduce` at
  `U2B.6`; `FlowNarrowingAt` / `ContextualTypeAt` at `U2B.7`; `FlowReturn` /
  `ResolveCall` (enum variant + spec row + dispatch behavior together) at `U6` —
  never a row ahead of its variant. The guard is green after EVERY block, never red
  between them.

### 2.10 Query modes are presets over the `ProjectionDemand` / `EvalPolicy` lattice

The five mode names (`Identity` / `Navigate` / `Shallow` / `Expanded` / `Skeleton`)
are **too coarse to be primary cache / semantic identity**: a single enum rung
cannot say "expand the member set but not the bodies", "preserve the alias but
reduce operators", or "open generics as shells but stop at carriers" without
multiplying into a combinatorial enum. The PRIMARY semantic-demand and the PRIMARY
cache-identity dimension is a two-part **demand lattice** — `ProjectionDemand`
(WHAT surface is demanded) plus `EvalPolicy` (HOW it is evaluated):

```rust
struct ProjectionDemand {
    path: ProjectionPath,            // the demanded projection path (interned, prefix-shared)
    facets: SurfaceFacetSet,         // which surface facets are demanded (members / index sigs / heritage / …)
    member_demand: MemberBodyDemand, // member-SET only vs member-set + per-member BODY
    call_signatures: bool,           // call signatures demanded
    construct_signatures: bool,      // construct signatures demanded
    index_signatures: bool,          // string/number/symbol index signatures demanded
    display_needs: DisplayNeeds,     // display/raw-string needs (display-only; never drives resolution)
}

struct EvalPolicy {
    alias_preservation: AliasPreservation,   // keep alias identity vs inline the alias body
    normalization_depth: NormalizationDepth, // how deep operators/unions/intersections normalize
    generic_open: GenericOpenPolicy,         // Bound | TypeParamShells (unbound params become TypeParam shells)
    operator_reduction: OperatorReduction,   // reduce vs leave operator carriers (Pick/Omit/keyof) unevaluated
    surface_role: SurfaceRole,               // prop / emit / model / slot / option / plain — structural role
    provenance: ProvenanceNeed,              // declaration-provenance retention
    merge_role: MergeRole,                   // how this demand participates in a merge (withDefaults, intersection arms)
    carrier_stop: CarrierStopPolicy,         // stop at semantic carriers (the Skeleton BFS stop) vs continue
}
```

`ProjectionDemand` + `EvalPolicy` is the demand identity carried on every projection
/ flow key field that today reads a coarse `ProjectionMode` (the `demand` field on
`ResolveClassSurface` / `ApparentType` / `ResolveMergedDeclaration` /
`ResolveAmbientNamespace` / `FlowReturn`, the `ProjectionReductionContext` projection
axes, and the flow `ReturnProjectionDemand` / `terminal_mode`). It is a key **demand
dimension** carried ALONGSIDE the split env hashes (§2.6–2.8), not a replacement: the
five-way env split (R21) is unchanged, and the demand point — like every
query-identity key field — carries no content/version hash and no `fact_dep_signature`
(R6). The five mode names remain as **public aliases / presets** over the lattice — a
stable public vocabulary, never a competing primary identity. Each preset is exactly one `(ProjectionDemand,
EvalPolicy)` point:

| Preset | `ProjectionDemand` | `EvalPolicy` |
|---|---|---|
| `Identity` | empty path; no member/body demand | `alias_preservation = Keep` — returns the alias declaration identity, never its body and never a miss |
| `Navigate` | the intermediate-hop path; member-set only | `alias_preservation = Keep`, `operator_reduction = NavigateOnly` — chooses the next hop, non-owning normalization only |
| `Shallow` | one shell level: member-name surface, no per-member body | `member_demand = SetOnly`, `operator_reduction = Leave` — one shell level; operator carriers (`Pick<…>`) stay `Ref` / unevaluated |
| `Expanded` | the terminal projection: member set + the demanded per-member bodies | `member_demand = SetPlusBody` on the terminal hop, `normalization_depth = Terminal` — `keyof T` emits the member-name literal-union from T's SHALLOW surface without entering member bodies |
| `Skeleton` | the BFS / generic-helper traversal surface | `generic_open = TypeParamShells` + `carrier_stop = StopAtCarrier` — unbound type parameters become `TypeParam` shells so Conditional branches do not collapse to `never` for unbound generics |

`Skeleton` is therefore **not a special semantic mode** — it is exactly
`generic_open = TypeParamShells` plus the carrier-stop policy on the same lattice;
`Instantiate { base, args: [], context: InstantiateContext { projection_reduction, resolve_env_hash } }`
with `context.projection_reduction.mode = Skeleton` is `Instantiate` with that preset.
The presets are a closed convenience surface; a demand that does not fit a preset
constructs a `(ProjectionDemand, EvalPolicy)` point directly rather than adding a
sixth mode rung.

**Cache satisfaction / backfill is by LATTICE RELATION, not enum ordering.** A cached
entry's `satisfied_projection` is the `(ProjectionDemand, EvalPolicy)` point it
actually materialised. A warm hit is served only when the cached point **dominates**
(in the lattice partial order: a broader-or-equal demand under a compatible policy)
the requested point — i.e. the cached work provably covers the request. A broader
result may **backfill** a narrower entry only for the narrower points it actually
materialised (the lattice meet it covered); a narrower result must not pretend broader
work is cached, and two incomparable points (e.g. a `Skeleton` / `TypeParamShells`
slice vs a `Bound` expansion) never satisfy each other. This replaces any "broader
mode satisfies narrower mode by enum rank" reasoning — there is no total order on the
five names to rank, and `Skeleton` is incomparable to the expansion presets rather
than "below" them. Guards:
**`query_modes_are_presets_over_projection_demand_eval_policy`** (each of the five
names resolves to exactly its `(ProjectionDemand, EvalPolicy)` preset; no mode name is
a primary key dimension on any cache),
**`cache_satisfaction_is_demand_lattice_not_enum_order`** (a warm hit / backfill is
decided by the lattice dominance relation, not by mode-enum ordering; two incomparable
demand points never satisfy each other), and
**`skeleton_is_typeparamshells_plus_carrier_stop_not_special_mode`** (the `Skeleton`
preset is exactly `generic_open = TypeParamShells` + `carrier_stop`, with no
special-cased semantic branch keyed on a `Skeleton` mode tag).

**Cache-axis MINIMALITY + normalization (perf hardening).** Every context /
substitution / demand / env axis carried on a query-identity key — the
`(ProjectionDemand, EvalPolicy)` point here, the `InferenceContextKey`, the
substitution canonical hash, the split env hashes — must be **proven minimal and
normalized under benchmark pressure**, because the axis set is the single biggest lever
on warm-hit rate: over-keying (an axis that does not change the value, or an
un-normalized axis whose two equivalent forms hash differently) fragments one logical
result across many slots and collapses the hit rate; under-keying (a missing
meaning-affecting axis) serves a stale result. Concretely: substitution and demand axes
are CANONICALIZED before they enter a key (the substitution canonical hash and the
prefix-interned projection path are normal forms, so `["a","b"]` and an equivalent
interned path hash identically, and two equivalent substitution environments collapse
to one key); a demand axis that a family never branches on is NOT carried on that
family's key. This is benched, not asserted by inspection: removing or denormalizing an
axis must either break a correctness fixture (the axis was load-bearing) or leave the
benched hit rate unchanged (the axis was dead and should be dropped). Pinned by
**`cache_key_axes_are_minimal_and_normalized`** (PART 1 §6.2).

---

## 3. Typed semantic-query value domain (`SemanticQueryValue`)

The current `execute` returns a single type-node result domain
(`QueryResult<SemanticNodeId>`). That is honest only for keys whose result is a type
node — not for `FlowNarrowingAt`, `ContextualTypeAt`, `ResolveOverloadSet`,
`FlowReturn`, `ResolveCall`, or `ResolveDeclarationAugmentation`. A typed value layer
returns each key's correct value domain over **one shared** `SemanticGraphStore`
admission/inflight substrate (the single `ProjectSemanticDispatch::execute →
SemanticGraphStore` dedup/admission path is unchanged — only the value type becomes
typed):

```rust
enum SemanticQueryValue {
    TypeNode(SemanticNodeId),                // type-value keys (NOT ResolveDeclarationAugmentation)
    ProgramAnalysis(ProgramAnalysisValue),   // FlowNarrowingAt, ContextualTypeAt — ProgramAnalysisGraph facts
    DeclarationAnalysis(DeclarationAnalysisValue), // ResolveDeclarationAugmentation (Module + Global) — in-process declaration-analysis VALUE domain (wire home stays GraphTypeNode arms 23/25)
    OverloadSet(Arc<[SignatureRef]>),        // ResolveOverloadSet — ordered overload signatures
    FlowReturn(Arc<FlowReturnResult>),       // FlowReturn — demand-sliced return/body flow result
    ResolvedCall(Arc<ResolvedCallResult>),   // ResolveCall — reusable call-resolution result
    Relation(RelationPayload),               // Relate — public relation payload (outcome / bindings / proof + typed BudgetExceeded)
    // RESERVED-SEAM (NON-LIVE — no live SemanticQueryKey maps here; no SemanticQueryKeySpec row carries it):
    DiagnosticAnalysis(CheckResult),         // future native checker — see "Reserved native-checker query/value space" below
}
```

The `DeclarationAnalysis` arm is the in-process value-domain home for
augmentation facts (the `DeclarationAnalysisGraph` wire relocation was rejected;
the wire home stays the existing `GraphTypeNode` kinds 21–25): the query layer
resolves augmentation facts to `DeclarationAnalysis`, never to a `TypeNode`
result. `ResolveDeclarationAugmentation`
(both env-free `Module(ModuleSpecifier)` and env-free `Global(GlobalEnvScope)`
targets) resolves to `DeclarationAnalysis`, never `TypeNode`. Global augmentations
(`declare global` / `export as namespace` / UMD globals) are produced by declaration
analysis (in `verter_semantic::analysis`, alongside module-augmentation analysis,
with the same contributor-provenance discipline as merged declarations) and are
queryable through the generalized `Global(GlobalEnvScope)` key.

The `Relate` value domain is the public `RelationPayload` (carrying outcome,
inference bindings, and the relation proof — the same proof exposed off the
type-values surface), plus a typed `BudgetExceeded` non-admission. A `BudgetExceeded`
relation result is `ReturnOnly`: never warm-admitted, never backfilled, never
published as a partial/torn entry.

(Equivalently the engine may expose typed `execute_*` wrappers — `execute_type_node`,
`execute_program_analysis`, `execute_declaration_analysis`, `execute_overload_set`,
`execute_flow_return`, `execute_resolved_call`, `execute_relation` — over the same
shared substrate; the requirement is that each key resolves to its correct value
domain.) Every `SemanticQueryKey` variant maps to **exactly one** value domain.

Guards: **`every_semantic_query_key_maps_to_exactly_one_value_domain`**,
**`flow_contextual_keys_return_program_analysis_value`**,
**`augmentation_keys_return_declaration_analysis_value`**,
**`declaration_augmentation_facts_not_type_nodes`**,
**`relate_query_value_carries_relation_proof_and_budget_state`**, and
**`no_non_type_value_smuggled_into_graph_type_node`** (no `OverloadSet` /
`FlowReturn` / `ResolvedCall` / `Relation` / `ProgramAnalysis` / `DeclarationAnalysis`
value and no relation-proof value is ever materialised as a `GraphTypeNode` arm —
the type-values-only surface admits only `TypeNode` values).

### Reserved native-checker query/value space (NON-LIVE — reserved names, not live spec rows)

The value domain reserves — but does NOT build — the surface a future native checker
block would land on, so that future block is a clean ADDITION rather than a re-shape:

- The reserved value-domain arm **`SemanticQueryValue::DiagnosticAnalysis(CheckResult)`**
  (above) is the would-be home for whole-region / whole-file / whole-program
  diagnostics. It is **NON-LIVE**: no live `SemanticQueryKey` variant maps to it, and
  the generated `SemanticQueryKeySpec` table carries **no** row for it — the standing
  enum==table meta-guard (`semantic_query_key_spec_table_equals_enum`) counts only the
  live query variants, and this arm has no live query, so it is a reserved value name,
  not a live spec row.
- The reserved query names **`CheckProgram` / `CheckFile` / `CheckRegion` /
  `CheckExpression` / `CheckAssignable` / `CheckCall` / `CheckDeclaration`** are
  RESERVED for that future checker block. They are NOT added to the live
  `SemanticQueryKey` enum or the spec table now; they are documented here only so the
  future block does not collide with an existing name and so its diagnostics route
  through the ONE `ProjectSemanticDispatch::execute` dispatch (not a second checker
  resolver). The future checker must produce diagnostics from the existing
  `Relate` / `ResolveCall` / `FlowReturn` / `ContextualTypeAt` facts — no second
  diagnostic engine beside those, no diagnostic projection-repair, and no TS
  text-based diagnostic path.
- **HARD RULE — typeinfo must NOT route through whole-body checking.** The reserved
  arm / names exist only so a future checker is clean; typeinfo parity (the 362-row
  scope) NEVER depends on them. No typeinfo query — `FlowReturn`, `ResolveCall`,
  `ContextualTypeAt`, member projection, or any U2/U6 reducer — may dispatch a
  `Check*` query or whole-body type-check a region to answer a typeinfo request.
  Typeinfo stays demand-sliced (parent §5); whole-body checking is a separate future
  layer over the same resolver, gated by `reserved_checker_queries_are_non_live_typeinfo_does_not_whole_body_check`.

This is a MINIMAL reservation: a reserved value arm + a reserved-names list + the
non-live note. The checker itself (its execution, its diagnostic taxonomy, its parity
manifest) is explicitly out of scope here and is a sibling follow-up plan, not a
typeinfo-parity block.

---

## 4. Inference / Relation / Operators

`Relate` is the **sole** assignability authority. It handles top/bottom/any/unknown/
never, optional/readonly/exact-optional, tuple rest, call/construct signatures,
abstract vs concrete construct signatures (an `AbstractConstruct` construct signature
is not assignable where a concrete constructor is required), private/protected
compatibility, apparent types, enum/unique-symbol identity, and relation-kind
differences. Its cache identity is the full upgraded key (§2.7), not the bare
`(source, target)` pair. Under that key it produces a public `RelationPayload`, not
a bare tri-state.

`InferBind` is **relation-owned**, and inference is **session-owned**: every
binding-producing relation runs inside the active `InferenceSession` of the
enclosing `CheckerTransaction` (§4.2), which is the SOLE inference substrate.
Add `InferTargetPattern::{ObjectProperty, TupleHead, TupleTail, TupleInit,
TupleLast, ParamTuple, ReturnPosition, TemplatePart}`. Conditional `infer`,
reverse-mapped inference, contextual-callback inference, overload applicability,
and final substitution all collect into the same session — there is no
per-surface matcher (the one-resolver rule applied to inference: §4.2).
Conditional reduction consumes relation bindings; it does not implement its own
matcher. Binding-producing relation work is keyed by the session's
`InferenceContextKey` — the cache-identity projection of the active session
(§2.7, §4.2) — so the same pair under a different inference setup does not
warm-hit, and the in-flight session deltas are never themselves cached.

### 4.0 Variance is MEASURED (marker-type probe fixed-point), not assumed

`InferenceContextKey.variance_phase` / `InferenceSession`'s `priority` measurement
(§2.7, §4.2) name the variance pass in scope (covariant / contravariant / invariant);
they do NOT settle WHAT a type parameter's variance IS. The `variance_phase` field is
the pass marker the relation consults — it is **not** a bare stand-in for the
parameter's measured variance. A generic type's per-parameter variance is computed by
a real algorithm — a **marker-type probe fixed-point** — and that MEASURED variance
feeds §4.1's relation (it decides whether relating `G<A>` to `G<B>` relates `A`/`B`
covariantly, contravariantly, invariantly, or bivariantly per the method-parameter
quirk):

- **Marker-type probe.** Variance of a type parameter `T` of a generic `G` is measured
  by instantiating `G` with two distinguished **marker** types (`super-marker` /
  `sub-marker` sentinels) in `T`'s position and relating the two instantiations through
  the SAME `Relate` engine (§4.1): if `G<super>` relates to `G<sub>` the parameter is
  covariant, if `G<sub>` relates to `G<super>` it is contravariant, if both it is
  bivariant, if neither it is invariant. There is no separate variance matcher — the
  probe runs the one relation engine.
- **SCC-aware fixed-point.** Mutually-recursive generics (`G` references `H` references
  `G` in their parameter positions) form a variance strongly-connected component. The
  measurement runs as a fixed-point over the SCC: parameters start at the unit
  (bivariant) and the probe iterates the relation until the measured variances stop
  changing (the standard variance fixed-point — a parameter only ever moves AWAY from
  bivariant toward a more-constrained variance, so the iteration is monotone and
  terminates). A parameter whose measurement cannot close (budget-abandoned) is treated
  as invariant (the sound conservative direction), never silently assumed covariant.
- **Cached by declaration / env / TS-version.** Measured variance is CACHED keyed by
  the generic's declaration identity (the content-free, env-bearing `ResolvedDeclSlotIdentity`, R6) + the split env
  hashes it depends on (`type_env_hash` — which now folds in the TS semantic version,
  §2 / `fact-based-cache.md`) + `lib_env_hash` where the parameter's constraint reaches
  lib surfaces. The cached measured-variance value is version-rooted on
  `ReadSetSignature.facts` like any other query-identity result; it is recomputed when
  the declaration or the relevant env changes, never per relation.
- **Bivariant-method quirks live in relation POLICY, not ad hoc.** TS's
  method-parameter bivariance (a method-shaped member's parameters relate bivariantly
  under the non-`strictFunctionTypes` method rule, whereas a property-shaped function's
  parameters relate contravariantly) is represented in `RelationPolicy` (§2.7) — the
  relation reads the policy flag to decide method-parameter bivariance, rather than a
  scattered special-case at each call site. The measured variance and the policy
  together decide each parameter relation.

The MEASURED variance — not a bare `variance_phase` enum stand-in — is what §4.1's
relation consumes when relating generic instantiations. Guard:
**`variance_is_measured_by_marker_probe_fixed_point_not_assumed`** (variance is computed
by the SCC-aware marker-probe fixed-point and cached by declaration/env/TS-version;
a discriminating fixture pins a contravariant-parameter and a bivariant-method case
against the oracle and asserts variance is measured, not assumed covariant; owned at
`U2.RELATION_INFER`).

### 4.1 Relation cycle / assumption protocol (coinductive SCC)

`Relate` is mutually recursive — relating two structural types relates their
members, relating a generic instantiation relates its substituted body, and a
conditional/inference relation can re-enter `Relate` on the same nodes. The full key
+ the `RelationBudget` pair memo bound the work but do not by themselves give a
checker-grade termination/soundness story for same-stack re-entry: a naive memo that
admits the in-progress pair's provisional state would publish a transient
`Unknown`/cycle value as if final. The end state is an explicit scoped-assumptions
protocol with a **coinductive SCC / obligation-discharge** algorithm. "Discharged
before publishing" is an SCC closure over the relation-stack assumption graph, and a
valid coinductive SCC (a genuinely recursive relation — `interface A { next: A }`
vs `interface B { next: B }`) MUST be publishable when the SCC closes with no
failing outgoing obligation; it is not treated as undischargeable merely because it
has no non-recursive base case:

- **Same-stack re-entry uses scoped ASSUMPTIONS keyed by the FULL `Relate` identity
  (including `InferenceContextKey`).** When `Relate` re-enters a pair already in
  progress on the current relation stack, it does not recompute and does not consult
  the warm memo; it records/consults a scoped assumption that the pair relates (the
  standard "assume the relation holds and verify the rest" coinductive step). The
  assumption is keyed by the full identity — `source`, `target`, `relation`,
  `policy`, `source_freshness`, `inference_context`, `context` — not the bare pair,
  and is scoped to the in-flight relation stack (a per-relation-root scope), not a
  process-global table. An assumption recorded under one full identity is never
  reused for a different relation identity on the same pair.
- **Same-stack assumptions form a relation SCC discharged by closure over outgoing
  obligations.** The set of mutually-assuming pairs forms a relation
  strongly-connected component in the assumption graph. The SCC is discharged — and
  every relation in it becomes admissible as a final, warm-cacheable result — when
  ALL its outgoing NON-ASSUMPTIVE obligations (the sub-relations that are not
  themselves back-edges into the SCC — member relations, instantiated-body
  relations, constraint relations) finish POSITIVE under the same full identity. The
  assumptive back-edges are not obligations to discharge separately — they are the
  coinductive "assume it holds" edges the SCC closure resolves. So a genuinely
  recursive relation whose only unresolved edges are its own back-edges, and whose
  every non-assumptive obligation holds, discharges and publishes — it is not
  rejected for lacking a non-recursive base case:
  - **All outgoing non-assumptive obligations POSITIVE ⇒ the SCC closes POSITIVE.**
    Every relation in the SCC is `Assignable` (carrying its bindings), and a
    successful recursive SCC MAY publish a proof node `CoinductiveCycle { keys }` —
    the set of full `Relate` keys that co-discharged — as the relation proof on each
    member's `RelationPayload`.
  - **Any outgoing non-assumptive obligation NEGATIVE ⇒ `NotAssignable`** (a final,
    publishable negative outcome — not a `ReturnOnly` cycle).
  - **Any outgoing obligation `Unknown` / cancelled / `BudgetExceeded` ⇒
    `ReturnOnly`** — returned to the caller but never warm-admitted, never
    backfilled, never published as a partial/torn `RelationPayload`, never recorded
    as a fact signature/backfill. This is the only genuinely undischargeable case.
- **Transient `Unknown`/cycle assumptions are NEVER warm-admitted or exposed as
  final `RelationPayload` proof; a closed positive SCC publishes
  `CoinductiveCycle { keys }` instead.** The provisional "assume it holds" value used
  during same-stack re-entry is a transient relation-stack sentinel, never a cache
  entry and never the published proof. When an SCC closes positive, the published
  proof is the durable `CoinductiveCycle { keys }` proof node — distinct from the
  in-progress sentinel. The cycle sentinel cannot leak into the warm memo or onto
  the type-values surface (it is not a `GraphTypeNode` arm — `CoinductiveCycle { keys }`
  is carried through `RelationPayload` / the payload-side proof table like any other
  relation proof).

Guards: **`relation_cycle_assumptions_are_scoped_to_full_relate_identity`**,
**`relation_coinductive_scc_discharges_on_outgoing_obligations`** (a valid coinductive
SCC closes positive and publishes `Assignable` with a `CoinductiveCycle` proof; a
negative obligation yields publishable `NotAssignable`; only `Unknown`/cancelled/
`BudgetExceeded` makes the SCC `ReturnOnly`), and
**`relation_cycle_sentinel_is_never_warm_admitted`** (the transient sentinel is
`ReturnOnly`, and a positive SCC publishes `CoinductiveCycle` rather than fabricating
it from the sentinel — the relation-cycle analogue of
`flow_cycle_sentinel_is_never_admitted_as_cache_entry`). Relation proofs stay off
the type-values surface, pinned by **`relation_proofs_not_graph_type_nodes`** and
**`typeinfo_relate_payload_exposes_relation_proof_without_graph_type_node`**.

**Memo locality + FAST negative/unknown paths (perf hardening).** Relation and
inference are the hottest reducers, and the common case is a CHEAP answer: a
not-assignable, no-match, or unknown outcome that should be decided WITHOUT the deep
structural / coinductive-SCC machinery above. Two design properties are required and
benched. (1) **Fast-reject before deep work.** Before opening an SCC scope or relating
members pairwise, `Relate` runs cheap structural discriminators — primitive / literal /
shape-tag / brand-identity / arity mismatches that prove `NotAssignable` (or a definite
no-match) in O(1)–O(tag) — and only the survivors enter the structural relation. A
mismatch never pays for member recursion. The `RelationBudget` pair memo (§6) is keyed
by the FULL `Relate` identity so a repeat negative is a memo hit, not a re-walk. (2)
**Memo locality.** The relation pair memo and the `InferenceSession` candidate tables
are laid out for locality — interned node IDs as keys, the per-relation-root assumption
scope kept contiguous, hot pairs (the same `(source, target)` relation re-asked inside
one root) resolving against a local table rather than the process-wide store. The
negative / unknown answer must be as cheap to reach as the positive answer is to cache;
a relation that fast-rejects must not allocate a proof, a fact payload, or a session
transaction. Pinned by **`relation_negative_and_unknown_paths_are_fast`** (a
discriminating bench asserts a mismatch / no-match / unknown is decided by the
fast-reject path and a repeat is a memo hit, both without entering the structural-SCC
or member-recursion machinery; PART 1 §6.2).

### 4.2 The `CheckerTransaction` + `InferenceSession` substrate

Cache identity (§2) makes results reusable; it does not by itself make inference
**checker-grade**. Type-parameter inference in TypeScript is a stateful
fixed-point: candidates accumulate per parameter across argument positions and the
return position, priority and variance decide how competing candidates combine,
fixation freezes a parameter, and overload resolution speculates per candidate and
discards losers. A flat `(source, target)` relation memo cannot express that
state. The end state is a first-class, transient inference substrate that lives
**inside the one resolver** (it is the cold-compute state of
`ProjectSemanticDispatch::execute`, not a second engine): a `CheckerTransaction`
holding one or more `InferenceSession`s. The same substrate drives generic call
inference, conditional `infer` extraction, reverse-mapped inference,
contextual-callback inference, overload applicability, and final substitution —
**one inference engine, not per-surface matchers** (the one-resolver rule applied
to inference). Only a completed, deterministic session is admitted, and what is
admitted is the FINAL typed result, never the mutable session.

**`CheckerTransaction` — the per-root cold-compute frame.** Opened when a root
`SemanticQueryKey` cold-computes a result that requires inference (a `ResolveCall`,
a binding-producing `Relate`, a `Conditional` with `infer`, a `FlowReturn` that
solves a generic call, a `ContextualTypeAt` that drives a callback). It carries:

```rust
struct CheckerTransaction {
    root: SemanticQueryKey,                  // the cold-compute root that opened the transaction
    env: SplitEnvHashes,                     // the five split env hashes (parse/resolve/type/lib/project), R21
    contextual_target: Option<ContextualTarget>, // the expected-type / contextual target in scope
    overload_policy: OverloadPolicy,         // first-applicable vs best-match overload selection
    freshness_excess_policy: FreshnessExcessPolicy, // fresh-object-literal excess-check / freshness mode
    relation_cycle_stack: RelationAssumptionStack,  // §4.1 coinductive-SCC scoped assumptions, shared
    reentry_stack: CheckerReentryStack,      // §4.2 shared re-entry / cycle-id space (below)
    budget: CheckerBudget,                   // shared with RelationBudget / CallResolutionBudget / FlowSliceBudget
    read_set: ReadSetSignatureAccumulator,   // the deterministic read-set (ReadSetSignature.facts) for admission
    sessions: SessionStack,                  // the active InferenceSession stack (speculative + committed)
}
```

The `read_set` is the same `ReadSetSignature.facts` path-precise observation set
the fact-cache validates on every warm hit (R-rules); admission records it on the
final value. `env` is the split five-hash set (never a bundled
`project_config_hash`, R21); the transaction carries no content / version hash and
no `fact_dep_signature` on any cache-identity it produces (R6).

**`InferenceSession` — one per active inference scope; holds one `InferenceInfo`
per inferable type parameter.** A session is the mutable working state for a single
attempt to infer a signature's type parameters from a set of argument / return
positions. `ResolveCall` opens one **speculative** session per overload candidate;
the candidate that wins keeps its session, the losers' sessions are discarded
without publishing anything. Each session carries:

```rust
struct InferenceSession {
    signature: SignatureRef,                 // the candidate signature whose params are being inferred
    infos: BTreeMap<TypeParamId, InferenceInfo>, // one per inferable type parameter, deterministic order
    no_infer_mask: NoInferMask,              // occurrence-local NoInfer suppression in effect (§1.2)
    contextual_inference_mode: ContextualInferenceMode, // whether/how the contextual target drives inference
    state: SessionState,                     // InProgress | CompletedDeterministic | Abandoned(reason)
}

struct InferenceInfo {
    candidates: CandidateSet,                // covariant inference candidates collected so far
    contra_candidates: CandidateSet,         // contravariant candidates collected so far
    priority: InferencePriority,             // the CLOSED priority ladder (below)
    top_level: bool,                         // candidate observed at a top-level (non-nested) position
    is_fixed: bool,                          // fixation flag — a fixed param collects no further candidates
    constraint: Option<SemanticNodeId>,      // the type-parameter constraint
    default: Option<SemanticNodeId>,         // the type-parameter default
    const_param_policy: ConstParamPolicy,    // `<const T>` const-ness propagation
}
```

`InferencePriority` is a **CLOSED enum** — a fixed ladder, not an open integer —
with an explicit `combinable` marker bit per rung:

```rust
enum InferencePriority {                     // lowest → highest; `combinable` marker per rung
    ReturnTypePosition,                      // lowest: a candidate inferred from the return position
    NakedTypeParameter,                      // a naked `T` argument position
    NonNakedTypeParameter,                   // `T` nested under a constructor (Array<T>, { x: T }, …)
    // …additional rungs (mapped-type source, contextual literal, etc.) added only by a reviewed
    //   schema bump; each rung declares whether it is `combinable` with a same-priority sibling.
}
```

**The EXPLICIT candidate-combination rule** (the meaning of "combine candidates",
stated mechanically rather than left to an implementation):

- **Same-priority covariant candidates UNION.** Two candidates collected for the
  same parameter at the same priority on the covariant side combine by union.
- **Contravariant candidates INTERSECT.** Candidates on the `contra_candidates`
  side combine by intersection (the contravariant measurement pass).
- **Higher priority REPLACES lower** — UNLESS the priority rung is marked
  `combinable`, in which case a higher-priority candidate unions with the retained
  lower-priority set instead of discarding it. (This is why the ladder is a closed
  enum with a per-rung `combinable` bit, not a bare integer: replace-vs-combine is
  a property of the rung, audited by the guard.)
- **A FIXED parameter collects no further candidates.** Once `is_fixed` is set,
  later positions contribute nothing to that parameter — fixation is monotone.
- **The widening-vs-literal fork happens on FIRST inference.** The first candidate
  for a parameter decides whether subsequent same-position literals widen to their
  primitive or stay literal (the literal-vs-widened decision is taken once, at the
  first candidate, consistent with §4.3's `widen_for_position`); `<const T>`
  (`const_param_policy`) and the `as const` site suppress widening per the oracle,
  not by a blanket rule.

**The composition — every inference surface runs inside the session:**

- **`ResolveCall`** opens one speculative `InferenceSession` per overload
  candidate, runs applicability + argument-to-parameter inference + fixation +
  final substitution inside the session, and publishes ONLY the winning completed
  `ResolvedCall`. Losing candidates' sessions never publish (no entry, no fact
  signature, no backfill).
- **Binding-producing `Relate`** (the `inference_context = Some(..)` arm of §2.7)
  MUTATES the active session — it deposits candidates into the relevant
  `InferenceInfo` and returns **session-local inference deltas**, NOT a globally
  cacheable partial result. A binding `Relate` delta is meaningful only within its
  session; it is never warm-admitted on its own. Pure non-binding assignability
  (`inference_context = None`) is a normal `Relate` that does not touch a session
  and whose `RelationPayload` caches normally.
- **Conditional `infer`** (§4.3) extracts its `infer` bindings through the session:
  the `infer U` positions are inferable parameters in a session whose candidates
  the conditional check-type relation deposits, and the true-branch substitution
  reads the session's fixed values.
- **Reverse-mapped inference, contextual-callback inference, and overload
  applicability** all run as sessions / session passes — there is no separate
  reverse-mapped matcher, no separate callback inference loop engine, and no
  separate applicability checker. The contextual-callback iterative generic
  inference loop (U6.CONTEXTUAL_CALLBACK) is exactly the session's **fixation
  fixed-point**: it iterates candidate collection → fixation → re-measurement until
  the session reaches `CompletedDeterministic` or the budget abandons it.
- **Reverse-mapped inference is a GENUINE added mechanism — relation-owned, inside
  the session.** Inferring a type parameter from a source assigned to a
  **homomorphic mapped target** (`inferring T from a value assigned to
  `{ [K in keyof T]: F<T[K]> }``) is not ordinary structural inference: TS reverses
  the mapping — for each source property it relates the source member type against the
  mapped template `F<T[K]>` to recover the corresponding `T[K]` candidate, then
  reassembles the inferred `T` from the per-key recoveries. This is the one place the
  inference engine genuinely needs reverse-mapped support, and it is added as a
  relation-owned pass INSIDE the `InferenceSession`: the reverse-mapping relates each
  source property through binding-producing `Relate` (§2.7) — depositing the recovered
  `T[K]` candidate into the relevant `InferenceInfo` — and the session's final
  substitution reassembles `T` from the fixed per-key values. There is NO separate
  reverse-mapped matcher and NO standalone reverse-mapping engine: it is a pass of the
  one session, owned by the mapped-type reducer (U2.MAPPED_TEMPLATE) + the session
  (U2.RELATION_INFER). Guard:
  **`reverse_mapped_inference_is_relation_owned_in_session`** (reverse-mapped inference
  runs through binding-producing `Relate` inside the `InferenceSession`, depositing
  per-key candidates; a discriminating fixture infers `T` from a value assigned to a
  homomorphic mapped target and asserts the recovery routes through the session, not a
  private reverse-mapping matcher; owned at `U2.MAPPED_TEMPLATE` + `U2.RELATION_INFER`).
- **Fresh-object-literal excess checking is per-property + spread-taint-aware, in the
  session.** `source_freshness: FreshnessKey` (§2.7) carries the fresh-vs-non-fresh
  mode, but freshness is not a single whole-object bit: an object literal's freshness is
  tracked **per property**, and a spread (`{ ...base, x: 1 }`) **taints** the freshness
  of the spread-in properties (a property that came from a non-fresh spread source is
  NOT subject to excess-property checking, while the literal's own written properties
  ARE). The session models this with a per-property freshness/spread-taint algorithm:
  each property of a fresh literal carries a freshness/taint bit (own-written = fresh
  / excess-checked; spread-in from a non-fresh source = tainted / not excess-checked;
  spread-in from a fresh source propagates that source's per-property bits), and the
  excess-property relation consults the per-property bit rather than the whole-object
  flag. This is the spread-aware extension of the `FreshnessKey` excess-check mode and
  it lives in the session (the relation/excess-check substrate), not a second checker.
  Guard: **`freshness_tracks_per_property_spread_taint`** (excess-property checking is
  decided per property with spread-taint propagation; a discriminating fixture spreads
  a non-fresh source into a fresh literal with an extra own property and asserts only
  the own property is excess-checked while the spread-in properties are not; owned at
  `U2.RELATION_INFER` + exercised on the return path at `U6.VALUE_INFERENCE`).
- **Final substitution** instantiates the signature from the session's fixed
  `InferenceInfo` values — the same session that collected the candidates produces
  the substitution, so there is no second inference pass over the result.

**ADMISSION (fact-cache R-rules).** Only a **COMPLETED, DETERMINISTIC** session may
be admitted, and what is admitted is the FINAL typed result — a `ResolvedCall`, a
`RelationPayload`, a `Conditional` reduction, or a concrete instantiation — **never
the mutable session and never a session-local partial / delta**. A session that is
cancelled, budget-exceeded, superseded by a generation change mid-flight, or left
non-deterministic routes its result through `ReturnOnly`: the value is returned to
the caller but nothing is published (no cache entry, no reverse-index metadata, no
fact signature, no backfill) — the inference analogue of the relation cycle's
`ReturnOnly` path (§4.1) and the flow slice's `BudgetExceeded` non-admission
(§6, U6.FLOW_RETURN_SUBSTRATE). The admitted value's cache identity is the §2 key
of the root query; for binding-producing relations it is the completed-session
fingerprint `InferenceContextKey` (§2.7) — never the in-flight session object.

**The `CheckerReentryGraph` — one shared re-entry / cycle-id space.** `ResolveCall`,
`FlowReturn`, `ContextualTypeAt`, and `FlowNarrowingAt` (and the reducers they call)
are mutually recursive: resolving a call contextually types a callback, which solves
a callback body via `FlowReturn`, whose narrowing relates argument types via
`Relate`, which can re-enter `ResolveCall`. The cycle
`ResolveCall → FlowReturn → narrowing → ResolveCall` MUST NOT self-await on the
in-flight dispatch slot or burn budget spinning. The end state is ONE
`CheckerReentryGraph` shared across all four entry points (and the reducers
beneath them), carried on the `CheckerTransaction.reentry_stack`:

- **Keying — full normalized identity per node.** Each node on the re-entry stack is
  keyed by the FULL normalized identity of its query: a `FlowReturn` node by
  `FlowReturnContext + ReturnProjectionDemand + FlowInputContext` (§2.5, §5), a
  `ResolveCall` node by the full `ResolveCall` identity (§2.4), a `ContextualTypeAt`
  / `FlowNarrowingAt` node by its `ProgramPointId` + `ProgramAnalysisContext` PLUS
  its per-variant key axis (`FlowNarrowingAt` carries `flow: FlowNarrowingKey`,
  `ContextualTypeAt` carries `contextual: ContextualTypingKey`)
  (§2.8). The re-entry stack is the single shared cycle-id space — the per-flow
  cycle space (§5) and the relation assumption stack (§4.1) are the relation /
  flow-typed views of this one shared stack, not separate spaces that could
  diverge.
- **Same-stack re-entry records a transient assumption — but discharge is
  per-value-domain SCC / fixed-point, not "return the in-flight assumption."** When
  dispatch reaches a node whose full normalized identity is already on the
  `reentry_stack`, it does NOT recompute, does NOT self-await the in-flight slot, and
  does NOT consult the warm memo; it records / consults a transient **re-entry
  assumption** for that node (the typed analogue of the relation coinductive "assume
  it holds" edge — §4.1) so the cross-engine cycle does not deadlock or budget-spin.
  That assumption is only the coinductive STEP. Each value domain then DISCHARGES its
  re-entry SCC to a domain-typed converged result before anything is warm-admitted,
  exactly as `Relate` discharges its §4.1 coinductive SCC over outgoing non-assumptive
  obligations (Relation keeps that §4.1 protocol unchanged; it simply participates in
  this one shared stack rather than owning a private one):
  - **`FlowReturn` — SCC fixed-point to a STABLE projected return type.** The
    re-entry SCC over `FlowReturn` nodes iterates the return contributors (each return
    site's selected-path value-provider result joined across control-region edges)
    to a fixed point. Only a STABLE, exact projected return type is publishable; if
    the iteration does not converge to an exact result (a contributor stays
    `Unknown` / cancelled / budget-abandoned), the node is `ReturnOnly` carrying a
    typed `DegradedReason` — never a warm entry built from the transient assumption.
  - **`ResolveCall` — SCC fixed-point to the overload-winner + substitution
    fingerprint.** The re-entry SCC over `ResolveCall` nodes iterates overload-winner
    selection plus the session's substitution fingerprint (the
    `InferenceContextKey`) to a fixed point. Only a COMPLETED, deterministic
    `ResolvedCall` (a settled overload winner with a settled substitution
    fingerprint) is publishable; a session that stays speculative / non-deterministic
    / budget-abandoned is `ReturnOnly`.
  - **`ContextualTypeAt` — SCC fixed-point to contextual-target equality.** The
    re-entry SCC over `ContextualTypeAt` nodes iterates the contextual target /
    substitution to EQUALITY (the contextual target stops changing between
    iterations). Only a STABLE contextual fact is publishable as a
    `ProgramAnalysisGraph` value; an unconverged contextual target is `ReturnOnly`.
  - **`Relate`** keeps its §4.1 coinductive-SCC discharge over outgoing non-assumptive
    obligations (positive ⇒ publish `CoinductiveCycle`; negative ⇒ publishable
    `NotAssignable`; `Unknown` / cancelled / `BudgetExceeded` ⇒ `ReturnOnly`).
  - **HARD RULE — no transient assumption or cycle sentinel may warm-admit.** The
    re-entry assumption (and any cross-engine cycle sentinel) is a transient stack
    value: it is NEVER warm-admitted, NEVER backfilled, NEVER published as a final
    result, NEVER recorded as a fact signature. ONLY a converged, stable,
    deterministic per-domain result is cacheable; everything else (unconverged,
    cancelled, superseded mid-flight, budget-exceeded) routes through `ReturnOnly` —
    the same `ReturnOnly`-vs-publish discipline as §4.1 and the flow slice's
    `BudgetExceeded` non-admission (§6).

Guards (R6 — registered in §11.8 and the owning blocks):
**`inference_runs_in_checker_transaction_not_per_surface_matcher`** (generic call
inference, conditional `infer`, reverse-mapped inference, contextual-callback
inference, overload applicability, and final substitution all enter the
`InferenceSession` substrate — there is no second inference matcher),
**`only_completed_deterministic_sessions_are_admitted`** (a session-local delta /
in-flight session is never warm-admitted; a cancelled / budget-exceeded / mid-flight
session is `ReturnOnly`, and only the final `ResolvedCall` / `RelationPayload` /
`Conditional` / instantiation publishes),
**`inference_candidate_combination_matches_priority_and_variance`** (same-priority
covariant candidates union, contravariant intersect, higher priority replaces lower
unless the rung is `combinable`, and a fixed parameter collects no further
candidates — an oracle-pinned discriminating fixture exercises return-position vs
argument-position competition), and
**`checker_reentry_graph_spans_flow_call_contextual_narrowing`** (the
`ResolveCall → FlowReturn → narrowing → ResolveCall` cycle records a re-entry
assumption on the shared stack and never self-awaits or budget-spins; keyed by full
normalized identity per node), and
**`cross_engine_cycle_discharge_admits_only_stable_deterministic_results`** (each
value-domain re-entry SCC discharges to a converged deterministic result before warm
admission — `FlowReturn` to a stable projected return type, `ResolveCall` to a
completed overload-winner + substitution fingerprint, `ContextualTypeAt` to
contextual-target equality; a discriminating fixture forces a non-converged /
budget-abandoned cross-engine cycle and asserts the transient assumption / cycle
sentinel is `ReturnOnly`, never warm-admitted, never recorded as a fact signature,
and only a converged result is cached),
**`reverse_mapped_inference_is_relation_owned_in_session`** (reverse-mapped inference
is a relation-owned session pass — per-key recovery through binding-producing `Relate`,
reassembled by the session's final substitution — not a private reverse-mapping
matcher), and **`freshness_tracks_per_property_spread_taint`** (fresh-object-literal
excess checking is decided per property with spread-taint propagation, in the session,
not a whole-object freshness bit).

### 4.3 Conditional / mapped / index / template reducers

- **Conditional:** `any` evaluates both branches and unions; distributive `never`
  collapses to `never`; open conditionals distribute the remaining `ProjectPath` into
  both branches; closed conditionals reduce immediately. `infer` extraction runs
  inside the active `InferenceSession` (§4.2): each `infer U` is an inferable
  parameter whose candidates the check-type relation deposits via binding-producing
  `Relate`, and the true branch substitutes from the session's fixed values — the
  conditional reducer never runs a private `infer` matcher.
- **Mapped / index / template:** mapped `-?` strips ONLY the optional-property-origin
  `undefined`; key remap runs through `TemplateLiteralReduce`; `as never` drops keys;
  indexed access distributes union keys, honors string/number/symbol index precedence,
  and keeps intermediate hops in `Navigate`.
- **Template-literal numeric / bigint lexing follows TS lexical rules.**
  `TemplateLiteralReduce` (§2.6) models TS's **lexical numeric/bigint parser** when a
  template-literal pattern infers a numeric/bigint segment (`infer N extends number` /
  `extends bigint`, and the placeholder-vs-literal matching that produces a numeric
  literal type). The reducer does NOT use Rust's `str::parse` or an ad-hoc numeric
  splitter; it applies TS's own lexical grammar so the matched literal type is exactly
  what `tsgo` produces: decimal / hex (`0x`) / octal (`0o`) / binary (`0b`) integer
  forms, exponent (`1e3`) and fractional forms, numeric separators (`1_000`), leading-
  `+`/`-` sign handling, the `n` `bigint` suffix (and the rule that a fractional /
  exponent form is NOT a valid bigint), and the canonical normalization TS applies when
  turning the lexed value back into a literal-type name (e.g. the normalized decimal
  spelling). A segment that does not lex as a valid number/bigint under TS's grammar
  does NOT match the numeric `infer` (it stays a string segment / fails the conditional
  branch), matching TS. Guard:
  **`template_literal_reduce_models_ts_numeric_bigint_lexing`** (template-literal
  numeric/bigint `infer` matching uses TS lexical numeric/bigint semantics, oracle-
  pinned against `tsgo`; a discriminating fixture pins a hex / separator / exponent /
  `bigint`-suffix case and asserts the matched literal type equals the oracle, not a
  Rust-`parse` result; owned at `U2.MAPPED_TEMPLATE`).
- **Mapped `-?` clears the optionality/presence FLAG, not arbitrary `undefined` in
  the value.** TS7's `-?` removes ONLY the `undefined` that originates from a property
  being optional (the member-presence / optional-origin component); it does **not**
  strip an explicitly declared `| undefined` on a required property. The reducer
  models the optional-origin / member-presence component SEPARATELY from the value
  type: the per-member shell carries an optional-origin/presence flag distinct from
  the value type, and `-?` clears that flag (and the optional-origin `undefined` it
  implies) WITHOUT rewriting the value type — so `{ a?: string }` under `-?` becomes
  `{ a: string }`, while `{ a: string | undefined }` (a required property with an
  explicit `| undefined` value) REMAINS `{ a: string | undefined }`. Symmetrically,
  `+?` / a bare `?` sets the presence flag without otherwise altering the value type.
  This is the same two-component model the two-fact `MemberPresence` / `Member` cache
  split uses — `-?` operates on the `MemberPresence` component, never on the `Member`
  value's declared `undefined`. The stripping fixture uses OPTIONAL properties
  (`a?: string; b?: number`) so it actually exercises optional-origin removal.
  Guards (oracle-pinned against the pinned `tsgo` oracle):
  **`mapped_minus_optional_strips_only_optional_origin_undefined`** and
  **`mapped_minus_optional_preserves_explicit_undefined_on_required_property`**.

Widening is one helper: `widen_for_position(ty, WideningSite)`.

### 4.4 `satisfies` — TS7 oracle-pinned

`E satisfies T` checks assignability of `E` to `T`, contextually types `E` with `T`,
then keeps the inferred source type of `E`, not `T`. The finer behavior is settled by
an oracle, not hand-written rules:

- Fresh object literals get excess-property checks unless the target admits the key
  through an index/key space. Non-fresh identifiers do not re-run fresh-object excess
  checks.
- Source keys are retained: `Record<string, V>` validates values but
  `keyof typeof value` stays the literal key union.
- `[1,2] satisfies readonly number[]` infers mutable `number[]`;
  `as const satisfies readonly number[]` stays readonly tuple;
  `as const satisfies number[]` must fail.
- Literal widening is not blanket — pinned against the oracle, not guessed; not
  collapsed into "all literals widen."

**Oracle mechanism (TS7-pinned, GENERATED — not hand-maintained):** oracle rows are
generated; each `OracleId` is deterministic from `(fixture, query,
compiler_options_hash, tsgo_version, oracle_schema_version)`. The generator runs
`pnpm exec tsgo` at the pinned version (`7.0.0-dev.20260526.1`) and writes checked-in
normalized snapshots. Default tests compare Verter ONLY to the checked-in snapshots;
they never invoke `tsgo`. Regeneration is feature/env-gated (a dedicated drift
generator) — the only place permitted to execute `tsgo`. A guard forbids `tsgo`
execution anywhere in runtime/default tests except the gated drift generator.

`satisfies` performs target-contextual validation and widening while preserving the
source member set where TypeScript does; it is not a blanket replacement with the
target and not a projection repair. Exact TS7 oracle pins are mandatory before lift.

### 4.5 Apparent types

`ApparentType` resolves primitive and array members through lib-declared wrapper
interfaces keyed by `lib_env_hash`. The query result is memory-only; reusable lib
artifacts may persist through `FileArtifactStore`, but query nodes do not persist
under U4.

### 4.6 `ThisType<T>` contextual object-literal binding (in `ContextualTypeAt`)

`ThisType<T>` is not an ordinary member surface — it is a **contextual marker** that
binds the `this` type inside an object literal's method bodies. When an object literal
is contextually typed by a target that includes `ThisType<T>` in an intersection (the
classic `{ methods: M } & ThisType<D & M>` pattern, e.g. a Vue-options / mixin object),
every method of the object literal is contextually typed with its `this` bound to `T`.
This contextual `this` binding is computed in **`ContextualTypeAt`** (the
program-analysis context that resolves the expected/contextual type at a point — §2.8,
behavior at `U6.CONTEXTUAL_CALLBACK`), NOT as a published type-node member and NOT a
structural rewrite of the object surface:

- When `ContextualTypeAt` resolves the contextual target of an object-literal method,
  it detects a `ThisType<T>` arm in the contextual target's intersection and supplies
  `T` as the method's contextual `this` type (so `this.x` inside the method resolves
  against `T`). The object literal's own surface is unchanged — `ThisType<T>` is a
  contextual fact consumed at the method body, exposed through `ProgramAnalysisGraph`
  like any other contextual-typing fact (§1.3), never a `GraphTypeNode` arm.
- `ThisType<T>` itself contributes no members to the object's apparent surface (it is a
  marker interface); only its contextual-`this` effect is observed. Absent an explicit
  `ThisType<T>` arm, the contextual `this` falls back to TS's default (the object type
  itself under the relevant `noImplicitThis` rule), via the same `ContextualTypeAt`
  path — no separate engine.

Guard: **`this_type_contextual_object_literal_binding_in_contextual_type_at`** (a
`ThisType<T>` arm in an object literal's contextual target binds the method `this` to
`T` through `ContextualTypeAt`, exposed as a `ProgramAnalysisGraph` contextual fact and
never a `GraphTypeNode` member; a discriminating fixture pins `this.x` inside a method
of a `… & ThisType<D>`-typed object literal resolving against `D`; owned at
`U6.CONTEXTUAL_CALLBACK`).

---

## 5. Flow Architecture (demand-sliced over a per-function flow graph)

The flow engine is demand-sliced; a full lowered body is not good enough. The flow
model is **one sparse `FunctionFlowGraph` built once per function** from the
function's `FunctionBodySkeleton`, with typed edges; a demand slice is **graph
reachability** over that structure, planned (not procedurally re-walked) per query.
The detailed U6 chapter lives in `docs/arch/native-flow-return.md`; the
cross-cutting contract is:

`FunctionBodySkeleton` (in/under `IndexedReady`): arena-free, shallow statement /
control skeleton, return-site index, lexical binding index, assignment/kill
summaries, no type lowering.

`FunctionFlowGraph` is a **sparse, arena-free dependence structure built ONCE per
function** from its `FunctionBodySkeleton` — the same density and the same
build-time-no-type-lowering discipline as the rest of `IndexedReady`. It does **no
type lowering at build time**: it stays a structural skeleton over interned slots /
paths / regions, and every type along an edge resolves on demand only when a slice
actually traverses it. Its nodes are the function's value definitions, return sites,
expression sites, control regions, and closure/loop boundaries; its edges are
**typed**, each edge class carrying exactly the dependence kind it represents:

**Build cost stays SHALLOW (perf hardening).** The build is a perf-critical path
because every queried function pays it once, so three properties are required and
benched: (1) **compact interned IDs** — slots, paths, regions, and node/edge handles
are interned integer IDs (prefix-shared projection paths), never owned strings or
boxed AST pointers, so the graph is cache-dense and cheap to hash; (2) **NO type
lowering at build** — the build reads only the arena-free `FunctionBodySkeleton`
(statement/control skeleton, return-site / binding indexes, write/kill summaries) and
emits structural edges; no `TypeExpr` is lowered, no `Relate`/`Instantiate`/import is
touched at build time (those happen only when a slice traverses an edge); and (3)
**LAZY region materialization for very large functions** — the graph is region-shaped
(§ the `ExecutableRegionId` abstraction below), so an oversized body does NOT eagerly
build or retain the full dense graph: the build materializes the region skeleton + a
per-region summary and materializes a region's interior edges only when a demand slice
reaches into that region. A huge function with a tiny demand slice pays only for the
regions the slice touches, never for the whole body. This is what makes the
shallow-by-default slice cheap at the build boundary, not just at the query boundary.

- **value-def** — a slot/path is defined by an expression (reaching definition).
- **path-write** — a write targets a specific projection path on a slot
  (`slot.P[0]…`), including optional / unknown writes.
- **eval-effect** — evaluating an expression mutates / narrows / calls into a binding
  even when its *value* is non-contributing (computed property-name expressions,
  spread / `Object.assign` source evaluation, assertion calls).
- **narrowing-predicate** — a branch predicate that narrows a slot along a control
  region (the fact the demand slice must carry to narrow the selected path).
- **control-region** — a node belongs to a control region (branch arm, switch case,
  try / catch / finally body) so the planner can compose branch joins and reachability.
- **closure-escape** — a slot is captured by an escaping closure (passed, returned, or
  stored beyond the frame), so its mutable value must widen at the escape boundary.
- **loop-summary** — a loop region's per-iteration write/kill summary, so the loop
  fixed-point joins on it without re-walking the body.
- **try/finally-override** — a `finally` control-return overrides the try / catch
  returns it dominates (a `finally` without return preserves them).

**Reserved region abstraction (`ExecutableRegionId` / `ExecutableRegionKind::Function`
— NON-LIVE beyond functions).** The 362 parity rows need function-body flow plus the
existing top-level expression lowering only; the flow graph is NOT generalized to a
whole `ExecutableRegionGraph` now. The architecture RESERVES a region abstraction so
`FunctionFlowGraph` is documented as ONE region kind — `ExecutableRegionKind::Function`,
addressable by an `ExecutableRegionId` — and the other region kinds (module top-level,
class static blocks, field / parameter initializers, decorator expressions, top-level
await, template regions) are NAMED as FUTURE region kinds, **not implemented**. The
demand planner, the slice nodes, and `flow_body_stable_hash` are already
region-shaped (they key on a function part / region identity), so a future block can
add a region kind without re-shaping the planner. Until such a block lands, only
`ExecutableRegionKind::Function` exists; no other region kind is built, and no
typeinfo row depends on one.

**Reserved injection seam (`ProgramAnalysisContributor` / `SemanticContribution` —
FUTURE-AWARE, NOT built).** The architecture reserves a NAMED seam by which a future
framework-semantic source (e.g. Vue template semantics) could feed typed facts into
the `ProgramAnalysisGraph` — `ProgramAnalysisContributor` emitting
`SemanticContribution`s (future typed facts `InjectedBinding` / `InjectedNarrowingFact`
/ `InjectedContextualType` / `InjectedRelation`). The framework-adapter system is NOT
designed here. The only requirement on the CURRENT architecture is that it stays
seam-clean: it must avoid text / fake-AST / type-node mutation as an injection
mechanism and must keep semantic SLOTS + provenance + env identity available, so the
future seam can deposit typed facts (carrying their own provenance + env identity)
rather than synthesising source. That is the whole obligation now; the contributor
system is a sibling follow-up, not a typeinfo-parity block.

`ReturnPathPeeker` is the **graph demand PLANNER** over the `FunctionFlowGraph` — not
a procedural mini-CFG walker. Given a demand `(return_site | expression_site,
projection_path, EvalPolicy)`, it computes the demand slice as **graph reachability**
from that origin across the typed edges, producing a `ReturnSlicePlan` whose nodes
are exactly those reachable under the edge-class rules below. It does not re-traverse
statement lists, re-discover bindings, or re-run a control-flow walk: the structure is
already in the graph; the planner only *selects* the reachable subgraph. Because the
origin can be a return site OR an arbitrary expression site, the same graph + planner
serves return-type queries **and** future expression-site queries (a typeinfo query at
an arbitrary program point) with **no second flow engine** — loops, closures, try /
finally, computed keys, and spreads are all already edge classes on the one graph.

The **two-frontier rule** is required for soundness and is preserved — expressed now
as **edge classes**, not as two separate procedural passes. Reachability follows two
edge-class families with different stop conditions:

- **Value-provider edges** (value-def + path-write) compute which sources provide the
  demanded value. Reachability along them MAY **stop at a definite-present write** for
  `P[0]` (the value is fully determined there). Optional / unknown writes are kept as
  `ProjectPath(source, P)` and earlier candidates remain reachable.
- **Effect edges** (eval-effect + narrowing-predicate + control-region + closure-escape
  + loop-summary + try/finally-override) **stay live even past a definite-present
  write**: an evaluation effect that already ran and mutated a binding the selected
  path reads is reachable regardless of whether the property carrying it is
  non-contributing for value. A sibling property whose value type cannot be lowered
  still contributes its **effect** edge when that effect changes a binding read by the
  selected path. Two effect classes the value-provider family skips because their value
  is overwritten later are carried by effect edges precisely because **evaluation
  effects survive a definite write even though value materialization does not**:
  - **Computed property-name expressions.** A computed key `[expr]: v` evaluates
    `expr` for its side effects regardless of whether that property's value is later
    overwritten or is not the demanded path. If `expr` assigns, narrows, or calls into
    a binding the selected path reads, its computed-key **eval-effect** edge is reachable
    — even when the property is non-contributing for value. Only its evaluation effect
    is taken; the selected value is not materialized from it.
  - **Spread / `Object.assign` evaluation effects.** A spread `...src` or an
    `Object.assign(target, src)` evaluates `src` (and reads its enumerable own keys)
    for side effects even when a later definite write to `P[0]` makes it
    non-contributing for the demanded value. If evaluating `src` affects a binding read
    by the selected path, its **eval-effect** edge is reachable past the definite write;
    only the spread's value contribution is skipped.

The two-frontier rule is required for soundness. Demanding `["b"]` in
`return { a: (x = "s"), b: x.toUpperCase() }` must not lower sibling `a`'s value type
but MUST stay reachable along `a`'s eval-effect edge, because `a`'s initializer assigns
`x` and `x` is read by the selected `b` — value-provider reachability stops at `b`'s
write, but the `x = "s"` eval-effect edge stays live and retypes `x` before
`x.toUpperCase()`.

Reachability rules (the typed-edge form of the contribution scan): object literals and
`Object.assign` scan path-write edges right-to-left for `P[0]` (value-provider
reachability stops only at a definite-present write; optional/unknown writes stay
reachable as `ProjectPath(source, P)`); known unrelated properties carry no
value-provider edge into the demanded path (skipped by syntactic key footprint, not
value resolution) but their eval-effect edges are still followed; `return { ...spread,
b }` with demand `["b"]` leaves `spread` and sibling `a` value-non-contributing (no
type resolution) while their eval-effect edges (including the spread / `Object.assign`
source's evaluation effect) stay reachable; `const r = { a, b }; return r` follows the
value-def edge to the last reaching definition if `r` is unescaped/unmutated, else
follows only the path-write edges that may affect `P` and returns a typed degraded path
result on unknown mutation — never lowers siblings; conditional returns reach per
return site across control-region edges, then join selected path results with the
narrowing-predicate edges needed for narrowing.

Flow-graph guards: **`function_flow_graph_built_once_per_function_skeleton`** (the
`FunctionFlowGraph` is constructed once per function from its `FunctionBodySkeleton`,
with no per-query rebuild and no type lowering at build time),
**`flow_slice_is_graph_reachability_not_procedural_walk`** (the demand slice is graph
reachability over the `FunctionFlowGraph` from the demand origin — the planner selects
a reachable subgraph and never re-runs a procedural mini-CFG statement walk),
**`flow_graph_effect_edges_stay_live_past_value_writes`** (the two-frontier soundness
as a typed-edge invariant: effect-class edges remain reachable past a definite-present
write for the demanded path, while value-provider edges may stop there), and the
perf-hardening guard
**`flow_graph_build_is_shallow_interned_no_lowering_lazy_regions`** (the build uses
compact interned IDs, lowers NO type at build time — asserting no `TypeExpr` lowering /
`Relate` / `Instantiate` / import fact is produced by graph construction — and
materializes oversized-function regions lazily: a large body with a small demand slice
materializes only the regions the slice touches, benched so build cost scales with the
sliced regions, not the whole body; PART 1 §6.2).

Planner guards: **`flow_return_path_peeker_spread_override_skips_overwritten_sibling`**,
**`flow_return_path_peeker_alias_return_projects_requested_member_only`**,
**`flow_return_path_peeker_unknown_alias_mutation_degrades_path_not_whole_body`**,
**`flow_return_path_peeker_definite_write_keeps_prior_effects_for_selected_value`**,
**`flow_return_path_peeker_compound_assignment_reads_previous_path_value`**,
**`flow_return_path_peeker_destructuring_assignment_tracks_path_writes`**,
**`flow_return_path_peeker_captured_binding_unknown_escape_degrades_path`**,
**`flow_return_path_peeker_local_closure_call_applies_path_write_summary`**,
**`flow_return_path_peeker_try_finally_return_override_controls_contribution`**,
**`flow_return_path_peeker_labelled_break_preserves_reachability`**,
**`flow_return_path_peeker_computed_key_effects_survive_definite_write`**, plus the
explicit `Mytype` negative guard.

`FlowReturn` / `FlowSlice` / `FlowSliceBudget` stay the cached query + slice + budget
**over** the `FunctionFlowGraph`; the slice is now the graph-reachability result the
planner produced. `FlowSliceHashNode` hashes only that reachable slice (the selected
return/control/binding subgraph); a full-body hash is allowed only for a true
whole-return request and is rejected for member-projection requests. Flow node / fact
identity is rooted by a per-function **`flow_body_stable_hash`** (body-SENSITIVE,
cosmetic-INSENSITIVE — computed from `FunctionBodySkeleton` + the `FunctionFlowGraph`
semantic structure, INCLUDING literals, operators, control flow, writes, calls,
property keys, and type-affecting syntax), NOT the body-insensitive decl-skeleton
`parse_stable_hash`: `return { b: 1 }` and `return { b: 2 }` MUST hash differently
(they share one `parse_stable_hash` — keying the flow node/fact on it would warm-hit
unsoundly). `parse_stable_hash` keeps its decl-skeleton meaning for decl-level
artifact caches; only the flow node / fact identity uses `flow_body_stable_hash`.
`FlowSliceLoweredBodyNode` lowers only the slice plan into `FlowSliceIR`. `FlowSliceIR`
carries `FlowStmt`, `FlowExpr`, `FlowSlotId`, `FlowPath`, `FlowFrame`, `NarrowingFact`,
`AliasCorrelation`, `FlowEffect`, `ReturnAccumulator`, `LoopSummary`.

**Acceptance example (non-materialization).**

```ts
function myType() { const a = new Mytype(); const b = 1; return { a, b } }
type Foo = ReturnType<typeof myType>['b']
```

Resolution must be: `IndexedAccess` threads demand `['b']` into `ReturnType`;
`ReturnType` produces/uses a lazy flow-return root; `ProjectPath` calls `FlowReturn`
with path `['b']`; the demand planner computes the slice as graph reachability from
`(return_site, ['b'], EvalPolicy)` over `myType`'s `FunctionFlowGraph`, reaching only
the `b` value-def edge and `const b = 1`; it does not lower `a`, does not resolve
`new Mytype()`, does not load `Mytype`, and does not walk sibling members (no
value-provider edge into `a` is reachable, and `a` carries no eval-effect edge into
`b`). The returned literal `1` widens to `number` at return-position. A guard asserts
no `ResolveClassSurface`, `TypeOf`, constructor, import, or route fact for `Mytype`
appears.

**Mutual recursion + flow cycle space.** Flow is mutually recursive with type
reduction: `ReturnType` calls `FlowReturn`; flow narrowing calls `Relate`; call
solving routes through `ResolveCall` / `ResolveOverloadSet` and `Relate`; return
member projection calls `ProjectPath`/`ProjectMember`; those may re-enter `FlowReturn`.
The `FunctionFlowGraph` and the cross-query cycle space are **distinct structures**:
the `FunctionFlowGraph` is the per-function intra-function dependence structure the
demand planner slices (it never spans functions or queries), while the
`CheckerReentryGraph` (§4.2) is the cross-query obligation stack shared across
`ResolveCall` / `FlowReturn` / `ContextualTypeAt` / `FlowNarrowingAt`. Flow's cycle-id
space is the flow-typed VIEW of that one shared `CheckerReentryGraph`, not a private
space. Re-entry is keyed on the FULL normalized
`FlowReturnContext + ReturnProjectionDemand + FlowInputContext`, not a narrow tuple —
the narrow `(function_slot, substitution_env_hash, projection_path, terminal_mode,
flow_policy)` form can terminate but can also mask a real result with a sentinel under
a different demand. Same-context recursion records the in-flight re-entry assumption on
the shared stack (a stable flow cycle sentinel); it never self-awaits. Guards:
**`flow_cycle_sentinel_is_never_admitted_as_cache_entry`** (the sentinel is
`ReturnOnly`) and **`flow_cycle_sentinel_does_not_hide_real_base_return_contributor`**
(a sentinel for one normalized context/demand/input is never served to a re-entry
under a different one).

**Demand-aware cache identity.** `FlowReturnContext` includes the five env hashes,
substitution canonical hash, `ProjectionReductionContext`, and `FlowPolicy`. It does
NOT carry `ReturnProjectionDemand` or `FlowInputContext` — those are the sibling
`demand` / `input` key fields of the canonical struct, so the full cache identity is
`FlowReturnContext + ReturnProjectionDemand + FlowInputContext` with no field
duplicated. `ReturnProjectionDemand` is the flow-typed `(ProjectionDemand, EvalPolicy)`
point for the return surface (§2.10); the cached flow result carries
`satisfied_projection` as the point it actually materialised, and warm-hit / backfill
is decided by the demand-lattice dominance relation (§2.10), NOT by mode-enum order:
`FlowReturn(path=['b'], Expanded)` cannot satisfy a whole return or `['a']` (neither
dominates it); a broader result backfills a narrower entry only for the narrower points
it actually materialised; a `Skeleton` (`TypeParamShells` + carrier-stop) slice is
**incomparable** to a bound-expansion slice and never satisfies it. The flow fact
signature includes
`FlowSlice { function_slot, projection_path, slice_hash, selected_binding_ids,
selected_effect_ids, selected_control_region_ids, closure_summary_ids }`, plus
`MemberPresence`, `Member`, `RouteGeneration`, `ExportSurface`, `ModuleAugmentation`,
`AmbientGlobal`, `LibIntrinsic`, `TypeEnvOptions`, and project-generation facts as
read. The extra `FlowSlice` fields beyond `selected_binding_ids` are required because
effect-only changes (an earlier sibling's assignment, an assertion call, a closure
write summary, a control-flow region) must invalidate a cached slice even when no
selected binding's identity changed. The `FlowSlice` fact lives in the new
**`FactDomain::ProgramAnalysis`** domain (the fourth closed `FactDomain` —
`docs/arch/fact-based-cache.md`), validated on every warm hit by
`StoreView::validates_program_analysis_domain`, which re-derives the live region's
`flow_body_stable_hash` + the recorded slice semantic hash and **FAILS CLOSED** on a
missing / overflowed / stale / unrooted fact. Budget, overflow, cycle, cancellation,
or partial slice results are `ReturnOnly`.

**Shallow-by-default (graph-reachability slice).** The shallow-by-default target is
valid only because the slice is graph reachability over the `FunctionFlowGraph`, not a
whole-body lowering. Path laziness must hold for cross-file return types (`ReturnType`
creates a lazy flow-return root; projected paths call `FlowReturn(path)`), imported
class methods (`ResolveClassSurface` accepts member demand and resolves only that
method/signature), nested `ReturnType<typeof f>["x"]["y"]` (intermediate hops run the
`Navigate` preset, the terminal hop runs the caller's `(ProjectionDemand, EvalPolicy)`
point — §2.10), spread/`Object.assign` (right-to-left scan; unknown spread contributes
only `ProjectPath(spread, P)`), and generic returns (`FlowReturn` key includes the
normalized substitution hash; open generics keep conditional/path shells instead of
whole-body lowering).

`Skeleton` is not a special semantic mode: it is the `generic_open = TypeParamShells`
+ carrier-stop preset over the demand lattice (§2.10), used by
`Instantiate { base, args: [], context: InstantiateContext { projection_reduction, resolve_env_hash } }`
with `context.projection_reduction.mode = Skeleton` — unbound type parameters become
`TypeParam` shells so Conditional branches do not collapse to `never` for unbound
generics.

---

## 6. Performance budgets / non-admission

The demand-sliced shape is only safe with explicit typed budgets. Every budget
returns a typed `BudgetExceeded` non-admission. A budget-exceeded result is
`ReturnOnly`: never warm-admitted, never backfilled, never published as a partial/torn
cache entry.

- **`RelationBudget`** — relation over large unions. Uses a pair memo keyed by the
  FULL `Relate` identity, not the bare `(source, target)` pair, so it cannot false-hit
  across relation-kind / policy / freshness / inference-context / env differences.
  There is NO union-arg call distribution. On exhaustion, `BudgetExceeded`.
- **`KeyspaceBudget`** — template/mapped explosion. Reverse-demand matching runs BEFORE
  enumeration (match the demanded keys back into the pattern rather than enumerating the
  full keyspace); the cartesian products for template/mapped reduction are capped. On
  overflow, `BudgetExceeded`.
- **Apparent types** — a lib member index keyed by `lib_env_hash`. Member-demand is
  REQUIRED on the hot path; there is no whole-lib materialization. A hot-path lookup
  that would force whole-lib materialization is a budget/non-admission failure, not a
  fallback.
- **`CallResolutionBudget`** — bounds overload candidates, inference bindings, and
  contextual passes. On exhaustion, `BudgetExceeded`.
- **`FlowSliceBudget`** — bounds return sites, selected statements, and effect+closure
  summaries. On exhaustion, `BudgetExceeded`.
- **Recursion-storm controls** — the separate flow cycle key (keyed on the FULL
  normalized identity, not the narrow tuple) plus prefix-interned projection paths and
  bounded per-function / per-substitution candidate retention so a recursion storm
  cannot grow unbounded candidate sets.

The three-layer non-admission rule (a `BudgetExceeded` admits NO semantic result, NO
artifact/intermediate, NO fact signature/backfill, and NO degraded exact-cache entry)
applies identically to every hot reducer, each with its own named guard:
**`relation_budget_exceeded_admits_nothing`**, **`keyspace_budget_exceeded_admits_nothing`**,
**`call_resolution_budget_exceeded_admits_nothing`**, and
**`apparent_type_budget_exceeded_admits_nothing`** (`FlowSliceBudget`'s equivalent is the
existing FlowReturn three-layer rule plus the `ReturnOnly` flow-result rule).

### 6.1 Multi-candidate cache substrate — per-family caps + env/fact dimensions

The query results these budgets gate are stored in the multi-candidate `FamilySlots`
substrate. Its candidate-retention policy and cache-key dimension set are owned by the
fact-based cache architecture (`docs/arch/fact-based-cache.md` → "Multi-candidate
`FamilySlots`" + the `IdeProjectConfig` 5-way env-hash audit; landed at
`U3.CACHE_FACT_MODEL`, with `TypeInfoGraphResultDb` candidate storage at
`U10.RESULT_DB`). The two contracts that matter for this engine:

- **Per-family adaptive caps, NOT a uniform cap-4 FIFO.** Each query family declares
  its own `candidate_cap()`; the inference/substitution-heavy families — `Relate`,
  `ResolveCall`, `Instantiate`, `Conditional`, `MappedType`, `FlowReturn` — get higher
  adaptive caps (their identities legitimately coexist across many live substitution /
  inference-context / env variants), content-light families keep small caps. Slot-cap
  eviction is **invalid-first, then least-recently valid-hit (LRU-by-valid-hit)**, not
  FIFO; the whole substrate is bounded by a **global memory ceiling**; each family's
  cold-recompute rate is held to a **benchmarked fallback-count bound** regression-gated
  through `BenchResultRow`. Per-candidate validity stays the
  `ReadSetSignature.validate_with_self_roots` rail. Pinned by
  **`cache_candidate_cap_is_per_family_not_uniform`** +
  **`family_eviction_prefers_invalid_then_lru_valid_hit`**.
- **The cache keys cover every meaning-affecting env/fact dimension, split per R21.**
  Beyond the five base env hashes, the split env hashes fold in the TS semantic
  version, JSX mode / import-source / factory, `moduleResolution`, package export/import
  conditions, `types`/`typeRoots`, the lib set, decorator + class-field semantics,
  `useDefineForClassFields`, `customConditions` / `moduleSuffixes`, and the
  `InstantiationDepthPolicy` (recursive-conditional/mapped depth beyond the budgets) —
  each entering ONLY the layers whose value depends on it, never a bundled
  `project_config_hash` (R21). These are split-env-hash / fact additions, never fields
  on a query-identity key (R6). Overlay/session identity is **session-cache identity
  only**: a persistent/base cache never admits an overlay-only result. Pinned by
  **`cache_keys_cover_ts_jsx_moduleresolution_decorator_lib_dimensions`**,
  **`instantiation_depth_policy_in_identity_and_facts`**, and
  **`persistent_caches_never_admit_overlay_only_results`**.

### 6.2 Performance contract

The §6 budgets keep each cold compute bounded; this contract states the COST SHAPE the
engine is held to — per query family, as a hit-rate target, as a memory budget, and as
an invalidation blast radius — and makes that contract regression-gated, not aspirational.
The architecture already has the right perf instincts (one resolver, parse/shallow once,
demand slicing, fact-validated lazy invalidation, typed non-admission); this section turns
those instincts into a benched, guarded contract. It REFERENCES the existing structures
(the `FunctionFlowGraph` §5, the `(ProjectionDemand, EvalPolicy)` demand lattice §2.10,
the per-family multi-candidate caps §6.1, the four `FactDomain`s incl. `ProgramAnalysis`)
— it does not redesign them.

**Governing rule — optimize so the fallback path is RARELY ENTERED, not for a fast
fallback.** The engine's performance win comes from AVOIDING work, not from a cheap
fallback. The whole architecture is built to make the slow path (cold recompute, full
relation, whole-surface materialization, whole-body flow) the EXCEPTION: one resolver
(no second engine to diverge or re-walk), parse + shallow-index once per content hash,
demand-sliced flow + projection (never whole-body / whole-surface), fact-validated lazy
invalidation (only what actually changed recomputes), and typed non-admission (a partial
/ budget-exceeded / cancelled result is `ReturnOnly`, so a cold miss never poisons the
warm path into re-missing). A "fallback" here means any cold recompute or
budget-degraded path; the design target is to drive its ENTRY RATE down via warm-hit
rate + minimal cache axes + cheap negative paths, NOT to make the fallback body fast.
A change that speeds the fallback while leaving its entry rate unchanged does not
satisfy this contract; a change that lowers the entry rate does. Pinned as a design
rule by **`architecture_minimizes_fallback_entry_not_fallback_cost`** (a bench asserts
the tracked metric is the family's fallback ENTRY count against its bound, and that the
warm path is O(validate) — see below — so the optimized-for quantity is fallback rate,
not fallback latency).

**Cold vs warm cost per query family.** For every family the WARM path is the same
shape — peek the multi-candidate slot, validate the candidate's `ReadSetSignature.facts`
against the caller's live `StoreView` (and self-roots), return the `Arc`. A warm hit is
therefore **O(validate)** — proportional to the recorded fact-set size, NOT to the cost
of recomputing the result — and allocates no audit payload without an active accumulator.
The COLD shape is per family:

| Family | COLD compute shape | WARM path (always O(validate) + return `Arc`) |
|---|---|---|
| `FlowReturn` | build/reuse the `FunctionFlowGraph` (once per function, shallow §5), plan the demand slice as graph reachability from `(origin, projection_path, EvalPolicy)`, lower ONLY the slice into `FlowSliceIR`, evaluate it | validate `FlowSlice` (+ `Member`/`Route`/…) facts in `FactDomain::ProgramAnalysis`; return `Arc<FlowReturnResult>` |
| `ResolveCall` | select the applicable overload, run the `InferenceSession` fixed-point (candidate accumulation + fixation), relate args via `Relate`, materialize the call result | validate the recorded facts; return `Arc<ResolvedCallResult>` |
| `Relate` | fast-reject discriminators first (§4.1 perf hardening); survivors open the coinductive-SCC scope, relate members / instantiated bodies / constraints, discharge the SCC | validate the recorded facts; return the `RelationPayload` (incl. proof) |
| `Instantiate` | substitute type args into the base body under the demand point; reduce per `EvalPolicy` (bound vs `TypeParamShells`) | validate; return the instantiated `TypeNode` `Arc` |
| `Conditional` | relate the check type to the extends type (open conditionals distribute the path into both branches §2.10); reduce closed conditionals immediately | validate; return the reduced branch / distributed `TypeNode` |
| `MappedType` | reverse-demand match the demanded keys BEFORE keyspace enumeration (§6 `KeyspaceBudget`); reduce only the demanded members with their optionality modifiers | validate; return the mapped surface `TypeNode` |
| `ResolveClassSurface` | substitute heritage, resolve ONLY the demanded static/instance member or signature (member-demand on the hot path), carry nominal brands | validate; return the demanded class-surface projection `Arc` |
| `ApparentType` | look the apparent member up in the `lib_env_hash`-keyed member index under member-demand (NO whole-lib materialization) | validate (`LibIntrinsic` + lib_env); return the apparent member `TypeNode` |
| `TemplateLiteralReduce` | reverse-demand match the pattern against the demanded keys; reduce template segments under TS lexical numeric/bigint semantics (§4.3) | validate; return the reduced literal-union `TypeNode` |
| projection / demand-lattice families (`ProjectPath` / `ProjectMember` / `KeyOf` / `IndexedAccess` / `NormalizeUnion` / `NormalizeIntersection`) | run the path-precise projection: intermediate hops in `Navigate`, the terminal hop in the caller's `(ProjectionDemand, EvalPolicy)` point; materialize ONLY the terminal demanded projection | validate; return the projected `TypeNode` `Arc` (broader results backfill narrower points by the lattice meet they covered §2.10) |

The cold shapes share one discipline: **demand-scoped, never whole-object.** No family
materializes a whole surface / whole body / whole keyspace when the demand is a member /
path / branch — that is the §2.10 + §5 + §6 shallow-by-default rule restated as a cost
contract. A cold path that would force whole-lib / whole-keyspace / whole-body
materialization on the hot path is a budget non-admission (§6), not a slow success.

**Cache hit-rate targets + benched fallback-count bound per family.** Each family
declares a hit-rate target and, the regression-gated form, a **bounded fallback (cold
recompute) count** on the benchmark corpus. The fallback count is the tracked metric (a
warm-hit-rate target is reported, but the GATE is on fallback count because it is exactly
the "fallback rarely entered" quantity the governing rule optimizes). This rides the
existing `BenchResultRow`, which already reports hit count + fallback count per run: a
bench regression FAILS when a family's fallback count exceeds its bound on the corpus.
The inference/substitution-heavy families (`Relate`, `ResolveCall`, `Instantiate`,
`Conditional`, `MappedType`, `FlowReturn`) carry the higher adaptive candidate caps that
keep their legitimately-coexisting variants resident (§6.1), so their steady-state
fallback count stays under bound even across many live substitution / inference-context
/ env variants; content-light families hold a tight bound under small caps. The bound is
per family, on the corpus, regression-gated — not a global average that can hide one
family thrashing.

**Memory budgets + compaction / sweep.** The demand-sliced shape keeps the working set
small, but the retained structures still need explicit budgets + an aggressive
compaction/sweep policy under the global memory ceiling (§6.1). Each structure carries
a budget and a sweep trigger:

| Retained structure | Budget | Compaction / sweep |
|---|---|---|
| `SemanticGraphStore` multi-candidate slots | per-family candidate cap (§6.1) + the process-wide global memory ceiling | invalid-first, then LRU-by-valid-hit eviction; at the ceiling, cross-slot invalid-first/LRU eviction; an un-admittable candidate routes through `ReturnOnly` (never published) |
| per-function `FunctionFlowGraph` | bounded resident graph set, keyed by `flow_body_stable_hash` | drop the graph for a superseded `flow_body_stable_hash` (a body edit) and for cold functions under the ceiling; oversized functions retain only materialized regions (§5 lazy regions) |
| flow slices (`FlowSliceIR`) | `FlowSliceBudget` (§6) | slices are demand-scoped and not retained beyond their cached `FlowReturn` result; a budget-exceeded slice is `ReturnOnly` |
| relation proofs (`CoinductiveCycle` etc.) | bounded with the `RelationPayload` they annotate | dropped with their owning relation candidate on eviction; never retained independently |
| query candidates | per-family cap (§6.1) | invalid-first / LRU-by-valid-hit, as above |
| audit records | the `verter_audit` accumulator bound; opt-in (`audit_enabled` + `footprint_capture`) | swept per the audit runtime's retention; a warm hit allocates NO audit payload without an active accumulator |
| `ProgramAnalysis` facts | the `FactDomain::ProgramAnalysis` fact-set per cached slice | superseded/stale facts fall out with their cached entry on validation failure; the domain FAILS CLOSED on a missing/overflowed/stale/unrooted fact |

Eviction never produces a warm cache entry, never backfills, and never publishes a torn
result — the typed non-admission rule (§6) governs every drop. Compaction prefers
dropping INVALID and SUPERSEDED structures first (they cannot serve a warm hit anyway),
then cold-but-valid by LRU-by-valid-hit.

**Invalidation scenarios + expected re-compute blast radius.** Invalidation is lazy and
fact-driven (the §6.1 / fact-cache rails), so the blast radius is exactly the recorded
read-set, not a broad sweep:

- **Same-file BODY edit** (`return { b: 1 }` → `return { b: 2 }`, an assertion call
  added, a branch reordered): the edited function's `flow_body_stable_hash` changes, so
  ONLY the `flow_body_stable_hash`-keyed `FunctionFlowGraph` + the `ProgramAnalysis`
  `FlowSlice` facts that read it recompute, plus the dependent facts that recorded them.
  The decl-skeleton artifacts (`IndexedReady`, `parse_stable_hash`-keyed decl-level
  caches, the file's export surface when the signature is unchanged) **survive** — a body
  edit is body-sensitive / cosmetic-insensitive and does NOT invalidate decl-level
  artifacts. Blast radius: the edited function's flow + its direct fact-readers.
- **Cross-file DECL edit** (a changed/removed exported type a dependent resolved): lazy
  fact-driven invalidation — only the dependents whose recorded `ReadSetSignature.facts`
  actually observed the changed declaration fail validation on their next warm hit and
  recompute; dependents that never read it keep their warm entries. The reverse dep
  graph is NOT the invalidation authority (it is not consulted as a truth source); the
  per-candidate fact validation against the live `StoreView` is. Blast radius: the
  transitive set that recorded a fact on the changed decl, discovered lazily at read
  time — not the whole importer closure.
- **Env-hash change** (a `tsconfig` / lib / JSX / decorator / `moduleResolution` option
  changes): only the cache layers KEYED on the affected split env dimension invalidate
  (R21 — each dimension enters only the layers whose value depends on it). A
  `type_env_hash`-only change (e.g. `strict`) does not invalidate a `ResolvedImportFacts`
  entry that excludes `lib_env_hash`; a `lib_env_hash` change invalidates the lib-reading
  layers (`ApparentType`, typed-IR resolve, `SemanticGraphStore` query nodes) but not the
  layers that never fold it in. Blast radius: exactly the layers whose key includes the
  changed dimension.

**Verter-vs-TS/tsgo benchmark fixtures.** Performance parity is demonstrated, not
asserted: a benchmark suite runs Verter and TS/tsgo over the SAME semantic queries and
reports both. The fixtures cover the consumer-real query shapes — component-meta
resolution, projected typeinfo, IDE hover / completion queries, selected member
expansion (`Pick` / `Omit` / a single demanded member off a large surface), and the
demand-slice case `ReturnType<typeof f>["b"]` (the §5 non-materialization example: only
the `b` slice is computed, `a` / `new Mytype()` are never resolved). Every run is
reported with the existing benchmark-reporting contract — cache mode, source-map policy,
batch shape, thread count, hit count, and fallback count (the `BenchResultRow` schema) —
so a Verter-vs-tsgo comparison is apples-to-apples on cache mode + batch + threads, and a
fallback-count regression is visible per family. These benches are part of TERMINAL
ACCEPTANCE (perf-regression-gated), not merely the functional gate — see the U15 bench
deliverable in `docs/arch/native-typeinfo-parity-adapters-final-lift.md`.

**Perf-hardening guards (baked into the engine sections, indexed here).** The four
perf-hardening properties are pinned by named guards introduced in their owning sections
and collected in the guards index (§11.8 / Guards index → "Performance contract"):
**`flow_graph_build_is_shallow_interned_no_lowering_lazy_regions`** (§5 — shallow
interned build, no type lowering, lazy region materialization),
**`cache_key_axes_are_minimal_and_normalized`** (§2.10 — every key axis benchmark-proven
minimal + normalized), **`relation_negative_and_unknown_paths_are_fast`** (§4.1 — fast
negative/unknown reject + memo locality), and
**`architecture_minimizes_fallback_entry_not_fallback_cost`** (this section — the
governing rule: the tracked/optimized metric is fallback ENTRY rate, with the warm path
held O(validate)). Typed non-admission (§6) already prevents warm-cache poisoning — a
budget-exceeded / cancelled / partial result is `ReturnOnly`, never warm-admitted — so
the fallback path, when it IS entered, cannot corrupt the warm path that keeps it rare;
that rule is cross-referenced here, unchanged.

### 6.3 Differential `tsgo`-parity oracle (the SEMANTIC-correctness gate)

The §6.2 benches gate the COST shape (fallback-count / cache-mode / warm-hit). They do
**not** gate semantic agreement with TypeScript. **Semantic tsc-parity is gated by the
differential `tsgo`-parity oracle** — distinct from, and additive to, the 362-row
coverage ledger (PART 2 §10) and the §6.2 performance benches.

**What it is.** A harness that runs a corpus through **Verter AND the pinned `tsgo`**,
diffs the **STRUCTURED** results (not display strings), and gates on a **per-family
divergence budget**:

- **Corpus = a TS-conformance slice + property-generated type fixtures.** The
  conformance slice exercises the published TS behaviors per family; the
  property-generated fixtures stress the reducers with mechanically-generated types
  (unions, intersections, conditionals, mapped/template constructs, generic
  instantiations) so parity is tested beyond the hand-written cases.
- **Structured diff.** Compare the projected `TypeInfoGraphPayload` / `RelationPayload`
  / `TypeDescriptor` structurally against `tsgo`'s answer for the same query — NOT a
  text compare of display output (display divergence is allowed; SEMANTIC divergence is
  the gate).
- **Per-family divergence budget.** Each reducer family declares a parity-coverage
  target shape — **"N conformance cases per family; divergence budget M per family"** —
  and the oracle FAILS that family when its structured divergence count exceeds M. The
  budget is per family (so one family cannot hide behind another's agreement), against
  the pinned `tsgo`.

**Why it replaces the 362 proxy as the semantic gate.** A green 362 ledger proves
**coverage** (every row owned + executably proven + wired — "detects un-wired"). The
oracle proves **semantic completeness** (the engine computes what `tsc`/`tsgo`
computes — "detects **wrong**"). Stating `362-green` as tsc-parity is the proxy error
the oracle closes: the two are distinct claims. The behavioral guards stay (they detect
un-wired); the oracle is the net-new semantic gate on top.

**Produced at each hard phase's rescope gate.** The oracle baseline for a family is a
**rescope-gate deliverable** (sequencing authority §3.2(b)): when a
RESCOPE-GATE-REQUIRED phase (`U2.RELATION_INFER`, U6 cross-engine, the native checker,
…) is designed at its rescope session, the session names that phase's families' N / M
and the oracle gates the phase's acceptance. So the oracle grows phase-by-phase
alongside the algorithm-depth design, rather than landing as one monolith.

**Distinct from the §6.2 / U15 perf benches.** The §6.2 Verter-vs-`tsgo` fixtures run
the same pinned `tsgo` but gate COST (fallback count / cache mode); the §6.3 oracle
gates SEMANTIC agreement (per-family divergence). Both ride the pinned `tsgo`; they are
separate gates. Terminal acceptance (PART 2 §12) requires both green, in addition to the
362 lift.

---

## 7. Class / relation / call parity matrix

Class private/protected, overload, and overload×generic interaction are first-class:

- Class surfaces carry nominal private/protected brand identities separate from
  published members. `#private` is absent from public projection but present in relation
  identity.
- Class surfaces carry abstract metadata (`ClassSurface.is_abstract` + per-member
  abstract flags); abstract construct signatures use `SignatureKind::AbstractConstruct`
  (§1.6).
- Class surfaces carry decorator / auto-accessor members per §1.7: an `accessor` is a
  declared property whose visibility follows its modifiers — only PUBLIC auto-accessors
  publish public properties; decorated method return types preserved; identity-compatible
  decorator effects validated through `ResolveCall`/`Relate` without rewriting the surface.
- Relation requires same-origin private/protected declarations for structural
  compatibility; subclass access rules are separate from assignability.
- Overload sets are ordered signatures. Call expressions use the first applicable
  declared overload; `ReturnType<typeof overloaded>` and `ConstructorParameters` use the
  last visible overload signature, not the implementation body.

Matrix rows: same-origin vs different-origin private/protected; inherited private brand;
protected subclass method flow; static/instance brand separation; generic class
instantiations; first-applicable overload; last-overload `ReturnType`; contextual
callback overloads; union-arg no-distribution; `NoInfer`; const type params; `this`
params; constructor overloads; abstract-base inheritance; `InstanceType<abstract new ...>`;
constructor-utility behavior on abstract; rejecting concrete `new Abstract`; and the four
decorator/accessor rows (§1.7).

---

## 8. JSX resolution (no new query keys)

JSX parity (the `JsxResolution` substrate, 9 rows) resolves through the **existing**
query surface — no dedicated JSX query keys and no dedicated JSX `GraphTypeNode`,
keeping the "exactly five added query keys" rule intact. There is no JSX-specific
resolution engine.

- **`JSX.IntrinsicElements` / `JSX.Element` / `JSX.ElementClass` / the `JSX` namespace**
  resolve through `ResolveAmbientNamespace` + module augmentation + merged declarations.
- **Intrinsic element attribute types** project through `IndexedAccess` / `KeyOf` over
  the resolved `JSX.IntrinsicElements` object surface (the props of `"div"` are
  `JSX.IntrinsicElements["div"]`). No `ResolveJsxIntrinsicElement` key and no
  `TypeNode::JsxIntrinsicElement` value.
- **JSX attribute types** are ordinary member/indexed-access projections over the
  resolved element props surface; no `ResolveJsxAttribute` key.
- **Component element types** (function and class) resolve through the normal class /
  function surfaces — `ResolveClassSurface` for class components, ordinary signature /
  return surfaces for function components.
- **`jsxImportSource` module/namespace resolution** is ordinary module + namespace
  resolution (import graph + `ResolveAmbientNamespace` + merged declarations) that selects
  the factory's declaring module/namespace — it dispatches no JSX-specific key. The actual
  `jsx` / `jsxs` / `createElement` factory INVOCATION (a normal call with its own cache
  identity) is resolved by `ResolveCall`, which is owned by `U6.CALL_RESOLVE`; factory-call
  dispatch is therefore a `U6.CALL_RESOLVE` backfill, NOT a `U2.JSX_FOUNDATIONS`-owned row
  (the JSX foundations block precedes U6 and must not depend on `ResolveCall`).

Guards: **`jsx_resolution_uses_existing_semantic_queries`**,
**`jsx_intrinsic_elements_project_via_indexed_access`**, **`jsx_no_dedicated_graph_type_node`**.

**No-new-key completeness submatrix** (full TS-checker parity). The five mechanisms above
cover a subset of the TS JSX checker; full parity resolves the remaining rules still
through the existing query surface. Each is an oracle/guard row in the `JsxResolution`
substrate:

- **`JSX.LibraryManagedAttributes<C, P>`** — `ResolveAmbientNamespace` for the member +
  `Instantiate` / `IndexedAccess` to apply it to `(C, P)`. Guard:
  **`jsx_library_managed_attributes_via_ambient_namespace_and_indexed_access`**.
- **`JSX.ElementAttributesProperty`** — `ResolveAmbientNamespace` + `KeyOf` (its single
  key) + `IndexedAccess`. Guard:
  **`jsx_element_attributes_property_via_ambient_namespace_keyof`**.
- **`JSX.ElementChildrenAttribute`** — `ResolveAmbientNamespace` + `KeyOf`; the children
  prop type is an ordinary `IndexedAccess` projection. Guard:
  **`jsx_element_children_attribute_via_ambient_namespace_keyof`**.
- **`JSX.IntrinsicAttributes` / `JSX.IntrinsicClassAttributes<C>`** —
  `ResolveAmbientNamespace` (+ `Instantiate` for the class form) intersected onto the
  element attribute surface via `NormalizeIntersection`. Guard:
  **`jsx_intrinsic_attributes_via_ambient_namespace_intersection`**.
- **Class-component `JSX.ElementClass` check** — `ResolveClassSurface` (instance surface)
  + `ResolveAmbientNamespace` (`JSX.ElementClass`) related through `Relate`. Guard:
  **`jsx_element_class_check_via_resolve_class_surface_and_relate`**.
- **`jsxImportSource` module-namespace resolution** — ordinary module/namespace resolution
  ONLY (import graph + `ResolveAmbientNamespace` + merged declarations) to select the
  factory's declaring module/namespace. This U2 additional row does NOT cover the factory
  INVOCATION: the `jsx` / `jsxs` / `createElement` call itself dispatches `ResolveCall`
  (owned by `U6.CALL_RESOLVE`) and is validated as a `U6.CALL_RESOLVE` backfill, since
  `U2.JSX_FOUNDATIONS` precedes U6 and cannot consume `ResolveCall`. Guard:
  **`jsx_import_source_module_namespace_via_existing_resolution`**.

These six rows add no query keys and no dedicated JSX `GraphTypeNode`. They land as
`AdditionalProofRow`s (the coverage-only table — they grow JSX coverage beyond the
original 362 and carry their own coverage-table rows + `ProofRequirement`s, but are
excluded from the ignored-count + bijection guards, so the binding 362 `IgnoredTestRow`
total is unchanged). Where a submatrix fixture corresponds to an existing ignored
`JsxResolution` row it stays in that `IgnoredTestRow` rather than being duplicated.

---

# PART 2 — The Execution Framework

This part owns the **cross-cutting** execution framework: the per-block contract
template, the two-table manifest ledger, the git/CI landing protocol, the no-skip
guarantee, and the resume protocol. The per-U-block block instances live in the
subplans; this is the shared machinery they all use.

**Two DISTINCT gates (do not conflate).** (1) The **per-big-phase RESCOPE GATE**
(sequencing authority §3.2) runs **before** a RESCOPE-GATE-REQUIRED phase
(`U2.RELATION_INFER`, U6 cross-engine, the native checker, U7) implements: a planner +
1-Claude+2-codex panel iterate to best-architecture and produce the phase's
algorithm-depth design + termination proof + the §6.3 oracle baseline + the
fail-today guards. (2) The **per-block three-reviewer LAND panel** (§11.12) authorizes
the squash-merge of each FINISHED block. The rescope gate is pre-implementation
design; the LAND panel is the merge gate. Both are **additive to** the git/CI landing
model below, and both are distinct from §14.1's codex-architect fork gate (an
UNFORESEEN fork resolved DURING a block).

There is no new top-level phase ladder — subplans, not stages. A block cannot start
until all prerequisite block IDs are done, and "done" is derived mechanically from the
manifest, the merged git history, and the guard suite, not from prose. The landing
boundary is git/CI itself: **git is the transaction log, branch protection is the
accept gate, and `git revert` is rollback.** There is no tracked orchestration cursor,
no write-ahead log, no lease, no revision CAS, and no persisted gate/review receipt —
those distributed-database mechanics were redundant on top of what git and CI already
provide, and a tracked cursor (a versioned file the landing protocol itself rewrites)
created cryptographic-fixed-point, paired-hash, and crash-recovery problems that git
does not have.

## 9. The per-block contract template

Every block uses this template:

```md
ID:
Parent U-block:
Subplan:
Prerequisites:
Blocked until:
Context: why this block exists — the gap/behavior it closes and why it is being done now
Changes: exact files / functions / code paths to modify or add for this block (no globs)
Deliverables:
Legacy deletions: exact old code paths / guards / stale docs / feature flags /
                  projection-repair-or-second-engine paths removed by this block (no globs);
                  empty only if the block genuinely deletes nothing
SemanticQueryKey/facts touched:
Exact test rows lifted: file::function list from manifest, no globs
Required new guards:
Critical-rule guards: for any new (CRITICAL) rule this block introduces, the named
                      architecture guard(s) registered for it (R6); empty only if the
                      block introduces no new (CRITICAL) rule
Proof requirement: per-row ProofRequirement coverage (oracle / negative guard /
                   structural guard / both / row test)
Exit acceptance:
Verification commands:
Commit cadence:
Review gate:
Docs updated:
Re-entry notes:
```

The template carries the full `CLAUDE.md` planning quartet and the R6 guard rule, so
every block states why it exists, the exact surface it modifies, what it deletes, and
how it is verified:

- **`Context:`** states the gap or behavior the block closes and why now.
- **`Changes:`** names the exact files / functions / code paths the block modifies or
  adds (no globs) — so a block's concrete modification surface is explicit, not inferred
  from the lifted rows.
- **`Legacy deletions:`** is mandatory: each block names the exact superseded code paths,
  guards, stale doc lines, feature flags, and any projection-repair / second-engine paths
  it removes — so no block silently leaves a dual path alive. A block that genuinely
  deletes nothing states that explicitly.
- **`Critical-rule guards:`** is mandatory per R6: if the block introduces any new
  `(CRITICAL)` rule, it registers the named guard(s) for that rule; a block introducing
  no new `(CRITICAL)` rule states that explicitly.
- **`Commit cadence:`** is the git-history discipline (§11.11): a WIP series on the
  block's branch during the block (high WIP count expected; **no per-commit gate** — the
  WIP exemption applies, so intermediate `todo!()` / placeholder states are permitted),
  squash-merged to EXACTLY ONE land commit on the target branch when the branch merges.
  The target branch receives one commit per landed block.
- **`Review gate:`** is the LAND authorization (§11.12): the three-reviewer panel — 1
  Claude Code + 2 codex, all bad-mood, holding the best-architecture-no-compromises /
  breaking-changes-allowed mandate — must all return LAND (or residuals are NITs-only)
  before the branch merges. This is a branch-protection required-approval rule, enforced
  by the merge gate, not a done-predicate part.

`Commit cadence:` and `Review gate:` are **PARENT-UNIFORM**: their value is IDENTICAL
for every block — the §11.11 one-squashed-land-commit discipline and the §11.12
three-reviewer LAND gate — so they are OWNED at the parent (§11.11 + §11.12) and a
block contract need NOT restate them per block; a subplan states them ONCE as the
uniform discipline for every block in it. (Contrast the block-SPECIFIC quartet
`Context:` / `Changes:` / `Legacy deletions:` / `Critical-rule guards:`, which carry
per-block data and so ARE restated in every block contract.)

`TYPEINFO_PARITY_BLOCKS: &[BlockContractRow]` lives in the same manifest module
(`crates/verter_session/tests/typeinfo_ignored_test_manifest.rs`). Each `BlockContractRow`
carries `block_id`, `owning_u_block`, `organ`, `prereqs`, `mechanism_id`,
`consumed_mechanisms`, `required_guards`, and `verification_labels` (the verification
command labels). There is no `subplan path` field.

## 10. The two-table manifest ledger

The manifest is extended **in place** (in
`crates/verter_session/tests/typeinfo_ignored_test_manifest.rs`); no second ledger file
is created. Both tables live in this one module.

> **State note:** the two-table schema below is BUILT. The live manifest carries the
> full 13-column `IgnoredTestRow` (file/function/substrate/capability/organ/owning_u_block/
> block_id/semantic_queries/proof/status/mechanism_id/consumed_mechanisms/unblocker), the
> closed 7-row `AdditionalProofRow` table, and the `TYPEINFO_PARITY_BLOCKS` DAG (each
> `BlockContractRow` carrying `required_guards` + `verification_labels`). The §10.4.1
> row→`block_id` partition table (all 362 rows) is HAND-AUTHORED authoritative input, but its
> authority is SCOPED: it is the source ONLY for the row→`block_id` PARTITION the generator
> READS to assign each `IgnoredTestRow`'s `block_id` (joined with the live `#[ignore]`
> discovery + the Capability Map). It lives in the BEGIN/END coverage block in §10.4.1, and
> the Python manifest generator's `parse_partition` READS it (it does not emit it) to derive
> each `IgnoredTestRow`'s `block_id`. The OTHER two manifest-data files are NOT derived from
> §10.4.1: the `AdditionalProofRow` table (`typeinfo_additional_proof_rows.rs`) is built from
> the generator's own Python maps (`build_additional_rows`, e.g. `JSX_NO_NEW_KEY_ROWS`), and
> the `TYPEINFO_PARITY_BLOCKS` DAG (`typeinfo_parity_blocks.rs`) — including each block's
> `required_guards`/`verification_labels`/prereqs/mechanisms — is authored in the generator's
> own block maps (`emit_block_rows`, `BLOCK_TO_REQUIRED_GUARDS`, `BLOCK_VERIFICATION_LABELS`,
> the prereq/mechanism maps), NOT from §10.4.1. `--check` regenerates all three files
> (`typeinfo_ignored_test_manifest_rows.rs`, `typeinfo_additional_proof_rows.rs`,
> `typeinfo_parity_blocks.rs`) from these inputs and byte-compares. What remains for the oracle/proof
> gate (U0-FINISH-B, NOT yet built) is the executable proof registry, the row-test wrapper,
> the reverse `cargo run` coverage-table generator, the §10.4 row-exact
> capability→mechanism→proof coverage GATE (the registry + checking guards that DEFINE
> completeness), and the TS7 oracle harness (the `ProofRequirement::Ts7Oracle` snapshots).
> The §10.4.1 partition itself is NOT the unbuilt part.

### 10.1 Two SEPARATE tables: the binding 362 vs additional coverage

The binding manifest total is EXACTLY 362 ignored rows. Full-parity coverage adds a
CLOSED set of exactly 7 coverage-only `AdditionalProofRow`s = the 6 JSX no-new-key
submatrix rows (owned by `U2.JSX_FOUNDATIONS`) + the 1 mapped companion
`mapped_modifier_minus_optional_preserves_explicit_undefined_on_required_property`
(owned by `U2.MAPPED_TEMPLATE`). The set is closed, not growing. Those two facts are
incoherent if the binding 362 and the additional fixtures share ONE table and ONE
`EXPECTED_TOTAL_IGNORED_COUNT` — additional rows would either break the exact 362
count/bijection or be untracked. The ledger therefore SPLITS into two tables:
`IgnoredTestRow` holds EXACTLY the 362 ignored test-site rows (count-guarded at 362,
bijective with the source `#[ignore]`s), and a SEPARATE coverage-only `AdditionalProofRow`
table holds EXACTLY the 7 closed coverage rows above. `AdditionalProofRow`s are EXCLUDED
from the ignored-count + bijection guards (they are not source-`#[ignore]` test sites, so
they neither add to `EXPECTED_TOTAL_IGNORED_COUNT` nor participate in the bijection) BUT
still require a `ProofRequirement` + a coverage-table entry, and the
`additional_proof_row_table_holds_exactly_7_rows` guard pins the set at exactly 7,
disjoint from `IgnoredTestRow`. A submatrix/additional fixture that corresponds to an
existing ignored row stays in that `IgnoredTestRow` (it is not duplicated); only the 7
genuinely NEW fixtures above are `AdditionalProofRow`s.

```rust
struct IgnoredTestRow {              // EXACTLY the 362 ignored test-site rows (count-guarded, bijective with source #[ignore]s); 13 fields
    file: &'static str,
    function: &'static str,
    substrate: TargetSubstrate,
    capability: TypeInfoCapability,
    organ: ArchitectureOrgan,
    owning_u_block: UBlock,
    block_id: TypeInfoParityBlockId,
    semantic_queries: &'static [SemanticQueryName],
    proof: ProofRequirement,
    status: IgnoreStatus,
    mechanism_id: MechanismId,
    consumed_mechanisms: &'static [MechanismId],
    unblocker: &'static str,
}

struct AdditionalProofRow {          // COVERAGE-ONLY: the closed set of exactly 7 rows (6 JSX no-new-key submatrix
                                     // rows + the 1 mapped `-optional`-preserves-explicit-undefined companion).
                                     // EXCLUDED from the ignored-count + bijection guards (not a source-#[ignore] site),
                                     // but STILL requires a ProofRequirement + a coverage-table entry.
                                     // Pinned closed-at-7 by additional_proof_row_table_holds_exactly_7_rows.
    file: &'static str,
    function: &'static str,
    substrate: TargetSubstrate,
    capability: TypeInfoCapability,
    organ: ArchitectureOrgan,
    owning_u_block: UBlock,
    block_id: TypeInfoParityBlockId,
    semantic_queries: &'static [SemanticQueryName],
    proof: ProofRequirement,
    mechanism_id: MechanismId,
    consumed_mechanisms: &'static [MechanismId],
    // NO `status: IgnoreStatus`: not an ignored test site, so no lifecycle and never in
    // EXPECTED_TOTAL_IGNORED_COUNT or the bijection. 11 fields (= 13 minus `status` and `unblocker`).
}

enum ProofRequirement {
    Ts7Oracle(OracleId),
    StructuralGuard(GuardId),
    NegativeGuard(GuardId),
    OracleAndGuard { oracle: OracleId, guard: GuardId },
    RowTestGuard { file: &'static str, function: &'static str },
}

enum IgnoreStatus {
    Ignored,
    Lifted { block_id: TypeInfoParityBlockId },
}
```

`IgnoreStatus` is binary — `Ignored` or `Lifted`. There is no tracked `Verifying`
transient and no `lease_id`: the "in-flight, not-yet-landed" state of a block is simply
its UNMERGED branch (§11). A block's branch removes the block's source `#[ignore]`s, flips
its rows `Ignored → Lifted`, and decrements `EXPECTED_TOTAL_IGNORED_COUNT` in the SAME
branch — so every committed state (the branch tip and the post-squash-merge target tip) is
count-consistent, and CI runs the full workspace gate against the branch's `Lifted` state.

### 10.2 `ProofRequirement` — every row resolves to an executable proof

> **State note:** the `ProofRequirement` enum + the `proof` field on every row ARE built
> (carried on the live `IgnoredTestRow` / `AdditionalProofRow`). The proof-resolution GATE
> described in §10.2–§10.4 — the generated oracle snapshots, the generated proof registry,
> the generated row-test wrapper, and the guards `every_oracle_id_resolves_to_checked_in_snapshot`
> / `every_guard_or_row_proof_resolves_to_default_suite_test` / `lifted_row_executes_declared_proof`
> / `every_manifest_row_has_non_placeholder_mechanism_and_executable_proof` /
> `capability_rows_map_to_expected_query_fact_mechanisms` /
> `block_rows_cannot_lift_without_complete_coverage` — is the U0-FINISH-B design and is NOT
> yet built. Those guard names are forward-declared in `BlockContractRow.required_guards`;
> §10.2–§10.4 describe their intended behavior, not the current tree.

Every row in BOTH tables carries `proof: ProofRequirement`, not a mandatory per-row
TS-oracle plus a mandatory per-row negative guard. A mandatory per-row TS-oracle is
wrong: cache-invalidation, audit-footprint, demand/mode-boundary, and negative rows are
not TS-oracle rows. `ProofRequirement` lets each row declare exactly the proof it needs —
a TS7 oracle, a structural guard, a negative guard, both, or a direct row test — while
still requiring every row to resolve to an EXECUTABLE proof. There is **no** proof escape
hatch: there is NO `NotTsOracleApplicable` arm. Every manifest row (every `IgnoredTestRow`
AND every `AdditionalProofRow`) must resolve to an executable proof artifact — an
`OracleId` snapshot, a `GuardId` test, or a named `RowTestGuard { file, function }`.

`Ts7Oracle(OracleId)` rows reference GENERATED oracle snapshots only (deterministic
`OracleId`, checked-in normalized snapshots, default tests compare only to snapshots,
regeneration feature/env-gated). The manifest never embeds hand-maintained oracle
expectations.

Three guards make "every row resolves to an executable proof that the row's test actually
consumes" mechanical:

- **`every_oracle_id_resolves_to_checked_in_snapshot`** — every `OracleId` referenced by a
  `Ts7Oracle` / `OracleAndGuard` row resolves to a checked-in normalized snapshot.
- **`every_guard_or_row_proof_resolves_to_default_suite_test`** — every `GuardId` (from a
  `StructuralGuard` / `NegativeGuard` / `OracleAndGuard` row) and every `RowTestGuard`
  resolves to a test that runs in the DEFAULT suite (not feature-gated, not ignored).
- **`lifted_row_executes_declared_proof`** — every `Lifted` row's test actually CONSUMES
  its declared `ProofRequirement`, enforced STATICALLY through the generated row-test
  wrapper (below): every `Lifted` row is represented by exactly one generated wrapper
  invocation that binds the row to its declared `ProofRequirement` and executes the exact
  oracle/guard/row proof inside that row's own test, and no `Lifted` row is backed by an
  ad-hoc hand-written `#[test]` outside the wrapper. It does NOT depend on observing tokens
  recorded by other row tests at runtime: default Rust tests run independently and
  unordered, so cross-test runtime token aggregation is unsound; the wrapper makes
  consumption a static, per-row, generation-time-checked property.

### 10.3 Proof-consumption mechanism (generated wrapper, not runtime aggregation)

The consumption guard is mechanical only with a concrete artifact tying each row's own
test back to its declared `ProofRequirement`, statically checkable. The mechanism is a
GENERATED / typed row-test wrapper (a macro/codegen contract), not runtime token
aggregation:

- **Generated proof registry.** A generated registry maps each manifest row
  (`file::function` identity) and its declared `ProofRequirement` to the executable proof
  artifact that satisfies it — a checked-in oracle snapshot path for `Ts7Oracle` /
  `OracleAndGuard`, the named `GuardId` test for `StructuralGuard` / `NegativeGuard` /
  `OracleAndGuard`, or the exact `file::function` row test for `RowTestGuard`. The registry
  is generated from the manifest by a dedicated `cargo run` generator (not hand-maintained)
  and checked in; a row with no resolvable proof artifact fails generation.
- **Generated / typed row-test wrapper (macro/codegen).** Every lifted row's test is
  declared THROUGH a generated row-test wrapper that binds the row's identity to its
  declared `ProofRequirement` and, inside that row's OWN test invocation, executes exactly
  the proof the registry maps the row to. The binding row → `ProofRequirement` → executed
  proof is fixed at generation time; a row cannot be lifted with a wrapper that runs a
  different proof than it declares, and a row whose proof does not resolve fails generation.
- **Static guard verification.** `lifted_row_executes_declared_proof` STATICALLY verifies,
  against the generated wrapper set and the registry, that every `Lifted` row is represented
  by exactly one wrapper invocation bound to the registry-mapped proof artifact, and that NO
  `Lifted` row is backed by an ad-hoc `#[test]`. This distinguishes "the row's own test
  consumed its declared proof" from "the proof artifact merely exists and runs somewhere in
  the suite."

### 10.4 U0 row-exact capability→mechanism→proof coverage table (DEFINES completeness)

The proof guards prove a row's declared proof EXISTS, RUNS, and is CONSUMED — but NOT that
the row is wired to the architecture MECHANISM intended to lift it. To make 362-row
full-parity completeness MECHANICAL, the U0-FINISH-B design is a row-exact coverage table
(GENERATED FROM and CHECKED against the manifest — NOT yet built; today the built artifact is
the §10.4.1 hand-authored partition the Python generator reads) that maps EVERY manifest row
through its full mechanism chain:

```
row (file::function)
  -> capability     (TypeInfoCapability — the manifest row's capability)
  -> mechanism_id   (the concrete architecture mechanism that lifts it — a reducer / query-path /
                     fact id, e.g. ReturnPathPeeker.two_frontier, Relate.coinductive_scc,
                     IndexedAccess.union_distribution, ResolveAmbientNamespace.jsx_namespace —
                     NOT a placeholder)
  -> semantic_queries / facts (the SemanticQueryName set + fact kinds the row's mechanism dispatches/reads)
  -> ProofRequirement (the row's executable proof)
  -> block_id       (the TypeInfoParityBlockId that owns lifting the row)
```

- **Hand-authored source, checked-in.** The `row → block_id` partition is hand-authored in
  §10.4.1 and is the AUTHORITATIVE SOURCE — it is NOT generated from the manifest. The
  built generator (`scripts/gen-typeinfo-ignore-manifest.py`) READS the §10.4.1 partition
  (joined with live `#[ignore = "..."]` discovery) and EMITS the manifest rows
  (`typeinfo_ignored_test_manifest_rows.rs`) + the block DAG (`typeinfo_parity_blocks.rs`)
  from it; the Rust guard tests only diff/fail and never write tracked source. One coverage
  row per manifest row; a row whose `mechanism_id` cannot be resolved (or resolves to a
  placeholder) fails generation. The authoritative `row → block_id` projection over all 362
  `IgnoredTestRow`s is enumerated in full in §10.4.1; each subplan's `Exact test rows
  lifted` list is the per-block slice of that partition. (A reverse `cargo run`
  coverage-table generator — a table generated FROM the manifest, in the same discipline as
  the oracle rows / proof registry / row-test wrapper — is a `U0-FINISH-B` deliverable and
  is NOT yet built; today the manifest is generated FROM §10.4.1, not the other way around.)
- **Completeness is DEFINED by this table over the 362 `IgnoredTestRow`s PLUS every
  `AdditionalProofRow`.** Full-parity completeness IS this row-exact coverage table being
  complete and non-placeholder over all rows in both tables. A "missing TS rule" is exactly
  one of two mechanically-detectable things: (a) a row (in either table) whose `mechanism_id`
  the guard REJECTS as a placeholder (an unimplemented / `todo` / `unknown` mechanism — a
  genuine gap), or (b) a genuinely NEW fixture not among the 362, handled by the SEPARATE
  `AdditionalProofRow` table. There is no third "we think it's covered" state.
- **Gate: a block's rows cannot flip to `Lifted` until their coverage is complete +
  non-placeholder.** The coverage table is a CI PRECONDITION on every block branch: the
  branch may transition its rows `Ignored → Lifted` (and strip their source `#[ignore]`s)
  only when every one of its rows has a non-placeholder `mechanism_id`, an executable
  `ProofRequirement`, and a `semantic_queries`/facts mapping consistent with its capability.
  The coverage guard runs in the full CI gate, so a branch that flips rows to `Lifted`
  without complete coverage fails CI and cannot merge.

Two guards make this mechanical:

- **`every_manifest_row_has_non_placeholder_mechanism_and_executable_proof`** — asserts
  EVERY manifest row across BOTH tables has a coverage-table entry whose `mechanism_id` is
  non-placeholder AND whose `ProofRequirement` resolves to an executable proof. This is the
  "missing TS rule" detector for class (a); it spans both tables (the ignored-count +
  bijection guards span only the `IgnoredTestRow` table).
- **`capability_rows_map_to_expected_query_fact_mechanisms`** — asserts each capability's
  rows map (in the coverage table) to the query/fact mechanisms that capability is supposed
  to use — e.g. `FlowNarrowing` rows dispatch `FlowNarrowingAt` / `FlowReturn` / `Relate`;
  `UtilityComposition` rows reduce through `Instantiate` / `IndexedAccess` / `KeyOf` /
  `MappedType` / `Conditional`; `RelationSemantics` rows go through `Relate`; `JsxResolution`
  rows go through `ResolveAmbientNamespace` / `IndexedAccess` / `KeyOf` / `ResolveClassSurface`
  / `Parameters` / `Instantiate` / `NormalizeIntersection` (no JSX-specific key, and NOT
  `ResolveCall` — the JSX-factory rows are `Parameters<…>` type-surface projections, not call
  dispatch; `jsx` / `jsxs` / `createElement` invocation is a `U6.CALL_RESOLVE` backfill);
  `ModuleFeatures` augmentation rows go through
  `ResolveDeclarationAugmentation` → `DeclarationAnalysis`. A row mapped to a mechanism
  inconsistent with its capability FAILS.

The gate is enforced by **`block_rows_cannot_lift_without_complete_coverage`** (in the
coverage guard set): the guard FAILS — and so fails CI on the block's branch, blocking the
merge — for any block whose rows are flipped `Lifted` while their coverage is not complete +
non-placeholder.

This closes the completeness class: 362-row full parity is not probed by sampling — it is the
mechanical property "the generated coverage table is complete and non-placeholder over every
manifest row in both tables, each mapped to the expected capability mechanism, before any
block's rows lift" — while the binding 362 total stays a separate, exact count/bijection over
the `IgnoredTestRow` table alone.

### 10.4.1 The authoritative U0 row → `block_id` partition (all 362 rows)

**Framing — the numbering is the coverage layering; the architecture is mechanism-first.**
The `U0`–`U15` block numbering is the **execution / coverage bucket** layering — the
units a branch lands and the rows each owns. It is NOT the architectural decomposition.
The ARCHITECTURE is **mechanism-first**, in dependence order: Foundation / cache / wire
(`U0` ledger, `U3` fact model, `U4` cache-runtime nodes, `U8` wire surface) → the
`CheckerTransaction` + relation / inference engine (`U2.RELATION_INFER` and the U2
reducers it drives) → the type constructors / reducers (indexed-access, mapped /
template, class surfaces, enums, utilities) → the `FunctionFlowGraph` + call / contextual
flow (the `U6` blocks — substrate, narrowing mechanisms, call resolution, contextual
callbacks) → projection / session / adapters (`U13` projection, `U11` session, `U14` /
`U15` adapters and final lift). The U-block buckets are read THROUGH that mechanism order:
each block's owning mechanism (its dominant `mechanism_id`) is what fixes its place in the
dependence DAG, not its number. This is framing only — it does not renumber `U0`–`U15` or
re-layout the blocks; the one mechanism-driven refinement below is that the narrowing
bucket is split into per-mechanism sub-blocks (`U6.NARROW_*`).

This is the **authoritative, exhaustive, HAND-AUTHORED** `row → block_id` map that the
Python manifest generator (`scripts/gen-typeinfo-ignore-manifest.py`) READS — it does not
emit this partition; it parses it (`parse_partition`) and emits the three checked-in
manifest-data files from it. It is a total function over the 362 `IgnoredTestRow`s: **every**
row appears exactly once under exactly one owning `block_id`, no row is owned by two blocks,
and the union is exactly the 362 manifest rows. The generator's `--check` mode regenerates
the three files from this partition and byte-compares, so a partition edit that drifts from
the checked-in manifest data fails `--check`. Each subplan's `Exact test rows lifted` list is
the projection of this partition onto that block — the two must agree row-for-row. The
row-exact completeness GATE that will additionally enforce this in-suite —
`capability_rows_map_to_expected_query_fact_mechanisms` (each row's owning block matches its
capability's expected mechanism), the bijection/count guards (§10.5), and
`every_manifest_row_has_non_placeholder_mechanism_and_executable_proof` — is FORWARD-DECLARED
in `BlockContractRow.required_guards` but NOT yet built (U0-FINISH-B); today the built check
is the Python generator's `--check`.

The owning `block_id` for each row is its **dominant mechanism** per the Capability Map
(§Capability Map) and the row-level-split notes in the subplans: a row's substrate maps it to
a capability, and the capability's row-level mechanism (a `mechanism_id` such as
`Relate.coinductive_scc`, `IndexedAccess.union_distribution`, `ReturnPathPeeker.two_frontier`,
`ResolveDeclarationAugmentation.declaration_analysis`, `ResolveAmbientNamespace.jsx_namespace`)
fixes the single owning block. The per-block counts (summing to 362) are:

| Owning `block_id` | Rows | Owning `block_id` | Rows |
|---|---:|---|---:|
| `U2.RELATION_INFER` | 20 | `U6.NARROW_*` (8 sub-blocks, below) | 104 |
| `U2.UTILITIES` | 42 | `U6.PREDICATE_ASSERTION` | 3 |
| `U2.INDEXED_ACCESS` | 16 | `U6.CALL_RESOLVE` | 21 |
| `U2.MAPPED_TEMPLATE` | 16 | `U6.CONTEXTUAL_CALLBACK` | 15 |
| `U2.CLASS_SURFACES` | 52 | `U6.VALUE_INFERENCE` | 1 |
| `U2.ENUMS` | 7 | `U6.ASYNC_GENERATOR` | 1 |
| `U2.MODULE_AUGMENTATION` | 8 | `U6.CROSS_FILE` | 6 |
| `U2.JSX_FOUNDATIONS` | 9 | `U6.LOOP_CLOSURE` | 3 |
| `U6.FLOW_RETURN_SUBSTRATE` | 7 | `U3.CACHE_FACT_MODEL` | 3 |
| `U10.RESULT_DB` | 13 | `U11.PUBLIC_RELATION_SESSION` | 9 |
| `U14.MACRO_ADAPTER` | 1 | `U15.FINAL_LIFT` | 5 |

Sum = 362 (the `U6.NARROW_*` bucket contributes its 104 rows as the sum of the eight
sub-blocks below). The narrowing bucket is split per-mechanism (mechanism-first
framing, above) into eight `U6.NARROW_*` sub-blocks; each of the former
`U6.NARROWING` block's 104 rows is assigned to exactly ONE sub-block by its dominant
narrowing mechanism (the `file::function` mechanism), so the sub-blocks partition the
104 with no row lost, added, duplicated, or re-tagged:

| `U6.NARROW_*` sub-block | Rows | Mechanism |
|---|---:|---|
| `U6.NARROW_TYPEOF` | 15 | `typeof` operator narrowing (`narrow_typeof.rs`) |
| `U6.NARROW_EQUALITY` | 15 | literal / `null` / `undefined` (strict-)equality narrowing (`narrow_equality.rs`) |
| `U6.NARROW_TRUTHINESS` | 15 | truthiness / optional-chain narrowing (`narrow_truthiness.rs`) |
| `U6.NARROW_IN` | 15 | `in`-operator narrowing (`narrow_in_operator.rs`) |
| `U6.NARROW_INSTANCEOF` | 14 | `instanceof` narrowing (`narrow_instanceof.rs`) |
| `U6.NARROW_DISCRIMINATED` | 14 | discriminated-union / switch / destructure correlation (`narrow_discriminated_union.rs`) |
| `U6.NARROW_SUBSTITUTION` | 11 | flow narrowing of a generic substitution (`substitution_types.rs` `sb01`–`sb08`/`sb11`–`sb13`) |
| `U6.NARROW_INVALIDATION` | 5 | narrowing preserved / invalidated across reassignment / opaque call / destructure (`flow_invalidations.rs` `fi01`/`fi02`/`fi04`/`fi05`/`fi09`) |

Sub-block sum = 104. These eight sub-blocks REPLACE the former single `U6.NARROWING`
block (which no longer exists as a `block_id`); `U6.PREDICATE_ASSERTION` (`fi08`,
`sb09`/`sb10`) and `U6.LOOP_CLOSURE` (`fi03`/`fi06`/`fi07`) remain SEPARATE blocks and
are unchanged by the split. Blocks not in this table (`U0.MANIFEST_SUBSTRATE`, `U2.QUERY_VALUE_DOMAIN`,
`U8.WIRE_SURFACE_CLOSURE`, `U12.EXPORTER`, `U13.PROJECTION`) own **zero** `IgnoredTestRow`s —
they build substrate (ledger / keys / wire / exporter / projection) the owning blocks lift
their rows through; their `Exact test rows lifted` is explicitly `none`.

The complete partition (each entry `file::function — substrate`):
<!-- BEGIN U0 row→block coverage table (362 rows). HAND-AUTHORED authoritative source: the manifest
     generator (scripts/gen-typeinfo-ignore-manifest.py) READS this partition and EMITS
     crates/verter_session/tests/manifest_data/typeinfo_ignored_test_manifest_rows.rs +
     typeinfo_parity_blocks.rs from it. Edit rows HERE, then regenerate. -->

**`U2.RELATION_INFER`** (20 rows):

- `conditional_infer.rs::conditional_infer_aliases_reduce_when_requested_directly` — `ConditionalInfer`
- `conditional_infer.rs::conditional_infer_tuple_pattern_resolves_each_slot` — `ConditionalInfer`
- `modern_ts_features.rs::satisfies_array_literal_widens_to_primitive_array` — `ModernTsFeatures`
- `no_infer.rs::no_infer_component_helper_pins_variant_from_props_argument` — `ConditionalInfer`
- `no_infer.rs::no_infer_literal_call_returns_pinned_literal_from_first_argument` — `ConditionalInfer`
- `recursive_conditional.rs::recursive_conditional_awaited_recursive_unwraps_nested_promises` — `ConditionalInfer`
- `recursive_conditional.rs::recursive_conditional_deep_partial_marks_every_nested_property_optional` — `ConditionalInfer`
- `recursive_conditional.rs::recursive_conditional_deep_readonly_marks_every_nested_property` — `ConditionalInfer`
- `recursive_conditional.rs::recursive_conditional_flatten_unwraps_three_deep_array_to_primitive` — `ConditionalInfer`
- `relation_semantics.rs::relation_any_extends_string_distributes_both_branches` — `RelationSemantics`
- `relation_semantics.rs::relation_distributive_conditional_over_union_emits_branch_union` — `RelationSemantics`
- `relation_semantics.rs::relation_fixed_tuple_assignable_to_first_plus_rest` — `RelationSemantics`
- `relation_semantics.rs::relation_infer_head_of_tuple_pattern` — `RelationSemantics`
- `relation_semantics.rs::relation_infer_params_of_function_preserves_optional_undefined` — `RelationSemantics`
- `relation_semantics.rs::relation_infer_tail_of_tuple_pattern` — `RelationSemantics`
- `relation_semantics.rs::relation_infer_value_of_object_property` — `RelationSemantics`
- `relation_semantics.rs::relation_never_via_generic_helper_collapses_to_never` — `RelationSemantics`
- `relation_semantics.rs::relation_optional_property_not_assignable_to_required` — `RelationSemantics`
- `relation_semantics.rs::relation_readonly_property_assignable_to_mutable` — `RelationSemantics`
- `typescript_rules.rs::typescript_rules_distributive_conditional_expands_each_union_arm` — `TypeScriptRules`

**`U2.UTILITIES`** (42 rows):

- `indexed_utilities.rs::direct_parameters_payload_extracts_function_argument` — `UtilityComposition`
- `indexed_utilities.rs::direct_parameters_second_extracts_number_argument` — `UtilityComposition`
- `indexed_utilities.rs::nested_indexed_utility_surface_resolves_all_terminal_members` — `UtilityComposition`
- `indexed_utilities.rs::nested_nonnullable_array_indexed_access_resolves_element` — `UtilityComposition`
- `indexed_utilities.rs::nested_parameters_nonnullable_indexed_payload_resolves` — `UtilityComposition`
- `tuple_labels.rs::tuple_labels_number_index_projects_all_elements_union` — `TupleFeatures`
- `tuple_labels.rs::tuple_labels_numeric_position_access_drops_label` — `TupleFeatures`
- `tuple_labels.rs::tuple_labels_numeric_position_access_on_optional_slot_carries_undefined` — `TupleFeatures`
- `tuple_labels.rs::tuple_labels_parameters_preserves_named_labels_and_optional_marker` — `TupleFeatures`
- `typescript_rules.rs::typescript_rules_awaited_recursively_unwraps_promises` — `TypeScriptRules`
- `utility_composition.rs::utility_composition_resolves_deep_intersection_config` — `UtilityComposition`
- `utility_composition.rs::utility_composition_resolves_required_pick_over_nested_nonnullable_payload` — `UtilityComposition`
- `utility_edge.rs::utility_edge_non_nullable_strips_null_and_undefined` — `UtilityComposition`
- `utility_edge.rs::utility_edge_omit_all_keys_yields_empty_object` — `UtilityComposition`
- `utility_edge.rs::utility_edge_pick_all_keys_yields_input_shape` — `UtilityComposition`
- `utility_edge.rs::utility_edge_pick_never_yields_empty_object` — `UtilityComposition`
- `utility_edge.rs::utility_edge_readonly_required_composes_modifiers` — `UtilityComposition`
- `utility_edge.rs::utility_edge_required_strips_optional_markers` — `UtilityComposition`
- `utility_top_bottom.rs::utility_top_bottom_utb01_return_type_of_any_is_any` — `UtilityComposition`
- `utility_top_bottom.rs::utility_top_bottom_utb02_return_type_of_never_is_never` — `UtilityComposition`
- `utility_top_bottom.rs::utility_top_bottom_utb07_parameters_of_any_is_unknown_array` — `UtilityComposition`
- `utility_top_bottom.rs::utility_top_bottom_utb08_parameters_of_never_is_never` — `UtilityComposition`
- `utility_top_bottom.rs::utility_top_bottom_utb11_constructor_parameters_any_is_unknown_array` — `UtilityComposition`
- `utility_top_bottom.rs::utility_top_bottom_utb12_instance_type_any_is_any` — `UtilityComposition`
- `utility_top_bottom.rs::utility_top_bottom_utb13_constructor_parameters_any_ctor_is_any_array` — `UtilityComposition`
- `utility_top_bottom.rs::utility_top_bottom_utb14_awaited_any_is_any` — `UtilityComposition`
- `utility_top_bottom.rs::utility_top_bottom_utb15_awaited_unknown_is_unknown` — `UtilityComposition`
- `utility_top_bottom.rs::utility_top_bottom_utb16_awaited_never_is_never` — `UtilityComposition`
- `utility_top_bottom.rs::utility_top_bottom_utb17_awaited_null_is_null` — `UtilityComposition`
- `utility_top_bottom.rs::utility_top_bottom_utb18_awaited_undefined_is_undefined` — `UtilityComposition`
- `utility_top_bottom.rs::utility_top_bottom_utb19_awaited_nested_promise_is_inner_primitive` — `UtilityComposition`
- `utility_top_bottom.rs::utility_top_bottom_utb20_non_nullable_any_is_any` — `UtilityComposition`
- `utility_top_bottom.rs::utility_top_bottom_utb21_non_nullable_unknown_is_empty_object` — `UtilityComposition`
- `utility_top_bottom.rs::utility_top_bottom_utb22_non_nullable_never_is_never` — `UtilityComposition`
- `utility_top_bottom.rs::utility_top_bottom_utb23_non_nullable_null_undefined_is_never` — `UtilityComposition`
- `utility_top_bottom.rs::utility_top_bottom_utb25_exclude_any_against_string_is_any` — `UtilityComposition`
- `variadic_tuples.rs::variadic_tuple_concat_alias_produces_joined_literal_tuple` — `TupleFeatures`
- `variadic_tuples.rs::variadic_tuple_head_of_sample_resolves_to_first_literal` — `TupleFeatures`
- `variadic_tuples.rs::variadic_tuple_init_of_sample_resolves_to_prefix_tuple` — `TupleFeatures`
- `variadic_tuples.rs::variadic_tuple_last_of_sample_resolves_to_terminal_literal` — `TupleFeatures`
- `variadic_tuples.rs::variadic_tuple_tail_of_sample_resolves_to_remaining_tuple` — `TupleFeatures`
- `variadic_tuples.rs::variadic_tuple_variadic_function_with_explicit_type_args_concatenates_tuples` — `TupleFeatures`

**`U2.INDEXED_ACCESS`** (16 rows):

- `deep_path.rs::deep_path_projection_resolves_terminal_without_losing_shape` — `PathProjection`
- `index_signatures.rs::index_signatures_dual_numeric_key_returns_numeric_signature_value` — `IndexSignatures`
- `index_signatures.rs::index_signatures_dual_string_key_returns_string_signature_value` — `IndexSignatures`
- `index_signatures.rs::index_signatures_numeric_index_publishes_signature` — `IndexSignatures`
- `index_signatures.rs::index_signatures_numeric_lookup_returns_signature_value` — `IndexSignatures`
- `index_signatures.rs::index_signatures_symbol_index_publishes_signature` — `IndexSignatures`
- `index_signatures.rs::index_signatures_symbol_lookup_returns_signature_value` — `IndexSignatures`
- `typescript_rules.rs::typescript_rules_indexed_access_reduces_terminal_property` — `TypeScriptRules`
- `typescript_rules.rs::typescript_rules_keyof_materializes_literal_key_union` — `TypeScriptRules`
- `typescript_rules.rs::typescript_rules_tuple_rest_element_resolves_array_element_type` — `TypeScriptRules`
- `union_key_access.rs::union_key_access_keyof_self_projects_full_value_union` — `UnionDistribution`
- `union_key_access.rs::union_key_access_two_key_union_projects_member_type_union` — `UnionDistribution`
- `wide_deep.rs::wide_deep_flag_active_resolves_boolean_terminal` — `PathProjection`
- `wide_deep.rs::wide_deep_projected_target_resolves_terminal_pick_intersection` — `PathProjection`
- `wide_deep.rs::wide_deep_projected_token_resolves_literal_union` — `PathProjection`
- `wide_deep.rs::wide_deep_row_flags_resolve_partial_record_surface` — `PathProjection`

**`U2.MAPPED_TEMPLATE`** (16 rows):

- `mapped_modifiers.rs::mapped_modifier_as_never_filter_drops_matching_keys` — `MappedTypes`
- `mapped_modifiers.rs::mapped_modifier_as_rename_capitalize_rewrites_keys` — `MappedTypes`
- `mapped_modifiers.rs::mapped_modifier_conditional_value_keeps_never_typed_members` — `MappedTypes`
- `mapped_modifiers.rs::mapped_modifier_minus_optional_strips_optional_and_undefined` — `MappedTypes`
- `mapped_template.rs::mapped_type_with_template_literal_key_remap_resolves_item_slot` — `MappedTypes`
- `mapped_template.rs::mapped_type_with_template_literal_key_remap_resolves_remapped_slot` — `MappedTypes`
- `mapped_template.rs::record_with_template_literal_key_union_projects_root_slot` — `MappedTypes`
- `mapped_template.rs::template_literal_key_alias_projects_static_template_slot` — `MappedTypes`
- `mapped_template.rs::template_literal_union_key_projects_static_slot_union` — `MappedTypes`
- `template_literal_inference.rs::template_literal_key_remap_capitalises_each_event_key` — `TemplateLiteralInference`
- `template_literal_inference.rs::template_literal_numeric_infer_extends_number_casts_to_literal` — `TemplateLiteralInference`
- `template_literal_inference.rs::template_literal_split_on_dot_produces_segment_tuple` — `TemplateLiteralInference`
- `template_literal_inference.rs::template_literal_strip_on_prefix_uncapitalises_remainder` — `TemplateLiteralInference`
- `template_literal_inference.rs::template_literal_strip_returns_input_unchanged_when_prefix_missing` — `TemplateLiteralInference`
- `typescript_rules.rs::typescript_rules_key_remap_exclude_filters_and_renames_keys` — `TypeScriptRules`
- `typescript_rules.rs::typescript_rules_template_intrinsic_evaluates_union` — `TypeScriptRules`

**`U2.CLASS_SURFACES`** (52 rows):

- `apparent_types.rs::apparent_types_ap01_string_length` — `ApparentTypes`
- `apparent_types.rs::apparent_types_ap02_string_to_upper_case` — `ApparentTypes`
- `apparent_types.rs::apparent_types_ap03_string_char_at` — `ApparentTypes`
- `apparent_types.rs::apparent_types_ap04_string_slice` — `ApparentTypes`
- `apparent_types.rs::apparent_types_ap05_number_to_fixed` — `ApparentTypes`
- `apparent_types.rs::apparent_types_ap06_number_to_string` — `ApparentTypes`
- `apparent_types.rs::apparent_types_ap07_number_to_exponential` — `ApparentTypes`
- `apparent_types.rs::apparent_types_ap08_array_length` — `ApparentTypes`
- `apparent_types.rs::apparent_types_ap09_array_map` — `ApparentTypes`
- `apparent_types.rs::apparent_types_ap10_array_filter` — `ApparentTypes`
- `apparent_types.rs::apparent_types_ap11_boolean_to_string` — `ApparentTypes`
- `apparent_types.rs::apparent_types_ap12_boolean_value_of` — `ApparentTypes`
- `apparent_types.rs::apparent_types_ap13_bigint_to_string` — `ApparentTypes`
- `apparent_types.rs::apparent_types_ap14_symbol_description` — `ApparentTypes`
- `apparent_types.rs::apparent_types_ap15_generic_constraint_length` — `ApparentTypes`
- `branded_types.rs::branded_double_intersection_collapses_to_never` — `ApparentTypes`
- `branded_types.rs::branded_key_access_projects_boolean_literal_brand_tag` — `ApparentTypes`
- `branded_types.rs::branded_key_access_projects_literal_brand_tag` — `ApparentTypes`
- `branded_types.rs::branded_symbol_key_access_projects_wrapped_value_type` — `ApparentTypes`
- `branded_types.rs::branded_unique_symbol_wrapper_publishes_branded_surface` — `ApparentTypes`
- `call_resolution.rs::call_resolution_abstract_constructor_instance_type_projects_class_shape` — `CallResolution`
- `class_features.rs::class_features_abstract_subclass_instance_includes_inherited_and_own_members` — `ClassFeatures`
- `class_features.rs::class_features_dog_sound_return_type_is_literal_woof` — `ClassFeatures`
- `class_features.rs::class_features_extends_plus_implements_projects_union_of_members` — `ClassFeatures`
- `class_features.rs::class_features_generic_subclass_substitutes_type_parameter_on_inherited_field` — `ClassFeatures`
- `class_features.rs::class_features_generic_subclass_with_own_type_param_substitutes_through_base` — `ClassFeatures`
- `class_features.rs::class_features_protected_inherited_member_drives_subclass_method_inference` — `ClassFeatures`
- `class_features.rs::class_features_static_generic_method_instantiation_projects_return_with_substitution` — `ClassFeatures`
- `class_features.rs::class_features_static_inheritance_resolves_inherited_field_type` — `ClassFeatures`
- `class_features.rs::class_features_static_inheritance_resolves_inherited_method_return` — `ClassFeatures`
- `decorators.rs::decorators_accessor_decorator_returning_same_target_publishes_public_property` — `ClassFeatures`
- `decorators.rs::decorators_identity_accessor_decorator_publishes_public_property` — `ClassFeatures`
- `decorators.rs::decorators_identity_method_decorator_preserves_return_inference` — `ClassFeatures`
- `decorators.rs::decorators_metadata_reader_describe_return_is_literal_union` — `ClassFeatures`
- `function_advanced.rs::function_advanced_call_construct_hybrid_constructor_parameters_uses_construct_signature` — `CallResolution`
- `function_advanced.rs::function_advanced_call_construct_hybrid_instance_type_uses_construct_signature` — `CallResolution`
- `function_advanced.rs::function_advanced_call_construct_hybrid_parameters_uses_call_signature` — `CallResolution`
- `function_advanced.rs::function_advanced_call_construct_hybrid_return_type_uses_call_signature` — `CallResolution`
- `function_advanced.rs::function_advanced_class_method_prototype_extraction_projects_parameters` — `CallResolution`
- `function_advanced.rs::function_advanced_class_method_prototype_extraction_projects_return` — `CallResolution`
- `function_advanced.rs::function_advanced_constructor_parameters_publishes_constructor_arg_tuple` — `CallResolution`
- `function_advanced.rs::function_advanced_instance_type_publishes_constructor_return_shape` — `CallResolution`
- `function_advanced.rs::function_advanced_return_type_of_overloaded_function_uses_last_overload` — `CallResolution`
- `modern_ts_features.rs::variance_annotation_in_substitution_through_consumer_consume_parameters` — `ModernTsFeatures`
- `substitution_types.rs::substitution_types_sb14_default_type_arg_ignored_by_return_type` — `TypeParameterFeatures`
- `substitution_types.rs::substitution_types_sb15_recursive_generic_substitution` — `TypeParameterFeatures`
- `typescript_rules.rs::typescript_rules_class_instance_type_includes_fields_and_methods` — `TypeScriptRules`
- `typescript_rules.rs::typescript_rules_constructor_parameters_resolve_tuple` — `TypeScriptRules`
- `typescript_rules.rs::typescript_rules_instance_type_resolves_constructed_object` — `TypeScriptRules`
- `typescript_rules.rs::typescript_rules_typeof_const_preserves_readonly_literals` — `TypeScriptRules`
- `unique_symbol.rs::unique_symbol_indexed_access_via_typeof_returns_literal_value` — `UniqueSymbol`
- `unique_symbol.rs::unique_symbol_string_key_access_returns_sibling_value` — `UniqueSymbol`

**`U2.ENUMS`** (7 rows):

- `enums.rs::enum_const_enum_member_resolves_to_inlined_string_literal` — `EnumResolution`
- `enums.rs::enum_discriminant_extract_projects_matching_arm_payload` — `EnumResolution`
- `enums.rs::enum_keyof_typeof_numeric_yields_member_name_union` — `EnumResolution`
- `enums.rs::enum_keyof_typeof_string_yields_member_name_union` — `EnumResolution`
- `enums.rs::enum_numeric_member_resolves_to_branded_literal_zero` — `EnumResolution`
- `enums.rs::enum_string_member_resolves_to_branded_string_literal` — `EnumResolution`
- `enums.rs::enum_template_literal_over_string_enum_produces_value_union` — `EnumResolution`

**`U2.MODULE_AUGMENTATION`** (8 rows):

- `modern_ts_features.rs::import_attribute_simulated_resolves_imported_json_shape` — `ModernTsFeatures`
- `modern_ts_features.rs::import_attribute_simulated_string_literal_indexed_member` — `ModernTsFeatures`
- `module_features.rs::module_features_cjs_export_equals_resolves_to_carrier` — `ModuleFeatures`
- `module_features.rs::module_features_namespace_geometry_vector_aliases_point` — `ModuleFeatures`
- `module_features.rs::module_features_namespace_interface_merge_namespace_value_resolves_to_literal` — `ModuleFeatures`
- `module_features.rs::module_features_typeof_import_default_resolves_value_shape` — `ModuleFeatures`
- `module_features.rs::module_features_typeof_import_named_shape_resolves_to_interface` — `ModuleFeatures`
- `module_features.rs::module_features_typeof_import_named_value_resolves_to_literal` — `ModuleFeatures`

**`U2.JSX_FOUNDATIONS`** (9 rows):

- `jsx.rs::jsx_element_resolves_to_declared_interface_shape` — `JsxResolution`
- `jsx.rs::jsx_factory_inferred_props_for_component_resolves` — `JsxResolution`
- `jsx.rs::jsx_fc_props_includes_children_optional` — `JsxResolution`
- `jsx.rs::jsx_intrinsic_augmented_custom_card_resolves_to_declared_shape` — `JsxResolution`
- `jsx.rs::jsx_intrinsic_div_resolves_to_declared_shape` — `JsxResolution`
- `jsx.rs::jsx_intrinsic_keys_resolves_to_string_literal_union` — `JsxResolution`
- `jsx.rs::jsx_intrinsic_span_resolves_to_declared_shape` — `JsxResolution`
- `jsx.rs::jsx_intrinsic_via_generic_lookup_div_resolves_to_div_shape` — `JsxResolution`
- `jsx.rs::jsx_intrinsic_via_generic_lookup_span_resolves_to_span_shape` — `JsxResolution`

**`U6.FLOW_RETURN_SUBSTRATE`** (7 rows):

- `value_inference.rs::value_inference_arrow_expression_body_publishes_return_shape` — `ValueInference`
- `value_inference.rs::value_inference_arrow_expression_body_substitutes_parameter_references` — `ValueInference`
- `value_inference.rs::value_inference_computed_block_callback_value_resolves_local_return_shape` — `ValueInference`
- `value_inference.rs::value_inference_computed_callback_object_value_resolves_from_callback_body` — `ValueInference`
- `value_inference.rs::value_inference_const_object_literal_expands_nested_shape` — `ValueInference`
- `value_inference.rs::value_inference_flow_variables_narrow_return_value_by_branch` — `ValueInference`
- `value_inference.rs::value_inference_function_body_return_union_from_return_statements` — `ValueInference`

**`U6.NARROW_TYPEOF`** (15 rows):

- `narrow_typeof.rs::narrow_typeof_nt01_string_on_binary_union` — `FlowNarrowing`
- `narrow_typeof.rs::narrow_typeof_nt02_number_on_triple_union` — `FlowNarrowing`
- `narrow_typeof.rs::narrow_typeof_nt03_boolean_on_union` — `FlowNarrowing`
- `narrow_typeof.rs::narrow_typeof_nt04_object_on_union_keeps_no_null` — `FlowNarrowing`
- `narrow_typeof.rs::narrow_typeof_nt05_function_on_union` — `FlowNarrowing`
- `narrow_typeof.rs::narrow_typeof_nt06_undefined_on_union` — `FlowNarrowing`
- `narrow_typeof.rs::narrow_typeof_nt07_bigint_on_union` — `FlowNarrowing`
- `narrow_typeof.rs::narrow_typeof_nt08_symbol_on_union` — `FlowNarrowing`
- `narrow_typeof.rs::narrow_typeof_nt09_string_on_unknown` — `FlowNarrowing`
- `narrow_typeof.rs::narrow_typeof_nt10_string_on_unbound_generic` — `FlowNarrowing`
- `narrow_typeof.rs::narrow_typeof_nt11_negated_on_binary_union` — `FlowNarrowing`
- `narrow_typeof.rs::narrow_typeof_nt12_switch_exhaustive` — `FlowNarrowing`
- `narrow_typeof.rs::narrow_typeof_nt13_negated_guard_early_return` — `FlowNarrowing`
- `narrow_typeof.rs::narrow_typeof_nt14_compare_literal_var_does_not_narrow` — `FlowNarrowing`
- `narrow_typeof.rs::narrow_typeof_nt15_compound_and_property` — `FlowNarrowing`

**`U6.NARROW_EQUALITY`** (15 rows):

- `narrow_equality.rs::narrow_equality_eq01_string_literal_on_literal_union` — `FlowNarrowing`
- `narrow_equality.rs::narrow_equality_eq02_negated_string_literal_on_literal_union` — `FlowNarrowing`
- `narrow_equality.rs::narrow_equality_eq03_number_literal_on_triple_union` — `FlowNarrowing`
- `narrow_equality.rs::narrow_equality_eq04_boolean_true_on_boolean` — `FlowNarrowing`
- `narrow_equality.rs::narrow_equality_eq05_null_on_nullable_string` — `FlowNarrowing`
- `narrow_equality.rs::narrow_equality_eq06_undefined_on_optional_string` — `FlowNarrowing`
- `narrow_equality.rs::narrow_equality_eq07_double_equals_null_on_nullish_string` — `FlowNarrowing`
- `narrow_equality.rs::narrow_equality_eq08_string_literal_on_string_does_not_narrow` — `FlowNarrowing`
- `narrow_equality.rs::narrow_equality_eq09_string_literal_on_primitive_union` — `FlowNarrowing`
- `narrow_equality.rs::narrow_equality_eq10_two_unions_mutual_equality_does_not_narrow` — `FlowNarrowing`
- `narrow_equality.rs::narrow_equality_eq11_impossible_compound_absorbs_never` — `FlowNarrowing`
- `narrow_equality.rs::narrow_equality_eq12_property_equality_discriminant` — `FlowNarrowing`
- `narrow_equality.rs::narrow_equality_eq13_as_const_literal_rhs` — `FlowNarrowing`
- `narrow_equality.rs::narrow_equality_eq14_number_literal_on_number_does_not_narrow` — `FlowNarrowing`
- `narrow_equality.rs::narrow_equality_eq15_nan_equality_does_not_narrow` — `FlowNarrowing`

**`U6.NARROW_TRUTHINESS`** (15 rows):

- `narrow_truthiness.rs::narrow_truthiness_tr01_string_or_undefined` — `FlowNarrowing`
- `narrow_truthiness.rs::narrow_truthiness_tr02_string_or_null` — `FlowNarrowing`
- `narrow_truthiness.rs::narrow_truthiness_tr03_string_or_nullish` — `FlowNarrowing`
- `narrow_truthiness.rs::narrow_truthiness_tr04_string_no_nullable_does_not_narrow` — `FlowNarrowing`
- `narrow_truthiness.rs::narrow_truthiness_tr05_number_literal_union` — `FlowNarrowing`
- `narrow_truthiness.rs::narrow_truthiness_tr06_string_literal_union` — `FlowNarrowing`
- `narrow_truthiness.rs::narrow_truthiness_tr07_boolean_union` — `FlowNarrowing`
- `narrow_truthiness.rs::narrow_truthiness_tr08_negated_string_or_undefined` — `FlowNarrowing`
- `narrow_truthiness.rs::narrow_truthiness_tr09_property_truthiness` — `FlowNarrowing`
- `narrow_truthiness.rs::narrow_truthiness_tr10_early_return_guard` — `FlowNarrowing`
- `narrow_truthiness.rs::narrow_truthiness_tr11_unknown_collapses_to_unknown` — `FlowNarrowing`
- `narrow_truthiness.rs::narrow_truthiness_tr12_object_or_null` — `FlowNarrowing`
- `narrow_truthiness.rs::narrow_truthiness_tr13_compound_and_chain` — `FlowNarrowing`
- `narrow_truthiness.rs::narrow_truthiness_tr14_number_or_undefined_does_not_split_zero` — `FlowNarrowing`
- `narrow_truthiness.rs::narrow_truthiness_tr15_optional_chain_truthiness` — `FlowNarrowing`

**`U6.NARROW_IN`** (15 rows):

- `narrow_in_operator.rs::narrow_in_operator_io01_binary_union` — `FlowNarrowing`
- `narrow_in_operator.rs::narrow_in_operator_io02_shared_key` — `FlowNarrowing`
- `narrow_in_operator.rs::narrow_in_operator_io03_else_branch` — `FlowNarrowing`
- `narrow_in_operator.rs::narrow_in_operator_io04_intersection` — `FlowNarrowing`
- `narrow_in_operator.rs::narrow_in_operator_io05_optional_property` — `FlowNarrowing`
- `narrow_in_operator.rs::narrow_in_operator_io06_on_unknown` — `FlowNarrowing`
- `narrow_in_operator.rs::narrow_in_operator_io07_on_object` — `FlowNarrowing`
- `narrow_in_operator.rs::narrow_in_operator_io08_compound_conjunction` — `FlowNarrowing`
- `narrow_in_operator.rs::narrow_in_operator_io09_negated` — `FlowNarrowing`
- `narrow_in_operator.rs::narrow_in_operator_io10_three_arm_union` — `FlowNarrowing`
- `narrow_in_operator.rs::narrow_in_operator_io11_generic_constrained` — `FlowNarrowing`
- `narrow_in_operator.rs::narrow_in_operator_io12_reassignment_renarrowing` — `FlowNarrowing`
- `narrow_in_operator.rs::narrow_in_operator_io13_class_vs_object` — `FlowNarrowing`
- `narrow_in_operator.rs::narrow_in_operator_io14_template_literal_key` — `FlowNarrowing`
- `narrow_in_operator.rs::narrow_in_operator_io15_symbol_key` — `FlowNarrowing`

**`U6.NARROW_INSTANCEOF`** (14 rows):

- `narrow_instanceof.rs::narrow_instanceof_in01_binary_union` — `FlowNarrowing`
- `narrow_instanceof.rs::narrow_instanceof_in02_class_plus_primitive` — `FlowNarrowing`
- `narrow_instanceof.rs::narrow_instanceof_in03_on_unknown` — `FlowNarrowing`
- `narrow_instanceof.rs::narrow_instanceof_in04_subclass_union` — `FlowNarrowing`
- `narrow_instanceof.rs::narrow_instanceof_in05_already_narrowed` — `FlowNarrowing`
- `narrow_instanceof.rs::narrow_instanceof_in06_abstract_class` — `FlowNarrowing`
- `narrow_instanceof.rs::narrow_instanceof_in07_else_reachability` — `FlowNarrowing`
- `narrow_instanceof.rs::narrow_instanceof_in08_interface_union` — `FlowNarrowing`
- `narrow_instanceof.rs::narrow_instanceof_in09_negated_early_return` — `FlowNarrowing`
- `narrow_instanceof.rs::narrow_instanceof_in10_intersection` — `FlowNarrowing`
- `narrow_instanceof.rs::narrow_instanceof_in11_generic_ctor` — `FlowNarrowing`
- `narrow_instanceof.rs::narrow_instanceof_in13_array_special_case` — `FlowNarrowing`
- `narrow_instanceof.rs::narrow_instanceof_in14_promise_special_case` — `FlowNarrowing`
- `narrow_instanceof.rs::narrow_instanceof_in15_nullable` — `FlowNarrowing`

**`U6.NARROW_DISCRIMINATED`** (14 rows):

- `narrow_discriminated_union.rs::narrow_discriminated_union_du01_if_equality_discriminant` — `FlowNarrowing`
- `narrow_discriminated_union.rs::narrow_discriminated_union_du02_switch_discriminant` — `FlowNarrowing`
- `narrow_discriminated_union.rs::narrow_discriminated_union_du03_switch_default_never` — `FlowNarrowing`
- `narrow_discriminated_union.rs::narrow_discriminated_union_du04_negated_discriminant` — `FlowNarrowing`
- `narrow_discriminated_union.rs::narrow_discriminated_union_du05_multi_property_discriminant` — `FlowNarrowing`
- `narrow_discriminated_union.rs::narrow_discriminated_union_du06_nested_discriminant` — `FlowNarrowing`
- `narrow_discriminated_union.rs::narrow_discriminated_union_du07_number_literal_discriminant` — `FlowNarrowing`
- `narrow_discriminated_union.rs::narrow_discriminated_union_du08_boolean_literal_discriminant` — `FlowNarrowing`
- `narrow_discriminated_union.rs::narrow_discriminated_union_du09_destructure_correlation` — `FlowNarrowing`
- `narrow_discriminated_union.rs::narrow_discriminated_union_du10_in_guard_plus_discriminant` — `FlowNarrowing`
- `narrow_discriminated_union.rs::narrow_discriminated_union_du11_switch_per_arm_join` — `FlowNarrowing`
- `narrow_discriminated_union.rs::narrow_discriminated_union_du12_switch_fall_through` — `FlowNarrowing`
- `narrow_discriminated_union.rs::narrow_discriminated_union_du14_reassignment_re_narrowing` — `FlowNarrowing`
- `narrow_discriminated_union.rs::narrow_discriminated_union_du15_template_literal_discriminant` — `FlowNarrowing`

**`U6.NARROW_SUBSTITUTION`** (11 rows):

- `substitution_types.rs::substitution_types_sb01_bare_narrowing_of_generic` — `TypeParameterFeatures`
- `substitution_types.rs::substitution_types_sb02_narrowing_in_constrained_generic` — `TypeParameterFeatures`
- `substitution_types.rs::substitution_types_sb03_substitution_survives_method_calls` — `TypeParameterFeatures`
- `substitution_types.rs::substitution_types_sb04_narrowed_substitution_to_return_position` — `TypeParameterFeatures`
- `substitution_types.rs::substitution_types_sb05_compound_typeof_and_instanceof` — `TypeParameterFeatures`
- `substitution_types.rs::substitution_types_sb06_narrowing_widens_after_reassignment` — `TypeParameterFeatures`
- `substitution_types.rs::substitution_types_sb07_constraint_flow_apparent_type` — `TypeParameterFeatures`
- `substitution_types.rs::substitution_types_sb08_generic_in_conditional_no_distribute_on_unknown` — `TypeParameterFeatures`
- `substitution_types.rs::substitution_types_sb11_generic_narrowed_via_in_operator` — `TypeParameterFeatures`
- `substitution_types.rs::substitution_types_sb12_truthiness_on_t_or_undefined` — `TypeParameterFeatures`
- `substitution_types.rs::substitution_types_sb13_substitution_carried_across_destructure` — `TypeParameterFeatures`

**`U6.NARROW_INVALIDATION`** (5 rows):

- `flow_invalidations.rs::flow_invalidations_fi01_reassignment_invalidates_string_narrowing` — `FlowNarrowing`
- `flow_invalidations.rs::flow_invalidations_fi02_narrowing_preserved_across_opaque_call` — `FlowNarrowing`
- `flow_invalidations.rs::flow_invalidations_fi04_destructured_discriminant_preserves_correlation` — `FlowNarrowing`
- `flow_invalidations.rs::flow_invalidations_fi05_destructured_discriminant_loses_on_reassignment` — `FlowNarrowing`
- `flow_invalidations.rs::flow_invalidations_fi09_exhaustive_never_tail_does_not_widen_return` — `FlowNarrowing`

**`U6.PREDICATE_ASSERTION`** (3 rows):

- `flow_invalidations.rs::flow_invalidations_fi08_asserts_narrows_dotted_member_path` — `FlowNarrowing`
- `substitution_types.rs::substitution_types_sb09_asserts_x_is_string_on_generic` — `TypeParameterFeatures`
- `substitution_types.rs::substitution_types_sb10_x_is_t_predicate_on_generic` — `TypeParameterFeatures`

**`U6.CALL_RESOLVE`** (21 rows):

- `call_resolution.rs::call_resolution_extracted_prototype_method_call_returns_declared_return` — `CallResolution`
- `call_resolution.rs::call_resolution_generic_infers_from_callback_return_type` — `CallResolution`
- `call_resolution.rs::call_resolution_generic_infers_from_positional_argument_through_callback_signature` — `CallResolution`
- `call_resolution.rs::call_resolution_generic_infers_object_literal_including_excess_properties` — `CallResolution`
- `call_resolution.rs::call_resolution_optional_overload_picks_first_arity_matching_signature` — `CallResolution`
- `call_resolution.rs::call_resolution_optional_overload_picks_two_arg_signature_when_required` — `CallResolution`
- `call_resolution.rs::call_resolution_rest_overload_picks_rest_signature_when_required` — `CallResolution`
- `call_resolution.rs::call_resolution_union_argument_picks_union_compatible_overload` — `CallResolution`
- `call_resolution.rs::call_resolution_specific_literal_argument_picks_matching_overload_first` — `CallResolution`
- `call_resolution.rs::call_resolution_specific_literal_argument_skips_non_matching_first_overload` — `CallResolution`
- `call_resolution.rs::call_resolution_this_receiver_method_call_returns_declared_return` — `CallResolution`
- `const_type_param.rs::const_type_param_route_call_preserves_readonly_tuple_with_literal_paths` — `TypeParameterFeatures`
- `const_type_param.rs::const_type_param_string_call_preserves_readonly_literal_string_tuple` — `TypeParameterFeatures`
- `function_advanced.rs::function_advanced_constrained_generic_infers_literal_under_as_const` — `CallResolution`
- `function_advanced.rs::function_advanced_higher_order_composition_returns_concrete_function` — `CallResolution`
- `function_advanced.rs::function_advanced_omit_this_parameter_returns_function_without_this` — `CallResolution`
- `function_advanced.rs::function_advanced_overload_call_picks_matching_signature_return` — `CallResolution`
- `function_advanced.rs::function_advanced_overload_generic_first_binds_to_literal_argument` — `CallResolution`
- `function_advanced.rs::function_advanced_overload_generic_first_widens_t_to_string_for_string_argument` — `CallResolution`
- `function_advanced.rs::function_advanced_this_parameter_type_returns_this_annotation` — `CallResolution`
- `function_advanced.rs::function_advanced_void_callback_return_preserves_void` — `CallResolution`

**`U6.CONTEXTUAL_CALLBACK`** (15 rows):

- `call_resolution.rs::call_resolution_contextual_callback_return_picks_first_overload` — `CallResolution`
- `contextual_typing.rs::contextual_typing_ct01_callback_parameter_from_contextual_signature` — `ContextualTyping`
- `contextual_typing.rs::contextual_typing_ct02_callback_return_type_published` — `ContextualTyping`
- `contextual_typing.rs::contextual_typing_ct03_object_literal_assignment_from_typed_target` — `ContextualTyping`
- `contextual_typing.rs::contextual_typing_ct04_object_literal_in_function_call` — `ContextualTyping`
- `contextual_typing.rs::contextual_typing_ct07_as_cast_erases_context` — `ContextualTyping`
- `contextual_typing.rs::contextual_typing_ct08_jsx_like_attribute_contextual_typing` — `ContextualTyping`
- `contextual_typing.rs::contextual_typing_ct09_discriminated_union_contextual_narrowing` — `ContextualTyping`
- `contextual_typing.rs::contextual_typing_ct10_array_literal_contextually_typed_as_tuple` — `ContextualTyping`
- `contextual_typing.rs::contextual_typing_ct11_as_const_readonly_modifier` — `ContextualTyping`
- `contextual_typing.rs::contextual_typing_ct12_function_expression_argument_from_contextual_signature` — `ContextualTyping`
- `contextual_typing.rs::contextual_typing_ct13_object_literal_as_cast_narrows_shape` — `ContextualTyping`
- `contextual_typing.rs::contextual_typing_ct14_satisfies_widens_to_target` — `ContextualTyping`
- `contextual_typing.rs::contextual_typing_ct15_contextual_type_via_type_parameter_constraint` — `ContextualTyping`
- `flow_return_catalog.rs::flow_return_ho09_keeps_unknown_declared_callback_result_opaque` — `FlowNarrowing`

**`U6.VALUE_INFERENCE`** (1 row):

- `modern_ts_features.rs::satisfies_widens_inner_value_to_primitive_without_as_const` — `ModernTsFeatures`

**`U6.ASYNC_GENERATOR`** (1 row):

- `modern_ts_features.rs::await_using_simulated_return_type_resolves_to_primitive` — `ModernTsFeatures`

**`U6.CROSS_FILE`** (6 rows):

- `flow_return_catalog.rs::flow_return_xf02_expands_imported_value_function_return` — `FlowNarrowing`
- `flow_return_catalog.rs::flow_return_xf04_expands_barrel_imported_value_function_return` — `FlowNarrowing`
- `flow_return_catalog.rs::flow_return_xf04_records_barrel_route_before_selected_leaf` — `FlowNarrowing`
- `flow_return_catalog.rs::flow_return_xf05_resolves_namespace_import_value_call` — `FlowNarrowing`
- `flow_return_catalog.rs::flow_return_xf06_keeps_value_type_namespace_separate` — `FlowNarrowing`
- `flow_return_catalog.rs::flow_return_xf09_terminates_cross_file_recursive_returns` — `FlowNarrowing`

**`U6.LOOP_CLOSURE`** (3 rows):

- `flow_invalidations.rs::flow_invalidations_fi03_closure_capture_preserves_narrowing_at_return` — `FlowNarrowing`
- `flow_invalidations.rs::flow_invalidations_fi06_finally_return_overrides_try_catch_returns` — `FlowNarrowing`
- `flow_invalidations.rs::flow_invalidations_fi07_finally_without_return_preserves_try_catch` — `FlowNarrowing`

**`U3.CACHE_FACT_MODEL`** (3 rows):

- `cross_file.rs::cross_file_label_parameter_resolves_local_item` — `CrossFileResolution`
- `cross_file.rs::cross_file_projected_extra_resolves_number_terminal` — `CrossFileResolution`
- `cross_file.rs::cross_file_projected_item_resolves_local_extension` — `CrossFileResolution`

**`U10.RESULT_DB`** (13 rows):

- `demand_boundary.rs::demand_boundary_projection_into_selected_alias_loads_needed_but_not_unused` — `DemandBoundary`
- `demand_boundary.rs::demand_boundary_terminal_projection_resolves_value_without_unused_branch` — `DemandBoundary`
- `expansion_boundaries.rs::expansion_imported_projection_loads_selected_but_not_unselected_branch` — `ExpansionBoundaries`
- `expansion_boundaries.rs::expansion_imported_terminal_projection_reduces_flag_without_unselected_branch` — `ExpansionBoundaries`
- `expansion_boundaries.rs::expansion_inline_details_projection_expands_only_terminal_inline_path` — `ExpansionBoundaries`
- `expansion_boundaries.rs::expansion_local_branch_projection_expands_target_without_sibling_meta` — `ExpansionBoundaries`
- `expansion_boundaries.rs::expansion_omit_does_not_load_excluded_import` — `ExpansionBoundaries`
- `expansion_boundaries.rs::expansion_pick_does_not_load_unpicked_imports` — `ExpansionBoundaries`
- `mode_boundary_invariants.rs::mode_boundary_identity_does_not_materialize_alias_body` — `ModeBoundary`
- `mode_boundary_invariants.rs::mode_boundary_keyof_across_reexport_chain_resolves_all_keys` — `ModeBoundary`
- `mode_boundary_invariants.rs::mode_boundary_keyof_deep_chain_is_bounded_in_expanded` — `ModeBoundary`
- `mode_boundary_invariants.rs::mode_boundary_reexport_chain_resolves_imported_alias` — `ModeBoundary`
- `mode_boundary_invariants.rs::mode_boundary_shallow_does_not_expand_member_bodies` — `ModeBoundary`

**`U11.PUBLIC_RELATION_SESSION`** (9 rows):

- `cache_invalidation.rs::cache_invalidation_aug_patch_edit_surfaces_augmented_shape` — `CacheInvalidation`
- `cache_invalidation.rs::cache_invalidation_barrel_edit_excludes_prior_leaf_from_v2_footprint` — `CacheInvalidation`
- `cache_invalidation.rs::cache_invalidation_barrel_edit_redirects_route_to_new_leaf` — `CacheInvalidation`
- `cache_invalidation.rs::cache_invalidation_basic_selected_leaf_edit_flips_published_surface` — `CacheInvalidation`
- `cache_invalidation.rs::cache_invalidation_in_place_package_edit_flips_published_surface` — `CacheInvalidation`
- `cache_invalidation.rs::cache_invalidation_unselected_leaf_edit_keeps_warm_cache` — `CacheInvalidation`
- `demand_boundary.rs::demand_boundary_barrel_resolution_does_not_load_unrequested_reexport` — `DemandBoundary`
- `footprint.rs::typeinfo_footprint_is_attached_for_named_symbol_request` — `AuditFootprint`
- `footprint.rs::typeinfo_footprint_reports_requested_import_and_excludes_unprojected_branch` — `AuditFootprint`

**`U14.MACRO_ADAPTER`** (1 row):

- `basic.rs::component_like_slot_payload_extracts_parameters_from_nested_slot_property` — `MacroResolution`

**`U15.FINAL_LIFT`** (5 rows):

- `menu_like.rs::menu_like_model_value_resolves_nested_conditional_utilities` — `CompositeSurfaces`
- `menu_like.rs::menu_like_slot_payload_extracts_item_and_model_value` — `CompositeSurfaces`
- `message_list_like.rs::message_list_like_extracts_pick_from_inferred_array_element` — `CompositeSurfaces`
- `message_list_like.rs::message_list_like_slot_remaps_payload_with_message_context` — `CompositeSurfaces`
- `table_like.rs::table_like_dynamic_slot_projection_uses_template_literal_keys` — `CompositeSurfaces`
<!-- END U0 row→block coverage table. Union = 362 unique IgnoredTestRows, each owned by exactly one block_id. -->

### 10.5 The exact-362 count and bijection

`EXPECTED_TOTAL_IGNORED_COUNT` is ALWAYS exactly `count(IgnoredTestRow where status ==
Ignored)`; it is 362 at U0. It is NOT a frozen constant that lags the row states — it is the
live count of `Ignored` `IgnoredTestRow`s, and the block branch that changes how many are
`Ignored` updates it IN THE SAME BRANCH (and so in the same squash-merge) so the count guard
never observes a disagreement in any committed state. The count and bijection are over the
`IgnoredTestRow` table ONLY — `AdditionalProofRow`s are excluded (they carry no
`IgnoreStatus`). The bijection guards are: live ignored test sites (source `#[ignore]`s) must
exactly equal `IgnoredTestRow`s with `status == Ignored`, and that set must also exactly equal
`EXPECTED_TOTAL_IGNORED_COUNT`.

> **Lifted-lifecycle deferral (U0-FINISH-A is intentionally all-`Ignored`):** the ledger this
> block lands lifts ZERO rows — the generator always emits `status: IgnoreStatus::Ignored`, the
> guards validate every row against a live source `#[ignore]` site, and the count is pinned at
> exactly 362 `Ignored` rows. The `IgnoreStatus::Lifted { block_id }` variant exists in the
> schema as the FORWARD-DECLARATION for the per-block lift lifecycle described below; the
> lifted-row GENERATION + validation path (a lifted row drops its `#[ignore]` and its status
> becomes `Lifted { block_id }`, gated by the row-exact coverage gate) is exercised when an
> actual parity block lands and lifts its rows. That path is OWNED by the lifting block / the
> U0-FINISH-B coverage gate — it is NOT built in U0-FINISH-A. The accounting below is the
> contract that lifecycle will honour, not machinery that runs here yet.

The accounting is a single coupled edit on the block's branch:

- **The block's branch** strips the block's source `#[ignore]`s AND flips its rows
  `Ignored → Lifted` AND sets `EXPECTED_TOTAL_IGNORED_COUNT = count(status == Ignored)`
  (decrements by exactly the block's row count) — ALL in the SAME branch, so at every committed
  instant (the branch tip AND the post-squash-merge target tip) the live source-`#[ignore]`
  count, the `Ignored` row count, and the count agree, and the count/bijection guards stay green.
  CI runs the FULL workspace gate against the branch's `Lifted` state (the block's `#[ignore]`s
  already removed, so the lifted tests execute under the gate); a `Lifted` row must correspond to
  a live test function without `#[ignore]`.
- **Rollback is `git revert`** of the block's squash commit — it restores the source
  `#[ignore]`s, the rows `Lifted → Ignored`, and `EXPECTED_TOTAL_IGNORED_COUNT` in one revert,
  exactly because the branch coupled all three edits. No separate compensating transaction is
  needed: reverting the one squash commit is the rollback.

The two-table split is pinned by a dedicated binding-total count guard:

- **`ignored_test_row_table_holds_exactly_362_rows`** — asserts the `IgnoredTestRow` table holds
  EXACTLY 362 rows (the binding manifest total), DISJOINT from the `AdditionalProofRow` table; no
  `AdditionalProofRow` participates in `EXPECTED_TOTAL_IGNORED_COUNT` or the bijection; and the two
  tables are disjoint (no `(file, function)` identity in both — a submatrix/additional fixture
  corresponding to an existing ignored row stays an `IgnoredTestRow`, never duplicated). An
  `IgnoredTestRow` count other than 362, an `AdditionalProofRow` counted toward the ignored total
  or bijection, or a `(file, function)` in both tables, FAILS.

- **`additional_proof_row_table_holds_exactly_7_rows`** — asserts the `AdditionalProofRow` table
  holds EXACTLY 7 coverage-only rows = the 6 JSX no-new-key submatrix rows (`U2.JSX_FOUNDATIONS`) +
  the 1 mapped companion `mapped_modifier_minus_optional_preserves_explicit_undefined_on_required_property`
  (`U2.MAPPED_TEMPLATE`). The set is CLOSED, not growing. The table is disjoint from `IgnoredTestRow`,
  and no `AdditionalProofRow` is counted toward `EXPECTED_TOTAL_IGNORED_COUNT` or the bijection. An
  `AdditionalProofRow` count other than 7, or a row that leaks into the ignored count/bijection, FAILS.

## 11. The git/CI landing protocol

Blocks land through git and CI, not through a tracked orchestration cursor. **Git is the
transaction log, branch protection is the accept gate, and `git revert` is rollback.** The
substrate that lands a block is exactly three things, each already provided by git/CI:

- the **manifest ledger** (the two-table `IgnoredTestRow` + `AdditionalProofRow` state, §10);
- the **source-`#[ignore]` state** (the test-site annotations);
- the **block branch + its squash-merge commit** (the per-block unit of work and the atomic
  landing edge).

A block is landed iff its squash commit (carrying the `Typeinfo-Block:` trailer) is merged AND
its rows are `Lifted` AND its required guards pass in CI. That is derivable from git + the
manifest alone — there is NO tracked cursor, NO write-ahead log, NO lease, NO revision CAS, and
NO persisted gate/review receipt. The eight parts of the protocol:

### 11.1 One branch per block

Each block is implemented on its OWN branch off the target branch
(`refactor/semantic-db-overhaul`). The branch is the unit of in-flight work and the only place a
block's incomplete state ever lives — there is no tracked cursor recording "block X is in
flight." A block's branch does exactly three coupled things in the SAME branch (so every commit
on it, and the eventual squash-merge, is internally consistent):

1. makes the block's code changes;
2. removes the block's exact source `#[ignore]`s; and
3. flips the block's manifest rows `Ignored → Lifted` and sets
   `EXPECTED_TOTAL_IGNORED_COUNT = count(status == Ignored)` (decrement by the block's row count).

Because (2) and (3) are coupled in one branch, the count/bijection guards (§10.5) hold at the
branch tip and at the post-merge target tip — there is no window where the source-`#[ignore]`
count, the `Ignored` row count, and `EXPECTED_TOTAL_IGNORED_COUNT` disagree in any committed,
mergeable state. The WIP series on the branch MAY carry intermediate `todo!()` / placeholder /
empty-test states (the `CLAUDE.md` Stub-Prevention WIP exemption) — those are scratch states that
never reach the target branch; the squash-merged commit is the final, gated state.

### 11.2 CI is the gate

CI is the precondition for landing — the branch may merge only on GREEN CI. CI runs, on the
block's branch:

- the COMPLETE Rust **AND** JavaScript workspace gate, green only when BOTH pass — **Rust:**
  `cargo nextest run --workspace` **AND** `cargo test -p verter_session --tests` (the
  canonical pair the block's `verification_labels` carry — bare `cargo test --workspace
  --tests` silently skips the verter_session integration suite and must NOT be the sole
  gate); `cargo clippy --workspace -- -D warnings`; `cargo fmt --all --check`.
  **JavaScript:** `pnpm test`; `pnpm install --frozen-lockfile`;
- the coverage / proof gates (§10.4 — `every_manifest_row_has_non_placeholder_mechanism_and_executable_proof`,
  `capability_rows_map_to_expected_query_fact_mechanisms`, `block_rows_cannot_lift_without_complete_coverage`,
  the proof-registry / row-test-wrapper guards, the count + bijection guards §10.5);
- the required guards for the block (its `TYPEINFO_PARITY_BLOCKS.required_guards` + the
  Critical-rule guards (R6) for any new `(CRITICAL)` rule it introduces); and
- the block-DAG / consumed-mechanism guard
  (`typeinfo_parity_block_dag_is_acyclic_and_consumed_keys_and_mechanisms_are_prereqs`, §11.5).

Because CI runs the FULL workspace gate against the branch's `Lifted` state (the block's
`#[ignore]`s already removed on the branch), the lifted tests execute under the gate. A branch
that flips rows `Lifted` without complete coverage, or whose lifted tests fail, or whose required
guards fail, has RED CI and cannot merge. Green CI is necessary; the three-reviewer LAND (§11.3)
is the additional human/agent precondition.

### 11.3 Three-reviewer LAND (branch-protection accept gate)

The branch merges only when the review panel says LAND — a branch-protection **required-approval**
rule, enforced by the merge gate, NOT a persisted hash-bound receipt. The panel is EXACTLY THREE
reviewers — **1 Claude Code reviewer + 2 codex reviewers** — each adversarial / bad-mood, every
reviewer holding a **best-architecture-no-compromises mandate**: breaking changes are ALLOWED and
DESIRED; the goal is the best architecture / solution possible, never the easiest or least-breaking
path. The LAND bar is: **all three return LAND, OR all residual findings are NITs (cosmetic /
non-material — P3-class) only.** Any open material finding (P0 / P1 / P2-class) from ANY of the
three blocks the merge.

Staleness is handled by git, not by a hash receipt: branch protection re-requires approval on new
commits / requires the branch to be up to date with the target before merge, so a review of an
older branch state does not authorize a merge of a newer one. There is no
`pre_accept_verifying_hash`, no persisted `review_receipts`, and no `gate_receipts` — the merge
gate (green CI + required approvals + up-to-date branch) IS the accept gate. This is pinned as a
PROCESS rule (the branch-protection configuration), recorded by
`typeinfo_block_accept_requires_review_land_verdict` (§11.12 — reframed to the merge gate, no
receipt).

### 11.4 Squash-merge lands the block atomically

The branch's WIP series is squash-merged into EXACTLY ONE commit on the target branch — that
squash-merge is the atomic landing edge. The target branch therefore receives EXACTLY ONE commit
per landed block: a LOW landing-commit count, a HIGH WIP count during the work. The single squash
commit carries a machine-readable trailer:

```
Typeinfo-Block: <block-id>
```

Content integrity is provided by git + branch protection — the squash commit's tree IS the
reviewed, CI-green branch content, and branch protection forbids force-pushing a different tree
past the gate. There is NO `post_accept_lifted_tree_hash`, no `post_accept_lifted_hash`, and no
content-hash binding to recompute: the merge gate guarantees the merged tree is the gated tree.
Pinned by `typeinfo_block_lands_as_single_squashed_commit` (§11.11 — exactly one target-branch
commit per block carrying the `Typeinfo-Block:` trailer; the tree-hash binding is dropped).

### 11.5 Git is the transaction log; branch protection is the accept gate; `git revert` is rollback

"Done" for a block is derivable from git + the manifest, with NO tracked cursor, NO WAL, NO lease,
NO revision CAS, and NO receipts. A block is **done** iff ALL of:

1. its squash commit (carrying the `Typeinfo-Block: <block-id>` trailer) is MERGED into the target
   branch, AND
2. every row in its row-set is `Lifted` in the manifest, AND
3. its `TYPEINFO_PARITY_BLOCKS.required_guards` / the Critical-rule guards (R6) for any new
   `(CRITICAL)` rule the block introduces are PRESENT (registered and passing in the default suite).

The merged trailer (part 1) and the `Lifted` rows (part 2) move together because the branch
couples them and the squash-merge lands them atomically — there is no rows-`Lifted`-but-not-landed
intermediate on the target branch (that state only ever exists on the unmerged branch). **Rollback
is `git revert`** of the block's squash commit: because the branch coupled the code, the
`#[ignore]` removals, and the row/count edits, one revert restores all of them in a single commit.
This is the same predicate the landed-agreement check and prereq derivation read (§14).

The block prerequisite DAG is pinned by
**`typeinfo_parity_block_dag_is_acyclic_and_consumed_keys_and_mechanisms_are_prereqs`**, which is
ALSO part of the CI gate (§11.2). It is the block prerequisite DAG acyclicity + key-prerequisite
+ mechanism-prerequisite consistency guard described next.

`typeinfo_parity_block_dag_is_acyclic_and_consumed_keys_and_mechanisms_are_prereqs`
(lives in `crates/verter_session/tests/typeinfo_ignored_test_manifest.rs`, which U0
owns) models BOTH the semantic-query-key owner edges AND the block-MECHANISM
dependencies, so it catches the fi08-class deadlock that a keys-only guard cannot.
It reads two metadata sources:

- the per-key `key_owning_block` map (each `SemanticQueryName` → its owning
  `block_id`, e.g. `ResolveCall → U6.CALL_RESOLVE`); and
- the per-mechanism metadata: every `IgnoredTestRow` / `AdditionalProofRow` /
  `BlockContractRow` carries a dominant `mechanism_id: MechanismId` plus a
  `consumed_mechanisms: &'static [MechanismId]` list, and the U0-owned
  `fn mechanism_owning_block(MechanismId) -> TypeInfoParityBlockId` map names the
  single block that OWNS (produces) each mechanism.

It then builds the block-dependency graph from `TYPEINFO_PARITY_BLOCKS` + both
metadata sources + each row's `semantic_queries` and `consumed_mechanisms` lists,
and FAILS if any of these five checks does not hold:

1. **Block DAG acyclic.** The explicit block prerequisite graph (every
   `BlockContractRow.prereqs` edge) has no cycle.
2. **Dominant-mechanism owner == block.** Every row's dominant `mechanism_id` is
   owned by that row's own `block_id` (`mechanism_owning_block(mechanism_id) ==
   row.block_id`) — a row's owning block IS the producer of its dominant mechanism.
3. **Consumed-mechanism prerequisite.** For every row AND every block-level
   `consumed_mechanism`, `mechanism_owning_block(mech)` is the consuming block
   itself OR a transitive prerequisite of it.
4. **Consumed-key prerequisite (retained from the keys-only guard).** A block OR a
   row that consumes a `SemanticQueryName` whose `key_owning_block` is not itself or
   a transitive prerequisite of its `block_id` FAILS; and a row whose `block_id`
   disagrees with the prerequisites implied by its consumed keys FAILS.
5. **Diagnostic on failure.** On any failure the guard prints the offending
   row/block, the consumed key OR mechanism, that key/mechanism's owning block, and
   the missing-prerequisite path (the absent edge in the block DAG).

This mechanically pins two ownership facts. First, the U2↔U6 `ResolveCall`
ownership (check 4): `ResolveCall` is owned by `U6.CALL_RESOLVE`, so any block or
row consuming it must have `U6.CALL_RESOLVE` as a transitive prerequisite — which
is why `U2.CLASS_SURFACES` / `U2.JSX_FOUNDATIONS` (which precede U6 and must NOT
depend on it) consume `ResolveCall` neither directly nor through any owned row, and
why the genuine `ResolveCall`-consuming rows are owned by U6 blocks. Second, the
fi08-class deadlock (checks 2–3): `flow_invalidations_fi08_asserts_narrows_dotted_member_path`
is a `FlowNarrowing`-substrate row (its sibling `flow_invalidations` narrowing rows live
in `U6.NARROW_INVALIDATION`) whose dominant mechanism is the
`PredicateAssertion.assertion_effect_dotted_member_path` engine owned by
`U6.PREDICATE_ASSERTION`; under a keys-only model the row could sit in a narrowing
sub-block (`U6.NARROW_INVALIDATION` by its `flow_invalidations` substrate)
while consuming `U6.PREDICATE_ASSERTION`'s assertion engine even though
`U6.PREDICATE_ASSERTION` is not a prerequisite of any `U6.NARROW_*` sub-block (the actual edge is the reverse) —
a latent mechanism deadlock. The mechanism model FAILS this (check 2: the row's
dominant-mechanism owner `U6.PREDICATE_ASSERTION` ≠ a `U6.NARROW_*` sub-block `block_id`),
forcing the row's `block_id` to `U6.PREDICATE_ASSERTION`, where it correctly consumes
the `FlowNarrowing.frame` mechanism that the `U6.NARROW_*` sub-blocks — its declared prerequisites —
produce (check 3 holds, no cycle).

### 11.6 Why git/CI replaces the transaction substrate

There is no byte-level locking, atomic-rename, CAS, write-ahead log, or receipt-persistence
protocol to realize: git's commit DAG, branch protection, the CI gate, and `git revert` ALREADY
provide an atomic, crash-safe, rollback-capable landing boundary. A crashed agent leaves an
unmerged branch (or an unmerged PR), never a torn tracked cursor — the target branch only ever
moves by a gated, atomic squash-merge, so there is no half-applied state to reconcile and no
mid-transaction window for a reader to observe. This is strictly SAFER than the retired tracked
`.cutover-state.typeinfo_parity` cursor: that cursor was a versioned file the landing protocol
itself rewrote, which forced a cryptographic-fixed-point exclusion (`.cutover-state*` excluded
from its own content hash), a paired-hash gate receipt to survive the `Verifying → Lifted`
re-hash, and a WAL to recover a writer that died mid-rewrite. Git has none of those problems
because the log is not a tracked file the protocol mutates in place — it IS the history.

### 11.7 The done predicate (git + manifest)

A block is "done" / its prerequisite is "satisfied" iff the three-part predicate of §11.5 holds:
its squash commit (with the `Typeinfo-Block:` trailer) is MERGED, every row in its row-set is
`Lifted`, and its required / Critical-rule guards are present and passing. There is no token, no
lease, and no under-lock snapshot to read: the merged trailer + the manifest row state + the
guard suite are the whole predicate, and they are mutually consistent by construction because the
branch couples them and the squash-merge lands them atomically. This is the SAME predicate the
landed-agreement check (the manifest agrees with the merged `Typeinfo-Block:` trailers) and
prereq derivation (§14) read.

### 11.8 The named guards

The landing protocol is pinned by:
`block_rows_cannot_lift_without_complete_coverage` (§10.4 — a branch flipping rows `Lifted`
without complete coverage fails CI),
`landed_typeinfo_blocks_have_required_guards` (a landed block's required / Critical-rule guards
are present and passing — the §11.5 done-predicate part 3),
`no_vacuous_parent_u_block_landing` (§11.9),
`zero_row_blocks_land_exactly_once` (§11.10),
`typeinfo_parity_block_dag_is_acyclic_and_consumed_keys_and_mechanisms_are_prereqs` (the block
prerequisite DAG is acyclic and both key-prerequisite-consistent AND mechanism-prerequisite-consistent —
above),
`typeinfo_block_lands_as_single_squashed_commit` (§11.11 — each landed block contributes exactly
one target-branch commit carrying the `Typeinfo-Block:` trailer), and
`typeinfo_block_accept_requires_review_land_verdict` (§11.12 — the merge gate requires the
three-reviewer LAND / NITs-only verdict from all three panel reviewers, 1 Claude Code + 2 codex).

All of these guards run in the CI gate (§11.2). There are no `.cutover-state`/lease/CAS/WAL/receipt
guards — the retired transaction substrate's guards
(`typeinfo_state_snapshots_are_locked_and_precondition_checked`,
`pending_typeinfo_transactions_reconciled_before_eligibility`,
`cutover_state_typeinfo_writes_are_locked_and_cas`,
`parallel_typeinfo_block_landing_preserves_all_tokens`,
`resume_rejects_stale_typeinfo_block_lease`,
`cutover_state_typeinfo_namespace_isolated_from_legacy_cutover_tokens`,
`legacy_cutover_completion_preserves_typeinfo_namespace_when_active`,
`cutover_state_landed_blocks_match_typeinfo_manifest`,
`typeinfo_block_prereqs_derive_from_manifest_status`,
`typeinfo_block_prereqs_ignore_verifying_blocks`,
`verifying_typeinfo_block_lease_blocks_dependents`,
`workspace_gate_passes_before_typeinfo_block_acceptance`) are DELETED with the substrate they
pinned. The `workspace_gate_passes_before_typeinfo_block_acceptance` intent — "the full gate
passed before the block landed" — is now the CI gate itself (§11.2): a branch cannot merge on red
CI, so a merged block is gate-passed by construction.

`typeinfo_parity_block_dag_is_acyclic_and_consumed_keys_and_mechanisms_are_prereqs` is the keystone
guard above; it is part of the CI gate and lives in
`crates/verter_session/tests/typeinfo_ignored_test_manifest.rs` (U0-owned).

### 11.9 Parent / aggregate U-block tokens (no vacuous parent landing)

A parent U-block token (e.g. `U2`) is an AGGREGATE over its child blocks (e.g. `U2.RELATION_INFER`, `U2.UTILITIES`, …);
it does NOT own manifest rows directly. The naive "every landed block's rows are all `Lifted`" rule is UNSOUND for such
a token: a parent that owns zero manifest rows directly satisfies "all its rows are `Lifted`" VACUOUSLY. The end state
is ONE of (either is acceptable; the requirement is that a parent token is never vacuously satisfiable):

- **Aggregate parent row-set = UNION of child rows.** A parent's row-set is DEFINED as the UNION of every child block's
  manifest rows (transitively, over all blocks whose `owning_u_block` is that parent). A parent is "done" iff every row
  in that union is `Lifted` — never by owning zero rows.
- **OR parent tokens are derived-only (not stored).** Parent "landed" status is a DERIVED predicate (every child block
  landed / all child rows `Lifted`), never an independently STORED token; only child block tokens are stored.

Pinned by **`no_vacuous_parent_u_block_landing`**.

### 11.10 Zero-row block lifecycle

Some real child blocks own ZERO manifest rows: `U0.MANIFEST_SUBSTRATE`,
`U2.QUERY_VALUE_DOMAIN`, `U8.WIRE_SURFACE_CLOSURE`, `U12.EXPORTER`, and
`U13.PROJECTION` (the §10.4.1 partition lists them as zero-row). They build
substrate / wire / projection surfaces, not row lifts. A purely row-status
eligibility predicate is ill-defined for them — "all its rows are `Lifted`" is
VACUOUSLY true, so a cold agent could neither unambiguously SELECT them nor avoid
RE-selecting an already-done one. Their lifecycle is therefore MERGED-TRAILER /
done-predicate driven, not row-status driven:

- **Eligible** iff the block is NOT done by the §11.5 done predicate — i.e. its
  squash commit carrying the `Typeinfo-Block:` trailer is NOT yet merged into the
  target branch — AND its block-ID prereqs are merged (by the same predicate). A
  zero-row block's done predicate collapses to predicate parts 1 + 3 (its
  `Typeinfo-Block:` trailer merged, its required guards present), since part 2 (rows
  `Lifted`) is vacuously satisfied.
- **Its branch** makes only the substrate/wire/projection code changes, with NO
  row-status transition, NO `EXPECTED_TOTAL_IGNORED_COUNT` change, and NO source
  `#[ignore]` edit (it owns no rows). It still carries a `Typeinfo-Block: <block-id>`
  trailer on its squash commit, gated by the same CI + three-reviewer LAND.
- **It lands** when its branch merges (one squash commit, trailer present) — the
  merged trailer is the done-edge, exactly as for row-owning blocks.

The "next actionable block" selector COMPOSES two predicates so a cold agent always
has an unambiguous next block: the row-status predicate (for row-owning blocks) AND
this merged-trailer/done-predicate predicate (for zero-row blocks). A zero-row block
lands EXACTLY ONCE, is never skipped, and is never re-selected once its
`Typeinfo-Block:` trailer is merged. Pinned by **`zero_row_blocks_land_exactly_once`**
— asserts every zero-row block lands exactly once (its `Typeinfo-Block:` trailer
appears on exactly one merged target-branch commit, after CI + LAND), is never
skipped, and is never re-selected after its trailer is merged.

### 11.11 Git commit history — WIP series → squash-merge → one commit per block

This subsection governs how many commits the target branch
(`refactor/semantic-db-overhaul`) receives per landed block.

- **During block implementation the owning agent commits FREELY as a WIP series on
  the block's branch.** A HIGH WIP commit count is expected and encouraged (per-fix
  commits aid crash recovery — a fix-agent's partial work survives an API failure
  cleanly). **WIP commits do NOT run the full workspace gate.** Per `CLAUDE.md`'s
  Stub-Prevention WIP exemption, the in-flight WIP series MAY carry `todo!()` /
  placeholder / empty-test intermediate states — they are scratch states on the way
  to the merge, exactly the case the WIP exemption permits.
- **CI runs the full workspace gate on the branch** (§11.2) — the complete Rust
  **AND** JavaScript gate plus the coverage/proof gates, required guards, and the
  block-DAG guard — and any failure is fixed (with more WIP commits, re-running CI
  after the fixes). Green CI is necessary to merge.
- **The WIP series is squash-merged into EXACTLY ONE commit per landed block** on the
  target branch — that squash-merge is the atomic landing edge. The target branch
  therefore receives EXACTLY ONE commit per landed block: a LOW landing-commit count,
  a HIGH WIP count during the work. The squash-merge may happen only AFTER green CI
  (§11.2) AND the three-reviewer LAND verdict (§11.12) — branch protection enforces
  both as merge preconditions, so the merge never precedes the LAND authorization.
- **The squash commit carries a machine-readable trailer** binding it to the block:
  `Typeinfo-Block: <block_id>`. That single trailer is the whole binding — there is
  NO `Typeinfo-Lease`, NO `Pre-Accept-Verifying-Hash`, and NO content-hash trailer.
  Content integrity comes from git + branch protection: the squash commit's tree IS
  the reviewed, CI-green branch content, and branch protection forbids force-pushing a
  different tree past the gate. There is no `post_accept_lifted_tree_hash` / canonical
  projected content hash to recompute and compare — the merge gate guarantees the
  merged tree is the gated tree, so a stray post-review tracked change cannot ride in
  the merge without re-triggering CI + re-required approval.

Pinned by **`typeinfo_block_lands_as_single_squashed_commit`** — asserts each landed
block's `Typeinfo-Block: <block_id>` trailer appears on EXACTLY ONE target-branch
(`refactor/semantic-db-overhaul`) commit. The block ↔ commit relation is a BIJECTION:
the guard REJECTS zero commits carrying a block's trailer, more than one commit
carrying it, or a commit with a missing / malformed trailer. The TREE-HASH binding is
DROPPED — the guard checks only the one-commit-per-block + `Typeinfo-Block:` trailer
property; git + branch protection provide content integrity (the retired
`Pre-Accept-Verifying-Hash` / `post_accept_lifted_tree_hash` / canonical-projection
comparison is gone, along with the cryptographic-fixed-point exclusion it required).
The mapping is machine-checkable from git history alone (the trailer set on the target
branch), not a prose intent and not a manual inspection.

### 11.12 Review-LAND verdict gate (three-reviewer panel before merge)

A branch merges only when reviewers say to LAND, not by green CI alone. This is a
branch-protection **required-approval** rule — a PROCESS rule enforced by the merge
gate, NOT a persisted hash-bound receipt.

The review panel is EXACTLY THREE reviewers — **1 Claude Code reviewer + 2 codex
reviewers** — each adversarial / bad-mood, every reviewer holding a
**best-architecture-no-compromises mandate**: breaking changes are ALLOWED and
DESIRED; the goal is the best architecture / solution possible, never the easiest or
least-breaking path. Each reviewer evaluates the block's branch (its `Lifted`-state
diff against the target branch).

The branch CANNOT merge unless, IN ADDITION to green CI (§11.2), all three panel
reviewers return LAND. The LAND bar is: **all three return LAND, OR all residual
findings are NITs (cosmetic / non-material — P3-class) only.** Any open material
finding (P0 / P1 / P2-class) from ANY of the three blocks the merge.

- Staleness is handled by git, not a hash receipt: branch protection re-requires
  approval on new commits and requires the branch to be up to date with the target
  before merge, so a review of an older branch state cannot authorize a merge of a
  newer one (covering Rust AND JS changes equally — any new commit, of any kind,
  re-triggers CI and re-required approval).
- A non-LAND verdict from any reviewer, a missing reviewer, or an open material
  finding → the merge is BLOCKED.
- There is NO persisted `review_receipts` artifact, NO `pre_accept_verifying_hash`,
  and NO `xtask review-receipt` step. The verdict lives in the PR / branch-protection
  required-approval record; the merge gate (green CI + required approvals + up-to-date
  branch) IS the accept gate.

The §11.5 done predicate is unchanged (done = merged `Typeinfo-Block:` trailer + rows
`Lifted` + guards present). The review-LAND verdict and green CI are MERGE
preconditions, not done-predicate parts: the squash-merge cannot fire — so the
`Typeinfo-Block:` trailer never appears on the target branch — until both hold. The
done predicate is NOT weakened; it simply can never observe a merged block that was
not authorized by green CI + the three-reviewer LAND.

Pinned by **`typeinfo_block_accept_requires_review_land_verdict`** — a PROCESS rule
recording that the merge gate requires the three-reviewer LAND / NITs-only verdict
from all three panel reviewers (1 Claude Code + 2 codex), enforced as a
branch-protection required-approval rule; no merge on any reviewer's non-LAND verdict,
any open material (P0/P1/P2-class) finding, or a missing reviewer. The persisted-receipt
binding is DROPPED — git branch protection (required approvals + up-to-date branch)
provides staleness protection without a hash-bound receipt.

## 12. No-skip guarantee

A skipped block is mechanically visible in three ways: its rows remain `Ignored`, its tests remain ignored or red, and
dependent-block prereq guards fail. If someone removes an ignore without flipping the row to `Lifted`, the bijection guard
fails. If someone flips the row without removing the ignore, the count guard fails. `EXPECTED_TOTAL_IGNORED_COUNT` is ALWAYS
exactly `count(status == Ignored)` (never frozen): the block's branch sets it in the SAME branch that strips the
`#[ignore]`s and flips the rows `Ignored → Lifted` (§10.5), so the count/bijection guards stay green at the branch tip and
the post-merge target tip, and CI runs the full gate against that `Lifted` state. If someone flips a row to `Lifted`
without coverage, `block_rows_cannot_lift_without_complete_coverage` fails CI. If someone marks a block landed while rows
remain `Ignored`, the landed-agreement check (the manifest must agree with the merged `Typeinfo-Block:` trailers) fails. If
someone lands a parent/aggregate U-block while any child block's rows remain `Ignored` — including the vacuous zero-row case
— `no_vacuous_parent_u_block_landing` fails.

The landing boundary is git/CI itself (§11), not a land-then-revert dance: a block's incomplete state lives only on its
unmerged branch, and the target branch moves only by a gated, atomic squash-merge. Prerequisite checks and parent-completion
read merged `Typeinfo-Block:` trailers + `Lifted` rows (§11.5 / §14), so a dependent block can never start on — and thus
never observes as landed — a block whose branch has not yet merged. The CI gate runs the full workspace gate on every
branch (§11.2), so a merged block is gate-passed by construction; and "done" requires the merged trailer AND the block's
required / Critical-rule guards present (`landed_typeinfo_blocks_have_required_guards`) — never row status alone. Because
blocks land through independent branches + the merge queue (already serialized + atomic), two concurrent landings cannot
clobber each other or lose a landing: each is its own squash-merge, and `git revert` of a block's squash commit is the
rollback. No tracked cursor, lease, or CAS is involved — git's history is the transaction log.

## 13. No tracked orchestration cursor

There is NO `.cutover-state.typeinfo_parity` namespace, no tracked-cursor TOML schema, no
namespaced xtask, and no crash-recovery machinery. The typeinfo-parity landing protocol does NOT
write any tracked orchestration file: git history is the transaction log, branch protection is the
accept gate, and `git revert` is rollback (§11). Concretely, none of the following exists in this
architecture:

- a tracked `[typeinfo_parity]` block in `.cutover-state` (no `revision` CAS counter, no
  `active_blocks` lease map, no `landed_blocks` token list, no `gate_receipts` / `review_receipts`
  / `land_records` / `pending_transactions` subtables);
- a namespaced `xtask cutover-state typeinfo {dispatch,heartbeat,adopt,prepare-verify,gate-receipt,review-receipt,accept,abort}`
  surface;
- a write-ahead log / pending-transaction journal, a stable-sibling lockfile, a paired input-hash
  gate receipt, a persisted three-reviewer review receipt, or a `post_accept_lifted_tree_hash`
  content-projection.

A landed block is identified by its merged `Typeinfo-Block: <block-id>` trailer plus its `Lifted`
manifest rows (§11.5) — derivable from git + the manifest, with no separate tracked cursor to keep
in sync, lock, CAS, or reconcile. The LEGACY top-level `.cutover-state` cutover cursor (the
separate, broader-plan execution cursor with its own `active_block` / `landed_blocks` keys) is
UNRELATED to this architecture and is untouched: the typeinfo-parity landing protocol neither reads
nor writes it.

## 14. Resume protocol (git + manifest)

The resume protocol is GIT- and MANIFEST-DRIVEN and PARALLEL-SAFE (multiple agents may run it
concurrently via independent branches). A fresh agent determines what's next from git + the
manifest alone — no tracked cursor, no lease adoption, no staleness predicate, no WAL reconcile.

1. Read `semantic-db-overhaul-unified-remaining-plan.md` and `native-typeinfo-parity.md`. Read the
   manifest (`status` of each `IgnoredTestRow`), the merged `Typeinfo-Block:` trailers on the
   target branch, and the block prereq DAG (`TYPEINFO_PARITY_BLOCKS.prereqs`).
2. Run the manifest guard test first.
3. **Next-actionable selection (replaces lease adoption).** Compute the done set from git + the
   manifest: a block is DONE iff its `Typeinfo-Block:` trailer is merged into the target branch AND
   its rows are `Lifted` AND its required guards pass (§11.5 / §11.7). Then pick the FIRST block
   whose prereqs are all DONE and whose own state is still un-landed, by the COMPOSED selector
   §11.10 defines:
   - for a **row-owning block** — its own rows still have `status == Ignored` (its
     `Typeinfo-Block:` trailer not yet merged); OR
   - for a **zero-row block (§11.10)** (`U0.MANIFEST_SUBSTRATE`, `U2.QUERY_VALUE_DOMAIN`,
     `U8.WIRE_SURFACE_CLOSURE`, `U12.EXPORTER`, `U13.PROJECTION`) — its `Typeinfo-Block:` trailer is
     NOT yet merged (a zero-row block owns no rows, so its eligibility is merged-trailer driven, not
     row-status driven).

   Idempotency is "trailer already merged → skip": a block whose `Typeinfo-Block:` trailer is
   already merged is DONE and is never re-selected. Parallelism is independent branches + the merge
   queue (already serialized + atomic); two agents that pick the same block simply produce two
   branches and the merge queue serializes the merge — no CAS, no lease.
4. **Cut a branch for the chosen block** off the target branch (`refactor/semantic-db-overhaul`).
   The branch is the unit of in-flight work; there is no "dispatch" / lease step.
5. Execute exactly that block contract on the branch (committing a WIP series freely; §11.11).
6. Dry-run the block's tests (the exact lifted-row proofs) to confirm they pass — either via
   `cargo test … -- --ignored` (or the equivalent generated-wrapper invocation, which executes the
   row's declared proof) BEFORE the branch strips the `#[ignore]`s, OR after the branch flips the
   rows. The branch's `Lifted`-flip + `#[ignore]`-removal + count-decrement are one coupled edit on
   the branch (§10.5 / §11.1).
7. **Flip the block's rows on the branch.** The branch makes the code changes, removes the block's
   exact source `#[ignore]`s, flips its rows `Ignored → Lifted`, and sets
   `EXPECTED_TOTAL_IGNORED_COUNT = count(status == Ignored)` — all in the SAME branch (zero-row
   blocks skip the row/`#[ignore]`/count edits and make only their substrate changes; §11.10). The
   branch tip is count-consistent at every commit.
8. **Push the branch; CI runs the full gate** (§11.2) — the complete Rust **AND** JavaScript gate
   (`cargo nextest run --workspace` + `cargo test -p verter_session --tests` — the canonical pair;
   bare `cargo test --workspace --tests` silently skips the verter_session integration suite —
   `cargo clippy --workspace -- -D warnings`, `cargo fmt --all --check`, `pnpm test`,
   `pnpm install --frozen-lockfile`) plus the coverage/proof
   gates, the block's required guards, and the block-DAG guard — against the branch's `Lifted`
   state. Green CI is the precondition. Fix any failure with more WIP commits and re-run CI. WIP
   commits do not gate; CI on the branch does.
9. **Three-reviewer LAND, then squash-merge** (§11.3 / §11.4 / §11.12): on green CI AND an all-LAND
   / NITs-only verdict from all three panel reviewers (1 Claude Code + 2 codex) AND no unresolved
   design fork (§14.1), squash-merge the branch into ONE target-branch commit carrying the
   `Typeinfo-Block: <block-id>` trailer. Branch protection enforces green CI + required approvals +
   up-to-date branch as merge preconditions, so the merge is the atomic landing edge. Rollback is
   `git revert` of that squash commit (§10.5 / §11.5). Both row-owning and zero-row blocks land the
   same way; "done" additionally requires the block's required / Critical-rule guards present and
   passing (vacuously true for the row part of a zero-row block).
10. **Parent U-block completion is derived, not landed (§11.9):** a parent is done when every row in
    its UNION-of-child-rows row-set is `Lifted` (every child block's `Typeinfo-Block:` trailer
    merged). A parent is NEVER landed while any child block's rows remain `Ignored`; if parent
    tokens are derived-only, the parent becomes done automatically and is never independently landed.

Re-running a partially done block is safe because the manifest tells which rows still need lifting,
the merged trailers tell which blocks are done, and all cache/query changes are idempotent under the
one-engine guards.

### 14.1 Unforeseen-design-fork escalation (codex-architect decision gate)

When an UNFORESEEN issue or design fork arises DURING a block's implementation — an architectural decision, a fork, or a
sub-agent escalation that is **not** already settled by this plan — the orchestrator resolves it by escalating the DECISION
(not merely a review) to a **codex architect**. This is an execution-discipline rule for the driving orchestrator, not a code
invariant.

- **codex DECIDES, it is not merely a reviewer.** The orchestrator spawns a codex architect to MAKE the decision, prompted under
  the **best-architecture, no-compromises, breaking-changes-allowed, be-honest** mandate (the same mandate the §11.12 panel holds).
  No compromise / easy-path fallback is taken to move faster; the goal is the best architecture / solution possible.
- **High confidence is required.** The decision is accepted ONLY when codex expresses **high confidence**. A low-confidence or
  hedged codex result is iterated — re-prompted with the specific doubt — until a confident best-architecture decision exists.
- **Work does NOT continue until the fork is decided.** The block stays in its WIP / pre-merge state (the branch is not pushed
  for CI/merge) until the fork is resolved. An unresolved design fork is a MERGE-blocking condition, DISTINCT from the CI gate
  (§11.2) and the three-reviewer LAND (§11.12).
- **The orchestrator drives this loop 100% autonomously.** Consistent with the §14 resume protocol, it never pauses for a human
  checkpoint on a fork it can route to codex; it routes, iterates to high confidence, then continues the block.
- **Composition with §11.12.** codex deciding a fork ≠ the three-reviewer LAND panel. The fork decision happens DURING the block
  (WIP state, on the branch); the three-reviewer LAND panel happens at block-done, before the squash-merge. They are different
  stages of the same block.

This rule introduces NO new `(CRITICAL)` code rule and NO new mechanical guard — it is an orchestration-process rule for the
driving orchestrator, so it does not trip the R6 meta-guard (which requires a guard only for new `(CRITICAL)` code rules).

---

# Capability Map

The binding total is **362** `IgnoredTestRow`s. (Stale/non-authoritative counts: 356 and 371; 384 is the raw `#[ignore]` line
count including macro/body/non-site lines. U0 rederives 362 with the manifest parser; the manifest is authoritative.) The
substrate row counts below are the `IgnoredTestRow` rows per substrate (they sum to 362). Additional coverage is the CLOSED set
of exactly 7 coverage-only `AdditionalProofRow`s — the 6 JSX no-new-key submatrix rows (`U2.JSX_FOUNDATIONS`) + the 1 mapped
companion `mapped_modifier_minus_optional_preserves_explicit_undefined_on_required_property` (`U2.MAPPED_TEMPLATE`) — held in the
SEPARATE coverage-only `AdditionalProofRow` table (pinned closed-at-7 by `additional_proof_row_table_holds_exactly_7_rows`),
excluded from this 362 count + the source-`#[ignore]` bijection.

| Capability class / manifest substrate | Rows | Architecture component | Owning U-block |
|---|---:|---|---|
| `FlowNarrowing` | 104 | demand-sliced flow/narrowing | U6 |
| `ContextualTyping` | 13 | contextual expected-type propagation | U6 |
| `ValueInference` | 7 | value inference/widening | U6 |
| `CallResolution` | 30 | overload/call/generic inference | U6, with U2 overload key |
| `TypeParameterFeatures` | 17 | substitution/const/variance/NoInfer | row-level U2/U6 |
| `ConditionalInfer` | 8 | conditional + relation infer | U2 |
| `TupleFeatures` | 10 | tuple/rest/labels/infer | U2 |
| `UtilityComposition` | 31 | intrinsic utilities as graph reductions | U2 |
| `MappedTypes` | 9 | mapped modifiers/remap | U2 |
| `TemplateLiteralInference` | 5 | template reduce/infer | U2 |
| `PathProjection` | 5 | path-precise projection | U2 |
| `ExpansionBoundaries` | 6 | mode/depth/budget exactness | U2/U10 |
| `IndexSignatures` | 6 | key-kind/index signatures | U2 |
| `UnionDistribution` | 2 | union key/distribution | U2 |
| `RelationSemantics` | 10 | public relation engine | U2 |
| `TypeScriptRules` | 11 | TS semantic quirks | row-level U2/U6 |
| `ApparentTypes` | 20 | apparent/brand/unique-symbol surfaces | row-level U2/U6 |
| `ClassFeatures` | 13 | class surface + method flow | row-level U2/U6 |
| `EnumResolution` | 7 | enum value/type duality | U2 |
| `UniqueSymbol` | 2 | nominal unique symbol | U2 |
| `ModuleFeatures` | 6 | merge/ambient/augmentation/modules | U2 |
| `JsxResolution` | 9 | JSX namespace/component resolution | U2/U14 |
| `CrossFileResolution` | 3 | route/import demand facts | U3/U6 |
| `CacheInvalidation` | 6 | fact validation/route invalidation | U3/U10/U11 |
| `AuditFootprint` | 2 | footprint attachment | U11 |
| `DemandBoundary` | 3 | demand/mode audit | U2/U10 |
| `ModeBoundary` | 5 | mode-boundary invariants | U2/U10 |
| `ModernTsFeatures` | 6 | satisfies/await/variance/import attrs | row-level U2/U6 |
| `MacroResolution` | 1 | framework macro graph adapter | U14 |
| `CompositeSurfaces` | 5 | end-to-end adapter surfaces | U15 |

## The un-ignore / guarantee protocol over the 362 rows

The real un-ignore sets must be **row-exact in the manifest**, not inferred from substrate alone. Indicative file groupings:

- **Lifted mainly by U6:** `narrow_*`, `flow_return_*`, `flow_invalidations.rs`, `contextual_typing.rs`, `value_inference.rs`,
  `call_resolution.rs`, `function_advanced.rs`, and flow rows in `substitution_types.rs`.
- **Lifted mainly by U2:** `relation_semantics.rs`, `conditional_infer.rs`, `recursive_conditional.rs`, `tuple_labels.rs`,
  `variadic_tuples.rs`, `utility_*`, `indexed_utilities.rs`, `mapped_*`, `template_literal_inference.rs`, `index_signatures.rs`,
  `enums.rs`, `unique_symbol.rs`, `module_features.rs`, the four `decorators.rs` decorator/accessor rows, and pure class/brand rows.
- **Lifted with U3/U10/U11:** `cache_invalidation.rs`, `footprint.rs`, `demand_boundary.rs`, `mode_boundary_invariants.rs`.
- **Finished in U14/U15:** `basic.rs`, `menu_like.rs`, `message_list_like.rs`, `table_like.rs`.

The guarantee over the 362 rows is the composition of: the two-table ledger (§10) with the exact-362 count + bijection (§10.5);
the U0 row-exact capability→mechanism→proof coverage table (§10.4) that DEFINES completeness mechanically; the per-row executable
`ProofRequirement` with the generated proof registry + row-test wrapper (§10.2, §10.3); the git/CI landing protocol (§11) —
branch per block → green CI (full Rust+JS gate + coverage/proof/required/DAG guards) → three-reviewer LAND → squash-merge with
the `Typeinfo-Block:` trailer; the no-skip guarantee (§12); and the git/manifest-driven, parallel-safe resume protocol (§14). A
block lifts only its exact manifest rows, its rows can flip `Lifted` only after its coverage is complete + non-placeholder, and
it reaches `Lifted` + merged trailer only after a green CI gate over the exact branch content + the three-reviewer LAND — so the
362-row parity is mechanically tracked from `Ignored` to `Lifted`, never skipped and never vacuously satisfied.

---

# Guards index

The named architecture guards introduced by this plan, grouped. All guards are registered in their owning guard set and
cross-referenced from the section that introduces them. Per the R6 meta-guard, every `(CRITICAL)` rule this architecture
introduces lands with at least one named guard here.

## Type IR — `GraphTypeNode` / wire-surface purity

- `node_taxonomy_complete` (LANDED — the single enumerating assertion that pins the
  EXACT 32-arm `GraphTypeNode` `oneof kind` set, INCLUDING `module_augmentation` (23)
  and `global_augmentation` (25) as live arms, plus the additive `reserved 33 to 100;`
  window; `crates/verter_session/tests/g_block/typeinfo_graph_contract_guards.rs`)
- `graph_type_node_oneof_contains_only_type_value_arms` (NOT LANDED — phantom; never
  landed in `crates/`; subsumed by `node_taxonomy_complete`)
- `graph_type_node_allowlist_arms_have_type_value_classification` (NOT LANDED —
  phantom; never landed in `crates/`; subsumed by `node_taxonomy_complete`)
- `no_non_type_value_smuggled_into_graph_type_node`
- `typeinfo_wire_surface_has_no_retired_concept_fields` (NOT LANDED — phantom; never
  landed in `crates/`. It must NOT denylist `module_augmentation` / `global_augmentation`:
  those are live `GraphTypeNode` arms 23/25, not retired concepts)
- `flow_contextual_facts_not_graph_type_nodes`
- `program_analysis_graph_exposes_flow_contextual_queries`
- `flow_contextual_doc_and_wire_placement_match_program_analysis_graph`
- `relation_proofs_not_graph_type_nodes`
- `typeinfo_relate_payload_exposes_relation_proof_without_graph_type_node`
- `no_infer_not_type_parameter_metadata`
- `diagnostics_only_on_typeinfo_graph_payload`

## Type IR — `SemanticTypeGraph` embeddings / response landing

- `typeinfo_graph_response_payload_arm_is_additive_not_retyped`
- `framework_surface_payload_graph_payload_is_additive_not_retyped`
- `all_public_semantic_type_graph_embeddings_are_payload_wrapped`

## Type IR — decorators / auto-accessors (TS7)

- `decorator_identity_method_preserves_declared_return`
- `accessor_decorator_publishes_public_property`
- `decorated_method_literal_union_return_projects`
- `accessor_decorator_identity_target_return_keeps_public_property`

## Query keys — value domain

- `every_semantic_query_key_maps_to_exactly_one_value_domain`
- `flow_contextual_keys_return_program_analysis_value`
- `augmentation_keys_return_declaration_analysis_value`
- `declaration_augmentation_facts_not_type_nodes`
- `relate_query_value_carries_relation_proof_and_budget_state`
- `reserved_checker_queries_are_non_live_typeinfo_does_not_whole_body_check` (the reserved `DiagnosticAnalysis(CheckResult)` arm + `Check*` query names are NON-LIVE — no live query maps to them, no `SemanticQueryKeySpec` row carries them — and no typeinfo query whole-body type-checks a region; owned at U2.QUERY_VALUE_DOMAIN, §3)

## Query keys — declaration augmentation (generalized)

- `global_augmentation_query_has_declaration_analysis_identity`
- `declaration_augmentation_target_is_env_free_env_comes_from_context`
- `declaration_augmentation_doc_wire_query_placement_match`

## Query keys — added-key cache identity (env/context)

- `flow_return_key_covers_env_dimensions`
- `flow_return_key_covers_input_context_and_projection_demand`
- `resolve_class_surface_key_covers_side_demand_type_args_and_context`
- `apparent_type_key_covers_lib_env_demand_and_context`
- `template_literal_reduce_key_covers_context`
- `resolve_call_key_covers_args_this_contextual_type_overload_policy_and_context`
- `resolve_call_same_expr_different_flow_or_substitution_does_not_warm_hit`

## Query keys — `Relate` identity (existing-key upgrade)

- `relate_key_covers_relation_kind_policy_freshness_and_context`
- `relate_same_nodes_different_relation_kind_policy_or_env_do_not_warm_hit`
- `relate_same_nodes_different_inference_context_do_not_warm_hit`

## Query keys — per-key cross-context closure + meta-guard

The per-key cross-context closure discipline (one no-cross-context-warm-hit guard per key). For the LANDED U2B.5/6/7
keys the live test-tree guards are the short `*_do_not_warm_hit` forms (named here); the remaining entries are the
design-intent names for keys whose closure guards land alongside their reducers:

- `resolve_ambient_namespace_do_not_warm_hit` (landed)
- `resolve_overload_set_do_not_warm_hit` (landed)
- `resolve_enum_do_not_warm_hit` (landed)
- `flow_narrowing_at_do_not_warm_hit` (landed)
- `contextual_type_at_do_not_warm_hit` (landed)
- `resolve_class_surface_do_not_warm_hit` (landed)
- `resolve_merged_declaration_same_site_different_env_or_context_do_not_warm_hit` (forward-planned U2)
- `declaration_augmentation_key_same_site_different_env_or_context_do_not_warm_hit` (forward-planned U2)
- `resolve_decl_same_site_different_env_or_context_do_not_warm_hit`
- `instantiate_same_base_different_env_or_context_do_not_warm_hit`
- `indexed_access_same_base_different_env_or_context_do_not_warm_hit`
- `key_of_same_base_different_env_or_context_do_not_warm_hit`
- `mapped_type_same_source_different_env_or_context_do_not_warm_hit`
- `conditional_same_nodes_different_env_or_context_do_not_warm_hit`
- `type_of_same_value_root_different_env_or_context_do_not_warm_hit`
- `normalize_union_same_members_different_env_or_context_do_not_warm_hit`
- `normalize_intersection_same_members_different_env_or_context_do_not_warm_hit`
- `project_path_same_base_path_different_env_or_context_do_not_warm_hit`
- `resolved_named_type_key_identity_is_env_scoped`
- `resolve_macro_payload_same_owner_different_env_or_context_do_not_warm_hit`
- `semantic_query_key_spec_table_equals_enum` (the mechanical enum/table-equality meta-guard — LIVE in
  `crates/verter_session/tests/g_block/u2_spec_table_guards.rs` — replacing the soft
  `every_semantic_query_key_has_explicit_context_and_cross_context_warm_hit_guard`)
- plus dispatch-completeness and schema-version guards for any public wire arm

## Query keys — projection-demand / eval-policy lattice (query modes, §2.10)

- `query_modes_are_presets_over_projection_demand_eval_policy`
- `cache_satisfaction_is_demand_lattice_not_enum_order`
- `skeleton_is_typeparamshells_plus_carrier_stop_not_special_mode`

## Flow — flow graph + demand planner + cycle

- `flow_slice_is_graph_reachability_not_procedural_walk`
- `function_flow_graph_built_once_per_function_skeleton`
- `flow_graph_effect_edges_stay_live_past_value_writes`
- `flow_slice_keys_on_body_sensitive_hash_not_parse_stable_hash` (the `FlowSliceHashNode` key + `FlowSlice` fact root on `flow_body_stable_hash`, body-sensitive / cosmetic-insensitive — `return { b: 1 }` vs `return { b: 2 }` hash differently; owned at U6.FLOW_RETURN_SUBSTRATE)
- `flow_return_path_peeker_spread_override_skips_overwritten_sibling`
- `flow_return_path_peeker_alias_return_projects_requested_member_only`
- `flow_return_path_peeker_unknown_alias_mutation_degrades_path_not_whole_body`
- `flow_return_path_peeker_definite_write_keeps_prior_effects_for_selected_value`
- `flow_return_path_peeker_compound_assignment_reads_previous_path_value`
- `flow_return_path_peeker_destructuring_assignment_tracks_path_writes`
- `flow_return_path_peeker_captured_binding_unknown_escape_degrades_path`
- `flow_return_path_peeker_local_closure_call_applies_path_write_summary`
- `flow_return_path_peeker_try_finally_return_override_controls_contribution`
- `flow_return_path_peeker_labelled_break_preserves_reachability`
- `flow_return_path_peeker_computed_key_effects_survive_definite_write`
- the `Mytype` non-materialization negative guard
- `flow_cycle_sentinel_is_never_admitted_as_cache_entry`
- `flow_cycle_sentinel_does_not_hide_real_base_return_contributor`

## Relation — cycle / coinductive SCC

- `relation_cycle_assumptions_are_scoped_to_full_relate_identity`
- `relation_coinductive_scc_discharges_on_outgoing_obligations`
- `relation_cycle_sentinel_is_never_warm_admitted`

## Inference — checker transaction / session (PART 1 §4.2)

- `inference_runs_in_checker_transaction_not_per_surface_matcher`
- `only_completed_deterministic_sessions_are_admitted`
- `inference_candidate_combination_matches_priority_and_variance`
- `checker_reentry_graph_spans_flow_call_contextual_narrowing`
- `cross_engine_cycle_discharge_admits_only_stable_deterministic_results` (per-domain SCC/fixed-point discharge — `FlowReturn`/`ResolveCall`/`ContextualTypeAt`; no transient assumption or cycle sentinel warm-admits; owned at U6.CALL_RESOLVE)
- `variance_is_measured_by_marker_probe_fixed_point_not_assumed` (variance is computed by the SCC-aware marker-type-probe fixed-point and cached by declaration/env/TS-version; bivariant-method quirks in `RelationPolicy`; replaces any bare `variance_phase` stand-in; owned at U2.RELATION_INFER, PART 1 §4.0)
- `reverse_mapped_inference_is_relation_owned_in_session` (reverse-mapped inference is a relation-owned `InferenceSession` pass — per-key recovery via binding-producing `Relate`, reassembled by final substitution — not a private reverse-mapping matcher; owned at U2.MAPPED_TEMPLATE + U2.RELATION_INFER, PART 1 §4.2)
- `freshness_tracks_per_property_spread_taint` (fresh-object-literal excess checking is per-property with spread-taint propagation, in the session; not a whole-object freshness bit; owned at U2.RELATION_INFER, exercised at U6.VALUE_INFERENCE, PART 1 §4.2)

## Fact cache — domains (fact-based-cache.md closed `FactDomain` set)

- `program_analysis_fact_domain_validates_flow_slice` (the fourth closed `FactDomain::ProgramAnalysis` owns the `FlowSlice` fact; `StoreView::validates_program_analysis_domain` fails closed on missing/overflowed/stale/unrooted; owned at U3.CACHE_FACT_MODEL, produced at U6.FLOW_RETURN_SUBSTRATE)

## Fact cache — multi-candidate substrate + env/fact dimensions (PART 1 §6.1; fact-based-cache.md)

- `cache_candidate_cap_is_per_family_not_uniform` (the multi-candidate `FamilySlots` candidate cap is per-family via `candidate_cap()` — higher adaptive caps for `Relate`/`ResolveCall`/`Instantiate`/`Conditional`/`MappedType`/`FlowReturn`, small caps for content-light families; FAILS against a single uniform `FAMILY_SLOT_CANDIDATE_CAP`; owned at U3.CACHE_FACT_MODEL)
- `family_eviction_prefers_invalid_then_lru_valid_hit` (slot-cap eviction evicts invalid candidates first, then least-recently valid-hit, NOT FIFO; the benched per-family fallback-count bound via `BenchResultRow` is asserted alongside; owned at U3.CACHE_FACT_MODEL)
- `cache_keys_cover_ts_jsx_moduleresolution_decorator_lib_dimensions` (the split env hashes cover TS semantic version / JSX mode·import-source·factory / `moduleResolution` / package export-import conditions / `types`·`typeRoots` / lib set / decorator·class-field semantics / `useDefineForClassFields` / `customConditions`·`moduleSuffixes`, each in the env hash of the layer it affects under R21 — no bundled `project_config_hash`; owned at U3.CACHE_FACT_MODEL)
- `instantiation_depth_policy_in_identity_and_facts` (the `InstantiationDepthPolicy` is part of the depth-sensitive query-identity caches' identity — folded into `type_env_hash` — AND validated against the recorded `ReadSetSignature.facts`; owned at U3.CACHE_FACT_MODEL)
- `persistent_caches_never_admit_overlay_only_results` (overlay/session-scoped results populate the session cache only — never a base/persistent cache; overlay/session identity is session-cache identity only; owned at U3.CACHE_FACT_MODEL)

## Reducers — mapped optionality / template / contextual / declaration-merge

- `mapped_minus_optional_strips_only_optional_origin_undefined`
- `mapped_minus_optional_preserves_explicit_undefined_on_required_property`
- `template_literal_reduce_models_ts_numeric_bigint_lexing` (template-literal numeric/bigint `infer` matching uses TS lexical numeric/bigint semantics — hex/octal/binary/exponent/separator/`n`-suffix — oracle-pinned, not a Rust `parse`; owned at U2.MAPPED_TEMPLATE, PART 1 §4.3)
- `this_type_contextual_object_literal_binding_in_contextual_type_at` (a `ThisType<T>` arm in an object literal's contextual target binds the method `this` to `T` through `ContextualTypeAt`, exposed as a `ProgramAnalysisGraph` contextual fact and never a `GraphTypeNode` member; owned at U6.CONTEXTUAL_CALLBACK, PART 1 §4.6)
- `declaration_merge_records_binder_overload_augmentation_order_as_facts` (the merged-declaration / augmentation reducers order contributors by TS binder order + overload-group precedence + augmentation-contributor sequence and record that sequence as facts validated by `ReadSetSignature`; owned at U2.MODULE_AUGMENTATION, PART 1 §1.8)

## Performance budgets — non-admission

- `relation_budget_exceeded_admits_nothing`
- `keyspace_budget_exceeded_admits_nothing`
- `call_resolution_budget_exceeded_admits_nothing`
- `apparent_type_budget_exceeded_admits_nothing`

## Performance contract — perf hardening (PART 1 §6.2)

- `flow_graph_build_is_shallow_interned_no_lowering_lazy_regions` (the `FunctionFlowGraph` build uses compact interned IDs, lowers NO type at build time — no `TypeExpr` lowering / `Relate` / `Instantiate` / import fact from graph construction — and materializes oversized-function regions lazily so build cost scales with the sliced regions, not the whole body; owned at U6.FLOW_RETURN_SUBSTRATE, PART 1 §5 + §6.2)
- `cache_key_axes_are_minimal_and_normalized` (every context / substitution / demand / env axis on a query-identity key is benchmark-proven minimal + normalized — removing or denormalizing an axis either breaks a correctness fixture or leaves the benched hit rate unchanged; over-keying fragments slots, under-keying is stale; owned at U2.QUERY_VALUE_DOMAIN / U3.CACHE_FACT_MODEL, PART 1 §2.10 + §6.2)
- `relation_negative_and_unknown_paths_are_fast` (the common not-assignable / no-match / unknown outcome is decided by the fast-reject discriminator path with a repeat served from the full-identity pair memo, both WITHOUT entering the coinductive-SCC / member-recursion machinery, and without allocating a proof / fact / session transaction; memo locality is benched; owned at U2.RELATION_INFER, PART 1 §4.1 + §6.2)
- `architecture_minimizes_fallback_entry_not_fallback_cost` (the governing design rule: the tracked + perf-regression-gated metric is each family's fallback ENTRY count against its `BenchResultRow` bound, and the warm path is held O(validate); optimization targets fallback RATE — via warm-hit rate + minimal axes + cheap negative paths — not fallback latency; owned at U3.CACHE_FACT_MODEL + the U15 bench deliverable, PART 1 §6.2)

## JSX — existing-query resolution (no new keys)

- `jsx_resolution_uses_existing_semantic_queries`
- `jsx_intrinsic_elements_project_via_indexed_access`
- `jsx_no_dedicated_graph_type_node`
- `jsx_library_managed_attributes_via_ambient_namespace_and_indexed_access`
- `jsx_element_attributes_property_via_ambient_namespace_keyof`
- `jsx_element_children_attribute_via_ambient_namespace_keyof`
- `jsx_intrinsic_attributes_via_ambient_namespace_intersection`
- `jsx_element_class_check_via_resolve_class_surface_and_relate`
- `jsx_import_source_module_namespace_via_existing_resolution`

## Manifest ledger — proof + coverage

- `every_oracle_id_resolves_to_checked_in_snapshot`
- `every_guard_or_row_proof_resolves_to_default_suite_test`
- `lifted_row_executes_declared_proof`
- `every_manifest_row_has_non_placeholder_mechanism_and_executable_proof`
- `capability_rows_map_to_expected_query_fact_mechanisms`
- `block_rows_cannot_lift_without_complete_coverage` (a branch flipping rows `Lifted` without complete coverage fails CI)
- `ignored_test_row_table_holds_exactly_362_rows`
- `additional_proof_row_table_holds_exactly_7_rows`
- `semantic_query_name_mirror_matches_live_tag_set` (the `SemanticQueryName` mirror tracks the live `SemanticQueryKeyTag::ALL` variant set; carried in `U0.MANIFEST_SUBSTRATE`'s `BlockContractRow.required_guards`)
- the source-`#[ignore]` ↔ `Ignored`-rows ↔ `EXPECTED_TOTAL_IGNORED_COUNT` bijection/count guards
- `no_landed_typeinfo_block_has_live_ignored_rows`

## Landing protocol — git/CI (§11)

All of these run in the CI gate (§11.2):

- `landed_typeinfo_blocks_have_required_guards` (the §11.5 done-predicate guard part: a landed block's required / Critical-rule guards are present and passing)
- `no_vacuous_parent_u_block_landing` (§11.9)
- `zero_row_blocks_land_exactly_once` (§11.10 — exactly one merged target-branch commit carries each zero-row block's `Typeinfo-Block:` trailer)
- `typeinfo_parity_block_dag_is_acyclic_and_consumed_keys_and_mechanisms_are_prereqs` (§11.5)
- `typeinfo_block_lands_as_single_squashed_commit` (§11.11 — exactly one target-branch commit per block carrying the `Typeinfo-Block:` trailer; the tree-hash binding is dropped)
- `typeinfo_block_accept_requires_review_land_verdict` (§11.12 — PROCESS rule: the merge gate requires the three-reviewer LAND / NITs-only verdict, 1 Claude Code + 2 codex, enforced by branch-protection required approvals; the persisted-receipt binding is dropped)

The retired tracked-cursor transaction guards — `typeinfo_state_snapshots_are_locked_and_precondition_checked`,
`pending_typeinfo_transactions_reconciled_before_eligibility`, `block_cannot_enter_verifying_without_complete_coverage`
(renamed to `block_rows_cannot_lift_without_complete_coverage`, above),
`typeinfo_block_prereqs_derive_from_manifest_status`, `typeinfo_block_prereqs_ignore_verifying_blocks`,
`verifying_typeinfo_block_lease_blocks_dependents`, `workspace_gate_passes_before_typeinfo_block_acceptance` (now the CI gate
itself, §11.2), `landed_typeinfo_blocks_have_required_guards_and_workspace_gate` (the gate part folded into CI; renamed to
`landed_typeinfo_blocks_have_required_guards`, above), `cutover_state_landed_blocks_match_typeinfo_manifest`,
`cutover_state_typeinfo_writes_are_locked_and_cas`, `parallel_typeinfo_block_landing_preserves_all_tokens`,
`resume_rejects_stale_typeinfo_block_lease`, `cutover_state_typeinfo_namespace_isolated_from_legacy_cutover_tokens`, and
`legacy_cutover_completion_preserves_typeinfo_namespace_when_active` — are DELETED with the tracked
`.cutover-state.typeinfo_parity` cursor they pinned (§13). Git history + branch protection + `git revert` provide the
landing/accept/rollback boundary those guards used to police.

---

# Deliverables / legacy

- **Pin the oracle toolchain — DONE (U0-FINISH-A).** `package.json` pins `"@typescript/native-preview"` to the exact oracle
  version `7.0.0-dev.20260526.1` (resolved identically in `pnpm-lock.yaml`); the floating `"latest"` range was removed. `"latest"`
  was not a durable oracle contract, so the manifest dependency is now pinned to the exact oracle version.
- **The oracle row generator** (deterministic `OracleId`, checked-in normalized snapshots, feature/env-gated regeneration) is a
  required deliverable; the `tsgo`-execution-forbidden guard for runtime/default tests is a required deliverable.
- **The git/CI landing protocol** (§11) is the required execution-framework deliverable: the per-block branch discipline, the CI
  gate (the full Rust+JS workspace gate + the coverage/proof/required/DAG guards), the branch-protection three-reviewer LAND
  required approval, and the squash-merge `Typeinfo-Block:` trailer convention. There is NO tracked `.cutover-state.typeinfo_parity`
  namespace, NO two-namespace TOML schema, NO namespaced xtask, and NO crash-recovery machinery to deliver (§13) — git history +
  branch protection + `git revert` are the transaction log / accept gate / rollback.
- **The generated artifacts** — the `SemanticQueryKeySpec` table (§2.9), the proof registry + typed row-test wrapper (§10.3), and
  the U0 row-exact coverage table (§10.4) — are each produced by a dedicated `cargo run` generator and checked in (generated, not
  hand-maintained).

# Cross-reference / doc-update obligations

These doc-update obligations land as part of the work. The four against the
**unified plan** (`semantic-db-overhaul-unified-remaining-plan.md`) — owned by the
unified-plan integration step (the U0-time reconciliation of the unified plan with
this architecture) — have been **APPLIED**: the unified plan now indexes this
parent and all four children, carries the two-table-ledger U0 entry, requires all
362 `IgnoredTestRow`s `Lifted`, and registers the five added keys + the generalized
augmentation key. The cross-references are therefore **bidirectional and accurate**:
the unified plan links out to every subplan, and every subplan links back to the
unified plan and to this parent. The two remaining obligations below (the U6 doc
and the recovered-foundation doc) are owned by their own blocks and stay pending.
None of these edits is performed in this parent document or its four children — the
unified-plan edits are applied directly in `semantic-db-overhaul-unified-remaining-plan.md`:

- **Unified plan** (`semantic-db-overhaul-unified-remaining-plan.md`) — **DONE** (all four applied by the integration step):
  - **(a) Index the parent + all four children, and delete the stale `/tmp` reference — DONE.** The unified plan indexes the
    parent (`native-typeinfo-parity.md`) AND all four children (`-u2-reducers.md`, `native-flow-return.md`,
    `-cache-export-session.md`, `-adapters-final-lift.md`) — in its §A doc-set index table and at the owning
    `U0`/`U2`/`U3`/`U6`/`U8`/`U10`–`U15` blocks — and the stale `/tmp/verter-native-flow-return-coverage.md` coverage-path reference
    is deleted (the coverage table is §10.4 / §10.4.1 here + the in-repo manifest, never a scratch/temp artifact).
  - **(b) Replace the "no-op 4-field-schema confirm" U0 entry with the extended two-table ledger — DONE.** The unified plan's U0
    entry now describes the extended ledger this architecture requires: the two-table ledger (`IgnoredTestRow` extended schema +
    the separate coverage-only `AdditionalProofRow` table — §10.1), `IgnoreStatus` (binary `Ignored` / `Lifted`),
    `ProofRequirement`, the proof registry + row-test wrapper, the §10.4 / §10.4.1 row-exact coverage table, and the git/CI
    landing protocol (§§11–14) — branch per block → green CI → three-reviewer LAND → squash-merge with the `Typeinfo-Block:`
    trailer (no tracked `.cutover-state.typeinfo_parity` cursor; git=log, branch-protection=accept, revert=rollback).
  - **(c) Require ALL 362 `IgnoredTestRow`s lifted in U15 + the §9 terminal checklist (not a majority/fraction) — DONE.** The
    unified plan's U15 + §9 terminal checklist now require EVERY one of the 362 `IgnoredTestRow`s `Lifted` (zero parity ignores),
    with the ONLY permitted residual `#[ignore]`s being the registered Svelte/React STOP-gate files (which are not among the 362) —
    the terminal acceptance §10.5 / §13 / `all_typeinfo_parity_rows_lifted_except_stop_gates` define.
  - **(d) Register the new query surface — DONE.** The unified plan registers all five new query keys (`FlowReturn`,
    `ResolveClassSurface`, `ApparentType`, `TemplateLiteralReduce`, `ResolveCall`) in its U2/U6 sections; registers the GENERALIZED
    augmentation key (the seventh U2 variant is `ResolveDeclarationAugmentation { target: Module | Global, context:
    DeclarationAnalysisContext }`, not the former `ResolveModuleAugmentation`); and reconciles wording implying a uniform type-node
    query result to the typed
    `SemanticQueryValue` value-domain layer.
- **U6 doc** (`native-flow-return.md`) — update for the new query keys and the per-function `FunctionFlowGraph` + the
  `ReturnPathPeeker` graph demand planner (the two-frontier rule expressed as typed edge classes) that amend it.
- **Recovered foundation doc** (`semantic-type-graph-plan-recovered.md`) — amend the self-contradictory stale wording so the doc
  is not internally inconsistent: the stale `NoInfer` declaration-metadata wording → occurrence-local; the
  `decorators.rs — UnsupportedConstruct::Decorator + diagnostic projection` line → the class-surface ruling (§1.7); the §2.17 /
  §3.11 flow/contextual `TypeNode::FlowNarrowing` / `TypeNode::ContextualType` placements → `ProgramAnalysisGraph` payload entries;
  the stale `TypeNode::RelationProof` wording → `RelationPayload` / payload-side proof table (tag 28 retired/`reserved`); the stale
  JSX `ResolveJsxIntrinsicElement` / `ResolveJsxAttribute` / `TypeNode::JsxIntrinsicElement` wording → the existing-query JSX
  mechanism (§8); and the module/global augmentation placement — NO relocation lands. The proposed move of the
  `module_augmentation = 23` / `global_augmentation = 25` arms off `GraphTypeNode` was **rejected**: these arms REMAIN the wire
  home (the live proto carries both, and the landed `node_taxonomy_complete` guard pins them as valid 32-arm graph state — see
  §1.3). The in-process `SemanticQueryValue::DeclarationAnalysis` value domain (reached via the seventh U2 variant
  `ResolveDeclarationAugmentation`) is the value-side counterpart, NOT a wire relocation. The earlier-planned
  `module_augmentation_is_public_graph_state` guard and the `*_are_public_graph_state` declaration-surface guard family were
  never landed; the single exact-set `node_taxonomy_complete` taxonomy assertion is the live guard and treats 23/25 as valid.

---

## Resolved positions (carried forward)

- **Count:** 362 is binding. U0 rederives it with the manifest parser. 356/371 are stale; 384 is the raw line count.
- **U2 sizing:** no `U2.5`. U2 gets child blocks, but downstream U3/U8 stay blocked until the whole U2 parent is done.
- **New query count:** exactly five beyond the U2 seven — `FlowReturn`, `ResolveClassSurface`, `ApparentType`,
  `TemplateLiteralReduce`, `ResolveCall`. `ResolveCall` is first-class because call resolution is reusable semantic work with its
  own cache identity; its key normalizes closed args to type identities and keeps expression identities only for context-sensitive
  args, plus `call_kind` and `policy`.
- **`type_env_hash`:** behavioral parity is in scope for strict, exact optional, unchecked indexed access, and any option that
  affects a lifted fixture. Add mode-matrix tests.
- **Flow cycles:** flow gets its own cycle-id space, keyed on the FULL normalized `FlowReturnContext + ReturnProjectionDemand +
  FlowInputContext` (not the narrow tuple). Relation's guard is not enough. Sentinels are `ReturnOnly` and must not hide a real
  base-return contributor under a different demand.
- **`ApparentType` persistence:** query result memory-only; lib artifacts may persist through pure artifact caches.
- **Overload/generic fidelity:** calls use the first applicable overload; `ReturnType<typeof overloaded>` (and
  `ConstructorParameters`) use the last visible overload signature, not the implementation body. Pin against the TS7 oracle.
- **`ResolveClassSurface`:** keep it as a key — heritage substitution plus static/instance/member demand is a real cache
  identity, not just `Instantiate`.
- **Abstract class / abstract construct:** carry `AbstractConstruct` / `is_abstract` forward; matrix rows cover abstract-base
  inheritance, `InstanceType<abstract new ...>`, constructor-utility behavior on abstract, and rejecting concrete `new Abstract`.
- **TS7 decorators / auto-accessors:** not `UnsupportedConstruct`, not diagnostic projection — class/member-surface lowering
  (§1.7), with the four `ClassFeatures` rows + named guards and the recovered-doc amendment.
- **`NoInfer`:** occurrence-local — on `NoInfer` nodes / the signature parameter inference policy, not type-parameter declaration
  metadata; the recovered doc's stale wording is amended.
- **`satisfies`:** TS7 oracle-pinned (generated checked-in `OracleId` snapshots; default tests compare only to snapshots;
  regeneration feature/env-gated; `tsgo` forbidden in runtime/default tests except the gated drift generator). Exact behavior
  across object excess properties, readonly arrays, and contextual literal widening is oracle-pinned before lift.
- **Manifest proof model:** `proof: ProofRequirement` per row, not a mandatory per-row TS oracle plus per-row negative guard.
  Non-TS-oracle rows use `StructuralGuard` / `NegativeGuard` / `RowTestGuard`. No `NotTsOracleApplicable` escape hatch; every row
  resolves to an executable proof, and every `Lifted` row's own test consumes its declared proof via the generated row-test
  wrapper (not runtime cross-test token aggregation, which is unsound under unordered Rust tests).
- **Performance:** every hot reducer carries a typed budget (`RelationBudget`, `KeyspaceBudget`, apparent-type member-demand
  index, `CallResolutionBudget`, `FlowSliceBudget`) with `BudgetExceeded` non-admission, plus recursion-storm controls; each
  non-`FlowReturn` budget carries a named three-layer non-admission guard matching the `FlowReturn` rule.
