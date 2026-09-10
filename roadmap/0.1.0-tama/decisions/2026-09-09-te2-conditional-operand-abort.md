# TE2 abort before mutation: conditional branch operands are not representable under the TE1 seal

- Status: proposed — requires maintainer ratification
- Date: 2026-09-09
- Node: TE2 — Conditional selective forcing (train `rev11.type-evaluation`)
- Concerns: `charters/rev11-type-evaluation/TE2.md` — "Independently
  acceptable outcome", "Source-specific scope" (dead-operand proof),
  "Acceptance IDs" TE2-AC1, and "Abort conditions"
- Ledger: `authority/state/implemented.toml` row `"TE2"` stays
  `status = "pending"`. No production source was mutated.

## Disposition

TE2 was stopped **before mutation** under two of its own abort
conditions:

> Stop before mutation if any predecessor lacks an implemented ledger
> row, **conditional selection cannot occur without first materializing
> both branches under the existing relation authority**, or **exact
> infer scoping cannot be represented by TE1 identity**.

All three predecessors (TE1, D3C, TA1B) are implemented rows, so
readiness was satisfied. Both of the remaining abort clauses hold on the
tree at `0c91f864`. The findings below are structural properties of the
landed code, not effort or budget objections.

## What TE2 requires

The charter requires `SemanticQueryKey::Conditional` to stop carrying
four already-materialized branch nodes and instead carry TE1 sealed
operands plus the force request's one complete
`ProjectionReductionContext`, with this proof obligation for a decided
conditional (TE2-AC1, "Dead-operand proof"):

> for the losing branch of a decided conditional, attributable semantic
> counters must be exactly zero: forcing attempts, locator
> dereferences, substitutions, nested dispatches (including relation
> reads), **semantic allocations/interns/origin edges**, and semantic
> fact reads.

"Zero interns" means the losing branch must never be lowered. Under
TE1's closed operand vocabulary
(`crates/verter_session/src/semantic_query/operand.rs`) the only operand
arm that defers lowering is `Authored(AuthoredBodyLocator,
substitution)`; the `Node` arm is by definition an already-materialized
handle. So every conditional-dispatch site must be able to name each
branch as an authored locator.

## Finding 1 — six of the seven conditional dispatch sites have only materialized branches

`SemanticQueryKey::Conditional` is *constructed* at seven production
sites (every other production mention destructures an existing key
rather than building one; the remaining matches are tests). Only the
lowering site has authored syntax in hand. Five of the other six start
from an **already-interned `SemanticNodeData::Conditional` shell** and
take `true_branch_ref` / `false_branch_ref` straight off it; the sixth
re-dispatches the branch ids of the key it was already handed. At those
sites there is no authored locator, no `TypeExpr`, and nothing lazily
addressable — the branches were interned before the deciding dispatch
was ever formed:

| site | shape |
| --- | --- |
| `project_semantic_dispatch/evaluate.rs:1001` | deferred-evaluator re-reduction of a substituted shell |
| `project_semantic_dispatch/relation.rs:5542` | `reduce_relation_conditional` |
| `project_semantic_dispatch/raise.rs:1228` | `dispatch_operator_with_recurse` |
| `project_semantic_dispatch/locator_view_worklist.rs:633` | projection finish frame |
| `meta_resolve/dispatch_helpers.rs:167` | callable-surface realization |
| `project_semantic_dispatch/build.rs:9046` | distributive per-member re-dispatch (reuses the incoming key's branch ids) |

This is not incidental. The charter itself mandates that the shell
survive:

> **Open/deferred:** preserve a `SemanticNodeData::Conditional` shell.

Once a conditional defers, its branch refs are materialized node ids for
the rest of their life, and every later decided re-reduction of that same
conditional can only be handed materialized branches.

That path is the dominant lifecycle for a generic conditional, not an
edge case. `build_instantiate`'s declaration-source route lowers the body
**unsubstituted**, with header parameters as `TypeParam` shells
(`project_semantic_dispatch/locator_shape_binder.rs:258` onward). An open
`TypeParam` check is a stable stop of the selection oracle, so the
conditional interns as a deferred shell with both branches lowered
(`build.rs:9148`). Substitution then rewrites that shell — descending all
four subtrees, already counted by the existing
`AuditEvent::SubstituteConditionalDescend` counter
(`project_semantic_dispatch/substitute.rs:699-743`) — and the decision
finally happens in `evaluate.rs:1001`. By then the losing branch has been
interned *and* substituted. TE2-AC1's zero-intern, zero-substitution
proof cannot hold for it without also removing the deferred-shell
carrier, which the same charter forbids.

## Finding 2 — the true branch of an infer conditional cannot be forced in isolation

Even where an authored locator exists, forcing a
`TypeBodyPathStep::ConditionalTrue` operand does not lower only that
branch. `step_crosses_binder_scope`
(`decl_body_memo/locator_deref.rs:1365`) classifies `ConditionalTrue` as
binder-crossing exactly when the sibling `extends` declares an `infer`.
For a crossing step the deref returns a `lexical_root`, and the
`LowerLocator` build re-lowers the **whole enclosing conditional** and
then navigates to the branch (`locator_shape_binder.rs:478-486`,
`navigate_lowered_locator`). Forcing the true operand of `T extends infer
X ? A : B` therefore lowers the check, the extends pattern, **and the
false branch**.

That rail exists precisely because the true branch's `InferRef` nodes
must bind to the `Infer` declarations minted from the sibling `extends`
(`lower.rs:2171-2230`). TE1's authored identity has no axis for that
frame: `OperandBinderVisibility` is `Body | Constraint{ordinal} |
Default{ordinal}` (`semantic_query/operand.rs:125-133`) and the
substitution axis is a positional `Arc<[SemanticNodeId]>` bound to
*declaration header* ordinals (`semantic_operand.rs:204-253`). A
conditional-`infer` frame is neither. This is the charter's second abort
clause verbatim: exact infer scoping is not representable by TE1
identity.

## Finding 3 — many conditional positions have no authored locator at all

Two further gaps, recorded for completeness because they also block a
locator-based branch operand:

- **Vocabulary gap.** The lowering-side syntax path
  (`semantic_query/infer_binder_names.rs:15-60`,
  `InferSyntaxPathStep`) is strictly richer than the addressable locator
  path (`verter_type_expr/src/locators.rs:143-238`,
  `TypeBodyPathStep`). A conditional under an array element, a rest
  element, a `keyof` operand, a template-literal expression, an
  `import()` / `typeof` type argument, or an object method's type
  parameter has no locator spelling. Closing that gap means changing
  `TypeBodyPathStep` and its deref, navigator, identity and witness
  mirrors — a third crate (`verter_type_expr`) against a two-package
  budget, and well past eight files.
- **Rootless lowering sites.** `shallow_lower_type_expr_with_context`
  (`lower.rs:877`) builds an `InferBinderFactory` with a **transient**
  root — a hash of the `TypeExpr`, not a locator
  (`semantic_query.rs:362-376`). Live production callers include
  `flow_return.rs`, `carrier.rs:676`,
  `structural_carrier_producer/macro_arg_producer.rs:179`,
  `build.rs:845` and `:1016`, and `mod.rs:1924`. A conditional lowered
  from any of those has no authored anchor to seal a branch operand
  against, and inventing one would require exactly the `TypeExpr`
  operands / source hashes the charter's "forbidden designs" list rules
  out.

## What was NOT the reason

Not effort, size, breadth, or migration cost. The context-axis half of
the charter is straightforwardly implementable: `FamilyKey::Conditional`
is currently context-free with `ModeSlot::Single`
(`semantic_query_memo/family.rs:1663`), and folding the request's five
`ProjectionReductionContext` axes into it is ordinary work. It is the
operand half — lazy branches with a zero-intern dead-operand proof —
that the landed representation cannot express.

## Requested ratification

One of the following, as a DAG amendment before TE2 is re-dispatched:

1. **Re-scope TE2** to the reachable outcome: keep branches as
   materialized `Node`-arm operands, add the request-owned
   `ProjectionReductionContext` to conditional family identity, and
   replace TE2-AC1's zero-intern dead-operand proof with a
   zero-*forcing* / zero-*nested-dispatch* / zero-*substitution-after-
   selection* proof that the landed shell can actually satisfy. Or
2. **Insert a predecessor** that owns (a) an authored path vocabulary
   covering every conditional position, and (b) a sealed
   conditional-`infer` binder frame on TE1 identity, so a true-branch
   operand can be forced without re-lowering its enclosing conditional.
   TE2 then lands unchanged on top of it. Or
3. **Ratify the current charter as-is** with an explicit statement of
   which of Findings 1–3 is considered wrong, so implementation can
   resume against a corrected reading.

Until one of these is recorded, the TE2 row stays `pending` and no
production source under `crates/verter_session/src` is changed.
