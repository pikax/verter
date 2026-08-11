Verdict: **IMPLEMENTABLE, with no Revision 11 contradiction and no deviation required.** The design below keeps A2C a content-free fact producer, represents uncertainty explicitly, performs no query-time AST walk, and leaves all public behavior unchanged. Baseline HEAD and clean worktree were verified at `70ea4c01bea870e9684a66f229230808aeb64235`.

One execution precondition remains: `charters/A2C.md` still has `Gate 0 lineage SHA: UNSET`, and the authoritative program ledger is external under ruling R-6. Implementation must not begin until the orchestrator records the accepted A2 SHA and unlocks A2C. That is an execution-state requirement, not a design gap.

## 1. Add the canonical completion fact module

Create [completion.rs](<REPO>/crates/verter_semantic/src/analysis/flow/completion.rs) and register it immediately after `pub mod frame_span` at [flow/mod.rs:43](<REPO>/crates/verter_semantic/src/analysis/flow/mod.rs:43):

```rust
pub mod completion;

pub use completion::{
    AuthoredReturnMembership, CompletionFact, CompletionKind, CompletionSet,
    CompletionTarget, CompletionTargetId, CompletionTargetKind, CompletionUnknown,
    EndpointUndefinedDisposition, EndpointUndefinedFact, FunctionCompletionFacts,
    NormalCompletionDisposition, NormalCompletionFact, StatementCompletionFact,
    MAX_COMPLETION_TARGETS,
};
```

Use exactly these definitions in `completion.rs`:

```rust
use std::sync::Arc;

use verter_no_typeexpr::NoTypeExpr;

use super::{FlowNameId, FrameSpan};

pub const MAX_COMPLETION_TARGETS: usize = 64;

#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, NoTypeExpr,
)]
#[repr(transparent)]
pub struct CompletionTargetId(u8);

impl CompletionTargetId {
    pub(crate) fn from_index(index: usize) -> Option<Self> {
        (index < MAX_COMPLETION_TARGETS).then_some(Self(index as u8))
    }

    #[must_use]
    pub const fn index(self) -> usize {
        self.0 as usize
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, NoTypeExpr)]
pub enum CompletionKind {
    Normal,
    Return,
    Throw,
    Break(CompletionTargetId),
    Continue(CompletionTargetId),
}

/// An exact set of syntactically possible completions before value/flow
/// feasibility. Target-bearing variants use one bit per function-local
/// completion target.
///
/// The 64-target ceiling is deliberate. Overflow is CompletionUnknown and
/// must never be truncated.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, NoTypeExpr)]
#[repr(C)]
pub struct CompletionSet {
    simple: u8,
    _padding: [u8; 7],
    breaks: u64,
    continues: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, NoTypeExpr)]
#[repr(u8)]
pub enum CompletionUnknown {
    TargetCapacityExceeded,
    UnresolvedBreakTarget,
    UnresolvedContinueTarget,
    LoopFlowRequired,
    ConditionalFlowRequired,
    FinallyInferenceFlowRequired,
    UnsupportedWithStatement,
    UnsupportedRecoveredSyntax,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, NoTypeExpr)]
pub enum CompletionFact {
    Exact(CompletionSet),
    Unknown(CompletionUnknown),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, NoTypeExpr)]
#[repr(u8)]
pub enum CompletionTargetKind {
    Label,
    Switch,
    Loop,
}

#[derive(Debug, Clone, PartialEq, Eq, NoTypeExpr)]
pub struct CompletionTarget {
    pub span: FrameSpan,
    pub label: Option<FlowNameId>,
    pub kind: CompletionTargetKind,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, NoTypeExpr)]
#[repr(transparent)]
pub struct AuthoredReturnMembership(u8);

impl AuthoredReturnMembership {
    const EXPLICIT_BARE: u8 = 1 << 0;
    const EXPLICIT_VALUE: u8 = 1 << 1;
    const IMPLICIT_VALUE: u8 = 1 << 2;

    pub const NONE: Self = Self(0);

    #[must_use]
    pub const fn contains_any(self) -> bool {
        self.0 != 0
    }

    #[must_use]
    pub const fn contains_explicit_bare(self) -> bool {
        self.0 & Self::EXPLICIT_BARE != 0
    }

    #[must_use]
    pub const fn contains_explicit_value(self) -> bool {
        self.0 & Self::EXPLICIT_VALUE != 0
    }

    #[must_use]
    pub const fn contains_implicit_value(self) -> bool {
        self.0 & Self::IMPLICIT_VALUE != 0
    }

    pub(crate) const fn explicit_bare() -> Self {
        Self(Self::EXPLICIT_BARE)
    }

    pub(crate) const fn explicit_value() -> Self {
        Self(Self::EXPLICIT_VALUE)
    }

    pub(crate) const fn implicit_value() -> Self {
        Self(Self::IMPLICIT_VALUE)
    }

    pub(crate) const fn union(self, other: Self) -> Self {
        Self(self.0 | other.0)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, NoTypeExpr)]
#[repr(u8)]
pub enum NormalCompletionDisposition {
    MayCompleteNormally,
    DoesNotCompleteNormally,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, NoTypeExpr)]
pub enum NormalCompletionFact {
    Exact(NormalCompletionDisposition),
    Unknown(CompletionUnknown),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, NoTypeExpr)]
#[repr(u8)]
pub enum EndpointUndefinedDisposition {
    Contributes,
    DoesNotContribute,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, NoTypeExpr)]
pub enum EndpointUndefinedFact {
    Exact(EndpointUndefinedDisposition),
    Unknown(CompletionUnknown),
}

#[derive(Debug, Clone, PartialEq, Eq, NoTypeExpr)]
pub struct StatementCompletionFact {
    pub span: FrameSpan,
    pub completion: CompletionFact,

    /// Whether the remaining statements in this statement's immediate
    /// statement list may complete normally. This is a typed fact, never
    /// the old syntax allowlist.
    pub following_siblings_normal: NormalCompletionFact,

    pub authored_returns: AuthoredReturnMembership,
}

#[derive(Debug, Clone, PartialEq, Eq, NoTypeExpr)]
pub struct FunctionCompletionFacts {
    pub body: CompletionFact,
    pub statements: Arc<[StatementCompletionFact]>,
    pub targets: Arc<[CompletionTarget]>,
    pub endpoint_undefined: EndpointUndefinedFact,
    pub authored_returns: AuthoredReturnMembership,
}
```

`CompletionSet` must expose only canonical constructors and operations:

- `singleton(CompletionKind)`
- `contains(CompletionKind)`
- `union`
- `without_normal`
- `only_breaks`
- `sequence`
- `route_break(target)`
- `route_continue(target)`
- `has_normal`
- `has_any_abrupt`
- `is_exactly_return`

Do not implement `Default`. In particular, neither an empty set nor `Unknown` may silently mean `Normal`.

Required layout assertions:

```rust
assert_eq!(std::mem::size_of::<CompletionTargetId>(), 1);
assert_eq!(std::mem::size_of::<CompletionSet>(), 24);
assert_eq!(std::mem::size_of::<CompletionFact>(), 32);
assert_eq!(std::mem::size_of::<StatementCompletionFact>(), 48);
assert_eq!(std::mem::size_of::<CompletionTarget>(), 20);

#[cfg(target_pointer_width = "64")]
assert_eq!(std::mem::size_of::<FunctionCompletionFacts>(), 72);

#[cfg(target_pointer_width = "32")]
assert_eq!(std::mem::size_of::<FunctionCompletionFacts>(), 56);
```

If these assertions fail, reorder fields until they hold. Do not relax them.

## 2. Store the facts on `FunctionBodySkeleton`

At [flow/mod.rs:558](<REPO>/crates/verter_semantic/src/analysis/flow/mod.rs:558), add this as the final field:

```rust
/// Content-free exact-or-typed-unknown completion facts, computed during
/// this skeleton's single construction walk.
pub completion: FunctionCompletionFacts,
```

At [flow/mod.rs:573](<REPO>/crates/verter_semantic/src/analysis/flow/mod.rs:573), add:

```rust
#[must_use]
pub fn endpoint_undefined_disposition(&self) -> EndpointUndefinedFact {
    self.completion.endpoint_undefined
}

#[must_use]
pub fn statement_completion(
    &self,
    span: FrameSpan,
) -> Result<&StatementCompletionFact, CompletionUnknown> {
    self.completion
        .statements
        .binary_search_by_key(
            &(span.start(), span.end()),
            |fact| (fact.span.start(), fact.span.end()),
        )
        .map(|index| &self.completion.statements[index])
        .map_err(|_| CompletionUnknown::UnsupportedRecoveredSyntax)
}
```

If `FrameSpan` lacks `start()`/`end()` accessors, add crate-visible accessors to [frame_span.rs:83](<REPO>/crates/verter_semantic/src/analysis/flow/frame_span.rs:83). Do not convert to an absolute span.

Every new coordinate is therefore either:

- absent from the fact, or
- a `FrameSpan` rebased through the existing `SkeletonBuilder::frame_span` ingress at [flow/mod.rs:966](<REPO>/crates/verter_semantic/src/analysis/flow/mod.rs:966).

No `oxc_span::Span`, `verter_span::Span`, absolute `u32`, file offset, URI, or source string may be retained by the new types.

## 3. Integrate composition into the existing skeleton walk

Extend `SkeletonBuilder` at [flow/mod.rs:924](<REPO>/crates/verter_semantic/src/analysis/flow/mod.rs:924) with:

```rust
completion_statements: Vec<StatementCompletionFact>,
completion_targets: Vec<CompletionTarget>,
break_target_stack: Vec<ActiveCompletionTarget>,
continue_target_stack: Vec<ActiveCompletionTarget>,
active_labels: Vec<ActiveCompletionLabel>,
last_statement_completion: Option<CompletionDraft>,
body_completion: Option<CompletionDraft>,
```

`CompletionDraft` is private to `flow/mod.rs`:

```rust
struct CompletionDraft {
    runtime: CompletionFact,
    inference: CompletionFact,
    authored_returns: AuthoredReturnMembership,
}
```

`runtime` is the canonical syntactic completion set. `inference` exists only during construction to calculate TypeScript’s endpoint-`undefined` disposition. Do not publish a second graph or retain `CompletionDraft`.

Modify `visit_statement_list` at [flow/mod.rs:1638](<REPO>/crates/verter_semantic/src/analysis/flow/mod.rs:1638) to return `CompletionDraft`:

1. Visit statements in authored order exactly once.
2. Capture each completed statement’s draft.
3. Compose the list using `sequence`.
4. Walk only the completed draft indexes in reverse—never the AST—to populate `following_siblings_normal`.
5. Sort the final published statement facts by `(FrameSpan.start, FrameSpan.end)` once in `finish`.

Change [build_function_body_skeleton:880](<REPO>/crates/verter_semantic/src/analysis/flow/mod.rs:880) to call `visit_statement_list(source.statements)` once instead of its current manual loop. Store that returned draft as `body_completion`.

For an expression-bodied arrow:

- runtime and inference are exactly `{Return}`;
- membership is `implicit_value`;
- endpoint undefined is `DoesNotContribute`.

At [finish:1651](<REPO>/crates/verter_semantic/src/analysis/flow/mod.rs:1651), move the two new vectors into `Arc` slices and populate `FunctionBodySkeleton::completion`.

This must remain one AST statement walk. The reverse sibling-suffix fold is over owned drafts and is linear; it is not an AST rewalk or retained control graph.

## 4. Exact transformation rules

Use these operations throughout:

```text
seq(A, B) =
    (A without Normal)
    ∪ (B if A contains Normal else ∅)

branch(A, B) = A ∪ B

route_break(A, t) =
    (A without Break(t))
    ∪ ({Normal} if A contains Break(t) else ∅)

route_continue is identical with Continue(t)
```

For `CompletionFact`, apply these fail-closed rules:

- union with `Unknown` is `Unknown`;
- sequencing after an exact set without `Normal` returns the first exact set and does not inspect the unreachable suffix;
- otherwise any required `Unknown` operand produces `Unknown`;
- never retain a partial exact set beside an unknown bit.

Construct rules:

1. Simple statements

   - empty, debugger, expression and declarations: runtime/inference `{Normal}`;
   - `return expr`: `{Return}`, membership `explicit_value`;
   - bare `return`: `{Return}`, membership `explicit_bare`;
   - `throw`: `{Throw}`;
   - unresolved/malformed break or continue: the corresponding typed unknown;
   - `with`: `Unknown(UnsupportedWithStatement)`.

   Expression evaluation is not classified as throwing. Treating calls, member access, or initializers as `Throw` would absorb D5/D6 work and is forbidden.

2. Block

   Compose its body with `seq`, starting from `{Normal}`. Empty block is `{Normal}`.

3. `if`

   - consequent completion union alternate completion;
   - absent alternate is `{Normal}`;
   - runtime completion is the structural may-completion union;
   - if the arms disagree on whether they contain `Normal`, set only the draft’s `inference` to `Unknown(ConditionalFlowRequired)`;
   - if both agree, inference uses the same union.

   This prevents a literal/type-sensitive condition from being mistaken for final exact flow knowledge.

4. Labels

   - allocate one `CompletionTargetId` and `CompletionTargetKind::Label` before entering the body;
   - resolve labeled `break` against the innermost matching active label;
   - route only `Break(this_target)` to `Normal` on exit;
   - other break targets pass through unchanged;
   - a labeled `continue` resolves to the associated loop target only when the labeled body is an iteration statement, including a chain of labels; otherwise it is `UnresolvedContinueTarget`.

5. `switch`

   - allocate/push one switch target;
   - calculate each case’s statement-list completion;
   - fold case suffixes from last case to first to model fallthrough;
   - union every possible case-entry suffix;
   - add `{Normal}` when there is no `default`;
   - route `Break(switch_target)` to `Normal`;
   - never consume `Continue`.

   Endpoint inference is exact only when all structural entries agree on normal completion. Otherwise use `ConditionalFlowRequired`; discriminant typing belongs later.

6. Loops

   Traverse the initializer, test, update and body for all existing skeleton facts and nested statement facts, but the loop statement’s completion must be:

```rust
CompletionFact::Unknown(CompletionUnknown::LoopFlowRequired)
```

   Apply this to `while`, `do`, all `for` forms, `for-in`, and `for-of`. Do not special-case `true`, `false`, a syntactic `break`, or a one-iteration-looking body. Loop entry, back-edge, fixed point and exit routing belong to D6/`U6.LOOP_CLOSURE`.

7. `try` and `catch`

For exact try completion `T` and catch completion `C`:

```text
caught(T, C) =
    (T without Throw)
    ∪ (C if T contains Throw else ∅)
```

No catch means `T`. Apply the same transformation to runtime and inference drafts.

Catch only the explicit `Throw` completion kind. Do not infer possible throws from expressions or calls.

8. `finally`

For pre-final completion `P` and finalizer completion `F`:

```text
finally_runtime(P, F) =
    (F without Normal)
    ∪ (P if F contains Normal else ∅)
```

For the endpoint-inference draft:

- if `F` contains no `Throw`, `Break`, or `Continue`, apply the same formula and additionally retain `P.only_breaks()` whenever `F` contains `Return`;
- this is the pinned X68/X80 behavior;
- if `F` contains `Throw`, `Break`, `Continue`, or either operand is unknown, use `Unknown(FinallyInferenceFlowRequired)` unless the result is made unreachable by a preceding exact non-Normal completion.

The published runtime completion remains exact where the standard `finally_runtime` formula is exact. Only the endpoint inference fact becomes unknown.

9. Endpoint `undefined`

After building the body:

```text
no authored return
    => DoesNotContribute

inference Unknown(reason)
    => Unknown(reason)

inference Exact(set) containing Normal
    => Contributes

inference Exact(set) without Normal
    => DoesNotContribute
```

Bare `return;` is an authored undefined contributor, but it is not an endpoint-undefined contribution. Keep those concepts separate.

## 5. A3-facing contract

A3 must consume only:

```rust
FunctionBodySkeleton::endpoint_undefined_disposition()
```

A3’s eventual logic is mechanical:

```rust
match skeleton.endpoint_undefined_disposition() {
    EndpointUndefinedFact::Exact(expected)
        if expected == observed_endpoint_undefined =>
    {
        // No A2C-detected contradiction.
    }
    EndpointUndefinedFact::Exact(_) | EndpointUndefinedFact::Unknown(_) => {
        // FlowGap::AbruptCompletion; no warm admission.
    }
}
```

`observed_endpoint_undefined` is the contribution the existing lowering is about to publish. A3 must not inspect statement syntax or recreate any completion rule.

Required explicit values:

- X05 `try { throw } catch { return c } ; return d`:

  - body runtime: `{Return}`;
  - endpoint: `Exact(DoesNotContribute)`;
  - current observation: `DoesNotContribute`;
  - result: clean, not a G10 hazard.

- Genuine G10 labeled suffix:

```ts
function makeProps() {
  L: try { break L } finally { return "a" as const }
  R: { return "b" as const }
}
```

  - body runtime: `{Return}`;
  - endpoint: `Exact(DoesNotContribute)`;
  - legacy suffix observation: `Contributes`;
  - result: exact mismatch, therefore A3 retracts it.

- G10 try suffix:

```ts
function makeProps() {
  L: try { break L } finally { return "a" as const }
  try { return "b" as const } finally {}
}
```

  Expected endpoint: `Exact(DoesNotContribute)`.

- G10 throw suffix:

```ts
function makeProps() {
  L: try { break L } finally { return "a" as const }
  throw 0
}
```

  Expected endpoint: `Exact(DoesNotContribute)`.

- X68 and X80:

  - runtime completion `{Return}`;
  - inference completion reaches `Normal` through the preserved pending break;
  - endpoint `Exact(Contributes)`;
  - current observation `Contributes`;
  - clean.

- X88:

  - the preserved break is routed through the target label into the outer `return "b"` suffix;
  - body runtime `{Return}`;
  - endpoint `Exact(DoesNotContribute)`;
  - current observation `DoesNotContribute`;
  - clean.

## 6. Non-interference and cache behavior

A2C must make no production edit to:

- [flow_slice_content.rs](<REPO>/crates/verter_session/src/flow_slice_content.rs)
- [flow_graph.rs](<REPO>/crates/verter_semantic/src/analysis/flow/flow_graph.rs)
- [lower.rs](<REPO>/crates/verter_semantic/src/analysis/flow/lower.rs)
- [hashing.rs](<REPO>/crates/verter_semantic/src/analysis/flow/hashing.rs)
- cache admission or `FlowReturnDegradation`.

That makes interference structurally impossible in A2C: the facts are written into the skeleton, retained, and read only by fact tests. No graph edge, slice plan, slice hash, lowered body, evaluator, result, or cache-admission branch reads them.

The new facts do not enter any cache key:

- the flow bundle remains keyed by the existing exact function identity and hashes at [flow_slice_node.rs:79](<REPO>/crates/verter_session/src/cache_runtime/flow_slice_node.rs:79);
- the slice hash remains unchanged at [hashing.rs:98](<REPO>/crates/verter_semantic/src/analysis/flow/hashing.rs:98);
- `FunctionBodySkeleton` is in-memory and not persisted.

Therefore existing key semantics and warm entries are unchanged. A process running the new binary constructs facts when its bundle is first built; an old process cannot contain a new-code bundle. No schema migration or cache epoch change is required.

## 7. Performance and retained size

The required retained payload increase is:

```text
64-bit:
  +72 bytes inline per FunctionBodySkeleton
  +48 × statement-fact count
  +20 × completion-target count
  +two Arc slice allocation headers/allocator rounding

32-bit:
  +56 bytes inline per FunctionBodySkeleton
  +48 × statement-fact count
  +20 × completion-target count
  +two Arc slice allocation headers/allocator rounding
```

`target_count <= 64`; overflow becomes typed unknown. `statement_count` is linear in the authored function body. `CompletionSet` performs no allocation.

Add test-only work accounting to `SkeletonBuilder`:

```rust
#[cfg(test)]
completion_work_units: usize,
#[cfg(test)]
completion_statement_count: usize,
#[cfg(test)]
completion_case_count: usize,
```

Increment once per constant-time set operation. The required bound is:

```text
completion_work_units
<= 16 * completion_statement_count
 + 8 * completion_case_count
 + 4 * completion_target_count
 + 1
```

Test this at 256, 512 and 1,024 generated statements. No timing assertion substitutes for this structural work bound.

Add [completion_facts.rs](<REPO>/crates/verter_semantic/benches/completion_facts.rs) and run:

```text
cargo bench -p verter_semantic --bench completion_facts
```

Measure:

- skeleton construction latency;
- allocation count and requested bytes;
- retained fact bytes;
- flat sequential body;
- nested block/if/label/try body;
- switch-heavy body;
- 64-target boundary and 65-target typed-unknown case.

Capture the pre-change baseline before implementing. Apply the Revision 11 default no-regression gate: upper slowdown bound `max(3%, 2 × measured noise floor)`. Query-time measurement must show zero AST walks and zero fact allocations: endpoint access is one field read.

## 8. Required tests

Add [completion_tests.rs](<REPO>/crates/verter_semantic/src/analysis/flow/completion_tests.rs):

1. `completion_set_is_canonical_and_allocation_free`
2. `completion_sequence_replaces_only_normal`
3. `completion_if_unions_arms_and_missing_else_normal`
4. `completion_label_routes_only_matching_break`
5. `completion_switch_routes_own_break_and_preserves_foreign_break`
6. `completion_try_routes_throw_into_catch`
7. `completion_finally_abrupt_replaces_runtime_completion`
8. `completion_unknown_is_disjoint_and_propagates`
9. `completion_target_65_is_typed_unknown_not_truncated`
10. `completion_fact_layout_is_locked`

Add [completion_skeleton_tests.rs](<REPO>/crates/verter_semantic/src/analysis/flow/completion_skeleton_tests.rs):

1. `a2c_g10_labeled_suffix_is_exact_endpoint_absent`
2. `a2c_g10_try_suffix_is_exact_endpoint_absent`
3. `a2c_g10_throw_suffix_is_exact_endpoint_absent`
4. `a2c_switch_terminal_suffix_is_exact_endpoint_absent`
5. `a2c_catch_terminal_suffix_is_exact_endpoint_absent`
6. `a2c_x05_catch_return_is_exact_and_not_a_hazard`
7. `a2c_x68_endpoint_undefined_is_exact_present`
8. `a2c_x80_endpoint_undefined_is_exact_present`
9. `a2c_x88_outer_suffix_makes_endpoint_undefined_exact_absent`
10. `a2c_loop_completion_is_typed_unknown`
11. `a2c_with_statement_completion_is_typed_unknown`
12. `a2c_completion_facts_are_no_type_expr_send_sync_static`
13. `a2c_completion_spans_are_frame_relative`
14. `a2c_completion_construction_is_deterministic_and_linear`

Add [u6_flow_a2c_non_interference_tests.rs](<REPO>/crates/verter_session/src/u6_flow_a2c_non_interference_tests.rs). Each test must assert both the new fact and the unchanged public cold/warm result, making it red before A2C while simultaneously proving non-interference:

1. `a2c_x05_fact_discriminates_while_public_result_stays_clean_warm`
2. `a2c_x68_fact_is_present_while_public_result_stays_exact_clean_warm`
3. `a2c_x80_fact_is_present_while_public_result_stays_exact_clean_warm`
4. `a2c_x88_fact_is_absent_while_public_result_stays_exact_clean_warm`

For all four, assert:

- first call cold;
- second call warm;
- exact JSON;
- `degradation == None`;
- `candidates == 1`;
- identical first/second JSON.

This satisfies “red before, green after”: before A2C the fact assertion/type is absent; after A2C the fact assertion passes, while the public assertions prove unchanged behavior.

Mutation recipes:

- In `route_break`, stop converting the matching target to `Normal`.  
  Must fail `completion_label_routes_only_matching_break` and `a2c_x68_endpoint_undefined_is_exact_present`.

- In catch composition, leave `Throw` in the try set or omit the catch set.  
  Must fail `completion_try_routes_throw_into_catch`. This exact-set assertion is necessary because X05’s endpoint alone could remain absent in the wrong direction.

- In `finally_runtime`, retain `P` even when `F` lacks `Normal`.  
  Must fail `completion_finally_abrupt_replaces_runtime_completion`.

Each mutation must be run independently and recorded in A2C evidence.

Verification commands:

```text
cargo test -p verter_semantic completion
cargo test -p verter_session a2c_
cargo nextest run --workspace
cargo test -p verter_session --tests
cargo bench -p verter_semantic --bench completion_facts
```

## 9. Scope and risk findings

1. **D5 absorption risk:** inspecting calls, closure bodies, captures, writes, freshness or escape behavior while deciding `Throw`/`Normal`. Exact change: forbid all expression-effect inference in `completion.rs`.

2. **D6 absorption risk:** emitting graph edges, routing slot state, solving case feasibility, evaluating conditions, or iterating loops. Exact change: loops and condition-dependent endpoint conclusions use typed unknown.

3. **D8 absorption risk:** using completion exactness to construct `CompleteFlowReturn`, alter degradation, admit candidates, or close obligations. Exact change: no session production consumer in A2C.

4. **`U6.LOOP_CLOSURE` absorption risk:** treating a syntactic loop as zero/one iteration, consuming its break/continue, or modeling `try`/`finally` state edges. Exact change: loop wrapper is always `LoopFlowRequired`.

5. **Wrong-exact label direction:** resolving an unknown label to the nearest label or outer breakable target. Required behavior: `UnresolvedBreakTarget`/`UnresolvedContinueTarget`.

6. **Wrong-exact capacity direction:** dropping the 65th target or aliasing bit 64 to bit 0. Required behavior: `TargetCapacityExceeded`.

7. **Wrong-exact switch direction:** adding normal completion despite a default with every entry terminal, or omitting the no-match path without a default. Required tests cover both directions.

8. **Wrong-exact catch direction:** treating every expression as possibly throwing or failing to route explicit `Throw`. Only explicit `throw` participates.

9. **Wrong-exact finally direction:** using only runtime override and thereby losing X68/X80’s inference-preserved break, or preserving that break without routing it through X88’s suffix.

10. **Unknown laundering:** implementing `unwrap_or(Normal)`, `unwrap_or(DoesNotContribute)`, `Default`, or an accessor that returns a boolean for unknown. This is a release blocker.

11. **Coordinate risk:** storing absolute statement or target spans. All new span fields must remain `FrameSpan`; the existing position-invariance test must include the new completion field through whole-skeleton equality.

12. **Cache churn risk:** folding completion facts into `flow_body_stable_hash`, `FlowSliceHash`, graph identity, or admission before they affect semantics. Do not change any key in A2C.

13. **Temporary duplicate-authority risk:** the legacy evaluator and `statement_guarantees_current_function_return` remain until A3/D6. A2C must not extend them. A3 consumes only the endpoint accessor; D6 later consumes this algebra and deletes the legacy completion machinery.

14. **Strongest counter-argument:** retaining per-statement facts adds material memory to every demanded skeleton even though A3 initially needs only the function endpoint. The charter expressly requires reusable statement/suffix facts for D6 and forbids a later second classifier, so storing them now is justified. The 48-byte record, two retained allocations, 64-target cap and explicit unknown overflow are the minimum durable shape that meets that contract without variable per-set allocation.

After implementation, record exact candidate SHA, tests, mutation results, layout measurements, benchmark raw data and unchanged public traces in `docs/arch/refactor/rev11/evidence/A2C-summary.md`. Do not edit the charter to claim acceptance; the maintainer owns that transition.

__DONE__
