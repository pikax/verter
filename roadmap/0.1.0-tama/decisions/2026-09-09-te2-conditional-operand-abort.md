# TE2 abort before mutation: conditional branch operands are not representable under the TE1 seal

- Status: proposed — requires maintainer ratification
- Date: 2026-09-09
- Node: TE2 — Conditional selective forcing (train `rev11.type-evaluation`)
- Concerns: `charters/rev11-type-evaluation/TE2.md` — "Independently
  acceptable outcome", "Source-specific scope" (dead-operand proof),
  "Acceptance IDs" TE2-AC1, and "Abort conditions"
- Ledger: `authority/state/implemented.toml` row `"TE2"` stays
  `status = "pending"`. The captured TE2 charter, the rev11
  type-evaluation DAG, and TE4's predecessor list are unchanged. The only
  production changes on this candidate are the ratification-independent
  dependency-fact fixes recorded under "Delivered"; the operand cutover is
  not implemented.

## Proposed rescope — NOT ratified

A rescope ruling was once recorded on this candidate branch together with a
rewritten TE2 charter, a new `TE2B` node, a TE4 predecessor change, and a
TE2 ledger transition. Review rejected it: the ruling existed only inside
the candidate's own commit, with no maintainer commit on the default branch
and no workflow architect-approval artifact behind it. An implementer
cannot cure its own unmet acceptance criteria by rewriting the contract it
is judged against. Those charter, DAG and ledger edits are withdrawn; the
text below is retained verbatim in substance as a **proposal** for the
maintainer, and binds nothing until it is ratified by an independent
authority.

**Proposed ruling: RESCOPE.** The grounds are properties of committed bytes
a reader can re-check:

- `step_crosses_binder_scope` (`decl_body_memo/locator_deref.rs`)
  returns true for `TypeBodyPathStep::ConditionalTrue` exactly when the
  sibling `extends` declares an `infer`, and the crossing route in
  `locator_shape_binder.rs` lowers `root.expr` — the whole enclosing
  conditional — before navigating. An authored true-branch force of an
  infer conditional therefore lowers the false branch. Finding 2 holds.
- `OperandBinderVisibility` is `Body | Constraint { ordinal } |
  Default { ordinal }` and `SemanticOperandKind` has exactly the
  `Node` and `Authored` arms; the true-branch infer binding in
  `lower.rs` is an environment insert of `Infer`/`InferRef` nodes,
  not a header-ordinal substitution. There is no frame on TE1 identity to
  seal it against. The charter's own abort clause — "exact infer scoping
  cannot be represented by TE1 identity" — is met on this tree.
- `TypeBodyPathStep` has no array, rest, keyof-operand, template,
  import-argument, or typeof-argument arm, so closing the authored-position
  gap changes `verter_type_expr`, outside the node's file set. Finding 3
  holds.
- `SemanticQueryKey::Conditional` is constructed at seven production
  sites, six of which destructure an already-interned shell or re-dispatch
  an incoming key's branch ids; the shell the charter mandates for open
  conditionals is interned with both branches lowered, and the
  `substitute.rs` `Conditional` arm descends all four subtrees before the
  `evaluate.rs` re-decision. Findings 1 and 1b hold under the strict
  reading of the counter list.
- `conditional_branch_selection` consumes only `check` and `extends`
  and routes every relation read through `SemanticQueryKey::Relate`; the
  decided arm of `build_conditional` returns the winner node and records
  its origin edge with sources `[check, extends]` only. The candidate's
  three source commits (`93ebdcc7`, `7dd244d1`, `315369d3`) remove the
  dispatch-boundary residue — rooting a decided, distributed, or absorbed
  result on a branch the answer never read — with tests whose negative
  controls were proven to apply.
- `force_semantic_operand` has no production caller; the authored
  vocabulary was never exercised at a binder-crossing position, which is why
  the gap surfaced here and not at TE1 or TE3.

**Why not REJECT.** The charter is not implementable as written inside its
own boundary: its lazy-`lower.rs` clause and its required "nested
same-name infer binders" fixture both need an isolated authored
`ConditionalTrue` force, and the source shows that force re-lowers the
enclosing conditional. Making it representable changes TE1's sealed identity
and the decl-body locator route, both outside TE2's named files and both a
TE1 completeness concern. The charter's first-listed abort clause is
therefore correctly invoked, and a reject would order the implementer to
substitute a local redesign of TE1 for the ratification CLAUDE.md requires.

**Why not ABORT.** The founding decision's outcome — a decided conditional
forces only its selected branch and dead operands add no dependency facts —
is neither wrong nor unreachable; only the authored-operand *mechanism* for
conditionals is. TE4 and TE5 consume the outcome. Cancelling the node would
discard a landed, tested correctness fix that the charter itself names
("dead operands add no semantic dependency facts") and would leave the
lowering-site gap the founding decision identified without an owner.

**The reading.** TE2-AC1's counter list is read as attributable to the
decided conditional's own evaluation at the dispatch boundary (the narrow
reading). Lowering the parameterized body once per declaration content is
the TE4 lower-once contract, not dead-operand work; the substitution descend
of an open shell and the eager lowering-site branch lowering are real dead
work and are carried forward as a separately owned node rather than waived.

**What the proposal would change if ratified.** None of the following is
applied on this candidate.

1. `charters/rev11-type-evaluation/TE2.md` would be rewritten to the rescoped
   contract: selection before branch forcing at the dispatch boundary over
   materialized branch operands; the losing branch dead for forcing,
   dispatch, relation, origin, fact-read, and dependency-fact purposes;
   infer bindings scoped to the selected true branch; open shell with
   walker residual projection; distributivity in `build_conditional` with
   selection-observed dependency facts. The header is unchanged, so the
   DAG topology for TE2 is unchanged.
2. A new node `TE2B — Conditional branch forcing at the lowering and
   instantiation sites` (train `rev11.type-evaluation`, predecessors
   TE2 and TE3, budgets 800/8/2 with 1500/12/3 rescope) owns the excluded
   scope: lowering-site selection before branch lowering through the one
   oracle, and instantiation-site deferral of losing-branch substitution
   through a sealed content-free pairing of a materialized handle with its
   pending positional substitution frame, with no selection during
   substitution. It forbids the authored-operand cutover of the conditional
   family, the conditional-`infer` binder frame, and any
   `verter_type_expr` vocabulary change: those are a TE1 completeness gap
   that a future consumer records as its own DAG amendment if it needs an
   isolated authored binder-crossing force. It also forbids the
   projection-context axes in `FamilyKey::Conditional`, for the reason
   recorded below (conditional reduction is context-free; the winner is
   projected under the consumer's own `ProjectPath` context).
3. TE4's predecessors become TE2, TE2B, and TE3, because TE4's demanded-key
   value forcing applies a substitution to a materialized value template
   and then forces it — the same materialized-handle-plus-substitution
   shape TE2B must seal — and must reuse it rather than mint a second one.
   TE5's edges are unchanged; it inherits TE2B through TE4.
4. The ledger row `"TE2"` would transition to implemented against the
   rescoped charter, and `"TE2B"` would be predeclared pending. Only the
   ratifying authority may make those edits.

**Precedence.** Until ratification, the captured TE2 charter and
`decisions/2026-09-01-demand-selected-semantic-operand-forcing.md` remain
the binding authority; this proposal does not supersede them.

**Open, not waived.** The TE1 completeness gap (an authored conditional-infer
binder frame and locator vocabulary for every conditional position) is
recorded here. Under the proposal it would also be named in TE2B's
forbidden designs and left unscheduled until a node needs an isolated
authored binder-crossing force.

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
production source under `crates/verter_session/src` is changed beyond the
ratification-independent dependency-fact fixes under "Delivered".

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

## The one question that decides which ratification applies

Findings 1 and 1b rest on a reading of TE2-AC1's counter list that the
charter does not disambiguate, and the choice between ratification
options 1 and 2 turns entirely on it. Stating it explicitly is the point
of this section; nothing below withdraws a finding.

The list is:

> for the losing branch of a decided conditional, attributable semantic
> counters must be exactly zero: forcing attempts, locator
> dereferences, substitutions, nested dispatches (including relation
> reads), semantic allocations/interns/origin edges, and semantic fact
> reads.

**Strict reading — every such event over the losing branch's lifetime.**
Findings 1/1b apply: for a generic conditional the branch is interned
while the conditional is still OPEN and is descended by `substitute`
before any decision exists, so `interns` and `substitutions` are
non-zero no matter how the decided arm behaves. TE2 is unreachable and
ratification option 2 (a new predecessor) is the only route.

**Narrow reading — events attributable to the DECIDED conditional's own
evaluation.** Then the two Finding-1 events fall outside the count, and
each is already licensed elsewhere in the same charter:

- The deferred intern happens in the OPEN arm, which the charter itself
  mandates (*"Open/deferred: preserve a `SemanticNodeData::Conditional`
  shell"*) and where it explicitly permits demand in both branch
  operands.
- The substitution descend happens in `substitute`, not in
  `build_conditional`, and rewrites a shell the charter requires to
  exist.

Under that reading the decided arm is already close to compliant.
`build_conditional`'s selection (`build.rs:9100`) consumes only `check`
and `extends`; the decided arm binds `result` to the winner alone
(`build.rs:9146-9148`) and records the `ConditionalSelect` origin edge
with sources `[check, extends]` only — the loser receives no origin
edge, no force, no nested dispatch and no fact read.

One residue remained even under the narrow reading, and it was small,
in-scope and concrete: `build_conditional` derived the result's observed
self-roots from all FOUR nodes, including the loser. That is not a
content re-read (`observed_self_roots_from_nodes` only projects the
identity each node already carries), and AC3 explicitly tolerates a
conservative same-owner rejection — but when the losing branch is owned
by a DIFFERENT file it contributed a cross-file root the winner's value
does not depend on, which was the one place the landed decided arm still
contradicted *"dead operands add no semantic dependency facts"*.

**That residue is now closed** — see "Delivered" below. It is the only
part of TE2 that was reachable without a ratification, because it is
required under BOTH readings of the counter list: a decided
conditional's dependency facts are wrong on the loser however the
intern/substitute events are accounted.

**Finding 2 survives BOTH readings.** It does not concern lifetime
accounting: forcing an `Authored` `ConditionalTrue` operand whose
sibling `extends` declares an `infer` re-lowers the whole enclosing
conditional, false branch included, at the moment of the force — and
`OperandBinderVisibility` has no frame to seal the binding against. So
even under the narrow reading the lower.rs lazy-branch replacement that
"Source-specific scope" mandates stays unreachable, and the charter's
own REQUIRED "nested same-name infer binders" fixture cannot pass. A
narrow-reading option 1 must therefore also drop or re-express that
clause, not merely restate AC1.

**Requested of the maintainer:** state which reading was intended. If
strict, option 2. If narrow, option 1 is small and concrete — keep
`Node`-arm operands at the six materialized construction sites, fold the
request's five `ProjectionReductionContext` axes into the currently
context-free `FamilyKey::Conditional`, and restate AC1 as
zero-forcing / zero-nested-dispatch / zero-origin-edge / zero-fact-read
on the loser of the decided evaluation. (The self-root half of that list
is already done — see "Delivered".)

The `FamilyKey::Conditional` context axis is deliberately NOT landed
ahead of the ratification. Conditional reduction does not read a
projection context today: `build_conditional` takes only the four nodes
plus `distributive`. Folding five context axes into the key before the
operand cutover makes the value context-dependent adds nothing and
fragments one warm entry into up to five axes' worth of duplicates — a
pure cache regression against the charter's own "zero warm-candidate
growth" budget. It is correct only together with the operand half that
makes the forced branch context-sensitive.

## Delivered

The dead-operand DEPENDENCY-FACT residue above is fixed on this
candidate, because it is required under both readings and needs no
ratification:

- `build_conditional` no longer roots a DECIDED conditional on the
  losing branch. The two decided arms (relation selection, and the
  binding-producing infer selection, which is always decided-true) root
  on `check`, `extends`, and the WINNER only. The deferred shell
  publishes both branch references in its value, so it keeps the full
  four-node set.
- A DISTRIBUTED conditional no longer roots on both branches
  unconditionally either. Its value is the normalised union of the
  per-member sub-conditionals, each of which is a nested
  `SemanticQueryKey::Conditional` read whose own fact rail (the
  member's winner, or both branches for a member that stays open)
  bubbles into the parent's read set exactly as every other nested
  dependency does. The parent therefore roots directly only on the
  distributive `check`, the resolved union surface it distributed over
  (so a global alias over a file-scoped union still roots the union's
  file), and `extends`; the branch files arrive as member-observed
  facts. When every member selects the same winner, the other branch
  contributes no dependency fact — the clause the charter states for
  dead operands, previously violated on the distributed arm.
- Staleness is impossible in the other direction: the memo key carries
  both branch node ids, so an edit that changes the losing branch mints
  a different id and therefore a different key rather than serving a
  stale hit. The change only removes rejections of values that never
  read the rejecting file.
- Discriminating proof, decided arm:
  `decided_conditional_roots_only_on_check_extends_and_the_winner`
  (`project_semantic_dispatch/tests.rs`) puts the two branch shells in
  DISTINCT files and asserts the decided-true root set is exactly the
  true branch's file, the decided-false root set is exactly the false
  branch's file (so the assertion cannot pass by always dropping one
  fixed branch), and the deferred shell keeps both. Restoring the
  four-node root set fails it.
- Discriminating proof, distributed arm:
  `distributed_conditional_dependencies_follow_member_selections`
  (same file) lowers check, extends and both branches from FOUR distinct
  files and table-drives all-true, all-false, mixed and open member
  sets, asserting the parent's completed read set carries exactly the
  files the members consumed — for a raw union check and for a global
  alias over it, on the cold build and again on the warm hit (which
  must not rebuild members). Restoring the unconditional four-node root
  set on the distributed arm fails the all-true and all-false rows.
- An ABSORBED conditional no longer roots on branches it never read. An
  `error` check and a distributive `never` check decide without reading
  either branch, so those rows root on `check` and `extends` only; the
  `any` row publishes the union of both branches and keeps all four
  roots. Discriminating proof: the same decided-conditional test builds
  those rows over file-scoped branch shells and asserts no branch file is
  rooted for `error`/`never` and both are for `any`; restoring the
  four-node set on the `error`/`never` rows fails it.

Nothing else in TE2 is landed. The operand cutover, the lazy `lower.rs`
branches, the conditional family context axis, and TE2-AC1/AC2/AC4 all
remain blocked on one of the three ratifications above.

## Why this surfaced at TE2 and not earlier

The forcing boundary TE2 is asked to consume has **no production
consumer today**. `ProjectSemanticDispatch::force_semantic_operand` is
called only from its co-located tests
(`semantic_operand_tests.rs`, `semantic_operand_binder_tests.rs`);
`SemanticOperand::from_authored_authority` and `SemanticOperand::node`
are minted nowhere outside the boundary module and those tests. Every
other production mention of the name is `QueryError` plumbing
(`ForeignSemanticOperand` / `StaleSemanticOperand` /
`IncompleteSemanticOperand`) in `broad_runtime.rs`,
`symbol_identity.rs`, `component_meta_query_engine/surface.rs` and
`compat_spelling.rs`.

The request vocabulary is likewise present and unwired:
`SemanticOperandForceRequest::new` / `::projecting` / `::key_domain` and
`SemanticOperandForceProjection` all exist and are exercised only by
tests.

TE2 would therefore be the FIRST production wiring of the boundary. That
reframes ratification option 2: the missing capability — an authored
path vocabulary that reaches every position a real consumer must name,
and a binder frame for a conditional `infer` — is a completeness gap in
the TE1 substrate, not something specific to conditionals. Any later
node that tries to force an authored operand at a position the locator
cannot spell will hit the same wall. Sizing the predecessor as a TE1
completion rather than a TE2 prerequisite is likely the cheaper reading
for the program.

This also explains how TE3 landed without exposing the gap: it extended
the force REQUEST vocabulary (the projection and key-domain arms above)
rather than standing up a production consumer that had to address
arbitrary authored positions.
