# AMD-011 — C2 gains B6 as a direct predecessor

**Status:** RATIFIED 2026-08-20 by the codex architect, to whom the maintainer
delegated amendment ratification. See §9.

**Prepared against:** local `program/architecture-lock` commit
`537cdfcd2a17f36f5bb13e03e2368896675441e8`, tree
`dee02c5b0f126aaec7be8656b48e169b40632435`.
Every `file:line` citation below was read directly on that tree.

**Amends on ratification:** three files —
[`../program-dag.toml`](../program-dag.toml) (the `predecessors` line of the
`C2` block row, line 181), [`../program.md`](../program.md) (C2's
**Predecessors** line, 201, which would otherwise go stale), and the
`program_dag_digest` field in
[`../../../architecture-lock/ledger/program-state.toml`](../../../architecture-lock/ledger/program-state.toml),
rebound to the edited DAG. **It changes one DAG edge set, adds no block, retires
no block, and moves no acceptance owner off any capability-matrix cell.**

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
for semantic projection ([`../charters/B6.md`](../charters/B6.md), lines 3–12).

C2 therefore consumes a type B6 owns, while the DAG does not record B6 as a C2
predecessor. The defective row is C2's, at `program-dag.toml` lines 177–181.
(The ruling's own prose cites lines 165–169, which is B6's row — a slip in the
ruling, not a second defect.)

The premise was verified independently of the ruling: the contract's stage 1 does
produce `PreparedCarrier`, B6's charter does own preparation and prepared-first
routes, and no C2 charter exists to contradict either.

## 3. Mechanical consistency check

`scripts/effective-state.mjs` reports this as its only finding against the live
ledger:

```
[ERROR] MISSING_DAG_EDGE_IMPLIED_BY_RULING: ruling C2-FIVE-FORKS
(ARCH-RULING-C2-FIVE-FORKS.md) says to add edge B6 -> C2, but program-dag.toml
block C2 does not list B6 as a predecessor
```

Read this for exactly what it is: a **ruling/DAG consistency check**. The
generator scans ruling text for an explicit "add edge X -> Y" statement and
compares it against the DAG's direct predecessor arrays
(`scripts/effective-state.mjs:497`). It does **not** re-derive the dependency
from the contract or the charters, and it is not independent evidence that the
edge is correct — §2 is. Its result is accurate: the current tree yields this one
finding, and the §4 row yields zero.

## 4. The proposed change

`docs/arch/refactor/rev11/program-dag.toml`, the `C2` block row at line 181:

```diff
 [[block]]
 id = "C2"
 name = "Staged compile transaction and sealed facade"
 class = "foundational"
-predecessors = ["B3", "B5", "C1"]
+predecessors = ["B3", "B6", "C1"]
```

`docs/arch/refactor/rev11/program.md`, line 201, which restates the same set and
would otherwise contradict the DAG it is subordinate to:

```diff
-**Predecessors:** `B3`, `B5`, `C1`.
+**Predecessors:** `B3`, `B6`, `C1`.
```

## 5. Why B5 leaves the direct set

The ruling normalizes to `["B3", "B6", "C1"]` rather than
`["B3", "B5", "B6", "C1"]`. Dropping the direct B5 edge loses no acceptance
constraint: `B6.predecessors = ["B5"]` (`program-dag.toml`, line 169), and the
validator checks the direct predecessors of **every** begun or accepted block
(`scripts/validate-program-state.mjs:695-755`), so an accepted B6 already entails an
accepted B5. The ordering constraint survives, expressed once.

This is **not** a general no-redundant-edge policy, and this amendment does not
introduce one. The DAG is not transitively reduced elsewhere and this amendment
does not make it so: C4 names both B6 and C3 directly (line 193), and F1 names
A6, B6, C4 and D2 (line 271). Those redundancies stay exactly as they are.

## 6. Disclosed consequences

- **C2 stops naming B5 directly.** Tooling that reads direct predecessor arrays
  without computing transitive closure will no longer see B5 in C2's list.
- **C2 and C3 gain B6 in their transitive closure.** C3's only predecessor is C2
  (line 187), so C3 inherits the new edge.
- **C4's direct B6 edge becomes redundant**, since C4 also depends on C3, which
  now reaches B6 transitively. It is left in place — see §5.
- Full traversal of the proposed graph finds a single root (`A0`), no cycle, and
  no unreachable block. C4's and F1's B6 edges become or remain redundant, never
  cyclic.

## 7. What this does NOT do

- It does not accept, unlock, or dispatch B6, C2, or any other block.
- It does not change any charter, contract, ADR, or capability-matrix cell.
- It does not change the program outcome. The ruling's governance line for Fork B
  reads: *"accepted ADR unchanged; DAG edge changed—formal amendment required;
  program outcome unchanged."*
- It does not re-open the other four forks, all settled "DAG unchanged".

## 8. Verification on ratification

1. Apply both §4 diffs.
2. Rebind `program_dag_digest` in `program-state.toml` to the edited DAG, in
   the same change, or validation fails closed on the stale digest.
3. ```
   node scripts/validate-program-state.mjs \
     --dag docs/arch/refactor/rev11/program-dag.toml \
     --state docs/arch/architecture-lock/ledger/program-state.toml \
     --mode live
   ```
   The DAG stays acyclic and every predecessor id remains a known block. (A bare
   invocation with no arguments exits 2 on usage — it is not a validation run.)
   This command passes on the current tree: 64 blocks, zero violations.
4. `node scripts/effective-state.mjs` — reports zero findings and exits 0
   (it exits 1 today on the finding quoted in §3).

## 9. Ratification

**RATIFIED**, 2026-08-20, by the codex architect — the authority to whom the
maintainer delegated amendment ratification.

The decision came in two rounds against the tree, not against this document's
account of itself.

**Round 1** returned RATIFY WITH CORRECTIONS: the edge sound, the premise
independently verified against the contract and B6's charter, full traversal
finding one root (`A0`), no cycle and no unreachable block — but seven factual
defects in the supporting prose. All seven were applied.

**Round 2**, against the corrected text, confirmed corrections 1–5 correct and
§4 still exactly the ruling's normalized set, found no remaining undisclosed
architectural consequence, and returned RATIFIED WITH CORRECTIONS on three
residual defects: a stale validator line citation in §5, two relative links one
directory short, and a §8 note claiming live validation currently fails for five
missing authorization records — false, since that finding was resolved by
`MAINTAINER-RULING-PRE-ENFORCEMENT-ACCEPTANCES`. All three were applied and the
link targets verified to resolve.

Those three were citation, path and status defects. Neither round raised an
architectural objection to the edge itself.
