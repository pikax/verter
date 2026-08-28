# C1 eighteenth deviation — F18: sequencing record ratification pass — items 1/2/4 corrected, items 4/6 settled, item 3 needs its own follow-up consult

Ratification pass over `docs/arch/refactor/rev11/evidence/C1/
sequencing.md` per F17's own requirement ("Ratification of
the resulting sequencing record is required BEFORE production conversion
begins"). Full consult prompt/output: `/tmp/c1-sequencing-ratification-prompt.md`
/ `/tmp/c1-sequencing-ratification-output.md` (not committed — ephemeral
scratch; this file plus the rewritten sequencing record are the durable
record).

## Overall verdict

**Production `ProjectResolver` conversion should NOT start yet.** Items
1-2 needed corrections (now incorporated into the sequencing record).
Items 4 and 6 are now SETTLED with concrete designs. Item 5 is
substantially advanced with a concrete symbol-level migration table (one
missing consumer found: `resolution_currency::evaluate_selected_context`).
**Item 3 has two load-bearing gaps that need their OWN dedicated
follow-up consult** before it can be ratified: (1) a field/source matrix
for all 13 `ResolverObservation` methods (which builder — workspace or
session — supplies which method, and where each keyed slot's `InputKey`
loader lives); (2) the authoritative `ResolutionBasis` minting recipe
(existing session peeks still use the documented PROVISIONAL
`ResolutionBasis::new(0)`, `decl_body_memo.rs:792` — the real basis must
bind project/config, resolver policy, captured-world/population identity,
and relevant environment without collision-prone or independently
invented sources).

## Corrections to item 1 (dependency-edge inventory)

The 25-line/5-name/2-shim grep count is CONFIRMED accurate (independently
re-run) — but my document's stronger conclusion ("all five names are
already fully dispositioned, nothing left to investigate") was WRONG:

- `AmbientSymbolHit` — genuinely plain data, no correction needed.
- `ProjectStableKey` — plain data AS A TYPE, but its `from_project`
  constructor depends on workspace-owned `OwnershipProject`/
  `ProjectPayload`. Move the enum/value OPERATIONS; keep the ownership-
  derived CONSTRUCTOR workspace-side.
- `FactVersionRef` — dependency-neutral as a VALUE but NOT a one-type
  relocation: it embeds `ResolveImportsFactRef::Resolution(
  ResolutionFactRef)`, whose closure includes `ResolutionFactKey`,
  population/query identities, and resolver request enums. Needs an
  explicit move/split row in item 5, not a bare "relocate it" note.
- Moving `IdeProjectConfig` also moves or canonicalizes the exact
  membership/glob type closure currently hidden behind the
  `project_resolver.rs` shim.

## Corrections to item 2 (portability)

The load-bearing result stands — `ConfiguredMembership` (as a completed
VALUE), `IdeProjectConfig`'s full 8 fields, and the owner-selection call
chain (`effective_configs_for_path`/`nearest_config_for_path`/
`project_for_ownership`) are all genuinely handle-free, I/O-free, pure
computation. **One real correction**: my document wrongly grouped
`materialize_from_spec` with `ConfiguredMembership::contains` as "both
pure." `materialize_from_spec` is NOT pure — it takes `WorkspaceAccess`
and calls `walk()` (`snapshot_builder.rs:385,413`), a genuine filesystem
walk. Correct boundary:

```
workspace snapshot construction
    -> filesystem walk/materialization (materialize_from_spec, IMPURE)
        -> completed ConfiguredMembership DTO (portable VALUE)
            -> pure kernel membership queries (contains, PURE)
```

**Settled shape (recommended, not yet ratified as final)**: make the
complete 8-field `IdeProjectConfig` semantic-owned (retaining ALL 8
fields matters — `provider_root` and the non-resolution compiler booleans
are used by env hashing, membership construction, and other workspace/LSP
logic beyond the resolution algorithm itself, even though resolution
principally reads only `base_url`/`paths`); move the exact membership
ENGINE with it (workspace keeps `materialize_from_spec`, constructs the
completed DTO); move `effective_configs_for_path`/`nearest_config_for_path`/
`project_for_ownership` WITH `ModuleResolverCore` (owner selection is
needed both before resolving the importer and after finding the target —
workspace-side preselection would split resolution authority and
introduce round trips); keep carrier-ownership's fail-closed authority
separate, unchanged. A minimal internal compiled graph sketch:

```rust
struct ModuleResolverCore {
    configs: Arc<[IdeProjectConfig]>,       // existing sorted precedence
    by_tsconfig: FxHashMap<String, ProjectNodeId>,
    reference_edges: Arc<[Arc<[Option<ProjectNodeId>]>]>,
}
```

Preserve exactly: reference order and duplicates; unresolved references
as `None`/skipped; first matching config in the existing sorted order;
the current depth-256 and active-path cycle protection; no new
normalization during edge compilation unless independently characterized.

## Item 3 — STILL OPEN, needs its own follow-up consult

`ResolverAttemptView` should be the ONE universal semantic-owned
implementor of all 13 `ResolverObservation` methods — NOT a
resolver-scoped implementor that panics on the other 10 (unacceptable),
and NOT one that returns fabricated defaults (unacceptable). Design
direction: eager immutable inputs (env hashes, project identities,
package-backed classification, ambient index, project generation,
basis/configuration) vs. keyed loadable slots (whole hashes, decl bodies,
augmentation index, flow skeletons, path probes, realpaths, manifests);
missing keyed slots return the exact `NeedInputs(InputKey)`; workspace and
session drivers populate DIFFERENT subsets but construct the SAME
semantic type; a driver receiving an unexpected key for its scope returns
typed `InputLoadUnavailable`, the observation method NEVER panics.
`CompletedAttempt<T>`/`KernelAttempt<T>` confirmed as exactly F16's shape;
one fresh `AttemptOutput` per attempt; no output publication on
`NeedInputs`/`Terminal`.

**Replay design**: do NOT add `ResolutionPopulation` to
`ConsumedResolutionObservationKey` (an open question from F16's own
evidence file) — population is NOT derivable from the selector alone, but
is already owned by the exact captured world/`ResolutionTransaction` used
for that attempt (`ResolutionTransaction::population`,
`resolution_currency.rs:2695`) — the workspace-side replay reads it from
there, not from the consumed key. Proposed a workspace-owned
`WorkspaceResolutionReplayLedger` companion structure
(`path_probes`/`realpaths`/`manifests` maps keyed by `CanonicalId`,
carrying RICHER workspace evidence than the semantic projection: probe
outcome, resolved realpath, manifest fingerprint, backend-emitted
directory observations). **Concrete gap found**: `ResolutionPackageManifest`
(landed, F15) omits `name`, but `manifest_fingerprint_of`
(`resolution_currency.rs:2217`) includes `name` in its fingerprint
computation — so the fingerprint CANNOT be reconstructed from the narrow
kernel DTO alone. This is NOT a bug in what's landed (F15 deliberately
narrowed `ResolutionPackageManifest` to fields the resolution ALGORITHM
reads, confirmed by grep, and that's still correct) — it means the
NOT-yet-built workspace replay ledger must retain the fingerprint computed
from the FULL manifest it already has access to, separately from the
narrow kernel-facing projection. `DirectoryMembers` stays workspace-only
ancillary replay evidence, NOT a 5th `ConsumedResolutionObservationKey`
variant — replay it only when its associated primitive selector was
actually consumed. The existing transaction methods implicitly add
recovery chains; exact output-led replay needs granular transaction
methods so `RecoveryScope` replays from its explicit output variant
rather than being silently added by `observe_path`/`observe_realpath`.

**Two items block ratifying item 3, need a dedicated follow-up consult**:
1. A field/source matrix for all 13 methods, naming the workspace vs.
   session builder and the loader for every `InputKey`.
2. The authoritative `ResolutionBasis` minting recipe — existing session
   peeks (`type_decl`/`value_decl`/etc.) still use the documented
   PROVISIONAL `ResolutionBasis::new(0)` placeholder
   (`decl_body_memo.rs:792`); the real basis must bind project/config,
   resolver policy, captured-world/population identity, and relevant
   environment hashes without collision-prone or independently invented
   sources.

## Item 4 — SETTLED: the priority-frontier combinator

Should be a REUSABLE PRIVATE helper in `verter_semantic::resolver_core` —
not a new public abstraction, not manually repeated at every fallthrough
site. The existing outcome types already encode the needed states:
`Complete(Some(T))` = hit; `Complete(None)` = exhausted miss;
`NeedInputs(LoadSet)` = blocked; `Terminal` = the exceptional exit.
Proposed shape:

```rust
fn priority_frontier<C, T>(
    expected_basis: ResolutionBasis,
    candidates: impl IntoIterator<Item = C>,
    mut evaluate: impl FnMut(C, &mut AttemptOutput) -> AttemptOutcome<Option<T>>,
    output: &mut AttemptOutput,
) -> AttemptOutcome<Option<T>>;
```

Required semantics (verbatim, load-bearing — this is the concrete
specification for whoever implements it): before any block, merge
completed-miss outputs in candidate order; on a hit before a block, merge
its output and return the hit; on the FIRST block, retain ONLY its
`LoadSet` — do not publish accumulated output; continue through bounded
siblings to union further same-basis missing keys; a known lower-priority
hit AFTER a higher block cannot win — stop and return the blocked set; a
terminal before any block propagates; a terminal encountered only
speculatively after a higher-priority block does NOT outrank that block —
return the blocked set and reconsider on retry; a basis mismatch is NOT
unioned or loaded — return the mismatching `LoadSet`, the outer driver
detects the mismatch and restarts under the new basis; every `NeedInputs`
or `Terminal` path discards ALL branch/frontier output; an exhausted miss
publishes the COMPLETE ordered rejected-candidate witness. Needs focused
TDD coverage for: same-basis union, lower hit after higher block, terminal
before/after block, output discard, basis mismatch, exact miss ordering.

## Item 5 — substantially advanced, one missing consumer found

Confirmed item 5 does NOT need items 2-4's rulings to INVENTORY current
callers (only to finalize DESTINATION symbol names). Produced a concrete
symbol-level migration table (call site -> production consumers ->
migration target) — see the rewritten sequencing record for the full
table. **One consumer the original sequencing-record snapshot missed**:
`resolution_currency::evaluate_selected_context`, which calls
`nearest_config_for_path` directly. Remaining work (mechanical but
mandatory, can be completed by a future round without its own consult):
choose and record the canonical semantic module path + alias policy;
enumerate every moved PUBLIC SYMBOL, not just files; classify retained
non-resolver workspace re-exports (membership/fact carriers); include
tests, benches, compile-fail guards, doc links, Cargo edges, architecture
guards; re-run the inventory immediately before the atomic change.

## Item 6 — RATIFIED: the dual-runner harness

Confirmed: port `resolution_witness_contract_tests.rs`'s two cases as the
first unignored tests against the real kernel seam, once it exists.
Proposed a concrete `ResolutionFixture` struct (declarative:
`projects`/`request`/`probes`/`realpaths`/`manifests`) with two runner
functions (legacy vs. kernel), both returning a normalized record
(semantic result, ordered consumed selectors, recovery-scope set,
replayed `ResolutionFactKey` set/signature, kernel-only `NeedInputs`
waves) for direct comparison. For the positive case, the kernel harness
should demonstrate that speculative sibling probes MAY be prefetched, but
the completed output consumes only the three facts the legacy witness
test itself asserts (absent `/p/mod.ts`, file `/p/mod.tsx`, realpath
`/p/mod.tsx`). For the miss, must retain the exact 24-probe precedence
order the legacy test already asserts. Follow-on tests (after these two):
manifest-fingerprint preservation including the omitted `name` field,
`DirectoryMembers` consumed-vs-prefetched behavior, base/session
population selection, basis-change restart.

## Explicit instruction, followed

"No files or production code were changed during this review." This
consult itself only reviewed and corrected the sequencing record — the
rewritten `sequencing.md` (this same commit) incorporates
every correction/finding above. **Item 3's two named gaps still need their
own dedicated follow-up consult before the sequencing record as a whole
can be considered ratified** — not attempted in the SAME consult that
produced this finding, per the consult's own explicit gating.
