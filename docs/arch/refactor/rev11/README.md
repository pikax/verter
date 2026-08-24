# Verter Revision 11 — Architecture-Lock Program

> **Naming note:** the authority package's own canonical `README.md` was landed as
> [`package-README.md`](package-README.md); this file is a repository-local index
> occupying the `README.md` path. `ORCHESTRATOR.md` §3 makes `README.md` normative
> read-order item 1 — wherever the plan refers to the package `README.md`, read
> `package-README.md`.

This directory contains the Revision 11 architecture-lock program: the split authority
package, the consolidated canonical master, the release artifacts, and the A0 evidence
records. The plan, contracts, charters, decisions, and evidence live here; the **live
program ledger (`program-state.toml`) is external** by maintainer decision
([`evidence/maintainer-rulings.md`](evidence/maintainer-rulings.md), R-6). Its location
is held by the maintainer and deliberately not recorded here — recording the real path
would embed a machine-specific root in the tracked tree. A future agent needs both
this directory and that external ledger to continue the program.

## Where to start

- **Normative entry point:** [`ORCHESTRATOR.md`](ORCHESTRATOR.md) — the orchestration
  contract for executing the program.
- [`governance.md`](governance.md) — review mandates, acceptance rules, maintainer decisions.
- [`program.md`](program.md) + [`program-dag.toml`](program-dag.toml) — the block program
  and its dependency DAG.
- [`architecture.md`](architecture.md) — the target architecture.
- [`verification.md`](verification.md) — verification requirements.
- [`charters/`](charters/) — per-block charters (A0–A6 plus templates).
- [`contracts/`](contracts/) — the normative contracts the blocks implement against.
- [`decisions/`](decisions/) — ADR-001 … ADR-020.
- [`templates/`](templates/) — record/report templates.
- [`baseline/`](baseline/) — the locked baseline record for entry SHA `9af553dd`.
- [`package-README.md`](package-README.md) — the authority package's own README
  (renamed here to avoid colliding with this index).
- [`consolidated/verter-architecture-lock-master-plan-v11.md`](consolidated/verter-architecture-lock-master-plan-v11.md)
  — the single-file canonical master the split files were reconstructed from.
- [`release/`](release/) — the published orchestrator prompt, start-here note, and
  validation report for Revision 11.
- [`evidence/A0-summary.md`](evidence/A0-summary.md) — a stable, identity-free
  description of the A0 landing and where its live state and exact-candidate
  evidence live (in the external ledger and evidence root, not in this tree).
- [`evidence/maintainer-rulings.md`](evidence/maintainer-rulings.md) — the maintainer
  rulings that shape this tree.
- [`evidence/A0-preflight-blocked.md`](evidence/A0-preflight-blocked.md) — the
  historical pre-candidate entry inspection (not the current A0 state).

## Program state

- The historical entry remains SHA `9af553dd262f82ac2f66e4ebf0a0faa70bc7aec0`
  (tree `3cf111cf5665586b7d8fdfd520f01cfee3bf8108`).
- The tracked live ledger records A0–B1 accepted at the integration tip from which
  AMD-005 was rebased. B1's accepted identity is
  `03b2fdbfc6d12452824768d9e389a5f6f3d680df`, tree
  `7f8230066735db17650b5d594a95d597540b3729`; its former worktree and branch have
  been removed.
- AMD-005's five new rows are locked. Its DAG/state edits are a validator-clean
  proposal only and do not expose BF1 before exact-tree maintainer ratification.

## Registered amendments

Amendments normally record deltas without editing the verbatim-reconstructed authority files. AMD-002, AMD-003, and AMD-004 are maintainer-ratified exceptions: predecessor authority is materialized in the machine-readable DAG and exact-state template, while the amended completion ownership, debt, and retraction exit are materialized in the live split files. The published consolidated and release artifacts remain immutable historical originals; for execution, AMD-004 and the amended live split files govern the A2-to-A3 lineage.

Amendments live in [`amendments/`](amendments/) and bind the program:

- [`amendments/AMD-001-stack-window-validator-prerequisite.md`](amendments/AMD-001-stack-window-validator-prerequisite.md)
  — the program-state validator fails closed on every begun successor of a
  `PRIVATE_CHECKPOINT` predecessor, yet `D1 PRIVATE_CHECKPOINT -> D2` is the plan's
  canonical atomic path; before any post-A6 stacked delivery, `A6` must deliver the
  Node stack-window validator, composite program-state cross-validation, CI wiring,
  and a discriminating D1/D2 transition test. The refusal is superseded by that
  delivery, never simply deleted.
- **AMD-002 — A2C completion-model predecessor for A3:**
  [`amendments/AMD-002-a2c-completion-predecessor.md`](amendments/AMD-002-a2c-completion-predecessor.md)
  inserted `A2C` between `A2` and `A3`; AMD-004 supersedes point 1 where it makes
  A2C the predecessor of A3, points 2 through 4 remain superseded by AMD-003, and
  point 5 remains in force.
- **AMD-003 — A2C completion-graph authority recalibration:**
  [`amendments/AMD-003-a2c-completion-graph-authority.md`](amendments/AMD-003-a2c-completion-graph-authority.md)
  supersedes AMD-002 points 2 through 4 while retaining `A2 → A2C → A3`, delivers
  D6's sole completion graph early through A2C, restricts A3 to typed-gap
  retraction/non-admission, and recalibrates the performance acceptance cells.
- **AMD-004 — Defer structural completion to D6 and reduce A3:**
  [`amendments/AMD-004-defer-completion-to-d6.md`](amendments/AMD-004-defer-completion-to-d6.md)
  supersedes the A2C predecessor and reduces A3 to non-G10 wrong-complete retractions,
  while leaving exact structural completion and G10 discrimination as debt `FR-D8`,
  owned by D6 / `U6.LOOP_CLOSURE`.
- **AMD-005 — Framework compiler conformance rescope (PROPOSED, NOT RATIFIED):**
  [`amendments/AMD-005-framework-compiler-conformance-rescope.md`](amendments/AMD-005-framework-compiler-conformance-rescope.md)
  prepares exact Vue RC.3 and Svelte 5.56.8 compatibility locks, five new
  framework/conformance blocks, amended B/C boundaries, and a validator-clean
  56-block DAG. It has no execution authority until three independent challenge
  reports and an exact-tree maintainer decision exist.
- **AMD-006 — Vue known-defect correction (PROPOSED, NOT RATIFIED):**
  [`amendments/AMD-006-vue-known-defect-correction.md`](amendments/AMD-006-vue-known-defect-correction.md)
  adds BV0 as an immediate, bounded Vue-defect-correction block between BF2 and
  {B2, B3}; narrows BF3 to Svelte/non-Vue-runtime scope; and requires BV1 to preserve
  BV0's corrections. It has no execution authority until three independent challenge
  reports and an exact-tree maintainer decision exist.

See [`PROVENANCE.md`](PROVENANCE.md) for exactly what is and is not attested about
these files.

## Orchestration

[`orchestration/`](orchestration/) — normative doctrine for implementing, reviewing and landing
program blocks: tier roles, round discipline, receipt validation, regression rails, escalation, and
the landing checklist. Entry point: [`orchestration/README.md`](orchestration/README.md).
