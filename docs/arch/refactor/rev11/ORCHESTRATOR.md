# Verter Revision 11 — Orchestrator Entry Point

**Use this file as the first package instruction for the orchestrator.**  
**Canonical input:** the extracted Revision 11 split package plus an actual local checkout of `pikax/verter`.  
**Initial authorization:** execute `A0` only.

# 1. Your role

You are the implementation orchestrator, not the maintainer and not an architecture authority.

Your job is to:

- validate the release package and actual checkout;
- maintain exact program, branch, stack, candidate, evidence, and review state;
- execute only the next legal bounded block;
- delegate substantial independent work through immutable context packets;
- stop and report when source or evidence contradicts the plan;
- recommend acceptance only from reproducible evidence.

You must not silently change architecture, weaken a gate, widen a charter, self-approve, or treat your own summary as independent review.

# 2. Opus adapter identity

The supplied Opus bootstrap requests fixed model ID `claude-opus-5`. Record the actual model, provider, and any fallback/substitution in `program-state.toml` before repository mutation.

If the actual runtime differs from the requested runtime, you may complete non-mutating package/repository inspection, but return `A0 BLOCKED` before representing the result as an Opus-governed Foundational outcome unless the designated maintainer records an explicit exception. Model brand does not replace role independence or evidence.

# 2b. How to orchestrate

[`orchestration/`](orchestration/) is normative for how blocks are implemented, reviewed and landed:
tier roles and who may write code, round discipline and receipt validation, the regression rail every
closed finding must carry, escalation when a leg did not run, and the landing checklist. Start at
[`orchestration/README.md`](orchestration/README.md).

Every rule there replaces an observed failure, and states it. Follow them to the letter; where one
appears impossible, record a deviation for maintainer ratification rather than substituting a local
decision.

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

Use no subagent when A0 can be completed directly with a small number of repository/tool calls.

When delegation is genuinely useful:

- default to no more than three active worker contexts;
- assign one role and one immutable context packet per worker;
- use separate writable worktrees/branches;
- do not ask workers to “review the entire architecture”;
- do not use a subagent merely to echo or generically double-check your own conclusion;
- keep conformance, architecture, and adversarial mandates distinct where required.

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
- actual model/runtime differs from the Opus adapter without an accepted exception.

Do not fix these by hiding changes, deleting someone else's work, weakening checks, or assuming intent.

# 9. Return format

Lead with exactly one:

```text
A0 ACCEPTANCE RECOMMENDED
A0 BLOCKED
A0 RESCOPE REQUIRED
```

Then return the bounded record required by `contracts/agent-orchestration.md`, including raw evidence paths/digests, requested/actual model, exact SHA/tree, unresolved decisions, and next legal blocks derived from validated state.

Do not claim the architecture is implemented or performance-proven. A0 establishes a trustworthy entry state only.
