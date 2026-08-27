# C1 fifteenth deviation — F15: starting the phase-4/7 cutover, narrow inert slice only

Continuation of F12's own deferral ("its own dedicated phase-4/7 resolution
cutover... whoever picks it up" — `f12-deviation-consult.md`). This round I
did a first-pass investigation of `ProjectResolver` (`crates/verter_workspace/
src/resolver.rs`, 2122 lines) and `WorkspaceRead`/`TransactionReader`/
`engine.rs`'s resolution-currency layer toward F12's own 5-item "evidence
that would justify STARTING that cutover" checklist, then consulted before
committing implementation effort (this is genuinely large, high-stakes
architecture work — the sixth-deviation protocol's spirit applies even
though this continues an already-ratified finding rather than reopening
one). Full consult prompt/output: `/tmp/c1-phase7-scoping-prompt.md` /
`/tmp/c1-phase7-scoping-output.md` (not committed — ephemeral scratch; this
file is the durable record).

## What I found before consulting

- Confirmed (grep across the whole file): exactly THREE distinct I/O
  primitives are used anywhere in `resolver.rs` — `WorkspaceRead::
  probe_path`, `::realpath`, `::read_package_manifest` — mapping 1:1 onto
  `InputKey`'s ALREADY-EXISTING `PathProbe`/`RealPath`/`PackageManifest`
  variants.
- The control-flow structure (fallthrough chains, the recursive
  depth-bounded cycle-guarded project-reference walk, the `node_modules`
  ancestor-directory walk) is entirely pure/in-memory computation over
  `Vec<IdeProjectConfig>`; only leaf calls touch I/O.
- `ProjectResolver::resolve_tracked`/`resolve_for_project_tracked` already
  have the exact capability-witnessed thin-adapter shape F4's corrected
  scope describes — `resolve_with_reader`/`resolve_for_project_with_reader`
  (which they delegate to) are the actual algorithm.
- `PackageManifest` (`verter_workspace::types.rs:130`) confirmed
  dependency-neutral plain data.

## Consult verdict

**ADOPT-NOW, but a NARROWER inert slice than I proposed.** Full findings
below; this section records the ratified disposition, not my own draft.

### 1. Tractability — confirmed separable, risk is real but ELSEWHERE

My three-primitive claim confirmed correct — but NOT every candidate is
computable purely upfront as I'd assumed: candidates DEPEND ON LOADED
MANIFEST CONTENT (`exports`/`imports` targets+conditions+wildcards,
`main`/`module`/`types`/`typings` fallback fields) — genuine multi-wave
dependency structure, not a single upfront-computable candidate set.
`realpath` is conditional on its own probe reporting `File`/`Directory`
first. F4's "highest-risk piece of work" framing stands — but the risk is
in `TransactionReader`/`engine.rs`'s orchestration layer, not hidden I/O in
`resolver.rs` itself:

- `TransactionReader` (`resolution_currency.rs:2274`) does far more than
  delegate: records `PathProbe`/`Realpath`/`Manifest` facts, adds
  recovery-scope facts, drains backend-internal directory enumerations
  into `DirectoryMembers`, converts `Inaccessible`/`Unknown` into
  non-admission, associates everything with one captured resolution world.
- `resolve_import_outcome_in_published` (`engine.rs:3088`) wraps the
  resolver with exact resolution, context selection, candidate reuse,
  evidence refresh, final world fencing, baseline folding, decision-node
  publication.
- The captured world stores actual probe/realpath VALUES but only
  manifest FINGERPRINTS, not parsed manifests — a coherent immutable
  kernel view CANNOT simply borrow the current world unchanged; needs its
  own materialized shape.
- The existing 8-attempt world-churn retry loop is a DIFFERENT mechanism
  from `AttemptOutcome`'s own attempt/depth/unique-key budgets — must
  stay separate, never conflated.

### 2. NeedInputs batching shape — "staged priority-frontier batching"

Neither pure-narrow (one key at a time) nor pure-batch (the whole
remaining chain). Batch the maximal BOUNDED set of SIBLING observations
within the current semantic branch AND current dependency wave:

- `probe_path(base)`: batch the extension/index candidate probes for
  that one base.
- `node_modules`: batch manifest probes for the WHOLE ancestor-directory
  list (this specific case IS whole-chain, since it's one bounded wave).
- An alias/`paths` branch: batch its currently-derivable target
  candidates.
- Manifest-DERIVED targets (`exports`/`imports`/legacy fields): load in a
  LATER wave, only after the manifest itself resolves — never expand
  through an unknown manifest.
- `realpath`: request only AFTER its corresponding probe is known
  positive — never speculatively ahead of that.

Boundary principle: semantic precedence STAGE plus data-dependency WAVE,
never a bare source-code function boundary, and never past a
lower-priority resolution-kind branch while an earlier branch remains
unresolved. Needs a 3-state ordered-search combinator (hit / exhausted-miss
/ blocked) that unions blocked sibling `LoadSet`s — NOT built this round,
recorded as a concrete design target for whoever converts the actual
algorithm.

**Correctness rule, load-bearing**: a `NeedInputs` attempt MAY prefetch
several facts speculatively, but the eventual `Complete` attempt's
recorded witness/fact-signature must report ONLY the facts it ACTUALLY
CONSUMED before short-circuiting — never every speculatively-prefetched
fact. Recording unconsumed prefetches as "observed" would make an
unrelated fallback-candidate content edit invalidate a successful
resolution that never depended on it.

### 3. Minimum first slice — MY proposal (relocate pure types/functions)
was REJECTED

Two concrete blockers found in my draft:
- `IdeProjectConfig` does NOT contain a `ProjectMembership` I could
  relocate in isolation — it contains `ConfiguredMembership`, whose
  transitive closure includes static membership specs, compiled globs,
  canonical paths, and materialized sets. Bigger than I'd assumed.
- `verter_semantic` still depends on `verter_workspace` TODAY (the edge
  reversal is a LATER step, scoping-spec §4 step 8). Moving live
  helper/config types into `verter_semantic` while `verter_workspace`
  still needs to consume them immediately either creates a Cargo cycle or
  forces a temporary duplicate implementation — the latter violates the
  single-authority rule this whole block exists to enforce. Also caught:
  `normalize_canonical_id` is ALREADY ultimately owned by `verter_span` —
  `verter_semantic` should import THAT owner directly, not create a
  second normalization implementation by relocating `resolver.rs`'s
  wrapper.

**The corrected safe first slice** (what's ADOPT-NOW, executable
immediately, no further ruling needed):
1. A characterization/equivalence harness (dual-runner-ready; the exact
   missing matrix: runtime JS with/without `.d.ts` companion, candidate
   priority, package-follow evidence, admitted-miss vs. refusal,
   observation order) — reuses existing coverage where possible. This is
   C1-AC-1's own early-and-often regression harness, not new scope.
2. INERT semantic-side module-resolution observation vocabulary only:
   dependency-neutral path-probe/manifest-projection DTOs, the three new
   `ResolverObservation` methods, `NeedInputs`-vs-stable-negative tests,
   the exhaustive test-double update.
3. **NO production resolver call-site changes. NO live resolver
   helper/config type relocation. NO Cargo-edge reversal.** All deferred
   to the coordinated cutover — pure helpers move WITH the canonical
   algorithm core when it converts, never ahead of it as duplicates.

### 4. Observation-view shape — ONE trait boundary, no parallel snapshot type

Both a DTO layer and a concrete implementor are needed, but `ResolverObservation`
stays the SOLE kernel-facing boundary — no separate `ResolutionProbeSnapshot`
interface. Since the trait is sealed (`verter_semantic`-only), `verter_workspace`
cannot implement it directly: a `verter_semantic`-owned concrete
`ResolverAttemptView`-shaped type is needed, which `verter_workspace`
CONSTRUCTS from captured/committed observations and which implements
`ResolverObservation`. Matches `RouteAnalysisInputs`'s own intent
(orchestration materializes owned data downward; the single
capability-limited trait remains the kernel-facing surface) — NOT built
this round (needs the actual conversion, not the inert vocabulary).

Method shapes (landed this round, see below):
```rust
fn path_probe(&self, path: &str) -> AttemptOutcome<verter_workspace::resolution_currency::PathProbe>;
fn real_path(&self, path: &str) -> AttemptOutcome<Option<CanonicalId>>;
fn package_manifest(&self, directory: &str) -> AttemptOutcome<Option<Arc<ResolutionPackageManifest>>>;
```
`package_manifest` takes a DIRECTORY (matching `InputKey::PackageManifest`'s
existing `directory` field) — NOT the full `package.json` file path the
current resolver code passes around (`read_package_manifest_if_present`'s
`package_json_path` parameter); the eventual session-side adapter joins
`"package.json"` at the actual read point, keeping the two key identities
distinct as the consult required. `path_probe`/`real_path` reuse
`verter_workspace`'s existing `PathProbe`/`CanonicalId` types directly
rather than inventing parallel mirrors — same precedent as
`lookup_ambient_symbol` (Part A row 67: "`verter_semantic` already depends
on `verter_workspace` today (fine for now)... revisit once F4's edge
reversal lands, not before"). `package_manifest`'s DTO IS narrowed
(`ResolutionPackageManifest`, new type) per the consult's explicit
"narrow resolution-manifest projection" instruction — confirmed by
grepping `resolver.rs`'s own field usage: `exports`/`imports`/`main`/
`module`/`types`/`typings` are used, `name`/`version`/`raw` are NOT.

**Open item, explicitly flagged, NOT resolved this round**: "consumed
observation keys/evidence need to be attempt output — or use the shared
Part F attempt-output carrier. Recording every prefetched fact as
consumed is not acceptable." This means Part F's attempt-output bundle
design (already blocking `observe_borrowed_signature`/
`record_ambient_dependency`/`cached_synthetic_binding_shape`) is now ALSO
a prerequisite for the phase-7 cutover's own correctness, not just those
three unrelated methods — raises Part F's priority.

## Explicit instruction, followed

"Safe to execute immediately without another ruling: characterization/
equivalence harness [not attempted this round, see below]. Semantic-owned
probe and narrow manifest observation DTOs. Three new `ResolverObservation`
methods. Exhaustive test-double updates. `NeedInputs`, stable-negative,
dependency-wave, and priority-frontier tests [the combinator-level
dependency-wave/priority-frontier tests are deferred with the algorithm
conversion itself — the three methods landed this round are independent
peeks, not yet composed into the ordered-search combinator the consult
describes]. No live resolver relocation or Cargo-edge reversal yet." The
characterization/equivalence harness (item 1) is NOT attempted this round
either — it requires two genuinely comparable lifecycles exercising the
SAME resolution paths, which do not yet exist for these brand-new inert
methods (identical to every prior landed method's status); recorded as
still-owed work, not silently dropped.
