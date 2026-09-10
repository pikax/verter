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
|  `project_semantic_dispatch/evaluate.rs:1008` | deferred-evaluator re-reduction of a substituted shell |
|  `project_semantic_dispatch/relation.rs:5554` | `reduce_relation_conditional` |
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
(`project_semantic_dispatch/locator_shape_binder.rs:262-272`, `build_lower_locator`: the deref'd body is graph-lowered "with the decl's type parameters bound as `TypeParam` shells"). An open
`TypeParam` check is a stable stop of the selection oracle, so the
conditional interns as a deferred shell with both branches lowered
(`build.rs:9148-9155`). Substitution then rewrites that shell — descending all
four subtrees, already counted by the existing
`AuditEvent::SubstituteConditionalDescend` counter
(`project_semantic_dispatch/substitute.rs:699-743`) — and the decision
finally happens in `evaluate.rs:1008`. By then the losing branch has been
interned *and* substituted. TE2-AC1's zero-intern, zero-substitution
proof cannot hold for it without also removing the deferred-shell
carrier, which the same charter forbids.

## Finding 1b — the one remaining escape is closed by substitution

Findings 1–3 each block a *route*. This closes the space, so the abort
does not rest on "no route was found".

If `lower.rs` stops lowering `true_type` / `false_type`, something
still has to be the value of the enclosing declaration body. That value
is produced by the shared `LowerLocator` build
(`locator_shape_binder.rs`, `build_lower_locator`), which graph-lowers
the whole decl body with header parameters bound as `TypeParam` shells.
For `type X<T> = T extends string ? A : Heavy` the check is therefore an
unbound shell, selection is a stable open stop, and `build_conditional`
MUST intern the deferred carrier the charter mandates
(`build.rs:9148-9155`).

That carrier is `SemanticNodeData::Conditional { check, extends,
true_branch_ref, false_branch_ref, distributive }` — five
`SemanticNodeId` fields. Keeping the branches unlowered therefore
requires the two branch fields to stop being node ids and start being
sealed operands. `SemanticNodeData::Conditional` IS inside TE2's
mutation boundary, so that is not a scope objection. It is closed for a
different reason:

- **Substitution has to rewrite the branches.** `substitute.rs`'s
  `Conditional` arm descends into all four sub-trees and rebuilds the
  node when any changed (`substitute.rs:699-747`), with the
  capture-avoidance rule that an inner `extends` re-declaring the same
  `infer` name shadows the extends clause and the TRUE branch while the
  check and FALSE branch still substitute. Substituting `T := string`
  into an unlowered branch operand is impossible without lowering it.
- **The only alternative is a forbidden design.** Deferring the
  substitution instead — accumulating pending `(param, arg)` rewrites on
  the operand so the branch can be lowered-and-substituted later —
  is precisely a recipe for reconstructing a value, which the charter's
  "Deletions and forbidden designs" rules out: *no `SemanticRecipeId`,
  closures, AST pointers, `TypeExpr` operands, arbitrary env maps* and
  *no branch recipe graph*. TE1's substitution axis cannot carry it
  either — it is a positional `Arc<[SemanticNodeId]>` validated against
  the anchor declaration's declared header arity
  (`semantic_operand.rs`, `seal_substitution` /
  `SemanticOperandMintError::SubstitutionArity`), and a conditional's
  pending rewrites are not declaration-header ordinals.

So within TE2's own boundary the branch fields must stay node ids, which
means the branches must be lowered before the deferred carrier is
interned, which means the losing branch of every generic conditional is
interned and substituted before selection is even possible. The
zero-intern / zero-substitution half of TE2-AC1 is not reachable.

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
  `locator_shape.rs:429`, `build.rs:845` and `:1016`, and
  `mod.rs:1924`. A conditional lowered from any of those has no
  authored anchor to seal a branch operand
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

## Independent re-verification

The findings above were re-checked against the candidate tree from the
source rather than accepted from the record. Each item below was
confirmed to be a property of the landed code:

- **Closed operand vocabulary.** `SemanticOperandKind` has exactly two
  arms, `Node { store_identity, generation, node, evidence }` and
  `Authored(Box<AuthoredSemanticOperand>)`
  (`semantic_query/operand.rs`). Only the authored arm defers lowering,
  so a zero-intern dead operand must be nameable as an
  `AuthoredBodyLocator`.
- **Eager branch lowering at the one authored-syntax site.**
  `lower.rs:2226` and `lower.rs:2237` lower `true_type` and `false_type`
  before the `SemanticQueryKey::Conditional` dispatch at `lower.rs:2248`.
  This is the site the charter names for replacement.
- **Seven production construction sites**, six of which can only supply
  materialized branch ids (table above).
- **Generic conditionals materialize both branches before any
  selection is possible.** `build_lower_locator` lowers a declaration
  body with header parameters bound as `TypeParam` shells, so the check
  of `type X<T> = T extends string ? A : Heavy` is an unbound shell at
  lowering time. Selection cannot occur there — it is a stable open stop
  — so `build_conditional` interns the deferred shell with `Heavy`
  already lowered, and only a later substitution + re-reduction decides.
  This covers the entire generic-conditional class, which is every
  conditional utility type in practice. It is the charter's first abort
  clause verbatim: *conditional selection cannot occur without first
  materializing both branches under the existing relation authority.*
- **Infer scoping has no TE1 axis.** `step_crosses_binder_scope`
  (`decl_body_memo/locator_deref.rs`) returns true for
  `TypeBodyPathStep::ConditionalTrue` whenever the sibling `extends`
  declares an `infer`; the crossing route in
  `locator_shape_binder.rs` (`DerefedBodyShape::Single` with
  `lexical_root`) lowers the whole ancestor expression and then
  navigates, so forcing the true operand lowers the false branch too.
  `OperandBinderVisibility` is `Body | Constraint{ordinal} |
  Default{ordinal}` — there is no conditional-`infer` frame to seal.
  The lowering-side binding is an environment mechanism
  (`InferRef` nodes inserted into the true branch's `env` at
  `lower.rs`), not a positional substitution, so it cannot be carried on
  the operand's declaration-header-ordinal substitution axis either.
- **Vocabulary gap is real, and narrower than first recorded.**
  `InferSyntaxPathStep` declares 45 steps
  (`semantic_query/infer_binder_names.rs:16-60`); `TypeBodyPathStep`
  declares 23 (`verter_type_expr/src/locators.rs:143-238`). The counts
  first recorded here (46 / 25) were wrong, and so were two entries of
  the original unaddressable list — the locator layer reaches both of
  those positions by other means:

  - `ParenthesizedInner` is NOT a gap. Parenthesization is structurally
    TRANSPARENT to the locator: every expression arm of
    `navigate_expr_detecting` calls `unwrap_parenthesized` before
    matching (`decl_body_memo/locator_deref.rs:1136-1231`), and
    `step_crosses_binder_scope` peeks through it the same way. A
    conditional inside parentheses is addressed by the SAME path as the
    unparenthesized one; the syntax-path step has no locator counterpart
    because it needs none.
  - `ObjectSpread` is NOT a gap. A spread member is reached as
    `Member { ordinal }` and its operand type as that member's VALUE
    slot: `member_value_expr` maps
    `ObjectMember::Spread(spread) => spread.ty`
    (`locator_deref.rs:1532-1534`).

  The positions that genuinely have no locator spelling are
  `ArrayElement` (`TypeBodyPathStep` has no array arm at all),
  `RestInner`, `KeyOfOperand`, `TemplateExpression`,
  `ImportTypeArgument` and `TypeOfTypeArgument` (the `TypeArgument`
  step derefs `TypeExpr::Ref` ONLY —
  `locator_deref.rs:1136-1148`), and every NON-HEADER type-parameter
  bound: `TypeParamBound` is documented and implemented as valid only
  as the FIRST path step rooted at the declaration header
  (`locators.rs:159-167`), so a function type's, an object method's, a
  call signature's, or a construct signature's own type-parameter
  constraint / default is unaddressable.

  The correction narrows the list but does not change the finding's
  force: `ArrayElement` alone
  (`type F<T> = (T extends string ? A : B)[]`) keeps the gap
  load-bearing, and closing it still means changing `TypeBodyPathStep`
  and its deref, navigator, identity and witness mirrors in a third
  crate.
