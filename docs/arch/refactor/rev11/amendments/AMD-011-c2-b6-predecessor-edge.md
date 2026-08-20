# AMD-011 — C2 gains B6 as a direct predecessor

**Status:** NOT RATIFIED — awaiting the designated maintainer's decision. The
preparer did not and cannot ratify, review, or satisfy any independent mandate.
Nothing in this document changes the DAG until it is ratified; the edit in §4 is
proposed text, not applied text.

**Prepared against:** local `program/architecture-lock` commit
`07b1e4358c1690085ee26f212b85c212ab79104e`, tree
`8a34233c99fe6368b27803376103e49b29da2344`, working tree with 0 uncommitted change(s) at preparation time.
Every `file:line` citation below was read directly on that tree.

**Amends on ratification:** [`../program-dag.toml`](../program-dag.toml) — the
single `predecessors` line of the `C2` block row (line 181). **It changes one
DAG edge set, adds no block, retires no block, and moves no acceptance owner off
any capability-matrix cell.**

## 1. Why this exists

[`ARCH-RULING-C2-FIVE-FORKS`](../rulings/ARCH-RULING-C2-FIVE-FORKS.md) ruled on
five open forks in the C2 charter draft. Four were tagged "DAG unchanged". Fork B
was tagged, on its own face, **"DAG edge changed — formal amendment required"**,
and the ruling states explicitly that "the required amendment (B6->C2) is not
itself ratified by this document."

This is that amendment. It exists only to carry Fork B's verdict into the DAG
under the governance the ruling itself demanded. It re-decides nothing.

## 2. The defect, as the ruling found it

C2's first normative stage returns a `PreparedCarrier`
([`../contracts/compile-transaction.md`](../contracts/compile-transaction.md), lines 8–15).
B6 expressly owns the preparation/reuse lifecycle and makes the direct core ready
for semantic projection ([`../charters/B6.md`](../charters/B6.md), lines 5–12).

C2 therefore consumes a type B6 owns, while the DAG does not record B6 as a C2
predecessor. The ruling's words: *"A separate C2-local preparation type would
violate the one-path cutover … The current DAG is therefore wrong at
`program-dag.toml:165-169`."*

## 3. Independent detection

This contradiction is not asserted on the strength of a human cross-read. It is
reported mechanically by `scripts/effective-state.mjs`, which derives the
effective program view from the DAG, the ledger, and the ruling corpus:

```
[ERROR] MISSING_DAG_EDGE_IMPLIED_BY_RULING: ruling C2-FIVE-FORKS
(ARCH-RULING-C2-FIVE-FORKS.md) says to add edge B6 -> C2, but program-dag.toml
block C2 does not list B6 as a predecessor
```

It is the only finding that generator reports against the live ledger. Ratifying
this amendment and applying §4 clears it to zero.

## 4. The proposed change — exactly one line

`docs/arch/refactor/rev11/program-dag.toml`, the `C2` block row at line 181:

```diff
 [[block]]
 id = "C2"
 name = "Staged compile transaction and sealed facade"
 class = "foundational"
-predecessors = ["B3", "B5", "C1"]
+predecessors = ["B3", "B6", "C1"]
```

## 5. Why B5 leaves the direct set rather than joining B6 in it

The ruling normalizes the set to `["B3", "B6", "C1"]` rather than
`["B3", "B5", "B6", "C1"]`. That is a deliberate normalization, not an omission:
`B6.predecessors = ["B5"]` (`program-dag.toml`, line 169), so B5 is already
transitively required through B6. Listing it directly as well would record the
same constraint twice and invite the two copies to drift.

**This is the one substantive consequence a reader should check.** The edit is
not purely additive: C2 stops naming B5 directly. Any tooling that reads direct
predecessor sets without computing transitive closure will see B5 disappear from
C2's list. The ordering constraint is unchanged — B5 still precedes C2 — but it
is now expressed once, through B6.

## 6. What this does NOT do

- It does not accept, unlock, or dispatch B6, C2, or any other block.
- It does not change any charter, contract, ADR, or capability-matrix cell.
- It does not change the program outcome. The ruling's own governance line for
  Fork B reads: *"accepted ADR unchanged; DAG edge changed—formal amendment
  required; program outcome unchanged."*
- It does not re-open the other four forks, all of which the ruling settled with
  "DAG unchanged".

## 7. Verification on ratification

1. Apply the §4 diff.
2. `node scripts/validate-program-state.mjs` — passes; the DAG stays acyclic and
   every predecessor id remains a known block.
3. `node scripts/effective-state.mjs` — reports zero findings and exits 0
   (it exits 1 today on the finding quoted in §3).
4. The `program_dag_digest` recorded in
   `docs/arch/architecture-lock/ledger/program-state.toml` is rebound to the
   edited DAG in the same change, or validation fails closed on the stale digest.

## 8. Maintainer decision

_Unrecorded._ This section is completed by the designated maintainer, not by the
preparer. Until it carries a decision, the DAG is unchanged and C2's predecessor
set remains `["B3", "B5", "C1"]`.
