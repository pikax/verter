# U2.QUERY_VALUE_DOMAIN — Typed `SemanticQueryValue` value-domain foundation (LOCKED design)

> Block: `U2.QUERY_VALUE_DOMAIN` (`docs/arch/native-typeinfo-parity-u2-reducers.md` L163–229).
> Parent architecture: `docs/arch/native-typeinfo-parity.md` §2 (query keys), §3 (value
> domain), §4 (relation/inference), §4.5 (apparent types), §5 (flow), §6 (budgets / non-admission).
> This doc EXTENDS/FORMALIZES the parent; it never contradicts it. Where the parent already
> fixes a shape (§2.10 the two demand structs + five presets; §3 the `SemanticQueryValue` enum;
> §2.1–2.9 the per-key shapes) we cite the section and only add the algebra/algorithm depth a
> STAGE B implementer needs.
>
> **Scope.** This is a DESIGN GATE. It locks the SHAPE + algebra of the value domain, the
> demand lattice, the per-key identity, display projection, error tolerance, module-resolution
> keying, and the error/any/never propagation lattice. Reducer BEHAVIOR, the inference-session
> substrate, the demand-exactness publish gating, and `FlowReturn`/`ResolveCall` variants land in
> later blocks (U2.RELATION_INFER, U3.CACHE_FACT_MODEL, U6, U10.RESULT_DB) and are referenced,
> not designed, here.
>
> **Model facts this design is built against (verified in tree).**
> `ResolvedDeclSlotIdentity` (in `semantic_query.rs`) is the 6-field ENV-BEARING, content-free
> slot: `{defining_canonical, merged_symbol_name, symbol_space: SemanticSymbolSpace,
> project_identity: u32, type_env_hash, lib_env_hash}` — it carries type+lib+project for the
> declaration SITE but NEITHER `resolve_env_hash` NOR `parse_env_hash`. `Instantiate{base}` /
> `ResolveMacroPayload{owner}` (`SemanticQueryKey` in `semantic_query.rs`, mirrored on `FamilyKey`
> in `semantic_query_memo/family.rs`) key on the
> env-bearing, content-free `ResolvedDeclSlotIdentity` (the `Instantiate`/`ResolveMacroPayload`
> base/owner are always `symbol_space = Type`); the extra `resolve_env_hash` rides on the
> per-key `InstantiateContext` / `MacroPayloadContext`. The relation key is the full
> `RelateMemoKey` (in `semantic_query.rs`):
> `{source, target, relation: RelationKind, policy: RelationPolicy, source_freshness: FreshnessKey,
> inference_context: Option<InferenceContextKey>, context: RelationContext}`, where
> `RelationContext` (in `semantic_query.rs`) carries the `R T L J` env dims (`resolve_env_hash`,
> `type_env_hash`, `lib_env_hash`, `project_identity`) plus `substitution` + `projection_reduction`;
> the public relation result is `SemanticQueryValue::Relation(RelationPayload)` (in
> `semantic_query.rs`). `ProjectionMode` is the coarse five-rung enum; `ReductionDemand` is
> Published/StructuralTransit/MacroObjectSurface; `ModeSlot` (in `semantic_query_memo/family.rs`) is
> the per-slot selector. `SemanticSymbolSpace` is `Type|Value|Namespace` (U2B.5 added `Namespace` as
> a real slot discriminator). `QueryError` carries `Miss / UnsupportedIntrinsic /
> BudgetExceeded / UnstableState / AliasCycle / RecursiveRef / Other / DeclPlaceholder`;
> `SemanticNodeData::Opaque(QueryError)` (in `semantic_query.rs`) is the boundary error carrier.
>
> **Superseded model (FORBIDDEN).** The `feat/u2a-semantic-key-cutover` env-LESS uniform
> envelope (`SemanticQueryEnvKey`/`TypeLibEnvKey`, the `SemanticGraphCacheKey{family,slot,env}`
> split) is NOT a model here. Env stays ON the key via per-key `*Context` structs (R21 per-layer).
> The env-LESS `GraphDeclSlotRef` query-identity wire slot it carried HAS BEEN RETIRED (deleted from
> the proto and the `verter_protocol` typed surface): the typeinfo query identity now rides the
> env-BEARING `GraphResolvedDeclSlotIdentity` (`GraphQueryIdentity.resolved_roots`, tag 18) and
> `TYPEINFO_GRAPH_SCHEMA_VERSION` is bumped to `2` for that retirement — the env-bearing direction
> this model endorses, NOT a return to the env-less envelope. We mine exactly three ideas from the
> superseded model, re-expressed on the env-bearing model — flagged inline as
> `[mined: class-dual-space]`, `[mined: merge+aug-rail]`,
> `[mined: producer-discriminator + SymbolSpace::Namespace]`.

---

## 0. Notation and the two-axis cache-identity model

Every query-identity key in this design is `(IdentityCore, EnvProjection, DemandAxis)` where:

- **`IdentityCore`** — the content-free *what* (a `ResolveDeclKey`, a `ResolvedDeclSlotIdentity`, a
  `ProgramPointId`, a `(source,target,RelationKind,…)` tuple). Carries NO content hash, NO
  `parse_stable_hash`, NO `fact_dep_signature` (**R6**).
- **`EnvProjection`** — the per-key `*Context` struct naming ONLY the split env dimensions the
  value depends on (subset of `{parse_env, resolve_env, type_env, lib_env, project_identity}`),
  never a bundled `project_config_hash` (**R21**).
- **`DemandAxis`** — the `(ProjectionDemand, EvalPolicy)` point (§3 here) plus non-env query axes
  (`SubstitutionCanonicalHash`, `FlowNarrowingKey`, `ContextualTypingKey`, `InferenceContextKey`,
  `RelationKind`, `FreshnessKey`). All canonicalized before key construction (§3.6).

Version-rooting lives EXCLUSIVELY on the cached VALUE via the multi-candidate `FamilySlots`
substrate: each candidate carries `ReadSetSignature.facts` + `self_root_canonicals`, validated on
every warm read (`validate_with_self_roots` in `fact_signature_helpers.rs`). The key never roots
versions.

We write `⊑` for "is dominated by / less-or-equal-demand", `⊔` lub/join, `⊓` glb/meet, `⊤` top,
`⊥` bottom. `a ⊒ b ≜ b ⊑ a`. "Cached point `c` SATISFIES request `r`" ≜ `c ⊒ r` under a compatible
policy (§3.4).

---

## Deliverable #3 — `ProjectionDemand × EvalPolicy` lattice algebra

Parent §2.10 fixes the two structs and the five presets and states "satisfaction by dominance".
We formalize the full algebra: the per-field orders, the product order, `join`/`meet`, the
satisfaction + backfill relations, monotone path composition, and minimality/normalization.

### 3.1 Per-field lattices

Each field of `ProjectionDemand` / `EvalPolicy` is itself a small lattice. The product (§3.2)
inherits its order componentwise. We give each field's order, its `⊥`, its `⊤`, and whether `join`
and `meet` are total.

**`ProjectionDemand` fields.**

| Field | Domain | Order `⊑` (lower = less demand) | `⊥` | `⊤` | join/meet |
|---|---|---|---|---|---|
| `path: ProjectionPath` | interned prefix-shared path | **prefix order**: `p ⊑ q` iff `p` is a prefix of `q`. (A demand for `A.b.c` strictly demands MORE drilling than `A.b`.) | `[]` (empty) | (no finite top — unbounded paths) | join = longest-common-extension iff one is a prefix of the other, else **incomparable** (no lub); meet = longest common prefix (ALWAYS exists) |
| `facets: SurfaceFacetSet` | `BitSet<SurfaceFacet>` (members / index-sigs / heritage / call / construct / …) | **subset order** `⊆` | `∅` | full set | join = `∪`, meet = `∩` (both total) |
| `member_demand: MemberBodyDemand` | `{SetOnly, SetPlusBody}` | `SetOnly ⊑ SetPlusBody` | `SetOnly` | `SetPlusBody` | join = max, meet = min (total) |
| `call_signatures: bool` | `{false,true}` | `false ⊑ true` | `false` | `true` | ∨ / ∧ (total) |
| `construct_signatures: bool` | same | same | `false` | `true` | total |
| `index_signatures: bool` | same | same | `false` | `true` | total |
| `display_needs: DisplayNeeds` | `BitSet<DisplayFacet>` (§Deliverable #14) | subset order | `∅` | full | join `∪`, meet `∩` (total) — **NOT a typed-value semantic axis. Carried ONLY on the display/publish projection key; MASKED OUT of every typed-value family key (§14 invariant). It is NOT part of the regime tuple, so it never induces incomparability.** |

**`EvalPolicy` fields.**

| Field | Domain | Order `⊑` | `⊥` | `⊤` | join/meet |
|---|---|---|---|---|---|
| `alias_preservation: AliasPreservation` | `{Keep, Inline}` | `Keep ⊑ Inline` (inlining the alias body is strictly MORE evaluation than keeping the `Ref`) | `Keep` | `Inline` | total (max/min) |
| `normalization_depth: NormalizationDepth` | `{None, NavigateOnly, Terminal, Deep}` | total chain `None ⊑ NavigateOnly ⊑ Terminal ⊑ Deep` | `None` | `Deep` | total (max/min) |
| `generic_open: GenericOpenPolicy` | `{Bound, TypeParamShells}` | **INCOMPARABLE** — two distinct evaluation REGIMES, not depths (see §3.1.1) | — | — | **join PARTIAL (no lub)**; meet PARTIAL (no glb) |
| `operator_reduction: OperatorReduction` | `{Leave, NavigateOnly, Reduce}` | total chain `Leave ⊑ NavigateOnly ⊑ Reduce` | `Leave` | `Reduce` | total |
| `carrier_stop: CarrierStopPolicy` | `{StopAtCarrier, Continue}` | `StopAtCarrier ⊑ Continue` (continuing past a carrier is strictly more work) | `StopAtCarrier` | `Continue` | total |
| `surface_role: SurfaceRole` | `{Plain, Prop, Emit, Model, Slot, Option}` | **flat (antichain except `x ⊑ x`)** — a role is a structural discriminator, not a depth | each is both `⊥` and `⊤` of its singleton | — | join/meet PARTIAL: defined only for equal roles; distinct roles incomparable |
| `provenance: ProvenanceNeed` | `{Drop, Retain}` | `Drop ⊑ Retain` | `Drop` | `Retain` | total |
| `merge_role: MergeRole` | `{Standalone, Heritage, WithDefaults, IntersectionArm}` | **flat antichain** (structural discriminator) | — | — | join/meet PARTIAL (equal-only) |

#### 3.1.1 Why `generic_open` is INCOMPARABLE (stated, not papered over)

`Bound` instantiates type parameters with their bound substitution arguments; `TypeParamShells`
leaves unbound parameters as `TypeParam` shells so a `Conditional` over an unbound generic does
NOT collapse to `never`. These produce *structurally different result graphs for the same input* —
a `Bound` result has no `TypeParam` nodes, a `TypeParamShells` result does. Neither is "more
evaluated" than the other; they answer different questions. Therefore:

- `Bound` and `TypeParamShells` are an **antichain** in the `EvalPolicy` order.
- A `Skeleton` point (`generic_open = TypeParamShells` + `carrier_stop = StopAtCarrier`) is
  **incomparable** to every `Bound` expansion point. This is the parent §2.10 "two incomparable
  points never satisfy each other" rule given its formal cause: the incomparability is inherited
  from this single field. `Skeleton` is NOT "below" `Expanded`; it is off to the side.

Consequence for the lattice: the product is **NOT a full lattice** — it is a poset that is a
**bounded meet-semilattice on the comparable sublattice** with the two antichain fields
(`generic_open`, `surface_role`, `merge_role`) partitioning it into incomparable strata. Within one
stratum (fixed antichain-field values) it IS a bounded lattice. We make this precise next.

### 3.2 The product order and the stratified lattice

`Demand ≜ (ProjectionDemand, EvalPolicy)`. The product order is componentwise:
`d₁ ⊑ d₂ ⟺ ∀ field f: d₁.f ⊑ d₂.f`.

Partition `Demand` by the **regime tuple** `R(d) ≜ (d.generic_open, d.surface_role, d.merge_role)`
(the three antichain fields). Two points are comparable ONLY IF `R(d₁) = R(d₂)`. Define the
per-regime subposet `Demand_R ≜ { d | R(d) = R }`. **`display_needs` is explicitly NOT part of the
regime tuple** — it is display-only (fix below) and never induces incomparability; two demands that
differ ONLY in `display_needs` share a regime and a typed-value slot (the family key masks it out).

- **Within a regime `Demand_R`**: every field is a totally-ordered or subset lattice EXCEPT `path`
  (prefix order — a meet-semilattice with total meet but partial join). So `Demand_R` is a
  **bounded meet-semilattice**: `meet` is total (componentwise, path → longest common prefix);
  `join` exists iff the two `path`s are prefix-comparable (else the join is undefined within the
  regime). `⊥_R = Identity-shaped point of regime R`; there is no single finite `⊤_R` because
  `path` is unbounded, but every bounded request has a well-defined `meet` with any cached point.

- **Across regimes**: incomparable. `meet`/`join` undefined (return `None`).

**Closure statement (confirmed resolution):**
`(Demand, ⊑)` is a **poset that is the disjoint union (product) of per-regime bounded
meet-semilattices, where join is partial (path-prefix-gated) even within a regime.** `meet` is total
within a regime
and undefined across regimes; `join` is partial both within (path) and across (regime) regimes.
This is sufficient for the cache algebra: satisfaction needs only `⊒` (a comparison, always
decidable), and backfill needs only `meet` (total within the regime a candidate and request share,
the only case where backfill is even attempted). We never need an arbitrary `join`, so the absence
of a total join is not a defect — it is the correct shape (two unrelated demands have no common
super-demand worth computing).

`⊤` (conceptual): `Expanded`-regime point with `facets = full`, `member_demand = SetPlusBody`, all
sig bools `true`, `alias_preservation = Inline`, `normalization_depth = Deep`,
`operator_reduction = Reduce`, `carrier_stop = Continue`, `provenance = Retain`, and the
*requested* `path` (top is path-relative). `⊥`: the `Identity` preset (empty path, `Keep`, no
member/body demand).

### 3.3 `join` / `meet` definitions (per-field, with partiality)

```
fn meet(a: Demand, b: Demand) -> Option<Demand> {
    if regime(a) != regime(b) { return None; }            // cross-regime: no glb
    Some(Demand {
        // ProjectionDemand
        path:               longest_common_prefix(a.path, b.path),     // total
        facets:             a.facets & b.facets,                       // ∩
        member_demand:      min(a.member_demand, b.member_demand),     // SetOnly if either
        call_signatures:    a.call_signatures && b.call_signatures,
        construct_signatures: a.construct_signatures && b.construct_signatures,
        index_signatures:   a.index_signatures && b.index_signatures,
        display_needs:      a.display_needs & b.display_needs,
        // EvalPolicy (regime fields equal by precondition)
        alias_preservation: min(a.alias_preservation, b.alias_preservation), // Keep wins
        normalization_depth: min(a.normalization_depth, b.normalization_depth),
        generic_open:       a.generic_open,    // equal by regime
        operator_reduction: min(a.operator_reduction, b.operator_reduction),
        carrier_stop:       min(a.carrier_stop, b.carrier_stop),       // StopAtCarrier wins
        surface_role:       a.surface_role,    // equal by regime
        provenance:         min(a.provenance, b.provenance),
        merge_role:         a.merge_role,      // equal by regime
    })
}

fn join(a: Demand, b: Demand) -> Option<Demand> {
    if regime(a) != regime(b) { return None; }
    let path = prefix_join(a.path, b.path)?;   // Some(longer) iff one is a prefix of the other
    Some(Demand { path, facets: a.facets | b.facets, member_demand: max(..), .. /* max/∨/∪ dually */ })
}
```

`meet` is **well-defined and total within a regime** (each line is a total operation on its field;
`longest_common_prefix` always exists). `join` is partial via `prefix_join` (returns `None` when
paths diverge) and via the regime guard. We prove `meet` is the glb within a regime by the standard
product-of-meet-semilattices argument: a componentwise meet of lower bounds is the greatest lower
bound when each component meet is the glb of that component (true for total chains, subset `∩`, and
`longest_common_prefix` on the prefix order). ∎

### 3.4 Satisfaction (warm-hit) and backfill — the MATERIALIZED-RECORD rule

The danger this section closes: a deep terminal projection (e.g. `A['c']['full']['bar']` expanded)
could, under a naive `cached ⊒ requested` demand test, *dominate* a shallower terminal surface
(e.g. `A['c']` shallow) that it NEVER materialised — it only walked `c` in `Navigate` on its way to
`bar`. Demand dominance over the requested point is NOT proof of materialisation. So satisfaction is
NOT defined against the candidate's nominal request demand; it is defined against the SET of points
the candidate ACTUALLY materialised.

**`satisfied_projection` = the set of actually-materialised `(path, point)` records.** A cached
candidate carries a `satisfied_projection: MaterializedSet` — the concrete record of what its
compute actually produced, NOT its requested demand:

- the **terminal projection point** `(terminal_path, terminal_point)` — the leaf the compute
  expanded under the caller's demand;
- PLUS one **prefix `Navigate` hop record** `(prefix_path, NAVIGATE_PRESET.with_regime(...))` for
  EACH intermediate hop the compute ACTUALLY walked (§3.5).

A compute that walked `c`→`full`→`bar` (expanding only `bar`) records:
`{ (["c"], Navigate), (["c","full"], Navigate), (["c","full","bar"], terminal_point) }`. It does
NOT record `(["c"], terminal_point)` — it never expanded `c`.

```
struct MaterializedPoint { path: ProjectionPath, point: Demand }   // point is regime-tagged
struct MaterializedSet(Arc<[MaterializedPoint]>);                  // recorded by the compute itself

// A warm hit serves `requested` iff SOME recorded materialised point dominates it.
fn cached_satisfies(satisfied: &MaterializedSet, requested: &MaterializedPoint) -> bool {
    satisfied.0.iter().any(|m|
        m.path == requested.path          // SAME path node (prefix-interned id equality)
        && m.point >= requested.point     // demand dominance at that path (§3.2 componentwise;
                                          // regime fields EQUAL — incomparable regimes never satisfy)
    )
}
```

The path must MATCH (interned-id equality), not merely be a prefix: a recorded `(["c"], Navigate)`
hop satisfies a request for `A['c']` ONLY under a `Navigate`-or-below demand at that exact path; it
does NOT satisfy `A['c']` *expanded*, because the deep compute never expanded `c`. A request whose
`(path, point)` is dominated by NO recorded materialised point is a MISS, even if the candidate's
nominal terminal demand would dominate it under §3.2. This is the soundness fix: dominance is tested
against MATERIALISED points, never against a computed `meet(terminal, request)` or the nominal
request.

A warm hit is served iff `cached_satisfies` AND the candidate's `ReadSetSignature` validates against
the caller's live view (`validate_with_self_roots` in `fact_signature_helpers.rs`). Demand
dominance over a materialised point and fact validity are **two independent gates**; both must pass.

**Backfill rule (broader → narrower) — RECORDED points only.** When a broader compute completes, it
backfills a narrower slot ONLY with the points it ACTUALLY materialised — the `MaterializedPoint`
records above, verbatim. It never synthesises a `meet(terminal, request)` point it did not compute:

```
// Backfill writes the RECORDED materialised points, not a meet-derived one.
fn backfill_points(satisfied: &MaterializedSet) -> &[MaterializedPoint] {
    &satisfied.0     // exactly the (path, point) records the compute produced — no derivation
}
```

A narrower slot receiving backfill gains an entry for each recorded `(path, point)` that lands in
its regime; a later request hits it via `cached_satisfies` above. A narrower result NEVER pretends
broader work is cached (no upward backfill — a shallow compute records only its shallow points). Two
incomparable points (a `Skeleton`/`TypeParamShells` slice vs a `Bound` `Expanded`) share no regime,
so no recorded point of one ever dominates a request in the other's regime ⇒ never satisfy, never
backfill each other.

This aligns to parent §2.10 "a broader result may backfill a narrower entry only for the narrower
points it actually materialised". Mechanized: **satisfaction = dominance over a RECORDED
materialised point; backfill = the recorded materialised points verbatim** — never enum rank, never
a meet-derived point, never the nominal request demand. The monotone path-composition proof (§3.5)
FEEDS this rule: it is what guarantees the recorded intermediate `Navigate` hops are reusable across
terminal modes (§3.5 corollary).

**Realization (U2B.10) — the two-gate warm hit + DIRECTIONAL gated backfill.** Wired into the
`semantic_query_memo` family memo: `MemoEntry.satisfied_projection: MaterializedSet` carries the
recorded set; `try_warm_hit_fast_path` / `get_validated` require BOTH `cached_satisfies(satisfied
_projection, requested_point_for_key(key))` AND `validate_with_self_roots`; `FamilySlots::publish`
backfills the recorded entry verbatim into narrower target slots gated by `cached_satisfies`. ONE
subtlety refines the literal "backfill = every dominated peer": the backfill TARGET set is the
projection-depth-NARROWER slots (the `Expanded→Shallow→Navigate→Identity` direction), NOT every
lattice-dominated peer. The lattice has `Navigate ⊒ Shallow` (Navigate's `NavigateOnly`
normalization/operator rungs dominate Shallow's `None`/`Leave`), but `Navigate` is the intermediate
next-hop demand that carrier-stops WITHOUT materialising a one-shell surface — so a `Navigate` result
must NOT be cloned into the `Shallow` slot (it would serve an under-materialised surface and hide,
e.g., a cyclic-heritage expansion the `Shallow` request would surface). Backfill therefore flows only
toward strictly-shallower projection depth; the `cached_satisfies` gate prunes the unsound enum-rank
cases WITHIN that direction (it rejects the legacy `Shallow → Navigate` clone, `Shallow ⊅ Navigate`).
The directional+gated set is a strict subset of the legacy `backfill_targets` fan-out (now retired),
so it can never introduce a warm hit the enum-rank path did not. Guards:
`cache_satisfaction_is_materialized_point_not_nominal_demand`,
`backfill_writes_only_recorded_materialized_points`.

### 3.5 Monotone path composition (the path-precise rule, proven)

A path projection `A.b.c.d` runs hop-by-hop. The demand AT each hop is:

```
fn demand_at_hop(i: usize, n: usize, terminal: Demand) -> Demand {
    if i < n - 1 { NAVIGATE_PRESET.with_regime(terminal) }   // intermediate hops: Navigate
    else         { terminal }                                // terminal hop: the caller's point
}
```

`NAVIGATE_PRESET` is `member_demand = SetOnly`, `operator_reduction = NavigateOnly`,
`alias_preservation = Keep`, `carrier_stop = Continue`, `path = the single hop`. We must show the
composition is **order-preserving in the terminal demand**: a broader terminal demand never makes
an intermediate slice narrower (the path-precise invariant — intermediate hops stay `Navigate`
regardless of how broad the terminal is).

**Claim.** For terminals `t₁ ⊑ t₂` (same regime), `demand_at_hop(i,n,t₁) ⊑ demand_at_hop(i,n,t₂)`
for every `i`.
**Proof.** For `i < n-1` both equal `NAVIGATE_PRESET.with_regime(t)`; `with_regime` copies only the
three regime fields, and `t₁,t₂` share a regime (precondition), so the two intermediate demands are
*equal*, hence `⊑`. For `i = n-1` they are `t₁ ⊑ t₂` by hypothesis. ∎

**Corollary (path-precision + materialised-record reuse).** Because intermediate demands are
CONSTANT in the terminal demand, widening the terminal projection (e.g. `Expanded` vs `Shallow` at
the leaf) never widens any intermediate slice. So `A['c']['full']['bar']` loads `c` and `full` in
`Navigate` (member-set, non-owning normalization only) and expands only the terminal `bar` —
*exactly* the parent §2.10 path-precise behavior, and it is monotone. This monotonicity is precisely
what makes the §3.4 RECORDED-point reuse sound: every terminal mode produces the SAME intermediate
`Navigate` hop records at the same paths, so a recorded `(["c"], Navigate)` / `(["c","full"],
Navigate)` materialised by one terminal-mode compute satisfies the identical intermediate need of
any other terminal-mode compute (an intermediate `Navigate` slice dominates every terminal mode's
`Navigate`-or-below intermediate need at that exact path). The terminal record `(["c","full","bar"],
terminal_point)` is reusable only for requests dominated at THAT path — never for a shallower
terminal that the deep compute did not materialise.

### 3.6 Cache-axis minimality + normalization (§2.10, §6.2)

Two canonicalizations run BEFORE a `(ProjectionDemand, EvalPolicy)` point enters a key:

1. **Path interning** — `ProjectionPath` is prefix-interned; `["a","b"]` and any structurally equal
   path hash identically (the interner is the normal form). The prefix order (§3.1) is then a cheap
   id-prefix check.
2. **Substitution canonicalization** — `SubstitutionCanonicalHash` is the normal form of the
   substitution environment; two equivalent substitution environments collapse to one hash.

**Family-local axis projection (minimality).** A demand axis a family never branches on is NOT
carried on that family's key. Each family declares `relevant_demand_axes() -> AxisMask`; the key
constructor zeroes irrelevant axes to their `⊥` before hashing (e.g. `ResolveEnum` carries no
substitution axis — an enum decl is not generic — and no `member_demand` body axis). This is the
benched `cache_key_axes_are_minimal_and_normalized` contract: removing/denormalizing a carried axis
must either break a correctness fixture (load-bearing) or leave the benched hit rate unchanged
(dead → drop). Lands here as the shape; benched under pressure at U3 + U15.

### 3.7 Worked examples

| Request | `ProjectionDemand` | `EvalPolicy` (regime in bold) | Notes |
|---|---|---|---|
| **Identity** | `path=[]`, no member/body, no sigs | **Bound/Plain/Standalone**, `Keep`, `None`, `Leave`, `Continue` | `⊥` of its regime; returns the alias *declaration identity*, never its body, never a miss |
| **Navigate** | one-hop path, `facets={members}`, `SetOnly` | **Bound/Plain/Standalone**, `Keep`, `NavigateOnly`, `op=NavigateOnly`, `Continue` | next-hop chooser; non-owning normalization only |
| **Shallow** | `path=[]`, `facets={members}`, `SetOnly` | **Bound/Plain/Standalone**, `Keep`, `None`, `op=Leave`, `Continue` | one shell level; operator carriers (`Pick<…>`) stay `Ref` |
| **Expanded** | terminal `path`, `facets={members,…}`, `SetPlusBody` | **Bound/Plain/Standalone**, `Inline`, `Terminal`, `op=Reduce`, `Continue` | member set + demanded bodies; `keyof T` emits the literal-union from T's SHALLOW surface |
| **Skeleton** | BFS surface, `SetOnly` | **TypeParamShells/Plain/Standalone**, `Keep`, `op=Leave`, `carrier_stop=StopAtCarrier` | `generic_open = TypeParamShells` ⇒ DIFFERENT REGIME ⇒ incomparable to all of the above |
| `Pick<Foo,"bar">` | terminal `path=[]` on the `Pick` carrier; `facets={members}` | **Bound/Plain/Standalone**, `op=NavigateOnly`/`Leave` at the carrier, `carrier_stop=StopAtCarrier` | carrier-stop materialises ONLY `bar`; other `Foo` members stay shallow (path-precise) |
| `A['c']['full']['bar']` | `path=["c","full","bar"]`, terminal `Expanded` | **Bound/Plain/Standalone** | hops `c`,`full` run `demand_at_hop = Navigate` (§3.5), only `bar` expands |
| open conditional on path | the conditional reducer (U2.RELATION_INFER §4.3) **distributes the remaining `ProjectPath` into BOTH branches**, each branch carrying the same terminal demand point | regime preserved | the lattice point is duplicated into both branches; the branch results union — no narrowing of either branch's demand |

The five presets are the only public vocabulary; a demand that fits no preset constructs a
`(ProjectionDemand, EvalPolicy)` point directly (parent §2.10) — no sixth mode rung.

---

## Deliverable #2 — Per-key identity model (`SemanticQueryKey → SemanticQueryValue → wire`)

### 2.1 The crux: how env stays ON the key (R21 ⊓ the env-bearing slot)

There is an apparent tension: R21 says "`lib_env_hash` enters a key only when the value depends on
lib", yet `ResolvedDeclSlotIdentity` carries `type_env + lib_env + project` *unconditionally*. The
resolution is a **two-tier env model**:

- **Tier 1 — the slot's intrinsic env.** `ResolvedDeclSlotIdentity` is a DECLARATION-SITE identity:
  "which resolved declaration does this name denote". A declaration's resolved *meaning* genuinely
  depends on `type_env` (strict family, target → which overload/merge wins), `lib_env` (a lib merge
  can add an interface arm to the very declaration), and `project_identity` (workspace isolation).
  So the slot carries these THREE unconditionally — they are not "extra", they are constitutive of
  declaration identity. It carries NEITHER `resolve_env` (the slot is already the *resolved* target;
  the resolution that produced it is upstream) NOR `parse_env` (parse env lives on
  `VersionedDeclIdentity` payload, not the content-free slot).

- **Tier 2 — the per-key `*Context` adds ONLY the EXTRA dims the QUERY depends on.** A query over a
  slot adds the dimensions its *operation* (not the decl) is sensitive to: chiefly
  `resolve_env_hash` for keys that resolve imports / augmentation targets / module specifiers, and
  `parse_env_hash` for keys that read the parsed body skeleton (class-surface decorator lowering,
  flow/contextual body analysis). A key NOT built on a slot (builtin `ApparentType`) does NOT
  inherit the slot's env — it carries only `lib_env + type_env + project` because its value comes
  from lib facts, full stop.

This keeps R21 honest at the per-key tier (each `*Context` lists only what its VALUE depends on)
while the slot's three dims are correct *declaration identity*, not an over-key. The guard
`declaration_augmentation_target_is_env_free_env_comes_from_context` and the per-key
`*_same_site_different_env_or_context_do_not_warm_hit` set mechanize this.

**`Instantiate`/`ResolveMacroPayload` key on the env-bearing slot.** `base`/`owner` key on the
env-bearing, content-free `ResolvedDeclSlotIdentity` directly (not a content-free, env-FREE
declaration key plus a separate env context).
*Rationale:* (a) the slot is the canonical declaration identity the rest of U2 keys on, so a single
identity type across `ResolveMergedDeclaration` / `ResolveClassSurface` / `Instantiate` /
`ResolveMacroPayload` means one resolution path and no decl-key↔slot adapter; (b) the slot is
content-free (R6-clean) and env-bearing for exactly the three dims a declaration's meaning
depends on; (c) it closes the "env validity is purely `ReadSetSignature`" gap where a
`type_env`/`lib_env` change to the *declaration* (not its deps) would only be caught by fact
revalidation rather than key separation — keying on the slot makes that difference a key
difference, which is strictly more correct and avoids candidate-slot churn.
**`Instantiate` additionally carries `resolve_env_hash` (`R`) on a
dedicated per-key `InstantiateContext { projection_reduction: ProjectionReductionContext,
resolve_env_hash }`** because instantiation can resolve imported type-argument references — the `R`
dim rides on this wrapper, NOT mutated into the shared `ProjectionReductionContext` (which stays a
pure projection-demand identity carried unchanged by `KeyOf` / `MappedType` / `ProjectPath`, per
§2.6's per-key-context rule; this mirrors how `RelationContext` / `CallResolutionContext` embed a
`projection_reduction: ProjectionReductionContext` field). `ResolveMacroPayload` carries a dedicated
`MacroPayloadContext { resolve_env_hash, mode }` (macro payload resolves imports). The `T,L,J` dims
come from the env-bearing `ResolvedDeclSlotIdentity` base/owner. KeyOf / MappedType /
ProjectPath / RelationContext and the shared `ProjectionReductionContext` are SEPARATE — their env
identity is unaffected by this slot/context shape.
*Two distinct `FamilyKey` axes (verified against `semantic_query_memo/family.rs`). (a) **Env-bearing
slot core:** `FamilyKey::Instantiate` and `FamilyKey::ResolveMacroPayload` carry their `base`/`owner`
as a `ResolvedDeclSlotIdentity` (`FamilyKey::KeyOf` / `MappedType` / `ProjectPath` root on a bare
`SemanticNodeId`, not a slot). (b) **`provenance` + `merge_role` discriminators:** these ride at
FAMILY-IDENTITY level on ALL FOUR `ProjectionReductionContext`-carrying families — `FamilyKey::Instantiate`,
`KeyOf`, `MappedType`, and `ProjectPath` — NOT demoted into a `*Context` (for `Instantiate` they are
sourced from the key's `InstantiateContext.projection_reduction`; for the other three from the key's
`ProjectionReductionContext` directly). `ResolveMacroPayload` does NOT carry them — its
`MacroPayloadContext { resolve_env_hash, mode }` has no provenance/merge_role axis. They are
query-identity discriminators (which merge arm / which provenance regime this projection answers),
not env dimensions, so they belong on the family key, not the env context.*

### 2.2 Per-key identity table

R21 env columns: P=parse_env, R=resolve_env, T=type_env, L=lib_env, J=project_identity. "slot"
means the three slot-intrinsic dims (T,L,J) come from `ResolvedDeclSlotIdentity`; the `*Context`
adds the marked extras. All keys: NO content/version hash, NO `fact_dep_signature` (R6).

**Landed vs. forward-planned.** `ResolveAmbientNamespace`, `ResolveOverloadSet`, `ResolveEnum`,
`FlowNarrowingAt`, `ContextualTypeAt` (plus the landed-elsewhere `ResolveClassSurface`,
`ApparentType`, `TemplateLiteralReduce`) are LIVE in `SemanticQueryKey` and the generated spec table.
`ResolveMergedDeclaration` and `ResolveDeclarationAugmentation` (marked **forward-planned** below) are
NOT in the landed enum or spec table — they are owned by a not-yet-landed block and appear here as the
end-state shape only.

| Key | IdentityCore | `*Context` (extra env beyond slot) | Env dims | Value domain | Facts read / written | Allowed demand axes | Family | Producer | Wire target |
|---|---|---|---|---|---|---|---|---|---|
| `ResolveMergedDeclaration` *(forward-planned)* | `decl_slot: ResolvedDeclSlotIdentity` + `type_args` | `MergedDeclarationContext` {P,R} + subst + proj-reduction | P R T L J | `TypeNode` | r: `Member`/`MemberPresence`, merge-contributor provenance | `MemberDemand`, subst | `ResolveMergedDeclaration` | `verter_semantic::analysis` merge | `GraphTypeNode` |
| `ResolveDeclarationAugmentation{target: Module\|Global}` *(forward-planned)* | `DeclarationAugmentationTarget` (env-FREE) | `DeclarationAnalysisContext` {R,L,J} — the `AugmentationTargetKey{J,R,L,population,target}` folds project+resolve+lib + the session-view `population` dim; parse_env enters ONLY via the analysis-body read, not the target key | R L J + `population` (parse_env via body read only) | **`DeclarationAnalysis`** | r/w: `module_augmentations`/`global_augmentations` via `AugmentationTargetKey{J,R,L,population,target}` ({J,R,L} derived from context, `population` from the active session view) | none (analysis key) | `ResolveDeclarationAugmentation` | declaration analysis | `GraphTypeNode` kinds 21–25 (`GraphModuleAugmentation` / `GraphGlobalAugmentation`) — a separate `DeclarationAnalysisGraph` wire message was adjudicated **REJECTED**; the merge/augmentation wire home already exists in the closed contract (see `/type-resolution`) |
| `ResolveAmbientNamespace` | `namespace_slot` (slot, `SymbolSpace::Namespace`) + `type_args` | `AmbientNamespaceContext` {P,R} + `mode` (substitution rides on the key's `type_args`) | P R T L J | `TypeNode` | r: namespace member facts | `MemberDemand`, subst | `ResolveAmbientNamespace` | `verter_semantic` namespace analysis | `GraphTypeNode` |
| `ResolveOverloadSet` | `callee: SemanticNodeId` + `type_args` | `OverloadSetContext` {R} (NO parse_env) + subst | R T L J | **`OverloadSet(Arc<[SignatureRef]>)`** | r: signature facts | subst | `ResolveOverloadSet` | signature lowering | `GraphTypeNode` signature list |
| `ResolveEnum` | `enum_slot` (slot) | `EnumContext` {R} (NO parse_env, NO subst) | R T L J | `TypeNode` | r: enum-member facts | none | `ResolveEnum` | enum analysis | `GraphTypeNode` |
| `FlowNarrowingAt` | `point: ProgramPointId` + `flow: FlowNarrowingKey` (per-key field) | `ProgramAnalysisContext` {P,R,T,L,J} + subst (no slot) | P R T L J | **`ProgramAnalysis`** | r: `FlowSlice`, narrowed-symbol facts (`FactDomain::ProgramAnalysis`) | `FlowNarrowingKey`, subst | `FlowNarrowingAt` | flow engine (U6 behavior) | `ProgramAnalysisGraph` |
| `ContextualTypeAt` | `point: ProgramPointId` + `contextual: ContextualTypingKey` (per-key field) | `ProgramAnalysisContext` {P,R,T,L,J} + subst (no slot) | P R T L J | **`ProgramAnalysis`** | r: contextual-typing facts | `ContextualTypingKey`, subst | `ContextualTypeAt` | contextual engine (U6) | `ProgramAnalysisGraph` |
| `ResolveClassSurface` | `decl_slot` + `type_args` + `side: ClassSurfaceSide` | `ClassSurfaceContext` {P,R} (incl. parse_env — decorators) + `mode` (substitution rides on the key's `type_args`) | P R T L J | `TypeNode` | r: heritage, member, brand facts | `MemberDemand`, subst, `side` | `ResolveClassSurface` | class-surface lowering | `GraphTypeNode` |
| `ApparentType` | `base: SemanticNodeId` | `ApparentTypeContext` {L,T,J} — **NO slot, NO parse/resolve** | T L J | `TypeNode` | r: `LibIntrinsic`, lib-wrapper member facts | `MemberDemand`, subst | `ApparentType` | lib member index | `GraphTypeNode` |
| `TemplateLiteralReduce` | `pattern` + `args` | `TemplateLiteralReduceContext` {R,T,L,J} (NO parse_env) + subst | R T L J | `TypeNode` | r: intrinsic (`Uppercase`…) facts | subst | `TemplateLiteralReduce` | template reducer | `GraphTypeNode` |
| `Relate` (UPGRADE) | `{source,target,relation: RelationKind, policy: RelationPolicy, source_freshness: FreshnessKey, inference_context: Option<InferenceContextKey>}` | `RelationContext` {R,T,L,J} + subst + proj | R T L J | **`Relation(RelationPayload)`** | r: `Member`/`MemberPresence`, `TypeEnvOptions`, `LibIntrinsic` (the relation outcome reads lib intrinsics, so `L` is in identity); coinductive proof rides the payload-side proof table off-surface | `RelationKind`, `RelationPolicy`, `FreshnessKey`, `InferenceContextKey`, subst | `Relate` (full-identity) | relation engine (U2.RELATION_INFER) | `RelationPayload` |
| `Instantiate` | `base: ResolvedDeclSlotIdentity` + `args` + `provenance` + `merge_role` (the last two stay FAMILY-IDENTITY on `FamilyKey`, NOT demoted into `*Context`) | `InstantiateContext { projection_reduction, resolve_env_hash }` ({R} + proj-reduction; subst rides on `args`) | R T L J | `TypeNode` | r: decl body, member facts | `(ProjectionDemand,EvalPolicy)`, subst, provenance, merge_role | `Instantiate` | instantiation reducer | `GraphTypeNode` |
| `ResolveMacroPayload` | `owner: ResolvedDeclSlotIdentity` + `macro_index` + `macro_kind` + `type_args` | `MacroPayloadContext { resolve_env_hash, mode }` ({R} + `mode`) | R T L J | `TypeNode` | r: `AnalyzedMacro` sidecar | `mode`, subst | `ResolveMacroPayload` | Vue macro resolver | `GraphTypeNode` |

`InferenceContextKey` is the content-free projection of the active `InferenceSession` (parent §4.2;
SHAPE only here, substrate in U2.RELATION_INFER): `{inferable_params, variance_phase,
candidate_priority, no_infer_mask, const_param_policy, contextual_inference_mode}`. It is part of
`Relate`'s identity iff `inference_context = Some` (binding-producing); `None` for pure
assignability. NO env/content fields (R6/R21).

`FlowNarrowingKey` / `ContextualTypingKey` are likewise content-free SHAPE-only projections
(newtypes over an interned `Arc<[SemanticNodeId]>` set, mirroring `InferableParamSetId`;
substrate in U6). They are PER-VARIANT key fields — `FlowNarrowingAt` carries `flow: FlowNarrowingKey`,
`ContextualTypeAt` carries `contextual: ContextualTypingKey` (shown directly in each row's
IdentityCore column) — NOT folded into the shared `ProgramAnalysisContext`, so neither variant
carries the other's dead axis. The shared `ProgramAnalysisContext` carries `{P,R,T,L,J} +
substitution` only (no `flow`/`contextual` fields); both variants depend on the shared
`substitution` axis, which rides on the context. NO env/content fields (R6/R21).

`SemanticSymbolSpace` gains `Namespace` `[mined: producer-discriminator + SymbolSpace::Namespace]`:
`enum SemanticSymbolSpace { Type, Value, Namespace }` — NEVER a `BothTypeValue` arm. A
namespace-only declaration (`namespace N {}` with no value/type half) keys on a slot with
`symbol_space = Namespace`; a class/enum that occupies BOTH type and value space is TWO slots
(`Type` + `Value`), resolved by the dual-space algorithm below, never one fused arm.

### 2.3 `[mined: class-dual-space]` — Class producer Type+Value dual-space algorithm

A class declaration occupies BOTH symbol spaces. `ResolveClassSurface` constructs the surface from
two shared-resolver queries, NO OXC at query time:

```
fn resolve_class_surface(decl_slot, type_args, side, demand, ctx) -> SemanticQueryValue::TypeNode {
    match side {
        Instance => {
            // instance side = the TYPE-space half: Instantiate the type slot under Shallow
            let type_slot = decl_slot.with_symbol_space(Type);
            let shape = execute(Instantiate { base: type_slot, args: type_args,
                                              context: shallow_ctx(demand, ctx) });
            // heritage descent + member-demand projection over the instantiated shape
            project_instance_surface(shape, demand)            // shared resolver, no walker
        }
        Static => {
            // static side = the VALUE-space half: TypeOf the value root (the constructor value)
            let value_slot = decl_slot.with_symbol_space(Value);
            let ctor = execute(TypeOf { value_root: value_root_of(value_slot) });
            // construct signatures + static members come from the constructor value's surface
            project_static_surface(ctor, demand)               // shared resolver, no walker
        }
    }
}
```

The instance surface is `Instantiate(type_slot, Shallow)`; the static surface is
`TypeOf(value_root)` constructing the constructor signatures. Both route through
`ProjectSemanticDispatch::execute` — there is NO query-time OXC element/frontier resolver (the
one-engine rule). `#private`/`#protected` brands are carried as nominal identities on the surface
(parent §7) but are absent from the published projection; they enter `Relate` identity only.

### 2.4 `[mined: merge+aug-rail]` — cross-file merging + augmentation-index fact rail

`ResolveMergedDeclaration` and `ResolveDeclarationAugmentation` share ONE merge/augmentation fact
rail (parent §9.5 of the superseded plan, re-expressed):

- Cross-file declaration merging (`interface Foo` in file A + `interface Foo` in file B) resolves
  to ONE merged slot; each contributor part carries `DeclPartId` provenance
  (`VersionedDeclIdentity.merged_parts`, payload not validation — in `semantic_query.rs`). Adding an
  overload to one part invalidates only consumers that observed THAT part's facts (slot-level fact
  validation is the oracle).
- The augmentation index (`AugmentationTargetKey {project_identity, resolve_env_hash, lib_env_hash,
  population, target}`, whose `{R,L,J}` env dims are derived from `DeclarationAnalysisContext` at
  execute time and whose `population: AugmentationPopulation {Base, Session(overlay-set fingerprint)}`
  dim is derived from the active session view — NOT from `DeclarationAnalysisContext`) provides
  inverse lookup: "which files augment module/global M", with Base/Session overlay isolation. The
  context is the SOLE source of the `{R,L,J}` env dims, so the query-key env and the index env cannot
  diverge (guard `declaration_augmentation_target_is_env_free_env_comes_from_context`). The wire/graph home for a
  merge/augmentation result is `GraphTypeNode` kinds 21–25 (`GraphModuleAugmentation` /
  `GraphGlobalAugmentation`); a proposed separate `DeclarationAnalysisGraph` wire message was
  adjudicated **REJECTED** (the home already exists in the closed contract — see `/type-resolution`).

### 2.5 `SemanticQueryKeySpec` table sketch (live variants only)

Per LIVE variant: `(lifecycle, context shape, value domain, env dims, allowed demand, cross-context
guard, admission/budget)`. The U2-landed rows are the seven + three U2 added keys + the `Relate`
upgrade + the migrated `Instantiate`/`ResolveMacroPayload`; the parent §2.9 table is the
end-state superset. Reserved NON-LIVE `DiagnosticAnalysis(CheckResult)` arm + `Check*` names carry
**NO** spec row (counted only over live variants — `semantic_query_key_spec_table_equals_enum`
stays green). The table is GENERATED by a `cargo run` target; the Rust test only diffs.

```
SemanticQueryKeySpec {
    variant: SemanticQueryKeyTag,          // enum discriminant name
    lifecycle: { Live, Retired, Renamed }, // no fourth state
    context_shape: &'static str,           // the named *Context struct
    value_domain: SemanticQueryValueTag,   // exactly one
    env_dims: EnvDimMask,                   // subset of {P,R,T,L,J}; benched-minimal
    allowed_demand: AxisMask,              // which DemandAxis fields this family branches on
    cross_context_guard: &'static str,     // the *_do_not_warm_hit guard name
    admission: AdmissionSpec,              // singleflight | KeyspaceBudget | RelationBudget | FlowSliceBudget | …; ReturnOnly conditions
}
```

`every_semantic_query_key_maps_to_exactly_one_value_domain` cross-checks `value_domain`;
`semantic_query_key_spec_table_equals_enum` asserts the spec set EQUALS the live enum set.

---

## Deliverable #14 — Canonical display policy (NET-NEW)

Display is a PROJECTION over the typed `SemanticQueryValue` / `GraphTypeNode`, NEVER a stored or
re-parsed string. This ties the typed-IR-only rule (a display string is never re-parsed by a
resolver) and the CodeTransform rule (no post-hoc string splicing of a typed result).

### 14.1 The single projection rule

```
fn display(value: &SemanticQueryValue, needs: DisplayNeeds) -> DisplayString {
    match value {
        TypeNode(id)            => display_type_node(graph.node(*id), needs),
        OverloadSet(sigs)       => join("; ", sigs.iter().map(|s| display_signature(s, needs))),
        Relation(payload)       => display_relation(payload),          // outcome only
        DeclarationAnalysis(d)  => display_declaration_analysis(d, needs),
        ProgramAnalysis(p)      => display_program_analysis(p, needs),
        FlowReturn(r)           => display_type_node(r.return_node, needs),   // U6
        ResolvedCall(r)         => display_type_node(r.result_node, needs),   // U6
        DiagnosticAnalysis(_)   => unreachable!("non-live reserved seam"),
    }
}
```

`DisplayNeeds` is a `BitSet<DisplayFacet>` (e.g. `ExpandAliases`, `IncludeReadonlyModifier`,
`TruncateLargeUnions`, `QualifyNames`) and is **DISPLAY-ONLY — it is NOT a typed-value semantic
cache axis** because it never drives resolution. **Invariant (guarded): `display_needs` never drives
resolution, and never enters a typed-value family key.** The discipline is mechanical, two-part:

1. **Typed-value family keys MASK `display_needs` OUT.** Every typed-value family's
   `relevant_demand_axes() -> AxisMask` (§3.6) zeroes the `display_needs` axis to `⊥` before hashing.
   Two queries that differ ONLY in `display_needs` therefore hash to the SAME typed-value slot and
   share the cached typed value — they do NOT create distinct typed-value candidates.
2. **`display_needs` is carried ONLY on the display/publish projection key.** It is part of the
   display-projection cache identity (so two display variants of the SAME typed value don't collide
   at the publish layer), but it is absent from the semantic family key entirely. The reducer
   dispatch path MUST NOT branch semantic resolution on it — only the final `display(...)` projection
   reads it.

Guard: `display_needs_is_display_only_never_drives_resolution` — a DISCRIMINATING fixture: two
queries differing only in `display_needs` (a) resolve to the SAME typed value and (b) hit the SAME
typed-value slot (one compute, not two), only the display string differs. The fixture FAILS if
`display_needs` is folded into a typed-value family key (it would induce a second slot / second
compute).

### 14.2 Per-arm rendering

- **`TypeNode`** — structural walk of `GraphTypeNode`; respects `EvalPolicy.alias_preservation`: a
  `Ref{name}` renders as the alias name when `alias_preservation = Keep` and `DisplayFacet::
  ExpandAliases` is absent; renders the inlined body when both say expand. A `Shallow` value
  displays shallow (member names, bodies as `…`/`Ref`); an `Expanded` value displays expanded.
- **`OverloadSet`** — each `SignatureRef` rendered; ordering preserved (first-applicable order).
- **`Relation`** — `display_relation` renders the `RelationPayload` outcome ONLY (`Assignable` /
  `NotAssignable` / `BudgetExceeded`). The payload-side proof table is OFF the display surface — a
  consumer that wants derivation / failure / cycle detail dereferences the proof table by
  `RelationProofId` directly; it is never rendered into a display string.
- **`DeclarationAnalysis`** — renders augmentation contributor list / merged-part provenance.
- **`ProgramAnalysis`** — renders the narrowed type at the point / the contextual type; the flow
  facts are rendered, not re-walked.

### 14.3 Display composes along the lattice (a sub-lattice, not a resolver)

`display_needs` orders by subset (§3.1) and is a **sub-lattice carried on the DISPLAY/PUBLISH
projection key — NOT on the typed-value family key** (§14.1: masked out of the semantic key). A
broader display need is satisfied by a cached typed value computed under a dominating
`(ProjectionDemand, EvalPolicy)` (the SEMANTIC demand) whose materialised surface is at least as deep
as the display needs. The semantic dominance gate (§3.4) is what enforces depth; `display_needs`
itself never re-resolves and never branches the reducer. Concretely: a `Shallow`
semantic value cannot render an `ExpandAliases`-expanded display string because the bodies were
never materialised — so `display` with `ExpandAliases` REQUIRES a value whose semantic
`alias_preservation = Inline` (the demand dominance gate at §3.4 already enforces this; display does
NOT independently re-resolve). Display never opens a second resolution path.

### 14.4 Tie to U13 published projection

Warm display is a RE-PROJECTION of the cached typed value: at publish time (U13) the published
payload computes `display(cached_value, published_display_needs)` from the cached typed
`SemanticQueryValue`, and the display string is NOT stored as a parsed string that any later stage
re-parses. `PropMeta.rawType` etc. are display-only passthroughs (typed-IR-only rule). New CRITICAL
rule (§Rules): **canonical-display-is-projection-not-stored-string**.

---

## Deliverable #18 — Error tolerance over broken / mid-edit code (HIGHEST FUNCTIONAL RISK)

The editor steady-state is half-written source. Every query must produce a useful result over
broken input WITHOUT poisoning warm caches.

### 18.1 Representing partial/broken input in the value domain

We do NOT add a new top-level `SemanticQueryValue` arm. Broken-ness is a **provenance taint** plus
the existing error carriers:

- **Provenance taint** — every `SemanticQueryValue` result carries (alongside the value) a
  `ResultProvenance { taint: ResultTaint }` where
  `enum ResultTaint { Clean, Partial(BrokenInputClass), Broken(BrokenInputClass) }`. `Clean` =
  computed over well-formed input; `Partial` = computed over input with recoverable errors (a
  missing member, an unresolved import the resolver degraded past); `Broken` = computed over a
  syntactically torn / mid-edit input or a torn read.
- **Existing carriers** — `SemanticNodeData::Opaque(QueryError)` (in `semantic_query.rs`) is the
  in-graph carrier for a node whose value could not be computed. `QueryError` gains nothing
  *required*, but `BrokenInputClass` is a small closed enum:
  `enum BrokenInputClass { SyntaxError, UnresolvedReference, IncompleteDeclaration, TornRead,
  MissingDependency }`. `TornRead` is the mid-edit-version-changed-under-us class.

The error TYPE (`X is the error type`) — a *legitimate* "this type IS an error" result — is
distinct from `Broken` taint: see §22.4. The error type is `Clean`-or-`Partial` and cacheable; a
`TornRead`/`SyntaxError` `Broken` result is not.

### 18.2 The non-admission rule (which broken-ness ⇒ ReturnOnly)

The decision gates the `Warm` arm on the **presence of the rooting fact in the result's
`ReadSetSignature`** — NOT on the taint enum class as a proxy. A `Partial(UnresolvedReference)` /
`Partial(MissingDependency)` is cacheable ONLY IF its negative/missing-dependency fact was actually
RECORDED on the signature; if the producer degraded the reference without recording the fact, there
is no invalidation rail, so it falls to `ReturnOnly`. The taint class narrows WHICH fact must be
present; the signature is the authority for whether it IS present.

```
fn admit_decision(result: &Result, taint: ResultTaint, sig: &ReadSetSignature) -> Admission {
    match taint {
        Clean => Admission::Warm,                          // normal publish
        Partial(MissingDependency) =>
            // cacheable iff the missing-dep fact is recorded (the invalidation rail)
            if sig.records_missing_dependency_fact() { Admission::Warm }
            else { Admission::ReturnOnly },
        Partial(UnresolvedReference) =>
            // cacheable ONLY if its NEGATIVE fact was recorded — never trust the enum class alone
            if sig.records_negative_resolution_fact() { Admission::Warm }
            else { Admission::ReturnOnly },
        Partial(IncompleteDeclaration) => Admission::ReturnOnly,  // mid-edit shape — no stable fact
        Broken(SyntaxError | TornRead) => Admission::ReturnOnly,  // never warm
    }
}
```

- **`ReturnOnly`** (parent §6 + completion-fence rule): returned to the caller, NEVER warm-admitted,
  NEVER backfilled, NEVER published, NO fact signature recorded, NO reverse-index entry. This reuses
  the existing `ComputeAdmission::ReturnOnly` discipline (the same path `BudgetExceeded` /
  cancellation / supersession take).
- **`Broken(SyntaxError)`** — the parse tree for the queried file is torn; any type read from it is
  not a stable fact ⇒ `ReturnOnly`.
- **`Broken(TornRead)`** — the content version changed mid-flight (the completion fence detected a
  generation bump before publish) ⇒ `ReturnOnly` and retry (≤3, the existing `UnstableState`
  budget); on exhaustion return the last `Broken` result `ReturnOnly`.

#### 18.2.1 The cacheable-error case (the subtle line)

A `Partial(UnresolvedReference)` over WELL-FORMED syntax — `import { X } from './missing'` where
`./missing` does not yet exist — produces a *legitimately cacheable* result: the type of `X` is the
error type, and that result is **rooted on a fact** (`MissingDependency` on `./missing`'s canonical
id). When `./missing` later appears, the recorded fact invalidates the entry (lazy cross-file
invalidation, the normal rail). So this is `Warm` with a `MissingDependency` fact — the cache is
correct because the missing-dependency *is* a tracked fact. The discriminator: **does the broken-ness
correspond to a tracked fact?** If yes (unresolved-ref, missing-dep) → cacheable, fact-rooted. If no
(torn syntax, torn read, an incomplete declaration whose shape isn't stable) → `ReturnOnly`.
New CRITICAL rule: **error-tolerance-returnonly-non-admission**.

### 18.3 Propagation + termination

- **Taint propagation (join over taint).** A query reading a dependency result with taint `t_dep`
  produces a result with taint `t_self ⊔_taint t_dep` where the taint join is the lattice
  `Clean ⊑ Partial ⊑ Broken` (so any `Broken` dep taints the consumer `Broken`; a `Partial` dep
  with a clean self stays `Partial`). A broken dependency yields a broken result — it TAINTS, it
  does not crash. The `BrokenInputClass` of the join is the more-severe class (`Broken` classes
  dominate `Partial` classes).
- **No infinite re-resolution of a broken cycle.** A broken input that participates in a cycle is
  bounded by the SAME cycle machinery as a healthy cycle: the coinductive-SCC re-entry assumption
  (parent §4.1) and the path/alias-cycle carriers (`QueryError::AliasCycle`,
  `QueryError::RecursiveRef`). A `Broken` SCC discharges as `ReturnOnly` (never warm-admitted, never
  the published proof). Termination is preserved because the cycle sentinel + the `UnstableState`
  retry budget (≤3) + the per-family budgets all fire regardless of taint.

### 18.4 U0 / foundation split (FORK-B = U0 FOUNDATION, locked)

**DECISION: split into a U0 substrate piece + this gate's piece.** The *value-domain shape* (the
`ResultProvenance` taint, the `BrokenInputClass` enum, the `admit_decision` rule) is owned HERE — it
is value-domain shape. The *production* of the taint — the host/scheduler detecting a torn read, the
parser surfacing `SyntaxError` recovery, the resolver degrading an unresolved reference into a
`MissingDependency` fact — is a **U0/foundation responsibility** (the read/parse/shallow lifecycle,
CLAUDE.md "Shallow File Processing" invariant). If U0 did not produce the taint, every reducer would
invent its own broken-input detection (a second engine, forbidden).
*Rescope consequence (owed doc edit, see Rescope section):* U0 must additionally land:
(a) parser error-recovery surfacing `SyntaxError` taint on `IndexedReady`; (b) the resolver
producing `MissingDependency`/`UnresolvedReference` facts (it largely does already via negative
caching); (c) the completion-fence `TornRead` taint already exists as `UnstableState`. This gate
defines the SHAPE and the `admit_decision`; U0 wires the producers. The differential `tsgo` oracle
(§6.3) does not cover broken input (tsgo also degrades), so error-tolerance parity is gated by
Verter-internal `ReturnOnly` fixtures, not the oracle.

---

## Deliverable #21 — Module-resolution matrix (design depth)

### 21.1 The resolution matrix

The resolution result is `(moduleResolution mode × specifier kind × condition set) → resolved
target`:

```
enum ModuleResolutionMode { Classic, Node10, Node16, NodeNext, Bundler }
enum SpecifierKind { Relative, BarePackage, SubpathImport(/* #-prefixed */), PathsAlias, BaseUrlRel }
struct ConditionSet { import: bool, require: bool, types: bool, node: bool, default: bool,
                      custom: Arc<[Arc<str>]> /* customConditions */ }

fn resolve_module(spec: &Specifier, mode, conditions, cfg: &IdeProjectConfig)
    -> Result<ResolvedTarget, ResolveError>
{
    let candidates: Vec<CandidatePath> = match (spec.kind, mode) {
        (Relative, _)                  => relative_candidates(spec, cfg),
        (PathsAlias, _)                => paths_candidates(spec, &cfg.paths, cfg.base_url),  // tsconfig paths
        (BaseUrlRel, _)                => base_url_candidates(spec, cfg.base_url),
        (BarePackage, Node16|NodeNext) => exports_candidates(spec, conditions, cfg),         // package.json "exports"
        (BarePackage, Node10)          => node10_walk(spec, cfg),                            // legacy node_modules walk
        (SubpathImport, Node16|NodeNext|Bundler) => imports_candidates(spec, conditions, cfg), // package.json "imports" (#…)
        (SubpathImport, Node10|Classic) => return Err(ResolveError::SubpathImportUnsupported(mode)), // #imports need exports/imports map support — no fallthrough
        (BarePackage, Bundler)         => exports_candidates(spec, conditions.with(import=true), cfg),
        (_, Classic)                   => classic_walk(spec, cfg),
    };
    // realpath/symlink: resolve each candidate's realpath (preserveSymlinks honored), bounded by workspace_root
    let real = candidates.into_iter()
        .map(|c| realpath_bounded(c, cfg.workspace_root, cfg.preserve_symlinks))
        .collect();
    // TS-first effective_target priority across the surviving candidates:
    effective_target(real).ok_or(ResolveError::NotFound)
}
```

`effective_target()` selects `.d.ts > .d.cts > .d.mts > .ts > .tsx > .js > .jsx > .cjs > .mjs`
(TS-first, type-resolution skill). It composes AFTER candidate generation: each resolution mode
produces a candidate SET, then `effective_target` picks the single highest-priority extension. Do
NOT try remaining candidates if the selected one lacks the needed type — treat as not-found.

`exports`/`imports` conditional resolution walks the package.json condition tree honoring the
ordered `ConditionSet` (import/require/types/node/default + `customConditions`), with `types`
winning for type resolution (TS-first). `moduleSuffixes` extends the candidate set per suffix.

### 21.2 Env keying — where `resolve_env_hash` enters the U2 surface

A module-resolution result depends on THREE SPLIT dimensions — they are kept split per R21; lib
dims are NEVER hidden inside `resolve_env`:
- **`resolve_env_hash`** (PRIMARY) — folds the `moduleResolution` mode, the `exports`/`imports`
  condition set, `customConditions`, `moduleSuffixes`, `paths`, `baseUrl`, `preserveSymlinks`. This
  is the import/module-resolution config dimension. It does NOT fold `types`/`typeRoots`.
- **`lib_env_hash`** — folds `typeRoots` and the `types` ambient-corpus list (the `@types/*` /
  ambient `typeRoots` resolution corpus a bare-specifier or types-condition resolution consults).
  Per the cache authority these belong to `lib_env_hash`, NOT `resolve_env`. Folding them into
  `resolve_env` would be an R21 hash-split violation — keep them on `lib_env_hash`.
- **`project_identity`** — for `workspace_root` / `paths` / `baseUrl` project isolation.
- **`parse_env_hash`** (TRANSITIVE) — module resolution is transitively dependent on `parse_env`:
  the import-specifier LIST that resolution consumes is a PARSE artifact (the live
  `ResolvedImportFactsKey` carries `parse_env_hash`). The name→file-path mapping does not itself read
  parse env, but the specifier set it resolves is produced by parsing, so any import-resolving key
  carries `parse_env_hash` transitively through the specifier list it reads. This replaces the
  earlier flat "NOT parse_env" claim: parse_env is a transitive dependency, not absent.

This is the answer to "where does `resolve_env_hash` enter the U2 key surface" (it is absent from
`ResolvedDeclSlotIdentity`): it enters through the per-key `*Context`, with the EXACT dim split per
key — NOT a uniform `{resolve_env, lib_env, parse_env, project_identity}` bundle on every context
(that would re-bundle dims R21 keeps split):

- `MergedDeclarationContext`, `DeclarationAnalysisContext`, `AmbientNamespaceContext`,
  `TemplateLiteralReduceContext`, `RelationContext`, `ProgramAnalysisContext` — these resolve imports
  AND/OR read the ambient/types corpus from the context, so each carries the dims it actually
  consults (`resolve_env`; `lib_env` when it consults the ambient/types corpus; `parse_env`
  transitively through the specifier list it reads; `project_identity`) SPLIT.
- The MIGRATED `Instantiate` / `ResolveMacroPayload` keys (§2.2) split DIFFERENTLY: the `T,L,J` dims
  (`type_env_hash`, `lib_env_hash`, `project_identity`) ride the env-bearing content-free
  `ResolvedDeclSlotIdentity` base/owner SLOT, and ONLY `resolve_env_hash` (`R`) rides the dedicated
  per-key context (`InstantiateContext { projection_reduction, resolve_env_hash }` /
  `MacroPayloadContext { resolve_env_hash, mode }`). These contexts do NOT carry `lib_env`,
  `project_identity`, or a standalone `parse_env` — those are on the slot (`T,L,J`) or simply not in
  this key's identity. Total env identity is `R T L J` for both keys (§2.2 table).

The slot itself stays resolve-env-free because it is the ALREADY-resolved declaration identity; the
resolution that produced it keys on `resolve_env_hash` UPSTREAM (in `ResolvedImportFacts`, which
carries `resolve_env_hash` + `parse_env_hash` but NOT `lib_env_hash` — type-cache skill R21 audit
table).

### 21.3 `workspace_root` bound + symlink cycle termination

Owned resolution is bounded by `IdeProjectConfig.workspace_root`: `node_modules` and `#imports`
ancestor walks STOP at `workspace_root` (CLAUDE.md macro-traversal rule). `realpath_bounded`
resolves symlinks but: (a) stops at `workspace_root`; (b) bounds symlink-chain depth by a constant
`MAX_SYMLINK_HOPS` and detects cycles via a visited-realpath set → `ResolveError::SymlinkCycle`
(never non-terminating). See §Termination.

### 21.4 U0 rescope (FORK-C = U0 RESOLVER_CORE, locked)

**DECISION: module resolution is a U0/foundation resolver deliverable, not a value-domain
deliverable.** The matrix and `resolve_env_hash` composition belong in the resolver/import-graph
layer (`verter_session::resolver_core`), the U0 substrate. This gate designs the matrix and PINS the
env keying (split `resolve_env` + `lib_env` + `project_identity` + transitive `parse_env`, §21.2);
the IMPLEMENTATION of the matrix (the conditional-exports walker, the symlink/realpath resolver)
lives in U0's resolver.
*Rescope consequence (owed doc edit, see Rescope section):* U0 must own `resolve_module` and fold
the listed config into the split dims. This gate's contribution is the keying contract (new CRITICAL
rule: **module-resolution-keys-on-resolve-env**, lib dims never hidden in resolve_env) and the
requirement that every U2 `*Context` resolving imports carries the split env dims. The differential
`tsgo` oracle (§6.3) DOES gate this: module-resolution parity fixtures (conditional exports, node16
vs bundler, paths, symlink) are a STAGE B U0 oracle-row requirement.

---

## Deliverable #22 — Error-type / `any` / `unknown` / `never` propagation lattice

Ties #3 (these are values flowing through the demand lattice) and #18 (the error type is the
broken-input carrier).

### 22.1 The type-lattice positions

```
                unknown   (⊤ — top: everything assignable TO it)
                  │
        … ordinary types …
                  │
                never     (⊥ — bottom: assignable FROM nothing-but-itself, TO everything)

  any   — OFF the assignability order: BOTH assignable-to AND assignable-from every type
          (the "wildcard"); it is its own band, not a top or bottom.
  error — a Partial/poison CARRIER (§18); structurally `any`-like in relation but TAINTS
          and is ReturnOnly-prone. Distinct node from `any`.
```

`unknown` is the join-top of the assignability lattice; `never` is the meet-bottom. `any` is
deliberately *outside* the partial order (it relates both directions), modeled as a distinct
`GraphTypeNode::Any` that the relation engine short-circuits. `error` does **NOT** get a new
`GraphTypeNode` wire arm — introducing `GraphTypeNode::ErrorType` would violate the wire-purity
closure + closed-enum discipline (parent §1.3/§1.4). The error type rides the EXISTING carrier:
`SemanticNodeData::Opaque(QueryError)` (in `semantic_query.rs`); in the §18.4 target it also carries
the §18 `ResultTaint` provenance taint. Relation-wise an `Opaque(QueryError)` error node behaves
like `any` (relates both directions so a broken sub-result does not cascade spurious assignability
failures); in the §18.4 target it carries the §18 taint via `ResultProvenance` and follows §18
admission (the current realization is carrier-dominating — see §22.2). No wire-arm addition; no
schema_version bump.

### 22.2 Absorption rules (per reducer)

| Operator | `any` | `never` | `unknown` | `error` |
|---|---|---|---|---|
| union `X \| ?` | `X \| any = any` | `X \| never = X` | `X \| unknown = unknown` | `X \| error = error` (taint) |
| intersection `X & ?` | `X & any = any` | `X & never = never` | `X & unknown = X` | `X & error = error` (taint) |
| indexed access `?[K]` | `any[K] = any` | `never[K] = never` | `unknown[K]` = **UNCONDITIONAL error** for ALL K (`unknown` has no index signatures — illegal index ⇒ `Opaque(QueryError)`, not per-K, not crash) | `error[K] = error` |
| `keyof ?` | `keyof any = string\|number\|symbol` | `keyof never = string\|number\|symbol` (TS quirk) | `keyof unknown = never` | `keyof error = error` |
| conditional `? extends T` | `any extends T ? X : Y = X \| Y` (union of BOTH branches via `NormalizeUnion`, mode-independent; §22 fast-reject, except when `extends` is an `infer` pattern — then the infer-binding path binds it) | DISTRIBUTIVE `never` ⇒ `never` (empty distribution); NON-distributive `never extends T ? X : Y` ⇒ the TRUE branch `X` (never ⊑ everything) — the fast-reject gates the collapse on `distributive` | reduces per relation | `error` ⇒ `error` (carrier-dominating) |
| mapped `{ [K in ?] }` | over `any`: `any` | over `never`: `{}` | over `unknown`: **`{ [K in keyof unknown] }` = `{ [K in never] }` = `{}`** (the COMMON path — mapping over `keyof unknown = never`); a DIRECT mapping over `unknown` itself (`{ [K in unknown] }`, K not constrained to a key set) is **illegal ⇒ error** | over `error`: `error` |
| template-literal segment of `?` | `` `${any}` = string `` | `` `${never}` = never `` | error (not lexable) | `error` |

These absorption rules are the reducers' FIRST check (a fast-reject discriminator, parent §6.2
"fast-reject discriminators first") before structural work. `any`/`never`/`unknown` are
LEGITIMATELY CACHEABLE results (they are `Clean`); the `(taint)` annotations above mark where, in
the §18.4 target, the error operand's taint is joined onto the absorbed output so the result
follows §18 admission.

**Current realization (U2B.12):** the conditional `any` row (⇒ union of both branches) and the
distributive-`never` row (⇒ `never`) are IMPLEMENTED in the §22 fast-reject (`absorb_conditional`),
not deferred to the branch logic: the `any` row builds `NormalizeUnion([true_branch, false_branch])`
(falling through to the infer-binding path when `extends` carries an `infer`), and the `never` row
is gated on `distributive` so a non-distributive `never extends T` still selects the true branch via
the relation path. The remaining seam is purely about TAINT: the absorption fast-reject is
*carrier-dominating*, not yet taint-propagating. An `error` operand dominates so the absorbed result
is the error CARRIER itself
(its `Opaque(QueryError)` node identity + payload survive, so relation/display still see the error
type), but the absorbed `QueryBuildOutput`'s `taint` is `Clean` — the operand's `ResultTaint` is
NOT yet joined onto the output. This is sound today because no producer emits non-`Clean` taint
(every build is `Clean`), so every absorbed type error is deterministic (`unknown[K]`, `keyof
error`, …) and legitimately cacheable. Joining the dominating operand's taint (the `(taint)`
annotations) is the §18.4 follow-up, tracked at `absorb.rs::absorbed_output`. `admit_decision`
itself — the gate that maps a non-`Clean` taint to `ReturnOnly` — is implemented and unit-tested
now; only the taint PRODUCERS and the absorption taint-join are §18.4.

### 22.3 `error` vs `any` (the distinguishing rule)

- `any` is a `Clean`, fully cacheable type. A query that legitimately produces `any` (e.g.
  `noImplicitAny: false` untyped param) warm-admits normally.
- `error` is `ReturnOnly`-prone when input-degraded (the §18.4 target). An `error` produced by a
  tracked fact (`MissingDependency`) is fact-rooted-cacheable (§18.2.1); an `error` produced by a
  torn/broken input is `ReturnOnly`. Today (carrier-dominating, see §22.2) the error carrier is
  preserved but its taint is not yet joined onto the absorbed output.
- Relation treats both as bidirectionally-relating (so neither cascades spurious failures); in the
  §18.4 target only `error` propagates taint. Guard: `error_type_is_returnonly_prone_any_is_cacheable`.

### 22.4 Tie-back

These are values flowing through the §3 demand lattice (an `any` member under `Shallow` displays
`any`; under `Expanded` is still `any`). The `error` type is the #18 broken-input carrier surfaced
into the type-values surface as `SemanticNodeData::Opaque(QueryError)` (the EXISTING carrier — NOT a
new `GraphTypeNode::ErrorType` arm, which would break the wire-purity closure) plus the §18
`ResultTaint`. New CRITICAL rule: **error-any-never-propagation-lattice**.

---

## Termination / convergence (every recursive rule introduced)

Every recursive rule below terminates and NEVER warm-admits a non-converged result.

1. **Lattice `meet`/`join` (§3.3).** Non-recursive: each operates on finite-height field lattices
   (`path` via finite interned ids; bitsets; finite chains). Terminates trivially. `join` over the
   prefix order may be `None` (partial) — partiality is a base case, not non-termination.

2. **Path composition (§3.5).** Bounded by `path.len()` (finite, prefix-interned). Each hop is one
   `execute`; no hop revisits a prefix (the path is a fixed sequence). Terminates in `n` hops.

3. **Display recursion (§14) — NET-NEW depth guard for #14.** `display_type_node` walks
   `GraphTypeNode`. A recursive type (`type Self = { next: Self }`) is bounded by the SAME coinductive
   discipline the parent §4.1/§13 uses: a visited-node set + an `isDeeplyNestedType`-style depth
   guard. **The display-side depth guard is NET-NEW for #14** (display is a new projection path; it
   does not inherit a resolver-side guard automatically) and lands with its own bound constant
   `MAX_DISPLAY_DEPTH`. On re-entry of a visited node, display emits a back-reference token (the alias
   name) rather than recursing — `alias_preservation` makes this natural (a recursive alias displays
   as its `Ref` name). Bounded by node count + the net-new depth guard.

4. **Error/taint propagation (§18.3).** The taint join is over the 3-element chain
   `Clean ⊑ Partial ⊑ Broken` — finite, monotone (taint only moves UP). A broken cycle is bounded by
   the coinductive-SCC re-entry assumption (parent §4.1) + the `UnstableState` retry budget (≤3) +
   per-family budgets. A `Broken` SCC discharges `ReturnOnly`; the cycle sentinel is transient,
   never the published proof.

5. **Module-resolution symlink cycles (§21.3).** `realpath_bounded` carries a visited-realpath set
   and a `MAX_SYMLINK_HOPS` constant; a revisit ⇒ `SymlinkCycle` error (a base case). The ancestor
   walk is bounded by `workspace_root`. Terminates.

6. **Type-lattice absorption (§22).** Non-recursive fast-reject checks run BEFORE structural
   recursion; they strictly reduce the problem (an `any`/`never`/`unknown`/`error` operand
   short-circuits, removing a recursion). No new recursion introduced.

No rule may non-terminate or warm-admit a non-converged result: every `ReturnOnly` path (§18, §22,
budgets) is the non-admission valve; every cycle is bounded by an existing coinductive/depth/budget
discipline cited above.

---

## Differential `tsgo`-oracle baseline note (this phase)

The value-domain SHAPE (the typed enum, the demand lattice, the per-key identity) is **structural**
and is gated by structural guards (§Rules), NOT the oracle. Three deliverables need `tsgo`-oracle
parity fixtures in STAGE B:

- **#21 module resolution** — conditional-exports / node16-vs-bundler / paths / symlink fixtures
  diffed structurally against `tsgo`'s resolved target. Owned at the U0 resolver rescope.
- **#22 propagation** — `any`/`never`/`unknown`/error absorption through each reducer, structurally
  diffed (`X | never = X`, `keyof never`, `any[K]`, distributive `any`). Owned per-reducer (U2
  reducers + U2.RELATION_INFER for the relation-side `any`/`never`).
- **#18 error tolerance** — NOT oracle-gated (tsgo also degrades on broken input); gated by
  Verter-internal `ReturnOnly` non-admission fixtures instead.

Per parent §6.3 the oracle is a per-family divergence budget (N cases / M divergence per family),
produced at each hard phase's rescope gate; this gate names the families needing rows (above), the
N/M numbers are set at the U0/U2.RELATION_INFER rescope sessions.

---

## Rescope of later phases (what this design forces)

- **U0 (foundation)** — gains TWO rescopes: (a) #18 broken-input taint PRODUCERS (parser
  error-recovery `SyntaxError` taint, resolver `MissingDependency`/`UnresolvedReference` facts,
  completion-fence `TornRead`) — FORK-B; (b) #21 module-resolution matrix IMPLEMENTATION in
  `resolver_core` + the split `resolve_env`/`lib_env` composition — FORK-C. This gate ships the
  SHAPE/keying; U0 ships the producers/resolver.
  **OWED CONCRETE DOC EDIT (at this gate):** the U0 block contract in
  `docs/arch/native-typeinfo-parity-u2-reducers.md` MUST be updated to OWN (a) the #18 taint
  producers and (b) the #21 module-resolution matrix as explicit U0 deliverables — this is a
  concrete edit owed at this gate, not just a prose fork. (This design doc does NOT edit the reducers
  doc; a later step lands that edit against the reducers doc directly.)
- **U2.RELATION_INFER** — consumes the `Relate` full-identity, `InferenceContextKey` SHAPE,
  `RelationKind`/`RelationPolicy`/`FreshnessKey` carriers, and the §22 `any`/`never` relation-side
  absorption defined here.
- **U3.CACHE_FACT_MODEL** — benches `cache_key_axes_are_minimal_and_normalized` (§3.6) and proves
  the `ReadSetSignature` tracer captures `FactDomain::ProgramAnalysis` + relation footprints.
- **U6** — adds `FlowReturn` / `ResolveCall` enum variants + spec rows + `SemanticQueryValue::
  {FlowReturn, ResolvedCall}` arms (additive; reuses the slot-identity SHAPE finalized here);
  `ContextualTypeAt`/`FlowNarrowingAt` behavior; `ThisType<T>` contextual binding.
- **U10.RESULT_DB** — lands the demand-lattice EXACTNESS publish gate
  (`cache_satisfaction_is_demand_lattice_not_enum_order`); this gate's multi-candidate dominance
  must keep it green.
- **U13** — published projection re-projects display (§14.4) from the cached typed value.

---

## (CRITICAL) rules this design establishes

Each NEW `(CRITICAL)` rule needs a registered guard (R6 meta-guard
`every_critical_rule_in_docs_has_registered_guard`). Distinguished from rules ALREADY covered by
existing CRITICAL sections.

**THREE-ARTIFACT LANDING REQUIREMENT (per new CRITICAL rule).** The R6 meta-guard scans `CLAUDE.md`
+ `.claude/skills/*/SKILL.md` ONLY — it does NOT scan `docs/arch/*`. A CRITICAL rule that lives only
in this design doc is INVISIBLE to the meta-guard and will not be enforced. Therefore each NEW
CRITICAL rule below must land THREE artifacts together, in the same change that lands the behavior:

1. a `(CRITICAL)` section (or bullet under an owning section) in `CLAUDE.md` OR a
   `.claude/skills/*/SKILL.md` — so the R6 meta-guard sees it;
2. a row in the `CRITICAL_RULE_GUARDS` registry naming the rule + its guard(s);
3. a named guard `#[test] fn` (the discriminating test in the proposed-guard column).

Landing only the doc text (artifact 0) does NOT satisfy R6. This is a hard precondition for each
rule below, not an optional follow-up.

**Genuinely NEW CRITICAL rules (each needs a guard):**

| Proposed rule | One-line | Proposed guard |
|---|---|---|
| **Typed value-domain (one key → one value domain)** | every `SemanticQueryKey` maps to exactly one `SemanticQueryValue` arm; no non-type-value smuggled into `GraphTypeNode` | `every_semantic_query_key_maps_to_exactly_one_value_domain` (+ `no_non_type_value_smuggled_into_graph_type_node`) |
| **Demand-lattice presets** | the five mode names are presets over `(ProjectionDemand, EvalPolicy)`; satisfaction/backfill by lattice dominance/meet, not enum order; `Skeleton` = `TypeParamShells`+carrier-stop (incomparable regime) | `query_modes_are_presets_over_projection_demand_eval_policy` + `skeleton_is_typeparamshells_plus_carrier_stop_not_special_mode` (+ `cache_satisfaction_is_demand_lattice_not_enum_order` at U10) |
| **Canonical display is projection, not stored/re-parsed string** | display is computed at publish from the cached typed value; `display_needs` is display-only and never drives resolution | `canonical_display_is_projection_not_stored_string` + `display_needs_is_display_only_never_drives_resolution` |
| **Error-tolerance ReturnOnly non-admission** | a result over torn/broken input or a torn read is `ReturnOnly`; a fact-rooted error (missing-dep) is cacheable | `error_tolerance_broken_input_is_returnonly_fact_rooted_error_is_cacheable` |
| **Module-resolution keys on resolve_env** | a module-resolution result keys on `resolve_env_hash` (+ `project_identity`), the dimension folding moduleResolution/exports-conditions/paths/symlink-policy | `module_resolution_keys_on_resolve_env_not_type_or_lib` |
| **Error/any/never propagation lattice** | `any`/`never`/`unknown` absorb per the §22 table and are cacheable; `error` relates bidirectionally and is ReturnOnly-prone when input-degraded (§18.4) | `error_any_never_propagation_lattice` (+ `error_type_is_returnonly_prone_any_is_cacheable`) |
| **Per-key `*Context` R21/R6-clean env discipline (two-tier)** | the slot carries its 3 intrinsic dims; each `*Context` adds ONLY the extra dims the QUERY depends on; no bundled config hash, no content/version/fact_dep on any key | `semantic_query_key_spec_table_equals_enum` (mechanical closure) + `cache_key_axes_are_minimal_and_normalized` |
| **Path-materialised-point satisfaction** | a warm hit serves a request ONLY iff dominated by a RECORDED materialised `(path, point)`; backfill writes ONLY recorded materialised points, never a meet-derived or nominal-request point | `cache_satisfaction_is_materialized_point_not_nominal_demand` (+ `backfill_writes_only_recorded_materialized_points`) |
| **Display-axis minimality** | `display_needs` is NOT a typed-value semantic axis; it is masked out of every typed-value family key and carried only on the display/publish key; two queries differing only in `display_needs` hit the SAME typed-value slot | `display_needs_is_display_only_never_drives_resolution` (+ `display_needs_masked_out_of_typed_value_family_key`) |
| **Resolve-env hash membership (no lib dims hidden in resolve_env)** | a module-resolution result keys on SPLIT `resolve_env` (moduleResolution/paths/baseUrl/conditions) + `lib_env` (typeRoots/types corpus) + `project_identity` + transitive `parse_env` (specifier list); lib dims are NEVER folded into resolve_env | `module_resolution_keys_on_resolve_env_not_type_or_lib` (+ `resolve_env_does_not_fold_lib_dims`) |
| **Error-carrier allowlist (no `ErrorType` wire arm)** | the error type rides the EXISTING `SemanticNodeData::Opaque(QueryError)` + §18 taint; no `GraphTypeNode::ErrorType` wire arm may be introduced (wire-purity closure) | `error_rides_opaque_no_new_error_type_wire_arm` |

**Already covered by existing CRITICAL sections (NOT new — reuse the existing guard):**

- Cache-axis minimality/normalization — already pinned by parent §6.2 / R21; this design references
  it (`cache_key_axes_are_minimal_and_normalized`).
- The five split env hashes (R21) and content-free query keys (R6) — existing
  `/type-cache-architecture` CRITICAL section; this design applies, does not re-establish them.
- One type-resolution engine / no second resolver / no query-time OXC — existing CLAUDE.md CRITICAL;
  the class-dual-space algorithm (§2.3) and display-is-projection (§14) are APPLICATIONS of it.
- The reserved native-checker seam — already specified by parent §3; pinned by
  `reserved_checker_queries_are_non_live_typeinfo_does_not_whole_body_check` (NON-LIVE, no spec row).

---

## Locked decisions

The core architecture is confirmed best-possible (two-tier env model, regime-stratified
meet-semilattice, display-as-projection, fact-rooted error line) and all four forks are LOCKED.
These are DECISIONS, not open questions:

- **Slot-keyed `Instantiate`/`ResolveMacroPayload`.** `Instantiate`/`ResolveMacroPayload` key
  `base`/`owner` on the env-bearing, content-free `ResolvedDeclSlotIdentity` (one identity type,
  R6-clean, closes the "env validity purely ReadSetSignature" gap), with the extra
  `resolve_env_hash` on the per-key `InstantiateContext` / `MacroPayloadContext`. The `provenance` +
  `merge_role` discriminators stay at FAMILY-IDENTITY level on `FamilyKey`, NOT demoted into a
  `*Context` (§2.1, §2.2 table).
- **FORK-B = U0 FOUNDATION.** The #18 broken-input taint PRODUCERS (parser error-recovery
  `SyntaxError` taint, resolver `MissingDependency`/`UnresolvedReference` facts, completion-fence
  `TornRead`) are owned by U0. This gate owns ONLY the value-domain SHAPE + `admit_decision`.
- **FORK-C = U0 RESOLVER_CORE.** The #21 module-resolution matrix IMPLEMENTATION lives in U0
  `resolver_core` (the conditional-exports walker, the symlink/realpath resolver, the
  `resolve_env_hash` composition). This gate owns ONLY the keying contract.
- **Lattice closure (CONFIRMED RESOLUTION).** The demand poset is NOT a full lattice: it is a
  disjoint union of per-regime bounded meet-semilattices with PARTIAL join (path-prefix-gated) and
  incomparable regimes (`generic_open`/`surface_role`/`merge_role`). `meet` is total within a regime;
  `join` is partial. **Partial join is ACCEPTABLE, not a defect** — the cache algebra needs only a
  decidable `⊒` (satisfaction, §3.4) + a within-regime total `meet` (backfill, §3.3). It never needs
  an arbitrary `join`. Stated, not papered (§3.2).
