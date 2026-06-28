# Semantic-DB-Overhaul — Unified Remaining-Work Plan

This is the single authoritative plan for the **remaining** work across the
two `refactor/semantic-db-overhaul` tracks: the **cache-runtime / scheduler**
track (`cache-runtime-overhaul-plan.md`) and the **semantic-type-graph** track
(`semantic-type-graph-plan-recovered.md`). It merges and sequences the
remaining items of both plans into one ordered backlog of 16 blocks (`U0`–`U15`)
and is the doc an orchestrator drives block-by-block.

It is composed from a binding codex merge decision (§A unified sequenced
backlog, §B Block-4 ↔ `SemanticQueryKey` co-sequencing, §C
`TypeInfoGraphResultDb` admission fork, §D MOOT adoptions, §E doc structure)
plus the two per-track landed-vs-remaining analyses. **This document SUPERSEDES
the two original plans for all REMAINING work.** The originals stay as
historical / per-item detail reference only.

### Scope: the target is FULL TypeScript-checker parity

**The TARGET is full TypeScript-checker-grade type parity** — relation /
assignability, inference (candidate accumulation / priority / fixation /
contextual-callback / reverse-mapped), measured variance, control-flow narrowing,
conditional / mapped / template-literal reduction, overload resolution, and the
cross-engine recursion that binds them. This is an **honest multi-person-year
scope**, not a flow-return patch. The bounded near-term lifts this plan sequences —
the **362-row ledger** plus the Vue-macro / component-meta / IDE surfaces — are the
**first measurable increments** toward that target, not the target itself.

**The full endgoal is Verter as a complete TypeScript replacement** (project model,
resolution parity, binder/nav, stdlib authority, checker, language service, emit) — framed
in §0.5, with the foundation / language-service / emit blocks named there and the
full-replacement done-bar in §10. **Native Emit Boundary (§0.5.2):** Verter natively owns
emit that is a PROJECTION over the one resolver — checker, language service, `.vue`
compilation, and `.d.ts` / `.d.ts.map` declaration emit (a CodeTransform producer over typed
facts) — and permanently DEFERS general, type-independent TS/JS transpile emit to
`tsc` / swc / esbuild / bundlers.

**The 362-row ledger tracks COVERAGE / wiring, NOT semantic tsc-parity.** A green
362 proves every row has an owner block, an executable proof, and a wired mechanism
(coverage completeness — "detects un-wired"). It does **not** prove the engine
agrees with `tsc`/`tsgo` on the SEMANTICS of those families (semantic completeness —
"detects wrong"). Semantic tsc-parity is gated separately by the **differential
`tsgo`-parity oracle** (§3.2 / parent `native-typeinfo-parity.md` §6.3), baselined
at each hard phase's rescope gate. State `362-green` and "tsc-parity" as two
distinct claims; never read one as the other.

### Native-typeinfo-parity doc set (indexed here)

The native TypeScript-parity typeinfo architecture is documented as one parent
plus four children. This unified plan is their **sequencing authority** and
indexes the whole set at the owning U-blocks below; each subplan links back here
and to the parent architecture via its standard subplan header.

| File | Owns | Owning U-block(s) |
|---|---|---|
| `docs/arch/native-typeinfo-parity.md` | **Parent architecture** — engine architecture, capability map, query/fact authority, the per-block contract template, the two-table manifest ledger, the git/CI landing protocol, the guards index | spans `U0`–`U15`; indexed at U0/U2/U6/U15 |
| `docs/arch/native-typeinfo-parity-u2-reducers.md` | U2 reducer / relation / utility / indexed / mapped / template / class / enum / module / JSX foundations + the U0 manifest/ledger substrate | U0, U2 |
| `docs/arch/native-flow-return.md` | U6 flow chapter — the demand-sliced `ReturnPathPeeker` (two-frontier model) and the flow IR | U6 |
| `docs/arch/native-typeinfo-parity-cache-export-session.md` | U3 / U8 / U10 / U11 / U12 / U13 — cache/fact model (U3), wire-surface closure (U8), result DB (U10), public relation/session (U11), exporter (U12), projection (U13) | U3, U8, U10, U11, U12, U13 |
| `docs/arch/native-typeinfo-parity-adapters-final-lift.md` | U14 / U15 — framework adapters, integrations, final lift | U14, U15 |

The foundation those documents build on is
`docs/arch/semantic-type-graph-plan-recovered.md` (graph / wire / cache
foundation).

### Design-gate output docs (indexed here)

The locked outputs of the rescope / design gates (§3.2). Each is the DESIGN
authority for its block — block contracts cite these instead of re-demanding
the gate; row/lift STATUS still derives from the §1.1 authorities.

| File | Owns | Owning block(s) |
|---|---|---|
| `docs/arch/u2-query-value-domain-design.md` | `U2.QUERY_VALUE_DOMAIN` design-gate LOCK — the typed value-domain shape, the `ProjectionDemand` / `EvalPolicy` demand lattice, the §18 broken-input taint / `admit_decision` model, the FORK-B / FORK-C ownership locks (#18 producers + #21 module-resolution impl → U0) | U0, U2 |
| `docs/arch/u2-relation-infer-design.md` | `U2.RELATION_INFER` locked design — `Relate` decided PERSISTENT (full-identity query-identity cache with the enumerated A.2 proof obligations), coinductive-SCC discharge, the RI-1..RI-10 implementation mini-DAG | U2 |
| `docs/arch/u6-flow-call-resolution-design.md` | U6 locked design — flow / call resolution over the U2/RI substrate, the cross-engine recursion + termination model, the narrowing join algebra | U6 |
| `docs/arch/u0-oracle-harness-design.md` | U0-FINISH-B oracle-harness design (LOCKED; the landed harness substrate — `ProofRequirement` registry, registry-bound driver-calling lifted row bodies, `oracle_core` driver — implements it) | U0 |
| `docs/arch/ts-compat-two-mode-model.md` | Oracle SEMANTICS authority — single-spec resolver (correct-by-default, no compat mode, no spec dimension on any cache key); TS-compat divergence = classified, review-gated DATA (tsgo snapshot + correction overlay + divergence registry) | U0; §3.4 |
| `docs/arch/u7-scheduler-submit-dag-decision.md` | U7 rescope-gate decision (LOCKED: DEFER — the multi-node `submit_dag` envelope held un-built; re-gate terms at U9) | U7, U9 |
| `docs/arch/u9-session-bridge-design.md` | U9 locked design — NO session bridge; single-node cache-node lowering into `SchedulerDag::submit`; the B7a leaf-primitive deletions | U9 |
| `docs/arch/parselower-design.md` | PARSELOWER staged migration design (LOCKED; 3/3 design-review validated) — delete `TypeExpr` from the HOT parse/shallow/macro/lazy-body/prepared caches; lower demanded OXC bodies + macro type-args to interned `HotTypeRef` handles in the `SemanticNodeData` arena (OXC AST stays worker-local `!Send`); `materialize_type_expr` is the SOLE reverse boundary (compat/JSON/test/diagnostic/output only); 9 stages — 1-4 non-breaking (additive / dormant dual-read), 5-9 breaking-but-each-gate-green | PARSELOWER |
| `docs/arch/b3-carry-forwards.md` | B3-review P3 carry-forward items (tracked follow-ups; detail reference, NOT a status or sequencing authority) | — |

### Stage5 cutover doc set (indexed here)

The Stage5+6 compiler-macro cutover (port `verter_compiler` macro lowering off the
parser `resolve_type/` OXC resolver onto the shared dispatch, deliver the
compiler-owned `ResolvedMacroSurfaces` DTO produced by the host, make parser macro
parsing spans-only, then delete the legacy rail) is documented as one plan. This
unified plan is their **sequencing authority** — the Stage5
blocks `S5.B1`–`S5.B12` are sequenced into the cross-plan order below (§3), with the
shared macro-surface gate `S5.B5` landing AFTER the typeinfo convergence gate U2.

| File | Owns | Cross-plan blocks |
|---|---|---|
| `docs/arch/stage5-cutover-plan.md` | **The Stage5+6 cutover plan** — macro DTO boundary, `resolve_macro_surfaces_for`, runtime-constructor relocation, parser-spans-only cutover, legacy `resolve_type/` deletion | owns `S5.B1`–`S5.B12` |

### Goto-definition overhaul doc set (indexed here)

The go-to-definition overhaul (typed `EmitOp` IDE-codegen mapping substrate, the
`SfcComponentAnchor` on `IndexedReady`, the reconciled `CompileSnapshotId`, and the
single definition engine) is documented as one plan plus two detail/reference docs.
This unified plan is their **sequencing authority** — the goto blocks `G.P1`–`G.P6/7`
are sequenced into the cross-plan order below (§3), with `G.P3` landing after U2,
`G.P4` after U3, and the `CompileSnapshotId` reconciliation + name disambiguation
stated there (§3.1.2).

| File | Owns | Cross-plan blocks |
|---|---|---|
| `docs/arch/goto-definition-overhaul-plan.md` | **The phased goto-definition plan (rev 2)** — phase contracts, file:line references, legacy deletions | owns `G.P1`–`G.P6/7` |
| `docs/arch/goto-definition-architecture-decision.md` | Binding architecture (detail/reference) — the V3-substrate decision, the `EmitOp` taxonomy, the one-definition-engine end-state. **NOT a sequencing authority.** | — |
| `docs/arch/goto-definition-review-findings.md` | Round-1 review findings + binding OQ resolutions (detail/reference). **NOT a sequencing authority.** | — |

`docs/arch/native-checker.md` is a **sibling follow-up** (non-parity): native
TypeScript-grade diagnostics as a LATER layer over the SAME resolver
(`SemanticQueryKey → ProjectSemanticDispatch::execute → SemanticGraphStore`),
sequenced AFTER the U0–U15 typeinfo-parity blocks. It is NOT part of the 362-row
parity scope and does not change the U-blocks; it only consumes the seams the parity
blocks reserve (`SemanticQueryValue::DiagnosticAnalysis`, the `Check*` query names,
the `ExecutableRegionId` region abstraction, the `ProgramAnalysisContributor` seam).
It is **RESCOPE-GATE-REQUIRED (effort weight HIGH, §3.3): deep design produced at
its own rescope session before its `Check*` queries implement** — its
diagnostics-algorithm depth + the per-family `tsgo`-oracle baseline are produced at
the gate, depending on the parity blocks (U2/U6) it layers over.

---

## 0.5 Native TypeScript Replacement Foundations (the full-replacement ENDGOAL frame)

This section frames the **endgoal**, the **ownership model**, and the **new foundation /
language-service / emit blocks** that complete it. It **EXTENDS** this plan; it does **not**
replace the 362-row typeinfo-parity scope (§Scope) or the rescope-gate governing process
(§3.2). The existing `U0`–`U15` backlog (§4) is the **typeinfo-parity INCREMENT** toward the
endgoal — the first measurable lifts — and the new blocks named here (`B.1`, `U0.RESOLVER_CORE`,
`B.4`, `B.13`, the emit boundary, `N0`, `N1`, `B.7`, `B.8`, `B.5`, `B.10`, `B.11`, `B.14`)
are the foundation, language-service, and emit layers that carry the increment to a full
TypeScript replacement. The cross-plan ORDER (§3.1.3) and dep-map (§3) thread these blocks in;
the per-block deep design is produced at each block's rescope gate (§3.2), exactly as the
`U2`/`U6`/`U7`/`S5.B5` phases are.

### 0.5.0 Endgoal

The TARGET is **Verter as a full TypeScript replacement** across the whole language-service
and compiler surface: the **project / program model**, **module / package resolution parity**,
the **binder / name-and-location index**, the **stdlib / intrinsics authority**, the
**typeinfo type-value engine**, the **checker** (diagnostics), the **language service**
(completion / hover / signature help / code actions / refactors / organize imports),
**emit** (the projection over the resolver — checker + LS + `.vue` compilation + `.d.ts`
declaration emit), **declaration emit**, and **JSDoc / JavaScript mode**. The `U0`–`U15`
typeinfo-parity blocks are the increment that lands the one resolver and its type-value
facts; this section names the foundation, language-service, and emit blocks that build on
that increment to reach the endgoal. Nothing here weakens a landed `(CRITICAL)` invariant —
one engine, typed-IR-only, shallow-by-default, the R21 five split env hashes, R6 rule↔guard
coupling, wire purity, and CodeTransform-as-sole-output-path are all **reinforced** by the
ownership statement and the acceptance gates below.

### 0.5.1 The ownership statement (LOAD-BEARING — the anti-side-path frame; B.15, resolves ledger #19)

The seven-surface ownership boundary is the single sentence that prevents a hidden
TypeScript-shaped side path. Each surface owns exactly one thing; no surface grows a second
resolver, a second expander, or a second cache authority:

> **Project model owns files/configs. Resolver owns module/package routes. Binder/index owns
> names and locations. Typeinfo owns semantic type values. Checker owns diagnostics. Language
> service owns editor orchestration. Emit owns generated JS/declarations.**

This is the resolution of **ledger #19** (reason-to-exist / authority boundary — "the deepest
strategic gap, root of the scope problem", §3.2.1) and pairs with **ledger #15** (the hard
typeinfo↔checker negative boundary). It is the framework-agnostic north-star of
`docs/arch/native-checker.md` §9 made concrete for the whole replacement, and it is consistent
with the one-engine rule (no surface owns a second resolver), shallow-by-default (binder/index
is the `IndexedReady` shallow inventory, not an eager expander), the typed-IR-only rule (every
semantic decision reads typed values, never display text), and the navigation zero-dispatch
guard (location surfaces never resolve types). It is written EARLY because it frames `U0` and
every foundation block, not deferred to the block that happens to need it last.

**Binder identity is produced BEFORE relation/checker consume it — and is DISTINCT from both the
merged-type-VALUE surface and the navigation projection (resolves the binder-ownership concern).**
The "Binder/index owns names and locations" surface decomposes into THREE layers, sequenced by the
demand-dependency graph, not a temporal phase:

1. **Binder IDENTITY — the named `BinderIdentityFacts` substrate (first-class, pre-`U2`, NOT `N0`-owned).**
   The layer-1 facts are NOT "somewhere inside `IndexedReady`" and they are NOT produced by `N0`: they
   are a **first-class, named substrate — `BinderIdentityFacts`** — **produced FROM `IndexedReady`** by
   `verter_semantic::analysis` and **CONSUMED BY the `U2` reducers BEFORE those reducers run** (it is a
   `U2`-tier *prerequisite* substrate, not a `U2` reducer output and not an `N0` projection). Its
   production has an explicit OWNER block: the **`U2.BINDER_IDENTITY_FACTS`** pre-reducer substrate
   sub-block (§4, the U2 block) — family-A production does not fall between gates. It is
   **demand-produced — NOT an eager whole-program binder pass** (the demand-driven core invariant and the
   one-engine rule forbid a second eager symbol authority). Producer vs carrier are distinct:
   `IndexedReady` holds the raw shallow symbol inventory + raw per-file provenance; `BinderIdentityFacts`
   is the demand-produced **typed projection** over it. Per the project's two-family cache rule
   (Cache-Architecture, R21 split-don't-bundle), `BinderIdentityFacts` is **not one cache** — it names
   TWO explicitly-keyed fact families plus the query-identity discriminator each feeds, never asserting
   both content-addressed and content-free keying of one entry:
   - **(family A) per-file binder facts — a content-addressed ARTIFACT cache**, keyed
     `(canonical, parse_stable_hash, parse_env_hash)` — a parse-stable content-addressed artifact in the
     `MemberSemanticFactStore` style (NOT the raw-`content_hash` form: `FileArtifactStore` itself is keyed
     `(canonical, content_hash, parse_env_hash, parser_version)`; family A rides `parse_stable_hash` so it
     is invariant under cosmetic edits). R6 does NOT govern artifact keys — it governs query-identity
     keys; an artifact key legitimately carries `parse_stable_hash`. It carries:
     - **lexical-scope identity** — the per-file scope tree + each scope's **stable structural scope id**
       (cosmetic-edit invariant, the analogue of `parse_stable_hash`; NOT a positional ordinal that
       renumbers when a scope is inserted above);
     - **declaration-slot SEEDS** — env-free `DeclarationSlotSeed` facts (the `BinderDeclSlotFact`
       payload) that are **stable, symbol-space-scoped** (a seed is identified within its
       declaration-space — value / type / namespace — so a value and a type sharing a name occupy
       DISTINCT seeds; the seed is NOT a raw name). The seed is **exactly the three env-free identity
       fields** of the landed `ResolvedDeclSlotIdentity` — `defining_canonical` + `merged_symbol_name`
       + `symbol_space` — and **deliberately omits every env dimension** (`project_identity`,
       `type_env_hash`, `lib_env_hash`). (Lexical-scope identity and contributor-order provenance are
       recorded by the OTHER family-A bullets, NOT folded into the seed — the seed feeds slot identity
       only.) Storing the fully-resolved env-bearing `ResolvedDeclSlotIdentity` in this parse-stable
       artifact would over-key it (an env-invariant artifact carrying an env-bearing payload) and be
       unsound; instead **`U2` DERIVES the env-bearing identity at query-key construction** — exactly
       `ResolvedDeclSlotIdentity = DeclarationSlotSeed + project_identity + type_env_hash + lib_env_hash`
       (the seed's 3 env-free fields + the 3 env dims = the landed six-field, R7 struct — ZERO ripple to
       landed code) — so the env dimensions enter ONLY the (content-free, R21-scoped) `SemanticQueryKey`,
       never family A;
     - **per-file declaration-merge + augmentation CONTRIBUTOR-ORDER provenance** — declaration
       source-order, overload-group membership, and the per-file module / global / ambient
       contribution-order facts, recorded at shallow-analysis time.
   - **(family B) ambient / global / lib-corpus COMPLETENESS facts — a separate corpus-scoped store**
     (NOT a single file's content identity — corpus completeness is whole-corpus, so keying it per-file
     would falsely validate when a *different* ambient contributor or the `lib` set changes). Keying a
     whole-corpus completeness answer by `lib_env_hash` + contributor-set ALONE is too weak — two
     projects, or two resolution modes within one project, could then share a negative/global answer they
     must not. So the family **SPLITS its key by completeness scope** (R21 split-don't-bundle):
     - **global / `lib` completeness:** `project_identity + lib_env_hash + contributor_set_fingerprint`
       (NO `resolve_env_hash` — global/`lib` enumeration performs no module-specifier resolution, and the
       fact-rooted fingerprint already roots contributor identity, so adding `resolve_env_hash` would
       over-key per R21);
     - **ambient / module-augmentation-target completeness:** `project_identity + resolve_env_hash +
       lib_env_hash + target + contributor_set_fingerprint` (mirroring the live
       `AugmentationTargetKey { project_identity, resolve_env_hash, lib_env_hash, population, target }` isolation —
       ambient/augmentation completeness IS specifier-resolution-sensitive, so it carries
       `resolve_env_hash`).

     Neither split key carries `type_env_hash`: name-enumeration completeness is a binder-level
     (pre-type-check) fact, independent of strict/target compiler options (R21 — a key includes only the
     dimensions its value depends on). The `contributor_set_fingerprint` is **FACT-ROOTED and
     schema/versioned** — derived from the recorded contributor FACTS (not an opaque parser/emitter side
     product) under an explicit schema version, so hidden parser/emitter drift cannot silently change the
     fingerprint without a version bump. Each split entry is `ReadSetSignature`-validated over its
     cross-file contributor facts. The
     store records whether the corpus a name could bind to has been fully enumerated, so a NEGATIVE
     (name-not-found) answer is backed by a recorded completeness fact rather than an un-rooted guess.
     This is the `B.4`-fed family (guard `ambient_global_and_lib_corpus_have_completeness_facts`, owned
     at the `B.4` gate).

   Each family is validated by `ReadSetSignature` (the sole cache-validity rail) over its own keying
   dimensions. The **`binder_scope_id` a context-sensitive query carries in its `SemanticQueryKey`
   identity is the QUERY-IDENTITY projection** of family A's stable structural scope id — it is a
   resolution-context discriminator (like generic type-args), content-free per R6, never a content/version
   hash; its R6-consistency rests on that scope id being a stable structural identity, not a positional
   counter (guard `binder_scope_id_enters_context_sensitive_query_identity`). The **cross-file merged contributor SEQUENCE** (TS
   binder order assembled OVER this recorded per-file provenance) is then computed and
   `ReadSetSignature`-validated by the `U2.MODULE_AUGMENTATION` reducer — child
   `native-typeinfo-parity-u2-reducers.md`, guard
   `declaration_merge_records_binder_overload_augmentation_order_as_facts` — which **reads
   `BinderIdentityFacts`, never re-derives the order from raw `IndexedReady`**. The ordering INPUTS
   (provenance + completeness) are thus available before `U2.RELATION_INFER`; the merged-VALUE assembly
   (which relates-through-`Relate`) is layer 2 below — binder IDENTITY does not depend on relation; the
   merged-VALUE surface does.
2. **Merged / ambient type-VALUE surfaces** — the queryable merged object surface
   (`ResolveMergedDeclaration` / `ResolveAmbientNamespace`). Producing a merged VALUE relates members
   through `Relate`, so the `U2.MODULE_AUGMENTATION` reducer legitimately lists `U2.RELATION_INFER` as
   a prerequisite. This relation edge is on the merged-VALUE surface, NOT on the binder-IDENTITY order
   facts of layer 1 — the two must not be conflated.
3. **Navigation / location PROJECTION** — `N0` is a **pure projection** that RENDERS location /
   navigation answers (def/refs/rename/symbols/highlights) **FROM `BinderIdentityFacts`** (layer 1). It
   is correctly POST-`U2` because it reuses the already-recorded ordering + slot facts ("does NOT fork a
   second ordering computation", §0.5.4 N0). `N0` is **NOT the producer of declaration identity, merge
   identity, declaration-slot identity, lexical-scope identity, augmentation identity, or route
   identity** — every one of those is a pre-`U2` `BinderIdentityFacts` (or resolver/route) fact that
   `N0` only reads; `N0` never WRITES a `SemanticQueryKey` query-identity fact or a route fact. It is a
   projection, not a second binder.

A separate pre-`U2` eager binder phase (a "`B.3a` before `U2`") is REJECTED: `BinderIdentityFacts` is
**demand-produced, not an eager pass** — re-introducing an eager whole-program binder would violate the
demand-driven core invariant ("collecting/indexing symbols, not eagerly evaluating"), add a second
symbol authority, broaden invalidation, fight the fact-cache, and re-sequence already-landed
slot-identity / reducer code. The binder-before-relation ordering is enforced by the demand-dependency
graph (`BinderIdentityFacts` entries are computed on demand before `RELATION_INFER` reads them through
`ProjectSemanticDispatch::execute`), not by a temporal pre-`U2` block. The block id `N0` denotes the
navigation projection (layer 3) — NOT the binder-identity producer (layer 1); for that reason its block
heading is named `N0.NAV_LOCATION_INDEX` (navigation / location), deliberately dropping "Binder" so the
name cannot be misread as the binder-identity owner.

**Planned guard (named as a deliverable, NOT added to `CLAUDE.md`/skills here):**
`ownership_boundaries_no_typescript_side_path` — registered with its `CRITICAL_RULE_GUARDS`
entry at the gate→implementation boundary of the foundation section's first implementing block
(per §3.2(e)), asserting no surface imports another surface's private resolver/expander/cache
authority.

**Planned binder-contract guards (named deliverables; per §0.5.7 each is committed with its
`CRITICAL_RULE_GUARDS` entry at its owning block's gate→implementation boundary, NOT added to
`CLAUDE.md`/skills now).** The binder substrate is not yet implemented, so each is a **named
future-acceptance guard** where it gates a not-yet-landed block, and a **fail-today discriminating
fixture** the moment its substrate lands (per §3.2(e) — the guard appears the moment the rule does, so
`every_critical_rule_in_docs_has_registered_guard` stays green). The eight guards pin the
`BinderIdentityFacts` ↔ `U2` ↔ `N0` contract above:

1. `binder_identity_facts_are_pre_u2_and_not_n0_owned` — `BinderIdentityFacts` is produced from
   `IndexedReady` and consumed by `U2` reducers; `N0` does not produce it (gate: `BinderIdentityFacts`
   substrate, `U2`-tier).
2. `u2_queries_do_not_read_n0_navigation_indexes` — the dependency edge runs `BinderIdentityFacts → U2`,
   never `N0 → U2`; no `U2` reducer reads an `N0` navigation/location index (gate: `U2`).
3. `n0_does_not_write_semantic_query_identity_or_route_facts` — `N0` is read-only over
   `BinderIdentityFacts` / resolver / route facts; it never WRITES a `SemanticQueryKey` query-identity
   fact or a route fact (gate: `N0`).
4. `declaration_slots_are_stable_symbol_space_scoped_facts` — family A stores env-free
   `DeclarationSlotSeed` facts: stable identities scoped to their declaration-space (value / type /
   namespace), not raw names, carrying NO env dimension (`project_identity` / `type_env_hash` /
   `lib_env_hash`). The guard asserts the seed is env-free AND that the env-bearing
   `ResolvedDeclSlotIdentity` is DERIVED by `U2` at query-key construction (seed + the three env
   dims), never stored in the parse-stable family-A artifact (gate: `BinderIdentityFacts` / `U2`
   slot-identity finalization).
5. `merge_order_and_augmentation_contributor_order_are_fact_validated` — merged-declaration order +
   module/global/ambient augmentation contributor order are `ReadSetSignature`-validated facts assembled
   over recorded provenance, never re-derived from raw `IndexedReady` (gate: `U2.MODULE_AUGMENTATION`;
   composes with `declaration_merge_records_binder_overload_augmentation_order_as_facts`).
6. `ambient_global_and_lib_corpus_have_completeness_facts` — the ambient / global / `lib` corpus carries
   recorded completeness facts in the corpus-scoped family-B store with SCOPE-SPLIT keys (NOT per-file
   content identity, NOT a single bundled key): global/`lib` completeness keyed
   `project_identity + lib_env_hash + contributor_set_fingerprint`; ambient / module-augmentation-target
   completeness keyed `project_identity + resolve_env_hash + lib_env_hash + target +
   contributor_set_fingerprint`. The guard asserts the split (a bundled `lib_env_hash`-only key fails)
   AND that `contributor_set_fingerprint` is fact-rooted + schema/versioned (gate: `B.4`
   stdlib/intrinsics authority — listed in `B.4`'s Guards deliverable, §0.5.4).
7. `negative_name_lookup_requires_recorded_completeness_or_returnonly` — a negative (name-not-found)
   binder answer must be backed by a recorded completeness fact AT THE MATCHING SPLIT SCOPE (the
   global/`lib` key for a global/lib name; the ambient/target key for an ambient / augmentation-target
   name), else it routes through `ReturnOnly` (never warms a cache as a falsely-authoritative miss; a
   completeness fact at the wrong scope does NOT authorize the negative) (gate: `BinderIdentityFacts` /
   `N0`).
8. `binder_scope_id_enters_context_sensitive_query_identity` — a query whose result depends on the
   lexical scope it is resolved from carries the `binder_scope_id` in its `SemanticQueryKey` identity as a
   **resolution-context discriminator** (the role generic type-args play), content-free per R6 and NOT a
   content/version hash. Its R6-consistency REQUIRES `binder_scope_id` to be a **stable structural scope
   id** (cosmetic-edit invariant), not a positional ordinal — the guard asserts that stability (gate: the
   context-sensitive `SemanticQueryKey` finalization, `U2`).

**Session / overlay augmentation is OVERLAY-AWARE (LANDED — the acceptance bar is the landed guard
trio, not a fail-closed gate).** The augmentation index is overlay-aware:
`FileArtifactStore::augmentation_index` is keyed by `AugmentationTargetKey { project_identity,
resolve_env_hash, lib_env_hash, population, target }`, where `population: AugmentationPopulation
{Base, Session(overlay-set fingerprint)}` keeps a session overlay's augmenters in a `Session` slot
(overlay ∪ base) distinct from `Base`. A session / overlay augmentation query (an unsaved-buffer or
overlay edit that adds/removes a `declare module` / `declare global` contributor) receives a REAL
stitched overlay-aware answer — NOT a fail-closed degradation and NOT a silently-stale base-only
answer; overlay augmenters never poison the base index and never cross sessions, and there is NO
base-only `session.is_none()` assert on this surface (CLAUDE.md §Declaration Augmentation is the
current-state authority). The landed acceptance bar is the guard trio
`session_overlay_augmenter_isolated_from_base_index` +
`effective_export_set_session_view_stitches_overlay_augmenter` +
`no_effective_export_set_base_only_session_assert` (registered under the Declaration Augmentation row
of `crates/verter_session/tests/g_misc0/critical_rules_have_guards.rs`). The formerly-planned
`session_overlay_augmentation_fails_closed_until_implemented` deliverable is RETIRED: its either/or
contract ("implemented OR fail closed until implemented") was discharged on the IMPLEMENTED arm, so
writing the fail-closed guard now would REGRESS landed behavior (a session view gets a stitched
answer, not `ReturnOnly`); it must not be reintroduced in any block's guard list or in the generated
block-contract table. This composes with the broken-code recovery contract (§0.5.3) and the
overlay-results-do-not-populate-base-caches Cache-Architecture rule (the query-identity
`EffectiveExportSetScope` stays content-free; overlay content identity is validated on the VALUE).

### 0.5.2 The Native Emit Boundary (B.9 — DECIDED, codex + claude converged)

**Verter natively owns all emit that is a PROJECTION over its one type resolver** — the native
checker + language service (semantic surfaces), `.vue` compilation (render-function emit
VDOM/Vapor + IDE TSX emit, already shipped), and future `.d.ts` / `.d.ts.map` declaration emit
(including for `.vue` components) as a `CodeTransform` producer over typed semantic values —
and **Verter permanently DEFERS general, type-independent TS/JS transpile emit** (`.ts`→`.js`
target/module lowering, helpers, import elision, decorator downleveling, JSX modes, interop
flags, bundler integration) to `tsc` / swc / esbuild / bundlers. This is the correct division
of labor, not a missing feature: Verter wins the SEMANTIC axis (the only tool that understands
`.vue` types + macro contracts) and refuses to compete on the commodity SYNTACTIC axis. A
`.d.ts` is a package's public SEMANTIC contract, not commodity syntax lowering, and `.vue`
always needs declaration emit that `tsc` never provides — so Verter must own it; `isolatedDeclarations`
is a fast path within that ownership, not a different decision. §Scope names this emit boundary;
`B.10` (declaration emit) is IN scope as a late block (below).

**Source FORMATTING is an explicit commodity non-goal** (parallel to transpile emit): the
TS-language-service formatting surface (`getFormattingEditsForRange` /
`getFormattingEditsForDocument` / `getFormattingEditsAfterKeystroke`) is type-independent syntactic
reflowing, NOT a projection over the one type resolver. Verter does NOT own it — it is deferred to
the editor's formatter (Prettier / `tsfmt` / the host's built-in formatter), exactly like commodity
transpile.

**The LS-surface owner set is EXHAUSTIVE-BY-CONSTRUCTION over surface ROLES** — not a closed
hand-list of surfaces (which can never be provably complete and leaks composed surfaces like
type-definition). Every TS language-service surface belongs to EXACTLY ONE role-owner by what the
surface fundamentally IS, applying the one dividing line (`N0` = identity/location, zero type walk;
the one resolver = every type query):

- **identity / location** (def / refs / rename, document symbols, document highlights, call
  hierarchy, semantic-token classification) → `N0` (navigation/location projection over
  `BinderIdentityFacts`; no type walk).
- **type-display & contextual** (hover, completion, signature-help, inlay hints) → `B.7` (routes
  every type query through the ONE resolver; never a nav-side or completion-side walker).
- **nav-by-type** (`textDocument/typeDefinition` — composes a typeinfo VALUE with a declaration
  LOCATION) → the **`N1`-orchestrated goto line**: the type-of-expression hop runs through the one
  engine (`ProjectSemanticDispatch`) and the location render comes from `N0`. It is neither pure
  location (so not `N0` alone) nor pure type-display (so not `B.7`) — `N1` composes the two, which
  is exactly its orchestration role.
- **diagnostics** → `B.5` (native checker).
- **edits / quick-fixes / refactors / organize-imports** → `B.8` (CodeTransform producers off
  checker diagnostics + `N0` shape).
- **semantic / declaration EMIT** (`getEmitOutput` with `emitOnlyDtsFiles` — `.d.ts` / `.d.ts.map`
  declaration emit, incl. `.vue` component declaration output) → `B.10` (a `CodeTransform` producer
  over typed semantic values, §0.5.2 above). This is the emit that PROJECTS over the one resolver, so
  it is a role-owner, not a non-goal — distinct from the commodity-syntactic arm below.
- **request orchestration** (snapshot / cancellation / degradation) → `N1`.
- **commodity SYNTACTIC, type-independent** (source formatting; transpile emit — the
  commodity JS-emit facet of `getEmitOutput`) → named non-goal (deferred to the editor formatter /
  `tsc`/swc/esbuild), per the boundary above.

The role test is the catch-all: any not-yet-named LS surface maps to exactly one role-owner by this
partition (identity/location → N0; type-query → B.7; value+location compose → N1 goto; diagnostic →
B.5; edit → B.8; semantic/declaration emit → B.10; orchestration → N1; type-independent syntactic →
non-goal), so no surface is delegated-by-omission and the §0.5.1 enumeration is complete by
construction, not by list length. The role-partition owner-set therefore EQUALS the
`language_service_api_manifest` owner-set (`N0`/`N1`/`B.7`/`B.8`/`B.5`/`B.10` + non-goal) — the manifest
is the mechanized form of this same partition.

**The role partition is the PRINCIPLE; a generated manifest MECHANIZES the coverage claim.** "Complete
by construction" is a reasoning argument, not an enforced gate — a concrete public surface
(`linkedEditingRange` / `prepareRename` / folding-range / selection-range / `implementation`) could be
silently omitted while the prose still "reads complete". The plan applies bijective-manifest rigor
everywhere else (the 362-row manifest, `checker_diagnostic_manifest_bijection`, the typeinfo wire
taxonomy-parity tests); LS surfaces get the SAME machinery: a **generated `language_service_api_manifest`**
that ENUMERATES every public `ts.LanguageService` method (pinned to the §3.4 / `B.4` TS version) and maps
each to its role-owner (`N0` / `N1` / `B.7` / `B.8` / `B.5` — the diagnostic methods
`getSemanticDiagnostics` / `getSyntacticDiagnostics` / `getSuggestionDiagnostics` are checker-owned —
or `B.10` for the emit facet: `getEmitOutput` SPLITS by facet — its `emitOnlyDtsFiles` declaration-emit
output is `B.10`-owned, while its commodity JS-emit facet is the §0.5.2 transpile NON-GOAL, §0.5.2)
or a named non-goal, gated by the NET-NEW
`language_service_api_manifest_covers_full_surface` coverage/bijection guard (a named deliverable at the
`N1` gate per §0.5.7). The manifest is GENERATED + diff-tested (like the typeinfo bindings), not a
hand-list — it ENFORCES the role partition rather than replacing it; an unmapped surface fails the gate.

**Deletion-ownership is per-surface, not deferred to the terminal sweep.** Each LS surface assigned
to an owner block above carries THAT block's Required-deletion of the corresponding runtime tsgo
`TypeProvider` path (`get_definition`/`get_references`/`get_rename`, `get_semantic_tokens`,
`get_document_highlights`, `get_inlay_hints`, `get_type_definition`,
`get_diagnostics` / `publish_merged_diagnostics`, …; call hierarchy is already native, so its
owner-block obligation is to STAY native with no tsgo path introduced, not a deletion), gated
BEFORE `§10` and registered with a guard at the owning block's gate (§0.5.4 names each). The
`§10` (7) deletion sweep CONSUMES these per-block deletions — it is the audit that none survive, not
the block that owns removing them. No assigned surface's tsgo path is left implicitly side-pathed to
the `§10` (1) catch-all.

**Planned guard:** `declaration_emit_derives_from_typed_facts_via_codetransform` — decl/`.d.ts`
shape derives from typed facts (binder/nav public-API shape + typeinfo VALUES) via CodeTransform,
never source-slicing, never `rawType`, never printed-type-string reparse; registered with `B.10`.

### 0.5.3 The global Broken-Code / Recovery contract (B.13 — generalizes ledger #18)

The editor steady-state is half-written source, so EVERY surface must produce a useful result
over syntactically / semantically broken, partial, stale, or mid-edit input. Ledger **#18**
(`u2-query-value-domain-design.md §18`, FORK-B) already designs this for the TYPEINFO QUERY
domain (`ResultTaint` Clean/Partial/Broken, `admit_decision`, taint join); `B.13` GENERALIZES
that into a **global broken-code / recovery contract spanning binder/nav, completion, checker,
and the language service**:

- **Producers stay `U0`-owned** (FORK-B): the broken-input taint producers are foundation work,
  not re-designed per surface.
- **Per-surface CONSUMPTION rules land WITH each surface block** — each surface declares how it
  degrades (the stage5 valid-empty-vs-unresolved distinction becomes one instance of this one
  taxonomy; see also the unified degradation taxonomy, ledger #9).
- **Nav "recovery" = location-graceful-degradation, NOT a typed-IR walk** — a location surface
  that cannot resolve a name returns a degraded location result; it never reaches for the type
  engine to "recover" (that would breach the navigation zero-dispatch guard).

**Planned guard:** `broken_code_recovery_contract_global_every_surface_degrades` — every
foundation/LS surface has a declared, tested degradation path; no surface hard-fails on broken
input and no nav surface escalates to type resolution to recover.

#### 0.5.3.1 The engine fault-containment contract (B.13 sibling — a CROSS-BOUNDARY invariant)

Broken INPUT (§0.5.3) and a broken ENGINE STEP are distinct failure classes; this names the
second as an invariant of admission-rule rank. **A reducer panic mid-
`ProjectSemanticDispatch::execute`, lock poisoning, or a partial unwind NEVER publishes memo /
cache entries, NEVER leaves in-flight or shared state poisoned (waiters are released, in-flight
slots and recursion stacks are unwound by RAII, no torn provisional value is observable), and is
CONVERTED to a typed degraded host result at EVERY public-API boundary** (host batch APIs, the
`_with_audit` entry points, NAPI/WASM FFI, the LSP handlers) — a panic is a degraded typed
answer to the caller, never a wedged engine and never a warm cache entry. The mechanical seams
already exist on the live tree (the semantic-query memo's in-flight panic guard + recursion-stack
RAII, `catch_unwind` at the host batch coordinator / scheduler execution / NAPI boundaries); this
contract PROMOTES them from per-seam implementation detail to one named cross-boundary invariant
so no new boundary (U10 result DB, U11 session surfaces, U12 exporter, the §0.5 LS blocks) ships
without it.

**Planned guard (named deliverable per §0.5.7, registered at the `B.13` gate):**
`engine_fault_containment_panic_never_admits_never_poisons_degrades_typed` — a discriminating
fixture panics inside a cold build and asserts: no cache entry was admitted, concurrent waiters
complete (no deadlock / poisoned lock), and the public boundary returns a typed degraded result.

Each block below carries a short contract — **scope / deps / sequence-position / required
deletions (if any) / named-guard deliverable** — matching the per-item rows of the integration
map. De-dup is already applied (`A.5≡B.2`, `A.10≡B.3`, `A.11≡B.6`, `A.12⊂B.15`, `A.3⊂B.12`):
each pair is stated ONCE here.

#### B.1 — Native Project / Program Model  (NET-NEW as a named block)

- **Scope:** the single project/program authority — **tsconfig DISCOVERY** (locating the governing
  `tsconfig.json` for a file), **`extends` / config INHERITANCE** (the full extends-chain merge,
  including package-relative `extends`), **solution-style configs** (a root config that only
  `references` leaf projects, with no own files), **normalized-option PRODUCTION** (the resolved,
  inheritance-merged, defaulted compiler-option set every downstream owner reads), include/exclude →
  root-file set, allowJs/checkJs file-graph gating, project-reference build-graph (composite /
  `tsbuildinfo`), lib selection as a program input, watch-driven program revalidation, AND
  **config-chain-edit invalidation** (an edit to ANY config in the discovery/extends chain
  re-derives the normalized options + root-file set under the canonical-dependency cache rule) —
  built on the landed `project_identity` cache-key + `IdeProjectConfig` (root / workspace_root /
  tsconfig_path / references / paths / baseUrl / Membership) substrate and the landed multi-project
  membership. `B.1` is the SOLE owner of discovery/inheritance/normalization — no other block
  re-derives configs.
- **Incremental builder-program state maps onto the existing demand-driven rails — NOT a second
  invalidation authority.** A full replacement needs the four `BuilderProgram`-equivalent
  responsibilities, but they are realized by the landed mechanisms, NOT by importing `tsc`'s eager
  `BuilderProgram`: **graph versioning** = `project_identity` + the workspace content generation;
  **affected-file calculation** = the fact-based **lazy revalidation** rail (`ReadSetSignature` +
  recorded facts revalidated against the live `StoreView`) — demand-driven, the inverse of `tsc`'s
  eager affected-set walk; **project-reference invalidation** = the canonical-dependency cache rule
  over the composite / `tsbuildinfo` build-graph (`tsbuildinfo` read/write semantics owned here);
  **emitted-artifact freshness** = `B.10`'s fact-VALIDATED declaration artifacts (content-addressed
  key; the value records the validated `ReadSetSignature` facts, revalidated on warm hit — the signature
  is never in the key; cross-project `.d.ts` freshness falls out of the same file-keyed fact mechanism,
  no affected-project graph needed).
  `B.1` NAMES these responsibilities and routes each through its existing rail; it introduces **no
  second invalidation authority** — VFS stays the file-change authority and lazy fact revalidation
  stays the staleness rail (reverse dependency graphs are NOT invalidation authority, per the Cache
  Architecture rules). A batch `verter --build` `tsbuildinfo` skip-without-full-load is a permitted
  later PERF optimization over this same rail, not an architectural second builder.
- **Deps:** none new (extends landed substrate). Feeds `U0.RESOLVER_CORE` (#21) + `B.4` (stdlib);
  emits the normalized-option set that `A.3`/`B.12` config semantics branch on.
- **Sequence:** PRE-`U0` / `U0`-tier; gates `U0.RESOLVER_CORE` and `B.4`.
- **Required deletions:** none (additive; it must NOT bundle a program-hash — it keys THROUGH
  `project_identity` + the R21 split, per `r21_no_bundled_config_hash`).
- **Guard:** `program_model_is_single_project_authority_keys_through_project_identity`.

#### U0.RESOLVER_CORE — Project / Package Resolution Parity  (A.5 ≡ B.2; promotes ledger #21 to a named block)

- **Scope:** the host-backed module/package resolution matrix to TS parity across the full
  moduleResolution mode set — **`Classic` / `Node10` / `Node16` / `NodeNext` / `Bundler`** — over
  relative / alias / project-refs (live) resolution PLUS the
  `exports`/`imports`/`typesVersions`/`paths`/`baseUrl`/`rootDirs`/`typeRoots`/`types`/
  `moduleSuffixes`/`customConditions`/`resolveJsonModule`/`allowImportingTsExtensions`/
  `allowArbitraryExtensions` config surface and the conditional `exports`/`imports`
  walker. It owns resolution SEMANTICS, not merely fixtures: **symlink / `preserveSymlinks`
  realpath resolution, pnpm / hoisted `node_modules` layouts, and workspace-linked packages**,
  and the **`package.json`-edit + in-place package-source-edit invalidation** path (an edited
  dependency manifest or dependency source re-resolves under the canonical-dependency cache
  rule). The full matrix is designed to executable depth as ledger **#21**
  (`u2-query-value-domain-design.md §21`, FORK-C LOCKED to `U0`); this promotes it to a named
  `U0`-tier block beside `U0.MANIFEST_SUBSTRATE`.
- **Deps:** `B.1` (program model) first. Shares the ambient corpus with `B.4` (resolve_env = WHERE
  to look; lib_env = WHICH corpus). KEYING stays contracted at the `U2.QUERY_VALUE_DOMAIN` gate;
  IMPL is `U0`.
- **Sequence:** `U0`, before `U2`; oracle-gated with one discriminating fixture per scope case —
  the moduleResolution modes (`classic` / `node10` / `node16-vs-bundler` / `nodenext`), the
  resolution-target cases (`relative` / `alias` / `project-references` (live, into a referenced
  project)) and the config-surface cases (`conditional-exports` / `conditional-imports` / `paths` /
  `baseUrl` / `typesVersions` / `rootDirs` / `typeRoots` / `types` / `moduleSuffixes` /
  `customConditions` / `resolveJsonModule` / `allowImportingTsExtensions` /
  `allowArbitraryExtensions`) PLUS the layout cases (`symlink` / `preserveSymlinks` / `pnpm-hoisted`
  / `workspace-linked-package`) PLUS the two edit-cycle invalidation fixtures (`package.json`-edit;
  in-place package-source-edit).
  The contract is **one discriminating fixture per scope case** (scope ⇄ fixtures ⇄ §8 row stay
  mutually consistent); the A.1 (4) rescope-gate deliverable backstops this enumeration — the gate
  rejects any scope case that lacks its discriminating fixture.
- **Required deletions:** none (extends the live resolver; no second resolution authority — CJS
  interop folds into this one matrix, per `B.11`).
- **`typeRoots` / `types` ownership — three concerns, split by env dimension (not a folding bug).**
  Listing `typeRoots`/`types` in this block's scope+fixtures is the resolution MECHANICS, not an
  env-hash ownership claim. The three concerns are allocated explicitly: **(a) module-specifier
  resolution** — `resolve_env` / this block; the module-specifier `resolve_env` NEVER includes
  `types`/`typeRoots`. **(b) ambient package inclusion / global type-acquisition corpus** — `lib_env`
  / `B.4` (the ambient inclusion-set / global corpus). **(c) `@types` package discovery + `typeRoots`
  walk** — this block owns the resolution MECHANICS (path-finding; the `typeRoots` discriminating fixture in the Sequence list above tests it), but
  the cached RESULT is keyed by `lib_env` (it selects WHICH corpus, not WHERE a specifier points),
  feeding `B.4`. This is the `resolve_env` = WHERE / `lib_env` = WHICH split working as designed.
- **Guards (already in the #21 design):** `module_resolution_keys_on_resolve_env_not_type_or_lib`
  + `resolve_env_does_not_fold_lib_dims` (`lib_env` — `typeRoots`/`types` — is NEVER folded into
  `resolve_env`); registered in `CRITICAL_RULE_GUARDS` at the `U0` gate.

#### B.4 — Standard Library / Intrinsics Authority  (promotes ledger #11 / #17 to a named U0-tier block)

- **Scope:** the formal stdlib authority — (a) `lib.d.ts` selection **pinned to a specific TS
  version**, (b) the `IntrinsicRegistry` as the formal intrinsics authority (the SDK audit already
  asserts every `= intrinsic` decl has a registry entry), (c) the JSX-namespace defaults owner, and
  (d) the `lib_env_hash` invalidation contract. The TS-version pin is the SAME pin as the §3.4
  `tsgo` parity oracle (single source).
- **Deps:** `B.4 ← {B.1, U0.RESOLVER_CORE}` — `B.1` (program model: lib selection is a program
  input) + `U0.RESOLVER_CORE` (the resolver feeds stdlib's shared ambient corpus), matching the
  §3.1.3 dep-map / §0.5.6 order `B.1 → U0.RESOLVER_CORE → B.4`. The R21 `lib_env` scoping rule is
  the authority for WHICH caches include `lib_env_hash` — do not redefine it.
- **Sequence:** `U0`-tier; gates the lib-fact-CONSUMING reducers (the
  `LibIntrinsic`-reading `U2` sub-blocks consume it) — NOT the landed
  `SemanticQueryKey` spine / value-domain layer (per-edge gating, §0.5.6).
- **Required deletions:** none (formalizes the live `IntrinsicRegistry` + `lib_env_hash` semantics).
- **Guards:** `lib_authority_pinned_ts_version_single_owner` (reuses the existing SDK audit guard) +
  `ambient_global_and_lib_corpus_have_completeness_facts` (§0.5.1 family-B guard, owned at this gate —
  the corpus-scoped completeness store uses scope-split keys (`project_identity + lib_env_hash +
  contributor_set_fingerprint` for global/`lib`; `project_identity + resolve_env_hash + lib_env_hash +
  target + contributor_set_fingerprint` for ambient / module-augmentation-target) with a fact-rooted,
  schema/versioned `contributor_set_fingerprint`, and records full enumeration, so a downstream negative
  name lookup is backed by a completeness fact at the matching scope, never an un-rooted miss; registered
  with its `CRITICAL_RULE_GUARDS` entry at the `B.4` gate per §0.5.7).

#### N0.NAV_LOCATION_INDEX — Navigation / Location Index  (A.10 ≡ B.3)

- **Scope:** the navigation/location PROJECTION surface — **identity and location ONLY, ZERO
  type expansion, ZERO typed-IR dispatch** (location ≠ type expansion: producing a definition /
  references / rename LOCATION is an identity-and-location answer, NOT a type walk, so `N0` owns it
  without becoming an expander). `N0` is a **pure projection over the pre-`U2` `BinderIdentityFacts`
  substrate (§0.5.1 layer 1)** — it READS the `BinderIdentityFacts` lexical-scope / declaration-slot /
  contributor-order / completeness facts (themselves produced from the `IndexedReady` shallow inventory)
  and reuses the `U2`-reducers' merged-declaration ordering facts (does NOT fork a second ordering
  computation); adds document/workspace symbols (as a substrate query), rename ranges, and validation
  tokens. **`N0` is NOT the producer of declaration identity, merge identity, declaration-slot identity,
  lexical-scope identity, augmentation identity, or route identity** — every one of those is a pre-`U2`
  `BinderIdentityFacts` (or resolver/route) fact that `N0` only reads; `N0` never WRITES a
  `SemanticQueryKey` query-identity fact or a route fact (guards
  `n0_does_not_write_semantic_query_identity_or_route_facts`,
  `u2_queries_do_not_read_n0_navigation_indexes` — the edge is `BinderIdentityFacts → U2`, never
  `N0 → U2`). It is NOT the merged-type-VALUE surface; it never re-derives ordering. A negative
  (name-not-found) location answer is backed by a recorded `BinderIdentityFacts` completeness fact, else
  it degrades / routes through `ReturnOnly` (guard
  `negative_name_lookup_requires_recorded_completeness_or_returnonly`) — it never escalates to the type
  engine to "recover" (§0.5.3 nav zero-dispatch). **`N0` is the native PRODUCER of definition /
  references / rename LOCATION answers** — it succeeds the
  **def/refs/rename LOCATION methods** of the tsgo-backed `verter_lsp::tsgo::TsgoNavigationBackend`
  (`getDefinitionAtPosition` / `getReferences` / `getRenameLocations`,
  `docs/arch/goto-definition-overhaul-plan.md` §Phase 5) that produce those locations TODAY, rather
  than sitting as an unowned layer over `TsNavigationBackend` (+ `SfcComponentAnchor`, G.P3). **`N0`
  does NOT own that backend's `getCodeActions` method** — code-actions are a B.8 surface (they need
  checker diagnostics + `CodeTransform`, not identity/location), so the code-action path of the
  struct is succeeded + deleted by `B.8`, not `N0` (see B.8).
- **Deps:** the pre-`U2` `BinderIdentityFacts` substrate (§0.5.1 layer 1) + `U2` (reuses its
  merged-decl ordering facts + `IndexedReady` finalized shape). `N0` consumes `BinderIdentityFacts`;
  it does not depend on the type-VALUE surface.
- **Sequence:** post-`U2`, ~alongside `G.P3`; before `N1`. Its `TsgoNavigationBackend`
  **def/refs/rename** deletion is gated BEFORE `§10` (a named deletion `§10` (1)+(7) depend on for
  navigation).
- **Required deletions (gated BEFORE §10):** the **def/refs/rename LOCATION paths** of
  `verter_lsp::tsgo::TsgoNavigationBackend` (`getDefinitionAtPosition` / `getReferences` /
  `getRenameLocations`) — once `N0` produces those locations natively, the tsgo def/refs/rename path
  is DELETED (no dual nav path, no fallback to a TS service). This is the named deletion that makes
  `§10` (1)+(7) satisfiable for navigation. The struct's `getCodeActions` path is NOT deleted here —
  it survives until `B.8` produces native code-actions and owns its deletion; the struct is fully
  removed once BOTH the `N0` (def/refs/rename) and `B.8` (code-actions) deletions land, both gated
  before `§10`. Carving def/refs/rename out of `§10` as a deferred non-goal is REJECTED — it
  contradicts the full-replacement endgoal. Preserves the nav one-engine guard; `N0` must not become
  an expander. **`N0` also owns the deletion of the aux identity/location tsgo `TypeProvider`
  paths it produces natively** — `TypeProvider::get_semantic_tokens` and `get_document_highlights`
  (`aux_features.rs`): once `N0` produces semantic-token classification and document highlights from
  binder/identity facts, those runtime tsgo paths are DELETED (gated before `§10`, no dual path).
  Call hierarchy is ALREADY native (`features/call_hierarchy.rs` reads `FileAnalysisSnapshot`; there
  is no tsgo call-hierarchy path to delete) — `N0` owns it as an identity/location surface and the
  guard asserts it STAYS native. These are identity/location surfaces (§0.5.2), NOT type walks —
  `N0` stays zero-dispatch.
- **Guards:** `nav_location_index_runs_zero_typed_ir_dispatch` + the projection-contract guards
  registered at the `N0` gate per §0.5.7 (named in §0.5.1):
  `n0_does_not_write_semantic_query_identity_or_route_facts` and
  `negative_name_lookup_requires_recorded_completeness_or_returnonly` — these pin `N0` as a pure
  read-only projection over `BinderIdentityFacts`. (The dependency-edge guard
  `u2_queries_do_not_read_n0_navigation_indexes` is `U2`-OWNED — registered at the `U2` gate, since it
  must hold the moment `U2` reducers run, before `N0` exists — and is listed in `U2.MODULE_AUGMENTATION`'s
  Guards, not here.) Plus the NET-NEW
  `native_navigation_replaces_ts_navigation_backend` (asserts def/refs/rename locations come from
  `N0` / `BinderIdentityFacts` and no tsgo **def/refs/rename** path survives — scoped to def/refs/rename,
  NOT the whole struct, since the code-action path is B.8's; registered with its `CRITICAL_RULE_GUARDS`
  entry at the `N0` gate per §0.5.7) + the NET-NEW
  `native_binder_surfaces_replace_ts_aux_nav_paths` (asserts semantic tokens / document highlights
  come from `N0`'s projection over `BinderIdentityFacts` and no tsgo `get_semantic_tokens` /
  `get_document_highlights` `TypeProvider` path survives, and that call hierarchy stays native with no
  tsgo path introduced; registered at the `N0` gate per §0.5.7).

#### N1.NATIVE_LANGUAGE_SERVICE_LAYER — Native Language Service Layer  (A.11 ≡ B.6)

- **Scope:** orchestration ONLY over typeinfo + `N0` + the checker — owns the request
  snapshot, cancellation, degradation routing, and public-API consistency. It is **not** a second
  engine: every type answer comes from the one resolver, every location answer from `N0`, every
  diagnostic from the checker. **`N1` owns the one nav-by-type composed surface,
  `textDocument/typeDefinition`** (§0.5.2): the type-of-expression hop runs through the one engine
  (`ProjectSemanticDispatch::execute`) and the declaration location render comes from `N0`. It is
  neither pure location (so not `N0` alone — it needs a type value) nor pure type-display (so not
  `B.7` — it returns a location), which is exactly the compose `N1` orchestrates; it adds NO second
  resolver.
- **Deps:** `U15` (typeinfo parity) + `N0`. Realizes ledger #19 (what native OWNS vs DEFERS) +
  #15; cites `native-checker.md` §9.
- **Sequence:** LATE — after `U15` + `N0`; before/with `B.7` / `B.8`.
- **Required deletions (gated BEFORE §10):** the tsgo `TypeProvider::get_type_definition` path
  (`nav_features.rs::handle_goto_type_definition`) — once `N1` produces type-definition by composing
  the one engine's type value with the `N0` location, the runtime tsgo type-definition path is
  DELETED (no dual path). (It MUST NOT import private reducers or write cache entries.)
- **Guards:** `language_service_layer_does_not_write_caches_or_import_private_reducers` + the NET-NEW
  `native_type_definition_replaces_ts_type_definition_path` (asserts type-definition composes the one
  engine's type value + an `N0` location and no tsgo `get_type_definition` `TypeProvider` path
  survives; registered at the `N1` gate per §0.5.7) + the NET-NEW
  `language_service_api_manifest_covers_full_surface` (asserts the generated
  `language_service_api_manifest` enumerates every public `ts.LanguageService` method, pinned to the
  §3.4 / `B.4` TS version, each mapped to its role-owner `N0`/`N1`/`B.7`/`B.8`/`B.5` (the diagnostic
  methods are checker-owned) / `B.10` (the `getEmitOutput` `emitOnlyDtsFiles` declaration-emit facet;
  its commodity JS-emit facet is the §0.5.2 transpile non-goal) or a named non-goal, with NO unmapped
  surface — this gate asserts
  owner-bijection COVERAGE only, NOT runtime-TS retirement: at the `N1` gate B.7/B.8 surfaces have not
  yet progressively cut over (§0.5.5), so "no residual runtime-TS path" is the §10(3) terminal bar +
  the per-owner deletion guards, not this coverage gate — §0.5.2; registered at the `N1` gate per
  §0.5.7).

#### B.7 — Completion / Hover / Signature-Help Semantics  (FUTURE Verter-engine successor to U15's tsserver/tsgo path)

- **Scope:** the native contextual completion / hover / signature-help semantics. **It does NOT
  remove U15's near-term tsserver/tsgo delegation** (see §0.5.5) — `B.7` is the LATER native
  successor, cut over progressively only once native contextual-type / inference facts are stable.
  Contextual completion needs contextual types, so `B.7` routes **every** type query through the
  ONE resolver (`ContextualTypeAt` / `ResolveCall` / `Relate`), never a nav-side or completion-side
  type walker. Dividing line: `N0` = location-only; `B.7`-types = through-resolver. The
  rescope-gate sub-surface inventory (the A.1 (1) checklist the gate enforces) is at minimum:
  candidate sources + auto-imports, member / property completions, JSX completions, template
  completions, the overload-selection policy, signature-help display parts, deprecation surfacing,
  broken-code completion, and **inlay hints** (parameter-name / type / return-type display, §0.5.2)
  — each routing its type queries through the one resolver.
- **Deps:** `N1` + `U2.RELATION_INFER` + `U6`.
- **Sequence:** after `N1` + `U2.RELATION_INFER` + `U6`; strictly LATER than U15's near-term
  delegation (which stays).
- **Required deletions (gated BEFORE §10):** the hover / completion / signature-help tsgo
  delegation is retired progressively as `B.7` reaches parity (not ripped out up front), AND the
  tsgo `TypeProvider::get_inlay_hints` path (`aux_features.rs`) is DELETED once `B.7` produces inlay
  hints through the one resolver (inlay hints are type-display, §0.5.2 — owned here, not a nav
  surface). Both are complete before `§10` (no residual runtime-TS type-display path).
- **Guards:** `completion_semantics_route_types_through_one_resolver` + the NET-NEW
  `native_inlay_hints_replace_ts_inlay_hint_path` (asserts inlay hints route every type query through
  the one resolver and no tsgo `get_inlay_hints` `TypeProvider` path survives; registered at the
  `B.7` gate per §0.5.7).

#### B.8 — Code Actions / Refactors / Organize Imports  (LAST LS block)

- **Scope:** native code actions, refactors, and organize-imports. Quick-fixes derive from the
  native-checker diagnostics; ALL edit generation goes through `CodeTransform` (the landed
  CRITICAL rule); rename-file import updates / organize / extract read the `N0` / project-model
  shape.
- **Deps:** `N0` / `N1` + the native checker (`B.5`).
- **Sequence:** LAST LS block — after `N0` / `N1` + checker diagnostics.
- **Required deletions (gated BEFORE §10):** the **`getCodeActions` path** of
  `verter_lsp::tsgo::TsgoNavigationBackend` — once `B.8` produces native code-actions, the tsgo
  code-action path is DELETED (no dual path, no fallback). This is the code-action half of the
  `TsgoNavigationBackend` retirement (the def/refs/rename half is `N0`'s, §0.5.4); with both landed,
  the struct is fully removed and the `§10` (7) deletion sweep has no residual tsgo nav/code-action
  path. `B.8` owns this deletion because it owns the replacement (N0 cannot produce code-actions —
  they need checker diagnostics + `CodeTransform`, not identity/location).
- **Guard:** `native_code_actions_replace_ts_navigation_backend_code_action_path` (asserts
  code-actions come from the native checker + `CodeTransform` and no tsgo `getCodeActions` path
  survives; registered with its `CRITICAL_RULE_GUARDS` entry at the `B.8` gate per §0.5.7) — plus
  the reused CodeTransform-is-single-source-of-truth rule + the `N1` guard.

#### B.5 — Native Checker Manifest  (POINTER into the replacement framing; does NOT rewrite native-checker.md)

- **Scope:** folds `docs/arch/native-checker.md` into the full-replacement framing — diagnostics
  are the **checker surface** of the endgoal (the ownership statement's "Checker owns diagnostics").
  This is a pointer, NOT a rewrite: the diagnostic-ROW manifest (codes / messages / categories /
  fixability) stays owned by native-checker's OWN rescope gate (effort weight HIGH, §3.3), produced
  at that gate, not here.
- **Deps:** AFTER typeinfo parity; consumes the reserved `Check*` / `DiagnosticAnalysis` /
  `ExecutableRegion` / `ProgramAnalysisContributor` seams (parent reserves them NON-LIVE).
- **Sequence:** after `U2`/`U6`/`U8`/`U10` facts stable, at native-checker's own rescope gate.
- **Required deletions (gated BEFORE §10):** the runtime tsgo diagnostics path — the
  `TypeProvider::get_diagnostics` delegation, its `get_diagnostics_background` background-priority
  variant (`tsgo/ipc.rs`), and the `publish_merged_diagnostics` tsgo merge
  (`sync_coordinator.rs` / `background_init.rs` / `sync_orchestration.rs`). Once the native checker
  produces diagnostics, the LSP publishes native diagnostics ONLY; the tsgo `get_diagnostics` merge
  is DELETED (no dual path, no belt-and-suspenders union). Diagnostics are the checker-role surface
  in the §0.5.2 per-surface deletion-ownership rule — `B.5` owns this deletion because it owns the
  native diagnostic producer. (The diagnostic-ROW manifest itself: new `Check*` keys beyond the
  reserved set need a `schema_version` bump and must NOT merge into the 362-row typeinfo manifest.)
- **Guards (at its land):** `checker_diagnostic_manifest_bijection` (manifest↔emitted bijection,
  DISTINCT from the 362 guard) + the NET-NEW `native_checker_replaces_ts_diagnostics_path` (asserts
  published diagnostics come from the native checker and no tsgo `get_diagnostics` /
  `publish_merged_diagnostics` runtime path survives — the bijection guard is compatible with a
  still-merging tsgo path, so a SEPARATE deletion guard is required; registered at the `B.5` gate per
  §0.5.7).

#### B.10 — Native Declaration Emit  (IN scope per B.9; one of the LAST blocks)

- **Scope:** `.d.ts` / `.d.ts.map` generation — public-API-shape extraction, re-export
  preservation, `stripInternal`, JSDoc preservation, and Vue/SFC component declaration output —
  as a `CodeTransform` producer that reads binder/nav public-API shape (`N0` / `IndexedReady`) +
  typeinfo VALUES. It NEVER source-slices, reads `rawType`, parses printed type strings, or runs
  over IDE TSX as semantic source (typed-IR-only). `.d.ts.map` maps public declarations to original
  user spans; render-fn maps, IDE-TSX maps, and declaration maps stay SEPARATE products; declarations
  cache as artifacts keyed by canonical file + parse/compiler/profile identity + the five split env
  hashes (incl. `lib_env_hash`, since decl emit depends on lib data) + decl policy + source-map
  policy; the cached VALUE records the validated `ReadSetSignature.facts` (revalidated against the live
  `StoreView` on every warm hit) — the fact signature is NEVER part of the key (R6 / Cache-Architecture:
  content-addressed artifact keys never carry a fact signature). **Declaration-conformance diagnostics are
  CHECKER-owned, not re-derived in emit:** faithful `.d.ts` parity couples the emit DECISION to
  declaration-conformance diagnostics (TS2742 unnameable-inferred-type, private-name leakage,
  visibility / exportability, `isolatedDeclarations` errors) which in `tsc` are checker-produced
  (`getDeclarationDiagnostics`) and consumed by the emitter. Per the ownership statement ("Checker
  owns diagnostics. Emit owns declarations"), `B.10` CONSUMES these from `B.5`'s `CheckDeclaration`
  declaration-conformance subset (`docs/arch/native-checker.md` §2 — the checker query layer, the
  `CheckDeclaration` key) and never spawns a second diagnostic producer in emit.
- **Deps:** `B.9` decision (in scope) + typeinfo VALUES stable + `N0` + (for SFC) `U14` + **`B.5`
  (the `CheckDeclaration` declaration-conformance diagnostic subset)** — the emitter reads
  checker-produced conformance facts; `B.5` already sequences before `B.10` (§0.5.6), so this is a
  dep edge, not a re-sequence.
- **Sequence:** one of the LAST blocks — after typeinfo values + `N0` + `U14` + `B.5`.
- **Required deletions:** none net-new; transpile emit is the permanent non-goal (§0.5.2).
- **Guards:** `declaration_emit_derives_from_typed_facts_via_codetransform` + the NET-NEW
  `declaration_emit_non_goals_are_registered_stop_gates` (every declaration-emit non-goal is a
  registered `decl_emit_*_stop_gate.rs` row with owner + public degradation + reason + exclusion from
  the replacement-acceptance set, mirroring the §9 Svelte/React stop-gate discipline; registered with
  its `CRITICAL_RULE_GUARDS` entry at the `B.10` gate per §0.5.7).

#### B.11 — JSDoc / JavaScript Mode  (EXPLICIT multi-slice umbrella — NOT one atomic block)

`B.11` is an umbrella over THREE independently-sequenced slices, each gated at its own point (it is
deliberately NOT a single block landing at one position): the **JSDoc-type-system slice** (near
`U2`), the **`checkJs` JS-mode slice** (after `B.5`), and the **CJS-interop slice** (folded into the
`#21` matrix at `U0`/`U2`). The deps/sequence rows below enumerate the three slices.

- **Scope:** (a) JSDoc `{Type}` payload text-parse is **ALREADY the sole text exception** under the
  Typed-IR-Only rule — do NOT widen it to JS-source regex/slicing; (b) JSDoc type CONSTRUCTS
  (`@template` / `@typedef` / `@satisfies` / `@import` as type-bearing decls) resolve through the
  ONE engine, lowered once at shallow analysis exactly like `lower_ts_type` (near `U2`); (c)
  `checkJs` / `allowJs` mixed-project diagnostics ride the native checker (`B.5`); (d) CommonJS
  interop folds into the `#21` module-resolution matrix (`U0.RESOLVER_CORE`) — NOT a second
  module-resolution authority.
- **Deps:** JSDoc-type-system near `U2`; `checkJs` after `B.5`; CJS interop with `#21` at `U0`/`U2`.
- **Sequence:** JSDoc-type-system near `U2`; `checkJs` JS-mode after `B.5`; CJS at `U0`/`U2`.
- **Required deletions:** none (upholds one-resolver + typed-IR-only; `{Type}` stays the SOLE text
  exception).
- **Guard:** `jsdoc_and_js_mode_resolve_through_one_engine_jsdoc_payload_only_text`.

### 0.5.5 U15 succession note (binding — near-term tsgo delegation STAYS)

U15's hover / completion / signature-help currently DELEGATE to tsserver/tsgo. **That is the
NEAR-TERM and STAYS** (see the U15 block, §4, and the forward-pointer there). `B.7`
(Completion/Hover/Signature-Help Semantics) + `N1` (the Native Language Service Layer) ARE the
FUTURE Verter-engine architecture that supersedes it, sequenced as a LATER phase. The succession
is progressive: U15's tsgo path is retired surface-by-surface, to the extent parity allows, only
as `B.7` reaches parity through the one resolver — it is never ripped out up front. This
gradual "to the extent parity allows" cutover is a NEAR-TERM narrative ONLY; the §10 terminal
bar (item (3)) is unconditional — at §10 done, ALL in-scope tsgo delegation MUST be fully
retired with zero residual runtime-TS path.

**Navigation (def / refs / rename) + code-action succession.** The tsgo-backed
`verter_lsp::tsgo::TsgoNavigationBackend` today serves FOUR methods across TWO native owners:
`getDefinitionAtPosition` / `getReferences` / `getRenameLocations` (def/refs/rename LOCATION
answers) → `N0.NAV_LOCATION_INDEX`, which PRODUCES those locations from binder/identity facts
(location ≠ type expansion, so `N0` owns this without becoming an expander); and `getCodeActions`
(code-actions) → `B.8`, which produces native code-actions from checker diagnostics + `CodeTransform`
(`N0` cannot produce code-actions). The struct's deletion is therefore SPLIT into two
Required-deletions, each owned by the block that produces its replacement and each gated BEFORE
`§10`: `N0` deletes the def/refs/rename paths; `B.8` deletes the `getCodeActions` path. Unlike the
hover/completion/sig-help delegation above — which is retired surface-by-surface as `B.7` reaches
parity — each nav/code-action deletion is a single outright cutover (no dual path) at its owner's
gate; the struct is fully removed once BOTH land. `§10` (1)+(7) depend on these two named deletions
for navigation + code-actions.

**The other live tsgo `TypeProvider` LS paths follow the same per-producer succession** (§0.5.2
deletion-ownership rule, §0.5.4 per-block contracts): `get_semantic_tokens` /
`get_document_highlights` → `N0` (call hierarchy is already native — owned by `N0`, no tsgo path to
delete); `get_inlay_hints` → `B.7`; `get_type_definition` → `N1` (the nav-by-type compose);
`get_diagnostics` / `publish_merged_diagnostics` → `B.5` (the native checker). Each tsgo path is
deleted by its producing block, gated BEFORE `§10`, with a registered guard — so no live
LS/diagnostics tsgo path is left to the `§10` (1) catch-all. `§10` (1)+(7) depend on
these named per-surface deletions for the full LS + diagnostics surface, not only navigation +
code-actions.

### 0.5.6 Sequencing of the foundation / replacement blocks (relative to U0–U15 / S5.B* / G.P*)

This reproduces the integration map's sequencing diagram, threaded coherently with §3.1.3:

```
FOUNDATION (U0-tier; PER-EDGE gating — see the rule below this diagram):
  Ownership statement (B.15, §0.5.1) + ledger #19 boundary ─┐  (frames everything)
  B.1 Program Model ─► U0.RESOLVER_CORE (A.5/B.2, #21) ─► B.4 Stdlib/Intrinsics Authority
  B.13 Broken-Code/Recovery (global contract; U0-owned producers)
  B.9 Native Emit Boundary (decision; stated at §Scope)
  A.1 rescope-rejection rubric (§3.2 meta)

→ U0 → U1 → U2 (CONVERGENCE GATE) → [existing U3..U15 / S5.B* / G.P*]
       │  BinderIdentityFacts (§0.5.1 layer 1) — demand-produced FROM IndexedReady,
       │  CONSUMED BY the U2 reducers BEFORE they run (NOT eager, NOT N0-owned;
       │  production OWNED by the U2.BINDER_IDENTITY_FACTS substrate sub-block)

AFTER U2:            N0.NAV_LOCATION_INDEX (~G.P3) — pure projection over BinderIdentityFacts
  A.3/#20 strict matrix + B.12 config matrix  → produced AT the U2.RELATION_INFER gate
  A.2/A.7/A.8/A.9 → folded into the U2.RELATION_INFER #1/#2/#5 cache-admission algebra
  A.4/#7 warm-hit validity → BLOCKING the U3 + U10 gates
  A.6/#9 degradation taxonomy → contract BEFORE U12/U9/U11

AFTER typeinfo parity (U15):
  B.5 Native Checker Manifest (its own rescope gate)
  B.11 JSDoc-type-system (near U2) / checkJs JS-mode (after B.5) / CJS (#21 at U0)
  N1.NATIVE_LANGUAGE_SERVICE_LAYER
  B.7 Completion/Hover/SigHelp semantics (after N1 + U2.RELATION_INFER + U6; U15 tsgo STAYS)
  B.10 Native Declaration Emit (after typeinfo values + N0 + U14 + B.5 CheckDeclaration)
  B.8 Code Actions / Refactors / Organize Imports (LAST LS block)

TERMINAL:
  B.14 Replacement Acceptance Gates (§10; fenced from §9)
```

**Foundation gating is PER-EDGE, not a blanket phase prerequisite.** The arrows above are
the real edges: `B.1` gates `U0.RESOLVER_CORE` + `B.4` (config / program derivation
consumers), `U0.RESOLVER_CORE` gates the `#21` resolution-semantics fixtures and feeds
`B.4`'s ambient corpus, and `B.4` gates the lib-fact-CONSUMING reducers (the
`LibIntrinsic`-reading `U2` sub-blocks). NO foundation block retroactively gates
already-landed work: the `U2` `SemanticQueryKey` spine, the value-domain layer, and the
`U0` manifest / oracle substrate landed with zero foundation blocks built — consistent
with this graph, because those artifacts consume no foundation edge. A block that
consumes resolver / lib / global / binder facts names the EXACT foundation edge it
requires in its own contract; "foundation before `U0`/`U2`" as a blanket phase rule is
RETIRED.

Every guard named in §0.5 is a **block/gate deliverable**, committed together with its
`CRITICAL_RULE_GUARDS` registry entry at the owning block's gate→implementation boundary (per
§3.2(e)), so `every_critical_rule_in_docs_has_registered_guard` stays green the moment a rule
appears. This plan edit does **NOT** add any `(CRITICAL)` heading to `CLAUDE.md` or
`.claude/skills/*` — pre-adding a `(CRITICAL)` rule with no landed guard would trip the R6
meta-guard and violate the gate-only rules-update rule (§3.2(e)). The §8 documentation-update
map carries each block's land-time owning-doc updates.

---

## 1. Status authority index

This section is an index of authorities, not a hand-maintained block-status ledger.
It must not copy current status words such as LANDED / NOT-STARTED / PARTLY-LANDED.
At every block entry, derive state from the authority of record at current `HEAD`.
If this plan and an authority disagree, the authority wins and this plan is stale.

### 1.1 Typeinfo parity authorities

| Scope | Authority of record | Derivation / gate |
|---|---|---|
| `TypeInfoParityBlockId` DAG (per-block prereqs, consumed mechanisms, `required_guards`, `verification_labels`) | `crates/verter_session/tests/manifest_data/typeinfo_parity_blocks.rs`, generated by `scripts/gen-typeinfo-ignore-manifest.py` | Block contracts derive from the generator, never from §1 prose; the generated table is the sole writer's output (`--check` freshness). |
| `IgnoredTestRow` manifest (per-row lift state) | generated `crates/verter_session/tests/manifest_data/typeinfo_ignored_test_manifest_rows.rs` + the consuming `crates/verter_session/tests/typeinfo_ignored_test_manifest.rs` | The 362 TOTAL is guard-pinned (`ignored_test_row_table_holds_exactly_362_rows`); the ignored/lifted split is DERIVED from `IgnoreStatus` and the live `#[ignore]` bijection. This doc never freezes today's ignored or lifted count. |
| `AdditionalProofRow` table | generated `crates/verter_session/tests/manifest_data/typeinfo_additional_proof_rows.rs` | Closed proof-only table (`additional_proof_row_table_holds_exactly_7_rows`); never counted as ignored rows. |
| Landing evidence | squash-merge commits carrying the `Typeinfo-Block: <block-id>` trailer | A typeinfo block is landed iff its trailer commit exists AND its rows are `Lifted` AND its `required_guards` pass. No tracked cursor, no hand status. |

### 1.2 Non-typeinfo authorities

| Block range | Authority of record today | Rule |
|---|---|---|
| U1 / U4 / U5 (cache-runtime scheduler / cache work) | the live tree + this plan's U1/U4/U5 block contracts (§4 — each names the symbols/tests whose presence is checked at HEAD) | `docs/arch/cache-runtime-overhaul-plan.md` is NOT a status authority: its own header SELF-SUPERSEDES to this plan (citing it for status would be circular) — it remains detail/historical reference only, and its scheduler-DAG portions are additionally superseded by the locked `u7-scheduler-submit-dag-decision.md` / `u9-session-bridge-design.md`. |
| U7 | `docs/arch/u7-scheduler-submit-dag-decision.md` | The locked DEFER decision doc is the authority. |
| U9 | `docs/arch/u9-session-bridge-design.md` | The locked design doc is the authority. |
| S5.B1–S5.B12 | `docs/arch/stage5-cutover-plan.md` | — |
| G.P1–G.P6/7 | `docs/arch/goto-definition-overhaul-plan.md` | The architecture-decision / review-findings docs are detail references, NOT status authorities. |
| Foundation / replacement blocks (`B.*`, `N0`/`N1`, `U0.RESOLVER_CORE`, native-checker) | this plan's owning §0.5 text, plus each block's named locked design doc once its gate produces one | — |

Do not copy status values into this table. If an owning doc has a `Status:` header,
cite the doc; do not duplicate the value. Locked design-gate OUTPUT docs for typeinfo
blocks (e.g. `u2-relation-infer-design.md`, `u6-flow-call-resolution-design.md`,
`u0-oracle-harness-design.md`, `u2-query-value-domain-design.md`) are indexed in the §0
doc map — they are DESIGN authorities; row/lift status still derives from §1.1.

### 1.3 Regeneration and verification

- `python3 scripts/gen-typeinfo-ignore-manifest.py --check` (alias
  `pnpm gen:typeinfo-manifest:check`) — generated-table freshness.
- `pnpm gen:typeinfo-manifest` when manifest source data changes (the generator is
  the SOLE writer of the `manifest_data/` files; the Rust tests only diff and fail).
- `cargo nextest run --workspace` + `cargo test -p verter_session --tests` — the
  canonical gate pair (§7).

### 1.4 Branch / tip binding rule (orientation, non-authoritative)

**Branch:** `refactor/semantic-db-overhaul`. This plan does NOT pin a "current tip"
SHA — the branch advances under a concurrent workstream, so any named SHA goes stale
the moment it is written. Instead, the recent history is recorded as HISTORICAL
WAYPOINTS for orientation only: `aba794d9` was the native-typeinfo full-TS-parity
doc-set land (the U0–U15 parent + four children); `cec9cfa4` (a v-on-handler
closing-suffix compiler fix) and `0a68332c` (a serena-memories commit) are later
commits on top of it; `51a49a35` (the prior scheduler-dispatch tip) and `b36e0835`
(the B7c land point) are older references still. None of these is a "current
authority" SHA. **Binding rule:** this plan does not run the workspace and does not
assert a test count at any specific commit — the live baseline AND the live tip are
RE-DERIVED at each block's entry gate against the then-current `HEAD` (per the env
re-home rule, §3.1.4), never pinned to a frozen SHA (see "Known-failure baseline"
below).

### 1.5 Landed-substrate reference (historical waypoints — orientation only, NOT a status authority)

This subsection is a write-time record of WHAT the earlier blocks delivered (the
substrate names and shapes the remaining blocks build on). It is orientation, not a
status ledger: per the §1 rule, current state derives from the §1.1 / §1.2 authorities
at `HEAD`, and on any disagreement this record is the stale party.

**Cache-runtime / scheduler track — Blocks 1–6 + B7a/b/c (delivered substrate):**

- **B1** — `WorldSnapshot` request-identity type (never enters a cache key) +
  plan-vocabulary guard (H19 `no_phase_archaeology_in_production_code`).
- **B2** — `cache_runtime/` substrate: `ArtifactNode` / `QueryNode` traits +
  `CacheAdmission<V>` + `cache_runtime::lookup`. The legacy
  `cooperative_admission` module is GONE; `singleflight.rs` +
  `cooperative_admit_with_post_publish` + `ComputeAdmission` are the H14
  singleflight substrate.
- **B3** — typed `SignatureAdmission::{Cacheable, NonCacheable}` in
  `fact_signature_helpers.rs`; `ReadSetSignature.overflowed: bool` carrier;
  `NonAdmissionReason` re-exported from `verter_audit`.
- **B5** — public `CompileCacheMode` (`Stateless` / `Content` / `Session`) +
  `classify_compile_mode` wired at `virtual_file_pipeline.rs`; typed
  `DowngradeReason` / `SourceMapPolicy`.
- **B6 — Phase 6 COMPLETE** — `HostCpuPool` (§6a), `submit_batch_atomic` (§6b),
  `upsert_many_with_priority` atomic-upsert cutover (§6c, `host_upsert.rs`),
  `HostBatchCoordinator` (deadlock fix), §6d∪§6e finalization. The per-call
  `CompileBatchOptions.threads` option was REMOVED outright (concurrency is now
  construction-time `HostConfig::host_cpu_threads`).
- **§7 / §7b — unified `SchedulerDag`** (single readiness/admission/reservation
  authority, H21): 3-variant `WorkNodeIdentity`, 5-variant
  `WorkKind{Load, Parse, Analysis, Artifact, CacheNode}`, cooperative
  pump. `queue.rs` / `JobIndex` / `BlockerRegistry` /
  `Submission::BlockerResolved` DELETED.
- **B7a** — leaf substrate (additive, unwired): `cache_id.rs` (opaque
  `SchedulerCacheId(pub u64)`), `cancellation.rs`, `cpu_concurrency.rs`
  (`CpuConcurrencySemaphore` + `CpuConcurrencyPermit`), `dedupe_hook.rs`
  (`DedupeHook`), `SubmissionResult<T>`.
- **B7b** — DAG readiness lanes + weighted credit (4 priority lanes,
  deficit/credit replacing the linear scan). `DagAgingConfig` /
  `effective_priority` DELETED. `DagCapacityBudget` is the SOLE ledger.
- **B7c** — pool topology: injected `Arc<SchedulerCpuPool>` /
  `Arc<SchedulerIoPool>` + nonblocking `try_submit`; 3-pool topology
  (HostCpuPool=External, SchedulerCpuPool=CpuWorker, SchedulerIoPool=IoWorker).
  `IoPool` / `IoHandle` / blocking `execute` DELETED.

**Semantic-type-graph track — A0a CONTRACT substrate (delivered on a pre-existing
`SemanticGraphStore` foundation):**

- The **`SemanticGraphStore` / `ProjectSemanticDispatch` / `Relate`-memo**
  foundation and the **five query modes** (`Identity` / `Navigate` / `Shallow` /
  `Expanded` / `Skeleton`) pre-exist and are the SOLE query-time type resolver.
- **A0a — typeinfo wire + audit CONTRACT substrate:**
  `crates/verter_protocol/proto/verter/v1/typeinfo.proto` rewritten
  (`GraphTypeNode` 32-arm oneof, `StructuredTypeExpression` 22 arms,
  `TypeInfoGraphRequest` 7 arms, `TypeInfoRequestError` 11 variants,
  `FrameworkSurfacePayload`, capability handshake, reserved field directives);
  `verter_protocol/src/typeinfo/graph.rs` (Rust re-exports, `Graph*`-prefixed);
  `verter_audit` extended with `RequestKind::TypeInfoGraph`=9 +
  `RequestKindPayload` + closed tag enums + 3 `StructuredAuditEvent` variants +
  `KindBit::TypeInfoGraph` + batch aggregator + regenerated `audit.generated.ts`;
  `request_validation.rs` shape-only validator (closed schema-version gate +
  exhaustive structured-expression coverage); the wire-contract guards
  (taxonomy parity, byte-equal TS freshness, audit parity, request validation);
  the `Typeinfo Wire Contract` CRITICAL rule registered in `CRITICAL_RULE_GUARDS`.
- **reconcile-#5 (CF-2 reviewed-clean):** the audit footprint payload was
  reshaped from 9× `exactness_*: u32` scalar fields to one
  `exactness_counts: BTreeMap<ExactnessTag, u32>` map field.

**SLOWPERF WAVE-1 — cold per-file artifact-build dedup (delivered):**
`ensure_indexed_ready_serve`'s materialise closure is the SINGLE per-file cold build —
one eval-program parse (`parse_eval_program`, the sole host parse entry; arena
stack-local on the singleflight flight), one `EvalEnv` (script-setup type params
applied; stored on `IndexedReady` behind the `eval_env()` accessor;
`base_eval_env_arc` collapses to the artifact read), one `ShallowFileState`
(the `from_analysis_inner` `parse_and_build_env` fallback is REMOVED), one
publication. The route-owned shallow system (`RouteOwnedShallowDb` /
`ensure_route_owned_shallow_entry` / its singleflight / the
`route_owned_generation` token dimension / the `HostStoreView` route-owned
`Route`-fact fallback) and the legacy `EvalEnvCacheDb` (legacy `Arc<EvalEnv>`
surface + producer-less `Arc<OwnedEvalProgram>` shell) are DELETED.
`IndexedReady` carries a `project_generation` stamp; route-resolution mutations
drive an **edge-refresh materialise** (retained content-addressed payload, route
surface rebuilt via `build_indexed_route_surface`, fresh stamps, pre-publish
fence with ReturnOnly) instead of a full re-parse. Counters
(`eval_program_parses` / `eval_env_builds` / `shallow_state_builds` /
`indexed_ready_materializes` / `indexed_ready_edge_refreshes` / `sfc_parses`)
+ the `cold_artifact_dedup_tests` suite + the
`no_production_route_owned_shallow_system` /
`no_production_parse_and_build_env_in_session` guards pin the contract.
**WAVE-2 (demand-scoped decl-body lowering) stays QUEUED** — one eager
`EvalEnv` build per file version remains the documented design until then.
Deferred follow-ups (lead-architect ruled, tracked here):

- Re-plan the producer-less `OwnedEvalProgram` / `TypeResolutionContextDb`
  owned-artifact substrate together with the WAVE-2 demand-scoped lowering work.
  That re-plan MUST require query-time context construction to carry
  `source_type` from the SELECTED artifact/view inputs, not from a
  scheduler-base re-read after input selection: today
  `resolve_external_type_from_indexed_ready_with_view`
  (`host_manage/prepared_decl.rs`) selects its inputs through the session view
  (possibly overlay) and then computes `source_type` via
  `imported_eval_source_type_for` (`host_manage/eval_program.rs`), which
  prefers the scheduler-base `authoritative_source_type_for` — an overlay
  `.vue` whose `<script>` `lang` differs from the base (e.g. base `ts`,
  overlay `tsx`) builds its type-resolution context under the BASE
  generation's `source_type` while parsing the overlay bytes. Acceptance
  includes an overlay `.vue` lang-flip regression proving no scheduler-base
  `source_type` is used after overlay input selection.
- VFS-cold `.vue` lane still pays TWO script-program parses (residual after the
  flight-shared `VueScriptProgram` threading): the scheduler WORKER's ingress
  parse inside `ensure_loaded` (`host_executor.rs` `execute_source` →
  `parse_vue_snapshot`) plus the flight's own eval-program parse. The worker's
  per-file OXC arena is dropped after lowering and must never enter host
  caches, so it cannot be threaded into the flight's borrowed eval program
  without either retaining a `!Send` arena in host state (forbidden by the
  IndexedReady core invariant) or relocating the `EvalEnv` typed lowering into
  the worker. Resolution path: that worker-side lowering relocation / an owned
  `Send`-safe program artifact — paired with the `OwnedEvalProgram` /
  `TypeResolutionContextDb` WAVE-2 re-plan bullet above.
- Repo-wide cleanup of pre-existing phase vocabulary in
  `crates/verter_session/src/**` as a dedicated cleanup item. Known
  pre-existing sites (not introduced by the slow-cold-dedup branch):
  `project_type_store.rs:360`, `types.rs:2165-2173`,
  `host_upsert.rs:1117/1310`, `meta_resolve.rs:166`,
  `meta_resolve/scoring.rs:332`, `semantic_query.rs:1501`.
- `set_import_dependencies` invalidation precision: the wrapper now stamps
  `project_generation` only on an ACTUAL route change (value-idempotent
  `replace_exact_resolutions` + route-table no-op oracle) and drains only the
  owner's derived layers (no wide evict). Remaining follow-up: evaluate whether
  a CHANGING push should additionally evict the dependents that resolved
  through the retargeted specifiers (today they re-validate read-side via the
  generation-gated fact rails — correct, but a targeted reverse-dep drain
  could cut the first warm-miss recompute).
- Migrate the typeinfo oracle-core snippet parsers onto a shared probe-parse
  helper: the parser-boundary guard
  (`no_direct_oxc_parser_calls_outside_scheduler_path`) is now import-resolving
  (bare/grouped/item-aliased/glob/module-aliased `use oxc_parser…` forms),
  blanks inline `#[cfg(test)]` modules, and pins each allow-listed file to its
  exact site count — so `typeinfo/oracle_core/hover_extract.rs` (1), `gen.rs`
  (1), and `admission.rs` (2) are visible, counted, allow-listed tracked debt
  rather than evasions. The remaining work is the migration itself, not guard
  coverage.
- **Oracle-lift of the seven `U2.ENUMS` parity rows (typeinfo).** The
  enum-family reducer is COMPLETE — all seven scoped enum projections resolve
  correctly (numeric/string/const member → branded literal; `${Enum}` → value
  union; `keyof typeof Enum` → member-name union; the `Extract<…>[…]`
  discriminant → its arm payload) — but NONE oracle-lift: the Ts7Oracle
  admission gate rejects every enum hover shape. Three (numeric/string/const
  member) are fundamentally non-liftable under tsgo's nominal enum-member hover
  (`Reject(EnumMemberOrQualified)` — the SAME class as the landed
  `class_features.rs` generic-class-instance defer); four (`${Enum}`, `keyof
  typeof Enum` ×2, the indexed-access discriminant) hit the gate's GLOBAL
  deferred-construct rejects and need a dedicated oracle-infra block (expansion
  probes + a `keyof typeof Enum` source-walk carve-out + `DistributiveIdentity`
  cross-check relaxation) to lift at most four of seven. The seven rows stay
  `Ignored` under `U2.ENUMS` (manifest total still 362). Lead-architect ruling
  2026-06-13: DEFER — the oracle-harness extension is a separate block; it does
  NOT block landing the complete reducer.
- **Cross-file value-export read-set admission (shared rail; surfaced by the
  `U2.ENUMS` cross-file `Enum.Member`).** The cross-file/barrel `Enum.Member`
  projection (`E.A` for an enum re-exported through a barrel) resolves the value
  through the SHARED `effective_prepared_value_decl` → `resolve_value_export_target`
  chase that `typeof E` already uses. That chase resolves the visible value
  correctly (verified, with an invalidation test) but does not visibly admit the
  resolved leaf decl into the consuming semantic query's read-set / reverse index
  (`build_instantiate` records consumer decl/args/augmenters; the value-export
  chase + prepared-decl warm reads do not bubble the leaf fact), so a warm
  consumer could in principle serve stale after a leaf-only member-value edit.
  This is a PRE-EXISTING property of the shared cross-file value-export rail —
  identical to `typeof E`'s pre-existing behavior, NOT enum-specific; the clean
  fix is system-wide read-set/fact-bubbling for cross-file value-export chases,
  not an enum-local patch. Lead-architect ruling 2026-06-14: DEFER — a dedicated
  shared-rail follow-up; it does NOT block landing SPINEENUM (the enum `E.A` reuse
  correctly matches `typeof E`). Residual nonblocking P3s tracked with it: the
  shared `static_name` helper panics on a TS-invalid substituted computed-template
  enum-member name (`` [`a${x}b`] ``) — pre-existing, shared by `index_enum` (sound
  fix: return `Option` and skip on both rails in lockstep); and cross-file
  enum-AS-A-TYPE (`${ImportedEnum}` / an imported enum in type position) is
  mechanically covered but lacks a direct test.
- `ResolvedImportFactsKey.known_miss_generation` carries a generation inside a
  cache key (R6-adjacent); move the negative-miss freshness to value-side
  validation semantics.
- Fenced-publication class-completion: `ResolvedTypeCacheDb`
  (`host_resolve/external_type_resolution.rs`) is not yet fact-validated /
  captured-stamp / fenced-serve-aware — bring it onto the same
  flight-captured-stamp + fenced-serve admission rails as the other shared
  caches, or delete it. Ruled DEFER by the lead architect at SLOWPERF round 6.
- Test-determinism cleanup: migrate the pre-existing wall-clock assertions
  (`component_meta_caches_tests.rs:171,:222-227`, `meta_tests.rs:12904+`) to
  counter-based pins / `verter_bench` measurements.
- **EXTERNAL-TYPE-RESOLUTION ENGINE CUTOVER (delete query-time OXC
  `ResolvedElements`; build typed-IR external resolution through the shared
  5-mode dispatch).** One legacy query-time OXC `ResolvedElements` external-type
  path — `resolve_external_type_from_indexed_ready_with_view`
  (`host_manage/prepared_decl.rs`) plus the frontier materializer in
  `host_resolve/external_type_resolution.rs` / `frontier_engine.rs` — still
  resolves external types at query time instead of routing through the single
  `SemanticQueryKey → ProjectSemanticDispatch::execute → SemanticGraphStore`
  five-mode dispatch. This is a REAL, long-standing single-engine-rule violation
  (the "Exactly one type-resolution engine" CRITICAL rule): it is PRE-EXISTING at
  `2dc03a529` and earlier, grandfathered by the
  `no_new_resolved_elements_production_file` shrinking-ledger guard, and was
  surfaced (not introduced) by the WAVE-2 demand-lowering escalation. It is
  ORTHOGONAL to WAVE-2 correctness — WAVE-2 keeps it working and does not grow it;
  it is not a WAVE-2 regression. It CANNOT be deleted mechanically in the
  demand-lowering pass because it is currently load-bearing; the replacement
  (typed-IR external resolution through the shared 5-mode dispatch) must be built
  first. Tracked here as a dedicated follow-up block; it is NOT a sanctioned rule
  exception — the single-engine CRITICAL rule states the target invariant and this
  live path is a debt against it, to be retired by the dedicated engine cutover.
- **WHOLE-ENV CONSUMER BOUNDED-DEMAND CUTOVER (convert the `whole_env()`
  env-shaped legacy adapters to bounded per-symbol demand surfaces).** The
  lazy `DeclBodyMemo::whole_env()` whole-file `EvalEnv` build
  (`decl_body_memo.rs`) — exposed via `base_eval_env_arc`
  (`host_manage/eval_env.rs`) — is M3-SANCTIONED for WAVE-2 (it is a lazy,
  on-demand product, NOT publish-time eager lowering; `IndexedReady` carries no
  `eval_env` and `indexed_ready_publish_lowers_zero_decl_bodies` pins zero
  publish-time body lowering). It is therefore NOT a WAVE-2 landing blocker, and the
  current behavior is result-correct and strictly better than base: base eagerly
  built and stored the whole-file env for EVERY file at publish, whereas now only
  TOUCHED files lower it, once, memoized — bounded to LOCAL body lowering and never
  triggering cross-file `Expanded` materialization (projector publication stays
  shallow-by-default). The PRIMARY / most-hit consumer is `local_type_declaration_id`
  (`host_manage/eval_env.rs`): it is reached on EVERY `get_component_meta` resolution
  (via `resolve_ref_to_root_identity` → `base_eval_env_arc` → `whole_env()` — the hot
  meta path), not merely fallthrough/alias, and it can route through header/prepared
  identity. The remaining consumers are env-shaped legacy adapters rather than true
  whole-file algorithms and could likewise run on bounded per-symbol demand:
  `peel_value_decl_alias` (`host_manage/eval_env.rs`) follows ONE value-alias
  chain and can route through `prepared_value_decl` / `ShallowFileState::value_decl`
  + header presence; fallthrough's `build_fallthrough_eval_env_lightweight`
  (`host_manage/fallthrough.rs`) clones the full base env then narrows to required
  root/template names and only uses it for bounded ops (root-spread consumption,
  generic prop overrides, dynamic root candidates). Follow-up block: build the
  bounded fallthrough/runtime value env from required owner/imported bindings +
  prop overrides; rewrite value-alias peeling over prepared/value decls; add tests
  proving fallthrough + alias peeling do NOT materialize `whole_env()` or lower
  unrelated declarations; then fence `base_eval_env_arc`/`whole_env()` as an
  explicit compatibility/debug demand only, or delete it if no true whole-file
  consumer remains. Lead-architect ruling 2026-06-13: DEFER (real over-broad
  semantic-demand debt, outside WAVE-2's settled publish-zero cutover).
- **COMPILE-TIER AUGMENTATION R29 COMPLETENESS CUTOVER (active augmentation-target
  population on the compile rail; cover all target kinds + the cold Session path).**
  The compile-tier augmentation rail `observe_augmentation_fingerprints`
  (`compile_fact_emission.rs`) PASSIVELY snapshots the augmentation index, returns on
  empty, and observes the `ModuleAugmentationIndexShape` fingerprint (plus, since
  WAVE-2, the per-augmenter `FileWholeHash`) ONLY for `ExternalSpecifier` rows that a
  prior Content classification already populated. Two pre-existing gaps follow: (a) a
  FIRST cold Session compile (no prior Content classification) can publish a compile
  slot with NO augmentation fact at all, because the Session prefetch path
  (`host_resolve/virtual_file_pipeline.rs`) resolves/import-indexes script deps but does
  not call `ensure_augmentation_index_populated`; (b) the rail's `by_external` filter +
  `_ => None` arm ignore resolved-relative, wildcard-ambient, and global augmentation
  targets, whereas the semantic/Content closure-aware probe (`host_cache_runtime.rs`)
  handles all kinds. Both behaviors PRE-DATE WAVE-2 at `2dc03a529` (the passive
  snapshot, the empty-return, the `ExternalSpecifier`-only `filter_map`, and the
  index-population dependency all exist at base); WAVE-2's e7e4c458a only ADDED the
  per-augmenter `FileWholeHash` observation inside the already-existing, already-populated
  external loop — it did not create the cold-population hole nor narrow the rail. The
  WAVE-2 regression test (`compile_slot_invalidates_on_external_augmenter_member_type_edit`)
  intentionally warms the index via a Content classification first, so it does not
  exercise the cold Session path. Lead-architect ruling 2026-06-13: DEFER (real
  pre-existing compile-rail coverage gap, NOT a WAVE-2 regression). Dedicated follow-up
  block: replace the passive snapshot / external-only compile observation with active
  augmentation-target derivation that calls `ensure_augmentation_index_populated`,
  observes the shape + per-contributor `FileWholeHash` for external, resolved-relative,
  wildcard, and global targets through the shared target-key mapping, fails closed when
  the target set is unverifiable, and adds cold Session tests with no prior Content
  classification.
- **DECL-BODY LOCATOR GRANULARITY CUTOVER (replace top-level statement locators
  with declaration/block-child locators).** WAVE-2's lazy body memo currently
  records contributor locators as top-level `stmt_index` values and lowers each
  demanded contributor via `lower_top_level_statement`, so a demand for one
  declarator in `export const a = ..., b = ...` or one inner declaration in a
  `namespace` / `declare module` block can lower and coverage-backfill
  same-statement siblings. This is real bounded-demand debt: it leaves common
  multi-declaration `.d.ts` shapes as coarser eager islands and makes the
  "demanded declaration" shorthand true only at statement granularity. It is
  DEFERRED because the current behavior is content-addressed, coverage-gated, and
  result-correct; it does not lower bodies at publish, does not serve stale bodies
  across edits/overlays, and does not under-invalidate. It is therefore not a
  WAVE-2 landing blocker for the sanctioned publish-zero / parse-once /
  lazy-first-demand cutover. Follow-up target: extend `DeclHeaderIndex`
  contributors from `stmt_index` to real declaration locators for variable
  declarators, namespace block children, and augmentation block children; add
  selective lowerers and per-locator dependency/raw-surface capture; keep backfill
  only for entries whose full locator set was actually lowered; add regressions for
  same-statement declarators, namespace blocks, and augmentation blocks.
  Lead-architect ruling 2026-06-13: DEFER (real per-symbol lazy-body
  locator-precision debt surfaced by the review6 codexA P1; the R6 scope-consult
  ruled it a new row that does NOT fold into WHOLE-ENV CONSUMER BOUNDED-DEMAND
  CUTOVER, which concerns `whole_env()` consumers, not the per-symbol lazy-body
  path's locator precision).

### 1.6 Known-failure baseline (recorded at the B7c land; re-derived at implementation)

The recorded historical baseline is the **long-standing 8-failure cluster recorded
at the B7c land (`b36e0835`)** — all long-standing and **OUT OF SCOPE for this
unified plan** (a Block-1.A `fact_dep_signature` substrate migration cluster carried
forward since reconcile-#2/#3):

- `compile_tier_signature_carries_*` ×5
  (`member`, `member_presence`, `import_ref`, `route_surface`,
  `module_augmentation_index_shape`)
- `family_a_fact_validation` ×2 (`family_a_entries_carry_fact_dep_signature`,
  `family_a_warm_hit_uses_fact_validation`)
- `materialise_structure_entry_carries_dep_signature` ×1

This is the **historical reference point** as of `b36e0835`, NOT a verified count at
the current tip. The tip has advanced well past `b36e0835` (through the
scheduler-dispatch work and the `aba794d9` typeinfo doc-set landing), and this plan
does NOT itself run the workspace — so implementation **re-derives the live baseline
at each block's entry gate at the then-current tip** (the separate later effort), and
treats the cluster above as the recorded starting set to reconcile against, not a
frozen count to assert at any specific commit.

**Named owner of the carried cluster:** `U3.CACHE_FACT_MODEL` (the canonical
fact-signature model block) owns reconciling any residual `fact_dep_signature`-era
failure in this cluster; the per-block entry-gate baseline re-derivation (§3.1.4) is
the detection rail. The named test fns exist on the live tree
(`tests/fact_matrix/compile_tier_*.rs`, `tests/g_family/family_a_fact_validation.rs`,
`tests/g_misc0/materialiser_observes_or_dies.rs`); whether any currently fails is
re-derived at the entry gate, never asserted here.

Plus one **environment-only** non-failure: `typeinfo_ts_bindings_*` fails in a
`node_modules`-less worktree (it regenerates TS bindings via the workspace `buf`
binary) and **PASSES on the main checkout with `node_modules` present** /
post-`pnpm install`. It is not a code failure.

Every block's verification expects ZERO NEW failures over the baseline re-derived at
that block's entry gate (at the then-current tip), not a count frozen to an old commit.

---

## 2. Binding decisions & MOOT adoptions

These are transcribed from the codex merge decision (§C, §D). They are binding
on every block; do not relitigate or resurrect superseded items.

### 2.1 `TypeInfoGraphResultDb` admission fork (§C)

**Decision: singleflight NOW, with NO later retarget to `submit_dag`.**

`TypeInfoGraphResultDb` admission belongs to the **cache-runtime
singleflight / fact-validation substrate**, specifically:

- `cooperative_admit_with_post_publish` (`cache_runtime/singleflight.rs`)
- `InflightTable` (`cache_runtime/singleflight.rs`)
- `BoundedCandidateMap` (`bounded_query_retention`)
- `FactReadSet::finalise` → `FactReadSetFinalise`
- `SignatureAdmission` (via `SignatureAdmission::from_finalise`)

B7e `submit_dag` is **scheduler execution / readiness plumbing, not a second
cache-admission authority**. The typeinfo DB must NOT be folded into the
`CacheNodeDag` / `submit_dag` design now or later. This resolves the single
biggest cross-track sequencing question: the semantic-graph execution layer
binds to the already-landed singleflight substrate and is therefore largely
PARALLEL to the remaining scheduler work (U1/U7/U9).

### 2.2 MOOT adoptions (§D — do NOT resurrect)

- **Adopt the A0a `Graph*` wire names** (`GraphTypeNode`, `GraphMergedDeclaration`,
  …) and the **single `TypeInfoGraphRequest` envelope** (7-arm oneof). The
  plan's bare-`TypeNode` / per-request-message proto naming is superseded.
  `TypeInfoRequestError` is 11 variants (A0a), not 8.
  `PredicateSubjectIdentifier` → `PredicateSubjectName` rename is landed.
- **Use `exactness_counts: BTreeMap<ExactnessTag, u32>`** (reconcile-#5 / CF-2).
  The `exactness_*: u32` scalar field list (§10 / A.17 of the semantic-graph
  plan) is MOOT.
- **Build the typeinfo DB / fence (U10) on `FactReadSet::finalise` +
  `SignatureAdmission` + `HostStoreView`.** Do NOT resurrect the retired
  `finalise_signature_or_empty` helper (it collapsed Overflow→Empty, a
  correctness defect) nor the retired request-view globals (`CURRENT_REQUEST_VIEW`
  and the `_in_view` signature surface — both retired). The plan's "live
  `StoreView`" maps onto `HostStoreView` directly: build the fence on
  `HostStoreView`, NOT on the request-bound `resolver_core::RequestStoreView`
  (a live `pub(crate)` overlay-chaining `StoreView` impl that exists for the
  request-overlay path, not the fence).
- **Keep the opaque `SchedulerCacheId(u64)` newtype.** Do NOT make it an enum
  (an enum leaks session cache-family semantics into the scheduler).
- **Do NOT build `DagAdmissionBudget`.** `DagCapacityBudget` /
  `DagCapacityReservation` is the single ledger.
- **Use `ShapeCacheDb`** (keyed by `ShapeSubject::SemanticNode`), NOT the retired
  split `MaterializeMemoDb` / `MemberShapeCacheDb` shape caches. The static guard
  `block_6i_static_guards.rs::shape_cache_db_replaces_split_caches` forbids
  re-introduction.
- **The whole-hash fix AND the slot-identity refinement BOTH LANDED.**
  `Instantiate.base` / `ResolveMacroPayload.owner` now carry the env-bearing
  content-free `ResolvedDeclSlotIdentity` slot (built via
  `dispatch.type_slot_for(...)` / `builtin_type_slot(...)`). The intermediate
  content-free `DeclKey { canonical_id, decl_name }` shape and the
  `to_decl_key()` / `DeclIdentity::to_decl_key` helpers it was produced through
  were DELETED in the U2 cutover — neither the struct nor the helper exists.
  The slot carries env dims `project_identity` / `type_env_hash` / `lib_env_hash`;
  the `resolve_env_hash` dim rides the dedicated per-key context
  (`InstantiateContext` / `MacroPayloadContext`). Per R6 the slot is content-free;
  the live whole-hash is re-sourced at value-compute time via
  `ensure_indexed_ready_serve(base.defining_canonical)`'s serve carrier `indexed.whole_hash`, NOT in the key. The
  R6 whole-hash violation is resolved.
- **Do NOT resurrect** `queue.rs`, `submit_batch` (the non-atomic one — 0 callers,
  deleted in §6c), `JobIndex`, `QueueEntry`, `EffectiveKey`,
  `AgingConfig`/`DagAgingConfig`, `BlockerRegistry`, `BlockerRef`,
  `Submission::BlockerResolved`, `FileNode.pending_requests`, per-call `threads`,
  or scheduler enum cache IDs. All were deleted by §7 / §6c / B7b.
- **`SubstitutionConcrete`** (semantic-graph §4.1.1) is superseded within its own
  plan by `CanonicalSubstitutionValueKey` (A.10). If U10 builds substitution keys,
  use the A.10 carrier.
- **`FlowNarrowing` / `ContextualType` placement:** A.11 (the later, authoritative
  revision) moves them OUT of `TypeNode` into a sibling `ProgramAnalysisGraph`
  (guards `type_node_contains_only_type_values` +
  `program_analysis_graph_gated_by_projection_required`). A0a's proto currently
  encodes the `GraphFlowNarrowing` / `GraphContextualType` arms INSIDE
  `GraphTypeNode` (the `flow_narrowing` (26) / `contextual_type` (27) oneof arms in
  `crates/verter_protocol/proto/verter/v1/typeinfo.proto`) — the OLD placement. **U8 performs the
  wire move** (re-home the two arms under `ProgramAnalysisGraph`, `reserved` the
  vacated `GraphTypeNode` tags 26/27, bump `SemanticTypeGraph.schema_version`) and
  produces the `program_analysis` half of the `TypeInfoGraphPayload` shape (the
  full 5-field literal lives in the U8 block contract). Do NOT
  keep the A0a inside-`GraphTypeNode` placement.
- **The legacy `evaluate_type_expression.rs` scratch-file evaluator is NOT a
  sanctioned 2nd `parse_type_annotation` exception.** It is the text-evaluator
  deleted in U12 once `StructuredTypeExpression` dispatch (U12) lands — the
  request-decode/dispatch home (`resolve_named_symbol.rs`) that obsoletes the
  text-evaluator is U12 itself (U11 DEPENDS on U12, so U12 cannot gate its own
  deletion on U11). §11 of the semantic-graph plan confirms no 2nd exception.

### 2.3 Deliberate architecture bets (LABELED — keep, each with its permanent acceptance bar)

These four positions are intentional bets, not oversights. Each is KEPT, stays
LABELED as a bet, and carries the acceptance bar it is permanently held to — do
not relitigate them block-by-block, and do not silently treat them as defects:

1. **Persistent `Relate`** (beyond-tsc: tsc's relation cache is per-check
   transient). LOCKED at `docs/arch/u2-relation-infer-design.md` (Decision 1):
   persistent, cross-request, fact-validated; transient comparison stacks never
   enter the key; SCC-batched admission. **Acceptance bar:** the six enumerated
   A.2 proof obligations (§3.2.1 #1) — every one provably in the identity or
   provably irrelevant; an unproven obligation routes `ReturnOnly`.
2. **Whole-file `FileWholeHash` self-roots** (sound-conservative: a same-file
   edit invalidates every decl in the file). **Acceptance bar:** strict self-root
   validation stays the same-canonical-edit rail; per-decl rooting via
   `parse_stable_hash` / `flow_body_stable_hash` is a legitimate LATER precision
   lift, not a defect to fix now.
3. **`project_identity` on package-backed decl slots** (forfeits cross-project
   warm sharing for poisoning-safety; the sound-first default). **Acceptance
   bar:** any future cross-project sharing proposal must prove isolation
   equivalence; until then, do not relitigate.
4. **The `U8 ← {U6, S5.B12}` serialization** (the whole wire/result/export stack
   waits on both extreme blocks + the Stage5 chain rather than being built around
   the doomed sidecar). **Acceptance bar:** it is THE named critical path (§3.5)
   — session/review capacity follows it; carving U8 forward around the sidecar is
   rejected.

---

## 3. Cross-track dependency map

The two tracks are **largely PARALLEL**. The key structural fact (§C):
the semantic-graph EXECUTION layer (U8 wire closure, U3 cache/fact model, U10 result
DB, U12 exporter, U11/U13, …) binds to the **already-landed singleflight /
fact-signature substrate**, NOT to the unbuilt scheduler cache-node DAG
(`submit_dag`). So scheduler work (U1 → U9; the U7 `submit_dag` envelope is
DEFERRED-to-U9, so U9 depends on U1 + B7a, NOT on a built U7) and semantic-graph execution
(U8 → U3 → U10 → U12 → U11/U13 → … → U15) proceed on **dependency-parallel lanes**
after the one convergence gate.

```
   U0 (semantic) ─────────┐  (U2 DEPENDS on U0; U0 lands FIRST)
   [NEXT]                  ▼
                      ┌─────────────────────────────────────────────┐
                      │  U2 = CONVERGENCE GATE                       │
   U1 (B7d, scheduler)│  (SemanticQueryKey identity shape + B4       │
   [∥ U0 and U2;      │   cache-node enumeration; highest            │
    scheduler-only] ──┤   correctness risk; one clean cutover)       │
                      └───────────────────┬─────────────────────────┘
                                          │
        ┌─────────────────────────────────┼──────────────────────────────┐
        │ CACHE-RUNTIME lane               │ SEMANTIC-GRAPH lane            │
        │                                  │                                │
   U4 (B9 persistent)   ──► depends U2     U8 (wire closure)  ──► U2 + U6    │
   U5 (B10 mem/audit)   ──► B2, best ≥U2     (keystone of the subplan)       │
   U6 (B11 flow-return) ──► depends U2,U4    U3.CACHE_FACT_MODEL ──► U8       │
   U7 (B7e submit_dag)  ──► depends U1          (+U2,U6; after wire closure)  │
     (submit_dag DEFERRED-to-U9)            U10 (result DB)  ──► U3 + U8      │
   U9 (B7f cache-node lowering)─► dep U1+B7a U12 (exporter)   ──► U10         │
        │                                    U11 (relation/session) ──► U12   │
        │                                    U13 (projection) ──► U12         │
        │                                    U14 (Vue adapter)  ──► U13 + U11 │
        └────────────────────────────────►  U15 (integration/bench/lift) ◄──┘
                                                depends on all code blocks
```

(Semantic-graph lane chain, per the revised subplan
`docs/arch/native-typeinfo-parity-cache-export-session.md`:
**U8 wire-closure → U3 cache/fact model → U10 result-DB → U12 exporter → {U11
relation/session, U13 projection}**. `U3.CACHE_FACT_MODEL` lands AFTER the wire
closure (its typed admission produces values whose wire shape U8 closes) and the
result DB (U10) + exporter (U12) admit through its rails. U8 is the keystone — the
wire surface every later block reads/writes — and is the sole hard prerequisite the
parity lane shares with the cache-runtime lane via U2/U6.)

**The one hard coupling = U2** (the `SemanticQueryKey` reshape + B4 cache-node
enumeration, merged per §B). It must land before graph execution (U8+) because
the exporter dispatches the final-shape `SemanticQueryKey` variants AND because
B4 enumerates the semantic-track-owned caches (`SemanticGraphStore`,
`ComponentMetaResultDb`, `MaterializeStructureDb`, `RefCycleResultDb`,
`ShapeCacheDb`) onto the `QueryNode` substrate — doing those twice (add variants
on `DeclKey`, then re-key to slot identity) is forbidden.

**Lower-grade shared edges (additive, no hard sequencing):**

- **Audit substrate** (`verter_audit` leaf) is shared. A0a already added the
  typeinfo arms; U5 (cache-node audit events) and U5's `StructuredAuditEvent`
  additions are additive under closed-enum discipline. Land any new
  `StructuredAuditEvent` variants through one coordinated `structured_event.rs`
  edit to avoid enum-variant churn / regen races.
- **Env-hash / R21 split** is a shared invariant; already landed; no sequencing.
- **Batch / host-pool coupling** is already satisfied by `HostBatchCoordinator`
  + `HostCpuPool` (§6a). A batch typeinfo session (if ever) routes through them;
  single-request typeinfo needs no new coupling.

### 3.1 Cross-plan integration — Stage5 + goto-definition folded in

Two further streams land under this same unified sequencing authority alongside the
typeinfo U-blocks: the **Stage5+6 compiler-macro cutover** (`S5.B1`–`S5.B12`, owned
by `docs/arch/stage5-cutover-plan.md`) and the **go-to-definition overhaul**
(`G.P1`–`G.P6/7`, owned by `docs/arch/goto-definition-overhaul-plan.md`). They are
folded in per a HIGH-CONFIDENCE codex-architect cross-plan sequencing decision. The
typeinfo dep-map above is unchanged; this section ADDS the `S5.*` and `G.*` edges and
the one mandatory shared gate that binds Stage5 and typeinfo together.

#### 3.1.1 The shared macro-surface gate (THE crux)

**Macro-resolution end-state (one path, one normalizer).** Vue macro resolution has
exactly ONE semantic path: `ResolveMacroPayload → ProjectSemanticDispatch::execute →
SemanticGraphStore`, followed by ONE shared macro normalizer. `VueMacroElements`,
`HostResolvedNamedTypeKey`, `SemanticQueryKey::ResolvedNamedType`, the parser
`NamedTypeCache` adapter, the graph named-type indexes, and the entire parser
`resolve_type/` OXC resolver ALL retire. This is the concrete realization of
CLAUDE.md's **Macro-Type-Traversal Rule** ("a macro is at bottom a single type lookup
plus thin normalisation — `shared_resolve(type) + normalise`"); Stage5 `S5.B5` and the
typeinfo blocks **jointly own** this cutover — it is NOT a standalone compiler cleanup.

**`S5.B5` is a SHARED macro-surface gate that lands AFTER typeinfo U2.** U2 finalizes
the `ResolveMacroPayload` identity (the slot-identity shape), so `S5.B5` cannot land
before it. **`S5.B5` is also RESCOPE-GATE-REQUIRED (§3.2): its normalizer
COMPATIBILITY MATRIX is a rescope-gate deliverable** — the full
valid-empty-vs-unresolved / visibility / defaults / `native_props` / public-surface
filtering / emits / slots / options / expose matrix, proved at the gate to lose **no**
semantic distinction versus the legacy rail BEFORE `S5.B11` deletes the fallback
(that proof is the gate's output, not asserted in this prose). Ownership at the gate
splits cleanly:

- **Typeinfo owns the semantic surface + the normalization contract.** The shared
  macro surface and its normalization (props / emits / slots / options / expose) are
  the typeinfo semantic authority, resolved through the one shared dispatch.
- **Stage5 owns the compiler-owned `ResolvedMacroSurfaces` DTO + `VerterHost::resolve_macro_surfaces_for`.**
  `ResolvedMacroSurfaces` is a **structural projection of the shared normalization**
  produced by the host for the compiler — it is **NOT a second resolver**, NOT a
  second normalizer, and NOT the `SemanticGraphStore` artifact. Typeinfo `U14` later
  consumes the **same** normalized surface structurally and **must not rebuild macro
  meaning** (no parallel surface/expander in the adapter).

**`native_props` + class-member VISIBILITY ownership.** The semantic visibility of
class/members belongs to **typeinfo surface ownership** — specifically the U2 class /
member surfaces (`ResolveClassSurface`) and the published projection (U13). Stage5
carries that visibility through `ResolvedMacroSurfaces` and is responsible for
**re-sourcing the FFI / public `native_props` carrier BEFORE `S5.B11`**. `S5.B11` is
**BLOCKED until that re-source is proved** (the carrier must demonstrably reflect the
shared visibility before the legacy rail is deleted).

**HARD GATE: `S5.B11`/`S5.B12` MUST complete BEFORE any typeinfo `U8+` work.** Do NOT
build the new typeinfo wire / result / export / projection stack (U8, U10, U12, U11,
U13, …) around a sidecar (`VueMacroElements` / `HostResolvedNamedTypeKey` /
`resolve_type/`) that is scheduled for deletion. The biggest correctness risk is the
B5 normalizer **silently losing semantic distinctions** while replacing the old rail —
especially **valid-empty vs unresolved**, **non-public visibility**, **`native_props`**,
**defaults**, and **public-surface filtering**. Once `S5.B11` deletes the fallback,
that class of regression is hard to recover without reintroducing a second path — so
the gate makes the post-U2 macro-surface cutover MANDATORY before any `U8+` typeinfo
work or Stage5 legacy deletion completes.

**The framework-adapter substrate landed ahead of order, so the sidecar cutover surface
is wider than the original Stage5 enumeration.** The merged framework-surface producer
(`typeinfo/framework_surface/vue_exec`, `typeinfo/framework_surface/graph_export.rs`) and
the Vue + Svelte adapter paths are themselves NEW consumers of the `VueMacroElements` /
`type_surface` sidecar. The `S5.B5`–`S5.B12` cutover therefore MUST repoint these merged
producers onto the one shared `ResolveMacroPayload` path as well — leaving a second
sidecar consumer alive (the framework-surface producer) is the same dual-path failure the
gate forbids. The cutover's repoint+deletion surface accordingly includes the
`type_surface` guards (e.g. `no_new_type_surface_engine_path_production_file`): they
shrink/retire as the sidecar engine is removed, never pin the old engine count past the
deletion (that guard's shrink/delete is owned at U6 — the block that cuts the legacy
return / type-surface machinery).

**Docs-updated deliverable on `S5.B11`.** CLAUDE.md's project-global-cache description
(the `VueMacroElements` / `HostResolvedNamedTypeKey` / `NamedTypeCache` paragraph) and
the `/type-cache-architecture` + `/type-resolution` skills are updated to the
one-`ResolveMacroPayload`-path end-state by `S5.B11`'s `Docs updated:` step **when
S5.B11 lands** — tracked here as a deliverable, NOT pre-edited now (pre-editing the
current-state authority to describe unbuilt state would make it lie about the live
code, exactly as §8's intentional-divergence note governs the typeinfo blocks).

#### 3.1.2 Goto-definition dependency edges + the `SnapshotId` reconciliation

- **`G.P1` → `G.P2` — LANDED on this branch** — the typed `PositionMapper`
  coordinates (`SourceByteRange` / `GeneratedByteRange` / `LspPosition` / `TsPosition`
  in `crates/verter_span/src/lib.rs`) and the typed `EmitOp` IDE-codegen mapping
  substrate (`crates/verter_compiler/src/ide/template/emit.rs`) + the
  collapsed-overwrite emit-site fixes landed via the `db1e2db8`/`3b4c1e3c`/`01dd889d`/
  `b437232f`/`47184ca3`/`b1611270`/`cec9cfa4` line (the retired `resolve_prefixed_expr`
  producer is gone). This is mapper / codegen substrate work with **NO typeinfo
  dependency**; it parallels the early lane.
- **`G.P3` lands AFTER U2** — it adds `SfcComponentAnchor` onto `IndexedReady`, the
  shared post-parse substrate U2 finalizes (`G.P3 ← U2`).
- **`G.P4` waits for U3's canonical fact-signature model** (`G.P4 ← U3`), and the
  goto/nav validity token — `CompileSnapshotId` — **is RECONCILED** to the compile
  warm-hit fact-validated identity:

  > `CompileSnapshotId` = `Hash128(compile warm-hit identity)` = `semantic_hash` + the
  > style/content override hashes + the compile profile + the parser/compiler/version
  > dimensions + the **canonicalized `ReadSetSignature` facts** (the cross-file
  > fact signature).

  The goto plan's `Hash128(prefix || generated_tsx_bytes)` (a TSX byte hash) is **NOT
  the authority**: a TSX byte hash may ride as **debug / collision metadata**, but the
  **validity token MUST match the compile-slot fact-validated identity** — otherwise
  the id is stale-stable under cross-file dependency edits (`semantic_hash`
  covers own-content only). `.ts` / `.js` / `.d.ts` real-source targets have no IDE
  TSX and use **`TargetSourceContext.source_hash`**, NOT a TSX `CompileSnapshotId`.
  Mismatch ⇒ DROP (do not recompute).

  **Name disambiguation (avoids a `verter_session` compile-time clash):** `G.P4`'s
  nav-validity token is named **`CompileSnapshotId(u128)`**, distinct from U5's net-new
  cache-entry-pin **`SnapshotId(u64)`** (the `ActiveSnapshotPinRegistry` id in
  `cache_runtime/{metrics,memory_policy}.rs`). Both are net-new in `verter_session`, so
  the goto/nav identity carries the `Compile` prefix throughout the goto doc set; U5's
  `SnapshotId` stays as-is.
- **`G.P5` + the merged `G.P6/7`** wait on `G.P4`; once U3 has landed they may run
  **parallel with typeinfo `U10`–`U13`**.
- **One-engine guard for navigation.** Definition / references / rename stay **ZERO
  typed-IR dispatch** (navigation is not type resolution). `type-definition` MAY use
  `ProjectSemanticDispatch` **only in the one explicitly allowed type-definition
  module** — a guard enforces no type-resolution leak into the other nav surfaces.

#### 3.1.3 The concrete cross-plan ORDER

```
X0  re-home + docs index  (this integration itself — §3.1; FRAMES the foundation box below)
       │
       ▼
FOUNDATION (framed at X0; PER-EDGE gating per §0.5.6 — each edge gates its NAMED consumer, never a blanket before-U0 / before-U2 phase):
   Ownership statement (B.15) + ledger #19 boundary  (frames everything)
   B.1 Program Model → U0.RESOLVER_CORE (A.5/B.2, #21) → B.4 Stdlib/Intrinsics Authority
   B.13 Broken-Code/Recovery (global; U0-owned producers)   B.9 Emit Boundary (at §Scope)
       │
       ▼
parallel-safe early:   G.P1 → G.P2   ∥   S5.B1   ∥   U0   ∥   U1
       │
       ▼
U2  = CONVERGENCE GATE  (finalizes ResolveMacroPayload identity + the slot-identity shape)
       │   (BinderIdentityFacts §0.5.1 layer 1 — demand-produced FROM IndexedReady, CONSUMED BY
       │    the U2 reducers BEFORE they run; pre-U2 prerequisite, NOT eager, NOT N0-owned;
       │    production OWNED by the U2.BINDER_IDENTITY_FACTS substrate sub-block)
       │   (A.3/#20 strict matrix + B.12 config matrix produced AT the U2.RELATION_INFER gate)
       ├──────────────────────────── after U2 ────────────────────────────┐
       │                                                                    │
  Stage5 macro chain (shared gate):                                         │
       S5.B5 (macro-surface GATE)                                           │
         → S5.B6 / S5.B7 / S5.B8 → S5.B9 → S5.B10 → S5.B11 → S5.B12         │
                                                                            │
  parallel after U2:                                                        │
       U4 → U6        U5        scheduler U9 (←U1+B7a; U7 DEFERRED)   G.P3 ∥ N0.NAV_LOCATION_INDEX │
                                                                            │
  U8  ONLY after  U6 + S5.B12   ◄── HARD GATE (no U8+ around the sidecar)   │
       │   (#7 warm-hit-validity BLOCKS U3 + U10; #9 degradation taxonomy   │
       ▼    ready BEFORE U12/U9/U11)                                        │
  U3   (canonical fact-signature model)                                     │
       │                                                                    │
       ├──────────────────────────── after U3 ────────────────────────────┤
       │                                                                    │
  goto tail:   G.P4 → G.P5 → G.P6/7     ∥     U10 → U12 → U11/U13 → U14 → U15
       │
       ▼
AFTER typeinfo parity (U15):
   B.5 Native Checker Manifest (own rescope gate)
   N1.NATIVE_LANGUAGE_SERVICE_LAYER  →  B.7 Completion/Hover/SigHelp (U15 tsgo STAYS)
   B.10 Native Declaration Emit      →  B.8 Code Actions / Refactors / Organize (LAST LS)
   B.11 JSDoc-type (near U2) / checkJs (after B.5) / CJS (#21 at U0)
       │
       ▼
TERMINAL:  B.14 Replacement Acceptance Gates (§10; fenced from §9)
```

**Cross-plan dep-map edges (added to the typeinfo dep-map above; the whole graph
stays ACYCLIC).** The typeinfo edges are unchanged; these are the ADDED `S5.*` / `G.*`
edges:

- `S5.B5 ← U2` (the macro-surface gate cannot land before U2 finalizes
  `ResolveMacroPayload` identity).
- `S5.B6/B7/B8 ← S5.B5`, `S5.B9 ← S5.B6/B7/B8`, `S5.B10 ← S5.B9`,
  `S5.B11 ← S5.B10` (and `S5.B11` additionally BLOCKED on the proved `native_props`
  re-source), `S5.B12 ← S5.B11`.
- `U8 ← {U6, S5.B12}` — U8 keeps its typeinfo prereqs (U2 + U6) AND additionally
  waits on `S5.B12` so the new wire/result/export stack is never built around the
  deleted sidecar. (This is the one new typeinfo-side edge; it adds a Stage5 prereq
  to U8, it does not change the U8↔U2/U6 edges.)
- `G.P2 ← G.P1` (early, no typeinfo dep); `G.P3 ← U2`; `G.P4 ← U3`;
  `G.P5 ← G.P4`; `G.P6/7 ← G.P5`.

**Foundation / language-service / emit edges (the §0.5 blocks; whole graph stays
ACYCLIC).** These ADD the new-block edges; the typeinfo + `S5.*` + `G.*` edges above are
unchanged:

- **Foundation edges (PER-EDGE, not a blanket phase — §0.5.6):** `U0.RESOLVER_CORE ← B.1`,
  `B.4 ← {B.1, U0.RESOLVER_CORE}` (the resolver feeds stdlib's shared ambient corpus, so
  stdlib depends on the resolver + program model — §0.5.6 order
  `B.1 → U0.RESOLVER_CORE → B.4`); `B.4` in turn gates only the lib-fact-CONSUMING
  reducers (the `LibIntrinsic`-reading `U2` sub-blocks), never the landed key spine /
  value-domain layer or the `U0` manifest substrate. The ownership statement (`B.15`)
  and the `B.9` emit boundary are framing decisions with no incoming edge; `B.13`'s
  broken-input taint PRODUCERS are `U0`-owned work items (FORK-B) — an edge into `U0`'s
  remaining scope, not a retroactive gate over its landed substrate.
- **`BinderIdentityFacts → U2`** (§0.5.1 layer 1): the named pre-`U2` binder-identity substrate is
  demand-produced FROM `IndexedReady` and CONSUMED BY the `U2` reducers before they run — a `U2`-tier
  prerequisite, NOT a `U2` output and NOT `N0`-owned; its production is OWNED by the
  `U2.BINDER_IDENTITY_FACTS` substrate sub-block (§4, the U2 block). The edge runs
  `BinderIdentityFacts → U2`, never
  `N0 → U2` (guards `binder_identity_facts_are_pre_u2_and_not_n0_owned`,
  `u2_queries_do_not_read_n0_navigation_indexes`). It is demand-produced, not an eager pass, so it adds
  no second symbol authority and no eager-binder edge.
- **`N0.NAV_LOCATION_INDEX ← {BinderIdentityFacts, U2}`** (a pure projection over `BinderIdentityFacts`;
  reuses the U2 merged-decl ordering facts + finalized
  `IndexedReady`); runs ~alongside `G.P3`. `N0` PRODUCES def/refs/rename locations natively and its
  `TsgoNavigationBackend` **def/refs/rename** deletion is gated `N0 → before B.14/§10` (a later→earlier
  edge: the deletion sweep `§10` (7) consumes `N0`). The same backend's **`getCodeActions`** deletion
  is owned by `B.8` (it produces native code-actions) and is gated `B.8 → before B.14/§10` (the sweep
  `§10` (7) also consumes `B.8`); the struct is fully removed once both land. The live tsgo
  `TypeProvider` LS paths carry the same per-producer deletion edges, all consumed by `§10` (7):
  `N0 → before B.14/§10` (semantic tokens / document highlights; call hierarchy already native),
  `B.7 → before B.14/§10` (inlay hints), `N1 → before B.14/§10` (type-definition),
  `B.5 → before B.14/§10` (the `get_diagnostics` / `publish_merged_diagnostics` tsgo merge). Every
  such edge is later→earlier, preserving DAG acyclicity.
- **`A.3`/`#20` strict matrix + `B.12` config matrix** are produced AT the
  `U2.RELATION_INFER` gate (gate deliverables, not separate blocks).
- **`#7` warm-hit validity BLOCKS the `U3` + `U10` gates**; **`#9` degradation taxonomy
  ready BEFORE `U12`/`U9`/`U11`** (consume-before-rely, §3.2.1).
- **After typeinfo parity (`U15`):** `B.5 ← {U2, U6, U8, U10}` (its own rescope gate);
  `N1 ← {U15, N0}`; `B.7 ← {N1, U2.RELATION_INFER, U6}`; `B.8 ← {N0, N1, B.5}`;
  `B.10 ← {B.9, U14, N0, B.5}` (typeinfo values + SFC + `B.5` `CheckDeclaration`
  declaration-conformance diagnostics — acyclic: `B.5 ← {U2, U6, U8, U10}` predates
  `B.10`, no cycle); `B.11` JSDoc-type ← near `U2`,
  `B.11` checkJs ← `B.5`, `B.11` CJS ← `U0.RESOLVER_CORE` (#21).
- **`B.14 ← all blocks`** (terminal, §10).

The new edges all run consistently with the order above: the feeds-edges
`foundation → `U0`/`U2`` and `IndexedReady → BinderIdentityFacts → U2` point earlier→later (a
prerequisite feeding its consumer), and the dependency-edges `N0 ← {BinderIdentityFacts, U2}`, the
LS/emit tail `← U15`, and `B.14` terminal point later→earlier (a consumer naming its prerequisite).
Both senses describe the SAME partial order (`IndexedReady → BinderIdentityFacts → U2 → N0`), so no
cycle is introduced; the combined graph remains a DAG.

Because `S5.B5 ← U2`, `U8 ← S5.B12`, `G.P3 ← U2`, and `G.P4 ← U3` all point from a
later block to an earlier one in the order above, no cycle is introduced; the combined
graph remains a DAG.

#### 3.1.4 Env re-home rule

The three streams' historical artifacts are **HISTORICAL ONLY**: the `D:/` worktrees,
the branches (`fix/goto-definition-overhaul`, `s5/b5`), the SHAs (`0b3a63894`,
`380967beb`, `63d682f69`), and the `D:/tmp/*` live-state artifacts. The **live
authority** is this repository on `refactor/semantic-db-overhaul`.

Goto `G.P1`/`G.P2` have since **landed on this branch** (typed `PositionMapper`
coordinates + the `EmitOp` substrate — see §3.1.2). Stage5 `S5.B5` is **NOT landed
here**: the `ResolvedMacroSurfaces` DTO struct exists (`crates/verter_compiler/src/compile/macro_dto.rs`),
but `resolve_macro_surfaces_for`, the cutover, and the legacy `resolve_type/` +
`VueMacroElements` / `HostResolvedNamedTypeKey` deletions are not done — re-implement /
re-land it from the plan against live HEAD (S5.B5 is RESCOPE-GATE-PENDING, gate after
U2). A cherry-pick is a
**patch-extraction technique after review**, never proof of correctness or sequencing.
The live-branch tests / guards / docs + the unified-plan block acceptance govern. All
three streams land under this unified plan's **git/CI landing model**: branch per
block → green CI gate → three-reviewer LAND → squash-merge with a trailer (e.g.
`Typeinfo-Block:` / the Stage5 / goto equivalent), exactly as §1 / U0 describe for the
typeinfo blocks.

---

## 3.2 Big-phase rescope gate (pre-implementation algorithm-depth gate)

The full-tsc-parity target (Scope, above) is multi-person-year, and its hardest
subsystems **cannot be designed to algorithm depth up-front**: a hard phase's deep
design depends on PRIOR phases having landed and on the substrate they expose. So a
**BIG PHASE is deliberately NOT pre-specified to algorithm depth in this plan.**
Instead, **before** a big phase implements, the orchestrator runs a **RESCOPE
SESSION** that produces that phase's deep design against the then-current tree.

**This is the resolution of the "under-designed core" finding.** The hard algorithmic
cores are **intentionally deferred to their rescope gate, not hand-waved.** Where a
block contract currently says only "route through `Relate`" / "route through the
reducers" / "route through the one engine" for a HARD core, that phrasing states the
one-engine *wiring* constraint — it is **not** the algorithm. The deep algorithm for a
RESCOPE-GATE-REQUIRED phase is produced at its rescope session, not asserted here.

**Gate-pass bar — the rescope-rejection rubric (A.1).** A rescope gate does **NOT**
pass — it **FAILS** — if it produces only guard NAMES, "route through `Relate`" /
"route through the reducers" / "route through the one engine" wiring statements, or
high-level prose. Those state the one-engine *wiring* constraint; they are **not** a
design. To PASS, a gate MUST produce ALL FIVE deliverables below. This is a fail-criteria framing
of the gate-OUTPUT requirements — it does NOT restate, and does NOT supersede, the
rescope-session output list that follows (in particular the session's `(b)` `tsgo`-oracle
baseline and `(d)` rescope-of-subsequent-phases items are NOT covered by this rubric and
must still be produced):

- **(1) data structures** — the real types the algorithm operates on, not a sketch.
- **(2) an executable-pseudocode algorithm** — the actual control flow, not a name on a
  seam.
- **(3) a termination / convergence / admission proof** — for any recursive or iterative
  design, a monotone measure + a well-founded bound (or an SCC-discharge argument showing
  non-convergence never yields a WARM admission).
- **(4) discriminating fail-today fixtures** — tests that FAIL against the pre-change tree
  and PASS after.
- **(5) the docs / guard updates** — the new `(CRITICAL)` rule(s) + their registered R6
  guards, updated AT the gate (per §3.2(e)).

A gate output missing any one of (1)–(5) is rejected and re-run; guard names without an
algorithm, or an algorithm without a termination proof, are the two most common rejected
shapes.

**The rescope session (autonomous, minimal-token).** A planner drafts the phase's
algorithm-depth design; a **rescope review panel — 1 Claude Code + 2 codex, all under
the best-architecture / breaking-changes-allowed / be-honest mandate** (the same loop
that converged this plan) — iterates WITH the planner until best-possible-architecture
is reached AND feasibility is confirmed. The orchestrator drives this **fully
autonomously** and does the **minimal work to keep token usage low**: it does not
re-litigate already-converged phases, and it scopes the panel to the one phase. The
session produces:

- **(a) the phase's algorithm-depth design** — **executable-pseudocode depth**: the
  real data structures, the actual algorithm, and — wherever the design is recursive
  or iterative — a **real termination / convergence proof** (e.g. a monotone
  measure + a well-founded bound, or an SCC-discharge argument showing
  non-convergence never yields a WARM admission). A proof is NOT "route through
  `Relate`" prose, and guard NAMES are NOT a design.
- **(b) the differential `tsgo`-parity oracle baseline** for the phase's families
  (§3.3) — the per-family divergence budget the phase's acceptance is gated on.
- **(c) the phase's discriminating guards** — fail-today fixtures (fail against the
  pre-change tree, pass after).
- **(d) any rescope of SUBSEQUENT phases** the design reveals (the gate may re-shape
  later phases' contracts).
- **(e) the rules update — at the gate, BEFORE the phase implements (NOT the
  implementer's job).** Once best-architecture is reached AND before the phase's
  implementing agent starts, the **rescope orchestrator** updates `CLAUDE.md` (its
  `(CRITICAL)` rule set) plus the relevant `.claude/skills/*` with the phase's NEW
  `(CRITICAL)` rules and their named R6 guards. The implementing agent must have those
  rules ALREADY present in `CLAUDE.md` / the skills (loaded in its context) by the time
  it codes the phase, so it implements WITH the rules in mind rather than
  discovering/adding them mid-block. **R6 coupling (keeps the meta-guard green):** each
  new `(CRITICAL)` rule is committed together with its REGISTERED guard — a guard name
  plus a `CRITICAL_RULE_GUARDS` registry entry — at this gate→implementation boundary,
  so `every_critical_rule_in_docs_has_registered_guard`
  (`crates/verter_session/tests/g_misc0/critical_rules_have_guards.rs`) is satisfied the moment
  the rule appears; the guard's DISCRIMINATING test then lands with the phase's FIRST
  implementation block (the rule + a registered guard appear in the same change, and the
  test goes green as that first block lands).

**This rules-update-at-gate step is DISTINCT from the already-converged
orchestration-layer "current-state authority / Docs-updated-deferred" note** (§3.1.1's
`S5.B11` deliverable + §8's intentional-divergence note): that deferral applies to
ALREADY-CONVERGED orchestration-layer changes, where pre-editing the current-state
authority to describe unbuilt code would make it lie about the live tree, so the doc
update is tracked as a land-time `Docs updated:` deliverable. For a RESCOPE-GATE big
phase the rule update happens AT the gate (just before that phase implements) precisely
because the implementer must CODE AGAINST the new rule — the rule is a design input the
phase consumes, not a post-hoc description of code that already shipped.

**Two DISTINCT gates.** The **rescope panel (per-big-phase, PRE-implementation)** and
the **three-reviewer LAND panel (per-block, at block-done, §11.12 of
`native-typeinfo-parity.md`)** are different gates at different stages. The rescope
gate produces the design *before* the phase's blocks are written; the LAND panel
authorizes the merge of each finished block. Both are **additive to** the git/CI
landing model — a phase still lands block-by-block via branch → green CI →
three-reviewer LAND → squash-merge **after** its rescope gate has produced the design.
The rescope panel is also distinct from §14.1's codex-architect fork gate (which
resolves an UNFORESEEN fork DURING a block); the rescope gate is the SCHEDULED
pre-design of a known-hard phase.

**RESCOPE-GATE-REQUIRED phases** (each tagged in its block contract below with a
one-line "deep design produced at its rescope session (depends on `<prior>`)" note;
effort weights in §3.3):

- **`U2.RELATION_INFER` — its rescope gate has RUN; the output is DESIGN-LOCKED at
  `docs/arch/u2-relation-infer-design.md`.** The relation / assignability +
  inference-session + measured-variance core (TS's hardest subsystem). The gate's
  deliverables — the assignability algorithm + the recursion-id /
  `isDeeplyNestedType`-style **termination proof** (non-convergence never yields a
  WARM admission), the inference candidate-accumulation / priority / fixation /
  contextual-callback-loop / reverse-mapped algorithm, the marker-probe variance
  iteration, the PERSISTENT-`Relate` decision with the enumerated A.2 proof
  obligations, and the RI-1..RI-10 implementation mini-DAG — are owned by that
  locked design; the plan cites it as authority instead of re-demanding the gate.
  (Parent `native-typeinfo-parity.md` §§4.0–4.2 seeded the core; the locked design
  hardened it.)
- **`U6` — its rescope gate has RUN; the output is DESIGN-LOCKED at
  `docs/arch/u6-flow-call-resolution-design.md`.** The cross-engine cycle /
  termination: `FlowReturn` ↔ `ResolveCall` ↔ narrowing ↔ `ContextualTypeAt`, the
  `CheckerReentryGraph`, and flow narrowing as a dataflow fixed-point. The gate's
  deliverables — runtime SCC detection + provisional-result-during-discharge + a
  stability decision + **fail-closed on non-convergence**, the narrowing **JOIN
  ALGEBRA** for conflicting predicates, the loop fixed-point, and the **8
  `U6.NARROW_*` sub-blocks re-cast as 1 hard lattice-substrate block + 7 cheap
  additions** — are owned by that locked design (it consumes the U2/RI substrate);
  the plan cites it as authority instead of re-demanding the gate.
- **The native checker** (`docs/arch/native-checker.md`) — the sibling/future
  diagnostics layer. **RESCOPE-GATE-REQUIRED before its `Check*` queries implement**
  (it is out of the 362-row parity scope, but its `Check*` algorithm-depth design is
  produced at its own rescope session).
- **`U7`** (the scheduler cache-node DAG, `submit_dag`) — **its rescope gate has RUN;
  the output is LOCKED at `docs/arch/u7-scheduler-submit-dag-decision.md`: DEFER.**
  The gate's "justify against MEASURED workload or CUT" question resolved to deferral —
  the multi-node envelope is held un-built (a model-checked DAG for an I/O-bound LSP
  already served by singleflight is an over-engineering risk), with a narrow re-gate
  at U9 (proven atomic-admission correctness need OR a measured graph-scoped
  completion/cancellation need; default absent that = permanent CUT).
- **`S5.B5`** (the shared macro-surface gate) — **RESCOPE-GATE-REQUIRED**: its
  normalizer **compatibility matrix** is a rescope-gate deliverable — the full
  valid-empty-vs-unresolved / visibility / defaults / `native_props` / public-surface
  filtering / emits / slots / options / expose matrix, proved to lose **no** semantic
  distinction versus the legacy rail before `S5.B11` deletes the fallback.

### 3.2.1 Rescope-gate deliverable ledger (the SEMANTIC-CORE design, produced at each gate)

Two rounds of independent architecture-only review converged on one finding: the
**substrate frame is near-ceiling** (one-engine dispatch, fact-validated cache, the R21
env-hash split + the R6 rule↔guard coupling, demand-driven scoping, wire purity), but the
**SEMANTIC CORE is still a named seam, not a design** — "route through `Relate`" / "route
through the reducers" states the one-engine WIRING constraint, not the algorithm. The
first pass (codex ~8, claude ~7) seeded **#1–#8**; a **second deep architecture-only pass
(codex + claude, 2026-06-03) re-confirmed the substrate frame near-ceiling (~9)** and
surfaced further semantic-core gaps, recorded below as **#9–#22**. Per the owner's
defer-to-gate process these gaps are **NOT designed here**; each is captured as an
explicit NAMED deliverable its rescope gate MUST produce (to algorithm /
executable-pseudocode depth, with proofs), so the gaps are assigned, not lost or
hand-waved. State plainly: **this expanded ledger IS the semantic-core design,
intentionally produced at each phase's rescope gate; the substrate frame is already
near-ceiling, and the plan does not pretend the core is designed yet.**

**Already credited (NOT gaps).** The second pass re-surfaced two items the plan already
owns: the differential `tsgo`-parity oracle (§3.4 + ledger **#8**) and the
`CheckerReentryGraph` (§3.2 `U6` + parent `native-typeinfo-parity.md`). Both are credited
as present, not re-opened as deliverables.

The relation / cycle pair (**#1 + #2**) is **resolved EARLY at the U2 foundation gate**
because it shapes the substrate every later phase consumes — it is not deferred to the
phase that happens to need it last.

- **`U2.RELATION_INFER` gate** (the highest-leverage; resolved FIRST, substrate-shaping
  — gate runs after `U2.QUERY_VALUE_DOMAIN` lands):
  - **#1 — Relation-caching category (the deepest gap).** Decide EXPLICITLY whether
    `Relate` is a **TRANSIENT intra-check relation cache** (keyed on the type-id pair +
    relation-kind, like tsc's per-check relation cache) or a **PERSISTENT cross-request
    cache**. **RESOLVED — the gate ran and LOCKED `Relate` as PERSISTENT
    (`docs/arch/u2-relation-infer-design.md`, Decision 1: a persistent, cross-request,
    fact-validated query-identity cache keyed by the full relation identity); the A.2
    proof obligations below remain the PERMANENT acceptance bar that decision is held
    to.** If the gate confirms persistent, it MUST PROVE the cache identity fully
    captures the live comparison stack + the in-flight inference context (the
    `InferenceContextKey` fingerprint must be complete) — otherwise the choice is
    unsoundness (a warm hit reuses a relation decided under different assumptions) or a
    perf collapse (the identity is so wide it never hits). **The persistent-vs-transient
    decision is gated on an explicit ENUMERATED proof-obligation list (A.2)** — if
    persistent, the cache identity MUST provably capture EVERY one of:
    1. the live **relation/comparison stack** (the in-flight `relate` reentry chain);
    2. the in-flight **inference context** (the complete `InferenceContextKey` fingerprint);
    3. the **strict-policy** family in force (per #20 — a relation decided strict-on must
       not warm-hit a strict-off request);
    4. **freshness** (the `ReadSetSignature.facts` of BOTH related types' structural
       reachability — per #4);
    5. the **five split env dimensions** (R21; never bundled);
    6. **reentry** state (the coinductive-cycle assumption stack of #2 — a result decided
       under an open assumption is not admissible until the assumption is discharged).
    Each obligation either enters the identity or is proven irrelevant; an unproven
    obligation routes the result through `ReturnOnly` (no warm admission).
  - **#2 — Coinductive cycle discharge as a first-class
    `ProjectSemanticDispatch::execute` PRIMITIVE.** A cross-query reentry / assumption
    stack — spanning `FlowReturn` / `ResolveCall` / `FlowNarrowingAt` /
    `ContextualTypeAt` / `Relate` — with a provisional-value protocol PLUS an explicit
    "**when a provisional result becomes STABLE and is therefore ADMISSIBLE**" rule. This
    is a property of the ONE engine, NOT per-family bolt-ons: it replaces / unifies the
    per-family stand-ins (`RefCycleResultDb` + the flow depth-sentinel). **#1 and #2 are
    ONE design problem** — the reentry / assumption stack is exactly what decides whether
    a relation result is stable enough to admit — so the gate designs them TOGETHER.
  - **#5 — Relation-proof structural shape.** A derivation witness / reason code, so
    `Relate` is the sole assignability authority with a DEFINED public payload, not a
    name on a side table. (Feeds the `RelationPayload` + payload-side `relation_proofs`
    proof table the U8 wire surface already names.) **The gate enumerates the FOUR relation
    outcome proof shapes (A.9):** (1) **assignable** (witness derivation), (2)
    **not-assignable** (reason code + the failing structural sub-relation), (3) **unknown**
    (budget/recursion-cap reached without a decision), (4) **coinductive-cycle** (decided
    under an open assumption, per #2). All four ride the payload-side `relation_proofs`
    table by opaque proof id — they stay OFF the type-values surface (NO `GraphTypeNode`
    arm; guard `relation_proofs_not_graph_type_nodes`).
  - **Cache-admission algebra** (codex). An exact, impossible-to-misread
    publish / no-publish matrix over the full discriminant set: `Cacheable` /
    `ReturnOnly` / overflow / budget-exhaustion / unresolved-provenance /
    incomplete-self-rooting / generation-supersession / overlay-only → which states
    PUBLISH a warm entry and which return-only. **Explicit relation/cycle + inference-session
    ROWS (A.7 + A.8 — these are ALREADY-COVERED to executable depth by parent §4.1
    coinductive-cycle / §4.2 inference-session; enumerated here so no row is implicit):**
    | Discriminant | Admission |
    |---|---|
    | relation **cycle sentinel** (open coinductive assumption) | `ReturnOnly` (no publish) |
    | **unconverged SCC** (assumption not yet discharged) | `ReturnOnly` (no publish) |
    | **budget-abandoned fixed point** (relation/flow budget exceeded mid-iteration) | `ReturnOnly` (no publish) |
    | **speculative / losing inference session** (a non-winning candidate attempt) | `ReturnOnly` (no publish) |
    | **session-local delta** (overlay-only result) | `ReturnOnly` (no publish to base/persistent) |
    | **abandoned inference session** | `ReturnOnly` (no publish) |
    | **in-flight inference session** (not yet `CompletedDeterministic`) | `ReturnOnly` (no publish) |
    | `CompletedDeterministic`, fully self-rooted, in-generation, non-overflowed | `Cacheable` (publish) |
    Only the last row PUBLISHES a warm entry; every cycle/SCC/budget/speculative/session
    state returns the computed value WITHOUT admitting it.
  - **#10 — Request-consistency model.** ONE request view held consistent across every
    nested query, so a parent and its sub-queries observe one immutable snapshot. The
    reentry / assumption stack of **#2** IS this request-consistency frame; the gate
    designs them together.
  - **#13 — Cycle semantics OUTSIDE `Relate`.** ONE cycle + depth policy table covering
    aliases, mapped types, conditional types, template-literal types, recursive object
    types, projection, and display — extending the **#2** coinductive primitive beyond
    relation so every recursive walk shares one termination policy.
  - **#20 — Strict-family semantics.** Reducers and assignability must BRANCH on
    `strictNullChecks` (and the strict family), not merely KEY a cache on it: the result
    differs under strict-on vs strict-off, so the difference is computed, not just
    isolated. **The gate produces an explicit strict-family MATRIX (A.3) — columns:
    `option` / `affected reducers` / `cache-env dim` / `behavioral branch` / `oracle
    fixture` — with at minimum the rows `strictNullChecks`, `strictFunctionTypes`,
    `exactOptionalPropertyTypes`, `noUncheckedIndexedAccess`, `strictBindCallApply`,
    `strictPropertyInitialization`, `alwaysStrict`, `useUnknownInCatchVariables`, `noImplicitAny`,
    `noImplicitThis`, and `strictBuiltinIteratorReturn` (the full strict family, pinned to the
    project's TS version — the `strict`-umbrella members plus the strictness-affecting
    `exactOptionalPropertyTypes` / `noUncheckedIndexedAccess`). The non-strict JSX / decorator / class-field /
    `lib` option sets are NOT A.3 rows — they belong to `B.12` / `B.4` (see the exhaustive partition
    below).** The matrix is **produced BEFORE `U2.RELATION_INFER`
    lands** (the relation core branches on it). **A.3 ⊂ B.12** — the strict matrix is the
    strict SLICE of the full TS Config Semantics Matrix (below); the strict rows belong to
    A.3/#20 and are NOT duplicated in B.12.
    - **Guard (at the `U2.RELATION_INFER` gate, §3.2(e)):**
      `reducers_branch_on_strict_family_not_only_key` — a discriminating fixture proves a
      reducer's RESULT differs under strict-on vs strict-off, not merely its cache key.
  - **TS Config Semantics Matrix (B.12)** — the full "config option → behavioral branch"
    SEMANTICS matrix that A.3 is a slice of. The KEYING side is already contracted (R21 +
    the U10 guard `cache_keys_cover_ts_jsx_moduleresolution_decorator_lib_dimensions`); the
    NEW part is the **behavioral-branch column** over the meaning-affecting options not yet
    enumerated as behavioral branches: the **JSX family**
    (`jsx` / `jsxFactory` / `jsxFragmentFactory` / `jsxImportSource`), the
    **decorator / class-field family** (`experimentalDecorators` / `emitDecoratorMetadata` /
    `useDefineForClassFields`), and the **module/interop family** (`target`, `module`,
    `moduleDetection`, `esModuleInterop`, `allowSyntheticDefaultImports`, `skipLibCheck`,
    `isolatedModules`, `verbatimModuleSyntax`). This is a sibling deliverable to #20; it does NOT
    duplicate the strict rows (those are A.3/#20's), and it reuses the U10 keying guard's dimension
    list rather than adding a new keying guard.
  - **The config-semantics partition is EXHAUSTIVE-BY-CONSTRUCTION over OWNER DOMAINS** — not a
    closed hand-list of options (which can never be provably complete and would re-mint the
    false-completeness defect). Every meaning-affecting tsconfig option belongs to EXACTLY ONE owner
    domain, by the option's role:
    - **config derivation** (discovery / `extends`-inheritance / normalized-option production) → `B.1`;
    - **module/path RESOLUTION surface** (the `moduleResolution` mode set + `exports` / `imports` /
      `paths` / `baseUrl` / `rootDirs` / `typeRoots` / `types` / `typesVersions` / `moduleSuffixes` /
      `customConditions` / `resolveJsonModule` / `allowImportingTsExtensions` /
      `allowArbitraryExtensions` / `preserveSymlinks` — i.e. anything that changes WHICH file a
      specifier resolves to) → `U0.RESOLVER_CORE` (#21);
    - **type-checking STRICTNESS** → `A.3`/#20;
    - **`lib` corpus selection** → `B.4`;
    - **emit/syntax SEMANTICS that branch type meaning** (the JSX, decorator/class-field, and
      module/interop families above) → `B.12`;
    - **cache KEYING dimensions** → the U10 guard
      `cache_keys_cover_ts_jsx_moduleresolution_decorator_lib_dimensions`.
    The named option lists above are EXEMPLARS of each domain, not the closed set; the catch-all is
    the role test (resolution-affecting → U0; strictness → A.3; emit-meaning → B.12; lib → B.4;
    derivation → B.1), so a not-yet-listed option (`moduleDetection`, `resolveJsonModule`,
    `allowImportingTsExtensions`, `allowArbitraryExtensions`, … now placed above; any future addition)
    is still owned by exactly one domain with no ownerless margin. The full per-option behavioral
    census stays a gate deliverable produced AT the `U2.RELATION_INFER` gate (the A.1 (4) backstop
    rejects any meaning-affecting option lacking a domain assignment + discriminating fixture) — this
    section fixes the OWNER DOMAINS, not the full behavior table.
- **`U2.QUERY_VALUE_DOMAIN` gate:**
  - **#3 — Formalize the `ProjectionDemand × EvalPolicy` lattice.** The partial order,
    the join / meet, the dominance + backfill rules, the satisfaction relation PROVEN,
    with worked examples for the important pairs (`Identity` / `Navigate` / `Shallow` /
    `Expanded` / `Skeleton`, path projection, carrier stops, generic-open) — the full
    lattice algebra, NOT just the five presets. **Satisfaction authority (LANDED — do
    not weaken):** the lattice is the demand/identity ALGEBRA only. Cache satisfaction
    is decided by the landed materialized-point model — a warm hit requires a RECORDED
    materialised `(path, point)` the compute actually produced to dominate the request
    at the same path (`cached_satisfies(MemoEntry.satisfied_projection,
    requested_point_for_key(key))`), NEVER nominal lattice dominance over the
    candidate's nominal slot/mode (nominal dominance is exactly the model that produced
    the removed lattice-unsound `Shallow → Navigate` backfill), and backfill clones only
    recorded materialized points. The gate's formalization COMPOSES with — never shadows
    or weakens — the landed guards
    `cache_satisfaction_is_materialized_point_not_nominal_demand` +
    `backfill_writes_only_recorded_materialized_points`
    (`crates/verter_session/src/semantic_query_memo/tests.rs`, registered in
    `critical_rules_have_guards.rs`; documented live in CLAUDE.md §Cache Architecture).
  - **Tabular `SemanticQueryKey → SemanticQueryValue → wire-projection`** (codex). Per
    key: identity fields, env-hash dimensions, value domain, fact domains read / written,
    allowed demand dimensions, cache family, producer, projection target.
  - **#14 — Canonical display policy.** Display is a PROJECTION over typed values, never
    a second resolver: one rule for how every family's typed value renders to its display
    string. Ties the **#3** lattice and the U13 published projection.
  - **#18 — Error-tolerance / resolution over broken / mid-edit code.**
    **Flagged EARLY — HIGHEST FUNCTIONAL RISK.** The editor steady-state is half-written
    source, so every query must produce a useful result over syntactically / semantically
    broken input. This gap may warrant escalation to a U0 / foundation treatment rather
    than staying inside this gate.
  - **#21 — Module-resolution matrix implementation.** Conditional `exports`, `node16`,
    `paths`, and symlink resolution, proven against the resolver / import-graph layer.
    Foundation-level: it is a resolver / import-graph deliverable that may rescope U0.
  - **DESIGN-GATE LOCK (`docs/arch/u2-query-value-domain-design.md`):** the
    `U2.QUERY_VALUE_DOMAIN` gate LOCKED **FORK-B** and **FORK-C** — the #18 broken-input
    taint PRODUCERS and the #21 module-resolution matrix IMPLEMENTATION are U0-owned; this
    gate keeps only the value-domain SHAPE / `admit_decision` (#18) and the
    module-resolution KEYING contract (#21).
  - **#22 — Error-type / `any` / `never` propagation lattice (worth-checking).** How the
    error type, `any`, and `never` propagate and absorb through reducers — ties the **#3**
    lattice.
- **`U3` / `U6` gates — #4 Fact-model completeness for the new fact domains.** PROVE the
  `ReadSetSignature` read-set tracer CAPTURES the footprints of
  `FactDomain::ProgramAnalysis` (a flow slice = control flow + EVERY narrowed symbol's
  type) and of relation results (the full structural reachability of BOTH related types),
  OR add the missing fact kinds — else warm hits go stale SILENTLY (the worst failure
  mode). (`#4` straddles U3's fact model and U6's `FlowSlice` / `FactDomain::ProgramAnalysis`
  domain.)
- **`native-checker` gate** (NEW sub-heading; `docs/arch/native-checker.md`):
  - **#15 — Hard typeinfo↔checker negative boundary.** Define the line typeinfo NEVER
    crosses: it does NOT compute whole-body diagnostics, does NOT run a full
    assignment-check (the public `relate` is the sole exception), does NOT compute
    control flow beyond the demanded slices, and does NOT perform checker error-recovery.
    Pairs with **#19** (reason-to-exist / authority boundary) — together they fix what
    native OWNS versus what it DEFERS to the running TS.
- **`U7` gate — #6 Parsimony pass (the DAG half RUN — locked DEFER).** The custom DAG
  scheduler half (deficit / credit lanes, model-checked liveness) was judged at the U7
  rescope gate and LOCKED at `docs/arch/u7-scheduler-submit-dag-decision.md`: DEFER —
  envelope held un-built, re-gated for JUSTIFY at U9 only, default permanent CUT. The
  persistent on-disk cache half was NOT ruled on by that decision doc and remains a
  LIVE justify-or-cut obligation — it attaches to `U4` (B9 persistent, the block that
  builds it) at entry: justify against the MEASURED I/O-bound LSP workload, or CUT —
  concentrate sophistication in the resolver + the cache.
- **Architecture-wide (substrate deliverable, stated once):**
  - **#7 — One unified soundness statement (BLOCKING the `U3` + `U10` gates; A.4).** Define
    ONCE what "**a warm hit is valid**" means uniformly across EVERY query family + fact
    domain, and require every cache to uphold that single invariant — versus the implicit
    distributed R1–R31 + per-family rules. **Re-filed from a free-floating architecture-wide
    deliverable to a BLOCKING prerequisite on the `U3` and `U10` gates: neither gate passes
    until the one warm-hit-validity statement is written AND every family's admission cites
    it.** Coupled with **#4** (the `ReadSetSignature` read-set tracer completeness is the
    MECHANISM #7's soundness statement audits — an incomplete tracer makes the single
    invalidation rail unsound). Added to the §9 terminal checklist.
    - **Candidate guard:** `unified_warm_hit_validity_statement_is_single_rail` — one
      soundness rail (`ReadSetSignature.facts` validation) is the sole warm-hit-validity
      authority; no family carries a private validity oracle.
  - **#7b — One unified determinism statement (stated ONCE, beside #7).** Same canonical
    inputs + the five split env dimensions ⇒ an IDENTICAL wire payload, independent of
    thread schedule, singleflight winner, AND cache temperature (cold compute ≡ warm
    hit). The `tsgo`-oracle snapshot comparison (the oracle driver fails loudly on any
    divergence), the checked-in `TypeExpr` snapshots, and the multi-candidate
    `FamilySlots` model all silently PRESUPPOSE this property — a schedule- or
    temperature-dependent payload would make every one of them flaky by construction —
    so it is written as one explicit statement rather than left implicit. BLOCKING
    alongside #7 at the `U3` + `U10` gates (the same gates that own warm-hit validity).
    - **Candidate guard:** `wire_payload_deterministic_across_schedule_and_cache_temperature`
      — a discriminating fixture runs the same request cold, warm, and under concurrent
      racing requesters and asserts byte-identical wire payloads.
  - **#8 — The differential `tsgo`-parity oracle as a DESIGNED structural surface** (tie
    to §3.4). Divergence from the oracle is observable BY CONSTRUCTION (a structural
    surface the engine emits), not ONLY via an external test harness.
  - **#9 — Unified degradation taxonomy (a NAMED contract; A.6).** ONE model for how every
    query degrades — miss / partial / fallback / unresolved — so every family reports the
    same degradation shape. Couples the FFI / client-compat surface (**#12**) and per-family
    fallback. **Promoted to a named degradation-taxonomy CONTRACT that the exporter (`U12`)
    and the session surfaces (`U9` / `U11`) consume BEFORE relying on reducer outputs** —
    sequencing note: the degradation taxonomy must be ready BEFORE `U12`/`U9`/`U11` depend on
    reducer outputs (added as a dep-map note, §3). This does NOT re-author the per-reducer
    absorption / admission table — that table already exists in
    `u2-query-value-domain-design.md §18/§22` and stays its authority; #9 only promotes the
    cross-family taxonomy + its consume-before-rely sequencing. Candidate guard at the
    `U12`/`U9` boundary: `unified_degradation_taxonomy`.
  - **#11 — Formal lib / intrinsics authority + TS-version pinning.** Define WHO owns
    `lib.d.ts` and the intrinsics, pinned to a specific TS version. Ties `lib_env_hash`
    and the pinned-`tsgo` oracle.
  - **#12 — FFI / client-compat ownership.** The downgrade is defined per wire variant,
    and no semantic distinction is lost across the NAPI / WASM / TS clients. Couples the
    degradation taxonomy (**#9**) and ties the U8 wire surface.
  - **#16 — Per-family performance + latency contracts.** Each query family's cost shape
    AND fallback shape, expressed as an interactive-latency SLO (ms — p50 / p95 on the
    MISS path; an LSP cannot gate on fallback-entry-COUNT). Each rescope gate emits its
    families' contract; ties §3.3 + §3.4.
  - **#17 — `lib.d.ts`-scale ambient cost (worth-checking).** The cost of carrying
    full-scale ambient lib declarations — ties the lib-authority (**#11**) and per-family
    perf (**#16**) deliverables.
  - **#19 — Reason-to-exist / authority boundary vs the TS already running.**
    **Flagged FOUNDATIONAL — the deepest strategic gap, root of the scope problem.** Why
    native reimplements what `tsgo` already serves; what native OWNS; where it DEFERS.
    Pairs with the typeinfo↔checker boundary (**#15**).

## 3.3 Effort / risk weights on the iceberg blocks (independent of row count)

Row count is **not** a difficulty signal — the hardest cores carry few rows. The
scheduler must treat the following **effort/risk weight as a scheduling input DISTINCT
from row count** (it sizes the rescope-gate session and the per-block fix-cycle budget,
it does not change the DAG order):

| Block | Rows | Effort / risk weight | Why |
|---|---|---|---|
| `U2.RELATION_INFER` | ~20 | **EXTREME** | 20 rows = the WHOLE TS checker core (relation + inference + variance). Row count vastly understates it. |
| `U6` (cross-engine recursion) | (flow chapter) | **EXTREME** | `FlowReturn`↔`ResolveCall`↔narrowing↔contextual fixed-point + the narrowing join algebra; non-convergence must fail closed. |
| native checker (`Check*`) | 0 (out of scope) | **HIGH** | net-new diagnostics layer over the same resolver; its own rescope gate. |
| `U3` (cache / fact model) | ~3 | **HIGH** | 3 rows but a concurrency-sensitive cache-eviction rewrite (per-family adaptive cap + invalid-first/LRU-by-valid-hit + global ceiling, invalidation-authority change). |
| `U7` (`submit_dag`) | — | **HIGH (justify-or-cut)** | highest scheduler risk; over-engineering candidate — rescope-gated against measured workload. |

The rest of the backlog is moderate/low weight at its stated row count. The weight is
advisory to the orchestrator (session sizing + review depth), **not** a re-ordering of
the acyclic dep-map (§3).

## 3.4 Differential tsgo-parity oracle (the semantic-correctness gate)

The semantic-correctness gate for the hard families is the **differential `tsgo`-parity
oracle harness** (owned in full by parent `native-typeinfo-parity.md` §6.3; this is the
sequencing/process pointer): run a corpus — a **TS-conformance slice + property-generated
type fixtures** — through **Verter AND the pinned `tsgo`**, diff the **STRUCTURED**
results, and gate on a **per-family divergence budget**.

- **It replaces the 362 proxy as the SEMANTIC gate.** 362-green proves wiring/coverage
  completeness (every row owned + executably proven); the oracle proves the engine
  agrees with `tsgo` on the families' SEMANTICS. The oracle is **produced / baselined at
  each hard phase's rescope gate (§3.2(b))** and gates THAT phase's acceptance — it
  converts the behavioral guards from "detects un-wired" to "detects **wrong**."
- **Per-family parity-coverage target shape:** "N TS-conformance cases per reducer
  family; divergence budget M per family" (each hard phase's rescope gate names its own
  concrete N / M for its families against the pinned `tsgo`).
- **Divergence semantics are owned by `docs/arch/ts-compat-two-mode-model.md` (LOCKED).**
  The resolver is single-spec — correct-by-default, no bug-for-bug compat mode, no spec
  dimension on any cache key. A Verter↔`tsgo` divergence is therefore CLASSIFIED,
  review-gated DATA, not a raw count: the `tsgo` snapshot records the `TsCompat` value,
  the review-gated correction overlay records the `Correct` value, and the checked-in
  divergence registry carries the classification. The per-family budget M gates the
  UNCLASSIFIED residue; a divergence classified into the registry (a conceded TS bug
  with its correction overlay) is review-gated data, not budget consumption.
- This is **distinct from** the §6.2 / U15 Verter-vs-`tsgo` PERFORMANCE benches (those
  gate cost-shape / fallback-count; this gates semantic agreement). Both run the same
  pinned `tsgo`.

## 3.5 The critical path (NAMED — session/review capacity follows it)

The dependency map concentrates the whole semantic-graph lane behind two EXTREME
blocks and the full Stage5 chain. That serialization is DELIBERATE and sound (never
build the wire/result/export stack around the doomed sidecar — §3.1.1), but it must
be named so orchestration, session sizing, and review capacity follow it instead of
discovering it:

```
U2.QUERY_VALUE_DOMAIN → U2.RELATION_INFER (EXTREME) → U6 (EXTREME) ─┐
U2 ─► S5.B5 → S5.B6/B7/B8 → S5.B9 → S5.B10 → S5.B11 → S5.B12 ───────┤  (joins at U8)
                                                                     ▼
                       U8 → U3 → U10 → U12 → {U11, U13} → U14 → U15
                                  └─► goto tail: G.P4 → G.P5 → G.P6/7
```

**THE critical path** is `U2.RELATION_INFER → U6 → U8 → U3 → U10 → U12 → {U11, U13} →
U14 → U15`, with the Stage5 arm `U2 → S5.B5 → … → S5.B12` joining at the hard gate
`U8 ← {U6, S5.B12}` (§3.1.1/§3.1.3) and the goto tail hanging off `G.P4 ← U3`. Every
block after U8 is serialized behind BOTH extreme blocks AND the full Stage5 chain —
so the orchestrator schedules its strongest sessions, deepest reviews, and earliest
rescope effort on `U2.RELATION_INFER`, `U6`, and the S5 chain, and treats slack
elsewhere (U1/U4/U5/U9, G.P1/G.P2, the foundation blocks) as genuinely parallel
capacity, not path work. The `U8 ← {U6, S5.B12}` serialization is a kept, labeled
bet (§2.3) — do not relitigate it; do name it when sequencing.

---

## 4. Unified block backlog (U0–U15)

Drive ONE block at a time: implement → triple review (independent reviewer +
codex) → per-block fix cycle until clean → land. Each block uses the template:
**ID / source track / scope / deps / parallelism / risk / required deletions /
guards**. Sequence is faithful to the binding merge-decision backlog order this
plan transcribes (§2); do not reorder.

---

### X0 — Cross-plan integration (this edit)

- **Scope:** the env re-home rule (§3.1.4), the three doc-set index clusters (native-typeinfo-parity / Stage5 / goto-definition, indexed in §0) wiring all subplans to this sequencing authority, and the back-references from the goto/stage5 docs to this plan.
- **Deps:** none (this is the integration edit itself; it gates nothing and is gated by nothing).
- **Done-when:** §3.1 cross-plan integration (incl. §3.1.1–§3.1.4) is present, and all 4 goto/stage5 docs (the 3 goto docs + the Stage5 cutover plan) back-reference this unified plan as their sequencing authority.

---

### XH — Deferred repo-wide phase-archaeology hygiene pass (tracked, not yet scheduled)

- **Scope:** all multi-block plan-pass / plan-rule labels in `crates/*/src/**` non-test comments left over from OTHER blocks of this refactor — the `\b[A-FR][0-9]+[a-z]?\b` family (cache-runtime F-passes, scheduler/semantic B-blocks `B4`/`B7d`…, R-rules `R6`/`R21`…) plus `C-phase`/`Pass C\d` pass labels. ~138 production comment labels repo-wide (count is indicative, not binding — the pass rederives it).
- **Goal:** rewrite every such comment as a final-state description (durable insight → `.claude/skills/*` or `docs/arch/`, per the No-phase-archaeology rule), then extend the existing `no_phase_archaeology_in_production_code` guard (`crates/verter_session/tests/architecture_guards.rs`) with ONE comprehensive branch catching the F/B/R/C-phase / plan-letter forms, backed by discriminating positive fixtures AND false-positive negatives (e.g. legitimate identifiers like `R21`-the-rule-name in a docs link, `B4`-in-a-hex-literal, `F32`/`C99`-in-type-or-std names) so the branch flags archaeology without flagging real code.
- **Why one pass, not per-block:** the labels are cross-cutting (a single file carries labels from several blocks), and a single guard branch with one fixture set is the only way to get non-overlapping, discriminating coverage; per-block scrubs would leave gaps and re-introduce drift. This pass is INTENTIONALLY out of U2 scope (codex scope decision B) and runs once the U-block sequence settles.
- **Done-when:** zero `[A-FR][0-9]+`/`C-phase` plan-label comments remain in `crates/*/src/**` non-test files; the extended guard is green and its negatives prove it does not flag legitimate identifiers.

---

### XP — nuxt-ui component-meta parity gaps (XP.2 + XP.3 LANDED on `mom/meta-xp23`; XP.1 corrected + XP.4 + XP.5 tracked, not yet scheduled)

Five CURRENT-pipeline component-meta parity rows, observed as typecheck
divergences against the `vue-component-meta` baseline on the real nuxt-ui corpus
(XP.1 corrected per the v4.8.2 re-verification below; XP.2 + XP.3 LANDED; XP.4 +
XP.5 added as post-U2 follow-ups from that same re-verification), plus a class/style
note recorded as a vue-component-meta divergence where Verter is source-faithful —
NOT a Verter defect.
These are TRACKED FOLLOW-UPS, not scheduled blocks: no critical-path insertion, no
deps-graph edges, no sequencing claim beyond "not yet scheduled". They are NOT
U14-adapter-rebuild scope — U14's "4 known Vue mismatch cases" (Popover
`SlotProps<M>`, theme-alias display, `Button["variants"]["color"]` indexed-access,
ContentSearch intersection) are a different, adapter-era list; do not merge these
into it. A fix landing before U14 binds U14's regression bar — the rebuilt
adapter must reproduce the fixed behavior. (U14's actual bar is its 4
mismatch-case regression tests plus its stated existing-corpus regression-risk
prose — "regression risk against the existing corpus is greatest here"; U14
names NO standing existing-corpus regression guard.) A
further observed defect from that original corpus diff (the Accordion
`update:modelValue` emit payload) maps to an EXISTING ignored manifest row and gets
NO item here — see the real-corpus manifestation note in the U2 block below.

The XP.2 / XP.3 family counts cited below (224 empty-description /
154 tags / 111 default-value, 81 of them quoting-only) come from the defining
artifact for these families: the diff of verter's component-meta output against
the `vue-component-meta` baseline over the real nuxt-ui corpus (the comparison
the `bench:meta:ui` harness's expected-backend mode drives;
`packages/benchmark/src/meta-ui-bench.ts`, corpus at
`.integration-tests/repos/nuxt-ui`). No in-repo artifact pins these counts —
the families are re-derived by re-running that comparison. The original XP.1
"253 missing members" figure from that same unpinned sweep is CLOSED — XP1PREP
refuted it reproducing via the stated heritage-`Pick` mechanism on five rails, and
the corrected XP.1 below carries its own pinned v4.8.2 evidence in its place.

**Severity order — XP.1 (sev 1) > {XP.2, XP.4} (sev 2) > {XP.3, XP.5} (sev 3),
and why:** XP.1 is member-PRESENCE loss — closed cross-file heritage / object-filter
members never surface at all (e.g. Button drops the entire `LinkProps` set); it
outranks every other row. The severity-2 rows are published-type/schema corruption
on surfaces that still partly appear: XP.2 is metadata loss at scale on members that
DO surface (224 empty-description / 154 tags mismatch families); XP.4 is a
directly-declared imported-alias prop wrongly EXPANDED (the published type becomes a
materially-wrong inlined surface where vcm and the heritage path both keep a shallow
`Ref`). The severity-3 rows are display/compat-layer divergences, type-semantics
harmless but baseline-divergent at volume: XP.3 is the quoting-only default-value
text divergence (default-value text fidelity distinguishes a string literal from an
identifier in the published API); XP.5 is the compat slot-binding function printer
divergence (`(props?: mapped): string` vs vcm's arrow form).

#### XP.1 — Routed cross-file heritage / object-filter (`Omit` / `Pick`) resolution drop (severity 1)

- **Closure of the original claim (REFUTED — do NOT keep the heritage-`Pick`
  wording or its fix direction):** the prior XP.1 mechanism — heritage `Pick<...>` /
  `Partial<Pick<...>>` member-drop — does NOT reproduce. The XP1PREP prework
  exonerated it on FIVE rails at verter `94fc46d3c` / corpus `v4.6.0-14-g39665c9d4`:
  fresh-cold native (back through `3424e23d8`), instrumented in-process predicate,
  one-host warm, session-overlay mimic, and the real JS-compat/NAPI bench — both
  candidate sub-mechanisms (the `KEY_DOMAIN_TYPE_EXPR_WALK_BUDGET = 256` exhaustion
  and the `'../types'` barrel-hop) measured ZERO firings corpus-wide, and the
  affected components' picked members all published. The original "253 missing
  members" figure traces to the unpinned pre-`3424e23d8` warm-rail
  partial-admission taint class (inferred, not a pinned artifact). Evidence rails
  are owed as committed artifacts under `docs/arch/parity-evidence/` (bisect /
  predicate / warm / session / realbench reports). **NO L1-classifier /
  `KEY_DOMAIN_TYPE_EXPR_WALK_BUDGET` / barrel-hop change is owed for that phantom**
  — implementing it would have risked weakening the correct L1 carrier-stop
  contract (`table_resolves_complete_and_warm` / `chat_messages_*`).
- **Corrected defect (codex-ruled; re-verified @ verter `4484e8596` / corpus
  `v4.8.2-30-g52d3c4547` / vue-component-meta 3.3.2):** closed, resolvable
  cross-file heritage / object-filter (`Omit` / `Pick`) sources are carrier-stopped
  or contribute ZERO members when the source declaration is reached through a
  routed materialisation path — BOTH (a) an `export *` barrel re-export of a type
  declared in a `.vue` `<script>` block (`LinkProps`, declared in `Link.vue`,
  re-exported via `types/index.ts` `export * from '../components/Link.vue'`), AND
  (b) a package-backed declaration (tiptap `EditorOptions` for `Editor.vue`). The
  honest root (codex): the `export *` CAN surface the name — the failure is the
  LATER heritage / `Omit` / closedness materialisation route, where the routed
  declaration becomes opaque / undecidable so the open-or-unknown carrier-stop
  suppresses the arm or it merges in zero members. This CONFIRMS the original
  XP.1's own candidate (b) (the barrel → `.vue` hop) over candidate (a) (the walk
  budget). Do NOT narrow it to ".vue barrel only" — the package-backed tiptap
  heritage is the same class.
- **Evidence (pinned):** Button verter=20 vs vcm=57 — 38 missing = exactly the
  `LinkProps` (NuxtLink / RouterLink / anchor) set plus the polymorphic `as`
  (Verter DOES publish the `UseComponentIconsProps` members; only the
  `Omit<LinkProps, …>` heritage arm drops); ContentSearchButton 27 missing (the
  inherited `ButtonProps` surface); Editor 29 missing (tiptap `extensions` /
  `editable` / `onUpdate` / …); EditorDragHandle 30 missing — the Button/link set
  PLUS the tiptap `nestedOptions` / `pluginKey` pair, which FOLDS IN the XP1PREP
  residual (those two were the only drop at v4.6.0). Discriminator pinning the
  mechanism: `Link.vue` ITSELF (`withDefaults(defineProps<LinkProps>(), …)`,
  same-file, no barrel, no `Omit`) resolves ALL 36 `LinkProps` members; the
  same-workspace `.ts` heritage (`UseComponentIconsProps` in
  `composables/useComponentIcons.ts`) resolves correctly — only the routed
  cross-file hop drops. (The comparison agent's higher per-component figures —
  Button "76" etc. — likely summed props+slots+events+schema; the named prop sets,
  direction, and mechanism reproduce exactly, the absolute counts do not, so this
  row pins the mechanism, not the count.)
- **Corpus-version confound (NO regression claim):** XP1PREP ran at corpus
  `v4.6.0`, this re-verification at `v4.8.2`; the vcm reference and the corpus type
  shapes both moved. XP1PREP's "0 missing" and this run's drops are NOT
  apples-to-apples — this row asserts the state AT verter `4484e8596` + corpus
  `v4.8.2-30-g52d3c4547` and the mechanism, NOT a Verter regression between commits.
- **Scope — absorbs the "deepening half" of XP.4:** the `Alert.avatar` cross-file
  member value-collapse-to-bare-`object` (Avatar.vue's `AvatarProps` members losing
  their value types when deepened through the `'../types'` barrel) shares THIS
  resolution surface and is carried under this umbrella — but with a SEPARATE
  acceptance test, because member-presence loss (this row) and member-value
  collapse are distinct symptoms of the same root. The expand-vs-shallow projector
  half stays in XP.4.
- **Connects to existing IGNORED contracts (the fix likely lands IN this work, not
  a standalone parity block):** U10
  `mode_boundary_reexport_chain_resolves_imported_alias` (verter leaves `Foo` an
  unresolved `Ref` after a re-export chain ending in `export * from './barrel'`) and
  U6 `flow_return_xf04_expands_barrel_imported_value_function_return` (barrel-imported
  expansion). The component-meta heritage drop is the parity manifestation of the
  same routed cross-file resolution gap those typeinfo contracts track.
- **Owner files:** `crates/verter_session/src/resolver_core/…` (the host-backed
  routed re-export / barrel / package-backed resolution into a `.vue` module's
  `<script>`-block named-type exports);
  `crates/verter_session/src/project_semantic_dispatch/raise.rs` (key-domain
  closedness walk — the opaque-hop conservative-OPEN when the heritage `Omit` source
  is the barrel-`.vue` or package type); the `verter_session` heritage member-merge
  for `extends`-clause contribution.
- **Mechanism (STEP-0 must NAME which fires):** (i) the routed `export *` / package
  hop leaves an unresolved `Ref`; (ii) the `Omit` / `Pick` key-domain goes
  open-or-unknown so the L1 carrier-stop suppresses the arm; or (iii) the heritage
  member-merge contributes zero members.
- **Acceptance (discriminating):** hermetic fixtures for BOTH routed shapes — a
  `.ts` barrel `export *`-ing a `.vue` `<script>`'s `export interface X` consumed by
  a component `extends Omit<X, 'k'>`, AND a package-backed `extends` — that FAIL
  pre-fix and PASS post-fix; feature-gated `external-corpus` checks (Button
  publishes the `LinkProps` set; `Alert.avatar` members carry real value types); the
  existing L1 carrier-stop pins plus `table_resolves_complete_and_warm` /
  `chat_messages_*` trackers stay green — the fix must NOT buy the members by
  weakening L1.
- **Workstream:** POST-U2 parity (codex: finish U2 first). The fix likely lands IN
  the cross-file resolution rebuild it depends on — coordinate with the external /
  imported resolution work (the U6 / U10 cross-file contracts above and the Stage-5
  macro-surface rebuild `S5.B5`–`S5.B12`); STEP-0 confirms whether this folds INTO
  that rebuild or follows it, rather than standing up a separate parity block. Not
  scheduled: no critical-path insertion, no deps-graph edges.

#### XP.2 — Per-member JSDoc loss across the cross-file publication path (severity 2)  **(LANDED — branch `mom/meta-xp23`)**

- **Landed state:** JSDoc rides the materialised member span-borne for ALL kinds,
  exactly per the fix direction below. Mechanism: (a) homomorphic mapped
  production threads the matched source member's `spans` + `declaration_origin`
  through (`project_semantic_dispatch/build.rs`, `walk.rs`) so cross-file docs
  survive `Partial`/`Pick`-class production; (b) emit doc supply was VERIFIED
  as already owned by the pre-existing DTO rail (call-signature,
  property-style, and generic-instantiated emits) and is now LOCKED by
  coverage fixtures — no emit-pairing production change was needed (the
  evaluator-only branch has no doc carrier, so name-pairing there is dead
  code); (c) the SFC
  name-scan text fallback is deleted; (d) exposed members gain real doc supply
  (object-literal leading-JSDoc capture on `AnalyzedExposeField` + the enriched
  surface spans for `defineExpose<T>()`; the published exposed surface is the
  UNION of the object-literal fields and the type-argument surface members,
  so a type-argument-only `defineExpose<T>()` publishes its members with
  their docs and raised surface types) and the named shape change landed:
  `ExposedAnalysis`/`ExposedMeta.tags` end-to-end (proto `ExposedMeta` field 5
  + `PublicInstanceMemberMeta` field 7, appended — additive);
  (e) the post-hoc name-keyed prop-only repair
  (`fill_missing_component_meta_prop_descriptions_from_imported_roots` + its
  barrel/heritage BFS helpers) is DELETED — span supply is the sole doc path,
  with the six tests that previously rode the repair green through the primary
  path. Hermetic discriminators in `macros_tests.rs` / `meta_tests.rs` /
  compat specs. HONEST RESIDUAL: external-corpus (nuxt-ui) re-verification of
  the 224/154 families is unavailable in-env (corpus not checked out) and
  remains gated follow-up evidence.

- **Defect:** 224 of the 253 description mismatches in the baseline diff are
  verter publishing an EMPTY description where the source member carries a doc
  comment; 154 `tags` mismatches are the same family — on members that DO
  surface.
- **Owner layer:** the shared shallow-surface JSDoc span supply PRIMARY, with
  the component-meta publication repair as a SEPARATE, narrower mechanism. TWO
  cross-file doc paths exist on the current tree — do not conflate them:
  (i) the span-carrying macro DTO path — `component_meta_resolved_macros`
  (`crates/verter_session/src/resolver_core/component_meta/mod.rs:149`) feeds
  `ResolvedMacroInput` props/emits/slots from `vue_macro_dtos`, whose surface
  slices JSDoc from the typeinfo spans for props, emits, AND slots
  (`crates/verter_session/src/typeinfo/adapters/vue/surface.rs:798`, `:989`,
  `:1257`); and (ii) the post-hoc, prop-only, name-keyed doc repair in
  `crates/verter_session/src/host_manage/component_meta_extract.rs` (~:878-985,
  call site :1571), which categorically excludes events/slots/exposed. The
  failure mechanism is therefore NOT a missing cross-file doc path: it is the
  lost members not participating in path (i) for their doc supply, and/or path
  (i)'s span supply losing source — semantic-graph-materialised members are
  built with EMPTY JSDoc spans
  (`crates/verter_session/src/typeinfo/surface.rs:464-470`) and rely on the
  host-layer `with_member_jsdoc_spans` enrichment (`surface.rs:306-420`), whose
  per-member fail-arm silently keeps the empty spans (members
  `surface.rs:348-355`, signatures `:393-400`) — loss whenever the member
  carries no `origin.canonical_file` or no `name_span` (synthetic /
  multi-origin members), or the declaring file's source
  (`IndexedReady.raw_source` via `source_for`) is unavailable to the
  enrichment closure.
- **Architectural fix direction:** JSDoc rides the materialised member through
  publication for ALL member kinds (props, events, slots, exposed), deleting the
  post-hoc name-scan repair — NOT a third recovery heuristic layered on top of
  the existing two mechanisms.
- **Acceptance (discriminating):** a surfaced member whose source declaration
  carries a doc comment publishes its description across props, events, slots,
  and exposed, and its tags across props, events, and slots, on the real corpus
  (the 224-description / 154-tags families go to zero modulo genuine baseline
  divergences). Exposed TAGS are an explicit API-extension requirement, not a
  repair: the published shape carries `description` but NO `tags` field for
  exposed today (`ExposedMeta`, `packages/component-meta/src/types.ts:213`;
  `ExposedAnalysis`,
  `crates/verter_semantic/src/analysis/component_meta.rs:232`) — adding it is
  in scope only as a named shape change.

#### XP.3 — Default-value quoting divergence (severity 3)  **(LANDED — branch `mom/meta-xp23`)**

- **Landed state:** the producer returns the verbatim source slice for string
  literals through the one shared helper `default_value_source_text`
  (`verter_semantic::analysis::macros`), used by BOTH the macro and Options API
  extractors (the two divergent extractors are unified); the compat
  `looksLikeStringCompatibleType` string-ness-inference branch is deleted;
  `evaluateDefault`'s lossless quote-STYLE normalization is kept per the
  acceptance note below. Hermetic discriminators flipped and landed
  (`macros_tests.rs`, `options_tests.rs`, compat specs, end-to-end meta test).
  HONEST RESIDUAL: the 81-row reconciliation against the real nuxt-ui baseline
  is unavailable in-env (corpus not checked out) — gated follow-up evidence.

- **Defect:** 81 of the 111 default-value mismatches in the baseline diff are
  quoting-only — verter prints `vertical` where the baseline prints `"vertical"`.
- **Root cause (verified):** `extract_default_value_text`
  (`crates/verter_semantic/src/analysis/macros.rs:2381-2405`) special-cases
  `Expression::StringLiteral(s) => s.value.to_string()` — the unquoted inner
  value — while every other expression kind returns the verbatim source slice.
- **Owner layer:** producer (the `verter_semantic` analyzer); the display layers
  pass the text through. The fix locus is the producer (return the
  source-faithful text). The compat layer must NOT infer quoting from descriptor
  string-compatibility — that is the forbidden recover-meaning-at-display
  heuristic (Typed-IR-Only Resolver Rule).
- **Adjacency (cited as adjacency, NOT existing tracking):**
  `TODO(typed-default-values)`
  (`docs/arch/typed-ir-cutover/compat-heuristic-mapping.md:120`) and the `S5.B5`
  normalizer "defaults must not lose distinctions" compatibility matrix (§3.1.1)
  are adjacent surfaces; neither covers this producer-side text fidelity defect.
- **Acceptance (discriminating):** string-literal defaults publish source-faithful
  quoted text (`'vertical'` stays quoted) with the producer returning the
  verbatim source slice; the 81 quoting-only rows reconcile against the baseline.
  Because the baseline prints the TS-printer-normalized form (`"vertical"`)
  while the verbatim slice may be single-quoted (`'vertical'`), the parity
  comparison may still need quote-style normalization vs the baseline's printed
  form — an item-text detail, not a layer change: LOSSLESS quote-STYLE
  normalization of an already-string-literal value at the
  parity-comparison/display boundary (string-literal-in → string-literal-out,
  no type-driven inference) is permitted, and is distinct from the forbidden
  recover-meaning-at-display heuristic, which would INFER string-ness from
  descriptor type compatibility.

#### XP.4 — Directly-declared imported-alias prop wrongly EXPANDED (severity 2)

- **Defect (re-verified @ verter `4484e8596` / corpus `v4.8.2-30-g52d3c4547` /
  vue-component-meta 3.3.2):** a directly-declared prop whose type is an imported
  alias is EXPANDED instead of published shallow — a Shallow-By-Default violation.
  `Alert.vue` declares `avatar?: AvatarProps` directly (`AvatarProps` imported from
  the `'../types'` barrel); vcm publishes `avatar?: AvatarProps | undefined` (a
  shallow named ref) but Verter inlines the full `AvatarProps` surface. The contrast
  that isolates the anomaly: `Button.props.avatar` reaches the SAME `AvatarProps` via
  `UseComponentIconsProps` HERITAGE and Verter keeps it shallow
  (`AvatarProps | undefined`, matching vcm) — so the same type stays shallow through
  heritage but expands when directly declared.
- **Scope split:** this row is the expand-vs-shallow projector half ONLY. The second
  symptom — the expanded members then collapsing to bare `object`
  (`crossorigin?: object`, …) — is a cross-file member value-collapse on the SAME
  routed resolution surface as XP.1 and is carried under XP.1's umbrella with its own
  acceptance test; it is NOT this row.
- **Owner files:** `meta_resolve::projectors` (`reduce_field_type_expr_with_mode` —
  why a directly-declared imported alias materialises instead of staying a `Ref`;
  contrast the heritage path that stays shallow).
- **Acceptance (discriminating):** `Alert.avatar` publishes `AvatarProps` shallow
  (matching `Button.avatar` and vcm), with a negative assertion against expansion; a
  hermetic fixture (a component directly declaring `x?: ImportedAlias`) FAILS pre-fix
  / PASSES post-fix.
- **NOT bundled — separate future row:** whether Verter should honor `@vue-ignore` on
  heritage arms (Avatar.vue's `AvatarProps extends /** @vue-ignore */
  Omit<ImgHTMLAttributes, …>` surfaces the ignored arm; vcm honors `@vue-ignore`) is
  a SEPARATE Vue-macro-policy question needing its own direct vcm/Verter prop-set
  evidence — do NOT fold it into XP.4 or XP.1.
- **Workstream:** POST-U2 parity. STEP-0 may merge with XP.1 (shared root for the
  value-collapse half); the expand-vs-shallow projector decision lives here. Not
  scheduled: no critical-path insertion, no deps-graph edges.

#### XP.5 — Compat slot-binding function printer divergence (severity 3)

- **Defect (re-verified @ verter `4484e8596` / corpus `v4.8.2-30-g52d3c4547` /
  vue-component-meta 3.3.2):** the `@verter/component-meta/compat` layer renders
  slot-binding functions as `(props?: mapped): string` — a literal `mapped` token in
  method-call syntax — instead of vcm's
  `(props?: Record<string, any> | undefined) => string`. Two divergences: the
  `mapped` token (a `Record<string, any>` param that degrades to an
  `unknown("mapped")` descriptor) and method-call vs arrow syntax. The structured
  slot `TypeDescriptor` is otherwise intact — this is a compat DISPLAY/bridge defect,
  NOT a native member loss, observed at volume (nearly every component with slots).
- **Owner files (TS, compat display + bridge ONLY):**
  `packages/component-meta/src/compat/checker.ts` (`compatFunctionTypeToString` —
  emit arrow syntax + the optional `| undefined`; `buildCompatUiBindingType` already
  emits the correct form but only engages on the indexed-access fast-path) and
  `packages/component-meta/src/type-expr-bridge.ts` (`resolveMappedDescriptor` —
  resolve `Record<string, any>` to an index-signature descriptor instead of
  `unknown("mapped")`). codex: the fix must NOT depend on keeping the binding shallow
  to trigger the `buildCompatUiBindingType` fast-path (that is a display fast-path,
  not an architectural dependency). Typed-IR-only — fix the producer (bridge +
  printer), NOT a text patch on the output string.
- **Acceptance (discriminating):** a vendored slot-binding fixture renders
  `(props?: Record<string, any> | undefined) => string`; negative assertions that the
  literal `mapped` token and the method-syntax `): string` are absent.
- **Workstream:** SEPARATE `@verter/component-meta` compat bridge/printer workstream —
  does NOT need the XP.1 / XP.4 resolver work and can land independently. Not
  scheduled: no critical-path insertion, no deps-graph edges.

#### class/style — vue-component-meta divergence, Verter source-faithful (NOT a defect row)

- **Finding (codex ruling B; re-verified @ verter `4484e8596` / corpus
  `v4.8.2-30-g52d3c4547`):** Verter publishes `class` / `style` IFF the source
  declares them as props, and does NOT inject inherited/fallthrough attributes as
  props. 113 of 121 top-level nuxt-ui components declare `class?: any` explicitly and
  Verter publishes `class` exactly on those; the 8 non-declaring components (App,
  Icon, DashboardSidebarToggle, EditorDragHandle, LinkBase, OverlayProvider, Theme,
  DashboardSidebarCollapse) publish NEITHER `class` NOR `style` — no spurious
  injection, no inherited-attr surfacing. vcm strips declared `class` / `style` from
  the props list as its own behavior.
- **Disposition:** Verter is the more source-faithful side — this is a
  vue-component-meta divergence where Verter is correct, **NOT a Verter defect: NO
  defect row, NO resolver change owed.** If byte-for-byte vcm compat is ever required,
  stripping declared `class` / `style` is a compat-PROJECTION-only change (never a
  resolver change), and the `bench:meta:ui` expected-backend comparison should NOT
  count this divergence against Verter.

---

### UP — LSP / Compiler Performance Backlog (tracked, not yet scheduled)

Tracked perf follow-ups handed off from the retired perf line. NOT on the
§3.5 critical path; NO deps-graph edges into the U-chain EXCEPT the explicit
unblock edges named below. Ungated UP.1 / UP.2 are parallel capacity (like
U1/U4/U5). Every item is a branch-independent SPEC (the source perf branches are
retired; SHAs are historical only — except UP.1's three, whose diffs landed in
this integration and cite their real refactor-history commit SHAs).

This block is the perf-track HOME: the prior `docs/arch/perf-handoff-to-semantic-overhaul.md`
is now a thin pointer to here, and `docs/arch/perf-lsp-orchestration-plan.md`
remains only for the historical multi-track integration model. The gated items
have their DELIVERABLE in an owning U-block (cross-ref'd in §B below); **UP.G is
the inventory index, the U-block carries the work.**

**Off-critical-path disclaimer.** Nothing in UP gates a U-block and no U-block
depends on a UP item except via the explicit unblock edges recorded in the
owning U-block's scope bullet (U11.C17 → UP.C4; U11.C19 → UP.C2b; U11.C20 →
UP.C8 host-reach; U11.F1 / U3.F2 / U3.B-typeinfo land their own deliverables).
The 3.5 critical path is unchanged.

**Governance NOTES (future-touch points; recorded for the governance reviewer +
future implementers — no CLAUDE.md / skill edit is made now):**
- **C15** (UP.D) touches a CodeTransform-CRITICAL surface and proposes a new
  static guard (`codetransform_no_post_build_provider_text_rewrite_static_guard`) —
  a future CLAUDE.md / skill touch when it lands.
- **D-F4** (UP.D) is an output-breaking CSS scoped-hash cutover — it interacts
  with the Cross-Platform byte-equality rule AND the CodeTransform-single-source
  rule; future-governance touch when it lands.
- **B-typeinfo / F2** (U3) must respect the single-engine
  `no_new_resolve_type_engine_path*` cluster + R6 / R21 + `no_off_store_host_caches`
  — a NEW cache DB off `ProjectTypeStore` would trip the R6 meta-guard. No new
  off-store cache is sanctioned by this backlog.
- **L-B TypeLocation** (UP.G homeless) may require a Typeinfo Wire Contract schema
  bump if `TypeLocation` is inside the closed-enum surface — confirm before
  implementing; STOP for user sign-off.

#### UP.0 — Prerequisites

- **PRE-0 — restore the LSP perf-counter substrate.** The framework-agnostic
  refactor deleted `crates/verter_lsp/src/perf_counters.rs`; only a generic
  disabled-by-default `statistics.rs` survives. Revive `perf_counters` (or extend
  `statistics.rs`) with the named counters `lsp.did_change.total_ms`,
  `host_upsert_ms`, `line_index_rebuild_ms`, `position_mapper_from_json_ms`, plus
  the drain / scanner / restart-replay counters. **Gates every UP.M item** — those
  land unmeasured without it.
- **PRE-1 — decide where the open/close-session epoch lives (`OpenSessionEpochAuthority`).**
  The C keystone's close-ABA fix (UP.C2) and item 8's completion-freshness rail
  (UP.C8) were designed to reuse the sync-coordinator's open-session epoch; the
  refactor replaced `sync_coordinator` with a function-based
  `spawn_sync_coordinator` / `coordinator_loop` carrying only `ownership_ready`
  flags — there is no epoch authority to reuse. This is a real design decision
  (an LSP-local server field vs a thin shared seam), NOT a port. **Blocks UP.C2
  and UP.C8.**

#### UP.1 — LANDED in THIS integration (ungated, real tested commits — NOT spec-only)

These three rode into the refactor on the fast-forward as real, tested,
separately-gated commits (touching only verter_scheduler / verter_workspace /
verter_lsp). Their diffs ARE in refactor history; the source perf branch is
retired and the SHAs below are the refactor-history commits, branch-independent.

- **G8.2 — scheduler supersede reverse-indices.** **LANDED ✅ (commit
  `24532fbca` in refactor history — branch-independent, source branch retired).**
  Supersede via per-canonical reverse maps: no global crate-wide scan; the sweep
  iterates only the entries in the bumped canonical's reverse-index buckets
  (node-token / file-waiter / blocker-owner / terminal-failure), so the work is
  O(affected) — proportional to that file's stale entries — not O(total DAG);
  `verter_scheduler/src/dag.rs` + `dag/{blocker_registry,terminal_failures}.rs`
  (+ `dag_supersede_index_tests.rs`).
- **G9.1 — workspace realpath memo.** **LANDED ✅ (commit `a4333b753` in
  refactor history — branch-independent, source branch retired).** Memoize
  `NativeFs` realpath with dir-index invalidation;
  `verter_workspace/src/{native_fs,filesystem}.rs` (+ `filesystem_tests.rs`).
- **G14.1 — `Arc<PositionMapper>` read-path share.** **LANDED ✅ (commit
  `0f2e19fc1` in refactor history — branch-independent, source branch retired).**
  Hand out `Arc` clones instead of deep-cloning the mapper + lookup tables on
  every hot provider read; `verter_lsp/src/documents/{mod.rs,provider_projection.rs}`
  (`Arc<PositionMapper>` on the `CarrierIde` / `SourceMap` carriers) + an
  `Arc::ptr_eq` regression test. **CROSS-NOTE for U13 / U14:** do NOT regress this
  `Box → Arc` read-path share if the published-projection layer reshapes
  `DocumentProviderProjection` / `ProviderPositionMapper` — re-apply the
  read-path Arc share, never revert to a per-read `Box` deep-clone.

#### UP.2 — Track-C ungated (LSP-local; no `verter_session/src` edit)

LSP-local perf items, all branch-independent specs. Preserve the handoff's
serialized order and the internal deps (UP.C4 ← UP.C2a; UP.C9 ← UP.C8). Carry
the §4 wave-1 review lessons (item-12 ABA = compare-and-swap from the SAME single
snapshot; item-23 / keystone TOCTOU + close-ABA = a single non-atomic version
check on a detached task does NOT close the ABA hole; the armed-restart monitor
owns its own cancellation) into UP.C2 / UP.C12 / UP.C19.

Suggested serialized order: PRE-0 → PRE-1 → quick LSP-local wins (C5, C14, C3) →
keystone C2a + C2A-residual → **C4 (AFTER C2a — C4 consumes `editor_source_version`
that C2a introduces)** → background/diagnostics cluster (C7 → C6 → C10+C13 → C7A)
→ transactionality (C12, C13a) → C23 active-first replay → C8 → C9.

- **C3 — LineIndex shared `Arc<str>` source.** `verter_type_runtime/src/codec.rs`
  `LineIndex.source: Vec<u8>` full-copies; `did_change_incremental` rebuilds per
  change. Back `LineIndex` by `Arc<str>`.
- **C5 — SFC-block-scan cache.** No `sfc_blocks` field on `DocumentState`; every
  handler re-runs `scan_sfc_blocks(&doc.source)` (nav_features / aux_features /
  custom_methods / component_resolve sites). Add `sfc_blocks: Arc<[SfcBlock]>`,
  populate in did_open / did_change, key off `FileLanguage::is_framework_carrier()`
  (carrier-general, NOT a `.vue` literal).
- **C11 — single-walk scanner.** Two full FS walks remain
  (`collect_carrier_paths` / `collect_source_paths` in `workspace_scanner.rs`);
  priority is a post-collect reorder. Add `walk_streaming` to `WorkspaceRead` /
  `NativeFs`; fold into one streaming inventory + `BinaryHeap`; re-express onto
  the refactor's async `scanner_loop` / `ScannerSignal`.
- **C12 — provider-sync single-snapshot CAS.** `prepare_sync_transition` /
  `commit_sync_transition` exist but commit is a bare `states.insert` with NO
  CAS. Add an explicit-previous-snapshot transition + `commit_if_state_unchanged(expected)`
  CAS; route non-carrier shadow transitions through one two-phase helper. (Carry
  the wave-1 ABA lesson: prepare from the same snapshot you later compare.)
- **C13a — drain-liveness split.** `drain_pending_snapshot_provider_sync`
  re-collects all pending with no reconciliation memo. Add a
  `ReconciliationSignal` / `ReconciledMemo` gate over the `SyncOutcome` drain.
- **C14 — canonical reverse-index O(1).** `canonical_id_to_uri`
  (`documents/mod.rs`) linear-scans. Add a `DashMap<canonical,uri>` maintained on
  did_open / close / change. Pairs with C8.
- **C2a — keystone, provider-sync off the `did_change` critical path + 2 freshness
  fields.** `handle_did_change` still awaits `sync_tsx` under `did_change_mutex`.
  Add `edit_arrival_version` + `editor_source_version` fences (a per-canonical
  map, or onto `DocumentState`); read the epoch from the PRE-1
  `OpenSessionEpochAuthority`; move eager sync into a detached version-checked
  task that self-drops when `live_edit_arrival > captured`. This part alone does
  NOT make same-canonical push race-free (TOCTOU / close-ABA per §4); the robust
  fix is C2b. **Uses PRE-1.**
- **C2A — keystone, style-only skip.** The provider-sync block is already skipped
  on `slice_changes.is_style_only()`; the residual compile / get_ide / mapper
  rebuild in the delegated `DocumentRegistry::did_change` still runs
  unconditionally. Gate the carrier compile / mapper rebuild on an `affects_ide`
  flag passed down from `handle_did_change`. (`is_style_only()` is already public
  on `HostUpdateResult.slice_changes`.)
- **C4 — cached `MappingContext`.** `provider_projection_context` re-builds
  `LineIndex::new` + a mapper clone per request. Cache
  `{Arc<str>, Arc<LineIndex>, Arc<PositionMapper>}` keyed
  `(canonical, editor_source_version, ide_path, artifact_hash)`. **Depends on
  C2a's `editor_source_version`** (sequence after C2a). Interim identity: derive
  `artifact_hash` LSP-side by hashing host-returned `ide.code` + `source_map`
  (NOT the doc version — `ensure_current_file_synced` can refresh artifacts
  without a version bump; the hard-invalidation test
  `mapping_context_invalidates_when_artifact_hash_changes_without_doc_version_change`
  must pass on the interim hash). **GATED-on-U11.C17 for the identity-complete
  version** (edge UP.C4 ← U11.C17): once the host returns an artifact id/hash,
  re-key onto it and STOP hashing generated strings (guard
  `mapping_context_uses_host_returned_artifact_identity_only`). Land ungated with
  the interim hash now; re-key when C17 lands.
- **C6 — background diagnostics + publish-token.** Non-interactive pulls use
  normal `get_diagnostics` with no token. Use background priority + a 6-field
  publish token revalidated post-await.
- **C7 — explicit background sync priority.** Background paths call plain
  `sync_tsx`. Make priority explicit in the API.
- **C7A — SyncCoordinator execution split.** `spawn_sync_coordinator` /
  `coordinator_loop` still `await`s sync + publish inline, blocking `rx.recv()`.
  Dispatch sync / publish as cancellable versioned tasks so the loop keeps
  draining.
- **C8 — VerterRequestIngress + completion freshness.** `completion_generation`
  is checked globally; the provider await has no post-await freshness check; no
  ingress. Per-request tokens / deadlines / cancel; scoped completion
  supersession; a freshness oracle. **Largest item; needs PRE-1.** (The
  host-reach cancellation half is GATED — U11.C20; the provider-native cancel
  half is measurement-held — UP.M.)
- **C9 — remove completion retry-sleeps.** `tokio::time::sleep` is on the
  completion path. Replace with `isIncomplete:true` + background repair.
  **Depends on C8.**
- **C10 — single `DiagnosticPublisher`.** Now 3-way duplicated
  (background_init ×2, sync_coordinator, rune_module_diagnostics). Collapse into
  one publisher taking the cached `MappingContext` + the C6 token.
- **C13 — collapse background-init dup.** Two open-file diag loops in
  background_init; folds into C10.
- **C2b — keystone, robust same-canonical push serialization. GATED.** Per-canonical
  push ordering with a monotonic accepted-version so a stale provider push can
  never clobber a newer one and close can never resurrect a closed doc (the ABA
  hole). **GATED on U11.C19** (edge UP.C2b ← U11.C19 — the per-canonical-lock +
  host-concurrency-contract territory). Lands with / after C19; do NOT pretend
  C2a closes the ABA hole. **MUST NOT depend on `verter_scheduler::CpuConcurrencySemaphore`**
  (U9 records it deletion-bound).
- **C23 (active-first replay, UNGATED).** On provider restart, replay active /
  open files first → mark the provider usable → background-replay the remainder;
  stale-replay linearization (per-path commit mutex + `restart_generation`).
  Re-express onto the refactor's single generic `ResilientProvider<P,B>` /
  `ResilientBackend` (`verter_type_runtime/src/resilient.rs`). Protected
  invariant: every previously-replayed file stays replayed; the K active files
  replay and the provider is marked usable before the bulk cached files finish.
  The armed-restart `restart_generation` rail must own its own cancellation
  (forward spawn-failure into the monitor; never abort it from a generation
  read), and the test matrix must include the wake-before-poll scheduler
  ordering. **The shared content-store half (Arc<str> provider content) is
  MEASUREMENT-HELD — UP.M — keep it OUT of this replay commit.**

#### UP.D — Compiler-perf, gated

- **D-F4 — output-breaking CSS scoped-hash cutover. GATED (measurement + a
  tested output diff).** A changed CSS scoped-hash changes generated bytes, so it
  is a tested cutover that interacts with the Cross-Platform byte-equality rule.
  Land with the output-diff cutover tests (the changed CSS hash byte-tested).
  **GOVERNANCE-FLAG:** Cross-Platform byte-equality CRITICAL + CodeTransform
  single-source CRITICAL (no post-`build_string()` rewrites).
- **C15 — ranged provider updates only over provider-materialized text. GATED
  (HARD-ORDER prerequisite).** Boundary is `verter_type_runtime` TSGO `ipc.rs` +
  a compiler/codegen handoff (NOT `verter_session`). Ranged provider `didChange` /
  diff updates instead of full-document re-sends. The former provider-side
  import rewrite (`rewrite_vue_imports_for_tsgo`) and its offset-translation
  compensation have been ELIMINATED: TSGO now receives byte-identical generated
  text (carrier→carrier imports are suffixed at compile time through
  `CodeTransform`; plain-file `./Comp.vue` specifiers are resolved to the
  `.verter.ts` carrier by the workspace resolver before the bytes reach the
  provider), so provider-visible offsets already match the source. With the
  byte-shifting rewrite gone, the prior HARD-ORDER prerequisite (forbidding
  direct source-range forwarding until the rewrite was emitted through
  `CodeTransform`) is satisfied by byte-preserving resolver behaviour. Until
  ranged updates are wired, every provider range/diff is
  computed ONLY over previous/current provider-materialized `ide.code`, never
  source ranges. **GOVERNANCE-FLAG:** CodeTransform single-source CRITICAL +
  proposed guard `codetransform_no_post_build_provider_text_rewrite_static_guard`
  (fails on any post-`CodeTransform` provider-text replace / regex / splice once
  the rewrite is deleted).

#### UP.M — Measurement-gated (run CPU-idle AFTER PRE-0; do NOT benchmark during implementation)

Gated on the PRE-0 named counters; do NOT benchmark while implementing.

- **C23 shared content-store half** — separate provider content storage from
  open/load mode; share `Arc<str>` content between `ResilientProvider` and
  concrete providers instead of per-provider `String` copies. Gated on the named
  counters `resilient_file_cache_bytes` + provider-contents bytes. DISTINCT from
  the UNGATED active-first replay half (UP.2 C23); do NOT bundle.
- **C8 provider-native cancel** (D1 / D2 / D3 counters) — distinct from the
  ungated ingress half (UP.2 C8) and the host-reach half (U11.C20).
- **C21 / C22 / C24 / C25 (speculative)** — config-aware FS pruning, lazy
  code-actions / ranged tokens, encoding prefix-scan, bounded node_modules
  provider-sync graph.
- **D ancestor-walk + Vapor effects-bubbling** conflicts.
- **B Stage5 ResolvedElements deletion / StoreView MVCC / frontier BFS batching**
  (the deliverables live in the U-blocks — see §B; benches gated here).
- **L-A storm counters** — go/no-go: duplicate same-text `didChange` → 0
  parse/compile/provider delta; typing burst → ≤1 parse + ≤1 IDE compile for the
  final content. (See L-A in UP.G / its U-owner.)

#### UP.G — Gated-item index (the inventory; the OWNING U-block carries the deliverable)

Each row names the deliverable's home. UP.G is an index — read the owning
U-block's scope bullet for the contract.

- **F1** host-owned retained-close → **U11** (§B).
- **C16** delta `UpsertRequest` → **U11**.
- **C17** host artifact-bundle return → **U11** (UNBLOCKS UP.C4).
- **C18** fold `notify_upsert` → **U11**.
- **C19** per-canonical locks → **U11** (UNBLOCKS UP.C2b; MUST NOT use
  `CpuConcurrencySemaphore`).
- **C20** cooperative cancel (host half) → **U11** (UNBLOCKS UP.C8 host-reach).
- **B-typeinfo cache items 1–11** (expr cache, header probe, route frontier,
  generic lower-once, combined cm-view cache, StoreView token-shortcut,
  `Arc<str>` identity, memo/arena, hash-cons reverse index, typeinfo wire/Stage5,
  `ensure_indexed_ready_many`) → split by surface across **U3** (cache substrate),
  **U10** (result-DB / StoreView), **U8** (wire/Stage5 — HARD-GATE U8+), **U12**
  (exporter-side) (§B).
- **D-I3** virtual-file-pipeline string-surgery removal → **U14**.
- **D-custom_elements** customElement host reach → **U14**.
- **D-F4** output-breaking CSS hash cutover → **UP.D** (gov-flagged).
- **F2** resolved-type read-set fact-scoping → **U3** (§B; F2 correction recorded
  there).
- **L-A** re-parse / re-compile storm on active typing → **needs an owner
  near host_upsert + scheduler admission; overlaps Track C** (shares
  documents/mod.rs, lifecycle.rs, sync_coordinator.rs, sync_orchestration.rs,
  project_sync.rs). Lands AFTER the host_upsert / scheduler cutover, co-designed
  with it; measurement-held (UP.M counters). An in-crate LSP-only
  "skip byte-identical didChange" is INSUFFICIENT.
- **L-event-args** imported-component `InstanceType<typeof C>["$props"]`
  materialization under tsgo → **U14**.
- **C15** ranged provider updates (CodeTransform hard-order) → **UP.D**
  (gov-flagged).
- **L-C.2 (HOMELESS — needs an owner; STOP for user sign-off).** Ownership race
  after a real snapshot: a published root flagged `ownership_ready:true` whose
  resolver membership does not consistently claim an open, genuinely-owned `.vue`.
  Boundary: `verter_workspace/src/resolver.rs` + snapshot publish / `ownership_ready`
  (the no-owner branch lives at `sync_coordinator.rs:216`). No clean U-owner
  today (nearest relative is the B.1 program-model work, §1.1 / l.504).
  Co-design with the ownership/publish cutover; STOP for user sign-off before
  editing `verter_workspace`; coordinate with Track C (shares
  `sync_coordinator.rs`).
- **L-B (HOMELESS — needs an owner; STOP for user sign-off).** References /
  rename / code-actions + `TypeLocation` protocol. The go-to-DEFINITION /
  TYPE-DEFINITION non-gated half is LANDED; this is the remaining half.
  `TypeLocation` is byte-offset-only (`verter_type_runtime/src/protocol.rs:130`,
  `pub start: u32, pub end: u32`, no line:col / offset-kind discriminant); carry
  a resolved line:col `Range` (or an offset-kind discriminant), update TSGO +
  tsserver so refs/rename/code-actions never pack line/char into byte-offset
  fields, then remove the fake external `Range::default()` fallbacks in
  `tsgo/merge.rs`. Shared-runtime protocol change → STOP for user sign-off.
  **GOVERNANCE-FLAG:** confirm whether `TypeLocation` is inside the Typeinfo Wire
  Contract closed-enum surface — if so it is a schema bump.

---

### U0 — Finish Typeinfo Contract Gaps  **(U0-FINISH-A + U0-FINISH-B substrate landed — current state derives from §1.1)**

- **Source track:** semantic-graph (R-0a).
- **Parity docs:** parent `docs/arch/native-typeinfo-parity.md` (manifest ledger
  §10, git/CI landing protocol §§11–14) + child
  `docs/arch/native-typeinfo-parity-u2-reducers.md` (the U0 manifest/ledger
  foundation block, `U0.MANIFEST_SUBSTRATE`).
- **Scope:** Close the contract gaps A0a left.
  - The `AuditedResult<T, E>` carrier ALREADY EXISTS in
    **`crates/verter_audit/src/audited_result.rs`** (the type, its `ts-rs`
    `audit.generated.ts` export, and the `ok`/`err` constructors) — NOT
    `verter_protocol`: it is generic over `T`/`E`, which protobuf cannot express,
    and embeds the audit-substrate `RequestAuditRecord`, so it rides the ts-rs
    export rather than forcing a dependency inversion. `packages/typeinfo` imports
    the generated TS type; there is no hand-written mirror. The host-API wiring is
    LANDED on the live tree: `AuditedResult` is consumed across the five host audit
    surfaces (`host_compile_audit.rs`, `host_analyze_audit.rs`,
    `host_resolve_type_audit.rs`, `host_mcp_audit.rs`, `component_meta_host.rs`)
    plus the typeinfo entry points; any residual unwired entry-point edge is
    derived from the §1.1 authorities at `HEAD`, not asserted here.
  - **Unignore-manifest — EXTEND the A0a-landed manifest into the two-table
    ledger end-state, do NOT create a second one.** A0a already landed the manifest
    as a Rust test, not a doc:
    `crates/verter_session/tests/typeinfo_ignored_test_manifest.rs` +
    `tests/manifest_data/typeinfo_ignored_test_manifest_rows.rs` (362 TOTAL rows;
    `EXPECTED_TOTAL_IGNORED_COUNT` is DERIVED from the rows' `IgnoreStatus`, never
    a frozen constant), with the backing guards. U0 keeps that
    same in-repo `.rs` test as the single ledger (there is no separate
    `typeinfo_tests_unignore_plan.md` doc and no second manifest at a competing
    path) and EXTENDS it to the **two-table ledger** the parent architecture
    requires (`native-typeinfo-parity.md` §10):
    - **`IgnoredTestRow`** holds EXACTLY the 362 test-site rows — TOTAL count-guarded
      at 362; the `Ignored` rows are bijective with the source `#[ignore]`s and the
      ignored count is DERIVED (a `Lifted` row carries no live `#[ignore]`) — with the
      extended row schema
      (`substrate: TargetSubstrate`, `capability`, `organ`, `owning_u_block`,
      `block_id`, `semantic_queries`, `proof: ProofRequirement`, `status: IgnoreStatus`,
      `unblocker`).
    - **`AdditionalProofRow`** is a SEPARATE coverage-only table holding the
      **closed exactly-7** new/additional coverage fixtures (e.g. the JSX no-new-key
      submatrix) — one block per row, pinned by `additional_proof_row_table_holds_exactly_7_rows`.
      It is EXCLUDED from the ignored-count + bijection guards (it carries no
      `IgnoreStatus`) but still requires a `ProofRequirement` + a coverage-table entry;
      a fixture corresponding to an existing ignored row stays an `IgnoredTestRow`
      (never duplicated). It is NOT an open/expandable bucket — the table is closed at
      seven.
    - **`IgnoreStatus { Ignored, Lifted { block_id } }`** is the per-`IgnoredTestRow`
      lifecycle — BINARY, with no tracked in-flight `Verifying` / lease state (the
      "in-flight, not-yet-landed" state of a block is simply its unmerged branch;
      branch/CI is the only acceptance transaction — `native-typeinfo-parity.md`
      §10.1 / §11.1). `ProofRequirement` + the proof registry + the row-test
      wrapper make "every row resolves to an executable proof its own test consumes"
      mechanical; the U0 row-exact capability→mechanism→proof coverage table
      (`native-typeinfo-parity.md` §10.4) DEFINES completeness — the §10.4
      table itself is the unbuilt residual forward-declared on the `U13.PROJECTION` /
      `U15.FINAL_LIFT` block-contract rows (only §10.4.1's hand-authored partition is
      landed). On the live
      tree the proof substrate is LANDED per the locked `u0-oracle-harness-design.md`: the
      per-row `ProofRequirement` registry (`Ts7Oracle(OracleId)` / `StructuralGuard` /
      `NegativeGuard` / `OracleAndGuard` / `RowTestGuard`), the registry-bound
      driver-calling lifted row bodies (the locked design's superseding shape of the
      §10.3 row-test wrapper), and the shared TS7 oracle driver
      (`crates/verter_session/src/typeinfo/oracle_core/driver.rs::run_row`, with the
      oracle registry, snapshot bindings, and the lifted-row trace / fidelity guards).
      Rows lift on the manifest schedule; per-row lift state derives from §1.1, never
      from this prose.
    Do NOT migrate to a competing `target_phase` / `target_substrate` / `unblocked_by`
    schema at a different path — the landed `substrate` / `unblocker` columns are
    preserved and extended in place. See `native-typeinfo-parity.md` §10 for the full
    two-table ledger schema, the proof model, and the exact-362 count/bijection
    contract (cited, not duplicated here).
  - **Landing protocol — git/CI, no tracked cursor (U0 deliverable).** Typeinfo
    parity blocks land through git/CI, not a tracked orchestration cursor: branch per
    block → green CI (the full Rust+JS workspace gate + the coverage/proof/required/DAG
    guards) → three-reviewer LAND (branch-protection required approval) → squash-merge
    with a `Typeinfo-Block: <block-id>` trailer. Git is the transaction log, branch
    protection is the accept gate, and `git revert` is rollback; resume is derived from
    the manifest status + the merged trailers + the prereq DAG (no lease / WAL / revision
    CAS / receipt). There is NO `.cutover-state.typeinfo_parity` namespace, no two-namespace
    TOML schema, no namespaced xtask, and no crash-recovery machinery to build. The
    SEPARATE legacy top-level `.cutover-state` cutover cursor (the broader-plan execution
    cursor) is unrelated and is neither migrated nor reset. See `native-typeinfo-parity.md`
    §§11–14 for the git/CI landing protocol (cited, not duplicated).
  - Add the remaining Phase-0a static guards not covered by A0a (symbol-node
    invariants, origin-edge taxonomy, closure-bound, substitution-canonicalisation,
    request-uniformity, the schema-version closed-surface pins) — see the Guards
    list below for the exhaustive set. The manifest's own backing guards stay green
    under the reconciled schema (they are not net-new; they extend in place).
  - Adopt / verify the landed A0a wire naming (`GraphTypeNode` /
    `TypeInfoGraphRequest`) as canonical (§2.2).
- **Deps:** A0a (landed). No cross-track dep. (The §0.5 foundation cluster does NOT
  gate this block: per the §0.5.6 per-edge rule, `B.1` / `U0.RESOLVER_CORE` / `B.4`
  gate their NAMED consumers; `U0.MANIFEST_SUBSTRATE` consumes no foundation edge —
  which is why its substrate landed with zero foundation blocks built.)
- **Parallelism:** Runs beside U1 (B7d) only. U2 DEPENDS on U0 and starts only
  after U0 lands (U2 is the convergence gate).
- **Risk:** small-medium (the `AuditedResult` host-API wiring over the
  already-landed carrier + additive guards + a manifest reconciliation/rename of
  the A0a-landed test).
- **Required deletions:** none net-new for the contract surface, BUT the
  unignore-manifest is an EXTENSION of the A0a-landed manifest into the two-table
  ledger end-state, not a fresh add: do NOT create a duplicate manifest at a second
  path with a competing schema — the A0a-landed `typeinfo_ignored_test_manifest.rs`
  + its `manifest_data/` rows + its backing guards stay in place and grow the
  `IgnoredTestRow` schema + the separate `AdditionalProofRow` table + `IgnoreStatus`
  / `ProofRequirement` in place (preserving the landed `substrate` / `unblocker`
  columns; do NOT migrate to a competing `target_phase` / `target_substrate` /
  `unblocked_by` schema). The `AuditedResult` carrier already exists in
  `verter_audit`, and its host-API wiring is landed across the five host audit
  surfaces (see Scope above).
- **Guards (the exhaustive Phase-0a remaining set per §R-0a / §8.0 / §A.23 — no
  silent "subset + etc."):**
  - **Dependency / projection:** `dependency_direction_one_way` (the typeinfo
    layering is one-way, machine-checked); `path_projection_mode_cascade`
    (intermediate hops `Navigate`, terminal hop the caller's mode);
    `typeinfo_request_validates_mode_present`.
  - **SymbolNode invariants:** `symbol_node_preserves_type_value_namespace_spaces`
    (`SymbolSpace = Type | Value | Namespace`, `BothTypeValue` forbidden);
    `class_dual_space_emits_two_symbols`;
    `symbol_node_preserves_resolved_decl_slot_identity` (content-free slot
    identity, not `(canonical_id, name, symbol_space)`).
  - **Origin-edge taxonomy:** `origin_edge_taxonomy_locked` (the three
    `OriginEdgeKind` enums — `verter_session` 9, `verter_audit` 10, `verter_protocol`
    10 — are pairwise consistent, differing by exactly `SharedLoadReuse`).
  - **Schema-version (contract gate already landed; these pin the closed surface):**
    `every_typeinfo_request_carries_schema_version`;
    `unknown_schema_version_shape_uniform_across_plan`;
    `typeinfo_request_error_union_is_consistent_across_sections`. (The schema-version
    RUNTIME encoders/negotiation land in U11/U12.)
  - **Request-uniformity:** `every_typeinfo_request_carries_context_or_is_exempted_with_rationale`
    (seven entry-points carry `context + closure + displayPolicy`; `listSymbols` /
    `relate` are the two NAMED exemptions); `list_symbols_is_scalar`;
    `relate_has_no_closure_field`; `every_closure_variant_has_concrete_resource_bound`.
  - **Closure-bound / substitution-canonicalisation:** the closure-bound guard and
    the substitution-canonicalisation guard (`literal_value_key_is_independent_of_wire_string_table`,
    `cycle_id_propagates_canonicalization_conflicts`).
  - **Unignore-manifest (reconciled with the A0a-landed manifest — see below):**
    the manifest's existing backing guards (`every_ignored_typeinfo_test_has_a_manifest_row`,
    `every_manifest_row_corresponds_to_a_live_ignored_test`,
    `every_manifest_row_has_non_empty_unblocker`,
    `every_manifest_row_unblocker_matches_live_ignore_reason`,
    `every_manifest_row_lists_a_valid_substrate`,
    `total_ignored_typeinfo_test_count_matches_expected`,
    `manifest_length_matches_documented_total`,
    `per_file_ignored_test_counts_match_manifest`, plus the two reason-quality
    guards), kept green under the reconciled schema. Plus the EXTENDED two-table-ledger
    guards: `ignored_test_row_table_holds_exactly_362_rows` (binding-total + table
    disjointness), `additional_proof_row_table_holds_exactly_7_rows` (the closed
    coverage-only table), and the git/CI landing-protocol DAG guard
    `typeinfo_parity_block_dag_is_acyclic_and_consumed_keys_and_mechanisms_are_prereqs`
    (`native-typeinfo-parity.md` §§10.1, 11.5). `EXPECTED_TOTAL_IGNORED_COUNT` is the
    DERIVED live `count(IgnoredTestRow where status == Ignored)`, never a frozen
    hand-maintained constant.

  Plus the broader §8.0 registry "etc." entries scoped to Phase 0a
  (`r21_*` for typeinfo, `node_taxonomy_complete`, `diagnostics_only_on_typeinfo_graph_payload`,
  `proto_closed_enums_declared_not_raw_uint32`, `proto_no_duplicate_enum_declarations`,
  `wire_dtos_generated_only_from_proto`, `ts_rs_not_applied_to_wire_dtos`,
  `part_a_carries_no_phase_archaeology`). All must be discriminating (fail against
  the pre-change tree).

---

### U1 — Scheduler Dispatch Split (B7d)

- **Source track:** cache-runtime / scheduler.
- **Scope:** Bring the dispatch enum into parity with the landed DAG-layer
  `WorkKind`. The `TaskKind` enum split (`Load` + `Parse` + `Analysis` +
  `Artifact` + `CacheNode { cache_id, key_hash }`), the dropped `Copy/Eq/Hash`
  (it derives `Clone, PartialEq, Debug`), `TargetStage`, and the
  `StageExecutor::as_any` + `execute_cache_node` trait surface are ALL LANDED
  (`crates/verter_scheduler/src/stage.rs`, `executor.rs`). CPU
  dispatch lands as `try_submit_cpu` (`scheduler.rs`), and `CacheNode`
  routing is wired through the single router `dispatch_ready_job` →
  `dispatch_ready_job_to_executor` (`scheduler.rs`) → `execute_cache_node`,
  with discriminating tests covering route / permit-release / failure / panic /
  success outcomes. The only REMAINING work is the host-side cache-node
  materializer that overrides `execute_cache_node` (its default impl returns a
  "not implemented" `StageError`) — that override is NOT U1's, and it is NOT
  owned by any current block: the locked U9 design
  (`docs/arch/u9-session-bridge-design.md`) builds NO production materializer
  (U9 ships only a test-only `StageExecutor` proof) and U7 is locked DEFER, so
  the override exists only if the U9 core re-gate (`u9-session-bridge-design.md`
  §7) produces a committed production submitter; absent that, the
  `WorkNodeIdentity::CacheNode` arm is DELETED at that re-gate. There is no
  session-bridge block. The `task_kind_for_ready_job` adapter is
  ALREADY GONE; the surviving `unreachable!()` in `admit_work` (`scheduler.rs`)
  is the correct end-state (`Parse` is intrinsic to the source stage and
  `CacheNode` is admitted into the DAG directly by the cache layer, never
  through file-stage admission), not a placeholder to replace.
- **Deps:** B7a/b/c (landed).
- **Parallelism:** Can run parallel with U0 and U2 (scheduler-only surface).
- **Risk:** medium — touches the per-task dispatch chokepoint; must not create a
  second dispatch path.
- **Required deletions:** none — the `task_kind_for_ready_job` adapter this block
  was scoped to delete is ALREADY GONE (0 src hits). `SchedulerJobKind`
  (component-meta batch fan-out) is **RETAINED** and must not alias `TaskKind`.
- **Guards:** keep `dag_arch_guards` (13/13) green; a discriminating test that
  the CacheNode dispatch arm actually routes (not `unreachable!()`); a guard that
  there is exactly one dispatch path (no second `task_kind_for_ready_job`-style
  adapter re-introduced).

---

### U2 — Semantic Key + B4 Cache-Node Convergence  **(CONVERGENCE GATE — highest correctness risk)**

- **Source track:** MERGED (semantic-graph R-1 + cache-runtime B4-completion).
  See the dedicated co-sequencing section (§5).
- **Parity docs:** parent `docs/arch/native-typeinfo-parity.md` (query-key
  taxonomy §2, typed `SemanticQueryValue` value-domain §3) + child
  `docs/arch/native-typeinfo-parity-u2-reducers.md` (the U2 reducer / relation /
  utility / indexed / mapped / template / class / enum / module / JSX blocks).
- **Sub-block decomposition (per the child's block graph).** U2 lands as the
  pre-reducer SUBSTRATE sub-block **`U2.BINDER_IDENTITY_FACTS`** (the §0.5.1
  family-A producer — contract in the next bullet), the
  foundation block **`U2.QUERY_VALUE_DOMAIN`** (the typed `SemanticQueryValue`
  value-domain layer + the five landed spine keys of the parent's seven-variant
  spine — the two augmentation spine keys `ResolveMergedDeclaration` /
  `ResolveDeclarationAugmentation` are forward-planned (below) — + the three
  U2-landed added keys + the
  finalized slot-identity SHAPE for the U6-landed `FlowReturn` / `ResolveCall` + the
  `ProjectionDemand` / `EvalPolicy` demand lattice with the five mode names as
  PRESETS + the per-block `SemanticQueryKeySpec` table) followed by the keystone
  reducer **`U2.RELATION_INFER`** (the `Relate` engine + coinductive-SCC discharge +
  relation-owned `InferBind`; the `CheckerTransaction` + `InferenceSession` +
  `InferenceInfo` substrate the `InferenceContextKey` fingerprints lands HERE, not in
  the value-domain block) — **DESIGN-LOCKED (effort weight EXTREME, §3.3): its
  rescope gate has RUN and the output is `docs/arch/u2-relation-infer-design.md`.**
  The assignability + inference-session + measured-variance algorithm (the whole TS
  checker core), the `isDeeplyNestedType`-style termination proof (non-convergence
  never WARM-admits), the PERSISTENT-`Relate` decision with the A.2 proof
  obligations, the RI-1..RI-10 implementation mini-DAG, and the per-family
  `tsgo`-oracle baseline are owned by that locked design, not asserted in this
  index; parent §§4.0–4.2 seeded it (marker-probe fixed-point + coinductive SCC +
  the session substrate). The "route-through-`Relate`" phrasing elsewhere is the
  one-engine WIRING constraint, NOT this phase's algorithm. — and the parallel
  reducer sub-blocks `U2.INDEXED_ACCESS`,
  `U2.MAPPED_TEMPLATE`, `U2.UTILITIES`, `U2.CLASS_SURFACES`, `U2.ENUMS`,
  `U2.MODULE_AUGMENTATION`, `U2.JSX_FOUNDATIONS`. The parent U2 token is the
  aggregate over every sub-block. The child doc owns the per-sub-block contracts;
  this index lists the names only.
- **`U2.BINDER_IDENTITY_FACTS` (pre-reducer SUBSTRATE sub-block — the explicit
  production owner of §0.5.1 family A; resolves the binder-ownership gap):** build
  the layer-1 `BinderIdentityFacts` family-A artifact — lexical-scope identity
  (stable structural scope ids), the env-free `DeclarationSlotSeed` facts, and the
  per-file declaration-merge / augmentation CONTRIBUTOR-ORDER provenance — FROM
  `IndexedReady` in `verter_semantic::analysis`, demand-produced (never an eager
  whole-program binder pass), keyed `(canonical, parse_stable_hash, parse_env_hash)`
  per §0.5.1. It is a `U2`-tier PREREQUISITE substrate the reducer sub-blocks
  consume BEFORE they run — not a reducer output, not `N0`-owned. Family B (the
  corpus-scoped completeness store) stays `B.4`-owned
  (`ambient_global_and_lib_corpus_have_completeness_facts` gates at `B.4`).
  **Acceptance bar: the §0.5.1 binder-contract guards gated on this substrate** —
  `binder_identity_facts_are_pre_u2_and_not_n0_owned`,
  `declaration_slots_are_stable_symbol_space_scoped_facts` (with the U2
  slot-identity finalization), and
  `negative_name_lookup_requires_recorded_completeness_or_returnonly` (shared with
  `N0`) — composing with the U2-gate guards
  (`u2_queries_do_not_read_n0_navigation_indexes`,
  `binder_scope_id_enters_context_sensitive_query_identity`) and the
  `U2.MODULE_AUGMENTATION`-gate guard
  (`merge_order_and_augmentation_contributor_order_are_fact_validated`), each
  registered at its owning gate per §0.5.7.
- **Scope (ONE clean cutover — no migrate-twice):**
  0. Land the typed **`SemanticQueryValue`** value-domain layer over the one shared
     dispatch/admission substrate (`{ TypeNode, ProgramAnalysis, DeclarationAnalysis,
     OverloadSet, Relation }`, extended in U6 with `{ FlowReturn, ResolvedCall }`;
     the reserved NON-LIVE `DiagnosticAnalysis(CheckResult)` arm + `Check*` query
     names stay reserved-not-live for the native-checker sibling), every variant
     mapped to exactly ONE value domain (parent §3); and the `ProjectionDemand` /
     `EvalPolicy` **demand lattice** as the PRIMARY demand + cache-identity dimension,
     with the five mode names (`Identity` / `Navigate` / `Shallow` / `Expanded` /
     `Skeleton`) as public presets over it (`Skeleton` = `generic_open =
     TypeParamShells` + carrier-stop, not a special mode), with cache satisfaction
     decided by the LANDED materialized-point model: a warm hit requires a RECORDED
     materialised `(path, point)` the compute actually produced to dominate the
     request via `cached_satisfies` — the lattice is the demand/identity ALGEBRA
     inside that check, never mode-enum order and never a nominal-demand authority
     (parent §2.10). The published-boundary satisfaction EXACTNESS gating lands in
     U10 ON TOP of the landed model; the lattice DEFINITION lands here.
  1. Finalize the **`SemanticQueryKey` identity SHAPE once** (the slot-identity
     model for every variant): existing `Instantiate { base }` /
     `ResolveMacroPayload { owner }` moved from the (now-deleted) intermediate
     `DeclKey` to `ResolvedDeclSlotIdentity` (slot identity, LANDED), AND land the
     **five landed spine variants** of the parent's seven-variant spine in that
     identity shape (`ResolveAmbientNamespace`, `ResolveOverloadSet`,
     `ResolveEnum`, `FlowNarrowingAt`, `ContextualTypeAt`). The remaining two spine
     variants — `ResolveMergedDeclaration` and **`ResolveDeclarationAugmentation`** —
     are **forward-planned**, owned by the later `U2.MODULE_AUGMENTATION` reducer
     block (parent `native-typeinfo-parity.md` §2.1: "five landed, two
     forward-planned"); they are NOT in the live enum or the generated spec table.
     The forward-planned seventh variant is the **generalized** augmentation key:
     the former `ResolveModuleAugmentation` slot is broadened to
     `ResolveDeclarationAugmentation { target: Module | Global, context:
     DeclarationAnalysisContext }` so module AND global
     declaration-environment-mutation facts share ONE concrete `SemanticQueryKey`
     identity (resolving to `SemanticQueryValue::DeclarationAnalysis`, never a
     `GraphTypeNode` arm). This is an existing-slot generalization, NOT a sixth U2
     variant — the END-STATE slot count stays **seven** and the added-key count
     stays exactly **five** (below). Same-name declaration merge and cross-file
     ambient augmentation already work TODAY via the live `MergedDecl` peer-merge
     reducer over `SemanticNodeData::MergedDecl` (CLAUDE.md "Declaration Merging /
     Augmentation (CRITICAL)") — so the value-bearing merged / ambient type-value
     surfaces exist now. The forward-planned `ResolveMergedDeclaration` /
     `ResolveDeclarationAugmentation` query keys AND the formal augmentation parity
     rows — which resolve to the in-process `SemanticQueryValue::DeclarationAnalysis`
     value domain — land together later in `U2.MODULE_AUGMENTATION`
     (`native-typeinfo-parity-u2-reducers.md`, the `U2.MODULE_AUGMENTATION` block); the rows do NOT lift without
     that new key.

     > **DEFERRED — codex STEP-0 ruling (SPINEMODAUG, 2026-06-14):** This
     > forward-plan was SUPERSEDED by what `U2.MODULE_AUGMENTATION` actually landed
     > (branch `mom/u2-module-aug`). The block delivered a COMPLETE value-space
     > `typeof` / module-augmentation reducer through the EXISTING shared dispatch
     > (`ResolveDecl` / `IndexedAccess` / `TypeOf` / `ProjectPath` via
     > `ProjectSemanticDispatch::execute`) with **NO new query keys**: 4 parity rows
     > LIFTED (2, 4, 6, 8), each RE-HOMED to its measured-trace terminal block
     > (`U2.INDEXED_ACCESS` rows 2/6/8, `U2.QUERY_VALUE_DOMAIN` row 4), and 4
     > honest-DEFERRED (1, 3, 5, 7) with measured oracle `Reject(...)` reasons;
     > manifest total UNCHANGED at 362. The claim immediately above — "**the rows do
     > NOT lift without that new key**" — is therefore DISPROVED: the rows lift
     > through the existing dispatch. The forward-planned `ResolveMergedDeclaration`
     > / `ResolveDeclarationAugmentation` query keys AND the
     > `SemanticQueryValue::DeclarationAnalysis` value-domain producer are
     > **DEFERRED, NOT landed** — codex STEP-0 ruled them STALE (making them live
     > would require editing CLAUDE.md's CRITICAL Declaration Merging / Declaration
     > Augmentation rules, which is out of scope); the live `MergedDecl` peer-merge
     > architecture already serves the merged / augmented type-value surfaces today.
     > Source: `/tmp/mom/SPINEMODAUG/codex-step0-RULING.md` Q2 (Query Keys).

     Every variant routes through
     `ProjectSemanticDispatch::execute` (the one-engine rule). This finalizes the
     identity SHAPE — later ADDITIVE variants land in that same slot-identity shape
     with NO cache re-key; adding a later variant is additive, not a second
     migration.
     - **Added query keys (exactly five beyond the seven; parent §2.3).** Register
       the five new `SemanticQueryKey` variants in the final slot-identity shape,
       each dispatched through `ProjectSemanticDispatch::execute`:
       - **`ResolveClassSurface`** (lands here, U2) — instance/static heritage with
         generic substitution, member-demand aware (`{ decl_slot, type_args, side,
         demand, context: ClassSurfaceContext }`). Generic `ResolveClass` folds in.
       - **`ApparentType`** (lands here, U2) — primitive/array/constrained-generic
         apparent member lookup via lib facts (`{ base, demand, context:
         ApparentTypeContext }`). Named `ApparentType`, not `GetApparentType`.
       - **`TemplateLiteralReduce`** (lands here, U2) — template-literal distribution,
         intrinsics, and `infer` splitting (`{ pattern, args, context:
         TemplateLiteralReduceContext }`).
       - **`FlowReturn`** (lands in **U6**, additive in this same shape, no re-key) —
         the demand-sliced return/body flow query (`{ function_slot,
         normalized_type_args, context, demand, input }`).
       - **`ResolveCall`** (lands in **U6** with the U2 `ResolveOverloadSet` key as a
         dependency) — reusable call resolution with its own cache identity
         (`{ callee, call_kind, receiver_this, args, explicit_type_args,
         contextual_result, policy, context }`); `CallResolve` is PROMOTED to
         first-class, not folded.
       Each added key carries an explicit per-key `*Context` (split env hashes only,
       R21; no content/version hash or `fact_dep_signature`, R6) and a
       no-cross-context-warm-hit guard. The existing `Relate` key is upgraded in the
       same shape (full relation identity, parent §2.7) — an existing-key upgrade,
       not a sixth added key.
  2. Add the matching producers in `verter_semantic::analysis` (namespace / merge /
     class analysis) + `verter_session::semantic_query`: the type-value
     `SemanticNodeData::{MergedDeclaration, AmbientNamespace, Class, Enum}` producers.
     Same-name declaration merge and cross-file ambient augmentation are LANDED via
     the live `MergedDecl` peer-merge reducer over `SemanticNodeData::MergedDecl` (the
     §9.5 cross-file-merge work), NOT a dedicated query key. The forward-planned
     module + global augmentation facts that `ResolveDeclarationAugmentation` resolves
     to — the in-process `SemanticQueryValue::DeclarationAnalysis` value domain (whose
     ON-THE-WIRE home stays `GraphTypeNode` kinds 23/25); declaration/environment-mutation
     facts are NOT smuggled into `GraphTypeNode` as TYPE values (parent §1.3, §3) — land
     with that forward-planned key in `U2.MODULE_AUGMENTATION`, where `ResolveDeclarationAugmentation`
     rides `FileArtifactStore::augmentation_index` (landed) with its
     `AugmentationTargetKey` derived from `DeclarationAnalysisContext` at execution time.
  3. Enumerate the remaining B4 caches onto `ArtifactNode` / `QueryNode` against
     that same final key model: `FileArtifactStore`, `ResolvedImportFacts`,
     typed-IR resolve, `MemberSemanticFactStore`, `MemberDisplayFactStore`,
     `ModuleAugmentationIndex`, `RouteDb` (×3),
     `TypeResolutionContextDb`, `DependencyCacheDb`,
     `SemanticGraphStore`, `ComponentMetaResultDb`, `MaterializeStructureDb`,
     `RefCycleResultDb`, `ShapeCacheDb`, `AnalysisReadyDb`,
     `OwnerImportSurfaceDb`, `ImportedRootDb`, `AppConfigNoOverrideProofDb`,
     `ResolvedTypeCacheDb`. Add the supporting key/value types
     (`FileArtifactKey`, `ResolvedImportFactsKey`, `CompileOutputKey`,
     `CompileOutputSlotKey`, `AnalysisSlotKey`, `AnalysisCandidate`,
     `ResolvedDeclSlotIdentity`). The framework-adapter substrate landed two
     already-merged dimensions ahead of this block that the finalized key model
     INTEGRATES (additively — they are part of the shape U2 fixes once, not a
     second key migration): the per-file `FileArtifactKey.file_language_id`
     dimension (the `FileLanguage` row, an R21-scoped per-file artifact-identity
     column — nothing capability-shaped enters the global `parse_env_hash`) and
     the `IndexedReady.framework_parse` slot (the optional `FrameworkParseArtifact`
     the post-parse artifact retains for `.vue`/framework files). `file_language_id`
     is today a STATIC extension-derived `FileLanguage` classification (one inert
     value per extension, identical to the host-resolved row while no gated registry
     rows exist); threading the live HOST-resolved row / an explicit-kind override
     into key construction — so a capability flip or an editor-honored non-`.vue` URI
     misses exactly the affected files' artifact slots — is a `TODO(follow-up)`
     carried here (the first gated row's producer wiring lands it in the same change).
- **Real-corpus manifestation note (`U2RelationInfer` row
  `relation_semantics.rs::relation_distributive_conditional_over_union_emits_branch_union`
  — the row NAME is the durable key; it lives in the auto-generated manifest
  `crates/verter_session/tests/manifest_data/typeinfo_ignored_test_manifest_rows.rs`
  (`:287` at the time of writing — a convenience anchor only; the file is
  generated and the line drifts on regeneration); ignored test
  `crates/verter_session/src/typeinfo/typeinfo_tests/relation_semantics.rs:417`):**
  on the real nuxt-ui corpus, Accordion's `update:modelValue` emit payload
  collapses to `string[] | undefined`, losing the `string` branch of reka-ui's
  single/multiple generic conditional — a component-meta manifestation of this
  row's False-branch-only selection (row unblocker, verbatim: "typeinfo
  currently selects only the False branch (`"no"`) for
  `IsStringDistributive<string | number>` instead of distributing the union
  across the conditional and emitting `"yes" | "no"`; keep as the future
  bare-type-parameter distribution contract"): one type-resolution engine, so
  the same shared-oracle bug is observed through the emit surface. Two
  consequences bind the row lift: (i) the lift's acceptance must include the
  emit/model payload surface on the real nuxt-ui corpus run (the feature-gated
  `external-corpus` / `bench:meta:ui`-class real-component run), not only the
  hermetic relation fixture; and (ii) the falsifier — the observed
  `string[]`-only output matches the row's False-branch-only signature for a
  `T extends 'single' ? string : string[]` arm order; that arm-order spelling
  is package knowledge (reka-ui is NOT vendored in-tree, so the spelling is
  unverifiable here), and ANY fix-time repro showing the payload source is NOT
  a bare-type-parameter conditional with this arm order — reversed arms with
  TRUE-branch selection, or a non-conditional spelling such as an
  indexed-access map — means the mechanism is NOT covered by this row and the
  mapping must be reopened. No new tracked item —
  the manifest row is the tracking (see also §4 XP, which tracks the
  genuinely-new nuxt-ui parity gaps and excludes this one).
- **Deps:** U0; B2 + B3 (landed); pre-existing `SemanticGraphStore` /
  `ProjectSemanticDispatch` / `execute_cooperative` (landed).
- **Parallelism:** U1 (B7d) may run beside it. NOTHING downstream of the gate
  (U3, U8+) starts until U2 lands.
- **Risk:** **very large / highest correctness risk** — touches the one shared
  resolver. Declaration merging + ambient + augmentation are the hardest
  TS-fidelity cases; cross-file merge completeness (§9.5 five properties) is a
  known hard sub-item.
- **Required deletions:** none of substance at this block — but it FORBADE adding
  the spine variants on the intermediate `DeclKey` shape and re-keying later (that
  double-migration is the anti-pattern §B exists to prevent). Use `ShapeCacheDb`,
  never the retired split shape caches. The `DeclKey` whole-hash fix landed and the
  `DeclKey` struct itself was subsequently deleted when the base/owner moved to
  `ResolvedDeclSlotIdentity` (§2.2).
- **Guards:** an H3 runtime guard
  (`cache_key_runtime_guards::semantic_query_keys_contain_no_content_hash_or_fact_signature`
  / equivalent) — query-identity keys carry no content/version hash or
  `fact_dep_signature`; a guard that every `SemanticQueryKey` variant dispatches
  through `ProjectSemanticDispatch::execute`; `shape_cache_db_replaces_split_caches`
  stays green; a cross-file merged-interfaces 5-property test (§9.5); per-variant
  producer discriminators. **Plus the §0.5.1 binder-contract guards owned at this
  gate** (named there, registered here per §0.5.7):
  `binder_identity_facts_are_pre_u2_and_not_n0_owned` +
  `u2_queries_do_not_read_n0_navigation_indexes` (the dependency edge is
  `BinderIdentityFacts → U2`, never `N0 → U2`);
  `declaration_slots_are_stable_symbol_space_scoped_facts` (family A stores env-free
  `DeclarationSlotSeed` facts scoped to their declaration-space; the env-bearing
  `ResolvedDeclSlotIdentity` is U2-derived at query-key construction, NOT stored in
  family A, finalized with the slot-identity SHAPE above);
  `merge_order_and_augmentation_contributor_order_are_fact_validated` (composes with
  the existing `declaration_merge_records_binder_overload_augmentation_order_as_facts`
  — order is `ReadSetSignature`-validated over recorded provenance, never re-derived
  from raw `IndexedReady`); `binder_scope_id_enters_context_sensitive_query_identity`
  (a scope-dependent query carries `binder_scope_id` in its `SemanticQueryKey` identity
  — a semantic discriminator, NOT a content/version hash, R6-consistent); and the
  LANDED overlay-aware augmentation guards kept green:
  `session_overlay_augmenter_isolated_from_base_index`,
  `effective_export_set_session_view_stitches_overlay_augmenter`,
  `no_effective_export_set_base_only_session_assert` (the augmentation index is
  overlay-aware via `AugmentationTargetKey.population` — §0.5.1; the retired
  fail-closed deliverable `session_overlay_augmentation_fails_closed_until_implemented`
  must NOT be reintroduced).

---

### U3 — Cache / Fact Model (`U3.CACHE_FACT_MODEL`)

- **Source track:** semantic-graph (cache/fact model end-state, with cache-runtime
  coupling).
- **Parity docs:** child `docs/arch/native-typeinfo-parity-cache-export-session.md`
  (`U3.CACHE_FACT_MODEL` — owns U3 / U8 / U10 / U11 / U12 / U13: facts, wire,
  exporter, DB, session, projections), under parent
  `docs/arch/native-typeinfo-parity.md` (PART 1 §6, the Cache Architecture rules,
  the Canonical Dependency Cache Rule). INDEX entry — the child doc owns the full
  block contract; it is cited, not restated.
- **Scope (the cache / fact-model end-state):** land the typed-admission +
  per-budget non-admission + route-fact-validation rails the result DB (U10), the
  exporter (U12), and the session surfaces (U11) all admit through. Concretely:
  the typed three-layer `BudgetExceeded` non-admission for every hot reducer budget
  (`RelationBudget` / `KeyspaceBudget` / `CallResolutionBudget` / `FlowSliceBudget` /
  apparent-type member-demand), each routing through `ReturnOnly`; the multi-candidate
  `FamilySlots` admission substrate with **per-family adaptive `candidate_cap()` +
  invalid-first/LRU-by-valid-hit eviction + a global memory ceiling** (replacing the
  uniform `FAMILY_SLOT_CANDIDATE_CAP = 4` FIFO); the five-dimension env-hash split
  (R21) with `lib_env_hash` entering only the layers whose value depends on lib data —
  AND, on the per-file artifact-identity side, the already-merged
  `FileArtifactKey.file_language_id` column (the `FileLanguage` row) is carried as an
  R21-scoped per-file artifact dimension: it stays on the per-file key, never folded into
  the global `parse_env_hash` (capability-shaped state is per-file, not project-wide);
  content-free query-identity keys (R6 — declaration-keyed families carry the env-bearing
  content-free `ResolvedDeclSlotIdentity` slot) with `ReadSetSignature.facts` as the
  sole validity rail; and route-fact validation / invalidation (selected-leaf-edit
  propagation, barrel-route redirect + prior-leaf drop, package-source-change
  invalidation, single-materialization collapse, VFS-authoritative). The
  off-store-cache detection this block enforces (`no_off_store_host_caches` / R6 — every
  host cache lives on `ProjectTypeStore`) is EXTENDED to SEE the merged framework-adapter
  caches the merge parked on registry rows: `FrameworkRegistration.surface_store` (the
  `FrameworkSurfaceStore`) and `FrameworkScriptCaches` both currently live off
  `ProjectTypeStore`, so the nested-off-store detector must reach them (their
  consolidation onto `ProjectTypeStore` / `TypeInfoGraphResultDb` + true singleflight is
  owned at U10 — this block makes the off-store residence visible to the rail, U10 removes
  it). The B8
  bespoke-invalidation deletion is a COMPONENT of this fact model: DELETE
  `component_meta_caches.rs` per-DB `clear_*` reverse-dependent eviction authority
  (replaced by validated lazy revalidation per B4 / skill R3), and remove
  `DeclIdentity` as a key field on any `SemanticQueryKey::*` variant — these are not
  a competing block. Lifts the three `cross_file.rs` `CrossFileResolution`
  route-demand rows.
- **Deps:** **`U8.WIRE_SURFACE_CLOSURE`** (the typed admission this block enforces
  produces values whose wire shape U8 closes — it lands AFTER the wire closure) +
  U2 (parent, the reducers it bounds/admits) + U6 (parent, the flow solver it
  bounds/admits). NOT parallel-to-U8.
- **Parallelism:** Semantic-graph lane, after U8 (the cache/fact rail the result DB
  + exporter + session ride on); runs beside the cache-runtime lane (U4/U5/U7/U9).
- **Risk:** medium-large — correctness-sensitive (typed-admission + invalidation-
  authority change).
- **Required deletions:** the uniform `FAMILY_SLOT_CANDIDATE_CAP = 4` constant +
  FIFO candidate eviction (→ per-family `candidate_cap()` + invalid-first/LRU-by-
  valid-hit eviction + global ceiling); any boolean/sentinel/side-channel cache
  admission (→ the typed `ComputeAdmission` enum); any bundled `project_config_hash`
  (→ the R21 five-dimension split); any content/version hash or `fact_dep_signature`
  on a query-identity KEY (→ version rooting on the value, R6); `component_meta_caches.rs`
  per-DB `clear_*` reverse-dependent eviction; `DeclIdentity` from `SemanticQueryKey::*`;
  any route-fact revalidation that is file-hash-only.
- **Guards (per the child's named set):** `relation_budget_exceeded_admits_nothing`,
  `keyspace_budget_exceeded_admits_nothing`, `call_resolution_budget_exceeded_admits_nothing`,
  `apparent_type_budget_exceeded_admits_nothing`; `program_analysis_fact_domain_validates_flow_slice`
  (the fourth closed `FactDomain::ProgramAnalysis` dispatch home); `cache_candidate_cap_is_per_family_not_uniform`;
  `family_eviction_prefers_invalid_then_lru_valid_hit`; `cache_keys_cover_ts_jsx_moduleresolution_decorator_lib_dimensions`;
  `instantiation_depth_policy_in_identity_and_facts`; `persistent_caches_never_admit_overlay_only_results`;
  `architecture_minimizes_fallback_entry_not_fallback_cost`. A guard that no
  `SemanticQueryKey` variant contains `DeclIdentity`; a guard that reverse-dependency
  graphs are not invalidation authority (validated lazy revalidation only); regression
  tests that same-canonical edits are caught by strict self-root validation and
  cross-file edits invalidate lazily through recorded facts.
- **Perf-backlog cross-ref (UP / §B):** this block OWNS the cache-side perf
  deliverables indexed in UP.G.
  - **F2 — resolved-type read-set fact-scoping.** Fact-scope the resolved-type
    cache read-set onto the `ReadSetSignature.facts` + `self_root_canonicals`
    validity rail. Targets `host_resolve/{external_type_resolution,frontier_engine,
    frontier_helpers}.rs` + `project_type_store.rs` + the `resolver_core/*resolver_context.rs`
    read-set plumbing. **SPEC-DRIFT CORRECTION:** the perf handoff's claim that
    `ResolvedTypeCacheDb` is "retired" is FALSE on this tree — the symbol EXISTS
    (`crates/verter_session/src/host_construction.rs:961`,
    `resolved_type_cache() -> &project_type_store::ResolvedTypeCacheDb`). F2 targets
    the `ReadSetSignature.facts` rail + verify-against-live-HEAD (path-precise R21
    read-set, `ReturnOnly` never publishes torn results, cold/warm equivalence),
    NOT a DB removal; re-verify the DB-vs-fact-rail split against live HEAD before
    implementing. Pair with the `r6_*` / migrated-query-identity-key guard cluster.
  - **B-typeinfo cache-substrate sub-items** (expr cache, header probe, route
    frontier, generic lower-once, combined component-meta view cache, `Arc<str>`
    identity, memo/arena, hash-cons reverse index). For EACH: **verify
    already-landed before re-implementing** (several overlap substrate already on
    the tree — `SemanticGraphStore` / hash-cons memos / the demand-scope
    declaration-body lowering); route through the shared resolver (single-engine
    rule — `no_new_resolve_type_engine_path*`); **NO new off-`ProjectTypeStore`
    cache DB** (R6 / `no_off_store_host_caches` — a new cache DB trips the R6
    meta-guard). Preserve `ReadSetSignature.facts` validity (R21) and the typeinfo
    wire-contract guards. No second resolution engine. (StoreView token-shortcut +
    `ensure_indexed_ready_many` → U10; typeinfo wire/Stage5 → U8 HARD-GATE;
    exporter-side projection → U12.)

---

### U4 — Persistent Pure Artifact Cache (B9)

- **Source track:** cache-runtime.
- **Scope:** Sealed `PersistentArtifactNode` trait (query nodes CANNOT persist) +
  `BaseWriteToken` / `BaseToken` capability witness +
  `PersistentCache` / `ManifestHeader` / `PERSISTENT_SCHEMA_VERSION` + CAS +
  manifest under `cache_runtime/persistent/`. Only pure content-addressed
  artifacts persist (e.g. `CompileOutputNode_PureContent`); semantic / session
  nodes stay memory-only.
- **Deps:** U2 (needs the node enumeration to know what is pure).
- **Parallelism:** Cache-runtime lane; beside U5/U6 and the semantic-graph lane.
- **Risk:** large — new on-disk format + sealed-capability type-gating.
- **Required deletions:** none (additive persistence layer); but query nodes must
  be type-level barred from `BaseWriteToken`.
- **Guards:** `cache_overlay_snapshot_cannot_construct_base_write_token`; a guard
  that only `PersistentArtifactNode` impls reach the persistent path; pure
  artifacts persist only with complete semantic/compiler/env/profile/plugin/
  source-map-policy keys.

---

### U5 — Memory Policy + Cache Audit (B10)

- **Source track:** cache-runtime.
- **Scope:** `MemoryPolicy`, `ActiveSnapshotPinRegistry` / `SnapshotId` /
  `CacheEntryId`, `EvictionRingBuffer`, `AdmissionDecision` / `ColdMissReason` /
  `StaleReason`, `CacheNodeMetrics` (single weight via
  `ArtifactNode::weight_bytes` / `QueryNode::weight_bytes` — no separate
  `WeightedAccountable`). Add `StructuredAuditEvent::CacheNode*` variants in
  `verter_audit` and emit from component-meta cache paths.
- **Deps:** B2 only (per inter-block DAG); best landed after U2 (observability is
  most useful once the cache nodes are enumerated).
- **Parallelism:** B2-gated; can run parallel with the U4/U6 cache-runtime work
  and the semantic-graph lane (U8/U3+).
- **Risk:** medium — audit additions are purely additive (closed-enum discipline
  on `StructuredAuditEvent`).
- **Required deletions:** none (`NonAdmissionReason` leaf already exists from the
  B3 split). Do NOT add a separate `WeightedAccountable` — weight is the single
  node method.
- **Guards:** closed-enum discipline guard on `StructuredAuditEvent`; a guard that
  cache hits do not allocate audit payloads without an active accumulator; metrics
  discriminators for cold-miss / stale / admission paths.

---

### U6 — Native Flow Return (B11)

- **Source track:** cache-runtime (with a semantic-key touch).
- **Parity docs:** child `docs/arch/native-flow-return.md` (the U6 flow chapter —
  the demand-sliced `ReturnPathPeeker` two-frontier model + the flow IR), under
  parent `docs/arch/native-typeinfo-parity.md`. The flow-return coverage detail
  lives in this in-repo doc and the parent's coverage sections (the landed
  hand-authored §10.4.1 partition; the §10.4 generated table is the U13/U15-gated
  residual), never a scratch/temp artifact.
- **Scope:** The demand-sliced flow architecture is documented at
  `docs/arch/native-flow-return.md` (parent §5 owns the cross-cutting contract). The
  keystone sub-block `U6.FLOW_RETURN_SUBSTRATE` lands the four artifacts: the
  arena-free `FunctionBodySkeleton` (no type lowering, under `IndexedReady`), the
  sparse per-function **`FunctionFlowGraph`** built ONCE from the skeleton with typed
  edge classes (value-def / path-write / eval-effect / narrowing-predicate /
  control-region / closure-escape / loop-summary / try/finally-override; no
  build-time type lowering), the **`ReturnPathPeeker`** graph demand PLANNER
  (the slice = graph reachability from `(return_site | expression_site,
  projection_path, EvalPolicy)`, the two-frontier rule as edge classes — not a
  procedural CFG walk), and the slice substrate `FlowSliceHashNode` /
  `FlowSliceLoweredBodyNode` / `FlowSliceIR` as B4-style cache-runtime nodes
  (`ArtifactNode` impls under U4, NOT bespoke `FileArtifactStore` side maps). Add the
  additive `SemanticQueryKey::FlowReturn` AND `SemanticQueryKey::ResolveCall` query
  nodes — each as enum variant + `SemanticQueryKeySpec` row + dispatch behavior
  together, in the U2-finalized slot-identity shape with NO cache re-key, routed
  through `ProjectSemanticDispatch::execute` (parent §2.4 / §2.5). Flow node / fact
  identity is rooted by the per-function **`flow_body_stable_hash`** — body-SENSITIVE,
  cosmetic-INSENSITIVE — NOT the decl-skeleton `parse_stable_hash` (so
  `return { b: 1 }` and `return { b: 2 }` key distinct slices). The `FlowSlice` fact
  lives in the fourth closed **`FactDomain::ProgramAnalysis`** domain, validated
  fail-closed by `StoreView::validates_program_analysis_domain`. Slice-hash
  production is SPLIT from slice lowering (`FlowSliceLoweredBodyNode::compute` must
  NOT call the slice-hash producer). **The narrowing surface re-casts as ONE hard
  lattice-substrate block + SEVEN cheap per-mechanism additions** (front-load the
  hard one): the **`FlowFrame` branch-fact lattice substrate + the JOIN ALGEBRA for
  conflicting predicates** is the single hard block (its narrowing join / meet for
  conflicting predicates is the design difficulty); once that lattice exists, the
  seven per-mechanism sub-blocks (`U6.NARROW_TYPEOF` / `_EQUALITY` / `_TRUTHINESS` /
  `_IN` / `_INSTANCEOF` / `_DISCRIMINATED` / `_SUBSTITUTION`, plus the
  invalidation/preserve frame they share) are **cheap additions** that each fill in
  the facts their mechanism carries onto the SAME lattice — no second flow structure.
  The remaining sub-blocks (`U6.PREDICATE_ASSERTION`, `U6.CALL_RESOLVE`,
  `U6.CONTEXTUAL_CALLBACK`, `U6.VALUE_INFERENCE`, `U6.ASYNC_GENERATOR`,
  `U6.CROSS_FILE`, `U6.LOOP_CLOSURE`) add their edge classes to the same graph. The
  child doc + parent §5 / §10.4.1 own the per-sub-block contracts and the row
  partition; this index does not restate them. The reserved `ExecutableRegionId` / `ExecutableRegionKind::Function` region
  abstraction + the `ProgramAnalysisContributor` injection seam are NON-LIVE / future
  (reserved-not-built; the native-checker sibling consumes them later).
- **Deps:** U2 + U4.
- **Parallelism:** Cache-runtime lane; beside the semantic-graph lane.
- **DESIGN-LOCKED (effort weight EXTREME, §3.3): its rescope gate has RUN and the
  output is `docs/arch/u6-flow-call-resolution-design.md`** (consumes the U2/RI
  substrate). The cross-engine cycle/termination — `FlowReturn` ↔ `ResolveCall` ↔
  narrowing ↔ `ContextualTypeAt`, the `CheckerReentryGraph`, and flow narrowing as
  a dataflow fixed-point — is designed there to executable-pseudocode depth: runtime
  SCC detection + provisional-result-during-discharge + a stability decision +
  **fail-closed on non-convergence**, the narrowing JOIN ALGEBRA, the loop
  fixed-point, and the per-family `tsgo`-oracle baseline. The block contract below
  names the artifacts / guards; the recursion algorithm + its termination proof are
  owned by that locked design, not asserted here.
- **Risk:** large — adds the `FunctionFlowGraph` + the `FlowReturn` / `ResolveCall`
  query nodes + the slice cache-runtime nodes.
- **Required deletions:** the legacy `type_eval_build.rs` lightweight return scanner
  (`infer_return_type` / `infer_expression_type` / `collect_return_types` /
  `extract_object_literal_as_type` / `append_spread_array_element_types` — the
  arena-borrowing OXC walker) is cut over to the demand-sliced resolver (child
  Legacy-deletions); no second flow engine survives. This block also OWNS
  shrinking/deleting the merged `no_new_type_surface_engine_path_production_file`
  guard as it removes the legacy return / `type_surface` machinery — the guard's
  allowed engine count must NOT be preserved once the `type_surface` sidecar is gone
  (a shrinking ledger that retires with the engine it counts, not a frozen pin).
- **Guards:** `flow_slice_lowered_body_does_not_compute_slice_hash` (the hash-then-
  lower split); `flow_slice_keys_on_body_sensitive_hash_not_parse_stable_hash`;
  `function_flow_graph_built_once_per_function_skeleton`;
  `flow_slice_is_graph_reachability_not_procedural_walk`;
  `flow_graph_effect_edges_stay_live_past_value_writes`;
  `flow_slice_budget_exceeded_admits_nothing` (fail-closed `BudgetExceeded` →
  `ReturnOnly`); `FlowReturn` / `ResolveCall` route through the one engine. The
  `no_new_type_surface_engine_path_production_file` guard is shrunk/retired here in
  lockstep with the `type_surface` engine deletion (above), never carried forward
  pinning a stale count.

---

### U7 — Scheduler Cache-Node DAG Admission (B7e)

> **SUPERSEDED by the rescope gate — `docs/arch/u7-scheduler-submit-dag-decision.md`
> (LOCKED).** Verdict: **DEFER**. The multi-node `submit_dag` envelope
> (`CacheNodeDag`/`KeyedJob`/`EdgeGate`/`DagHandle`/`DagCompletionAggregator`) is held
> **un-built**: the hard scheduling core is already landed in `SchedulerDag::submit`, a
> cache→cache result edge is expressible at the raw `SchedulerDag` layer via
> `DepKey::CacheNode` + single-node `submit()` (the production scheduler *dispatch* path
> still asserts cache nodes are terminal — the `scheduler.rs` terminal-cache dispatch
> asserts — so the
> edge is not yet live end-to-end), and the biggest workload (`TypeInfoGraphResultDb`) is
> permanently singleflight-bound (§2.1). The landed cache-node substrate + B7a leaf
> primitives are KEPT (U9 machinery). U9 closes the `execute_cache_node` reachability gap
> via single-node lowering into the existing `SchedulerDag::submit` — lifting those
> terminal-cache dispatch assertions and wiring cache-node waiter-release + failure
> propagation — NOT a net-new envelope. The envelope
> is re-gated for JUSTIFY at U9 **only** on a proven all-or-none-atomic-admission
> correctness property OR a measured graph-scoped completion/cancellation need; default
> absent that = permanent CUT. The scope sketch below stands only if that re-gate passes.

- **Source track:** cache-runtime / scheduler.
- **Scope:** Add `CacheNodeDag`, `CacheNodeDagNode` (non-Clone; ready-queue
  element is `Arc<CacheNodeDagNode>`), `CacheNodeDagEdge` / `EdgeGate`, `KeyedJob`
  (envelope metadata), `CacheNodeOutcome` / `CacheNodeValue`,
  `CacheNodeCompletionSender`, `DagHandle`, `DagCompletionAggregator`. Implement
  `try_submit_dag(dag) -> SubmissionResult<DagHandle>` (typed `Backpressure`
  BEFORE readiness mutation, per H22) and `submit_dag_blocking` (parks on capacity
  condvar). Lower ALL cache nodes into the EXISTING `SchedulerDag` under ONE
  admission path (extend the §6b atomic admission core — NOT a parallel path).
- **Deps:** U1 (TaskKind::CacheNode + execute_cache_node); B7a (`SubmissionResult`
  / `DedupeHook` / `SchedulerCacheId`).
- **Parallelism:** Can run parallel AFTER U1, alongside the U4/U5/U6
  cache-runtime work and the entire semantic-graph lane (U8/U3+; the typeinfo DB
  does NOT ride `submit_dag` per §2.1).
- **Rescope gate: RUN (effort weight HIGH / justify-or-cut, §3.3) — verdict LOCKED at
  `docs/arch/u7-scheduler-submit-dag-decision.md`: DEFER.** The gate asked whether the
  cache-node DAG is justified against a MEASURED workload — a model-checked DAG for
  an I/O-bound LSP already served by singleflight is an over-engineering risk (the
  typeinfo DB explicitly does NOT ride `submit_dag` per §2.1, narrowing the workload
  that would justify it) — and answered DEFER: the envelope is held un-built, re-gated
  for JUSTIFY at U9 only, default permanent CUT. The block contract above and below is
  the scope sketch the U9 re-gate would resurrect IF it ever passes; it is not
  scheduled work.
- **Risk:** **very large / highest scheduler risk.** #1 stated risk: do NOT create
  a second readiness/accounting system beside `SchedulerDag` (no `ArrayQueue`, no
  `DagAdmissionBudget`, no parallel `DedupKey`). `WorkNodeIdentity` is THE dedupe
  identity. Preserve h23 capacity-reservation single-release + cooperative-pump
  invariants.
- **Required deletions:** none net-new to delete (no submitter-side `ArrayQueue` /
  `yield_now` / readiness-lock exists post-§7). `submit_dag` is net-new, NOT a
  `submit_batch` replacement (`submit_batch` was already deleted in §6c).
- **Guards:** keep `dag_arch_guards` (13/13) +
  `b7b_no_second_admission_budget_or_ready_queue` green; a guard that there is no
  second readiness structure; typed-`Backpressure`-before-mutation test (H22);
  single-release reservation test (h23).

---

### U8 — Wire-Surface Closure

- **Source track:** semantic-graph (R-2).
- **Parity docs:** child `docs/arch/native-typeinfo-parity-cache-export-session.md`
  (`U8.WIRE_SURFACE_CLOSURE` — the keystone block of that subplan), under parent
  `docs/arch/native-typeinfo-parity.md` (wire-surface purity §§1.3–1.5, the
  `TypeInfoGraphPayload` shape, the Typeinfo Wire Contract). This is an INDEX entry —
  the child doc owns the full block contract; it is cited, not restated.
- **Scope (whole-surface wire-purity end-state, NOT the exporter):** reconcile the
  ENTIRE public proto surface with the moved-concept end-state under the Typeinfo Wire
  Contract. **The framework-adapter merge landed part of this wire ahead of the gate, so
  the framework-surface arm is a RETAG / CUT-OVER of an already-merged provisional shape,
  NOT a from-scratch introduction:** the merged producer emits a provisional
  `FrameworkSurfacePayload` whose `graph` field is an embedded `SemanticTypeGraph`
  (`FrameworkSurfacePayload.graph = 4`, served at `TYPEINFO_GRAPH_SCHEMA_VERSION = 4`,
  populated by `typeinfo/framework_surface/graph_export.rs`). U8 RETAGS
  `FrameworkSurfacePayload.graph` to a `TypeInfoGraphPayload` carrier at a FRESH field
  tag, bumps `SemanticTypeGraph.schema_version`, `reserved`s the old `graph = 4` field
  (never reused), provides downlevel handling for pre-bump consumers (the U12 per-version
  encoder path), and updates the merged framework-surface proto/taxonomy guards to the
  retagged shape. Introduce `TypeInfoGraphPayload { graph, program_analysis, diagnostics,
  diagnostic_directives, relation_proofs }` and the
  sibling `ProgramAnalysisGraph { flow_narrowings, contextual_types }`, plus the
  `RelationPayload` + payload-side `relation_proofs` proof table; retire-and-`reserved`
  every relocated/retired `GraphTypeNode` arm (flow-narrowing 26 / contextual-type 27 /
  relation-proof 28), the
  `SemanticTypeGraph.diagnostics` (9) and `GraphTypeParameter.no_infer` (9) fields.
  Module / global augmentation are NOT relocated and NOT retired: they REMAIN the live
  `GraphTypeNode` arms `module_augmentation` (23) / `global_augmentation` (25) — the
  proposed `DeclarationAnalysisGraph { module_augmentations, global_augmentations }`
  wire message (and its `declaration_surfaces` payload field) was REJECTED. Decision of
  record: `native-typeinfo-parity-u2-reducers.md` → `U2.MODULE_AUGMENTATION` ("NO wire
  change") + the `/type-resolution` skill → "Merge/augmentation WIRE domain"; the live
  proto carries the arms and `typeinfo_graph_contract_guards.rs` pins them as live
  taxonomy members;
  move every public `SemanticTypeGraph` embedding to a `TypeInfoGraphPayload` carrier at
  a fresh tag — `TypeInfoGraphResponse.graph` as a clean additive migration, and
  `FrameworkSurfacePayload.graph` as the retag-of-provisional cut-over detailed above
  (the merge already shipped its embedded `SemanticTypeGraph`, so this arm cuts over the
  live provisional, it does not introduce a new field); bump
  `SemanticTypeGraph.schema_version`; extend
  `SUPPORTED_TYPEINFO_GRAPH_SCHEMA_VERSIONS` + the closed-set validator; regenerate the
  byte-equal TS bindings. The exporter that POPULATES this shape is U12; this block
  only closes the shape every later block reads/writes.
- **Deps:** U2 (parent) + U6 (parent) + **S5.B12** — the wire surface carries the U2
  type-value arms and the U6 flow/relation facts, so it closes only once those producers
  exist; AND S5.B12 must be done first (the HARD GATE `U8 ← {U6, S5.B12}` of §3.1.1 /
  §3.1.3: do NOT build the new wire/result/export stack around the `VueMacroElements` /
  `HostResolvedNamedTypeKey` / `resolve_type/` sidecar that S5.B11/B12 delete). It does
  NOT depend on the exporter (U12) or the result DB (U10) — it is their prerequisite.
- **Parallelism:** Semantic-graph lane head (the keystone every other cache/export
  block — including U3 — depends on); runs beside the cache-runtime lane (U1/U4–U7/U9).
- **Risk:** large (whole-proto closure + a schema_version bump + byte-equal TS
  regeneration); mechanical once the moved-concept homes are fixed.
- **Required deletions:** the relocated `GraphTypeNode` arms (26/27/28 — NOT the
  augmentation arms 23/25, which stay live; the `DeclarationAnalysisGraph` relocation
  was rejected),
  `SemanticTypeGraph.diagnostics` (9), `GraphTypeParameter.no_infer` (9) — retired +
  `reserved`, never reused; the `SemanticTypeGraph graph = 1` server-populated embedding
  AND the merged provisional `FrameworkSurfacePayload.graph = 4` embedded-`SemanticTypeGraph`
  field (retired/`reserved` or downgrade-only, the framework arm cut over from the live
  provisional, both replaced by fresh `TypeInfoGraphPayload` carriers). No field number is
  reused.
- **Guards:** `node_taxonomy_complete` (the LANDED single enumerating assertion that
  pins the EXACT 32-arm `GraphTypeNode` `oneof kind` set, INCLUDING the live
  augmentation arms 23/25 — never flagging them for retirement, per the locked
  rejection in `native-typeinfo-parity-u2-reducers.md` → `U2.MODULE_AUGMENTATION`; the
  earlier-planned split/denylist guards
  `graph_type_node_oneof_contains_only_type_value_arms` /
  `graph_type_node_allowlist_arms_have_type_value_classification` /
  `typeinfo_wire_surface_has_no_retired_concept_fields` were NOT landed — they do not
  exist in `crates/` — and are subsumed by it),
  `no_non_type_value_smuggled_into_graph_type_node` (classifies the merge/augmentation
  kinds 21–25 as legitimate value-bearing arms),
  `flow_contextual_facts_not_graph_type_nodes`,
  `program_analysis_graph_exposes_flow_contextual_queries`,
  `flow_contextual_doc_and_wire_placement_match_program_analysis_graph`,
  `relation_proofs_not_graph_type_nodes`,
  `typeinfo_relate_payload_exposes_relation_proof_without_graph_type_node`,
  `no_infer_not_type_parameter_metadata`, `diagnostics_only_on_typeinfo_graph_payload`,
  `typeinfo_graph_response_payload_arm_is_additive_not_retyped`,
  `framework_surface_payload_graph_payload_is_additive_not_retyped`,
  `all_public_semantic_type_graph_embeddings_are_payload_wrapped` (the generated U8
  block-contract row in `gen-typeinfo-ignore-manifest.py` carries this exact set); plus
  the four wire-contract guards (proto/TS oneof parity, byte-equal TS freshness, audit
  parity, request validation).
- **Perf-backlog cross-ref (UP / §B):** the **B-typeinfo "typeinfo wire / Stage5"**
  sub-item (UP.G) is owned here. **HARD CONSTRAINT: U8+ ONLY — FORBIDDEN before
  S5.B11/B12** (the `U8 ← {U6, S5.B12}` HARD GATE of §3.1.1, bet #4): do
  NOT build the new wire/result/export stack around the `VueMacroElements` /
  `HostResolvedNamedTypeKey` / `resolve_type/` sidecar that S5.B11/B12 delete. Any
  wire-side typeinfo perf rides this block's schema closure, never a pre-S5 shape.

---

### U9 — Scheduler Cache-Node Lowering (B7f) — DESIGN-LOCKED

**LOCKED design: `docs/arch/u9-session-bridge-design.md` (supersedes this block body).** The design gate
found NO cache-node consumer (tree-wide or committed; the only result DB, `TypeInfoGraphResultDb`, is
permanently singleflight-bound) and that the central `DedupeHook` bridge is UNBUILDABLE under R6 (the
`WorkNodeIdentity.key_hash: Hash16 → InflightTable<K>` map is lossy + pre-admission). So U9 is NOT a session
bridge.

- **Source track:** cache-runtime / scheduler.
- **Scope:** (a) finish + HARDEN the half-built cache-node DAG edge so a cache→cache dependency is live and
  sound — lift the three terminal-cache dispatch asserts → `Submission::Wake`; net-new cache failed-dep fanout
  (`cancel()` records `FailedDepRecord` under `DepKey::CacheNode` + persistent `terminal_dep_failures`) + a
  cache-arm pre-execute `DependencyFailed` short-circuit (the file-stage chokepoints are `unreachable!()` for
  `CacheNode`); a net-new bounded cache-edge cycle guard at the submit chokepoint; relax the
  `scheduler.rs` forbid for the cache path ONLY. Proven by a discriminating test-only `StageExecutor`
  (release / failure-propagate / cycle-reject). (b) DELETE the dead B7a leaf primitives (below). NO session
  bridge, NO registry, NO `DedupeHook`, NO `CpuConcurrencySemaphore` wiring, NO host back-edge, NO production
  materializer, NO H20 session edge. Update `.claude/skills/{scheduler,host-session}/SKILL.md`.
- **Deps:** U1 (landed `TaskKind::CacheNode` + `WorkNodeIdentity::CacheNode` + `DepKey::CacheNode` +
  `execute_cache_node` surface) + B7a (leaf primitives). **NOT a built `submit_dag` envelope** — U7 is
  DEFERRED-TO-U9 (`docs/arch/u7-scheduler-submit-dag-decision.md`): U9 closes the gap via single-node lowering
  into the existing `SchedulerDag::submit`.
- **Parallelism:** Cache-runtime lane tail; beside the semantic-graph lane.
- **Risk:** medium — the load-bearing work is the net-new failure path + cycle guard, NOT the assert lift.
- **Required deletions:** `DedupeHook`/`DedupeJoiner`/`NoDedupeHook` (`dedupe_hook.rs`), `SubmissionResult`,
  `CpuConcurrencySemaphore`/`CpuConcurrencyPermit` (`cpu_concurrency.rs`), the rich `CancellationToken`
  (`cancellation.rs`) — per the U7 §6 standing-honesty condition (unconsumed B7a primitives). `SchedulerCacheId`
  survives as the opaque `WorkNodeIdentity` identity field only.
- **Core re-gate:** the `WorkNodeIdentity::CacheNode` arm survives U9 with a test-only proof under a HARD
  deletion re-gate — if the next block lands no committed production cache-node submitter, the whole arm is
  re-gated for deletion (full γ). See design §7.
- **Guards:** keep `no_session_dep` (H20) green (NO sanctioned session edge is added);
  `b7b_no_second_admission_budget_or_ready_queue` + `no_parking_lot_semaphore` stay green; the discriminating
  three-axis cache-edge characterization test (release / failure-propagate / cycle-reject) is the correctness
  guard.

---

### U10 — Result DB + Mode/Demand Exactness

- **Source track:** semantic-graph (R-3 + R-4).
- **Parity docs:** child `docs/arch/native-typeinfo-parity-cache-export-session.md`
  (`U10.RESULT_DB`), under parent `docs/arch/native-typeinfo-parity.md` (§§5–6, the
  fact-based cache architecture). INDEX entry — the child doc owns the full contract.
- **Scope:**
  - `TypeInfoGraphResultDb` on `ProjectTypeStore` — the query-identity final-result
    cache handing out immutable `Arc<TypeInfoGraphPayload>`, validating each candidate's
    `ReadSetSignature.facts` against the live `StoreView` on every warm hit, cold-admitting
    through singleflight (`cooperative_admit_with_post_publish`), with the multi-candidate
    `FamilySlots` storage under the per-family adaptive cap + invalid-first/LRU-by-valid-hit
    eviction + global memory ceiling (the U3 substrate). Its key excludes content/version
    hashes + `fact_dep_signature` (R6) and includes `lib_env_hash` (R21). The
    completion-fence `publish_with_retry` consumes the canonical `MAX_INFLIGHT_RETRIES = 3`
    (NO second retry constant); overlay-only results never populate this base/persistent DB.
  - **Consolidate the merged off-store framework-surface result cache onto this DB.** The
    framework-adapter merge parked its surface-result cache on registry rows — the
    `FrameworkRegistration.surface_store` (`FrameworkSurfaceStore`) and `FrameworkScriptCaches`
    live OFF `ProjectTypeStore`, and (worse) have NO in-flight collapse. Leaving the
    framework-surface result cache on registry rows is NOT the final architecture: this
    block moves it onto `ProjectTypeStore` / `TypeInfoGraphResultDb` (the U3 off-store
    detector saw it; U10 removes the off-store residence) AND adds TRUE singleflight
    (`cooperative_admit_with_post_publish`) so concurrent cold framework-surface requests
    for the same component collapse onto one materialization path — matching every other
    typeinfo result the DB serves.
  - The mode / demand / expansion-boundary **EXACTNESS** gating over the U2 reducers,
    stated over the `ProjectionDemand` / `EvalPolicy` demand lattice (parent §2.10) of
    which the five mode names are public presets: `Identity` returns the alias decl
    identity; `Shallow` exposes one shell level (operator carriers stay `Ref`);
    `Expanded` `keyof T` is bounded to T's shallow member-name surface; `Navigate` runs
    the intermediate hops; `Skeleton` is the `TypeParamShells` + carrier-stop preset.
    `Pick`/`Omit`/inline/local/imported projection is path-precise (no unpicked/excluded/
    unselected branch loaded). Cache satisfaction/backfill is by the LANDED
    materialized-point model: a warm hit requires a RECORDED materialised `(path, point)`
    (the candidate's `MemoEntry.satisfied_projection` set — what the compute actually
    produced) to dominate the request at the same path
    (`cached_satisfies(MemoEntry.satisfied_projection, requested_point_for_key(key))`),
    and backfill clones only recorded materialized points — the demand lattice is the
    algebra INSIDE that check, never the satisfaction authority, never mode-enum order,
    and never a nominal-demand oracle. U10's residual satisfaction work is the
    published-boundary EXACTNESS gating over this landed rail (confirming the U2
    reducers are path-precise at the result-DB boundary), NOT a re-implementation or
    replacement of the satisfaction model.
- **Deps:** U3 (the typed-admission + `ReadSetSignature` validity rail this DB admits
  through) + U8 (the closed `TypeInfoGraphPayload` shape it caches) + U2/U6 parents
  (the reducers + flow solver it gates). **Per §2.1: built on the singleflight
  substrate, NOT on `submit_dag`.**
- **Parallelism:** Semantic-graph lane; cache-runtime lane (U7/U9) is unrelated.
- **Risk:** medium-large — the fence/admission contract is invariant-dense (warm-exact-only,
  3-retry, no-partial-admit, zero-alloc warm hit) and the mode/demand exactness is the
  gate that confirms the U2 reducers are path-precise at the published boundary.
- **Required deletions:** any final-result cache keyed on content/version hash or
  `fact_dep_signature` (R6); any `Identity`/`Shallow`/`Expanded` path that returns the
  alias body / reduces a `Pick<…>` member body / walks member bodies into the keyspace;
  any eager sibling materialization during projection. Do NOT resurrect
  `finalise_signature_or_empty` — build on `FactReadSet::finalise` + `SignatureAdmission`.
- **Guards:** the LANDED satisfaction guards kept green and exercised through
  `TypeInfoGraphResultDb`: `cache_satisfaction_is_materialized_point_not_nominal_demand`
  + `backfill_writes_only_recorded_materialized_points` (the formerly-planned
  `cache_satisfaction_is_demand_lattice_not_enum_order` is RETIRED — written as planned
  it would reintroduce a WEAKER nominal-lattice satisfaction rail under a new name);
  plus the NET-NEW published-boundary exactness guard
  `result_db_published_boundary_serves_only_recorded_materialized_points` (named here;
  mirrored verbatim in the generated U10 block-contract row alongside the landed pair),
  which COMPOSES with (never shadows or weakens) the landed pair by asserting the
  result-DB boundary serves only recorded materialized points; the
  U3 multi-candidate-substrate guards exercised through `TypeInfoGraphResultDb`
  (`cache_candidate_cap_is_per_family_not_uniform`, `family_eviction_prefers_invalid_then_lru_valid_hit`,
  `persistent_caches_never_admit_overlay_only_results` — must not regress); the
  demand-lattice DEFINITION guards `query_modes_are_presets_over_projection_demand_eval_policy`
  / `skeleton_is_typeparamshells_plus_carrier_stop_not_special_mode` (defined at U2,
  depended on here); a guard that there is no second retry constant; a zero-alloc warm-hit
  test (no audit payload allocation without accumulator).
- **Perf-backlog cross-ref (UP / §B):** the **B-typeinfo StoreView token-shortcut**
  + **`ensure_indexed_ready_many`** sub-items (UP.G) are owned here on the
  result-DB / `StoreView` surface. Verify what is already landed before
  re-implementing (the StoreView token + the indexed-ready-many path overlap
  substrate already on the tree); route through the shared resolver / the existing
  `TypeInfoGraphResultDb` admission; NO new off-`ProjectTypeStore` cache.

---

### U11 — Public Relation / Session Surfaces + Audit Execution

- **Source track:** semantic-graph (R-6 + R-5).
- **Parity docs:** child `docs/arch/native-typeinfo-parity-cache-export-session.md`
  (`U11.PUBLIC_RELATION_SESSION`), under parent `docs/arch/native-typeinfo-parity.md`
  (§4, PART 2 §§10–11, the audit infrastructure). INDEX entry — the child doc owns the
  full contract.
- **Scope (the session + document-lifecycle host surface the exporter feeds):**
  - Public `relate()` returning the public `RelationPayload` (outcome / inference
    bindings / relation proof + typed `BudgetExceeded`), the proof carried off the
    type-values surface via the payload-side proof table — NOT the bare tri-state
    `RelationResult`. `relate` is the sole assignability authority.
  - The request-footprint attachment pipeline on every audited typeinfo resolver path
    when `footprint_capture=true` (the host-audit passive-observer footprint miner):
    the footprint reports the requested import / projected indexed-access members
    precisely, excludes unprojected branches, and stays attached on warm reads.
  - End-to-end edit-cycle cache invalidation at the session boundary over the U3
    route-fact rails: selected-leaf edit flips the surface; unselected-sibling edit
    keeps the warm cache (zero VFS reads / zero RouteDb misses, footprint attached);
    barrel edit redirects the route + drops the prior leaf from the V2 footprint;
    augmentation-patch edit surfaces the augmented shape; in-place package edit flips
    the surface.
  - The session execution surface: the `_with_audit` methods returning
    `AuditedResult<Arc<...>, TypeInfoRequestError>`, validate-before-execute, routed
    through `TypeInfoGraphResultDb`; per-entry-point cold / warm / degraded
    `StructuredAuditEvent` emission (warm = counter only), the
    `TypeInfoGraphFootprintCell` in `footprint_miner.rs`, nested-record semantics for
    `expand_graph_around`, `wave_3_entry_points_propagate_tls` extended with the
    typeinfo drivers. **Use `exactness_counts: BTreeMap` (§2.2), not `exactness_*`.**
  - **Schema-version RUNTIME (server side; A.5/A.6 — the runtime layer on top of A0a's
    contract GATE).** Per-request schema-version ECHO (every response carries the
    negotiated `schema_version`); runtime NEGOTIATION via
    `SchemaVersionCapabilities::validated_supported_versions()` restricted to versions
    backed by a registered encoder; a `V < current` session emits via the U12
    `encode_typeinfo_payload_for_version(V, payload)` downlevel path (co-landed with U12
    so negotiation is never advertised without a backing encoder). The negotiation /
    downlevel policy covers the merged framework-surface operation
    (`GRAPH_OPERATION_FRAMEWORK_SURFACES`) like EVERY other typeinfo operation: it rides
    the same request envelope and the same post-U8 `TypeInfoGraphPayload` carrier, so a
    framework-surface request negotiates and downlevels its schema through this same path
    — never a framework-specific negotiation fork.
- **Deps:** U12 (the exporter whose `TypeInfoGraphPayload` / `RelationPayload` this
  surface returns) + U3 (the route-fact rails footprint/invalidation observe) + U8 +
  U2/U6 parents. A0a request validators + `StructuredTypeExpression` proto + the
  schema-version handshake gate + `Relate` memo are landed.
- **Parallelism:** Semantic-graph lane.
- **Risk:** medium-large.
- **Required deletions:** the bare tri-state `RelationResult` on the public path
  (replaced by `RelationPayload`); any resolver path that records the scratch/owner
  footprint without attributing projected imported members precisely; any cache
  participant that invalidates on an unreferenced barrel sibling edit.
- **Guards:** `relate_query_value_carries_relation_proof_and_budget_state`,
  `relation_proofs_not_graph_type_nodes`,
  `typeinfo_relate_payload_exposes_relation_proof_without_graph_type_node` (kept green at
  the session boundary); validate-before-execute coverage; the audit 3-branch
  (cold/warm/degraded) discriminators; `wave_3_entry_points_propagate_tls` extended and
  discriminating; `every_typeinfo_request_carries_schema_version` (per-request echo);
  `server_supported_versions_have_encoders` (never advertise a version without a
  registered encoder).
- **Perf-backlog cross-ref (UP / §B — the gated `verter_session` host-surface +
  document-lifecycle perf deliverables; UP.G indexes them):** all of these clean-room
  re-implement under the §4 wave-1 lessons (ABA / TOCTOU / armed-monitor) — the
  source perf branches are retired, no diff safety net.
  - **F1 — host-owned retained-close (ENTIRE item).** Retain read-only externals
    across close; distinguish dirty-vs-clean; serialize close-vs-upsert. Needs
    `VerterHost::close_document() -> HostCloseDisposition` (+ `HostCloseEvictReason`
    / `HostCloseAbsentReason`), `evict_after_close_decision`,
    `DependencyState.retained_source_content_generation`, the workspace retain
    authority (`WorkspaceAccess::is_retainable_read_only` + `WorkspaceCloseEffect`),
    then the LSP gate/generation layer (`open_seq` discipline + `*_serialized` /
    `*_gated` mutators). Target: `host_lifecycle.rs` (the close/evict authority),
    `host_upsert.rs`, `crates/verter_workspace/src/`, and
    `crates/verter_lsp/src/server/lifecycle.rs` `handle_did_close`. **F1 has NO
    diff safety net** (the source commit is gone from this repo) — clean-room
    re-impl; **preserve the Svelte rune-module branches now in `handle_did_close`**
    (the source predates them). Named race tests: close-vs-reopen (stale close must
    not clobber a concurrent reopen), dirty-buffer-evict (a dirty buffer is NOT
    retained), close-after-open provider-window. The race it closes is still live.
  - **C16 — delta `UpsertRequest`.** `UpsertRequest` (`verter_session/src/types.rs:1426`)
    carries only full `source: Arc<str>`. Make it delta-capable; host stays
    canonical source authority, APPLIES the delta, computes the final whole-content
    hash, routes through `upsert_many_with_priority`, publishes the same
    `IndexedReady` / facts a cold full upsert would; stale-base / torn deltas fail
    closed. Guards: `delta_host_upsert_cold_full_delta_equivalence`,
    `delta_host_upsert_stale_base_rejects_closed`. Re-confirm `types.rs:1426` is
    still full-source-only at impl time.
  - **C17 — host artifact-bundle return.** `IdeResponse` (`types.rs:1807`) carries
    NO artifact id. One host call returns update result + IDE artifact metadata
    (generated code `Arc<str>`, source-map handle, **artifact id/hash**, diagnostics,
    public API if materialized). **UNBLOCKS UP.C4** (edge UP.C4 ← U11.C17) — lets
    the LSP stop hashing generated strings. Guard:
    `mapping_context_uses_host_returned_artifact_identity_only`.
  - **C18 — fold `notify_upsert`.** The LSP calls both `host.upsert` and
    `host.notify_upsert` for one edit; `upsert` already calls `notify_upsert`
    internally (`host_upsert.rs:824`), and the artifact-only eviction arm is a no-op
    early return. Remove the extra LSP `notify_upsert` + source clone; fold overlay
    semantics into the authoritative upsert. Guard:
    `notify_upsert_fold_preserves_scheduler_tracked_noop_and_artifact_only_eviction`.
  - **C19 — cross-file concurrent upsert + per-canonical locks.** The global
    `Server::did_change_mutex` (taken at `lifecycle.rs:597`) serializes ALL
    cross-file edits. Host must explicitly support concurrent upserts for DIFFERENT
    canonical ids with same-canonical ordering preserved; THEN replace the global
    mutex with per-canonical keyed locks + an LSP-owned admission limiter. **Do NOT
    depend on `verter_scheduler::CpuConcurrencySemaphore`** (U9 records it
    deletion-bound, ~l.3451 region — cross-constraint with U9's required deletions).
    **UNBLOCKS UP.C2b** — the robust same-canonical push serialization. Guard:
    `host_upsert_cross_canonical_concurrency_same_canonical_serialization`.
  - **C20 — cooperative cancellation (host half).** `tokio::time::timeout` stops
    awaiting but cannot cancel synchronous `block_in_place` host work. Reach the LSP
    request-cancellation token into host computations; cancelled / superseded work
    may finish privately but must NOT publish cache-visible artifacts / diagnostics
    / facts unless current (aligns with this block's existing
    "cancelled/superseded never publishes" rule). **UNBLOCKS UP.C8's host-reach
    cancel** (the LSP-local ingress half is ungated — UP.C8). Guard:
    `host_cooperative_cancellation_no_publish_after_stale`.

---

### U12 — TypeInfo Graph Exporter

- **Source track:** semantic-graph (R-2 + R-7).
- **Parity docs:** child `docs/arch/native-typeinfo-parity-cache-export-session.md`
  (`U12.EXPORTER`), under parent `docs/arch/native-typeinfo-parity.md` (wire-surface
  purity §§1.3–1.5, §3, the Typed-IR-Only Resolver Rule). INDEX entry — the child doc
  owns the full contract.
- **Scope (the request → graph projection; a thin projection of the engine's typed
  results, NOT a second resolver):** `crates/verter_session/src/typeinfo/{surface,raise,
  evaluate_type_expression,resolve_named_symbol}.rs` — project the engine's typed
  `SemanticQueryValue` results into the closed `TypeInfoGraphPayload`: type values onto
  the closed `GraphTypeNode` type-value allowlist, flow/contextual facts onto
  `ProgramAnalysisGraph`, declaration/environment (`DeclarationAnalysis`) facts onto the
  live augmentation `GraphTypeNode` arms 23/25 on `graph` (no `DeclarationAnalysisGraph`
  side surface — the relocation was rejected; see the U8 block),
  diagnostics onto their side tables, relation proofs onto the payload-side
  `relation_proofs` table by opaque proof id, and the `RelationPayload` for public
  `relate`. No non-type value is materialised as a `GraphTypeNode` arm; the exporter does
  NO query-time resolution (it dispatches the U2 queries — e.g. `ResolveMergedDeclaration`
  — rather than re-deriving merge/augmentation structure from `IndexedReady`). It
  publishes the payload into `TypeInfoGraphResultDb` (U10) via cooperative admission.
  The Rust→wire encoding helpers live in `crates/verter_protocol/src/typeinfo/graph.rs`.
  - **For framework surfaces, this block does NOT build the exporter from scratch — it
    FOLDS / REPLACES the merged old-shape `typeinfo/framework_surface/graph_export.rs`.**
    The framework-adapter merge already shipped `graph_export.rs` as a provisional
    framework-surface exporter that emits the embedded `SemanticTypeGraph`
    (`FrameworkSurfacePayload.graph`) ahead of order. U12 cuts that producer over IN PLACE
    to the same thin zero-dispatch projection it applies to every other surface: it emits
    the post-U8 `TypeInfoGraphPayload` carrier (the retagged framework-surface field, U8),
    projecting the engine's already-resolved typed `SemanticQueryValue` results — it must
    stay zero-dispatch (NO query-time re-resolution, the one-resolver rule) and must NOT
    leave the old-shape `SemanticTypeGraph` producer alive beside the new carrier (cut
    over, never dual-pathed).
  - **Schema-version downlevel ENCODERS (A.5/A.6 — the Rust encoder backing for U11's
    negotiation, co-landed across U11/U12).** `encode_typeinfo_payload_for_version(V,
    payload)` + the `KNOWN_VARIANTS_AT_VERSION` cumulative-exhaustive table (per-version
    `&[VariantId]` sets). Newer-only variants project to compatible substitutes for
    `v(N-1)` consumers (e.g. post-V `ExpansionStatus::ExactOpenGeneric` → `ExactSymbolic
    { reason: GenericPreserved }`); unsupported constructs degrade through
    `UnsupportedConstruct::DowngradedFromNewerSchema` → `…::Unsupported { construct:
    "schema_skew" }` → `Opaque(Miss { reason: SchemaSkew })`. Each encoder is validated
    against `KNOWN_VARIANTS_AT_VERSION[target_version]` (NOT `encoder.version`).
- **Deps:** U10 (the result DB the exporter's payload is admitted into) + U8 (the closed
  payload shape it populates) + U3 + U2/U6 parents. The session surfaces (U11) and the
  TS projection (U13) consume the exporter's output.
- **Parallelism:** Semantic-graph lane.
- **Risk:** large (wide lowering table); mechanical once U2 is correct (the exporter is a
  thin projection).
- **Required deletions (one clean cutover, with their replacements):**
  - Any exporter path that emits flow / contextual / relation-proof facts as
    `GraphTypeNode` arms (relocated to the payload side tables). Module / global
    augmentation facts are NOT in this deletion: they stay on the live augmentation
    `GraphTypeNode` arms 23/25 (the `DeclarationAnalysisGraph` relocation was rejected
    — see the U8 block).
  - The merged old-shape framework-surface emission path in
    `typeinfo/framework_surface/graph_export.rs` (the provisional embedded-`SemanticTypeGraph`
    producer) — cut over IN PLACE to the post-U8 `TypeInfoGraphPayload` projection (above),
    leaving NO old-shape producer alive beside the new carrier (replacement, not dual-path).
  - Any exporter path that re-resolves a type at projection time (the one-resolver rule).
  - The legacy scratch text-evaluator (`typeinfo/{evaluate_type_expression,
    scratch_cache}.rs`) once `StructuredTypeExpression` dispatch (U12) lands — the
    `StructuredTypeExpression`→`SemanticQueryKey` decode that obsoletes the
    text-evaluator lands in U12 itself, via the `resolve_named_symbol.rs` request
    entrypoint (U11 DEPENDS on U12 per the U11 child `Prerequisites: U12.EXPORTER`, so
    U12 cannot gate its own deletion on U11). The text-evaluator BODY is gutted from
    `evaluate_type_expression.rs` while the TYPED per-node raise/evaluate path in that
    same file SURVIVES (policed by
    `evaluate_type_expression_does_not_call_parse_type_annotation`); `scratch_cache.rs`
    is deleted outright. The `verter_protocol` `GraphBuilder` / `graph/schema/*` /
    `graph/mod.rs` re-exports
    (≈36KB) with the legacy proto `EvaluateTypeExpressionRequestDto` fields `reserved`;
    rename `symbol_inventory.rs` → `list_symbols.rs`. (The legacy NAPI/WASM typeinfo
    entries + the pure-TS decoder/component-meta type-graph files are deleted in U13,
    same clean cutover.)
- **Guards:** the U8 wire-purity guards over the exporter's OUTPUT
  (`no_non_type_value_smuggled_into_graph_type_node`,
  `program_analysis_graph_exposes_flow_contextual_queries`, `relation_proofs_not_graph_type_nodes`,
  `all_public_semantic_type_graph_embeddings_are_payload_wrapped` — kept green); a guard
  that the exporter is pure (no second resolution path; the scratch evaluator's
  `parse_type_annotation` use no longer exists in the resolver pipeline);
  `known_variants_at_version_rows_are_cumulative_exact_sets`,
  `downgrade_encoder_never_emits_variant_unknown_to_target_version`,
  `known_variants_table_matches_proto_at_version` (CI — table regenerated from proto).
- **Perf-backlog cross-ref (UP / §B):** any **B-typeinfo exporter-side perf**
  (UP.G) — the combined component-meta view projection / zero-dispatch projection —
  rides this block's thin-projection cutover. The exporter does NO query-time
  resolution (single-engine rule); perf here means a cheaper projection of the
  already-resolved typed results, never a second resolver or a new off-store cache.

---

### U13 — Published Projection (GraphTypeNode + TS TypeDescriptor)

- **Source track:** semantic-graph (R-9 + R-10).
- **Parity docs:** child `docs/arch/native-typeinfo-parity-cache-export-session.md`
  (`U13.PROJECTION`), under parent `docs/arch/native-typeinfo-parity.md` (§1, §8, the
  Typed-IR-Only Resolver Rule, the Component-Meta Native vs Compat Rule). INDEX entry —
  the child doc owns the full contract.
- **Scope (the published-surface block U14/U15 + host-backed consumers read):**
  - The closed `GraphTypeNode` type-value projection of every U2 type value on the
    published `TypeInfoSurface` (only the allowlist arms appear; no non-type value in the
    projection — the moved-off facts are read from the relocated side tables).
  - The TS `TypeDescriptor` projection consuming the wire payload STRUCTURALLY:
    `packages/type-ir/src/type-ir.ts` (the `TypeDescriptor` schema — any missing variant
    added here, never recovered through text); `packages/component-meta/src/{type-graph-decode,
    type-graph-proto-decode}.ts` + `compat/native-projection.ts` (structural decode of the
    wire `TypeInfoGraphPayload`); the compat interop (`compat/checker.ts`,
    `compat/schema.ts`) reading `prop.type` (`TypeDescriptor`) for every semantic decision
    (`prop.rawType` display-only; type-role classification structural, not identifier
    suffix). The FFI binary-protobuf surface (`verter_napi/src/typeinfo.rs` +
    `verter_wasm/src/typeinfo.rs`, `Buffer` / `Uint8Array`) + the public TS session
    (`packages/typeinfo/src/{session,index}.ts`) + the projection packages
    (`projections/{display, type-descriptor, json-schema, zod, storybook, docs}/`, cycle-id
    `z.lazy` memoisation, `SharedLoadReuse` audit-terminal skip) wire through. `TypeDescriptor`
    becomes a projection target.
  - **Framework surfaces are consumed through the post-U8 `TypeInfoGraphPayload`, NOT the
    provisional embedded `SemanticTypeGraph`.** The framework-adapter merge's producer
    currently emits the embedded `SemanticTypeGraph` shape (`FrameworkSurfacePayload.graph`)
    provisionally; once U8 retags that field and U12 cuts the producer over, the TS / public
    projection MUST decode framework-surface responses through the retagged
    `TypeInfoGraphPayload` carrier — the same structural decode path every other typeinfo
    response uses — never the old embedded-`SemanticTypeGraph` shape (no provisional decoder
    branch left behind).
- **Deps:** U12 (the exporter whose `TypeInfoGraphPayload` this projection decodes) + U8
  (the closed wire shape it reads).
- **Parallelism:** Semantic-graph lane.
- **Risk:** medium-large (the structural decode + 6 projections); this block owns ZERO
  `IgnoredTestRow`s (a substrate block — every published type-value row stays owned by its
  computing U2/U6 block per §10.4.1).
- **Required deletions (one clean cutover):** any published projection / TS compat branch
  that drives a semantic decision from `prop.rawType` / a raw/display string (`looksLike*`
  / `extract*` / `normalize*` / `split*` / `strip*` / `prefer*` / `repairOpaque*`) or from
  an identifier-name suffix (`name.ends_with("Props")`); the descriptor-bridge
  (`descriptor-to-native.ts` / `native-to-descriptor.ts`, A.20) + compat semantic-recovery
  hooks; the legacy NAPI/WASM typeinfo entries (consuming `EvaluateTypeExpressionRequestDto`),
  `packages/typeinfo/src/{types, native-type-expr}.ts`, and `packages/component-meta/src/{type-graph*,
  type-expr-bridge}.ts` (the 8 legacy type-graph files); any AST/source fallback or second
  TS resolver/expander; legacy entry-point names.
- **Guards:** the `@verter/component-meta` no-rawtype-reads contract
  (`packages/component-meta/tests/no-rawtype-reads.spec.ts`) extended to the projection
  packages; the published-surface constants parity
  (`crates/verter_audit/tests/published_surface_constants_match_ts_port.rs`); the U8
  wire-purity guards kept green over the published surface;
  `capability_rows_map_to_expected_query_fact_mechanisms` (no published-surface row mapped
  to U13); projection round-trip discriminators.
- **Perf-backlog cross-note (UP.1 G14.1):** if this block reshapes
  `DocumentProviderProjection` / `ProviderPositionMapper`, do NOT regress the
  landed `Box → Arc<PositionMapper>` read-path share (UP.1 G14.1, commit
  `0f2e19fc1`) — re-apply the Arc read-path share, never revert to a per-read
  `Box` deep-clone.

---

### U14 — Vue Framework Adapter Re-Wire

- **Source track:** semantic-graph / component-meta (R-11).
- **Parity docs:** child `docs/arch/native-typeinfo-parity-adapters-final-lift.md`
  (owns U14 / U15 — framework adapters, integrations, final lift), under parent
  `docs/arch/native-typeinfo-parity.md`.
- **Scope (RE-WIRE the already-merged adapter substrate, NOT build it):** the
  framework-adapter merge already landed the `FrameworkAdapterRegistry` + the thin
  plan/normalize adapter shape (A.15) and the Vue + Svelte adapters. This block therefore
  does NOT build the registry / surface adapter from scratch — it RE-WIRES the merged
  `FrameworkAdapterRegistry` / adapters onto the U11/U13 PUBLIC surfaces: the adapter
  consumes the U11 public session (the framework-surface request path) and the U13 published
  projection (the post-U8 `TypeInfoGraphPayload` / `FrameworkSurfacePayload` structural
  decode), and `@verter/component-meta` / `compat` become thin projection wrappers over that
  one published surface. The re-wire MUST NOT rebuild macro meaning — NO parallel surface,
  NO second resolver/expander in the adapter; the macro semantics are the shared
  `ResolveMacroPayload` normalization (per §3.1.1, which `U14` consumes structurally and is
  forbidden from re-deriving). Fix the 4 known Vue mismatch cases on the re-wired path:
  Popover `SlotProps<M>`, theme-alias display, `Button["variants"]["color"]` indexed-access,
  ContentSearch intersection. Lifts the single `MacroResolution` row
  (`Parameters<NonNullable<T['slot']>>[0]` slot-payload extraction routed through the shared
  `IndexedAccess` / `NonNullable` / `Parameters` reductions + the U6 call signature).
- **Deps:** U13 + U11 (the published projection + the public session surface the
  adapter consumes; and transitively U2/U6/U8/U10/U12).
- **Parallelism:** Semantic-graph lane tail.
- **Risk:** **large / high regression surface** — re-points the live native
  component-meta pipeline onto the published U11/U13 surface; regression risk against the
  existing corpus is greatest here.
- **Required deletions:** the legacy native-component-meta resolution path the re-wire
  supersedes (cut over to the published surface, do not dual-path); any adapter-local
  surface/expander that would re-derive macro meaning instead of consuming the shared
  normalization.
- **Guards:** the 4 Vue mismatch-case regression tests are PRESERVED as the acceptance
  bar (each fails on the legacy path, passes on the re-wired adapter); a guard that
  component-meta is a thin adapter (no second resolver/expander) per the native-vs-compat
  CRITICAL rule.
- **Perf-backlog cross-ref (UP / §B — the gated Vue-adapter / materialization perf
  deliverables; UP.G indexes them):**
  - **D-I3 — virtual-file-pipeline string-surgery removal.** Remove the
    `verter_session` string-surgery in the compile / IDE path
    (`crates/verter_session/src/host_resolve/virtual_file_pipeline.rs` + the compile
    lane); add regression coverage that the removal preserves codegen. Honour
    CodeTransform-as-single-source (no post-`build_string()` rewrites).
  - **D-custom_elements — `customElement` host reach.** The `customElement` handling
    surfaces in `host_resolve/virtual_file_pipeline.rs` + `types.rs`; needs
    regression coverage that the reach preserves codegen.
  - **L-event-args — imported-component event-arg payload resolution under tsgo.**
    The codegen is already byte-exact; the REMAINING gap is resolution: an imported
    component's `InstanceType<typeof C>["$props"]` resolves to `any` under tsgo, so
    a component-spread `$event.<member>` completion cannot expose members. Fix is a
    host/API edit to the cross-component `.vue.ts` synthesized-instance-type
    materialization this block owns; once the host materializes imported-component
    instance types under tsgo, the skipped real-provider rows assert live.
- **Framework-aware import placement (DEFERRED design, tracked).** Auto-import edit
  placement is Vue-`<script setup>`-hardcoded + string-based in `verter_lsp`, so `.svelte`
  and plain-`<script>` Vue get no/invalid auto-import. The codex-architect-recommended fix is
  a framework-neutral `FrameworkImportPlacement` capability in `verter_session::framework`
  (registry-dispatched, typed `ImportPlacementOutcome`, neutral parse-artifact root/block
  layout, Vue + Svelte impls, LSP delegation) — captured in
  `docs/arch/framework-import-placement-design.md`. It is a shared placement primitive that
  `B.7` (candidate sources + auto-imports) and `B.8` (organize-imports) CONSUME, not reinvent.

---

### U15 — Integrations, Ignored-Test Lift, Bench Schema

- **Source track:** MERGED terminal (semantic-graph Phases 6/7/8 + cache-runtime
  B12).
- **Parity docs:** child `docs/arch/native-typeinfo-parity-adapters-final-lift.md`
  (the U14 / U15 framework-adapter + integrations + final-lift blocks), under parent
  `docs/arch/native-typeinfo-parity.md` (terminal acceptance §10.5 / §12).
- **Scope:** Zod/schema client helpers; LSP hover→graph+display,
  completion→framework-surface, MCP `typeinfo.*` / `component-meta.*` tools,
  playground type explorer; lift **EVERY one of the 362 `IgnoredTestRow`s to
  `Lifted`** on the U0 manifest schedule — **zero remaining parity `#[ignore]`s**.
  - **Framework-adapter re-base closure (final-lift checklist).** Because the
    framework-adapter substrate landed ahead of order, the final-lift sweep additionally
    REQUIRES, as a closure check: ZERO provisional pre-U8 framework wire remaining (no
    embedded-`SemanticTypeGraph` framework-surface field — the post-U8
    `TypeInfoGraphPayload` carrier is the only shape); NO old-shape `graph_export` producer
    left (the U12 cut-over leaves no dual path); the framework-surface cache CONSOLIDATED
    onto `ProjectTypeStore` (per U10, off the registry rows); TRUE singleflight in place for
    framework-surface results (per U10); and BOTH Vue and Svelte registered on the final
    graph path. These verify the U8/U10/U12/U14 re-base obligations actually closed end to
    end; they are a terminal checklist, not new work this block originates.
  - **Hover / completion / signature-help delegation STAYS near-term.** U15 keeps
    delegating hover / completion / signature-help to tsserver/tsgo for the editor surface;
    that is the NEAR-TERM and is NOT removed here. The FUTURE native successor is
    `N1.NATIVE_LANGUAGE_SERVICE_LAYER` + `B.7` (Completion/Hover/Signature-Help Semantics),
    a LATER phase that supersedes this delegation progressively through the one resolver —
    see §0.5.4 (`B.7`) and the succession note §0.5.5.
  The ONLY permitted residual `#[ignore]`s are the registered Svelte/React
  STOP-gate files (`svelte_adapter_stop_gate.rs`, `react_adapter_stop_gate.rs`),
  which are NOT among the 362. The binding total is exactly 362 `IgnoredTestRow`s
  (count-guarded + bijective with the source `#[ignore]`s; the ~384 figure is the
  raw `#[ignore]` SITES before macro-family collapse, and `AdditionalProofRow`
  coverage fixtures are excluded from the count); do NOT hard-code a stale absolute,
  and do NOT settle for a majority/fraction — terminal acceptance requires all 362
  `Lifted` (parent §10.5 / §12 `all_typeinfo_parity_rows_lifted_except_stop_gates`).
  Plus the B12 typed bench schema: `BenchResultRow`
  (`packages/benchmark/src/cache-runtime-bench.ts`) reporting cache mode /
  source-map policy / batch shape / thread count / hit count / fallback count;
  vendored cm corpus benches (`component_meta_cold` / `_warm`);
  `MAX_TEST_TIMEOUT` + `test_support/timeout.rs`. Plus the PART 1 §6.2
  **performance-contract benches as perf-regression-gated TERMINAL ACCEPTANCE**
  (not merely the functional gate): the **Verter-vs-TS/tsgo fixtures**
  (`packages/benchmark/src/verter-vs-tsgo-bench.ts`) running Verter and TS/tsgo over
  the SAME in-scope semantic queries (component-meta resolution, projected typeinfo,
  IDE hover/completion, selected member expansion off a large surface, the
  `ReturnType<typeof f>["b"]` demand-slice case), each reported via `BenchResultRow`;
  and the **per-family fallback-bound benches** (`test_support/perf_contract.rs`)
  declaring each query family's (`FlowReturn` / `ResolveCall` / `Relate` /
  `Instantiate` / `Conditional` / `MappedType` / `ResolveClassSurface` /
  `ApparentType` / `TemplateLiteralReduce` / the projection·demand-lattice families)
  fallback-count bound on the vendored corpus and FAILING the bench when a family
  exceeds it. The reserved native-checker seams (`SemanticQueryValue::DiagnosticAnalysis`,
  the `Check*` names, `ExecutableRegionId`, `ProgramAnalysisContributor`) stay
  reserved-not-live; the sibling `docs/arch/native-checker.md` is the LATER non-parity
  layer over the same resolver (NOT part of the U0–U15 parity scope).
- **Deps:** all code-producing blocks (U0–U14).
- **Parallelism:** terminal — runs last.
- **Risk:** large aggregate; lower per-item risk (mostly integration + un-ignoring
  + gating).
- **Required deletions:** any remaining legacy entry-point names surfaced by the
  final sweep.
- **Guards:** the extended two-table-ledger guards (the backing guards on the
  A0a-landed `typeinfo_ignored_test_manifest.rs`): `ignored_test_row_table_holds_exactly_362_rows`
  (binding-total + table disjointness), the exact-362 count/bijection guards
  (`total_ignored_typeinfo_test_count_matches_expected` +
  `manifest_length_matches_documented_total` track the DERIVED live
  `count(status == Ignored)` row state, not a hard-coded stale absolute and not a
  tracked-cursor/lease transaction), and the terminal
  **`all_typeinfo_parity_rows_lifted_except_stop_gates`** (asserting every one of the
  362 `IgnoredTestRow`s is `Lifted` and the only residual `#[ignore]`s are the
  registered Svelte/React STOP-gates); `merged_interfaces_across_files` 5-property
  test (§9.5) green; Svelte/React STOP-gate guards
  (`svelte_adapter_stop_gate_is_registered_out_of_scope`,
  `react_adapter_stop_gate_is_registered_out_of_scope`); the perf-contract guards
  `bench_result_row_reports_cache_mode_sourcemap_batch_thread_hit_fallback` +
  `architecture_minimizes_fallback_entry_not_fallback_cost` (the PART 1 §6.2
  governing-rule guard — per-family fallback ENTRY count under bound, warm path
  O(validate); perf-regression-gated terminal acceptance); `no_vacuous_parent_u_block_landing`;
  hermeticity guard (`external_corpus_paths_not_present_outside_gated_tests`) — bench
  corpora are vendored, no `.integration-tests/repos/<third-party>/`.

---

## 5. U2 in depth — B4 ↔ SemanticQueryKey co-sequencing (§B)

**Binding decision (§B): merge Block-4 and the seven-variant `SemanticQueryKey`
spine into a single block, U2. The spine variants were NOT staged on the
(now-deleted) intermediate `DeclKey` shape and then re-migrated to slot
identity.** One clean cutover, no migrate-twice. (LANDED: `Instantiate.base` /
`ResolveMacroPayload.owner` now key on `ResolvedDeclSlotIdentity`; the `DeclKey`
struct and `to_decl_key()` were deleted in the cutover.) Of the seven spine
variants, **five are landed** as live `SemanticQueryKey` variants and **two
(`ResolveMergedDeclaration` / `ResolveDeclarationAugmentation`) are
forward-planned** (parent §2.1) — owned by the later `U2.MODULE_AUGMENTATION`
reducer block and NOT in the live enum or spec table.

U2 finalizes the **`SemanticQueryKey` identity SHAPE once** (the slot-identity
model). It does NOT freeze the variant LIST — later ADDITIVE variants land in this
same shape with no cache re-key (notably U6's `SemanticQueryKey::FlowReturn`, B11).
What U2 fixes once is the identity model every variant keys on:

1. **Existing variants → slot identity (LANDED).** `Instantiate { base }` and
   `ResolveMacroPayload { owner }` moved from the intermediate content-free
   `DeclKey { canonical_id, decl_name }` to `ResolvedDeclSlotIdentity`; the
   `DeclKey` struct and the `to_decl_key()` / `DeclIdentity::to_decl_key` helpers
   were DELETED in the same cutover. This was the last identity refinement; the
   whole-hash R6 violation was already resolved (§2.2), so it was a slot-precision
   change, not a re-key from scratch.

2. **Seven-variant spine: five landed, two forward-planned.** The five landed
   spine variants — `ResolveAmbientNamespace`, `ResolveOverloadSet`,
   `ResolveEnum`, `FlowNarrowingAt`, `ContextualTypeAt` — are each added directly
   in the slot-identity shape and dispatched through
   `ProjectSemanticDispatch::execute` (the one-engine rule). The remaining two —
   `ResolveMergedDeclaration` and **`ResolveDeclarationAugmentation`** — are
   **forward-planned** (parent `native-typeinfo-parity.md` §2.1: "five landed,
   two forward-planned"), owned by the later `U2.MODULE_AUGMENTATION` block and
   NOT in the live enum or spec table. The forward-planned seventh variant is the
   **generalized** augmentation key: the former `ResolveModuleAugmentation` is
   broadened to `ResolveDeclarationAugmentation { target: Module | Global, context:
   DeclarationAnalysisContext }` so module AND global declaration-environment-mutation
   facts share ONE concrete identity (parent `native-typeinfo-parity.md` §2.1–§2.2).
   This is an existing-slot generalization, **not a sixth U2 variant** — the
   END-STATE slot count stays seven. Today, same-name declaration merge and
   cross-file ambient augmentation already work via the live `MergedDecl`
   peer-merge reducer over `SemanticNodeData::MergedDecl` (CLAUDE.md "Declaration
   Merging / Augmentation (CRITICAL)") — NOT a dedicated query key — so the
   value-bearing merged / ambient type-value surfaces exist now. The
   forward-planned `ResolveMergedDeclaration` / `ResolveDeclarationAugmentation`
   query keys AND the formal augmentation parity rows — which resolve to the
   in-process `SemanticQueryValue::DeclarationAnalysis` value domain (wire home
   stays `GraphTypeNode` kinds 23/25) — land together later in the reducer block
   (`U2.MODULE_AUGMENTATION`); the rows do NOT lift without that new key. When
   the forward-planned augmentation work lands, its type-value /
   `SemanticQueryValue::DeclarationAnalysis` producers land with it (§9.5
   cross-file merge completeness).

   **Five added keys beyond the seven (parent §2.3) — register them in this same
   slot-identity shape.** `ResolveClassSurface`, `ApparentType`, and
   `TemplateLiteralReduce` land HERE (U2); `FlowReturn` and `ResolveCall` land in U6
   as additive variants in the identical shape (no cache re-key). `Relate` is an
   existing-key upgrade to its full relation identity (parent §2.7), not a sixth
   added key. The added-key count is exactly five; every key routes through
   `ProjectSemanticDispatch::execute`, carries an explicit R21/R6-clean per-key
   `*Context`, and resolves to its correct `SemanticQueryValue` value domain (type
   keys → `TypeNode`; `ResolveDeclarationAugmentation` → `DeclarationAnalysis`;
   `FlowReturn` / `ResolveCall` / `Relate` → their typed result domains) — no
   non-type value is smuggled into `GraphTypeNode`.

3. **Semantic + component-meta caches onto `QueryNode` / `ArtifactNode` against
   the same key model.** The query-identity caches owned by the semantic track —
   `SemanticGraphStore` (family / relation / named-type, parameterised over ALL
   `SemanticQueryKey` variants), `ComponentMetaResultDb`, `MaterializeStructureDb`,
   `RefCycleResultDb`, `ShapeCacheDb` — plus the content-addressed artifact caches
   (`FileArtifactStore`, `ResolvedImportFacts`, typed-IR resolve, member fact
   stores, `ModuleAugmentationIndex`) become `QueryNode` / `ArtifactNode` impls
   keyed by the final model. Query-identity keys carry NO content/version hash or
   `fact_dep_signature`; content-addressed keys carry `content_hash` /
   `parse_stable_hash`. The five env-hash dimensions stay split (R21);
   `lib_env_hash` enters only the caches that depend on lib data.

This is why U2 is the convergence gate and the highest-correctness-risk block:
the same caches are re-keyed exactly once, the one shared resolver gains its
final variant set in one cutover, and every downstream consumer (the wire-surface
closure U8, the result DB U10, the exporter U12, the Vue adapter U14) builds on the
final key model.

---

## 6. TypeInfoGraphResultDb admission fork (§C)

**Binding decision (§C): `TypeInfoGraphResultDb` admission is singleflight NOW,
with NO later retarget to `submit_dag`.**

The typeinfo result DB (built in U10) admits through the cache-runtime
**singleflight / fact-validation substrate**, which is already landed:

- `cooperative_admit_with_post_publish` (`cache_runtime/singleflight.rs`)
- `InflightTable` (`cache_runtime/singleflight.rs`); the canonical
  `MAX_INFLIGHT_RETRIES = 3` lives in `semantic_query_memo/inflight.rs`
- `BoundedCandidateMap` + `GlobalRetentionBudget` (`bounded_query_retention`)
- `FactReadSet::finalise` → `SignatureAdmission`
- `HostStoreView` for warm-hit revalidation

The B7e cache-node DAG (`submit_dag`, U7) is **scheduler execution / readiness
plumbing — not a second cache-admission authority.** The typeinfo DB must never
be folded into `CacheNodeDag` / `submit_dag`, now or as a future migration.

**Consequence for sequencing:** because U10's admission binds to the
already-landed singleflight path, the entire semantic-graph execution lane (U8 →
U3 → U10 → U12 → U11/U13 → … ) is **independent of the remaining scheduler work
(U1, U7, U9)** and proceeds on its own dependency-parallel lane after the U2 gate. The only
forward-coupling the original plans worried about — retargeting the typeinfo DB
onto a future `submit_dag` — is explicitly ruled out here.

---

## 7. Verification baseline & known failures

Run after EVERY block. The crates are highly interconnected; always run the full
workspace suite, not a scoped subset.

```bash
# Rust
cargo check --workspace --tests
cargo clippy --workspace -- -D warnings
cargo fmt --all -- --check

# CANONICAL Rust gate — the CLAUDE.md pair (completeness + shared-process surface)
cargo nextest run --workspace --no-fail-fast     # authoritative completeness gate — runs
                                                  # every workspace test target INCLUDING
                                                  # the ~25 verter_session integration
                                                  # binaries; --no-fail-fast so one early
                                                  # failure cannot mask downstream ones
cargo test -p verter_session --tests              # shared-process verter_session surface
                                                  # (one run covers all binaries since the
                                                  # `mod harness;` consolidation)

# Focused scheduler (every block that touches it)
cargo test -p verter_scheduler --tests

# TS + full build (gates wasm cfg-gating that --tests cannot catch)
pnpm install --frozen-lockfile
pnpm test
pnpm build                                        # native → lsp → wasm → ts
```

**Gate-method notes (banked from prior landings):**

- **CLAUDE.md is the gate-method authority; this plan defers to it.** The
  canonical Rust gate is the PAIR above (`cargo nextest run --workspace` +
  `cargo test -p verter_session --tests`). Bare `cargo test --workspace --tests`
  SILENTLY SKIPS the consolidated `verter_session` integration binaries
  (feature unification drops them from the workspace test set) and MUST NOT be
  the sole gate. Any real, reproducible environment-specific caveat against
  nextest must be recorded in CLAUDE.md / the `/testing` skill (the
  current-state authorities), never carried as a silent fork in this plan.
- Default `cargo nextest run` is fail-fast; gate with `--no-fail-fast` so a
  single early failure cannot cancel the run and hide downstream failures.
- `pnpm build` is a required gate: the wasm `cfg`-gating breaks have surfaced
  ONLY under `build:wasm`, never under `cargo --tests`.
- Trust-but-verify: re-run the full gate independently of any sub-agent's
  `tail -N` summary.

**Expected outcome for every block:** **ZERO new failures** over the baseline
RE-DERIVED at that block's entry gate at the then-current tip (§1) — not a count
frozen to the old `b36e0835` land point. The recorded historical reference is the
8-failure cluster at `b36e0835`; implementation re-derives the live baseline at each
block's entry (this plan does not run the workspace). The `typeinfo_ts_bindings_*`
env-only failure is a non-failure on the main checkout (passes with `node_modules`
present).

`sixteen_cold_concurrent…attribute_per_joiner_contract` was a load-flake earlier
in the chain and is now stable (fixed at `27c25a7a`); treat any recurrence under
pathological oversubscription as a known load-flake, not a new regression, and
confirm 3/3 in isolation.

**JS known-failure debt — 29 pre-existing component-meta/native-eval specs.**
A 29-name set of `packages/component-meta` native-eval/checker specs is red at
the METAXP merge-base `4beb6d067` (verified by a like-for-like full-suite
base-vs-tip run, 2026-06-10: failing-name sets byte-identical, zero tip-only
names). Pre-existing display/compat parity gaps (raw-type preference, registry
materialisation, indexed-access helpers, native-payload sidecar surfaces) —
tracked debt, NOT part of XP.2/XP.3 and not a METAXP regression. Treat these
29 names as the JS baseline until a dedicated parity block lifts them; any
NEW failing name on a block tip is a real regression.

**Exposed-surface debt (XP.2 follow-ups, terse).**

- `ExposedAnalysis` lacks a scope carrier for its `type_expr` (asymmetric with
  props' `shallow_type_expr_scope`) — an unanchored-`Ref` trap if a future
  consumer demand-walks a published exposed type. Fix at the producer: carry
  the scope alongside the expr, mirroring the props pair.
- Literal-overlap expose members (`defineExpose<T>({ a })`): the object-literal
  field's binding/eval type takes precedence and ignores the available raised
  `resolved_field.type_expr` even when binding/eval yields `Unknown { raw }` —
  the surface-raised type should backstop an unknown literal-side type.

**Framework-adapters merge debt (merged ahead of order, 2026-06-17).** The
`feat/framework-adapters-clean` merge (`9e10f2324`) landed U8/U12/U14-shaped
substrate (typeinfo framework-surface wire + producer + `FrameworkAdapterRegistry`
/ thin adapters, Vue + Svelte) AHEAD of the hard gate `S5.B11/B12 → U8`, over the
still-live `VueMacroElements` / `type_surface` sidecar (historical record of the
ahead-of-order landing — kept here so the divergence is not lost). The re-base
obligations are no longer bolt-on notes: they are ABSORBED into the affected block
bodies as those blocks' true post-merge scope — the provisional pre-U8
`FrameworkSurfacePayload` / embedded-`SemanticTypeGraph` wire retag is folded into U8
(and §3.1.1's sidecar-cutover surface), the old-shape `graph_export.rs` producer
cut-over into U12, the off-`ProjectTypeStore` `FrameworkSurfaceStore` cache
consolidation + true singleflight into U10 (with U3 making the off-store residence
visible to the rail), and the registry/adapter re-wire into U14; U13/U15 close the
projection + final-lift ends. Engine-compatible (the merge routes through the one
shared resolver); the obligations are owned by those named block scopes, not a new
block and not a separate note set.

---

## 8. Documentation-update map

After landing each block, update the OWNING documentation (skill that owns the
module/API; `CLAUDE.md` only if a summary or skill pointer changes; `AGENTS.md`
if skill routing changes; `docs/` for API/guide pages; inline rustdoc/JSDoc on
changed signatures). Every new CRITICAL rule lands with a static guard or a
discriminating regression test in the same change (R6 meta-guard).

**Intentional current-state-authority divergence (NOT pre-edited).** `CLAUDE.md`
(the Cache Architecture (CRITICAL) rule) and the `/type-cache-architecture` skill
currently describe the LIVE uniform `FAMILY_SLOT_CANDIDATE_CAP = 4` + FIFO eviction
model. That is the one live intentional divergence from this plan's end-state: it
describes the code as it exists TODAY and is deliberately NOT pre-edited — making the
current-state authority docs describe unbuilt state would make them lie about the live
code. It is updated to the per-family-adaptive `candidate_cap()` +
invalid-first/LRU-by-valid-hit eviction + global memory ceiling by U3's `Docs updated`
step WHEN U3 lands. The divergence is therefore an intentional, tracked deliverable,
not an oversight. (The query-mode / satisfaction model is NOT a divergence: `CLAUDE.md`
and the `/type-resolution` skill already document the landed materialized-point
satisfaction — recorded `(path, point)` dominance via `cached_satisfies`, with the
demand lattice as the algebra and the five modes as presets. The
`U2.QUERY_VALUE_DOMAIN` / U10 `Docs updated` steps REFINE that text; there is no live
"mode-rank model" left to replace.)

| Block | Primary docs to update |
|---|---|
| **U0** | `/type-resolution` (typeinfo contract surface); the reconciled A0a-landed `tests/typeinfo_ignored_test_manifest.rs` manifest (schema notes); `/audit-infrastructure` (`AuditedResult`). |
| **U1** | `/scheduler` SKILL (TaskKind split, `execute_cache_node`); `/host-session` if dispatch surface changes. |
| **U2** | `/type-resolution` + `/type-cache-architecture` (final `SemanticQueryKey` surface, slot identity, B4 node enumeration, R21 key composition); `CLAUDE.md` project-global-cache + macro-traversal summaries; `docs/arch/fact-based-cache.md` per-cache key tables. |
| **U3** | `/type-cache-architecture` (invalidation authority, no reverse-dep eviction; **the per-family adaptive `candidate_cap()` + invalid-first/LRU-by-valid-hit eviction + global memory ceiling, replacing the uniform `FAMILY_SLOT_CANDIDATE_CAP = 4` FIFO**); **`CLAUDE.md`** (the Cache Architecture (CRITICAL) `FAMILY_SLOT_CANDIDATE_CAP` / FIFO-eviction text → the per-family-adaptive cap model); `/component-meta` (cache contracts); `docs/arch/fact-based-cache.md` (multi-candidate `FamilySlots` section — already current). |
| **U4** | `/type-cache-architecture` (persistent pure-artifact rules, sealed `PersistentArtifactNode`); `docs/arch/fact-based-cache.md`. |
| **U5** | `/type-cache-architecture` (memory policy, metrics); `/audit-infrastructure` (`StructuredAuditEvent::CacheNode*`). |
| **U6** | `docs/arch/native-flow-return.md` (moved here in U6); `/type-resolution` (`FlowReturn` query node); `/compiler-codegen` if flow lowering surfaces. |
| **U7** | `docs/arch/u7-scheduler-submit-dag-decision.md` (the DEFER decision — multi-node `submit_dag` envelope held un-built). **No `/scheduler` envelope-symbol docs** (`CacheNodeDag` / `submit_dag` / `KeyedJob` / `DagHandle`) are written unless the envelope is re-gated AND built at U9; the deferred-substrate skill refresh rides U1/U9. |
| **U8** | `/type-cache-architecture` (wire-payload notes — `TypeInfoGraphPayload` / `ProgramAnalysisGraph` placement; module/global augmentation stays on the `GraphTypeNode` arms 23/25 — no `DeclarationAnalysisGraph`, relocation rejected); `Typeinfo Wire Contract` rule pointers; amend `docs/arch/semantic-type-graph-plan-recovered.md` stale wire wording. |
| **U9** | `/scheduler` SKILL (cache-node lowering via single-node `SchedulerDag::submit`, the lifted terminal-cache dispatch asserts, `execute_cache_node`, the B7a leaf-primitive deletions — NO session bridge, NO `CpuConcurrencySemaphore` wiring, per the DESIGN-LOCKED `docs/arch/u9-session-bridge-design.md`); `/host-session` only if a host-visible surface changes. |
| **U10** | `/type-resolution` (`TypeInfoGraphResultDb`, `CompletionFence`; **the Query Mode Contract → the `ProjectionDemand` / `EvalPolicy` demand-lattice with the five modes as presets — the lattice as the demand/identity ALGEBRA inside the landed materialized-point satisfaction (`cached_satisfies` over recorded materialised `(path, point)`s), which CLAUDE.md and the skill already document; U10 REFINES that text, it does not replace the satisfaction authority**) + `/type-cache-architecture` (result-DB membership on `ProjectTypeStore`). The `U2.QUERY_VALUE_DOMAIN` block lands the demand-lattice DEFINITION; U10 lands the published-boundary exactness gating over the landed satisfaction rail and updates the skill's mode-contract text. |
| **U11** | `/type-resolution` (public `relate` → `RelationPayload`, `_with_audit`, the session surface); `/audit-infrastructure` (3-branch emission, footprint-attachment pipeline, footprint cell, nested records); `/component-meta` if the public relation surface contract changes. |
| **U12** | `/type-resolution` (the exporter + lowering table; the exporter is a thin projection, not a resolver); `/architecture` (FFI surface); `/type-cache-architecture` exporter/payload notes; legacy-deletion notes in the owning skills. |
| **U13** | `/component-meta` (native-vs-compat, the structural `TypeDescriptor` projection) + `/architecture` (projections); `/type-resolution` (typed-schema-contract notes); `@verter/type-ir` schema docs; `docs/` API pages. |
| **U14** | `/component-meta` (native-vs-compat, framework adapter registry, Vue surface). |
| **U15** | `/e2e-vscode-testing`, `/build-and-profiling` (bench schema), `/testing`; `/architecture` (MCP/LSP/playground integration); the unignore manifest's final counts. |
| **B.1** (program model) | `/host-session` (project/program model authority, watch-driven revalidation); `/type-cache-architecture` (`project_identity` keying, no bundled program-hash); `/type-resolution` (program as resolver input). |
| **U0.RESOLVER_CORE** (A.5/B.2, #21) | `/type-resolution` (the full moduleResolution-mode matrix — classic / node10 / node16 / nodenext / bundler — over conditional-exports / imports / paths / baseUrl / typesVersions / rootDirs / typeRoots / types / moduleSuffixes / customConditions / resolveJsonModule / allowImportingTsExtensions / allowArbitraryExtensions, PLUS symlink/`preserveSymlinks` realpath + pnpm/hoisted layouts + workspace-linked package resolution and `package.json`-edit / in-place package-source-edit invalidation); `/type-cache-architecture` (`resolve_env` vs `lib_env` split, R21). |
| **B.4** (stdlib/intrinsics) | `/type-resolution` (lib.d.ts selection pinned to a TS version, JSX-namespace defaults); `/type-cache-architecture` (`lib_env_hash` invalidation contract); `/architecture` (`IntrinsicRegistry` authority). |
| **N0.NAV_LOCATION_INDEX** (A.10/B.3) | `/host-session` (the navigation/location PROJECTION over the pre-`U2` `BinderIdentityFacts` substrate — read-only, zero typed-IR dispatch, writes no query-identity/route fact; native def/refs/rename location production + the `TsgoNavigationBackend` **def/refs/rename** deletion gated before §10 — its `getCodeActions` deletion is `B.8`'s — + the aux `TypeProvider` `get_semantic_tokens`/`get_document_highlights` deletions gated before §10 (call hierarchy already native) + the `nav_location_index_runs_zero_typed_ir_dispatch`, `n0_does_not_write_semantic_query_identity_or_route_facts`, `native_navigation_replaces_ts_navigation_backend` & `native_binder_surfaces_replace_ts_aux_nav_paths` guards); `/architecture` (doc/workspace symbols, rename ranges, document highlights, semantic tokens, call hierarchy); `docs/arch/goto-definition-overhaul-plan.md` (nav-surface integration + `TsgoNavigationBackend` retirement). |
| **N1.NATIVE_LANGUAGE_SERVICE_LAYER** (A.11/B.6) | `/host-session` (request snapshot/cancellation/degradation orchestration; the `textDocument/typeDefinition` nav-by-type compose + its `TypeProvider::get_type_definition` deletion gated before §10 + the `native_type_definition_replaces_ts_type_definition_path` guard); `/architecture` (LS layering); `docs/arch/goto-definition-overhaul-plan.md`. |
| **B.7** (completion/hover/sig-help) | `/host-session` (the native LS semantics; the U15 tsgo→native succession; inlay hints as a type-display surface + its `TypeProvider::get_inlay_hints` deletion gated before §10 + the `native_inlay_hints_replace_ts_inlay_hint_path` guard); `/architecture`; `docs/arch/goto-definition-overhaul-plan.md`. |
| **B.8** (code actions/refactors/organize) | `/host-session` (action routing; the `TsgoNavigationBackend` `getCodeActions` deletion gated before §10 + the `native_code_actions_replace_ts_navigation_backend_code_action_path` guard); `/architecture`; `/compiler-codegen` (CodeTransform-only edit generation). |
| **B.5** (native checker manifest) | `docs/arch/native-checker.md` (the diagnostic-row manifest, at its own rescope gate — pointer only here; the runtime tsgo `get_diagnostics` / `publish_merged_diagnostics` deletion gated before §10 + the `native_checker_replaces_ts_diagnostics_path` guard). |
| **B.10** (native declaration emit) | `/compiler-codegen` (the `.d.ts`/`.d.ts.map` CodeTransform producer, separate-products map policy); `/component-meta` (SFC component decl output). |
| **B.11** (JSDoc / JS mode) | `/type-resolution` (JSDoc type constructs through the one engine; `{Type}` the sole text exception); `docs/arch/native-checker.md` (checkJs diagnostics); `/type-cache-architecture` (CJS interop in the #21 matrix). |
| **B.14** (replacement acceptance, §10) | `CLAUDE.md` only if a summary/skill pointer changes; the §10 terminal gates are self-contained. |

These rows are land-time deliverables of each block (consistent with the
intentional-current-state-divergence note above) — the skills are NOT pre-edited now; each
block updates its owning docs at its `Docs updated:` step when it lands.

The two original plans (`cache-runtime-overhaul-plan.md`,
`semantic-type-graph-plan-recovered.md`) carry a SUPERSEDED-for-remaining-work
pointer to this doc but are otherwise unchanged (historical/detail reference).

---

## 9. Terminal acceptance checklist

The unified effort is "done" when ALL of the following hold:

- [ ] **U0–U15 all landed** (each implement → triple review → clean re-review →
  land), in the §4 backlog order, with the U2 convergence gate landed before any
  graph-execution block (U8+).
- [ ] **One `SemanticQueryKey` identity shape** (slot-identity) finalized in U2 with
  the five landed U2 spine variants (the two augmentation spine variants
  `ResolveMergedDeclaration` / `ResolveDeclarationAugmentation` forward-planned per
  parent §2.1), the additive U6 `FlowReturn` / `ResolveCall` variants landed in that same shape
  (no cache re-key), every variant dispatched through
  `ProjectSemanticDispatch::execute`; NO `DeclIdentity` and NO content/version
  hash or `fact_dep_signature` in any query-identity key.
- [ ] **All B4 caches** are `ArtifactNode` / `QueryNode` impls on the B2 substrate;
  bespoke reverse-dependent `clear_*` invalidation deleted; validated lazy
  revalidation is the sole invalidation rail.
- [ ] **Unified warm-hit-validity statement** (A.4 / ledger #7) written ONCE; every cache
  family's admission cites that single statement
  (`unified_warm_hit_validity_statement_is_single_rail` green); no family carries a private
  validity oracle. BLOCKS the U3 + U10 gates from counting as passed.
- [ ] **Unified determinism statement** (ledger #7b) written ONCE beside it: same inputs +
  the five split env dims ⇒ identical wire payload, independent of thread schedule,
  singleflight winner, and cache temperature (cold ≡ warm);
  `wire_payload_deterministic_across_schedule_and_cache_temperature` green.
- [ ] **`TypeInfoGraphResultDb`** admits through the singleflight / fact-validation
  substrate (NOT `submit_dag`), with warm-exact-only admission, the canonical
  3-retry fence, no second retry constant, and zero-alloc warm hits.
- [ ] **Scheduler** has the TaskKind split (U1) and the landed cache-node substrate
  under one admission path with no second readiness/ledger structure. The multi-node
  `submit_dag` cache-node DAG envelope (U7) is **DEFERRED to U9 / default permanent
  CUT** (`docs/arch/u7-scheduler-submit-dag-decision.md`) — this checklist does NOT
  require it built; U9 closes the cache-node reachability gap via single-node lowering
  into the existing `SchedulerDag::submit` with NO session bridge and NO
  `CpuConcurrencySemaphore` wiring (the dead B7a leaf primitives —
  `DedupeHook` / `SubmissionResult` / `CpuConcurrencySemaphore` /
  `CpuConcurrencyPermit` / the rich `CancellationToken` — are in U9's Required
  deletions, per the DESIGN-LOCKED `docs/arch/u9-session-bridge-design.md`);
  `dag_arch_guards` and the B7b guards green.
- [ ] **Typeinfo session** exposes the 8 `_with_audit` methods + public `relate()`,
  validate-before-execute, with cold/warm/degraded audit + footprint cell + nested
  records using `exactness_counts: BTreeMap`; every request response echoes the
  negotiated `schema_version` and the advertised supported set is restricted to
  versions with a registered encoder (`every_typeinfo_request_carries_schema_version`,
  `server_supported_versions_have_encoders` green).
- [ ] **FFI/TS** exposes the binary-protobuf typeinfo surface; the schema-version
  downlevel encoders (`encode_typeinfo_payload_for_version` +
  `KNOWN_VARIANTS_AT_VERSION` cumulative-exhaustive table) ship with their guards
  (`known_variants_at_version_rows_are_cumulative_exact_sets`,
  `downgrade_encoder_never_emits_variant_unknown_to_target_version`); ALL legacy
  typeinfo Rust modules, the `GraphBuilder`, the scratch evaluator, and the legacy
  TS/component-meta type-graph files are DELETED (no dual path).
- [ ] **Projections** (display / type-descriptor / json-schema / zod / storybook /
  docs) ship; `TypeDescriptor` is a projection; descriptor-bridge deleted.
- [ ] **`@verter/component-meta`** is a thin `FrameworkSurfacePayload` adapter over
  the graph; the 4 known Vue mismatches are fixed; no second resolver/expander.
- [ ] **Ignored-test lift:** ALL 362 `IgnoredTestRow`s are `Lifted` (zero remaining
  parity `#[ignore]`s) per the U0 manifest schedule — NOT a majority/fraction. The
  binding total is exactly 362 (A0a baseline, count-guarded + bijective; not a
  hard-coded stale absolute and not the ~384 raw `#[ignore]` SITES). The only
  permitted residual `#[ignore]`s are the registered Svelte/React STOP-gate files
  (not among the 362). The extended two-table-ledger guards assert it
  (`ignored_test_row_table_holds_exactly_362_rows` +
  `all_typeinfo_parity_rows_lifted_except_stop_gates`); Svelte/React STOP-gate files
  present.
- [ ] **Bench schema:** `BenchResultRow` reports cache mode / source-map policy /
  batch shape / thread count / hit count / fallback count; cm corpus benches
  vendored and hermetic.
- [ ] **Gate green:** `cargo check` / `clippy -D warnings` / `fmt` clean;
  `verter_scheduler` + `verter_session` + full workspace = ZERO new failures over the
  baseline re-derived at this block's entry gate (§1 records the historical 8-failure
  cluster at `b36e0835`; the live baseline is re-derived at the then-current tip);
  `pnpm test` + `pnpm build` (native → lsp → wasm → ts) fully green;
  `pnpm install --frozen-lockfile` in sync.
- [ ] **Architecture clean:** every new CRITICAL rule has a registered guard (R6
  meta-guard green); `no_phase_archaeology_in_production_code` green;
  one-engine / typed-IR-only / shallow-by-default / CodeTransform invariants
  intact.

---

## 10. Full-replacement terminal acceptance (the ENDGOAL done-bar)

**Fenced from §9 (binding).** **§9 is the typeinfo-parity INCREMENT done-bar** (the
`U0`–`U15` blocks + the 362-row lift); **§10 is the full-TypeScript-replacement done-bar**
(the §0.5 foundation / language-service / emit blocks). **Neither loosens the other:** §9
passing does NOT imply §10 passes, and §10 does NOT relax any §9 gate. §10 is reached only
AFTER §9 is green AND the native checker + `N1` language service + emit blocks have landed.

The full-replacement effort is "done" when ALL of the following hold:

- [ ] **(1) No runtime TypeScript at query time** — there are NO runtime calls into a
  TypeScript service / compiler (tsserver / tsgo / `tsc`) on any query path; every type,
  location, diagnostic, completion, and **Verter-OWNED** emit answer comes from native Verter
  surfaces. (Commodity, type-independent transpile emit — `.ts`→`.js` lowering, helpers, import
  elision, JSX modes — is the permanent non-goal deferred to `tsc`/swc/esbuild per §0.5.2, and is
  explicitly OUTSIDE this bar; "Verter-OWNED emit" = the checker + LS + `.vue` compilation + `.d.ts`
  declaration emit that project over the one resolver.)
  Guard `no_runtime_typescript_calls`. **(tsgo stays the OFFLINE parity oracle (§3.4) — §10
  forbids RUNTIME TS, NOT the oracle harness, which runs out-of-band at the rescope gates.)**
- [ ] **(2) Native checker manifest green + tsgo diagnostics path retired** — `B.5`'s
  diagnostic-row manifest is complete and bijective with the emitted diagnostics
  (`checker_diagnostic_manifest_bijection`), gated by the native-checker `tsgo`-oracle baseline at
  its own rescope gate; AND the live tsgo diagnostics path (`TypeProvider::get_diagnostics` /
  `publish_merged_diagnostics`) is DELETED, asserted by `native_checker_replaces_ts_diagnostics_path`
  (a bijective manifest is compatible with a still-merging tsgo path, so the deletion is a SEPARATE
  bar — §0.5.4 B.5).
- [ ] **(3) Native language-service manifest green** — the FULL LS surface (§0.5.2 role
  partition) passes its native manifest with zero residual runtime-TS path: `N0` (def / refs /
  rename / document symbols / **document highlights / semantic tokens / call hierarchy**, zero
  typed-IR dispatch) + `N1` (orchestration + **`textDocument/typeDefinition`** nav-by-type) + `B.7`
  (completion / hover / signature-help / **inlay hints**, every type query through the one resolver)
  + `B.8` (code actions / refactors / organize-imports). ALL in-scope tsgo delegation across these
  surfaces — hover/completion/sig-help behind `B.7`, AND the aux `TypeProvider` paths
  (`get_semantic_tokens` / `get_document_highlights` / `get_inlay_hints` / `get_type_definition`)
  per their per-producer deletions (§0.5.2/§0.5.4), with call hierarchy already native — is FULLY
  retired; any
  residual runtime-TS path on ANY of these surfaces means §10 fails (consistent with the absolute
  item (1) bar + the item (7) deletion sweep). The PROGRESSIVE surface-by-surface cutover that gets
  there lives in §0.5.5, not in this terminal bar. **The "native manifest" here is the generated
  `language_service_api_manifest` (§0.5.2)** — every public `ts.LanguageService` method (pinned to the
  §3.4 / `B.4` TS version) enumerated and mapped to its role-owner (`N0`/`N1`/`B.7`/`B.8`/`B.5` — the
  diagnostic methods are checker-owned — or `B.10` for the `getEmitOutput` `emitOnlyDtsFiles`
  declaration-emit facet; the commodity JS-emit facet is the §0.5.2 transpile non-goal) or a named
  non-goal, gated by
  `language_service_api_manifest_covers_full_surface`; an unmapped or runtime-TS-backed surface fails
  §10 (the coverage claim is mechanized, not prose).
- [ ] **(4) Declaration-emit parity OR REGISTERED STOP-GATE non-goal** — `B.10` `.d.ts` /
  `.d.ts.map` output matches the `tsgo`-oracle baseline for in-scope inputs (incl. `.vue`
  components), OR the specific construct is a **registered declaration-emit STOP-gate** — NOT a free
  "explicit non-goal" escape hatch. Mirroring the §9 Svelte/React stop-gate discipline
  (`svelte_adapter_stop_gate.rs`, guard `*_is_registered_out_of_scope`), every declaration non-goal
  is a `decl_emit_<construct>_stop_gate.rs` row carrying: an OWNER, the PUBLIC degradation behavior
  (what the consumer sees for that construct), the reason, and an `*_is_registered_out_of_scope`
  guard, and is EXPLICITLY EXCLUDED from the replacement-acceptance set — so "full replacement"
  cannot pass §10 with a silent declaration hole. General transpile emit is the permanent non-goal
  (§0.5.2). Guards `declaration_emit_derives_from_typed_facts_via_codetransform` +
  `declaration_emit_non_goals_are_registered_stop_gates` (NET-NEW, named deliverable at the `B.10`
  gate per §0.5.7).
- [ ] **(5) Resolver / config parity green** — the `#21` module-resolution matrix
  (`U0.RESOLVER_CORE`) + the tsconfig semantics matrix (`A.3`/#20 strict + `B.12` config)
  pass their `tsgo`-oracle baselines; `module_resolution_keys_on_resolve_env_not_type_or_lib`
  + `reducers_branch_on_strict_family_not_only_key` green.
- [ ] **(6) Perf + memory budgets green at scale** — the per-family `#16` interactive-latency
  SLOs (p50 / p95 on the MISS path) and the memory budgets hold on the full-scale corpus, not
  only the vendored slice.
- [ ] **(7) Deletion sweep — no dual path** — every TS-backed entry point that the native
  surfaces replace is DELETED (no compatibility shim, no second resolver/expander, no fallback
  to a TS service); the ownership statement (§0.5.1) holds with no surface owning a second
  authority. This includes BOTH halves of `verter_lsp::tsgo::TsgoNavigationBackend`: its
  def/refs/rename location paths (deletion owned by `N0`, §0.5.4) and its `getCodeActions` path
  (deletion owned by `B.8`, §0.5.4); AND the live tsgo `TypeProvider` LS paths, each deleted by its
  producing block (§0.5.2 per-surface deletion-ownership rule): `get_semantic_tokens` /
  `get_document_highlights` → `N0` (call hierarchy already native, nothing to delete);
  `get_inlay_hints` → `B.7`; `get_type_definition` → `N1`;
  `get_diagnostics` / `publish_merged_diagnostics` → `B.5` — each gated
  BEFORE `§10` by its owning block, with no §10-deferred carve-out and nothing left to this sweep's
  catch-all. The sweep AUDITS that none survive; it does not own removing them. Ties ledger #19's
  deletion-sweep scope.
- [ ] **(8) Architecture clean (carried from §9, not loosened)** — every new `(CRITICAL)`
  rule from the §0.5 blocks has a registered R6 guard;
  `ownership_boundaries_no_typescript_side_path`,
  `broken_code_recovery_contract_global_every_surface_degrades`,
  `engine_fault_containment_panic_never_admits_never_poisons_degrades_typed`,
  `program_model_is_single_project_authority_keys_through_project_identity`,
  `lib_authority_pinned_ts_version_single_owner`,
  `binder_identity_facts_are_pre_u2_and_not_n0_owned`,
  `u2_queries_do_not_read_n0_navigation_indexes`,
  `n0_does_not_write_semantic_query_identity_or_route_facts`,
  `declaration_slots_are_stable_symbol_space_scoped_facts`,
  `merge_order_and_augmentation_contributor_order_are_fact_validated`,
  `ambient_global_and_lib_corpus_have_completeness_facts`,
  `negative_name_lookup_requires_recorded_completeness_or_returnonly`,
  `binder_scope_id_enters_context_sensitive_query_identity`,
  `session_overlay_augmenter_isolated_from_base_index`,
  `effective_export_set_session_view_stitches_overlay_augmenter`,
  `no_effective_export_set_base_only_session_assert`,
  `nav_location_index_runs_zero_typed_ir_dispatch`,
  `native_navigation_replaces_ts_navigation_backend`,
  `native_binder_surfaces_replace_ts_aux_nav_paths`,
  `native_code_actions_replace_ts_navigation_backend_code_action_path`,
  `native_type_definition_replaces_ts_type_definition_path`,
  `native_inlay_hints_replace_ts_inlay_hint_path`,
  `native_checker_replaces_ts_diagnostics_path`,
  `language_service_layer_does_not_write_caches_or_import_private_reducers`,
  `completion_semantics_route_types_through_one_resolver`,
  `declaration_emit_non_goals_are_registered_stop_gates`,
  `language_service_api_manifest_covers_full_surface`,
  `jsdoc_and_js_mode_resolve_through_one_engine_jsdoc_payload_only_text` all green; the
  one-engine / typed-IR-only / shallow-by-default / R21 / wire-purity / CodeTransform
  invariants intact.
