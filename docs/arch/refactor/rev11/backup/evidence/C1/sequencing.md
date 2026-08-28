# C1 phase-4/7 cutover — implementation sequencing record

Authority: F17 (`docs/arch/refactor/rev11/evidence/C1/f17-deviation-consult.md`)
— "the next safely startable slice is a dedicated implementation-sequencing
record for the atomic phase-4/7 cutover... Ratification of the resulting
sequencing record is required BEFORE production conversion begins."
F18 (`docs/arch/refactor/rev11/evidence/C1/f18-deviation-consult.md`) was a
first ratification pass: items 1/2 corrected, items 4/6 settled, item 5
substantially advanced with one missing consumer found; item 3 left open
pending its own follow-up. **F19
(`docs/arch/refactor/rev11/evidence/C1/f19-deviation-consult.md`) closed
that follow-up and RATIFIED item 3 with corrections.** All six items are
now ratified/settled. **This document, taken as a whole, is now sufficient
to authorize STARTING the production `ProjectResolver` ->
`ModuleResolverCore` conversion** — F19's own explicit verdict.

**F19's named first implementation step is DONE**: the scalar
`ResolutionBasis` is replaced with the exact structured basis; the
semantic-owned `ResolverAttemptView`, `CompletedAttempt<T>`, and
`KernelAttempt<T>` are landed with full TDD coverage (base/session basis
changes, unsupported capabilities, each loader mapping — see items 3/4
below); item 4's `priority_frontier` combinator is also landed with all
six required scenarios covered; item 6's two ratified witness-contract
cases (plus a basis-change-restart follow-on) are landed in the real
dual-runner harness — item 6's SMALL follow-on work is now exhausted.

**F21 (`docs/arch/refactor/rev11/evidence/C1/f21-deviation-consult.md`)
scoped the REAL atomic cutover (items 1/2/5) that comes next.** Bottom
line: "The Cargo-edge deletion is part of the final atomic production
cutover. A transitional `verter_workspace::ProjectResolver` forwarding
wrapper is neither compliant nor technically useful. Preparatory changes
may land first, but every landed state must have exactly one production
module-resolution authority." Item 1's inventory is refreshed below
(stale AGAIN — this session's own `ResolutionBasis` work added three new
references). **The named FIRST next implementation step is NOT the
resolver algorithm — it is the complete C1-AC-8 `RouteAnalysisInputs`
conversion** (`analysis/routes.rs`'s six `&dyn WorkspaceRead` parameters,
unrelated to this session's resolver-core work, already a known/named
acceptance ID) — a genuine, independently-landable production conversion
that reduces the final cutover's remaining surface without touching the
sole resolver authority. **Flagged, not fixed this round**: the
authority-registry.toml's `C1-CHARTER` digest pin (`ff25fdce...`) is
stale against the charter's current bytes (`0367b175...`, independently
verified) — traced to trunk commit `4107210c7`'s scope-neutral
predecessor-status prose edit, confirmed via `git show`. A mechanical
re-pin, not a re-ratification, but sits at the implementer/program-
orchestrator authority boundary — left for the orchestrator/maintainer.

Context this builds on: F4/F5 (scoping-spec.md §0), F12 (the phase-4/7 SCC
finding), F15 (the inert `path_probe`/`real_path`/`package_manifest`
slice), F16 (`AttemptOutput`'s design), F17 (this record's own charter),
F18 (the first ratification pass), F19 (item 3's follow-up, RATIFIED),
F20 (item 6's harness home), F21 (the real atomic-cutover scoping), the
disposition table's Part I (the witness-retention contract).

## Item 1 — every remaining production `verter_semantic -> verter_workspace` reference

**Status: STALE AGAIN, REFRESHED (F21); C1-AC-8 CLOSED, RE-INVENTORIED.**
F18's 5-name/25-line snapshot predates this session's `ResolutionBasis`
restructuring, which added three new direct references. F21's refreshed
crate-wide count: 40 production lines, 8 distinct types/traits including
`WorkspaceRead` (`analysis/routes.rs`'s six `&dyn WorkspaceRead`
parameters, C1-AC-8, then still outstanding).

**C1-AC-8 is now CLOSED** (landed this round, `crates/verter_semantic/
src/analysis/routes.rs` + a new `verter_session::route_analysis_inputs`
caller-side walker + repointed `verter_lsp`/`verter_mcp` call sites — see
item 5's redirected first step, below, for the full design). Re-run
crate-wide inventory confirms: `WorkspaceRead` no longer appears as an
actual type reference anywhere in production `verter_semantic` source —
only doc-comment prose mentions remain (grep-verified: every non-doc-
comment `verter_workspace::` line inspected by hand). Current count: 49
production lines, still 8 distinct types/traits — `WorkspaceRead` is
GONE, but `verter_workspace::error::DirEntry` (plain data, matching the
established dependency-neutral-value precedent set by `PathProbe`/
`AmbientSymbolHit`) is NEW in `RouteAnalysisInputs`'s own fields. Net:
`FactVersionRef`, `ProjectStableKey`, `AmbientSymbolHit`, `PathProbe`,
`WorkspaceAuthorityId`, `ResolutionPopulation`, `ResolutionWorldId`,
`DirEntry` — plus the resolver re-export shim (23 symbols) and the
fact-registry wildcard shim (16 symbols). Two Cargo entries (production
`[dependencies]` + `test-support`-gated `[dev-dependencies]`).

**F21 clarifications**: `SessionFingerprint` is transitive through
`ResolutionPopulation`, not a direct reference — no separate disposition
row needed. Workspace `PackageManifest` is NOT a current production
reference (the observation API already uses semantic-owned
`ResolutionPackageManifest`; only prose comments mention the workspace
name). `CapturedResolutionWorld` need not relocate — it can stay sealed
and workspace-owned while `verter_semantic` projects exact semantic-owned
basis/replay DTOs from it (matches how `StoreViewValidationToken` already
works: the concrete session-side capture type stays where its
`&VerterHost` dependency lives, only the dependency-neutral comparison
value crosses).

**Superseded status text (F18, kept for history)**: 25 production lines,
5 distinct type/trait names + 2 whole-file re-export shims —
independently re-confirmed by F18's own `rg` re-run, with no crate alias
/ `extern crate` / renamed dependency / macro import found hiding another
reference.

| Site | What it names | Disposition |
|---|---|---|
| `resolver_core/attempt_output.rs` (3 sites) | `verter_workspace::fact_cache::FactVersionRef` | **F18 correction**: dependency-neutral as a VALUE, but NOT a one-type relocation — embeds `ResolveImportsFactRef::Resolution(ResolutionFactRef)`, whose closure includes `ResolutionFactKey`, population/query identities, and resolver request enums. Needs an explicit move/split row in item 5 (not yet written), not a bare "relocate it." |
| `resolver_core/observation.rs:93,95` (`lookup_ambient_symbol`) | `verter_workspace::ProjectStableKey`, `verter_workspace::AmbientSymbolHit` | **F18 correction**: `AmbientSymbolHit` genuinely plain data, no correction needed. `ProjectStableKey` is plain data AS A TYPE, but its `from_project` CONSTRUCTOR depends on workspace-owned `OwnershipProject`/`ProjectPayload` — move the enum/value operations; keep the ownership-derived constructor workspace-side. |
| `resolver_core/observation.rs:186` (`path_probe`) | `verter_workspace::resolution_currency::PathProbe` | Confirmed dependency-neutral (F15). Relocate-vs-mirror decision unchanged by F18. |
| `analysis/routes.rs:196,251,661,672,869,1120` (6 sites) | `&dyn verter_workspace::WorkspaceRead` | C1-AC-8's `routes.rs` -> `RouteAnalysisInputs` item (scoping-spec §4). Still pending, not re-scoped here. |
| `analysis/project_resolver.rs` (whole file) | `pub use verter_workspace::resolver::{...}`, `pub use verter_workspace::types::{...}` | F5's already-dispositioned re-export shim (delete + repoint consumers, including `verter_lsp::project_resolver`). **F18 addendum**: moving `IdeProjectConfig` also moves/canonicalizes the membership/glob type closure currently hidden behind this shim. |
| `facts/registry.rs:3` (whole file) | `pub use verter_workspace::fact_registry::*;` | Scoping-spec §1's ownership-move item (`facts/registry.rs` becomes the OWNER, not a re-export). Not re-scoped here. |

**Conclusion for item 1**: the inventory (5 names, 2 shims) is accurate
and complete. What was WRONG in the prior draft was declaring all five
names "already fully dispositioned" — `ProjectStableKey` and
`FactVersionRef` both need a finer-grained split (value type moves;
constructor/embedded-closure stays or needs its own row) that item 5's
migration table must carry explicitly, not a blanket "relocate."

## Item 2 — the semantic-owned project-resolution DTO/graph shape

**Status: SETTLED SHAPE PROPOSED (F18); COMPLETE OWNED SURFACE DOCUMENTED
(F22, `docs/arch/refactor/rev11/evidence/C1/f22-deviation-consult.md`).**

**F22's headline finding: the three already-landed observation
primitives (`path_probe`/`real_path`/`package_manifest`) are sufficient
for the ENTIRE resolver algorithm — no new `InputKey`, no new
`ResolverObservation` method needed, confirmed against the highest-risk
candidates specifically** (`resolve_node_modules_package_from_dirs`,
`ancestor_dirs*`, `resolve_package_imports`, `resolve_project_references`,
`resolve_existing_path`, `read_package_manifest_if_present`) — every one
of them is either graph-only/pure lexical computation or reaches live
state through exactly those 3 observations. `ancestor_dirs*` and the
node_modules walk are LEXICAL path construction (`{ancestor}/
node_modules/{package}`), never directory enumeration; a backend MAY
internally enumerate while answering a typed probe, but that stays
loader/driver-side evidence, never a kernel `read_dir` demand; ancestor
recovery is already represented outbound by the existing
`ConsumedResolutionObservationKey::RecoveryScope` (kernel-derived,
driver-replayed) — no `InputKey::DirectoryMembers` needed either. This
closes the last open question about whether item 3's landed seam would
need widening before the real port could start — it does not.

F22's full owned-function-surface enumeration (every function reachable
from the four public entry points — `resolve_with_reader`/
`resolve_for_project_with_reader`/`preferred_specifier`/
`project_exact_result` — grouped by concern: entry points; owner/project
selection; relative/absolute; workspace aliases; tsconfig `paths`/
`baseUrl`; project-reference recursion; `#imports`; node_modules/
exports/conditions/legacy; provider-graph/carrier projection;
preferred-specifier reverse mapping; the free helper API) lives in the
F22 evidence file — condensed summary there, full line-numbered table in
the consult transcript. Two load-bearing corrections/additions from that
pass: **`package.json#browser` is NOT supported by the current
algorithm at all** (neither `PackageManifest` nor
`ResolutionPackageManifest` has the field) — the port preserves this
absence as-is; adding browser support is a SEPARATE semantic change
needing its own ruling, never bundled into this port. And: private
`preferred_specifier` is confirmed test-only with no production caller
— DELETE (not move) at cutover; `preferred_specifier_candidates` moves
as the pure candidate generator, Engine keeps round-trip orchestration —
the EXISTING split is correct, unchanged.

**Branch-complete `KernelAttempt`/`AttemptOutput` witness rules (F22, now
explicit)**: every consumed positive or negative path probe is recorded;
`real_path` recorded only after a positive probe; a demanded manifest
records `PackageManifest { directory }`; higher-priority completed
misses remain in the winning/exhausted witness; `NeedInputs`/`Terminal`
discard all partial `AttemptOutput`; same-basis blocked siblings union
through `priority_frontier`; project-reference recursion merges child
output in traversal order; pure owner selection/JSON mapping/provider
projection/preferred-candidate generation add NO observation witness.

`ConfiguredMembership` (`verter_workspace/src/membership.rs:191`) as a
COMPLETED VALUE is portable data — `StaticMembershipSpec` (`files`,
`include: Vec<CompiledGlob>`, `exclude: Arc<[CompiledGlob]>`),
`materialized_files: FxHashSet<CanonicalPath>`; `CompiledGlob`/
`NormalizedGlob` are plain data wrapping a `String` + an owned
`glob::Pattern`; `CanonicalPath` is already `verter_span`-owned
(`verter_workspace/src/canonical_path.rs:9` is itself just a re-export),
so it costs `verter_semantic` NO new dependency. `ConfiguredMembership::
contains` (`membership.rs:248`) is a pure in-memory lookup/glob match.

**F18 correction**: the prior draft wrongly claimed
`materialize_from_spec` was ALSO pure alongside `contains`. It is NOT —
it takes `WorkspaceAccess` and calls `walk()`
(`snapshot_builder.rs:385,413`), a genuine filesystem walk. Corrected
boundary:

```text
workspace snapshot construction
    -> filesystem walk/materialization (materialize_from_spec, IMPURE, stays workspace-side)
        -> completed ConfiguredMembership DTO (portable VALUE, crosses)
            -> pure kernel membership queries (contains, PURE, moves with the kernel)
```

`IdeProjectConfig`'s complete 8-field list (`resolver.rs:76-90`:
`root`, `workspace_root`, `tsconfig_path`, `provider_root`,
`workspace_aliases`, `compiler_options`, `references`, `membership`) is
confirmed complete and handle-free — independently re-confirmed by F18.
The owner-selection call chain (`effective_configs_for_path`/
`nearest_config_for_path`/`project_for_ownership`, `resolver.rs:157-233`)
performs no I/O and consults no live handle (`normalize_canonical_id`
emits audit instrumentation but its answer is still deterministic path
computation).

**F18's recommended settled shape** (proposed, not yet ratified as the
final decision — the whole document awaits item 3's follow-up before
anything here is authorized):

- Make the complete 8-field `IdeProjectConfig` semantic-owned — retaining
  ALL 8 fields matters even though the resolution algorithm itself
  principally reads `base_url`/`paths`, because `provider_root` and the
  non-resolution compiler booleans are used by env hashing, membership
  construction, and other workspace/LSP logic.
- Move the exact membership ENGINE with it; workspace keeps
  `materialize_from_spec` and constructs the completed DTO before handing
  it to the kernel.
- Move `effective_configs_for_path`/`nearest_config_for_path`/
  `project_for_ownership` WITH `ModuleResolverCore` — owner selection is
  needed both before resolving the importer and after finding the
  target; a workspace-side preselection step would split resolution
  authority and introduce round trips.
- Keep carrier-ownership's fail-closed authority (`CarrierOwnershipResolution`)
  separate, unchanged.

Minimal internal compiled-graph sketch (proposed):

```rust
struct ModuleResolverCore {
    configs: Arc<[IdeProjectConfig]>,       // existing sorted precedence
    by_tsconfig: FxHashMap<String, ProjectNodeId>,
    reference_edges: Arc<[Arc<[Option<ProjectNodeId>]>]>,
}
```

Must preserve exactly: reference order and duplicates; unresolved
references as `None`/skipped; first matching config in the existing
sorted order; the current depth-256 and active-path cycle protection; no
new normalization during edge compilation unless independently
characterized.

**F21 verdict on the sketch: storage concept fine, write-up incomplete.**
The 3-field struct is plausibly sufficient PERSISTENT STATE — aliases/
`base_url`/paths/compiler-options live in each config; project
references use the compiled index/edges; package imports/exports and
node_modules walks are request-local algorithms, not stored state;
manifests/probes/realpaths come through `ResolverObservation`; provider
projection computes from the target config. Do NOT add a workspace
reader, manifest cache, filesystem handle, transaction, or package index
to the core. What the write-up currently lacks (a documentation-
completeness gap, not a structural redesign): the full owned API surface
— `resolve_attempt`, `resolve_for_project_attempt`, importer/target owner
selection, the FULL relative/absolute + alias + paths/`baseUrl` +
project-reference + `#imports` + node_modules + legacy-package
fallthrough (not just the dual-runner's narrow `probe_path_for_context`
slice), `preferred_specifier_candidates`, exact-result and
provider-graph projection, carrier/provider path helpers, request/result
and project-selection DTOs, `KernelAttempt`/`AttemptOutput` witness
behavior for EVERY branch, precise semantic ownership for
`IdeProjectConfig`/membership/glob values/env-hash methods and their
mode/condition-set inputs, explicit dispositions for `FactVersionRef`/
resolution-identity DTOs/ambient DTOs/`PathProbe`, and canonical public
module paths plus an explicit decision to DELETE `NativeProjectResolver`
(not preserve it as a compatibility alias).

## Item 3 — `ResolverAttemptView`, `KernelAttempt<T>`, workspace replay of consumed selectors into versioned facts

**Status: RATIFIED (F19); IMPLEMENTED.** `ResolutionBasis`/
`ResolutionWorldBasis` restructured to the exact structured shape
(`crates/verter_semantic/src/resolver_core/attempt_outcome.rs`);
`InputKey::DeclBody` gained the `DeclarationSpace` field;
`CompletedAttempt<T>`/`KernelAttempt<T>` landed;
`AttemptFailure::ObservationUnavailable { observation:
ResolverObservationKind }` landed; `ResolverAttemptView` (`resolver_core::
resolver_attempt_view.rs`) landed as the sealed trait's one
closure-capability implementor, with `workspace_only(...)` populating
exactly the three required primitives. All inert — no production driver
wires real closures in yet; that is the atomic-cutover migration (item 5)
plus the not-yet-built workspace replay ledger's own follow-on work. The
replay design below (`WorkspaceResolutionReplayLedger`) remains a
proposal, not yet built.

`ResolverAttemptView` must be the ONE universal semantic-owned implementor
of all 13 `ResolverObservation` methods — NOT a resolver-scoped
implementor that panics on the other 10 (F18: "unacceptable"), and NOT one
that returns fabricated defaults (also "unacceptable"). Design direction:

- Eager immutable inputs (env hashes, project identities, package-backed
  classification, ambient index, project generation, basis/configuration)
  vs. keyed loadable slots (whole hashes, decl bodies, augmentation index,
  flow skeletons, path probes, realpaths, manifests).
- Missing keyed slots return the exact `NeedInputs(InputKey)`.
- Workspace and session drivers populate DIFFERENT subsets, but BOTH
  construct the SAME semantic type.
- A driver receiving a request for a key outside its own scope returns a
  typed `InputLoadUnavailable` — the observation method never panics.

`CompletedAttempt<T>`/`KernelAttempt<T>` confirmed as exactly F16's shape
(`CompletedAttempt<T> { value: T, output: AttemptOutput }`,
`KernelAttempt<T> = AttemptOutcome<CompletedAttempt<T>>`); one fresh
`AttemptOutput` per attempt; no output publication on `NeedInputs`/
`Terminal`.

### Replay design (F18)

**Do NOT add `ResolutionPopulation` to `ConsumedResolutionObservationKey`**
(closes an open question F16's own evidence file left dangling) —
population is not derivable from the selector alone, but is already owned
by the exact captured world/`ResolutionTransaction` used for that attempt
(`ResolutionTransaction::population`, `resolution_currency.rs:2695`); the
workspace-side replay reads it from there.

Proposed workspace-owned companion structure (not yet built):

```rust
struct WorkspaceResolutionReplayLedger {
    path_probes: HashMap<CanonicalId, PathProbeReplay>,
    realpaths: HashMap<CanonicalId, RealPathReplay>,
    manifests: HashMap<CanonicalId, ManifestReplay>,
}
```

On `Complete`, while still bound to the same view/basis/captured
world/transaction, replay the output exhaustively:
`PathProbe { path }` -> `ResolutionFactKey::PathProbe { canonical, population }`;
`RealPath { path }` -> `Realpath { requested, population }`;
`PackageManifest { directory }` -> join `package.json`, then
`Manifest { canonical, population }`;
`RecoveryScope { canonical_prefix }` -> `RecoveryScope { canonical_prefix, population }`.
The ledger must preserve RICHER workspace evidence than the semantic
projection: probe outcome, resolved realpath, manifest fingerprint,
backend-emitted directory observations associated with that specific
load.

**Concrete gap found, F18**: `ResolutionPackageManifest`
(`module_resolution_observation.rs`, landed F15) omits `name`, but
`manifest_fingerprint_of` (`resolution_currency.rs:2217`) includes `name`
in its fingerprint computation — the fingerprint CANNOT be reconstructed
from the narrow kernel DTO alone. This is NOT a defect in what's already
landed — F15 deliberately narrowed `ResolutionPackageManifest` to exactly
the fields `resolver.rs`'s resolution ALGORITHM reads (confirmed by grep,
still correct). It means the NOT-yet-built workspace replay ledger must
retain the fingerprint computed from the FULL manifest it already has,
separately from the narrow kernel-facing projection — a design note for
whoever builds `WorkspaceResolutionReplayLedger`, not a code change owed
right now.

`DirectoryMembers` stays workspace-only ancillary replay evidence — NOT a
5th `ConsumedResolutionObservationKey` variant. Replay it only when its
associated primitive selector was actually consumed. The existing
transaction methods implicitly add recovery chains; exact output-led
replay needs granular transaction methods so `RecoveryScope` replays from
its own explicit output variant rather than being silently added by
`observe_path`/`observe_realpath`.

### Gap 1, resolved (F19) — the 13-method field/source matrix

`ProjectResolver`'s complete algorithm reaches `WorkspaceRead` only
through `probe_path`/`realpath`/`read_package_manifest`
(`resolver.rs:1193,1251,1654`) — mapping exactly to `path_probe`/
`real_path`/`package_manifest`. **Correction to an earlier framing**:
this does NOT mean two separate trait implementors (one workspace-only,
one session-only) — `ResolverObservation` is SEALED inside
`verter_semantic`, so the only production implementor can be the ONE
semantic-owned `ResolverAttemptView`; workspace and session are
BUILDERS/DRIVERS that populate DIFFERENT CAPABILITIES on that SAME type.

| Observation | Workspace-only capability | Full session capability | Missing/load behavior |
|---|---|---|---|
| `env_hashes` | Derivable from captured `PublishedRoot`/project graph; unused by `ModuleResolverCore` | Captured `ProjectEnvRoot`, per-canonical project selection | Immediate; no `InputKey` |
| `project_identity` | Same captured project graph/table; unused by the resolver | Captured `ProjectEnvRoot`/published project-identity table | Immediate; no `InputKey` |
| `whole_hash` | Unsupported in a workspace-only attempt | Sealed `HostStoreView::whole_hash` state | `InputKey::FileContent` |
| `workspace_is_package_backed` | Derivable from project roots + loaded realpath; unused by the resolver | Same derivation from the captured project graph + realpath observation | Propagate `RealPath` when needed; no independent loader |
| `lookup_ambient_symbol` | May carry the immutable ambient-index snapshot; unused by the resolver | Captured ambient index | Immediate; no `InputKey` |
| `project_generation` | Unsupported — `WorkspaceSnapshot::generation` is NOT `ProjectTypeStore::project_generation` | Captured `ProjectEnvRoot.project_generation` | Immediate; typed observation-unavailable outside session |
| `type_decl`/`value_decl` | Unsupported | `DeclBodyMemo::peek_type_decl`/`peek_value_decl` | `InputKey::DeclBody` |
| `module_augmentation_index` | Unsupported | Captured `FileArtifactStore` root / `get_augmenter_set` | `InputKey::ModuleAugmentationIndex` |
| `function_body_skeleton` | Unsupported | `FlowSliceStores::peek_skeleton_for` | `InputKey::FlowFunctionSkeleton` |
| `path_probe`/`real_path`/`package_manifest` | REQUIRED — workspace observation map | Same workspace-backed map under the session population | `InputKey::PathProbe`/`RealPath`/`PackageManifest` |

The 5 immediate-value observations need explicit `Available`/`Unsupported`
state, never a default. Keyed slots need THREE states: `Unloaded`,
`Loaded(value, including a stable None)`, `Unsupported`.
`AttemptFailure::InputLoadUnavailable { key: InputKey }` cannot represent
an unavailable NON-KEYED method (e.g. `project_generation` has no
`InputKey`) — needs a typed `ObservationUnavailable { observation:
ResolverObservationKind }` (or equivalent widening of the failure enum).

**A genuine correction owed to already-landed code**: `InputKey::DeclBody
{ canonical, owner, name }` (round 4a) does not say type-space vs.
value-space, so the retry driver cannot recover which observation
(`type_decl` vs. `value_decl`) produced a given key. Fix: add a
semantic-owned `DeclarationSpace::{Type, Value}` field to `InputKey::
DeclBody` (preferred — better demand precision) rather than defining one
load as populating both spaces. This is its own small, additive
implementation unit (touches `type_decl`/`value_decl`'s `NeedInputs` arms
too), not bundled into the `ResolutionBasis`/`ResolverAttemptView` work.

### Gap 2, resolved (F19) — the `ResolutionBasis` minting recipe

**Rejected**: folding `ResolutionWorldId` + population into a scalar
`u64`. The `AggregateStamp` precedent (`fact_cache.rs:81`) deliberately
chooses exact tuples over digests, and the same reasoning applies here.

**Ratified structured recipe**:

```rust
struct ResolutionWorldBasis {
    workspace_authority: WorkspaceAuthorityId,
    population: ResolutionPopulationIdentity,
    base: ResolutionWorldId,
    session: Option<ResolutionWorldId>,
}

struct ResolutionBasis {
    resolution_world: ResolutionWorldBasis,
    session_view: Option<StoreViewValidationToken>,
}
```

Rules: a workspace-only attempt has `session_view = None`; a full session
attempt carries the EXACT `StoreViewValidationToken` (never its folded
`external_supersession_fingerprint()`); base population = exact base root
only; session population = exact base root + exact session root + session
population/fingerprint; mint from the SAME `Arc<CapturedResolutionWorld>`
used by the `ResolutionTransaction`/loader commit fence/replay; never
hash/fold into `u64`; the production `ResolutionBasis::new(u64)` path is
REPLACED/REMOVED (`new(0)` stays test-only synthetic vocabulary at most,
matching the already-documented PROVISIONAL status every landed method's
`NeedInputs` arm uses today). `workspace_authority: WorkspaceAuthorityId`
is required because the world-ID counter restarts per-`Engine` (root IDs
are unique within an engine, not across engines); generalize/reuse the
existing `strict_self_root_authority_id` concept rather than inventing a
new hash. No separate `resolve_env_hash`/policy hash needs folding in
separately, PROVIDED `ModuleResolverCore`, its configuration graph, and
the basis all come from the SAME captured `PublishedRoot` (publishing
that root always remints the base world ID; its `ResolveContextId`
already contains project identity, resolver policy, provider policy, and
resolve-env identity — `resolution_currency.rs:143`).

**Correction to F18's own note**: confirmed (not newly discovered) —
`ResolutionWorldRoot` retains path probes and realpaths in full, but only
MANIFEST FINGERPRINTS, not full manifest contents — the package-manifest
loader must retain the full manifest separately for the narrow kernel
projection + replay fingerprint, as F18 already flagged.

### Closure

Item 3 is ratified with these corrections recorded normatively: one
semantic-owned `ResolverAttemptView` with driver-specific capability
population; typed unavailable state for non-keyed observations; exact
loader ownership for all 7 `InputKey` variants; declaration-space-exact
`DeclBody` loading; the exact structured basis (workspace authority +
population + base/session roots + full session validation token); no
scalar fold, no `ResolutionBasis::new(0)` in production; one fresh
`AttemptOutput` per attempt, output published only on `Complete`. These
are now IMPLEMENTATION obligations, not open architecture questions.

## Item 4 — same-basis `LoadSet` union, terminal precedence, attempt-output discard rules (the priority-frontier combinator)

**Status: SETTLED (F18); IMPLEMENTED.**
`crates/verter_semantic/src/resolver_core/priority_frontier.rs`. A
REUSABLE PRIVATE (`pub(crate)`) helper in `verter_semantic::resolver_core`
— not a new public abstraction, not manually repeated at every
fallthrough site. The existing outcome types already encode the needed
states: `Complete(Some(T))` = hit; `Complete(None)` = exhausted miss;
`NeedInputs(LoadSet)` = blocked; `Terminal` = the exceptional exit.

**Implementation refinement over F18's proposed shape**: the signature
below writes directly into a shared `&mut AttemptOutput`, which would
need an output-rollback mechanism `AttemptOutput` deliberately has no
public API for (F16: private fields, no public struct literal). Landed
instead as `evaluate: impl FnMut(C) -> KernelAttempt<Option<T>>` — reusing
item 3's `CompletedAttempt`/`KernelAttempt` envelope so each candidate's
`Complete` carries its own fresh output and `NeedInputs`/`Terminal`
structurally carry none, making the discard rule a type-level guarantee
rather than a runtime rollback. The ten rules themselves are unchanged.
F18's original proposed shape, superseded:

```rust
fn priority_frontier<C, T>(
    expected_basis: ResolutionBasis,
    candidates: impl IntoIterator<Item = C>,
    mut evaluate: impl FnMut(C, &mut AttemptOutput) -> AttemptOutcome<Option<T>>,
    output: &mut AttemptOutput,
) -> AttemptOutcome<Option<T>>;
```

Required semantics (load-bearing specification for the implementer):

- Before any block, merge completed-miss outputs in candidate order.
- On a hit before a block, merge its output and return the hit.
- On the FIRST block, retain ONLY its `LoadSet` — do not publish
  accumulated output.
- Continue through bounded siblings to union further same-basis missing
  keys.
- A known lower-priority hit AFTER a higher block cannot win — stop and
  return the blocked set.
- A terminal before any block propagates.
- A terminal encountered only speculatively after a higher-priority block
  does NOT outrank that block — return the blocked set, reconsider on
  retry.
- A basis mismatch is NOT unioned or loaded — return the mismatching
  `LoadSet`; the outer driver detects the mismatch and restarts under the
  new basis.
- Every `NeedInputs`/`Terminal` path discards ALL branch/frontier output.
- An exhausted miss publishes the COMPLETE ordered rejected-candidate
  witness (matches Part I's witness-retention contract exactly).

TDD coverage landed for all six scenarios (`priority_frontier_tests.rs`):
same-basis union, lower hit after higher block, terminal before/after
block, output discard (a type-level guarantee, tested for regression),
basis mismatch, exact miss ordering — plus hit-before-block merging prior
misses' output. `union_load_sets` and `priority_frontier` are
`#[allow(dead_code)]` pending the not-yet-ported resolver algorithm that
will call them (matches the `decl_body_memo::peek_type_decl` precedent).

## Item 5 — the atomic migration/deletion table

**Status: SUBSTANTIALLY ADVANCED (F18), still open.** Confirmed item 5
does NOT need items 2-4's rulings to INVENTORY current callers — only to
finalize DESTINATION symbol names. Concrete table (F18):

| Current surface | Production consumers | Migration |
|---|---|---|
| `resolve_with_reader` | `resolve_tracked`; private `preferred_specifier` | Move algorithm to `ModuleResolverCore::resolve_attempt`; delete raw method |
| `resolve_for_project_with_reader` | `resolve_for_project_tracked` only | Move to `resolve_for_project_attempt`; delete raw method |
| `resolve_tracked` | `engine.rs:3330`, `engine.rs:3857` | Workspace retry/replay adapter drives kernel |
| `resolve_for_project_tracked` | `engine.rs:3633` | Same adapter, explicit-project entry |
| Private `preferred_specifier` | No non-test caller; test-only | Delete after tests are parameterized |
| `preferred_specifier_candidates` | `Engine::preferred_specifier` (`engine.rs:3790`) | Move pure candidate generation with core; Engine retains round-trip orchestration |
| `project_exact_result` | `engine.rs:3298`, `engine.rs:3921` | Move pure result projection with core |
| `WorkspaceSnapshot.resolver` | Six `Engine` uses plus `resolution_currency.rs:1423` | Retype to `ModuleResolverCore` |
| Resolver construction | `snapshot_builder.rs` ×3, `ProjectGraph::to_project_resolver`, `Engine::rebuild_and_publish` | Construct semantic config/graph once |
| LSP shim | `crates/verter_lsp/src/project_resolver.rs` + its LSP consumers | Repoint to the real semantic owner |
| N-API/WASM analysis helpers | Existing `verter_semantic::analysis::project_resolver` paths | Can remain unchanged if that module becomes the canonical owner |
| TSC helper calls | `checker.rs:1861,1930` | Repoint `is_relative_specifier` to semantic |
| Direct session/LSP workspace resolver helpers | Session path-helper files + LSP config/provider/TSGO files (scoping-spec §2's table) | Per-symbol relocation/repoint |
| Workspace resolver module/re-exports | `verter_workspace/src/lib.rs`, `resolver.rs` | Delete after all callers move |
| Fact/membership carrier closure | `FactVersionRef`, resolution fact keys, membership/glob DTOs | Explicit move/re-export rows; not a one-type operation |

**Missing consumer found, F18**: `resolution_currency::
evaluate_selected_context` calls `nearest_config_for_path` directly — the
original snapshot list missed it.

**F22 re-grep against the current tree — the table above was accurate
where it had rows, but MATERIALLY UNDERCOUNTED real consumers**
(`docs/arch/refactor/rev11/evidence/C1/f22-deviation-consult.md` has the
full line-numbered lists; condensed here):

- **`WorkspaceSnapshot.resolver`**: the six `Engine` sites remain
  (`engine.rs:3298,3330,3633,3790,3857,3921`; the resolution-currency
  lookup moved to `resolution_currency.rs:1500`), but the table MISSED
  the LSP's direct field borrows/clones across NINE files:
  `server_utils.rs`, `background_drain.rs`, `workspace_scanner.rs`,
  `sync_coordinator.rs`, `background_drain_decl_closure.rs`,
  `server/provider_state.rs`, `background_drain_owner_loss.rs`,
  `server/sync_orchestration.rs` — mandatory retyping/repointing sites,
  primarily cloning/passing the resolver into
  `PublishedResolverSnapshot`, provider-sync helpers, carrier-sync
  requests.
- **LSP shim** consumers span more files than recorded: `server/mod.rs`,
  `server_utils.rs`, `config.rs`, `provider_sync.rs`,
  `external_ts/carrier_sync.rs`, `workspace_scanner.rs`,
  `carrier_provider_projection.rs`, `server/sync_orchestration.rs`.
  Several `server_utils` resolver parameters are intentionally UNUSED
  (named `_resolver`) — delete with their call-site arguments, do not
  mechanically retype.
- **N-API/WASM/session DTO consumers MISSED**: "can remain unchanged" is
  correct only for the two real analysis functions (N-API
  `lib.rs:2102,2124`; WASM `lib.rs:640,667`) — it missed DTO consumers
  relying on the shim's re-exports: `verter_napi/src/meta.rs`,
  `verter_napi/src/lib.rs` (3 sites), `verter_session/src/
  component_meta_host.rs`, `verter_session/src/host_lifecycle.rs`,
  `verter_session/src/meta.rs`.
- **Direct path/carrier-helper consumers** (`is_relative_specifier`,
  `collapse_path`, `normalize_canonical_id`, `path_is_carrier`,
  `carrier_ide_provider_path`, `carrier_api_provider_path`,
  `carrier_source_extensions`, `strip_carrier_extension`) each have
  their own multi-file consumer lists across `verter_session`,
  `verter_lsp`, `verter_mcp`, `verter_tsc` (full detail in F22).
- **Value/type closure needs EXPLICIT rows**: the core-facing DTO
  closure (`ProjectOwnership`, `ResolveRequestKind`, `ResolvePhase`,
  `ResolutionContext`, `ProviderTarget`, `ResolutionKind`,
  `ResolveRequest`, `ResolveResult`); the project/config closure
  (`WorkspaceAlias`, `IdeProjectCompilerOptions`, `IdeProjectConfig`,
  `ConfiguredMembership`, `StaticMembershipSpec`, `CompiledGlob`,
  `NormalizedGlob` — `ProjectMembership` STAYS workspace-owned, a
  config-ingress type not core state, but stops being re-exported
  through semantic analysis); the ENV-HASH closure
  (`IdeProjectConfig`'s four env-hash methods + `project_identity`,
  `EnvHashInputs`, `ModuleResolutionMode`, `ConditionSet` — these must
  move WITH the hash methods or to a lower dependency-neutral owner,
  since semantic cannot keep depending on workspace after the cutover);
  `SpecifierKind` needs an explicit disposition at the same time.
- **Tests/bridge/Cargo/guards, itemized precisely for the first time**:
  `resolver_tests.rs` (now 3,929 lines — move/parameterize around
  attempt views); `resolution_witness_contract_tests.rs` (PRESERVE as
  public-boundary characterization); `resolution_dual_runner_tests.rs`
  (DELETE with the final cutover); `resolver.rs::test_support::
  legacy_resolve_with_reader` (DELETE); `verter_semantic`'s
  `verter_workspace` test-support dev edge (DELETE when the dual runner
  disappears); the `raw_resolver_entry_points_are_private` compile-fail
  fixture (RETARGET to the new private attempt boundary); **the A5-DD1
  exception row + `RATIFIED_ROOT_CRATES` + the semantic→workspace canary
  test in `crates/verter_identity/tests/cases/
  workspace_dependency_layers.rs` (lines 145, 335, 424-430) — ALL must
  be removed or inverted TOGETHER** (a genuinely new finding — this
  architecture-guard file was not previously named anywhere in this
  record); `verter_workspace`'s `pub mod resolver` + resolver re-exports
  at `lib.rs:103,183-188` (DELETE — explicitly NO forwarding
  `ProjectResolver`/`NativeProjectResolver` alias).
- One additional constructor found: `ProjectRegistry::
  to_native_project_resolver` (`verter_lsp/src/config.rs:832-838`),
  currently only called from `test_utils.rs:111`.

Remaining work, mechanical but mandatory (F18: "can be completed by you
in a future investigation round after item 3's names are settled; it does
not require its own architecture consult"): choose and record the
canonical semantic module path (F22: `verter_semantic::resolver_core::
ModuleResolverCore`) + alias policy (F22: NO alias —
`ProjectResolver`/`NativeProjectResolver` are DELETED outright); enumerate
every moved PUBLIC SYMBOL, not just files (F22's condensed lists above
are the starting point, not yet a final per-symbol table); classify
retained non-resolver workspace re-exports (membership/fact carriers);
include tests, benches, compile-fail guards, doc links, Cargo edges,
architecture guards (F22 named the A5-DD1/`RATIFIED_ROOT_CRATES`/canary
cluster specifically); re-run the inventory immediately before the
atomic change (F22's pass is itself a re-run, but the NEXT re-run must
happen immediately before the move, not treated as permanently current).

**F21's two-stage execution model** (item 5 is the atomic-cutover STAGE's
own table; the table above is the CUTOVER-STAGE content, not something to
start piecemeal):

| Stage | Allowed work | Invariant held throughout |
|---|---|---|
| Pre-cutover preparation (now, and future rounds) | Refresh item 1/2/5's inventory; fix the authority digest (flagged above, orchestrator-owned); convert `RouteAnalysisInputs` (C1-AC-8, the redirected first step below); add black-box characterization and migration guards | `verter_workspace::ProjectResolver` remains the ONLY production module resolver |
| Final atomic cutover (one landed state, not staged) | Move the complete algorithm + value closure; wire retry/replay; repoint EVERY caller (the table above); reverse the Cargo edge; delete the old resolver, the `test_support` bridge, and the dual-runner harness; remove the A5-DD1 exception row | `verter_semantic::ModuleResolverCore` becomes the ONLY production module resolver, in one transition |

**Explicitly REJECTED**: a transitional form where `verter_workspace`
delegates through a thin forwarding `ProjectResolver` wrapper around a
newly-built `ModuleResolverCore`. It is a compatibility shim CLAUDE.md
forbids; it leaves two public authorities/names; it produces a Cargo
cycle before the edge is removed; and it serves no purpose after the edge
is removed (callers can and must be repointed directly). "Atomic" means
no landed production state ever has two engines or a superseded wrapper —
not that all documentation, characterization, and unrelated edge cleanup
must land in one working commit.

**F21's named FIRST next implementation step — LANDED.** The complete
C1-AC-8 `RouteAnalysisInputs` conversion is done, in three parts: (1)
`RouteAnalysisInputs` (a plain immutable snapshot — `files`/
`existing_files`/`directories`, matching the four `WorkspaceRead` methods
route analysis used) plus all six `analysis/routes.rs` functions
converted (`crates/verter_semantic/src/analysis/routes.rs`); (2) the
caller-side snapshot walker, `verter_session::route_analysis_inputs::
build_route_analysis_inputs` (mirrors `build_route_analysis`'s own
framework-branching so it walks exactly the directories the pure
analyzer will consult); (3) `verter_lsp`'s `get_route_tree` and
`verter_mcp`'s `scan_project`/`build_route_snapshot` repointed to build a
snapshot first. Every existing route-analysis test assertion preserved
(42 pre-existing + 3 production-behavior tests unchanged); 10 new tests
(7 direct `RouteAnalysisInputs` unit tests, 3 end-to-end walker tests).
Confirmed by re-running the crate-wide inventory: `WorkspaceRead` no
longer appears as an actual type in production `verter_semantic` source
— only doc-comment prose. No text/grep-based structural guard landed for
this (would be a name-keyed file scanner, forbidden as a landed guard by
CLAUDE.md's Stub Prevention rule) — recorded as manually re-verified
here instead, matching the rule's own review-enforced fallback.

This reduces the final cutover's remaining surface. **F22 closed items
2/5's documentation-completeness gap and confirmed the observation seam
needs NO widening before the real port starts** — the algorithm and
observation seam are now scoped enough; item 5's table is far more
complete but still needs the final per-symbol enumeration + a
last re-grep immediately before the actual move (F22's own item 4
verdict). **The next work is the real resolver algorithm/
`ModuleResolverCore` port itself (items 1/2/5's atomic cutover)** — not
another inert kernel slice, not a forwarding resolver. Given the
confirmed scale (a ~2100-line algorithm, dozens of consumer files across
6+ crates, an architecture-guard cluster in `verter_identity` that must
flip in the same change), this remains a task for a dedicated round with
full context budget, not something to start mid-round without room to
finish atomically.

**F23 (Stage-2 sequencing consult) — NO-GO, narrow and specific.** With
item 2's full owned-function-surface enumeration ported (every function
reachable from the real four public entry points), dispatched a fresh
re-grep (found the LSP consumer count is precisely 10 production files,
not 8 or 11 — two prior "8" entries were real production blockers under
different field names, `server/mod.rs`'s `ServerState.resolver` and
`external_ts/carrier_sync.rs`'s `CarrierSyncRequest.resolver`; one new
entry, `background_init.rs:767`, is test-only) plus a Stage-2 sequencing
consult (`docs/arch/refactor/rev11/evidence/C1/f23-deviation-consult.md`
has the full checklist). Verdict: the algorithm port is complete, but
Stage 2 is NOT yet safe to execute. Named gap, in priority order: (1)
no `ModuleResolverCore` struct/shell exists yet — everything ported is
free functions, not a public core type with the real methods; (2) item
6's dual-runner harness covers only the original narrow relative-probe
slice, not full top-level differential coverage against the real legacy
resolver across every branch; (3) comparison scope must widen beyond
final `source_id` to ordered consumed selectors, `NeedInputs` wave
shape, recovery scopes, and replayed `ResolutionFactKey`
set/signature; (4) four specific still-open replay-contract gaps named
in the ledger (manifest fingerprint `name` preservation, `DirectoryMembers`
consumed-vs-prefetched, complete fact replay/signature, basis-restart
on the real driver, no-progress/terminal/transient-load-failure
behavior). F23 also RATIFIED the DTO/Cargo-edge/`verter_identity`-guard-
flip answers for when Stage 2 does execute (recorded in the evidence
file, not repeated here) — those are settled; only the GO decision is
blocked, on the four named Stage-1 gaps above.

**F23 gaps 1/2/3 CLOSED, same round.**

- **Gap 1** — `ModuleResolverCore` (`crates/verter_semantic/src/
  resolver_core/module_resolver_core.rs`) now exists: an immutable,
  precedence-sorted `configs: Arc<[IdeProjectConfig]>` plus the real
  four public surfaces (`resolve_attempt`/`resolve_for_project_attempt`/
  `preferred_specifier_candidates`/`project_exact_result`) and owner
  selection, all delegating to the already-ported free functions. Still
  `pub(crate)`, still test-only reachable. 8 characterization tests.
- **Gaps 2/3** — the dual-runner harness (item 6,
  `resolution_dual_runner_tests.rs`) gained a second generation:
  `run_kernel_core`/`run_kernel_core_for_project` (calling
  `ModuleResolverCore::resolve_attempt`/`resolve_for_project_attempt`,
  the REAL top-level orchestrators, not the narrow probe slice) vs.
  `run_legacy_with_projects`/`run_legacy_for_project` (calling the real
  `ProjectResolver::resolve_with_reader`/`resolve_for_project_with_reader`
  through a new `legacy_resolve_for_project_with_reader` test-support
  bridge, same door discipline as the existing one). 12 new
  differential tests cover every F23-named branch: relative/absolute,
  workspace alias, tsconfig `paths`, `baseUrl` fallback, project
  references (direct + a genuine A↔B cycle proven to terminate on the
  REAL legacy algorithm, not just the ported one), `#imports`,
  `node_modules` exports-with-conditions, legacy scoped-package fields,
  explicit-project resolution, owner-overlap nearest-root selection,
  full-chain miss. Comparison widened beyond final `source_id` to the
  primitive witness set (probe/realpath/recovery-scope facts).

  **A genuine bug was caught and fixed while building this**, exactly
  the kind of thing this expansion exists to catch: a naive per-witness
  strip of manifest-boundary facts (the already-ratified F15 difference
  — kernel's `package_manifest` is one atomic probe-then-read primitive,
  legacy is a separate probe-then-conditional-read) was UNSOUND. A
  `"{dir}/package.json"` probe's first `ancestor_scopes` entry is
  `{dir}` itself, but that SAME scope entry is also legitimately
  produced whenever the resolution independently probes a file inside
  that directory (e.g. `node_modules`' own "no manifest -> probe
  directly" fallback does exactly this, right next to the manifest
  check) — stripping by directory prefix silently deleted that second,
  unrelated origin's entry too. The full-chain-miss differential test
  caught it immediately. Fixed with symmetric enrichment
  (`primitive_witness_pair` synthesizes the manifest probe's own
  recovery scopes onto BOTH sides before comparison, a no-op where
  already present, rather than stripping from one side) instead of a
  one-sided strip.

  211/211 `resolver_core::` tests pass; 312/312 combined with
  `verter_workspace`'s own `resolver_tests.rs` +
  `resolution_witness_contract_tests.rs`.

**F24 (follow-up consult) — gap 4 settled Stage-2-only; Stage-1
completeness re-checked and closed.** Gap 4's five items (manifest
fingerprint `name` preservation, `DirectoryMembers` consumed-vs-
prefetched, complete fact replay/signature, basis-restart on the real
driver, no-progress/terminal/transient-load-failure behavior) all
describe the PRODUCTION retry/replay DRIVER's behavior — Codex
confirmed these are Stage-2 acceptance criteria (the driver lands IN
the atomic cutover commit, per Codex's own checklist step 4), NOT a
Stage-1 characterization target; re-scoped accordingly
(`docs/arch/refactor/rev11/evidence/C1/f24-deviation-consult.md` has
the full per-item acceptance table for whoever plans Stage 2's
execution).

The same consult's spot-check found two things needing closure before
Stage 1 could genuinely be called done, both closed the same round:

1. **`no_phase_archaeology_in_production_code` was failing** — 68
   violations across 30 files, spanning this whole block's work, not
   just this round's (CLAUDE.md's MANDATORY final-state-prose rule).
   Fixed: every doc comment rewritten to remove plan/phase/cutover
   framing and block-identifier possessives while preserving all
   technical content; guard now green.
2. **The differential harness had specific named gaps**: consumed
   selectors compared only as a deduplicated set (no order assertion),
   no `NeedInputs`-wave assertion, `preferred_specifier_candidates`/
   `project_exact_result` had no legacy-vs-kernel differential case,
   and five branch cases were missing (absolute-path for an owned
   importer, alias/paths/`baseUrl` precedence competition, a dangling
   project reference, `exports` array form, carrier/provider
   projection through the full driver rather than in isolation). All
   closed: `KernelCoreRunResult` now carries an ordered selector
   sequence and a wave counter (asserted on in the workspace-alias
   test — manifest-check < path-probe < realpath, `waves >= 3`); two
   new `legacy_preferred_specifier_candidates`/`legacy_project_exact_result`
   test-support bridges plus their dual-run tests; the five branch
   cases added, including one comparing the COMPLETE `ResolveResult`
   DTO (not just `resolved`/witness) end to end for a carrier import.

24/24 `resolution_dual_runner_tests` pass (was 17); 404 combined tests
across `verter_semantic`/`verter_workspace` pass.

**Stage 1 is now COMPLETE**: the algorithm is fully ported, the real
`ModuleResolverCore` type exists, and it is differential-tested against
the real legacy resolver across every named branch with both content
and order/wave assertions. What remains is Stage 2 itself — item 5's
final per-symbol migration table re-grep, then executing the ratified
atomic-cutover checklist (F23's evidence file has the full ordered
checklist; F24 added gap 4's acceptance table to it).

**F25 — item 5's FINAL re-grep, dispositioning F23's 5 remaining
undispositioned closure items. Item 5 is now CLOSED.** Two independent
read-only ground-truth investigations re-verified all 8 of F23's flagged
"additional non-resolver references" against the live tree (cross-checked
against F22's own DTO enumeration — no new unaccounted type surfaced). 2
of the 8 were already fully dispositioned (the fact-registry wildcard
shim; the 15 core DTOs). The remaining 5 were confirmed real, previously-
undispositioned couplings, then ruled on by a fresh consult
(`docs/arch/refactor/rev11/evidence/C1/f25-deviation-consult.md`):

| Item | Definition | Disposition |
|---|---|---|
| `FactVersionRef` + payload closure (`ResolveImportsFactRef`, `ResolutionFactRef`, `ResolutionFactVersion`, `ResolutionFactKey`, `ResolutionQueryKey`) | `verter_workspace::fact_cache.rs:905` + `resolution_currency.rs:389,700` | MOVE to `verter_semantic::facts`/`facts::resolution`/`resolver_core` as appropriate. Cache authority (roots, mutation propagation, version counters, `ResolutionTransaction`, replay ledgers, validators, invalidation, publication) stays workspace/session-owned — vocabulary moves, not cache authority. |
| `ProjectStableKey` + `AmbientSymbolHit` | `verter_workspace::project_key.rs:27`, `ambient_lib.rs:60` | MOVE both to `verter_semantic::resolver_core`, keeping `to_hex_tag`/`parse_hex_tag` with the type. `from_project` becomes a workspace free function `project_stable_key_from_project(&OwnershipProject, &CanonicalPath) -> verter_semantic::resolver_core::ProjectStableKey` (Rust's same-crate inherent-impl constraint forces this — `from_project` cannot stay an inherent method on a relocated type). Ambient registry/registration/lookup storage stays workspace-owned. |
| `PathProbe` | `verter_workspace::resolution_currency.rs:344` | MOVE unchanged to `verter_semantic::resolver_core`. `ResolverObservation::path_probe` returns the semantic-owned type directly; workspace VFS maps onto it. Workspace crate-root `pub use` is an acceptable value alias. |
| `WorkspaceAuthorityId` / `ResolutionPopulation` / `ResolutionWorldId` | `verter_workspace::resolution_currency.rs:86,191,44` | MOVE all three (with embedded `SessionFingerprint`) into semantic-owned resolution identity vocabulary, co-located with `ResolutionWorldBasis` (already semantic-owned, compares this exact tuple). Workspace mints values via narrow checked constructors semantic exposes (preserving the `0`-placeholder invariant), keeps its own counters/world/cache machinery. |
| Route analyzer's `DirEntry` | `verter_workspace::error.rs:43` (plain `{path, is_dir}`, confirmed NOT `std::fs::DirEntry`) | MIRROR/PROJECT — the one exception to MOVE. `verter_workspace::error::DirEntry` stays canonical for VFS; new semantic-owned `RouteDirEntry { path: Arc<str>, is_dir: bool }`; `RouteAnalysisInputs::directories` and its APIs switch to it; the session-side walker (`route_analysis_inputs.rs`) does the one-way projection. Legitimate because these are genuinely different domain rows, not two encodings of one identity. |

**All 5 MUST close in the Stage-2 commit — none is a residual exception.**
Each currently causes `verter_semantic` to name a workspace type, blocking
the required Cargo-edge reversal; the `DirEntry` mirror still closes the
edge because semantic thereafter names only `RouteDirEntry`. No opaque-
handle disposition was justified for any item (would require interning/
serialization/downcasting, erasing the typed fact IR).

**Commit shape reconfirmed: ONE atomic commit** (F23 stands — the
confirmed breadth doesn't invalidate it; a concrete Cargo SCC reason
applies too: the edge `verter_workspace -> verter_semantic` cannot be
added while any `verter_semantic -> verter_workspace` reference remains
without creating a cycle). **WIP commits/checkpoints are explicitly
sanctioned as long as squashed before landing.** F25's internal work
order for Stage 2 execution:

1. Establish all semantic-owned values (15 DTOs + the 5 items above +
   `RouteDirEntry`), workspace projections, and workspace value
   re-exports.
2. Repoint the inert kernel so production semantic code contains zero
   workspace names.
3. Reverse both Cargo edges and flip the complete `verter_identity` guard
   cluster together.
4. Build the real workspace retry/replay driver and satisfy F24's five
   replay/failure contracts.
5. Repoint every production caller to `ModuleResolverCore`.
6. In that same unlanded transition, delete `ProjectResolver`,
   aliases/wrappers, bridges, dual-runner harness, and obsolete tests.
7. Verify zero production/dev `semantic -> workspace` edge, the positive
   `workspace -> semantic` edge, authority uniqueness, and the full gate;
   then create the single final (squashed) commit.

**Item 5 is now fully closed** — every symbol named across F18/F22/F23/F25
has a settled destination and disposition. Stage 2 is ready for mechanical
execution starting at step 1 above; no further consult is anticipated
before execution begins, since F25 was the last named open architectural
question.

**F26 — `FactVersionRef`'s payload closure corrected during execution.**
While starting to port F25's `FactVersionRef` row, reading the type's full
definition revealed it is `verter_workspace`'s crate-wide, general-purpose
fact-versioning vocabulary — only 1 of 10 variants (`ResolveImports`) is
resolution-related; the other 9 span whole-file/derived-fact hashing, parse
facts, route-surface facts, program-analysis facts, file source-environment
facts, project-generation counters, domain-aggregate compaction, and
strict-self-root-world cache-validation witnesses, referenced across 11
`verter_workspace` files and 40+ `verter_session` files. F25's ruling was
made without this visibility. A fresh consult (`docs/arch/refactor/rev11/
evidence/C1/f26-deviation-consult.md`) confirmed this as a genuine
sixth-deviation trigger and ruled a REFINED F25 disposition (F25's
conclusion survives, its inventory was incomplete): the full immutable
`FactVersionRef` value graph — the type itself, every payload type its
variants embed transitively, `FactAttribution`, `CompactionDomain`,
`DomainGenerationFact` — moves to `verter_semantic::facts` alongside F25's
already-named resolution DTOs (`ResolveImportsFactRef`, `ResolutionFactRef`,
`ResolutionFactVersion`, `ResolutionFactKey`, `ResolutionQueryKey`). Cache
AUTHORITY stays workspace/session-owned: `FactVersionValidator`,
`FactReadSet`, admission/validation/mutation-propagation/counters/
compaction/replay-ledgers/publication/invalidation, `CANDIDATE_CAP`. This
does not move `fact_cache.rs` (1914 lines) wholesale — only its immutable
discriminated identity IR.

## Item 6 — the dual-runner harness plan

**Status: RATIFIED (F18); LOCATION + VISIBILITY CORRECTED (F20); FIRST
TWO CASES IMPLEMENTED AND PASSING.** Port
`resolution_witness_contract_tests.rs`'s two cases as the FIRST unignored
tests against the real kernel seam, once — and only once — that seam
exists (item 3, now landed).

**Landed**: `crates/verter_semantic/src/resolver_core/
resolution_dual_runner_tests.rs`. Both the positive case (`./mod.js` from
`/p/main.ts`, resolving to `/store/pkg/mod.tsx` via the `.tsx` TS-source-
sibling) and the miss case (`./missing`, exhausting the full 24-candidate
precedence order) pass: kernel and legacy runners resolve to the same
source id AND retain byte-identical witness fact sets (path probes,
realpaths, recovery-scope ancestors). Scoped to exactly the
`probe_path_for_context`/`probe_path` slice these two cases exercise (see
F20's recommended shape above) — NOT the full `resolve_source_id`
surface, NOT wired into production.

**Basis-change restart — LANDED** (one of F18's named follow-ons). Found
a genuine gap while implementing: the driver had been accumulating
`NeedInputs` facts unconditionally, never comparing a returned
`LoadSet`'s basis against what it currently expected — so a basis change
mid-resolution would have been silently ignored rather than triggering
the rule-8 restart. Fixed: `run_kernel_with_view_builder` now discards
the ENTIRE snapshot and restarts under the new basis on a mismatch,
never mixing facts loaded under two different resolution worlds. New
test `kernel_runner_restarts_cleanly_on_a_basis_change` proves a driver
told to expect `basis(1)` but answered under `basis(2)` restarts exactly
once and reaches the SAME witness a normal no-restart run would.

**Base/session population selection — covered by composition, no new
test needed.** The driver's restart check (`load_set.basis() !=
expected_basis`) is a single structural `PartialEq` over the WHOLE
`ResolutionBasis` — it does not branch on WHICH field differs. A
population-specific integration test would exercise the identical code
path `kernel_runner_restarts_cleanly_on_a_basis_change` already proves,
with the population-equality semantics themselves already covered by
`attempt_outcome_tests.rs`'s dedicated `resolution_basis_differs_on_population`
unit test. Composition of the two already closes this follow-on; a
third, population-specific variant would be redundant coverage padding,
not a new discriminating test.

Still open (F18's remaining named follow-ons): manifest-fingerprint
preservation, `DirectoryMembers` consumed-vs-prefetched behavior, full
`ResolutionFactKey` replay/signature comparison (needs the deferred
`verter_workspace/test-support` replay helper, F20) — these need
node_modules/package-manifest handling the current narrow kernel runner
does not implement, which starts to blur into "port the full production
algorithm" (item 2/5) rather than staying a small follow-on test. Item
6's SMALL, self-contained follow-on work is now exhausted; the next
increment is either expanding the kernel runner into node_modules/
package resolution, or moving to scope/start the real production
`ModuleResolverCore` port (item 2/5) directly.

**F20 correction**: lives in `verter_semantic::resolver_core` (a
`#[cfg(test)]` unit-test module) — NOT `verter_workspace` (cannot reach
`ResolverAttemptView`/`priority_frontier`, no dependency edge) and NOT
`verter_session` (cannot reach `priority_frontier` either — it is
`pub(crate)` to `verter_semantic` per item 4's own ratified design;
widening it to reach a session-hosted harness would itself violate item
4). `ProjectResolver::resolve_with_reader` stays PRIVATE in production —
an existing architecture guard
(`raw_resolver_entry_points_are_private`, `crates/verter_session/tests/
cases/compile-fail/raw_resolver_entry_points_are_private.rs`) pins that
only the Engine transaction may mint a resolution witness, and making it
`pub` would break that guard's contract. Landed instead: a `#[cfg(any(
test, feature = "test-support"))] pub mod test_support` bridge inside
`resolver.rs` itself (`test_support::legacy_resolve_with_reader`),
compiled out of every production build, not widening
`resolve_with_reader`'s own visibility at all — verified the compile-fail
guard still passes clean. The prior wording "without touching
`resolver.rs`" is corrected to: **"without changing any
production-reachable `resolver.rs` behavior or call path."** Item 5's
deletion list must additionally remove this bridge at the atomic cutover,
alongside `resolve_with_reader` itself.

Proposed concrete fixture (F18):

```rust
struct ResolutionFixture {
    projects: Vec<IdeProjectConfig>,
    request: ResolveRequest,
    probes: BTreeMap<CanonicalId, PathProbe>,
    realpaths: BTreeMap<CanonicalId, Option<CanonicalId>>,
    manifests: BTreeMap<CanonicalId, PackageManifest>,
}
```

Both runners (legacy vs. kernel) return a normalized record: semantic
result; ordered primitive observations/consumed selectors; recovery-scope
set; replayed `ResolutionFactKey` set/signature; kernel-only `NeedInputs`
waves — for direct comparison. Positive case: the kernel harness should
demonstrate speculative sibling probes MAY be prefetched, but the
completed output consumes only the three facts the legacy witness test
itself asserts (absent `/p/mod.ts`, file `/p/mod.tsx`, realpath
`/p/mod.tsx`). Miss case: must retain the exact 24-probe precedence order
the legacy test already asserts. **F20 addition**: the positive case's
recovery-scope set also contains `/` (root) alongside `/p`/`/store`/
`/store/pkg` — not a contradiction of the legacy test (which never
asserts `/`'s absence). Follow-on tests (after these two):
manifest-fingerprint preservation including the omitted `name` field,
`DirectoryMembers` consumed-vs-prefetched behavior, base/session
population selection, basis-change restart, and (F20) full
`ResolutionFactKey` replay/signature comparison via a separate small
`verter_workspace/test-support` replay helper (deferred — not required
for the first two cases, since `ResolutionFactKey` constructors are
workspace-private).

**F20's recommended kernel-runner shape**: one fresh, immutable
`ResolverAttemptView` snapshot per attempt (never reused across retries —
the input-loading contract requires this); `workspace_only(...)` closures
over a growing loaded-facts snapshot; a GENERAL (not fixture-specific)
candidate-generation function implementing relative/absolute base
construction, JS-family source-sibling candidates
(`resolve_ts_source_sibling`'s logic), declaration-companion candidates
gated by `prefers_declaration_files` (`resolve_declaration_companion`'s
logic), then the 12 bare-extension-or-as-is candidates and the 12 index
candidates (`probe_path`'s logic) — ALL flattened into ONE ordered
candidate list fed through the REAL `priority_frontier`, since
`probe_path_for_context`'s own nested short-circuit structure is itself
already a priority-ordered "try candidates in sequence, first hit wins"
chain, exactly `priority_frontier`'s own model; probe-then-realpath-on-hit
per candidate (`resolve_existing_path`'s logic). Outer retry-loop driver:
run `priority_frontier`, on `NeedInputs` load the requested keys into the
snapshot and retry, a repeated empty delta fails as no-progress. A narrow
test-only relative-path slice (NOT the full `resolve_source_id`/
`resolve_source_id_unowned` surface — no tsconfig paths, workspace
aliases, node_modules, package manifests, project references, none of
which either witness-contract case exercises) is acceptable scope for
item 6's first two cases; it does NOT satisfy C1's eventual full-surface
requirement and must NOT survive the atomic cutover as a second resolver
implementation.

## Status: RATIFIED (F19) — production conversion may now start

All six items are ratified/settled. Per F19's explicit verdict: "Once
these additions are incorporated and the status header is updated, the
sequencing record is sufficient to authorize starting the future
conversion." This edit incorporates those additions; this document now
satisfies F17's own gating requirement.

- Item 1: inventory ratified, dispositions corrected (F18). Not yet
  executed (no symbols moved).
- Item 2: settled shape proposed — the recommended shape (F18), not
  re-opened by F19; treat as the design to implement against. Not yet
  implemented (`ModuleResolverCore`/`IdeProjectConfig` relocation has not
  started).
- Item 3: RATIFIED (F19); IMPLEMENTED — `ResolutionBasis`/
  `ResolutionWorldBasis`, `InputKey::DeclarationSpace`,
  `CompletedAttempt<T>`/`KernelAttempt<T>`,
  `AttemptFailure::ObservationUnavailable`, and `ResolverAttemptView`
  (the sealed trait's one closure-capability implementor) are all landed
  with full TDD coverage. Inert — no production driver populates real
  closures yet.
- Item 4: SETTLED (F18); IMPLEMENTED — `priority_frontier<C,T>` landed
  (`resolver_core::priority_frontier`), all six required scenarios
  covered, `#[allow(dead_code)]` pending its call site.
- Item 5: substantially advanced (F18); remaining work is mechanical
  ATOMIC-CUTOVER PREFLIGHT (symbol inventory/aliases/guards/tests/Cargo
  edges/final re-grep), not an open architecture question. Not started.
- Item 6: RATIFIED (F18) — the `ResolutionFixture` dual-runner design.
  Not yet implemented — needs a real "kernel runner" side, which in turn
  needs at minimum a first cut of the ported resolve algorithm (using the
  now-landed `ResolverAttemptView`/`priority_frontier` seam) to run
  against; the harness and that minimal algorithm cut are the next
  increment.

What this document still does NOT do: it does not itself execute item 1's
dispositions, item 2's shape, item 5's migration, or item 6's harness —
those remain designs authorized for implementation, not landed production
code (items 3 and 4 ARE now landed, per above — this document is a
record, not a live status board, so treat the per-item notes above as
authoritative over this paragraph's older framing). F19's named first
implementation step is DONE (see the top-of-file summary); the next step
is item 6's dual-runner harness plus the minimal resolve-algorithm cut it
needs to exercise, before the full atomic migration (item 5) moves the
remaining production call sites.

### Progress log — real algorithm port (post-F22, "Plan A")

Item 6's dual-runner harness landed (`resolution_dual_runner_tests.rs`)
and, per F22's confirmed finding that the three already-landed
observation primitives are sufficient for the entire resolver
algorithm, the real port has started as new, inert, side-by-side code
under `resolver_core`. `ProjectResolver` remains the sole production
resolver throughout — nothing below has a production call site yet.

Landed so far:

- `probe_path_resolution.rs` — `probe_path_for_context`/`probe_path`'s
  full candidate-generation + evaluation, promoted out of the
  dual-runner harness into a shared `pub(crate)` module (both boolean
  gates — JS-family source-sibling substitution, declaration-companion
  preference — faithfully ported, including the `ctx.kind ==
  SfcSrcAttr` skip-substitution case).
- `package_target_resolution.rs` — `resolve_package_target`
  (String/Array/Object forms, Array/Object branches as nested
  `priority_frontier` calls), `resolve_package_exports`,
  `resolve_legacy_package`, `resolve_manifest_types_entry`,
  `package_conditions`, `resolve_package_path`,
  `capture_tsconfig_pattern`, `match_package_mapping` (all ported
  verbatim, pure/no-I/O), and `read_package_manifest_if_present`
  (wrapping the `package_manifest` observation's own directory-keyed
  probe+read semantics into the `KernelAttempt` envelope). 20
  characterization tests, all passing.
- `node_modules_resolution.rs` — `resolve_node_modules_package_from_dirs`
  (the per-directory step is genuinely sequential, not an ordered-
  candidate fallthrough, so it's expressed via a small local `then`
  sequencing helper rather than nested `priority_frontier` calls),
  `resolve_node_modules_package`/`_from_dir`, `resolve_package_imports`/
  `_from_dir`, `ancestor_dirs`/`ancestor_dirs_from_dir`/
  `split_package_specifier` (pure). 13 characterization tests.
- `tsconfig_paths_resolution.rs` — `resolve_path_mapping_target`
  (its own manifest-then-exports-then-legacy shape is genuinely
  DIFFERENT from node_modules' per-directory step — a miss falls
  through to legacy at the SAME directory, no outer directory loop),
  `resolve_tsconfig_paths`, `apply_tsconfig_target`,
  `sorted_workspace_aliases`, and the extracted shared
  `resolve_via_workspace_config` (the aliases -> tsconfig `paths` ->
  `baseUrl` trio, run VERBATIM at three legacy call sites — one shared
  port instead of tripling it). 9 characterization tests.
- `project_references_resolution.rs` — `resolve_project_references`/
  `resolve_project_references_inner`, the transitive descent bounded by
  a cycle-detection active set + depth fuse, both genuinely mutable and
  threaded through a `priority_frontier`'s `FnMut` closure exactly like
  the legacy recursive method. `projects: &[IdeProjectConfig]` is plain
  in-memory config data, not an observation. 6 characterization tests,
  including a genuine A<->B reference cycle proven to terminate cleanly.
- `source_id_resolution.rs` — the three `ProjectResolver`-INTERNAL
  per-shape dispatch methods: `resolve_source_id_unowned` (the only one
  gated by `package_follow_is_confirmed`'s node_modules re-entry
  boundary check), `resolve_source_id`, `resolve_source_id_for_project`
  (both compose `resolve_via_workspace_config` -> `resolve_project_references`
  -> `#imports`/`node_modules`, bounded by `workspace_root`, with NO
  re-entry guard — confirmed by re-reading the legacy functions side by
  side, and by a characterization test proving the SAME boundary-
  escaping relative follow `resolve_source_id_unowned` rejects,
  `resolve_source_id` accepts). 15 characterization tests.

**A prior round's report mischaracterized the above as "the full public
entry-point surface" — WRONG.** `resolve_source_id*` are
`ProjectResolver`-internal per-shape dispatch methods, not the real
public API. F22's full-file read already named the actual four public
entry points correctly (see item 2 above): `resolve_with_reader`/
`resolve_for_project_with_reader`/`preferred_specifier`/
`project_exact_result`. This was not a new architectural deviation —
F22 had already scoped owner/project selection and provider-graph
projection as separate, not-yet-ported concerns; it was the progress
summary that conflated the two. Corrected the same round it was found,
continuing in item 2's already-scoped order (no fresh consult needed):

- `project_ownership_resolution.rs` — `effective_configs_for_path`/
  `nearest_config_for_path`/`project_for_ownership`/`compare_projects`.
  ENTIRELY PURE (F22/F18 confirmed: `IdeProjectConfig::matches_file`
  delegates to `ConfiguredMembership::contains`, a completed-value
  in-memory glob match) — no `KernelAttempt` involvement at all. 9
  characterization tests, including nearest-root pruning and
  `project_for_ownership`'s duplicate-refusal case.
- `provider_projection_resolution.rs` — `build_resolve_result`/
  `build_project_resolve_result`/`provider_id_for_source`/
  `provider_ide_id_for_source`/`source_id_from_provider_id`/
  `relative_specifier`/`project_exact_result`. Also entirely pure.
  `path_is_carrier`/`carrier_ide_provider_path`/`carrier_api_provider_path`
  are already `pub fn`/`pub const` on `verter_workspace::resolver`
  (registry-backed, no `ProjectResolver` instance state) — called
  directly, NOT re-ported, unlike the private per-`ProjectResolver`
  helpers ported elsewhere. 14 characterization tests, including the
  carrier `relative_specifier` case and the discrimination between
  `build_resolve_result`'s computed relative specifier for a carrier
  target vs `build_project_resolve_result`'s always-literal one.
- `top_level_resolution.rs` — `resolve_with_reader`/
  `resolve_for_project_with_reader`, THE REAL TOP-LEVEL ORCHESTRATORS:
  owner selection -> per-shape dispatch -> provider-graph projection,
  composing every piece above into the actual `ResolveResult` DTO. 6
  characterization tests — genuine end-to-end integration tests from a
  `ResolveRequest`/`ProjectOwnership` down to the final `ResolveResult`.
- `preferred_specifier_resolution.rs` — `preferred_specifier_candidates`/
  `reverse_tsconfig_path`, both pure. Per F22's confirmed disposition,
  private `preferred_specifier` itself is test-only with no production
  caller and is DELETED (not moved) at the real cutover — its
  round-trip-verify-and-pick-shortest orchestration calls back into
  `resolve_with_reader`, which belongs to the Engine/session retry
  loop, not this kernel; only the pure candidate generator moves. 5
  characterization tests.

**Milestone: item 2's ENTIRE owned-function-surface enumeration is now
ported** — every function F22's full-file read named as reachable from
the real four public entry points (`resolve_with_reader`/
`resolve_for_project_with_reader`/`preferred_specifier_candidates`/
`project_exact_result`) has an inert, characterization-tested port. The
remaining free helpers (`join_paths`/`normalize_canonical_id`/
`is_absolute_specifier`/`is_relative_specifier`/`parent_dir`/the
carrier-path helpers) are already `pub fn` on `verter_workspace::resolver`
and called directly, not re-ported — matching the established
precedent throughout this port. 191/191 `resolver_core::` tests pass
(up from the 115 baseline at the top of the round two rounds ago,
158 at the top of this round). `ProjectResolver` remains the sole
production resolver throughout — nothing above has a production call
site.

**What genuinely remains before Stage 2 (the atomic cutover) can be
planned:** item 5's FINAL per-symbol migration table re-grep (F22:
"needs the final per-symbol enumeration + a last re-grep immediately
before the actual move") — this is explicitly the last preparation
step per F21/F22's model, and per F21's own framing the atomic cutover
itself (delete old resolver, repoint ~14+ call sites across 6+ crates,
reverse the Cargo edge, flip the `verter_identity` guard cluster) is a
genuinely irreversible step that needs its own ratified plan (likely a
fresh Codex consult) before execution, not a decision to make inline
at the tail of a porting round.
