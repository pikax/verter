# B3 Cache Runtime — Carry-Forward Items

Items the B3 review rounds identified as P3 nits worth tracking
but not in scope for the round under which they surfaced. Each is
documented here as an explicit follow-up so a future change owner
can close the loop without rediscovering the issue.

  * **CF-1 / CF-2 / CF-3** — surfaced in round 3 (synthetic vs
    production-routed behavioral test, structured refusal sink,
    scheduler signal asymmetry).
  * **CF-4 / CF-5 / CF-6 / CF-7** — surfaced in round 4 (regex
    comment-stripper limits, module-local lint scope, parameter
    naming, AST walker theoretical gaps).

## CF-1: Production-producer-routed behavioral test for typed refusal reasons

**Status**: Synthetic test in place; production-routed test deferred.

The `cache_runtime_lowering_carries_armed_typed_reason` test
(`crates/verter_session/src/cache_runtime/node_tests.rs`) drives
every `NonAdmissionReason` variant through a synthetic producer
(`let _reason_guard = SetReasonGuard::arm(reason);` followed by
`return ComputeAdmission::ReturnOnly(())`), lowers it through the
exact pattern the three production lowering sites use, and asserts
the typed reason carried into `CacheAdmission::ReturnOnly { reason,
.. }` matches.

The test uses the REAL bridge primitives (`SetReasonGuard::arm`,
`consume_return_only_reason_for_lowering`), so it catches the
primary regression modes — an unmigrated callsite, a defective
`drop` on the guard, an off-by-one in the slot semantics — and is
the load-bearing discriminator for the round-2 TLS bridge.

A second layer of coverage routing through an actual production
producer (the materialise / ref-cycle / imported-registry refusal
paths) would catch a regression that touched producer-internal
state ordering without touching the bridge. The current code wires
the bridge via three different production paths (component-meta
materialise, ref-cycle BFS, imported-registry resolution) and each
is exercised end-to-end by other tests (`compile_cache_overflow_\
return_only`, `query_db_self_root_tests`); the synthetic-bridge
test plus the existing E2E paths together cover the regression
surface, so a dedicated "behavioral test through real producer +
lowering + structured refusal sink" is a follow-up rather than a
load-bearing gate.

When the structured refusal sink (CF-2 below) lands, the natural
shape of CF-1 becomes "production producer X observes refusal
counter Y advancing on every refusal arm" — i.e. CF-1 and CF-2
collapse into one end-to-end discriminator.

**Where to file when ready**: a new test under
`crates/verter_session/src/cache_runtime/` or
`crates/verter_session/tests/`, named after the producer-route
under test.

## CF-2: `CacheAdmission::ReturnOnly { reason }` dead-ends at the singleflight lookup adapter

**Status**: Pre-existing from B3 round 1. The reason flows TO the
`CacheAdmission` site but no observability hook reads it today.

`CacheAdmission::ReturnOnly { value, reason }`
(`crates/verter_session/src/cache_runtime/admission.rs`) is
constructed at three call sites by `consume_return_only_reason_\
for_lowering()`. The three lowering adapters then map the
`CacheAdmission` shape back to the substrate `ComputeAdmission`
shape via the lookup wrapper at `crates/verter_session/src/cache_\
runtime/node.rs:221, :431`:

```rust
CacheAdmission::ReturnOnly { value, .. } =>
    ComputeAdmission::ReturnOnly(value),
```

The `reason` field is dropped on the floor at this step. No
observability hook (no `AuditObserver::record_event`, no
`StructuredAuditEvent` emission, no metric counter, no
log statement) reads the reason between its construction and its
discard. The typed-refusal infrastructure (`SetReasonGuard`, the
TLS bridge, the `consume_*_for_lowering` debug-assert) all exist
in advance of a downstream consumer.

The "structured refusal telemetry" the bridge claims to enable is
**aspirational** until a consumer reads `reason` and emits it
through one of:

  * `verter_audit::AuditEvent` — the per-event counter hook the
    crate exposes for cooperatively-counted observability.
  * `verter_audit::StructuredAuditEvent` — the discriminated-union
    structured event surface.
  * A per-host metrics counter on `VerterHost` (e.g. a
    `RefusalsByReason` map keyed on `NonAdmissionReason`).

**Where to file when ready**: extend `verter_audit::AuditEvent`
with a `CacheAdmissionRefused { reason: NonAdmissionReason }`
variant (or similar), wire it at the three `consume_return_only_\
reason_for_lowering` call sites, and add a discriminating test
that observes the counter advancing on every refusal route.

**Skill-file update required**: `.claude/skills/type-cache-\
architecture/SKILL.md` should be updated when CF-2 lands to flip
"the typed reason flows to a structured refusal event" from
aspirational to actual.

## CF-3: `Scheduler::remove_artifact_if_not_newer_than` does NOT signal pending Artifact requests

**Status**: Pre-existing from B3 round 1 (carrier was originally
`remove_artifact`). Round 3 renamed and gated on generation but did
not address the asymmetry with `commit_artifact`.

`Scheduler::commit_artifact(file_id, profile_hash, snapshot)` in
`crates/verter_scheduler/src/scheduler.rs` performs two things on a
successful publish:

  1. `node.artifacts.insert(profile_hash, snap)` — stores the
     snapshot for warm `try_get_artifact` reads.
  2. `node.pending_requests.signal_stage_complete(generation,
     &TaskKind::Artifact { profile_hash }, &RequestResult::Artifact(snap))`
     — wakes any pending `Artifact` request handles that were
     blocked on the same `(file_id, profile_hash)`.

`Scheduler::remove_artifact_if_not_newer_than(file_id, profile_\
hash, max_generation)` only does step (1) — `node.artifacts.\
remove_if(...)`. It does NOT signal pending requests. A pending
Artifact request that was waiting on `(file_id, profile_hash)`
when the refused compile reached its eviction arm would continue
to wait indefinitely, even though the producer that would have
satisfied it just declined.

In practice this asymmetry is dormant because:

  * The only production caller of `remove_artifact_if_not_newer_\
    than` is the `virtual_file_pipeline`'s compile-refusal arm.
  * That arm runs ON THE SAME THREAD as the host's `get_virtual_\
    file` request — no separate Artifact request is parked on the
    same key during the refusal.
  * The host's compile pipeline does not submit a separate
    `TargetStage::Artifact` request that could race; it threads
    the result directly through the synchronous
    `compile_entry()` path.

But the asymmetry IS a latent gap. A future caller of `remove_\
artifact_if_not_newer_than` from a context with a concurrent
pending Artifact request (e.g. a multi-threaded MCP batch that
submits Artifact requests in parallel with compile-refusal
arms) would deadlock.

**Where to file when ready**: add `signal_failed_for_stage(file_\
id, generation, &TaskKind::Artifact { profile_hash })` (or
`signal_stage_failed_for(...)`) to `remove_artifact_if_not_newer_\
than` so a pending request observes a deterministic failure
result instead of blocking indefinitely. Pair with a scheduler
unit test that submits an `Artifact` request and then calls
`remove_artifact_if_not_newer_than`, asserting the handle wakes
with a failure status.

## CF-4: `strip_comments` does not track nested block comments or raw strings

**Status**: Pre-existing in the regex-based route-generation arch
guard. No current producer body triggers the gap.

The string-stripping pass in
`crates/verter_session/tests/route_generation_admission_guard.rs`
(`strip_comments`) handles `//` line comments and a single layer of
`/* ... */` block comments, but does NOT:

  * Track nesting for `/* /* */ */` — Rust permits nested block
    comments; the stripper terminates on the first inner `*/` and
    leaves the trailing block visible to the regex.
  * Recognise raw-string literals (`r"..."` / `r#"..."#`) — any
    `/*` or `//` inside a raw string is preserved by `rustc` but
    would be stripped by the current pass; conversely, a
    `RouteGenerationDependency::Resolved {` token inside a raw
    string would currently match the regex even though the token
    is data, not code.

In practice the gap is dormant because no production
`RouteGenerationDependency::*` constructor body contains nested
block comments or raw-string literals — the regex pattern set
inspects normal Rust expression syntax in cooperative-admission
arms.

**Where to file when ready**: switch the route-generation arch
guard from regex over `strip_comments(src)` to a `syn`-based pass
that tokenises with the real lexer (`syn::parse_file` →
`syn::visit::Visit` over `ExprCall` / `ExprStruct`), or implement
nesting-depth tracking + raw-string skipping in `strip_comments`
itself.

## CF-5: Module-local `#![deny(clippy::let_underscore_must_use)]` on `admission.rs`

**Status**: Implementer-applied module-scoped lint. Documented
trade-off — the broader sweep is out of round-4 scope.

The directive at the top of
`crates/verter_session/src/cache_runtime/admission.rs` (`#![deny(\
clippy::let_underscore_must_use)]`) catches `let _ = SetReason\
Guard::arm(...)` patterns INSIDE that module. The intent is to
prevent a future producer from accidentally arming a refusal-reason
guard and then immediately dropping it (the bug shape that the
round-2 RAII guard was introduced to prevent — a guard that
unwinds before the `ReturnOnly` path runs leaves the TLS slot
empty and the lowering site reads `None`).

The scope gap: a producer in a DIFFERENT module that writes
`let _ = SetReasonGuard::arm(reason);` is not caught by the
module-local deny. The implementer's documented trade-off is that
turning the deny on crate-wide would cascade into 28 unrelated
`let _ = <Result>` patterns across the crate that are out of
round-4 scope. The primary defense remains the `SetReasonGuard`
module-level docstring which explicitly warns against the
let-underscore pattern.

**Where to file when ready**: as each producer module migrates its
pre-existing `let _ = <Result>` patterns to either explicit
`drop(...)` calls or named bindings, add the same
`#![deny(clippy::let_underscore_must_use)]` directive at the top
of that module. Once every producer module is covered, promote the
deny to a crate-level lint group in `lib.rs`.

## CF-6: `Scheduler::remove_artifact_if_not_newer_than` parameter naming

**Status**: Round-3 introduced the parameter as `max_generation`.
Pure naming nit — the function behaves correctly.

The signature reads `remove_artifact_if_not_newer_than(file_id,
profile_hash, max_generation: WorkspaceGeneration)`. The doc
comment clarifies that `max_generation` is "the compile-start
generation captured before the producer ran". Callers must pass
the SAME generation the producer originally observed, so a
post-producer compile-tier bump (which advances the workspace
generation) leaves the stored artifact alone.

The naming nit: `max_generation` reads more naturally as "the
highest generation we are willing to evict", which describes the
behavioral effect (any artifact whose stored generation is `<=
max_generation` is evicted). A self-documenting alternative like
`compile_start_generation` or `evict_if_stored_at_most` would tie
the parameter name to its semantic role rather than the predicate
shape.

**Where to file when ready**: rename the parameter at
`crates/verter_scheduler/src/scheduler.rs` and update the three
callsites accordingly. Pure refactor — no behavior change.

## CF-7: AST walker theoretical gaps beyond the round-4 fix

**Status**: Round-4 closed the inferred-call shape (`Arc::from(\
<arg>)` / `Arc::from_iter(<arg>)` where the argument carries the
`FactVersionRef` anchor). Other theoretical bypass shapes remain
out of scope because no current production code uses them.

Known remaining theoretical bypass surfaces:

  * **`static`/`const` initialisers**: `static EMPTY: Arc<[Fact\
    VersionRef]> = Arc::from(...);` or
    `const EMPTY: Arc<[FactVersionRef]> = ...` at item scope. The
    walker visits `ItemFn` and `ImplItemFn` but not `ItemStatic` /
    `ItemConst`.
  * **Struct-field initialisation at construction sites**: `Foo {
    facts: Arc::from(Vec::<FactVersionRef>::new()), .. }`. The
    field-of-type `Arc<[FactVersionRef]>` anchor lives on the
    struct definition, not the construction expression — the
    walker would need to maintain a per-type field-type table
    keyed on `(struct_name, field_name) -> type`.
  * **Closure bodies**: `Lazy::new(|| Arc::from([]))` returning the
    empty rail through a closure. The walker visits `ExprCall` so
    the inner `Arc::from(...)` is reached, but the BOUNDARY
    inference (typed-local / fn-return / arg-anchor) does not
    propagate into the closure tail.
  * **`Box::new`-wrapped empty + later `Box::into_arc`**: any
    multi-step wrapper sequence the walker does not recognise as
    an "empty constructor inside `Arc::from`".

None of these patterns appear in current production code under
`crates/verter_session/src/`. The round-4 walker is structurally
type-anchored and covers the boundary shapes producers actually
write today; the gaps above are pre-emptive theoretical concerns
codex flagged for the long-term arch guard.

**Where to file when ready**: extend `BypassWalker` with
`visit_item_static` / `visit_item_const`, add a `visit_field`
arm with a per-type field-type lookup, and propagate
boundary-inference through closure bodies — but only when actual
production code begins to exercise those shapes.

## CF-8 — AST walker false-positive surface on `Arc<Vec<FactVersionRef>>`

The `check_arg_anchored_arc_call` boundary in
`crates/verter_session/tests/finalise_signature_or_empty_is_gone.rs` flags
`Arc::from(<FactVersionRef-anchored empty>)` whenever the argument syntactically mentions
`FactVersionRef` AND classifies as empty. The walker does NOT check the surrounding result-type
boundary, so a benign construction like
`let x: Arc<Vec<FactVersionRef>> = Arc::from(Vec::<FactVersionRef>::new());` would be FALSELY
FLAGGED as a bypass even though `Arc<Vec<T>>` is a different `From` impl from the forbidden
`Arc<[FactVersionRef]>` signature rail.

**Status**: No current production code uses `Arc<Vec<FactVersionRef>>` (the shape is structurally
pointless — `Arc<Vec<T>>` allocates a heap Vec inside an Arc, no win over `Arc<[T]>` and worse for
sharing). The false-positive surface is theoretical.

**Fix (future)**: extend `check_arg_anchored_arc_call` to propagate the surrounding boundary's
expected-type (`Arc<[FactVersionRef]>` slice form) into the check. Only flag when the boundary IS
the slice rail. Add a benign-fixture for `Arc<Vec<FactVersionRef>>` to the no-false-positive test
set.

## CF-9 — `host_resolve/virtual_file_pipeline.rs` extraction

The compile-tier producer file grew from 1407 to 1558 lines during the B3 cache-runtime overhaul.
The B3-added content includes:

  * The SetReasonGuard arming on the cold-build `NonCacheable` path.
  * The scheduler-eviction guard (`remove_artifact_if_not_newer_than`) on the same arm.
  * The compile-start-generation threading from `sched_snapshot_at_start` into the eviction call.
  * The cache-runtime substrate hookups (test-only `CompileForceOverflowGuard` + atomic +
    injection block).

The file is currently EXEMPT in `guard6_exemptions()` at
`crates/verter_session/tests/architecture_guards.rs`. Extraction is queued for a follow-up block.

**Suggested extraction boundary**: move the SetReasonGuard arming + scheduler-eviction plumbing
+ compile-start-generation snapshot capture into a new helper module
`host_resolve/virtual_file_pipeline_admission.rs` (or similar name). The compile-tier core
remains in `virtual_file_pipeline.rs`. After extraction, remove the exemption.

**Fix (future)**: extract the cache-admission lifecycle from the compile pipeline into a
dedicated module, then remove the `guard6_exemptions()` entry.

## Cross-references

The round-3 through round-5 fix-implementer briefs and the per-round
dual-review outputs are recorded in the orchestration ledger (brief and
review artifacts).
