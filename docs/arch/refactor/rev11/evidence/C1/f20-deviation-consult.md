# C1 twentieth deviation — F20: item 6's dual-runner harness home + `resolve_with_reader` visibility

Found while starting item 6 (the dual-runner harness): F18/F19's design
never addressed WHICH CRATE hosts `ResolutionFixture` and its two
runners, or `ProjectResolver::resolve_with_reader`'s visibility. Since
the kernel runner needs `verter_semantic`-owned types
(`ResolverAttemptView`/`priority_frontier`, unreachable from
`verter_workspace` — confirmed `verter_workspace` has no `verter_semantic`
dependency) and the legacy runner needs `resolve_with_reader` (private,
reachable today only from within `verter_workspace` itself), no single
crate could reach both without a visibility change I was not authorized
to make unilaterally — my own prior-round report's scoping ("without
touching `resolver.rs`'s production code path at all") was revealed
impossible for a REAL comparison harness. Full consult prompt/output:
`/tmp/c1-item6-harness-home-prompt.md` / `/tmp/c1-item6-harness-home-output.md`
(not committed — ephemeral scratch; this file plus the rewritten
sequencing record are the durable record).

## Verdict

**Put the harness inside `verter_semantic`; do not make the raw legacy
method production-public. Item 6 needs a small implementation-note
correction, not an ownership redesign.**

## 1. Harness location

`verter_semantic::resolver_core`, a `#[cfg(test)]` unit-test module (e.g.
`resolver_core/resolution_dual_runner_tests.rs`). Verified the graph
directly: `verter_workspace` has no `verter_semantic` dependency;
`verter_semantic` depends on `verter_workspace`; `verter_session` depends
on both but CANNOT reach `priority_frontier` regardless — it is
`pub(crate)` to `verter_semantic` (item 4's own ratified design: "a
REUSABLE PRIVATE helper... not a new public abstraction"), so widening it
for a session-hosted harness would itself violate item 4. `verter_semantic`
is the only crate that can reach both `ResolverAttemptView`/
`priority_frontier` (its own `pub(crate)` internals) and, via a
test-support bridge (below), the legacy algorithm.

## 2. `resolve_with_reader` visibility — stays private

**No, `resolve_with_reader` must NOT become production `pub`.** There is
an EXISTING architecture guard whose CONTRACT this would break:
`crates/verter_session/tests/cases/compile-fail/
raw_resolver_entry_points_are_private.rs` (driven by
`g_compile::compile_fail::raw_resolver_entry_points_are_private`, gated
behind the `compile-fail` Cargo feature) pins that `resolve_with_reader`/
`resolve_for_project_with_reader`/`preferred_specifier` stay private
because ONLY the Engine transaction may mint a resolution witness. Making
any of them production-`pub` would intentionally break that guard.

No suitable existing public wrapper exists either (`WorkspaceRead::
resolve_import` defaults to `None`; `resolve_tracked` is `pub(crate)` and
requires the private Engine capability + `TransactionReader`; neither
lets an external test supply an arbitrary `ProjectResolver` + test reader
pair).

**Minimal fix, landed this round**: a `#[cfg(any(test, feature =
"test-support"))] pub mod test_support` bridge inside `resolver.rs`
itself, exposing ONE free function
(`test_support::legacy_resolve_with_reader`) that calls the still-private
`resolve_with_reader`. The wrapper is `pub` ONLY in test-support builds;
`resolve_with_reader` itself is untouched — the compile-fail guard still
targets it directly and still fails to compile as designed (verified:
`raw_resolver_entry_points_are_private` passes green, 48s once caches
were warm — its own first cold-cache attempt hit nextest's 360s timeout
under heavy sibling load on this shared machine, a resource artifact, not
a compile result; the retry with warm caches passed cleanly).
`verter_semantic` already activates `verter_workspace`'s `test-support`
feature via `[dev-dependencies]` (landed last round for the
`ResolutionBasis` `test_only` constructors) — no new Cargo edge needed.

## 3. Item 6 scope and sequencing record — corrected wording, not re-ratified

Stays within item 6; does not affect item 2's final `ModuleResolverCore`/
`IdeProjectConfig` ownership decision — the harness's location says where
the transitional COMPARISON test can reach both sides, not where the
final production driver or algorithm lives.

Corrected framing (replaces "without touching `resolver.rs`" in the prior
report and this document): **"without changing any production-reachable
`resolver.rs` behavior or call path."** The bridge is physically declared
in `resolver.rs` but compiles out of every production build.

Item 5's deletion list (not yet written in full) must additionally name:
delete the `test_support` bridge, and replace the test-local kernel slice
with calls to the real `ModuleResolverCore::resolve_attempt` once it
exists — at the same atomic cutover that deletes `resolve_with_reader`
itself.

## 4. Traced algorithms — confirmed correct, one addition

Both of my own pre-consult traces (positive case: 2 probes + 1 realpath
via `resolve_ts_source_sibling`'s `.js` -> `[".ts", ".tsx"]` list, short-
circuiting before `probe_path`; miss case: 24 probes via `probe_path`'s
bare-extension-then-index scan, zero realpaths) were confirmed correct
against direct source reads. One addition: the positive case's recovery-
scope set also contains `/` (root) alongside the test's explicitly
asserted `/p`/`/store`/`/store/pkg` — not a contradiction (the test never
asserts `/`'s absence), just a fact the witness recording produces that
the existing test doesn't bother to check. Also noted:
`package_follow_is_confirmed` runs in the positive case (trivially `true`
since `/p/main.ts` isn't under `node_modules`) — contributes no
additional probes/facts for this fixture, but any general port of
`resolve_source_id_unowned`'s relative branch should still call it (or an
equivalent) for fidelity, even though it's a no-op here.

## 5. Recommended kernel-runner shape

**A narrow test-only relative-path slice IS acceptable scope for item 6.
A partial production `ModuleResolverCore` API is NOT.**

- One fresh, immutable `ResolverAttemptView` snapshot per attempt — never
  a single mutable view reused across retries (the input-loading contract
  requires one immutable view per attempt).
- `ResolverAttemptView::workspace_only(...)` closures over a growing
  loaded-facts snapshot: `Complete(value)` when loaded (including stable
  `Absent`/`None`, distinct from unloaded), `NeedInputs(InputKey)` when
  not yet loaded.
- A GENERAL (not fixture-name-specific) candidate-generation + evaluation
  function implementing: relative/absolute base-path construction;
  JS-family source-sibling candidates (`resolve_ts_source_sibling`'s
  logic); declaration-companion candidates gated by
  `prefers_declaration_files` (`resolve_declaration_companion`'s logic);
  the 12 bare-extension-or-as-is candidates then the 12 index candidates
  (`probe_path`'s logic); probe-then-realpath-on-hit per candidate
  (`resolve_existing_path`'s logic) — flattened into ONE ordered
  candidate list fed through the REAL `priority_frontier`, since
  `probe_path_for_context`'s own nested short-circuit structure (source-
  sibling, then declaration-companion, then bare probe_path) is itself
  already a priority-ordered "try candidates in sequence, first hit wins"
  chain — exactly `priority_frontier`'s own model.
- Outer retry-loop driver: run `priority_frontier`; on `NeedInputs`, load
  the requested keys from the fixture into the snapshot, build a fresh
  view, retry; a repeated empty delta fails as no-progress rather than
  looping forever.
- Compare, per case: full semantic result; ordered primitive
  observations/consumed selectors; recovery-scope set; kernel-only
  `NeedInputs` waves. Full `ResolutionFactKey` replay/signature comparison
  (F18's original ask) needs a SEPARATE small `verter_workspace/
  test-support` replay helper (since `ResolutionFactKey` constructors are
  workspace-private) — deferred as one of item 6's own named follow-on
  tests, not required for the first two cases.

Narrowing counts as satisfying item 6's first two cases ONLY if the
runner: genuinely retries from `NeedInputs`; actually calls
`ResolverAttemptView` and `priority_frontier` (not a shortcut); derives
candidates algorithmically (not the two fixtures' literal answers
hard-coded); compares the completed consumed witness, not a hard-coded
expected success value. It does NOT satisfy C1's eventual full-surface
requirement and must NOT survive the atomic cutover as a second resolver
implementation — item 5's deletion list must remove it at cutover.

## Explicit instruction, followed

"No files were changed during this review." This round's next actions:
(1) land the `test_support` bridge (already verified, this evidence file
documents it); (2) update the sequencing record's item 6 wording per
section 3 above; (3) build the dual-runner harness per section 5's shape,
porting the two witness-contract cases.
