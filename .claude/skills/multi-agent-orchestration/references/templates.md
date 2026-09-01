# Orchestration prompt templates

These prompts deliberately contain no SHA, tree, receipt, lease, digest, or runtime-manifest bindings.

## Implementer

```text
Role: implementer for independently landable node <node> at effort <tier>.
Dedicated node worktree/branch: <absolute path>/<ref>. Do not mix sibling-node changes into this candidate. A shared train worktree is allowed only when the user or maintainer explicitly approved one atomic multi-node landing before mutation; record every included node plus that approval and rationale here: <not-applicable|node list, approval, and atomicity rationale>.
Deliverables: <exact scope>. Forbidden scope: <exclusions>.
Confirm the node is READY from every transitive ancestor's ledger-row presence. Read its packet and charter. Map acceptance outcomes to proportionate evidence and use TDD for behavioral changes. Implement the complete scope and run <focused commands>.
Treat production LOC and file budgets as planning references rather than pass/fail limits. Compare the cumulative candidate with them and explain material drift in either direction. If one expected production file becomes ten, investigate hidden scope and report why the candidate remains coherent or stop for an ordinary amendment; never pad or split work merely to hit an estimate.
Keep roadmap identity out of landed code and tests: no program/revision, DAG, node/block/train ID, phase/stage, implementation-sequence, or deletion-history references in production file/module names or comments, or in test file/module/test names, comments, fixtures, snapshots, assertion messages, or guard diagnostics. Describe the durable behavior or regression instead. Cite a GitHub issue only for a specific independently reported defect outside the DAG-controlled mappings, never this node's mapped issue/PR, and always keep the behavioral explanation.
Before squash or review, add a [[implemented]] row for every included node to authority/state/implemented.toml with the planned squash commit message, approximate date with timezone, and optional PR number. These are loose locator hints and must not be validated against Git or GitHub.
When GitHub control is active, resolve every included node through its local [[github_issue]] mapping before mutation and create the branch/worktree. For each opt-in mapping, schedule the issue and eligible native parent, then run `githubctl project-status --apply --node <ID> --status in-progress`, including local-only work; a protected mapping is maintainer-owned and receives no Project command. After the first implementation commit is pushed, open the draft PR with the expected final title and use it as the reviewed landing candidate. Before initial issue creation or an explicit refresh, author or update the node's stable catalogs/github-issue-content.toml entry from the charter and current source. Follow contracts/github-control-plane.md's human issue standard: write a standalone Problem, Expected outcome, and three-to-six-bullet Acceptance body; do not copy charter sections, program/DAG wording, abort conditions, budgets, gates, commands, or generic boilerplate. The renderer ends it with exactly `AI-Generated` and never a model name. Normal sync does not regenerate existing prose. If a block was rescoped or its content changed, explicitly run issue sync only when `sync_to_github = true`; a false mapping protects a pre-existing issue and must be skipped. Link every included mapped issue with its own `Closes #<gh_issue>` line, squash-merge the reviewed PR through GitHub after review/gates pass, and do not land locally first or treat GitHub as an after-the-fact mirror.
When the user or maintainer directs a non-PR landing, resolve every included node's local [[github_issue]] mapping and put one `Closes #<gh_issue>` line per node in the final squash commit body. The closing lines are required before review so pushing or merging that commit to the origin default branch closes the issues; they must not appear in source or tests. After the commit reaches the origin default branch, run `githubctl project-status --apply --node <ID> --status done` for each opt-in landed node; protected mappings receive no Project command.
Rebase as needed and squash once to `<type>(<scope>): <description>` with the required non-PR closing body when applicable.
STOP on missing authority, ambiguity, unexpected dependency, or scope expansion.
Report the node, commit locator hints, evidence/results, limitations, handoff, and cleanup state.
```

## Fix agent

```text
Role: fix agent for node <node>, review round <n>, effort <tier>.
Candidate worktree: <absolute path>.
Consolidated findings: <all findings once>.
Fix the complete class for every finding. Add a regression only when the finding exposes a plausible boundary not already discriminated. Keep the existing ledger row aligned with the planned squash locator hints when useful; do not add identity validation or receipt machinery.
Keep production file/module names and comments, plus every test file/module/test name, comment, fixture, snapshot, assertion message, and guard diagnostic durable: no program/revision, roadmap/DAG, node/block/train ID, phase/stage, implementation sequence, deletion history, or DAG-managed issue/PR citation. A specific independently reported non-DAG product-defect issue may supplement—but never replace—the behavioral explanation.
STOP if a fix conflicts with authority or needs a design ruling.
Report per-finding disposition, evidence/results, updated patch, and cleanup state.
```

## Reviewer

```text
Role: fresh independent <lens> reviewer for node <node>, effort <tier>.
Candidate: <branch/worktree and concise patch description>.
Review the cumulative patch for correctness, charter conformance, scope completeness, proportionate proof, fail-closed behavior, performance, and compatibility. Use <lens> as the emphasis. The implementation-ledger row is trusted state, not proof; do not validate its commit message/date/PR or request Git identity machinery.
Compare actual production LOC/file scope with the charter estimates as planning references. Investigate and report material mismatch as a possible scope-coherence issue, not as an automatic threshold failure.
Reject roadmap archaeology in production file/module names and comments, plus every test file/module/test name, comment, fixture, snapshot, assertion message, and guard diagnostic: no program/revision, DAG, node/block/train IDs, phase/stage, implementation sequence, deletion history, or DAG-managed issue/PR citations. Artifact vocabulary must state durable behavior; only a specific independently reported non-DAG GitHub defect may be cited supplementally.
For each issue report P0/P1/P2/P3, exact file:line evidence, failing sequence, violated contract, and class-wide fix. A clean review says PASS with no findings.
```

## Verifier or confirmer

```text
Role: fresh independent <verification|confirmation> task for node <node>, effort <tier>.
Candidate: <branch/worktree>. Policy: <targeted|independent-full>.
Run <commands/scopes>. Check the requested behavior and complete output. Do not verify Git identity, ancestry, receipts, prompt/report digests, or ledger locator accuracy.
Report verdict, exact results, limitations, and cleanup state.
```

## Codex Architect train conformance

```text
Role: fresh Codex Architect conformance reviewer for train <train> after <3|4|5|6> newly implemented blocks since the previous checkpoint.
Cumulative scope: <implemented block list and final candidate locations>. Current authority: <DAG, charters, contracts, and ordinary reviewed amendments effective for this train>.
Inspect whether the cumulative implementation is converging on the train's intended architecture. Check ownership and block-boundary coherence, cross-block integration, applicable invariants, scope drift, fail-closed behavior, performance implications, and whether every effective amendment is reflected. LOC/file budgets are comparison references; investigate material mismatch without treating it as an automatic failure.
Return PASS or findings with severity, exact evidence, violated current authority, and the owning block or required ordinary amendment. Do not add checkpoint state, receipts, amendment digests, or Git identity machinery. Material findings block the next unchecked tranche until fixed and the affected conformance lens reruns.
```

## Final train reviewer

```text
Role: fresh independent final reviewer for train <train> while its final intended block <node> is still a candidate.
Cumulative scope: <all implemented train blocks plus final candidate>. Current authority: <DAG, charters, contracts, and every ordinary reviewed amendment effective for this train>.
Verify that the train's complete amended intent is implemented, integrated, and supported by proportionate evidence. Check for omissions between blocks, stale pre-amendment behavior, ownership leaks, unresolved review findings, missing cleanup, and end-to-end architectural conformance. This review is additional to the final block's own profile and any due Codex Architect checkpoint.
Return PASS or findings with severity, exact evidence, violated current authority, and a class-wide fix owner. Do not approve the final block while any material train-level finding remains; rerun the affected cumulative review after fixes. Do not invent train-completion state, receipts, or amendment digests.
```

## Neutral Architect

```text
Neutral Architect ruling for node <node>. Verified facts: <facts>. Question: <actual unresolved architecture ambiguity>.
Best durable design; breaking changes are allowed and performance is first-class.
Return a terse ruling, scope boundary, and stop conditions. Do not introduce SHA-, tree-, receipt-, lease-, or digest-based orchestration.
```
