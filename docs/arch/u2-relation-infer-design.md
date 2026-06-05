# U2.RELATION_INFER — Relation-cache substrate + coinductive dispatch (LOCKED design)

> Block: `U2.RELATION_INFER` (`docs/arch/semantic-db-overhaul-unified-remaining-plan.md` §1583–1708, the
> `U2.RELATION_INFER` gate). Design gate deliverable — design #1 (relation-caching category) + design #2
> (coinductive cycle discharge) together, as ONE coupled problem, plus the inference-session admission
> tightening (#3), relation-proof acceptance (#5/A.9), and the second-engine forbiddances.
>
> Parent architecture (LOCKED shapes this design builds OVER, never redesigns):
> `docs/arch/native-typeinfo-parity.md` §2.7 (the full `Relate` key identity), §4.1 (the coinductive-SCC
> assumption protocol), §4.2 (`CheckerTransaction` / `InferenceSession`), §6 (`RelationBudget` /
> non-admission). `docs/arch/u2-query-value-domain-design.md` §2.2 (the `Relate` row: value domain
> `Relation(RelationPayload)`, env dims `R T L J`, **plus** the content-free `InferenceSession` projection
> SHAPE — both live at qvd §2.2; the `CheckerTransaction` / `InferenceSession` substrate is the parent
> `native-typeinfo-parity.md §4.2` cited above, not a qvd §4.2).
>
> Status: **DESIGN-LOCKED.** Adjudicated by a two-panelist design panel (codex gpt-5.5/xhigh + a claude
> reviewer), reconciled by an independent adjudicator. Panel artifacts: `/tmp/mom/RELATION-INFER/PANEL.md`,
> `panel-codex.txt`, `panel-claude.md`, `ADJUDICATION.md`. A subsequent max-mandate confirm-gate (codex + an
> independent claude reviewer) returned precision findings (P1-A non-cyclic `Unknown` admission row; P1-B
> in-flight binding-`Relate` reentry identity; P1-C transient `RelationComputeResult`; P1-E negative-SCC fact
> set; P2-D the live `GraphTypeNode.relation_proof = 28` wire migration into RI-2; P3-F singleflight lifetime;
> P3-G mini-DAG art) which are folded in below. A SECOND max-mandate confirm-gate (round 2) then returned two
> REAL findings — **P1-2** (`BudgetExceeded` restored to the PUBLIC `RelationPayload` / `display_relation`
> surface as a `ReturnOnly`-but-public outcome via `RelationOutcome::BudgetExceeded`; warm admission stays
> binary `Assignable`/`NotAssignable` at the gate, never by deleting the variant) and **P1-3** (RI-8 rescoped
> to the U2-available substrate: build the shared `CheckerReentryStack` + retire `RefCycleResultDb` + wire only
> `Relate`; defer `FlowReturn`/`ResolveCall`/`ContextualTypeAt`/`FlowNarrowingAt` routing AND the flow
> depth-sentinel retirement to U6, dropping the RI-6 dep) plus two P3 wording polishes (§2.4 §4.1-protocol
> scope + R-c cross-ref; mini-DAG art) — all folded in below — precision/sequencing fixes layered on the locked
> core, NOT a redesign; no invariant weakened. A THIRD max-mandate confirm-gate (round 3) then returned one
> P1 + two P2 (all VERIFIED real) — folded in below: **P1-RI3** (the RI-3 guard
> `checker_reentry_graph_spans_flow_call_contextual_narrowing` named the U6-deferred
> `FlowReturn`/`ResolveCall`/`ContextualTypeAt`/`FlowNarrowingAt` variants — NOT pre-registered at U2, and the
> standing `semantic_query_key_spec_table_equals_enum` meta-guard rejects any U2 tree referencing them, so it
> could not land TDD-first at U2; SAME class as P1-3 — rescoped RI-3's U2 guard to the U2-live
> `checker_reentry_stack_substrate_built_and_relate_wired` and DEFERRED the full cross-engine span assertion to
> a **U6-owned** `checker_reentry_graph_spans_flow_call_contextual_narrowing`), **P2-Dec4-guard** (the parent
> value-domain guard `relate_query_value_carries_relation_proof_and_budget_state` was neither NAMED nor OWNED
> here, and round-2's fold dropped the literal `budget_state` field — reconciled by keeping the folded
> `RelationOutcome::BudgetExceeded` form: the public `RelationPayload` carries BOTH the `relation_proof` AND the
> budget state typed INTO the outcome arm, satisfying the guard in intent and matching the parent's own prose
> ["the public `RelationPayload` … plus a typed `BudgetExceeded` non-admission", native-typeinfo-parity.md:1057]
> + qvd three-valued `display_relation`; OWNERSHIP assigned — U2.QUERY_VALUE_DOMAIN owns the guard over the
> value-domain shape, RI-2 exercises/satisfies it), and **P2-remap** (the positive-SCC `CoinductiveCycle { keys }`
> proof's `keys` set is now stated to be remapped from in-flight transient `SessionId` identities to completed
> full §2.7 `Relate` keys BEFORE any durable proof construction/publish — a durably-published proof never carries
> a transient `SessionId`) — precision/consistency fixes layered on the locked core, no invariant weakened. A FOURTH
> max-mandate confirm-gate (round 4) then returned three REAL-MUST-FIX findings (all folded in below): **P1**
> (§2.3 step 4 published binding-`Relate` SCC members at SCC-close on the false premise that their enclosing
> session is `CompletedDeterministic` by then — SCC-close and inference-session completion are two INDEPENDENT
> convergence axes, so a binding member's relation SCC can close while its session is still `InProgress`; the
> cure is a per-session **`SessionAdmissionLedger`** that DEFERS binding-member admission, proof-key remap, and
> `CoinductiveCycle` slot-fill to session-close, dropping the member to `ReturnOnly` if the session ends
> `Abandoned`), **P2-pred** (Decision 3 / §3.3 now makes the admission predicate for a binding SCC member an
> explicit conjunction gated on the LATER event — `admit(Kᵢ) ⇔ (SCC closed POSITIVE ∨ publishable-NEGATIVE) ∧
> (session == CompletedDeterministic)` — and pins the session-local *delta* (always `ReturnOnly`, row 7) and the
> binding member's *final re-keyed `RelationPayload`* (admitted at session-close) as DISTINCT objects), and
> **P2-RI8** (the mini-DAG had RI-3 AND RI-8 both "build … and wire `Relate`" onto the shared substrate — a
> forked-second-substrate hazard; RI-3 is now the SOLE builder + `Relate`-wirer and RI-8 is reworded
> CONSUME-ONLY: reuse the RI-3 substrate + retire `RefCycleResultDb`). The new `SessionAdmissionLedger` is
> assigned to RI-6 (session substrate owner), populated by RI-4's `SccLedger` at SCC-close, drained at
> session-close by RI-6, pinned by the deferred guard `binding_relate_scc_member_admits_only_at_session_close`
> (RI-6) — precision/sequencing fixes layered on the locked core, no invariant weakened. A FIFTH max-mandate
> confirm-gate (round 5) returned a CONVERGENT send-back (codex + claude, no disagreement) — all four REAL,
> all folded in below: **P1-mixed-SCC** (the round-4 `SessionAdmissionLedger` deferral did NOT propagate to a
> deeper facet of the same defect — in a MIXED positive SCC (≥1 binding + ≥1 non-binding member) every member's
> published payload carries the SHARED `CoinductiveCycle { keys: S }` proof, and `S` references the binding
> members whose slots are UNFILLED until session-close, so a POSITIVE non-binding member ALSO cannot publish at
> SCC-close; the publish/singleflight gate is now made to depend on SCC COMPOSITION not member class alone — a
> pure non-binding SCC publishes at SCC-close, a mixed SCC defers its WHOLE positive batch (binding AND
> non-binding) to the LATER of {SCC-close, last binding member's session-close}, while a NEGATIVE non-binding
> member (`NotAssignable`, no `keys: S`) still publishes at SCC-close even in a mixed SCC (round 7 NARROWS this
> last clause — see the round-7 note below — to a NEGATIVE non-binding member whose transitive consumed-verdict
> closure contains no binding member); applied to §2.3
> step 3/4, the §2.3 header, §3.3, admission row 14, residual risk 6, and the §0/obligation-1/coupling
> summaries), **P2-EXACT-header** (the §2.3 "EXACT" rule header overclaimed SCC-close as the admission instant
> — reworded: SCC-close is the admission instant only for a pure non-binding SCC, and is necessary-not-sufficient
> when any binding member participates), **P2-binding-singleflight** (a binding member has NO joinable
> cross-transaction mid-flight singleflight key — its per-transaction transient `SessionId` is private and its
> final §2.7 key does not exist until session-close; corrected to: cross-transaction sharing happens only on the
> FINAL published slot, a separate transaction recomputes deterministically in its own session, recorded as a
> bounded PERF residual in risk 6), and **P2-RI3-builder** (a stale §7/Rescope + residual-risk-5 line still read
> "substrate RI-8 builds" — reworded to "RI-3 builds (RI-8 reuses; RI-8 wires no `Relate`)" consistent with the
> RI-3/RI-8 table rows + §2.4) — precision/consistency fixes layered on the locked core, no invariant weakened.
> Subsequent max-mandate confirm-gate rounds (6 → 7) tightened the binding/SCC admission area further. The round-7
> gate (codex + an independent claude reviewer) returned three REAL findings + one P3 — all folded in below:
> **P1-neg-verdict** (the round-5/6 NEGATIVE non-binding mixed-SCC carve-out was sound only on the IDENTITY-leak
> axis — a `NotAssignable { reason, failing_sub }` proof carries no `keys: S` — but UNSOUND on the
> VERDICT-DEPENDENCY axis: a negative non-binding member whose verdict transitively CONSUMED a binding sibling's
> not-yet-converged verdict warm-published a negative that the content-fact rail cannot invalidate when that
> binding session later FLIPs to POSITIVE or `Abandon`s (inference convergence is not a content edit). The cure
> GENERALIZES the deferral predicate — a NEGATIVE non-binding member publishes at SCC-close ONLY when its transitive
> consumed-verdict closure contains NO binding member; otherwise it rides the SAME deferred batch as the positive
> members to the LATER of {SCC-close, that binding sibling's session-close}, dropping to `ReturnOnly` on `Abandon`
> — superseding the unconditional round-5/6 negative carve-out at line ~69, applied to §0/obligation-1/§2.3
> step 3/§2.3 step 4/§3.3/admission rows 13–14/risk 6), **P2-Abandon-exit** (the deferred batch's two exits were
> under-specified — the `Abandon` branch now spells out RELEASE-WITHOUT-PUBLISH: the held singleflight registration
> is released with no entry / no fact signature / no backfill / no reverse-index metadata, and any concurrent
> joiner blocked on it then RECOMPUTES since it cannot validate an entry that will never exist — added to §2.3
> step 4 and admission rows 13/14), and **P2-RI8-wires** (a stale "RI-8 wires only `Relate`" at §Rescope
> contradicted the RI-3-sole-builder reframe — reworded to "RI-3 wires only `Relate`; RI-8 wires nothing onto the
> substrate"), plus **P3-failing-sub** (`SubRelationRef` / `failing_sub` pinned content-free — a `(source-node,
> target-node, sub-position)` descriptor that EXCLUDES any session-bearing full `Relate` key, so a published
> `NotAssignable` never leaks a transient `SessionId`; Decision 4 proof table) — precision/consistency fixes
> layered on the locked core, no invariant weakened. A round-8 confirm-gate (codex CONFIRM-LAND-BEST; an
> independent claude reviewer SEND-BACK) returned one REAL P2 + one P3 — both folded in below: **P2-drain-signflip**
> (the §2.3 step-4 deferred-batch session-close drain handled only TWO outcomes — `CompletedDeterministic` ⇒
> publish-recorded-verdict, `Abandon` ⇒ release-without-publish — and so lacked a sound exit for the
> **converged-but-SIGN-FLIPPED** sub-case: a binding sibling's SCC-close verdict is PROVISIONAL and can FLIP sign
> before its session's `CompletedDeterministic` (parent §4.2:1413–1415 re-measures across fixation iterations), so
> publishing the dependent member's *recorded* verdict at session-close re-introduces the stale-verdict defect —
> a stale false-NEGATIVE for a binding-consuming negative member whose sibling flipped to `Assignable`, AND a
> stale false-POSITIVE for a held positive non-binding member whose binding sibling flipped to `NotAssignable` —
> relocated from SCC-close to session-close; the same fold also carried a proof-category mismatch where the
> unified deferred-members paragraph applied a `CoinductiveCycle` slot-fill + "now-complete proof" to deferred
> NEGATIVE members, which carry a slotless `NotAssignable { reason, failing_sub }` proof with no `keys: S` slot to
> fill — contradicting the already-correct §2.3 step-3 negative-close. The cure makes the drain a THREE-outcome
> gate stated as a VALUE re-evaluation — (1) `CompletedDeterministic` AND every consumed binding-sibling verdict
> converged to the SAME sign held at SCC-close ⇒ publish (positive members get the proof-key remap +
> `CoinductiveCycle` slot-fill, binding-consuming negative members publish their slotless `NotAssignable` with NO
> slot-fill); (2) `CompletedDeterministic` BUT a consumed sibling verdict FLIPPED sign ⇒ release-without-publish →
> `ReturnOnly` → joiners recompute; (3) `Abandon` ⇒ release-without-publish → recompute — applied to §2.3 step-3/
> step-4/the coupling paragraph/§3.3/admission rows 13–14/risk 6/the RI-4 + RI-6 mini-DAG rows + Decision 4
> proof bullet), plus **P3-bold** (a malformed overlapping `**…**…**` bold span in the coupling paragraph
> rendered "and verdict-dependency" un-bold — collapsed to a single bold run) — precision/consistency fixes
> layered on the locked core, no invariant weakened. A round-9 confirm-gate (codex + an independent claude
> reviewer, CONVERGENT SEND-BACK) returned one REAL P2 + one P3 — both folded in below: **P2-converged-re-discharge**
> (round 8's drain re-confirmed only consumed-sibling SIGNS — necessary but NOT sufficient: two escape cases still
> published a stale SCC-close snapshot — (a) a deferred BINDING member's OWN verdict can flip at its session's
> `CompletedDeterministic` with NO consumed sibling, so the sibling-sign check never fires; and (b) even on an
> unchanged sign the converged `bindings`/`relation_proof` may differ from the snapshot, shipping stale
> bindings/proof. The cure is the DEFINITIVE general close of the deferred-publish staleness class: the SCC-close
> verdict/bindings/proof recorded for ANY deferred member is **PROVISIONAL** — a caller-return value + deferral
> metadata, NEVER the published payload; at the batched-publish instant (the LATER of all relevant sessions'
> closes) the member's cold compute COMPLETES by RE-DISCHARGING the SCC against the fully-converged state through
> the same `execute(Relate{K})` dispatch — the one engine finishing once its session inputs are final, NOT a
> second engine — and the published `RelationPayload` IS that re-discharge result, so a stale snapshot is
> impossible by construction. The consumed-sibling "same-sign" check becomes a NAMED SPECIAL CASE of "the
> re-discharge yields a stable publishable outcome." Also fixed the §3.3 R-c FORMAL biconditional (~L720), which
> still asserted the two-conjunct `admit ⇔ (SCC closed) ∧ (session complete)` and was literally FALSIFIED by the
> round-8 sign-flip case — a third conjunct "yields a STABLE determined publishable outcome" was added and the
> "must require BOTH" framing changed to "must require all of: SCC-close, session-completion, AND a stable
> converged re-discharge"; applied to §2.3 step-3/step-4/the coupling paragraph/§3.3/admission rows 13–14/risk 6/
> the RI-4 + RI-6 mini-DAG rows + Decision 4 proof bullet, keeping the proof-category split intact (positive ⇒
> `CoinductiveCycle` slot-fill; negative ⇒ slotless `NotAssignable`)), plus **P3-qvd-ref** (the header cited a
> non-existent `qvd §4.2` for the content-free `InferenceSession` projection shape — that shape lives at qvd
> §2.2; corrected at the header line and the two other `§2.2/§4.2` value-domain-shape citations, leaving the
> correct parent `native-typeinfo-parity.md §4.2` references untouched) — precision/consistency fixes layered on
> the locked core, no invariant weakened.
> This is a PLAN block: it locks the design and the
> implementation mini-DAG; it does NOT build the relation engine. Implementation is sequenced AFTER the U2
> value-domain spine via the RI-1..RI-10 sub-blocks below.

---

## 0. Scope and the one-sentence architecture

`Relate` is a **PERSISTENT, cross-request, fact-validated query-identity cache** keyed by the full §2.7
identity; the in-flight comparison/assumption stack and the mutable inference session are **TRANSIENT**
per-`CheckerTransaction` state that is **NEVER** a cache key; recursion terminates through a **coinductive
SCC** that publishes `Assignable + CoinductiveCycle` on a clean positive close, a publishable
`NotAssignable` on a negative non-assumptive obligation, and `ReturnOnly` on any `Unknown`/cancel/budget
edge; admission is **batched at SCC-close** for a pure non-binding SCC (and for a NEGATIVE non-binding
member of a mixed SCC whose transitive consumed-verdict closure contains no binding member), while a binding
member — any POSITIVE non-binding member of a MIXED SCC (which carries the same binding-referencing
`CoinductiveCycle` proof), and any NEGATIVE non-binding member whose consumed-verdict closure DOES reach a
binding sibling — is **deferred to the relevant enclosing session's close** through a per-session
`SessionAdmissionLedger` (SCC-close and session-completion are two
independent convergence axes — a binding member's relation SCC can close while its session is still
`InProgress`); only a `CompletedDeterministic` inference session admits its
final typed result; and the relation derivation rides a payload-side `relation_proofs` table **off** the
type-values surface.

This design is part of the ONE resolver. The relation engine is one node of the single
`SemanticQueryKey → ProjectSemanticDispatch::execute → SemanticGraphStore` dispatch — never a second
engine. #1 and #2 are ONE design problem: the reentry/assumption stack is exactly what decides whether a
relation result is stable enough to admit.

### 0.1 The baseline this design corrects (historical + future-RI deletions)

This section names the pre-`U2.RELATION_INFER` baseline the design addresses. Two of the three defects are
already corrected on the current tree by the value-domain-shape block; the third (the entry-point + cycle
guard) still stands and is slated for deletion in later RI work — see the **Current state** paragraph under
Decision 5. Concretely:

1. The relation memo key was once a **bare pair** `{ source, target }` — not a sound cache identity, since
   the same pair relates differently under a different `RelationKind`, excess/variance `RelationPolicy`,
   source freshness, inference setup, or env/substitution, all colliding on one memo slot. **Corrected:** the
   landed `RelateMemoKey` (`semantic_query.rs:2682`) carries the full identity (source/target/`RelationKind`/
   `RelationPolicy`/`FreshnessKey`/`Option<InferenceContextKey>`/`RelationContext`), and
   `SemanticQueryKey::Relate` (`:2875`) mirrors it.
2. `RELATION_IN_FLIGHT` is a **process-global** (`thread_local!`) cycle guard with
   `enter_/exit_relation_guard`, returning `RelationResult::Unknown` on re-entry. **Still present** on the
   current tree (now keyed on the full `RelateMemoKey`); the `relate_nodes(source, target)` entry point and
   this thread-local guard are the future-RI deletions tracked under Decision 5.
3. The cold path once **MEMOIZED `Unknown`** — a genuinely-recursive `interface A { next: A }` vs
   `interface B { next: B }` was mis-decided `Unknown` and that sentinel was cached as a warm,
   fact-validated, persistent entry (the relation analogue of admitting a flow-cycle sentinel). **Corrected:**
   the public `RelationOutcome` (`semantic_query.rs:1841`) has no `Unknown` arm, and overflow / cycle / budget
   edges route through `ReturnOnly` so no sentinel is ever warm-admitted.

The design therefore targets: (a) full-identity keys (landed); (b) a coinductive SCC that publishes a *valid*
recursive relation as `Assignable + CoinductiveCycle`; (c) NEVER warm-admitting `Unknown` / open-assumption /
budget / cycle sentinel — those route through `ReturnOnly`. `RELATION_IN_FLIGHT`, `enter_/exit_relation_guard`,
and the bare-pair `relate_nodes(source, target)` signature are the remaining future-RI deletions (per-block
legacy-deletion lists, §6); the bare-pair memo key and the memoized `RelationResult::Unknown` arm are already
gone.

---

## Decision 1 — Relation cache category: PERSISTENT full-identity query-identity cache

### 1.1 The category

`Relate` is a **PERSISTENT, cross-request, fact-validated query-identity cache** living in the ONE
`SemanticGraphStore` family-memo substrate (`FamilySlots`, multi-candidate,
`ReadSetSignature.validate_with_self_roots` rail). It is **NOT** a transient intra-check cache. The
`RelationBudget` pair memo (today `BudgetedRelationMemo`) is re-keyed from the bare `(source, target)` pair
to the **full §2.7 identity** and folds into the same `Relate`-family slot — there is no second relation
cache type after this gate.

The transient-vs-persistent tension is resolved by **separating two distinct objects** the current code
conflates into one process-global table:

| Object | Lifetime | Cache key? | Role |
|---|---|---|---|
| Full `Relate` identity (§2.7) | request-independent | **YES** — the persistent query-identity key | identifies the slot |
| `RelationAssumptionStack` (in-flight reentry/comparison chain) | TRANSIENT, per-`CheckerTransaction` | **NEVER** | gates admission via `ReturnOnly` until SCC discharge |

The crux: **the live stack is not a property of the relation, it is a property of the in-flight
computation.** Once the SCC closes positive, the truth of `A ~ B` is determined by `(identity, content)`
alone and is independent of which stack we happened to be on — so the stack must NOT enter the key (doing so
makes every cyclic result un-shareable across requests = perf collapse, and is a category error). A result
decided while an assumption is still open is gated to `ReturnOnly`; once discharged it is admissible under
the bare full-identity key.

### 1.2 The key

```rust
SemanticQueryKey::Relate {
    source: SemanticNodeId,
    target: SemanticNodeId,
    relation: RelationKind,                          // assignable / subtype / identity / comparable / strict-subtype
    policy: RelationPolicy,                          // overload-selection / excess-property / variance (incl. method-bivariance) policy
    source_freshness: FreshnessKey,                  // fresh-object-literal excess-check mode vs non-fresh
    inference_context: Option<InferenceContextKey>,  // Some = binding-producing; None = pure assignability
    context: RelationContext {
        resolve_env_hash, type_env_hash, lib_env_hash, project_identity,  // R21 split — R T L J
        substitution: SubstitutionCanonicalHash,                          // same hash as flow/call keys
        projection_reduction: ProjectionReductionContext,
        // NO parse_env_hash (P); NO project_config_hash (R21); NO content/parse_stable_hash; NO fact_dep_signature (R6)
    },
}
```

**Env dims: `R T L J`** (resolve / type / lib / project), split, never bundled — matches value-domain §2.2
(`Relate … Env dims R T L J`). `lib_env_hash` **is** included because relation reads `LibIntrinsic`
apparent-member facts (relating against `string`/`number`/`Array` consults prototype/apparent surfaces keyed
by lib — consistent with the CLAUDE.md rule that typed-IR resolve carries `lib_env_hash`).

**Why `parse_env_hash` MUST be EXCLUDED (obligation-5 minimality):** the relation engine operates
**exclusively on `SemanticNodeData`** (post-lowering typed IR) — it never reaches into the parser arena. The
parse-env sensitivity of lowering OXC→`TypeExpr`→`SemanticNodeId` is already captured upstream on the
`SemanticNodeId`'s provenance and on the value's `ReadSetSignature.facts`. Folding `parse_env_hash` into the
relation key would be a non-minimal axis (R21 minimality / §6.2 benched-minimal env dims) that never changes
the relation outcome independently of `R/T/L/J`.

### 1.3 The six proof-obligations (A.2) — each enters the IDENTITY, rides the VALUE, or is proven irrelevant

The persistent choice is sound iff every obligation is captured. An unproven obligation routes the result
through `ReturnOnly`.

| # | Obligation | Captured | Mechanism |
|---|---|---|---|
| 1 | live relation/comparison stack (in-flight reentry chain) | **NOT in key — `ReturnOnly` gate** | transient `RelationAssumptionStack` per-`CheckerTransaction`; a result decided under an open assumption admits only at SCC-close — for a non-binding member of a **pure non-binding SCC**, or a NEGATIVE non-binding member whose transitive consumed-verdict closure contains no binding member — or, for a binding member (`inference_context = Some`), **a POSITIVE non-binding member of a MIXED SCC** (it carries the shared binding-referencing proof), or **a NEGATIVE non-binding member whose consumed-verdict closure reaches a binding sibling**, only at the relevant enclosing session's close (the LATER event, R-c / §2.3 step 4). Keying it would collapse all cyclic-result reuse (perf collapse) and is a category error. |
| 2 | in-flight inference context | **IN KEY** | `inference_context: Option<InferenceContextKey>` — the content-free projection of the COMPLETED session (R-b generated, R-c admit-at-complete). |
| 3 | strict-policy family in force | **IN KEY** (`context.type_env_hash`) **+ value BRANCHES** | the strict family ⊂ `type_env_hash` (R21 split); per #20/RI-10 the reducer *branches* on strict, so the value differs AND `type_env_hash` isolates strict-on from strict-off. A strict-on relation cannot warm-hit a strict-off request. |
| 4a | freshness MODE (fresh-object-literal excess-check vs non-fresh) | **IN KEY** | `source_freshness: FreshnessKey`. |
| 4b | freshness STRUCTURE (reachability of BOTH related types) | **ON VALUE** (R6) | `ReadSetSignature.facts` records path-precise `Member`/`MemberPresence` of both source and target; warm-hit revalidates. A structural edit to either type misses the warm read. |
| 5 | five split env dimensions | **IN KEY** (R21 split, never bundled) | `R T L J` + `substitution` + `projection_reduction`; `parse_env_hash` proven irrelevant (§1.2). |
| 6 | reentry state (coinductive assumption stack) | **NOT in key — `ReturnOnly` gate** | identical to #1: a result decided under an open assumption is inadmissible until the SCC discharges; satisfied by the batched-admission gate (R-a), not by a key axis. |

### 1.4 R6 — version rooting on the VALUE only

The full identity carries **no** `content_hash`, `parse_stable_hash`, or `fact_dep_signature` (R6). The
`source`/`target` are interned content-free `SemanticNodeId`s; the env axes are content-free project-config
projections. Version rooting is entirely on the cached `RelationPayload`:

- `ReadSetSignature.facts` — the path-precise observation set traced during the relate (member
  presence/types of both surfaces, `TypeEnvOptions` reads, `LibIntrinsic` reads). Revalidated on **every**
  warm hit against the live `StoreView`.
- `self_root_canonicals` — the file-derived origins of `source`, `target`, **and every declaration visited
  during structural descent** (not just the two roots — an SCC may pull members from third files). Strict
  self-root validation (`validate_with_self_roots`) rejects on a same-canonical content edit.
- `validated_at_generation` — recency metadata only (catches a `ProjectGeneration` reset that bumps no file
  content); NEVER a semantic-validity oracle.

### 1.5 The `RelationBudget` re-key

`BudgetedRelationMemo` is re-keyed from `(SemanticNodeId, SemanticNodeId)` to the **interned full §2.7
identity** and folds into the same `Relate`-family slot. A repeat negative or positive is a memo hit on the
FULL identity, so it cannot false-hit across relation-kind / policy / freshness / inference-context / env
differences (§6 `RelationBudget`). The old bare-pair memo + its memoized `Unknown` arm are DELETED, not
wrapped. The per-family candidate cap for the `Relate` family is the **adaptive per-family cap**
(invalid-first then LRU-by-valid-hit), owned by `U3.CACHE_FACT_MODEL` §6.1 and wired in RI-1 — NOT the
legacy uniform cap-4 FIFO.

---

## Decision 2 — Coinductive dispatch as a first-class `execute` primitive

### 2.1 The transient substrate (replaces the process-global guard)

`Relate` is a first-class `ProjectSemanticDispatch::execute` primitive producing
`SemanticQueryValue::Relation(RelationPayload)`. The coinductive primitive lives on the **cold-compute frame
of the one resolver**, not a second engine:

```rust
struct CheckerTransaction {                          // §4.2 — the per-root cold-compute frame
    // … §4.2 env / contextual_target / overload_policy / freshness_excess_policy fields …
    reentry_stack: CheckerReentryStack,              // ONE shared re-entry / cycle-id space (designed-for cross-engine span; only Relate wired at U2 — §2.1)
    relation_cycle_stack: RelationAssumptionStack,   // typed VIEW over the Relate-tagged nodes of reentry_stack
    sessions: SessionStack,                          // the active InferenceSession stack (Decision 3); each in-flight
                                                     // session carries a transient content-free SessionId (§2.2/§3.3)
    session_admission: SessionAdmissionLedger,       // per-session deferred-admission ledger keyed by transient SessionId
                                                     //   (owned by RI-6); RI-4's SccLedger deposits binding SCC members at
                                                     //   SCC-close, drained at each session's CompletedDeterministic close (§2.3 step 4 / §3.3)
    read_set: ReadSetSignatureAccumulator,           // the deterministic ReadSetSignature.facts for admission
    budget: CheckerBudget,                           // shared with RelationBudget / CallResolutionBudget / FlowSliceBudget
}
```

- **`CheckerReentryStack`** is the single shared re-entry / cycle-id space. Its DESIGNED-FOR scope spans
  `Relate`, `ResolveCall`, `FlowReturn`, `ContextualTypeAt`, `FlowNarrowingAt` (and the reducers beneath) —
  one stack so the per-engine cycle spaces can never diverge. **At U2 only `Relate` is WIRED onto it** (plus
  the `Instantiate{args:[], body_mode: Skeleton}` BFS it subsumes); the `ResolveCall` / `FlowReturn` /
  `ContextualTypeAt` / `FlowNarrowingAt` engines — their enum variants, spec rows, and behavior — land at
  **U6** (native-typeinfo-parity.md:507 / qvd:942-948; they are **NOT pre-registered at U2**, and the standing
  `semantic_query_key_spec_table_equals_enum` meta-guard would reject any U2 tree that referenced them), and
  U6 wires them onto this same substrate then. Each node is keyed by its **full normalized identity** (a
  `Relate` node by the full §2.7 identity incl. `InferenceContextKey`; a U6 `FlowReturn` node by its
  `FlowReturnContext + ReturnProjectionDemand + FlowInputContext`; etc.). The per-flow cycle space and the
  relation assumption stack are *typed views* of this one stack — they cannot diverge.
- **`RelationAssumptionStack`** is the projection of `reentry_stack` onto the `Relate`-tagged nodes plus the
  recorded **assumption edges** (which in-flight relation assumed which other in-flight relation). It is
  **per-`CheckerTransaction` (per-relation-root), heap-backed, NEVER thread-local-global, NEVER
  process-wide.**

This **deletes** `RELATION_IN_FLIGHT` / `enter_relation_guard` / `exit_relation_guard`.

### 2.2 The dispatch step (what happens on re-entry)

When `execute(Relate{K})` is dispatched and `K`'s full normalized identity is **already on
`reentry_stack`**, the dispatcher does **NOT** recompute, does **NOT** self-await the in-flight slot, and
does **NOT** consult the warm memo. It records/consults a **scoped assumption** `RelationAssumption::Holds`
for `K`, returns it to the local reducer as a **transient sentinel value**, records an assumption edge
`caller → K`, and marks the caller's accumulator `OpenAssumption(K)` (a transient accumulator flag, NEVER
written to a published `ReadSetSignature.facts`).

This is the coinductive "assume the relation holds and verify the rest" step. The assumption is keyed by the
**full identity** — an assumption recorded for `K` is never reused for a different relation identity on the
same `(source, target)` pair (different `relation` / `policy` / `freshness` / `inference_context` /
`context` ⇒ different assumption).

**The cycle-detection identity is NOT the admission identity for an in-flight binding-`Relate`.** A
binding-producing `Relate` (`inference_context = Some`) re-enters while its enclosing `InferenceSession` is
still mutating, so its completed `InferenceContextKey` is **not yet well-defined** (§3.1/§3.3 R-c) — there is
no concrete fingerprint to key its reentry-stack node by *now*, when cycle detection needs it. The two
identities are therefore distinct objects:

| Identity | When | Composition | Cache key? |
|---|---|---|---|
| **Reentry-stack / assumption identity** (in-flight cycle detection, NOW) | while the session is in-flight | `(source, target, relation, policy, source_freshness, context)` **+ the transient `SessionId`** of the enclosing in-flight session (a content-free per-session token on the `CheckerTransaction`, in place of the not-yet-knowable `InferenceContextKey`) | **NEVER** |
| **Admission identity** (at `CompletedDeterministic`, R-c) | at session close | the full §2.7 identity with the now-knowable completed `InferenceContextKey` substituted for the transient `SessionId` | **YES** (the persistent slot key) |

For a pure non-binding assignability (`inference_context = None`) the two coincide — there is no session and
no transient handle. The transient `SessionId` is a per-`CheckerTransaction` in-flight token (content-free,
allocated per session on `sessions`); it **NEVER** enters a published key, a `ReadSetSignature.facts`
observation, or any fact signature. Soundness: two distinct in-flight sessions over the same `(source,
target)` cannot collide on the reentry stack (distinct `SessionId`), and the result they each decide is
re-keyed to its completed `InferenceContextKey` before admission (§3.3 R-c). When such a binding-`Relate`
participates in a positive SCC, its slot in the `CoinductiveCycle { keys }` proof is left **UNFILLED at
SCC-close** and is filled — together with the remap from the transient `SessionId`-based reentry identity to
its completed full §2.7 key — only when its enclosing session reaches `CompletedDeterministic`, through the
deferred `SessionAdmissionLedger` (§2.3 step 4). SCC-close and session-completion are independent convergence
axes, so for a binding member this remap+slot-fill is a **session-close** event, not an SCC-close event; the
durable proof is published only once every member's slot carries a completed full §2.7 key and never carries a
transient `SessionId`.

### 2.3 The EXACT "provisional becomes STABLE / ADMISSIBLE" rule (SCC closure)

A `Relate` result becomes **STABLE iff its SCC has CLOSED with no undischargeable obligation** — but
SCC-close is the **admission instant only for a pure non-binding SCC** (and for a NEGATIVE non-binding member
of a mixed SCC whose `NotAssignable` proof carries no `keys: S` set **AND whose transitive consumed-verdict
closure contains no binding member**). When any binding member participates — a binding member
(`inference_context = Some`), a POSITIVE non-binding member that shares the SCC's binding-referencing
`CoinductiveCycle { keys: S }` proof, OR a NEGATIVE non-binding member whose consumed-verdict closure
transitively reaches a binding sibling — SCC-close is **necessary but NOT sufficient**: admission additionally
requires the relevant enclosing session(s) to reach `CompletedDeterministic`, so the admission instant is the
**LATER of {SCC-close, the relevant binding session(s)' close}**. Mechanically:

1. **SCC root.** The deepest-on-stack relation of an assumption cycle is the SCC root. When the root's cold
   build returns (all structural descent has folded), the dispatcher runs **SCC closure**: Tarjan over the
   assumption edges recorded since the root was pushed, collecting the strongly-connected set
   `S = {K₁ … Kₙ}` of mutually-assuming relations.
2. **Classify each member's outgoing obligations.** An *assumptive* obligation is a back-edge into `S` (the
   coinductive "assume it holds" edges — NOT discharged separately). A *non-assumptive* obligation is any
   sub-relation that is not a back-edge into `S` (member relations, instantiated-body relations, constraint
   relations, apparent-member relations).
3. **Discharge verdict (bottom-up over the condensation DAG):**
   - **ALL non-assumptive obligations of EVERY member POSITIVE ⇒ SCC closes POSITIVE.** Every `Kᵢ` is decided
     `Assignable { bindings }` with a `CoinductiveCycle { keys: S }` proof. **Admission timing splits by SCC
     COMPOSITION, not member class alone (§2.3 step 4):** every positive member carries the SAME shared
     `keys: S` proof, and `S` references the binding members whose completed keys are not knowable until
     session-close, so that proof is not constructible until every binding slot is filled. Therefore in a
     **pure non-binding SCC** every member publishes at SCC-close (the proof is complete then); in a **mixed
     SCC** (≥1 binding member) the `SccLedger` keeps the WHOLE positive batch pending and publishes NONE at
     SCC-close — each binding member's verdict + fact-set is recorded into the enclosing session's
     `SessionAdmissionLedger`, and each POSITIVE non-binding member's verdict + fact-set is held on the
     `SccLedger` against the same later event (its `inference_context = None` does NOT exempt it — it carries
     the binding-referencing proof). The whole batch publishes together at the **LATER of {SCC-close, the last
     binding member's session-close}**, when the proof is remapped and complete. A member whose *only*
     unresolved edges are back-edges (a genuinely recursive type with no non-recursive base case) **discharges**
     — it is NOT rejected for lacking a base case. The published `ReadSetSignature.facts` is the **UNION of all
     SCC members' non-assumptive observed facts** (so a content edit to any file any member visited misses every
     member's warm read); the `OpenAssumption` taint is cleared at close.
   - **ANY non-assumptive obligation NEGATIVE ⇒ the implicated member is `NotAssignable` (final,
     publishable — NOT `ReturnOnly`).** The assumption that depended on it collapses; dependents recompute
     bottom-up over the condensation (no blanket fail). **Admission timing for the NEGATIVE close splits on TWO
     axes — the IDENTITY-leak axis (member class) AND the VERDICT-dependency axis (does the member's verdict
     transitively consume a binding sibling's not-yet-converged verdict?):**
       - On the IDENTITY-leak axis a `NotAssignable { reason, failing_sub }` proof carries NO `keys: S` set (and
         `failing_sub` is content-free, Decision 4), so a published negative never leaks a transient binding
         identity — a negative member never defers *merely* because the SCC is mixed.
       - But on the VERDICT-dependency axis the negative member's published fact set is its **transitive
         consumed-verdict closure** (below), which MAY include a binding sibling's verdict. That binding sibling's
         SCC-close verdict is **PROVISIONAL** until its session reaches `CompletedDeterministic` (§3.3; parent
         §4.2:1413–1415 RE-MEASURES across fixation iterations), so it can later FLIP to `Assignable` (⇒ this
         member should be `Assignable`) or `Abandon` (⇒ the sibling never validly produced the verdict this member
         consumed). In BOTH cases the content-fact rail CANNOT catch it — inference convergence is NOT a content
         edit, so this member's `ReadSetSignature.facts` (even the transitive consumed-verdict closure including the
         sibling's content facts) still validates. Warm-publishing the negative then would warm-admit work that did
         not converge on every axis (the §3.3 / risk 6 prohibition — the SAME defect class round 5 fixed for
         positive members, on the complementary NEGATIVE sign, surfacing through verdict CONSUMPTION rather than
         shared-proof completeness).
     **Unified rule (a non-binding member of a mixed SCC):** it publishes at SCC-close ONLY when it is NEGATIVE
     **and** its transitive consumed-verdict closure contains **no binding member**; otherwise it defers. (Positive
     members always defer in a mixed SCC because they carry the shared binding-referencing `keys: S` proof; negative
     members defer when their verdict transitively consumed a binding sibling's not-yet-converged verdict.) Hence: a
     NEGATIVE non-binding member whose closure contains no binding member publishes at SCC-close **even in a mixed
     SCC** (every verdict it consumed is itself final at SCC-close); a NEGATIVE non-binding member whose closure
     DOES reach a binding member rides the **SAME deferred batch** as the positive members (§2.3 step 4) to the
     LATER of {SCC-close, that binding sibling('s) session-close}, dropping to `ReturnOnly` if that session
     `Abandon`s — exactly as the positive batch already does. A binding `NotAssignable` member always records its
     negative verdict + fact-set into the enclosing session's `SessionAdmissionLedger` at SCC-close and publishes
     only at session-close (§2.3 step 4). **Do NOT instead defend this by asserting a binding member's verdict is
     final-and-monotone at SCC-close** — parent §4.2:1413–1415 re-measurement + §3.3 "setup still mutating"
     contradict that; the deferral GATE is the sound fix, not a monotonicity invariant.
     **Each negative member publishes with the SAME
     fact-set rule as positive members — NOT a per-member fact set:** a member that is `NotAssignable` SOLELY
     because a back-edge to a sibling collapsed (its own non-assumptive obligations were all positive/absent)
     never observed the fact that drove the sibling negative in its own frame, so a per-member fact set would
     publish a warm negative whose `ReadSetSignature.facts` does not cover that fact — editing it flips the
     sibling (and therefore this member) to `Assignable` while the stale warm negative still validates
     (stale false-negative warm hit, the unsound-coinduction class this gate exists to prevent). The
     published fact set is therefore the **transitive consumed-verdict closure**: the union of the
     non-assumptive observed facts of this member AND of every SCC member whose verdict this member's verdict
     actually consumed (transitively through collapsed back-edges). In the common case where the negative
     close consumed every member's verdict this equals the full SCC-union (identical to the positive close);
     the tighter closure is admissible only when the consumed set is a strict subset. An edit to any file in
     that closure misses this member's warm read.
   - **ANY obligation `Unknown` / cancelled / `BudgetExceeded` ⇒ the ENTIRE SCC is `ReturnOnly`.** The only
     genuinely undischargeable case (R-a batched poison — accepted, §7 residual risk 1).
4. **(R-a) Batched admission, split by SCC composition and close sign.** SCC-close and inference-session
   completion are **two independent convergence axes**: a binding member's relation SCC can close (relation
   recursion converges) while its enclosing `InferenceSession` is still `InProgress` (the session's fixation
   fixed-point loop continues after that relation pass — §2.2, parent §4.2:1413–1415). Admission
   therefore splits by SCC composition and close sign, and a binding member is **NEVER published at SCC-close**.
   A member computed under an open assumption cannot admit before the root closes — its slot stays empty until
   at least SCC-close. The per-`CheckerTransaction` `SccLedger` accumulates `(Kᵢ, per-member observed-fact set,
   per-member verdict)` and at SCC-close computes each member's **published** fact set from those per-member
   sets — the SCC-union for a positive close, the transitive consumed-verdict closure for a negative close
   (§2.3 step 3, never the bare per-member set). It then routes by close sign and composition:

   - **Negative close — non-binding members (`inference_context = None`).** A `NotAssignable { reason,
     failing_sub }` proof carries no `keys: S` set (and `failing_sub` is content-free, Decision 4), so on the
     identity-leak axis nothing forces a negative member to defer. Publish timing splits on the VERDICT-dependency
     axis: if the member's transitive consumed-verdict closure (§2.3 step 3) contains **NO binding member**, the
     `SccLedger` performs one batched `FamilySlots::publish` pass for that key **at SCC-close** — no remap — **even
     in a mixed SCC** (the entry is sound: every verdict it consumed is final at SCC-close, and the closure's
     content facts back the warm read). If the closure DOES reach a binding member, that sibling's verdict is
     PROVISIONAL until session-close (it may FLIP or `Abandon`, and inference convergence is not a content edit the
     fact rail can catch), so the negative member rides the SAME deferred batch as the held positive non-binding
     members (below) to the LATER of {SCC-close, that binding sibling('s) session-close} — and its published
     payload is the **session-converged re-discharge** (§2.3 step 4): it publishes the re-discharged slotless
     `NotAssignable` ONLY when the re-discharge yields a stable publishable negative (the common case being the
     sibling converging to the SAME negative sign it held at SCC-close, §2.3 step-4 outcome 1), or is released
     WITHOUT publish (dropped to `ReturnOnly`, joiners recompute) when the re-discharge is non-stable — the
     sibling verdict FLIPPED to `Assignable` (so this member should now be `Assignable`), its own verdict flipped,
     or its bindings changed — OR when that session `Abandon`s (§2.3 step-4 outcomes 2 and 3).
   - **Positive close — non-binding members.** In a **pure non-binding SCC** every key in `S` is a non-binding
     key whose in-flight and completed identities coincide (§2.2), so the shared `CoinductiveCycle { keys:
     S }` proof is fully constructible at SCC-close and the `SccLedger` performs one batched
     `FamilySlots::publish` pass per member key **at SCC-close** — no remap. In a **mixed SCC** (≥1 binding
     member) the same proof references binding members whose completed keys are not knowable until session-close,
     so the `SccLedger` does **NOT** publish the positive non-binding members at SCC-close either — it holds them
     on the same deferred batch as the binding members (below); they publish at the LATER event the
     **session-converged re-discharge** result, whose `CoinductiveCycle` proof references only completed keys
     (§2.3 step 4).
   - **Deferred members — every binding member (either sign), the held POSITIVE non-binding members of a
     mixed SCC, AND any NEGATIVE non-binding member whose transitive consumed-verdict closure reaches a binding
     sibling.** At SCC-close the `SccLedger` does **NOT** publish them and does **NOT** remap any binding key.
     Each binding member is handed to the enclosing session's **`SessionAdmissionLedger`** (the per-session
     ledger on the `CheckerTransaction`, keyed by the transient `SessionId`, §3.3), recording **PROVISIONAL
     deferral metadata** `(Kᵢ in-flight identity, provisional relation verdict, provisional SIGN, the set of
     consumed binding-sibling verdicts WITH the sign each held when consumed at SCC-close, accumulated SCC
     observed-fact-set)` — the same fact-set the rules above compute (SCC-union for positive, transitive
     consumed-verdict closure for negative). **This recorded snapshot is NEVER the published payload — it is a
     caller-return value + the identity of WHAT to re-discharge at session-close.** The recorded consumed-sibling
     signs are load-bearing only as inputs to that re-discharge; the published verdict comes from the
     re-discharge against the *converged* state, NOT from the provisional sign held at SCC-close. The held
     positive non-binding members AND the held binding-consuming negative non-binding members ride the same
     deferred batch (their own keys are already complete, but a positive member's shared proof references binding
     keys not yet knowable, and a binding-consuming negative member's consumed verdict is not yet converged, until
     session-close). Each binding member's slot in the provisional `CoinductiveCycle { keys: S }` snapshot is left
     **UNFILLED at SCC-close**.

     **The session-close drain is a THREE-outcome gate whose published payload IS the session-converged
     RE-DISCHARGE.** At the batched-publish instant (when ALL relevant sessions of the deferred batch have reached
     `CompletedDeterministic` — the LATER of all their session-closes) each deferred member's cold compute
     **COMPLETES**: the SCC is **RE-DISCHARGED against the fully-converged state** (final inference bindings, final
     own verdict, final consumed-sibling verdicts) through the **same `execute(Relate{Kᵢ})` dispatch** — the one
     engine's cold compute finishing once its session inputs are final, NOT a second engine. The published
     `RelationPayload` (outcome + bindings + proof) **IS that re-discharge result**, keyed by the now-complete
     full §2.7 identity, so a stale SCC-close snapshot is **impossible by construction**. The gate routes on the
     re-discharge outcome:
       1. **Every relevant session `CompletedDeterministic` AND the re-discharge yields a STABLE determined
          publishable outcome ⇒ PUBLISH the re-discharge result.** The ledger runs the batched
          `FamilySlots::publish` pass for the deferred batch, splitting by re-discharged member sign:
            - **POSITIVE members** (every binding-positive member + every held POSITIVE non-binding member): the
              re-discharge runs against the now-knowable completed identities, so the `CoinductiveCycle { keys: S }`
              proof it produces already references **completed full §2.7 keys** (the re-keying is intrinsic to the
              re-discharge — there is no separate remap of a stale recorded proof; the snapshot's unfilled slots
              are discarded). Publish each positive member as `Assignable` with the re-discharged final
              `bindings` + the **complete `CoinductiveCycle` proof** and the re-discharged fact-set.
            - **NEGATIVE members** (every binding-negative member + every held binding-consuming NEGATIVE
              non-binding member): the re-discharge yields a publishable slotless `NotAssignable { reason,
              failing_sub }` proof (Decision 4 / the §2.3 step-3 negative-close above) keyed by the binding
              member's completed §2.7 key (the re-discharge supplies the completed `InferenceContextKey`). It
              carries NO `keys: S` set and receives **NO `CoinductiveCycle` slot-fill** — there is no positive
              proof to complete. This reconciles with the §2.3 step-3 negative-close, which already omits
              proof-fill for negatives: the `CoinductiveCycle` slot-fill applies ONLY to re-discharged POSITIVE
              members.
            - **Named special case (the consumed-sibling "same-sign" check).** When every consumed binding-sibling
              verdict converged to the SAME sign it held at SCC-close — and the member's own verdict + bindings are
              unchanged — the re-discharge reproduces the provisional outcome; this is the COMMON publishable path,
              but it is a *named instance* of "the re-discharge yields a stable publishable outcome," not the whole
              gate.
       2. **A relevant session is `CompletedDeterministic` BUT the re-discharge yields a NON-stable / undetermined
          / `Unknown` / `BudgetExceeded` outcome ⇒ RELEASE WITHOUT PUBLISH → `ReturnOnly` → joiners recompute.**
          This subsumes every staleness instance: (i) the member's OWN verdict flips at its session's
          `CompletedDeterministic` (its own inference bindings converge differently) **even with no consumed
          sibling**; (ii) a consumed binding-sibling verdict FLIPPED sign — a held POSITIVE member whose binding
          sibling converged to `NotAssignable` (a **stale false-positive** under the snapshot) or a held
          binding-consuming NEGATIVE member whose binding sibling converged to `Assignable` (a **stale
          false-negative**); (iii) the member's converged `bindings` / `relation_proof` differ from the snapshot
          even when the sign is unchanged (publishing the snapshot would ship stale bindings/proof). In ALL of
          these the content-fact rail CANNOT catch the staleness because inference convergence is not a content
          edit — which is exactly why the published payload must be the re-discharge, not the snapshot. The held
          singleflight registration is **released WITHOUT a publish** (no entry, no `ReadSetSignature.facts`
          signature, no backfill, no reverse-index metadata); a concurrent joiner blocked on it then **recomputes**
          in its own transaction (the recompute re-discharges the SCC from a clean root — the same bounded perf
          cost as the `Abandon` recompute below).
       3. **Any relevant session ends `Abandoned(reason)` (cancel / budget / superseded / non-deterministic) ⇒
          RELEASE WITHOUT PUBLISH → `ReturnOnly` → joiners recompute.** Every deferred member of that batch —
          binding members, held positive non-binding members, AND held binding-consuming negative non-binding
          members — is dropped to `ReturnOnly`, and the held singleflight registration is **released WITHOUT a
          publish** (no entry, no `ReadSetSignature.facts` signature, no backfill, no reverse-index metadata); any
          concurrent joiner blocked on that registration then **recomputes** (it cannot validate an entry that
          will never exist).

     Because the published value is always the converged re-discharge, a positive non-binding member of a mixed
     SCC never publishes a proof whose binding slot never filled or whose binding sibling converged to the
     opposite sign, and a binding-consuming negative member never publishes a verdict whose consumed sibling
     never converged or converged to the opposite sign — both surface as a non-stable re-discharge (outcome 2).

   **Singleflight lifetime.** Within a single `CheckerTransaction` the SCC/session machinery already prevents
   duplicate work (the reentry stack and the `SccLedger` dedup every in-flight relation). Across transactions
   the story splits by member kind:

   - **Non-binding members — singleflight join on the publish instant.** A non-binding member's in-flight
     registration (R-a) is **held open until its publish instant**: in a **pure non-binding SCC** that is
     SCC-close; a NEGATIVE non-binding member of a mixed SCC releases at SCC-close **only when its transitive
     consumed-verdict closure contains no binding member** (else it is held with the deferred batch, below); a
     POSITIVE non-binding member of a **mixed SCC**, and a NEGATIVE non-binding member whose consumed-verdict
     closure reaches a binding sibling, are held until the **LATER of {SCC-close, the last binding member's
     session-close}** — they do NOT release at SCC-close, because they do not publish until then (the positive
     member's proof references binding keys; the negative member's consumed verdict is not yet converged). A
     concurrent top-level transaction that asked for the same non-binding `Kᵢ` joins cooperatively on that release
     event and validates the winner's published entry against its own view once it publishes — it does NOT degrade
     to a duplicate full-SCC recompute. **The deferred batch has THREE exits, and the singleflight registration
     follows all three.** (1) When every relevant session reaches `CompletedDeterministic` AND the
     session-converged re-discharge yields a STABLE determined publishable outcome, the batch **publishes that
     re-discharge result** (batched `FamilySlots::publish` — `Assignable` + complete `CoinductiveCycle` proof for
     positive members / slotless `NotAssignable` for negative members) and joiners validate the published entry.
     (2) When a relevant session reaches `CompletedDeterministic` BUT the re-discharge yields a non-stable /
     undetermined / `Unknown` / `BudgetExceeded` outcome (an own-verdict flip, a consumed binding-sibling sign
     flip, or a bindings change that alters the result), the snapshot is stale, so the whole deferred batch is
     **released WITHOUT a publish** (§2.3 step 4) — no entry, no `ReadSetSignature.facts` signature, no backfill,
     no reverse-index metadata — and a concurrent joiner held on the registration **recomputes** in its own
     transaction, re-discharging the SCC from a clean root. (3) When a relevant session `Abandon`s the whole
     deferred batch drops to `ReturnOnly` (§2.3 step 4) and the held registration is **released WITHOUT a
     publish** — no entry, no
     `ReadSetSignature.facts` signature, no backfill, no reverse-index metadata; a concurrent joiner held on that
     registration CANNOT validate an entry that will never exist, so on either release-without-publish exit
     ((2) or (3)) it **recomputes** in its own transaction (deterministically — the same bounded perf cost as the
     binding-member recompute below, never a hang or a stale hit).
   - **Binding members — NO mid-flight cross-transaction join.** A binding member's in-flight identity is keyed
     by its enclosing `CheckerTransaction`'s transient `SessionId`, which is **private to that transaction and
     not shareable across transactions**, and its final §2.7 key does not exist until session-close — so there
     is **no joinable singleflight key for a binding member's in-flight work across transactions**.
     Cross-transaction sharing of a binding `Relate` happens ONLY on the FINAL published query-identity slot,
     available AFTER session-close (keyed by the completed full §2.7 identity, with the now-knowable
     `InferenceContextKey`). A separate top-level transaction that needs the same binding `Relate` mid-flight
     opens its **OWN** inference session and recomputes; this is deterministic (both sessions converge to the
     same completed `InferenceContextKey`, so the first publish wins the slot and the other validates/joins on
     it) and is a **bounded PERF cost, NOT an unsoundness or a hang** (recorded as a perf residual, §7 risk 6).

   **Completed keys come from the re-discharge, not a remap of a stale proof (never carries a transient
   `SessionId`).** The in-flight reentry/SCC identities of binding-`Relate` members carry the transient
   `SessionId` stand-in (§2.2/§3.3 R-c), and the provisional SCC-close snapshot's `CoinductiveCycle { keys: S }`
   has unfilled binding slots. Those provisional artifacts are NEVER published. At **session-close** the deferred
   member's cold compute re-discharges against the converged state (§2.3 step 4); because every relevant session
   is `CompletedDeterministic` by then, every binding member's `InferenceContextKey` is knowable, so the proof
   the re-discharge produces already references **completed full §2.7 `Relate` keys** — the re-keying is
   intrinsic to the re-discharge, not a separate remap step over a recorded proof. Non-binding members of a pure
   non-binding SCC have no session handle, so their in-flight and completed keys already coincide and they
   publish at SCC-close (no re-discharge needed, no remap). A durable `CoinductiveCycle { keys: S }` proof is
   published only as the result of a re-discharge in which **every** member slot carries a completed full §2.7
   `Relate` key: for a **pure non-binding SCC** that instant is SCC-close (published directly, unchanged); for an
   SCC containing any binding member it is the **LATER of {SCC-close, the last binding member's session-close}**,
   so no member ever durably publishes a proof with an unfilled slot or a transient `SessionId` (a durably-
   published proof referencing a transient session identity would be dangling/unsound).

**Coupling to #1 — when a coinductively-derived result admits vs `ReturnOnly`:** a result becomes admissible
*exactly* when the SCC closes POSITIVE (`Assignable + CoinductiveCycle`) or NEGATIVE on a non-assumptive
obligation (publishable `NotAssignable`) — but the **publish trigger depends on SCC composition, close sign,
and verdict-dependency, not member class alone**: in a pure non-binding SCC every member admits at
SCC-close, and a NEGATIVE non-binding member of a mixed SCC admits at SCC-close ONLY when its transitive
consumed-verdict closure contains no binding member; while a **binding** member
(`inference_context = Some`), a POSITIVE non-binding member of a MIXED SCC (it shares the
binding-referencing proof), AND a NEGATIVE non-binding member of a mixed SCC whose consumed-verdict closure
reaches a binding sibling, admit only on the LATER event, when every relevant enclosing session reaches
`CompletedDeterministic` **AND the session-converged re-discharge of the member yields a STABLE determined
publishable outcome** (§2.3 step 4, §3.3 R-c) — and the **published payload IS that re-discharge result, NOT
the SCC-close snapshot** (the snapshot is provisional deferral metadata). The drain re-discharges each deferred
member against the converged state (final bindings, final own verdict, final consumed-sibling verdicts), not
merely against the fact that the session finished; the consumed-sibling "same-sign" condition is a named
special case of a stable re-discharge. A binding member whose session never converges — `Abandoned` (cancel /
budget / superseded / non-deterministic) — is dropped to `ReturnOnly`, exactly as a result is `ReturnOnly`
*whenever* any obligation is `Unknown`/cancelled/`BudgetExceeded`, i.e. the assumption is never discharged;
symmetrically, a deferred member whose re-discharge is non-stable — a consumed binding-sibling verdict FLIPS
sign, the member's own verdict flips, or its bindings change — is dropped to `ReturnOnly` (release-without-
publish, joiners recompute) because the snapshot it would have published is stale (§2.3 step-4 outcome 2). The
transient `RelationAssumption::Holds` sentinel is **NEVER** warm-admitted, **NEVER** the published proof,
**NEVER** a fact signature or backfill. A positive SCC's `CoinductiveCycle { keys }` proof is produced fresh by
the session-converged re-discharge — referencing only completed keys — distinct from the sentinel.

### 2.4 Cross-engine unification (replaces per-family stand-ins)

`Relate` keeps the §4.1 **assumption / discharge protocol** unchanged (the scoped-assumption + coinductive-SCC
algorithm of §2.3) and participates in the **one shared** `CheckerReentryStack`. ("Unchanged" scopes to that
assumption/discharge *protocol*; the in-flight *keying* of §4.1's `inference_context` component is specialized
per **R-c (§3.3)** — the completed `InferenceContextKey` is provably not knowable in-flight, so the reentry
node uses the transient `SessionId` stand-in, §2.2.) The substrate is DESIGNED for the full cross-engine cycle
`ResolveCall → FlowReturn → narrowing → Relate → ResolveCall` — a transient re-entry assumption on the one
stack, never a self-await / budget-spin — so that each value domain can discharge its own SCC fixed-point to a
converged deterministic result before anything warm-admits (`FlowReturn` → stable projected return type;
`ResolveCall` → completed overload-winner + substitution fingerprint; `ContextualTypeAt` → contextual-target
equality; `Relate` → §4.1 closure). The HARD RULE is uniform: only a converged/stable/deterministic per-domain
result is cacheable; unconverged / cancelled / superseded-mid-flight / budget-exceeded ⇒ `ReturnOnly`.

**At U2, only `Relate` is wired onto the substrate.** The `ResolveCall` / `FlowReturn` / `ContextualTypeAt` /
`FlowNarrowingAt` engines — their enum variants, spec rows, and behavior — land at **U6** (native-typeinfo-
parity.md:507, qvd:942-948; pre-registration at U2 is forbidden and the standing
`semantic_query_key_spec_table_equals_enum` meta-guard would reject it), and **U6** routes them onto this same
shared stack (the Rescope section records U6, not RI-8, as the owner of that routing).

**RI-8 (U2) consumes the RI-3-built substrate and deletes ONE per-family stand-in: `RefCycleResultDb`** (today's
cycle authority for parameterized generics — a `ComputeAdmission` cold-path BFS dispatching
`Instantiate { args: [], body_mode: Skeleton }` with strict self-root validation) → its *transient* cycle
detection collapses into the `Skeleton`-mode SCC over the shared stack (the BFS becomes a `reentry_stack` walk);
its *persistent boolean* `ref_root_reaches_transitive_cycle_node` result becomes an ordinary derived
query-identity cache entry off the closed SCC. There is no bespoke ref-cycle DB after RI-8. **Keeping
`RefCycleResultDb` "as a non-authoritative optimization" is FORBIDDEN** — two cycle-detection paths over the
same question is the precise divergence/hang class the architecture forbids. The RI-8 migration MUST preserve
the existing strict self-root warm-read semantics (the BFS root file plus every visited declaration's file)
inside the SCC's `ReadSetSignature.self_root_canonicals`. **RI-3 is the SOLE builder + `Relate`-wirer of
`CheckerReentryStack`; RI-8 builds no substrate and wires no `Relate`** — it only consumes the existing
substrate (a second "build … and wire `Relate`" would invite a forked second substrate, the divergence class
the architecture forbids).

**The flow depth-sentinel retirement is DEFERRED to U6** — it is replaced by the `FlowReturn` view of
`reentry_stack`, which only exists once `FlowReturn` lands at U6 — so it is **NOT** an RI-8 deletion. RI-8's
sole legacy deletion is the `RefCycleResultDb` / `ref_root_reaches_transitive_cycle_node` path (a
generic-instantiation concern live at U2 via the `Skeleton` BFS).

---

## Decision 3 — Inference-session admission tightening

The mutable `InferenceSession` (on `CheckerTransaction.sessions: SessionStack`, with mutable `infos`,
candidate sets, fixation flags) is TRANSIENT, never a key, never admitted. Admission rule:

- `state == InProgress` ⇒ **`ReturnOnly`** (not yet converged).
- `state == Abandoned(reason)` (cancel / budget-exceeded / non-deterministic / superseded mid-flight) ⇒
  **`ReturnOnly`**.
- A **speculative / losing** overload candidate's session ⇒ **`ReturnOnly`** (no entry, no fact signature,
  no backfill).
- A **session-local delta** (a binding-producing `Relate` depositing candidates into an `InferenceInfo`) ⇒
  **`ReturnOnly`** — meaningful only within its session.
- Only `state == CompletedDeterministic` admits, and only the **FINAL typed result** (a `ResolvedCall`, a
  `RelationPayload`, a `Conditional` reduction, a concrete instantiation) under the §2 root key, when:
  `no speculative-losing session survived && all inferable params fixed-or-deterministically-defaulted &&
  final substitution/bindings immutable && read-set finalization Cacheable`.

### 3.1 `InferenceContextKey` content-free (R6-clean)

`InferenceContextKey` is the **fingerprint of the COMPLETED session's SETUP**, frozen at the instant the
session reaches `CompletedDeterministic`:

```rust
struct InferenceContextKey {
    inferable_params: InferableParamSetId,          // interned id of the SET of inferable TypeParamIds (NOT their bodies)
    variance_phase: VariancePhase,                  // closed enum: covariant / contravariant / invariant measurement pass
    candidate_priority: InferenceCandidatePriority, // closed priority-ladder rung
    no_infer_mask: NoInferMask,                     // occurrence-local NoInfer suppression mask (§1.2)
    const_param_policy: ConstParamPolicy,           // <const T> propagation
    contextual_inference_mode: ContextualInferenceMode, // whether / how the contextual target drives inference
    // NO env / content / parse_stable_hash / fact_dep_signature / AST handle / borrowed session pointer / candidate vector (R6/R21)
}
```

Every field is a small closed enum, a mask, or an interned id of a SET of `TypeParamId`s — none carries a
`SemanticNodeData` body, a borrowed session pointer, an AST node, or an in-flight candidate set. The
**inferred candidate bodies live in the VALUE** (`bindings: Arc<[InferBinding]>` on `RelationPayload`),
version-rooted by `ReadSetSignature.facts`. Soundness: two sessions with the **same setup** over the same
`source`/`target` identity and the same content **must** produce the same inferred result, so the setup
fingerprint + the content fact rail is a complete identity. Different setups ⇒ different `InferenceContextKey`
⇒ distinct slots.

### 3.2 (R-b) `InferenceContextKey` is GENERATED from the `InferenceSession` setup-field set — MANDATORY

Hand-maintaining the `InferenceSession`→`InferenceContextKey` projection re-mints the false-completeness
defect: a future inference axis (a new priority rung, a new strict-affecting mask) added to the session but
forgotten in the fingerprint yields a **silent unsound warm-hit** (two semantically distinct sessions
sharing one slot) — the worst failure class. The fingerprint is **generated** by a `cargo run` target from
the canonical setup-field list and the Rust test only diffs (same discipline as the `SemanticQueryKeySpec`
table, the proof registry, the oracle rows). The generator projects every `#[setup_axis]`-tagged field
across **BOTH** `InferenceSession` (`no_infer_mask`, `contextual_inference_mode`, the inferable-param set)
**and** `InferenceInfo` (`priority`, `const_param_policy`, the variance side that becomes `variance_phase`),
since the fingerprint axes are sourced from both structs. Guard
`inference_context_key_projects_every_session_setup_axis` diffs generated-vs-tagged and fails on any
untagged-or-unprojected setup axis (owner RI-6).

### 3.3 (R-c) Binding-producing `Relate` is `ReturnOnly` until its session completes — MANDATORY

A binding-producing `Relate` (`inference_context = Some`) is a **session-internal delta and is `ReturnOnly`
for its entire in-flight session lifetime**; its `RelationPayload` admits **only at the instant its
enclosing session reaches `CompletedDeterministic`**, keyed by the now-knowable completed fingerprint.
Before completion the fingerprint is not even well-defined (the setup is still mutating), so a mid-session
admission would be unsound by construction.

**The SCC-close snapshot is PROVISIONAL; the published payload is the session-converged RE-DISCHARGE.** The
verdict / `bindings: Arc<[InferBinding]>` / `relation_proof` recorded for any deferred member at SCC-close is a
**caller-return value + deferral metadata, NEVER the published payload.** At the batched-publish instant (the
LATER of all relevant sessions' closes), the member's cold compute **COMPLETES**: the SCC is **RE-DISCHARGED
against the fully-converged state** — final inference bindings, final own verdict, final consumed-sibling
verdicts — through the **same `execute(Relate{Kᵢ})` dispatch** (this is the one engine's cold compute finishing
once its session inputs are final, NOT a second engine, NOT a second resolver). The published `RelationPayload`
(outcome + bindings + proof) **IS that re-discharge result**, keyed by the now-complete full §2.7 identity.
Therefore a stale SCC-close snapshot is **impossible by construction** — the published value is always the
converged re-evaluation.

**The admission predicate is a conjunction gated on the LATER event AND on a stable converged re-discharge.**
SCC-close (relation recursion converges) and session-completion (the fixation fixed-point converges) are two
**independent** axes — a binding member's relation SCC can close while its session is still `InProgress` — so
the predicate for a deferred member must require all of: SCC-close, session-completion, AND a stable converged
re-discharge, and fire at whichever happens last:

> `admit(Kᵢ) ⇔ (SCC closed POSITIVE ∨ SCC closed publishable-NEGATIVE) ∧ (every relevant session ==
> CompletedDeterministic) ∧ (the session-converged re-discharge of Kᵢ — against final bindings, final own
> verdict, and final consumed-sibling verdicts — yields a STABLE determined publishable outcome)`; the
> published `RelationPayload` **IS that re-discharge result** (positive ⇒ `Assignable` with final bindings + a
> complete `CoinductiveCycle { keys: S }` proof; publishable-negative ⇒ slotless `NotAssignable { reason,
> failing_sub }`, no slot-fill). Any **non-stable / undetermined / `Unknown` / `BudgetExceeded`** re-discharge —
> including the member's OWN verdict flipping, a consumed binding-sibling verdict flipping sign, OR a bindings
> change that alters the result — ⇒ `ReturnOnly` (release-without-publish, joiners recompute). If any relevant
> session ends `Abandoned(reason)` the conjunction never holds and the member is `ReturnOnly`. The consumed-
> sibling "same-sign" condition is a **named special case** of "the re-discharge yields a stable publishable
> outcome," not the whole gate.

For a non-binding member (`inference_context = None`) the member has no session of its own, but **SCC
composition AND verdict-dependency still govern its publish**: a non-binding member of a **pure non-binding
SCC**, and a **NEGATIVE** non-binding member of a mixed SCC **whose transitive consumed-verdict closure contains
no binding member**, publishes at SCC-close (the predicate collapses to SCC-close — no session gates it). A
**POSITIVE** non-binding member of a **MIXED SCC** carries the shared binding-referencing `CoinductiveCycle {
keys: S }` proof; a **NEGATIVE** non-binding member of a mixed SCC whose consumed-verdict closure reaches a
binding sibling consumed that sibling's not-yet-converged verdict; BOTH have their publish gated on the LATER
of {SCC-close, the last binding member's session-close} — for the positive member, exactly when its proof
becomes constructible; for the negative member, exactly when the consumed verdict converges (§2.3 step 3/4).
This is the explicit sequencing of the two axes: the SCC verdict is recorded **provisionally** at SCC-close (as
deferral metadata — into the `SessionAdmissionLedger` for binding members, held on the `SccLedger`'s deferred
batch for positive non-binding members AND for negative non-binding members whose consumed-verdict closure
reaches a binding sibling), and the published payload is gated on the relevant binding session(s) reaching
`CompletedDeterministic` **AND the session-converged re-discharge of the member yielding a STABLE determined
publishable outcome** — the published `RelationPayload` IS that re-discharge result, not the recorded snapshot
(§2.3 step 4). On any non-stable / flipped / abandoned re-discharge the deferred batch is released without
publish so the joiner recomputes (§2.3 step 4).

**The session-local *delta*, the provisional SCC-close *snapshot*, and the binding member's *published
re-discharge `RelationPayload`* are THREE DISTINCT objects.** The delta is the candidate deposit a binding
`Relate` makes into an `InferenceInfo` (admission-table row 7) — meaningful only within its session, **always
`ReturnOnly`**, never publishable. The SCC-close snapshot (verdict + bindings + provisional proof) is **deferral
metadata + the caller-return value**, never the published payload. The published `RelationPayload` is the
**session-converged re-discharge result** — the cold compute of the member completing once its session inputs
are final, through the same `execute(Relate{Kᵢ})` dispatch, keyed by the completed §2.7 identity. The three are
never conflated: the §2.3-step-4 SCC-close batched pass publishes only members whose verdict is complete and
stable at SCC-close (a pure non-binding SCC's members, and any NEGATIVE non-binding member whose consumed-verdict
closure contains no binding member); binding members, POSITIVE non-binding members of a mixed SCC, AND NEGATIVE
non-binding members whose consumed-verdict closure reaches a binding sibling — publish exclusively at the
relevant session-close, through the `SessionAdmissionLedger` / the `SccLedger`'s held batch, and only when the
§2.3 step-4 **converged re-discharge** yields a stable publishable outcome (any non-stable / flipped / abandoned
re-discharge releases the batch without publish so joiners recompute).

Because the completed `InferenceContextKey` does not exist while the session is in-flight, **cycle detection
and admission use two distinct identities for the same in-flight binding-`Relate`** (§2.2): while in-flight,
its reentry-stack node / recorded assumption is keyed by `(source, target, relation, policy,
source_freshness, context)` **+ the transient per-session `SessionId`** (content-free, never a cache key);
at the instant the session reaches `CompletedDeterministic`, the decided result is **re-keyed** to the full
§2.7 identity with the now-knowable completed `InferenceContextKey` substituted for the transient handle, and
only then admitted. The admitted entry is then reusable by a future request opening a session with the same
setup. Pure non-binding assignability (`inference_context = None`) never touches a session, has no transient
handle, and caches normally.

### 3.4 One inference engine, not per-surface matchers

Generic call inference, conditional `infer` extraction, reverse-mapped inference, contextual-callback
inference, overload applicability, and final substitution **all** run inside the session — there is no
second inference matcher. The `InferenceSession` is the cold-compute STATE of `execute`, not a standalone
engine.

---

## Decision 4 — Relation-proof acceptance (`RelationPayload`)

`SemanticQueryValue::Relation(RelationPayload)` is a landed forward-declared value arm. Its SHAPE is already
the rich struct below — `outcome` + `bindings` + an opaque `RelationProofId` into a payload-side four-shape
proof table — with no live producer yet. RI-2 **POPULATES** that shape from the relation reducer and performs
the wire migration; it does not add a brand-new value arm. (The historical tri-state display placeholder
`enum RelationPayload { Holds, DoesNotHold, Unknown }` has been retired — there is no public `Unknown`.)

```rust
struct RelationPayload {
    outcome: RelationOutcome,         // Assignable | NotAssignable | BudgetExceeded — the PUBLIC value-domain outcome (qvd §display_relation); NO Unknown
    bindings: Arc<[InferBinding]>,    // empty for non-binding / pure assignability / BudgetExceeded
    relation_proof: RelationProofId,  // opaque id into the payload-side relation_proofs table — OFF the type-values surface
}
enum RelationOutcome {
    Assignable,
    NotAssignable,
    BudgetExceeded(BudgetExceededKind),   // ReturnOnly-but-PUBLIC: expressible on the payload + rendered by display_relation, NEVER warm-admitted
}

enum RelationProof {                                                        // payload-side table, opaque RelationProofId
    Assignable       { witness: DerivationTree },                          // (1) structural derivation: which sub-relations held, variance arms, member-by-member
    NotAssignable    { reason: RelationFailureCode, failing_sub: SubRelationRef }, // (2) reason code + the failing structural sub-relation
    BudgetExceeded   { cap: RecursionOrBudgetCap },                        // (3) the budget / recursion cap that stopped the relate — rides a ReturnOnly-but-public BudgetExceeded payload, never warm
    CoinductiveCycle { keys: Arc<[RelateKeyId]> },                         // (4) the set of full Relate keys that co-discharged (§4.1)
}
```

**`SubRelationRef` / `failing_sub` is CONTENT-FREE (load-bearing for the negative carve-out).** `failing_sub:
SubRelationRef` is a `(source-node, target-node, sub-position)` descriptor — interned content-free
`SemanticNodeId`s plus the structural sub-position — that **EXCLUDES any session-bearing full `Relate` key** (no
`inference_context`, no transient `SessionId`, no `RelationContext`). A published `NotAssignable` proof therefore
never leaks a transient `SessionId`, which is exactly what lets a NEGATIVE member publish on the IDENTITY-leak
axis (§2.3 step 3); the VERDICT-dependency axis is gated separately by the consumed-verdict closure.

**Parent value-domain guard — NAMED, OWNED, reconciled (`relate_query_value_carries_relation_proof_and_budget_state`).**
The locked parent pins a value-domain guard `relate_query_value_carries_relation_proof_and_budget_state`
(native-typeinfo-parity.md:1074; also referenced in native-typeinfo-parity-cache-export-session.md:402, the
unified plan:2572, and native-typeinfo-parity-u2-reducers.md:200) asserting the public
`SemanticQueryValue::Relation(RelationPayload)` carries BOTH the relation_proof AND the budget state. This
design satisfies it in the **folded** shape: the public `RelationPayload` carries `relation_proof:
RelationProofId` AND the budget state — the latter typed INTO `outcome` as
`RelationOutcome::BudgetExceeded(BudgetExceededKind)`, NOT as a separate `budget_state` field. The fold is the
cleaner data model (single source of truth; no representable `Assignable`-with-exceeded-budget contradiction)
and matches the parent's own value-domain prose ("the public `RelationPayload` … plus a typed `BudgetExceeded`
non-admission", native-typeinfo-parity.md:1057; "carrying the proof + typed `BudgetExceeded`",
native-typeinfo-parity-cache-export-session.md:402) and the qvd three-valued `display_relation`
(`Assignable`/`NotAssignable`/`BudgetExceeded`). The guard's `_and_budget_state` clause is therefore satisfied
in **intent** — the budget state IS carried, as a typed outcome arm — and a future implementer / the parent
guard author should read the guard against the folded outcome, not a literal `budget_state` field. **Ownership:**
the guard is owned by **U2.QUERY_VALUE_DOMAIN** (the value-domain block that ships the `Relate` value-domain
SHAPE + the content-free `InferenceSession` projection shape, both qvd §2.2 — it may land there per the parent
and native-typeinfo-parity-cache-export-session.md:402);
**RI-2 (this block) exercises and satisfies it** when it upgrades the landed tri-state placeholder to the rich
`RelationPayload` carrying the relation_proof + the typed `BudgetExceeded` outcome. The warm subset stays binary
`Assignable`/`NotAssignable` + within-budget; `Unknown` stays OFF the public surface (unchanged).

**Public value-domain payload vs warm-admitted subset vs transient-only results — THREE layers, not two.**
The `RelationPayload` above is the PUBLIC value-domain value (`SemanticQueryValue::Relation`): its `outcome`
is the three-valued `Assignable` | `NotAssignable` | `BudgetExceeded` that `display_relation` renders (qvd
§display_relation). **Expressibility and warm-admissibility are DIFFERENT layers, and the locked value domain
requires both** (native-typeinfo-parity.md:1052 "the public `RelationPayload` … plus a typed `BudgetExceeded`
non-admission … a `BudgetExceeded` relation result is `ReturnOnly`"): a `BudgetExceeded` payload is fully
expressible and renderable, yet is `ReturnOnly` — never warm-admitted — enforced AT THE ADMISSION GATE (row
4), NOT by deleting the variant. Only the binary `Assignable`/`NotAssignable` subset is eligible for the
publish rows. The cold compute can ALSO bottom out in states that have **no public value-domain form at all**
— a deferred shell / opaque carrier (`Unknown`), or a verdict decided under an assumption that never
discharged (`OpenAssumption`). Those must be returned to the caller WITHOUT being forced to lie about an
`outcome`, so the cold compute returns a distinct transient enum whose `Undecided` arm carries exactly those
formless states; only its `Decided` arm carries a value-domain `RelationPayload`:

```rust
enum RelationComputeResult {                          // TRANSIENT — the return of the cold compute, NEVER cached as-is
    Decided(RelationPayload),                         // a PUBLIC value-domain payload (Assignable | NotAssignable | BudgetExceeded);
                                                      //   the GATE — not this arm — decides warmth: only Assignable/NotAssignable ADMIT (rows 13–15),
                                                      //   a BudgetExceeded outcome is ReturnOnly-but-public (row 4)
    Undecided(UndecidedReason),                       // NO public value-domain form — ReturnOnly-ONLY, never admitted, never published, never a fact signature
}
enum UndecidedReason {                                // states with no `SemanticQueryValue::Relation` surfacing
    Unknown(RecursionOrBudgetCap),                    // deferred shell / opaque carrier / recursion cap — NOT in the value domain (no public payload)
    OpenAssumption(RelateKeyId),                       // decided under an open coinductive assumption not yet discharged
}
```

The dispatch boundary maps `RelationComputeResult` onto the public `SemanticQueryValue::Relation` and the
admission table: a `Decided(payload)` surfaces `payload` (and the gate decides warmth — only an
`Assignable`/`NotAssignable` outcome reaches rows 13–15; a `BudgetExceeded` outcome is `ReturnOnly`-but-public
via row 4); an `Undecided(_)` has no value-domain form, so it surfaces the caller's provisional value and
matches a `ReturnOnly` row (1–12) by construction. **`BudgetExceeded` is NOT an `UndecidedReason` arm** — it
rides the public `Decided(RelationPayload{ outcome: BudgetExceeded, … })` so `display_relation` can render it,
while the gate keeps it ReturnOnly.

- **`Unknown` is NEVER on the public value surface.** A deferred shell / opaque carrier / undischargeable-`Unknown`
  obligation routes through `ReturnOnly` as `RelationComputeResult::Undecided(UndecidedReason::Unknown)`; it has
  no `SemanticQueryValue::Relation` form at all (qvd §display_relation renders only `Assignable`/`NotAssignable`/
  `BudgetExceeded` — no `Unknown`). This is the structural deletion of the current `RelationResult::Unknown`
  cache arm.
- **`BudgetExceeded` IS on the public value surface, but never warm.** A `RelationOutcome::BudgetExceeded`
  payload is a legitimate `SemanticQueryValue::Relation` that `display_relation` renders, yet it is `ReturnOnly`
  by the admission gate (row 4) — never warm-admitted, never backfilled, never a fact signature. Expressible
  and non-admitted are two layers; the locked value domain (native-typeinfo-parity.md:1052, qvd
  §display_relation) requires both.
- **`CoinductiveCycle { keys }` carries only COMPLETED keys (produced by the session-converged re-discharge).**
  The durable proof is NOT a remap of the provisional SCC-close snapshot's `keys: S` — it is the proof the
  **session-converged re-discharge** produces (§2.3 step 4 / §3.3). For a **pure non-binding SCC** every key in
  `S` is a non-binding key whose in-flight and completed identities coincide, so the proof is fully constructible
  and published **at SCC-close** (no re-discharge needed). For an SCC with any **binding** member, the binding
  member used the transient `SessionId` stand-in on the reentry stack (§2.2/§3.3 R-c) and its
  `InferenceContextKey` is not knowable until its session reaches `CompletedDeterministic`, so its slot is left
  **UNFILLED in the provisional snapshot at SCC-close**; the durable proof is produced at the **LATER of
  {SCC-close, the last binding member's session-close}** by re-discharging against the converged state — because
  every relevant session is `CompletedDeterministic` by then, the re-discharge's proof already references
  **completed full §2.7 keys** (the re-keying is intrinsic to the re-discharge, not a separate remap of a stale
  proof). The durable proof is published only when that re-discharge yields a STABLE positive — a non-stable
  re-discharge (a binding member converging to `NotAssignable`, the member's own verdict flipping, or its
  bindings changing) releases the positive batch WITHOUT publish (joiners recompute) rather than completing the
  proof. It therefore references ONLY completed full `Relate` keys, NEVER a transient `SessionId` — a proof
  referencing a transient session identity would be dangling/unsound.
- Proofs ride the **payload-side `relation_proofs` table by opaque proof id**. The END STATE keeps the proof
  **OFF the type-values surface**. NOTE the current tree does NOT yet satisfy this: `GraphTypeNode.kind`
  carries a live `GraphRelationProof relation_proof = 28` variant
  (`crates/verter_protocol/proto/verter/v1/typeinfo.proto:208`). **RI-2 RETIRES that tag-28 arm** and
  relocates the proof to the payload-side `relation_proofs` table referenced by opaque `RelationProofId`, so
  that after RI-2 there is **NO `GraphTypeNode` relation-proof arm** on the wire. (`GraphTypeNode` is the
  wire/proto surface, not a live Rust enum.) Guards `relation_proofs_not_graph_type_nodes` +
  `typeinfo_relate_payload_exposes_relation_proof_without_graph_type_node` land at RI-2 in their wire/proto-
  surface form, gated on the tag-28 retirement + wire migration below.
- **The proof is a descriptive witness, NOT a validity oracle.** Warm-hit validity is decided SOLELY by
  `ReadSetSignature.validate_with_self_roots` (plus `validated_at_generation` recency) against the caller's
  live `StoreView`. Consulting the proof for validity would be a forbidden **second validity rail**. On a
  warm hit the proof is returned verbatim; on a miss the whole entry recomputes.
- **Why a proof-carrying value can relate bidirectionally yet still be `ReturnOnly`-prone:** (i) a
  `CoinductiveCycle` decided under an open assumption is admissible only after its SCC closes cleanly — any
  `Unknown`/cancel/budget anywhere in the component makes the whole component `ReturnOnly` (R-a batched) — and
  a **binding** member additionally admits only at its enclosing session's close, so until then it is
  `ReturnOnly`, and a session that ends `Abandoned` drops it to `ReturnOnly` permanently (§2.3 step 4, §3.3);
  (ii) an `Unknown` obligation has no public value-domain form at all (it is `Undecided`), and a
  `BudgetExceeded` outcome — though PUBLIC and rendered by `display_relation` — is `ReturnOnly` by the
  admission gate; both are `ReturnOnly` by construction (the `BudgetExceeded` proof shape (3) rides the public
  but never-warm `BudgetExceeded` payload).

---

## Decision 5 — Consistency with landed CRITICAL invariants (second-engine forbiddances)

The relation engine remains ONE node of the single `SemanticGraphStore` dispatch.

**Target end-state (deferred to RI work — NOT the current tree).** There is to be exactly one relation
entry point: `ProjectSemanticDispatch::execute(SemanticQueryKey::Relate { full §2.7 identity })`, and the
bare-pair `relate_nodes(source, target)` is to be DELETED. An ergonomic caller-facing helper MAY then exist
**only** if it (i) takes or constructs the **full** `Relate` key (it must NOT re-expose a bare
`(source, target)` signature), (ii) owns **ZERO** memoization, cycle, assumption, or admission logic, and
(iii) is a pure delegation whose entire body is `execute(Relate { … })`. Such a helper is a constructor, not
an engine — it has no behavior to diverge. (No-dual-path / no-shim rule.)

**Current state.** The `execute(SemanticQueryKey::Relate)` family arm is intentionally degenerate — it owns
no relation logic and always yields `Opaque(Miss)`, fenced on the project generation. The live relation
entry point is still `ProjectSemanticDispatch::relate_nodes(source, target)`, which constructs the full
`RelateMemoKey` and owns warm lookup, the cycle guard, fact tracing, and relation-memo admission. The
`execute(Relate)` cutover above (folding that ownership into the family dispatch and deleting `relate_nodes`)
is later RI work, not this value-domain-shape block.

Every place this design *risks* a second relation engine / a query-time re-walk / a private matcher, and the
forbiddance:

| Risk site | Forbiddance / guard |
|---|---|
| Fast-reject discriminator prefilter (§4.1 perf) | O(tag) prefilter inside the relate cold path reading `SemanticNodeData` tags ONLY (primitive / literal / shape-tag / brand / arity mismatch); MUST NOT re-resolve / re-lower / walk types — it short-circuits to `NotAssignable` or falls through. Guard `relation_negative_and_unknown_paths_are_fast` (bench). |
| Reverse-mapped inference | a **relation-owned session pass**, NOT a private reverse-mapping matcher; each per-key recovery routes through binding-`Relate` into `InferenceInfo`; the session's final substitution reassembles `T`. Guard `reverse_mapped_inference_is_relation_owned_in_session`. |
| Per-property freshness spread-taint | lives IN the session/relation substrate (the excess-check consults the per-property bit), NOT a second checker. Guard `freshness_tracks_per_property_spread_taint`. |
| Identity-carrier unwrap (`unwrap_identity_carrier_for_relation`) | MUST route through `execute(Instantiate{…})` — the shared dispatch — never a private instantiation path. |
| `InferenceSession` | cold-compute STATE of `execute`, not a standalone engine. Guard `inference_runs_in_checker_transaction_not_per_surface_matcher`. |
| typed-IR-only | reads `SemanticNodeData` (typed IR) exclusively — no source text / regex / type-text splitters / `parse_type_annotation`. (Current code already complies.) |
| no query-time OXC | `Relate` never lowers TS source; OXC lowering happened once during shallow analysis. |
| R21 five split env | `RelationContext` carries `R/T/L/J` split, never a bundled `project_config_hash`. |
| R6 | the key carries no content / version / `fact_dep`; version rooting is on the value's `ReadSetSignature`. |
| persisted `Unknown` (the deleted bug) | `RelationOutcome` has no `Unknown` arm at all, and its `BudgetExceeded` arm is PUBLIC-but-never-warm (gated `ReturnOnly` at row 4); the `ReturnOnly` gate is the sole route for both. Guards `relation_cycle_sentinel_is_never_warm_admitted` + the admission-table rows. |

None of these weakens a landed invariant.

---

## Cache-admission decision TABLE (full discriminant set)

First matching row wins; `ReturnOnly` rows are checked **before** publish rows.

| # | Discriminant at the moment of would-be admission | Admission |
|---|---|---|
| 1 | relation **cycle sentinel** (`RelationAssumption::Holds`, open coinductive assumption) | **ReturnOnly** |
| 2 | **unconverged SCC** (assumption not yet discharged at the time of the read) | **ReturnOnly** |
| 3 | relation outcome is **`Unknown`** (deferred shell / opaque carrier / undecidable), **cyclic OR non-cyclic** — covers a non-cyclic `Relate` that bottoms out in `Unknown` AND any SCC member with an `Unknown` non-back-edge obligation | **ReturnOnly** (whole SCC if cyclic, R-a batched) |
| 4 | **`BudgetExceeded`** outcome (budget exhaustion: `RelationBudget` / `CallResolutionBudget` / `FlowSliceBudget`) — a `RelationOutcome::BudgetExceeded` payload is PUBLIC and rendered by `display_relation`, but matches here BEFORE the publish rows | **ReturnOnly** (public value, never warm) |
| 5 | obligation **cancelled** / generation-**superseded** mid-flight | **ReturnOnly** |
| 6 | **speculative / losing** inference-session candidate (non-winning overload attempt) | **ReturnOnly** |
| 7 | **session-local delta** (binding-`Relate` candidate deposit; overlay-only result) | **ReturnOnly** (no publish to base/persistent) |
| 8 | inference session **`Abandoned(reason)`** | **ReturnOnly** |
| 9 | inference session **`InProgress`** (not yet `CompletedDeterministic`) | **ReturnOnly** |
| 10 | **incomplete self-rooting** (torn / conflicting self-root observation; `None` carrier) | **ReturnOnly** |
| 11 | **unresolved provenance** (an SCC member's visited decl has no version root) | **ReturnOnly** |
| 12 | **overlay/session-only** identity attempting to publish to a base/persistent cache | **ReturnOnly** (session-cache identity only) |
| 13 | SCC closed **NEGATIVE** on a non-assumptive obligation ⇒ `NotAssignable` — for a **non-binding** member (`inference_context = None`) **whose transitive consumed-verdict closure contains no binding member** (publishes at SCC-close even in a mixed SCC); a non-binding member whose closure DOES reach a binding sibling, OR a **binding** member (`inference_context = Some`), reaches a publish only when every relevant enclosing session is ALSO `CompletedDeterministic`, else row 9 (`InProgress`) / row 8 (`Abandoned`) matches FIRST | **Cacheable** (publishable negative, final; published fact set = transitive consumed-verdict closure, NOT the bare per-member set — §2.3 step 3; a binding member — or a negative non-binding member whose closure reaches a binding sibling — publishes at session-close via the deferred batch / `SessionAdmissionLedger`, and the **published payload IS the session-converged re-discharge** — a slotless `NotAssignable` keyed by the completed §2.7 identity — NOT the recorded SCC-close snapshot; it publishes ONLY when that re-discharge yields a stable publishable negative (commonly: the consumed binding-sibling verdict converged to the SAME negative sign held at SCC-close); on session `Abandon`, OR if the re-discharge is non-stable (the sibling verdict FLIPPED, the member's own verdict flipped, or its bindings changed — so the snapshot is stale), the held registration is released WITHOUT publish and joiners recompute — §2.3 step 4) |
| 14 | SCC closed **POSITIVE**, all non-assumptive obligations positive ⇒ `Assignable` (+ `CoinductiveCycle` if cyclic) — admits at SCC-close ONLY for a member of a **pure non-binding SCC**. A **binding** member reaches here only when every relevant enclosing session is ALSO `CompletedDeterministic`, else row 9 (`InProgress`) / row 8 (`Abandoned`) matches FIRST. A **POSITIVE non-binding member of a MIXED SCC** carries the shared binding-referencing `CoinductiveCycle { keys: S }` proof whose binding slots are UNFILLED at SCC-close, so it does NOT admit at SCC-close either — it is held on the `SccLedger`'s deferred batch and admits at the LATER of {SCC-close, last binding member's session-close} | **Cacheable** at the publish instant (pure non-binding SCC → SCC-close, published directly; binding member, and positive non-binding member of a mixed SCC → session-close, via the `SessionAdmissionLedger` / the `SccLedger`'s deferred batch). The **published payload IS the session-converged re-discharge** — `Assignable` with the re-discharged final `bindings` + a `CoinductiveCycle` proof referencing only completed keys — NOT the recorded SCC-close snapshot; it admits ONLY when that re-discharge yields a stable publishable positive. **On session `Abandon`, OR if the re-discharge is non-stable (a consumed binding-sibling verdict FLIPPED to `NotAssignable`, the member's own verdict flipped, or its bindings changed — so the snapshot is a stale false-positive), the deferred batch does NOT reach this row:** it is released WITHOUT publish (no entry / fact signature / backfill) and drops to `ReturnOnly` (row 8), and any concurrent joiner held on the singleflight registration recomputes — §2.3 step 4 |
| 15 | **non-cyclic** `Relate` with a `Decided` **binary** `Assignable`/`NotAssignable` outcome (NOT `BudgetExceeded` — that matches row 4 first), `CompletedDeterministic` (or no) session, fully self-rooted, in-generation, non-overflowed, within budget | **Cacheable** (publish) |

Only rows 13–15 publish. Every cycle/SCC/budget/speculative/session/overlay/rooting state returns the
computed value WITHOUT admitting it. **Binding-member reconciliation (rows 7/8/9 vs 13/14):** a binding SCC
member (`inference_context = Some`) whose SCC has closed POSITIVE/NEGATIVE but whose enclosing session has not
yet reached `CompletedDeterministic` matches row 9 (`InProgress`) — or row 8 if the session is `Abandoned` —
BEFORE rows 13/14, so it is `ReturnOnly`/deferred at SCC-close and reaches a publish row only at session-close
through the `SessionAdmissionLedger` (§2.3 step 4); the session-local delta it deposited en route is row 7,
always `ReturnOnly` and a distinct object from the final re-keyed payload (§3.3). There is no contradiction —
the `ReturnOnly` rows being checked first is exactly what defers a binding member past SCC-close. **A POSITIVE
non-binding member of a MIXED SCC** has no session of its own, so no session row (7/8/9) defers it; it is
deferred instead by row 14's own gate — the shared `CoinductiveCycle { keys: S }` proof has UNFILLED binding
slots at SCC-close, so its publish instant is the LATER of {SCC-close, the last binding member's
session-close}, where the **published payload is the session-converged re-discharge** (NOT the SCC-close
snapshot); a non-stable re-discharge — a binding sibling converging to `NotAssignable` (a stale false-positive),
its own verdict flipping, or its bindings changing — releases the batch without publish so joiners recompute,
and at SCC-close it is held on the `SccLedger`'s deferred batch (NOT admitted). A NEGATIVE
non-binding member carries no shared proof, so on the identity-leak axis it never defers on composition; but it
publishes at SCC-close via row 13 **only when its transitive consumed-verdict closure contains no binding
member** — if that closure reaches a binding sibling, the consumed verdict is not yet converged (it may FLIP or
`Abandon`, and inference convergence is not a content edit the fact rail catches), so the negative member rides
the SAME deferred batch, and its published payload is likewise the session-converged re-discharge (publishing
the re-discharged slotless `NotAssignable` at the binding session's close ONLY when the re-discharge is a stable
publishable negative — commonly the sibling converging to the same sign held at SCC-close — or released-without-
publish on a non-stable re-discharge or on `Abandon`, with joiners recomputing). Row 3 enforces Decision 4's "`Unknown` is never on the warm OR public
surface": it precedes the publish rows and matches an `Unknown` obligation whether cyclic or non-cyclic, so a
non-cyclic `Relate` that bottoms out in a deferred shell / opaque carrier can NEVER fall through to row 15 —
the deleted `RelationResult::Unknown` memoization cannot re-enter via the table. Row 4 enforces the
complementary "`BudgetExceeded` is PUBLIC but never warm": a `RelationOutcome::BudgetExceeded` payload is a
legitimate `SemanticQueryValue::Relation` (rendered by `display_relation`) yet is `ReturnOnly` because row 4
precedes rows 13–15. Only a `Decided` result with a **binary** `Assignable`/`NotAssignable` outcome (P1-C /
Decision 4) is eligible for rows 13–15.

---

## Worked examples (coinductive cycle)

### Example A — positive recursive SCC: `interface A { next: A }` vs `interface B { next: B }`

Goal: `execute(Relate { source: A, target: B, relation: Assignable, … })`.

1. `relate(A, B)`: fast-reject sees two object surfaces with one member `next` each — no tag/arity mismatch
   → enter structural relate. Push `relate(A,B)` onto `reentry_stack`; open `SccLedger` for the root.
2. Structural obligation: member `next` requires `relate(A.next = A, B.next = B)` = `relate(A, B)` — **the
   same full identity already on the stack** → record assumption edge `relate(A,B) → relate(A,B)`, return
   sentinel `RelationAssumption::Holds`. The caller's accumulator gets `OpenAssumption(relate(A,B))`.
3. `relate(A,B)`'s structural descent completes. Its **only outgoing obligation is the `next` member
   relation, which is assumptive** (a back-edge). It has **zero non-assumptive obligations**.
4. SCC closure at the root: `S = { relate(A,B) }` (self-cycle via the symmetric `next` back-edge). "ALL
   non-assumptive obligations POSITIVE" holds **vacuously** (there are none) ⇒ **SCC closes POSITIVE.**
5. Admit (row 14): `RelationPayload { outcome: Assignable, bindings: [], relation_proof: CoinductiveCycle {
   keys: [id(relate(A,B))] } }`. `ReadSetSignature.facts` = { `MemberPresence(A,"next")`,
   `Member(A,"next")=A`, `MemberPresence(B,"next")`, `Member(B,"next")=B`, both shallow surfaces };
   `self_root_canonicals` = { file(A), file(B) }. Clear the `OpenAssumption` taint.
6. A future `relate(A,B)` request **warm-hits** (validate facts + self-roots). A content edit to file(A)'s
   `A` declaration misses the warm read and recomputes.

> Both members here are **non-binding** (`inference_context = None` — pure structural assignability, no
> inference session), so this is a pure non-binding SCC: publish happens at SCC-close (§2.3 step 4). Had a
> member been binding (`inference_context = Some`), its slot would stay unfilled at SCC-close and publish only
> at its session's `CompletedDeterministic` close through the `SessionAdmissionLedger`.

**Contrast with current code:** today step 2 returns `RelationResult::Unknown` and step 5 **caches the
`Unknown`** — a genuinely-recursive valid relation is permanently mis-decided. The new design publishes
`Assignable + CoinductiveCycle`.

### Example B — mixed SCC with one NEGATIVE obligation

```ts
interface A { next: B; tag: "a" }
interface B { next: A; tag: number }
```
Goal: `relate(A, B)` (is `A` assignable to `B`?).

1. `relate(A,B)`: push. Obligations: member `next` → `relate(A.next=B, B.next=A)` = `relate(B, A)` (new pair,
   recurse); member `tag` → `relate(A.tag="a", B.tag=number)`.
2. `relate(B,A)`: push. Obligations: member `next` → `relate(B.next=A, A.next=B)` = `relate(A,B)` — **on
   stack** → assumption edge `relate(B,A) → relate(A,B)`, sentinel `Holds`. member `tag` →
   `relate(B.tag=number, A.tag="a")`.
3. `relate(A.tag="a", B.tag=number)`: fast-reject — a string-literal `"a"` against primitive `number` is a
   **primitive-kind mismatch** ⇒ `NotAssignable` (O(tag), no recursion). This is a **non-assumptive
   obligation of `relate(A,B)` and it is NEGATIVE.**
4. SCC closure: `S = { relate(A,B), relate(B,A) }` (mutually assuming via `next`). Condensation-DAG
   bottom-up verdict:
   - `relate(A,B)` has a non-assumptive obligation (`tag`) that is **NEGATIVE** ⇒ `relate(A,B)` =
     **`NotAssignable`** (final, publishable — row 13). Proof `NotAssignable { reason: PrimitiveKindMismatch,
     failing_sub: relate("a", number) }`.
   - `relate(B,A)`'s `next` obligation depended on `relate(A,B)` which is now `NotAssignable` ⇒ the
     assumption collapses ⇒ `relate(B,A)` = **`NotAssignable`** (its `tag` obligation `relate(number, "a")`
     is also negative, reinforcing).
5. Admit both as publishable `NotAssignable` (row 13) — **not `ReturnOnly`.** Each carries its own
   `NotAssignable` proof. (Both members are non-binding and neither's transitive consumed-verdict closure
   contains a binding member, so SCC-close is the correct publish instant — §2.3 step 3. Had either consumed a
   binding sibling's verdict, that member would instead defer to the binding session's close.) Future requests
   warm-hit the negative.

> **Collapsed-back-edge fact set (P1-E).** Here `relate(B,A)`'s own `tag` obligation is also negative, so it
> would publish a correct fact set even per-member. But the general rule must cover the case where a member
> is `NotAssignable` SOLELY because a collapsed back-edge dragged it down — e.g. if `B.tag` had been `string`
> (so `relate(B.tag=string, A.tag="a")` is positive: `"a"` is assignable to `string`), `relate(B,A)`'s only
> negative input would be the collapsed `next` back-edge into the now-negative `relate(A,B)`. A bare
> per-member fact set for `relate(B,A)` would then NOT include the `A.tag="a"` fact that actually drove the
> verdict; editing `A.tag` to a `number`-compatible literal would flip `relate(A,B)` → `Assignable` and
> therefore `relate(B,A)` → `Assignable`, while a stale per-member warm negative for `relate(B,A)` still
> validates. The transitive consumed-verdict closure (§2.3 step 3) prevents this: `relate(B,A)`'s published
> fact set includes the facts of `relate(A,B)` (whose verdict it consumed through the back-edge), so the edit
> misses its warm read.

### Example B′ — same SCC but one obligation is `Unknown` ⇒ whole SCC `ReturnOnly`

Replace `A.tag: "a"` with `A.tag: KeyOf<SomeDeferredShell>` that the budget cannot reduce, so
`relate(A.tag, B.tag)` returns **`Unknown`** (a deferred shell, not a fast-reject mismatch). Now
`relate(A,B)` has a non-assumptive obligation that is `Unknown` ⇒ **row 3: the ENTIRE SCC
`{relate(A,B), relate(B,A)}` is `ReturnOnly`.** Neither member admits; both return their computed
(provisional) value to the caller without a warm entry, without a fact signature, without backfill. A
subsequent identical request recomputes (no warm short-circuit on `Unknown` — the deleted bug). This is the
R-a batched-poison residual perf risk (§7), and it is the *correct* soundness behavior.

---

## Implementation mini-DAG (RI-1 … RI-10)

Each sub-block is bounded, independently-landable, and lands its discriminating guard(s) **TDD-first in the
same change** (write RED → verify fail → implement → verify green — within the block, where the
implementation lands in the same change so the green `cargo nextest run --workspace` +
`cargo test -p verter_session --tests` gate is preserved per-block). Topological order; `{…}` parallelizable.
Deps are authoritative from the table.

```
RI-1 ──┬── RI-2 ──────────────────┐
       ├── RI-3 ──┬── RI-6 ──┬── RI-4 ──┬── RI-9
       │          │          │          └── RI-8
       │          │          └── RI-7
       │          └─────────────┘
       ├── RI-5
       └── RI-10
       (RI-1 → {RI-2,RI-3,RI-5,RI-10};  RI-6 ← RI-3;  RI-4 ← {RI-2,RI-3,RI-6};
        RI-7 ← RI-6;  RI-9 ← RI-4;  RI-8 ← RI-4 — printed once)
       (RI-4 ← RI-6 because RI-4's SccLedger deposits binding SCC members into RI-6's SessionAdmissionLedger
        at SCC-close, which RI-6 drains at session-close — §2.3 step 4)
```

| Sub-block | Deps | Deliverable | Discriminating guard(s) — TDD-first in-block | Legacy deletions |
|---|---|---|---|---|
| **RI-1** Full-identity `Relate` key | U2.QUERY_VALUE_DOMAIN | §2.7 key struct; `RelationKind`/`RelationPolicy`/`FreshnessKey`/`RelationContext`/`InferenceContextKey` types; `SemanticQueryKeySpec` row; re-key `BudgetedRelationMemo` bare→interned full identity; wire into per-family adaptive cap | `relate_key_covers_relation_kind_policy_freshness_and_context`; `relate_same_nodes_different_relation_kind_policy_or_env_do_not_warm_hit` | bare-pair `Relate` key; bare-pair `BudgetedRelationMemo` key |
| **RI-2** `RelationPayload` value-domain + proof table + wire migration | RI-1 | UPGRADE the landed tri-state `RelationPayload` placeholder to the rich struct; transient `RelationComputeResult`/`UndecidedReason` (Decision 4); payload-side `relation_proofs` table referenced by opaque `RelationProofId`; four `RelationProof` shapes. **Wire migration (Typeinfo Wire Contract):** relocate the proof OFF `GraphTypeNode` to the payload-side `relation_proofs` table; add `reserved 28;` + `reserved "relation_proof";` at `GraphTypeNode` message scope (proto3 forbids `reserved` inside the `oneof` — neighbour of the existing `reserved 33 to 100;`); bump `SemanticTypeGraph.schema_version` | `relation_proofs_not_graph_type_nodes` (wire/proto-surface form — RED today via live tag 28, lands GREEN here); `typeinfo_relate_payload_exposes_relation_proof_without_graph_type_node`; keep `typeinfo_graph_taxonomy` + `typeinfo_proto_ts_freshness` green; exercises/satisfies the parent value-domain guard `relate_query_value_carries_relation_proof_and_budget_state` (OWNED by U2.QUERY_VALUE_DOMAIN — the upgraded payload carries the `relation_proof` + the budget state typed into `RelationOutcome::BudgetExceeded`) | tri-state `enum RelationPayload { Holds, DoesNotHold, Unknown }`; `RelationResult` tri-state at the value boundary; **`GraphTypeNode.relation_proof = 28` / `GraphRelationProof` as a `kind` arm** (tag 28 reserved, name reserved, schema_version bumped) |
| **RI-3** `CheckerReentryStack` + `RelationAssumptionStack` + `CheckerTransaction` | RI-1 | transient cold-compute frame; scoped assumption recording keyed by full identity | `relation_cycle_assumptions_are_scoped_to_full_relate_identity`; `checker_reentry_stack_substrate_built_and_relate_wired` (the shared `CheckerReentryStack` substrate is built and **only `Relate`** — plus the `Instantiate{args:[], body_mode: Skeleton}` BFS it subsumes — is wired onto it as a typed view; testable at U2 with only the live variants. The full cross-engine span assertion is DEFERRED to the U6-owned `checker_reentry_graph_spans_flow_call_contextual_narrowing` — see Rescope/U6); retired-symbol: `relate_nodes(source,target)` / `RELATION_IN_FLIGHT` / `enter_/exit_relation_guard` cannot be re-introduced | `RELATION_IN_FLIGHT` thread-local; `enter_/exit_relation_guard`; bare-pair `relate_nodes` |
| **RI-4** Coinductive SCC discharge + SCC-composition-split admission + `ReturnOnly` gate | RI-2, RI-3, RI-6 | `SccLedger`; Tarjan over assumption edges; §2.3 discharge verdict; at SCC-close publish ONLY the members whose verdict is complete then — a **pure non-binding SCC**'s members, and any **NEGATIVE** non-binding member (no `keys: S`) **whose transitive consumed-verdict closure contains no binding member** — via the batched `FamilySlots::publish` pass, and DEFER the rest (every binding member, the **POSITIVE non-binding members of a MIXED SCC** which share the binding-referencing proof, AND any **NEGATIVE** non-binding member whose consumed-verdict closure reaches a binding sibling) by handing binding members (verdict + SCC fact-set) to RI-6's per-session `SessionAdmissionLedger` and holding the deferred non-binding members on the `SccLedger`'s batch (NO binding-member, NO mixed-SCC positive, and NO binding-consuming negative publish or proof-key remap at SCC-close); the SCC-close verdict/bindings/proof recorded for a deferred member is PROVISIONAL (caller-return + deferral metadata, NEVER the published payload); the deferred batch's session-close drain is a THREE-outcome gate (§2.3 step 4) whose **published payload IS the session-converged re-discharge** — when every relevant session is `CompletedDeterministic` it re-discharges each member through the same `execute(Relate{K})` dispatch against the converged state and publishes that result ONLY when it yields a STABLE determined publishable outcome (positive ⇒ `Assignable` + complete `CoinductiveCycle` proof; negative ⇒ slotless `NotAssignable`, NO slot-fill); on a non-stable re-discharge (own-verdict flip, consumed-sibling sign-flip, or bindings change) or on `Abandon`, releases WITHOUT publish (no entry / fact signature / backfill; joiners recompute) | `relation_coinductive_scc_discharges_on_outgoing_obligations`; `relation_cycle_sentinel_is_never_warm_admitted`; admission rows 1–5,13,14; exercises `binding_relate_scc_member_admits_only_at_session_close` (owned by RI-6) for the SCC-close→ledger hand-off | the memoized `RelationResult::Unknown` admission arm |
| **RI-5** Fast-reject prefilter + memo locality | RI-1 | O(tag) structural discriminators before structural relate; interned-id locality layout | `relation_negative_and_unknown_paths_are_fast` (bench) | — |
| **RI-6** `InferenceSession`/`SessionStack` + candidate combination + generated `InferenceContextKey` + completed-deterministic admission + `SessionAdmissionLedger` | RI-3 | §4.2 session substrate; closed `InferencePriority` ladder + combination rules; generated fingerprint (R-b); binding-`Relate` `ReturnOnly`-until-complete (R-c); the per-session `SessionAdmissionLedger` (on `CheckerTransaction`, keyed by transient `SessionId`) — populated by RI-4's `SccLedger` with the PROVISIONAL SCC-close snapshot (caller-return + deferral metadata, NEVER published verbatim) and DRAINED at session-close through a THREE-outcome gate whose **published payload IS the session-converged re-discharge**: when every relevant session is `CompletedDeterministic` it re-discharges each deferred member through the same `execute(Relate{K})` dispatch against the converged state (final bindings, final own verdict, final consumed-sibling verdicts), publishing that result ONLY when it is a STABLE determined publishable outcome — each deferred **POSITIVE** member as `Assignable` + a `CoinductiveCycle` proof referencing only completed keys (the re-keying intrinsic to the re-discharge, no separate remap), each deferred **NEGATIVE** member as a slotless `NotAssignable` with NO slot-fill; on a non-stable re-discharge (own-verdict flip, consumed-sibling sign-flip, or bindings change), OR on `Abandoned`, it drops the whole deferred batch to `ReturnOnly` and **releases the held singleflight registration WITHOUT publish** (no entry / fact signature / backfill), so any concurrent joiner recomputes | `inference_runs_in_checker_transaction_not_per_surface_matcher`; `only_completed_deterministic_sessions_are_admitted`; `inference_candidate_combination_matches_priority_and_variance`; `relate_same_nodes_different_inference_context_do_not_warm_hit`; `inference_context_key_projects_every_session_setup_axis` (R-b); `binding_relate_scc_member_admits_only_at_session_close` (deferred binding-member admission — RED-today, lands TDD-first here) | any standalone / per-surface inference matcher |
| **RI-7** Reverse-mapped inference pass + per-property freshness spread-taint | RI-6 | relation-owned session pass for homomorphic-mapped recovery; per-property freshness/taint algorithm | `reverse_mapped_inference_is_relation_owned_in_session`; `freshness_tracks_per_property_spread_taint` | any private reverse-mapping matcher |
| **RI-8** `CheckerReentryStack` substrate REUSE + `RefCycleResultDb` retirement (U2 scope) | RI-4 | CONSUME the RI-3 `CheckerReentryStack` substrate (RI-3 is the SOLE builder + `Relate`-wirer — RI-8 builds NO substrate and wires NO `Relate`): collapse the `Instantiate{args:[], body_mode: Skeleton}` ref-cycle BFS into the shared `Skeleton`-mode SCC over that substrate (the BFS becomes a `reentry_stack` walk) and demote the persistent boolean `ref_root_reaches_transitive_cycle_node` to a derived query-identity entry off the closed SCC; retire `RefCycleResultDb`. **Routing `FlowReturn`/`ResolveCall`/`ContextualTypeAt`/`FlowNarrowingAt` onto the substrate is DEFERRED to U6** (those variants land at U6). | `cross_engine_cycle_discharge_admits_only_stable_deterministic_results` (at U2 exercises a U2-available `Relate` ↔ `Instantiate`/`Skeleton` cross-engine cycle ONLY; the `FlowReturn`/`ResolveCall` cross-engine-discharge guard lands at U6); retired-symbol: `RefCycleResultDb` / `ref_root_reaches_transitive_cycle_node` / its `ComputeAdmission` BFS cannot be re-introduced | `RefCycleResultDb`; `ref_root_reaches_transitive_cycle_node` bespoke path (NOT the flow depth-sentinel — that retires at U6 with `FlowReturn`) |
| **RI-9** `RelationBudget` on full identity + three-layer non-admission | RI-4 | budget exhaustion → `BudgetExceeded` → `ReturnOnly` (no result / artifact / fact signature) | `relation_budget_exceeded_admits_nothing` | — |
| **RI-10** Strict-family behavioral branch (A.3 slice) | RI-1 | reducers BRANCH on the strict family; `type_env_hash` isolates strict-on/off | `reducers_branch_on_strict_family_not_only_key` | — |

**Deferred-guard ownership is the durable artifact of this gate.** No guard binary lands at the design gate
(§ "Now-landable-guard decision"); each guard above lands TDD-first inside its owning sub-block.

---

## Now-landable-guard decision

**ZERO new architecture guards and ZERO new CLAUDE.md `(CRITICAL)` headings land at this design gate.** The
three binding constraints:

1. Every guard that would *discriminate* against the current tree is a RED test (it FAILS on the current
   bare-pair / memoized-`Unknown` / process-global-guard tree). A RED test cannot land at this gate without
   breaking the green `cargo nextest run --workspace` gate. "Discriminating against the current tree" is
   exactly the property that makes such guards un-landable *now*; they are deferred to their owner sub-blocks
   and land green there (implementation in the same change).
2. The wire/proto guard `relation_proofs_not_graph_type_nodes` (asserting the relation proof is NOT a
   `GraphTypeNode.kind` arm) is **RED-discriminating TODAY**: the current tree carries a live
   `GraphRelationProof relation_proof = 28` variant on `GraphTypeNode.kind`
   (`crates/verter_protocol/proto/verter/v1/typeinfo.proto:208`), so the guard would FAIL on the current tree
   and could not land at this green gate. It is deferred to RI-2 **because it is a RED test** (it would break
   the green `cargo nextest run --workspace` gate until RI-2's tag-28 retirement + wire migration lands), NOT
   because it is a non-discriminating stub — the earlier "stub guard" justification was wrong (`GraphTypeNode`
   is the wire/proto surface, not a live Rust enum, but the proto variant it asserts against is real and
   present today). It lands GREEN at RI-2 alongside the wire migration, paired with
   `typeinfo_relate_payload_exposes_relation_proof_without_graph_type_node`, and must keep the existing
   wire-contract guards (`typeinfo_graph_taxonomy`, `typeinfo_proto_ts_freshness`) green.
3. No `(CRITICAL)` heading may land: a heading with no writable + registered guard trips the R6 meta-guard
   `every_critical_rule_in_docs_has_registered_guard`. This design lives in `docs/arch/u2-relation-infer-design.md`
   (a design doc, not a CLAUDE.md CRITICAL section), which does NOT trip R6. The relation-engine CRITICAL
   rules enter CLAUDE.md / the owning skill **at the implementation blocks**, each landing its three
   artifacts (doc heading + `CRITICAL_RULE_GUARDS` row + guard `#[test]`) together with the behavior.

The deferred-guard→owner registry (the mini-DAG table) is the gate's landable artifact.

---

## Residual risks (recorded)

1. **Batched-poison perf cliff (R-a).** One `Unknown`/cancel/budget edge anywhere in a large recursive SCC
   poisons the *whole* component to `ReturnOnly` (Example B′). This is *correct* for soundness (admitting any
   member of an undischarged component would warm a result decided under an open assumption) and is accepted,
   not a defect. Mandatory mitigations: fast-reject (RI-5) keeps most components tiny so they never enter SCC
   machinery; `RelationBudget` (RI-9) bounds the blast; the §6.2 fallback-entry-rate bench MUST specifically
   track recursive-type relate fallback rate. **Hard prohibition:** no future "partial admit" of a
   not-yet-discharged SCC member — that shortcut is the exact unsoundness the gate forbids; the RI-4 guards
   (`relation_cycle_sentinel_is_never_warm_admitted` + admission rows 1–5) hold the line.
2. **SCC-closure cost.** Tarjan over the assumption graph at every cycle close is O(edges) per component;
   pathological mutual recursion could make closure itself hot. Bounded by `RelationBudget`; interned-id
   locality (RI-5) keeps it cache-friendly. Bench-gated.
3. **Reverse-mapped inference + per-property freshness (RI-7)** are the highest-divergence-risk corner —
   complex semantics under perf pressure is exactly where a private matcher tends to sprout. The two guards
   forbid it structurally; this is the review corner to watch.
4. **`InferenceContextKey` completeness over future TS versions.** Even generated (R-b), a new TS inference
   axis not yet modeled as a `#[setup_axis]` field is invisible to the generator. Backstop: the differential
   `tsgo`-parity oracle (§6.3) surfaces a divergence as an oracle failure, not a silent unsound hit.
5. **RI-8 migration risk (U2 scope).** RI-8 reuses the shared `CheckerReentryStack` substrate (RI-3 is the
   SOLE builder + `Relate`-wirer; RI-8 builds no substrate and wires no `Relate`) and retires
   `RefCycleResultDb`; the retirement must preserve the existing strict self-root warm-read semantics (the BFS
   root file plus every visited declaration's file) inside the SCC's `ReadSetSignature.self_root_canonicals`.
   Routing the `FlowReturn`/`ResolveCall`/`ContextualTypeAt`/`FlowNarrowingAt` engines onto the substrate — and
   the flow depth-sentinel retirement — are DEFERRED to U6 (those variants land at U6), so they are NOT part of
   RI-8's risk surface. Scoped as its own sub-block with a retired-symbol guard; second only to RI-6/RI-7 in
   risk. Not free, correctly priced.
6. **Deferred binding-member admission (`SessionAdmissionLedger`) + mixed-SCC positive deferral + duplicate
   in-flight binding recompute.** A binding-`Relate` SCC member whose SCC closes POSITIVE/NEGATIVE but whose
   enclosing session never reaches `CompletedDeterministic` — it ends `Abandoned` (cancel / budget / superseded
   / non-deterministic) — has its decided relation verdict **dropped to `ReturnOnly`** and never published, even
   though the relation recursion itself converged. This is the *correct* soundness behavior — admitting a
   binding member before its session converges would warm a result keyed on a not-yet-final
   `InferenceContextKey` — and it mirrors the batched-poison residual (risk 1): work that did not converge on
   every axis is recomputed, never warm-admitted. **The deferral is per-SCC-composition AND
   per-verdict-dependency, NOT purely per-member:** in a **mixed SCC** (≥1 binding member) the POSITIVE
   non-binding members defer too — they carry the shared binding-referencing `CoinductiveCycle { keys: S }` proof,
   which is not constructible (and so not publishable) until every binding slot is filled at the last binding
   member's session-close — AND a **NEGATIVE** non-binding member defers when its transitive consumed-verdict
   closure reaches a binding sibling: that sibling's SCC-close verdict is provisional until session-close (it may
   FLIP to `Assignable` or `Abandon`, and inference convergence is not a content edit the fact rail catches), so
   warm-publishing the negative at SCC-close would warm a verdict that did not converge on every axis (the
   stale-false-negative class, the complement of the positive defect round 5 fixed). **The SCC-close
   verdict/bindings/proof recorded for any deferred member is PROVISIONAL — caller-return + deferral metadata,
   NEVER the published payload; the published payload is the session-converged RE-DISCHARGE.** The session-close
   drain is a THREE-outcome gate (§2.3 step 4): when every relevant session reaches `CompletedDeterministic` it
   re-discharges each deferred member through the same `execute(Relate{K})` dispatch against the converged state
   (final bindings, final own verdict, final consumed-sibling verdicts) and publishes that re-discharge result
   ONLY when it yields a STABLE determined publishable outcome (a stale snapshot is impossible by construction).
   If any relevant session is `Abandoned`, every held member of that deferred batch — binding members, POSITIVE
   non-binding members, AND binding-consuming NEGATIVE non-binding members — drops to `ReturnOnly`, the held
   singleflight registration is **released WITHOUT publish** (no entry / fact signature / backfill), and any
   concurrent joiner recomputes (§2.3 step 4). Symmetrically, if every relevant session reaches
   `CompletedDeterministic` but the re-discharge is **non-stable** — the member's OWN verdict flips (even with no
   consumed sibling), a consumed binding-sibling verdict FLIPS sign (a held positive member's sibling converging
   to `NotAssignable`, or a held binding-consuming negative member's sibling converging to `Assignable`), or the
   converged `bindings`/`relation_proof` differ from the snapshot — the deferred batch is **likewise released
   WITHOUT publish** so the joiner recomputes against the converged state (§2.3 step 4) — the SAME
   release-without-publish exit, on the convergence axis rather than the abandonment axis (the stale-false-positive
   / stale-false-negative / stale-bindings class, all subsumed by the single re-discharge gate). The
   consumed-sibling "same-sign" check is a named special case of a stable re-discharge, not the whole gate. The
   blast is still bounded: a **pure non-binding SCC**
   publishes entirely at SCC-close, and a
   **NEGATIVE** non-binding member whose consumed-verdict closure contains no binding member (proof
   `NotAssignable`, no `keys: S`) publishes at SCC-close even in a mixed SCC.
   **Duplicate in-flight binding recompute (PERF, not unsoundness).** A binding member offers no mid-flight
   cross-transaction singleflight join — its transient `SessionId` is private to its transaction and its final
   §2.7 key does not exist until session-close (§2.3 step 4) — so a second top-level transaction needing the
   same binding `Relate` mid-flight opens its OWN inference session and recomputes; both converge to the same
   completed `InferenceContextKey` and the first publish wins the slot (the other validates/joins on it). This
   is a bounded, deterministic perf cost — not a hang or unsoundness. The §6.2 fallback-entry-rate bench tracks
   binding-`Relate` deferral/drop rate, mixed-SCC positive-non-binding deferral rate, AND duplicate in-flight
   binding-recompute rate alongside the recursive-relate fallback rate.

---

## Rescope / consumers

- **U2.QUERY_VALUE_DOMAIN** ships the `Relate` full-identity SHAPE + `InferenceSession` projection SHAPE
  (both qvd §2.2; the `CheckerTransaction` / `InferenceSession` substrate is parent §4.2). THIS block
  (U2.RELATION_INFER) owns the substrate (the persistent cache category, the
  coinductive dispatch primitive, the session admission incl. the per-session `SessionAdmissionLedger` that
  defers binding-member admission to session-close, the proof table) OVER those shapes.
- **U3.CACHE_FACT_MODEL** owns the per-family adaptive cap + multi-candidate substrate RI-1 wires `Relate`
  into (§6.1), and proves the `ReadSetSignature` tracer captures relation footprints.
- **U6** consumes the shared `CheckerReentryStack` substrate RI-3 builds (RI-8 reuses; RI-8 wires no
  `Relate`), and **routes** the
  `FlowReturn`/`ResolveCall`/`ContextualTypeAt`/`FlowNarrowingAt` engines onto it (those variants + spec rows +
  behavior land at U6 per native-typeinfo-parity.md:507 / qvd:942-948 — RI-3 wires only `Relate`; RI-8 wires
  nothing onto the substrate). The flow
  depth-sentinel retires here too (it depends on the U6 `FlowReturn` view of `reentry_stack`). The
  contextual-callback iterative generic inference loop is the session's fixation fixed-point. **U6 OWNS the
  deferred cross-engine span guard `checker_reentry_graph_spans_flow_call_contextual_narrowing`** — it asserts
  the one `CheckerReentryStack` spans `Relate`/`FlowReturn`/`ResolveCall`/`ContextualTypeAt`/`FlowNarrowingAt`
  as typed views and lands TDD-first at U6 when those engines (and their enum variants/spec rows) wire onto the
  substrate; it CANNOT land at U2 because those variants are not pre-registered there (the standing
  `semantic_query_key_spec_table_equals_enum` meta-guard rejects any U2 tree referencing them). RI-3's U2 guard
  `checker_reentry_stack_substrate_built_and_relate_wired` covers the U2-live subset (substrate built + `Relate`
  wired).
- **U8 / wire** consumes the payload-side `relation_proofs` table off the type-values surface (RI-2).
- **U10.RESULT_DB** stores `Relate` candidates; the demand-lattice exactness publish gate must stay green
  under the multi-candidate relation dominance.
