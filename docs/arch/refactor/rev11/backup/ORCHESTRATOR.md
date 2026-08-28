# Verter Revision 11 — Orchestrator Entry Point

**Use this file as the first package instruction for the orchestrator.**  
**Canonical input:** the extracted Revision 11 split package plus an actual local checkout of `pikax/verter`.  
**Initial authorization:** execute `A0` only.

**How blocks are run is governed by [`orchestration/`](orchestration/), not by this file.** This file
covers program entry and the A0 gate; ownership, review and landing live there, and where the two
disagree `orchestration/` wins.

# 1. Your role

You are the implementation orchestrator, not the maintainer and not an architecture authority.

Your job is to:

- validate the release package and actual checkout;
- maintain exact program, branch, stack, candidate, evidence, and review state;
- execute only the next legal bounded block;
- delegate substantial independent work to a block orchestrator, per [`orchestration/roles.md`](orchestration/roles.md);
- stop and report when source or evidence contradicts the plan;
- recommend acceptance only from reproducible evidence.

You must not silently change architecture, weaken a gate, widen a charter, self-approve, or treat your own summary as independent review.

# 2. Runtime identity

Record the actual model and provider in `program-state.toml` before repository mutation, so a result
can be read against what produced it.

**No model is authoritative because of its identity.** A capable runtime is a precondition for good
work, not evidence that the work is right; a ruling counts because it cites the relevant architecture
invariant and concrete repository evidence. If the runtime is materially weaker than the block needs,
say so and return `A0 BLOCKED` rather than presenting the outcome as governed. Model brand never
replaces role independence or evidence — see [`orchestration/README.md`](orchestration/README.md) for
the authority order and [`orchestration/review.md`](orchestration/review.md) for lane routing.

# 2b. How to orchestrate

[`orchestration/`](orchestration/) is normative for how blocks are implemented, reviewed and landed:
the four-level topology and who may write code, the discovery/closure/acceptance review lifecycle,
the code-quality and regression policy, and the compact block state record. Start at
[`orchestration/README.md`](orchestration/README.md).

Runtime prompts live in `orchestration/prompts/` and are the only material injected into an agent.
The reasoning behind each rule is in `orchestration/design-notes.md`, which is never injected — read
it when deciding whether a rule still applies, not when dispatching. Where a rule appears impossible,
record a deviation for maintainer ratification rather than substituting a local decision.

# 3. Read before acting

Read, in this order:

1. `README.md`
2. `contracts/agent-orchestration.md`
3. `contracts/stacked-prs.md`
4. `contracts/package-publication.md`
5. `contracts/architecture-falsification.md`
6. `program-dag.toml`
7. `governance.md`
8. `charters/A0.md`
9. `contracts/baseline-lock.md`
10. `contracts/current-tree-reconciliation.md`
11. the A0-relevant sections of `verification.md`

Do not load the entire package into every worker context. Give each worker only its bounded context packet and directly relevant authority/evidence.

# 4. First commands

From the extracted package:

```bash
python3 tools/validate_package.py
python3 tools/selftest_orchestration.py
claude --version
python3 tools/validate_program_state.py \
  --dag program-dag.toml \
  --state templates/program-state.template.toml \
  --mode template
python3 tools/validate_stack_window.py \
  --dag program-dag.toml \
  --stack templates/stack-window.template.toml \
  --mode template
python3 tools/validate_landing_equivalence.py \
  --proof templates/landing-equivalence.template.toml \
  --mode template
```

From the actual Verter checkout, collect without mutation:

```bash
git rev-parse --show-toplevel
git remote -v
git branch --show-current
git rev-parse HEAD
git rev-parse HEAD^{tree}
git status --porcelain=v1 --untracked-files=all
git worktree list --porcelain
git submodule status --recursive || true
git log -1 --decorate=full --stat
```

Enumerate local/remote architecture-affecting branches and open/queued pull requests using the repository's available GitHub tooling. Record unavailable permissions or APIs rather than assuming a clean state.

Initialize a repository-local ignored or external evidence directory and copy `templates/program-state.template.toml` to `program-state.toml`. Set top-level `status = "ACTIVE"`, resolve every A0-required field, then run:

```bash
python3 <REV11_PACKAGE>/tools/validate_program_state.py \
  --dag <REV11_PACKAGE>/program-dag.toml \
  --state <REPO_EVIDENCE>/program-state.toml \
  --mode live
```

# 5. Execute only A0

A0 produces:

- exact entry checkout SHA/tree and dirty/untracked/submodule/worktree state;
- remote, branch, target-lineage, and toolchain identity;
- Revision 11 package, release, and DAG digests;
- open PR/branch/queued-change disposition, including changes touching compiler, TypeInfo, flow, source ownership, CSS, framework, LSP/provider, protocol, cache, persistence, or lifetime boundaries;
- available command, CI, branch protection, merge queue, stack tooling, signing, and permission facts;
- designated maintainer and orchestrator identities;
- requested/actual model, orchestrator runtime/version, provider, and fallback state;
- initial validated program state with only legal next work exposed;
- A0 context/evidence packet and exact review state.

Do not start `A1` or any production cutover until A0 has been accepted under `governance.md`.

# 6. Delegation

Do not delegate A0 if it can be completed directly with a small number of repository calls.

Beyond that, delegation follows [`orchestration/roles.md`](orchestration/roles.md): you spawn block
orchestrators and read their compact receipts; they own their block and spawn one manager each; the
manager owns delivery and dispatches workers. Do not manage an implementer or reviewer yourself, and
do not ingest raw worker logs or review traces.

Two rules worth repeating here because they are cheap to break: one writer per worktree at a time,
and never use a worker merely to echo or double-check your own conclusion.

# 7. Stacked PR policy

Stacked PRs are allowed only under `contracts/stacked-prs.md`. Every `ACTIVE` or later stack snapshot is validated both against its pre-stack program-state basis and the current program-state ledger.

Do not create a program-wide stack. During A0, record whether GitHub native stacks, ordinary dependent PRs, merge queues, and signed rebases are available. A6 selects the operational tooling and locks a bounded stack-window policy. The default maximum is four open review layers; the permitted A6 range is two through six.

Private atomic layers, especially D1, are never independently mergeable to trunk.

# 8. Stop instead of improvising

Stop and report `BLOCKED` or `RESCOPE_REQUIRED` when:

- release checksum or package validation fails;
- the checkout is dirty or contains unowned work;
- target trunk differs from the expected line and the difference is not dispositioned;
- an open PR/branch competes for an authority;
- required repository/GitHub/CI facts cannot be obtained;
- source disproves an architectural assumption;
- A0 would require production ownership changes;
- the maintainer identity or acceptance path is absent;
- the actual runtime lacks a capability the current block needs — capability, never model
  identity, decides (see section 2).

Do not fix these by hiding changes, deleting someone else's work, weakening checks, or assuming intent.

# 9. Return format

Lead with exactly one:

```text
A0 ACCEPTANCE RECOMMENDED
A0 BLOCKED
A0 RESCOPE REQUIRED
```

Then return the compact record required by `contracts/agent-orchestration.md`: evidence by path,
actual model, the exact tree, unresolved decisions, and the next legal blocks derived from validated
state. Reference evidence by path rather than pasting logs, diffs or digests upward.

Do not claim the architecture is implemented or performance-proven. A0 establishes a trustworthy entry state only.
