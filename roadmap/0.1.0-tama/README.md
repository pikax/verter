# Tama 0.1.0 roadmap

This directory is the live Tama roadmap for Verter 0.1.0. It carries the ratified Revision 11 architecture forward as an execution program rather than documentation. Static work definition lives in `authority/dag/` and `charters/`. Implemented state lives in one intentionally simple file: `authority/state/implemented.toml`.

A node is implemented when its predeclared `[implementation]` line has `status = "implemented"`. Its `commit_message`, `commit_date`, and optional `pull_request` are loose locator hints for a person or agent who later wants to find the work. They are not identity, proof, or validator inputs. Flipping the row back to `status = "pending"` is the deliberate operation that marks a node unimplemented.

The same file may contain separate `[[github_issue]]` rows with `node_id`, `gh_issue`, and required `sync_to_github`. Those rows are a local lookup table and mutation policy only; they never mark a node implemented. `true` opts an issue into deterministic managed-label synchronization and explicit one-way content refresh. `false` protects a pre-existing issue manually mapped into the DAG. After GH6, `githubctl sync-issues` creates missing opt-in issues and reconciles the versioned catalogs without rewriting existing issue prose. Before creation or `--refresh-content`, author the selected node's reviewed `catalogs/github-issue-content.toml` entry; missing or invalid content fails before GitHub mutation. Opt-in descriptions follow the human issue standard in `contracts/github-control-plane.md`; they explain the problem, expected outcome, and observable acceptance in standalone prose rather than copying charter sections. GitHub edits never flow back, and protected issues are never read or rewritten.

Each independently landable node uses its own worktree, branch, and candidate by default, plus its own PR when GitHub control is active. A shared multi-node train worktree is an explicit atomic-landing exception, not the normal orchestration shape. With GitHub control active, the reviewed PR is squash-merged through GitHub as the landing path; GitHub is not an after-the-fact mirror of a local landing. For an explicitly directed non-PR landing, the reviewed squash commit body carries one `Closes #<gh_issue>` line per included node so the issues close when that commit reaches the origin default branch.

Production LOC and file budgets are planning references rather than hard acceptance lines; material mismatch is investigated for hidden scope. During train execution, a fresh Codex Architect checks cumulative conformance after each tranche of 3 to 6 implemented blocks. The final intended block additionally receives a fresh cumulative train review against the complete current authority, including reviewed amendments.

There are no commit-SHA, tree, parent, ancestry, receipt, lease, activation-journal, authority-digest, or prompt/report-digest checks in the lifecycle. Agents are trusted to transition accurate rows to implemented and to obey charters, review profiles, and gates.

`programctl frontier` is only a stateless convenience report. It derives the currently dispatchable nodes from DAG ancestors and implemented-line presence; it does not start, reserve, activate, or write anything. A node with no unimplemented ancestor can start immediately.

Core commands from the repository root:

```text
node roadmap/0.1.0-tama/tools/programctl.mjs frontier
node roadmap/0.1.0-tama/tools/programctl.mjs explain ID
node roadmap/0.1.0-tama/tools/programctl.mjs packet ID
node roadmap/0.1.0-tama/tools/programctl.mjs implemented
node roadmap/0.1.0-tama/tools/programctl.mjs github-issues
node roadmap/0.1.0-tama/tools/programctl.mjs github-issue NUMBER
node roadmap/0.1.0-tama/tools/validate-program-dag.mjs --strict
node scripts/githubctl/githubctl.mjs check
node scripts/githubctl/githubctl.mjs sync-issues --check --train TRAIN
node scripts/githubctl/githubctl.mjs sync-issues --apply --train TRAIN --create-blockers
node scripts/githubctl/githubctl.mjs sync-issues --apply --train TRAIN --ignore-blockers
node scripts/githubctl/githubctl.mjs sync-issues --apply --nodes ID --refresh-content
```

See `APPLICATION.md` for the implementation and review workflow. The historical failure, maintainer ruling, and reasons for intentionally superseding ORC0 are recorded in `decisions/2026-08-28-trusted-implementation-ledger.md`. The simpler post-GH issue mapping and PR flow are recorded in `decisions/2026-08-28-minimal-github-issue-mapping.md`. The maintainer-directed authority correction to the rev11.flow D-train charters (source-verified owners and API boundaries, AMD-004 debt ownership, atomic D1+D2 landing) is recorded in `decisions/2026-08-29-rev11-flow-authority-correction.md`. The round-2 correction (D1 as private hermetic checkpoint, D2 as the indivisible contract-§6 cutover, the A6 flow-rows ratification gate, and corrected RESIDUAL-NON-CALL-ANY-FABRICATION ownership) is recorded in `decisions/2026-08-29-rev11-flow-d1-hermetic-checkpoint.md`. The codex scope ruling splitting D2 into the D2A substrate and D2B cutover nodes (D1+D2A+D2B as one atomic multi-node candidate, gh_issue 174 rekeyed to D2B, D2A intentionally unmapped) is recorded in `decisions/2026-08-29-rev11-flow-d2-split.md`. The codex scope ruling splitting D3 into the D3R/D3I/D3P/D3C atomic four-node candidate (gh_issue 175 rekeyed to D3C, D3R/D3I/D3P intentionally unmapped, D4–D6 repointed to D3C) is recorded in `decisions/2026-08-30-rev11-flow-d3-split.md`. The D2B budget amendment (atomic cutover exceeds the standard rescope trigger; codex-ruled budget amendment rather than a further split) is recorded in `decisions/2026-08-30-rev11-flow-d2b-budget-amendment.md`. The canonical-type-algebra predecessor ruling (TA1A/TA2 as explicit D2B predecessors, the TA1/TA2 mechanism split, ratified budget variances, and the heritage-carrier and bounded-work corrections) is recorded in `decisions/2026-08-31-canonical-type-algebra-predecessor.md`. The D2C DEFER ruling and debt row (`FLOW-RETURN-AUDIT-PARTIALITY`, a new `D2C` node gating `D3R` dispatch) is recorded in `decisions/2026-09-01-d2c-flow-return-audit-partiality-defer.md`. The product-model/source-module/unplugin and paired Nuxt–SvelteKit activation ruling is recorded in `decisions/2026-09-01-product-surface-ownership-and-paired-project-profiles.md`.
