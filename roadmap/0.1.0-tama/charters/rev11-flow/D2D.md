<!-- unified-charter-v2
id=D2D
name=Typed resolution outcome for every surface producer
phase=rev11
train=rev11.flow
product=rev11
kind=implementation
semantic_role=delivery
class=foundational
predecessors=D2A
owner=rev11.flow:sole shared flow authority
conflict_domains=semantic_authority
resource_class=rust-mixed
review_profile=semantic-3
gate_profile=targeted-domain
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
charter=charters/rev11-flow/D2D.md
max_production_loc=800
max_production_files=12
max_related_packages=2
rescope_loc=1500
rescope_files=20
rescope_unrelated_packages=3
-->

# D2D — Typed resolution outcome for every surface producer

Readiness comes only from trusted implementation-ledger rows. A READY node may start; tooling does not validate commit locators, Git identity, receipts, leases, external state, or runtime admission.

## Independently acceptable outcome

A producer that resolves a component surface can no longer report success while
handing back nothing. Every resolution-to-surface API returns either
`Resolved(surface)` or `Incomplete(reason)`, and the raw empty-success pair is
UNCONSTRUCTIBLE — a producer that cannot build a surface has no way to spell
"empty and fine" and must name why.

This is the structural close of a defect class that five successive rounds of
per-instance repair did not exhaust. Each instance is the same move: a
resolution step cannot produce a surface, returns an empty surface with a
success status, and the emptiness becomes indistinguishable from "nothing was
declared" — the wrong-complete outcome, a SUBSET, and in at least one case
warm-admitted. Four instances were fixed individually (the FFI completeness
marker, the sidecar-less lane, the dropped merged term at five publication call
sites, and unresolved imports); an audit then enumerated nine more. Nothing in
the type system required recording a reason at the drop, so the class kept
reappearing one producer over.

The architect ruling this node executes: "make every resolution-to-surface API
return either `Resolved(surface)` or `Incomplete(reason)`, with raw
empty-success unconstructible, then migrate all nine", ruled BLOCKING for D2B
re-certification because the warming chokepoint violates D2B-AC2/AC3 — and
"pre-existence changes blame, not acceptance".

## Concrete surfaces and APIs

- Production surfaces: `crates/verter_session/src/typeinfo`,
  `crates/verter_session/src/meta_resolve`, `crates/verter_session/src/framework`.
- Named boundaries: the resolution-outcome carrier itself (new),
  `TypeInfoSurface`, `MacroSurfaceDtos`, `ComponentMetaColdResult`,
  `ResultCompleteness`, `PublishedCompleteness`.
- Mutation boundary: only those surfaces plus the nine migration sites; sibling
  ownership is excluded.

## Exact predecessor contracts

- **D2A:** implemented ledger row. D2A owns the shared flow substrate these
  producers resolve through.
- **External requirements:** none.

## Source-specific scope

- Introduce the typed outcome and make empty-success unconstructible at the type
  level, not by convention. `PublishedCompleteness` is the existing precedent at
  the publication boundary: a module-private field with named constructors, so a
  call site must state its claim. Mirror that discipline at the producer
  boundary.
- Migrate all nine sites:
  `typeinfo/shallow_surface.rs` (the terminal-hop chokepoint, 7 call sites,
  currently publishes empty + `Complete` AND WARMS — this one is why the node
  blocks), `framework_surface/svelte_exec.rs` `resolve_runes_props` and two
  further sites, `structural_carrier_producer/macro_payload_substrate.rs` (the
  conditional emit merge that publishes one branch as the complete set),
  `meta_resolve/normalize_slots.rs` (two sites),
  `meta_resolve/slot_binding_graph.rs`, and `meta_resolve/projectors/mod.rs`.
- Absorb the named failure semantics of
  `macro_payload_substrate.rs::resolve_macro_payload_diagnostic_probe` into the
  mandatory typed outcome, then DELETE it. It has zero call sites: it is the
  written defence for exactly this class that was never wired. Do not wire it as
  a second dispatch.
- `TypeInfoSurface::members_complete` has one production reader while every Vue
  normalizer ignores it. Fold it into the typed outcome or delete it; a
  completeness signal nothing reads is the same defect one level up.
- Deletion discipline: the empty-success spelling is REMOVED, not deprecated. No
  compatibility shim, no dual path.

## Acceptance IDs and discriminating proof

Preflight evidence selection: preserve all four outcomes, then select the
smallest evidence set that actually discriminates. Existing behavioral coverage,
compiler/type/capability enforcement, static validation, canonical gates,
bounded inspection, and benchmarks are valid with a terse rationale.

- **D2D-AC1 — unconstructible by type, not by convention:** a compile-fail
  fixture proves a producer cannot return an empty surface with a success
  status. Prefer type/privacy enforcement (the forcing function) over a runtime
  assertion; a `trybuild` fixture with a pinned `.stderr` is the shape this repo
  already uses for sealed carriers.
- **D2D-AC2 — every migrated site names its reason:** for each of the nine, a
  discriminating test that the unresolvable input publishes a typed partial with
  the correct reason and the resolvable control publishes complete. The
  `shallow_surface` chokepoint additionally proves it no longer warms.
- **D2D-AC3 — no false partial:** a genuinely empty surface (a component that
  declares nothing, a slot set that is legitimately empty) still publishes
  complete, exact, and warm-capable. This is the discrimination the whole node
  exists for and the easy way to satisfy AC2 wrongly.
- **D2D-AC4 — bounded work:** the typed outcome adds no query, no cache, and no
  additional resolution pass; it carries a reason the producer already had.
  Prove zero additional cold computation using existing counters or record a
  terse not-applicable rationale.
- Every proposed new test must name a plausible regression not already
  discriminated; do not add implementation mirrors or duplicate permutations.
- Test homes: `crates/verter_session/src`, `crates/verter_session/tests/cases`,
  `crates/verter_ffi/src/convert/tests.rs`.

## Deletions and forbidden designs

- DELETE `resolve_macro_payload_diagnostic_probe` after absorbing its semantics.
- DELETE the empty-success spelling at every migrated site; no shim.
- Never let a producer report success it cannot evidence — that is the class
  this node closes, and re-introducing it anywhere is a correctness-budget
  violation, not a scope question.
- Never satisfy the contract by making everything partial: a legitimately empty
  surface stays complete, exact, and warm-capable.
- Never read completeness off the resolution sidecar — the sidecar-less lanes
  have none, and doing so publishes every degraded sidecar-less payload as
  complete.

## Budgets and mandatory rescope

- Target ceiling: 800 production LOC, 12 production files, 2 related packages.
- Mandatory rescope above 1,500 production LOC, 20 files, 3 unrelated packages,
  or when public/wire, unsafe, concurrency, or lifetime work is combined with
  another major concern.
- Correctness budget: zero stale publication, silent fallback, wrong-complete
  result, map/provenance loss, or identity aliasing.
- Performance budget: equivalent-work counters may increase by 0; wall,
  allocation and RSS regression allowance is 0.0% unless an owning-authority
  amendment supplies exact replacement thresholds.

## Abort conditions

- Stop before mutation if current source disproves the named owner/API boundary,
  an ancestor lacks an implemented ledger row, or the complete diff will not fit
  one review context.
- Stop if making empty-success unconstructible appears to require changing the
  published wire shape; that is an amendment, not a local decision.
- Abort the candidate on unexplained output, source-map, diagnostic,
  cancellation, allocation, latency, or RSS divergence.

## Targeted verification

1. `cargo nextest run -p verter_session -p verter_ffi`
2. Run every final command in the bound `targeted-domain` profile on the squashed
   review candidate; targeted success alone is iteration evidence, not
   acceptance.
3. Bind the preflight evidence selection and terse rationale in the review
   report. Behavioral code changes require TDD with a failing discriminating
   regression before production changes.

## Review and lower-severity findings

Apply `semantic-3`: 2 fresh distinct harness tasks covering exactly
`adversarial`, `conformance`. P0/P1 block final acceptance. A P2 follows the
owning review policy and must have a named owner when deferred; otherwise it
blocks. P3 follows the currently binding owning policy and must be recorded when
that policy requires it. Any post-review content change invalidates every
verdict. Final acceptance requires the complete 2/2 current-round profile to
contain independent clean PASS reports on the squashed review candidate, plus
`targeted` confirmation when required.

## Trusted implementation ledger

Before squashing or review, the implementation patch transitions this node's
predeclared row in `authority/state/implemented.toml` from `status = "pending"`
to `status = "implemented"` with the planned squash commit message, approximate
date with timezone, and optional pull-request number. The transitioned row is the
implementation fact. Commit metadata is a loose locator only and is never resolved or
validated against Git or GitHub. Reviewers inspect the squashed candidate patch without
SHA-, tree-, ancestry-, receipt-, lease-, or digest-bound orchestration manifests.

## Amendments

- **Outcome vocabulary (ratified shape).** The typed outcome carries FOUR
  arms, not two: `Resolved(surface)` and `Incomplete(non-empty reason)` as
  chartered, plus two additional COMPLETE claims the migration surfaced —
  `OpenPresence(surface)` (a positive presence-only projection of an OPEN
  member domain: every carried member is real, omission is not absence
  evidence; complete-as-a-result and warm-capable) and `NoSurface` (the
  proven complete negative answer: the demand resolved and there is no such
  surface). Both are SUCCESS claims and therefore carry the same opaque
  evidence as `Resolved` (below); neither is a failure spelling.
- **AC1 mechanism (architect-ruled).** Every success arm carries opaque
  evidence minted by exactly ONE private finalizer in the carrier's module:
  `Resolved` / `OpenPresence` hold their surface inside the proof-bearing
  `Witnessed<T>` wrapper and `NoSurface` holds a bare `SurfaceProof`; the
  proof's field is private, so no other module or crate can construct a
  success arm — the crate-internal mint surface is the named claim-stating
  constructors, which route through the finalizer. `Incomplete`'s reason is
  the type-level non-empty `NonEmptyReasons` newtype (no `Default`, no empty
  spelling, no runtime empty-set normalizer); its constructors are either
  type-level (one closed `PartialReason` maps to its guaranteed non-zero
  bit) or checked (`Option`-returning). Compile-fail fixtures pin the
  previously-legal shapes: raw `Resolved(TypeInfoSurface::empty())` from
  outside, bare `NoSurface` construction from outside, and a reason-free
  incomplete claim.
- **Completeness is enforced at the publication/warm-admission sinks
  (architect-ruled ADOPT-NOW).** Audits are supplemental, not the
  enumeration authority: the sinks demand sealed proof-bearing types, so a
  producer that never migrated cannot reach publication. Concretely: the
  emit EVENT-NAME enumeration returns the typed outcome (an unenumerable
  authored name set is an `Incomplete` claim, recorded at the metadata and
  runtime sinks; the TSC projection takes the named authored-fallback
  discharge because its authored `emit_fields` rows ride regardless); the
  payload-surface authorities discharge the carrier through `recorded()` at
  the token mint, so incomplete reasons ride the return value rather than
  bare side effects; the graph-native slot-binding synthesis returns a
  SEALED completeness claim (`SynthesisCompleteness`) minted only from its
  typed suppression ledger, and the `ComponentMetaResultDb` admission fold
  consumes that claim — a reverted boolean rail cannot feed the sink; and
  the shallow-surface chokepoint folds observed read partiality into EVERY
  returned arm (`with_read_partiality`), so no arm makes a reason-free
  complete/warm claim over a partial read.
- **Emits/slots value-failure asymmetry (deliberate, D2D-AC2 scope note).**
  A `defineSlots` member whose VALUE is unresolvable suppresses and never
  warms; a `defineEmits` OBJECT-VALUE member with the byte-identical shape
  publishes the member and stays warm-capable. This is a difference in
  MEMBERSHIP AUTHORITY, not a missed migration: slot membership is DERIVED
  from callable classification of the value (an unresolvable value makes
  membership itself unprovable — fail closed), while emit membership is the
  authored object KEY (complete without value resolution); the emit
  payload's unresolved interior rides the published replay route / authored
  display under the established stable-carrier member-position rule rather
  than a resolved-surface claim. The uniform rule across the nine producers
  is therefore: membership-bearing resolution failures name a reason and
  never publish complete; value-interior carriers follow the member-position
  stable-carrier contract.
- **Open generic props payloads.** `<script setup generic="T">
  defineProps<T>()` is an OPEN member domain, not an empty surface: the
  constraint's closed part publishes as the presence lower bound
  (`T extends { a: number }` publishes `a`) and an unconstrained parameter
  publishes the empty presence floor — `OpenPresence`, complete-as-a-result
  and warm-capable, never a reason-free `NoSurface` and never a false
  partial.
