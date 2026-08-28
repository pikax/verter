# Tama 0.1.0 roadmap

This directory is the live Tama roadmap for Verter 0.1.0. It carries the ratified Revision 11 architecture forward as an execution program rather than documentation. Static work definition lives in `authority/dag/` and `charters/`. Implemented state lives in one intentionally simple file: `authority/state/implemented.toml`.

A node is implemented when its `[[implemented]]` row exists. Its `commit_message`, `commit_date`, and optional `pull_request` are loose locator hints for a person or agent who later wants to find the work. They are not identity, proof, or validator inputs.

The same file may contain separate `[[github_issue]]` rows with `node_id`, `gh_issue`, and required `sync_to_github`. Those rows are a local lookup table and mutation policy only; they never mark a node implemented. `true` opts an issue into one-way DAG/charter refresh. `false` protects a pre-existing issue manually mapped into the DAG. After GH6, an explicit `githubctl sync-issues` run may create missing opt-in issues or refresh an opt-in mapped issue after a rescope/content change. GitHub edits never flow back, and protected issues are never rewritten.

There are no commit-SHA, tree, parent, ancestry, receipt, lease, activation-journal, authority-digest, or prompt/report-digest checks in the lifecycle. Agents are trusted to add accurate rows and to obey charters, review profiles, and gates.

`programctl frontier` is only a stateless convenience report. It derives the currently dispatchable nodes from DAG ancestors and ledger-row presence; it does not start, reserve, activate, or write anything. A node with no unimplemented ancestor can start immediately.

Core commands from the repository root:

```text
node roadmap/0.1.0-tama/tools/programctl.mjs frontier
node roadmap/0.1.0-tama/tools/programctl.mjs explain ID
node roadmap/0.1.0-tama/tools/programctl.mjs packet ID
node roadmap/0.1.0-tama/tools/programctl.mjs implemented
node roadmap/0.1.0-tama/tools/programctl.mjs github-issues
node roadmap/0.1.0-tama/tools/programctl.mjs github-issue NUMBER
node roadmap/0.1.0-tama/tools/validate-program-dag.mjs --strict
```

See `APPLICATION.md` for the implementation and review workflow. The historical failure, maintainer ruling, and reasons for intentionally superseding ORC0 are recorded in `decisions/2026-08-28-trusted-implementation-ledger.md`. The simpler post-GH issue mapping and PR flow are recorded in `decisions/2026-08-28-minimal-github-issue-mapping.md`.
