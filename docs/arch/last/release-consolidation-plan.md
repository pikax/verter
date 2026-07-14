# Verter release consolidation authority

Status: active integration authority. This document owns the raw integration sequence and is
updated when evidence changes. It is intentionally omitted from the reconstructed clean release
tree; the clean tree instead carries the final release-state/remaining-work document.

## Frozen inputs and protected state

- ParseLower: `origin/refactor/semantic-db-overhaul` at
  `300be6dcd4dda66a68634b837f30fd4fc408df20`.
- LSP/editor: local `fix/lsp-provider-parity` at
  `0e27ba138deabbc94698f95b02f4976f105baf40`.
- Optional Svelte compiler: `origin/feat/framework-adapters-clean` at
  `1fc8b2323f38cfa99ab069f654d5aca07712a2a5`.
- Release base: `origin/main` at `5c62b6b505b456d61074b18e0fb6d5578b4605b4`.
- Rejected family-selection experiment: `block/1-min-repro-fix` at
  `746f01029d4ca71f6bdea764f69a0c8ff6278359`; evidence only, never a merge input.
- Preserved E2E work: `preserve/verter-e2e-ab` at
  `329f2f69349d6008aff10117217a71ba76f60563`; salvage only after first-hand review.
- Archive refs live under `refs/archive/verter-release-consolidation/{parselower,lsp,framework-adapters}`.
- Raw integration branch/worktree: `codex/release-consolidation-raw` in the isolated
  release-consolidation worktree, based on the exact ParseLower tip.
- Final branch/worktree: `codex/release-clean`, reconstructed later from the exact release base.
- The main checkout and its untracked `REVIEW-B-DONE`,
  `packages/playground/public/tsgo.wasm`, and
  `packages/playground/public/wasm_exec.js` are protected. No command in this work uses, stages,
  moves, cleans, or deletes them. No source branch is rewritten. No push or merge to `main` occurs.

## Release scope and conflict ownership

The release combines the complete ParseLower semantic/cache cutover with the editor-owned
TypeScript serving behavior from the LSP line. ParseLower owns semantic meaning, fact/cache
identity, admission, projection, session/query internals, and the single-engine boundary. The LSP
line owns editor behavior, provider acceptance, source mapping, and non-vacuous editor tests.
Conflicts are resolved hunk by hunk: retain both discriminating test surfaces; retain ParseLower's
cache/session semantics; port LSP-facing behavior onto those semantics. No blanket ours/theirs
resolution is permitted.

The rejected `provider_decision` / `editorEngine` / `--editor-engine` experiment and its selector
family do not return. Policy, runtime identity, and attach feasibility are separate facts. Batch
`verter-tsc` is not part of editor engine lifecycle.

## Evidence-backed defects to close

1. Workspace Clippy is the first verification deliverable. The ParseLower tip is known to expose
   83 findings (78 dead-code and 5 style findings). Each symbol is classified from callers and
   tests: retire dead `TypeExpr` code and its obsolete tests together, make genuinely test-only
   helpers `#[cfg(test)]`, preserve intentional data carriers, and fix style findings directly.
   Blanket allow attributes are forbidden. The steady state is zero workspace warnings.
2. Cache admission must be owner-scoped around the complete cold computation. By-value sealed
   evidence is minted only after fact tracing, completeness, overflow/cancellation, provenance,
   invalidation, and signature validation. Non-cacheability is an intrinsic typed finalisation
   result, not a sibling boolean. Invalid/partial results may be returned but never admitted.
   Raw write/read mutators are test-only or deleted. Symbol, fallthrough, component-meta, route,
   imported-root, follower/parent, facts-less, and singleflight paths are all audited with mutation
   tests that prove changing each validation input changes admission.
3. Projection recursion is replaced with an explicit staged heap worklist. The implementation must
   preserve exact evaluation order, laziness, memo visibility, context, substitution install and
   rollback, and completeness propagation. Work and host-depth fuses are independent; there is no
   structural depth cap. A 2 MiB subprocess must demonstrate the pre-fix failure and post-fix
   success. Shallow and broad workloads must retain their allocation/throughput behavior.
4. Exactly one production semantic engine remains. All production surfaces dispatch through it;
   the parser becomes syntax/locator extraction only. The second query-time expansion/frontier
   authority, dead `ResolvedNamedType`/`VueMacroElements` storage, and terminal semantic
   `TypeExpr` carriers/bridges are deleted. Query execution remains behind the intended
   `verter_session_query` dependency firewall and structural guards prove forbidden parser/OXC
   dependencies cannot enter its closure.
5. Gate integrity is repaired before serving behavior is accepted. At the frozen LSP tip,
   `.github/workflows/ci.yml` disables both VS Code E2E jobs with `if: false`, while
   `packages/vue-vscode/package.json` names only eight unit specs. The canonical gate must derive
   its universe independently from the tree, execute every owned spec exactly once, always emit an
   input-bound summary, and fail on disabled/missing jobs, missing/stale summaries, zero execution,
   unexpected skips, invalid timeout nesting, absent fixtures/providers, and plants that did not
   apply. The preserved E2E tag contributes only three reviewed candidates: unconditional
   non-vacuity, unconditional provider-sync readiness, and readable failure summaries.
6. Editor serving order is live editor-owned first:
   - attach to the exact VS Code Native Preview `tsgo` Program/session/project;
   - activate the Verter TypeScript plugin in the editor-owned `tsserver` project;
   - only after an observable bounded attach/plugin failure start the pinned managed `tsgo`;
   - terminate, kill, and reap managed children on bounded failure or shutdown.
   Session, Program, project, engine, and process identities are explicit and observable. The
   frozen D1 test currently documents a Verter-owned relay that spawns an independent second tsgo;
   that interim mode is removed. Acceptance proves editor reuse or plugin activation, no duplicate
   engine process in the healthy path, managed fallback only after observed failure, bounded
   failover, neutral editor facts, and operator-visible provenance.
7. The original Native Preview "No Project" reproduction is a pre-fix red and final green
   acceptance. Every assertion reads behavior (diagnostics, resolved component surface, mapping,
   process/identity receipts), not source text or log-substring presence. The same fixed session is
   mutated to prove the negative controls.

## Implementation and verification order

1. Commit this authority and record frozen refs/archive refs.
2. Run and classify the ParseLower workspace Clippy baseline; close findings in small verified
   groups.
3. Merge the exact LSP tip into the raw branch and resolve conflicts under the ownership rules
   above. Keep the raw merge/integration history for evidence only.
4. Close cache admission, projection worklist, terminal `TypeExpr`, parser-expander, and
   single-engine gaps with TDD. Run focused `cargo check`, focused tests, and Clippy after each
   group.
5. Provision real provider fixtures, pin and record the tsgo path/version, repair the canonical
   gate/CI aggregator, and salvage only valid preserved-tag changes. Both
   `VERTER_REQUIRE_TSSERVER=1` and `VERTER_REQUIRE_TSGO=1` are required for the release gate.
6. Implement editor-owned attach/plugin-first serving, managed fallback, lifecycle cleanup, and
   the original reproduction. Run the real editor matrix and behavioral mutation controls.
7. Reconcile owning architecture/skill docs to the final code and remove stale status reports,
   machine-local paths, orchestration transcripts, and phase archaeology. One concise final
   release-state/remaining-work document distinguishes shipped support from deferred work.
8. Reconstruct `codex/release-clean` from the exact main base in roughly 3-7 conventional,
   buildable commits. Subjects and bodies contain no WIP/checkpoint/stage/phase/block/train/round,
   agent/model/handoff, attribution, or trailers. Raw and clean final trees are byte-identical
   except for this intentionally omitted raw-process authority.
9. Run the complete release gate on the final clean SHA with `CARGO_BUILD_JOBS=2`: session
   lib/tests checks, workspace Clippy `-D warnings`, fmt, `cargo nextest run --workspace`,
   `cargo test -p verter_session --tests`, `pnpm test`, provider-required LSP/editor E2E, the
   inspected canonical gate driver, freshness/conformance checks, architecture/portability/file-size
   guards, no-archaeology guards, and mutation plants. Record executed/pass/fail/skip counts and
   provider provenance. No red baseline is accepted.
10. Freeze that SHA. Only then run exactly three independent cumulative reviewers over the same
    immutable tree: semantic/cache/ParseLower; LSP/host/editor; release integrity. Valid findings
    are fixed once in a consolidated change, the full gate is rerun, a new SHA is frozen, and all
    three reviews repeat. Completion requires 3/3 approval on one identical SHA.

## Provider provisioning contract

The release records absolute resolved executable paths, versions, and source/provisioning receipts
for tsserver and tsgo without committing machine-local paths. Required-mode provider discovery may
not skip. Missing provider assets, missing fixture dependencies, zero selections, pending tests,
timeouts, or absent summaries are failures. CI and local canonical entry points use the same
tree-derived manifest and always-running aggregator.

## Optional Svelte decision

Deferred. The frozen Svelte handoff explicitly reports only 6/10 release trains confirmed and names
T4 (script completion), T5 (reachability), T6 (quality gates), and T7 (release close) as remaining
release-blocking work. Inclusion would therefore require implementing the forbidden missing T4-T7
scope. The Svelte ref stays archived and unmerged. This does not weaken Vue/ParseLower/LSP release
criteria, and no documentation claims Svelte native-compiler completion.

## Final documentation and deferred work

Owning documentation changes travel with the code they describe: type resolution/cache rules,
component-meta, host session/editor serving, testing/E2E, compiler/codegen only where the required
single-engine cutover changes it, and the unified semantic-db remaining plan. The final release-state
document records only shipped behavior, verified counts/provenance, supported-editor facts, and
genuinely deferred post-release work. The semantic-db continuation is reconciled but not started.

Permitted deferrals are fail-closed unsupported/completeness work outside the required release,
including the unfinished optional Svelte trains and later measured cache compaction. Cache
correctness, projection stack safety, single-engine completion, provider attach order, lifecycle
cleanup, the original reproduction, non-vacuous gates, warnings, and release documentation are not
deferrable.

## Progress ledger

- 2026-07-14: frozen refs verified; archive refs and isolated raw worktree created; main checkout
  protected; ParseLower, LSP, gate-integrity, Stage 10, and optional Svelte handoffs read from their
  owning refs; Svelte classified deferred from its own 6/10 release status.
- Raw integration SHA: pending.
- Clean release SHA: pending.
- Current failing gate/evidence: workspace Clippy baseline and raw merge not yet executed.
