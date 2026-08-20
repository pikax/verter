# AMD-012 — J1 is promoted from subsystem to foundational

**Status:** NOT RATIFIED — awaiting the codex architect, to whom the maintainer
delegated amendment ratification. The edit in §3 is proposed text, not applied
text.

**Prepared against:** local `program/architecture-lock` commit `6afba972d403ad1b8779983884b40ff02ce4f24b`.

**Amends on ratification:** two files —
[`../program-dag.toml`](../program-dag.toml) (the `class` field of the `J1`
block row, line 330) and the `program_dag_digest` field in
[`../../../architecture-lock/ledger/program-state.toml`](../../../architecture-lock/ledger/program-state.toml),
rebound to the edited DAG. **It changes no DAG edge, adds no block, retires no
block, and moves no acceptance owner off any capability-matrix cell.**

## 1. Why this exists

[`ARCH-RULING-J-TRAIN-FIVE-FORKS`](../rulings/ARCH-RULING-J-TRAIN-FIVE-FORKS.md)
decided the five forks blocking Track J charter drafting. Four resolve inside the
charters. One does not: the ruling records, as a non-edge correction, that **J1
must be promoted from `subsystem` to `foundational`**. That is a
`program-dag.toml` change, so it needs a formal amendment. The ruling states on
its own face that it does not apply it.

## 2. The trigger was pre-declared

This is not a new judgement. J1's own charter template declares the promotion
condition in advance ([`../charters/J1.template.md`](../charters/J1.template.md),
line 4):

> **Class:** Subsystem, promoted to Foundational if it changes shared syntax
> ownership or public compatibility.

Both limbs are now met, by the maintainer's ratified directive rather than by
this amendment:

- **Shared syntax ownership changes.** `StyleSyntaxIr` becomes the sole
  CSS-family syntax authority and Lightning CSS is removed entirely
  ([`MAINTAINER-DIRECTIVE-CSS-CLEAN-CUTOVER`](../rulings/MAINTAINER-DIRECTIVE-CSS-CLEAN-CUTOVER.md),
  §"Architectural decision" and §"Lightning CSS removal").
- **Public compatibility changes.** The directive supersedes the standalone CSS
  API and instructs that where existing public behaviour conflicts with the
  architecture, the architecture wins and the breaking change is recorded
  (§"Public API cleanup", §"Final invariant").

The template's condition has fired. The amendment records that it fired.

## 3. The proposed change

`docs/arch/refactor/rev11/program-dag.toml`, the `J1` block row at line 330:

```diff
 [[block]]
 id = "J1"
 name = "CSS owner reconciliation"
-class = "subsystem"
+class = "foundational"
 predecessors = ["A4", "A6"]
```

## 4. Consequences

- J1 is held to the foundational bar rather than the subsystem bar. Given that
  J1 now also absorbs J4's parser-coverage, no-duplicate-grammar and
  no-fallback-dependency evidence (per the same ruling's fork 1), and is
  parity-gated for every currently retained Native operation (fork 5), the
  foundational bar is the one it was already going to have to clear.
- `foundational` is the DAG's most common class — 40 of 64 rows carry it, and
  `subsystem` drops from 13 rows to 12. No new class is introduced.
- No edge changes. The ruling examined J2's and J3's predecessor needs
  explicitly and found both already satisfied, with B4's mapping substrate
  reaching J3 transitively through `B4 → BV1/BS1 → B5 → B6 → J3`.
- J2, J3 and J4 keep their classes.

## 5. What this does NOT do

- It does not accept, unlock, or dispatch J1 or any other block.
- It does not draft or ratify any Track J charter. The ruling binds those
  charters; this amendment only records the class change the ruling identified.
- It does not change any contract, ADR, or capability-matrix cell.

## 6. Verification on ratification

1. Apply the §3 diff.
2. Rebind `program_dag_digest` in `program-state.toml` in the same change, or
   validation fails closed on the stale digest.
3. ```
   node scripts/validate-program-state.mjs \
     --dag docs/arch/refactor/rev11/program-dag.toml \
     --state docs/arch/architecture-lock/ledger/program-state.toml \
     --mode live
   ```
   Expect OK at 64 blocks, zero violations, as on the current tree.
4. `node scripts/effective-state.mjs` — expect zero findings, exit 0.

## 7. Ratification

_Unrecorded._ Completed by the codex architect. Until it carries a decision, J1's
class remains `subsystem`.
