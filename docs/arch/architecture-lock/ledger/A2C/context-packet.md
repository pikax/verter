# Verter Revision 11 Worker Context Packet

**Packet digest:** see `command-proofs/digests.sha256`  
**Created from program-state digest:** `6b9d3022de6451392ebe94f07a2a8923a44652ccceb300fe2425bf6061b9415c`  
**Role:** Implementor  
**Block / charter:** A2C — Abrupt-completion facts for G10 safety discrimination  
**Stack window / StackSnapshotId / layer_id / acceptance block:** none / PRE-A6 foundational block  
**Writable worktree / branch:** `<REPO>-wt-a2c` / `block/a2c-completion`  
**Maintainer:** external program-ledger authority  
**Orchestrator:** Revision 11 program orchestrator

# 1. Exact identities

- authority package digest: package README SHA-256 `4f958b8946acd109c8b67e6737c3c445c835a6ea05f2f4e019d3cd301d0a1461`
- A6 Implementation Lock digest or `PRE-A6`: PRE-A6
- entry checkout SHA/tree: `70ea4c01bea870e9684a66f229230808aeb64235` / tree retained by Git
- implementation baseline SHA/tree or `UNSET`: UNSET
- block base SHA/tree: `70ea4c01bea870e9684a66f229230808aeb64235` / tree retained by Git
- current candidate SHA/tree: `04048a9471f1c13e81cda075fa27a6c35b59a842` / `5ca44c8e58d79cf040bbd066a25c44323cf0e10c`
- charter digest: `50bbb992eadcb9080de2f48ff9d21ce667f409dabdb1bea117d6bbf3899be895`
- relevant predecessor accepted SHAs/trees/evidence digests: external program ledger

# 2. Assigned objective

Retain exact-candidate evidence for the content-free completion model, including the mandatory construction-latency comparison, without any tracked change after the committed candidate.

# 3. Current source facts

- current authorities/readers/writers: `verter_semantic::analysis::flow` owns skeleton construction and completion facts; A3 is the future consumer.
- exact files/symbols/contracts inspected: A2C charter and amendments, `completion.rs`, completion tests, skeleton tests, benchmark, context template, and orchestration contract §9.
- current behavior/capability status: semantic and non-interference suites are supplied as proven; the exact-candidate latency gate is failed.
- known open PR/branch conflicts and disposition: none observed; branch identity is exact and porcelain is empty.

# 4. Allowed write set

- files/modules/generated outputs allowed: external `<EVIDENCE>\A2C\` and scratch content only.
- dependency/lockfile/protocol changes allowed: none.
- branch operations allowed: none; no push, merge, or GitHub action.

# 5. Forbidden changes

- architecture/ADR/gate weakening: forbidden.
- scope widening or unrelated cleanup: forbidden.
- compatibility shim, shadow path, runtime switch, alternate authority: forbidden.
- ambient I/O, secret/permission changes, or unowned worktree mutation: forbidden.
- self-approval or review-result fabrication: forbidden; state remains BLOCKED.

# 6. Required end state and deletions

- surviving owner/path/API: committed candidate only; no further tracked change.
- old declarations/implementations/caches/tasks/metrics/flags/docs to delete: none authorized in evidence execution.
- public/protocol/compatibility consequences: none.
- exact one-path/atomicity invariant: every claim binds the unchanged candidate SHA/tree; evidence writes are external.

# 7. Required commands and proof

| Command/evidence | Expected non-vacuous work | Required result | Raw output path |
|---|---:|---|---|
| stable-control calibration | 40 measured samples after 5 warmups | freeze noise before shapes | `command-proofs/latency/00-control-calibration.txt` |
| exact-SHA archive builds | baseline and candidate | same harness, successful optimized builds | `command-proofs/latency/10-equivalent-harness-install-final.txt`, `11-equivalent-baseline-build.txt`, `12-equivalent-candidate-build.txt` |
| interleaved construction benchmark | 40 complete pairs × 5 shapes | every shape within frozen gate | `command-proofs/latency/13-valid-interleaved-40-pairs.txt` |
| bootstrap analysis | 100,000 resamples per cell | upper CI ≤ 3% | `command-proofs/latency/14-valid-bootstrap-analysis.txt` |
| clean-tree proof | exact SHA/tree/parent/branch and porcelain | empty porcelain | `command-proofs/latency/15-final-candidate-identity-clean.txt` |

# 8. Review scope and output

- mandatory changed surface: candidate commit `04048a9471…` over base `70ea4c01bea…`.
- required dependency/owner closure: completion-fact construction boundary and public non-interference surface.
- causal blocker rule: any candidate cell exceeding the frozen latency gate blocks.
- output format: `A2C-exact-candidate-record.md`, contract §9, STATE BLOCKED.

# 9. Stop/rescope conditions

Any shape above the frozen latency gate stops evidence execution. This condition is satisfied.

# 10. Handoff result

Return the BLOCKED exact-candidate record with the retained latency finding and raw command digests. Acceptance requires maintainer action and is not recommended by this worker.
