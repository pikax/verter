# AMD-002 — A2C completion-model predecessor for A3

**Status:** Registered amendment (maintainer-ratified exception to the normal
verbatim-authority policy — see [`../PROVENANCE.md`](../PROVENANCE.md)).
**Registered in:** [`../README.md`](../README.md) and
[`../evidence/maintainer-rulings.md`](../evidence/maintainer-rulings.md) (R-9).
**Amends:** [`../program-dag.toml`](../program-dag.toml),
[`../program.md`](../program.md), [`../charters/A3.md`](../charters/A3.md),
[`../templates/program-state.template.toml`](../templates/program-state.template.toml),
[`../../../u6-flow-return-gaps-and-target.md`](../../../u6-flow-return-gaps-and-target.md),
and [`../../../native-flow-return.md`](../../../native-flow-return.md).

The published consolidated master, release artifacts, `_EXTRACTION_INDEX.md`, and
historical readiness-review prose remain immutable historical originals. The
originally reconstructed bytes remain recoverable from the pinned consolidated master;
the current live split files named in `PROVENANCE.md` are execution authority as
amended here.

## The defect

`A3` must retract every A2-catalogued wrong-complete result, including G10, without
misclassifying checker-correct clean results. The existing boolean
`statement_guarantees_current_function_return` cannot distinguish normal, return,
throw, labeled or unlabeled break and continue, `try`/`catch`/`finally` replacement,
structural authored-return membership, endpoint `undefined`, or unknown completion.
Landing A3 without G10 would knowingly preserve a catalogued wrong-and-warm result;
extending the boolean would create a second syntax-shaped completion authority.

## The amendment

1. Insert block `A2C` — “Abrupt-completion facts for G10 safety discrimination” —
   directly after `A2` and directly before `A3`. `A2C` has class
   `foundational-safety` and predecessor `A2`; `A3`'s sole predecessor becomes
   `A2C`. No other block predecessor changes. The DAG and exact-state template each
   contain 51 blocks.
2. `A2C` adds one content-free, exact-or-typed-unknown completion fact family on
   `FunctionBodySkeleton`, computed once during skeleton construction. It owns the
   completion vocabulary and composition, structural authored-return membership,
   endpoint-`undefined` disposition, and fact-level discrimination evidence. It does
   not change public semantic results.
3. `A3` retains its full exit criterion. It consumes accepted A2C facts as the sole
   G10 discriminator and may retract proved or unknown unsafe completion cases to a
   typed non-admissible outcome. It may not add or reinterpret completion rules.
4. `D6` / `U6.LOOP_CLOSURE` consumes the same A2C completion algebra as its structural
   input and remains the owner of graph edges, loop fixed points, state routing, and
   final clean semantics. D5 and D8 ownership and predecessor lists remain unchanged.
5. The external live `program-state.toml` must receive the same `A2C` row as the
   tracked exact-state template, with its DAG/package digests recomputed, before it is
   used for a program transition.

## Execution precedence

For execution, AMD-002 and the amended live split files supersede every old
`A0 → A1 → A2 → A3` lineage in the pinned consolidated master, release artifacts,
`_EXTRACTION_INDEX.md`, and historical readiness-review prose. Their historical bytes
remain unchanged; the executable lineage is
`A0 → A1 → A2 → A2C → A3 → A4 → A5 → A6`.

## Scope boundary

`A2C` does not own value typing, capture/effect transfer, loop fixed-point state,
graph-edge emission, obligation-ledger closure, complete-result construction, cache
admission, compatibility repair, or a second flow representation. Unsupported facts
remain typed unknown and fail closed; they are never guessed exact.
