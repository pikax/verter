# Path-precise resolution currency contract

Status: executable C0 contract plus the C1 resolution world/transaction, the
C2 workspace resolution-cache cutover, the C3 resolve-domain store cutover,
the C5a resolution-root capture, and the C4a + C4b authority cutover.

This document is normative for import/module resolution currency. The
test-only contract modules in `verter_workspace` and `verter_session` include
this file at compile time and freeze the behavior that the production C1–C5
cutover must satisfy. C1 adds the production world root, sealed transaction,
typed publication carrier, and immutable filesystem-evidence bridge. C2 makes
the fact signature the sole warm-validity oracle for the workspace resolution
cache — global `content_generation` equality and global cache clearing are
removed as correctness mechanisms. C3 consolidates every resolution
producer — the caller-supplied exact table included — behind one bounded
multi-candidate owner-edge slot, moves `ResolvedImportFactsDb` onto the shared
`ValidatedFactCache` substrate, and deletes the known-miss key dimension and
its generation sidecar. C5a gives the session store view its captured
resolution root and the validating `Resolution` arm that reads it. C4a
re-homes the import-route value onto the resolve domain: the
`DerivedFactKind::ImportRoute` digest and its generation-current
producers are deleted, and every consumer roots on the owner's
import-route RESOLUTION WITNESS instead. C4b completes the authority
cutover — `IndexedReady` and `ShallowFileState` retain NO resolved
target, materialisation performs ZERO import resolution, the
edge-currency oracle chain and the route-only edge-refresh materialise
lane are deleted, the host-side positive-route memo and its
`PositiveRouteStamp` are deleted, and the `Route` derived fact becomes a
pure parse-domain digest whose resolve-domain half rides the traversal
witness. C5b (the observe-only, O(1)-in-published-owners store-view
build) remains a later cutover.

C0 adds 25 newly ignored executable gates: 19 resolution-currency cases and
6 mutation-point concurrency cases. The pre-existing unrelated-new-path
marginal gate is unchanged and is not part of that count.

## Request identity

Resolution request identity is the complete `ResolutionQueryKey`:

```text
ResolutionQueryKey {
    entry: ResolutionEntry,
    normalized_specifier: NormalizedSpecifier,
    phase: ResolvePhase,
    request_kind: ResolveRequestKind,
    context: ResolveContextId,
    population: ResolutionPopulation,
}

ResolutionEntry =
    Importer(CanonicalId)
  | ExplicitProject(ProjectIdentity)

ResolutionPopulation =
    Base
  | Session(SessionFingerprint)

ResolveContextId {
    project_identity: ProjectIdentity,
    resolver_policy_identity: ResolverPolicyIdentity,
    provider_policy_identity: ProviderPolicyIdentity,
    resolve_env_hash: ResolveEnvHash,
}
```

`ResolveContextId` is the identity of the project/config/provider selection
used by the request. The exact-resolution table is consulted and witnessed
before context selection; an exact miss is an observation.

The existing five-way environment split remains structural:

- `parse_env_hash`, `resolve_env_hash`, `type_env_hash`, `lib_env_hash`, and
  `project_identity` remain independent typed dimensions.
- Resolution identity carries `resolve_env_hash` and `project_identity`; it
  does not bundle the other dimensions into a `project_config_hash`.
- `parse_env_hash`, `type_env_hash`, and `lib_env_hash` remain independent and
  enter only layers that actually depend on them.
- Raw `u64` values do not cross request, context, world, epoch, content, or
  store-generation APIs.

## Closed resolution-fact taxonomy

`ResolutionFactKey` is closed over exactly seven key families. Adding a new
resolver observation is compile-breaking until the enum, mutation rule,
validator, and witness tests all handle it.

| Key family | Key | Observed value |
|---|---|---|
| Path probe | `PathProbe { canonical, population }` | Overlay-aware `File`, `Directory`, `Absent`, `Inaccessible`, or `Unknown` |
| Manifest | `Manifest { canonical, population }` | Parsed resolution-semantic fingerprint; unrelated raw bytes are excluded |
| Realpath | `Realpath { requested, population }` | Typed target, absence, or error plus resolved-path recovery identity |
| Exact resolution | `ExactResolution { entry, specifier, phase, kind, population }` | Exact target or exact miss |
| Directory membership | `DirectoryMembers { canonical, population }` | Canonical membership digest, only when enumeration occurred |
| Recovery scope | `RecoveryScope { canonical_prefix, population }` | Boundary-correct watcher-recovery version |
| Context selection | `ContextSelection { entry, population }` | Selected `ResolveContextId` |

`Absent` is a stable cacheable observation. `Inaccessible` and `Unknown` are
distinct observations and are never converted to `Absent`.

The C0 reader fixture records the typed result at
`ContractReader::observe_probe_path`. Its temporary
`WorkspaceRead::file_exists` implementation is the only adapter onto the
current boolean production seam. C1 replaces that exact trait-method adapter
with typed `WorkspaceRead::probe_path` plumbing; it does not replace
`observe_probe_path` or `assert_typed_probe_return_only`. The assertion first
proves the resolver observed `Inaccessible`/`Unknown`, then proves that
observation forces typed non-admission and a later request probes again. Thus
C1 may change how the typed observation enters the resolver, but cannot change
the property or launder it into cacheable `Absent`.

## Observation-to-fact table

Every cold resolution records facts for the operations it actually performs.
Candidate ordering is part of the witness: a positive retains the selected
candidate and every higher-priority guard that would have won; a miss retains
the complete exhausted probe set.

| Resolver observation | Facts recorded |
|---|---|
| Exact-table hit or miss | One `ExactResolution` fact for the complete `(entry, normalized specifier, phase, kind, population)` key |
| Project/config/provider selection | One `ContextSelection` fact carrying the selected `ResolveContextId` |
| Path probe | The exact `PathProbe`, plus `RecoveryScope` for every requested-path ancestor that can change its meaning |
| Path probe whose canonical meaning crosses a realpath/symlink | The exact requested `PathProbe`, the corresponding `Realpath`, and `RecoveryScope` facts for both requested-path and resolved-path ancestor chains |
| Realpath lookup | The exact `Realpath` plus requested-path and resolved-path ancestor recovery facts; absence/error still records the requested chain |
| Manifest/config read | The exact `Manifest`; its prerequisite path/realpath observations retain their own facts separately |
| Directory enumeration | The exact `DirectoryMembers`; its prerequisite path/realpath observations retain their own facts separately |
| Positive candidate | The selected candidate's complete observation set and every higher-priority absent/negative candidate's complete observation set |
| Resolution miss | Every probe, manifest, realpath, directory enumeration, exact miss, context selection, and recovery fact observed before exhaustion |
| Provider projection | The facts for the provider policy in `ResolveContextId` and every path/realpath observation used to project the canonical result |

Recovery chains are canonical path-component chains, not byte prefixes.
`/a/b` contains `/a/b/x` but never `/a/b2` or `/a/b2/x`. Both the requested
chain and the resolved chain are retained because either can change a
resolution without changing the other.

## Mutation-to-fact table

Mutations advance facts only when the corresponding observed value changes.
They never fan out through reverse dependants and never fall back to a global
content or file-set generation.

| Mutation | Facts advanced |
|---|---|
| Path appearance, disappearance, or type change | Exact `PathProbe`, exact `Realpath`, and enumerated parent `DirectoryMembers` facts — never a `RecoveryScope` |
| Ordinary source-byte overwrite | No resolution fact, unless the file was observed as a resolution-semantic manifest/config input |
| Consulted manifest/config semantic change | That `Manifest`/context fact and affected persistent project-selection facts |
| Overlay open, close, or reveal | Only facts whose population-visible effective values changed |
| Symlink/realpath change | Exact `Realpath` plus requested-path and resolved-path ancestor `RecoveryScope` facts |
| Exact override insert, update, or remove | That exact `(entry, specifier, phase, kind, population)` fact |
| Project graph, resolver policy, or provider policy change | Persistent selection/config index nodes for affected projects/scopes; never importer fan-out |
| Imprecise watcher event | The narrowest representable boundary-correct `RecoveryScope`; recorded descendant witnesses fail through that ancestor |

`ContextSelection` is a computed resolve-imports fact. Validation queries the
captured persistent project-prefix/config index for the one entry and compares
the selected `ResolveContextId`.

Recovery scopes are advance/observe asymmetric: every path or realpath
observation OBSERVES the recovery facts for its requested-path (and
resolved-path) ancestor chains, but only an IMPRECISE mutation — a watcher
subtree recovery whose member set the engine cannot enumerate — ADVANCES a
recovery scope, and then only the narrowest boundary-correct one. A precise
per-path mutation advancing an ancestor scope would destroy every sibling
witness under that ancestor.

A per-canonical content transition reported without a reader in scope
(`bump_content_generation_for`) advances zero resolution facts speculatively.
The canonical enters a pending-evidence ledger and the world identity
advances, so an in-flight transaction straddling the transition retries. The
resolve path — where the reader is in scope — then re-observes exactly the
intersection of that ledger with the canonicals the candidate's witness
recorded (strictly O(witness facts)): typed path probe, realpath, and, for a
manifest, the parsed resolution-semantic fingerprint, advancing only facts
whose observed value actually changed against the world's recorded evidence
baseline. Admitted attempts fold their raw observed values into that baseline
through the mutation protocol; a fold that conflicts with the recorded
baseline reveals state newer than the captured root, advances the affected
facts, and forces the attempt to retry instead of admitting.

## World publication protocol

`ResolutionWorldRoot` is immutable and pins every resolution-visible input:
exact resolutions, project/config/provider selection, base/session overlay
population, realpath state, and the persistent resolution-fact version root.
A base root and each session overlay domain have independent typed gates and
epochs. A session transaction fences both its base and overlay roots.

Every resolution-visible mutation publishes in exactly four ordered steps:

1. Take the resolution-world write gate and advance `ResolutionEpoch` from a
   stable even value to a write-in-progress odd value.
2. Apply the mutation and construct replacement immutable world/fact roots.
3. Release-publish the new `Arc<ResolutionWorldRoot>`.
4. Advance the epoch to the next stable even value and release the write gate.

A transaction captures the relevant even epochs and immutable roots. It
retries if an epoch is odd or changes, or if a root changes. Immediately before
admission it requires the same stable epochs/roots and validates the final
signature against the composed root. Bounded retry exhaustion is typed
non-admission. A candidate assembled from observations belonging to different
world roots is never admitted.

For a filesystem-backed transaction, discovery may consult the ordinary
workspace caches only to identify which observations the resolver made. Freeze
then independently re-reads every observed path type, file/manifest value,
realpath, and exact sorted directory membership from the live filesystem,
bypassing mutable snapshot, probe, package, realpath, and directory caches.
The admitted replay reads only that immutable captured evidence. A missing,
conflicting, inaccessible, or unstable observation makes the bridge incomplete
and forces typed non-admission. Overlay evidence is composed into the same
capture, while the final bridge, world, epoch, and published-root comparisons
remain mandatory.

A freeze-time re-read that CONFLICTS with the discovery observation is state
newer than the captured root, so it enters through the ordinary invalidation
protocol: the affected directory index and realpath memo are marked dirty
before the attempt is refused. Without that, a cache which went stale because
its change never reached the bridge would be contradicted by the independent
re-read on every attempt and the request could never admit again; with it, the
retry's discovery observes the live truth and converges within the bounded
attempt budget.

The live filesystem reader is never bridge complete, so it may not admit any
resolution — including the resolutions a parsed-edge batch performs. Recording
parsed edges therefore runs the SAME two-pass protocol: one discovery pass that
publishes nothing, then the identical recording replayed against the frozen
reader, which is the only reader whose parsed-edge resolutions the Engine
admits. A workspace with no published root records no batch at all: there is no
resolution authority to admit against, and applying the batch's destructive
half (clearing the exact/lazy/semantic-transitive stores) without its recording
half would leave exactly the torn state the protocol exists to prevent.

### Request-local snapshots, and what the filesystem backend is not

A reader is REQUEST-LOCAL when its answers are scoped to one request and may
neither read nor populate the shared candidate slot. Exactly one reader is:
the overlay snapshot composed over an Engine-backed workspace, whose answers
are overlay-effective while its cache key names the enclosing population.

The filesystem backend's two readers are NOT. Their split of duties is
admission, not scope:

- the DISCOVERY pass reads live disk through mutable caches and is never
  bridge complete, so it can never admit. It may still READ a warm
  candidate, and must: a warm owner edge answers with no filesystem syscall
  at all.
- the FROZEN replay is bridge complete exactly when its independently
  re-read evidence is complete and revalidates. Its admitted candidate is
  publishable, and its observed values are folded into the world's evidence
  baseline through the mutation protocol — a conflict advances the affected
  facts and forces a retry, so a candidate is admitted only once the
  recorded baseline AGREES with live disk for every value it observed.

Treating either as request-local silently disables the resolution memo on
the production LSP backend: no candidate is ever read, none is ever
published, and `add_lazy_resolved_dep` never runs, so lazily resolved
(bare-specifier) edges never become reverse-queryable. Every import then
pays two full resolver passes with real `probe_path` / `realpath` traffic,
on every touch, forever. That is strictly worse than the global-generation
memo this design replaced, and it is invisible to a suite whose warm-reuse
assertions all run on the in-memory backend —
`filesystem_resolution_publishes_and_reuses_a_warm_owner_edge` exists to
keep it visible.

A warm REUSE, however, observes nothing — so the freeze-time fold has
nothing to re-read, and the frozen replay's bridge-completeness check is
satisfied without confirming anything. On a backend where every mutation
enters the protocol that is exactly right. On this one it is not: a package
installed into `node_modules` reaches the workspace with no event of any
kind, because that tree is inside VS Code's default `files.watcherExclude`,
so no `didChangeWatchedFiles` notification is produced and no
`WorkspaceChange` is applied. A known miss's recorded `Absent` probe would
keep validating for the process's lifetime, and the diagnostic would survive
until the server restarted.

So a BACKEND — never a reader — states its evidence capability, once, at the
Engine resolution entry it calls:
`ResolutionEvidenceSource::{Inert, ReaderAuthoritative, Uncovered(source)}`.
At the FIRST resolution after each content transition, an `Uncovered`
backend's reused candidate has its own witness canonicals re-read through
that source and folded value-sensitively: a value that moved advances its
exact fact, the candidate stops validating, and the demand re-resolves
through the ordinary retry.

**The capability is deliberately NOT a `WorkspaceRead` hook.** A reader hook
is forwarded by every delegating wrapper — the overlay snapshot reader, the
transaction recorder, the frozen replay — and a wrapper that forgets one
silently inherits the trait default. Three consecutive review rounds landed
an evidence fix on one reader while a second reader was the one production
composed. A required parameter on `Engine::resolve_import_outcome_*` cannot
be forwarded, stripped or forgotten: the backend that owns the Engine states
it, and no reader layered on top participates at all. The evidence-less
Engine entries (`Engine::resolve_import`, `Engine::resolve_import_outcome`)
are `cfg(test)`, so production cannot name them.

`ResolutionEvidenceSource::Inert` is the fail-closed answer: nothing is
re-observed, no baseline is folded, and no canonical is stamped verified. It
can only ever fail to HEAL; it can never certify stale state as freshly
verified. There is no path that synthesises an observation without a
`LiveResolutionEvidence` implementation, so "a caller that did not opt in has
its canonicals stamped verified" is unrepresentable rather than merely
unlikely.

#### One live-observation primitive

`LiveResolutionEvidence::observe_live_resolution_evidence(canonical, recorded)
-> Option<LiveResolutionObservation>` returns the live triple
`{probe, realpath, manifest}` and is the ONLY way a REUSE-time consumer learns
what the source currently says.

That is not a tidiness point. The two paths were built separately and
promptly diverged: the freeze bridge went through the backend's independent
rail (bypassing the file snapshot, directory index, realpath memo and parsed
manifest cache), while the refresh path called the ordinary accessors — the
very caches an evidence read exists to check. **An evidence read exists to
detect the changes the event stream missed, so routing it through a cache
whose invalidation depends on that same event stream can only ever confirm
the cache.** The overlay is the one exemption: it is authoritative state, not
a copy of state.

**What freeze and refresh actually share, precisely.** They are not one read.
They share the backend's `independent_*` rail (`independent_probe_path` /
`independent_file_bytes` / `independent_realpath` / `independent_manifest`) —
the cache-bypassing reads themselves — and the one memo-repair function
(`repair_resolution_memos`), so a repair can never be performed by one path
and skipped by the other. They differ in two recorded, deliberate ways, and
in both the FREEZE side is strictly BROADER, so it can over-invalidate but
never under-invalidate:

- **Comparison depth for manifests.** Freeze compares the whole
  `PackageManifest`, `version` and `raw` included (`manifests_equal`); refresh
  compares the resolution-SEMANTIC fingerprint only
  (`manifest_resolution_fingerprint`). A `version`-only rewrite therefore
  makes freeze re-run its attempt and leaves refresh's fact alone — which is
  correct for refresh, because no resolution outcome depends on it.
- **Non-present short-circuit.** `observe_live_evidence` stops at the probe
  for `Absent` / `Inaccessible` / `Unknown` and reports the realpath and
  manifest limbs as absent, because the same errno would come back from both;
  freeze re-reads every recorded family unconditionally, since it replays a
  recorded observation set rather than a witness.

`freeze_and_refresh_agree_per_family_on_the_same_input` pins both halves: a
resolution-semantic change in any family reaches the SAME verdict through
both paths, and the one divergent input (a `version`-only manifest rewrite)
is broader-on-freeze rather than narrower.

Consequences that follow from the single primitive:

- **One read, not five.** The old path re-synchronised, then re-probed
  through `probe_path` / `realpath` / `read_file`. The source now returns what
  it read and the engine folds exactly that.
- **The verified stamp certifies only actual live reads — and an
  `Inaccessible` probe IS a read.** `Absent`, `Inaccessible` and `Unknown` are
  observed VALUES: they are folded, they advance their fact on a genuine
  value change, they stamp their canonical and they drain it from the pending
  ledger. `Inaccessible` in particular is a first-class outcome the resolver
  already acts on — an observed `Inaccessible` forces
  `NonAdmissionReason::ResolutionInaccessiblePath` — so dropping it would
  leave the candidate's `PathProbe` fact frozen at its last readable value,
  its signature validating forever, and a target the process can no longer
  read served warm for the process's lifetime. `None` is reserved for "this
  source genuinely cannot observe the canonical at all" (unstable I/O behind
  a present path; a target with no live filesystem): not stamped, not folded,
  not repaired. Stamping an unread canonical does not merely fail to heal —
  it certifies stale bytes as freshly verified.
- **Memo repair fires only on disagreement**, per family, against the
  world's recorded baseline (`RecordedResolutionBaseline`), and runs through
  one repair function shared with the freeze bridge. A family with NO
  recorded value is not a belief and contradicts nothing — conflating
  "never observed" with "observed absent" fires a repair on every tick for
  canonicals nothing has changed about.
- **Manifest repair drops both layers.** The parsed manifest is derived from
  bytes the shared file snapshot holds read-through, so dropping only the
  parse re-parses the same stale bytes; the two-pass bridge then never
  converges and the resolution is refused for retry exhaustion rather than
  answered.
- **Healing is pinned at every production entry.** The three healing
  scenarios (manifest rewrite, snapshot-resident deletion, known-miss
  appearance) plus the inaccessible-target case run against
  `resolve_import_outcome`, `resolve_import_outcome_with_overlay` and
  `resolve_import_at_published`. A request-local entry reads no candidate and
  therefore cannot serve a stale one, but it is covered anyway: a later change
  that lets it reuse inherits the healing instead of silently losing it.

#### One first-observation rule, and what a fill may not do

Observations and mutations follow OPPOSITE rules about an unrecorded
baseline, and both rules are now stated exactly once:

- a MUTATION (`update_base_*_fact`) says "this path's value is now X". An
  unrecorded baseline counts as a change, because a witness that observed the
  path holds the fact at `INITIAL` and would otherwise keep validating.
- an OBSERVATION (`fold_observed_baseline`) says "I looked, and it is X".
  Filling an unrecorded baseline contradicts nothing, so it advances no fact,
  supersedes no captured root, and forces no retry. Both evidence consumers
  fold through that one function.

A fill nevertheless has to PERSIST, which is why the world write is
three-valued (`WorldWrite::{Discard, Retain, Publish}`): `Retain` stores the
replacement root under the SAME world identity and epoch. Publishing a new
identity for a fill is a self-fence — every concurrent attempt's capture
stops being current and retries for a fill that moved no fact — and
discarding it leaves the family permanently unrecorded, so every later
observation of it is a first observation forever and a real change can never
be detected as one.

#### Fold totality

The admitting fold records a baseline for EVERY reobservable family a witness
can carry — probes, realpaths AND manifests, exactly the families
`ResolutionFactKey::reobservable_path_canonical_id` admits. The base world's
`manifest_fingerprints` is `Option<[u8; 16]>` for the same reason `realpaths`
is `Option<String>`: "no manifest here" is a value, and a map that cannot
represent it cannot distinguish it from "never observed".

Totality is what makes "unrecorded baseline at refresh time" impossible for a
canonical any admitted witness names, and therefore what makes the
first-observation arm of the fold a residual rather than load-bearing policy.
While manifests were missing, no manifest baseline was ever recorded on the
resolve path, so a `package.json` `exports`/`types` rewrite — what every
`npm install` performs, with no watched-file event — could never be detected
as a change at all.

Three bounds keep that from becoming the cost it replaces:

- **paths only.** `ResolutionFactKey::reobservable_path_canonical_id` admits
  `PathProbe`, `Realpath` and `Manifest` and excludes `RecoveryScope` (an
  ancestor PREFIX, up to and including `/`), `DirectoryMembers`,
  `ExactResolution` and `ContextSelection`. Re-reading those enumerates the
  filesystem root. The match is exhaustive, so a new fact family cannot skip
  the question.
- **once per content generation.** A per-canonical stamp records when a
  path's base evidence was last read live; the steady state selects no target
  and performs no I/O at all, so warm reuse inside one generation is
  unchanged.
- **outside the write gate.** The values are read before the resolution-world
  gate is entered, and an unchanged observation never clones the world root.

**The cost model, stated plainly.** On a backend with no event coverage,
warm reuse is NOT zero-syscall. It is zero-syscall WITHIN a
content-generation tick, and O(distinct witness path canonicals) live
observations per tick, deduped engine-wide by the verified stamp — one live
observation per canonical, not five. Per canonical that is one `metadata` for
an absent path, plus one `canonicalize` for a present one, plus one `read`
for a present manifest; an `Absent` probe short-circuits the other two,
because the same `NotFound` would come back from both. That is the
irreducible price of correctness without events; the only cheaper ratified
option is non-admission, which measured worse than the old global clear. The
`resolution_evidence_live_read_count` provenance counter is the measurement
rail, and
`warm_reuse_costs_one_live_read_per_witness_canonical_per_generation` pins
both directions: zero reads for repeated demands inside one generation, and a
bounded non-zero count for the first demand after a transition.

The correct external fix for `npm install` is a client-side
`didChangeWatchedFiles` (or an explicit refresh) covering `node_modules`. The
generation-tick re-observation is the BACKSTOP for when that never comes, not
a substitute for it.

This heals at exactly the point clearing the whole resolution memo per
content generation used to heal — the previous design's only mechanism for
this class — by re-reading one candidate's own witness instead of discarding
every candidate in the workspace. The residual exposure is narrower and
stated plainly: a disk change that reaches neither the Engine nor any
subsequent content transition is not observed, which is `.DECISION.md` §4's
event/watch-bridge case.

A StoreView captures the root in O(1), validates only against that immutable
root, and never resolves. A cold miss through a non-current root produces
typed `ViewSuperseded`; it does not consult live mutable state or inject a
newer witness into the older view.

The capture is the sealed `CapturedResolutionWorld`: the current base root
plus, for a session population, that session's overlay root. A consumer view
obtains one ONLY through `WorkspaceRead::capture_resolution_world` — the
carrier has no public constructor, so a probe result, an overlay lookup, or a
normalized string can never be laundered into a trusted world. The session
store view takes it in the same pre-build read window as its other
by-value dimensions and stamps it into the immutable snapshot, and the
resolve-imports validator routes every `ResolveImportsFactRef::Resolution`
fact through that stamped capture. A view that captured nothing validates no
resolution fact.

Validity is fact-precise, never world identity: two captures of different
world roots agree on every fact neither of them moved, so a witness stays
valid exactly as long as the inputs it observed do. Population is part of
that comparison — a session-population fact resolves against that session's
overlay root and falls back to the base root only for its own population, so
a session witness validates against neither a base capture nor another
session's capture.

Fact-precision alone settles only two FRESH captures. A consumer view is
RETAINED: the session store view freezes its `Arc<CapturedResolutionWorld>`
into an immutable snapshot and answers every resolution fact out of that
frozen capture for as long as its cache reuses the view. A resolution-visible
mutation that advanced a fact version while every cache-reuse dimension stood
still would therefore leave the retained capture answering with pre-mutation
fact versions — a stale warm hit, not a conservative miss, and one no
argument about two fresh captures addresses.

The reuse oracle closes that gap with a `resolution_fact_generation`
dimension: the monotonic count of resolution fact-version MINTS. While it is
unchanged, no fact version has moved, so a retained capture is
observationally identical to a fresh one; when it moves, every consumer
holding an older capture rebuilds. It is not world identity and never
decides a fact's validity — world identity remains barred from cross-root
warm validity, and validity stays the fact-by-fact comparison above. What
makes it the right dimension rather than identity is that recording a
first-observation baseline for a path the world has never seen mints no
version, so a cold compute's own discovery does not churn the view cache.

Unlike the additive artifact and load generations, it belongs in the
EXTERNAL-SUPERSESSION fence too — and therefore in the singleflight lane
identity and the request-scoped compat token that derive from it. Every mint
is an external change entering the world, never a compute's own work: the
observed-value fold and the reader-driven evidence refresh both RECORD a
first observation without advancing a version, and advance one only on a
conflict with the recorded baseline or on a re-read value that moved;
exact-table, project-publication and mutation-protocol advances are external
by construction. That is the same criterion `content_generation` is included
under.

The concrete uncovered case was a workspace-level exact-resolution retarget
(`WorkspaceAccess::set_exact_resolutions`): it advances the `ExactResolution`
fact inside the resolution-world write gate and moves no content, project,
artifact, load, env, identity or overlay state at all. Leaving the dimension
out of the fence therefore broke three things at once — a stability promotion
of a pre-retarget result, a shared coalescing lane whose contract is that a
leader's result needs no per-follower revalidation, and a request-scoped
prepared-decl bundle memo re-serving pre-retarget resolved edges to the very
retry that exists to escape them.

An admitted resolution carries its own `ReadSetSignature`
(`AdmittedResolution::signature`), so a durable consumer roots its entry on
the same witness the transaction admitted rather than reconstructing one.

Exact-resolution mutation is pinned by five independent gates so no earlier
failure can shadow a later property:

1. the old candidate's exact-resolution witness is rejected;
2. StoreView capture performs zero routing work;
3. the next real demand recomputes one owner edge and publishes it exactly
   once;
4. the subsequent demand reuses that publication as a warm hit;
5. after exact miss → exact hit → exact miss, the original candidate still
   fails exact-fact validation and cannot revive through ABA.

The C0 session fixture observes those properties through a test-only semantic
event vocabulary: `ExactWitnessValidation`, `OwnerEdgeRecomputed`,
`OwnerEdgePublished`, and `OwnerEdgeReused`. The events are attached to the
owner edge and carry its returned target; they do not expose a cache name,
generation, hit/miss counter, `IndexedReady` field, or artifact-refresh
mechanism. C1-C5 may move the emission sites to the resolution transaction and
owner-edge authority, but the five cases and their assertions remain
unchanged. The observer and all current emission hooks are `cfg(test)`.

## Signature and admission

Resolution uses the existing fact/admission authority:

```text
FactReadSet
    -> SignatureAdmission
    -> ReadSetSignature
    -> ValidatedFactCache

FactVersionRef::ResolveImports(ResolveImportsFactRef)
    -> StoreView::validates_resolve_imports_domain
```

There is no sibling resolution signature, cache, validator, overflow
convention, or empty-signature fallback. `ResolvedImportFactsDb` moves onto
this existing bounded multi-candidate substrate during the cutover.

The fact-signature bound is `FACT_SIGNATURE_CAP = 1_024`. Empty means
dependency-free and cacheable. Overflow means the computed result is valid but
non-cacheable; it is not represented as an empty signature.

`SignatureAdmission` is:

```text
Cacheable(ReadSetSignature)
NonCacheable(NonAdmissionReason)
```

The following are typed non-admission and use
`CacheAdmission::ReturnOnly`/skip-publish:

- signature overflow;
- `Inaccessible` or `Unknown` probe outcome;
- unstable or newer-than-root I/O without an event-bridge publication;
- incomplete provenance or an untracked resolver branch;
- self-root conflict;
- cancellation, supersession, or bounded retry exhaustion.

`ReturnOnly` returns the computed result but publishes no cache candidate,
reverse-index metadata, or persistent artifact.

`ResolutionOutcome::into_publication()` is the only durable conversion.
`ResolutionPublication::Admitted` carries a sealed `AdmittedResolution`; no
caller can manufacture it from a direct probe, overlay lookup, or normalized
string. Its projection helpers require an existing Engine-minted carrier.
The former `from_admitted_state` constructor is deleted. Full
resolution-derived batches are admitted before any route, overlay, prepared,
provider, or session state mutates; one refusal rejects the whole batch.

Context selection over a complete published index always yields a real
observed value. "No configured project owns this entry" is a fully determined
function of the captured immutable root — it selects the stable
`ResolveContextId::unowned()` context (seeded into every published root, so a
project republish that does not change the entry's non-ownership keeps its
version) and remains cacheable. The genuine provenance gaps are the four
`ContextProvenanceError` variants — no published root, a resolver selection
with no snapshot projection, and a selected project missing its identity or
resolve-environment table row. Those yield `ResolutionIncompleteProvenance`:
the transient result may be returned, but no resolution-cache candidate,
parsed/lazy edge, provider/session state, or context-version entry is
published. Production eval/declaration canonical normalization re-enters the
Engine; the former direct existence-probe normalizer is compiled only for its
isolated test oracle.

### Structural-guard closure

The contract structurally type-checks the landed rail:
`FactVersionRef::ResolveImports(ResolveImportsFactRef)`,
`SignatureAdmission`, `ReadSetSignature`, and `ValidatedFactCache`. It cannot
yet structurally prove the later C3 store exclusivity because the production
`ResolvedImportFactsDb` is still a first-writer-wins `DashMap`. C1 does seal
`ResolutionTransaction` and the admitted publication carrier. A source-name
scanner would only approximate the remaining C3 claim and is forbidden.

The structural closure required from the cutover is:

1. Make `ResolutionFactKey` a closed resolve-imports-domain key carried only by
   `ResolveImportsFactRef` and therefore only by
   `FactVersionRef::ResolveImports`.
2. Make the Engine-owned `ResolutionTransaction` sealed. Only its
   resolution-specific finalizer may turn the transaction's resolver
   observations into `SignatureAdmission::Cacheable(ReadSetSignature)` for a
   `ResolutionOutcome`, and only the transaction may construct or record the
   new `ResolutionFactKey` observations.
3. Make `ResolvedImportFactsDb` a private-field newtype whose storage field is
   `ValidatedFactCache<ResolvedImportFactsKey, ResolvedImportFacts>`; the
   content-addressed key remains `{canonical, content_hash, parse_env_hash,
   resolve_env_hash, resolver_version}`, while base/session/world variants
   coexist as value-side signature candidates. Expose no raw insert,
   unvalidated read, alternate validator, or alternate signature constructor.
4. Make both the workspace resolution-cache publisher
   (`ResolutionQueryKey -> ResolutionOutcome`) and the
   `ResolvedImportFactsDb` owner-bundle publisher accept the transaction's
   `SignatureAdmission` directly and exhaustively lower its non-cacheable arm
   to `CacheAdmission::ReturnOnly`.

The exclusivity in step 2 is resolution-scoped, not a claim that the
`SignatureAdmission::Cacheable` enum arm becomes globally private. The
checkable production producer inventory is:

- `framework::script_facts::resolve_script_facts_inner` calls
  `SignatureAdmission::from_finalise`;
- `host_resolve::virtual_file_pipeline::ensure_compile_artifacts` calls
  `SignatureAdmission::from_finalise`;
- `fact_signature_helpers::fact_signature_for_exported_type`, reached in
  production through
  `component_meta_query_engine::engine_fact_signature_for_exported_type`,
  constructs the exported-type identity signature used by
  `registry_cache_producers`;
- `component_meta_query_engine::engine_fact_signature_for_materialize_memo`
  constructs the materialization signature used by
  `meta_resolve::projectors::output_sink` and
  `project_semantic_dispatch::semantic_source`.

`cache_runtime::SignatureAdmission::from_finalise` is the shared constructor
used by the first two producers. `ComputeCtx::signature_from` is currently an
unused wrapper, and `fact_signature_for_canonical_member` is `#[cfg(test)]`;
neither is a production producer. There is no
`component_meta_query_engine::engine_signature_from_deps`.

Those real producers remain legal for their current non-resolution domains.
They may continue to finalize or construct signatures over their existing
parse, semantic, compile, framework, and component-meta facts. They may not
construct `ResolutionFactKey`, finalize resolver observations, mint a
cacheable `ResolutionOutcome`, or publish a `ResolvedImportFactsDb` candidate.
Those operations require the sealed resolution transaction capability.

Those types, visibility boundaries, and sealed capabilities are the primary
structural guard. C0 deliberately lands no textual/name-keyed scanner as a
substitute.

## Concurrency outcomes

Mutation during exact-table lookup, project selection, filesystem probing,
provider projection, pre-admission validation, or request completion may
produce only:

- a wholly old result rooted and admitted only in the old world;
- a wholly new result rooted and admitted only in the new world;
- a retry that recomputes wholly in one world; or
- a typed `ReturnOnly` result with no admission.

A result whose observations span old and new roots is mixed-world. Admitting
it is forbidden even if a later generation check would happen to reject it.
Request-completion promotion revalidates under the destination population and
completion fence; a newer-world witness is never promoted into an older view.

For a cacheable outcome, the returned result and the admitted signature must
match the same captured world oracle; retry count does not relax that pairing.
A typed `ReturnOnly` is independently valid non-admission and need not equal
the old or new cacheable oracle. The C0 concurrency oracle materializes each
expected result in a fresh isolated `Engine` configured for its associated
`ResolutionWorldSignature`; it does not clear or otherwise inspect
`lazy_resolution_cache`, so the oracle survives C2 removal of that mechanism.

## One owner-edge authority

Every resolution producer resolves through ONE bounded candidate slot per
`(importer, specifier, phase, kind, population)`:

- The caller-supplied exact-resolution table is a cold COMPUTE of that
  authority, not a bypass of it. An exact hit reuses a validating candidate
  when one exists and otherwise publishes its result through the same slot,
  so an exact retarget invalidates through the same witness rail as any other
  resolution input.
- Each slot retains up to `CANDIDATE_CAP` candidates, oldest-evicted-first —
  the same policy, and the same constant, as the session `ValidatedFactCache`
  slot. A superseded target's witness therefore survives the demand that
  supersedes it, so that demand can name every witness it rejected.
- A cache-validation read path is an OBSERVER, never a producer — and after
  C4b it is not a resolver at all. C4a's interim `Route` edge-currency refresh
  needed an observe-but-do-not-admit mode (a validator that warmed the slot
  would answer the very demand whose recompute it exists to make observable),
  so it ran inside a thread-local `ResolutionObserverScope`. C4b deleted that
  refresh: `HostStoreView::build` publishes `Route` observe-only from the
  stored artifact and performs ZERO routing work, so there is no validator
  left that resolves. The scope had no remaining production call site and is
  DELETED with the refresh it existed for — a landed public API with no
  production caller is a worse contract than the invariant it once carried.
  Should a future validating read need to resolve, it must be reintroduced
  with its caller, not kept warm as a dead capability.
- Witness construction runs at PRODUCERS and admits normally — it is part of
  a cold compute, not of validation. It deliberately does NOT observe-only:
  the observations it records are the ones a later demand must be invalidated
  by, and a producer that refused to admit them would rebuild the same
  candidate on every touch.

## The import-route rail

An owner's import-route dependency is a set of resolve-domain facts, not a
digest. `VerterHost::owner_import_route_witness` resolves the owner's
AUTHORED specifiers through the shared route-edge policy and returns the
union of the admitted transactions' `ReadSetSignature` observations.

The inventory is pure parse domain — the scheduler parse snapshot's SFC
`src=` external requests, script import declarations, and module references,
unioned with an already-published `IndexedReady`'s shallow routing surface
(reexports and wildcard reexports), read observe-only. A resolved route table
is never an input.

Collection is SCOPED, not ambient. An admitted resolution's signature is the
transaction's complete observation set — a single miss carries its whole
exhausted probe set — and `resolve_for_persistent_state` sits on every
route-edge hop, so fanning every resolution into whatever fact tracer happens
to be installed inflates every read set in the process. The recorder
therefore writes only into an explicit witness scope that the builder
installs; with no scope active it is one thread-local integer load. Every
resolution the builder drives is recorded, not only the returned carrier's
own signature: the shared type-route policy probes `TypeImport`, falls back
to `EsmImport`, then re-normalises, so a `.d.ts` companion appearing beside
an already-resolving `.js` target retargets the edge through an INTERMEDIATE
lane's observation.

`None` — an unreadable parse surface, a refused resolution, or a union that
overflows `FACT_SIGNATURE_CAP` — is unrootable. The consumer serves its value
and refuses its own admission, and the refusal raises the enclosing
cold-compute suppression chokepoint. It is never represented by a truncated
or empty signature.

## Resolve-domain store

`ResolvedImportFactsDb` is one `ValidatedFactCache` slot per
`(canonical, content_hash, parse_env_hash, resolve_env_hash,
resolver_version)` — the shared bounded multi-candidate substrate, the
standard per-slot FIFO cap, and per-reader `ReadSetSignature` validation.
There is exactly one read (`get_if_valid`), and it validates against the
caller's own view.

Resolution currency is NOT a key dimension: it lives on the VALUE side, as
the owner's import-route resolution witness recorded in the candidate's
`ReadSetSignature`. The former `known_miss_generation` tag and the
`DerivedRawState` sidecar it folded are deleted, and so is the producer-side
hard removal that stood in for currency while the witness was unavailable — a
retargeted recomputation is a genuinely distinct candidate whose predecessor
simply stops validating, and a hard removal would also drop a sibling
candidate still valid for another view. A recomputation reproducing BOTH the
retained payload and its witness is skipped rather than churning the slot.

The admission is ORDERED strictly after the exact-resolution sync in
`set_import_dependencies`. The witness observes an `ExactResolution` fact per
specifier, so admitting before the push would record the pre-push version
that the push immediately advances — the candidate would never validate again
and every read would rebuild it.

For the same reason a known-miss carries no currency stamp at all. A negative
answer is not evidence that the answer is still negative, and a global
content generation is not a validity oracle for it, so
`import_route_entry_is_generation_current` reports NOT-current for every
known-miss and the specifier re-resolves through the one owner-edge
authority — where a warm candidate whose exhausted probe set is unchanged is
reused, so the re-resolve is cheap rather than cold. Host-memoized positives
keep their capture-before-resolve stamp; caller-supplied authoritative
positives keep serving until replaced.

## Parse/resolve ownership (C4b)

`IndexedReady` is a content-addressed PARSE/INDEX artifact and nothing
else. It retains authored import/export syntax, specifiers, shallow
declarations, locators, and parse-domain facts. It retains NO resolved
canonical, and materialising it performs ZERO import resolution.

Deleted with the resolved state:

- `IndexedReady.import_routes`, `import_route_hash`, `route_hash`,
  `edge_generation`, `project_generation`, and `has_cross_file_edges`;
- the resolved `canonical_id` on `ShallowFileState`'s `ImportTarget`,
  `WildcardReexport`, `ExportTarget::Reexport`, and `ExternalSymbolRef`
  (and, with them, `ImportRouteTarget.canonical_id` and
  `ExternalRouteRefFact.canonical_id` in the lower-crate route-fact
  grammar);
- the `ShallowImportResolver` trait and `HostShallowImportResolver` —
  shallow construction resolves nothing;
- the edge-currency oracle `route_surface_is_edge_current` and the
  route-only edge-refresh materialise lane
  (`refresh_indexed_route_surface`, `build_indexed_route_surface`, the
  `edge_refresh_gate_seam_hook`);
- the host-side positive-route memo (`cache_positive_import_route_result`)
  with `PositiveRouteStamp`, `import_route_is_generation_current`, and
  `import_route_entry_is_generation_current` — the last global-generation
  equality used as a warm-resolution validity test;
- `hash_import_route_targets` and the `IndexedReady.import_routes`
  fallback in `authoritative_import_route`.

`IndexedReady.built_at_content_generation` replaces `edge_generation` for
its ONE surviving consumer, `artifact_only_candidate_is_fresh`: a
CONTENT-domain stamp compared per-canonical against the workspace's
content-transition ledger, so a `get_any`-served artifact-only candidate
cannot outlive its canonical's content. It is never an equality test
against a live global generation and never a route/edge currency oracle.

The complete reuse gate for a published artifact is
`indexed_surface_is_current` = the owner's `parse_env_hash` still equals
its live parse environment. Nothing on the artifact is dependency-set
derived, so no route-resolution mutation can stale it; a moved parse
environment routes it through the FULL re-materialise, because its
`framework_parse` / `shallow_state` / `decl_bodies` were produced under
the superseded one.

`DerivedRawState.import_routes` is exclusively the CALLER-SUPPLIED
authoritative route table (`set_import_dependencies`). Its keys — the
caller's DECLARED request identities — join the owner's authored
specifiers in the import-route witness inventory, so a caller-pushed
specifier with no authored counterpart is witnessed like any other
resolution (its currency rides the `ExactResolution` facts the same push
installs). The resolved VALUES are never read by the witness builder.

`Route` is a pure PARSE-domain digest: authored specifiers, exported and
original names, type-only-ness, local owners, and the owner's
`whole_hash`. `HostStoreView::build` publishes it observe-only from the
stored artifact and performs no refresh — the interim observer-scoped
re-index C4a introduced is gone. The resolve-domain half of a route
answer — WHICH target each specifier names — rides the import-route
resolution witness:

- a route walk collects a PATH-PRECISE witness through
  `ResolutionWitnessScope`, folding the observations of exactly the edges
  it TRAVERSED into the `RouteDb` / `ImportedRootDb` entry's signature.
  The owner's whole authored inventory is deliberately NOT used there: a
  barrel's inventory is far broader than any route through it, and
  resolving the unvisited siblings would both over-root the entry and
  defeat the walk's path precision;
- the layer-ordered wildcard walk therefore keeps each descendant as an
  unresolved `(owner, source_specifier)` edge and resolves it only when
  the walk actually visits it, so a barrel's later-declared `export *`
  siblings are never resolved (nor, for carrier targets, loaded) when an
  earlier-declared one carries the requested export.

Resolution is SESSION-SCOPED for a session-bound consumer:
`SessionResolverContext::resolve_type_dependency_canonical` resolves
through the session's own overlay. While the artifact baked resolved
targets, a session's overlay-only dependency reached base readers
implicitly — the overlay materialiser resolved it and the baked canonical
travelled with the artifact. With every consumer resolving on demand, a
session-bound consumer that resolved through the base host would make an
overlay-only target disappear.

## Complexity and ownership

- World-root and StoreView capture are O(1).
- Warm lookup is O(candidates × witness facts), with candidate cap 4 and
  signature cap 1,024.
- Cold resolution is O(actual resolver observations).
- Mutation is O(changed facts + boundary-correct recovery scopes), never
  reverse-dependent importer fan-out.
- `ResolvedImportFactsDb` remains the sole owner/specifier resolution
  authority and uses `ValidatedFactCache`; `RouteDb` remains the route
  authority.
- Its candidates root on the owner's own `FileWholeHash` PLUS the owner's
  import-route resolution witness. The bundle's values are resolved
  canonicals, so the owner's bytes alone are not a validity oracle: a
  dependency appearing, or a higher-priority candidate appearing beside an
  already-resolving one, retargets a clause while the bytes stay put. An
  owner whose witness is unrootable admits nothing.
- `IndexedReady` and `ShallowFileState` are parse/index artifacts and own no
  resolution currency.

## Zero-work counter set — what is measured, and what is still owed

The architecture decision names four counters that must read zero at every
tested workspace size. Their landed status is:

| Counter | Status |
|---|---|
| `indexed_ready_materializes` | LIVE producer, asserted zero by `store_view_marginal_admit_tests` |
| `import_resolution_cache_misses` | LIVE producer, asserted zero by the same gates |
| `indexed_ready_edge_refreshes` | DELETED — see below |
| `store_view_owner_visits` | NOT IMPLEMENTED — owed by the O(1)-build follow-on |

`indexed_ready_edge_refreshes` measured the route-only edge-refresh
materialise lane. C4b deleted that lane (`refresh_indexed_route_surface`,
`build_indexed_route_surface`, the edge-refresh gate seam), so the counter had
no producer left and every assertion on it was a tautology: it read zero
because nothing could ever bump it, not because the builder did no work. The
counter is deleted with the lane rather than kept as a green-by-construction
gate. The two surviving legs carry an explicit anti-vacuity control
(`the_measured_counters_move_when_the_work_actually_runs`) proving both DO move
when the work runs, so their zero assertions discriminate.

`store_view_owner_visits` does not exist in the tree, and the property it would
measure is NOT yet held: `HostStoreView::build` still walks
`indexed().snapshot_all()` per published owner. The walk is now observe-only —
it performs no resolution, no materialisation and no route refresh, which is
what the three live counters pin — but it is still O(published owners), so the
counter would read non-zero. Making the build O(1) in published-owner count,
and instrumenting owner enumeration directly, is the remaining half of the
observe-only store-view work and owns this counter. Recording the gap here is
deliberate: the alternative is an architecture decision whose required counter
set is silently unmet.

## The named follow-on block

Five things are deliberately NOT in this tree. They are one block, gated on
measurement, and listed here so none of them reads as an oversight:

1. **Directory-grouped absence revalidation.** Group a witness's `Absent`
   probes by parent directory (pure string work) and issue ONE live `readdir`
   per distinct parent, comparing membership against the probed basenames: a
   ~32-path bare-specifier miss under a few `node_modules` ancestors becomes a
   handful of readdirs instead of ~64 syscalls. It must NOT recheck absence
   through the directory index — that index is event-maintained, i.e. the same
   snapshot-blindness one level down. Positive limbs stay per-path (live
   `canonicalize`; full live read plus byte-compare for manifests, matching the
   freeze bridge). An mtime/len pre-filter is a legitimate later optimisation
   but must not be the default, since mtime-preserving rewrites exist. No new
   types: the positive/negative asymmetry is a refresh-time grouping strategy.
2. **One evidence ledger under one lock.** Consolidate
   `pending_resolution_refresh` + `evidence_verified_generation` + the
   baselines. This is the STRUCTURAL answer to the ABBA deadlock the ledgers
   produced: that deadlock was fixed by comment discipline, and
   comment-enforced lock ordering between two ledgers and a world gate is how
   it wedged for fourteen minutes in the first place. One ledger, one lock,
   world gate entered only on a real value change — the entire ordering class
   disappears instead of being documented.
3. **O(1) `HostStoreView::build`** in published-owner count, plus the
   `store_view_owner_visits` counter it owns (see the counter table above).
4. **The marginal-admit contract question** — whether the ratified zero-work
   gate's measurement window should exclude the newly-admitted file's own cold
   build. Recorded as OPEN and owned by the O(1)-build block; the ratified
   discriminator is not reclassified as mis-specified in the meantime.
5. **Witness memoisation** for the per-demand witness inventory.

## Recorded adjudications

Three rulings changed this contract's execution while it was being implemented.
All are recorded here because the reasoning is load-bearing for anyone reading
the resulting tree, and because one of them authorises an edit that the C0
freeze otherwise forbids.

### Re-ratification of reader-driven re-observation

The re-observation mechanism failed two consecutive adversarial review rounds
and was re-adjudicated under the second-REOPEN circuit breaker. The mechanism
STANDS — it is in the normative contract above, and shipping it as originally
built was worse than deleting it, which is a different judgement from the
mechanism being wrong.

The root cause named by that ruling is the one this document now states as a
rule: **the branch grew a SECOND evidence-read path beside the freeze bridge
instead of reusing it.** Every defect the two rounds found — a snapshot
read-through, a double read, a stamp certifying canonicals it never read, a
fold that recorded no manifest baseline, a fill that republished the world —
follows from that divergence, and the fact that the two paths HAD already
drifted is the argument that two primitives is one too many. The ratified end
state is exactly one live-observation primitive for resolution evidence, shared
by freeze, refresh, and any future driver; the trigger (first reuse of a
retained candidate after the content-generation tick, before candidate
selection, over the union of the slot's witness path canonicals not yet
live-read at this tick, plus the pending-ledger channel) was found correct and
is unchanged.

### The C5 split, and why C4 cannot come first

The architecture decision's implementation order runs C4 (atomic authority
cutover) before C5 (observe-only StoreView). That order is not achievable, for
a reason internal to the fact rail rather than to scheduling:

- `HostStoreView::validates_resolve_imports_domain` destructured
  `ResolveImportsFactRef::Semantic { .. }` with `else { return false }`, so the
  `Resolution(..)` arm could never validate;
- `StoreViewSnapshot` captured no resolution world root at all;
- `CapturedResolutionWorld` — the only `FactVersionValidator` for that arm —
  was `pub(crate)` and unexported, and `AdmittedResolution` exposed only its
  result, so a consumer could not reach the per-resolution `ReadSetSignature`.

C4 requires the import-route value to become a resolve-domain fact sourced from
the resolved owner-edge surface. To be a correct successor to the legacy
`DerivedFactKind::ImportRoute` it must shift when a known-miss specifier becomes
resolvable — and rooting it on `FileWholeHash` plus producer-side supersession
cannot, because a dependency appearing while the owner's bytes are unchanged
fires neither. Rooting it on `ResolveImportsFactRef::Resolution(..)` is the
answer, and that needs the StoreView root capture C5 owns. Consulting the live
registry instead is forbidden by the world-publication protocol; the
reverse-dependent push alternative is forbidden by the retention contract.

So C5 splits and the order becomes **C5a → C4 → C5b**:

- **C5a — resolution-root capture.** `StoreViewSnapshot` captures the immutable
  published `Arc<ResolutionWorldRoot>` in O(1); the `Resolution(..)` arm
  validates against that capture; the `verter_workspace` surface that requires
  is exported. Validation is against the captured root ONLY, never a live
  mutable registry, and a non-current root returns typed `ViewSuperseded`.
- **C4 — atomic authority cutover**, scope unchanged: re-home the import-route
  value onto the resolve-domain surface, switch `ResolvedImportFacts` rooting
  onto the `Resolution(..)` witness, then land the deletions in one
  compile-breaking cutover.
- **C5b — observe-only StoreView**, the remainder: remove the published-artifact
  and tracked-owner scans, make `HostStoreView::build` O(1) in published-owner
  count, and un-ignore the counter gates.

C5b keeps ownership of un-ignoring the counter gates. An earlier stage that
incidentally makes one passable must NOT un-ignore it, because C5b owns proving
it is green for the right reason.

### The concurrency fixture's `resolution_event_bridge_complete` override

`crates/verter_workspace/src/resolution_concurrency_contract_tests.rs` is inside
the C0 frozen range, whose editing envelope is "remove `#[ignore]` only". Its
`ConcurrentReader` fixture nevertheless gained a `probe_path` implementation and
a `resolution_event_bridge_complete() -> true` override.

That edit **stands**, deliberately and on the record. Measured: with the
override removed, all six frozen concurrency gates pass **even with the
mixed-world defect planted** — without it every resolution is
`NonCacheable(UntrackedBackend)` and the mixed-world admission assertion is
never reached. The override is the only thing making those gates discriminate.
Reverting it would leave six frozen gates vacuous, which is strictly worse than
the envelope breach: the freeze exists to stop tests being weakened, and this
edit is what keeps them strong. The `probe_path` half is compelled by C1
replacing the boolean `file_exists` seam with the typed probe, which the C0
contract document itself anticipates and the fixture could not have written in
advance.

The companion claim that
`resolution_concurrency_mutation_during_exact_table_lookup_never_admits_mixed_world`
never reaches its assertion is **false**. `Engine::new` publishes a root, so
`selected_context_for_path` returns the unowned context rather than
`NoPublishedRoot`. Instrumented: outcome `RetryEquivalent`, admission
`Cacheable` with captured == validated == new world, result matching the
oracle, assertion reached. Planting `resolution_world_still_current -> true`
turns all six cases red, that one included.

No further edits to C0-frozen files are authorised without a fresh ruling.
