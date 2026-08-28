# F24 — gap-4 disposition + Stage-1 completeness re-check

**Trigger:** F23 gave a NO-GO with four named gaps; three (algorithm
port, `ModuleResolverCore` struct, expanded differential harness) were
closed the same round. Before treating Stage 1 as complete, dispatched
a narrow follow-up consult specifically on gap 4's disposition (is it
Stage-1-addressable or genuinely Stage-2-only) and asked Codex to
re-check whether closing gaps 1-3 changes the overall verdict.

**Command:** same `codex exec` invocation as prior consults. Full
prompt/output at `/tmp/c1-f24-prompt.md` / `/tmp/c1-f24-output.md` (not
committed; condensed here).

## Gap 4 disposition — settled

**Gap 4 is Stage-2-only**, re-scoped as Stage-2 acceptance criteria,
not a Stage-1 blocker. Reasoning: all five gap-4 items (manifest
fingerprint `name` preservation, `DirectoryMembers` consumed-vs-
prefetched, complete fact replay/signature, basis restart on the real
driver, no-progress/terminal/transient-load-failure behavior) describe
behavior of the WORKSPACE-OWNED retry/replay DRIVER, which does not
exist as ported code today — the current production path
(`verter_workspace::engine.rs` + `resolution_currency.rs`) is
synchronous with no `NeedInputs` loader loop, and a faithful inert
driver cannot land cleanly before the Cargo-edge reversal (`verter_semantic`
currently depends on `verter_workspace`; the real driver needs the
reversed edge). Do NOT build a semantic-owned surrogate driver or
another test-only retry implementation meant to be deleted — the real
driver and its replay ledger land IN the Stage-2 atomic change, with
gap 4's five items as that change's acceptance checklist (a concrete
per-item table is in the full consult output for whoever plans Stage
2's execution).

## Headline finding: the guard was RED

While spot-checking the current tree before answering, Codex found
`no_phase_archaeology_in_production_code` failing — 68 violations
across 30 files (spanning this whole block's work, not just this
round), triggered by "C1 phase-4/7 cutover"/"pre-cutover" framing,
"C1's" possessive block-identifier citations, and literal citations of
the cutover-bearing sequencing-record filename. **Fixed the same
round** (see the `d5153f845` commit) — every doc comment rewritten to
final-state, technical content preserved, guard now green, 1039
combined tests unaffected.

## Stage-1 completeness — NOT yet declared complete

Independent of the guard finding, the consult's direct spot-check of
`resolution_dual_runner_tests.rs` found the differential harness still
short of F23's own gap-2/3 asks in specific ways:

- `KernelCoreRunResult` stores consumed selectors in a `BTreeSet`,
  destroying the ORDER F23 asked to compare.
- `NeedInputs` waves are loaded and discarded inside the driver loop,
  never recorded or asserted on.
- The 12 full-driver tests dual-run only `resolve_attempt`/
  `resolve_for_project_attempt` — `preferred_specifier_candidates` and
  `project_exact_result` (the other two real core surfaces) have
  new-side-only unit tests, no legacy-vs-kernel differential case.
- Missing branch coverage named specifically: absolute-path (owned
  importer) differential case, alias/paths/`baseUrl` COMPETITION case
  (more than one branch could match — which wins), an unresolved
  (dangling) project reference, an `exports` ARRAY form (only object-
  with-conditions is covered), a carrier/provider-projection
  differential case (only unit-tested against
  `provider_projection_resolution` in isolation, never through the
  full dual-runner).

Codex's verdict: **GO for Stage-2 PLANNING**, but Stage 1 should not be
declared complete until this specific remainder closes. This is a
small, bounded, genuinely Stage-1-addressable list — not a new
unknown, not requiring another consult to scope.

## Disposition

Not a rule conflict, not a STOP condition. Two concrete follow-ups,
both continuing in this same round if budget allows:
1. (Done, same round) Fix the phase-archaeology guard.
2. (This round, if budget allows) Close the named differential-harness
   remainder: ordered selector comparison, `NeedInputs`-wave assertion,
   dual-run `preferred_specifier_candidates`/`project_exact_result`,
   and the five named missing branch cases (absolute-path, alias/paths/
   baseUrl competition, unresolved reference, exports array, carrier/
   provider projection through the full driver).

Gap 4's Stage-2 acceptance-checklist table and the DTO/Cargo-edge/
guard-flip answers already ratified in F23 remain unchanged — this
consult only narrowed gap 4's disposition and surfaced the guard/
completeness findings above.
