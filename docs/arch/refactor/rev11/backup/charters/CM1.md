# CM1 — Component-meta request-view materialization and runtime-prop type regression correction

**Status:** DRAFT — authored for maintainer review; no AMD ratifies it yet. **Class:** subsystem
(`program-dag.toml`). **Predecessors:** `BV1`, `BS1` (both `ACCEPTED`/`IN_PROGRESS` per the ledger at
authoring time — see `docs/arch/architecture-lock/ledger/program-state.toml`).

## Context

A beta.4 regression intake (maintainer-ratified) ran `pikax/vue-benchmarks` against Verter versus
published `@verter/*@0.0.1-beta.3` and found two component-meta regressions: exposed bindings
(`defineExpose`) silently losing their type, and runtime prop constructors (`defineProps({ label:
String })`) silently losing their type. A bounded, read-only investigation reproduced both in-repo
(`crates/verter_session/tests/cases/findbc_regression.rs`, 12 tests, 9 red) and a ratified architecture
ruling (Codex xhigh, 2026-08-20) turned the investigation into repair, path, and placement decisions.
This charter turns that ruling into an executable charter. It does not reopen the ruling's REPAIR,
PATH, or PLACEMENT decisions — see "Rulings applied" below.

## There are two distinct defects, not one — this is the central finding

The maintainer's original directive hypothesised a single defect: a `defineExpose`d/`defineProps`d
position reaching `ComponentMetaOutputError::UnraisableSource` — *"the source has no live graph
representation under the request view"* (`meta_resolve/output.rs:562-564`, the `exposed[].type` lane at
`:376`). The in-repo investigation instead reproduced a **silent** degrade: the exposed/prop position
never reaches a `Present` source at all — it resolves `SourcePosition::Absent`, which is a documented,
intentional **success** path (`meta_resolve/projectors/output_sink/envelope.rs:154`) rendering
`TypeExpr::Unknown(UnknownValue::missing_output())` with no error.

The ruling holds these are **mutually exclusive control-flow states, not one defect presenting two
ways**: `Absent` returns success; `UnraisableSource` can only arise once a `Present(source)` enters the
strict raise (`raise_semantic_type_source_to_hot_strict`,
`project_semantic_dispatch/semantic_source.rs:428`) and that raise yields no graph handle
(`output_sink.rs:1221`, `:1273`) against a disposition the sink does not accept as a legitimate carrier.
Both the NAPI scalar and batch routes preserve this failure identically (`crates/verter_napi/src/meta.rs:280,303`;
`component_meta_host.rs:73`). **Both remain independent beta.4 blockers.** CM1 is not complete until
both are closed, each proven by its own reproducing, discriminating test:

- **Finding B/C (reproduced): `Absent → Unknown`.** The exposed/prop binding never gets offered a
  source at all. Root-caused below.
- **The benchmark's `Present → UnraisableSource`.** A binding DOES reach a `Present` source, but the
  strict raise cannot produce a graph handle for it. Not yet root-caused in-repo — the investigation did
  not have a fixture that reaches this state. **CM1 owns constructing or isolating a minimized fixture
  that genuinely reaches `Present` and fails the raise, tracing it to its owning producer, and correcting
  it there.**

Closing only the `Absent→Unknown` path and treating the hard error as the same bug leaves a release
blocker armed and shipping. Do not demote the hard-error case to `Absent` or `Unknown` to make it
disappear — the strict output error is PREFERABLE to an invented type.

## Established root cause

### Finding B — `defineExpose` entries never offered to macro-type expansion

`resolve_exposed_type` (`verter_semantic/src/analysis/component_meta.rs`) only sees a name if the
general macro-argument-type expansion's binding-entry admission gate,
`component_meta_binding_type_entries` (`verter_session/src/host_manage/eval_env.rs:907-956`, gate at
`:938`), offers it. That gate admits only when `PreparedValueDecl.type_annotation.classification` is
non-`Absent` — but `PreparedValueDecl` separately carries signatures
(`verter_semantic/src/analysis/type_solver/prepared.rs:390`) and object-shape facts (`:395`) that a
plain `function` declaration or a call-initialized `const` (`ref(0)`, `computed(...)`) uses instead
(function declarations intentionally lower with no value annotation plus a signature,
`type_eval_build.rs:2823`). The gate has no branch for either, so both are silently dropped before any
raise/dispatch step runs — confirmed in-repo for methods, refs/computed, and full-API fixtures, typed
and untyped alike.

A further unresolved residual: an *explicitly annotated* call-initializer (`const count: Ref<number> =
ref(0)`) should already classify `Direct` (explicit annotations become `Direct` at
`fact_projection.rs:140`; an untyped call's expression source becomes `Direct` at `:101`) but still
fails in the investigation's isolation test. The owner/lookup/preparation handoff has an unresolved
divergence somewhere between those facts and the admission gate's read of them.

Separately, `resolve_exposed_type` (`component_meta.rs:2801`) collapses `ResolvedTypeOutcome::Failed`
and `::Absent` to the same `None` — a binding that *was* offered but failed to prepare is
indistinguishable from one that was never offered, so a genuine preparation failure also renders as
silent `Unknown` instead of failing strictly.

### Finding C — runtime prop constructors are display-only on the macro path

Two structurally parallel prop-extraction paths exist in `verter_semantic/src/analysis/`. The
Options-API path, `extract_props_from_options` (`component_meta.rs:3023`, mapping at `:3029`), reads
the recognized runtime constructor identifier and mints a real, closed semantic fact:
`SemanticTypeSource::Closed(ClosedTypeFact::Leaf(LeafTypeFact::Primitive(...)))`. The Composition-API
macro path, `extract_prop_fields_from_runtime`/`constructor_to_ts_type` (`macros.rs:2982`, write site
`:3057`), recognizes the same constructor identifiers but writes the result **only** to
`AnalyzedPropField.type_annotation` — a field explicitly documented as display-only
(`analysis/types.rs:1111`) that must never feed semantic decisions. No `field.payload` is populated
unless the author wrote an explicit `X as PropType<T>` assertion
(`has_authored_prop_type_assertion`/`type_expr_scope` gating, `macros.rs:2998-3023`, `:3139-3140`). So
for the ordinary `defineProps({ label: String })` shorthand, no typed source is ever constructed —
`SourcePosition` resolves genuinely `Absent` and the sink correctly (per its own contract) renders
`Unknown`. `required`/`has_default` compute independently of `field.payload` and are unaffected, which
is why optionality survives while the primitive type does not.

## Owned scope

CM1 owns, and is the sole owner of:

1. **Finding B — structural admission repair.** Replace the `type_annotation.classification`-only gate
   in `component_meta_binding_type_entries` with admission of every structurally demanded, lexically
   visible local value binding, deferring source selection to the single prepared-value resolver that
   already handles annotation, expression-source, object-shape, signature, enum, and class facts
   elsewhere (`project_semantic_dispatch/build.rs:741` for expression sources, `:840` for signatures). A
   `!signatures.is_empty()` patch alone is insufficient — the fix is exhaustive over
   `PreparedValueDecl`'s fact carriers, not one added branch.
2. **Finding B — call-initializer residual.** Trace the owner/lookup/preparation handoff for both typed
   and untyped call-initialized bindings and correct the earliest point of divergence from the `Direct`
   classification `fact_projection.rs:101`/`:140` already predict. No call-expression allowlist.
3. **Finding B — `Failed` vs `Absent`.** `resolve_exposed_type` (`component_meta.rs:2801`) stops
   collapsing `Failed` into `Absent`; `SourcePosition`/authority is preserved through publication so a
   demanded binding that cannot be prepared fails strictly instead of rendering `Unknown`.
4. **Finding C — shared runtime-constructor semantics.** One typed runtime-constructor fact/enum,
   producer-owned, shared by Options (`extract_props_from_options`) and macro
   (`extract_prop_fields_from_runtime`) extraction, lowering exact `String`/`Number`/`Boolean` identities
   to `ClosedTypeFact::Primitive` at analysis time. Not a copy of the Options path's string switch into
   the macro path, not a display-text parse, not a mapping at the output seam. Covers the shorthand
   (`label: String`) and expanded (`{ type: String }`) forms, `required`/`has_default`/default-value
   combinations, constructor-array (`[String, Number]`) and nullable forms. The one existing correct
   authored-payload path — a custom-class constructor whose class is module-owned or imported — stays on
   that route and must remain green, unmodified in substance. `PropType<T>` assertions and
   `<script setup>`-local custom classes are NOT on a correct path: both publish `unknown`, so neither
   can discriminate "stayed on the correct path" from "broke". Both are discharged as deferred captures
   instead — see Required exits.
5. **The benchmark's `Present → UnraisableSource` hard error.** Construct or isolate a minimized fixture
   that genuinely drives an exposed/prop position to `SourcePosition::Present` and fails the strict raise
   (`raise_semantic_type_source_to_hot_strict`) against a disposition `output_sink.rs:1216-1273`
   classifies as not a legitimate carrier — reproducing the literal message at `output.rs:562-564`. Trace
   it to its owning producer and correct the producer. Do not weaken the strict raise and do not demote
   the failure to `Absent`/`Unknown` to make it disappear.
6. **Regression tests.** Turn `findbc_regression.rs`'s 9 red reproductions green, and add a new,
   independently discriminating test for the `UnraisableSource` hard-error fixture (red pre-repair, green
   post-repair) in the same test family.
7. Acceptance coverage across the matrix below, for both native and `@verter/component-meta/compat`
   surfaces, across cold, warm, sequential, `Promise.all`-equivalent concurrent, batch, and
   overlay/request-view invocation.

CM1 does **not** own: C1's immutable-observation/request-view lifecycle rework (the error wording
mentions a request view; it does not establish that architecture is required — C1 explicitly preserves
current component-meta answers and does not own publication policy, `C1.md:50,56`); the Vue declaration
output fidelity gap where call-initialized values become `unknown` and functions become `(...): any` by
explicit policy (`crates/verter_compiler/src/tsc/script.rs:6003,6015` — **BV2-owned**, see "Blast-radius
findings assigned elsewhere" below); the framework-surface semantic API returning runtime-object macros
as memberless (`crates/verter_session/src/typeinfo/framework_surface/vue_exec/mod.rs:533` —
**BV2-owned**, not C3, which explicitly excludes runtime object syntax, `C3.md:33`); PublicApi/TSC/TSX
output (unaffected by these two defects — runtime expose already emits `typeof` correctly,
`tsc/script.rs:5981`; runtime constructors are already correct in TSC/Public output, `tsc/tests.rs:1727`);
C3's project-aware macro projection; any Vue/Svelte framework semantic repair unrelated to these two
findings; component-meta publication policy beyond correcting these two defects.

## Forbidden outcomes

Per the maintainer's hard constraints, none of the following is an acceptable CM1 result, regardless of
whether it makes the reproducing tests pass:

- Replacing an unraisable source with `unknown`, or any other invented type.
- Omitting exposed entries, or silently omitting a prop/expose member from the published surface.
- Swallowing the materialization failure, or converting it into an apparently successful result.
- Stale graph state, or retaining a graph representation beyond its lifetime.
- Bypassing request-view isolation to make a `Present` source raise.
- Mapping `String`/`Number`/`Boolean` (or any constructor identity) at the output seam without tracing
  and correcting semantic ownership at the producer.
- A call-expression allowlist as Finding B's residual fix.
- Copying the Options path's string switch literally into the macro path instead of a shared,
  producer-owned typed fact.
- Demoting the benchmark's `UnraisableSource` hard error to `Absent`/`Unknown` to close the finding
  without fixing the underlying producer.
- Landed enforcement of any invariant this charter introduces that is a name-keyed source scanner rather
  than structural (type-state, sealed traits, visibility/`E0603`, or a closed enum matched exhaustively).

## Acceptance matrix

CM1's required exit is proven across the full cross of these axes, not the single reported cell:

| Axis | Values |
|---|---|
| `defineExpose` shape | simple / multiple / refs / computed refs / methods / mixed full-API |
| `defineExpose` type position | imported / local; source-local / project-aware |
| `defineProps` runtime form | shorthand (`label: String`) / expanded (`{ type: String }`) / required / optional / `required: true` / with default / constructor array (`[String, Number]`) / nullable / mixed runtime + type-declared — EXCLUDED: deferred authored runtime-assertion type-publication capture; not a demanded CM1 cell |
| Constructor kind | `String` / `Number` / `Boolean` (positive); module-owned or imported custom class (negative control, must stay on the existing correct path). `PropType<T>` and `<script setup>`-local custom classes are EXCLUDED from the control — both publish `unknown` and cannot discriminate; both are captured, not demanded |
| Invocation | cold / warm (same session, resolved twice) / sequential / `Promise.all`-equivalent concurrent / batch (`get_component_meta_output_batch`) |
| Surface | native / `@verter/component-meta/compat` |
| Request-view scope | overlay / base session |
| Hard-error fixture | a `Present`-source position whose strict raise fails — must surface `UnraisableSource` with the exact producer-traced cause, never a silent `Unknown` |

For every cell: no silent `Unknown`/`Absent` substitution for a legitimately typed binding; a genuine
preparation failure or unraisable source fails strictly with the typed error, never a swallowed or
invented result; identical results across cold/warm/sequential/concurrent/batch invocation; no stale
graph state; no request-view bypass; native and compat surfaces agree.

## Required exits

`FC-CM-001` passes: every cell of the acceptance matrix above produces the required outcome, both
`findbc_regression.rs` reproductions and the new `UnraisableSource` hard-error test are green and
independently discriminating (each fails against the pre-repair tree and passes against the post-repair
tree), and existing component-meta suites (`crates/verter_session/src/meta_tests.rs` and the wider
`meta_resolve`/`component_meta` gate coverage) stay green, unmodified in substance.

Three axis values are EXCLUDED from the demanded cells and discharged as deferred captures
instead: `PropType<T>` resolution, `<script setup>`-local custom-class resolution, and the
`mixed runtime + type-declared` runtime form. They are TWO distinct defects, not three — the counts
differ because two of those values are two spellings of the same loss. The third was ratified separately in
[`rulings/ARCHITECT-RULING-2026-08-25-CM1-AUTHORED-ASSERTION-CAPTURE.md`](../rulings/ARCHITECT-RULING-2026-08-25-CM1-AUTHORED-ASSERTION-CAPTURE.md),
which supersedes an earlier ruling that had held the value satisfiable: detection accepts both
`PropType<T>` and `X as () => T`, but the authored payload is discarded at one shared publication
point, so the value cannot be demanded green. `PropType<T>` and `X as () => T` are two spellings of
ONE defect; the `<script setup>`-local class is a separate mechanism turning on declaration site,
and the two are deliberately not merged. Each is carried as one
`#[ignore]`d discriminating test — passing against a correct implementation, failing today for its own
stated reason — naming the post-program maintainer type-correction work as its owner, per
[`rulings/MAINTAINER-RULING-BUGS-AND-TYPES.md`](../rulings/MAINTAINER-RULING-BUGS-AND-TYPES.md) rule 3,
which waives type-correctness work from every block for the program's duration. An `#[ignore]`d capture
documents a deferred defect and is NOT green evidence, so these three cells are not demanded. Under this
amendment no demanded cell is unevidenced.

## Structural confinement

Every invariant CM1 introduces is enforced structurally — type-state, sealed traits, visibility/`E0603`,
or a closed enum matched exhaustively — never a name-keyed source scanner. Concretely: the corrected
admission gate is an exhaustive read over `PreparedValueDecl`'s existing fact carriers (annotation,
expression source, object shape, signatures, enum, class), not an added ad hoc boolean flag; the shared
runtime-constructor fact is one producer-owned typed enum consumed by both `extract_props_from_options`
and `extract_prop_fields_from_runtime` through a single call site, not two independently-maintained
string switches; a reviewer verifies the sharing by reading that one call site, not by running a scanner.

## Review

Three independent mandates on one candidate SHA and tree (`governance.md` §1): conformance (charter,
diff, the acceptance matrix, and whether both the `Absent→Unknown` reproductions and the
`Present→UnraisableSource` hard-error fixture are independently, discriminatingly tested); architecture
(confirms Path A holds — no C1 rework was actually needed — and confirms the BV2-owned blast-radius items
were not silently absorbed into CM1); adversarial (mutation-tested regression coverage for both findings
and the hard-error fixture; confirms no `unknown`-substitution, allowlist, or output-seam-mapping
shortcut landed).

## Abort/rescope

Stop for: evidence that either finding, or the hard-error fixture, genuinely cannot be closed without
C1's immutable-observation/request-view rework (that reopens the ruling, not a quiet local substitution
for its PATH decision); discovery of a third, distinct defect class beyond Finding B, Finding C, and the
`UnraisableSource` locator; inability to construct or isolate a fixture that genuinely reaches
`Present → UnraisableSource` after a bounded, good-faith effort (escalate for a ruling on how to satisfy
that acceptance condition — do not silently drop it); or pressure to close either finding via an
allowlist, a display-text parse, or an output-seam mapping.

## Rulings applied

Binding ruling: the ratified CM1/Findings-B-and-C architecture ruling (Codex xhigh, 2026-08-20), following
the maintainer's beta.4 regression-intake directive and the bounded, read-only root-cause investigation.
Do not relitigate here; preserve the ruling's and root-cause investigation's evidentiary records into
`docs/arch/refactor/rev11/evidence/CM1/` at implementation time.

1. **Discrepancy — decided.** The reproduced `Absent→Unknown` degrade and the benchmark's
   `Present→UnraisableSource` hard error are distinct defects, not one defect presenting two ways. Both
   remain beta.4 blockers; CM1 owns both.
2. **Repair B — decided.** Structurally admit every demanded, lexically visible local value binding
   (not a `signatures.is_empty()` patch); fix the call-initializer owner/preparation handoff; preserve
   `Failed` through publication instead of collapsing it into `Absent`.
3. **Repair C — decided.** The Options-API path's closed-primitive semantics are the shared analyzer
   model; the macro path is the anomaly to be corrected to match it. Never derive semantics from display
   text or at the output seam.
4. **Path — decided.** Both repairs, and the separate `UnraisableSource` locator failure, are Path A
   (bounded correction under the current architecture). Neither requires C1.
5. **Placement — decided.** `{BV1, BS1} → CM1 → C1`, independent of the existing `{BV1, BS1} → BV2 → B5`
   chain — no edge between CM1 and BV2. C1's predecessors become `["A6", "B1", "B2", "CM1"]`
   (`program-dag.toml`) — its prior three predecessors are preserved, not replaced. C2 already
   reconverges B5 and C1. Beta.4 requires both branches accepted; E1 is not the owner of these
   regressions and does not receive a deferral.
6. **Acceptance-matrix amendment — ratified.** Recorded in-tree at
   [`rulings/ARCHITECT-RULING-2026-08-24-CM1-CONTROL-AXIS-AMENDMENT.md`](../rulings/ARCHITECT-RULING-2026-08-24-CM1-CONTROL-AXIS-AMENDMENT.md)
   (`RULING-2026-08-24-CM1-CONTROL-AXIS-AMENDMENT`, RATIFIED). The charter as first written was unsatisfiable: it
   demanded green evidence at `:118`, `:174` and `:187` for authored routes that publish `unknown`, so
   a cell could be neither satisfied nor honestly dropped. The negative control is restricted to
   module-owned and imported custom classes; `PropType<T>` and `<script setup>`-local custom classes
   leave the control and survive solely as the two `#[ignore]`d captures above. Repair ownership is
   settled elsewhere and is not reopened here.
7. **Blast-radius findings assigned elsewhere — decided.** The declaration-output fidelity gap
   (`tsc/script.rs:6003,6015`) and the framework-surface memberless-runtime-macro gap
   (`typeinfo/framework_surface/vue_exec/mod.rs:533`) belong to BV2's already-approved scope (added as
   acceptance coverage, not a new DAG edge), because both are Vue declaration/tooling and source-local
   macro concerns BV1/BV2 already own — not C3, which explicitly excludes runtime object syntax. No
   ruling changes an accepted ADR or the final program target.
