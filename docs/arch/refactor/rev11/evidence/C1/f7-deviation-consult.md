# C1 sixth deviation — F7: `HostStoreView` does not relocate

Found during phase-3 implementation (scoping-spec.md §4 step 3), re-tracing
`HostStoreView`'s transitive closure per the spec's own re-verify
instruction. Dispositioned via a fresh Codex xhigh consult per
`docs/arch/refactor/rev11/evidence/C1/scoping-spec.md`'s own "sixth
deviation" instruction. Full consult prompt/output:
`/tmp/c1-deviation-consult-prompt.md` / `/tmp/c1-deviation-consult-output.md`
(not committed — ephemeral scratch; this file is the durable record).

## Finding

F1 (scoping-spec.md §1, "`verter_semantic` gains") instructs relocating
`HostStoreView` and `StoreViewValidationToken` as "immutable observation
value types," tracing "whatever they concretely depend on to compile as
standalone values... with no `&VerterHost`." Recon's own dependency list for
`HostStoreView` names only `StoreViewCompatToken` and
`Arc<StoreViewMemo>` — it omits `HostStoreView.snapshot:
Arc<StoreViewSnapshot>` entirely.

Re-tracing that field: `StoreViewSnapshot.roots: StoreViewRoots`
(`crates/verter_session/src/store_view_roots.rs:386`) directly holds
`Option<Arc<SchedulerSourceRoot>>`, `Option<Arc<FileArtifactRoot>>`,
`Arc<ProjectEnvRoot>`, `Option<Arc<CapturedResolutionWorld>>`,
`Option<Arc<SessionOverlayRoot>>`, `Option<Arc<ResolvedImportFactsDb>>`,
`Option<Arc<ProjectTypeStore>>` (the same `ProjectTypeStore` CLAUDE.md's
"Project-global cache (final state)" section names as `VerterHost`'s sole
shared cache graph), and `Option<Arc<dyn WorkspaceAccess>>`.
`HostStoreView.memo: Arc<StoreViewMemo>`
(`store_view_roots.rs:941`) retains `CanonicalView` values holding
`Option<Arc<FileFacts>>` / `Option<Arc<IndexedReady>>` — the canonical
post-parse artifact.

A literal relocation of `HostStoreView` "and whatever it concretely depends
on" therefore bottoms out at relocating or duplicating essentially the
entire `verter_session`-owned cache-storage graph — directly contradicting
F1's own next sentence ("`StoreViewManager` and all cache-retention policy
stays in `verter_session`"), the scoping-spec's "`verter_session` keeps"
list, and CLAUDE.md's description of `ProjectTypeStore` as host/session
owned.

`StoreViewValidationToken` does NOT have this problem: its own fields are 7
`u64`/`Option<u64>` primitives, one `Hash16` (already mirrored in
`verter_semantic::analysis::types::Hash16`), one `ProjectIdentity` (a
trivial `Hash16` newtype), one `Option<OverlayIdentity>` (itself trivial).
Only its `capture(host: &VerterHost)` associated fn needs to leave (become
a `verter_session`-side free fn — Rust's orphan rule forbids a downstream
crate adding inherent impls to a relocated type).

## Disposition: ADOPT-NOW

Verdict from the Codex xhigh consult (full text in
`/tmp/c1-deviation-consult-output.md`): **ADOPT-NOW**. Not a second-resolver
signal (`HostStoreView` performs observation/validation/leasing/memoization,
not query-time resolution semantics) — none of the charter's Abort/rescope
triggers fire.

### F7 — `HostStoreView` is session-owned committed-store view machinery, not a relocatable observation value. ADOPT-NOW.

Corrected rule, superseding F1's "verter_semantic gains" bullet for
`HostStoreView` specifically (F1's `StoreViewValidationToken` half is
unchanged):

- Relocate `StoreViewValidationToken` and its dependency-neutral chain
  (`ProjectIdentity`, store-view `OverlayIdentity`, existing semantic
  `Hash16`) into `verter_semantic`, including its pure
  comparison/fingerprint methods (`externally_superseded_by`,
  `external_supersession_fingerprint`, `lane_fingerprint`).
- Keep host capture/token construction (`StoreViewValidationToken::capture`)
  in `verter_session` as a free helper — it reads `VerterHost` and
  constructs the semantic-owned token.
- Keep `HostStoreView`, `StoreViewSnapshot`, `StoreViewRoots`,
  `RootCapture`, `StoreViewMemo`, `CanonicalView`, current/cold capability
  wrappers, overlay re-rooting, and `StoreViewManager` in `verter_session`.
- The semantic observation interface (`ResolverObservation`, F3) accepts
  dependency-neutral keys and returns dependency-neutral observations
  through `AttemptOutcome<T>`; no `HostStoreView`, session cache root, or
  host/scheduler handle crosses the crate boundary. Observation methods
  expose facts and immutable DTOs — not operations like `resolve_import`
  that would let an environment implement semantics itself.
- `HostResolverContext`/`SessionResolverContext` do NOT implement
  `ResolverObservation` (unchanged from F3/scoping-spec:314) — the
  session-side driver that DOES implement it may hold `&VerterHost` and
  `HostStoreView` internally to perform capture/loading/retry, translating
  each observation-trait method into a peek + `AttemptOutcome`, but the
  trait itself gains no host escape hatch.
- C2's later I/O-free counterpart is a distinct captured data-only
  environment, not a relocated or duplicated `HostStoreView`.
- `StoreViewCompatToken` may still relocate independently if the
  semantic-side lane/observation contract needs it; that does not imply
  `HostStoreView` relocates.

## Correction to my own report

My original consult-prompt draft described the session-side
`ResolverObservation` implementor as something that "internally holds
`HostStoreView`/`&VerterHost`" without qualification. Codex flagged this
imprecisely stated: the LOCKED trait boundary (F3, scoping-spec:314) still
holds — `HostResolverContext`/`SessionResolverContext` never implement the
new trait; only a distinct session-side driver type (not yet designed,
deferred to the phase-6 wiring step) implements it, and that driver is the
one permitted to hold the host reference. `ResolverObservation` itself gains
zero new host-naming surface.
