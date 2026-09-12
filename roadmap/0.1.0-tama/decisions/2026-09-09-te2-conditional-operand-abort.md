# TE2 abort before mutation: conditional branch operands are not representable under the TE1 seal

- Status: ratified — RESCOPE (architect ruling, 2026-09-11)
- Date: 2026-09-09 (abort recorded); 2026-09-11 (ratified)
- Node: TE2 — Conditional selective forcing (train `rev11.type-evaluation`)
- Concerns: `charters/rev11-type-evaluation/TE2.md` — "Independently
  acceptable outcome", "Source-specific scope" (dead-operand proof),
  "Acceptance IDs" TE2-AC1, and "Abort conditions"
- Ledger: `authority/state/implemented.toml` row `"TE2"` transitions to
  `status = "implemented"` against the rescoped charter; `"TE2B"` is
  predeclared `pending`. `authority/dag/rev11-type-evaluation.toml` gains
  the `TE2B` node (predecessors TE2, TE3) and TE4's predecessors become
  TE2, TE2B, TE3. The production changes on this candidate are the
  dependency-fact fixes recorded under "Delivered"; the lowering-site and
  instantiation-site cutover is `TE2B`'s.

## Ratification — RESCOPE (architect ruling, 2026-09-11)

This section is the ratification. It was written by the architect selected
by the workflow to rule on this candidate, after reading the captured
charter, the candidate against its baseline `0c91f864`, the retained author
turns, and the existing decision records, and after re-deriving every
load-bearing ground from the source on the candidate tree at `7a93382a`. It
replaces the earlier self-recorded ruling that review rejected for lacking an
authority outside the implementer's own commit; the grounds below are stated
so a reader can re-check each against committed bytes rather than trust this
paragraph.

**Ruling: RESCOPE.** TE2 is delivered under the amended contract now in
`charters/rev11-type-evaluation/TE2.md` — selection before branch forcing at
the dispatch boundary over materialized branch operands, with the losing
branch dead for forcing, dispatch, relation, origin, fact-read, and
dependency-fact purposes, and distributed and absorbed dependency facts
following what each result read. The lowering-site and instantiation-site
cutover is carried to the new node `TE2B` (`charters/rev11-type-evaluation/TE2B.md`),
not waived. TE4 depends on `TE2B` because its demanded-key value forcing must
reuse the same sealed materialized-handle-plus-pending-substitution pairing
rather than mint a second one.

**Grounds, re-verified from the source this turn.**

- TE1's authored operand identity is
  `AuthoredSemanticOperand { locator, lexical_scope, binder, substitution,
  split_env, .. }` (`semantic_query/operand.rs`). `binder` is
  `OperandBinderIdentity { visibility: OperandBinderVisibility }` with
  exactly the arms `Body | Constraint { ordinal } | Default { ordinal }`;
  `substitution` is a positional `Arc<[SemanticNodeId]>` bound to the
  anchor declaration's header ordinals. TE1's own doc comment on
  `OperandBinderIdentity` states the design rule directly: runtime binder
  handles — `NodeScopeId`, `InferBinderId`, a mapper binder — "are store-
  and generation-local, so they are identity only on the runtime-node arm
  of `SemanticOperand` and never on the authored arm, whose whole point is
  to survive independently of any one graph." A conditional-`infer` frame on
  an authored true-branch operand is therefore excluded by TE1's ratified
  design, not merely absent from it. The captured charter's abort clause
  "exact infer scoping cannot be represented by TE1 identity" is met.
- The captured charter lets TE2 "specialize the closed force-request
  vocabulary" but not change operand identity; it lists
  `semantic_query.rs` and `project_semantic_dispatch` as its surfaces and
  admits two further files only "if the closed TE1 request vocabulary
  requires them". Adding a binder frame to the authored arm is a TE1
  amendment outside that boundary.
- `step_crosses_binder_scope` (`decl_body_memo/locator_deref.rs`) returns
  true for `TypeBodyPathStep::ConditionalTrue` exactly when the sibling
  `extends` declares an `infer`, and the binder-crossing route re-lowers the
  whole lexical ancestor before navigating. An isolated authored true-branch
  force of an infer conditional lowers the false branch.
- The lowering-side infer binding is an environment insert
  (`SemanticNodeData::Infer` into the `extends` environment,
  `SemanticNodeData::InferRef` into the true branch's environment,
  `project_semantic_dispatch/lower.rs`), not a positional substitution;
  neither TE1 axis can carry it.
- `SemanticQueryKey::Conditional` / `FamilyKey::Conditional` carry four
  `SemanticNodeId` operands plus `distributive`; `force_semantic_operand`
  has no production caller. The cumulative production delta against
  `0c91f864` is `build.rs` and `absorb.rs` dependency-fact rooting plus
  tests: the dispatch-boundary clause of the charter, nothing else.

**Why not REJECT.** REJECT asserts the captured charter is implementable
as written. It is not: its lazy-`lower.rs` clause and its required "nested
same-name infer binders" fixture both need an authored `ConditionalTrue`
operand that can be forced in isolation under committed infer bindings, and
TE1 identity excludes that frame by design. Ordering the implementer to
proceed would order a TE1 redesign the captured charter does not authorize
and CLAUDE.md reserves for ratification. Sixteen implementer turns across
two harnesses and three independent reviewers re-derived the same source
facts; none found a route, and this ruling found none either.

**Why not ABORT.** The founding decision
(`decisions/2026-09-01-demand-selected-semantic-operand-forcing.md`,
required outcome 1 — "a decided conditional forces only its selected
branch") is correct and reachable; only the authored-operand *mechanism*
for conditional branches is not. TE4 and TE5 consume the outcome through
this node. The candidate carries a landed, discriminatingly tested
correctness fix — a decided, distributed, or absorbed conditional no longer
roots on a branch its answer never read — whose negative controls were
proven to apply and whose absorbed arm was independently confirmed. Abandoning
the node would discard that fix and leave the lowering-site gap the founding
decision itself identified without an owner.

**The reading.** TE2-AC1's counter list is read as attributable to the
decided conditional's own evaluation at the dispatch boundary (the narrow
reading). Lowering the parameterized body once per declaration content is
the TE4 lower-once contract, not dead-operand work; the substitution descend
of an open shell and the eager lowering-site branch lowering are real dead
work and are carried to `TE2B` rather than waived. `TE2B` reaches that work
without authored branch operands: its admissible representation is a
content-free pairing of a materialized handle with its pending positional
substitution frame, which is exactly the runtime-node arm plus substitution
axis TE1 already seals, and the shape TE4 must reuse.

**Applied on this branch by the ruling.**

1. `charters/rev11-type-evaluation/TE2.md` is the rescoped contract. Its
   production files are `build.rs` and `absorb.rs` (the candidate touched
   both); its acceptance IDs bind to the existing discriminating guards by
   name and to the three declared mutation recipes.
2. `TE2B — Conditional branch forcing at the lowering and instantiation
   sites` is added to `authority/dag/rev11-type-evaluation.toml`
   (predecessors TE2, TE3; budgets 800/8/2 with 1500/12/3 rescope) with its
   charter. It owns lowering-site selection before branch lowering and
   instantiation-site deferral of losing-branch substitution, forbids
   selection during substitution, and forbids the authored-operand cutover,
   the conditional-`infer` frame on TE1 authored identity, the
   `verter_type_expr` vocabulary change, and a projection-context axis in
   `FamilyKey::Conditional`.
3. TE4's predecessors become TE2, TE2B, TE3 (DAG and charter); TE5's
   transitive-closure sentence names TE2B. TE5's edges are unchanged.
4. Ledger: `"TE2"` transitions to implemented against the rescoped charter;
   `"TE2B"` is predeclared pending. TE4 stays blocked on TE2B.

**Dropped with rationale, not carried.** The captured TE2-AC3 requirement
to fold the request's five `ProjectionReductionContext` axes into
`FamilyKey::Conditional` presupposed sealed branch operands forced under a
request context. Without them conditional reduction reads no projection
context — `build_conditional` consumes four nodes plus `distributive` and
the winner is projected under the consumer's own `ProjectPath` context — so
the axes would fragment one warm entry into duplicates with zero
correctness gain, against the charter's own zero-warm-growth budget. `TE2B`
forbids them unless a branch force becomes context-dependent inside the
family.

**Retained-finding dispositions.** The governance and ledger findings
(self-recorded rescope, unauthorized charter/DAG/ledger edits) are resolved
by this ruling: it is the workflow architect-approval artifact they required,
and the edits are re-applied under it. The captured-outcome and TE2-AC1–AC4
findings are judged against the captured charter; under the replacement
contract they are scope disagreements — the eager lowering and substitution
descend belong to `TE2B`, the five-axis identity is dropped as above, and the
authored-operand cutover is the TE1 gap below — and they remain open to
independent review against the replacement contract.

**Precedence.** This ruling supersedes the captured TE2 charter. The
founding decision's outcome 1 stands unchanged; only its conditional
mechanism is amended through TE2 and TE2B.

**Open, not waived.** The TE1 completeness gap — an authored
conditional-`infer` binder frame and locator vocabulary for every conditional
position — is recorded here and named in `TE2B`'s forbidden designs. No live
consumer needs an isolated authored binder-crossing force; the first node that
does records its own DAG amendment before mutation.

**What this record cannot do.** It does not approve landing. Independent
`semantic-3` review, the `targeted-domain` gate on the stable candidate, and
CI still gate the candidate against the replacement contract.

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

Ratified 2026-09-11 (see "Ratification" above): option 1 under the narrow
reading, with the lowering-site and instantiation-site scope carried to
`TE2B` rather than dropped and the five-axis family identity dropped with
rationale. The TE2 row is implemented against the rescoped charter; the
production source changes are the dependency-fact fixes under "Delivered".

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

*Answered by the ratification above: the narrow reading. `TE2B` owns the
lowering-site and instantiation-site work instead of a new TE1 predecessor,
and the five-axis fold into `FamilyKey::Conditional` is dropped with
rationale rather than landed.*

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
