# F25 — final disposition of F23's 8 additional non-DTO closure items

**Trigger:** F23 ratified the atomic-cutover checklist but explicitly flagged
8 additional `verter_semantic -> verter_workspace` references (beyond the 15
core DTOs) as "needing independent re-verification, not yet confirmed."
Before executing Stage 2, dispatched two read-only ground-truth
investigations to verify each item precisely (file:line, exact coupling
shape), then this consult to rule on disposition (move / mirror / opaque
handle) for whichever items turned out to be real and undispositioned.

**Command:** same `codex exec` invocation as prior consults. Full
prompt/output at `/tmp/c1-f25-prompt.md` / `/tmp/c1-f25-output.md` (not
committed; condensed here).

## Ground truth confirmed (both investigations, cross-checked against F22's
own DTO enumeration — no new unaccounted type surfaced)

Of F23's 8 items, 2 were already fully dispositioned and needed no fresh
ruling:
- The fact-registry wildcard shim (`verter_semantic/src/facts/registry.rs:3`,
  `pub use verter_workspace::fact_registry::*;`, 16 symbols) — already the
  scoping-spec's item-1 ownership-move target; `facts/registry.rs` becomes
  the OWNER.
- The 15 core DTOs — unchanged disposition from F22/F23 (all move to
  `verter_semantic::resolver_core`; `ProjectMembership` is the sole
  exception, stays workspace-owned).

The remaining 5 were real, confirmed couplings without a settled
disposition:

1. `FactVersionRef` (`verter_workspace::fact_cache.rs:905`) — `AttemptOutput`
   stores/returns it by value only, but the type embeds a closure
   (`ResolveImportsFactRef` -> `ResolutionFactRef` -> `ResolutionFactKey`,
   the latter a 9-variant enum carrying `ResolutionPopulation` + other
   workspace-private identity types).
2. `ProjectStableKey` + `AmbientSymbolHit` — used only as an opaque identity
   key by `verter_semantic`/`verter_session`; `from_project`/`to_hex_tag`/
   `parse_hex_tag` are called exclusively inside `verter_workspace`.
   `from_project` depends on workspace-private `OwnershipProject`/
   `ProjectPayload` snapshot types.
3. `PathProbe` (`verter_workspace::resolution_currency.rs:344`) — closed
   5-variant enum, `verter_semantic`'s `ResolverObservation::path_probe`
   trait signature and its one production consumer
   (`probe_path_resolution.rs`) depend on it by value.
4. `WorkspaceAuthorityId` / `ResolutionPopulation` / `ResolutionWorldId` —
   the densest coupling: `verter_semantic`'s production `ResolutionWorldBasis`
   struct has all four fields typed directly as these three workspace types.
5. The route analyzer's `DirEntry` (`verter_workspace::error.rs:43`, plain
   `{path: String, is_dir: bool}`, confirmed NOT `std::fs::DirEntry`) —
   `RouteAnalysisInputs::directories`'s value type.

## Ruling — all 5 MUST close in Stage 2; no residual exception

| Item | Disposition |
|---|---|
| `FactVersionRef` + payload closure | **MOVE** — the whole dependency-neutral fact-reference value graph (`FactVersionRef`, `ResolveImportsFactRef`, `ResolutionFactRef`, `ResolutionFactVersion`, `ResolutionFactKey`, `ResolutionQueryKey`) moves to `verter_semantic::facts`/`facts::resolution`/`resolver_core` as appropriate. Cache authority (`ResolutionFactRoot`, world roots, mutation propagation, version counters, `ResolutionTransaction`, replay ledgers, validators, invalidation, publication) stays workspace/session-owned — this moves vocabulary, not cache authority. |
| `ProjectStableKey` + `AmbientSymbolHit` | **MOVE**, both to `verter_semantic::resolver_core`. Keep `to_hex_tag`/`parse_hex_tag` with the type (pure value ops). Replace the workspace-dependent inherent constructor with a workspace free function `project_stable_key_from_project(&OwnershipProject, &CanonicalPath) -> verter_semantic::resolver_core::ProjectStableKey` (Rust's same-crate inherent-impl constraint forces this). Ambient registry/registration/lookup storage/ownership snapshots stay workspace-owned. |
| `PathProbe` | **MOVE**, unchanged, to `verter_semantic::resolver_core`. `ResolverObservation::path_probe` returns the semantic-owned type directly; workspace VFS implementations map filesystem outcomes onto it. Workspace crate-root `pub use` is an acceptable value alias. |
| `WorkspaceAuthorityId` / `ResolutionPopulation` / `ResolutionWorldId` | **MOVE**, together with embedded `SessionFingerprint`, into semantic-owned resolution identity vocabulary (co-located with `ResolutionWorldBasis`, which is itself semantic-owned and compares this exact structured tuple). Workspace stays responsible for minting authority/world/session values and publishing captured worlds, via narrow checked constructors semantic exposes (preserving the `0`-placeholder invariant) rather than making representation fields generally mutable. |
| Route `DirEntry` | **MIRROR/PROJECT** — the one item that stays a mirror, not a move. `verter_workspace::error::DirEntry` stays canonical for VFS. Add a new semantic-owned `RouteDirEntry { path: Arc<str>, is_dir: bool }`; `RouteAnalysisInputs::directories` and its APIs switch to it. The session-side walker (`route_analysis_inputs.rs`) performs the one-way projection; semantic never names the workspace type. Legitimate because these are different domain rows (a VFS result vs. dependency-neutral route-analysis input IR) — not a duplicate authority. |

No opaque-handle disposition was justified for any item: an opaque handle
would require an interning table, serialization, downcasting, or an
allocation-bearing trait object to recover equality/hash/ordering, which
would erase the typed fact IR and introduce the cross-crate
heap/serialization seam this block forbids.

Rationale common to the 4 MOVE items: mirroring would create two encodings
of the same value at a point where exact equality/identity is the
correctness condition (ambient lookup, basis-restart comparison, fact
version equality) — introducing a conversion at that fence is exactly the
lifecycle-dependent representation drift the shared-optimized-codebase rule
exists to eliminate. `DirEntry` is the one legitimate exception because the
two sides are genuinely different domain rows, not two encodings of the same
identity.

## Scope ruling: all 5 MUST-close in the Stage-2 commit

Leaving even one blocks the required removal of the normal Cargo edge —
each currently causes `verter_semantic` to name a workspace type. The
`DirEntry` mirror still closes the edge because semantic thereafter names
only `RouteDirEntry`, never `verter_workspace::error::DirEntry`. The only
acceptable compatibility surface post-reversal is workspace-to-semantic
value re-exporting; no semantic-to-workspace alias, opaque registry,
resolver wrapper, or second canonical identity remains.

## Commit shape: ONE atomic commit retained

The confirmed breadth does not invalidate F23 — the ratified one-commit
cutover stands (`ProjectResolver` sole production resolver before; sole
production caller is `ModuleResolverCore` after; no independently landed
intermediate commit changes which resolver is in production). Concrete
reason beyond the already-ratified rule: making `verter_semantic` canonical
for the moved types requires the edge `verter_workspace -> verter_semantic`;
that edge cannot be added while any `verter_semantic -> verter_workspace`
reference remains without creating a cycle, so the value moves, remaining
reference closures, manifest reversal, driver construction, and guard flip
are one graph transition, and caller repointing plus legacy deletion are one
authority transition.

**WIP commits/checkpoints are explicitly sanctioned as long as squashed
before landing.** Internal work order:

1. Establish all semantic-owned values, fact vocabulary, `RouteDirEntry`,
   workspace projections, and workspace value re-exports.
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

The route-row projection (item 5) is the only mechanically independent
item, but peeling it off separately would not break the real SCC or reduce
cutover risk — it stays inside the same atomic Stage-2 change.

## Disposition

Not a rule conflict, not a STOP condition. Item 5's per-symbol migration
table is now fully closed (all 15 DTOs + all 8 F23-flagged items
dispositioned). Stage 2 is fully specified and ready for mechanical
execution per the 7-step work order above. No further consult is needed
before starting execution; the next round begins step 1.
