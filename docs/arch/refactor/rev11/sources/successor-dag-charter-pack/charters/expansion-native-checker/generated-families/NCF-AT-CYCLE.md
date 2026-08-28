<!-- unified-charter-v2
id=NCF-AT-CYCLE
name=Recursive type, instantiation cycle, and complexity diagnostics
phase=expansion
train=expansion.native-checker
product=native_checker
kind=implementation
semantic_role=delivery
class=successor-generated
predecessors=NCK4,NCK6,D8,G2
conditional_predecessors=
owner=expansion.native-checker:one certified semantic diagnostic feature slice
conflict_domains=semantic_authority,diagnostic_action_service,vertical_manifest
resource_class=rust-mixed
review_profile=semantic-3
gate_profile=targeted-domain
implementation_effort_min=high
implementation_effort_default=high
review_effort_min=high
review_effort_default=high
verification_effort_min=high
verification_effort_default=high
confirmation_effort_min=high
confirmation_effort_default=high
size=M
dispatchable=true
optional=false
release_gating=none
source_refs=catalog:docs/arch/refactor/rev11/catalogs/native-checker-family-manifest.toml
external_requirements=
activation_gate=ORC0
charter=charters/expansion-native-checker/generated-families/NCF-AT-CYCLE.md
max_production_loc=800
max_production_files=8
max_related_packages=2
rescope_loc=1500
rescope_files=12
rescope_unrelated_packages=3
initial_state=
-->

# NCF-AT-CYCLE - Recursive type, instantiation cycle, and complexity diagnostics

## Independently acceptable outcome

Implement, certify, and promote exactly one native checker feature slice for **recursive aliases, circular base/constraint/default/reference relationships, excessive instantiation, query depth, union/intersection complexity, and cycle-safe degradation**. The slice consumes shared semantic facts and proofs, emits first-class authored diagnostics through NCK2/NCK3, passes the NCK4 hermetic oracle/correction-overlay contract, and promotes through NCK6 at exact family/slice/profile identity. It does not broaden adjacent semantic families.

## Architectural boundary

This node is generated from `native-checker-family-manifest.toml`. Its identity, predecessor set, scope, evidence keys, and budgets are manifest-derived. The node owns diagnostic evaluation for this slice only. Symbol/type/relation/call/flow/module/project facts remain owned by their existing semantic authorities; provider TypeScript remains oracle/fallback until promotion and is never called from the native query.

### Required fact and proof inputs

- CheckerReentryGraph/cycle IDs
- instantiation/query budgets
- type dependency graph
- complete/partial admission evidence

### Required oracle obligations

- type alias circularly references itself
- base/constraint cycle
- excessively deep instantiation
- complex union representation limit

## Expected production changes

- Add the exact `DiagnosticRuleSpec` rows and rule evaluators for this slice.
- Add any missing typed fact view or proof reference at the lowest existing semantic owner; do not create a checker-private fact store or resolver.
- Add hermetic source fixtures, canonical expected diagnostics, and pinned tsgo oracle rows.
- Add reviewed correction-overlay entries only for clear conceded TypeScript bugs; the runtime resolver remains single-spec.
- Add incremental invalidation/read-set wiring, complete-only cache admission, cancellation/budget handling, and PER0 work counters.
- Add promotion evidence mapping this generated node to exact `DiagnosticAuthorityKey` rows in NCK6.

## Internal subblocks

### NCF-AT-CYCLE-SB1 - Oracle and RED characterization

- Partition the exact semantic cases, negative controls, recovery/profile dimensions, and expected authored anchors.
- Produce failing native tests before implementation and a pinned external oracle snapshot.
- Prove the harness detects wrong code, missing/extra diagnostic, wrong primary/related anchor, wrong completeness, and wrong fix intent.

### NCF-AT-CYCLE-SB2 - Fact-demand and proof contract

- Enumerate every fact/proof/query read and exact environment dimension.
- Add a typed read-only fact view if necessary at the existing owner.
- Abort/rescope if the proposed evaluator would walk source text/types or duplicate semantic resolution.

### NCF-AT-CYCLE-SB3 - Bounded rule implementation

- Implement one or more small `DiagnosticRule` evaluators sharing NCK3 infrastructure.
- Use semantic subjects and proof references; no message-text/source-regex/generated-TSX semantic decisions.
- Emit stable diagnostic/family/rule/subject/anchor identity and typed fix intents only where safely specified.

### NCF-AT-CYCLE-SB4 - Incremental, cancellation, and admission closure

- Key by exact semantic subject/profile/env/read-set identity.
- Admit only complete results; cancelled, stale, budget-exceeded, NeedInputs, unsupported, or partial results are ReturnOnly.
- Prove targeted invalidation and incremental-equals-fresh across relevant edits/config/lib/project changes.

### NCF-AT-CYCLE-SB5 - Parity and correction-overlay certification

- Compare canonical native diagnostics against the pinned oracle across all manifest rows/property fixtures.
- Classify every mismatch as implementation defect, unsupported/rescope, oracle defect, or reviewed correction overlay.
- Bind implementation, oracle, overlay, fixture, toolchain, and evidence digests in the certification receipt.

### NCF-AT-CYCLE-SB6 - Family-scoped promotion and zero-provider proof

- Promote only exact profile/family/slice rows through NCK6.
- Prove ObserveNative was non-publishing, CertifiedNative atomically suppresses external diagnostics, and rollback is receipt-backed.
- Prove warm certified requests perform zero external diagnostic work for this slice.

## Acceptance IDs

- **NCF-AT-CYCLE-AC-ORACLE:** exact row coverage and planted wrong-answer sensitivity.
- **NCF-AT-CYCLE-AC-ONE-ENGINE:** no checker-private resolver/walker/source-text/generated-text semantic path.
- **NCF-AT-CYCLE-AC-FACTS:** every result records exact shared fact/proof read set and environment identity.
- **NCF-AT-CYCLE-AC-INCREMENTAL:** incremental equals fresh; targeted edits invalidate exactly.
- **NCF-AT-CYCLE-AC-ADMISSION:** cancelled/stale/partial/budget/NeedInputs outcomes never warm-admit as complete.
- **NCF-AT-CYCLE-AC-PARITY:** all required rows match oracle or carry accepted correction-overlay evidence.
- **NCF-AT-CYCLE-AC-PROMOTION:** exact slice promotion has no duplicate/gap and certified warm runtime performs zero provider diagnostic work.
- **NCF-AT-CYCLE-AC-WORK:** equivalent-work, allocation, latency, and retained-memory thresholds pass.

## Forbidden designs

- A second type/relation/call/flow/module/project resolver or checker-private semantic store.
- Runtime tsgo/tsserver invocation from native rule evaluation.
- Source slicing, regex/type-text parsing, synthesize-then-reparse, or generated TSX as semantic truth. The only text carve-out is the dedicated JSDoc parser for the JSDoc slice.
- Message/range-only diagnostic identity or deduplication.
- Whole-program eager checking for a scoped region/file demand.
- Extending adjacent manifest slices without a generator amendment.

## Budgets and mandatory rescope

Target ceiling is 800 production LOC, 8 production files, and 2 related packages. Rescope before mutation above 1,500 LOC, 12 files, 3 unrelated packages, or whenever the slice requires a new major semantic algorithm rather than consuming accepted facts. A slice that cannot fit one independent review context must be split in the manifest before implementation.

## Verification

1. Run generated hermetic rows and negative controls for `NCF-AT-CYCLE`.
2. Run pinned oracle comparison and correction-overlay validation.
3. Run incremental/fresh, cancellation/budget, cache-admission, authority promotion/rollback, provider-zero-work, allocation/latency/RSS tests.
4. Run the canonical targeted gate and independent semantic review on the landing-frozen candidate.

## Deletion and residual ownership

Delete only legacy diagnostic routes/tests/ignored rows exactly mapped to this slice after promotion. Neighboring diagnostic families and remaining external provider ownership are untouched and remain capability-visible.
