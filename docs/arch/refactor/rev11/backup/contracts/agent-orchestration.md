# Agent Orchestration and Handoff Contract

**Status:** Normative implementation-execution contract.  
**Entry document:** `ORCHESTRATOR.md`.

# 1. Purpose

This contract makes Revision 11 executable by an orchestrator agent without turning the agent into architecture, repository, or review authority.

The authority package states what must become true. The repository and accepted evidence state what is currently true. The orchestrator may never fill a gap between them by assumption.

# 2. Required principals

Every run names:

- **Maintainer:** human or explicitly designated repository authority that can accept architecture, gates, blocks, and merges.
- **Orchestrator:** one coordinating agent/context that owns sequencing, program-state mutation, worker assignment, evidence collection, and stop/rescope reporting.
- **Workers:** scoped implementor, scoper, or investigator contexts.
- **Review mandates:** conformance, architecture, and adversarial/performance contexts as required by `governance.md`.

The orchestrator may also implement a block when assigned, but it cannot provide that block's only substantive review, cannot self-accept any block or gate, and cannot accept its own work.

# 3. Model adapter identity

The durable contract is model-independent. The supplied Opus adapter requests the fixed ID `claude-opus-5` and records:

- requested model ID;
- actual model ID;
- orchestrator runtime and exact runtime version;
- provider;
- fallback or substitution;
- any explicit maintainer exception.

A mismatch does not become invisible merely because work appears correct. Without an explicit maintainer exception, a substituted runtime may inspect and report but cannot represent an Opus-specific Foundational result as accepted. Multiple instances of one model are not automatically independent evidence.

# 4. Trust bootstrap

Before repository mutation, the orchestrator must:

1. read `README.md`, `ORCHESTRATOR.md`, `program-dag.toml`, `governance.md`, this contract, and the current block charter;
2. run `python3 tools/validate_package.py` against the extracted package;
3. compute and record the package manifest digest;
4. record the requested and actual model, orchestrator runtime/version, and provider identity, including any fallback or substitution;
5. inspect the actual local repository, remotes, branch, full SHA/tree, dirty/untracked/submodule state, worktrees, open local branches, and available GitHub/CI permissions;
6. initialize or validate `program-state.toml` with `tools/validate_program_state.py`;
7. confirm that only the next legal block is marked `READY`;
8. stop if package validation, repository identity, predecessor state, or authority ownership is ambiguous.

A web page, prompt summary, prior conversation, branch label, or short SHA is not authoritative repository state.

# 5. First-run rule

A fresh Revision 11 handoff authorizes **only `A0`**.

The orchestrator does not start `A1`, implementation tracks, or a broad repository rewrite in the same unreviewed act. It completes the `A0` evidence packet, updates program state, returns the result, and follows the maintainer's accepted sequencing process.

After `A0`, the orchestrator may continue through later blocks only when:

- every direct predecessor is accepted in program state, or the block is contingent `READY`/`IN_PROGRESS`/`REVIEW` work over an exact lower layer in the same validated stack snapshot;
- the block is explicitly `READY`;
- the exact charter and context packet are accepted;
- required permissions, branch/worktree, tests, and review mandates exist;
- no unresolved architecture deviation or competing owner change blocks it.

# 6. Context packet

Every non-trivial worker receives an immutable packet based on `templates/context-packet.md` containing:

- authority package and A6 lock digests;
- exact baseline/base/candidate identities;
- block, predecessors, charter digest, and stack position;
- current authority/readers/writers and source references;
- allowed write set and forbidden ownership changes;
- required commands, evidence, deletion set, output format, and stop conditions;
- one assigned role and review-independence constraints.

The packet is addressed by digest and stored with evidence. A worker does not receive a vague instruction to “follow the whole plan.”

# 7. Delegation policy

Delegation is used for substantial independent work, not ceremony.

Default limits until `A6` records another bounded policy:

- at most three active worker/subagent contexts under one orchestrator;
- one implementor owns one writable block branch at a time;
- one writable worktree per worker/branch;
- no subagent spawned merely to restate, summarize, or generically double-check the orchestrator's work;
- role-mandated independent reviews use clean contexts and direct evidence, not the implementor's conclusion.

The orchestrator may lower concurrency whenever shared owners, files, generated artifacts, tool resources, or benchmark machines would make parallel work unsafe or noisy.

# 8. Repository mutation and permissions

The orchestrator:

- never commits directly to a protected trunk unless repository policy explicitly makes that the accepted landing mechanism;
- never force-pushes an accepted, queued, or shared branch without invalidating affected state and reviews;
- never changes branch protection, required checks, secrets, credentials, release policy, or external service permissions as an implementation convenience;
- never exposes secrets in prompts, logs, evidence, commits, or review reports;
- treats generated files, lockfiles, submodules, vendored code, and benchmark artifacts as explicit write surfaces;
- uses one branch/worktree owner and records every mutation in the program ledger;
- leaves no untracked evidence or hidden local patch as part of an accepted claim.

# 9. Orchestrator output contract

For each block, the orchestrator emits a bounded record:

```text
BLOCK: <id>
STATE: ACCEPTANCE_RECOMMENDED | BLOCKED | RESCOPE_REQUIRED | FAILED
BASE: <full sha/tree>
CANDIDATE: <reviewed full sha/tree or none>
ACCEPTED_TARGET: <actual landed full sha/tree or none>
LANDING_EQUIVALENCE: <digest or none>
CHARTER_DIGEST: <digest>
CONTEXT_PACKET_DIGEST: <digest>
STACK: <id/layer or none>
CHANGES: <concise summary>
DELETIONS: <completed/missing>
EVIDENCE: <paths + digests + non-vacuous counts>
REVIEWS: <mandate/status/exact candidate>
DISCOVERIES: <disposition>
NEXT_LEGAL_BLOCKS: <derived from validated state>
MAINTAINER_DECISION_REQUIRED: <yes/no + exact decision>
```

The orchestrator never reports `PASS` or `LAND_READY` merely because it believes the design is good. It reports the actual validated state and missing proof.

# 10. Mandatory stop conditions

Stop and return `BLOCKED` or `RESCOPE_REQUIRED` when any of these occurs:

- package, manifest, DAG, program-state, charter, or performance-gate validation fails;
- actual checkout differs from the locked source and affected evidence has not been refreshed;
- dirty/unowned work or a competing PR/branch touches the same authority;
- a required command executes zero intended work or cannot be proven non-vacuous;
- current source disproves an architecture assumption;
- the true consumer/cutover closure is wider than the charter;
- a public/protocol/compatibility obligation is newly discovered;
- a benchmark premise or oracle is invalid;
- a worker requires forbidden permissions, ambient I/O, secret access, or destructive history changes;
- an atomic block would leave two production paths or an independently mergeable private layer;
- required independent review cannot be obtained.

The remedy is a source-backed deviation/rescope record, not a shim, hidden flag, weakened assertion, or unilateral architecture edit.

# 11. Maintainer-only decisions

The orchestrator may recommend but cannot self-authorize:

- package adoption/supersession;
- accepted ADR or architecture amendment;
- `A6` acceptance;
- gate weakening or benchmark premise recalibration;
- formal block rescope or DAG amendment;
- irreversible public compatibility/protocol policy;
- merge/land/release when repository policy requires maintainer approval.

# 12. Evidence custody

Raw command output, profiles, benchmark samples, generated manifests, review reports, range-diffs, landing-equivalence proofs, and lock records are stored at stable paths and addressed by digest. Summaries point to raw evidence and do not replace it.

A result created from a different SHA, tree, toolchain, profile, corpus, or stack base is not silently relabeled as current evidence.
