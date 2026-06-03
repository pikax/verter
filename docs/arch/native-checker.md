# Native Checker — Diagnostics as a Later Layer Over the One Resolver

> **Relationship:** this is a **sibling follow-up plan**, sequenced **after** the
> native typeinfo parity architecture (`docs/arch/native-typeinfo-parity.md`, the
> `U0`–`U15` blocks). Typeinfo parity is the **foundation**; the native checker is a
> **later layer** over the **same** resolver
> (`SemanticQueryKey → ProjectSemanticDispatch::execute → SemanticGraphStore`). It is
> **not** part of the 363-row typeinfo parity blocks and does not change them.
>
> **Foundation:** `docs/arch/native-typeinfo-parity.md` (the engine architecture, the
> typed `SemanticQueryValue` value domain, the relation / call / flow / contextual
> fact authority, the `ExecutableRegionId` reservation, and the
> `ProgramAnalysisContributor` injection seam). `docs/arch/native-flow-return.md` (the
> per-function `FunctionFlowGraph` + flow IR). `docs/arch/fact-based-cache.md` (the
> `FactDomain::ProgramAnalysis` domain and `flow_body_stable_hash`).
>
> **Reserved seams this plan realizes.** The typeinfo blocks **reserve** the seams a
> native checker lands on but do **not** build the checker: the value-domain arm
> `SemanticQueryValue::DiagnosticAnalysis(CheckResult)`, the `Check*` query names, the
> `ExecutableRegionId` / `ExecutableRegionKind::Function` region abstraction, and the
> `ProgramAnalysisContributor` / `SemanticContribution` injection seam (parent §3, §5).
> This plan is where those reservations become live work — as a clean addition, never
> a re-shape.

The native checker produces TypeScript-grade diagnostics from the **same** semantic
facts the typeinfo engine already computes. It has the same **one resolver** as
typeinfo: `SemanticQueryKey → ProjectSemanticDispatch::execute → SemanticGraphStore`.
There is no second checker walker, no checker-specific resolver, no OXC query-time
resolver, no tsserver/tsgo execution path, no diagnostic projection-repair path, and
no TS-text-based diagnostic path. OXC stays the syntax/lowering front-end only.

---

## 1. Scope and relationship

Typeinfo parity answers "what is the type of X" demand-sliced (parent §5). The native
checker answers "what is wrong with this region / file / program" — the diagnostic
surface (assignability errors, call-applicability errors, unreachable code,
control-flow errors, missing-return / definite-assignment errors, contextual-typing
mismatches, and the rest of the TS diagnostic catalogue). These are different
questions, but they read the **same** facts:

- typeinfo parity is the **foundation** — it lands the one resolver, the typed
  `SemanticQueryValue` value domain, and the relation / call / flow / contextual fact
  producers the checker consumes;
- the native checker is a **later layer over the same resolver** — its query keys
  route through the **one** `ProjectSemanticDispatch::execute` dispatch, and its
  diagnostics derive **from** the existing relation / call / flow / contextual facts,
  not from a second engine that re-walks types.

This sequencing is deliberate. A checker built before the typeinfo facts are stable
would grow a parallel type-walking path to fill the gaps; a checker built **after**
the facts are stable is a thin layer that reads them. The typeinfo blocks already
reserve every seam the checker needs (parent §3, §5), so the checker is an additive
block, not a re-shape of the typeinfo engine.

This plan is **not** part of the 363-row parity scope (parent §10.4.1). It does not
modify the `U0`–`U15` blocks, the 363-row partition, the seven `AdditionalProofRow`s,
or any typeinfo manifest. Diagnostic parity gets its **own** future manifest
(§6) — the 363-row typeinfo manifest is never expanded into a checker-parity manifest.

---

## 2. The checker query layer

Diagnostics are **first-class semantic-query results**. They are never stuffed into a
`GraphTypeNode` arm and never carried on a payload side table as an identity-less
side product. The reserved value-domain arm
`SemanticQueryValue::DiagnosticAnalysis(CheckResult)` (parent §3) is their home:
every checker query resolves to `DiagnosticAnalysis`, and a `CheckResult` carries the
region's diagnostics plus the read-set fact signature that version-roots the cached
result, exactly like every other query-identity cache.

The future checker query keys — RESERVED in the typeinfo value domain (parent §3),
landed here — are:

| Key | Scope |
|---|---|
| `CheckProgram` | every checked file in the project, joined |
| `CheckFile` | one file's full diagnostic surface |
| `CheckRegion` | one `ExecutableRegionId` (a function body, a module top-level, a static block, an initializer, …) |
| `CheckExpression` | one expression site (the demand-sliced leaf — the checker analogue of a typeinfo query at a program point) |

with `CheckCall` / `CheckAssignable` / `CheckDeclaration` as the finer-grained keys a
later checker block may add for incremental call-applicability / assignability /
declaration-conformance diagnostics (`CheckProgram` is the program/project-level key —
the parent reserves no separate `CheckProject`). This set is exactly the parent's
reserved `Check*` taxonomy (`CheckProgram` / `CheckFile` / `CheckRegion` /
`CheckExpression` / `CheckAssignable` / `CheckCall` / `CheckDeclaration`, parent §3);
the exact live subset is settled when the checker lands; what is fixed now is the
discipline below.

Every checker key is added as **enum variant + `SemanticQueryKeySpec` row + dispatch
behavior together**, in the already-final slot-identity shape, so the standing
`semantic_query_key_spec_table_equals_enum` meta-guard stays green incrementally when
the checker lands (parent §2.3). Each key carries a content-free query-identity key
with its split env hashes on a named per-key `*Context` (R21 — never a bundled
`project_config_hash`; R6 — never a content hash, `parse_stable_hash`, or
`fact_dep_signature`), and its `CheckResult` is version-rooted on the cached value via
the recorded `ReadSetSignature.facts`. `lib_env_hash` enters a checker key (a
diagnostic depends on lib-declared surfaces). A diagnostic result that is cancelled,
budget-exceeded, superseded mid-flight, or partial is `ReturnOnly` — never
warm-admitted — exactly as the typeinfo query-identity caches handle non-admission.

**Diagnostics are a value domain, never a `GraphTypeNode` arm.** The
`GraphTypeNode`-purity class is closed (parent §1.3): the published type-values
surface admits only `TypeNode` values. Diagnostics live on
`TypeInfoGraphPayload.diagnostics` / `diagnostic_directives` at the wire surface
(parent §1.5) and resolve through `DiagnosticAnalysis` at the query surface. A checker
that materialised a diagnostic as a `GraphTypeNode` arm would re-open the closed
purity class; the guard below forbids it.

---

## 3. One engine — diagnostics from facts, not from a second walker

The native checker produces every diagnostic **from** the facts the typeinfo engine
already computes — it does not re-derive types to find errors. This is the
one-resolver rule (parent: "exactly one type-resolution engine") and the
typed-IR-only rule (parent: the resolver drives decisions from the typed IR, never
from type text) applied to the diagnostic surface:

- **Assignability errors** read `Relate` (parent §2.7, §4): a diagnostic is the
  negative `RelationPayload` outcome plus its proof, never a re-run of a second
  assignability matcher. The relation proof the typeinfo surface already exposes off
  the type-values surface (parent §1.3, `TypeInfoGraphPayload.relation_proofs`) is the
  diagnostic's evidence.
- **Call-applicability / overload errors** read `ResolveCall` / `ResolveOverloadSet`
  (parent §2.4, §4.2): a diagnostic is the failed-applicability result of the **one**
  call resolver running its speculative `InferenceSession`s, never a checker-private
  overload matcher.
- **Missing-return / unreachable / definite-assignment / control-flow errors** read
  the flow facts — the per-function `FunctionFlowGraph`, the `FlowReturn` result, and
  the `FlowSlice` facts in `FactDomain::ProgramAnalysis`
  (`docs/arch/native-flow-return.md`, parent §5). A control-flow diagnostic is a
  reachability / return-coverage judgement over the **existing** flow graph, never a
  second control-flow walk.
- **Contextual-typing / narrowing diagnostics** read `ContextualTypeAt` /
  `FlowNarrowingAt` — the `ProgramAnalysisGraph` facts (parent §1.3, §3). A
  contextual-mismatch diagnostic is a `Relate` over the contextual target and the
  observed type, both already facts.

The cross-engine recursion the checker inherits (a diagnostic over a call that solves
a callback that narrows an argument) discharges through the **one shared**
`CheckerReentryGraph` (parent §4.2) — the same re-entry / cycle-id space `ResolveCall`,
`FlowReturn`, `ContextualTypeAt`, and `FlowNarrowingAt` already share — not a
checker-private cycle space that could diverge.

Three forbidden patterns, stated as the checker's bug class:

- **No second diagnostic engine beside `Relate` / `ResolveCall` / `FlowReturn` /
  `ContextualTypeAt`.** A checker that walks types to recompute assignability,
  applicability, or reachability is a second resolver — delete it and read the facts.
- **No diagnostic projection-repair.** A diagnostic is never recovered by
  reconstructing or repairing a projected type (the projection-repair anti-pattern the
  typeinfo engine already forbids). The published surface is shallow-by-default; a
  diagnostic reads the typed facts, never a re-materialised projection.
- **No TS-text-based diagnostic path.** Diagnostics are computed from the typed IR
  (`SemanticNodeData` / `TypeExpr` / the `ProgramAnalysisGraph` facts), never from
  source slicing, regex over type text, hand-rolled type-text splitters, or the
  synthesise-then-reparse pattern. The single text exception the typeinfo rule already
  carves out (JSDoc tag-type payloads) is the only text a checker may parse, and only
  through the dedicated JSDoc path.

---

## 4. The `ExecutableRegionGraph`

The native checker is where the **region-graph generalization is realized**. The
typeinfo blocks only **reserve** it: the per-function `FunctionFlowGraph` is
documented as ONE region kind — `ExecutableRegionKind::Function`, addressable by a
reserved `ExecutableRegionId` — and the other region kinds are named as future and not
built, because the 363 parity rows need function-body flow plus the existing top-level
expression lowering only (parent §5, `docs/arch/native-flow-return.md`). A whole-file
or whole-program checker needs more region kinds, so this plan builds the
`ExecutableRegionGraph` the reservation anticipated.

`FunctionFlowGraph` is **one region kind**. The `ExecutableRegionGraph` is the same
sparse, arena-free, build-time-no-type-lowering dependence structure generalized over
every executable region:

- **module top-level** — the module's statement / control region (the existing
  top-level expression lowering becomes a region);
- **class static blocks** — `static { … }` initializer regions;
- **field initializers** — instance / static field initializer expressions;
- **parameter initializers** — default-value expressions;
- **decorator expressions** — decorator-call regions (resolved + relation-checked, per
  the typeinfo decorator rules, parent §1.7);
- **top-level await** — module-level `await` regions;
- and, via the injection seam (§5), **framework template regions** — e.g. a Vue
  template's bindings as an injected region rather than generated TSX.

Every region kind builds on the **same** substrate the flow chapter already lands: the
`FunctionBodySkeleton` / flow-graph structure
(`docs/arch/native-flow-return.md`), the `flow_body_stable_hash` content-derived
flow-node identity (body-sensitive, cosmetic-insensitive), and the
`FactDomain::ProgramAnalysis` machinery (`docs/arch/fact-based-cache.md`) — the
`FlowSlice` fact, the `validates_program_analysis_domain` fail-closed validator, and
the demand-slice budget / `ReturnOnly` non-admission contract. The demand planner, the
slice nodes, and `flow_body_stable_hash` are already region-shaped (they key on a
function part / region identity), so the generalization adds region kinds **without
re-shaping the planner** — exactly the property the reservation preserved. The checker
demand-slices a region the same way typeinfo demand-slices a return: a `CheckRegion`
diagnostic over a region is graph reachability over that region's flow graph, not a
whole-region typecheck unless the region's diagnostic surface genuinely requires it.

---

## 5. Diagnostics from facts and the injection seam

The future framework-semantic injection seam (parent §5) feeds the **same** checker.
A `ProgramAnalysisContributor` emits typed `SemanticContribution`s — `InjectedBinding`
/ `InjectedNarrowingFact` / `InjectedContextualType` / `InjectedRelation` — into the
`ProgramAnalysisGraph`. The checker reads those injected facts identically to facts the
flow engine produced: a Vue template's narrowing (`v-if="x"` narrowing `x` in the
consequent) becomes a typed `InjectedNarrowingFact`, **not** generated TSX a second
engine re-checks. A framework-template region (§4) enters as an
`ExecutableRegionKind` via the seam; the checker diagnoses it from injected facts, the
same path as a native region.

The only obligation the typeinfo blocks carry now is that the architecture stays
**seam-clean**: no text / fake-AST / type-node mutation as an injection mechanism, with
semantic slots + provenance + env identity available so the seam can deposit typed
facts carrying their own provenance + env identity (parent §5). This plan **names** the
seam as the future cross-framework diagnostic path; it does **not** design the
adapter framework here. The contributor system is its own follow-up; the checker only
guarantees that injected facts flow through the **same** `DiagnosticAnalysis` query
path as native facts, with no framework-specific diagnostic engine.

---

## 6. Parity discipline — tsgo oracle, runtime-independent

Diagnostic parity is measured against the TypeScript / tsgo **oracle**: the parity
target is "Verter's diagnostics match TypeScript's diagnostics for this region / file
/ program". But the runtime stays **independent** — there is no tsserver/tsgo at query
time, the same rule the typeinfo engine already enforces (parent: no tsserver/tsgo
execution path). tsgo is the parity ORACLE that pins the expected diagnostics in the
manifest; it is never invoked to PRODUCE a diagnostic at query time.

Diagnostic parity gets its **own** future manifest, separate from the typeinfo
manifest. The 363-row typeinfo parity manifest
(`crates/verter_session/tests/typeinfo_ignored_test_manifest.rs`, parent §10.4.1) is
**not** expanded into a checker-parity manifest: the binding 363 stays an exact
count/bijection over the typeinfo ignored rows, and checker diagnostics are
characterized by a distinct manifest the checker plan owns. Mixing the two tables
would make the typeinfo count incoherent (the same defect the typeinfo plan avoids by
splitting `IgnoredTestRow` from `AdditionalProofRow`, parent §10.1).

---

## 7. Architecture guards (named, landing with the checker)

These guards are pinned here so the discipline is fixed when the checker lands. They
are the one-resolver + typed-IR-only rules applied to the diagnostic surface. They
live with the **future checker work** — they are NOT registered in the typeinfo
guards index (parent §11.8 / the typeinfo guards index) now; they are named here so
the rule text already declares its guard when the checker block lands:

- **`checker_diagnostics_derive_from_shared_facts_not_a_second_walker`** — every
  diagnostic is produced FROM `Relate` / `ResolveCall` / `FlowReturn` /
  `ContextualTypeAt` / the `ProgramAnalysisGraph` facts; no diagnostic is produced by a
  second checker walker that re-derives types.
- **`no_checker_specific_resolver`** — the checker has no resolver of its own; every
  checker query routes through the one `ProjectSemanticDispatch::execute` dispatch,
  with no parallel type-walking / relation / call / flow engine beside it.
- **`no_diagnostic_projection_repair`** — no diagnostic is recovered by
  reconstructing or repairing a projected type; the published surface stays
  shallow-by-default and diagnostics read typed facts.
- **`no_ts_text_based_diagnostic_path`** — no diagnostic is computed from source
  slicing, type-text regex, hand-rolled type-text splitters, or synthesise-then-reparse;
  the single carve-out is the dedicated JSDoc tag-type path.
- **`check_queries_route_through_project_semantic_dispatch`** — `CheckProgram` /
  `CheckFile` / `CheckRegion` / `CheckExpression` (and any finer-grained `Check*` key)
  dispatch through `ProjectSemanticDispatch::execute → SemanticGraphStore`, never a
  side path.
- **`diagnostics_are_first_class_query_results_not_graphtypenode_arms`** — a
  diagnostic resolves to `SemanticQueryValue::DiagnosticAnalysis(CheckResult)`; no
  diagnostic value is ever materialised as a `GraphTypeNode` arm (the closed
  type-values purity class, parent §1.3, stays closed).

When the checker block lands, it registers these guards in its own guards index and in
the R6 meta-guard registry alongside its `(CRITICAL)` rule text — the same discipline
the typeinfo blocks follow for their guards.

---

## 8. Sequencing and explicit non-goals

**Sequencing:**

1. **Finish typeinfo parity with checker-compatible boundaries** — done by the
   reserved seams (parent §3 reserves `DiagnosticAnalysis(CheckResult)` + the `Check*`
   names; parent §5 reserves `ExecutableRegionId` / `ExecutableRegionKind::Function`
   and the `ProgramAnalysisContributor` / `SemanticContribution` seam). The typeinfo
   blocks reserve these but do NOT build the checker.
2. **This native-checker plan as a sibling** — sequenced after typeinfo parity, owning
   the checker query layer, the `ExecutableRegionGraph`, the diagnostics-from-facts
   discipline, and the checker guards.
3. **Implement `Check*` queries incrementally, after the facts they read are stable** —
   once the U2 reducers, the U6 flow / call resolution, the U8 `ProgramAnalysisGraph`,
   and the U10 facts are stable, the `Check*` queries land block-by-block, each as enum
   variant + spec row + dispatch behavior, each lifting its own checker-manifest rows.
4. **tsgo as the parity oracle, runtime-independent** — the diagnostic manifest is
   pinned against tsgo; the runtime never invokes tsserver/tsgo at query time.

**Explicit non-goals NOW:**

- **No full-file checker execution in the typeinfo flow block.** The flow block lands
  function-body flow plus the existing top-level expression lowering; it does NOT
  whole-body type-check a region to answer a typeinfo request, gated by the typeinfo
  reservation guard `reserved_checker_queries_are_non_live_typeinfo_does_not_whole_body_check`
  (parent §3).
- **No typeinfo block waits on checker diagnostics.** No typeinfo row, query, or
  reducer depends on a `Check*` query or a `CheckResult`; the reserved arm / names are
  non-live until this plan lands.
- **No expanding the 363-row typeinfo manifest into checker parity.** Diagnostic parity
  is a distinct manifest (§6); the binding 363 stays an exact count/bijection over the
  typeinfo ignored rows.
- **No parallel "diagnostic engine" beside `Relate` / `ResolveCall` / `FlowReturn` /
  `ContextualTypeAt`.** Diagnostics derive from those facts through the one resolver;
  there is no second diagnostic-producing engine, no checker-specific resolver, no
  diagnostic projection-repair, and no TS-text-based diagnostic path.

---

## 9. Framework-agnostic end-state (north-star)

> **Status: NORTH-STAR, not a current deliverable.** Nothing in this section is built
> now. There is no adapter framework and no second engine. The immediate deliverable is
> typeinfo parity (the `U0`–`U15` plan), Vue stays the first consumer through the
> existing component-meta path, and the native checker (§§1–8) is the next layer. This
> section records the end-state the type engine and the reserved seams are **shaped
> toward** — it is the *reason* those seams exist — and Verter core stays
> framework-NEUTRAL until an adapter block is actually planned.

The end-state Verter is a **framework-agnostic semantic checker**. Verter core must
**not** be Vue-aware: Vue, Angular, Svelte, Solid, and any future framework are
**semantic adapters** that contribute regions, bindings, flow facts, component
contracts, and diagnostics **into the same engine** — the one
`SemanticQueryKey → ProjectSemanticDispatch::execute → SemanticGraphStore` resolver, the
one relation / call / flow / contextual fact authority, and the one diagnostic pipeline
(§§2–5). A framework is data fed to the core, not a fork of it.

The honest bar: **TypeScript syntax and framework templates are equal semantic
contributors to one native checker; Vue is the first adapter, not the special case the
core is built around.** A `.ts` file and a `.vue` template both *lower into the shared
semantic IR* and are checked by the same engine — neither is privileged, and the core
contains no `if framework == vue` branch.

### The `SemanticAdapter` trait (end-state shape)

An adapter is a contributor, never a resolver. It discovers and indexes its own
syntax, then lowers that syntax into typed contributions the shared engine consumes:

```rust
trait SemanticAdapter {
    fn framework_id(&self) -> FrameworkId;
    fn discover_files(&self, project: &ProjectView) -> AdapterFileSet;
    fn index_file(&self, file: SourceFile) -> AdapterIndexedFile;
    fn contribute_semantics(&self, cx: AdapterContext) -> Vec<SemanticContribution>;
}
```

`discover_files` / `index_file` mirror the shallow-file lifecycle the host already owns
(read / parse / shallow-process once per content hash); `contribute_semantics` is the
lowering step that emits typed facts. An adapter **never** calls a private resolver
path and **never** synthesizes TSX as a vehicle for semantic truth — it deposits typed
semantic data the engine validates like any native fact.

### The `SemanticContribution` enum (end-state shape)

```rust
enum SemanticContribution {
    Declaration(InjectedDeclaration),
    ExecutableRegion(ExecutableRegionGraph),
    Binding(InjectedBinding),
    FlowFact(InjectedNarrowingFact),
    ContextualType(InjectedContextualType),
    RelationCheck(InjectedRelation),
    ComponentContract(ComponentContract),
    DiagnosticRule(AdapterDiagnosticRule),
}
```

These are the typed `SemanticContribution`s the reserved `ProgramAnalysisContributor`
seam (§5, parent §5) anticipates — `InjectedBinding` / `InjectedNarrowingFact` /
`InjectedContextualType` / `InjectedRelation` are the same injected-fact vocabulary that
section already names; `ExecutableRegion` carries an `ExecutableRegionGraph` (§4);
`Declaration` / `ComponentContract` / `DiagnosticRule` extend it to whole-declaration,
component-contract, and adapter-diagnostic contributions. Every arm is typed semantic
data with its own provenance + env identity — never text, never a fake AST, never a
type-node mutation.

### The 6-point core architecture

1. **A native TypeScript-compatible checker core** — relations, inference, overloads,
   flow, and diagnostics, exposed as `CheckFile` / `CheckRegion` / `CheckExpression`
   (the reserved `Check*` query names, §2). This is the TS-grade semantic engine; it is
   *the* checker, not one of several.
2. **Framework-neutral executable regions** — the `ExecutableRegionGraph` (§4) over
   every executable region: function body, Vue template branch, Angular template block,
   Svelte reactive statement, event handler, slot-content projection, class-field
   initializer, and so on. `FunctionFlowGraph` is **one region kind**
   (`ExecutableRegionKind::Function`); a template branch is just another region kind,
   not a special path.
3. **A framework-neutral component-contract IR** — props/inputs, emits/outputs/events,
   slots/children/content-projection, the exposed instance surface, refs,
   directives/actions, bindings, and lifecycle-injected values, modelled once so every
   framework's component surface lowers into the same contract shape (the
   `ComponentContract` contribution).
4. **Semantic adapters lower their syntax into the shared semantic IR** — they do
   **not** call private resolver paths and do **not** synthesize TSX for semantic
   truth. An adapter's only job is to translate its syntax into typed contributions; the
   engine owns all resolution.
5. **Program-analysis facts are first-class VALIDATED facts** — narrowing, contextual
   typing, template scope bindings, event payloads, directive effects, and reactive
   dependencies enter the `ProgramAnalysisGraph` as typed facts under the same
   `ReadSetSignature` self-root validation as native facts (the `FactDomain::ProgramAnalysis`
   pattern, §3 / parent §5), never as generated source a second engine re-checks.
6. **One resolver / one checker** — every adapter feeds the **same** `SemanticGraphStore`,
   `Relate`, `ResolveCall`, `FlowReturn`, `CheckRegion`, and diagnostic pipeline. There
   is no per-framework resolver, no per-framework checker, and no second fact store.

### Connection to the reserved seams

This north-star is *why* the typeinfo plan reserves the seams it does, and each reserved
seam is shaped toward exactly one piece of it:

- the **`ProgramAnalysisContributor` / `SemanticContribution` injection seam** (parent
  §5) is the future `SemanticAdapter::contribute_semantics` entry point — the typed-fact
  injection path adapters lower into;
- the **`ExecutableRegionId` / `ExecutableRegionKind`** reservation (§4, parent §5) is
  the future region-graph generalization — `Function` today, template/handler/initializer
  region kinds when an adapter lands;
- the **`SemanticQueryValue::DiagnosticAnalysis(CheckResult)` value arm and the `Check*`
  query names** (§2, parent §3) are the future diagnostic surface every adapter's
  diagnostics resolve through.

All three are **reserved and non-live** today
(`reserved_checker_queries_are_non_live_typeinfo_does_not_whole_body_check`,
the flow region reservation, the seam-clean injection rule). The discipline that keeps
this north-star reachable is the same discipline this whole plan enforces: diagnostics
are query-results / side-tables, never `GraphTypeNode` arms; there is no
checker-specific or framework-specific resolver; and no whole-body diagnostic walker.
When an adapter framework is eventually planned, it lands as a clean addition over the
one engine — exactly because these seams were shaped for it now.
