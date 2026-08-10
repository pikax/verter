# Verter Revision 11 Worker Context Packet

**Packet digest:**  
**Created from program-state digest:**  
**Role:** Scoper | Implementor | Conformance reviewer | Architecture reviewer | Adversarial/performance reviewer | Investigator  
**Block / charter:**  
**Stack window / StackSnapshotId / layer_id / acceptance block:**  
**Writable worktree / branch:**  
**Maintainer:**  
**Orchestrator:**

# 1. Exact identities

- authority package digest:
- A6 Implementation Lock digest or `PRE-A6`:
- entry checkout SHA/tree:
- implementation baseline SHA/tree or `UNSET`:
- block base SHA/tree:
- current candidate SHA/tree or `UNSET`:
- charter digest:
- relevant predecessor accepted SHAs/trees/evidence digests:

# 2. Assigned objective

One paragraph stating exactly what must become true.

# 3. Current source facts

- current authorities/readers/writers:
- exact files/symbols/contracts already inspected:
- current behavior/capability status:
- known open PR/branch conflicts and disposition:

Do not substitute the implementor/orchestrator summary for direct source inspection when the role requires independent evidence.

# 4. Allowed write set

- files/modules/generated outputs allowed:
- dependency/lockfile/protocol changes allowed:
- branch/history operations allowed:

Everything else is read-only unless the orchestrator accepts a rescope.

# 5. Forbidden changes

- architecture/ADR/gate weakening:
- scope widening or unrelated cleanup:
- compatibility shim, shadow path, runtime switch, alternate authority:
- ambient I/O, secret/permission changes, or unowned worktree mutation:
- self-approval or review-result fabrication:

# 6. Required end state and deletions

- surviving owner/path/API:
- old declarations/implementations/caches/tasks/metrics/flags/docs to delete:
- public/protocol/compatibility consequences:
- exact one-path/atomicity invariant:

# 7. Required commands and proof

| Command/evidence | Expected non-vacuous work | Required result | Raw output path |
|---|---:|---|---|

Include correctness, determinism, work/copy/allocation, performance, memory, platform, failure, dependency, generated-file, and clean-tree proof applicable to the charter.

# 8. Review scope and output

- mandatory changed surface:
- required dependency/owner closure:
- causal blocker rule:
- output format:

# 9. Stop/rescope conditions

List exact facts that require stopping rather than improvising.

# 10. Handoff result

Return the block record required by `contracts/agent-orchestration.md`, with raw evidence paths/digests and no unsupported success claim.
