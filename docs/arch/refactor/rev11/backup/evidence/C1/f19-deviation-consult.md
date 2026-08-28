# C1 nineteenth deviation — F19: item 3's two gaps closed; sequencing record RATIFIED

Follow-up consult on F18's two named item-3 gaps (the all-13-method
field/source matrix; the authoritative `ResolutionBasis` minting recipe).
Full consult prompt/output: `/tmp/c1-item3-followup-prompt.md` /
`/tmp/c1-item3-followup-output.md` (not committed — ephemeral scratch;
this file plus the rewritten sequencing record are the durable record).

## Overall verdict

**Item 3 is ratifiable with corrections recorded normatively. The
sequencing record, once these corrections are incorporated, is
sufficient to authorize STARTING the production `ProjectResolver` ->
`ModuleResolverCore` conversion in a future round.** My own pre-consult
narrowing ("only 3 of 13 methods matter for this cutover") was CONFIRMED
correct for the resolver ALGORITHM, but my proposed MECHANISM ("two
separate implementors, one workspace-only, one session-only") was
WRONG — corrected below. My `ResolutionWorldId`-fold-to-`u64` candidate
for `ResolutionBasis` was also WRONG — corrected below to a structured
type.

## Gap 1 corrections — field/source matrix

**Confirmed**: `ProjectResolver`'s complete algorithm reaches
`WorkspaceRead` only through `probe_path`/`realpath`/
`read_package_manifest` (`resolver.rs:1193,1251,1654`) — mapping exactly
to `path_probe`/`real_path`/`package_manifest`. None of the other 10
`ResolverObservation` methods is called by the current algorithm.

**Two corrections to my proposed mechanism**:
1. **There must NOT be separate workspace and session trait
   implementors.** `ResolverObservation` is SEALED inside
   `verter_semantic`, so the only production implementor can be the ONE
   semantic-owned `ResolverAttemptView`. Workspace and session are
   BUILDERS/DRIVERS that populate DIFFERENT CAPABILITIES on that SAME
   type — not two competing impls. (This corrects my own "workspace
   ModuleResolver builder vs. full session builder as two implementors"
   framing from the consult prompt — they're two BUILDERS of the one
   type, exactly as F18 already said, which I'd drifted from in my own
   proposal.)
2. **`AttemptFailure::InputLoadUnavailable { key: InputKey }` cannot
   represent an unavailable NON-KEYED method** (e.g. `project_generation`
   has no `InputKey` at all — it's an "immediate" value, not a keyed
   loadable slot). Needs a typed `ObservationUnavailable { observation:
   ResolverObservationKind }` (or an equivalent widening of the existing
   failure enum) — an arbitrary fake `InputKey` would be unacceptable.

### Concrete 13-method matrix (normative)

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

The 5 immediate-value observations need explicit `Available`/
`Unsupported` state, never a default. Keyed slots need THREE states:
`Unloaded`, `Loaded(value, including a stable None)`, `Unsupported`.

**Confirmed**: the module-resolution session peeks (`path_probe`/
`real_path`/`package_manifest`) do NOT already exist as production
`ResolverAttemptView` wiring — the workspace primitives (`WorkspaceRead`)
and backing stores exist, but the production adapter is still to be
built (matches every other landed method's status — inert until wired).

### A genuine correction to ALREADY-LANDED code: `InputKey::DeclBody` needs a type/value-space discriminator

`InputKey::DeclBody { canonical, owner, name }` (landed round 4a,
`attempt_outcome.rs`) does NOT currently say type-space vs. value-space —
so the retry driver cannot recover WHICH observation (`type_decl` vs.
`value_decl`) produced a given `DeclBody` key. Two options: add a
semantic-owned `DeclarationSpace::{Type, Value}` field (PREFERRED — better
demand precision), or define that one `DeclBody` load populates BOTH
spaces for the same `(canonical, owner, name)` key. **Not yet fixed** —
this is a real, actionable finding about existing landed code, recorded
here for a dedicated small fix (adding a field to an existing `InputKey`
variant is additive/backward-compatible in the same sense `AttemptOutput`'s
variants are, but touches `type_decl`/`value_decl`'s own `NeedInputs` arms
too, so it's its own small implementation unit, not bundled into this
consult's other findings).

## Gap 2 corrections — `ResolutionBasis`'s minting recipe

**My `ResolutionWorldId`-plus-`population`-folded-to-`u64` candidate was
REJECTED.** The captured resolution world IS the correct foundation, but
folding into a scalar `u64` is wrong — the existing `AggregateStamp`
documentation explicitly chooses exact tuples over digests for the same
reason (`fact_cache.rs:81`), and that reasoning applies here too. Verified
directly: `ResolutionWorldId` identifies one immutable root
(`resolution_currency.rs:42`); base and session roots each carry their own
ID (`:962`, `:1000`); a captured session world composes BOTH roots, not
one or the other (`:1532`); `resolution_stamp` already encodes the
authoritative rule (base population = `{base, session: None}`; session
population = `{base, session: Some(session_id)}`, `:1594`); base
publication mints a new ID only on `WorldWrite::Publish`
(`engine.rs:883`); session publication does the same (`engine.rs:1142`);
`WorldWrite::Retain` deliberately PRESERVES the ID when nothing existing
became invalid (`engine.rs:193`); stable-capture/final-currentness checks
compare EXACT epochs and root IDs (`engine.rs:1950,2053`).

**Normative structured recipe**:

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
attempt includes the EXACT `StoreViewValidationToken` (not its
`external_supersession_fingerprint()` fold); base population = exact base
root, no session root; session population = exact base root + exact
session root + the session population/fingerprint; mint it from the SAME
`Arc<CapturedResolutionWorld>` used by the `ResolutionTransaction`, loader
commit fence, and replay; do NOT hash/fold into `u64`; the public
`ResolutionBasis::new(u64)` production path must be REPLACED/REMOVED
(`new(0)` stays test-only synthetic vocabulary AT MOST — matches the
already-documented "PROVISIONAL placeholder" status every landed method's
`NeedInputs` arm currently uses). `workspace_authority` is needed because
the world-ID counter starts at `1` for every `Engine` — root IDs are
unique WITHIN an engine, not across engines; the existing process-unique
engine authority (`strict_self_root_authority_id`) generalizes/reuses
rather than inventing a new hash. No separate `resolve_env_hash`/policy
hash needs folding in separately, PROVIDED one invariant stays explicit:
`ModuleResolverCore`, its configuration graph, and the basis all come from
the SAME captured `PublishedRoot` — publishing that root always remints
the base world ID, and its `ResolveContextId` already contains project
identity, resolver policy, provider policy, and resolve-env identity
(`resolution_currency.rs:143`). The full session token additionally binds
the non-resolution session/store-view dimensions exactly.

**Correction to my own pre-consult claim**: `ResolutionWorldRoot` retains
path probes and REALPATHS in full, but only MANIFEST FINGERPRINTS, not
full manifest contents — confirms (does not newly discover) F18's earlier
finding that the package-manifest loader must retain the full manifest
separately for the narrow kernel projection + replay fingerprint.

## Item 3: RATIFIED with these corrections recorded normatively

Closed once the record states: one semantic-owned `ResolverAttemptView`
with driver-specific capability population; typed unavailable state for
non-keyed observations; exact loader ownership for all 7 `InputKey`
variants; declaration-space-exact `DeclBody` loading; the exact structured
basis (workspace authority + population + base/session roots, plus the
full session validation token); no scalar fold, no
`ResolutionBasis::new(0)` in production; one fresh `AttemptOutput` per
attempt, output published only on `Complete`. These are now IMPLEMENTATION
obligations, not unanswered architecture questions.

## Final go/no-go: YES, production conversion may start

"Once these additions are incorporated and the status header is updated,
the sequencing record is sufficient to authorize starting the future
conversion." Item 5's remaining symbol inventory/aliases/guards/tests/
Cargo edges/final re-grep remain mandatory ATOMIC-CUTOVER PREFLIGHT work
(not an architecture question) — no longer requiring another consult.

**Concrete first implementation step, named explicitly**: "Replace the
scalar `ResolutionBasis` with the exact structured basis and add the
semantic-owned `ResolverAttemptView`, `CompletedAttempt<T>`, and
`KernelAttempt<T>`, with failing tests for base/session basis changes,
unsupported capabilities, and each loader mapping." THEN port the two
ratified witness-contract cases into the real dual-runner harness BEFORE
moving the resolver algorithm itself.

## Explicit instruction, followed

"No files were changed during this consult." This round's next action:
incorporate these corrections into `sequencing.md` (making
it the RATIFIED record), then begin the named first implementation step —
`ResolutionBasis`'s restructuring — carefully, in small TDD-verified
increments, since it touches every one of the 13 already-landed methods'
`NeedInputs` construction sites.
