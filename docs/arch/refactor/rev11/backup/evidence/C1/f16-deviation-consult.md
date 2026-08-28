# C1 sixteenth deviation — F16: designing `AttemptOutput` now that it's a real phase-7 prerequisite

Found while continuing straight from F15's landing (`path_probe`/`real_path`/
`package_manifest`, commit `339386fed`). F15's own consult flagged that
"consumed observation keys/evidence need to be attempt output — or use the
shared Part F attempt-output carrier. Recording every prefetched fact as
consumed is not acceptable" — elevating Part F's previously-deferred
"attempt output" bundle design (`disposition-table.md` Part F, round
5: "NOT yet designed as concrete Rust... the concrete next design task once
`NeedInputs`-side work exhausts what's independently buildable") from a
nice-to-have for three unrelated methods into an actual phase-7 correctness
prerequisite. With 13 `ResolverObservation` methods now landed, I judged
the "NeedInputs-side work exhausts what's independently buildable"
threshold met and consulted before designing. Full consult prompt/output:
`/tmp/c1-part-f-attempt-output-prompt.md` / `/tmp/c1-part-f-attempt-output-output.md`
(not committed — ephemeral scratch; this file is the durable record).

## Consult verdict: ADOPT-NOW, inert top-level-owned accumulator only

**Designing it now is correct.** The earlier "blocked on the top-level
entry point" conclusion (my own round-6 reasoning) was too broad — the
entry point is still required to decide the FINAL completion envelope and
application wiring, but NOT required to establish attempt-scoped
ownership, accumulate-during-execution semantics, discard-on-`NeedInputs`/
`Terminal`, apply-only-after-`Complete`, or the three already-concrete
output categories. Correct split: design and land the accumulator now;
defer its top-level RETURN WIRING.

**`AttemptOutcome::Complete(T)` stays UNCHANGED.** My own suspicion
confirmed: changing it to `Complete { value: T, output: AttemptOutput }`
would incorrectly attach outbound kernel effects to every inbound
observation response — all 13 landed methods are inbound queries using
that protocol as-is. The eventual top-level shape reuses the EXISTING
generic payload without touching the enum:

```rust
pub struct CompletedAttempt<T> {
    pub value: T,
    pub output: AttemptOutput,
}
pub type KernelAttempt<T> = AttemptOutcome<CompletedAttempt<T>>;
```

`AttemptOutput` is therefore top-level-owned and top-level-published,
though deep kernel functions may contribute to it (threading `&mut
AttemptOutput`, or a future `&mut AttemptContext` containing it, through
the relocated kernel call graph once that call graph exists). A
`NeedInputs`/`Terminal` result discards that attempt's accumulator; only
`Complete` transfers it to the driver. NOT built this round (needs the
top-level entry point).

## Landed this round: the bare inert accumulator

`crates/verter_semantic/src/resolver_core/attempt_output.rs`:

```rust
pub struct AttemptOutput {              // private fields, no public
    observed_facts: Vec<FactVersionRef>,        // struct literal — every
    ambient_dependencies: Vec<AmbientDependency>,  // future field (e.g. a
    consumed_resolution_observations:               // ShapeCacheAdmission-
        Vec<ConsumedResolutionObservationKey>,      // Candidate) is additive
}
pub struct AmbientDependency { pub consumer_canonical: CanonicalId, pub virtual_id: CanonicalId }
pub enum ConsumedResolutionObservationKey { PathProbe { path }, RealPath { path }, PackageManifest { directory } }
```

Design notes, per the consult's explicit corrections to my own draft:

- **NOT a bare `Vec<InputKey>`** for consumed observations — `InputKey`
  means "independently loadable missing input" and carries unrelated
  variants (`FileContent`/`DeclBody`/`ModuleAugmentationIndex`/
  `FlowFunctionSkeleton`) that have no place in a consumed-observation
  witness. A dedicated `ConsumedResolutionObservationKey` (exactly the 3
  F15 variants) prevents invalid states.
- **A consumed key is only a SELECTOR, not itself an authoritative version
  witness** — `FactVersionRef` carries the actual versioned
  cache-validation currency; the session/workspace side translates or
  enriches a consumed key using that attempt's own observation snapshot
  before replaying it into the fact tracer. `AttemptOutput` accumulates
  raw facts only — it owns NO signature deduplication, caps, or overflow
  policy; those remain with the existing fact-read tracer/finalization
  rail (`verter_workspace::fact_read_set`).
- **`ShapeCacheAdmissionCandidate` does NOT block this landing** — do NOT
  add a generic parameter, erased placeholder, or opaque slot for it
  (would prematurely spread cache-admission policy through every attempt
  type, and its cardinality isn't settled — a shared attempt may
  eventually emit more than one candidate, so freezing `Option<...>` now
  could be wrong). Simply OMIT the field until F11's cache-admission DTO
  gets its own concrete design ruling; private fields make adding it later
  straightforward.
- Private fields + `Default` + `new()` + per-category `record_*` methods +
  read accessors + a `merge()` operation (for composing a parent attempt's
  output from sub-attempts, e.g. the recursive project-reference walk's
  per-node outputs) + `is_empty()`.

## Explicit instruction, followed

"Safe immediate scope, with no further ruling required: add inert
`AttemptOutput`; add named ambient-dependency and consumed-resolution-key
types; add accumulator/accessor/merge unit tests; keep all fields private;
make ZERO changes to `AttemptOutcome`, `ResolverObservation`, or
production call sites; do NOT add `CompletedAttempt<T>` wiring yet; do NOT
add a shape-cache placeholder." All followed exactly — 7 new unit tests
(fresh-is-empty, `Default`-matches-`new`, per-category record/read
round-trips, merge preserves both sides' contributions in order, merging
two empties stays empty), zero `AttemptOutcome`/`ResolverObservation`/
production changes.

## What remains for this bucket

1. `CompletedAttempt<T>`/`KernelAttempt<T>` wiring — needs the top-level
   kernel entry point (`project_semantic_dispatch`'s eventual relocation).
2. `ShapeCacheAdmissionCandidate`'s own concrete DTO design (F11) — a
   separate ruling, not attempted here.
3. Actually threading `&mut AttemptOutput` through kernel call chains —
   needs the algorithm conversion work itself (F15's own still-deferred
   scope), not this bucket's own design.
