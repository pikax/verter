# A6 context packet — verbatim dispatch prompt

The prompt below is the artifact of record, reproduced verbatim rather than reconstructed. Where it
carries maintainer decisions ("MAINTAINER-RATIFIED DECISIONS", "MAINTAINER STANDING GUIDANCE"), those
are the ratifications the lock record incorporates; they are quoted here in their original form so a
reviewer can check the lock against what was actually authorised rather than against a paraphrase.

**One byte-level normalisation, disclosed rather than silent.** The dispatch prompt named the sibling
worktree by its absolute machine path once. That exact string is a tracked-path violation the
`tracked_paths_no_machine_roots` guard rejects, so it is replaced here with `<MACHINE_ROOT>`,
following the same normalisation the ledger transport copy uses. Nothing else is altered. The two
preceding blocks' context packets carry the unnormalised string and **currently fail that guard** —
recorded as a discovery in [`command-proofs.md`](command-proofs.md), because repairing them changes
digests the ledger already records and is therefore not this block's edit to make.

## 1. Implementer dispatch prompt

```
You are a bounded BLOCK IMPLEMENTER for the Verter Revision 11 architecture program, block A6.

Working directory: this repo (dedicated git worktree, branch work/a6-implementation-lock, based on
program/architecture-lock at 6af543c8a, which includes A5's landed inventories/decision records). Do not
touch <MACHINE_ROOT>/verter or any other worktree.

AUTHORITY (read before acting, in this worktree):
- docs/arch/refactor/rev11/charters/A6.md (ratified charter — binding)
- docs/arch/refactor/rev11/program.md section "A6 — Accept the Implementation Lock Record"
- docs/arch/refactor/rev11/governance.md (Foundational review class + stack-window requirements)
- docs/arch/refactor/rev11/decisions/ADR-016-implementation-lock-and-performance-gates.md
- docs/arch/refactor/rev11/templates/implementation-lock-record.md (the record's required shape)
- docs/arch/refactor/rev11/templates/performance-gates.template.toml (the gate file's required shape)
- docs/arch/refactor/rev11/templates/stack-window.template.toml (the stack-window file's required shape)
- docs/arch/refactor/rev11/evidence/A5-summary.md and docs/arch/refactor/rev11/evidence/A5/* (A5's
  inventories/decision records — the primary raw material for this lock)
- Predecessor: A5, accepted/landed (this worktree's base, tree already contains A5's evidence)

MAINTAINER-RATIFIED DECISIONS (already ratified, do not re-litigate; incorporate as-is into the lock record):
- A5-L1: `loop5_instrumentation` disposition = Converge then Delete. Counter migration owner = block G4.
  Watchdog migration owner = block K3. Backstop = block L4.
- A5-G1: `attribution`/`compile-fail` test arms become A6-locked per-block commands (i.e. THIS block names
  the exact command(s) each future block must run for these arms). CI job wiring is deferred post-program
  (do not add a CI job for these arms now).
- A5-DD1: `verter_semantic -> verter_workspace` is recorded as an equality-pinned exception (record the
  wasm32-cfg-gated nuance A5 already captured — verter_scheduler unconditional, verter_tsgo_api
  cfg(not(wasm32))-only). Removal gate = block C1.
- R-12: the 469-candidate unlanded local-branch population is abandoned as a class (lineage-bound
  justification: every candidate's merge-base is at or before 2de3b2d07). No branch is deleted, no GitHub
  action taken. `port/rust` keeps its individually-recorded disposition from A5 (net +370,822 driven by one
  generated artifact absent from main; excluding it, it is the population's largest net deletion; its
  merge-base already satisfies the lineage bound).
- S-1: stack policy = `max_open_stack_layers = 2`, `stack_mode_policy = "ATOMIC_REVIEW"`,
  `stack_tool = "LOCAL_BRANCH_CHAIN"`. THIS is the policy A6 itself locks and that every block after A6
  (including A6's own landing) must follow — apply it when you populate the stack-window file's fields, and
  state in the lock record that this is now the standing policy.

MAINTAINER STANDING GUIDANCE (apply throughout, this overrides any instinct to gold-plate):
"Follow the plan. If something will be fixed later we can defer it — we must move quickly and steady, the
plan should be followed." / "If before Track B we need something done, we should do it; otherwise we only
do it when we must need it done." Concretely: A6's job is to unlock B1 (and optionally J1) — do not
front-load exhaustive detail for blocks far down the DAG (C-track, D-track, etc.) that nothing before Track
B actually requires. Where the templates ask for tables/rows that only matter once a later block is
in flight, it is legal and CORRECT to record them as thin/deferred with an explicit "deferred, resolved
when block X starts" note rather than fabricating detail. The one place this does NOT apply: the charter's
named non-negotiables — the lock record itself, the exact baseline SHA/tree, the performance-gates.toml
with literally zero placeholder/REQUIRED_* values, and the concrete unlock of B1 (and optionally J1). Those
must be real and complete; everything else may be deferred with a named resolution point.

OBJECTIVE (from charter + program.md, binding):
Freeze one maintainer-accepted immutable Implementation Lock Record binding: exact entry checkout, exact
post-Gate-0 implementation baseline SHA/tree, Revision 11 manifest/DAG/program-state digests; non-vacuous
command/capability evidence; owner/consumer dispositions (the five ratified decisions above);
identity/profile/compatibility/protocol decisions; instrumentation/work baseline; a concrete
`performance-gates.toml` with NO placeholder/REQUIRED_* values (this file must actually validate — see
"tools/validate_performance_gates.py" referenced by the template; if that tool doesn't exist in this repo,
say so explicitly in the report and validate structurally against the template's required fields/types
instead, do not silently skip validation); resolved orchestration/permission/worktree/CI/merge/stack policy
(S-1 above); and unlock B1 (and optionally J1 if CSS work is in scope — check program.md/governance.md for
whether J1 is real and ready) as the first BLOCK_READY foundational charter(s).

Exit criterion (from program.md): program state becomes PROGRAM_LOCKED; foundational blocks may become
BLOCK_READY. Gate thresholds cannot be relaxed after candidate direction is observed.

IN SCOPE: only the deliverables named for A6 in program.md/charter (see charter "Required evidence"); the
Implementation Lock Record document; performance-gates.toml (locked, no placeholders, scoped to what B1 —
and J1 if in scope — actually needs measured, not a fabricated exhaustive suite for every future block);
updating docs/arch/architecture-lock/ledger/program-state.toml's status field to PROGRAM_LOCKED and
recording B1 (+J1 if applicable) as BLOCK_READY, IF that file's schema is something you should write to —
check whether the program orchestrator owns that file exclusively (prior blocks' state suggests the
program orchestrator writes it after accepting each block's transition, not the implementer) — if unsure,
DO NOT touch program-state.toml yourself; prepare the lock record's content and let the orchestrator apply
the ledger transition, same pattern as A4/A5.

OUT OF SCOPE: actually implementing B1 or any later block; running real hardware benchmarks you cannot
actually execute in this environment (if a performance-gates.toml cell requires a real measured baseline
number you cannot obtain here, that is a charter tension — STOP and report RESCOPE_REQUIRED naming exactly
what's missing rather than fabricating a number); adding a CI job for attribution/compile-fail (G1 defers
that); deleting loop5_instrumentation.rs now (L1 assigns that to G4/K3/L4, not A6); deleting or touching
any of the 469 unlanded branches (R-12 is a recorded disposition, not an action).

ABORT/RESCOPE: if the exact checkout, command target, product capability, current owner, compatibility
obligation, or proof boundary differs materially from charter assumptions — STOP, do not improvise a
substitute design, write status RESCOPE_REQUIRED with the exact contradiction to .agent-run/a6-report.yaml.
In particular: if performance-gates.toml genuinely cannot be populated with real (non-placeholder) numbers
in this environment (no benchmark corpus, no measurable baseline), STOP and report exactly what's missing —
do not invent numbers to satisfy the "no placeholders" requirement.

TDD / EVIDENCE DISCIPLINE (mandatory, no exceptions per CLAUDE.md):
- No stubs, no always-true assertions, no unconditional default/fabricated values presented as coverage.
- Every lock-record field must be evidence-backed (cite actual file/line/SHA from the current tree).
- performance-gates.toml REQUIRED_* placeholders are a Stub Prevention violation if left in a file this
  block claims is "locked" — either populate genuinely or report RESCOPE_REQUIRED.

WORK PROCESS:
1. Read the charter, program.md A6 section, ADR-016, and all three templates carefully.
2. Read A5's evidence in full (A5-summary.md + evidence/A5/*) — it is the primary raw material.
3. Determine the exact entry checkout SHA (A0), the Gate 0 commit lineage (A1-A5), and the exact
   implementation baseline SHA/tree this lock binds to (this worktree's base, 6af543c8a — confirm and cite).
4. Write the Implementation Lock Record (docs/arch/refactor/rev11/evidence/A6/implementation-lock-record.md,
   following the template's section structure) incorporating the five ratified decisions verbatim in
   sections 4-9 as appropriate, and the deferred-detail convention from the maintainer's standing guidance
   for anything not needed before B1/J1.
5. Write performance-gates.toml (repo root or docs/arch location — check where the template says A6 "copies
   this file to the implementation repository" and follow that) scoped to what B1 (and J1 if applicable)
   need measured, with zero placeholder values, status = "LOCKED".
6. Write the stack-window file (or the relevant section of the lock record) applying S-1's policy.
7. Determine B1's (and J1's, if in scope) exact predecessor-verified BLOCK_READY status per program.md's
   DAG (B1 predecessor = A6; confirm J1's predecessors and readiness from program.md if it exists).
8. Verify anything checkable against the real tree (SHAs, file:line citations, command existence) — do not
   assume.
9. You MAY make WIP commits freely (plain descriptive messages, no AI attribution) — do not squash, the
   orchestrator squashes at landing.
10. Write your final structured report to .agent-run/a6-report.yaml (fields: block, status
    DONE|BLOCKED|RESCOPE_REQUIRED, changed files, decisions/records produced, evidence paths, what was
    deferred and why plus its named resolution point, discoveries, open questions for the orchestrator).

Follow "no phase archaeology": do not reference "A6"/"phase"/"block"/"rev11" in source code
comments/identifiers under crates/packages/scripts — docs/evidence only. performance-gates.toml is a
program artifact, not source code, so referencing "A6" inside it (as its own header comment already does in
the template) is fine.

Follow CLAUDE.md hard rules (no stubs, no co-author lines, no git push).
```

## 2. How the dispatch's open questions were resolved

Recorded here because each was a decision the prompt left to the implementer rather than settled.

| the prompt's question | resolution | where |
|---|---|---|
| does `tools/validate_performance_gates.py` exist? | **No.** The plan's five remaining Python validators were never available and are to be reimplemented in Node. The gate file's "must pass a validator" condition was therefore met by writing one — not by skipping validation and not by asserting the file is clean. | `scripts/validate-performance-gates.mjs` |
| may the implementer write the ledger? | **No.** The orchestrator is its sole writer; an implementer that records its own row is self-accepting. The lock record prepares the transition content; the ledger is untouched. | lock record §9 |
| where does `performance-gates.toml` live? | **Repository root** — the template's literal instruction, and the only placement outside the authority-tree aggregate's input set, so the digest it records stays recomputable rather than self-referential. | lock record §1 |
| is a stack-window *file* required? | A **policy** file, not a snapshot instance. No window is open; depth-1 sequential operation needs no snapshot, and minting a one-layer one would record a stack that does not exist. | `stack-window-policy.toml` |
| is the CSS block in scope? | **No.** The template makes it conditional on CSS work being selected; it is not selected, no CSS cell is locked, and nothing unlocked consumes the CSS inventory. | lock record §10, item U-8 |
| can the gate file be populated with real numbers here? | **Yes** — the environment can build and run the baseline harness, and the baseline tree is source-identical to the tree the retained dataset was captured on. No `RESCOPE_REQUIRED` was needed, and no number is invented. | `baseline-measurement.md` |

## Addendum — AMD-001 traceability (added post-review, not part of the original dispatch)

**This section is not dispatch text.** It was appended after the three Foundational review mandates
returned, and it is separated from §1 precisely so that §1 stays what it claims to be: the verbatim
prompt of record. Nothing above this heading was rewritten to add the amendment retroactively.

**The gap.** The registered amendment
[`AMD-001 — Stack-Window Validator Is a Prerequisite for the D1/D2 Path`](../../amendments/AMD-001-stack-window-validator-prerequisite.md)
requires, in its §4, that the A6 context packet **and** the A6 implementation-lock evidence each name
it by identifier and bind its SHA-256; it states that a candidate whose packet or lock evidence omits
either "has not carried this prerequisite" and that the reviews must treat that as a missing required
input. The dispatch prompt reproduced in §1 above **does not reference `AMD-001` at all** — not in its
AUTHORITY list, not in its ratified-decisions block, not anywhere. That is an **orchestrator dispatch
gap**, discovered during Foundational review, not an implementer omission: the implementer cannot
carry a prerequisite the prompt never names, and the authority list it was given did not include the
amendments directory.

**The binding, made here retroactively.** Recorded so the prerequisite is mechanically traceable from
this block's own record rather than dependent on a reader re-finding the amendment:

| | |
|---|---|
| identifier | `AMD-001` |
| path at the base tree | `docs/arch/refactor/rev11/amendments/AMD-001-stack-window-validator-prerequisite.md` |
| base tree | `6af543c8a65b495aad2d6231e5e90878c3bf1769` |
| **SHA-256 (lowercase hex, over the raw bytes)** | `b70ed6e8e6f7b8dcc86ae684d0568ca8c77ed6a93ade144b55fd8488f2e06208` |
| recomputed by | `git show 6af543c8a…:docs/arch/refactor/rev11/amendments/AMD-001-stack-window-validator-prerequisite.md \| shasum -a 256` |

The amendment spells that command `sha256sum`, which is absent on the locked runner class;
`shasum -a 256` is the same algorithm over the same bytes. The digest is quoted here rather than
inlined into the amendment, per its own instruction — a self-digest is a fixpoint.

**What this addendum does and does not claim.** It discharges §4 (traceability) for the packet side;
the lock-record side is discharged in [`implementation-lock-record.md`](implementation-lock-record.md)
§9. It does **not** claim §1's four deliverables were delivered — they were not, §9 enumerates them,
and the deferral is written up as a `governance.md` §10 deviation memo at
[`AMD-001-deviation-memo.md`](AMD-001-deviation-memo.md) with status **PENDING MAINTAINER RULING**.
And it does not claim the original prompt carried the prerequisite. It did not; that is recorded above
as the defect it is.

**Process note for later dispatches.** The prompt's AUTHORITY list enumerated the charter,
`program.md`, `governance.md`, the ADR, three templates and the predecessor's evidence, but no
`amendments/` entry — while `README.md` places the amendments in the `ORCHESTRATOR.md` §3 read order.
A dispatch packet that enumerates authority file-by-file will keep missing amendments registered after
the enumeration was written. Later packets should name the amendments directory, or the rulings
register that indexes it, rather than a fixed file list.

---

# Addendum 2 — the amendment was rescoped, and this packet's binding moves with it

**Added after Addendum 1, for the same reason it was: so the prerequisite stays mechanically
traceable from this block's own record.** Addendum 1 above is left exactly as it was written. It
bound the amendment's PRE-amendment bytes at the base tree this block was originally dispatched
against, and that is what it should keep saying — it records what was actually carried at that point.

**What changed.** Before this candidate's acceptance the maintainer ruled **AMEND-AMD-001-TIMING**,
registered as [`../maintainer-rulings.md` R-12](../maintainer-rulings.md) and recorded inside the
amendment itself under "Amendment to §1's timing". `AMD-001` §1 is amended in place: its four
artifacts remain mandatory before the first post-lock stack window opens, and unconditionally before
`D1` enters `PRIVATE_CHECKPOINT`, but the delivery duty binds to whichever accepted candidate
immediately precedes that event **rather than to this block by name**. §§2-4 stand unchanged —
including §4, the traceability duty this addendum discharges.

Amending the amendment changes its SHA-256, so the §4 binding on the packet side is rebound here.
The integration lineage was also rewritten below this block (message-only; see the lock record §1),
so the base commit is named by its current SHA:

| | |
|---|---|
| identifier | `AMD-001`, as amended by R-12 |
| path at the base tree | `docs/arch/refactor/rev11/amendments/AMD-001-stack-window-validator-prerequisite.md` |
| base tree | `fb863297a04c7eb114d53ff65736c00240354504` |
| **SHA-256 (lowercase hex, raw bytes), POST-amendment** | `01661d01445e76f8861995061fd61511415550633a05b6ad351ec562b0ad5fd4` |
| recomputed by | `git show fb863297a…:docs/arch/refactor/rev11/amendments/AMD-001-stack-window-validator-prerequisite.md \| shasum -a 256` |
| superseded binding | `b70ed6e8e6f7b8dcc86ae684d0568ca8c77ed6a93ade144b55fd8488f2e06208` at `6af543c8a…` — Addendum 1, retained as historical |

As in Addendum 1: the amendment spells that command `sha256sum`, which is absent on the locked runner
class, and `shasum -a 256` is the same algorithm over the same bytes; the digest is quoted here
rather than inlined into the amendment, per its own instruction, because a self-digest is a fixpoint.

**What this addendum does and does not claim.** It discharges §4 for the packet side against the
POST-amendment text; the lock-record side is discharged in
[`implementation-lock-record.md`](implementation-lock-record.md) §9. It does **not** claim §1's four
deliverables were delivered — they were not, and under the amended timing that is now correct rather
than a gap, which is precisely what the rescope decided. Addendum 1 recorded that deferral as a
`governance.md` §10 deviation memo with status PENDING; that memo is now **RULED and superseded** —
the maintainer adopted the rescope rather than the memo's own `DEFER` recommendation. See
[`AMD-001-deviation-memo.md`](AMD-001-deviation-memo.md), retained as the historical record, and the
lock record's §11 row U-9, restated as informational.

**And it does not revise §1.** The verbatim dispatch prompt reproduced there still does not reference
`AMD-001`, and the orchestrator dispatch gap Addendum 1 records is still a real defect of that
dispatch. A rescope of the amendment does not retroactively put it in a prompt that never named it.
