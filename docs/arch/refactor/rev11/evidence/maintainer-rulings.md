# Maintainer Rulings — Verter Revision 11, A0

This file records the maintainer decisions that shape the tree under
`docs/arch/refactor/rev11/`. It is the authoritative explanation of why the tree looks
the way it does. A future agent continuing the program reads this file together with
`README.md`, `evidence/A0-preflight-blocked.md` (historical entry inspection), and
`evidence/A0-summary.md` (the committed, identity-free description of the A0
landing; live A0 state lives in the external ledger).

**Maintainer:** Carlos Rodrigues (GitHub `pikax`). Rulings R-1 through R-8 were made
and recorded on **2026-08-09**; R-9 was made and recorded on **2026-08-10**; R-10
was made and recorded on **2026-08-11**.

## R-1 — Maintainer designation

Carlos Rodrigues (`pikax`) is the designated maintainer and repository authority for the
Revision 11 program: package adoption/supersession, A6 acceptance, ADR amendments,
formal rescopes, gate recalibrations, irreversible compatibility/protocol decisions,
and accept/land/merge where repository policy requires maintainer authority
(`governance.md` §1.1), plus final block acceptance (`governance.md` §9 "maintainer
acceptance/land" and §15 "Final maintainer decision" — block acceptance is not part of
the §1.1 grant itself). Resolves preflight blocker B2 ("no maintainer designated").

## R-2 — Package-validation waiver; the tree stays as-is

Package validation is **waived**. The release ZIP `verter-architecture-v11.zip` and its
`.sha256` were never available on the machine, and the `tools/*.py` validators could not
run. The 67-file reconstructed tree in this directory **stays as-is**; it is **not**
ratified as a renamed derivative of the canonical 85-file package. `origin/main` is
frozen until this landing completes. Resolves preflight blocker B1.

## R-3 — The Revision 11 plan supersedes `CLAUDE.md` and existing repo rules

Where the Revision 11 plan and existing repository rules conflict, the plan governs.
Two concrete conflicts this ruling resolves:

1. **Python validators vs the no-Python dependency policy.**
   - The plan (`contracts/agent-orchestration.md` §4): "run
     `python3 tools/validate_package.py` against the extracted package" — and the
     mandated first commands further name `tools/selftest_orchestration.py`,
     `tools/validate_program_state.py`, `tools/validate_stack_window.py`, and
     `tools/validate_landing_equivalence.py`.
   - `CLAUDE.md` (Dependencies Policy): "Repo-owned toolchain is Rust + JS/Node only —
     no committed Python. Repo-owned gate, build, CI, test, code-generation, packaging,
     and release tooling is implemented as Rust bins or JS/Node scripts; Python is not a
     committed implementation language for those paths."
   - Resolution: the plan supersedes. See also R-4 — the validators are to be
     reimplemented in Node, so no Python is committed.

2. **`architecture.md` §8.2 vs `CLAUDE.md`'s Typed-IR-Only rule.**
   - The plan (`architecture.md` §8.2): "The final architecture contains no general
     recursive owned `TypeExpr` or `PortableTypeExpr` as a generic semantic transit IR,
     final cache value, compile projection contract, or public result."
   - `CLAUDE.md` (Typed-IR-Only Resolver Rule (CRITICAL)): "The native component-meta /
     typeinfo type resolver — analyzer → projector → registry → policy → materialiser —
     drives semantic decisions exclusively from the typed IR
     (`verter_semantic::analysis::type_expr::TypeExpr` on Rust, `TypeDescriptor` from
     `@verter/type-ir` on TS)."
   - Resolution: the plan supersedes; §8.2's ordered cutover away from a general owned
     `TypeExpr` governs the end state.

## R-4 — Python-to-JS validator reimplementation

The six validators named in `_EXTRACTION_INDEX.md` (`validate_package.py`,
`selftest_orchestration.py`, `validate_program_state.py`,
`validate_performance_gates.py`, `validate_stack_window.py`,
`validate_landing_equivalence.py`) are to be **reimplemented in Node, not Python**.
Status: `validate_program_state.py` is reimplemented as
`scripts/validate-program-state.mjs` (with its `node --test` suite) and lands with
this commit as a new ratified implementation — the Python original was never
available, so no behavior was ported; every check is derived from the Revision 11
tree's own text. The other five remain future work.

## R-5 — PR #98 disposition: abandon

PR #98 (`main <- agent/rsvelte-runtime-engine`, DRAFT, "feat(svelte): delegate runtime
compilation to rsvelte") is dispositioned as **abandon** — the nearest value in
`contracts/baseline-lock.md` §3's closed set (include before freeze / exclude and
rebase-reconcile later / abandon / coordinate as predecessor-dependent block). This
records the **program's relationship** to the PR only. **No GitHub action was taken or
is to be taken**: the PR is an external contributor's draft and is left untouched on
GitHub.

## R-6 — The program ledger stays external

The live program ledger (`program-state.toml`) **stays external** to the repository, in
an operator-local evidence directory (referred to in the evidence records as
`<EXTERNAL_EVIDENCE_ROOT>`); it is not committed. Consequence: this directory alone is
not sufficient to resume the live program — the external ledger is also required (see
`README.md`).

## R-7 — `update-docs` workflow left as-is; one narrow CI-wiring edit authorized

The repository's existing `update-docs` GitHub workflow is left alone. **Amendment
(A0 fix round):** the maintainer-directed review mandates required the program-state
validator test suite to be wired to a real gate, and editing `.github/workflows/` was
explicitly authorized **for that one purpose only**. The sole `.github/` edit of this
landing is therefore adding `scripts/validate-program-state.mjs` and
`scripts/validate-program-state.test.mjs` to the `js` change-detection path filter in
`.github/workflows/ci.yml` (the convention already used there for
`scripts/sccache-env.test.mjs` and its siblings), so a validator-only change triggers
the `js-build-test` job, whose `pnpm run test:scripts` step is the authoritative
runner of the suite (`test:scripts` in `package.json` runs
`node --test scripts/validate-program-state.test.mjs`). No other workflow content is
touched.

## R-8 — Nothing is ever pushed to `origin`

All Revision 11 program work stays local: work happens on a local worktree branch,
and landing is a local fast-forward of `main` — nothing is pushed to `origin`, and
`origin/main` is frozen until this work lands. No GitHub action of any kind is taken
as part of this program work.

## R-9 — Ratify A2C completion-model predecessor for A3

The maintainer ratifies **AMD-002 — A2C completion-model predecessor for A3**. `A2C`
is inserted directly after `A2` and directly before `A3`; `A3` now depends on `A2C`,
and no other predecessor list changes. `A2C` owns only content-free,
exact-or-typed-unknown completion facts and does not change public semantic results.
`A3` must consume those accepted facts as its sole G10 discriminator, while `D6` /
`U6.LOOP_CLOSURE` must consume the same completion algebra for final graph and flow
semantics rather than create a second classifier. The published consolidated master,
release artifacts, `_EXTRACTION_INDEX.md`, and historical readiness reviews remain
immutable historical originals; for execution, AMD-002 and the amended live split
files supersede their `A0 → A1 → A2 → A3` lineage.

## R-10 — Ratify A2C completion-graph authority recalibration

The maintainer ratifies all four distinct items in **AMD-003 — A2C completion-graph
authority recalibration**:

1. Reopen AMD-002 and supersede its points 2 through 4. Retain the DAG lineage
   `A2 → A2C → A3`, while redefining A2C as an early structural delivery of D6's
   sole completion/flow-graph authority. AMD-002 point 1 and point 5 onward remain
   in force.
2. Move structural G10 discrimination from an independent A2C skeleton-fact owner
   to D6's completion graph. A3 owns only retraction and non-admission in response
   to typed `FlowGap::AbruptCompletion`.
3. Recalibrate the performance cells: the five per-shape skeleton-relative cells
   remain diagnostics only; successor acceptance uses aggregate universal-index,
   public cold-request, linear-work, 64/65-target, retained-topology, and
   zero-completion-allocation cells. All numeric absolute limits must be frozen in
   plan text before successor implementation. The recalibration states no numeric
   absolute cold-request SLO or numeric work/byte bounds, so the architecture
   authority must set them as open items before that work begins.
4. Invalidate candidate `04048a…` while preserving it and its digest-verified bundle
   as failed historical evidence. Implementation restarts from `70ea4c…`; no
   approval, mutation result, or latency result transfers.

## R-11 — Ratify completion rescope and reduced retraction exit

Maintainer decision (verbatim): **ACCEPTED IN FULL**.

The maintainer ratifies **AMD-004 — Defer structural completion to D6 and reduce A3**.
A2C is terminally superseded as an executable predecessor while its reachable DAG and
ledger row remain. A3 depends directly on A2 and retracts only non-G10 wrong-complete
results. Exact structural completion and G10 discrimination remain open debt `FR-D8`,
owned by D6 / `U6.LOOP_CLOSURE`, with the sole demanded `FunctionFlowGraph` as completion
authority and no syntax-only fallback or second classifier.

## Registered amendments

Amendments normally record deltas without editing the verbatim-reconstructed authority
files. AMD-002, AMD-003, and AMD-004 are the maintainer-ratified exceptions described by
R-9, R-10, and R-11; the historical fidelity boundary is stated in `PROVENANCE.md`.
Registry:

- **AMD-001** — [`../amendments/AMD-001-stack-window-validator-prerequisite.md`](../amendments/AMD-001-stack-window-validator-prerequisite.md):
  the program-state validator's fail-closed rejection of begun successors of a
  `PRIVATE_CHECKPOINT` predecessor collides with the plan's canonical
  `D1 PRIVATE_CHECKPOINT -> D2` atomic path; before any post-A6 stacked delivery,
  `A6` must deliver the Node stack-window validator, composite program-state
  cross-validation, CI wiring, and a discriminating D1/D2 transition test; the
  composite validator accepts `D2` only when `D1` is the declared private checkpoint
  in the same validated `ATOMIC_REVIEW` snapshot with `D2` as its acceptance block;
  the refusal is superseded by that delivery, never simply deleted.
- **AMD-002 — A2C completion-model predecessor for A3** —
  [`../amendments/AMD-002-a2c-completion-predecessor.md`](../amendments/AMD-002-a2c-completion-predecessor.md):
  inserts `A2C` between `A2` and `A3`; its points 2 through 4 are superseded by
  AMD-003, while point 1 and point 5 onward remain in force.
- **AMD-003 — A2C completion-graph authority recalibration** —
  [`../amendments/AMD-003-a2c-completion-graph-authority.md`](../amendments/AMD-003-a2c-completion-graph-authority.md):
  supersedes AMD-002 points 2 through 4, retains `A2 → A2C → A3`, makes A2C an
  early structural delivery of D6's sole completion graph, limits A3 to typed-gap
  retraction/non-admission, and replaces the acceptance instrument while leaving
  the rejected candidate's evidence intact.
- **AMD-004 — Defer structural completion to D6 and reduce A3** —
  [`../amendments/AMD-004-defer-completion-to-d6.md`](../amendments/AMD-004-defer-completion-to-d6.md):
  supersedes the A2C predecessor and reduces A3 to non-G10 wrong-complete retractions
  while leaving exact structural completion and G10 discrimination as debt `FR-D8`,
  owned by D6 / `U6.LOOP_CLOSURE`.
