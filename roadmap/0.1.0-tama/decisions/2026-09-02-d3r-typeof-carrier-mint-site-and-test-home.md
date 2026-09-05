# D3R typeof-carrier mint site and test-home amendment (rev11.flow)

- Status: accepted (ratified by the maintainer, 2026-09-05)
- Date: 2026-09-02 (rewritten 2026-09-04 to describe the landed candidate)
- Amends: `charters/rev11-flow/D3R.md` production-file list, test-home section, and the
  literal form of one required guard
- Scope: D3R only; no other node's charter, budget, or ledger changes
- Landing: D3R lands as the first member of the stacked D3 chain (D3R -> D3I -> D3P ->
  D3C) in one squash of D3C; no standalone merge.

This document was rewritten in place rather than extended with a fifth addendum. The
earlier text described a broader design — nominal identity for class-static `unique
symbol` members — that has since been WITHDRAWN and deleted from the candidate, so
carrying its rationale forward would have left a decision record describing code that no
longer exists. What that design was, and why it was withdrawn, is recorded below under
"Withdrawn: class-static member identity"; the branch history holds its implementation.

## Context

D3R's charter names its production surface as `crates/verter_session/src` ONLY, and
within it the files `semantic_query.rs`, `project_semantic_dispatch/{relation.rs,
relation_predicates.rs, flow_return.rs}`, plus `lower.rs` "only if carrier preservation is
required". Test homes are `crates/verter_semantic/tests` and
`crates/verter_session/tests/cases`.

Making `RelationKind::Identity` live on the nominal axis requires the nominal identity of
a `unique symbol` declaration to SURVIVE from the declaration to the relation. It cannot
survive as a lowered annotation: `unique symbol` widens to the bare `symbol` primitive,
which is interned ONCE per graph, so lowering hands every consumer the same node for
`typeof A_KIND` and `typeof B_KIND` before any relation can tell them apart. The identity
has to ride a node that is not the shared primitive.

## Decision

1. **The nominal identity rides the `typeof` CARRIER, minted at ONE site.**
   `ProjectSemanticDispatch::build_typeof` — the `TypeOf` query's builder — answers a
   `unique symbol` DECLARATION ROOT with a `TypeOf` carrier carrying the declaring
   `ValueDeclIdentityPart`, built from the prepared facts that builder already read. It
   is the SOLE producer of a marked carrier. Every typeof-resolving consumer reaches a
   carrier head through that same query key, so a declaration root converges here however
   it was reached, and no consumer mints a second marked node from a projected value.
   `semantic_query/carrier.rs` (the carrier payload and its two private accessors) and
   `build.rs` (the mint) are production files outside the charter's named list required by
   this rule.

2. **The carrier's HEAD is the AUTHORED reference; the identity rides the PAYLOAD.**
   Rebuilding the head from the declaring identity would make `import { TOKEN as T }`
   publish and display `typeof TOKEN` — a symbol not in scope in the consuming file — and
   would re-key a namespace-declared symbol into a scope where its bare name does not
   resolve. Nominal equality is decided by the payload, so two spellings intern as two
   nodes the relation answers EQUAL.

3. **A marked carrier is a CONCRETE terminal, and every consumer of it classifies.**
   Consumers asking a NOMINAL question (the relation's identity unwrap, the deferred
   evaluator, the locator-view planner, the carrier normalizer, the raising reducer) read
   the marker in O(1) and stop; resolving the carrier again would project the annotation
   back down to the shared `symbol`. Consumers asking a STRUCTURAL question take the
   widened `symbol` inhabitant through the ONE shared helper `widened_nominal_typeof`,
   because their `resolved == node` rail means "this reference did not resolve" and a
   marked carrier self-resolves. Reading a terminal through that rail turns a program that
   resolves fine into a missing dependency: a `unique symbol`-typed prop lost its `Symbol`
   runtime constructor (and, because an unclassifiable arm clears the WHOLE ordered
   constructor list, its siblings' entries with it), a `unique symbol`-typed member
   published an `Unresolved { MissingDependency }` callable role, and an
   `export const TOKEN: unique symbol` in a Svelte module script projected as the circular
   `export declare const TOKEN: typeof TOKEN` that the checker rejects outright. The
   structural sites are `broad_runtime.rs`, `symbol_identity.rs`, `walk.rs`,
   `typeinfo/vue_macro_codegen/runtime.rs`, and
   `typeinfo/framework_surface/svelte_exec.rs`; the complementary half of the Svelte fix is
   `resolver_core/component_meta_registry.rs`, whose reference collector skipped `typeof`
   carriers entirely, so a legitimately rendered `typeof EXT_TOKEN` named an undeclared
   identifier in the generated surface. Those six, plus the nominal-question sites
   `evaluate.rs`, `locator_view_worklist.rs` + `locator_view_worklist/finish.rs`,
   `carrier.rs` and `raise.rs`, are production files outside the charter's named list
   required by this rule.

4. **`lower.rs`'s duplicate nominal-identity resolver is deleted.** The
   `unique_symbol_identity_for_typeof` helper (a second bare-name/one-segment lookup plus a
   two-segment `format!`-joined namespace lookup) is replaced by
   `lower_typeof_for_authored_key`, which calls the SAME
   `lower_type_expr_with_infer_factory` every other `TypeExpr` lowers through and recovers
   the identity from the produced node's marker. An authored `typeof` property key or index
   key is therefore certified from the node the shared lowering produced, never from a
   private path parser that could truncate a qualified reference or certify a shadowed
   namespace prefix.

5. **The relation's nominal widen step interns the `symbol` primitive directly.** Re-asking
   the typeof key would return the carrier, so the widened inhabitant is minted structurally
   with no second resolution pass.

6. **The nominal widen preserves composite and inference frames, symmetrically.** A nominal
   source is not widened before a union or intersection TARGET distributes, and a union or
   intersection SOURCE is likewise distributed before a nominal target is judged — the
   mirror arm mattered: without it `Comparable(typeof A | typeof B, typeof C)` answered
   Overlaps one way and Disjoint the other, from an oracle whose contract is a symmetric
   overlap question feeding a consumer that publishes the intersection as a COMPLETE warm
   narrow. Deferred sources remain undecided and `Infer` reaches its deposit arm.
   Assignability additionally declines for an `Infer` position on either side so a covariant
   deposit cannot receive the widened `symbol`. The gate ORDER of `expand_pair` is otherwise
   unchanged: a pair steps over the deferred gate ONLY when the nominal leaf declined a pair
   that actually carries a nominal identity, and the canonical frame opens for a declaration
   carrier ONLY when the pair's other side is nominal. An earlier candidate hoisted all four
   distribution arms above the deferred gate and widened
   `relation_eval_requires_canonical_frame` to every declaration-ish carrier — a general
   broadening of the most load-bearing engine in the type system inside a bounded
   nominal-axis node. That is withdrawn.

7. **`Comparable` is one bounded iterative worklist, and it refuses the unread
   operand — unresolved AND unreduced alike.** Pair descent, union alternatives,
   and shared required-member comparisons consume one budget; an active-pair
   set terminates cycles and a local memo avoids repeating completed pairs.
   `Intersection` and `ObjectSpreadProgram` roots compose through the same
   shared empty-path `Shallow` surface reader the displaced classifier used,
   before the member descent — a composition that yields no surface stays
   permissive, so no proof is ever minted from a surface the oracle could not
   read. An UNRESOLVABLE operand (`Opaque` / `InferRef` / a bare, imported, or
   declaration REFERENCE that survived the identity unwrap) AND an UNREDUCED
   OPERATOR (`typeof` shell, `keyof`, indexed access, mapped, conditional, raw
   fallback) both stay UNDECIDED — `Unknown`, `ReturnOnly`, zero admission: the
   permissive arm is a promise that no proof of empty overlap exists, and an
   operand whose content was never read cannot back that promise whether the
   content is absent or merely not yet expanded. (The unreduced-operator half
   is a deliberate warmth regression against the displaced classifier, which
   answered those pairs complete and warm; recorded here because an earlier
   draft of this point described the permissive arm instead.) The negative arm
   is unreachable from either branch, so no disjointness proof is ever minted
   from an unread operand.

8. **`reduce_identity` / `reduce_comparable` refuse the axes they ignore.** Both read only
   `source` and `target`, while the memo key also carries substitution, inference-context,
   and freshness axes. A key varying one of those is refused (undecided, `ReturnOnly`)
   rather than answered by a value that never consulted it — the same axis-refusal
   discipline the reducer already applies to an unimplemented relation kind.

9. **The two comparators stay two descents, deliberately.** Assignability and comparability
   agree on nothing below the root — a union SOURCE conjoins for one and disjoins for the
   other; an extra source member is an excess-property question for one and irrelevant to
   the other — so folding them into one walker would mean branching on the relation kind at
   every hop: a second engine wearing one name. What they share is the single
   proven-disjoint tag oracle and the single nominal leaf, which is where drift would
   actually be a defect. The divergence and its consequence (object-descent extensions must
   be decided per descent, not assumed to propagate) are recorded in the module doc.

10. **Tests stay with the owning harness.** Nominal relation, qualified-reference,
    component-publication, generated-surface, and incremental-equivalence contracts live in
    `crates/verter_session/tests/cases/relation_nominal_authority.rs`. Flow-gap,
    classifier-ownership, call-resolution, readable-composite, and computed-key contracts
    extend the existing `crates/verter_session/src/flow_gap_retraction_tests.rs` harness
    instead of duplicating its cold/warm/partial assertion machinery in the integration
    suite. `src/*_tests.rs` is a sanctioned Rust test home in this repository, but it is not
    one of the two the charter names — recorded as a deviation.

11. **The per-kind relation counter is test-only.**
    `SemanticGraphStore::relation_memo_count_of_kind` is
    `#[cfg(any(test, feature = "test-support"))]`-gated; production code is byte-identical
    to its pre-D3R shape. Integration tests reach the needed internals through the
    crate-internal `for_tests` shim module (compile-absent in production profiles), plus
    three `pub(super)` → `pub(crate)` visibility bumps inside `relation.rs`
    (`relation_nominal_identity`, `unwrap_identity_carrier_for_relation`,
    `IdentityCarrierUnwrap`) — crate-internal only, no public-API change.

12. **The classifier-ownership guard is behavioral + type-confined, not a scanner.** Landed
    guards are structural, never name-keyed source scanners, so the charter's literal
    "structural guard: no `NodeDisjointness` / `nodes_provably_disjoint` / disjointness
    table remains in `flow_return.rs`" cannot land as written.
    `flow_has_no_private_relation_classifier` instead holds it by (a) the disjointness proof
    value's module-private field — no consumer module can construct the verdict a narrow
    consumes — and (b) a behavioural assertion that the representative narrow populates the
    `Comparable` memo family and issues no `Identity` judgement, plus the pinned
    authority-derived result (two distinct `unique symbol` discriminants have equal
    primitive tags, so a private tag-reading classifier would publish an intersected object
    where the authority publishes the checker's `never` collapse). The private field is a
    supporting constraint, not the enforcement: a re-introduced private classifier would
    branch on a bare `bool` and never construct the type. The behavioural assertion is the
    primary rail.

13. **One charter-named test landed under a different name.** The charter names
    `flow_g3_nominal_relation_gap_retracts_only_when_decided`; it landed as
    `nominal_relation_gap_retracts_only_when_decided` in the flow-gap harness. `g3` is a
    charter-side label for the gap-catalog row, not a live in-tree identifier, and importing
    it into a test name would be roadmap archaeology in a test artifact. The contract is
    unchanged: the unique-symbol discriminant fixture reaches the exact checker-compatible
    value and is warm on the second request, and an unresolved-symbol control stays
    partial/cold.

## Withdrawn: class-static member identity — RE-INTRODUCED in the round-3 fix round

An earlier round of this candidate extended nominal identity to class-static members;
a later round WITHDREW it (original reasons kept at the end of this section); the
round-2 review rejected the withdrawal as an unratified deferral (two P1 findings:
distinct `typeof C.A` / `typeof C.B`, object members, and inherited statics aliasing
to one `symbol` type violates the correctness budget's zero identity-aliasing line),
and the fix round re-introduced member identity in a smaller shape than the withdrawn
attempt:

- ONE member-level fact: `ValueTypeAnnotationFact.unique_symbol_members` (a
  `#[serde(default)]` name list) — not a per-member bit on `ObjectPropertyFact`.
- Producers sit where the authored AST still distinguishes `unique symbol`
  (`type_eval_build.rs`: class statics; object-type-literal annotation members) — no
  member fact is derived from erased `TypeExpr` content.
- ONE shared rail (`member_nominal_typeof`) consulted by every typeof resolution arm
  (lowering, evaluator, walker, raising) BEFORE the generic member projection — no
  unmarked-carrier promotion replicated per consumer, so the mint stays singular (all
  member carriers intern through `intern_nominal_typeof`).
- Inherited statics name the DECLARING base class (the rail chases the heritage
  chain); the identity is the declaring anchor plus `member_path`, which nothing
  populated before this round.
- A member whose annotation is not authored `unique symbol` still widens (pinned
  control in `value_member_unique_symbols_carry_their_own_nominal_identity`).

This keeps the candidate outside `crates/verter_session/src` (the fact field lives in
`verter_type_expr`, the producers in `verter_semantic`) — two RELATED packages, inside
the charter's 2-package planning reference, but the production-surface deviation the
ratification section describes is now CROSS-CRATE and is part of the ratification ask:
ratify it as the member-identity surface the review demanded, or the surface must
shrink again.

Original withdrawal reasons, kept for the record:

- It was the only thing that took the candidate outside `crates/verter_session/src`, which
  the charter states as the whole production surface. It made the candidate span three
  crates, which fires the charter's MANDATORY architect-rescope trigger.
- It is required by NONE of the charter's five named discriminating tests (the flow-gap
  discriminant fixture uses `declare const A_KIND: unique symbol`), and the charter's
  outcome statement scopes preservation to "aliases, imports, and re-exports" — class
  statics are never named. The charter forbids implementing successors or silently
  enlarging itself: "discovery of a second independently acceptable outcome requires an
  amendment and a new DAG node before mutation".
- It brought a second identity-production path — a declaration CHASE re-resolving a root the
  `TypeOf` query had already resolved — and a promotion that had to be replicated at every
  resolving consumer. Three successive review rounds each found another site that had not
  been given it. Deleting it makes the mint singular by construction.

## Recorded bounds and open items

- **Nominal identity covers declaration roots AND recorded members.** A
  `unique symbol` class static or object-annotation member is its own nominal type
  (`typeof Tokens.A`, `typeof CONFIG.K`, inherited `typeof Derived.A` naming the
  declaring base); every other member shape (a non-`unique` annotation, a deeper
  projection) widens to the shared `symbol` primitive. Namespace-qualified references
  (`typeof Ns.KIND`) resolve their own joined root through the same query.
- **A member certified through a NAMED TYPE annotation anchors on the TYPE
  declaration.** `a: I; b: I` where `I` declares the member: `typeof a.K` and
  `typeof b.K` are ONE nominal type (`I` + member), because tsc unifies them
  through the single member type — value-anchored identities would prove legal
  code disjoint. Value anchoring applies only to INLINE annotation members and
  class statics, where the annotated value IS the declaring site. The heritage
  chase stops at any class that DECLARES the member itself (a non-nominal
  override never inherits the base's nominal identity), and only READONLY
  member annotations certify — tsc widens a mutable `K: unique symbol` member
  and rejects every `as unique symbol` assertion spelling (TS1335), so neither
  ill-typed shape mints identity.
- **`RelationKind::Identity` is live and warm-admittable with NO production asker yet**, and
  carries a surprising bound: `Identity(X, X)` is UNDECIDED for any non-nominal `X`, because
  the relation is bounded to the nominal axis and scope-insensitive structural constituent
  identity belongs to the canonical type algebra's own comparator.
- **The disjointness PROOF carries the checker's intersection-collapse class.** The
  oracle still proves empty overlap for a conflicting shared required member at any
  depth, but `DisjointnessProof::checker_reduces_intersection_to_never` — minted only
  inside the relation module — answers whether the checker would reduce the
  intersection: disjoint tags, distinct nominal identities, or a conflicting shared
  REQUIRED member whose values are both unit types reduce to `never`; a conflict
  reachable only through non-unit member values KEEPS `A & B`, exactly as `tsc`'s
  `getNarrowedType` keeps `{ m: { b: number } } & { m: { b: string } }`. The flow
  consumer reads the class off the authority-minted proof and decides nothing about
  collapse itself. Whether an intersection collapses remains the canonical algebra's
  (TA1A's) policy; the class function is the interim checker-compatible boundary the
  round-2 review required and is expected to move behind a TA1A-owned proof category
  when that authority lands.
- **Unreduced operators are UNDECIDED, not permissive.** An operand the reduction did
  not itself expand (`typeof` shell, `keyof`, indexed access, mapped, conditional,
  raw fallback) reports NO fact: `Unknown`, `ReturnOnly`, zero admission. The
  permissive arm is a promise about proofs not found, and an unread operand cannot
  back it.
- **Comparability's single-frame descent carries a coinductive assumption** where
  assignability re-enters the full authority: a re-entered pair is assumed to OVERLAP. The
  direction is safe — both folds propagate permissiveness, so an assumption can only miss a
  proof — and only the ROOT pair is published to the shared memo, so no assumed sub-result
  escapes the frame that made the assumption. Recorded rather than re-engineered: a guard
  suppressing tainted memo writes was written and then removed, because no reachable shape
  distinguishes it and unexercised precision reads as enforcement that is not there.
- **A namespace-qualified VALUE read on the flow channel** (`return { v: Ns.K }`, including
  non-symbol values) leaves the flow-return result complete but COLD on the second request.
  It predates the nominal work (it reproduces with plain symbols and literals) and is not
  addressed here; the affected test leg records the reason inline.
- **RESOLVED (round 3): the nominal terminal is compiler-fenced.** The nominal carrier
  is its own `SemanticNodeData::TypeOfNominal` variant, so every exhaustive match over
  the enum must classify the terminal; substitution cannot rebuild it with type
  arguments; the structural hashes (audit footprint, cycle guard, canonical algebra)
  hash the declaring identity; and the symbolic-equivalence proof compares identity,
  not just the authored head.

## Measured surface

59 production files, +2876 / -652 lines, FOUR crates, no `packages/**` change. (Measured
over `crates/*/src/**` excluding `*_tests.rs`, `for_tests.rs`, and the `typeinfo_tests`
fixture tree, branch tip against the merge base.) Per crate:

- `verter_session`: 51 files, +2682 / -641.
- `verter_semantic`: 4 files, +178 / -8 (the `unique_symbol_members` producers and their
  fact projection).
- `verter_type_expr`: 2 files, +13 / -0 (the fact field and its witnesses).
- `verter_protocol`: 2 files, +3 / -3 (the consumer-manifest pin for the schema bump).

The charter's crate-level production surface — `crates/verter_session/src` only — is NOT
met: the member-identity fact lives in `verter_type_expr` and is produced in
`verter_semantic`, and the schema bump the new gated field requires pins one manifest
constant in `verter_protocol`. The 2-package planning reference is crossed; the 3-package
mandatory-rescope trigger is crossed too, though two of the three extra crates carry only
the fact definition, its producers, and the version pin — not a second resolution path.
Both numeric rescope signals are crossed (59 files > 12; +2876 > 1500), so this section IS
the scope-coherence investigation the sizing contract requires. The coherence explanation
is one subject with one fan-out: `relation.rs` (+1258) is the node's own named subject —
the two new reducers plus the nominal leaf — and the fan-out is the cost of introducing a
NOMINAL terminal into a structural graph. Each consumer file changes by 1–75 lines and
does exactly one thing: classify the new terminal, either as terminal (nominal question)
or as its widened inhabitant (structural question). None of them adds a resolution path,
and five of them are reproduced-regression fixes rather than new capability. Collapsing
that fan-out is precisely the compiler-fence follow-up recorded above; it cannot be done
by writing less code here.

## What ratification is still owed

Four items:

1. The production-file list (decision points 1, 3, 4): 46 files beyond the
   charter's five named ones inside `crates/verter_session/src`, plus the
   cross-crate surface of item 4 (measured in the section above).
2. The test home (decision point 10): flow-channel contracts extend
   `src/flow_gap_retraction_tests.rs` instead of moving to `tests/cases`.
3. The guard form (decision point 12): the charter's literal source-scanner form is
   forbidden by the repository's landed-guards rule (CLAUDE.md: landed guards are
   structural, never name-keyed file scanners) and is replaced by a behavioural +
   type-confinement guard — the disjointness verdict and its collapse class are
   mintable only inside the relation module.
4. The round-3 member-identity surface: `ValueTypeAnnotationFact.unique_symbol_members`
   (`verter_type_expr`) plus its producers (`verter_semantic`), re-introduced on the
   round-2 review's demand, outside the charter's `crates/verter_session/src`-only
   production surface — together with the `CACHE_CLUSTER_SCHEMA_VERSION` bump the new
   gated field requires (`cache_schema.rs` and the one pinned constant in
   `verter_protocol`'s consumer manifest).

Ratified by the maintainer on 2026-09-05: the production-file list (item 1, including the
cross-crate surface it measures for item 4), the test home (item 2), and the guard form
(item 3) are accepted as the amended D3R charter surface. Nothing above remains owed.
