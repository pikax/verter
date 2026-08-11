# Verdict

**APPROVE this specification for implementation.** The architecture is durable: the skeleton retains only canonical D6 topology; demanded `FunctionFlowGraph` construction derives completion meaning; A3 receives one sealed typed gap. No fixed-capacity representation, endpoint payload on the skeleton, AST rewalk, second graph, or second classifier is permitted.

I verified:

- `HEAD` is exactly `ed7804aeaf17e9fc9df9944e889cfcad0a88e3e8`.
- The relevant `crates/` implementation is unchanged from the frozen product baseline.
- The worktree contains none of the rejected candidate’s `completion.rs`, endpoint encoding, `CompletionSet`, `CompletionTargetId`, `MAX_COMPLETION_TARGETS`, `active_completion_targets`, `completion_overflowed`, `labels_before_iteration`, or `visit_*_completion` machinery.
- The historical evidence and superseded specifications agree with the stated failures.

The strongest counterargument is performance: even a linear demanded reducer adds cold-request work. The representation below guarantees the allocation, retained-byte, continuity, and asymptotic properties by construction. It cannot mathematically guarantee the frozen 3% wall-clock gates; only the prescribed measurements can do that. A failed timing cell rejects the candidate—there is no architectural permission to weaken the gates.

## Mechanical implementation specification

### 1. Add the canonical completion topology types

Add [completion.rs](<REPO>/crates/verter_semantic/src/analysis/flow/completion.rs) and declare it from [mod.rs](<REPO>/crates/verter_semantic/src/analysis/flow/mod.rs:25).

Define exactly:

```rust
pub struct SkeletonControlId(u32);
pub struct SkeletonCompletionEventId(u32);
pub struct SkeletonCompletionLabelId(u32);
pub struct SkeletonReturnOrdinal(u32);

#[repr(u8)]
pub enum SkeletonControlKind {
    Label,
    Switch,
    SwitchCase,
    While,
    DoWhile,
    For,
    ForIn,
    ForOf,
    Try,
    Catch,
    Finally,
}

#[repr(u8)]
pub enum SkeletonCompletionEventKind {
    Return,
    Throw,
    Break,
    Continue,
}

pub struct SkeletonCompletionLabel {
    offset: u32,
    len: u32,
}

pub struct SkeletonControl {
    region: SkeletonRegionId,
    parent_raw: u32,
    link_raw: u32,
    label_raw: u32,
    span: FrameSpan,
    meta: u32,
}

pub struct SkeletonCompletionEvent {
    region: SkeletonRegionId,
    parent_control_raw: u32,
    payload_raw: u32,
    kind_ordinal: u32,
    span: FrameSpan,
    meta: u32,
}

pub struct SkeletonCompletionTopology {
    controls: Box<[SkeletonControl]>,
    labels: Box<[SkeletonCompletionLabel]>,
    label_bytes: Box<[u8]>,
}
```

Requirements:

- Use `u32::MAX` privately as the absent-ID sentinel. Do not expose raw sentinel values.
- `SkeletonControl` must be exactly 28 bytes.
- `SkeletonCompletionEvent` must be exactly 28 bytes.
- `SkeletonCompletionLabel` must be exactly 8 bytes.
- Add compile-time size assertions with `static_assertions`.
- All types derive the appropriate `Debug`, `Clone`, equality/hash traits and `NoTypeExpr`.
- All spans are `FrameSpan`; absolute spans are forbidden.

Field interpretation is fixed:

- `SkeletonControl.parent_raw` is the immediately enclosing completion control.
- `link_raw` is used only by `Label`, and names its direct labeled-body control when that direct body is another label or an iteration. Otherwise it is absent.
- `SwitchCase.parent_raw` names its `Switch`.
- `Catch.parent_raw` and `Finally.parent_raw` name their `Try`.
- `Try.parent_raw` names the surrounding completion control.
- `label_raw` is a `SkeletonCompletionLabelId` only for `Label`.
- `SkeletonCompletionEvent.payload_raw` is:
  - expression-site ID for `Return` and `Throw`;
  - label ID for labeled `Break`/`Continue`;
  - absent for bare return and unlabeled `Break`/`Continue`.
- `kind_ordinal` is the dense return ordinal for `Return`, and the dense non-return abrupt ordinal otherwise.
- `meta` contains only kind and structural flags: implicit return, switch-case default, loop-test presence, and parser-recovery flag. It must contain no target, endpoint, completion set, or derived normal-completion fact.

This is D6 topology, not an A3 payload: controls define structured routing identity, events are the graph’s abrupt source nodes, labels provide collision-free exact routing, and spans/regions establish source ordering and containment.

### 2. Generalize the existing return-site index

At [mod.rs](<REPO>/crates/verter_semantic/src/analysis/flow/mod.rs:123), remove `SkeletonReturnSiteId`. At [mod.rs](<REPO>/crates/verter_semantic/src/analysis/flow/mod.rs:520), remove `SkeletonReturnSite`.

At [FunctionBodySkeleton](<REPO>/crates/verter_semantic/src/analysis/flow/mod.rs:558), replace:

```rust
pub return_sites: Arc<[SkeletonReturnSite]>
```

with:

```rust
pub completion_events: Arc<[SkeletonCompletionEvent]>,
pub completion_topology: Option<Arc<SkeletonCompletionTopology>>,
```

Rules:

- `completion_events` is the sole abrupt-event index.
- Its order is authored source order.
- The implicit expression-bodied-arrow return is a `Return` event with `implicit = true`.
- Nested functions remain separate skeletons and never contribute events to the enclosing skeleton.
- `completion_topology` is `None` exactly when `C == 0`; do not allocate an empty topology.
- `completion_events` uses the same empty-`Arc` behavior as the existing empty return-site collection and must not introduce an allocation when `E == 0`.

Add these accessors only:

```rust
pub fn completion_event(
    &self,
    id: SkeletonCompletionEventId,
) -> &SkeletonCompletionEvent;

pub fn return_events(
    &self,
) -> impl ExactSizeIterator<Item = (SkeletonCompletionEventId, &SkeletonCompletionEvent)>;

pub fn completion_topology(&self) -> Option<&SkeletonCompletionTopology>;
```

`return_events()` filters `kind == Return`; it must not allocate. Do not add any endpoint, target, root-normal, or completion-coverage accessor to `FunctionBodySkeleton`.

Update return consumers mechanically:

- [peeker.rs](<REPO>/crates/verter_semantic/src/analysis/flow/peeker.rs:94): iterate `return_events()`.
- [lower.rs](<REPO>/crates/verter_semantic/src/analysis/flow/lower.rs:46): carry `SkeletonCompletionEventId`; assert/debug-assert `Return` before reading argument or implicitness.
- [flow_graph.rs](<REPO>/crates/verter_semantic/src/analysis/flow/flow_graph.rs:35): replace return-site imports with completion-event imports.
- Update graph, hashing, peeker, lowering, session cache, and skeleton tests found by searching `SkeletonReturnSite`, `return_sites`, and `return_site_node`.
- Do not retain a compatibility `return_sites` slice or build a second return index.

### 3. Extend the single skeleton visitor

At [SkeletonBuilder](<REPO>/crates/verter_semantic/src/analysis/flow/mod.rs:924), replace the return-site draft vector with:

```rust
completion_events: Vec<CompletionEventDraft<'a>>,
control_head: Option<Box<ControlDraft<'a>>>,
control_count: u32,
current_control: Option<SkeletonControlId>,
return_count: u32,
abrupt_count: u32,
```

Implementation rules:

1. Keep exactly one AST visitor.
2. `ControlDraft` is a singly linked draft allocated once per control. Do not use a geometrically growing `Vec<SkeletonControl>`; that would reintroduce a 64/65 allocator cliff.
3. Opening a control:
   - assign `SkeletonControlId(control_count)`;
   - increment with checked conversion;
   - record current control as parent;
   - prepend its draft;
   - install it as `current_control`;
   - restore the previous scalar on exit.
4. `LabeledStatement` creates a `Label` control before visiting its body. After dispatching the direct body, set `link_raw` only if the direct statement is a labeled statement or iteration.
5. Every switch creates one `Switch` control and one `SwitchCase` control per case/default.
6. Every iteration statement creates exactly one control with its precise kind. Record whether a `for`/`while` test exists; do not evaluate it.
7. A try statement creates:
   - one `Try` control spanning the complete `TryStatement`;
   - optional `Catch` and `Finally` child controls;
   - events in the try block parented to `Try`;
   - events in catch/finally parented to their respective child controls.
8. [visit_return_statement](<REPO>/crates/verter_semantic/src/analysis/flow/mod.rs:1939) appends `Return`.
9. [visit_throw_statement](<REPO>/crates/verter_semantic/src/analysis/flow/mod.rs:1961) must retain the argument site and append `Throw`.
10. Add `visit_break_statement` and `visit_continue_statement`; retain only the optional label spelling and structural parent.
11. Do not resolve a target in the visitor.
12. Do not add `active_completion_targets`, an active-label vector, target bitsets, endpoint composition, or `visit_*_completion`.

At [finish](<REPO>/crates/verter_semantic/src/analysis/flow/mod.rs:1651):

- Canonicalize all label spellings into dense `SkeletonCompletionLabelId`s using an MSD byte-radix grouping pass over control/event drafts. Equality must compare the complete UTF-8 byte slice; hashes/fingerprints alone are never identity.
- Assign label IDs lexicographically, so output is deterministic.
- Allocate `label_bytes`, `labels`, and the final control slice at their exact lengths.
- Materialize the reversed linked controls directly into `Box::<[SkeletonControl]>::new_uninit_slice(C)` by dense ID. Use one reviewed unsafe initialization helper and document that every slot is written exactly once.
- Convert completion-event drafts to the exact final `Arc` slice.
- Do not rewalk the AST.

### 4. Add completion nodes without perturbing existing dependency nodes

At [FlowNodeKind](<REPO>/crates/verter_semantic/src/analysis/flow/flow_graph.rs:65), rename the existing return node concept to `ReturnProjection` and add:

```rust
pub enum CompletionBoundary {
    Entry,
    RootNormalExit,
    ReturnExit,
    ThrowExit,
}

pub enum FlowNodeKind {
    Binding(SkeletonBindingId),
    ExprSite(SkeletonExprSiteId),
    ReturnProjection(SkeletonCompletionEventId),
    Region(SkeletonRegionId),
    CompletionBoundary(CompletionBoundary),
    CompletionControl(SkeletonControlId),
    CompletionEvent(SkeletonCompletionEventId),
}
```

The node layout is fixed:

1. existing bindings;
2. existing expression sites;
3. return projections in dense `SkeletonReturnOrdinal` order;
4. existing regions;
5. four completion boundaries;
6. controls in `SkeletonControlId` order;
7. events in `SkeletonCompletionEventId` order.

The first four ranges must retain their current offsets. Consequently, all existing binding/expression/return-projection/region IDs remain stable.

Keep dependency edges and completion edges in separate storage planes inside the same `FunctionFlowGraph`. This is not a second graph because both planes:

- use the same `FlowNodeId`;
- are built in the same `build_function_flow_graph` call;
- live in the same `FunctionFlowGraph`;
- share the existing `FunctionFlowGraphStore` memo/lifetime/key;
- have no independent owner, cache, builder entry point, or coverage reducer.

Do not add completion variants to the current large `FlowEdgeKind`; that would burden existing dependency traversal and breach the retained-edge budget.

### 5. Define the compact completion edge plane

In [flow_graph.rs](<REPO>/crates/verter_semantic/src/analysis/flow/flow_graph.rs:65), add:

```rust
#[repr(u8)]
pub enum CompletionEdgeKind {
    Sequential,
    ConditionalTaken,
    ConditionalBypass,
    SwitchDispatch,
    SwitchFallthrough,
    LoopEnter,
    LoopBack,
    Return,
    ThrowExit,
    ThrowToCatch,
    Break,
    Continue,
    FinallyEnter,
    FinallyPreserve,
    FinallyOverride,
    FinallyInferencePreserve,
}

pub struct CompletionEdge {
    from: FlowNodeId,
    to: FlowNodeId,
    aux_raw: u32,
    meta: u32,
}

struct CompletionEdges {
    edges: Box<[CompletionEdge]>,
    offsets: Box<[u32]>,
}
```

Requirements:

- `CompletionEdge` is exactly 16 bytes.
- `offsets` covers only the dense appended completion range: four boundaries plus `C + E` nodes.
- `aux_raw` is a typed control/event/predicate ID selected by edge kind. Add kind-specific constructors and accessors; production code must not interpret raw values directly.
- `FunctionFlowGraph` receives:

```rust
completion: Option<CompletionEdges>,
completion_coverage: CompletionCoverage,
return_event_ids: Box<[SkeletonCompletionEventId]>,
```

- `completion == None` exactly when `C == E == G == 0`.
- `return_event_ids[return_ordinal]` maps the preserved dependency projection node back to the sole completion event.
- Existing `out_edges()` returns an empty dependency slice for appended completion nodes.
- Add crate-private `completion_out_edges(FlowNodeId)`.
- Do not make graph edges or target access public to the session/A3 layer.

### 6. Define `CompletionCoverage` and typed unknowns

Add to `completion.rs`:

```rust
pub enum RootNormalCoverage {
    Reachable,
    Unreachable,
    Unknown(CompletionCoverageUnknown),
}

pub struct CompletionCoverage {
    authored_return_present: bool,
    root_normal: RootNormalCoverage,
}

pub enum CompletionCoverageUnknown {
    MalformedControlTopology(SkeletonControlId),
    RecoveredSpanOrder(SkeletonRegionId),
    UnresolvedBreak(SkeletonCompletionEventId),
    UnresolvedContinue(SkeletonCompletionEventId),
    PredicateFeasibility(SkeletonRegionId),
    NestedFinallyState(SkeletonControlId),
    UnsupportedFinallyInference(SkeletonControlId),
}
```

No `Default`, boolean conversion, `unwrap_or`, or unknown-collapsing accessor is allowed.

The endpoint expectation is derived transiently:

```text
no authored Return
    => Exact(DoesNotContribute)

authored Return + root normal Reachable
    => Exact(Contributes)

authored Return + root normal Unreachable
    => Exact(DoesNotContribute)

authored Return + root normal Unknown(reason)
    => Unknown(reason)
```

A bare `return;` is an authored undefined return arm but is not an endpoint-undefined contribution. Do not conflate those concepts.

### 7. Extend `build_function_flow_graph`

Extend [build_function_flow_graph](<REPO>/crates/verter_semantic/src/analysis/flow/flow_graph.rs:243) after the existing dependency-plane construction.

The reducer must perform exactly these phases:

1. **Empty fast path.** When `C == E == 0`, set exact root-normal reachable, no authored return, and allocate nothing completion-owned.

2. **Region classification scratch.** In one parent-before-child pass over existing `SkeletonRegion` records:
   - group `IfConsequent`/`IfAlternate` by their shared `control_input`;
   - record missing-else bypasses;
   - record conditional ancestry;
   - validate parent/span containment.
   Do not retain separate branch records on the skeleton. Branch records are absent from `C`/`E`, so retaining them would violate the frozen byte formulas.

3. **Control resolution.** Allocate exact-size `ResolvedControl` scratch, no geometric vectors. In one source-order traversal:
   - maintain scalar current unlabeled break, continue, catch, and finally destinations;
   - maintain an exact `current_label_target: Box<[u32]>` indexed by dense `SkeletonCompletionLabelId`;
   - push/restore a label entry at label entry/exit;
   - compute labeled-continue eligibility by following only the label’s direct-body `link_raw`;
   - populate switch-case ownership and try/catch/finally relationships from control parent/kind.
   No event may scan ancestor depth.

4. **Event resolution.** Allocate exactly one 8-byte `ResolvedEvent` per event:
   - destination ID;
   - nearest crossed finally ID or absent.
   `Return` routes to `ReturnExit`; unhandled throw routes to `ThrowExit`; explicit throw in a try block routes to that try’s catch; breaks and continues use the resolved target tables.

5. **Edge count.** Count edges from resolved control/event scratch without repeating target resolution. Allocate final edge and offset boxes at exact sizes.

6. **Edge emission.** Emit edges once, grouped by source node.

7. **Coverage reduction.** Derive `CompletionCoverage` from the emitted completion edge plane, not from syntax or a parallel completion-set algebra. Use the reachability lattice:
   - `Absent`;
   - `Conditional`;
   - `Definite`.
   A definite path to `RootNormalExit` is `Reachable`; no path is `Unreachable`; only conditional paths are `Unknown(PredicateFeasibility(...))`.

Specific semantics:

- Missing `else` emits a conditional bypass.
- Switch without default emits a conditional no-match bypass.
- Switch with default has no no-match bypass.
- Case normal completion falls through to the next case.
- Unlabeled `break` targets the nearest loop/switch; labeled break targets only the exact active label.
- `continue label` is legal only when the label’s direct-body chain terminates at an iteration.
- Explicit throw in the try block routes to its catch; throw in the catch does not route back to the same catch.
- Finally intercepts pending completion from try and catch, but never its own abrupt event.
- A normally completing finalizer emits `FinallyPreserve`.
- An abrupt finalizer emits `FinallyOverride`.
- For the pinned TypeScript inference behavior only, a pending `Break` passing through a return-only finalizer emits `FinallyInferencePreserve` to the original break destination. This produces X68/X80 while still routing X88 through its suffix.
- Any result requiring nested-finally state routing is `Unknown(NestedFinallyState)` in A2C. Implementing that state machine belongs to later D6.
- Any other inference/runtime divergence is `Unknown(UnsupportedFinallyInference)`.
- Never infer predicate truth, call effects, throwing expressions, or loop convergence.

### 8. Add the A3-facing sealed contract

Define in `completion.rs`:

```rust
pub enum EndpointUndefinedDisposition {
    DoesNotContribute,
    Contributes,
}

pub enum AbruptCompletionExpectation {
    Exact(EndpointUndefinedDisposition),
    Unknown(CompletionCoverageUnknown),
}

pub enum FlowGap {
    AbruptCompletion {
        expected: AbruptCompletionExpectation,
        observed: EndpointUndefinedDisposition,
    },
}
```

At [FlowGraphBundle](<REPO>/crates/verter_session/src/cache_runtime/flow_slice_node.rs:203), make no storage change. Add `FlowSliceStores::bundle_for(...) -> Option<Arc<FlowGraphBundle>>` beside [skeleton_for](<REPO>/crates/verter_session/src/cache_runtime/flow_slice_node.rs:567).

At [flow_return.rs](<REPO>/crates/verter_session/src/project_semantic_dispatch/flow_return.rs:2102), replace the separate skeleton lookup with `bundle_for`; pass `bundle.skeleton` to existing content lowering and read only `bundle.graph.completion_coverage()` for the comparison.

Add to [FlowEvaluationOutcome](<REPO>/crates/verter_session/src/project_semantic_dispatch/flow_return.rs:246) and the transient pending/SCC carriers:

```rust
flow_gap: Option<FlowGap>
```

The affected carriers are:

- `FlowEvaluationOutcome`;
- [FlowReturnPendingState](<REPO>/crates/verter_session/src/project_semantic_dispatch/dispatch_txn.rs:631);
- `FlowDischargeEntry`;
- [CompletedFlowReturnMember](<REPO>/crates/verter_session/src/project_semantic_dispatch/dispatch_txn.rs:1836);
- `FlowRootClose::Complete`.

Do not add the field to `FlowReturnResult`, a cache key, admitted cache entry, audit JSON, or public semantic type.

At the existing result construction immediately before [line 2388](<REPO>/crates/verter_session/src/project_semantic_dispatch/flow_return.rs:2388):

```text
observed =
    result.can_fall_through
        ? Contributes
        : DoesNotContribute

expected = graph.completion_coverage().endpoint_expectation()

flow_gap =
    Exact(expected) where expected == observed => None
    Exact(expected)                         => AbruptCompletion
    Unknown(reason)                         => AbruptCompletion
```

Under A2C, the single root-close adapter drops `flow_gap` and returns the existing `FlowReturnResult` unchanged. A3 changes only this adapter to match `FlowGap::AbruptCompletion` and retract/non-admit. A3 receives no graph, skeleton, event, region, edge, or endpoint accessor.

### 9. Required exact discrimination

| Case | Expected | Observed | Gap |
|---|---|---|---|
| X05 | `Exact(DoesNotContribute)` | `DoesNotContribute` | `None` |
| X68 | `Exact(Contributes)` | `Contributes` | `None` |
| X80 | `Exact(Contributes)` | `Contributes` | `None` |
| X88 | `Exact(DoesNotContribute)` | `DoesNotContribute` | `None` |
| Genuine labeled G10 | `Exact(DoesNotContribute)` | `Contributes` | `Some(FlowGap::AbruptCompletion { … })` |

For X05, explicit `Throw` routes into `Catch`, the catch return terminates that path, and the suffix return terminates the non-throw path. Therefore root-normal coverage is `Unreachable`. Its public result remains exactly:

- checker match;
- clean;
- `MatchesChecker`;
- `degradation: None`;
- `candidates: 1`.

For genuine G10:

```ts
function makeProps() {
  L: try { break L } finally { return "a" as const }
  R: { return "b" as const }
}
```

The inference-preserved break exits `L`, reaches `R`, and encounters its return; no route reaches the function endpoint. The graph therefore says `DoesNotContribute`, while the legacy suffix evaluator currently claims `Contributes`. That exact disagreement—not mere complexity—creates the typed gap.

### 10. Non-interference requirements

A2C changes no public result because:

- dependency nodes retain their old offsets;
- dependency edges remain unchanged;
- completion edges are invisible to `ReturnPathPeeker`, slice hashing, and lowering;
- return projection order and payload are preserved through `SkeletonReturnOrdinal`;
- no cache key or stable hash includes completion coverage;
- `FlowGap` exists only in transient cold-compute carriers;
- the A2C root adapter discards it before public return and admission;
- existing `FlowReturnDegradation` is unchanged;
- existing syntax lowering remains the observed claim and is not edited or extended.

This makes interference structurally impossible through the permitted APIs. It is not merely “no observed test changed.”

### 11. Capacity and memory design

There is no `MAX_COMPLETION_TARGETS`.

Controls use:

- dense `u32` IDs;
- an exact final boxed slice;
- one fixed-size linked draft allocation per authored control;
- an exact packed label table.

Graph scratch uses exact-length boxes based on known `C`, `E`, and label counts. No `Vec` capacity transition or power-of-two hash table participates in target handling.

For the 64/65/66 target fixtures, one additional label adds approximately:

- 28 retained control bytes;
- 8 retained label-table bytes;
- its packed spelling bytes.

That remains below 48 retained bytes per target. Builder requested bytes are the fixed draft, final control, label entry, and spelling, below 96 bytes per target. No representation switch, overflow, fallback, or unknown occurs.

The cost is linear retained topology plus demanded graph work. A cold flow request now pays completion target resolution, compact edge emission, and root reachability before content evaluation. Warm graph reuse pays none of the build cost.

### 12. Performance acceptance

Add [a2c_completion.rs](<REPO>/crates/verter_bench/benches/a2c_completion.rs) and a matching `[[bench]]` entry to [verter_bench/Cargo.toml](<REPO>/crates/verter_bench/Cargo.toml).

The harness must implement the charter verbatim, including:

- frozen 199-row blob and 3% aggregate skeleton gate;
- cold `mixed(4)`, `mixed(64)`, `mixed(256)`, `targets(65)` cells;
- 2/8/25/2 ms SLOs;
- 64/65/66 continuity formulas;
- `controls`, `events`, `finally` at 64/256/1024/4096;
- `W = C + E + G`;
- `6C + 6E + 4G + 32` work bound;
- all frozen time and byte formulas;
- zero added calls/bytes for `C = E = G = 0`;
- exact pairing, warmups, controls, bootstrap seed and invalidation rules.

Add an `a2c-evidence` feature for work counters only. It must compile out completely in production and latency builds. Allocation measurements use a separate counting-allocator binary and may not supply latency observations.

Every retained byte in the evidence report must map to:

- control field;
- label identity/spelling;
- completion event field;
- completion node offset;
- completion edge;
- return-event projection mapping;
- `CompletionCoverage`.

### 13. Required tests

Add [completion_skeleton_tests.rs](<REPO>/crates/verter_semantic/src/analysis/flow/completion_skeleton_tests.rs):

- `completion_index_generalizes_returns_in_source_order`
- `completion_index_records_dense_controls_and_exact_labels`
- `completion_index_has_no_target_capacity_boundary`
- `completion_index_is_no_type_expr_send_sync_static`
- `completion_index_spans_are_frame_relative`
- `completion_index_is_position_invariant`
- `completion_index_is_deterministic`
- `completion_index_allocates_nothing_when_c_and_e_are_zero`

Add graph tests to [flow_graph_tests.rs](<REPO>/crates/verter_semantic/src/analysis/flow/flow_graph_tests.rs):

- `completion_label_routes_only_matching_break`
- `completion_switch_routes_own_break_and_preserves_foreign_break`
- `completion_try_routes_throw_into_catch`
- `completion_finally_abrupt_replaces_runtime_completion`
- `completion_finally_return_preserves_break_for_inference`
- `a2c_g10_labeled_suffix_is_exact_endpoint_absent`
- `a2c_g10_try_suffix_is_exact_endpoint_absent`
- `a2c_g10_throw_suffix_is_exact_endpoint_absent`
- `a2c_switch_terminal_suffix_is_exact_endpoint_absent`
- `a2c_catch_terminal_suffix_is_exact_endpoint_absent`
- `a2c_x05_catch_return_is_exact_and_not_a_hazard`
- `a2c_x68_endpoint_undefined_is_exact_present`
- `a2c_x80_endpoint_undefined_is_exact_present`
- `a2c_x88_outer_suffix_makes_endpoint_undefined_exact_absent`
- `completion_malformed_break_is_typed_unknown`
- `completion_malformed_continue_is_typed_unknown`
- `completion_nested_finally_state_is_typed_unknown`
- `completion_dependency_plane_is_byte_for_byte_unchanged`

Add [u6_flow_a2c_non_interference_tests.rs](<REPO>/crates/verter_session/src/u6_flow_a2c_non_interference_tests.rs):

- `a2c_g10_emits_gap_while_public_result_is_unchanged`
- `a2c_x05_gap_is_none_and_public_result_stays_clean_warm`
- `a2c_x68_gap_is_none_and_public_result_stays_exact_clean_warm`
- `a2c_x80_gap_is_none_and_public_result_stays_exact_clean_warm`
- `a2c_x88_gap_is_none_and_public_result_stays_exact_clean_warm`
- `a2c_completion_plane_does_not_change_slice_hash`
- `a2c_completion_plane_does_not_change_cache_admission`
- `a2c_two_demands_build_one_shared_flow_graph`

For X05/X68/X80/X88 assert cold then warm, exact JSON, `degradation == None`, `candidates == 1`, `MatchesChecker`, identical cold/warm JSON.

Mutation evidence is mandatory and independent:

1. Change labeled break resolution to select the nearest active label regardless of spelling.  
   Must fail `completion_label_routes_only_matching_break`.

2. Route explicit try-block throw to `ThrowExit` instead of its catch.  
   Must fail `completion_try_routes_throw_into_catch`. Coverage-only X05 is insufficient because that wrong-direction mutation can coincidentally preserve an absent endpoint.

3. Preserve a pending completion when a finalizer has no normal edge.  
   Must fail `completion_finally_abrupt_replaces_runtime_completion`.

Run the canonical gates:

```text
cargo test -p verter_semantic completion
cargo test -p verter_session a2c_
cargo nextest run --workspace
cargo test -p verter_session --tests
```

Record raw performance data, layout sizes, byte attribution, mutation diffs/results, candidate SHA, and unchanged public traces in `docs/arch/refactor/rev11/evidence/A2C-summary.md`.

### 14. Scope and wrong-exact risks

1. **D5 absorption:** inspecting call effects, expression throwability, captures, writes, freshness, escape, or value types is forbidden. Only explicit `throw` is a throw event.

2. **Later D6 absorption:** no loop fixed point, binding-state routing, predicate feasibility, nested-finally continuation state, or final clean semantics. Those cases become typed unknown.

3. **D8 absorption:** `CompletionCoverage::Exact` is not a proof token, does not mint `CompleteFlowReturn`, and does not alter admission in A2C.

4. **Second-authority risk:** do not extend `statement_guarantees_current_function_return`, syntax suffix scans, or legacy break handling. They remain only the observation until A3/D6 removes them.

5. **Wrong label direction:** unresolved or recovered labels are `Unknown`; never choose the nearest target.

6. **Wrong catch direction:** catch only intercepts explicit throws originating in its associated try block.

7. **Wrong finally direction:** runtime override and the pinned return-inference preservation are distinct edges. Implementing only one direction breaks either G10/X88 or X68/X80.

8. **Wrong switch direction:** default removes the no-match bypass; lack of default retains a conditional bypass.

9. **Wrong loop direction:** never infer a condition as true/false or assume an iteration count.

10. **Unknown laundering:** unknown may not convert to either exact endpoint value.

11. **Recovery risk:** inconsistent span containment, duplicated active labels, or invalid target ownership produces a specifically identified unknown, never a guessed exact answer.

12. **Hash-collision risk:** hashes may accelerate label processing but never establish equality. The dense label ID is derived from full UTF-8 bytes.

13. **Memory risk:** a retained generic branch/sequence table is forbidden because its size is not represented by `C` or `E`. Reuse existing regions transiently during graph construction.

14. **Graph-split risk:** `CompletionEdges` must remain a private storage plane of `FunctionFlowGraph`, not a separately named graph owner/store/memo.

15. **Timing risk:** linked control drafts protect continuity but add one transient allocation per control. If the frozen skeleton aggregate fails, replace only the transient construction mechanism under the same final representation and same frozen gates; do not restore fixed capacity or eager completion meaning.

__DONE__
