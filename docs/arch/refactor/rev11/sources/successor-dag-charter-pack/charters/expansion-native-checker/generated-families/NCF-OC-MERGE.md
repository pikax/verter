<!-- unified-charter-v2
id=NCF-OC-MERGE
name=Enums, namespaces, ambient declarations, and declaration merging diagnostics
phase=expansion
train=expansion.native-checker
product=native_checker
kind=implementation
semantic_role=delivery
class=successor-generated
predecessors=NCK4,NCK6,IDX0,D8
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
charter=charters/expansion-native-checker/generated-families/NCF-OC-MERGE.md
max_production_loc=800
max_production_files=8
max_related_packages=2
rescope_loc=1500
rescope_files=12
rescope_unrelated_packages=3
initial_state=
-->

# NCF-OC-MERGE - Enums, namespaces, ambient declarations, and declaration merging diagnostics

## Independently acceptable outcome

Implement, certify, and promote exactly one native checker feature slice for **enum value/type rules, namespace/value/type merging, ambient/module/global declarations, augmentation legality, merge ordering, and duplicate exported surfaces**. The slice consumes shared semantic facts and proofs, emits first-class authored diagnostics through NCK2/NCK3, passes the NCK4 hermetic oracle/correction-overlay contract, and promotes through NCK6 at exact family/slice/profile identity. It does not broaden adjacent semantic families.

## Architectural boundary

This node is generated from `native-checker-family-manifest.toml`. Its identity, predecessor set, scope, evidence keys, and budgets are manifest-derived. The node owns diagnostic evaluation for this slice only. Symbol/type/relation/call/flow/module/project facts remain owned by their existing semantic authorities; provider TypeScript remains oracle/fallback until promotion and is never called from the native query.

### Required fact and proof inputs

- merged declarations
- enum value/type facts
- ambient/module/global augmentation authority
- module/export index facts

### Required oracle obligations

- invalid enum initializer/member access
- illegal namespace merge
- ambient initializer error
- invalid augmentation target

## Expected production changes

- Add the exact `DiagnosticRuleSpec` rows and rule evaluators for this slice.
- Add any missing typed fact view or proof reference at the lowest existing semantic owner; do not create a checker-private fact store or resolver.
- Add hermetic source fixtures, canonical expected diagnostics, and pinned tsgo oracle rows.
- Add reviewed correction-overlay entries only for clear conceded TypeScript bugs; the runtime resolver remains single-spec.
- Add incremental invalidation/read-set wiring, complete-only cache admission, cancellation/budget handling, and PER0 work counters.
- Add promotion evidence mapping this generated node to exact `DiagnosticAuthorityKey` rows in NCK6.

## Internal subblocks

### NCF-OC-MERGE-SB1 - Oracle and RED characterization

- Partition the exact semantic cases, negative controls, recovery/profile dimensions, and expected authored anchors.
- Produce failing native tests before implementation and a pinned external oracle snapshot.
- Prove the harness detects wrong code, missing/extra diagnostic, wrong primary/related anchor, wrong completeness, and wrong fix intent.

### NCF-OC-MERGE-SB2 - Fact-demand and proof contract

- Enumerate every fact/proof/query read and exact environment dimension.
- Add a typed read-only fact view if necessary at the existing owner.
- Abort/rescope if the proposed evaluator would walk source text/types or duplicate semantic resolution.

### NCF-OC-MERGE-SB3 - Bounded rule implementation

- Implement one or more small `DiagnosticRule` evaluators sharing NCK3 infrastructure.
- Use semantic subjects and proof references; no message-text/source-regex/generated-TSX semantic decisions.
- Emit stable diagnostic/family/rule/subject/anchor identity and typed fix intents only where safely specified.

### NCF-OC-MERGE-SB4 - Incremental, cancellation, and admission closure

- Key by exact semantic subject/profile/env/read-set identity.
- Admit only complete results; cancelled, stale, budget-exceeded, NeedInputs, unsupported, or partial results are ReturnOnly.
- Prove targeted invalidation and incremental-equals-fresh across relevant edits/config/lib/project changes.

### NCF-OC-MERGE-SB5 - Parity and correction-overlay certification

- Compare canonical native diagnostics against the pinned oracle across all manifest rows/property fixtures.
- Classify every mismatch as implementation defect, unsupported/rescope, oracle defect, or reviewed correction overlay.
- Bind implementation, oracle, overlay, fixture, toolchain, and evidence digests in the certification receipt.

### NCF-OC-MERGE-SB6 - Family-scoped promotion and zero-provider proof

- Promote only exact profile/family/slice rows through NCK6.
- Prove ObserveNative was non-publishing, CertifiedNative atomically suppresses external diagnostics, and rollback is receipt-backed.
- Prove warm certified requests perform zero external diagnostic work for this slice.

## Acceptance IDs

- **NCF-OC-MERGE-AC-ORACLE:** exact row coverage and planted wrong-answer sensitivity.
- **NCF-OC-MERGE-AC-ONE-ENGINE:** no checker-private resolver/walker/source-text/generated-text semantic path.
- **NCF-OC-MERGE-AC-FACTS:** every result records exact shared fact/proof read set and environment identity.
- **NCF-OC-MERGE-AC-INCREMENTAL:** incremental equals fresh; targeted edits invalidate exactly.
- **NCF-OC-MERGE-AC-ADMISSION:** cancelled/stale/partial/budget/NeedInputs outcomes never warm-admit as complete.
- **NCF-OC-MERGE-AC-PARITY:** all required rows match oracle or carry accepted correction-overlay evidence.
- **NCF-OC-MERGE-AC-PROMOTION:** exact slice promotion has no duplicate/gap and certified warm runtime performs zero provider diagnostic work.
- **NCF-OC-MERGE-AC-WORK:** equivalent-work, allocation, latency, and retained-memory thresholds pass.

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

1. Run generated hermetic rows and negative controls for `NCF-OC-MERGE`.
2. Run pinned oracle comparison and correction-overlay validation.
3. Run incremental/fresh, cancellation/budget, cache-admission, authority promotion/rollback, provider-zero-work, allocation/latency/RSS tests.
4. Run the canonical targeted gate and independent semantic review on the landing-frozen candidate.

## Deletion and residual ownership

Delete only legacy diagnostic routes/tests/ignored rows exactly mapped to this slice after promotion. Neighboring diagnostic families and remaining external provider ownership are untouched and remain capability-visible.
