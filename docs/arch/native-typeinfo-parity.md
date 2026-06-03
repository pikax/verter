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

The end state is a full native checker-grade typeinfo engine — not a larger
flow-return patch. It has **one resolver**:
`SemanticQueryKey → ProjectSemanticDispatch::execute → SemanticGraphStore`. There
is no OXC query-time resolver, no tsserver/tsgo execution path, no projection
repair path, and no whole-body typecheck path. OXC is the syntax/lowering
front-end only.

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
| `docs/arch/native-typeinfo-parity.md` | **Parent architecture** (this file): engine architecture, capability map, query/fact authority, the per-block contract template, the two-table manifest ledger, the cutover/ledger transaction contract, the guards index |
| `docs/arch/native-typeinfo-parity-u2-reducers.md` | **U2** child blocks: reducer / relation / utility / indexed / mapped / template / class / enum / module / JSX foundations |
| `docs/arch/native-flow-return.md` | **U6** flow chapter: the demand-sliced `ReturnPathPeeker` (two-frontier model) and the flow IR |
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
type nodes. Declaration / environment-mutation facts live in
`DeclarationAnalysisGraph`, never as published type nodes.

Consequently the query engine's result domain is **typed**, not uniformly
`SemanticNodeId`: each `SemanticQueryKey` resolves to its correct value domain via
the typed `SemanticQueryValue` layer (see §3), so flow / contextual keys return
`ProgramAnalysisGraph` values and augmentation keys return
`DeclarationAnalysisGraph` values rather than type nodes, and no non-type value is
smuggled into `GraphTypeNode`.

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
`ambient_namespace` (24), `infer_node` (29), `enum_node` (30), `opaque` (31),
`cycle` (32). `merged_declaration` / `ambient_module` / `ambient_namespace` remain
because each is explicitly classified as a value-bearing namespace / object-type
surface whose members are queryable type values (the same object surface
`ResolveMergedDeclaration` / `ResolveAmbientNamespace` projects) — as distinct
from an augmentation **fact** that mutates the declaration environment. Their
place on the allowlist is conditional on that value-bearing classification, not on
their name.

Every non-type-value arm is relocated:

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
- **Module augmentation (tag 23) and global augmentation (tag 25)** →
  `DeclarationAnalysisGraph`. These are declaration / environment-mutation facts —
  they describe a change to the module / global declaration environment, not a
  published TS type value — so they are **not** on the type-value allowlist. They
  are relocated to a concrete declaration / environment **side surface**,
  `DeclarationAnalysisGraph` (a sibling of `ProgramAnalysisGraph`), carried on
  `TypeInfoGraphPayload.declaration_surfaces`. The arms are retired + `reserved`
  (tags `23`/`25` + names `module_augmentation`/`global_augmentation`).
- **Diagnostics and diagnostic directives** → `TypeInfoGraphPayload.diagnostics` /
  `TypeInfoGraphPayload.diagnostic_directives` (and off `SemanticTypeGraph`, §1.5).

Two mechanical guards close the class:

- **`graph_type_node_oneof_contains_only_type_value_arms`** — scans the proto's
  `GraphTypeNode` `oneof kind` and FAILS if any arm outside the type-value
  allowlist above (now **without** `module_augmentation` / `global_augmentation`)
  remains live. A new live arm not on the allowlist fails until it is either added
  to the allowlist as a genuine type value via a reviewed schema-version bump, or
  retired/`reserved` and relocated.
- **`graph_type_node_allowlist_arms_have_type_value_classification`** — asserts
  every arm ON the allowlist carries an explicit type-value classification and
  REJECTS any declaration / environment-mutation arm from the allowlist (module
  augmentation, global augmentation, and any other environment-mutation fact). An
  arm may be allowlisted only if explicitly classified as a published type value
  (including the value-bearing namespace/object classification that keeps
  `merged_declaration` / `ambient_module` / `ambient_namespace`), never merely
  because it currently appears in the oneof.

With both registered, no future non-type-value arm — type value or declaration/env
fact — needs case-by-case handling: the whole `GraphTypeNode`-purity class is
closed by one enumerating assertion plus one classification assertion.

The DTO end-state shape is:

```
TypeInfoGraphPayload {
    graph,                  // the GraphTypeNode type-values / topology surface
    program_analysis,       // ProgramAnalysisGraph { flow_narrowings, contextual_types }
    declaration_surfaces,   // DeclarationAnalysisGraph { module_augmentations, global_augmentations }
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
  flow narrowing, contextual type, the declaration/environment-mutation
  augmentation facts (module + global), `no_infer` type-parameter metadata, and any
  other relocated/retired concept is, wherever it appears, retired (its tag + name
  in the enclosing message's `reserved` list, never reused) and relocated to its
  end-state home (`ProgramAnalysisGraph` for program-analysis facts,
  `DeclarationAnalysisGraph` on `TypeInfoGraphPayload.declaration_surfaces` for
  declaration/environment-mutation facts, a `TypeInfoGraphPayload` side table such
  as `diagnostics` / `diagnostic_directives` / `relation_proofs`, a
  `RelationPayload`, or an occurrence-local node), or removed outright where it has
  no end-state value (`no_infer`).

Two guards close the wire-surface-purity class:

- **`typeinfo_wire_surface_has_no_retired_concept_fields`** — scans the whole proto
  against a denylist of retired-concept field/arm names (`flow_narrowing`,
  `contextual_type`, `relation_proof`, `diagnostics`/`diagnostic_directives` on
  type-value messages, `module_augmentation`, `global_augmentation`, `no_infer`,
  and every other relocated/retired concept) and asserts none remains live on any
  type-value message (`GraphTypeNode`, `SemanticTypeGraph`, `GraphTypeParameter`,
  and the other type-value messages). Each denylisted name must appear only in the
  enclosing message's `reserved` list (or, for embeddings, only behind a registered
  versioned downgrade encoder).
- **`all_public_semantic_type_graph_embeddings_are_payload_wrapped`** — the
  whole-class embedding guard (§1.5).

Together with `graph_type_node_oneof_contains_only_type_value_arms` and
`graph_type_node_allowlist_arms_have_type_value_classification`, these close the
entire public-wire-surface-purity class.

### 1.5 `SemanticTypeGraph` embeddings and the response landing

`SemanticTypeGraph` carries graph topology and type values **only**; diagnostics,
relation proofs, flow/contextual facts, and declaration/environment facts belong to
the payload, not the graph.

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

---

## 2. Query Keys

> **State note:** the query keys below are the **end state** to be built. Current
> `SemanticQueryKey` lacks the proposed U2 / future keys; this architecture does not
> imply they already exist.

### 2.1 U2 keeps seven variants

U2 keeps exactly seven variants: `ResolveMergedDeclaration`,
`ResolveDeclarationAugmentation`, `ResolveAmbientNamespace`, `ResolveOverloadSet`,
`ResolveEnum`, `FlowNarrowingAt`, `ContextualTypeAt`.

The seventh variant is the **generalized** augmentation key. The former
`ResolveModuleAugmentation` slot is broadened to `ResolveDeclarationAugmentation`
so module **and** global declaration-environment-mutation facts share **one**
concrete `SemanticQueryKey` identity, per the one-resolver rule. The slot count
stays seven and the added-key count stays exactly five — this is an existing-slot
generalization, not a sixth U2 variant.

### 2.2 `ResolveDeclarationAugmentation` key shape (declaration-environment identity)

The `target` is **env-free** — the env lives on `DeclarationAnalysisContext`, not
duplicated on the target. The `FileArtifactStore`
`AugmentationTargetKey { project_identity, resolve_env_hash, lib_env_hash, target }`
is DERIVED from `DeclarationAnalysisContext` at execution time, so the
augmentation-target env has exactly one source — the context — and cannot diverge
from the query-key env:

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
    parse_env_hash: ParseEnvHash,      // declaration analysis reads parser output (parse-option / parser-version sensitive)
    resolve_env_hash: ResolveEnvHash,  // name/import resolution (derives AugmentationTargetKey.resolve_env_hash)
    type_env_hash: TypeEnvHash,        // options affecting the declaration surface
    lib_env_hash: LibEnvHash,          // lib-declared global/ambient surfaces a global augmentation mutates
    project_identity: ProjectIdentity, // project isolation (derives AugmentationTargetKey.project_identity)
    // NO project_config_hash (R21), NO content hash / parse_stable_hash, NO fact_dep_signature (R6).
}
```

Both `target` variants map to exactly `SemanticQueryValue::DeclarationAnalysis`
(the `DeclarationAnalysisGraph` fact domain — module facts lower to
`module_augmentations`, global facts to `global_augmentations`), NEVER
`SemanticQueryValue::TypeNode` and NEVER a `GraphTypeNode` arm. `parse_env_hash` is
included (not excluded): a parse-option / parser-version change can alter the
analysed declaration surface, so the parse env is part of the cache identity.

Guards: **`global_augmentation_query_has_declaration_analysis_identity`** (global
declaration-environment-mutation facts are reachable through the generalized
`ResolveDeclarationAugmentation { target: Global(GlobalEnvScope), .. }` key — a
concrete `SemanticQueryKey` identity, not an identity-less side product — resolving
to `SemanticQueryValue::DeclarationAnalysis`); and
**`declaration_augmentation_target_is_env_free_env_comes_from_context`** (the query
`target` is env-free, the `AugmentationTargetKey` is derived from
`DeclarationAnalysisContext` at execution time, the derived target env equals the
context env, and no public constructor can create a target/context env mismatch).

### 2.3 The five added keys

Exactly five variants are added beyond the seven:

| Added key | Lands | Purpose |
|---|---:|---|
| `FlowReturn { function_slot, normalized_type_args, context, demand, input }` | U6 | Demand-sliced return/body flow query |
| `ResolveClassSurface { decl_slot, type_args, side, demand, context: ClassSurfaceContext }` | U2 | Instance/static heritage with generic substitution, member-demand aware |
| `ApparentType { base, demand, context: ApparentTypeContext }` | U2 | Primitive/array/constrained-generic apparent member lookup via lib facts |
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
that they are part of the cache identity.) Guards:
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
key and the cycle-re-entry key are the same normalized identity. Guards:
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
R21) plus the projection/substitution inputs. No `*Context` carries a content hash,
`parse_stable_hash`, or `fact_dep_signature` (these are query-identity keys —
version rooting lives on the cached value, not the key):

```rust
ResolveClassSurface {
    decl_slot: SemanticNodeId,
    type_args: Arc<[SemanticNodeId]>,     // heritage generic substitution (instantiated)
    side: ClassSurfaceSide,               // instance / static
    demand: MemberDemand,                 // member-demand aware (no whole-surface flatten)
    context: ClassSurfaceContext,
}

struct ClassSurfaceContext {
    parse_env_hash: ParseEnvHash,         // decorator / auto-accessor lowering is parse-env / parser-version sensitive
    resolve_env_hash: ResolveEnvHash,     // heritage/import resolution
    type_env_hash: TypeEnvHash,
    lib_env_hash: LibEnvHash,
    project_identity: ProjectIdentity,
    substitution: SubstitutionCanonicalHash,
    projection_reduction: ProjectionReductionContext,
}

ApparentType {
    base: SemanticNodeId,
    demand: MemberDemand,                 // member-demand REQUIRED on the hot path
    context: ApparentTypeContext,
}

struct ApparentTypeContext {
    lib_env_hash: LibEnvHash,             // apparent members come from lib wrapper interfaces
    type_env_hash: TypeEnvHash,
    project_identity: ProjectIdentity,
    substitution: SubstitutionCanonicalHash, // constrained-generic apparent lookup under substitution
    projection_reduction: ProjectionReductionContext,
}

TemplateLiteralReduce {
    pattern: SemanticNodeId,
    args: Arc<[SemanticNodeId]>,          // instantiated distribution arguments
    context: TemplateLiteralReduceContext,
}

struct TemplateLiteralReduceContext {
    resolve_env_hash: ResolveEnvHash,     // name/intrinsic resolution
    type_env_hash: TypeEnvHash,
    lib_env_hash: LibEnvHash,             // intrinsic (Uppercase/Lowercase/Capitalize/Uncapitalize) facts
    project_identity: ProjectIdentity,
    substitution: SubstitutionCanonicalHash, // substitution feeding `infer` splitting / distribution
    projection_reduction: ProjectionReductionContext,
}
```

`ApparentType` omits `parse_env_hash` (apparent-member lookup does not depend on the
consumer file's parse env); `ResolveClassSurface` carries it (class-surface lowering
owns decorators / auto-accessors, which are parse-env sensitive). Guards:
**`resolve_class_surface_key_covers_side_demand_type_args_and_context`** (asserting
`ClassSurfaceContext` carries `parse_env_hash` and none of the R21/R6 forbidden
fields), **`apparent_type_key_covers_lib_env_demand_and_context`**, and
**`template_literal_reduce_key_covers_context`**.

### 2.7 `Relate` key shape (existing-key upgrade, full relation identity)

`Relate` is the sole assignability authority (§4) and carries the same
cache-soundness discipline. The current `Relate` is only `{ source, target }`, which
is not a sound cache identity: the same pair relates differently under a different
relation kind, overload-selection / excess-check policy, source freshness, or
env/substitution context. This is an explicit existing-key upgrade (not a new key —
the added-key count stays five):

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
identical. `Relate` therefore carries an `inference_context: Option<InferenceContextKey>`
— `Some` (part of identity) for binding-producing relations, `None` for pure
non-binding assignability checks:

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
§1.2). The relation/inference engine is the **sole** owner of this binding work; it
does not implement a parallel matcher (§4).

The `RelationBudget` pair memo (§6) is keyed by **this full** `Relate` identity
(`source`, `target`, `relation`, `policy`, `source_freshness`, `inference_context`,
`context`), NOT the bare `(source, target)` pair — the memo cannot false-hit across
relation-kind / policy / freshness / inference-context / env differences. Under this
key, `Relate` produces a public `RelationPayload` (outcome / bindings / proof +
typed `BudgetExceeded` non-admission), not a bare tri-state `RelationResult`; its
value domain is `SemanticQueryValue::Relation(RelationPayload)`, and `RelationPayload`
is exactly where public `relate` returns its proof off the type-values surface.

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
ResolveAmbientNamespace  { namespace_slot, type_args, demand: MemberDemand, context: AmbientNamespaceContext }
ResolveOverloadSet       { callee, type_args, context: OverloadSetContext }
ResolveEnum              { enum_slot, context: EnumContext }
FlowNarrowingAt          { point: ProgramPointId, context: ProgramAnalysisContext }
ContextualTypeAt         { point: ProgramPointId, context: ProgramAnalysisContext }
```

`MergedDeclarationContext` and `AmbientNamespaceContext` carry the split env
(including `parse_env_hash` — the skeleton is parse-env sensitive) + substitution +
projection/reduction. `OverloadSetContext` and `EnumContext` omit `parse_env_hash`
(they read already-lowered signatures / enum members); `EnumContext` carries no
substitution axis (an enum declaration is not generic). `ProgramAnalysisContext` is
the program-analysis context covering env + flow + contextual + substitution:

```rust
struct ProgramAnalysisContext {
    parse_env_hash: ParseEnvHash,           // flow/contextual analysis reads the parsed body skeleton
    resolve_env_hash: ResolveEnvHash,
    type_env_hash: TypeEnvHash,             // strict / exact-optional / index-access options that change narrowing
    lib_env_hash: LibEnvHash,
    project_identity: ProjectIdentity,
    substitution: SubstitutionCanonicalHash, // same hash as the flow/call/relation keys
    flow_narrowing: FlowNarrowingKey,        // the flow-in facts in scope at the queried point
    contextual_typing: ContextualTypingKey,  // the contextual target / expected-type propagation at the point
    projection_reduction: ProjectionReductionContext,
}
```

`FlowNarrowingKey` and `ContextualTypingKey` are the same axis identities the
`ResolveCall` context-sensitive-arg identity and `CallResolutionContext` carry —
there is one flow/narrowing axis space and one contextual-typing axis space shared
across the call, flow-return, and program-analysis keys.

Per-key no-cross-context-warm-hit guards (one per remaining key, same discipline as
the call/relate guards):
**`resolve_merged_declaration_same_site_different_env_or_context_do_not_warm_hit`**,
**`resolve_ambient_namespace_same_site_different_env_or_context_do_not_warm_hit`**,
**`resolve_overload_set_same_site_different_env_or_context_do_not_warm_hit`**,
**`resolve_enum_same_site_different_env_or_context_do_not_warm_hit`**,
**`flow_narrowing_at_same_point_different_env_flow_or_substitution_do_not_warm_hit`**,
**`contextual_type_at_same_point_different_env_contextual_or_substitution_do_not_warm_hit`**,
and **`declaration_augmentation_key_same_site_different_env_or_context_do_not_warm_hit`**
(the named proof that `DeclarationAnalysisContext` includes `parse_env_hash`).

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
| `ResolveDecl` | live | `ResolveDeclContext` (split env + projection-reduction) | `TypeNode` | `resolve_decl_same_site_different_env_or_context_do_not_warm_hit` | singleflight; `ReturnOnly` on overflow/cancel |
| `Instantiate` | live | `ProjectionReductionContext` + split env (content-free `DeclKey` base) | `TypeNode` | `instantiate_same_base_different_env_or_context_do_not_warm_hit` | singleflight; `ReturnOnly` on overflow/cancel/budget |
| `ProjectMember` | live (canonicalised to length-1 `ProjectPath`) | `ProjectionReductionContext` + split env | `TypeNode` | covered by `ProjectPath` guard | singleflight |
| `IndexedAccess` | live | `ProjectionReductionContext` + split env | `TypeNode` | `indexed_access_same_base_different_env_or_context_do_not_warm_hit` | `KeyspaceBudget` (union-key distribution); `ReturnOnly` on overflow |
| `KeyOf` | live | `ProjectionReductionContext` + split env | `TypeNode` | `key_of_same_base_different_env_or_context_do_not_warm_hit` | `KeyspaceBudget`; `ReturnOnly` on overflow |
| `MappedType` | live | `ProjectionReductionContext` + split env | `TypeNode` | `mapped_type_same_source_different_env_or_context_do_not_warm_hit` | `KeyspaceBudget` (keyspace explosion); `ReturnOnly` on overflow |
| `Conditional` | live | `ProjectionReductionContext` + split env (check/extends/branches + `distributive`) | `TypeNode` | `conditional_same_nodes_different_env_or_context_do_not_warm_hit` | consumes `Relate` bindings; `ReturnOnly` on budget/cycle |
| `TypeOf` | live | `ProjectionReductionContext` + split env (`value_root`) | `TypeNode` | `type_of_same_value_root_different_env_or_context_do_not_warm_hit` | singleflight; `ReturnOnly` on overflow |
| `NormalizeUnion` | live | `ProjectionReductionContext` + split env (members) | `TypeNode` | `normalize_union_same_members_different_env_or_context_do_not_warm_hit` | `KeyspaceBudget` on large unions; `ReturnOnly` on overflow |
| `NormalizeIntersection` | live | `ProjectionReductionContext` + split env (members) | `TypeNode` | `normalize_intersection_same_members_different_env_or_context_do_not_warm_hit` | `KeyspaceBudget`; `ReturnOnly` on overflow |
| `ProjectPath` | live | `ProjectionReductionContext` + split env (base + path) | `TypeNode` | `project_path_same_base_path_different_env_or_context_do_not_warm_hit` | singleflight; `ReturnOnly` on budget/cycle |
| `ResolvedNamedType` | live (read-dominant macro artifact; `execute` returns `Miss` until written) | `HostResolvedNamedTypeKey` (own env/identity) | `TypeNode` | `resolved_named_type_key_identity_is_env_scoped` | read-only memo; writes via `NamedTypeCache` adapter |
| `Relate` | live (existing-key UPGRADE — `{source,target}` → full identity) | `RelationContext` + `InferenceContextKey` (binding-producing) | `Relation(RelationPayload)` | `relate_same_nodes_different_relation_kind_policy_or_env_do_not_warm_hit` + `relate_same_nodes_different_inference_context_do_not_warm_hit` | `RelationBudget`; coinductive-SCC discharge; `ReturnOnly` on `Unknown`/cancel/`BudgetExceeded` |
| `ResolveMacroPayload` | live (Vue-macro payload key, distinct from the typeinfo macro story) | `MacroPayloadContext` (content-free `DeclKey` owner; split env + `mode`) | `TypeNode` | `resolve_macro_payload_same_owner_different_env_or_context_do_not_warm_hit` | singleflight; `ReturnOnly` on overflow/cancel |
| `ResolveMergedDeclaration` | live (added — U2) | `MergedDeclarationContext` | `TypeNode` | `resolve_merged_declaration_same_site_different_env_or_context_do_not_warm_hit` | singleflight; `ReturnOnly` on budget/cycle |
| `ResolveDeclarationAugmentation` | live (added — U2; generalizes `ResolveModuleAugmentation`) | `DeclarationAnalysisContext` (incl. `parse_env_hash`) | `DeclarationAnalysis(DeclarationAnalysisValue)` | `declaration_augmentation_key_same_site_different_env_or_context_do_not_warm_hit` | singleflight; `ReturnOnly` on overflow/cancel |
| `ResolveAmbientNamespace` | live (added — U2) | `AmbientNamespaceContext` | `TypeNode` | `resolve_ambient_namespace_same_site_different_env_or_context_do_not_warm_hit` | singleflight; `ReturnOnly` on budget/cycle |
| `ResolveOverloadSet` | live (added — U2) | `OverloadSetContext` | `OverloadSet(Arc<[SignatureRef]>)` | `resolve_overload_set_same_site_different_env_or_context_do_not_warm_hit` | singleflight; `ReturnOnly` on overflow |
| `ResolveEnum` | live (added — U2) | `EnumContext` | `TypeNode` | `resolve_enum_same_site_different_env_or_context_do_not_warm_hit` | singleflight; `ReturnOnly` on overflow |
| `FlowNarrowingAt` | live (added — U2) | `ProgramAnalysisContext` (env + flow + contextual + substitution) | `ProgramAnalysis(ProgramAnalysisValue)` | `flow_narrowing_at_same_point_different_env_flow_or_substitution_do_not_warm_hit` | `FlowSliceBudget`; `ReturnOnly` on budget/cycle |
| `ContextualTypeAt` | live (added — U2) | `ProgramAnalysisContext` | `ProgramAnalysis(ProgramAnalysisValue)` | `contextual_type_at_same_point_different_env_contextual_or_substitution_do_not_warm_hit` | `FlowSliceBudget`; `ReturnOnly` on budget/cycle |
| `FlowReturn` | live (added — U6) | `FlowReturnContext` + `demand`(`ReturnProjectionDemand`) + `input`(`FlowInputContext`) | `FlowReturn(Arc<FlowReturnResult>)` | `flow_return_key_covers_input_context_and_projection_demand` | `FlowSliceBudget`; flow-cycle sentinel `ReturnOnly` |
| `ResolveClassSurface` | live (added — U2) | `ClassSurfaceContext` (incl. `parse_env_hash`) | `TypeNode` | `resolve_class_surface_key_covers_side_demand_type_args_and_context` | singleflight; `ReturnOnly` on budget |
| `ApparentType` | live (added — U2) | `ApparentTypeContext` | `TypeNode` | `apparent_type_key_covers_lib_env_demand_and_context` | apparent-type member-demand budget; whole-lib materialization is `BudgetExceeded`/`ReturnOnly` |
| `TemplateLiteralReduce` | live (added — U2) | `TemplateLiteralReduceContext` | `TypeNode` | `template_literal_reduce_key_covers_context` | `KeyspaceBudget`; `ReturnOnly` on overflow |
| `ResolveCall` | live (added — U6) | `CallResolutionContext` (+ `ContextSensitiveExprKey` per arg) | `ResolvedCall(Arc<ResolvedCallResult>)` | `resolve_call_same_expr_different_flow_or_substitution_does_not_warm_hit` | `CallResolutionBudget`; `ReturnOnly` on `BudgetExceeded` |

No current variant is omitted: every live variant appears with `live` + spec; any
variant intended to retire/rename carries `retired` (+ `reserved` on its wire
surface) or `renamed` instead — there is no fourth state.

The class is closed by one meta-guard asserting enum/table EQUALITY:

- **`semantic_query_key_spec_table_equals_enum`** — asserts the generated table's
  variant set EXACTLY EQUALS the closed `SemanticQueryKey` enum's variant set (no
  omissions, no extras — every enum variant has exactly one row, every row names a
  real enum variant or is explicitly `retired`/`renamed`). For every `live` row it
  asserts (1) an explicit named R21/R6-clean context shape (split env hashes +
  applicable substitution / flow / contextual axes; none of the forbidden fields),
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
  appear ONLY in the same block that adds its enum variant. The `(added — U2)` rows
  land at `U2.QUERY_VALUE_DOMAIN`; the `(added — U6)` rows (`FlowReturn`,
  `ResolveCall`) land their enum variant AND this spec row AND their dispatch
  behavior together at U6 — never a row ahead of its variant. The guard is green
  after EVERY block, never red in the gap between U2 and U6.

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
    DeclarationAnalysis(DeclarationAnalysisValue), // ResolveDeclarationAugmentation (Module + Global) — DeclarationAnalysisGraph facts
    OverloadSet(Arc<[SignatureRef]>),        // ResolveOverloadSet — ordered overload signatures
    FlowReturn(Arc<FlowReturnResult>),       // FlowReturn — demand-sliced return/body flow result
    ResolvedCall(Arc<ResolvedCallResult>),   // ResolveCall — reusable call-resolution result
    Relation(RelationPayload),               // Relate — public relation payload (outcome / bindings / proof + typed BudgetExceeded)
}
```

The `DeclarationAnalysis` arm is the value-domain counterpart of the wire-side
`DeclarationAnalysisGraph` relocation: once `module_augmentation` /
`global_augmentation` leave the `GraphTypeNode` type-value surface, the query layer
must not reintroduce them as `TypeNode` results. `ResolveDeclarationAugmentation`
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

`InferBind` is **relation-owned**. Add `InferTargetPattern::{ObjectProperty,
TupleHead, TupleTail, TupleInit, TupleLast, ParamTuple, ReturnPosition,
TemplatePart}`. Conditional reduction consumes relation bindings; it does not
implement its own matcher. Binding-producing relation work carries the
`InferenceContextKey` so the same pair under a different inference setup does not
warm-hit.

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

### 4.2 Conditional / mapped / index / template reducers

- **Conditional:** `any` evaluates both branches and unions; distributive `never`
  collapses to `never`; open conditionals distribute the remaining `ProjectPath` into
  both branches; closed conditionals reduce immediately.
- **Mapped / index / template:** mapped `-?` strips ONLY the optional-property-origin
  `undefined`; key remap runs through `TemplateLiteralReduce`; `as never` drops keys;
  indexed access distributes union keys, honors string/number/symbol index precedence,
  and keeps intermediate hops in `Navigate`.
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

### 4.3 `satisfies` — TS7 oracle-pinned

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

### 4.4 Apparent types

`ApparentType` resolves primitive and array members through lib-declared wrapper
interfaces keyed by `lib_env_hash`. The query result is memory-only; reusable lib
artifacts may persist through `FileArtifactStore`, but query nodes do not persist
under U4.

---

## 5. Flow Architecture (demand-sliced)

The flow engine is demand-sliced; a full lowered body is not good enough. The
detailed U6 chapter lives in `docs/arch/native-flow-return.md`; the cross-cutting
contract is:

`FunctionBodySkeleton` (in/under `IndexedReady`): arena-free, shallow statement /
control skeleton, return-site index, lexical binding index, assignment/kill
summaries, no type lowering.

`ReturnPathPeeker` is a path-contribution algorithm over a shallow
`FunctionBodySkeleton`. Given `ReturnProjectionDemand { path, terminal_mode }`, it
builds a `ReturnSlicePlan` using **two distinct frontiers** so demand-slicing stays
sound under effects:

- **Value-provider frontier:** computes which sources provide the demanded value. For
  each return site and demanded path `P`, compute value contributors by
  reverse-walking only the returned expression, reaching definitions, path-affecting
  assignments, branch predicates, and call effects. This frontier MAY stop at a
  definite-present write for `P[0]`.
- **Effect frontier:** stays open even past a definite-present write. It scans earlier
  evaluated expressions for assignments, assertion calls, known local closure
  effects, abrupt completion, and control-flow effects that can affect the selected
  value expressions. A sibling property whose value type cannot be lowered must still
  contribute its effect summary when that effect changes a binding read by the
  selected path. The effect frontier MUST also sweep two effect classes the
  value-provider frontier skips because their value is overwritten later — evaluation
  effects survive a definite write even though value materialization does not:
  - **Computed property-name expressions.** A computed key `[expr]: v` evaluates
    `expr` for its side effects regardless of whether that property's value is later
    overwritten or is not the demanded path. If `expr` assigns, narrows, or calls into
    a binding the selected path reads, the effect frontier includes that computed-key
    evaluation effect — even when the property is non-contributing for value. Only its
    evaluation effect is taken; the selected value is not materialized from it.
  - **Spread / `Object.assign` evaluation effects.** A spread `...src` or an
    `Object.assign(target, src)` evaluates `src` (and reads its enumerable own keys)
    for side effects even when a later definite write to `P[0]` makes it
    non-contributing for the demanded value. If evaluating `src` affects a binding read
    by the selected path, the effect frontier carries that evaluation effect past the
    definite write; only the spread's value contribution is skipped.

The two-frontier rule is required for soundness. Demanding `["b"]` in
`return { a: (x = "s"), b: x.toUpperCase() }` must not lower sibling `a`'s value type
but MUST include `a`'s effect summary, because `a`'s initializer assigns `x` and `x`
is read by the selected `b`.

Contribution-scan rules: object literals and `Object.assign` scan write sources
right-to-left for `P[0]` (the value-provider frontier stops only at a definite-present
write; optional/unknown writes are included as `ProjectPath(source, P)`); known
unrelated properties are skipped for value purposes by syntactic key footprint, not
value resolution, but the effect frontier still sweeps them; `return { ...spread, b }`
with demand `["b"]` makes `spread` and sibling `a` non-contributing for value (no
type resolution), while the effect frontier still inspects them (including the
spread/`Object.assign` source's evaluation effect); `const r = { a, b }; return r`
inlines the last reaching definition if `r` is unescaped/unmutated, else includes only
writes that may affect `P` and returns a typed degraded path result on unknown
mutation — never lowers siblings; conditional returns run this per return site then
join selected path results with the branch predicates needed for narrowing.

Peeker guards: **`flow_return_path_peeker_spread_override_skips_overwritten_sibling`**,
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

`FlowSliceHashNode` hashes only the selected return/control/binding slice; a full-body
hash is allowed only for a true whole-return request and is rejected for
member-projection requests. `FlowSliceLoweredBodyNode` lowers only the slice plan into
`FlowSliceIR`. `FlowSliceIR` carries `FlowStmt`, `FlowExpr`, `FlowSlotId`, `FlowPath`,
`FlowFrame`, `NarrowingFact`, `AliasCorrelation`, `FlowEffect`, `ReturnAccumulator`,
`LoopSummary`.

**Acceptance example (non-materialization).**

```ts
function myType() { const a = new Mytype(); const b = 1; return { a, b } }
type Foo = ReturnType<typeof myType>['b']
```

Resolution must be: `IndexedAccess` threads demand `['b']` into `ReturnType`;
`ReturnType` produces/uses a lazy flow-return root; `ProjectPath` calls `FlowReturn`
with path `['b']`; `ReturnPathPeeker` selects only the `b` property and `const b = 1`;
it does not lower `a`, does not resolve `new Mytype()`, does not load `Mytype`, and
does not walk sibling members. The returned literal `1` widens to `number` at
return-position. A guard asserts no `ResolveClassSurface`, `TypeOf`, constructor,
import, or route fact for `Mytype` appears.

**Mutual recursion + flow cycle space.** Flow is mutually recursive with type
reduction: `ReturnType` calls `FlowReturn`; flow narrowing calls `Relate`; call
solving routes through `ResolveCall` / `ResolveOverloadSet` and `Relate`; return
member projection calls `ProjectPath`/`ProjectMember`; those may re-enter `FlowReturn`.
This needs a **separate flow cycle-id space**. Re-entry is keyed on the FULL
normalized `FlowReturnContext + ReturnProjectionDemand + FlowInputContext`, not a
narrow tuple — the narrow `(function_slot, substitution_env_hash, projection_path,
terminal_mode, flow_policy)` form can terminate but can also mask a real result with a
sentinel under a different demand. Same-context recursion returns a stable flow cycle
sentinel; it never self-awaits. Guards:
**`flow_cycle_sentinel_is_never_admitted_as_cache_entry`** (the sentinel is
`ReturnOnly`) and **`flow_cycle_sentinel_does_not_hide_real_base_return_contributor`**
(a sentinel for one normalized context/demand/input is never served to a re-entry
under a different one).

**Demand-aware cache identity.** `FlowReturnContext` includes the five env hashes,
substitution canonical hash, `ProjectionReductionContext`, and `FlowPolicy`. It does
NOT carry `ReturnProjectionDemand` or `FlowInputContext` — those are the sibling
`demand` / `input` key fields of the canonical struct, so the full cache identity is
`FlowReturnContext + ReturnProjectionDemand + FlowInputContext` with no field
duplicated. A cached flow result carries `satisfied_projection`: `FlowReturn(path=['b'],
Expanded)` cannot satisfy a whole return or `['a']`; a broader result backfills a
narrower entry only when the broader computation actually materialised that narrower
path; `Skeleton` remains isolated. The flow fact signature includes
`FlowSlice { function_slot, projection_path, slice_hash, selected_binding_ids,
selected_effect_ids, selected_control_region_ids, closure_summary_ids }`, plus
`MemberPresence`, `Member`, `RouteGeneration`, `ExportSurface`, `ModuleAugmentation`,
`AmbientGlobal`, `LibIntrinsic`, `TypeEnvOptions`, and project-generation facts as
read. The extra `FlowSlice` fields beyond `selected_binding_ids` are required because
effect-only changes (an earlier sibling's assignment, an assertion call, a closure
write summary, a control-flow region) must invalidate a cached slice even when no
selected binding's identity changed. Budget, overflow, cycle, cancellation, or partial
slice results are `ReturnOnly`.

**Shallow-by-default (post-peeker).** The shallow-by-default target is valid only after
the `ReturnPathPeeker` correction. Path laziness must hold for cross-file return types
(`ReturnType` creates a lazy flow-return root; projected paths call `FlowReturn(path)`),
imported class methods (`ResolveClassSurface` accepts member demand and resolves only
that method/signature), nested `ReturnType<typeof f>["x"]["y"]` (intermediates are
`Navigate`, terminal is caller mode), spread/`Object.assign` (right-to-left scan;
unknown spread contributes only `ProjectPath(spread, P)`), and generic returns
(`FlowReturn` key includes the normalized substitution hash; open generics keep
conditional/path shells instead of whole-body lowering).

`Skeleton` is the BFS / generic-helper traversal mode used by
`Instantiate { args: [], body_mode: Skeleton }` — unbound type parameters become
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
original 363 and carry their own coverage-table rows + `ProofRequirement`s, but are
excluded from the ignored-count + bijection guards, so the binding 363 `IgnoredTestRow`
total is unchanged). Where a submatrix fixture corresponds to an existing ignored
`JsxResolution` row it stays in that `IgnoredTestRow` rather than being duplicated.

---

# PART 2 — The Execution Framework

This part owns the **cross-cutting** execution framework: the per-block contract
template, the two-table manifest ledger, the crash-safe cutover/ledger transaction
contract, the no-skip guarantee, the resume protocol, and the `.cutover-state`
`[typeinfo_parity]` namespace. The per-U-block block instances live in the subplans;
this is the shared machinery they all use.

There is no new top-level phase ladder — subplans, not stages. A block cannot start
until all prerequisite block IDs are done, and "done" is derived mechanically from the
manifest and guard suite, not from prose.

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
- **`Commit cadence:`** is the git-history discipline (§11.11): a WIP series during the
  block (high WIP count expected; **no per-commit gate** — the WIP exemption applies, so
  intermediate `todo!()` / placeholder states are permitted), squashed to EXACTLY ONE
  land commit at the `accept` done-edge. Mainline receives one commit per landed block.
- **`Review gate:`** is the LAND authorization (§11.12): the three-reviewer panel — 1
  Claude Code + 2 codex, all bad-mood, holding the best-architecture-no-compromises /
  breaking-changes-allowed mandate — must all return LAND (or residuals are NITs-only)
  before `accept`, bound to the gated input-hash pair (so a stale review never
  authorizes the land). This is a SECOND `accept` precondition alongside the workspace
  gate, not a done-predicate part.

`Commit cadence:` and `Review gate:` are **PARENT-UNIFORM**: their value is IDENTICAL
for every block — the §11.11 one-squashed-land-commit discipline and the §11.12
three-reviewer LAND gate — so they are OWNED at the parent (§11.11 + §11.12) and a
block contract need NOT restate them per block; a subplan states them ONCE as the
uniform discipline for every block in it. (Contrast the block-SPECIFIC quartet
`Context:` / `Changes:` / `Legacy deletions:` / `Critical-rule guards:`, which carry
per-block data and so ARE restated in every block contract.)

`TYPEINFO_PARITY_BLOCKS: &[BlockContractRow]` lives in the same manifest module
(`crates/verter_session/tests/typeinfo_ignored_test_manifest.rs`). It contains block ID,
U-block, prereqs, subplan path, required guards, and verification command labels.

## 10. The two-table manifest ledger

The manifest is extended **in place** (in
`crates/verter_session/tests/typeinfo_ignored_test_manifest.rs`); no second ledger file
is created. Both tables live in this one module.

> **State note:** the schema below is the **end state** to be built. The current manifest
> has only file/function/substrate/unblocker fields — no block/status/proof/guard fields.
> This architecture does not imply the extended schema already exists.

### 10.1 Two SEPARATE tables: the binding 363 vs additional coverage

The binding manifest total is EXACTLY 363 ignored rows. Full-parity coverage adds a
CLOSED set of exactly 7 coverage-only `AdditionalProofRow`s = the 6 JSX no-new-key
submatrix rows (owned by `U2.JSX_FOUNDATIONS`) + the 1 mapped companion
`mapped_modifier_minus_optional_preserves_explicit_undefined_on_required_property`
(owned by `U2.MAPPED_TEMPLATE`). The set is closed, not growing. Those two facts are
incoherent if the binding 363 and the additional fixtures share ONE table and ONE
`EXPECTED_TOTAL_IGNORED_COUNT` — additional rows would either break the exact 363
count/bijection or be untracked. The ledger therefore SPLITS into two tables:
`IgnoredTestRow` holds EXACTLY the 363 ignored test-site rows (count-guarded at 363,
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
struct IgnoredTestRow {              // EXACTLY the 363 ignored test-site rows (count-guarded, bijective with source #[ignore]s)
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
    // NO `status: IgnoreStatus`: not an ignored test site, so no lifecycle and never in
    // EXPECTED_TOTAL_IGNORED_COUNT or the bijection.
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
    Verifying { block_id: TypeInfoParityBlockId, lease_id: LeaseId },
    Lifted { block_id: TypeInfoParityBlockId },
}
```

### 10.2 `ProofRequirement` — every row resolves to an executable proof

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
the row is wired to the architecture MECHANISM intended to lift it. To make 363-row
full-parity completeness MECHANICAL, U0 GENERATES and CHECKS a row-exact coverage table that
maps EVERY manifest row through its full mechanism chain:

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

- **Generated, checked-in.** The coverage table is generated from the manifest by a
  dedicated `cargo run` generator and checked in (same discipline as the oracle rows, the
  proof registry, and the row-test wrapper). One coverage row per manifest row; a row whose
  `mechanism_id` cannot be resolved (or resolves to a placeholder) fails generation. The
  authoritative `row → block_id` projection of this table over all 363 `IgnoredTestRow`s is
  enumerated in full in §10.4.1; each subplan's `Exact test rows lifted` list is the
  per-block slice of that partition.
- **Completeness is DEFINED by this table over the 363 `IgnoredTestRow`s PLUS every
  `AdditionalProofRow`.** Full-parity completeness IS this row-exact coverage table being
  complete and non-placeholder over all rows in both tables. A "missing TS rule" is exactly
  one of two mechanically-detectable things: (a) a row (in either table) whose `mechanism_id`
  the guard REJECTS as a placeholder (an unimplemented / `todo` / `unknown` mechanism — a
  genuine gap), or (b) a genuinely NEW fixture not among the 363, handled by the SEPARATE
  `AdditionalProofRow` table. There is no third "we think it's covered" state.
- **Gate: a block cannot enter `Verifying` until its rows' coverage is complete +
  non-placeholder.** The coverage table is a PRECONDITION on `prepare-verify`: a block may
  transition its rows to `Verifying` only after every one of its rows has a non-placeholder
  `mechanism_id`, an executable `ProofRequirement`, and a `semantic_queries`/facts mapping
  consistent with its capability — BEFORE `prepare-verify` strips any `#[ignore]`.

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

The gate is enforced by **`block_cannot_enter_verifying_without_complete_coverage`** (in the
two-phase guard set): `prepare-verify` is rejected for any block whose rows are not all
complete + non-placeholder.

This closes the completeness class: 363-row full parity is not probed by sampling — it is the
mechanical property "the generated coverage table is complete and non-placeholder over every
manifest row in both tables, each mapped to the expected capability mechanism, before any
block verifies" — while the binding 363 total stays a separate, exact count/bijection over the
`IgnoredTestRow` table alone.

### 10.4.1 The authoritative U0 row → `block_id` partition (all 363 rows)

This is the **authoritative, exhaustive** `row → block_id` map the U0 coverage-table
generator emits and `every_manifest_row_has_non_placeholder_mechanism_and_executable_proof`
checks. It is a total function over the 363 `IgnoredTestRow`s: **every** row appears exactly
once under exactly one owning `block_id`, no row is owned by two blocks, and the union is
exactly the 363 manifest rows. Each subplan's `Exact test rows lifted` list is the projection
of this partition onto that block — the two must agree row-for-row, enforced by
`capability_rows_map_to_expected_query_fact_mechanisms` (each row's owning block matches its
capability's expected mechanism) and the bijection/count guards (§10.5).

The owning `block_id` for each row is its **dominant mechanism** per the Capability Map
(§Capability Map) and the row-level-split notes in the subplans: a row's substrate maps it to
a capability, and the capability's row-level mechanism (a `mechanism_id` such as
`Relate.coinductive_scc`, `IndexedAccess.union_distribution`, `ReturnPathPeeker.two_frontier`,
`ResolveDeclarationAugmentation.declaration_analysis`, `ResolveAmbientNamespace.jsx_namespace`)
fixes the single owning block. The per-block counts (summing to 363) are:

| Owning `block_id` | Rows | Owning `block_id` | Rows |
|---|---:|---|---:|
| `U2.RELATION_INFER` | 20 | `U6.NARROWING` | 104 |
| `U2.UTILITIES` | 42 | `U6.PREDICATE_ASSERTION` | 3 |
| `U2.INDEXED_ACCESS` | 16 | `U6.CALL_RESOLVE` | 19 |
| `U2.MAPPED_TEMPLATE` | 16 | `U6.CONTEXTUAL_CALLBACK` | 15 |
| `U2.CLASS_SURFACES` | 52 | `U6.VALUE_INFERENCE` | 1 |
| `U2.ENUMS` | 7 | `U6.ASYNC_GENERATOR` | 1 |
| `U2.MODULE_AUGMENTATION` | 11 | `U6.CROSS_FILE` | 6 |
| `U2.JSX_FOUNDATIONS` | 9 | `U6.LOOP_CLOSURE` | 3 |
| `U6.FLOW_RETURN_SUBSTRATE` | 7 | `U3.CACHE_FACT_MODEL` | 3 |
| `U10.RESULT_DB` | 13 | `U11.PUBLIC_RELATION_SESSION` | 9 |
| `U14.MACRO_ADAPTER` | 1 | `U15.FINAL_LIFT` | 5 |

Sum = 363. Blocks not in this table (`U0.MANIFEST_SUBSTRATE`, `U2.QUERY_VALUE_DOMAIN`,
`U8.WIRE_SURFACE_CLOSURE`, `U12.EXPORTER`, `U13.PROJECTION`) own **zero** `IgnoredTestRow`s —
they build substrate (ledger / keys / wire / exporter / projection) the owning blocks lift
their rows through; their `Exact test rows lifted` is explicitly `none`.

The complete partition (each entry `file::function — substrate`):
<!-- BEGIN U0 row→block coverage table (363 rows). Generated by the U0 coverage-table generator from
     crates/verter_session/tests/manifest_data/typeinfo_ignored_test_manifest_rows.rs; do not hand-edit. -->

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

**`U2.MODULE_AUGMENTATION`** (11 rows):

- `modern_ts_features.rs::import_attribute_simulated_resolves_imported_json_shape` — `ModernTsFeatures`
- `modern_ts_features.rs::import_attribute_simulated_string_literal_indexed_member` — `ModernTsFeatures`
- `module_features.rs::module_features_cjs_export_equals_resolves_to_carrier` — `ModuleFeatures`
- `module_features.rs::module_features_declare_global_merges_two_blocks` — `ModuleFeatures`
- `module_features.rs::module_features_external_module_augmentation_merges_config` — `ModuleFeatures`
- `module_features.rs::module_features_module_augmentation_merges_plugin_surface` — `ModuleFeatures`
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

**`U6.NARROWING`** (104 rows):

- `flow_invalidations.rs::flow_invalidations_fi01_reassignment_invalidates_string_narrowing` — `FlowNarrowing`
- `flow_invalidations.rs::flow_invalidations_fi02_narrowing_preserved_across_opaque_call` — `FlowNarrowing`
- `flow_invalidations.rs::flow_invalidations_fi04_destructured_discriminant_preserves_correlation` — `FlowNarrowing`
- `flow_invalidations.rs::flow_invalidations_fi05_destructured_discriminant_loses_on_reassignment` — `FlowNarrowing`
- `flow_invalidations.rs::flow_invalidations_fi09_exhaustive_never_tail_does_not_widen_return` — `FlowNarrowing`
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

**`U6.PREDICATE_ASSERTION`** (3 rows):

- `flow_invalidations.rs::flow_invalidations_fi08_asserts_narrows_dotted_member_path` — `FlowNarrowing`
- `substitution_types.rs::substitution_types_sb09_asserts_x_is_string_on_generic` — `TypeParameterFeatures`
- `substitution_types.rs::substitution_types_sb10_x_is_t_predicate_on_generic` — `TypeParameterFeatures`

**`U6.CALL_RESOLVE`** (19 rows):

- `call_resolution.rs::call_resolution_extracted_prototype_method_call_returns_declared_return` — `CallResolution`
- `call_resolution.rs::call_resolution_generic_infers_from_callback_return_type` — `CallResolution`
- `call_resolution.rs::call_resolution_generic_infers_from_positional_argument_through_callback_signature` — `CallResolution`
- `call_resolution.rs::call_resolution_generic_infers_object_literal_including_excess_properties` — `CallResolution`
- `call_resolution.rs::call_resolution_optional_overload_picks_first_arity_matching_signature` — `CallResolution`
- `call_resolution.rs::call_resolution_optional_overload_picks_two_arg_signature_when_required` — `CallResolution`
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
<!-- END U0 row→block coverage table. Union = 363 unique IgnoredTestRows, each owned by exactly one block_id. -->

### 10.5 The exact-363 count and bijection

`EXPECTED_TOTAL_IGNORED_COUNT` is ALWAYS exactly `count(IgnoredTestRow where status ==
Ignored)`; it is 363 at U0. It is NOT a frozen constant that lags the row states — it is the
live count of `Ignored` `IgnoredTestRow`s, and every phase that changes how many are `Ignored`
updates it IN THE SAME LOCKED STATE TRANSACTION so the count guard never observes a
disagreement. The count and bijection are over the `IgnoredTestRow` table ONLY —
`AdditionalProofRow`s are excluded (they carry no `IgnoreStatus`). The bijection guards are:
live ignored test sites (source `#[ignore]`s) must exactly equal `IgnoredTestRow`s with
`status == Ignored`, and that set must also exactly equal `EXPECTED_TOTAL_IGNORED_COUNT`. The
phase accounting:

- **`prepare_verify`** strips the block's source `#[ignore]`s AND flips its rows
  `Ignored → Verifying` AND sets `EXPECTED_TOTAL_IGNORED_COUNT = count(status == Ignored)`
  (decrements by exactly the block's row count) — ALL in one locked transaction, so at every
  committed instant the live source-`#[ignore]` count, the `Ignored` row count, and the count
  agree. This lets the FULL workspace gate run while the block is `Verifying` (its `#[ignore]`s
  already removed) WITHOUT tripping the count guard: a `Verifying` row is neither a source
  `#[ignore]` nor an `Ignored` row nor counted in the total.
- **`accept`** changes the block's rows `Verifying → Lifted` with NO further count change (the
  rows already left the `Ignored` set at `prepare_verify`); `Lifted` rows must correspond to a
  live test function without `#[ignore]`.
- **`abort`** restores the block's source `#[ignore]`s AND restores the rows `Verifying →
  Ignored` AND restores `EXPECTED_TOTAL_IGNORED_COUNT` to `count(status == Ignored)` — again all
  in one locked transaction.

The two-table split is pinned by a dedicated binding-total count guard:

- **`ignored_test_row_table_holds_exactly_363_rows`** — asserts the `IgnoredTestRow` table holds
  EXACTLY 363 rows (the binding manifest total), DISJOINT from the `AdditionalProofRow` table; no
  `AdditionalProofRow` participates in `EXPECTED_TOTAL_IGNORED_COUNT` or the bijection; and the two
  tables are disjoint (no `(file, function)` identity in both — a submatrix/additional fixture
  corresponding to an existing ignored row stays an `IgnoredTestRow`, never duplicated). An
  `IgnoredTestRow` count other than 363, an `AdditionalProofRow` counted toward the ignored total
  or bijection, or a `(file, function)` in both tables, FAILS.

- **`additional_proof_row_table_holds_exactly_7_rows`** — asserts the `AdditionalProofRow` table
  holds EXACTLY 7 coverage-only rows = the 6 JSX no-new-key submatrix rows (`U2.JSX_FOUNDATIONS`) +
  the 1 mapped companion `mapped_modifier_minus_optional_preserves_explicit_undefined_on_required_property`
  (`U2.MAPPED_TEMPLATE`). The set is CLOSED, not growing. The table is disjoint from `IgnoredTestRow`,
  and no `AdditionalProofRow` is counted toward `EXPECTED_TOTAL_IGNORED_COUNT` or the bijection. An
  `AdditionalProofRow` count other than 7, or a row that leaks into the ignored count/bijection, FAILS.

## 11. The cutover / ledger TRANSACTION CONTRACT (crash-safe journaled transaction)

The orchestration substrate that lands blocks — the `.cutover-state` typeinfo-parity cursor, the
manifest ledger, and the source-`#[ignore]` state — is governed by ONE transaction contract,
specified in full at the plan level here. It is a CRASH-SAFE JOURNALED TRANSACTION. The seven
parts:

### 11.1 Snapshot isolation (reads included)

Every mutation AND every observation operates on a single locked, consistent under-lock snapshot
of `.cutover-state` + manifest/count + source-`#[ignore]` state. The three artifacts are published
as SEPARATE temp-renames within a transaction, so any read could otherwise catch a transaction
mid-flight and see two artifacts at one transaction's value and the third at another's — exactly
the torn cross-artifact view per-file atomic-rename does not prevent. The end-state rule is snapshot
isolation over the WHOLE state, reads included: EVERY observation — the invariant/bijection/count
checks, the parallel-safety / landed-agreement guards, prereq derivation, parent-completion, the
resume protocol's lease-staleness + next-block selection reads, and dispatch-eligibility reads —
takes `.cutover-state.lock` (the STABLE SIBLING lock, never `.cutover-state` itself) and reads the
FULL snapshot under that single lock. A read of any one artifact WITHOUT the lock, or a read that
observes one artifact under the lock and another outside it (a torn cross-artifact snapshot), is a
violation. The lock serializes mutations against each other AND against every observation, so the
only states any reader ever sees are committed, whole-snapshot boundaries.

### 11.2 Two-phase prepare-verify → gate → accept / abort (each a LOCKED STATE TRANSACTION)

A block is NOT accepted by landing-then-reverting. Under parallel execution a "land before the
workspace gate, compensating-revert on failure" model is unsound — a dependent block can start on a
landed-but-not-yet-gated block about to be reverted, and a single atomic rename cannot transactionally
cover `.cutover-state` + the manifest rows + the expected counts + the source-`#[ignore]` removals.
The end state is an explicit TWO-PHASE COMMIT with a distinct pending state, where each phase is a
LOCKED STATE TRANSACTION (not a single multi-file atomic rename), and "done" is decided by both the
manifest AND the guard suite.

Each phase (`prepare_verify` / `accept` / `abort`) touches THREE distinct on-disk artifacts —
`.cutover-state` (the TOML cursor), the manifest rows + `EXPECTED_TOTAL_IGNORED_COUNT`, and the
source-file `#[ignore]` lines. No filesystem rename can cover three separate files atomically. The
phase is made correct by a LOCK plus a snapshot-revalidate plus a transition ORDER that keeps every
intermediate observable state non-done, NOT by pretending one rename covers all three:

- **Lock + snapshot-revalidate under the lock (writes AND all reads).** All phase writes, the xtask's
  precondition checks, AND every cutover-state observation take the STABLE SIBLING lock
  `.cutover-state.lock`. UNDER the lock, the reader/writer RE-READS all three artifacts as one snapshot
  and (for a write phase) re-checks the phase's preconditions against that fresh snapshot. A precondition
  that fails on the under-lock snapshot ABORTS the phase without writing anything; a read-only observer
  decides on the under-lock whole-snapshot, never a partial mid-transaction view.
- **Write each file by temp-rename, with a `revision`-CAS on `.cutover-state`.** Each of the three
  artifacts is published by its own write-temp-then-rename. The `.cutover-state` write additionally
  applies the `revision`-CAS (commit only if `[typeinfo_parity].revision` is unchanged since the
  under-lock read, bump on commit, retry on CAS miss). This keeps per-file temp-rename + CAS-on-revision
  for `.cutover-state` while NOT claiming one rename spans all three files.
- **Transition ORDER makes every intermediate on-disk state non-done.** The lease keep/release order and
  the file-write order are chosen so EVERY intermediate on-disk state is observably NON-DONE. `accept`
  does NOT append the `landed_blocks` token / release the lease until AFTER the manifest rows are written
  `Lifted` and the source `#[ignore]`s are already removed, so the `.cutover-state` `accept` commit (the
  LAST write) is the single observable done-edge; before it lands, the block's lease is still LIVE and
  its `landed_blocks` token still absent. `prepare_verify` keeps the lease LIVE and never appends a token.
  `abort` restores `#[ignore]`s and the rows to `Ignored` and clears the lease.

The pending state is `IgnoreStatus::Verifying { block_id, lease_id }` (alongside `Ignored` / `Lifted`):
the block's source `#[ignore]`s are removed and its rows are `Verifying`, but it is NOT yet completed/
landed; the `lease_id` ties the verifying rows to the live `active_blocks` lease that owns the in-flight
verification. `prepare_verify` (phase 1) removes the source `#[ignore]`s, marks the rows `Verifying`,
KEEPS the lease LIVE, and does NOT append to `landed_blocks`. The full workspace gate runs WHILE the
block is `Verifying` (the `#[ignore]`s already removed, so the lifted tests execute under the gate);
prerequisite checks and parent-completion MUST treat `Verifying` rows as NOT done. `accept` (phase 2)
runs ONLY after the gate passes; `abort` (gate failed) restores the pre-verify state.

The guard is **`typeinfo_state_snapshots_are_locked_and_precondition_checked`**: every phase is a locked
state transaction, NOT a single multi-file atomic rename — eligibility reads + precondition checks take
the stable sibling lock; under the lock the phase re-reads all three artifacts as one snapshot and
revalidates preconditions before any write; each artifact is published by its own temp-rename (with the
`revision`-CAS on `.cutover-state`); and the lease/file-write order makes every intermediate non-done.

### 11.3 The durable workspace-gate RECEIPT bound to the input-hash PAIR

"`accept` ran only after a green workspace gate" must be MECHANICALLY ENFORCEABLE over the EXACT content
the gate ran against AND the exact deterministic post-accept content, not a named intent and not a single
live hash. A single live `workspace_gate_input_hash` is insufficient: the gate runs while the block is
`Verifying`, so it hashes the manifest rows in their `Verifying` form, but `accept` transitions those rows
`Verifying → Lifted` and the manifest rows/count are part of the hashed input — so a single hash recomputed
AFTER `accept` flips the rows would no longer match, and a receipt over the `Verifying` input alone does not
PROVE the post-accept `Lifted` state was gated. The receipt therefore binds the PAIR
`{ pre_accept_verifying_hash, post_accept_lifted_hash }`.

The gate is the COMPLETE Rust **AND** JavaScript gate — it is green only when BOTH gates pass:

- **Rust:** `cargo test --workspace --tests --verbose`; `cargo clippy --workspace -- -D warnings`;
  `cargo fmt --all --check`.
- **JavaScript:** `pnpm test`; `pnpm install --frozen-lockfile`.

The `pre_accept_verifying_hash` input below already covers the JS-relevant tracked inputs
(`package.json` / the lockfile / the TS sources), so the receipt binds the JS gate exactly as it binds the
Rust gate — a stale receipt fails the hash match whether the intervening change was Rust or JS.

The gate-pass precondition is satisfied in exactly ONE of two ways: (a) `accept` itself RUNS the full
workspace gate inline (the complete Rust + JS gate above) and proceeds only on green;
OR (b) `accept` CONSUMES a durable GATE RECEIPT artifact bound to `{ block_id, lease_id,
workspace_gate_input_hash: { pre_accept_verifying_hash, post_accept_lifted_hash }, command set, success }`,
where:

- `pre_accept_verifying_hash` covers the FULL gated input the gate ran against, with the block's manifest
  rows in their `Verifying` form: the workspace tracked source/tests/protos/config/lockfiles, PLUS the
  manifest rows-as-`Verifying` + `EXPECTED_TOTAL_IGNORED_COUNT`, the source-`#[ignore]` state, the
  `block_id`, the `lease_id`, the command set, and the toolchain/config fingerprint; and
- `post_accept_lifted_hash` covers the DETERMINISTIC post-`accept` state: the SAME full input with ONLY the
  block's rows transitioned `Verifying → Lifted` (no count change — the count left the `Ignored` set at
  `prepare-verify`), and `success = true`.

Distinct from both is a third, CONTENT-ONLY operand persisted at the SAME `accept` point:

- `post_accept_lifted_tree_hash` is a deterministic CANONICAL PROJECTED CONTENT HASH over the post-`accept`
  tracked tree with the `.cutover-state*` orchestration-cursor family EXCLUDED — a blake3 over the sorted
  tracked-blob set (path + blob content) of every tracked file EXCEPT the `.cutover-state` file and any
  tracked sibling cursor artifact (`.cutover-state.lock`, journal/WAL siblings — the `.cutover-state*`
  family). It is NOT a whole-worktree tree id (such an id WOULD include `.cutover-state`).
  It carries NONE of the gated-input metadata. It is computed and persisted at `accept` ALONGSIDE
  `post_accept_lifted_hash`, and is DISTINCT from it: `post_accept_lifted_hash` is the whole-gated-input
  hash (workspace content PLUS `block_id` / `lease_id` / command set / toolchain-config fingerprint /
  `success` / the manifest rows-as-`Lifted`) and is the §11.3 accept-recheck operand UNCHANGED;
  `post_accept_lifted_tree_hash` is the content-only operand the §11.11 land-commit comparison targets,
  because the canonical projection (tracked content minus the `.cutover-state*` cursor) can NEVER equal the
  whole-gated-input hash.

  The `.cutover-state*` exclusion has TWO independent justifications. (a) **It avoids a cryptographic fixed
  point.** `.cutover-state` is a TRACKED file, and `accept` writes the `post_accept_lifted_tree_hash` itself
  (plus the `landed_blocks` token, the `land_records` entry, and the lease release) INTO `.cutover-state`. A
  hash that included `.cutover-state` would have to equal a tree that contains its own value — unsatisfiable
  — so excluding the cursor family lets those accept-time writes be persisted without self-reference. (b)
  **It is the SEMANTICALLY correct content binding.** The workspace gate (§11.3) ran on the AUTHORED content
  — the block's source / tests / docs plus the manifest rows — while `.cutover-state`'s `landed_blocks` /
  `land_records` / lease-release fields are written by `accept` AFTER the gate and were NEVER part of the
  gated input. They must therefore NOT enter the content-binding operand; binding only the authored content
  is exactly what the land commit should be pinned to.

Under `.cutover-state.lock`, `accept`'s precondition recheck (1) RECOMPUTES the pre-accept input hash over
the LIVE gated input (rows still `Verifying`) and verifies it EQUALS the receipt's `pre_accept_verifying_hash`;
(2) verifies the receipt EXISTS and its binding MATCHES (same `block_id` + `lease_id` as the lease being
accepted); (3) APPLIES ONLY the deterministic `Verifying → Lifted` transition; and (4) recomputes the
resulting state's input hash and verifies it EQUALS `post_accept_lifted_hash`. A STALE receipt — produced
before ANY gated-input content change, or from a different block, or a different lease — fails the
`pre_accept_verifying_hash` match; a non-deterministic / tampered post-state fails the
`post_accept_lifted_hash` match. WITHOUT a present, bound, success-true receipt whose recomputed
`pre_accept_verifying_hash` matches the live `Verifying` input AND whose `post_accept_lifted_hash` equals the
deterministic post-`accept` state (and absent the inline-gate option), `accept` REFUSES.

**Status-sensitive-guard equivalence.** The two-hash receipt is SOUND — gating the `Verifying` state proves
the `Lifted` state — ONLY if every proof/guard check the gate evaluates on a `Verifying` row is EQUIVALENT to
what it would evaluate on that row's soon-to-be `Lifted` form. Any STATUS-SENSITIVE guard or proof check (one
whose result depends on `IgnoreStatus`) MUST, while the block is `Verifying` under the gate, treat a
`Verifying { block_id, lease_id }` row as its post-accept `Lifted { block_id }` form for that check — e.g.
`lifted_row_executes_declared_proof` (the row's generated wrapper runs its declared proof while `Verifying`,
exactly as once `Lifted`), `landed_typeinfo_blocks_have_required_guards_and_workspace_gate` (the block's
required/Critical-rule guards present and passing during the gate), and the row-test wrapper itself. The
deterministic `Verifying → Lifted` transition `accept` applies is then the ONLY difference between the gated
state (`pre_accept_verifying_hash`) and the accepted state (`post_accept_lifted_hash`), and that difference
does not change any proof/guard outcome. Status-INSENSITIVE checks are unaffected.

The guard is **`workspace_gate_passes_before_typeinfo_block_acceptance`**: `accept` either ran the full gate
inline on green, or consumed a durable receipt EXISTING and BOUND to the pair, with the recomputed
`pre_accept_verifying_hash` matching, applying ONLY the deterministic transition, then matching
`post_accept_lifted_hash` — and the gate's status-sensitive checks treating a `Verifying` row as its post-accept
`Lifted` form. A receipt produced before an intervening Rust/TS/proto/test/package/config/lockfile (or
manifest/source-`#[ignore]`) change recomputes to a non-matching `pre_accept_verifying_hash` and does NOT
authorize the accept. A lifecycle that flips rows to `Lifted` / appends the landed token without a present,
bound, success-true receipt (and without an inline green gate), or that applies more than the deterministic
transition before checking `post_accept_lifted_hash`, or that evaluates a status-sensitive check on a
`Verifying` row as not-yet-`Lifted`, FAILS.

### 11.4 Every intermediate on-disk state is non-done

Because the artifacts are written as ordered per-file temp-renames, the lease keep/release order and write
order make every mid-transaction state observably non-done (done is gated on the final `accept` `.cutover-state`
write).

### 11.5 The lease lifecycle

Dispatch / heartbeat / adopt / clear, with the precise four-clause staleness predicate (LANDED OR
MANIFEST-COMPLETE OR EXPIRED/ADOPTED OR CLEARED), never "any newer revision." A lease is STALE iff: (a) its
block id is already in `[typeinfo_parity].landed_blocks` (LANDED — i.e. `accept`ed); (b) its block's manifest
rows are all `Lifted` (MANIFEST-COMPLETE — a `Verifying` block is NOT manifest-complete); (c) it is EXPIRED
(`now_unix > expiry_unix` with no fresher heartbeat) OR has been explicitly ADOPTED (its `lease_id` / `owner_id`
no longer match the original holder after a CAS-adoption write); or (d) it has been explicitly CLEARED (an
explicit release / `abort` removed the `active_blocks` entry). A lease is NOT stale merely because
`acquired_revision < revision`: unrelated parallel writes bump `revision` without making a live, heartbeated,
unaccepted lease old. A lease whose block is `Verifying` under that same live holder is a LIVE in-flight cursor,
NOT stale. `acquired_revision` is recency/CAS metadata, never the staleness oracle.

### 11.6 The crash-recoverable PENDING-TRANSACTION JOURNAL (WAL), reconcile-on-read

Snapshot isolation only protects readers from mid-transaction temp-renames while the lock is HELD — it does NOT
cover a writer DYING mid-transaction (e.g. `accept` leaving rows `Lifted` with no landed token / an unreleased
lease). A crashed `accept` can leave rows `Lifted` but no `landed_blocks` token / an unreleased lease; a crashed
`prepare-verify` can leave the `#[ignore]`s removed / rows `Verifying` but the count or lease half-written; a
crashed `abort` can leave a half-restored state — all states the four-part done predicate correctly reads as NOT
done, but for which the contract must give a recovery path.

The end-state rule is a DURABLE PENDING-TRANSACTION JOURNAL (a write-ahead log) under `[typeinfo_parity]`: BEFORE
any phase renames its artifacts, the xtask WRITES a journal entry recording (i) the PHASE (`prepare-verify` /
`accept` / `abort`), (ii) the BLOCK ID, (iii) the LEASE ID, (iv) the BEFORE snapshot hash and the AFTER snapshot
hash (the intended post-commit state's hash), and (v) the INTENDED ARTIFACT TRANSITIONS (the exact `.cutover-state`
/ manifest-row+count / source-`#[ignore]` edits the phase will apply); the entry is CLEARED only after the phase's
FINAL done-edge write commits, so a present entry means a transaction was interrupted mid-flight.

Recovery is folded into snapshot isolation: EVERY under-lock observation MUST, as its FIRST under-lock act,
RECONCILE the pending journal — for any present entry it either (a) COMPLETES a valid partially-applied `accept` by
re-applying the intended remaining transitions to reach the recorded AFTER state, but ONLY when a matching gate
receipt is present (its `pre_accept_verifying_hash` matching the recorded pre-accept state AND its
`post_accept_lifted_hash` matching the completion target) AND the on-disk state matches the recorded BEFORE hash, OR
(b) ROLLS BACK to the recorded previous (BEFORE) state when completion is not valid (no matching receipt, a
BEFORE-hash mismatch, or a `prepare-verify`/`abort` interruption), restoring the artifacts and clearing the entry.
Only AFTER reconciliation leaves a clean, whole, committed snapshot does the observation read its decision. This
makes a crashed writer's stranded state RECOVERABLE-OR-REJECTED before any prereq/dispatch/eligibility decision ever
reads it. Pinned by **`pending_typeinfo_transactions_reconciled_before_eligibility`** and the extended landed-agreement
guard **`cutover_state_landed_blocks_match_typeinfo_manifest`** (which reconciles the journal before reading agreement).

### 11.7 The 4-part done predicate

Because `accept` writes the manifest rows `Lifted` BEFORE it appends the `landed_blocks` token / releases the lease
(the rows-`Lifted`-but-token-absent in-transaction window), "rows all `Lifted`" ALONE is NOT a sufficient
done/prereq-satisfied test. A block is "done" / its prerequisite is "satisfied", EVALUATED ON THE UNDER-LOCK WHOLE
SNAPSHOT, iff ALL of:

1. every row in its row-set is `Lifted`, AND
2. the block's `landed_blocks` token is PRESENT in `.cutover-state.typeinfo_parity.landed_blocks`, AND
3. there is NO live active lease for that block in `.cutover-state.typeinfo_parity.active_blocks` (no in-flight
   `accept`/`abort`/verification still holds it — a `Verifying`-under-live-lease block is never done), AND
4. the block's `TYPEINFO_PARITY_BLOCKS.required_guards` / the Critical-rule guards (R6) for any new `(CRITICAL)` rule
   the block introduces are PRESENT (registered and passing in the default suite).

A TOKEN-ABSENT state, a LIVE-LEASE state, or a GUARD-ABSENT state is NOT done. Row status ALONE is not sufficient;
neither is the token alone. This is the SAME predicate the landed-agreement guard
(`cutover_state_landed_blocks_match_typeinfo_manifest`) and prereq derivation
(`typeinfo_block_prereqs_derive_from_manifest_status`) read, reconciled to one definition. It is well-defined precisely
because the observer reads rows + token + lease as ONE under-lock snapshot (§11.1).

### 11.8 The named guards

The transaction contract is pinned by:
`typeinfo_state_snapshots_are_locked_and_precondition_checked`,
`typeinfo_block_prereqs_derive_from_manifest_status`,
`typeinfo_block_prereqs_ignore_verifying_blocks`,
`verifying_typeinfo_block_lease_blocks_dependents`,
`workspace_gate_passes_before_typeinfo_block_acceptance`,
`landed_typeinfo_blocks_have_required_guards_and_workspace_gate`,
`cutover_state_landed_blocks_match_typeinfo_manifest` (extended to reconcile the pending journal before reading
agreement),
`pending_typeinfo_transactions_reconciled_before_eligibility`,
`cutover_state_typeinfo_writes_are_locked_and_cas`,
`parallel_typeinfo_block_landing_preserves_all_tokens`,
`resume_rejects_stale_typeinfo_block_lease`,
`no_vacuous_parent_u_block_landing`,
`zero_row_blocks_land_exactly_once`,
`typeinfo_parity_block_dag_is_acyclic_and_consumed_keys_and_mechanisms_are_prereqs` (the block prerequisite DAG is acyclic and both key-prerequisite-consistent AND mechanism-prerequisite-consistent —
see below),
`typeinfo_block_lands_as_single_squashed_commit` (§11.11 — each landed block contributes exactly one mainline commit; the WIP
series is squashed at `accept`), and
`typeinfo_block_accept_requires_review_land_verdict` (§11.12 — `accept` requires a present, bound review receipt recording a
LAND / NITs-only verdict from all three panel reviewers, 1 Claude Code + 2 codex, whose recomputed `pre_accept_verifying_hash`
matches).

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
is a `U6.NARROWING`-substrate row whose dominant mechanism is the
`PredicateAssertion.assertion_effect_dotted_member_path` engine owned by
`U6.PREDICATE_ASSERTION`; under a keys-only model the row could sit in `U6.NARROWING`
while consuming `U6.PREDICATE_ASSERTION`'s assertion engine even though
`U6.PREDICATE_ASSERTION` is not a prerequisite of `U6.NARROWING` (the actual edge is the reverse) —
a latent mechanism deadlock. The mechanism model FAILS this (check 2: the row's
dominant-mechanism owner `U6.PREDICATE_ASSERTION` ≠ a `U6.NARROWING` `block_id`),
forcing the row's `block_id` to `U6.PREDICATE_ASSERTION`, where it correctly consumes
the `FlowNarrowing.frame` mechanism that `U6.NARROWING` — its declared prerequisite —
produces (check 3 holds, no cycle).

The exact BYTE-LEVEL locking + atomic-rename + CAS IMPLEMENTATION (the file-lock primitive on the stable sibling
`.cutover-state.lock`, the temp-write-then-rename per artifact, the `revision` compare-and-swap, the receipt
persistence/binding to the input-hash PAIR, the pending-transaction journal write/reconcile/clear protocol, and the
write-order proof) is REALIZED and VERIFIED in the owning implementation block (U0) UNDER these named guards. Snapshot
isolation + the WAL pending-transaction journal (reconcile-on-every-observation) + the input-hash-bound gate receipt +
the four-part done predicate together constitute a COMPLETE crash-safe transaction — even a mid-transaction crash is
recoverable-or-rejected, and any deviation surfaces as a guard failure rather than an unspecified gap.

### 11.9 Parent / aggregate U-block tokens (no vacuous parent landing)

A parent U-block token (e.g. `U2`) is an AGGREGATE over its child blocks (e.g. `U2.RELATION_INFER`, `U2.UTILITY`, …);
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
RE-selecting an already-done one. Their lifecycle is therefore TOKEN/done-predicate
driven, not row-status driven:

- **Eligible** iff the block is NOT done by the four-part done predicate (§11.7),
  AND its `.cutover-state.typeinfo_parity.landed_blocks` token is ABSENT, AND its
  block-ID prereqs are landed (by the same done predicate). A zero-row block's done
  predicate collapses to predicate parts 2–4 (token present, no live lease, required
  guards present), since part 1 (rows `Lifted`) is vacuously satisfied.
- **`prepare-verify`** records the lease + phase with NO row-status transitions and
  NO `EXPECTED_TOTAL_IGNORED_COUNT` change (it owns no rows).
- **`accept`** appends the `landed_blocks` token ONLY AFTER the block's required
  guards + the `{pre,post}` input-hash-PAIR gate receipt pass — the token append /
  lease release is the LAST observable done-edge, exactly as for row-owning blocks.

The "next actionable block" selector COMPOSES two predicates so a cold agent always
has an unambiguous next block: the row-status predicate (for row-owning blocks) AND
this token/done-predicate predicate (for zero-row blocks). A zero-row block lands
EXACTLY ONCE, is never skipped, and is never re-selected once its token is present.
Pinned by **`zero_row_blocks_land_exactly_once`** — asserts every zero-row block
lands exactly once (token appended exactly once, after guards + receipt), is never
skipped, and is never re-selected after its token is present.

### 11.11 Git commit history — WIP series → squash → one land commit at `accept`

The two-phase transaction (§11.2) is the ledger/cutover-state axis of acceptance.
This subsection is the GIT-HISTORY projection of that same acceptance — a SEPARATE
axis from the xtask transaction, not a replacement for it. It governs how many
commits mainline (`refactor/semantic-db-overhaul`) receives per landed block.

- **During block implementation the owning agent commits FREELY as a WIP series.**
  A HIGH WIP commit count is expected and encouraged (per-fix commits aid crash
  recovery — a fix-agent's partial work survives an API failure cleanly). **WIP
  commits do NOT run the full workspace gate.** Per `CLAUDE.md`'s Stub-Prevention
  WIP exemption, the in-flight WIP series MAY carry `todo!()` / placeholder /
  empty-test intermediate states — they are scratch states on the way to the landed
  commit, exactly the case the WIP exemption permits.
- **When the block is implementation-complete, the full workspace gate runs ONCE**
  (§11.3 / §14 resume step 8) — the complete Rust **AND** JavaScript gate — and any
  failure is fixed (with more WIP commits, re-running the gate after the fixes).
- **The WIP series is squashed into EXACTLY ONE land commit per landed block**,
  created at the `accept` done-edge — the squash is the git-history projection of
  the `accept` phase. Mainline therefore receives EXACTLY ONE commit per landed
  block: a LOW landing-commit count, a HIGH WIP count during the work. The single
  land commit may be created only AFTER both the green gate receipt (§11.3) and the
  three-reviewer LAND verdict (§11.12) authorize the `accept` — the squash never
  precedes the LAND authorization.
- **The land commit is CONTENT- and IDENTITY-bound to the accept receipt** — the
  git-history axis is hash-bound exactly as the cutover-state axis is (§11.3), so no
  stray post-review / post-gate tracked change can ride in the land commit and the
  guard can never degrade to manual history inspection:
  - **Machine-readable land-commit trailer.** The single squashed mainline land
    commit carries a trailer binding it to the accepted state:
    `Typeinfo-Block: <block_id>`, `Typeinfo-Lease: <lease_id>`,
    `Pre-Accept-Verifying-Hash: <pre_accept_verifying_hash>` — the SAME hash the gate
    receipt (§11.3) and the review-LAND receipt (§11.12) bind to. The trailer is
    NON-CIRCULAR: its three values are COPIED from the accept receipt recorded at
    `accept`, not recomputed from the land commit itself.
  - **Content binding.** The canonical projection of the land commit's tracked tree —
    the same blake3 over the sorted tracked-blob set with the `.cutover-state*`
    orchestration-cursor family EXCLUDED (§13.1) — EQUALS the accepted
    `post_accept_lifted_tree_hash`, the content-only operand recorded at `accept` over
    the exact authorized post-`accept` AUTHORED content (rows `Verifying → Lifted`, no
    count change). This is the CONTENT-ONLY canonical projection, NOT the
    whole-gated-input `post_accept_lifted_hash` (the projection can never equal that —
    §11.3); the full-input `post_accept_lifted_hash` keeps its §11.3 role as the `accept`
    precondition recheck of the whole gated state. The `.cutover-state*` cursor is
    excluded on BOTH sides because it is accept-mutated bookkeeping written AFTER the gate
    (the `landed_blocks` token / `land_records` / lease release / the stored hash itself)
    and was never part of the gated AUTHORED input. A clean index is required at squash /
    `accept`; any land commit whose canonical projection DIVERGES from
    `post_accept_lifted_tree_hash` (stray authored files, post-gate edits, a dirty index
    in the AUTHORED content) is REJECTED — while `.cutover-state`'s own accept-writes are
    correctly outside the comparison.
  - **Land-record persistence.** The trailer source (`block_id` / `lease_id` /
    `pre_accept_verifying_hash`) plus the content-only `post_accept_lifted_tree_hash`
    is persisted in the §13.1 `[typeinfo_parity]` state at `accept`, so the token ↔
    commit mapping is machine-checkable, not a prose intent.
- **Disambiguation from §13.2.** §13.2's "There is NO single `land` command" refers
  to the **cutover-state xtask**: acceptance is the two-phase `prepare-verify` →
  gate → `accept` transaction, NOT a one-shot xtask `land` subcommand. The **land
  COMMIT** here is a DIFFERENT axis — the git-history artifact produced by squashing
  the WIP series at the `accept` phase. The two never contradict: "one land commit
  (git)" is produced by the `accept` phase; there is still no one-shot `land` xtask
  subcommand. The xtask transaction decides WHEN acceptance is valid; the squash is
  HOW that acceptance shows up in mainline git history.

Pinned by **`typeinfo_block_lands_as_single_squashed_commit`** — maps each
`.cutover-state.typeinfo_parity.landed_blocks` token to EXACTLY ONE mainline
(`refactor/semantic-db-overhaul`) commit carrying the matching land-commit trailer
(`Typeinfo-Block` / `Typeinfo-Lease` / `Pre-Accept-Verifying-Hash` equal to the
accept-receipt values) AND a canonical tracked-tree projection equal to the accepted
content-only `post_accept_lifted_tree_hash`. The guard computes the SAME canonical
projection over the land commit that `accept` recorded — the blake3 over the sorted
tracked-blob set with the `.cutover-state*` orchestration-cursor family EXCLUDED (NOT
a whole-worktree tree id, and NOT the whole-gated-input
`post_accept_lifted_hash`, which the projection can never equal — §11.3) — and compares
it to the stored value. The token ↔ commit relation is a BIJECTION: the guard REJECTS
zero commits for a token, more than one commit for a token, a commit with a missing /
mismatched trailer, a content-divergent commit (canonical projection ≠
`post_accept_lifted_tree_hash`, i.e. stray authored files / post-gate edits / a dirty
index in the AUTHORED content at squash), or a squash performed before the gate +
review-LAND authorization. Because both sides exclude the `.cutover-state*` cursor, the
accept-time writes into `.cutover-state` (`landed_blocks` / `land_records` / lease
release / the stored hash itself) are correctly OUTSIDE the comparison — the projection
binds only the gated AUTHORED content, so the guard is satisfiable rather than chasing a
cryptographic fixed point. The comparison is NON-CIRCULAR: the trailer and hash values
are COPIED from the receipt recorded at `accept`, not recomputed from the commit itself.
Because the trailer source is persisted in the §13.1 `[typeinfo_parity]` state at
`accept`, the mapping is machine-checkable, not a manual history inspection.

### 11.12 Review-LAND verdict gate (three-reviewer panel before `accept`)

`accept` must be authorized by reviewers who say to LAND, not by the workspace gate
alone. This is a SECOND `accept` precondition alongside the workspace-gate receipt
(§11.3), bound to the SAME gated input so a stale review can never authorize a land.

The review panel is EXACTLY THREE reviewers — **1 Claude Code reviewer + 2 codex
reviewers** — each adversarial / bad-mood, every reviewer holding a
**best-architecture-no-compromises mandate**: breaking changes are ALLOWED and
DESIRED; the goal is the best architecture / solution possible, never the easiest or
least-breaking path. Each reviewer evaluates the block's implementation against the
SAME `Verifying`-state gated input the workspace gate ran against.

`accept` REFUSES unless, IN ADDITION to the green workspace-gate receipt (§11.3), a
durable **review-LAND verdict receipt** is present and bound to
`{ block_id, lease_id, pre_accept_verifying_hash }`, recording a verdict from ALL
THREE panel reviewers. The LAND bar is: **all three return LAND, OR all residual
findings are NITs (cosmetic / non-material — P3-class) only.** Any open material
finding (P0 / P1 / P2-class) from ANY of the three blocks the land.

- The review is evaluated against the same `Verifying`-state gated input the
  workspace gate ran against; a review produced before any intervening
  tracked-content change recomputes to a non-matching `pre_accept_verifying_hash`
  and does NOT authorize the accept (exactly the staleness rule §11.3 uses for the
  gate receipt — the JS-relevant tracked inputs are covered by that same hash, so a
  JS-only change also invalidates a stale review).
- A non-LAND verdict from any reviewer, a missing reviewer, an open material
  finding, or a verdict bound to a different `block_id` / `lease_id` / hash →
  `accept` REFUSES.
- The verdict receipt is persisted by the `review-receipt <block-id>` xtask step
  (§13.2), mirroring `gate-receipt`: it records the panel composition (1 Claude Code
  + 2 codex), each reviewer's verdict, and that all residuals are NITs-only, bound to
  the input-hash pair, only after all three return LAND / NITs-only. `accept`
  consumes BOTH the gate receipt AND this three-reviewer review-LAND receipt.

The §11.7 four-part done predicate is unchanged (done = rows `Lifted` + token + no
live lease + guards present). The review-LAND verdict and the workspace gate are
`accept` PRECONDITIONS, not done-predicate parts: the `accept` phase cannot fire — so
the `landed_blocks` token never appears — until BOTH receipts are present and bound.
The done predicate is NOT weakened; it simply can never observe a `Lifted` /
token-present state that was not authorized by both receipts.

Pinned by **`typeinfo_block_accept_requires_review_land_verdict`** — `accept`
requires a present, bound review receipt recording a LAND / NITs-only verdict from
all three panel reviewers (1 Claude Code + 2 codex), whose recomputed
`pre_accept_verifying_hash` matches the live `Verifying` input; no land on any
reviewer's non-LAND verdict, any open material (P0/P1/P2-class) finding, a missing
reviewer, or a stale / mis-bound receipt.

## 12. No-skip guarantee

A skipped block is mechanically visible in three ways: its rows remain `Ignored`, its tests remain ignored or red, and
dependent block prereq guards fail. If someone removes an ignore without changing the row, the bijection guard fails. If
someone changes the row without removing the ignore, the count guard fails. `EXPECTED_TOTAL_IGNORED_COUNT` is ALWAYS
exactly `count(status == Ignored)` (never frozen): `prepare-verify` sets it in the SAME locked transaction that strips
the `#[ignore]`s and flips the rows to `Verifying`, `accept` makes no further count change, and `abort` restores it — so
the count guard stays green while the gate runs on `Verifying` rows. If someone marks a block landed while rows remain
`Ignored`, `no_landed_typeinfo_block_has_live_ignored_rows` fails. If someone lands a parent/aggregate U-block token while
any child block's rows remain `Ignored` — including the vacuous zero-row case — `no_vacuous_parent_u_block_landing` fails.

Block acceptance is a TWO-PHASE commit, NOT land-then-revert, and each phase is a LOCKED STATE TRANSACTION, NOT a single
multi-file atomic rename (§11.2). Prerequisite checks and parent-completion treat `Verifying` rows as NOT done, so a
dependent block can never start on — and thus never observes as accepted — a block that has only been prepared-for-
verification (`typeinfo_block_prereqs_ignore_verifying_blocks`, `verifying_typeinfo_block_lease_blocks_dependents`);
`accept` runs strictly after a green workspace gate (`workspace_gate_passes_before_typeinfo_block_acceptance`); and "done"
requires both `Lifted` rows AND the block's required / Critical-rule guards present
(`landed_typeinfo_blocks_have_required_guards_and_workspace_gate`) — never row status alone. Because blocks may land in
PARALLEL under the multi-agent / handoff model, a concurrent `accept` can no longer silently lose a landed token or clobber
another agent's in-flight cursor: every typeinfo `.cutover-state` write is locked (on the stable sibling lockfile, never the
atomically-replaced state file) + atomic-rename + `revision`-CAS over an `active_blocks` lease map
(`cutover_state_typeinfo_writes_are_locked_and_cas`, `parallel_typeinfo_block_landing_preserves_all_tokens`,
`resume_rejects_stale_typeinfo_block_lease`).

## 13. The `.cutover-state` `[typeinfo_parity]` namespace

`.cutover-state` remains the execution cursor, but the typeinfo-parity cutover tokens are NAMESPACED and isolated from the
legacy top-level cutover tokens. The typeinfo block tokens live under `.cutover-state.typeinfo_parity.landed_blocks`, NOT
in the top-level `landed_blocks` / `active_block` keys. Namespacing (not migrating or resetting the legacy tokens) is a
required U0 deliverable: the legacy top-level tokens keep their existing meaning untouched, and every typeinfo guard / xtask
/ resume read targets only the `typeinfo_parity` namespace. The manifest is semantic progress; `.cutover-state.typeinfo_parity`
is in-flight orchestration state; they must agree before a block is accepted, enforced by named guards.

The typeinfo-parity execution state is PARALLEL-SAFE (multiple agents may execute different blocks concurrently). A single
`active_block` scalar plus read-modify-write landing is not safe: two agents landing different blocks (or one dispatching
while another lands) can clobber the cursor or LOSE a landed token. The `[typeinfo_parity]` section therefore carries a
`revision` counter (bumped on every write) and an `active_blocks` MAP keyed by block id (each entry a full LEASE, supporting
MULTIPLE concurrently-active blocks), NOT a single scalar. Every typeinfo write goes through `xtask cutover-state typeinfo …`,
which serializes concurrent writers with file locking, publishes atomically via atomic rename, and applies a CAS on `revision`.

### 13.1 TOML schema (both namespaces in one file)

The top-level legacy keys keep their single `active_block` / `landed_blocks`; the `[typeinfo_parity]` section is parallel-safe.

```toml
active_block = ""
landed_blocks = [...]   # legacy tokens, e.g. "0", "1.6", "6.i"

[typeinfo_parity]
revision = 0            # monotonically increasing CAS token; bumped on every typeinfo write
landed_blocks = []      # typeinfo parity block IDs, e.g. "U2.RELATION_INFER"

# active_blocks is a MAP keyed by block id (NOT a single active_block scalar):
# multiple blocks may be concurrently active. Each entry is a full LEASE.
[typeinfo_parity.active_blocks."U2.RELATION_INFER"]
lease_id = "01J…ULID"   # unique per lease acquisition (fresh ULID/UUID each dispatch)
owner_id = "agent-7"    # the agent/process that holds the lease
acquired_revision = 0   # the `revision` value at which this lease was acquired
heartbeat_unix = 0      # last heartbeat (wall-clock unix seconds); refreshed while alive
expiry_unix = 0         # EXPIRED if now_unix > expiry_unix with no fresher heartbeat (heartbeat_unix + lease_ttl)

# Durable GATE RECEIPT: gate-receipt persists this on green; accept consumes it as its gate-pass
# precondition (recomputing pre-accept Verifying hash, applying only the deterministic Verifying->Lifted
# transition, verifying post-accept Lifted hash, all under the lock), or refuses. Bound to the PAIR.
[typeinfo_parity.gate_receipts."U2.RELATION_INFER"]
block_id = "U2.RELATION_INFER"
lease_id = "01J…ULID"              # must match the lease being accepted
[typeinfo_parity.gate_receipts."U2.RELATION_INFER".workspace_gate_input_hash]
pre_accept_verifying_hash = "blake3:…"  # full gated input with rows in their VERIFYING form
post_accept_lifted_hash = "blake3:…"    # deterministic post-accept state (rows Verifying->Lifted, no count change)
command_set = ["cargo test --workspace --tests --verbose", "cargo clippy --workspace -- -D warnings", "cargo fmt --all --check", "pnpm test", "pnpm install --frozen-lockfile"]
success = true                      # accept refuses unless success == true (BOTH the Rust AND the JS gate green) and BOTH paired hashes match

# Durable REVIEW-LAND VERDICT RECEIPT: review-receipt persists this on an all-LAND / NITs-only verdict; accept
# consumes it as its SECOND precondition (alongside the gate receipt), bound to the SAME pre_accept_verifying_hash
# so a stale review cannot authorize a land. Panel is EXACTLY three: 1 Claude Code + 2 codex, all bad-mood,
# best-architecture-no-compromises / breaking-changes-allowed mandate.
[typeinfo_parity.review_receipts."U2.RELATION_INFER"]
block_id = "U2.RELATION_INFER"
lease_id = "01J…ULID"                       # must match the lease being accepted
pre_accept_verifying_hash = "blake3:…"      # same gated input the workspace gate ran against (covers JS-relevant inputs)
panel = ["claude-code", "codex", "codex"]   # EXACTLY 1 Claude Code + 2 codex
verdicts = ["LAND", "LAND", "LAND"]         # all three LAND, OR all residual findings are NITs-only (P3-class)
residuals_nits_only = true                  # any open material (P0/P1/P2-class) finding from ANY reviewer blocks the land
# accept refuses unless all three reviewers returned LAND/NITs-only, residuals_nits_only == true, and the
# recomputed pre_accept_verifying_hash matches the live Verifying input.

# Durable LAND RECORD: accept persists this so the single squashed mainline land commit is content/identity
# bound to the accepted state (§11.11). It is the trailer source the git-history guard reads — the values are
# COPIED from the gate/review receipts at accept (non-circular), so the token <-> commit bijection is
# machine-checkable, not a prose intent.
[typeinfo_parity.land_records."U2.RELATION_INFER"]
block_id = "U2.RELATION_INFER"
lease_id = "01J…ULID"                       # Typeinfo-Lease trailer value
pre_accept_verifying_hash = "blake3:…"      # Typeinfo-Block/Pre-Accept-Verifying-Hash trailer value (same hash the gate + review receipts bind to)
post_accept_lifted_hash = "blake3:…"        # whole-gated-input hash; the §11.3 accept-recheck operand (copied from the gate receipt) — NOT the content-projection comparison target
post_accept_lifted_tree_hash = "blake3:…"   # content-only CANONICAL PROJECTED CONTENT hash over the post-accept tracked tree MINUS the .cutover-state* cursor family (NOT a whole-worktree tree id); the land commit's same projection MUST equal THIS (§11.11)

# Durable PENDING-TRANSACTION JOURNAL / WAL: each phase writes its entry BEFORE renaming any artifact
# and clears it only after the phase's final done-edge write commits. Every under-lock observation
# reconciles a present entry FIRST — completing a valid partial accept (matching receipt + matching
# before_hash) or rolling back to before_hash.
[typeinfo_parity.pending_transactions."U2.RELATION_INFER"]
phase = "accept"                    # "prepare-verify" | "accept" | "abort"
block_id = "U2.RELATION_INFER"
lease_id = "01J…ULID"
before_hash = "blake3:…"           # whole under-lock snapshot BEFORE the transaction (the roll-back target)
after_hash = "blake3:…"            # intended whole under-lock snapshot AFTER commit (the completion target)
intended_transitions = ["rows U2.RELATION_INFER Verifying->Lifted", "landed_blocks += U2.RELATION_INFER", "release lease U2.RELATION_INFER"]
```

### 13.2 Namespaced xtask command (two-phase acceptance)

Typeinfo orchestration uses an explicit namespaced subcommand — `xtask cutover-state typeinfo dispatch <block-id>`,
`heartbeat <block-id>`, `adopt <block-id>`, `prepare-verify <block-id>`, `gate-receipt <block-id>`,
`review-receipt <block-id>`, `accept <block-id>`, and
`abort <block-id>` — reading and writing ONLY the `[typeinfo_parity]` section's FULL subtable set —
`revision` / `active_blocks` / `landed_blocks` / `gate_receipts` / `review_receipts` / `land_records` /
`pending_transactions` (and the block's manifest row status / expected count). `dispatch` ACQUIRES a lease (minting a fresh `lease_id`, recording
`owner_id` / `acquired_revision` / `heartbeat_unix` / `expiry_unix`); `heartbeat` REFRESHES the holder's heartbeat; `adopt`
takes over an EXPIRED lease by replacing its `lease_id` + `owner_id` (only when expired, under the CAS). `gate-receipt` RUNS
the full workspace gate (the complete Rust **AND** JavaScript gate — `cargo test --workspace --tests --verbose`,
`cargo clippy --workspace -- -D warnings`, `cargo fmt --all --check`, `pnpm test`, `pnpm install --frozen-lockfile` — green only
when BOTH pass) for the `Verifying` block and, on green, PERSISTS the durable receipt bound to the input-hash PAIR (whose
`pre_accept_verifying_hash` covers the JS-relevant tracked inputs too, so the receipt binds the JS gate). `review-receipt` RUNS the
three-reviewer LAND panel (§11.12) and, on an all-LAND / NITs-only verdict, PERSISTS the durable review-LAND receipt bound to the
same input-hash pair. `accept` consumes BOTH receipts (or runs the gate inline plus consumes the review-LAND receipt) and PERSISTS the
`land_records` entry — its trailer fields (`block_id` / `lease_id` / `pre_accept_verifying_hash`) COPIED from the gate + review receipts, plus the
content-only `post_accept_lifted_tree_hash` (the canonical projection over the tracked tree MINUS the `.cutover-state*` cursor family — §11.11) — BEFORE the final observable done-edge (the `landed_blocks` append + lease release), so the single
squashed land commit is content/identity bound to the accepted AUTHORED state (§11.11). The legacy `xtask cutover-state land <block-id>` / `dispatch <block-id>`
commands (the top-level cutover) continue to touch ONLY the top-level keys and are unaffected.

There is NO single `land` command and NO compensating-revert-after-land path: acceptance is `prepare-verify` → gate →
`accept`/`abort`. Each phase is a LOCKED, CRASH-RECOVERABLE STATE TRANSACTION (under `.cutover-state.lock`; under-lock
snapshot + precondition recheck that FIRST reconciles any pending journal entry; each phase WRITING its journal entry before
renaming any artifact and CLEARING it after the final done-edge write; each artifact written by its own temp-rename; the
`.cutover-state` write `revision`-CAS-guarded; the lease keep/release + file-write order chosen so every intermediate is
non-done).

### 13.3 Locking, whole-file round-trip, and legacy-deletion lifecycle

- **Locked (STABLE SIBLING lockfile), atomic per-file, CAS-guarded writes.** Every typeinfo write MUST take a file lock on
  `.cutover-state.lock` — NEVER on `.cutover-state` itself (the state file is atomically REPLACED on every write, so a lock on
  its inode is silently dropped at the rename and serializes nothing). The `.cutover-state` write publishes atomically via
  atomic rename and applies a CAS on `[typeinfo_parity].revision` (read, compute, commit only if unchanged, bump, retry on
  miss). The accompanying manifest-row transitions and source-`#[ignore]` edits are written as their OWN sibling temp-renames
  within the SAME locked transaction, ordered so the `.cutover-state` `landed_blocks`/lease write is the LAST, observable
  done-edge. Pinned by `cutover_state_typeinfo_writes_are_locked_and_cas` and `parallel_typeinfo_block_landing_preserves_all_tokens`.
- **Precise staleness predicate (NOT "any newer revision").** The four-clause predicate of §11.5. Pinned by
  `resume_rejects_stale_typeinfo_block_lease`.
- **Whole-file round-trip.** The xtask must round-trip the ENTIRE file (preserve BOTH namespaces on every write); a write
  through either command must leave the other namespace byte-faithful.
- **Legacy guards stay structural on top-level keys.** The existing legacy `.cutover-state` guards parse the top-level
  `active_block` / `landed_blocks` STRUCTURALLY (not by whole-file text-scan) and IGNORE the `[typeinfo_parity]` section
  entirely, so the new namespace is invisible to them.
- **Legacy-deletion lifecycle.** Because the typeinfo-parity tokens share the SAME file under `[typeinfo_parity]`, legacy
  completion must NOT delete the file while `[typeinfo_parity]` is active or non-empty. Legacy completion may CLEAR/RETIRE only
  the TOP-LEVEL state; it MUST leave the `[typeinfo_parity]` section byte-faithful and MUST NOT delete the file while
  `.typeinfo_parity.active_blocks` is non-empty or `.typeinfo_parity.landed_blocks` is non-empty. The file may be DELETED only
  when BOTH namespaces are retired/empty. Symmetrically, retiring the typeinfo namespace never deletes the file while the legacy
  top-level state is live. Pinned by `legacy_cutover_completion_preserves_typeinfo_namespace_when_active`.

### 13.4 Namespace / parallel-safety / two-phase guards

Beyond the transaction-contract guards (§11.8), the namespace and parallel-safety surface is enforced by:
**`cutover_state_typeinfo_namespace_isolated_from_legacy_cutover_tokens`** (typeinfo tokens live only under
`.cutover-state.typeinfo_parity`, never the legacy top-level keys; the two token spaces stay disjoint) and
**`legacy_cutover_completion_preserves_typeinfo_namespace_when_active`**.

## 14. Resume protocol (lease-based, parallel-safe)

The resume protocol is LEASE-BASED and PARALLEL-SAFE (multiple agents may run it concurrently). A fresh agent:

1. Read `semantic-db-overhaul-unified-remaining-plan.md`, `native-typeinfo-parity.md`, and the `.cutover-state.typeinfo_parity`
   namespace (its `revision`, `active_blocks` map, and `landed_blocks`).
2. Run the manifest guard test first.
3. Take `.cutover-state.lock` and inspect `active_blocks` (a MAP, not a single cursor) AS PART OF THE UNDER-LOCK WHOLE
   SNAPSHOT (rows + token + lease + source-`#[ignore]` state read together — §11.1). For each lease, decide LIVE vs STALE by
   the four-clause predicate (§11.5), NOT "any newer revision". Resume a LIVE lease this agent owns idempotently, or ADOPT an
   EXPIRED one via `xtask cutover-state typeinfo adopt <block-id>`; do NOT blindly resume a stale lease and do NOT adopt a
   still-live lease held by another agent.
4. Otherwise — still under the lock, reading the same whole snapshot — choose the first eligible `TYPEINFO_PARITY_BLOCKS`
   block by the COMPOSED selector §11.10 defines (so both kinds of block have an unambiguous next-actionable). A block is
   eligible iff its prereqs are DONE by the four-part predicate (§11.7) AND it has no LIVE lease held by another agent
   (including a lease whose block is mid-`Verifying`), AND:
   - for a **row-owning block** — its own rows still have `status == Ignored`; OR
   - for a **zero-row block (§11.10)** (`U0.MANIFEST_SUBSTRATE`, `U2.QUERY_VALUE_DOMAIN`, `U8.WIRE_SURFACE_CLOSURE`,
     `U12.EXPORTER`, `U13.PROJECTION`) — it is NOT done by the four-part predicate (§11.7) AND its
     `.cutover-state.typeinfo_parity.landed_blocks` token is ABSENT (a zero-row block owns no rows, so its eligibility is
     token/done-predicate driven, never row-status driven).

   Then ACQUIRE its lease via `xtask cutover-state typeinfo dispatch <block-id>` (a locked, atomic-rename, `revision`-CAS
   write). If the CAS loses to a concurrent agent, re-read and pick the next eligible block.
5. Execute exactly that block contract (refreshing the lease via `xtask cutover-state typeinfo heartbeat <block-id>` while the
   work is in flight).
6. Dry-run the block's tests (the exact lifted-row proofs) to confirm they pass before committing anything — WITHOUT any source
   `#[ignore]` edit: run them either via `cargo test … -- --ignored` (or the equivalent generated-wrapper invocation, which
   executes the row's declared proof) so the still-`Ignored` rows execute WITHOUT removing their source `#[ignore]`s, OR defer
   the proof run until AFTER `prepare-verify` strips them. There is NO "remove the `#[ignore]`s locally before `prepare-verify`"
   step — source `#[ignore]` removal happens ONLY inside `prepare-verify`'s locked transaction. Do NOT yet `prepare-verify`,
   `accept`, append a landed token, or change the count.
7. PHASE 1 — `prepare-verify` as a LOCKED STATE TRANSACTION: takes `.cutover-state.lock`, re-reads `.cutover-state` + manifest +
   source-`#[ignore]` state and revalidates preconditions (the lease live), then writes each artifact by temp-rename (the
   `.cutover-state` write `revision`-CAS-guarded). The row work BRANCHES by block kind (§11.10):
   - for a **row-owning block** — revalidate this block's rows `Ignored` with `#[ignore]`s present, then remove the exact source
     `#[ignore]`s, flip this block's rows to `Verifying { block_id, lease_id }`, AND set `EXPECTED_TOTAL_IGNORED_COUNT =
     count(status == Ignored)` (all in the SAME locked transaction), WITHOUT appending to `landed_blocks`;
   - for a **zero-row block (§11.10)** — record ONLY the lease + phase, with NO row-status transition, NO
     `EXPECTED_TOTAL_IGNORED_COUNT` change, and NO source-`#[ignore]` edit (it owns no rows), WITHOUT appending to `landed_blocks`.

   Either branch KEEPS the lease LIVE. After it, the block is observable only as in-flight-verifying; prereqs/parent-completion
   treat its `Verifying` rows (row-owning) — and, for a zero-row block, its still-absent `landed_blocks` token — as NOT done.
   The gate (step 8) and `accept` (step 9) run for BOTH block kinds.
8. Run the full workspace gate — the complete Rust **AND** JavaScript gate, green only when BOTH pass: Rust
   (`cargo test --workspace --tests --verbose`, `cargo clippy --workspace -- -D warnings`, `cargo fmt --all --check`) and
   JavaScript (`pnpm test`, `pnpm install --frozen-lockfile`) — WHILE the block is `Verifying` (the
   `#[ignore]`s already removed; the gate's status-sensitive checks treat each `Verifying` row as its post-accept `Lifted` form —
   §11.3) — via `xtask cutover-state typeinfo gate-receipt <block-id>`, which runs the gate and, on GREEN, PERSISTS the durable
   receipt bound to the input-hash PAIR (whose `pre_accept_verifying_hash` covers the JS-relevant tracked inputs —
   `package.json` / lockfile / TS sources — so the receipt binds the JS gate too). The gate runs strictly BETWEEN
   `prepare-verify` and `accept` — never before `prepare-verify`, never skipped by `accept`. (Equivalently `accept` runs the gate
   inline on green and skips the persisted receipt.) Then run the three-reviewer LAND panel (§11.12) via
   `xtask cutover-state typeinfo review-receipt <block-id>`, persisting the durable review-LAND receipt bound to the same
   input-hash pair; `accept` (step 9) consumes BOTH receipts. WIP commits made during the block do NOT run this gate — only the
   block-done gate here does (§11.11).
9. PHASE 2 — on a GREEN gate AND an all-LAND / NITs-only three-reviewer verdict (§11.12) AND no unresolved design fork (§14.1),
   `accept` as a LOCKED, CRASH-RECOVERABLE STATE TRANSACTION: takes the lock, re-reads the artifacts — FIRST reconciling any
   pending journal entry — and WRITES a `phase = "accept"` journal entry BEFORE renaming any artifact, then writes each artifact
   by temp-rename (the `.cutover-state` write `revision`-CAS-guarded), with the `landed_blocks` append + lease release (the
   observable done-edge) LAST. Both block kinds keep the gate receipt + the review-LAND receipt bound + the journal + the
   done-edge-LAST ordering; the row work BRANCHES by block kind (§11.10):
   - for a **row-owning block** — revalidate preconditions (rows `Verifying` under this lease AND the durable gate receipt
     present + bound with the RECOMPUTED pre-accept `Verifying` hash MATCHING — or inline green gate — AND the durable review-LAND
     receipt present + bound with its recomputed `pre_accept_verifying_hash` MATCHING, recording a LAND / NITs-only verdict from
     all three panel reviewers), then write the rows `Verifying → Lifted` FIRST (the ONLY deterministic transition, no further
     count change), VERIFY the resulting state's recomputed whole-input hash EQUALS `post_accept_lifted_hash`, then the
     `landed_blocks` append + lease release LAST. On a RED gate, `abort` as a LOCKED STATE TRANSACTION instead: restores the
     source `#[ignore]`s, restores the rows to `Ignored`, restores the count, and clears the lease — because the block was never
     appended to `landed_blocks` and its rows were `Verifying` (never `Lifted`), no dependent could ever have observed it as
     accepted.
   - for a **zero-row block (§11.10)** (`U0.MANIFEST_SUBSTRATE`, `U2.QUERY_VALUE_DOMAIN`, `U8.WIRE_SURFACE_CLOSURE`,
     `U12.EXPORTER`, `U13.PROJECTION`) — revalidate the live lease AND its `landed_blocks` token ABSENT AND its required /
     Critical-rule guards present AND BOTH receipts (gate + review-LAND) present + bound with their recomputed
     `pre_accept_verifying_hash` MATCHING — then perform NO row-status transition, NO `EXPECTED_TOTAL_IGNORED_COUNT` change, and
     NO source-`#[ignore]` edit (it owns no rows) — then the `landed_blocks` append + lease release LAST. On a RED gate, `abort`
     simply CLEARS the lease: there is no row / `#[ignore]` / count restore because none were touched.

   "Done" additionally requires the block's required / Critical-rule guards present and passing, not row status alone (vacuously
   true for the row part of a zero-row block).
10. When all child blocks of a U-block are done — every row in the parent's UNION-of-child-rows row-set `Lifted` (no child row
    left `Ignored` or `Verifying`) — land the parent U-block token (or, if parent tokens are derived-only, the parent becomes
    done automatically and is never independently landed). A parent is NEVER landed while any child block's rows remain `Ignored`
    OR `Verifying`.

Re-running a partially done block is safe because the manifest tells which rows still need lifting and all cache/query changes
are idempotent under the one-engine guards.

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
- **Work does NOT continue until the fork is decided.** The block stays in its WIP / pre-`accept` state (no `prepare-verify`, no
  gate, no `accept`, no land) until the fork is resolved. An unresolved design fork is an `accept`-blocking condition, DISTINCT
  from the gate receipt (§11.3) and the three-reviewer review-LAND receipt (§11.12).
- **The orchestrator drives this loop 100% autonomously.** Consistent with the §14 resume protocol, it never pauses for a human
  checkpoint on a fork it can route to codex; it routes, iterates to high confidence, then continues the block.
- **Composition with §11.12.** codex deciding a fork ≠ the three-reviewer LAND panel. The fork decision happens DURING the block
  (WIP state); the three-reviewer LAND panel happens at block-done, before `accept`. They are different stages of the same block.

This rule introduces NO new `(CRITICAL)` code rule and NO new mechanical guard — it is an orchestration-process rule for the
driving orchestrator, so it does not trip the R6 meta-guard (which requires a guard only for new `(CRITICAL)` code rules).

---

# Capability Map

The binding total is **363** `IgnoredTestRow`s. (Stale/non-authoritative counts: 356 and 371; 384 is the raw `#[ignore]` line
count including macro/body/non-site lines. U0 rederives 363 with the manifest parser; the manifest is authoritative.) The
substrate row counts below are the `IgnoredTestRow` rows per substrate (they sum to 363). Additional coverage is the CLOSED set
of exactly 7 coverage-only `AdditionalProofRow`s — the 6 JSX no-new-key submatrix rows (`U2.JSX_FOUNDATIONS`) + the 1 mapped
companion `mapped_modifier_minus_optional_preserves_explicit_undefined_on_required_property` (`U2.MAPPED_TEMPLATE`) — held in the
SEPARATE coverage-only `AdditionalProofRow` table (pinned closed-at-7 by `additional_proof_row_table_holds_exactly_7_rows`),
excluded from this 363 count + the source-`#[ignore]` bijection.

| Capability class / manifest substrate | Rows | Architecture component | Owning U-block |
|---|---:|---|---|
| `FlowNarrowing` | 104 | demand-sliced flow/narrowing | U6 |
| `ContextualTyping` | 13 | contextual expected-type propagation | U6 |
| `ValueInference` | 7 | value inference/widening | U6 |
| `CallResolution` | 28 | overload/call/generic inference | U6, with U2 overload key |
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
| `ModuleFeatures` | 9 | merge/ambient/augmentation/modules | U2 |
| `JsxResolution` | 9 | JSX namespace/component resolution | U2/U14 |
| `CrossFileResolution` | 3 | route/import demand facts | U3/U6 |
| `CacheInvalidation` | 6 | fact validation/route invalidation | U3/U10/U11 |
| `AuditFootprint` | 2 | footprint attachment | U11 |
| `DemandBoundary` | 3 | demand/mode audit | U2/U10 |
| `ModeBoundary` | 5 | mode-boundary invariants | U2/U10 |
| `ModernTsFeatures` | 6 | satisfies/await/variance/import attrs | row-level U2/U6 |
| `MacroResolution` | 1 | framework macro graph adapter | U14 |
| `CompositeSurfaces` | 5 | end-to-end adapter surfaces | U15 |

## The un-ignore / guarantee protocol over the 363 rows

The real un-ignore sets must be **row-exact in the manifest**, not inferred from substrate alone. Indicative file groupings:

- **Lifted mainly by U6:** `narrow_*`, `flow_return_*`, `flow_invalidations.rs`, `contextual_typing.rs`, `value_inference.rs`,
  `call_resolution.rs`, `function_advanced.rs`, and flow rows in `substitution_types.rs`.
- **Lifted mainly by U2:** `relation_semantics.rs`, `conditional_infer.rs`, `recursive_conditional.rs`, `tuple_labels.rs`,
  `variadic_tuples.rs`, `utility_*`, `indexed_utilities.rs`, `mapped_*`, `template_literal_inference.rs`, `index_signatures.rs`,
  `enums.rs`, `unique_symbol.rs`, `module_features.rs`, the four `decorators.rs` decorator/accessor rows, and pure class/brand rows.
- **Lifted with U3/U10/U11:** `cache_invalidation.rs`, `footprint.rs`, `demand_boundary.rs`, `mode_boundary_invariants.rs`.
- **Finished in U14/U15:** `basic.rs`, `menu_like.rs`, `message_list_like.rs`, `table_like.rs`.

The guarantee over the 363 rows is the composition of: the two-table ledger (§10) with the exact-363 count + bijection (§10.5);
the U0 row-exact capability→mechanism→proof coverage table (§10.4) that DEFINES completeness mechanically; the per-row executable
`ProofRequirement` with the generated proof registry + row-test wrapper (§10.2, §10.3); the two-phase prepare-verify → gate →
accept lifecycle with the input-hash-bound gate receipt (§11); the no-skip guarantee (§12); and the lease-based, parallel-safe
resume protocol (§14). A block lifts only its exact manifest rows, can enter `Verifying` only after its coverage is complete +
non-placeholder, and reaches `Lifted`/`landed_blocks` only after a green workspace gate over the exact accepted content — so the
363-row parity is mechanically tracked from `Ignored` to `Lifted`, never skipped and never vacuously satisfied.

---

# Guards index

The named architecture guards introduced by this plan, grouped. All guards are registered in their owning guard set and
cross-referenced from the section that introduces them. Per the R6 meta-guard, every `(CRITICAL)` rule this architecture
introduces lands with at least one named guard here.

## Type IR — `GraphTypeNode` / wire-surface purity

- `graph_type_node_oneof_contains_only_type_value_arms`
- `graph_type_node_allowlist_arms_have_type_value_classification`
- `no_non_type_value_smuggled_into_graph_type_node`
- `typeinfo_wire_surface_has_no_retired_concept_fields`
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

- `resolve_merged_declaration_same_site_different_env_or_context_do_not_warm_hit`
- `resolve_ambient_namespace_same_site_different_env_or_context_do_not_warm_hit`
- `resolve_overload_set_same_site_different_env_or_context_do_not_warm_hit`
- `resolve_enum_same_site_different_env_or_context_do_not_warm_hit`
- `flow_narrowing_at_same_point_different_env_flow_or_substitution_do_not_warm_hit`
- `contextual_type_at_same_point_different_env_contextual_or_substitution_do_not_warm_hit`
- `declaration_augmentation_key_same_site_different_env_or_context_do_not_warm_hit`
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
- `semantic_query_key_spec_table_equals_enum` (the mechanical enum/table-equality meta-guard, replacing the soft
  `every_semantic_query_key_has_explicit_context_and_cross_context_warm_hit_guard`)
- plus dispatch-completeness and schema-version guards for any public wire arm

## Flow — peeker + cycle

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

## Reducers — mapped optionality

- `mapped_minus_optional_strips_only_optional_origin_undefined`
- `mapped_minus_optional_preserves_explicit_undefined_on_required_property`

## Performance budgets — non-admission

- `relation_budget_exceeded_admits_nothing`
- `keyspace_budget_exceeded_admits_nothing`
- `call_resolution_budget_exceeded_admits_nothing`
- `apparent_type_budget_exceeded_admits_nothing`

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
- `ignored_test_row_table_holds_exactly_363_rows`
- `additional_proof_row_table_holds_exactly_7_rows`
- the source-`#[ignore]` ↔ `Ignored`-rows ↔ `EXPECTED_TOTAL_IGNORED_COUNT` bijection/count guards
- `no_landed_typeinfo_block_has_live_ignored_rows`

## Cutover / ledger — transaction contract + namespace / parallel-safety

- `typeinfo_state_snapshots_are_locked_and_precondition_checked`
- `pending_typeinfo_transactions_reconciled_before_eligibility`
- `block_cannot_enter_verifying_without_complete_coverage`
- `typeinfo_block_prereqs_derive_from_manifest_status`
- `typeinfo_block_prereqs_ignore_verifying_blocks`
- `verifying_typeinfo_block_lease_blocks_dependents`
- `workspace_gate_passes_before_typeinfo_block_acceptance`
- `landed_typeinfo_blocks_have_required_guards_and_workspace_gate`
- `cutover_state_landed_blocks_match_typeinfo_manifest`
- `no_vacuous_parent_u_block_landing`
- `zero_row_blocks_land_exactly_once`
- `typeinfo_parity_block_dag_is_acyclic_and_consumed_keys_and_mechanisms_are_prereqs`
- `typeinfo_block_lands_as_single_squashed_commit`
- `typeinfo_block_accept_requires_review_land_verdict`
- `cutover_state_typeinfo_writes_are_locked_and_cas`
- `parallel_typeinfo_block_landing_preserves_all_tokens`
- `resume_rejects_stale_typeinfo_block_lease`
- `cutover_state_typeinfo_namespace_isolated_from_legacy_cutover_tokens`
- `legacy_cutover_completion_preserves_typeinfo_namespace_when_active`

---

# Deliverables / legacy

- **Pin the oracle toolchain.** `package.json` currently declares
  `"@typescript/native-preview": "latest"`. `"latest"` is not a durable oracle contract — the manifest dependency must be pinned
  to the exact oracle version (`7.0.0-dev.20260526.1`) in `package.json`. This is a required deliverable (and a legacy fix:
  replace the floating `"latest"` range).
- **The oracle row generator** (deterministic `OracleId`, checked-in normalized snapshots, feature/env-gated regeneration) is a
  required deliverable; the `tsgo`-execution-forbidden guard for runtime/default tests is a required deliverable.
- **Namespacing the `.cutover-state` `[typeinfo_parity]` tokens** (not migrating or resetting the legacy tokens) is a required U0
  deliverable, along with the two-namespace TOML schema, the namespaced two-phase xtask command, and the legacy-deletion
  lifecycle (§13).
- **The generated artifacts** — the `SemanticQueryKeySpec` table (§2.9), the proof registry + typed row-test wrapper (§10.3), and
  the U0 row-exact coverage table (§10.4) — are each produced by a dedicated `cargo run` generator and checked in (generated, not
  hand-maintained).

# Cross-reference / doc-update obligations

These doc-update obligations land as part of the work. The four against the
**unified plan** (`semantic-db-overhaul-unified-remaining-plan.md`) — owned by the
unified-plan integration step (the U0-time reconciliation of the unified plan with
this architecture) — have been **APPLIED**: the unified plan now indexes this
parent and all four children, carries the two-table-ledger U0 entry, requires all
363 `IgnoredTestRow`s `Lifted`, and registers the five added keys + the generalized
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
    the separate coverage-only `AdditionalProofRow` table — §10.1), `IgnoreStatus` (`Ignored` / `Verifying` / `Lifted`),
    `ProofRequirement`, the proof registry + row-test wrapper, the §10.4 / §10.4.1 row-exact coverage table, and the
    `.cutover-state.typeinfo_parity` namespace + two-phase prepare-verify → gate → accept transaction contract (§§11–14).
  - **(c) Require ALL 363 `IgnoredTestRow`s lifted in U15 + the §9 terminal checklist (not a majority/fraction) — DONE.** The
    unified plan's U15 + §9 terminal checklist now require EVERY one of the 363 `IgnoredTestRow`s `Lifted` (zero parity ignores),
    with the ONLY permitted residual `#[ignore]`s being the registered Svelte/React STOP-gate files (which are not among the 363) —
    the terminal acceptance §10.5 / §13 / `all_typeinfo_parity_rows_lifted_except_stop_gates` define.
  - **(d) Register the new query surface — DONE.** The unified plan registers all five new query keys (`FlowReturn`,
    `ResolveClassSurface`, `ApparentType`, `TemplateLiteralReduce`, `ResolveCall`) in its U2/U6 sections; registers the GENERALIZED
    augmentation key (the seventh U2 variant is `ResolveDeclarationAugmentation { target: Module | Global, context:
    DeclarationAnalysisContext }`, not the former `ResolveModuleAugmentation`); and reconciles wording implying a uniform type-node
    query result to the typed
    `SemanticQueryValue` value-domain layer.
- **U6 doc** (`native-flow-return.md`) — update for the new query keys and the demand-sliced `ReturnPathPeeker` (two-frontier
  model) that amend it.
- **Recovered foundation doc** (`semantic-type-graph-plan-recovered.md`) — amend the self-contradictory stale wording so the doc
  is not internally inconsistent: the stale `NoInfer` declaration-metadata wording → occurrence-local; the
  `decorators.rs — UnsupportedConstruct::Decorator + diagnostic projection` line → the class-surface ruling (§1.7); the §2.17 /
  §3.11 flow/contextual `TypeNode::FlowNarrowing` / `TypeNode::ContextualType` placements → `ProgramAnalysisGraph` payload entries;
  the stale `TypeNode::RelationProof` wording → `RelationPayload` / payload-side proof table (tag 28 retired/`reserved`); the stale
  JSX `ResolveJsxIntrinsicElement` / `ResolveJsxAttribute` / `TypeNode::JsxIntrinsicElement` wording → the existing-query JSX
  mechanism (§8); and the stale module/global augmentation placements (the §8 exporter `TypeNode::ModuleAugmentation` DTO, the §3
  `module_augmentation = 23` / `global_augmentation = 25` type-value arms, and the §2.17 `module_augmentation_is_public_graph_state`
  guard) → `DeclarationAnalysisGraph` on `TypeInfoGraphPayload.declaration_surfaces` + `SemanticQueryValue::DeclarationAnalysis`,
  with `module_augmentation_is_public_graph_state` RETIRED/REPLACED by the declaration-surface guards (`merged_declarations_are_public_graph_state`
  / `ambient_namespaces_are_public_graph_state` / `overload_sets_are_public_graph_state` are UNCHANGED — only the two augmentation
  facts relocate). Pinned by `flow_contextual_doc_and_wire_placement_match_program_analysis_graph`,
  `declaration_augmentation_doc_wire_query_placement_match`, and the value-domain / wire-surface guards.

---

## Resolved positions (carried forward)

- **Count:** 363 is binding. U0 rederives it with the manifest parser. 356/371 are stale; 384 is the raw line count.
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
