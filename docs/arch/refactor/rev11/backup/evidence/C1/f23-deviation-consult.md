# F23 — Stage-2 atomic-cutover sequencing consult

**Trigger:** with item 2's entire owned-function-surface enumeration
ported (all four real public entry points assembled), the natural next
question is whether Stage 2 (the atomic cutover) is ready to plan/
execute. Per F21's own framing, the cutover is a genuinely irreversible
step needing a ratified plan before execution — dispatched a fresh
Codex xhigh consult rather than deciding inline.

**Command:** `PATH="$HOME/.codex/plugins/.plugin-appserver:$PATH"
codex exec --sandbox read-only -m gpt-5.6-sol -c model_reasoning_effort=xhigh
< prompt.md > output.md` — full prompt and raw output preserved at
`/tmp/c1-f23-prompt.md` / `/tmp/c1-f23-output.md` (not committed;
condensed here).

## Fresh re-grep basis (via a dispatched Explore sub-agent, read-only)

Re-verified the F18/F22 consumer inventory against the CURRENT tree
before consulting. Headline corrections:

- **`resolver_core/` is no longer a blank future target** — it now
  contains ~23 modules / ~10.6K lines, a complete inert port, most
  files landed the same day. The inventory/plan must treat this as
  substantially-built Stage-1 state, not a design sketch.
- **LSP `.resolver` touch count**: 11 files, not 8 — `background_init.rs:767`
  (test-only, not a production blocker) plus two genuine production
  blockers Codex's answer separately confirmed: `server/mod.rs:190/199`
  (`ServerState.resolver: NativeProjectResolver`, a DIFFERENT field
  than `WorkspaceSnapshot.resolver` but still a genuine migration
  site) and `external_ts/carrier_sync.rs:624` (`CarrierSyncRequest`'s
  own `resolver` field, same situation). Net: **10 production
  migration files**, not 8 or 11.
- The prior pass's "A5-DD1" label does not literally exist anywhere in
  the repo — the real mechanism is `ratified_upward_exceptions()`
  (~line 140-149 of `workspace_dependency_layers.rs`) plus
  `RATIFIED_ROOT_CRATES` (line 335, confirmed exact). Treat "A5-DD1"
  as informal prior-round shorthand, not a literal name to grep for.
- Everything else (DTO definitions, N-API/WASM line numbers,
  `resolver_tests.rs` line count, `to_native_project_resolver`'s
  single test-only caller, the compile-fail fixture) confirmed
  ACCURATE, unchanged from F18/F22.

## Codex's verdict: **NO-GO today** — a narrow, specific gap

The algorithm port itself is judged complete. What's missing before
Stage 2 is safe:

1. **No `ModuleResolverCore` struct/shell exists yet.** Everything
   ported so far is free functions across 23 modules; there is no
   actual public core type wrapping them with the four real methods
   (`resolve_attempt`/`resolve_for_project_attempt`/
   `preferred_specifier_candidates`/`project_exact_result`) plus the
   owner-selection methods, storing immutable graph/config state only
   (`configs: Arc<[IdeProjectConfig]>` per the settled sketch — no
   reader, manifest cache, transaction, or workspace handle).
2. **The dual-runner harness (item 6) only covers a narrow relative-
   probe slice** (the original 2 ratified witness-contract cases) —
   Codex wants FULL top-level differential coverage against the REAL
   legacy resolver across every branch: relative/absolute, workspace
   alias/`paths`/`baseUrl` precedence, owner overlap/explicit-project,
   project references (incl. unresolved refs + cycles), `#imports`,
   scoped/unscoped node_modules, `exports` array/object/conditions +
   legacy fields, provider/carrier projection, preferred candidates,
   exact-result projection.
3. **Comparison scope needs to widen beyond final `source_id`**: also
   compare the ordered consumed-selector list, `NeedInputs` wave
   shape, recovery scopes, and the replayed `ResolutionFactKey`
   set/signature — not just whether the two engines agree on the
   final answer.
4. **Specific replay-contract gaps named as still-open** in the
   ledger: manifest fingerprint preserving `name`, `DirectoryMembers`
   consumed-vs-prefetched distinction, complete fact replay/signature,
   basis-restart behavior on the REAL driver (not just the
   characterization harness), no-progress/terminal/transient-load-
   failure behavior.

Explicitly: NOT asking for randomized fuzzing as a gate — "a
deterministic branch-complete differential matrix plus replay-contract
tests is the higher-value requirement."

## Ratified answers for when Stage 2 does execute (recorded now, to act on later)

- **One atomic commit, no split** — explicitly rejects a Cargo-edge
  split, a code/guard split, or any multi-commit sequence with an
  intermediate broken/dual-resolver state. "Before commit:
  `ProjectResolver` is the sole production resolver. After commit:
  `ModuleResolverCore` is the sole production resolver." (Working-tree
  intermediate states during the edit are fine; nothing intermediate
  ever lands as its own commit.)
- **DTO disposition — all 15 named DTOs move to `verter_semantic`**
  (canonical def under `verter_semantic::resolver_core`), with
  `verter_workspace` keeping crate-root `pub use verter_semantic::
  resolver_core::{...}` VALUE re-exports (not a resolver-authority
  shim — explicitly distinguished from the forbidden forwarding-
  wrapper pattern). `ProjectMembership` is the sole exception — stays
  workspace-owned, no semantic re-export, matching F18's existing
  note.
- **Env-hash closure moves with the struct**: `IdeProjectConfig`'s
  five env-hash methods, `EnvHashInputs`/`ModuleResolutionMode`/
  `ConditionSet`/`SpecifierKind` all move into semantic-owned
  resolution modules; use semantic's dependency-neutral `Hash16`
  rather than creating a new semantic→scheduler edge.
- **Membership boundary, precisely split**: semantic owns
  `ConfiguredMembership::contains`/`directly_includes`,
  `StaticMembershipSpec::matches`, compiled glob matching,
  `typescript_default_excludes`; workspace retains `ProjectMembership`,
  `FallbackMembership`, `SupportedExtensions`, config-ingress
  conversion, and `materialize_from_spec` (the genuine filesystem
  walk) — workspace constructs the completed semantic-owned DTO and
  hands it to the kernel.
- **Cargo-edge disposition, explicit**: `verter_workspace` MUST gain a
  normal dependency on `verter_semantic` post-cutover (workspace
  stores/drives `ModuleResolverCore`, constructs semantic DTOs,
  re-exports semantic values — "depends on neither" is impossible).
  `verter_semantic`'s current dependency on `verter_workspace` is
  REMOVED (both the normal dep in `Cargo.toml` and the `test-support`
  dev-dep, once the dual-runner harness is deleted). Final direction:
  `verter_workspace --production--> verter_semantic`, zero edge back.
- **`verter_identity` guard flip, same commit, exact edits**: remove
  the `verter_semantic` entry from `ratified_upward_exceptions()`
  (keep the `verter_diagnostics` entry — it has an independent,
  unrelated reason to reach workspace/scheduler/tsgo); shrink
  `RATIFIED_ROOT_CRATES` to `&["verter_diagnostics"]`; replace the
  semantic→workspace canary (lines 424-430) with a BOTH-DIRECTIONS
  assertion (semantic's production closure must NOT reach workspace;
  workspace's production closure MUST reach semantic). Codex is
  explicit that landing code and guard flip separately is unsafe in
  either order (too-permissive if code lands first, false-failing if
  guard lands first) — same commit is the only correct choice.
- Also surfaced additional non-resolver `verter_semantic ->
  verter_workspace` references that must close in the SAME cutover
  (found while reasoning about the edge reversal, not previously
  enumerated in item 5): `FactVersionRef` + its embedded
  resolution-fact key/ref/population closure, `ProjectStableKey` value
  operations + `AmbientSymbolHit` (keep the ownership-derived
  `ProjectStableKey::from_project` projection workspace-side),
  `PathProbe`, `WorkspaceAuthorityId`/`ResolutionPopulation`/
  `ResolutionWorldId`, the fact-registry wildcard shim, the route
  analyzer's workspace `DirEntry` dependency (replace with a
  semantic-owned route-input row; session walker converts). **This
  list needs its own verification pass before Stage 2 — recorded here
  as Codex's finding, not yet independently re-confirmed.**

## Disposition

**NOT a rule conflict, not a STOP condition** — a clear, actionable
verdict with concrete next steps. Continuing Stage-1 preparation per
Codex's named gap list: (1) build the `ModuleResolverCore` struct
shell, (2) expand item 6's dual-runner harness to full top-level
differential coverage, (3) widen comparison scope beyond final
`source_id`, (4) close the four named replay-contract gaps. Item 5's
DTO/Cargo-edge/guard-flip answers above are RATIFIED FOR WHEN Stage 2
executes — recorded now so a future round doesn't have to re-derive
them, but Stage 2 itself remains NOT STARTED until the Stage-1 gap
closes.
