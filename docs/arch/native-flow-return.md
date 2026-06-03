# Native Typeinfo Parity — U6 Native Flow Return

Parent architecture: docs/arch/native-typeinfo-parity.md
Sequencing authority: docs/arch/semantic-db-overhaul-unified-remaining-plan.md
Owning U-block(s): U6
Prerequisites: U2, U4
Consumers: U8, U11, U13, U15
Progress ledger: crates/verter_session/tests/typeinfo_ignored_test_manifest.rs

---

## Scope and authority

This child subplan owns **U6 — Native Flow Return**: the demand-sliced
function-body flow resolver, the `ReturnPathPeeker` two-frontier slicing model, the
flow IR, the `SemanticQueryKey::FlowReturn` query node, narrowing / contextual /
value-inference facts, predicate / assertion signature effects, and call /
overload / generic-inference resolution as it drives flow. It cites — never
restates — the parent for the engine architecture (PART 1 §5 owns the
cross-cutting flow contract; this chapter is the U6 implementation detail).

The parent is the architecture authority. Every block below cites the parent
section that defines the architecture it implements and states only the concrete
**block contract** — what changes, what is deleted, which named guards land, which
exact manifest rows lift, and how it is verified. No block restates the engine
spec, the per-key cache-soundness rules, the demand-slice budget contracts, or the
wire-purity closure; those live in the parent and are referenced by section number.

Every block contract uses the parent's per-block contract template (PART 2 §9).
"Done" for any block is the parent's four-part done predicate (PART 2 §11.7); a
block may enter `Verifying` only after its rows' coverage is complete and
non-placeholder (PART 2 §10.4); acceptance is the two-phase prepare-verify → gate
→ accept lifecycle over the `.cutover-state.typeinfo_parity` namespace (PART 2
§§11–14). None of that machinery is re-specified here.

### Prerequisite relationship (dependency, not a competing schedule)

- **Prerequisite U2** finalizes the one `SemanticQueryKey` identity SHAPE (slot
  identity) and the typed `SemanticQueryValue` value-domain layer, and lands the
  seven U2 keys plus the three U2-landed added keys (`ResolveClassSurface`,
  `ApparentType`, `TemplateLiteralReduce`) — each as enum variant **+**
  `SemanticQueryKeySpec` row **+** dispatch behavior together. U2 finalizes the
  identity SHAPE/model for `FlowReturn` / `ResolveCall`; it does NOT pre-register
  their spec rows or enum variants. `SemanticQueryKey::FlowReturn` /
  `SemanticQueryKey::ResolveCall` are **additive** variants in that already-final
  slot-identity shape with **no cache re-key** (unified §5, terminal-checklist
  item 2), landed at U6 (see below). U6 also consumes the U2 reducers it
  dispatches into:
  `Relate` (narrowing / call-argument assignment), `ResolveOverloadSet` and
  `ResolveCall` (call resolution), `ResolveClassSurface` / `ApparentType`
  (method / apparent-member projection), `TemplateLiteralReduce`, `IndexedAccess`,
  `KeyOf`, `MappedType`, `Conditional`.
- **Prerequisite U4** provides the persistent / cache-runtime node substrate. The
  flow slice-hash and slice-lowered-body artifacts are **B4-style cache-runtime
  nodes** (`FlowSliceHashNode`, `FlowSliceLoweredBodyNode`), not bespoke
  `FileArtifactStore` side maps.
- **This block U6** adds the flow nodes and the `SemanticQueryKey::FlowReturn` and
  `SemanticQueryKey::ResolveCall` query nodes — each as enum variant **+**
  `SemanticQueryKeySpec` row **+** dispatch behavior landed together in this same
  block (in the U2-finalized slot-identity shape, no re-key), so the standing
  meta-guard `semantic_query_key_spec_table_equals_enum` stays green incrementally
  after U6. Every flow-return answer routes
  through the **one shared resolver**: `SemanticQueryKey::FlowReturn →
  ProjectSemanticDispatch::execute → SemanticGraphStore`. There is no second
  resolver, no per-surface walker, and no query-time OXC walking.
- **Consumers U8 / U11 / U13 / U15** read flow results. Published shape projection
  (`TypeDescriptor`, the typeinfo graph wire surface) is deferred to **U13**;
  flow-narrowing / contextual-type *facts* live in the U8 `ProgramAnalysisGraph`,
  never in published `GraphTypeNode`. The audit envelope lives in the
  `verter_audit` leaf substrate. The host API returns the landed
  `AuditedResult<T, E>` carrier consumed by the U11 native `TypeInfoSession`
  `_with_audit` methods.

### Block dependency graph (within this subplan)

```
U2.QUERY_VALUE_DOMAIN + U2.RELATION_INFER + U4 (cache-runtime nodes)
        │
        ▼
U6.FLOW_RETURN_SUBSTRATE   (FunctionBodySkeleton + ReturnPathPeeker + slice nodes + FlowReturn dispatch + audit)
        │
        ├─► U6.NARROWING            (typed branch-fact lattice; narrow_* + flow_invalidations)
        │        │
        │        └─► U6.PREDICATE_ASSERTION  (Predicate/Assertion/AssertsCondition signature effects)
        │                 ▲  (also requires U6.CALL_RESOLVE: predicate/assertion effects
        │                 │   read the carrier off the ResolveCall-resolved callee signature)
        ├─► U6.CALL_RESOLVE ─────────┘
        │        │   (ResolveCall + ResolveOverloadSet + generic inference + ReturnType admission)
        │        └─► U6.CONTEXTUAL_CALLBACK  (callback contextual typing + iterative generic inference)
        │
        ├─► U6.VALUE_INFERENCE      (object/spread/as-const/satisfies/value-inference return shapes)
        ├─► U6.ASYNC_GENERATOR      (Awaited / Generator / AsyncGenerator carriers)
        ├─► U6.CROSS_FILE           (resolver_core-routed cross-file flow + flow cycle space)
        └─► U6.LOOP_CLOSURE         (loop fixed-point + FlowSliceBudget + closure-capture barrier)
```

`U6.FLOW_RETURN_SUBSTRATE` is the keystone: every other U6 block consumes the
`FunctionBodySkeleton`, the `ReturnPathPeeker`, the slice cache nodes, and the
`FlowReturn` dispatch it lands. The parent U6 token is an aggregate over every
block below (PART 2 §11.9): U6 is done only when every row in the union of all
U6-block row-sets is `Lifted`. The flow-return catalog under
`crates/verter_session/src/typeinfo/typeinfo_tests/` (`flow_return_catalog.rs`,
`flow_return_edge_catalog.rs`, `flow_return_parity_contracts.rs`,
`flow_return_path_contracts.rs`, `flow_invalidations.rs`) is the U6 capability
surface; its `#[ignore]` rows are lifted as the resolver capabilities they
characterize land.

---

## The U6 flow architecture (per parent §5)

The flow engine is **demand-sliced**. A full lowered body is not good enough — the
load-bearing acceptance case is that `ReturnType<typeof myType>['b']` must resolve
the `b` member without loading sibling `a` or its type `Mytype`. The cross-cutting
contract is owned by the parent (`docs/arch/native-typeinfo-parity.md` §5); this
chapter implements it. The architecture below is the implementation expansion of
that section.

### Three artifacts: skeleton, peeker, slice

**`FunctionBodySkeleton`** (in / under `IndexedReady`): an arena-free, shallow
statement / control skeleton with a return-site index, a lexical binding index,
and assignment / kill summaries. **No type lowering happens in the skeleton** — it
is a structural index produced once during shallow analysis, the same density as
the rest of `IndexedReady`. It carries enough structure for the peeker to compute a
slice without re-opening the OXC arena and without resolving any type.

**`ReturnPathPeeker`** is a path-contribution algorithm over the shallow
`FunctionBodySkeleton`. Given `ReturnProjectionDemand { path, terminal_mode }`, it
builds a `ReturnSlicePlan` using **two distinct frontiers** so demand-slicing stays
sound under effects:

- **Value-provider frontier** — computes which sources provide the demanded value.
  For each return site and demanded path `P`, it computes value contributors by
  reverse-walking only the returned expression, reaching definitions,
  path-affecting assignments, branch predicates, and call effects. This frontier
  MAY stop at a definite-present write for `P[0]`.
- **Effect frontier** — stays open even past a definite-present write. It scans
  earlier evaluated expressions for assignments, assertion calls, known local
  closure effects, abrupt completion, and control-flow effects that can affect the
  selected value expressions. A sibling property whose value type cannot be lowered
  must still contribute its **effect summary** when that effect changes a binding
  read by the selected path. The effect frontier MUST also sweep two effect classes
  the value-provider frontier skips because their value is overwritten later —
  **evaluation effects survive a definite write even though value materialization
  does not**:
  - **Computed property-name expressions.** A computed key `[expr]: v` EVALUATES
    `expr` for its side effects (and to order the key) regardless of whether that
    property's value is later overwritten or is not the demanded path. If `expr`
    assigns, narrows, or calls into a binding the selected path reads, the effect
    frontier includes that computed-key evaluation effect — even when the property
    it names is non-contributing for value. Only its evaluation effect is taken;
    the selected value is not materialized from it.
  - **Spread / `Object.assign` evaluation effects.** A spread `...src` or an
    `Object.assign(target, src)` EVALUATES `src` (and reads its enumerable own
    keys) for side effects even when a later definite write to `P[0]` makes it
    non-contributing for the demanded value. If evaluating `src` (a getter
    invocation, a call expression, an assignment sub-expression) affects a binding
    read by the selected path, the effect frontier carries that evaluation effect
    past the definite write; only the spread's value contribution is skipped.

  The rule for both: a definite later write suppresses VALUE materialization of the
  overwritten / non-selected property, but it does NOT suppress the EVALUATION
  EFFECT of a computed-key expression or a spread / `Object.assign` source that
  already ran and mutated a binding the selected path depends on.

The two-frontier rule is required for soundness. Demanding `["b"]` in
`return { a: (x = "s"), b: x.toUpperCase() }` must NOT lower sibling `a`'s value
type but MUST include `a`'s effect summary, because `a`'s initializer assigns `x`
and `x` is read by the selected `b`. The value-provider frontier supplies `b`; the
effect frontier carries the `x = "s"` write that retypes `x` before
`x.toUpperCase()`.

Contribution-scan rules (parent §5): object literals and `Object.assign` scan write
sources right-to-left for `P[0]` (the value-provider frontier stops only at a
definite-present write; optional / unknown writes enter as `ProjectPath(source, P)`
and earlier candidates remain possible); known unrelated properties are skipped for
value purposes by syntactic key footprint, not value resolution, but the effect
frontier still sweeps them; `return { ...spread, b }` with demand `["b"]` makes
`spread` and sibling `a` non-contributing for value (no type resolution) while the
effect frontier still inspects them (including the spread / `Object.assign`
source's evaluation effect); `const r = { a, b }; return r` inlines the last
reaching definition if `r` is unescaped / unmutated, else includes only writes that
may affect `P` and returns a typed degraded path result on unknown mutation —
**never lowers siblings**; conditional returns run this per return site then join
selected path results with the branch predicates needed for narrowing.

**`FlowSliceHashNode`** hashes only the selected return / control / binding slice. A
full-body hash is allowed only for a true whole-return request and is REJECTED for
member-projection requests. **`FlowSliceLoweredBodyNode`** lowers only the slice
plan into `FlowSliceIR`. **`FlowSliceIR`** carries `FlowStmt`, `FlowExpr`,
`FlowSlotId`, `FlowPath`, `FlowFrame`, `NarrowingFact`, `AliasCorrelation`,
`FlowEffect`, `ReturnAccumulator`, `LoopSummary`. `FlowSlotId` is the
solver-internal SSA slot identity — it is an IR type only and never a public
`TypeExpr` / `GraphTypeNode` variant (`FlowSlot` is forbidden outside flow IR,
parent §1.1).

### Acceptance example (non-materialization)

```ts
function myType() { const a = new Mytype(); const b = 1; return { a, b } }
type Foo = ReturnType<typeof myType>['b']
```

Resolution must be: `IndexedAccess` threads demand `['b']` into `ReturnType`;
`ReturnType` produces / uses a lazy flow-return root; `ProjectPath` calls
`FlowReturn` with path `['b']`; `ReturnPathPeeker` selects only the `b` property and
`const b = 1`; it does NOT lower `a`, does NOT resolve `new Mytype()`, does NOT load
`Mytype`, and does NOT walk sibling members. The returned literal `1` widens to
`number` at return-position. The `Mytype` non-materialization negative guard asserts
no `ResolveClassSurface`, `TypeOf`, constructor, import, or route fact for `Mytype`
appears.

### Mutual recursion + flow cycle space

Flow is mutually recursive with type reduction: `ReturnType` calls `FlowReturn`;
flow narrowing calls `Relate`; call solving routes through `ResolveCall` /
`ResolveOverloadSet` and `Relate`; return member projection calls `ProjectPath` /
`ProjectMember`; those may re-enter `FlowReturn`. This needs a **separate flow
cycle-id space**. Re-entry is keyed on the FULL normalized
`FlowReturnContext + ReturnProjectionDemand + FlowInputContext` (the contextual
callback input signature plus the relation / call demand mode are included in
`FlowInputContext`), NOT a narrow tuple — the narrow
`(function_slot, substitution_env_hash, projection_path, terminal_mode,
flow_policy)` form can terminate but can also mask a real result with a sentinel
under a different demand (two re-entries differing only in contextual callback input
signature or relation / call demand mode would collide on the narrow key and one
would wrongly receive the other's sentinel). Same-context recursion returns a stable
flow cycle sentinel; it never self-awaits.

### `FlowReturn` key shape and demand-aware cache identity (parent §2.5, §5)

```rust
SemanticQueryKey::FlowReturn {
    function_slot: FlowFunctionSlotIdentity, // U2 ResolvedDeclSlotIdentity + content-free function part
    normalized_type_args: Arc<[SemanticNodeId]>,
    context: FlowReturnContext,              // env + substitution + projection-reduction + flow policy
    demand: ReturnProjectionDemand,          // projection path + terminal mode (its OWN key field)
    input: FlowInputContext,                 // contextual callback input signature + relation/call demand mode
}
```

`FlowReturnContext` includes the five split env hashes (R21 — each a named direct
field, never a bundled `project_config_hash`), the substitution canonical hash,
`ProjectionReductionContext`, and `FlowPolicy`. It does NOT carry
`ReturnProjectionDemand` or `FlowInputContext` — those are the sibling `demand` /
`input` key fields, so the full cache identity is
`FlowReturnContext + ReturnProjectionDemand + FlowInputContext` with **no field
duplicated** across `context` / `demand` / `input`. This canonical key is identical
to the full normalized re-entry identity used by the flow cycle-id space: the cache
key and the cycle-re-entry key are the same normalized identity. The key never
carries a content hash, `parse_stable_hash`, or `fact_dep_signature` (R6/R26 —
version rooting lives on the cached value). Classification: **query-identity cache**
(multi-candidate slot; concurrent variants coexist as candidates under one
`function_slot`-rooted slot, validated by `ReadSetSignature.validate_with_self_roots`
against the caller's live view).

A cached flow result carries **`satisfied_projection`**. `FlowReturn(path=['b'],
Expanded)` cannot satisfy a whole return or `['a']`. A broader result backfills a
narrower entry **only when** the broader computation actually materialised that
narrower path; a narrower result must not pretend broader work is cached.
`Skeleton` remains isolated. The flow fact signature includes
`FlowSlice { function_slot, projection_path, slice_hash, selected_binding_ids,
selected_effect_ids, selected_control_region_ids, closure_summary_ids }`, plus
`MemberPresence`, `Member`, `RouteGeneration`, `ExportSurface`, `ModuleAugmentation`,
`AmbientGlobal`, `LibIntrinsic`, `TypeEnvOptions`, and project-generation facts as
read. The extra `FlowSlice` fields beyond `selected_binding_ids` are required
because effect-only changes (an earlier sibling's assignment, an assertion call, a
closure write summary, a control-flow region) must invalidate a cached slice even
when no selected binding's identity changed. Budget, overflow, cycle, cancellation,
or partial slice results are `ReturnOnly` — never warm-admitted (parent §6).

### Value domain and the typed result

`SemanticQueryKey::FlowReturn` resolves to `SemanticQueryValue::FlowReturn(
Arc<FlowReturnResult>)` (parent §3). The result carries the projected return type,
the read-set fact signature, and any degraded reason. Predicate / assertion effects
are **signature / effect metadata**, not standalone published `GraphTypeNode` type
nodes: a consumer asking for the *return type* of a predicate function gets its
`boolean` (or asserted result), not the `Predicate` carrier — the carrier is
consumed by the solver's caller-side branch substitution (U6.PREDICATE_ASSERTION).
Flow-narrowing and contextual-typing facts are exposed through
`ProgramAnalysisGraph` (U8), never as `GraphTypeNode` arms.

### Demand-slice budgets and non-admission (parent §6)

The demand-sliced shape is only safe with explicit typed budgets. Every budget
returns a typed `BudgetExceeded` non-admission; a budget-exceeded result is
`ReturnOnly` (never warm-admitted, never backfilled, never published as a partial /
torn cache entry). The U6 budget is **`FlowSliceBudget`** — it bounds return sites,
selected statements, effect + closure summaries. Recursion-storm control adds
prefix-interned projection paths and bounded per-function / per-substitution
candidate retention so a recursion storm cannot grow unbounded candidate sets. The
flow / call / relation budgets the U6 solver consumes — `RelationBudget`,
`KeyspaceBudget`, `CallResolutionBudget`, apparent-type member-demand budget — are
owned by their reducers and each carries its own named three-layer non-admission
guard (parent §6).

### Host API surface (`AuditedResult`)

The host flow-return entry point returns the landed `verter_audit`
`AuditedResult<T, E>` carrier — an enum `Ok { value, audit } | Err { error, audit }`
— so degraded-but-successful results (`Unknown` with a `DegradedReason`) ride the
`Ok` arm with the audit record carrying the reason, while a true error (the
function symbol does not resolve to a value) rides the `Err` arm. This is the single
public seam; consumer rewrites point here. The records store / accumulator /
footprint miner that consume `AuditedResult.audit` live in `verter_session`; the
substrate DTO lives in `verter_audit`. Cold-vs-warm contract: a flow-return-started
structured event fires only on the cold path; warm cache hits emit only the standard
dispatch envelope (`DispatchEnter { key_kind: FlowReturn }` + `DispatchExit {
outcome: Hit }`) and allocate no audit payload without an active accumulator.

---

# U6 — Native Flow Return

## U6.FLOW_RETURN_SUBSTRATE

ID: U6.FLOW_RETURN_SUBSTRATE
Parent U-block: U6
Subplan: docs/arch/native-flow-return.md

Prerequisites: U2.QUERY_VALUE_DOMAIN, U2.RELATION_INFER, U4 (the cache-runtime node substrate).
Blocked until: U2.QUERY_VALUE_DOMAIN done (the `FlowReturn` / `ResolveCall` key shapes + the typed `SemanticQueryValue` value domain are registered there), U2.RELATION_INFER done (narrowing and call-argument assignment relate through `Relate`), and U4 done (the persistent `ArtifactNode` cache-runtime substrate the slice nodes register on). This is the keystone U6 block; every other U6 block consumes the skeleton, the peeker, the slice nodes, and the `FlowReturn` dispatch it lands.

Context: The substrate today "infers" return types via a lightweight scanner in `crates/verter_semantic/src/analysis/type_eval_build.rs` (`infer_return_type`, `infer_expression_type`, `collect_return_types`, `extract_object_literal_as_type`, `append_spread_array_element_types`) that walks OXC `Statement::ReturnStatement` directly off the parse tree (holding an arena borrow), only descends into `BlockStatement` / `IfStatement`, treats every identifier as `TypeOf(path)` without resolving against the parameter / local environment, and unions returns naively. It is not callable from `SemanticGraphStore::execute_cooperative`, never participates in `ReadSetSignature.facts` validation, never emits an audit event, and never warms a reusable cache. The parent (§5) requires a demand-sliced flow resolver: a `FunctionBodySkeleton` (no type lowering), a two-frontier `ReturnPathPeeker`, the `FlowSliceHashNode` / `FlowSliceLoweredBodyNode` / `FlowSliceIR` substrate, the additive `SemanticQueryKey::FlowReturn` query node routed through the one shared dispatch, the demand-aware `satisfied_projection` cache identity, the `FlowSlice` fact, `FlowSliceBudget` non-admission, and the `AuditedResult` host API. This block lands that substrate plus the narrowest real semantic surface (primitive literal-return widening, selective object widening, `as const` preservation, bare-return-as-void) so the substrate is not a stub. It exists now because every other U6 block dispatches through `FlowReturn` and reads `FlowReturnResult`.

Changes (exact files / functions):
- `crates/verter_semantic/src/analysis/flow/mod.rs` (new dir `crates/verter_semantic/src/analysis/flow/`) — the `FunctionBodySkeleton` producer: an arena-free shallow statement / control skeleton with return-site index, lexical binding index, and assignment / kill summaries, produced during shallow analysis and stored in / under `IndexedReady`. No type lowering.
- `crates/verter_semantic/src/analysis/flow/flow_ir.rs` — `FlowSliceIR` (`FlowStmt`, `FlowExpr`, `FlowSlotId`, `FlowPath`, `FlowFrame`, `NarrowingFact`, `AliasCorrelation`, `FlowEffect`, `ReturnAccumulator`, `LoopSummary`), the `ReturnSlicePlan`, and `FunctionKind` (`Sync` / `Async` / `Generator` / `AsyncGenerator` / `Arrow`). Fully arena-detached: all `&str` interned into `Arc<str>`, all spans `verter_span::Span`, all OXC node ids dropped; `Send + Sync + 'static`.
- `crates/verter_semantic/src/analysis/flow/peeker.rs` — `ReturnPathPeeker`: the two-frontier (value-provider + effect) path-contribution algorithm over `FunctionBodySkeleton` producing a `ReturnSlicePlan` from `ReturnProjectionDemand { path, terminal_mode }` (parent §5). The contribution scan, the right-to-left object / `Object.assign` write scan, the definite-write value-suppression rule, and the evaluation-effect (computed-key + spread / `Object.assign` source) effect-frontier sweep.
- `crates/verter_semantic/src/analysis/flow/hashing.rs` — `compute_flow_slice_hash(plan: &ReturnSlicePlan, lens: &dyn FlowCrossDeclLens) -> FlowSliceHashOutcome` hashing only the selected return / control / binding slice; a full-body hash only for a true whole-return request, rejected for member-projection requests. Stack-safe (explicit worklist + visit set + depth budget); alpha-normalises parameter / local identifiers; over-budget paths return a closed fail-closed `BudgetExceeded` variant. Deliberately a separate module from `lower` so the slice hash never becomes a side-product of lowering.
- `crates/verter_semantic/src/analysis/flow/lower.rs` — `lower_slice_plan(&ReturnSlicePlan, &ShallowFileState, lens: &dyn FlowCrossDeclLens) -> FlowSliceIR`, invoked by `FlowSliceLoweredBodyNode::compute` on the cold-miss path only; never during shallow analysis. It must NOT call the slice-hash producer.
- `crates/verter_session/src/flow_return/mod.rs` (new dir) + `solver.rs` — the flow-return solver: frame composition, the literal-return-widening / selective-object-widening / `as const` / bare-return-as-void surface, `satisfied_projection` computation, `FlowSlice` fact recording, and audit emission.
- `crates/verter_session/src/flow_return/env.rs` — `FlowFunctionSlotIdentity` (U2 `ResolvedDeclSlotIdentity` + content-free `FunctionPartId`), `FlowReturnContext`, `ReturnProjectionDemand`, `FlowInputContext`, `FlowPolicy`, and `SubstitutionEnv` canonical normalization (`canonical_hash()` sorts by `(type_parameter_declaration_site, index_within_decl)` — order-independent; intern table on `ProjectTypeStore`, GC'd on workspace generation change).
- `crates/verter_session/src/flow_return/result.rs` + `error.rs` — DEFINE the two U6-extension result types: `FlowReturnResult { return_type, fact_dep_signature: ReadSetSignature, satisfied_projection, degraded: Option<DegradedReason> }` and `ResolvedCallResult` (the call-resolution result value the `ResolveCall` arm carries — consumed by U6.CALL_RESOLVE's `build_resolve_call`); `FlowReturnError` (the typed `Err` arm of `AuditedResult`).
- `crates/verter_session/src/semantic_query.rs` — wire `SemanticQueryKey::FlowReturn { function_slot, normalized_type_args, context, demand, input }` as an additive variant in the U2-finalized slot-identity shape (no cache re-key), adding its enum variant AND its `SemanticQueryKeySpec` row together with its dispatch behavior in this same block — U2 finalizes the identity SHAPE/model but does NOT pre-register the FlowReturn/ResolveCall spec rows or variants — so the standing meta-guard `semantic_query_key_spec_table_equals_enum` stays green incrementally after U6. This block EXTENDS the U2-landed `SemanticQueryValue` enum with BOTH U6 value-domain arms — `SemanticQueryValue::FlowReturn(Arc<FlowReturnResult>)` (mapped from `SemanticQueryKey::FlowReturn` here) and `SemanticQueryValue::ResolvedCall(Arc<ResolvedCallResult>)` (whose `SemanticQueryKey::ResolveCall` dispatch BEHAVIOR lands in U6.CALL_RESOLVE) — an additive enum-arm addition that keeps `semantic_query_key_spec_table_equals_enum` green. This block lands the `SemanticQueryKey::FlowReturn` enum variant + its `SemanticQueryKeySpec` row + its `execute_flow_return` executor + its cache-identity guards ONLY. It does NOT land the `SemanticQueryKey::ResolveCall` enum variant, its `SemanticQueryKeySpec` row, the `execute_resolved_call` / `build_resolve_call` executor, the `ResolveCall` cache-identity guards, or any `ResolveCall` dispatch behavior — ALL of those land in U6.CALL_RESOLVE. The only `ResolveCall`-adjacent thing this block defines is the inert shared `ResolvedCallResult` result struct + the matching `SemanticQueryValue::ResolvedCall` value-domain arm (both inert data carriers that U6.CALL_RESOLVE populates).
- `crates/verter_session/src/project_semantic_dispatch/mod.rs` (the `execute` impl) + a new `project_semantic_dispatch/flow.rs` — `build_flow_return(&self, key: &SemanticQueryKey) -> CacheAdmission<Arc<FlowReturnResult>>` plus the `execute_flow_return` typed dispatch wrapper (landed HERE, over the shared `SemanticGraphStore` admission/inflight substrate), hooked into the dispatch match alongside `build_relate` / `build_instantiate`. The `FlowSliceHashNode` / `FlowSliceLoweredBodyNode` lookups (hash-then-lower; the producer pipeline drives the value-provider demand into the peeker, then lowers only the slice).
- `crates/verter_session/src/cache_runtime/` — register `FlowSliceHashNode` (content-addressed: key carries `parse_stable_hash` + `parse_env_hash` + `parser_version` + `function_part` + the `ReturnProjectionDemand` slice identity) and `FlowSliceLoweredBodyNode` (keyed additionally on the slice hash) as `ArtifactNode` impls; the lowered-slice node's eviction follows the standard `remove_canonical(canonical_id)` cascade.
- `crates/verter_semantic/src/facts/registry.rs` — extend `FactKey` with `FlowSlice { function_slot, projection_path, slice_hash, selected_binding_ids, selected_effect_ids, selected_control_region_ids, closure_summary_ids }`, recorded at the result-publish point on every cold rebuild where the slice hash is `Hash(_)`; under `BudgetExceeded` it is not recorded.
- `crates/verter_audit/src/record.rs` + `structured_event.rs` — additive `RequestKind::FlowReturnInference` tag + `RequestKindPayload::FlowReturnInference` arm + the cold-path `FlowReturnStarted` / `FlowSliceBudgetExceeded` / `FlowCycleSentinelHit` structured events (closed-enum discipline; consumers ignoring the new fields keep working). Extend `RequestKind::matches_filter`, the `KindBit` set, and the batch aggregator. The ts-rs export rides into `packages/types/audit.generated.ts` via the existing generator script (a `cargo run` binary wrapped in a pnpm script).
- `crates/verter_session/src/lib.rs` (`impl VerterHost`) — `get_flow_return_type_with_audit(&self, function: &SymbolHandle, substitution_env: &SubstitutionEnv, demand: ReturnProjectionDemand, mode: ProjectionMode) -> AuditedResult<Arc<FlowReturnResult>, FlowReturnError>`. The single public seam.
- `crates/verter_session/src/types.rs` (`HostConfig`) — `flow_return_loop_budget: u16` (default 32) and `flow_return_cycle_sentinel_depth: u8` (default 4). No `flow_return_enabled` flag (a transitional flag would gate a forbidden dual path or be an unused-field stub; incremental delivery is the audited-degraded mechanism).
- `crates/verter_session/src/meta_resolve/projectors/published_reducer.rs` (`reduce_published_field_types`) + `projectors/mod.rs` (`reduce_field_type_expr`) — the sole admission point: when the per-field reducer encounters `TypeExpr::TypeOf(ValueRef)` whose value root resolves to a **function value** AND the caller wrote `ReturnType<typeof callee>`, the existing builtin-utility dispatch table (where `Pick` / `Omit` / `Required` / `Partial` live) gains a `ReturnType` entry that resolves the callee to a function `SymbolHandle`, dispatches `SemanticQueryKey::FlowReturn`, and returns `FlowReturnResult.return_type`. Path-precise: the call result is materialised ONLY on the consumer-walked path; sibling fields stay shallow.

Deliverables:
- The `FunctionBodySkeleton` (arena-free, no type lowering) under `IndexedReady`, the two-frontier `ReturnPathPeeker`, the `FlowSliceHashNode` / `FlowSliceLoweredBodyNode` cache-runtime nodes (hash-then-lower split), and `FlowSliceIR`.
- The additive `SemanticQueryKey::FlowReturn` query node routed `FlowReturn → ProjectSemanticDispatch::execute → SemanticGraphStore`, resolving to `SemanticQueryValue::FlowReturn(Arc<FlowReturnResult>)`, with the demand-aware `satisfied_projection` cache identity and the `FlowSlice` fact.
- The `AuditedResult` host API, the audit substrate additions, and the `ReturnType<typeof callee>` projector admission point.
- The narrowest real surface: primitive literal-return widening, selective object widening, `as const` preservation, bare-return-as-void — plus the non-materialization acceptance case (`ReturnType<typeof myType>['b']` loads only `b`).

Legacy deletions:
- The legacy return scanner `infer_return_type` / `collect_return_types` / `infer_expression_type` / `extract_object_literal_as_type` / `append_spread_array_element_types` in `crates/verter_semantic/src/analysis/type_eval_build.rs` — deleted in the SAME change (no scanner coexistence, no feature flag, no dual path). Every caller is rewritten in the same change: function-bodied call sites → `host.get_flow_return_type_with_audit`; per-expression call sites (variable initializer, object-property value, spread element, `as` / `satisfies`) → the existing shallow-pass per-expression `TypeExpr` lowering (NOT `lower_slice_plan` — they are not function bodies).
- The opaque-`ReturnType` carrier in `meta_resolve::projectors` that emits `TypeExpr::Unknown { raw: "ReturnType<typeof X>" }` when the inner `TypeOf` resolves to a function — replaced by the `ReturnType` builtin-utility dispatch entry.
- No `flow_return_enabled` feature flag is introduced; no second route through `verter_compiler` or `@verter/component-meta/compat` for `ReturnType<typeof callee>` is created.

SemanticQueryKey/facts touched: `FlowReturn` (value domain `FlowReturn(Arc<FlowReturnResult>)`); dispatches into `TypeOf` (callee value resolution), `ProjectPath` / `ProjectMember` (return member projection), `IndexedAccess` (the `ReturnType<typeof f>['x']` thread). Facts recorded: `FlowSlice` (new — slice identity + selected binding / effect / control-region / closure ids). Facts read: `MemberPresence`, `Member`, `RouteGeneration`, `ExportSurface`, `ModuleAugmentation`, `AmbientGlobal`, `LibIntrinsic`, `TypeEnvOptions`, project-generation facts. Admission: `FlowSliceBudget`; flow-cycle sentinel `ReturnOnly`; budget / overflow / cycle / cancellation / partial-slice route through `CacheAdmission` non-admission.

Exact test rows lifted (capability `ValueInference`, `value_inference.rs`; the substrate / non-materialization subset of capability `FlowNarrowing`, `flow_return_catalog.rs`):
- value_inference.rs::value_inference_const_object_literal_expands_nested_shape
- value_inference.rs::value_inference_function_body_return_union_from_return_statements
- value_inference.rs::value_inference_arrow_expression_body_publishes_return_shape
- value_inference.rs::value_inference_arrow_expression_body_substitutes_parameter_references
- value_inference.rs::value_inference_flow_variables_narrow_return_value_by_branch
- value_inference.rs::value_inference_computed_callback_object_value_resolves_from_callback_body
- value_inference.rs::value_inference_computed_block_callback_value_resolves_local_return_shape

(The `narrow_*` rows lift in U6.NARROWING; the `flow_return_catalog.rs` `xf*` / `ho09` rows lift in U6.CROSS_FILE / U6.CONTEXTUAL_CALLBACK by mechanism. This block owns only the substrate + the pure value-inference rows above whose mechanism is the bare `FlowReturn` slice + return-position widening. `value_inference_flow_variables_narrow_return_value_by_branch` exercises the substrate's branch join; the full narrowing lattice lands in U6.NARROWING.)

Required new guards (parent §5, §6):
- `flow_return_routes_through_project_semantic_dispatch` — the `SemanticQueryKey::FlowReturn` arm dispatches through `ProjectSemanticDispatch::execute → SemanticGraphStore`, and no other call path constructs a `FlowReturnResult`. Fails against any second resolver / per-surface walker.
- `flow_slice_lowered_body_does_not_compute_slice_hash` — greps `FlowSliceLoweredBodyNode::compute` + `lower_slice_plan` for any call to `compute_flow_slice_hash`; fails if one appears (pins the hash-then-lower split).
- `flow_return_key_covers_env_dimensions` — two `FlowReturn` keys differing in exactly one of the five env-hash dimensions (parse / resolve / type / lib / project) hash unequal; `project_config_hash` cannot satisfy this (R21). Lands HERE with the `FlowReturn` variant (the variant must exist on the committed tree for the guard to test it).
- `flow_return_key_covers_input_context_and_projection_demand` — two `FlowReturn` keys differing only in `FlowInputContext` (contextual callback input signature) OR in `ReturnProjectionDemand` (the walked projection path) hash unequal and coexist as distinct candidates. Lands HERE with the `FlowReturn` variant; U6.CONTEXTUAL_CALLBACK's `contextual_callback_input_signature_differentiates_cache_candidates` shares it.
- `flow_solver_never_slices_source_text` — greps the `flow_return` + `analysis::flow` module trees for `parse_type_annotation`, `split_top_level_*`, `find_top_level_char`, `starts_with("Pick<")`, `path.contains("/node_modules/")`, regex against type text, and `format!("…{…}").parse_*`; fails if any appear (Typed-IR-Only).
- `no_flow_slot_in_published_type_surface` — no `TypeExpr` variant named `FlowSlot` exists, AND every published consumer surface (`PropMeta.type`, `ComponentMetaResultDb` values, the U13 `TypeDescriptor` projection, the typeinfo graph wire surface, compat output) rejects any flow-slot identity. Discriminating fixture: a `FlowReturn` whose intermediate solve transits a slot and whose published result reduces it carries no slot.
- `flow_slice_budget_exceeded_admits_nothing` — forces `FlowSliceBudget` `BudgetExceeded`; asserts the result is RETURNED (not a panic / None), the `SemanticGraphStore` slot is NOT warmed, the `FlowSliceLoweredBodyNode` store has NO entry, the result's `ReadSetSignature.facts` has NO `FlowSlice` entry, and the audit payload records the budget-exceeded reason (the three-layer non-admission rule).
- The `Mytype` non-materialization negative guard — `ReturnType<typeof myType>['b']` produces no `ResolveClassSurface`, `TypeOf`, constructor, import, or route fact for `Mytype`.

Critical-rule guards: this block implements the parent's `(CRITICAL)` demand-sliced-flow / one-resolver / Typed-IR-Only / Shallow-By-Default rules; the seven guards above are their R6 guards. Supporting guards landing alongside: `flow_slice_ir_detaches_from_oxc_arena` (`FlowSliceIR: Send + Sync + 'static`; no transitive `&'arena T` / `oxc_allocator::Box<'arena, T>` field), `flow_return_value_lifetime_independent_of_oxc_arena`, `substitution_env_canonical_hash_is_order_independent`, `flow_return_warm_validation_runs_facts` (a warm read revalidates `ReadSetSignature.facts` — extended to verify the `FlowSlice` fact is recorded whenever the solver entered a function body), `every_unknown_fallback_has_audit_event`, `no_eager_body_expansion_in_meta_projection` (`Pick<ReturnType<typeof f>, "selected">` materialises only `selected`; no second dispatch for the unselected sibling), and `no_caching_of_partial_or_budget_exceeded_results`. Any new `(CRITICAL)` rule text added to docs in this change registers its guard here in the same change.

Proof requirement: structural guards (the seven above + the supporting set) plus per-row — the `value_inference_*` rows are TS7-oracle-pinned (`Ts7Oracle`) where the outcome is an exact TS shape (the const-object expansion, the return-union, the arrow-body shapes), and `OracleAndGuard` where a row also pins a non-materialization / branch property (`value_inference_flow_variables_narrow_return_value_by_branch` pairs an oracle with the substrate branch-join assertion). Each row's declared proof is consumed by its generated row-test wrapper (PART 2 §10.3). The discriminating property: the substrate widens a literal return to its primitive (BL01-class) while preserving `as const` (BL09-class), and `ReturnType<typeof myType>['b']` loads only `b`.

Exit acceptance: all 7 `value_inference.rs` rows lift and pass on the normal `lib*.d.ts` corpus; `FlowReturn` routes through the one dispatch; the slice hash precedes the lookup; an over-budget slice is non-admitted at all three layers; the published surface carries no `FlowSlot`; `ReturnType<typeof myType>['b']` produces no `Mytype` fact; cold and warm calls obey the cold-vs-warm audit contract (warm hits emit no `FlowReturnStarted` and allocate no audit payload without an accumulator).

Verification commands:
- `cargo test --package verter_session flow_return` and the value-inference tests.
- `cargo test --package verter_session --test typeinfo_ignored_test_manifest` (coverage gate for this block's rows).
- The block's lifted-row proofs via the generated wrapper (or `-- --ignored` pre-`prepare-verify`).
- `node scripts/gen-corpus-audit-tests.mjs` (idempotent; the audit-record schema gains the `FlowReturnInference` kind / payload + the new structured events).
- The full workspace gate (the complete Rust **AND** JavaScript gate, green only when BOTH pass; non-mutating — clippy-fix / formatting run BEFORE `prepare-verify`, so the gated content is already clean and the receipt certifies an unchanging input): `cargo test --workspace --tests --verbose`; `cargo clippy --workspace -- -D warnings`; `cargo fmt --all --check`; `pnpm test`; `pnpm install --frozen-lockfile`.
- Commit cadence / review gate: PARENT-UNIFORM — the uniform discipline for EVERY block in this subplan (parent PART 2 §11.11 / §11.12), stated once and not restated per block: each block lands as ONE squashed commit (WIP series during the work, no per-commit gate) after the three-reviewer LAND verdict (1 Claude Code + 2 codex).

Docs updated: update the `/type-resolution` skill's flow-return notes (the demand-sliced `ReturnPathPeeker` two-frontier model, `FunctionBodySkeleton` / `FlowSliceIR`, the `SemanticQueryKey::FlowReturn` query + `satisfied_projection` cache identity, the `FlowSlice` fact, `FlowSliceBudget` non-admission); update the `/audit-infrastructure` skill for the new `RequestKind::FlowReturnInference` + the cold-path structured events + the `AuditedResult` host API; reaffirm the Component-Meta Shallow-By-Default `ReturnType<typeof callee>` projector admission in `/component-meta`.

Re-entry notes: idempotent. The slice nodes / `FlowReturn` dispatch are the source of truth — if a caller produces a `FlowReturnResult` outside `build_flow_return`, `flow_return_routes_through_project_semantic_dispatch` fails. The legacy-scanner-removed state is mechanically pinned: a surviving `infer_return_type` / `infer_expression_type` / `collect_return_types` / `extract_object_literal_as_type` / `append_spread_array_element_types` callsite (by string literal) or a surviving opaque-`ReturnType` carrier fails `legacy_return_scanner_removed`. If partial, the manifest shows which `value_inference` rows still carry `#[ignore]`.

---

## U6.NARROWING

ID: U6.NARROWING
Parent U-block: U6
Subplan: docs/arch/native-flow-return.md

Prerequisites: U6.FLOW_RETURN_SUBSTRATE.
Blocked until: U6.FLOW_RETURN_SUBSTRATE done (the narrowing lattice runs on the `FlowFrame` infrastructure landed there; narrowing-of-types relates through `Relate` from U2.RELATION_INFER). This is the largest U6 capability surface.

Context: The `FunctionBodySkeleton` carries branch structure but the substrate block lands only literal-return widening + the bare branch join. The parent (§5) requires the full typed branch-fact lattice (positive / negative / intersection / union) so flow narrowing is checker-grade. The `FlowFrame` ops are `narrow_typeof`, `narrow_in`, `narrow_equality`, `narrow_strict_equality`, `narrow_truthiness`, `narrow_instanceof`, `narrow_array_isarray` (using the lib intrinsic sourced from `lib_env_hash`-dependent declarations, NOT text-matched), and `narrow_optional_chain` (via the `OptionalChain` carrier). Switch flow joins case branches with the discriminant-narrowing fact; discriminated-union narrowing correlates a destructured discriminant with its arm payload. Narrowing-of-types (the actual type each fact produces) routes through `Relate` and the existing reducers — flow does not implement a parallel matcher. Narrowing facts live in `ProgramAnalysisGraph` (U8), never as published `GraphTypeNode` arms. `flow_invalidations.rs` characterizes that narrowing is preserved or invalidated correctly across reassignment, opaque calls, closure capture, destructuring, and try / catch / finally. This block exists now because narrowing is the dominant flow capability (104 `FlowNarrowing` rows) and every later flow capability composes on its frame.

Changes (exact files / functions):
- `crates/verter_semantic/src/analysis/flow/flow_ir.rs` — the typed branch-fact lattice on `FlowFrame` / `NarrowingFact`: positive / negative / intersection / union composition; `AliasCorrelation` for destructured-discriminant correlation.
- `crates/verter_session/src/flow_return/solver.rs` — the `FlowFrame` narrowing ops (`narrow_typeof`, `narrow_in`, `narrow_equality`, `narrow_strict_equality`, `narrow_truthiness`, `narrow_instanceof`, `narrow_array_isarray`, `narrow_optional_chain`), switch per-arm join, discriminated-union arm selection, and the early-return / negated-guard reachability composition. Each narrowing fact produces its narrowed type by dispatching `Relate` / the existing reducers — never a private matcher.
- `crates/verter_type_expr/src/lib.rs` — confirm the `OptionalChain { base, projection, nullish_propagates }` carrier (parent §1.1) is present so `narrow_optional_chain` injects `undefined` into the join when a short-circuit link's base contains `null` / `undefined`; the lib `Array.isArray` / intrinsic narrowing reads `lib_env_hash`-keyed declarations.
- `crates/verter_session/src/flow_return/result.rs` — the narrowing facts are exposed as `ProgramAnalysisGraph` entries (U8 reads them), never `GraphTypeNode` arms; the `FlowReturnResult` carries the narrowed return type only.

Deliverables:
- The typed branch-fact lattice (positive / negative / intersection / union) on `FlowFrame`, with all eight `FlowFrame` narrowing ops, switch per-arm join, discriminated-union arm correlation, and early-return reachability.
- Narrowing-of-types routed through `Relate` / the existing reducers (no parallel matcher); narrowing facts surfaced as `ProgramAnalysisGraph` entries.

Legacy deletions:
- Any narrowing performed off the OXC parse tree at query time (the scanner is already deleted in U6.FLOW_RETURN_SUBSTRATE; this block adds no second narrowing path).
- Any private type-matcher inside narrowing (narrowed types come from `Relate` / the reducers).
- No projection-repair path remains for narrowing; no text-matched intrinsic narrowing (`Array.isArray` is read from `lib_env_hash` declarations).

SemanticQueryKey/facts touched: `FlowReturn` (value domain `FlowReturn(Arc<FlowReturnResult>)`); consumes `Relate` (each narrowing fact's narrowed type), `Conditional` / `NormalizeUnion` (branch joins), `OptionalChain` reduction. Facts read: `Member` / `MemberPresence` (discriminant / property narrowing), `LibIntrinsic` (`Array.isArray` and apparent narrowing), `TypeEnvOptions` (strict / exact-optional narrowing), project-generation facts. Admission: `FlowSliceBudget`; flow-cycle sentinel `ReturnOnly`.

Exact test rows lifted (capability `FlowNarrowing`, `narrow_typeof.rs` / `narrow_equality.rs` / `narrow_truthiness.rs` / `narrow_in_operator.rs` / `narrow_instanceof.rs` / `narrow_discriminated_union.rs` / `flow_invalidations.rs`; capability `TypeParameterFeatures` flow-narrowing-of-generic subset, `substitution_types.rs`):
- narrow_typeof.rs::narrow_typeof_nt01_string_on_binary_union
- narrow_typeof.rs::narrow_typeof_nt02_number_on_triple_union
- narrow_typeof.rs::narrow_typeof_nt03_boolean_on_union
- narrow_typeof.rs::narrow_typeof_nt04_object_on_union_keeps_no_null
- narrow_typeof.rs::narrow_typeof_nt05_function_on_union
- narrow_typeof.rs::narrow_typeof_nt06_undefined_on_union
- narrow_typeof.rs::narrow_typeof_nt07_bigint_on_union
- narrow_typeof.rs::narrow_typeof_nt08_symbol_on_union
- narrow_typeof.rs::narrow_typeof_nt09_string_on_unknown
- narrow_typeof.rs::narrow_typeof_nt10_string_on_unbound_generic
- narrow_typeof.rs::narrow_typeof_nt11_negated_on_binary_union
- narrow_typeof.rs::narrow_typeof_nt12_switch_exhaustive
- narrow_typeof.rs::narrow_typeof_nt13_negated_guard_early_return
- narrow_typeof.rs::narrow_typeof_nt14_compare_literal_var_does_not_narrow
- narrow_typeof.rs::narrow_typeof_nt15_compound_and_property
- narrow_equality.rs::narrow_equality_eq01_string_literal_on_literal_union
- narrow_equality.rs::narrow_equality_eq02_negated_string_literal_on_literal_union
- narrow_equality.rs::narrow_equality_eq03_number_literal_on_triple_union
- narrow_equality.rs::narrow_equality_eq04_boolean_true_on_boolean
- narrow_equality.rs::narrow_equality_eq05_null_on_nullable_string
- narrow_equality.rs::narrow_equality_eq06_undefined_on_optional_string
- narrow_equality.rs::narrow_equality_eq07_double_equals_null_on_nullish_string
- narrow_equality.rs::narrow_equality_eq08_string_literal_on_string_does_not_narrow
- narrow_equality.rs::narrow_equality_eq09_string_literal_on_primitive_union
- narrow_equality.rs::narrow_equality_eq10_two_unions_mutual_equality_does_not_narrow
- narrow_equality.rs::narrow_equality_eq11_impossible_compound_absorbs_never
- narrow_equality.rs::narrow_equality_eq12_property_equality_discriminant
- narrow_equality.rs::narrow_equality_eq13_as_const_literal_rhs
- narrow_equality.rs::narrow_equality_eq14_number_literal_on_number_does_not_narrow
- narrow_equality.rs::narrow_equality_eq15_nan_equality_does_not_narrow
- narrow_truthiness.rs::narrow_truthiness_tr01_string_or_undefined
- narrow_truthiness.rs::narrow_truthiness_tr02_string_or_null
- narrow_truthiness.rs::narrow_truthiness_tr03_string_or_nullish
- narrow_truthiness.rs::narrow_truthiness_tr04_string_no_nullable_does_not_narrow
- narrow_truthiness.rs::narrow_truthiness_tr05_number_literal_union
- narrow_truthiness.rs::narrow_truthiness_tr06_string_literal_union
- narrow_truthiness.rs::narrow_truthiness_tr07_boolean_union
- narrow_truthiness.rs::narrow_truthiness_tr08_negated_string_or_undefined
- narrow_truthiness.rs::narrow_truthiness_tr09_property_truthiness
- narrow_truthiness.rs::narrow_truthiness_tr10_early_return_guard
- narrow_truthiness.rs::narrow_truthiness_tr11_unknown_collapses_to_unknown
- narrow_truthiness.rs::narrow_truthiness_tr12_object_or_null
- narrow_truthiness.rs::narrow_truthiness_tr13_compound_and_chain
- narrow_truthiness.rs::narrow_truthiness_tr14_number_or_undefined_does_not_split_zero
- narrow_truthiness.rs::narrow_truthiness_tr15_optional_chain_truthiness
- narrow_in_operator.rs::narrow_in_operator_io01_binary_union
- narrow_in_operator.rs::narrow_in_operator_io02_shared_key
- narrow_in_operator.rs::narrow_in_operator_io03_else_branch
- narrow_in_operator.rs::narrow_in_operator_io04_intersection
- narrow_in_operator.rs::narrow_in_operator_io05_optional_property
- narrow_in_operator.rs::narrow_in_operator_io06_on_unknown
- narrow_in_operator.rs::narrow_in_operator_io07_on_object
- narrow_in_operator.rs::narrow_in_operator_io08_compound_conjunction
- narrow_in_operator.rs::narrow_in_operator_io09_negated
- narrow_in_operator.rs::narrow_in_operator_io10_three_arm_union
- narrow_in_operator.rs::narrow_in_operator_io11_generic_constrained
- narrow_in_operator.rs::narrow_in_operator_io12_reassignment_renarrowing
- narrow_in_operator.rs::narrow_in_operator_io13_class_vs_object
- narrow_in_operator.rs::narrow_in_operator_io14_template_literal_key
- narrow_in_operator.rs::narrow_in_operator_io15_symbol_key
- narrow_instanceof.rs::narrow_instanceof_in01_binary_union
- narrow_instanceof.rs::narrow_instanceof_in02_class_plus_primitive
- narrow_instanceof.rs::narrow_instanceof_in03_on_unknown
- narrow_instanceof.rs::narrow_instanceof_in04_subclass_union
- narrow_instanceof.rs::narrow_instanceof_in05_already_narrowed
- narrow_instanceof.rs::narrow_instanceof_in06_abstract_class
- narrow_instanceof.rs::narrow_instanceof_in07_else_reachability
- narrow_instanceof.rs::narrow_instanceof_in08_interface_union
- narrow_instanceof.rs::narrow_instanceof_in09_negated_early_return
- narrow_instanceof.rs::narrow_instanceof_in10_intersection
- narrow_instanceof.rs::narrow_instanceof_in11_generic_ctor
- narrow_instanceof.rs::narrow_instanceof_in13_array_special_case
- narrow_instanceof.rs::narrow_instanceof_in14_promise_special_case
- narrow_instanceof.rs::narrow_instanceof_in15_nullable
- narrow_discriminated_union.rs::narrow_discriminated_union_du01_if_equality_discriminant
- narrow_discriminated_union.rs::narrow_discriminated_union_du02_switch_discriminant
- narrow_discriminated_union.rs::narrow_discriminated_union_du03_switch_default_never
- narrow_discriminated_union.rs::narrow_discriminated_union_du04_negated_discriminant
- narrow_discriminated_union.rs::narrow_discriminated_union_du05_multi_property_discriminant
- narrow_discriminated_union.rs::narrow_discriminated_union_du06_nested_discriminant
- narrow_discriminated_union.rs::narrow_discriminated_union_du07_number_literal_discriminant
- narrow_discriminated_union.rs::narrow_discriminated_union_du08_boolean_literal_discriminant
- narrow_discriminated_union.rs::narrow_discriminated_union_du09_destructure_correlation
- narrow_discriminated_union.rs::narrow_discriminated_union_du10_in_guard_plus_discriminant
- narrow_discriminated_union.rs::narrow_discriminated_union_du11_switch_per_arm_join
- narrow_discriminated_union.rs::narrow_discriminated_union_du12_switch_fall_through
- narrow_discriminated_union.rs::narrow_discriminated_union_du14_reassignment_re_narrowing
- narrow_discriminated_union.rs::narrow_discriminated_union_du15_template_literal_discriminant
- flow_invalidations.rs::flow_invalidations_fi01_reassignment_invalidates_string_narrowing
- flow_invalidations.rs::flow_invalidations_fi02_narrowing_preserved_across_opaque_call
- flow_invalidations.rs::flow_invalidations_fi04_destructured_discriminant_preserves_correlation
- flow_invalidations.rs::flow_invalidations_fi05_destructured_discriminant_loses_on_reassignment
- flow_invalidations.rs::flow_invalidations_fi09_exhaustive_never_tail_does_not_widen_return
- substitution_types.rs::substitution_types_sb01_bare_narrowing_of_generic
- substitution_types.rs::substitution_types_sb02_narrowing_in_constrained_generic
- substitution_types.rs::substitution_types_sb03_substitution_survives_method_calls
- substitution_types.rs::substitution_types_sb04_narrowed_substitution_to_return_position
- substitution_types.rs::substitution_types_sb05_compound_typeof_and_instanceof
- substitution_types.rs::substitution_types_sb06_narrowing_widens_after_reassignment
- substitution_types.rs::substitution_types_sb07_constraint_flow_apparent_type
- substitution_types.rs::substitution_types_sb08_generic_in_conditional_no_distribute_on_unknown
- substitution_types.rs::substitution_types_sb11_generic_narrowed_via_in_operator
- substitution_types.rs::substitution_types_sb12_truthiness_on_t_or_undefined
- substitution_types.rs::substitution_types_sb13_substitution_carried_across_destructure

(104 rows: 88 `narrow_*` + 5 `flow_invalidations` (`fi01`, `fi02`, `fi04`, `fi05`, `fi09`) + 11 `substitution_types` flow-narrowing-of-generic rows. The 11 `substitution_types_sb01`–`sb08`/`sb11`–`sb13` rows are flow narrowing applied to a substitution — bare / constrained-generic narrowing, compound `typeof`+`instanceof`, un-narrowing on reassignment, `in`-operator, truthiness, destructure correlation, and constraint-flow apparent-type access — so their dominant mechanism is this narrowing frame, not the predicate / call / loop surfaces (`sb09`/`sb10` are predicate/assertion rows owned by U6.PREDICATE_ASSERTION; `sb14`/`sb15` are pure-substitution rows owned by U2.CLASS_SURFACES — §10.4.1). The `flow_invalidations_fi08_asserts_narrows_dotted_member_path` row's dominant mechanism is the assertion-effect-on-a-dotted-path engine, so its owning `block_id` is U6.PREDICATE_ASSERTION (it consumes this narrowing frame, which is that block's prerequisite) and it is NOT listed here. The `flow_invalidations_fi03` closure-capture row and the `fi06`/`fi07` try / finally control-return rows are owned by U6.LOOP_CLOSURE (their dominant mechanism is the closure barrier / control-flow surface) and are NOT listed here — §10.4.1 assigns each row to exactly one `block_id`. Note this 104 is the U6.NARROWING block's row-set (88 `narrow_*` + 5 `flow_invalidations` FlowNarrowing-substrate rows + 11 `substitution_types` `TypeParameterFeatures`-substrate rows); it is a per-`block_id` count, distinct from — though now numerically equal to — the 104-row `FlowNarrowing` capability/substrate tally (which spans U6.NARROWING, U6.PREDICATE_ASSERTION's fi08, U6.LOOP_CLOSURE's fi03/fi06/fi07, and the cross-file `xf*`/`ho09` catalog rows). A capability tally counts a substrate across every owning block; a block count is the row-set of one `block_id`.)

Required new guards (parent §5):
- `narrowing_facts_compose_in_predicate_keyed_frames` — `(typeof x === "string") && (typeof x === "number")` returns `never` (positive / negative intersection composition).
- `narrowing_facts_are_program_analysis_not_graph_type_nodes` — narrowing facts surface as `ProgramAnalysisGraph` entries, never `GraphTypeNode` arms (shares the U8 `type_node_contains_only_type_values` gate).
- `array_isarray_narrowing_reads_lib_intrinsic_not_text` — `Array.isArray` narrowing is sourced from `lib_env_hash`-keyed declarations, not a text match.

Critical-rule guards: this block implements the parent's `(CRITICAL)` demand-sliced-flow (narrowing) and one-resolver rules and the Typed-IR-Only rule (no text-matched intrinsic narrowing); the three guards above plus the inherited `flow_solver_never_slices_source_text` are their R6 guards. No NEW `(CRITICAL)` engine rule beyond the parent's flow rule.

Proof requirement: per-row — every `narrow_*` and `substitution_types_sb01`–`sb08`/`sb11`–`sb13` row is TS7-oracle-pinned (`Ts7Oracle`) for the exact narrowed return shape; the `does_not_narrow` rows (e.g. `narrow_typeof_nt14`, `narrow_equality_eq08`/`eq10`/`eq14`/`eq15`, `narrow_truthiness_tr04`) are oracle-pinned negative cases; the `flow_invalidations_*` rows owned here (`fi01`, `fi02`, `fi04`, `fi05`, `fi09`) are `OracleAndGuard` pairing the oracle with the preservation / invalidation assertion (`fi01` pairs with the reassignment-invalidation assertion, `fi02` with the opaque-call-preservation assertion). Consumed by each row's generated wrapper.

Exit acceptance: all 104 rows above lift and pass on the normal `lib*.d.ts` corpus; the typed lattice composes positive / negative / intersection / union (the `never` composition guard green); switch per-arm join and discriminated-union arm correlation are exact; flow narrowing of a generic substitution (`substitution_types_sb01`–`sb08`/`sb11`–`sb13`) matches the oracle; narrowing facts are `ProgramAnalysisGraph` entries; `Array.isArray` narrowing reads the lib intrinsic; reassignment / opaque-call / destructuring preservation matches the oracle.

Verification commands:
- `cargo test --package verter_session` narrowing tests (`narrow_*`, `flow_invalidations`).
- `cargo test --package verter_session --test typeinfo_ignored_test_manifest` (coverage gate).
- The block's lifted-row proofs via the generated wrapper.
- Full workspace gate (as U6.FLOW_RETURN_SUBSTRATE).

Docs updated: update the `/type-resolution` skill's flow-narrowing notes (the typed branch-fact lattice on `FlowFrame`, the eight `FlowFrame` narrowing ops, switch per-arm join + discriminated-union arm correlation, narrowing-of-types routed through `Relate`, flow-narrowing-of-generic substitutions); reaffirm in `/type-cache-architecture` that narrowing facts live in `ProgramAnalysisGraph`, never as published `GraphTypeNode` arms.

Re-entry notes: idempotent. The narrowing lattice composes on the substrate `FlowFrame` — do not add a second narrowing path. If partial, the manifest shows which `narrow_*` / `flow_invalidations` / `substitution_types` (`sb01`–`sb08`/`sb11`–`sb13`) rows remain `#[ignore]`.

---

## U6.PREDICATE_ASSERTION

ID: U6.PREDICATE_ASSERTION
Parent U-block: U6
Subplan: docs/arch/native-flow-return.md

Prerequisites: U6.FLOW_RETURN_SUBSTRATE, U6.NARROWING, U6.CALL_RESOLVE.
Blocked until: all three prerequisites done (predicate / assertion effects apply onto the narrowing frame; the narrowed type each effect produces routes through `Relate`; predicate / assertion application consumes the resolved callee signature from `ResolveCall` — the `Predicate` / `Assertion` / `AssertsCondition` carrier is read off the callee signature that U6.CALL_RESOLVE resolves, so this block requires U6.CALL_RESOLVE landed, not merely the substrate).

Context: A function whose signature carries `x is T` (a type predicate) or `asserts x is T` / `asserts cond` (an assertion) has an EFFECT on caller flow that the narrowing engine must apply. The parent (§1.1) carries these as `SignatureEffect::{Predicate, Assertion, AssertsCondition}` metadata on function signatures — NOT as standalone published `TypeExpr` / `GraphTypeNode` type nodes. When a consumer asks for the *return type* of a predicate function it gets that function's `boolean` (or the asserted result), not the carrier; the carrier is consumed by the solver's caller-side branch substitution. A call whose callee signature carries `Predicate { param_idx, narrowed_to }` applies a positive fact on the true branch and a negative fact on the false branch; generic predicates instantiate `narrowed_to` at the call site against `normalized_type_args`. A signature-only (declared, body-less) predicate still applies its signature fact. This block exists now because predicate / assertion application is the second narrowing source (after operator narrowing) and several `FlowNarrowing` / `TypeParameterFeatures` rows depend on it.

Changes (exact files / functions):
- `crates/verter_type_expr/src/lib.rs` — the publishable carrier variants `Predicate { param_idx: u16, narrowed_to }`, `Assertion { param_idx: u16, narrowed_to }`, `AssertsCondition { effect: AssertedConditionEffect }` (parent §1.1), each with a discriminating round-trip fixture.
- `crates/verter_type_expr_oxc/src/lib.rs` — extend `lower_ts_type` for `TSTypePredicate` (the `x is T` / `asserts x is T` / `asserts cond` syntax) so the carrier is lowered ONCE during shallow analysis (front-end lowering only; no query-time re-parse).
- `crates/verter_session/src/flow_return/solver.rs` — the caller-side branch substitution: on a `FlowExpr::Call` whose resolved callee signature carries a `Predicate` / `Assertion` / `AssertsCondition` effect, apply the positive fact on the true branch and the negative fact on the false branch (predicate), apply the asserted fact past the call (assertion), and instantiate `narrowed_to` at the call site against `normalized_type_args` for generic predicates. The narrowed type routes through `Relate` / the reducers.
- `crates/verter_audit/src/structured_event.rs` — additive `PredicateEffectApplied { predicate_symbol, branch, narrowed_to_hash }` and `AssertionEffectApplied { assertion_symbol, branch, narrowed_to_hash }` structured events.

Deliverables:
- The `Predicate` / `Assertion` / `AssertsCondition` carriers, their `TSTypePredicate` lowering, and the solver's caller-side branch substitution (positive / negative branch facts; generic instantiation of `narrowed_to`).
- Predicate / assertion treated as signature / effect metadata (the return type of a predicate function is its `boolean` / asserted result), never a standalone published type node.

Legacy deletions:
- Any predicate / assertion handling that published the carrier as a return type or a `GraphTypeNode` arm (the carrier is effect metadata).
- No projection-repair path for predicates / assertions.

SemanticQueryKey/facts touched: `FlowReturn` (value domain `FlowReturn(Arc<FlowReturnResult>)`); consumes `ResolveCall` / `ResolveOverloadSet` (callee signature resolution), `Relate` (the narrowed type). Facts read: `Member` / `MemberPresence`, `TypeEnvOptions`, project-generation facts. Admission: `FlowSliceBudget`; flow-cycle sentinel `ReturnOnly`.

Exact test rows lifted (capability `FlowNarrowing` assertion-effect row — `flow_invalidations.rs`; capability `TypeParameterFeatures` flow subset — predicate / assertion on generic — `substitution_types.rs`):
- flow_invalidations.rs::flow_invalidations_fi08_asserts_narrows_dotted_member_path
- substitution_types.rs::substitution_types_sb09_asserts_x_is_string_on_generic
- substitution_types.rs::substitution_types_sb10_x_is_t_predicate_on_generic

(3 rows. The `flow_invalidations_fi08_asserts_narrows_dotted_member_path` row (substrate `FlowNarrowing`) exercises the assertion effect on a dotted member path; its dominant mechanism is this block's assertion-effect-on-dotted-path engine, so its owning `block_id` is U6.PREDICATE_ASSERTION (§10.4.1) even though its substrate is `FlowNarrowing`. It consumes the already-live `U6.NARROWING` frame — that block is this block's declared prerequisite, so there is no cycle: U6.PREDICATE_ASSERTION depends on U6.NARROWING, not the reverse. The non-generic predicate / assertion catalog rows in `flow_return_catalog.rs` / `flow_return_edge_catalog.rs` (`pa*`) are un-ignored as the catalog macros convert; the coverage table assigns each manifest `pa*` row to this block by mechanism. This block owns the two generic-predicate `substitution_types_sb09/sb10` manifest rows plus the `fi08` assertion-effect row directly.)

Required new guards (parent §5):
- `predicate_signature_without_body_audits_signature_only_outcome` — a declared (signature-only) predicate applied at the call site applies the signature fact and emits `PredicateEffectApplied` even though no body was lowered; the result carries `degraded_reason: None`.
- `predicate_assertion_effect_is_signature_metadata_not_published_type_node` — the return type of a `x is T` function is `boolean` (or the asserted result), never the `Predicate` carrier; the carrier does not appear on any published consumer surface.

Critical-rule guards: this block implements the parent's `(CRITICAL)` demand-sliced-flow (predicate / assertion as effect metadata) rule and the Macro-Type-Traversal "predicate / assertion as signature effect" ruling; the two guards above are their R6 guards. No NEW `(CRITICAL)` engine rule beyond the parent's.

Proof requirement: per-row — the `substitution_types_sb09/sb10` rows are TS7-oracle-pinned (`Ts7Oracle`) for the narrowed generic return; `sb09` (`asserts x is string` on generic) pairs the oracle with `predicate_signature_without_body_audits_signature_only_outcome` where the predicate is signature-only (`OracleAndGuard`); the `flow_invalidations_fi08_asserts_narrows_dotted_member_path` row is `OracleAndGuard` pairing the oracle for the narrowed dotted-path return with the assertion-effect-on-dotted-path preservation assertion. Consumed by each row's generated wrapper.

Exit acceptance: all three rows (`substitution_types_sb09/sb10` + `flow_invalidations_fi08_asserts_narrows_dotted_member_path`) lift and pass; a `x is T` call applies a positive fact on the true branch and a negative fact on the false branch; an `asserts x is T` call applies the asserted fact past the call (including the dotted-member-path case `fi08` pins, narrowing onto the live U6.NARROWING frame); a signature-only predicate applies its signature fact and emits `PredicateEffectApplied`; the return type of a predicate function is its `boolean` / asserted result (the carrier is never published).

Verification commands:
- `cargo test --package verter_session` predicate / assertion tests + the `substitution_types` flow rows.
- `cargo test --package verter_session --test typeinfo_ignored_test_manifest` (coverage gate).
- The block's lifted-row proofs via the generated wrapper.
- Full workspace gate (as U6.FLOW_RETURN_SUBSTRATE).

Docs updated: update the `/type-resolution` skill's predicate/assertion notes (the `Predicate` / `Assertion` / `AssertsCondition` carriers as `SignatureEffect` metadata, `TSTypePredicate` lowering, the solver's caller-side branch substitution + generic-predicate instantiation); update the `/audit-infrastructure` skill for the `PredicateEffectApplied` / `AssertionEffectApplied` structured events.

Re-entry notes: idempotent. Predicate / assertion effects are signature metadata applied caller-side — do not publish the carrier as a return type. If partial, the manifest shows which `substitution_types` predicate rows (`sb09`/`sb10`) and the `flow_invalidations_fi08_asserts_narrows_dotted_member_path` assertion-effect row remain `#[ignore]`.

---

## U6.CALL_RESOLVE

ID: U6.CALL_RESOLVE
Parent U-block: U6
Subplan: docs/arch/native-flow-return.md

Prerequisites: U6.FLOW_RETURN_SUBSTRATE, U2.RELATION_INFER, U2.CLASS_SURFACES.
Blocked until: U6.FLOW_RETURN_SUBSTRATE done (call results feed the flow solver), U2.RELATION_INFER done (argument-to-parameter assignment + generic inference bindings come from `Relate`), and U2.CLASS_SURFACES done — but the U2.CLASS_SURFACES prerequisite is **SHAPE-only**: this block consumes the ordered overload sets, abstract constructors, hybrid call/construct signatures, and class / prototype surfaces that U2.CLASS_SURFACES produces, and explicitly NOT any decorator-call or JSX-call behavior (none of that is part of the prerequisite — it is this block's own backfill, below). This block owns the genuine call-expression rows: the flow / generic-inference / `this`-receiver call rows, the overload-SELECTION call rows (first-applicable candidate selection at the call site over U2.CLASS_SURFACES's ordered overload SHAPE), and the two `const_type_param_*` call rows. `ResolveCall` is a first-class key (parent §2.4) — U2.QUERY_VALUE_DOMAIN finalizes only the slot-identity SHAPE/model it reuses; this block lands its enum variant + `SemanticQueryKeySpec` row + `execute_resolved_call` dispatch + cache-identity guards + BEHAVIOR together. Because `ResolveCall` lands HERE (not in U2), this block also BACKFILLS the decorator-call-routing validation for U2.CLASS_SURFACES's decorator rows and the `jsx` / `jsxs` / `createElement` call-dispatch validation for U2.JSX_FOUNDATIONS's factory rows — a U6 backfill that does NOT re-own those U2-resident rows (they stay listed in their U2 blocks).

Context: Call resolution is reusable semantic work, not merely a flow helper (parent §2.4): without its own cache identity, contextual typing, flow return, overload selection, generic inference, and typeinfo expression evaluation would duplicate work or hide meaning-affecting inputs inside a body solver. The `ResolveCall` key normalizes closed arguments to TYPE identities and keeps an EXPRESSION identity only for context-sensitive arguments. For each `FlowExpr::Call { callee, args }` the solver resolves `callee` to a `SymbolHandle` via the existing typed-IR resolver (no text parsing), determines declared + body signatures, assigns `args` to parameters via `Relate` collecting inferred type-argument bindings, picks the best candidate per TS overload order (declared overloads before generic inference; implementation signature internal-only; `ReturnType<typeof overloaded>` / `ConstructorParameters` use the LAST visible overload — that overload-SHAPE selection lands in U2.CLASS_SURFACES, consumed here), and recursively dispatches `SemanticQueryKey::FlowReturn` for the chosen signature with the canonical-normalized substitution env. The `ReturnType<typeof callee>` projector admission point landed in U6.FLOW_RETURN_SUBSTRATE is wired through here for the call case. `this`-receiver method calls and extracted-prototype method calls resolve the receiver and return the declared / inferred return. This block exists now because call resolution drives the factory-prop pattern (`Props = ReturnType<typeof createProps>`) and every composable `useX()`.

Changes (exact files / functions):
- `crates/verter_session/src/semantic_query.rs` — land the `SemanticQueryKey::ResolveCall { callee, call_kind, receiver_this, args, explicit_type_args, contextual_result, policy, context }` enum variant **+** its `SemanticQueryKeySpec` row together (U2 finalized only the slot-identity SHAPE/model the variant reuses; the variant + spec row land HERE so `semantic_query_key_spec_table_equals_enum` stays green incrementally) and its `SemanticQueryValue::ResolvedCall(Arc<ResolvedCallResult>)` mapping; `CallArgKey::{ Eager, ContextSensitive }` and `ContextSensitiveExprKey` (the context-sensitive arg identity carrying `flow_narrowing`, `substitution`, `binder`, `contextual_typing`).
- `crates/verter_session/src/project_semantic_dispatch/mod.rs` + a new `project_semantic_dispatch/call.rs` — `build_resolve_call(&self, key) -> CacheAdmission<Arc<ResolvedCallResult>>` plus the `execute_resolved_call` typed dispatch wrapper (landed HERE, over the shared `SemanticGraphStore` admission/inflight substrate): callee resolution via the typed-IR resolver, argument-to-parameter assignment via `Relate` (collecting inference bindings), overload candidate selection via `ResolveOverloadSet` (first applicable for calls), generic-inference iteration, and the recursive `FlowReturn` dispatch for the chosen signature under the normalized substitution env.
- `crates/verter_session/src/flow_return/solver.rs` — the `FlowExpr::Call` handling: dispatch `ResolveCall`, thread the result into the flow frame, and apply the `ReturnType<typeof callee>` admission for the call case (the projector entry from U6.FLOW_RETURN_SUBSTRATE resolves the callee to a function `SymbolHandle` and dispatches `FlowReturn`).
- `crates/verter_session/src/semantic_query_memo/budgeted_caches.rs` — `CallResolutionBudget` bounding overload candidates, inference bindings, and contextual passes; `BudgetExceeded` non-admission (`ReturnOnly`).
- `crates/verter_audit/src/structured_event.rs` — additive `LoopFixedPointConverged { iterations }` / `LoopFixedPointAbandoned { iterations, reason }` (consumed by the generic-inference iteration here and the loop solver in U6.LOOP_CLOSURE).

Deliverables:
- `ResolveCall` behavior (first-class key) with callee resolution, `Relate`-driven argument assignment + generic inference, overload selection (first applicable for calls, over U2.CLASS_SURFACES's ordered overload SHAPE), and the recursive `FlowReturn` dispatch under the normalized substitution env.
- The `ReturnType<typeof callee>` admission for the call case; `this`-receiver and extracted-prototype method-call return resolution.
- The genuine call-expression rows this block OWNS: the overload-SELECTION call rows (`call_resolution_optional_overload_picks_*`, `call_resolution_specific_literal_argument_*`, `function_advanced_overload_call_picks_matching_signature_return`) and the two `const_type_param_*` call rows (TS7 `<const T>` inferred from a call-site array argument) — moved here from U2.CLASS_SURFACES because their dominant mechanism is the `ResolveCall` dispatch, not a U2 SHAPE reducer.
- The U6 BACKFILL of the U2-resident call-dispatch validation: decorator-call routing for U2.CLASS_SURFACES's decorator rows and `jsx` / `jsxs` / `createElement` call dispatch for U2.JSX_FOUNDATIONS's factory rows (those rows stay owned by their U2 blocks; this block supplies the `ResolveCall` machinery they exercise once it lands).
- `CallResolutionBudget` with three-layer `BudgetExceeded` non-admission.

Legacy deletions:
- Any call resolution living inside the body solver without its own `ResolveCall` cache identity (folded into the first-class key).
- Any text-based callee resolution (the callee resolves through the typed-IR resolver).
- Any second `ReturnType<typeof callee>` route (the projector admission point is the sole one); no projection-repair path for calls.

SemanticQueryKey/facts touched: `ResolveCall` (value domain `ResolvedCall(Arc<ResolvedCallResult>)`), `FlowReturn` (recursive dispatch for the chosen signature); consumes `ResolveOverloadSet` (candidate order), `Relate` (argument assignment + inference bindings), `ResolveClassSurface` / `ApparentType` (method / receiver resolution). Facts read: `Member` / `MemberPresence`, `LibIntrinsic`, `TypeEnvOptions`, `RouteGeneration`, project-generation facts. Admission: `CallResolutionBudget`; `ReturnOnly` on `BudgetExceeded`.

Exact test rows lifted (capability `CallResolution` flow / generic-inference / `this`-receiver subset, `call_resolution.rs`; capability `CallResolution` higher-order / void-callback / generic subset, `function_advanced.rs`):
- call_resolution.rs::call_resolution_generic_infers_from_positional_argument_through_callback_signature
- call_resolution.rs::call_resolution_generic_infers_from_callback_return_type
- call_resolution.rs::call_resolution_generic_infers_object_literal_including_excess_properties
- call_resolution.rs::call_resolution_optional_overload_picks_first_arity_matching_signature
- call_resolution.rs::call_resolution_optional_overload_picks_two_arg_signature_when_required
- call_resolution.rs::call_resolution_specific_literal_argument_picks_matching_overload_first
- call_resolution.rs::call_resolution_specific_literal_argument_skips_non_matching_first_overload
- call_resolution.rs::call_resolution_this_receiver_method_call_returns_declared_return
- call_resolution.rs::call_resolution_extracted_prototype_method_call_returns_declared_return
- const_type_param.rs::const_type_param_route_call_preserves_readonly_tuple_with_literal_paths
- const_type_param.rs::const_type_param_string_call_preserves_readonly_literal_string_tuple
- function_advanced.rs::function_advanced_this_parameter_type_returns_this_annotation
- function_advanced.rs::function_advanced_omit_this_parameter_returns_function_without_this
- function_advanced.rs::function_advanced_higher_order_composition_returns_concrete_function
- function_advanced.rs::function_advanced_void_callback_return_preserves_void
- function_advanced.rs::function_advanced_overload_call_picks_matching_signature_return
- function_advanced.rs::function_advanced_overload_generic_first_binds_to_literal_argument
- function_advanced.rs::function_advanced_overload_generic_first_widens_t_to_string_for_string_argument
- function_advanced.rs::function_advanced_constrained_generic_infers_literal_under_as_const

(19 rows. This block owns the genuine call-expression rows: the flow / generic-inference rows whose mechanism is the `ResolveCall` dispatch + recursive `FlowReturn`, the overload-SELECTION call rows (`call_resolution_optional_overload_picks_*`, `call_resolution_specific_literal_argument_*`, `function_advanced_overload_call_picks_matching_signature_return` — first-applicable candidate selection AT THE CALL SITE, consuming U2.CLASS_SURFACES's ordered overload SHAPE), and the two `const_type_param_*` call rows (the TS7 `<const T>` modifier applied while inferring `T` from a call-site array argument). The `call_resolution_contextual_callback_return_picks_first_overload` row lifts in U6.CONTEXTUAL_CALLBACK (its mechanism is the nested-callback contextual-typing frame). The overload-SHAPE / abstract-constructor / hybrid-signature / prototype-extraction `call_resolution.rs` + `function_advanced.rs` rows — `call_resolution_abstract_constructor_instance_type_projects_class_shape`, `function_advanced_return_type_of_overloaded_function_uses_last_overload`, `function_advanced_constructor_parameters_*`, `function_advanced_instance_type_*`, the four `function_advanced_call_construct_hybrid_*`, `function_advanced_class_method_prototype_extraction_*` — lift in U2.CLASS_SURFACES because their mechanism is `ResolveOverloadSet` / `ResolveClassSurface` SHAPE, NOT call dispatch. The `U2.CLASS_SURFACES` prerequisite of this block is SHAPE-only (see Blocked until). The coverage table assigns each row to exactly one `block_id`; no row is double-counted.)

Required new guards (parent §2.4, §6):
- `call_resolution_budget_exceeded_admits_nothing` — `CallResolutionBudget` `BudgetExceeded` admits no resolved-call result, no overload-candidate / inference-binding intermediate, no fact signature / backfill, and no degraded exact-cache entry (the three-layer rule).
- `flow_call_resolves_callee_via_typed_ir_not_text` — callee resolution routes through the typed-IR resolver, never a text parse (shares `flow_solver_never_slices_source_text`).
- `resolve_call_key_covers_args_this_contextual_type_overload_policy_and_context` — the `ResolveCall` key covers `args` (Eager + ContextSensitive), `receiver_this`, `contextual_result`, overload `policy`, and `context`; two `ResolveCall` keys differing in exactly one hash unequal. Lands HERE with the `ResolveCall` variant (its variant must exist on the committed tree for the guard to test it).
- `resolve_call_same_expr_different_flow_or_substitution_does_not_warm_hit` — the same call expression under a different flow-narrowing / substitution context does NOT warm-hit a prior candidate. Lands HERE with the `ResolveCall` variant.

Critical-rule guards: this block implements the parent's `(CRITICAL)` one-resolver (call resolution as a shared key) and Macro-Type-Traversal (one shared resolver, thin normalisation) rules; the two guards above are their R6 guards. The `ResolveCall` cache-identity guards (`resolve_call_key_covers_args_this_contextual_type_overload_policy_and_context`, `resolve_call_same_expr_different_flow_or_substitution_does_not_warm_hit`) are landed HERE in U6 together with the `ResolveCall` variant + its `SemanticQueryKeySpec` row + dispatch + `build_resolve_call` executor — the variant must exist on the committed tree for a cache-identity guard to test it. No NEW `(CRITICAL)` engine rule beyond the parent's.

Proof requirement: per-row — the generic-inference rows (`call_resolution_generic_infers_*`, `function_advanced_overload_generic_first_*`, `function_advanced_constrained_generic_infers_literal_under_as_const`) are TS7-oracle-pinned (`Ts7Oracle`) for the inferred return; the overload-SELECTION call rows (`call_resolution_optional_overload_picks_*`, `call_resolution_specific_literal_argument_*`, `function_advanced_overload_call_picks_matching_signature_return`) are oracle-pinned for the selected-signature return/parameters; the two `const_type_param_*` call rows are oracle-pinned for the preserved readonly literal tuple/string under `<const T>`; the `this`-receiver / prototype rows (`call_resolution_this_receiver_*`, `call_resolution_extracted_prototype_*`, `function_advanced_this_parameter_*`, `function_advanced_omit_this_parameter_*`) are oracle-pinned for the declared return; `function_advanced_void_callback_return_preserves_void` pairs the oracle with the void-preservation assertion (`OracleAndGuard`). Consumed by each row's generated wrapper.

Exit acceptance: all 19 rows above lift and pass; callee resolution is typed-IR (no text); argument assignment + generic inference come from `Relate`; the chosen signature's return is solved by a recursive `FlowReturn`; the overload-SELECTION call rows pick the first applicable candidate at the call site over U2.CLASS_SURFACES's ordered overload SHAPE; the two `const_type_param_*` call rows preserve the readonly literal tuple/string inferred under `<const T>`; `this`-receiver / prototype method calls return the declared return; the `void` callback return is preserved; `CallResolutionBudget` `BudgetExceeded` is non-admitted at all three layers; no overload-SHAPE / abstract-constructor row is lifted here (those are U2.CLASS_SURFACES rows whose SHAPE this block consumes); the decorator-call-routing + `jsx`/`jsxs`/`createElement` call-dispatch BACKFILL validates the U2-resident decorator / JSX-factory rows without re-owning them.

Verification commands:
- `cargo test --package verter_session` call-resolution tests + the flow `function_advanced` rows.
- `cargo test --package verter_session --test typeinfo_ignored_test_manifest` (coverage gate).
- The block's lifted-row proofs via the generated wrapper.
- Full workspace gate (as U6.FLOW_RETURN_SUBSTRATE).

Docs updated: update the `/type-resolution` skill's call-resolution notes (the first-class `ResolveCall` key — `CallArgKey::{Eager, ContextSensitive}`, callee resolution via the typed-IR resolver, `Relate`-driven argument assignment + generic inference, overload selection, the recursive `FlowReturn` dispatch); update the `/audit-infrastructure` skill for `LoopFixedPointConverged` / `LoopFixedPointAbandoned`.

Re-entry notes: idempotent. `ResolveCall` is the single call-resolution surface — do not add a parallel call resolver inside the body solver. If partial, the manifest shows which flow / overload-selection / `const_type_param` call `call_resolution` / `function_advanced` rows remain `#[ignore]`. The decorator-call / JSX-factory call-dispatch BACKFILL validates the U2-resident rows but does not flip their manifest status — those rows are owned and flipped by U2.CLASS_SURFACES / U2.JSX_FOUNDATIONS.

---

## U6.CONTEXTUAL_CALLBACK

ID: U6.CONTEXTUAL_CALLBACK
Parent U-block: U6
Subplan: docs/arch/native-flow-return.md

Prerequisites: U6.CALL_RESOLVE, U6.NARROWING.
Blocked until: both prerequisites done (callback contextual typing flows from the callee signature resolved by `ResolveCall`; the iterative generic-inference loop converges on substitution-env equality and consumes narrowed callback-parameter facts).

Context: Callee → callback contextual typing flows BEFORE the callback's return is solved. A nested `FlowFrame` per callback invocation pre-binds the callback's parameters to the contextual types derived from the callee signature, then solves the callback body in that frame, returning a `FlowReturnResult` the outer call's signature resolution consumes for generic inference. The generic-inference iteration loop (when contextual typing of a callback parameter depends on a type variable also constrained by another argument) is bounded by `flow_policy.loop_budget`, converging on substitution-environment equality. The `FlowInputContext` key field (the contextual callback input signature plus the relation / call demand mode — parent §2.5) makes two re-entries differing only in contextual input signature distinct cache identities. Object-literal-argument contextual typing (`acc` in `arr.reduce((acc, item) => …, {} as Record<string, V>)`) must NOT pollute the caller frame with the callback return. The `ContextualTypeAt` query (a U2 key) reads the contextual target / expected type at a point and returns a `ProgramAnalysisGraph` value. This block exists now because callback contextual typing is iterative generic inference over U6.CALL_RESOLVE and drives the `ContextualTyping` capability (13 rows), the higher-order flow rows, and the contextual-callback overload-return row (`call_resolution_contextual_callback_return_picks_first_overload`, moved here from U2.CLASS_SURFACES — the callback's contextual return drives the outer overload selection).

Changes (exact files / functions):
- `crates/verter_session/src/flow_return/solver.rs` — the nested-`FlowFrame`-per-callback machinery: derive the callback's contextual parameter types from the callee signature, pre-bind them, solve the callback body in the nested frame, and return its `FlowReturnResult` to the outer call's generic inference. The iterative generic-inference loop bounded by `flow_policy.loop_budget`, converging on `SubstitutionEnv::canonical_hash()` equality; emit `CallbackContextualTypingDescend` / `LoopFixedPointConverged` / `LoopFixedPointAbandoned`.
- `crates/verter_session/src/flow_return/env.rs` — `FlowInputContext` carrying the contextual callback input signature + relation / call demand mode (the `input` key field of `FlowReturn`).
- `crates/verter_session/src/project_semantic_dispatch/mod.rs` — the `ContextualTypeAt { point, context: ProgramAnalysisContext }` reducer behavior resolving to `SemanticQueryValue::ProgramAnalysis(ProgramAnalysisValue)` (the key shape is registered in `U2.QUERY_VALUE_DOMAIN`); contextual expected-type propagation into object-literal / array-literal / function-expression arguments.
- `crates/verter_audit/src/structured_event.rs` — additive `CallbackContextualTypingDescend { callback_symbol, contextual_param_count }`.

Deliverables:
- Nested-`FlowFrame`-per-callback contextual typing (parameters pre-bound from the callee signature before the callback body is solved), the iterative generic-inference loop bounded by `flow_policy.loop_budget` converging on substitution-env equality, and the `FlowInputContext` differentiator.
- `ContextualTypeAt` behavior resolving to `ProgramAnalysisGraph`; contextual propagation into object / array / function-expression arguments without caller-frame pollution.

Legacy deletions:
- Any callback-return type computed before contextual parameter typing (the contextual types flow first).
- Any contextual-typing fact published as a `GraphTypeNode` arm (it is a `ProgramAnalysisGraph` value).
- No projection-repair path for contextual typing.

SemanticQueryKey/facts touched: `FlowReturn` (nested callback solve), `ResolveCall` (the outer call), `ContextualTypeAt` (value domain `ProgramAnalysis(ProgramAnalysisValue)`); consumes `Relate` (contextual assignment + inference). Facts read: `Member` / `MemberPresence`, `TypeEnvOptions`, project-generation facts. Admission: `FlowSliceBudget` + `CallResolutionBudget`; `FlowSliceBudget` on the iteration loop; flow-cycle sentinel `ReturnOnly`.

Exact test rows lifted (capability `ContextualTyping`, `contextual_typing.rs`; the higher-order declared-callback subset of capability `FlowNarrowing`, `flow_return_catalog.rs`; the contextual-callback overload-return row of capability `CallResolution`, `call_resolution.rs`):
- call_resolution.rs::call_resolution_contextual_callback_return_picks_first_overload
- contextual_typing.rs::contextual_typing_ct01_callback_parameter_from_contextual_signature
- contextual_typing.rs::contextual_typing_ct02_callback_return_type_published
- contextual_typing.rs::contextual_typing_ct03_object_literal_assignment_from_typed_target
- contextual_typing.rs::contextual_typing_ct04_object_literal_in_function_call
- contextual_typing.rs::contextual_typing_ct07_as_cast_erases_context
- contextual_typing.rs::contextual_typing_ct08_jsx_like_attribute_contextual_typing
- contextual_typing.rs::contextual_typing_ct09_discriminated_union_contextual_narrowing
- contextual_typing.rs::contextual_typing_ct10_array_literal_contextually_typed_as_tuple
- contextual_typing.rs::contextual_typing_ct11_as_const_readonly_modifier
- contextual_typing.rs::contextual_typing_ct12_function_expression_argument_from_contextual_signature
- contextual_typing.rs::contextual_typing_ct13_object_literal_as_cast_narrows_shape
- contextual_typing.rs::contextual_typing_ct14_satisfies_widens_to_target
- contextual_typing.rs::contextual_typing_ct15_contextual_type_via_type_parameter_constraint
- flow_return_catalog.rs::flow_return_ho09_keeps_unknown_declared_callback_result_opaque

(15 rows: the 13 `contextual_typing.rs` manifest rows, the one `flow_return_catalog.rs` higher-order manifest row `flow_return_ho09_keeps_unknown_declared_callback_result_opaque` (the audited-degraded declared-callback case whose mechanism is the nested-callback contextual-typing frame), plus the one `call_resolution.rs` row `call_resolution_contextual_callback_return_picks_first_overload` — moved here from U2.CLASS_SURFACES because its dominant mechanism is the nested-callback contextual-typing frame (the callback's contextual return drives the outer overload selection), not a U2 SHAPE reducer. The remaining higher-order callback catalog rows in `flow_return_catalog.rs` / `flow_return_edge_catalog.rs` (`ho*`) are un-ignored as the catalog macros convert and are NOT among the 363 `IgnoredTestRow`s; §10.4.1 assigns each manifest row to exactly one `block_id` by mechanism.)

Required new guards (parent §5):
- `callback_contextual_typing_does_not_pollute_caller_frame` — `arr.reduce((acc, item) => ({...acc, [item.k]: item.v}), {} as Record<string, V>)` fixes `acc` to `Record<string, V>` via contextual typing; the callback return does not pollute the explicit `Record<string, V>`.
- `contextual_callback_input_signature_differentiates_cache_candidates` — two `FlowReturn` calls differing only in `FlowInputContext` (contextual callback input signature) coexist as distinct candidates (shares `flow_return_key_covers_input_context_and_projection_demand`).

Critical-rule guards: this block implements the parent's `(CRITICAL)` demand-sliced-flow (contextual typing) and typed-value-domain (contextual facts in `ProgramAnalysisGraph`) rules; the two guards above are their R6 guards. No NEW `(CRITICAL)` engine rule beyond the parent's.

Proof requirement: per-row — the `contextual_typing_ct*` rows are TS7-oracle-pinned (`Ts7Oracle`) for the contextually-typed shape; `ct03`/`ct04`/`ct12` (object / function-expression contextual typing) pair the oracle with `callback_contextual_typing_does_not_pollute_caller_frame` where they exercise a callback (`OracleAndGuard`); `ct07` (`as` cast erases context) and `ct11`/`ct14` (`as const` / `satisfies` interaction) are oracle-pinned; `call_resolution_contextual_callback_return_picks_first_overload` is TS7-oracle-pinned (`Ts7Oracle`) for the overload picked from the callback's contextual return; `flow_return_ho09_keeps_unknown_declared_callback_result_opaque` is `OracleAndGuard` pairing the oracle with the audited-degraded assertion that the unknown declared-callback result stays opaque as `unknown` with the cross-file dependency footprint attached. Consumed by each row's generated wrapper.

Exit acceptance: all 15 rows above lift and pass; callback parameters are pre-bound from the callee signature before the callback body is solved; the generic-inference loop converges on substitution-env equality within `flow_policy.loop_budget`; `acc`-style accumulators are fixed by contextual typing without caller-frame pollution; the contextual-callback overload-return row picks the first overload via the callback's contextual return; the `ho09` declared-callback result stays opaque as `unknown`; contextual facts resolve to `ProgramAnalysisGraph`; the `FlowInputContext` differentiator keeps distinct contextual-input candidates separate.

Verification commands:
- `cargo test --package verter_session` contextual-typing tests + the higher-order flow catalog rows.
- `cargo test --package verter_session --test typeinfo_ignored_test_manifest` (coverage gate).
- The block's lifted-row proofs via the generated wrapper.
- Full workspace gate (as U6.FLOW_RETURN_SUBSTRATE).

Docs updated: update the `/type-resolution` skill's contextual-typing notes (nested-`FlowFrame`-per-callback pre-binding, the bounded iterative generic-inference loop converging on `SubstitutionEnv::canonical_hash()` equality, the `FlowInputContext` differentiator, `ContextualTypeAt` returning a `ProgramAnalysisGraph` value); update the `/audit-infrastructure` skill for `CallbackContextualTypingDescend`.

Re-entry notes: idempotent. Contextual types flow before the callback return is solved — do not invert the order. The iteration loop is bounded by `flow_policy.loop_budget` and converges on substitution-env equality; same-context recursion records the flow cycle sentinel, never self-awaits. If partial, the manifest shows which `contextual_typing` rows remain `#[ignore]`.

---

## U6.VALUE_INFERENCE

ID: U6.VALUE_INFERENCE
Parent U-block: U6
Subplan: docs/arch/native-flow-return.md

Prerequisites: U6.FLOW_RETURN_SUBSTRATE, U6.CALL_RESOLVE.
Blocked until: both prerequisites done (object / spread return shapes use the call-driven cases of U6.CALL_RESOLVE inside object literals; mapped / conditional return annotations instantiate against body-derived types via the existing reducers).

Context: This block lands the remaining object-shape handling for non-call return expressions plus conditional / mapped return reduction and `satisfies` on the return path. `FlowExpr::Spread`, `…ObjectMember`, `…IndexedAccess`, `…TemplateLiteralComputed`, `…AsConst`, `…Satisfies` light up beyond the substrate's literal / selective-object set. `satisfies` is TS7 oracle-pinned (parent §4.3): `E satisfies T` checks assignability of `E` to `T`, contextually types `E` with `T`, then keeps the inferred source type of `E`, NOT `T` — fresh object literals get excess-property checks unless the target admits the key, source keys are retained (`Record<string, V>` validates values but `keyof typeof value` stays the literal key union), and literal widening is not blanket (pinned against `tsgo 7.0.0-dev.20260526.1`, not guessed). Spread / `Object.assign` reduce left-to-right with explicit later writes winning (the two-frontier value-provider rule from §5). Mapped / conditional return annotations instantiate against body-derived types via the existing `MappedType` / `Conditional` paths. This block exists now because object / spread / `satisfies` return shapes are the second-most-common factory pattern after plain object returns, and they compose on the call-driven cases.

Changes (exact files / functions):
- `crates/verter_session/src/flow_return/solver.rs` — the non-call return-expression handling: `FlowExpr::Spread` (left-to-right reduce, later writes win — the value-provider frontier), `…ObjectMember`, `…IndexedAccess`, `…TemplateLiteralComputed`, `…AsConst` (`as const` preservation), `…Satisfies` (widen the body literal against the target while keeping the literal's value shape on the return path). Mapped / conditional return annotations dispatch the existing `MappedType` / `Conditional` keys against body-derived types.
- `crates/verter_semantic/src/analysis/flow/peeker.rs` — the spread / `Object.assign` value-provider right-to-left scan and the definite-write value-suppression that this block's object returns rely on (the peeker rule from §5 is landed in the substrate; this block exercises its object / spread cases).
- `crates/verter_session/src/project_semantic_dispatch/mod.rs` — the `satisfies` widening on the return path via the shared `widen_for_position(ty, WideningSite)` helper and `Relate` (target-contextual validation + widening without surface rewrite), with exact TS7 oracle pins.

Deliverables:
- Object / spread / `as const` / `satisfies` / indexed / mapped / conditional return-shape handling on the return path (non-call expressions), with spread / `Object.assign` left-to-right reduction (later writes win) and `satisfies` keeping the source member set where TS does.
- Mapped / conditional return annotations instantiated against body-derived types via the existing reducers.

Legacy deletions:
- Any spread / object return that inlines into a union or arbitrary last-wins instead of the left-to-right value-provider reduction.
- Any `satisfies` handling that replaces the source type with the target (it keeps the source) or that is a projection repair.
- No projection-repair path for object / spread / mapped / conditional returns.

SemanticQueryKey/facts touched: `FlowReturn` (value domain `FlowReturn(Arc<FlowReturnResult>)`); consumes `MappedType` / `Conditional` (mapped / conditional return annotations), `IndexedAccess` / `KeyOf` (indexed return), `TemplateLiteralReduce` (computed template keys), `Relate` (`satisfies` validation). Facts read: `Member` / `MemberPresence`, `LibIntrinsic`, `TypeEnvOptions`, project-generation facts. Admission: `FlowSliceBudget` + `KeyspaceBudget` (mapped / template return); flow-cycle sentinel `ReturnOnly`.

Exact test rows lifted (capability `ModernTsFeatures` flow subset — `satisfies` on the return path under flow context — `modern_ts_features.rs`):
- modern_ts_features.rs::satisfies_widens_inner_value_to_primitive_without_as_const

(1 row. The `satisfies_array_literal_widens_to_primitive_array` and `variance_annotation_*` / `import_attribute_*` rows lift in U2 — `satisfies` lands with U2.RELATION_INFER (relation + contextual validation) and U2.CLASS_SURFACES (the widening helper) per the U2-reducers subplan's row-level-split note; this block exercises the `satisfies` widening on the flow return path for the single `satisfies_widens_inner_value_to_primitive_without_as_const` row whose mechanism is flow-return-driven value widening. The object / spread / `as const` / mapped / conditional return catalog rows in `flow_return_catalog.rs` / `flow_return_edge_catalog.rs` (`ob*`) and the parity `tp*` object rows are un-ignored as the catalog macros convert; the coverage table assigns each `ob*` / `tp*` manifest row to this block (or U6.CROSS_FILE for cross-file object returns, or U6.LOOP_CLOSURE for `tp08` try / finally) by mechanism. The `value_inference.rs` const-object / nested-shape rows are owned by U6.FLOW_RETURN_SUBSTRATE.)

Required new guards (parent §4.3, §5):
- `satisfies_does_not_widen_returned_value` — `E satisfies T` on the return path keeps the inferred source type of `E`, not `T`; the source member set is preserved where TS preserves it (oracle-pinned against `tsgo 7.0.0-dev.20260526.1`).
- `flow_return_spread_reduces_left_to_right_later_write_wins` — `return {...a, ...b, k: 1}` reduces left-to-right with explicit `k: 1` winning (the value-provider frontier rule).

Critical-rule guards: this block implements the parent's `(CRITICAL)` demand-sliced-flow (two-frontier value provider) and Typed-IR-Only rules; the two guards above are their R6 guards. The `satisfies` oracle mechanism is the parent's `(CRITICAL)` oracle-pinned-`satisfies` ruling (§4.3) — pinned against the pinned `tsgo` version, not prose. No NEW `(CRITICAL)` engine rule beyond the parent's.

Proof requirement: per-row — `satisfies_widens_inner_value_to_primitive_without_as_const` is TS7-oracle-pinned (`Ts7Oracle`) and paired with `satisfies_does_not_widen_returned_value` (`OracleAndGuard`). The object / spread catalog rows lifted by this block are oracle-pinned; spread-override-order rows pair the oracle with `flow_return_spread_reduces_left_to_right_later_write_wins`. Consumed by each row's generated wrapper.

Exit acceptance: the `satisfies_widens_inner_value_to_primitive_without_as_const` row lifts and passes; `satisfies` keeps the source type (not the target); spread / `Object.assign` reduce left-to-right with later writes winning; `as const` is preserved on the return path; mapped / conditional return annotations instantiate against body-derived types via the existing reducers; no projection-repair path exists for object / spread returns.

Verification commands:
- `cargo test --package verter_session` value-inference / object-return / `satisfies` tests.
- `cargo test --package verter_session --test typeinfo_ignored_test_manifest` (coverage gate).
- The block's lifted-row proofs via the generated wrapper.
- Full workspace gate (as U6.FLOW_RETURN_SUBSTRATE).

Docs updated: update the `/type-resolution` skill's return-path value-inference notes (object / spread / `as const` / `satisfies` / indexed / mapped / conditional return-shape handling, spread / `Object.assign` left-to-right reduction with later writes winning, `satisfies` keeping the source type — oracle-pinned against the pinned `tsgo` version).

Re-entry notes: idempotent. `satisfies` keeps the source type and is oracle-pinned — do not collapse it into a blanket widen-to-target. If partial, the manifest shows which `modern_ts_features` / object-return rows remain `#[ignore]`.

---

## U6.ASYNC_GENERATOR

ID: U6.ASYNC_GENERATOR
Parent U-block: U6
Subplan: docs/arch/native-flow-return.md

Prerequisites: U6.FLOW_RETURN_SUBSTRATE.
Blocked until: U6.FLOW_RETURN_SUBSTRATE done (the async / generator carriers compose at the variant level on the substrate's `FunctionKind`; `Awaited` reduces through the existing builtin-utility dispatch).

Context: This block wires the `GeneratorReturn` / `AsyncGeneratorReturn` / `Awaited(T)` carriers (parent §1.1) into the producer (detect `function*` / `async function` / `async function*` via `FunctionKind`) and the solver. The solver: `async function` → wrap the body return in `Promise<>` (`Awaited<T>` reduced into `Promise<T>`); `function*` → synthesise `Generator<Yield, Return, Next>`; `async function*` → `AsyncGenerator<…>`; `await` → `Awaited(T)` reduced by the existing builtin-utility dispatch. Join contract: `Yield` = union of yields, `Return` = union of `return expr`, `Next` = `.next()` parameter type (contextual, else `unknown`). The `Generator` / `Promise` / `AsyncGenerator` intrinsics are sourced from `lib_env_hash`-keyed declarations, so a non-default lib gives a different `lib_env_hash` on the key. This block exists now to compose async / generator semantics at the carrier level introduced in the substrate.

Changes (exact files / functions):
- `crates/verter_type_expr/src/lib.rs` — confirm the `GeneratorReturn { yield_t, return_t, next_t }`, `AsyncGeneratorReturn { yield_t, return_t, next_t }`, and `Awaited(Arc<TypeExpr>)` carriers (parent §1.1), each with a discriminating round-trip fixture.
- `crates/verter_semantic/src/analysis/flow/flow_ir.rs` — `FunctionKind` detection (`Sync` / `Async` / `Generator` / `AsyncGenerator` / `Arrow`) on the skeleton / slice plan.
- `crates/verter_session/src/flow_return/solver.rs` — the async / generator return synthesis: `async` → `Promise<Awaited<T>>`; `function*` → `Generator<Yield, Return, Next>` (yield union, return union, `.next()` parameter contextual / `unknown`); `async function*` → `AsyncGenerator<…>`; `await` → `Awaited(T)` reduced by the builtin-utility dispatch; `yield*` delegation joins the delegated generator's yields.

Deliverables:
- `GeneratorReturn` / `AsyncGeneratorReturn` / `Awaited(T)` carriers wired into the producer (`FunctionKind`) and the solver, with the yield / return / next join contract and `yield*` delegation.
- `Awaited` recursive unwrap via the existing builtin-utility dispatch; `Generator` / `Promise` / `AsyncGenerator` intrinsics keyed by `lib_env_hash`.

Legacy deletions:
- Any async return that does not wrap in `Promise<>` / any generator return that does not synthesise the protocol shape.
- No projection-repair path for async / generator returns; no hand-rolled `Awaited` unwrap (it routes through the builtin-utility dispatch).

SemanticQueryKey/facts touched: `FlowReturn` (value domain `FlowReturn(Arc<FlowReturnResult>)`); consumes the `Awaited` / `Promise` / `Generator` builtin-utility dispatch and `NormalizeUnion` (yield / return joins). Facts read: `LibIntrinsic` (`Promise` / `Generator` / `AsyncGenerator` / `Awaited` intrinsics keyed by `lib_env_hash`), `Member` / `MemberPresence`, `TypeEnvOptions`, project-generation facts. Admission: `FlowSliceBudget`; flow-cycle sentinel `ReturnOnly`.

Exact test rows lifted (capability `ModernTsFeatures` flow / await subset, `modern_ts_features.rs`):
- modern_ts_features.rs::await_using_simulated_return_type_resolves_to_primitive

(1 row. The async / generator catalog rows in `flow_return_catalog.rs` / `flow_return_edge_catalog.rs` (`bl07`/`bl08`/`bl11`/`bl17`/`bl21`/`bl22`) and the parity `tp04` for-await async-generator row are un-ignored as the catalog macros convert; the coverage table assigns each async / generator manifest row to this block by mechanism. This block owns the `await_using_simulated_return_type_resolves_to_primitive` manifest row directly, whose mechanism is the `await` / `Awaited` carrier under the U6 flow / await path per the U2-reducers row-level-split note.)

Required new guards (parent §5):
- `lib_env_hash_drives_generator_return_resolution` — `Generator` from a non-default lib gives a different `lib_env_hash` on the `FlowReturn` key (the intrinsic is read from lib declarations, not synthesised by text).
- `async_return_wraps_in_promise_via_builtin_utility` — `async () => 1` returns `Promise<number>` via the `Awaited` / `Promise` builtin-utility dispatch, not a hand-rolled wrap.

Critical-rule guards: this block implements the parent's `(CRITICAL)` demand-sliced-flow (async / generator carriers) rule and the Macro-Type-Traversal (`Awaited` as a first-class carrier reduced by the shared dispatch) ruling; the two guards above are their R6 guards. No NEW `(CRITICAL)` engine rule beyond the parent's.

Proof requirement: per-row — `await_using_simulated_return_type_resolves_to_primitive` is TS7-oracle-pinned (`Ts7Oracle`) for the unwrapped primitive; the async / generator catalog rows are oracle-pinned and pair with `lib_env_hash_drives_generator_return_resolution` where they exercise the `Generator` / `Promise` intrinsic (`OracleAndGuard`). Consumed by each row's generated wrapper.

Exit acceptance: the `await_using_simulated_return_type_resolves_to_primitive` row lifts and passes; `async` returns wrap in `Promise<>`; `function*` / `async function*` synthesise the protocol shape with the yield / return / next join; `await` reduces through the builtin-utility dispatch; the `Generator` / `Promise` intrinsics are keyed by `lib_env_hash`.

Verification commands:
- `cargo test --package verter_session` async / generator / await tests.
- `cargo test --package verter_session --test typeinfo_ignored_test_manifest` (coverage gate).
- The block's lifted-row proofs via the generated wrapper.
- Full workspace gate (as U6.FLOW_RETURN_SUBSTRATE).

Docs updated: update the `/type-resolution` skill's async/generator notes (the `GeneratorReturn` / `AsyncGeneratorReturn` / `Awaited(T)` carriers, `FunctionKind` detection, the async/generator return synthesis + yield/return/next join contract + `yield*` delegation, `Generator` / `Promise` / `AsyncGenerator` intrinsics keyed by `lib_env_hash`).

Re-entry notes: idempotent. `Awaited` reduces through the builtin-utility dispatch and the `Generator` / `Promise` intrinsics are lib-keyed — do not synthesise them by text. If partial, the manifest shows which async / generator / await rows remain `#[ignore]`.

---

## U6.CROSS_FILE

ID: U6.CROSS_FILE
Parent U-block: U6
Subplan: docs/arch/native-flow-return.md

Prerequisites: U6.VALUE_INFERENCE, U6.CALL_RESOLVE.
Blocked until: both prerequisites done (cross-file factories return object literals — U6.VALUE_INFERENCE — and call into other files — U6.CALL_RESOLVE). Cross-file flow capabilities are jointly owned with U3 (the route / import-demand fact substrate); the coverage table assigns each cross-file manifest row to exactly one of U3 / U6 by mechanism.

Context: Every cross-file value-symbol lookup (callee in another file, predicate from a barrel, namespace-imported value call) routes through `crates/verter_session/src/resolver_core` — no bespoke walker (the Canonical Dependency Cache Rule + the one-resolver rule). The value / type symbol-space split keeps `import { x }` value space separate from `import type { x }`. Cross-file cycle sentinels: re-entry on the FULL normalized `FlowReturnContext + ReturnProjectionDemand + FlowInputContext` within one resolution stack emits a flow cycle sentinel and returns the conservative fixed-point via non-admission. The `xf*` flow-return catalog rows characterize imported-value-function expansion, barrel-route recording before the selected leaf, namespace-import value calls, the value / type namespace split, and cross-file recursive-return termination. This block exists now because cross-file factories are the highest-value real-world pattern (a `useX()` composable or a `createProps` factory imported across files) and they need the substrate, value-inference, and call-resolution blocks beneath them.

Changes (exact files / functions):
- `crates/verter_session/src/flow_return/solver.rs` — route every cross-file value-symbol lookup through `resolver_core` (callee in another file, predicate from a barrel, namespace-imported value call); no bespoke cross-file walker. The value / type symbol-space split. The cross-file flow cycle sentinel: re-entry on the full normalized `FlowReturnContext + ReturnProjectionDemand + FlowInputContext` within one resolution stack returns the conservative fixed-point via non-admission and emits the flow cycle sentinel event.
- `crates/verter_session/src/resolver_core` (the host-backed resolver stack) — the cross-file flow-return lookups consume the existing resolver-core route / import facts; no new resolver path. Barrel-hop and namespace-import value resolution reuse the shared resolver caches.
- `crates/verter_audit/src/structured_event.rs` — additive `FlowCycleSentinelHit { cycle_id, function_symbol }` (the cross-file / in-file recursive-return sentinel; shared with the in-file recursion case).

Deliverables:
- Cross-file flow-return lookups routed entirely through `resolver_core` (no bespoke walker), the value / type symbol-space split, and the cross-file flow cycle sentinel keyed on the full normalized re-entry identity.

Legacy deletions:
- Any bespoke cross-file walker inside the flow solver (cross-file lookups route through `resolver_core`).
- Any treatment of `import type { x }` as a value import (the symbol-space split keeps them separate).
- No projection-repair path for cross-file flow; no second cross-file resolver.

SemanticQueryKey/facts touched: `FlowReturn` (value domain `FlowReturn(Arc<FlowReturnResult>)`), `ResolveCall` (cross-file callee); routes through `resolver_core` route / import resolution. Facts read: `RouteGeneration`, `ExportSurface`, `ModuleAugmentation`, `AmbientGlobal`, `Member` / `MemberPresence`, `LibIntrinsic`, `TypeEnvOptions`, project-generation facts. Admission: `FlowSliceBudget`; flow-cycle sentinel `ReturnOnly`.

Exact test rows lifted (capability `FlowNarrowing` cross-file subset, `flow_return_catalog.rs`):
- flow_return_catalog.rs::flow_return_xf02_expands_imported_value_function_return
- flow_return_catalog.rs::flow_return_xf04_expands_barrel_imported_value_function_return
- flow_return_catalog.rs::flow_return_xf04_records_barrel_route_before_selected_leaf
- flow_return_catalog.rs::flow_return_xf05_resolves_namespace_import_value_call
- flow_return_catalog.rs::flow_return_xf06_keeps_value_type_namespace_separate
- flow_return_catalog.rs::flow_return_xf09_terminates_cross_file_recursive_returns

(6 rows — the six `flow_return_catalog.rs` `xf*` cross-file value-return rows. The three `cross_file.rs` rows (`cross_file_projected_item_resolves_local_extension`, `cross_file_projected_extra_resolves_number_terminal`, `cross_file_label_parameter_resolves_local_item`) carry the `CrossFileResolution` substrate (U3/U6 split in the Capability Map) and are owned by `docs/arch/native-typeinfo-parity-cache-export-session.md::U3.CACHE_FACT_MODEL` — their dominant mechanism is the route-fact / cross-file indexed-access projection path, not the flow-return slice — so they are NOT listed here (§10.4.1). This block's `xf*` rows still route every cross-file value-symbol lookup through `resolver_core`. The path-precise `fp*` rows of `flow_return_path_contracts.rs` and the module-augmentation `mp*` parity rows are un-ignored as the path / parity macros convert and are NOT among the 363 `IgnoredTestRow`s. The `flow_return_catalog.rs` `ho09` row is owned by U6.CONTEXTUAL_CALLBACK.)

Required new guards (parent §5):
- `cross_file_flow_routes_via_resolver_core` — greps the `flow_return` module for any non-`resolver_core` cross-file lookup; fails if one appears.
- `cross_file_recursion_terminates_with_audit_event` — cross-file mutual recursion emits the flow cycle sentinel, returns the conservative fixed-point via non-admission, and the sentinel is never warm-admitted.
- `value_type_namespace_split_does_not_leak` — `import { x }` value space stays separate from `import type { x }` type space across the flow lookup.

Critical-rule guards: this block implements the parent's `(CRITICAL)` one-resolver / Canonical-Dependency-Cache / Macro-Type-Traversal rules (cross-file value lookup routes through the shared resolver, flow cycle sentinel is `ReturnOnly`); the three guards above plus the inherited `flow_cycle_sentinel_is_never_admitted_as_cache_entry` and `flow_cycle_sentinel_does_not_hide_real_base_return_contributor` are their R6 guards. No NEW `(CRITICAL)` engine rule beyond the parent's.

Proof requirement: per-row — the six `flow_return_xf*` rows are TS7-oracle-pinned (`Ts7Oracle`) for the cross-file projected shape; `flow_return_xf04_records_barrel_route_before_selected_leaf` pairs the oracle with a route-fact-recording assertion, `flow_return_xf06_keeps_value_type_namespace_separate` with `value_type_namespace_split_does_not_leak`, and `flow_return_xf09_terminates_cross_file_recursive_returns` with `cross_file_recursion_terminates_with_audit_event` (`OracleAndGuard`). Consumed by each row's generated wrapper.

Exit acceptance: all 6 rows above lift and pass on the normal `lib*.d.ts` corpus; every cross-file value lookup routes through `resolver_core` (no bespoke walker); the value / type namespace split does not leak; cross-file mutual recursion emits the flow cycle sentinel, returns the fixed-point, and never warm-admits the sentinel; the three `cross_file.rs` rows are owned by U3.CACHE_FACT_MODEL (not re-claimed here).

Verification commands:
- `cargo test --package verter_session` cross-file flow tests (the `flow_return_catalog` `xf*` rows).
- `cargo test --package verter_session --test typeinfo_ignored_test_manifest` (coverage gate + the U3/U6 split assignment confirming the three `cross_file.rs` rows belong to U3).
- The block's lifted-row proofs via the generated wrapper.
- Full workspace gate (as U6.FLOW_RETURN_SUBSTRATE).

Docs updated: update the `/type-resolution` skill's cross-file flow notes (cross-file value-symbol lookup routes through `resolver_core`, the value / type symbol-space split, the cross-file flow cycle sentinel keyed on the full normalized `FlowReturnContext + ReturnProjectionDemand + FlowInputContext` re-entry identity); update the `/audit-infrastructure` skill for `FlowCycleSentinelHit`; reaffirm the Canonical Dependency Cache Rule in `/type-resolution`.

Re-entry notes: cross-file flow is mutually recursive — re-entry is bounded by the flow cycle sentinel keyed on the full normalized identity; same-path recursion records the sentinel, never self-awaits. Cross-file lookups route through `resolver_core` only. If partial, the manifest shows which `flow_return_catalog` `xf*` rows remain `#[ignore]` (the `cross_file.rs` rows are tracked under U3.CACHE_FACT_MODEL).

---

## U6.LOOP_CLOSURE

ID: U6.LOOP_CLOSURE
Parent U-block: U6
Subplan: docs/arch/native-flow-return.md

Prerequisites: U6.CALL_RESOLVE, U6.PREDICATE_ASSERTION.
Blocked until: both prerequisites done (the closure-capture barrier needs call resolution + predicate detection to know which calls do NOT invalidate captured slots; the loop fixed-point joins call-driven mutations).

Context: This block lands the loop-flow operator (`for`, `for…in`, `for…of`, `for…await…of`, `while`, `do…while`, labeled break / continue), the loop fixed-point engine, the `FlowSliceBudget` / `flow_policy.loop_budget` enforcement, and the closure-capture barrier. Loop fixed-point convergence is slot-equality: after iteration N+1, if every loop-mutated `FlowSlotId`'s resolved type equals its iteration-N value, converged; on `loop_budget` overflow, return the iteration-N value via non-admission with the budget-exceeded reason; the slot is NOT warmed. The loop join joins `break` paths and the post-loop continuation with the body's fixed-point. The closure-capture barrier: a call whose callee value escapes the current function scope (passed as argument, returned, or stored in a captured variable) widens all captured mutable slots back to their declared type at the next statement boundary — exception: known-signature predicate / assertion calls with no escape-by-mutation effect (the signature is local and analysable). Escape detection uses the lowered IR (`FunctionExpr` / arrow argument capturing a mutable slot; `ReturnStatement` returning a capturing callee; assignment of a capturing callee to a slot outliving the frame, via `captured_value_refs`). Try / finally control-return override and labeled-break reachability are part of the control-flow surface. This block exists now because loops + the closure barrier need call-resolution + predicate detection beneath them and land the loop-divergence cases.

Changes (exact files / functions):
- `crates/verter_semantic/src/analysis/flow/peeker.rs` — the loop / control-flow surface in the skeleton slice: `for` / `for…in` / `for…of` / `for…await…of` / `while` / `do…while` loop regions, labeled break / continue reachability, try / catch / finally control-return override (the `finally` return overrides try / catch returns; a `finally` without return preserves them), and the `captured_value_refs` escape summary.
- `crates/verter_session/src/flow_return/solver.rs` — the loop fixed-point engine (slot-equality convergence; `flow_policy.loop_budget` enforcement; iteration-N value returned via non-admission on overflow; the loop join with `break` paths and the post-loop continuation), and the closure-capture barrier (widen captured mutable slots to their declared type at the next statement boundary on escape; the predicate / assertion exception). Divergent loops (`while (true) {}` with no reachable `break`) model as `void`.
- `crates/verter_session/src/semantic_query_memo/budgeted_caches.rs` — `FlowSliceBudget` enforcement on the loop iteration (return sites, selected statements, effect + closure summaries); `BudgetExceeded` non-admission (`ReturnOnly`).
- `crates/verter_audit/src/structured_event.rs` — the `FlowSliceBudgetExceeded` event fires on loop-budget overflow (paired with `LoopFixedPointAbandoned` from U6.CALL_RESOLVE); `LoopFixedPointConverged` on slot-equality convergence.

Deliverables:
- The loop-flow operator (all loop forms + labeled break / continue + try / finally control-return override), the loop fixed-point engine (slot-equality convergence, `flow_policy.loop_budget`, non-admission on overflow), the loop join, and divergent-loop-as-`void`.
- The closure-capture barrier (widen captured mutable slots on escape; the predicate / assertion exception) with escape detection off the lowered IR.

Legacy deletions:
- Any loop handling that ignores `ForStatement` / `WhileStatement` / `SwitchStatement` (the scanner is already deleted; this block adds the full loop CFG).
- Any caching of a loop result that exceeded `loop_budget` (it routes through non-admission).
- No projection-repair path for loops / closures.

SemanticQueryKey/facts touched: `FlowReturn` (value domain `FlowReturn(Arc<FlowReturnResult>)`); consumes `ResolveCall` (loop-body calls, escape detection), `Relate` (slot-type joins). Facts read: `Member` / `MemberPresence`, `TypeEnvOptions`, project-generation facts. Admission: `FlowSliceBudget`; iteration-N value on overflow via `ReturnOnly`; flow-cycle sentinel `ReturnOnly`.

Exact test rows lifted (capability `FlowNarrowing` loop / closure / control-flow subset, `flow_invalidations.rs`):
- flow_invalidations.rs::flow_invalidations_fi03_closure_capture_preserves_narrowing_at_return
- flow_invalidations.rs::flow_invalidations_fi06_finally_return_overrides_try_catch_returns
- flow_invalidations.rs::flow_invalidations_fi07_finally_without_return_preserves_try_catch

(3 rows. These three `flow_invalidations` rows are owned EXCLUSIVELY by this block (§10.4.1): `fi03` (closure capture), `fi06` / `fi07` (try / finally control-return override) — their dominant mechanism is the closure barrier / control-flow surface, not the narrowing lattice, so they are NOT listed under U6.NARROWING. The narrowing-preservation rows that are NOT loop / closure / control-flow — `fi01`, `fi02`, `fi04`, `fi05`, `fi09` — are owned by U6.NARROWING (the `fi08` assertion-effect-on-dotted-path row is owned by U6.PREDICATE_ASSERTION by its dominant mechanism — §10.4.1). The loop / break / control-flow catalog rows in `flow_return_catalog.rs` / `flow_return_edge_catalog.rs` (`cf*`, `bl15` divergent-loop, `lr10`/`lr16` closure-capture) and the parity `tp08` try / finally + callback-return row are un-ignored as the catalog macros convert and are NOT among the 363 `IgnoredTestRow`s; §10.4.1 assigns each manifest row to exactly one `block_id` by mechanism.)

Required new guards (parent §5, §6):
- `no_caching_of_partial_or_budget_exceeded_results` — cycle-sentinel / loop-budget / cancellation / supersession results route through `CacheAdmission` non-admission, not warmed (the loop case extends the substrate's `flow_slice_budget_exceeded_admits_nothing`).
- `closure_capture_barrier_widens_captured_mutable_slots` — a call whose callee value escapes widens captured mutable slots back to their declared type at the next statement boundary.
- `predicate_call_does_not_trigger_closure_barrier` — a known-signature predicate / assertion call with no escape-by-mutation effect does NOT trigger the closure barrier (the signature is local and analysable).
- `divergent_loop_models_as_void` — `while (true) {}` with no reachable `break` models as `void`, distinguishable from a function returning `undefined` on a reachable path.

Critical-rule guards: this block implements the parent's `(CRITICAL)` demand-sliced-flow (loop fixed-point + closure barrier) and budget-non-admission rules; the four guards above are their R6 guards. The `flow_policy` candidate differentiation is pinned by `flow_policy_differentiates_cache_candidates` (landed in U6.FLOW_RETURN_SUBSTRATE, exercised here). No NEW `(CRITICAL)` engine rule beyond the parent's.

Proof requirement: per-row — the three `flow_invalidations` rows are `OracleAndGuard`: `fi03` pairs the oracle with `closure_capture_barrier_widens_captured_mutable_slots`, `fi06`/`fi07` pair the oracle with the try / finally control-return assertion. The loop catalog rows (`cf*`) are oracle-pinned; `bl15` (divergent loop) pairs with `divergent_loop_models_as_void`; budget-overflow loop rows pair with `no_caching_of_partial_or_budget_exceeded_results`. Consumed by each row's generated wrapper.

Exit acceptance: the three `flow_invalidations` rows lift and pass; the loop fixed-point converges on slot-equality within `flow_policy.loop_budget`; a loop needing more iterations than the budget returns the iteration-N value, emits the budget-exceeded event, and the slot is NOT warmed; the closure-capture barrier widens captured mutable slots on escape while a known-signature predicate / assertion call does not trigger it; `while (true) {}` models as `void`; try / finally control-return override is exact.

Verification commands:
- `cargo test --package verter_session` loop / closure / control-flow tests (`flow_invalidations` `fi03`/`fi06`/`fi07`, the `cf*` / `bl15` catalog rows).
- `cargo test --package verter_session --test typeinfo_ignored_test_manifest` (coverage gate + the single-block assignment of the shared `flow_invalidations` rows).
- The block's lifted-row proofs via the generated wrapper.
- Full workspace gate (as U6.FLOW_RETURN_SUBSTRATE).

Docs updated: update the `/type-resolution` skill's loop/closure flow notes (the loop-flow operator + all loop forms + labeled break/continue + try/finally control-return override, the loop fixed-point engine with slot-equality convergence + `flow_policy.loop_budget` + non-admission on overflow, the closure-capture barrier + the predicate/assertion exception, divergent-loop-as-`void`); update the `/audit-infrastructure` skill for the `FlowSliceBudgetExceeded` loop-overflow event.

Re-entry notes: idempotent. A budget-exceeded loop result is `ReturnOnly` — the next call cold-rebuilds (it is never a warm hit). The closure barrier widens captured slots on escape; a known-signature predicate / assertion call is exempt. If partial, the manifest shows which loop / closure `flow_invalidations` / `cf*` rows remain `#[ignore]`.

---

## Test-macro conversion (un-`#[ignore]`)

The flow-return catalog tests are un-`#[ignore]`d by converting each
`future_*_contract!` invocation to its non-future counterpart as the resolver
capabilities each characterizes land. The macro families and their conversions:

- `flow_return_catalog.rs` — `future_catalog_contract!` → `catalog_contract!`;
  `future_cross_contract!` → `cross_contract!`; `future_cross_package_contract!` →
  `cross_package_contract!`.
- `flow_return_path_contracts.rs` — `future_path_contract!` → `path_contract!`.
- `flow_return_parity_contracts.rs` — `future_parity_contract!` →
  `parity_contract!`; `future_parity_aug_contract!` → `parity_aug_contract!`.
- `flow_return_edge_catalog.rs` — `future_edge_contract!` → `edge_contract!`;
  `future_edge_package_contract!` → `edge_package_contract!`.

Where a non-future macro variant does not yet exist for a family, the implementer
adds it in the same change. Each `#[ignore]` row's lift is gated by the parent's
two-phase prepare-verify → gate → accept lifecycle (PART 2 §11); a block reaches
`Lifted` only after a green workspace gate over the exact accepted content.

---

## Coverage source-of-truth pointers

The authoritative fixtures and harnesses live in the repo (versioned with the
code, cannot drift from the harness). The manifest
(`crates/verter_session/tests/typeinfo_ignored_test_manifest.rs`) is the row-exact
authority; the per-block "Exact test rows lifted" lists above are the U6 row
assignment, and the generated coverage table (PART 2 §10.4) maps every row to
exactly one `block_id` by `mechanism_id`.

- `crates/verter_session/src/typeinfo/typeinfo_tests/flow_return_catalog.rs` (+
  `.../fixtures/flow_return_catalog.ts`) — primary BL / LR / CN / PA / CG / HO / OB
  / CF / XF / VV flow fixtures.
- `.../flow_return_edge_catalog.rs` (+ `flow_return_edge_catalog.ts`,
  `flow_return_edge_cross.ts`, `flow_return_edge_package_declarations.ts`) — edge
  cases.
- `.../flow_return_parity_contracts.rs` (+ `flow_return_parity_catalog.ts`,
  `flow_return_parity_aug_*.ts`) — TS parity + module-augmentation parity.
- `.../flow_return_path_contracts.rs` (+ `flow_return_path_*.ts`) — path-precise
  projection contracts.
- `.../flow_invalidations.rs` (+ `flow_invalidations.ts`) — invalidation /
  preservation contracts.
- `.../narrow_typeof.rs`, `narrow_equality.rs`, `narrow_truthiness.rs`,
  `narrow_in_operator.rs`, `narrow_instanceof.rs`, `narrow_discriminated_union.rs`
  — narrowing.
- `.../contextual_typing.rs` — contextual typing.
- `.../value_inference.rs` — value inference.
- `.../call_resolution.rs`, `function_advanced.rs` — call / overload / generic
  inference (split with U2.CLASS_SURFACES by mechanism).
- `.../substitution_types.rs` — generic substitution under flow (split with U2 by
  mechanism: flow-narrowing-of-generic rows `sb01`–`sb08`/`sb11`–`sb13` in
  U6.NARROWING, generic-predicate rows `sb09`/`sb10` in U6.PREDICATE_ASSERTION,
  pure-substitution `sb14`/`sb15` in U2.CLASS_SURFACES — §10.4.1).
- `.../cross_file.rs` — cross-file projection. All three `cross_file.rs` manifest
  rows are owned by U3.CACHE_FACT_MODEL (route-fact / cross-file indexed-access
  mechanism), NOT this subplan; the `xf*` cross-file value-return rows in
  `flow_return_catalog.rs` are the U6.CROSS_FILE rows (§10.4.1).

---

## Verification (whole-subplan)

Every block runs the full workspace gate as its final acceptance step (PART 2
§§11.3, 14) — the complete Rust **AND** JavaScript gate, green only when BOTH pass;
non-mutating — clippy-fix / formatting run BEFORE `prepare-verify`
so the gated content is already clean and the receipt certifies an unchanging
input: `cargo test --workspace --tests --verbose`, `cargo clippy --workspace --
-D warnings`, `cargo fmt --all --check`, `pnpm test`, and
`pnpm install --frozen-lockfile`; plus `node scripts/gen-corpus-audit-tests.mjs`
(the audit-record schema gains the `FlowReturnInference` kind / payload + the new
structured events) and, where the wasm cfg-gating surface is touched,
`pnpm build`. A block reaches `Lifted` / `landed_blocks` only after a green
workspace gate over the EXACT accepted content AND the three-reviewer LAND verdict
(1 Claude Code + 2 codex; PART 2 §11.12), via the two-phase prepare-verify →
gate → accept lifecycle with the input-hash-PAIR gate receipt (PART 2 §11.3); the
block's WIP series squashes to ONE mainline land commit at `accept` (PART 2 §11.11). The
parent U6 token is the aggregate over every block above and is done only when every
row in the union of all U6-block row-sets is `Lifted` (PART 2 §11.9) — never
vacuously. Downstream U8 / U11 / U13 / U15 read the flow results; U13 projects the
published shape (`TypeDescriptor`, the graph wire surface), U8 exposes the flow /
contextual facts through `ProgramAnalysisGraph`, and U11 surfaces the audit
footprint through the `AuditedResult` carrier.

The whole-subplan parity guarantee is the parent's composition (Capability Map →
"the guarantee over the 363 rows"): the two-table ledger with the exact-363 count +
bijection (PART 2 §§10.1, 10.5); the U0 row-exact coverage table that DEFINES
completeness (PART 2 §10.4); the per-row executable `ProofRequirement` with the
generated proof registry + row-test wrapper (PART 2 §§10.2–10.3); the two-phase
lifecycle with the gate receipt (PART 2 §11); the no-skip guarantee (PART 2 §12);
and the lease-based parallel-safe resume protocol (PART 2 §14). U0 builds that
machinery; the U6 blocks lift their exact manifest rows through it. No row is owned
twice — the `FlowNarrowing` (104), `ContextualTyping` (13), and `ValueInference`
(7) substrates are wholly U6, while `CallResolution` (28), `TypeParameterFeatures`
(17), `ModernTsFeatures` (6), `TypeScriptRules` (11), `ApparentTypes` (20), and
`ClassFeatures` (13) are row-level split with U2 (and `CrossFileResolution` (3)
with U3) — this subplan's U6 blocks own only the rows whose coverage `block_id` is a
U6 block, and the binding 363 total stays exact.
