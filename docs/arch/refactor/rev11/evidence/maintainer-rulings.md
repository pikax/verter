# Maintainer Rulings — Verter Revision 11, A0

This file records the maintainer decisions that shape the tree under
`docs/arch/refactor/rev11/`. It is the authoritative explanation of why the tree looks
the way it does. A future agent continuing the program reads this file together with
`README.md`, `evidence/A0-preflight-blocked.md` (historical entry inspection), and
`evidence/A0-summary.md` (the committed, identity-free description of the A0
landing; live A0 state lives in the external ledger).

**Maintainer:** Carlos Rodrigues (GitHub `pikax`). All rulings were made and recorded on
**2026-08-09**.

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

## Registered amendments

Amendments record deltas to the execution plan without editing the
verbatim-reconstructed authority files (which would void the fidelity attestation —
see `PROVENANCE.md`). Registry:

- **AMD-001** — [`../amendments/AMD-001-stack-window-validator-prerequisite.md`](../amendments/AMD-001-stack-window-validator-prerequisite.md):
  the program-state validator's fail-closed rejection of begun successors of a
  `PRIVATE_CHECKPOINT` predecessor collides with the plan's canonical
  `D1 PRIVATE_CHECKPOINT -> D2` atomic path; before any post-A6 stacked delivery,
  `A6` must deliver the Node stack-window validator, composite program-state
  cross-validation, CI wiring, and a discriminating D1/D2 transition test; the
  composite validator accepts `D2` only when `D1` is the declared private checkpoint
  in the same validated `ATOMIC_REVIEW` snapshot with `D2` as its acceptance block;
  the refusal is superseded by that delivery, never simply deleted.
