# Native Checker generated feature-slice charters

> Generated review artifact. The family manifest is the source of node identity/scope and individual charter files remain the proposal authority.


---

<!-- unified-charter-v2
id=NCF-BD-SCOPE
name=Lexical scope, shadowing, and name binding diagnostics
phase=expansion
train=expansion.native-checker
product=native_checker
kind=implementation
semantic_role=delivery
class=successor-generated
predecessors=NCK4,NCK6,IDX0
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
charter=charters/expansion-native-checker/generated-families/NCF-BD-SCOPE.md
max_production_loc=800
max_production_files=8
max_related_packages=2
rescope_loc=1500
rescope_files=12
rescope_unrelated_packages=3
initial_state=
-->

# NCF-BD-SCOPE - Lexical scope, shadowing, and name binding diagnostics

## Independently acceptable outcome

Implement, certify, and promote exactly one native checker feature slice for **lexical/module/function/class/block/catch/parameter scope construction, shadowing, unresolved names, temporal visibility, and namespace selection**. The slice consumes shared semantic facts and proofs, emits first-class authored diagnostics through NCK2/NCK3, passes the NCK4 hermetic oracle/correction-overlay contract, and promotes through NCK6 at exact family/slice/profile identity. It does not broaden adjacent semantic families.

## Architectural boundary

This node is generated from `native-checker-family-manifest.toml`. Its identity, predecessor set, scope, evidence keys, and budgets are manifest-derived. The node owns diagnostic evaluation for this slice only. Symbol/type/relation/call/flow/module/project facts remain owned by their existing semantic authorities; provider TypeScript remains oracle/fallback until promotion and is never called from the native query.

### Required fact and proof inputs

- canonical symbol/declaration identity
- scope and binding tables
- source-order/region identity
- module/global environment facts

### Required oracle obligations

- unresolved identifier
- illegal shadowing/capture
- wrong value/type namespace
- scope leakage across regions

## Expected production changes

- Add the exact `DiagnosticRuleSpec` rows and rule evaluators for this slice.
- Add any missing typed fact view or proof reference at the lowest existing semantic owner; do not create a checker-private fact store or resolver.
- Add hermetic source fixtures, canonical expected diagnostics, and pinned tsgo oracle rows.
- Add reviewed correction-overlay entries only for clear conceded TypeScript bugs; the runtime resolver remains single-spec.
- Add incremental invalidation/read-set wiring, complete-only cache admission, cancellation/budget handling, and PER0 work counters.
- Add promotion evidence mapping this generated node to exact `DiagnosticAuthorityKey` rows in NCK6.

## Internal subblocks

### NCF-BD-SCOPE-SB1 - Oracle and RED characterization

- Partition the exact semantic cases, negative controls, recovery/profile dimensions, and expected authored anchors.
- Produce failing native tests before implementation and a pinned external oracle snapshot.
- Prove the harness detects wrong code, missing/extra diagnostic, wrong primary/related anchor, wrong completeness, and wrong fix intent.

### NCF-BD-SCOPE-SB2 - Fact-demand and proof contract

- Enumerate every fact/proof/query read and exact environment dimension.
- Add a typed read-only fact view if necessary at the existing owner.
- Abort/rescope if the proposed evaluator would walk source text/types or duplicate semantic resolution.

### NCF-BD-SCOPE-SB3 - Bounded rule implementation

- Implement one or more small `DiagnosticRule` evaluators sharing NCK3 infrastructure.
- Use semantic subjects and proof references; no message-text/source-regex/generated-TSX semantic decisions.
- Emit stable diagnostic/family/rule/subject/anchor identity and typed fix intents only where safely specified.

### NCF-BD-SCOPE-SB4 - Incremental, cancellation, and admission closure

- Key by exact semantic subject/profile/env/read-set identity.
- Admit only complete results; cancelled, stale, budget-exceeded, NeedInputs, unsupported, or partial results are ReturnOnly.
- Prove targeted invalidation and incremental-equals-fresh across relevant edits/config/lib/project changes.

### NCF-BD-SCOPE-SB5 - Parity and correction-overlay certification

- Compare canonical native diagnostics against the pinned oracle across all manifest rows/property fixtures.
- Classify every mismatch as implementation defect, unsupported/rescope, oracle defect, or reviewed correction overlay.
- Bind implementation, oracle, overlay, fixture, toolchain, and evidence digests in the certification receipt.

### NCF-BD-SCOPE-SB6 - Family-scoped promotion and zero-provider proof

- Promote only exact profile/family/slice rows through NCK6.
- Prove ObserveNative was non-publishing, CertifiedNative atomically suppresses external diagnostics, and rollback is receipt-backed.
- Prove warm certified requests perform zero external diagnostic work for this slice.

## Acceptance IDs

- **NCF-BD-SCOPE-AC-ORACLE:** exact row coverage and planted wrong-answer sensitivity.
- **NCF-BD-SCOPE-AC-ONE-ENGINE:** no checker-private resolver/walker/source-text/generated-text semantic path.
- **NCF-BD-SCOPE-AC-FACTS:** every result records exact shared fact/proof read set and environment identity.
- **NCF-BD-SCOPE-AC-INCREMENTAL:** incremental equals fresh; targeted edits invalidate exactly.
- **NCF-BD-SCOPE-AC-ADMISSION:** cancelled/stale/partial/budget/NeedInputs outcomes never warm-admit as complete.
- **NCF-BD-SCOPE-AC-PARITY:** all required rows match oracle or carry accepted correction-overlay evidence.
- **NCF-BD-SCOPE-AC-PROMOTION:** exact slice promotion has no duplicate/gap and certified warm runtime performs zero provider diagnostic work.
- **NCF-BD-SCOPE-AC-WORK:** equivalent-work, allocation, latency, and retained-memory thresholds pass.

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

1. Run generated hermetic rows and negative controls for `NCF-BD-SCOPE`.
2. Run pinned oracle comparison and correction-overlay validation.
3. Run incremental/fresh, cancellation/budget, cache-admission, authority promotion/rollback, provider-zero-work, allocation/latency/RSS tests.
4. Run the canonical targeted gate and independent semantic review on the landing-frozen candidate.

## Deletion and residual ownership

Delete only legacy diagnostic routes/tests/ignored rows exactly mapped to this slice after promotion. Neighboring diagnostic families and remaining external provider ownership are untouched and remain capability-visible.


---

<!-- unified-charter-v2
id=NCF-BD-DUP
name=Duplicate and conflicting declaration diagnostics
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
charter=charters/expansion-native-checker/generated-families/NCF-BD-DUP.md
max_production_loc=800
max_production_files=8
max_related_packages=2
rescope_loc=1500
rescope_files=12
rescope_unrelated_packages=3
initial_state=
-->

# NCF-BD-DUP - Duplicate and conflicting declaration diagnostics

## Independently acceptable outcome

Implement, certify, and promote exactly one native checker feature slice for **duplicate declarations, conflicting symbol kinds, incompatible merged declarations, duplicate parameters/members, and declaration-order conflicts**. The slice consumes shared semantic facts and proofs, emits first-class authored diagnostics through NCK2/NCK3, passes the NCK4 hermetic oracle/correction-overlay contract, and promotes through NCK6 at exact family/slice/profile identity. It does not broaden adjacent semantic families.

## Architectural boundary

This node is generated from `native-checker-family-manifest.toml`. Its identity, predecessor set, scope, evidence keys, and budgets are manifest-derived. The node owns diagnostic evaluation for this slice only. Symbol/type/relation/call/flow/module/project facts remain owned by their existing semantic authorities; provider TypeScript remains oracle/fallback until promotion and is never called from the native query.

### Required fact and proof inputs

- merged declaration authority
- symbol namespace/kind facts
- relation proofs for compatibility
- module/global augmentation facts

### Required oracle obligations

- duplicate block binding
- conflicting interface/property merge
- duplicate private name
- illegal value/type merge

## Expected production changes

- Add the exact `DiagnosticRuleSpec` rows and rule evaluators for this slice.
- Add any missing typed fact view or proof reference at the lowest existing semantic owner; do not create a checker-private fact store or resolver.
- Add hermetic source fixtures, canonical expected diagnostics, and pinned tsgo oracle rows.
- Add reviewed correction-overlay entries only for clear conceded TypeScript bugs; the runtime resolver remains single-spec.
- Add incremental invalidation/read-set wiring, complete-only cache admission, cancellation/budget handling, and PER0 work counters.
- Add promotion evidence mapping this generated node to exact `DiagnosticAuthorityKey` rows in NCK6.

## Internal subblocks

### NCF-BD-DUP-SB1 - Oracle and RED characterization

- Partition the exact semantic cases, negative controls, recovery/profile dimensions, and expected authored anchors.
- Produce failing native tests before implementation and a pinned external oracle snapshot.
- Prove the harness detects wrong code, missing/extra diagnostic, wrong primary/related anchor, wrong completeness, and wrong fix intent.

### NCF-BD-DUP-SB2 - Fact-demand and proof contract

- Enumerate every fact/proof/query read and exact environment dimension.
- Add a typed read-only fact view if necessary at the existing owner.
- Abort/rescope if the proposed evaluator would walk source text/types or duplicate semantic resolution.

### NCF-BD-DUP-SB3 - Bounded rule implementation

- Implement one or more small `DiagnosticRule` evaluators sharing NCK3 infrastructure.
- Use semantic subjects and proof references; no message-text/source-regex/generated-TSX semantic decisions.
- Emit stable diagnostic/family/rule/subject/anchor identity and typed fix intents only where safely specified.

### NCF-BD-DUP-SB4 - Incremental, cancellation, and admission closure

- Key by exact semantic subject/profile/env/read-set identity.
- Admit only complete results; cancelled, stale, budget-exceeded, NeedInputs, unsupported, or partial results are ReturnOnly.
- Prove targeted invalidation and incremental-equals-fresh across relevant edits/config/lib/project changes.

### NCF-BD-DUP-SB5 - Parity and correction-overlay certification

- Compare canonical native diagnostics against the pinned oracle across all manifest rows/property fixtures.
- Classify every mismatch as implementation defect, unsupported/rescope, oracle defect, or reviewed correction overlay.
- Bind implementation, oracle, overlay, fixture, toolchain, and evidence digests in the certification receipt.

### NCF-BD-DUP-SB6 - Family-scoped promotion and zero-provider proof

- Promote only exact profile/family/slice rows through NCK6.
- Prove ObserveNative was non-publishing, CertifiedNative atomically suppresses external diagnostics, and rollback is receipt-backed.
- Prove warm certified requests perform zero external diagnostic work for this slice.

## Acceptance IDs

- **NCF-BD-DUP-AC-ORACLE:** exact row coverage and planted wrong-answer sensitivity.
- **NCF-BD-DUP-AC-ONE-ENGINE:** no checker-private resolver/walker/source-text/generated-text semantic path.
- **NCF-BD-DUP-AC-FACTS:** every result records exact shared fact/proof read set and environment identity.
- **NCF-BD-DUP-AC-INCREMENTAL:** incremental equals fresh; targeted edits invalidate exactly.
- **NCF-BD-DUP-AC-ADMISSION:** cancelled/stale/partial/budget/NeedInputs outcomes never warm-admit as complete.
- **NCF-BD-DUP-AC-PARITY:** all required rows match oracle or carry accepted correction-overlay evidence.
- **NCF-BD-DUP-AC-PROMOTION:** exact slice promotion has no duplicate/gap and certified warm runtime performs zero provider diagnostic work.
- **NCF-BD-DUP-AC-WORK:** equivalent-work, allocation, latency, and retained-memory thresholds pass.

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

1. Run generated hermetic rows and negative controls for `NCF-BD-DUP`.
2. Run pinned oracle comparison and correction-overlay validation.
3. Run incremental/fresh, cancellation/budget, cache-admission, authority promotion/rollback, provider-zero-work, allocation/latency/RSS tests.
4. Run the canonical targeted gate and independent semantic review on the landing-frozen candidate.

## Deletion and residual ownership

Delete only legacy diagnostic routes/tests/ignored rows exactly mapped to this slice after promotion. Neighboring diagnostic families and remaining external provider ownership are untouched and remain capability-visible.


---

<!-- unified-charter-v2
id=NCF-BD-INIT
name=Use-before-declaration and initialization diagnostics
phase=expansion
train=expansion.native-checker
product=native_checker
kind=implementation
semantic_role=delivery
class=successor-generated
predecessors=NCK4,NCK6,D8,IDX0
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
charter=charters/expansion-native-checker/generated-families/NCF-BD-INIT.md
max_production_loc=800
max_production_files=8
max_related_packages=2
rescope_loc=1500
rescope_files=12
rescope_unrelated_packages=3
initial_state=
-->

# NCF-BD-INIT - Use-before-declaration and initialization diagnostics

## Independently acceptable outcome

Implement, certify, and promote exactly one native checker feature slice for **temporal dead zones, use before assignment, initialization ordering, parameter/property initializer visibility, and module initialization hazards**. The slice consumes shared semantic facts and proofs, emits first-class authored diagnostics through NCK2/NCK3, passes the NCK4 hermetic oracle/correction-overlay contract, and promotes through NCK6 at exact family/slice/profile identity. It does not broaden adjacent semantic families.

## Architectural boundary

This node is generated from `native-checker-family-manifest.toml`. Its identity, predecessor set, scope, evidence keys, and budgets are manifest-derived. The node owns diagnostic evaluation for this slice only. Symbol/type/relation/call/flow/module/project facts remain owned by their existing semantic authorities; provider TypeScript remains oracle/fallback until promotion and is never called from the native query.

### Required fact and proof inputs

- executable-region graph
- declaration/initialization facts
- flow reachability and definite-assignment facts
- module dependency order

### Required oracle obligations

- block-scoped use before declaration
- property used before initialization
- parameter initializer forward reference
- module cycle initialization hazard

## Expected production changes

- Add the exact `DiagnosticRuleSpec` rows and rule evaluators for this slice.
- Add any missing typed fact view or proof reference at the lowest existing semantic owner; do not create a checker-private fact store or resolver.
- Add hermetic source fixtures, canonical expected diagnostics, and pinned tsgo oracle rows.
- Add reviewed correction-overlay entries only for clear conceded TypeScript bugs; the runtime resolver remains single-spec.
- Add incremental invalidation/read-set wiring, complete-only cache admission, cancellation/budget handling, and PER0 work counters.
- Add promotion evidence mapping this generated node to exact `DiagnosticAuthorityKey` rows in NCK6.

## Internal subblocks

### NCF-BD-INIT-SB1 - Oracle and RED characterization

- Partition the exact semantic cases, negative controls, recovery/profile dimensions, and expected authored anchors.
- Produce failing native tests before implementation and a pinned external oracle snapshot.
- Prove the harness detects wrong code, missing/extra diagnostic, wrong primary/related anchor, wrong completeness, and wrong fix intent.

### NCF-BD-INIT-SB2 - Fact-demand and proof contract

- Enumerate every fact/proof/query read and exact environment dimension.
- Add a typed read-only fact view if necessary at the existing owner.
- Abort/rescope if the proposed evaluator would walk source text/types or duplicate semantic resolution.

### NCF-BD-INIT-SB3 - Bounded rule implementation

- Implement one or more small `DiagnosticRule` evaluators sharing NCK3 infrastructure.
- Use semantic subjects and proof references; no message-text/source-regex/generated-TSX semantic decisions.
- Emit stable diagnostic/family/rule/subject/anchor identity and typed fix intents only where safely specified.

### NCF-BD-INIT-SB4 - Incremental, cancellation, and admission closure

- Key by exact semantic subject/profile/env/read-set identity.
- Admit only complete results; cancelled, stale, budget-exceeded, NeedInputs, unsupported, or partial results are ReturnOnly.
- Prove targeted invalidation and incremental-equals-fresh across relevant edits/config/lib/project changes.

### NCF-BD-INIT-SB5 - Parity and correction-overlay certification

- Compare canonical native diagnostics against the pinned oracle across all manifest rows/property fixtures.
- Classify every mismatch as implementation defect, unsupported/rescope, oracle defect, or reviewed correction overlay.
- Bind implementation, oracle, overlay, fixture, toolchain, and evidence digests in the certification receipt.

### NCF-BD-INIT-SB6 - Family-scoped promotion and zero-provider proof

- Promote only exact profile/family/slice rows through NCK6.
- Prove ObserveNative was non-publishing, CertifiedNative atomically suppresses external diagnostics, and rollback is receipt-backed.
- Prove warm certified requests perform zero external diagnostic work for this slice.

## Acceptance IDs

- **NCF-BD-INIT-AC-ORACLE:** exact row coverage and planted wrong-answer sensitivity.
- **NCF-BD-INIT-AC-ONE-ENGINE:** no checker-private resolver/walker/source-text/generated-text semantic path.
- **NCF-BD-INIT-AC-FACTS:** every result records exact shared fact/proof read set and environment identity.
- **NCF-BD-INIT-AC-INCREMENTAL:** incremental equals fresh; targeted edits invalidate exactly.
- **NCF-BD-INIT-AC-ADMISSION:** cancelled/stale/partial/budget/NeedInputs outcomes never warm-admit as complete.
- **NCF-BD-INIT-AC-PARITY:** all required rows match oracle or carry accepted correction-overlay evidence.
- **NCF-BD-INIT-AC-PROMOTION:** exact slice promotion has no duplicate/gap and certified warm runtime performs zero provider diagnostic work.
- **NCF-BD-INIT-AC-WORK:** equivalent-work, allocation, latency, and retained-memory thresholds pass.

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

1. Run generated hermetic rows and negative controls for `NCF-BD-INIT`.
2. Run pinned oracle comparison and correction-overlay validation.
3. Run incremental/fresh, cancellation/budget, cache-admission, authority promotion/rollback, provider-zero-work, allocation/latency/RSS tests.
4. Run the canonical targeted gate and independent semantic review on the landing-frozen candidate.

## Deletion and residual ownership

Delete only legacy diagnostic routes/tests/ignored rows exactly mapped to this slice after promotion. Neighboring diagnostic families and remaining external provider ownership are untouched and remain capability-visible.


---

<!-- unified-charter-v2
id=NCF-RO-ASSIGN
name=Assignment and return assignability diagnostics
phase=expansion
train=expansion.native-checker
product=native_checker
kind=implementation
semantic_role=delivery
class=successor-generated
predecessors=NCK4,NCK6,D8
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
charter=charters/expansion-native-checker/generated-families/NCF-RO-ASSIGN.md
max_production_loc=800
max_production_files=8
max_related_packages=2
rescope_loc=1500
rescope_files=12
rescope_unrelated_packages=3
initial_state=
-->

# NCF-RO-ASSIGN - Assignment and return assignability diagnostics

## Independently acceptable outcome

Implement, certify, and promote exactly one native checker feature slice for **assignment, variable initialization, return/yield/awaited return, destructuring, argument/result relation sites, and satisfies assertions**. The slice consumes shared semantic facts and proofs, emits first-class authored diagnostics through NCK2/NCK3, passes the NCK4 hermetic oracle/correction-overlay contract, and promotes through NCK6 at exact family/slice/profile identity. It does not broaden adjacent semantic families.

## Architectural boundary

This node is generated from `native-checker-family-manifest.toml`. Its identity, predecessor set, scope, evidence keys, and budgets are manifest-derived. The node owns diagnostic evaluation for this slice only. Symbol/type/relation/call/flow/module/project facts remain owned by their existing semantic authorities; provider TypeScript remains oracle/fallback until promotion and is never called from the native query.

### Required fact and proof inputs

- Relate outcomes and proofs
- contextual target types
- FlowReturn/awaited/generator facts
- source semantic subjects

### Required oracle obligations

- incompatible initializer
- wrong return type
- invalid destructuring target
- failed satisfies relation

## Expected production changes

- Add the exact `DiagnosticRuleSpec` rows and rule evaluators for this slice.
- Add any missing typed fact view or proof reference at the lowest existing semantic owner; do not create a checker-private fact store or resolver.
- Add hermetic source fixtures, canonical expected diagnostics, and pinned tsgo oracle rows.
- Add reviewed correction-overlay entries only for clear conceded TypeScript bugs; the runtime resolver remains single-spec.
- Add incremental invalidation/read-set wiring, complete-only cache admission, cancellation/budget handling, and PER0 work counters.
- Add promotion evidence mapping this generated node to exact `DiagnosticAuthorityKey` rows in NCK6.

## Internal subblocks

### NCF-RO-ASSIGN-SB1 - Oracle and RED characterization

- Partition the exact semantic cases, negative controls, recovery/profile dimensions, and expected authored anchors.
- Produce failing native tests before implementation and a pinned external oracle snapshot.
- Prove the harness detects wrong code, missing/extra diagnostic, wrong primary/related anchor, wrong completeness, and wrong fix intent.

### NCF-RO-ASSIGN-SB2 - Fact-demand and proof contract

- Enumerate every fact/proof/query read and exact environment dimension.
- Add a typed read-only fact view if necessary at the existing owner.
- Abort/rescope if the proposed evaluator would walk source text/types or duplicate semantic resolution.

### NCF-RO-ASSIGN-SB3 - Bounded rule implementation

- Implement one or more small `DiagnosticRule` evaluators sharing NCK3 infrastructure.
- Use semantic subjects and proof references; no message-text/source-regex/generated-TSX semantic decisions.
- Emit stable diagnostic/family/rule/subject/anchor identity and typed fix intents only where safely specified.

### NCF-RO-ASSIGN-SB4 - Incremental, cancellation, and admission closure

- Key by exact semantic subject/profile/env/read-set identity.
- Admit only complete results; cancelled, stale, budget-exceeded, NeedInputs, unsupported, or partial results are ReturnOnly.
- Prove targeted invalidation and incremental-equals-fresh across relevant edits/config/lib/project changes.

### NCF-RO-ASSIGN-SB5 - Parity and correction-overlay certification

- Compare canonical native diagnostics against the pinned oracle across all manifest rows/property fixtures.
- Classify every mismatch as implementation defect, unsupported/rescope, oracle defect, or reviewed correction overlay.
- Bind implementation, oracle, overlay, fixture, toolchain, and evidence digests in the certification receipt.

### NCF-RO-ASSIGN-SB6 - Family-scoped promotion and zero-provider proof

- Promote only exact profile/family/slice rows through NCK6.
- Prove ObserveNative was non-publishing, CertifiedNative atomically suppresses external diagnostics, and rollback is receipt-backed.
- Prove warm certified requests perform zero external diagnostic work for this slice.

## Acceptance IDs

- **NCF-RO-ASSIGN-AC-ORACLE:** exact row coverage and planted wrong-answer sensitivity.
- **NCF-RO-ASSIGN-AC-ONE-ENGINE:** no checker-private resolver/walker/source-text/generated-text semantic path.
- **NCF-RO-ASSIGN-AC-FACTS:** every result records exact shared fact/proof read set and environment identity.
- **NCF-RO-ASSIGN-AC-INCREMENTAL:** incremental equals fresh; targeted edits invalidate exactly.
- **NCF-RO-ASSIGN-AC-ADMISSION:** cancelled/stale/partial/budget/NeedInputs outcomes never warm-admit as complete.
- **NCF-RO-ASSIGN-AC-PARITY:** all required rows match oracle or carry accepted correction-overlay evidence.
- **NCF-RO-ASSIGN-AC-PROMOTION:** exact slice promotion has no duplicate/gap and certified warm runtime performs zero provider diagnostic work.
- **NCF-RO-ASSIGN-AC-WORK:** equivalent-work, allocation, latency, and retained-memory thresholds pass.

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

1. Run generated hermetic rows and negative controls for `NCF-RO-ASSIGN`.
2. Run pinned oracle comparison and correction-overlay validation.
3. Run incremental/fresh, cancellation/budget, cache-admission, authority promotion/rollback, provider-zero-work, allocation/latency/RSS tests.
4. Run the canonical targeted gate and independent semantic review on the landing-frozen candidate.

## Deletion and residual ownership

Delete only legacy diagnostic routes/tests/ignored rows exactly mapped to this slice after promotion. Neighboring diagnostic families and remaining external provider ownership are untouched and remain capability-visible.


---

<!-- unified-charter-v2
id=NCF-RO-OPER
name=Operator and property/index access diagnostics
phase=expansion
train=expansion.native-checker
product=native_checker
kind=implementation
semantic_role=delivery
class=successor-generated
predecessors=NCK4,NCK6,D8
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
charter=charters/expansion-native-checker/generated-families/NCF-RO-OPER.md
max_production_loc=800
max_production_files=8
max_related_packages=2
rescope_loc=1500
rescope_files=12
rescope_unrelated_packages=3
initial_state=
-->

# NCF-RO-OPER - Operator and property/index access diagnostics

## Independently acceptable outcome

Implement, certify, and promote exactly one native checker feature slice for **unary/binary/logical/comparison/operator applicability, property access, element access, optional chaining, delete/in/instanceof/typeof semantics**. The slice consumes shared semantic facts and proofs, emits first-class authored diagnostics through NCK2/NCK3, passes the NCK4 hermetic oracle/correction-overlay contract, and promotes through NCK6 at exact family/slice/profile identity. It does not broaden adjacent semantic families.

## Architectural boundary

This node is generated from `native-checker-family-manifest.toml`. Its identity, predecessor set, scope, evidence keys, and budgets are manifest-derived. The node owns diagnostic evaluation for this slice only. Symbol/type/relation/call/flow/module/project facts remain owned by their existing semantic authorities; provider TypeScript remains oracle/fallback until promotion and is never called from the native query.

### Required fact and proof inputs

- resolved operand types
- operator applicability relation proofs
- object/member/index signatures
- flow narrowing and optionality facts

### Required oracle obligations

- operator on incompatible operands
- missing property
- invalid index type
- possibly null/undefined access

## Expected production changes

- Add the exact `DiagnosticRuleSpec` rows and rule evaluators for this slice.
- Add any missing typed fact view or proof reference at the lowest existing semantic owner; do not create a checker-private fact store or resolver.
- Add hermetic source fixtures, canonical expected diagnostics, and pinned tsgo oracle rows.
- Add reviewed correction-overlay entries only for clear conceded TypeScript bugs; the runtime resolver remains single-spec.
- Add incremental invalidation/read-set wiring, complete-only cache admission, cancellation/budget handling, and PER0 work counters.
- Add promotion evidence mapping this generated node to exact `DiagnosticAuthorityKey` rows in NCK6.

## Internal subblocks

### NCF-RO-OPER-SB1 - Oracle and RED characterization

- Partition the exact semantic cases, negative controls, recovery/profile dimensions, and expected authored anchors.
- Produce failing native tests before implementation and a pinned external oracle snapshot.
- Prove the harness detects wrong code, missing/extra diagnostic, wrong primary/related anchor, wrong completeness, and wrong fix intent.

### NCF-RO-OPER-SB2 - Fact-demand and proof contract

- Enumerate every fact/proof/query read and exact environment dimension.
- Add a typed read-only fact view if necessary at the existing owner.
- Abort/rescope if the proposed evaluator would walk source text/types or duplicate semantic resolution.

### NCF-RO-OPER-SB3 - Bounded rule implementation

- Implement one or more small `DiagnosticRule` evaluators sharing NCK3 infrastructure.
- Use semantic subjects and proof references; no message-text/source-regex/generated-TSX semantic decisions.
- Emit stable diagnostic/family/rule/subject/anchor identity and typed fix intents only where safely specified.

### NCF-RO-OPER-SB4 - Incremental, cancellation, and admission closure

- Key by exact semantic subject/profile/env/read-set identity.
- Admit only complete results; cancelled, stale, budget-exceeded, NeedInputs, unsupported, or partial results are ReturnOnly.
- Prove targeted invalidation and incremental-equals-fresh across relevant edits/config/lib/project changes.

### NCF-RO-OPER-SB5 - Parity and correction-overlay certification

- Compare canonical native diagnostics against the pinned oracle across all manifest rows/property fixtures.
- Classify every mismatch as implementation defect, unsupported/rescope, oracle defect, or reviewed correction overlay.
- Bind implementation, oracle, overlay, fixture, toolchain, and evidence digests in the certification receipt.

### NCF-RO-OPER-SB6 - Family-scoped promotion and zero-provider proof

- Promote only exact profile/family/slice rows through NCK6.
- Prove ObserveNative was non-publishing, CertifiedNative atomically suppresses external diagnostics, and rollback is receipt-backed.
- Prove warm certified requests perform zero external diagnostic work for this slice.

## Acceptance IDs

- **NCF-RO-OPER-AC-ORACLE:** exact row coverage and planted wrong-answer sensitivity.
- **NCF-RO-OPER-AC-ONE-ENGINE:** no checker-private resolver/walker/source-text/generated-text semantic path.
- **NCF-RO-OPER-AC-FACTS:** every result records exact shared fact/proof read set and environment identity.
- **NCF-RO-OPER-AC-INCREMENTAL:** incremental equals fresh; targeted edits invalidate exactly.
- **NCF-RO-OPER-AC-ADMISSION:** cancelled/stale/partial/budget/NeedInputs outcomes never warm-admit as complete.
- **NCF-RO-OPER-AC-PARITY:** all required rows match oracle or carry accepted correction-overlay evidence.
- **NCF-RO-OPER-AC-PROMOTION:** exact slice promotion has no duplicate/gap and certified warm runtime performs zero provider diagnostic work.
- **NCF-RO-OPER-AC-WORK:** equivalent-work, allocation, latency, and retained-memory thresholds pass.

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

1. Run generated hermetic rows and negative controls for `NCF-RO-OPER`.
2. Run pinned oracle comparison and correction-overlay validation.
3. Run incremental/fresh, cancellation/budget, cache-admission, authority promotion/rollback, provider-zero-work, allocation/latency/RSS tests.
4. Run the canonical targeted gate and independent semantic review on the landing-frozen candidate.

## Deletion and residual ownership

Delete only legacy diagnostic routes/tests/ignored rows exactly mapped to this slice after promotion. Neighboring diagnostic families and remaining external provider ownership are untouched and remain capability-visible.


---

<!-- unified-charter-v2
id=NCF-RO-EXCESS
name=Freshness, excess-property, and object conformance diagnostics
phase=expansion
train=expansion.native-checker
product=native_checker
kind=implementation
semantic_role=delivery
class=successor-generated
predecessors=NCK4,NCK6,D8
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
charter=charters/expansion-native-checker/generated-families/NCF-RO-EXCESS.md
max_production_loc=800
max_production_files=8
max_related_packages=2
rescope_loc=1500
rescope_files=12
rescope_unrelated_packages=3
initial_state=
-->

# NCF-RO-EXCESS - Freshness, excess-property, and object conformance diagnostics

## Independently acceptable outcome

Implement, certify, and promote exactly one native checker feature slice for **fresh object literal checks, excess properties, weak targets, exactness/freshness, spread interactions, discriminated object conformance, and contextual object sites**. The slice consumes shared semantic facts and proofs, emits first-class authored diagnostics through NCK2/NCK3, passes the NCK4 hermetic oracle/correction-overlay contract, and promotes through NCK6 at exact family/slice/profile identity. It does not broaden adjacent semantic families.

## Architectural boundary

This node is generated from `native-checker-family-manifest.toml`. Its identity, predecessor set, scope, evidence keys, and budgets are manifest-derived. The node owns diagnostic evaluation for this slice only. Symbol/type/relation/call/flow/module/project facts remain owned by their existing semantic authorities; provider TypeScript remains oracle/fallback until promotion and is never called from the native query.

### Required fact and proof inputs

- object projection facts
- freshness/contextual typing state
- Relate proofs
- spread/correlation evidence

### Required oracle obligations

- unknown object literal property
- weak target no common properties
- spread-induced excess behavior
- discriminant mismatch

## Expected production changes

- Add the exact `DiagnosticRuleSpec` rows and rule evaluators for this slice.
- Add any missing typed fact view or proof reference at the lowest existing semantic owner; do not create a checker-private fact store or resolver.
- Add hermetic source fixtures, canonical expected diagnostics, and pinned tsgo oracle rows.
- Add reviewed correction-overlay entries only for clear conceded TypeScript bugs; the runtime resolver remains single-spec.
- Add incremental invalidation/read-set wiring, complete-only cache admission, cancellation/budget handling, and PER0 work counters.
- Add promotion evidence mapping this generated node to exact `DiagnosticAuthorityKey` rows in NCK6.

## Internal subblocks

### NCF-RO-EXCESS-SB1 - Oracle and RED characterization

- Partition the exact semantic cases, negative controls, recovery/profile dimensions, and expected authored anchors.
- Produce failing native tests before implementation and a pinned external oracle snapshot.
- Prove the harness detects wrong code, missing/extra diagnostic, wrong primary/related anchor, wrong completeness, and wrong fix intent.

### NCF-RO-EXCESS-SB2 - Fact-demand and proof contract

- Enumerate every fact/proof/query read and exact environment dimension.
- Add a typed read-only fact view if necessary at the existing owner.
- Abort/rescope if the proposed evaluator would walk source text/types or duplicate semantic resolution.

### NCF-RO-EXCESS-SB3 - Bounded rule implementation

- Implement one or more small `DiagnosticRule` evaluators sharing NCK3 infrastructure.
- Use semantic subjects and proof references; no message-text/source-regex/generated-TSX semantic decisions.
- Emit stable diagnostic/family/rule/subject/anchor identity and typed fix intents only where safely specified.

### NCF-RO-EXCESS-SB4 - Incremental, cancellation, and admission closure

- Key by exact semantic subject/profile/env/read-set identity.
- Admit only complete results; cancelled, stale, budget-exceeded, NeedInputs, unsupported, or partial results are ReturnOnly.
- Prove targeted invalidation and incremental-equals-fresh across relevant edits/config/lib/project changes.

### NCF-RO-EXCESS-SB5 - Parity and correction-overlay certification

- Compare canonical native diagnostics against the pinned oracle across all manifest rows/property fixtures.
- Classify every mismatch as implementation defect, unsupported/rescope, oracle defect, or reviewed correction overlay.
- Bind implementation, oracle, overlay, fixture, toolchain, and evidence digests in the certification receipt.

### NCF-RO-EXCESS-SB6 - Family-scoped promotion and zero-provider proof

- Promote only exact profile/family/slice rows through NCK6.
- Prove ObserveNative was non-publishing, CertifiedNative atomically suppresses external diagnostics, and rollback is receipt-backed.
- Prove warm certified requests perform zero external diagnostic work for this slice.

## Acceptance IDs

- **NCF-RO-EXCESS-AC-ORACLE:** exact row coverage and planted wrong-answer sensitivity.
- **NCF-RO-EXCESS-AC-ONE-ENGINE:** no checker-private resolver/walker/source-text/generated-text semantic path.
- **NCF-RO-EXCESS-AC-FACTS:** every result records exact shared fact/proof read set and environment identity.
- **NCF-RO-EXCESS-AC-INCREMENTAL:** incremental equals fresh; targeted edits invalidate exactly.
- **NCF-RO-EXCESS-AC-ADMISSION:** cancelled/stale/partial/budget/NeedInputs outcomes never warm-admit as complete.
- **NCF-RO-EXCESS-AC-PARITY:** all required rows match oracle or carry accepted correction-overlay evidence.
- **NCF-RO-EXCESS-AC-PROMOTION:** exact slice promotion has no duplicate/gap and certified warm runtime performs zero provider diagnostic work.
- **NCF-RO-EXCESS-AC-WORK:** equivalent-work, allocation, latency, and retained-memory thresholds pass.

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

1. Run generated hermetic rows and negative controls for `NCF-RO-EXCESS`.
2. Run pinned oracle comparison and correction-overlay validation.
3. Run incremental/fresh, cancellation/budget, cache-admission, authority promotion/rollback, provider-zero-work, allocation/latency/RSS tests.
4. Run the canonical targeted gate and independent semantic review on the landing-frozen candidate.

## Deletion and residual ownership

Delete only legacy diagnostic routes/tests/ignored rows exactly mapped to this slice after promotion. Neighboring diagnostic families and remaining external provider ownership are untouched and remain capability-visible.


---

<!-- unified-charter-v2
id=NCF-CO-CALL
name=Call, construct, tag, and invocation applicability diagnostics
phase=expansion
train=expansion.native-checker
product=native_checker
kind=implementation
semantic_role=delivery
class=successor-generated
predecessors=NCK4,NCK6,D8
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
charter=charters/expansion-native-checker/generated-families/NCF-CO-CALL.md
max_production_loc=800
max_production_files=8
max_related_packages=2
rescope_loc=1500
rescope_files=12
rescope_unrelated_packages=3
initial_state=
-->

# NCF-CO-CALL - Call, construct, tag, and invocation applicability diagnostics

## Independently acceptable outcome

Implement, certify, and promote exactly one native checker feature slice for **function/method/constructor/tagged-template/new/call signatures, arity, optional/rest parameters, this argument, and callable/constructable checks**. The slice consumes shared semantic facts and proofs, emits first-class authored diagnostics through NCK2/NCK3, passes the NCK4 hermetic oracle/correction-overlay contract, and promotes through NCK6 at exact family/slice/profile identity. It does not broaden adjacent semantic families.

## Architectural boundary

This node is generated from `native-checker-family-manifest.toml`. Its identity, predecessor set, scope, evidence keys, and budgets are manifest-derived. The node owns diagnostic evaluation for this slice only. Symbol/type/relation/call/flow/module/project facts remain owned by their existing semantic authorities; provider TypeScript remains oracle/fallback until promotion and is never called from the native query.

### Required fact and proof inputs

- ResolveCall result
- signature/this/parameter facts
- argument contextual types
- relation proofs

### Required oracle obligations

- not callable/constructable
- wrong arity
- argument mismatch
- invalid this context

## Expected production changes

- Add the exact `DiagnosticRuleSpec` rows and rule evaluators for this slice.
- Add any missing typed fact view or proof reference at the lowest existing semantic owner; do not create a checker-private fact store or resolver.
- Add hermetic source fixtures, canonical expected diagnostics, and pinned tsgo oracle rows.
- Add reviewed correction-overlay entries only for clear conceded TypeScript bugs; the runtime resolver remains single-spec.
- Add incremental invalidation/read-set wiring, complete-only cache admission, cancellation/budget handling, and PER0 work counters.
- Add promotion evidence mapping this generated node to exact `DiagnosticAuthorityKey` rows in NCK6.

## Internal subblocks

### NCF-CO-CALL-SB1 - Oracle and RED characterization

- Partition the exact semantic cases, negative controls, recovery/profile dimensions, and expected authored anchors.
- Produce failing native tests before implementation and a pinned external oracle snapshot.
- Prove the harness detects wrong code, missing/extra diagnostic, wrong primary/related anchor, wrong completeness, and wrong fix intent.

### NCF-CO-CALL-SB2 - Fact-demand and proof contract

- Enumerate every fact/proof/query read and exact environment dimension.
- Add a typed read-only fact view if necessary at the existing owner.
- Abort/rescope if the proposed evaluator would walk source text/types or duplicate semantic resolution.

### NCF-CO-CALL-SB3 - Bounded rule implementation

- Implement one or more small `DiagnosticRule` evaluators sharing NCK3 infrastructure.
- Use semantic subjects and proof references; no message-text/source-regex/generated-TSX semantic decisions.
- Emit stable diagnostic/family/rule/subject/anchor identity and typed fix intents only where safely specified.

### NCF-CO-CALL-SB4 - Incremental, cancellation, and admission closure

- Key by exact semantic subject/profile/env/read-set identity.
- Admit only complete results; cancelled, stale, budget-exceeded, NeedInputs, unsupported, or partial results are ReturnOnly.
- Prove targeted invalidation and incremental-equals-fresh across relevant edits/config/lib/project changes.

### NCF-CO-CALL-SB5 - Parity and correction-overlay certification

- Compare canonical native diagnostics against the pinned oracle across all manifest rows/property fixtures.
- Classify every mismatch as implementation defect, unsupported/rescope, oracle defect, or reviewed correction overlay.
- Bind implementation, oracle, overlay, fixture, toolchain, and evidence digests in the certification receipt.

### NCF-CO-CALL-SB6 - Family-scoped promotion and zero-provider proof

- Promote only exact profile/family/slice rows through NCK6.
- Prove ObserveNative was non-publishing, CertifiedNative atomically suppresses external diagnostics, and rollback is receipt-backed.
- Prove warm certified requests perform zero external diagnostic work for this slice.

## Acceptance IDs

- **NCF-CO-CALL-AC-ORACLE:** exact row coverage and planted wrong-answer sensitivity.
- **NCF-CO-CALL-AC-ONE-ENGINE:** no checker-private resolver/walker/source-text/generated-text semantic path.
- **NCF-CO-CALL-AC-FACTS:** every result records exact shared fact/proof read set and environment identity.
- **NCF-CO-CALL-AC-INCREMENTAL:** incremental equals fresh; targeted edits invalidate exactly.
- **NCF-CO-CALL-AC-ADMISSION:** cancelled/stale/partial/budget/NeedInputs outcomes never warm-admit as complete.
- **NCF-CO-CALL-AC-PARITY:** all required rows match oracle or carry accepted correction-overlay evidence.
- **NCF-CO-CALL-AC-PROMOTION:** exact slice promotion has no duplicate/gap and certified warm runtime performs zero provider diagnostic work.
- **NCF-CO-CALL-AC-WORK:** equivalent-work, allocation, latency, and retained-memory thresholds pass.

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

1. Run generated hermetic rows and negative controls for `NCF-CO-CALL`.
2. Run pinned oracle comparison and correction-overlay validation.
3. Run incremental/fresh, cancellation/budget, cache-admission, authority promotion/rollback, provider-zero-work, allocation/latency/RSS tests.
4. Run the canonical targeted gate and independent semantic review on the landing-frozen candidate.

## Deletion and residual ownership

Delete only legacy diagnostic routes/tests/ignored rows exactly mapped to this slice after promotion. Neighboring diagnostic families and remaining external provider ownership are untouched and remain capability-visible.


---

<!-- unified-charter-v2
id=NCF-CO-OVER
name=Overload resolution and implementation conformance diagnostics
phase=expansion
train=expansion.native-checker
product=native_checker
kind=implementation
semantic_role=delivery
class=successor-generated
predecessors=NCK4,NCK6,D8
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
charter=charters/expansion-native-checker/generated-families/NCF-CO-OVER.md
max_production_loc=800
max_production_files=8
max_related_packages=2
rescope_loc=1500
rescope_files=12
rescope_unrelated_packages=3
initial_state=
-->

# NCF-CO-OVER - Overload resolution and implementation conformance diagnostics

## Independently acceptable outcome

Implement, certify, and promote exactly one native checker feature slice for **overload candidate applicability, ambiguity, best-signature selection, implementation signature compatibility, and overload declaration ordering**. The slice consumes shared semantic facts and proofs, emits first-class authored diagnostics through NCK2/NCK3, passes the NCK4 hermetic oracle/correction-overlay contract, and promotes through NCK6 at exact family/slice/profile identity. It does not broaden adjacent semantic families.

## Architectural boundary

This node is generated from `native-checker-family-manifest.toml`. Its identity, predecessor set, scope, evidence keys, and budgets are manifest-derived. The node owns diagnostic evaluation for this slice only. Symbol/type/relation/call/flow/module/project facts remain owned by their existing semantic authorities; provider TypeScript remains oracle/fallback until promotion and is never called from the native query.

### Required fact and proof inputs

- ResolveOverloadSet/ResolveCall proofs
- signature identity/effects
- relation and inference sessions
- declaration merge/order facts

### Required oracle obligations

- no overload matches
- ambiguous call
- implementation incompatible with overload
- invalid overload ordering

## Expected production changes

- Add the exact `DiagnosticRuleSpec` rows and rule evaluators for this slice.
- Add any missing typed fact view or proof reference at the lowest existing semantic owner; do not create a checker-private fact store or resolver.
- Add hermetic source fixtures, canonical expected diagnostics, and pinned tsgo oracle rows.
- Add reviewed correction-overlay entries only for clear conceded TypeScript bugs; the runtime resolver remains single-spec.
- Add incremental invalidation/read-set wiring, complete-only cache admission, cancellation/budget handling, and PER0 work counters.
- Add promotion evidence mapping this generated node to exact `DiagnosticAuthorityKey` rows in NCK6.

## Internal subblocks

### NCF-CO-OVER-SB1 - Oracle and RED characterization

- Partition the exact semantic cases, negative controls, recovery/profile dimensions, and expected authored anchors.
- Produce failing native tests before implementation and a pinned external oracle snapshot.
- Prove the harness detects wrong code, missing/extra diagnostic, wrong primary/related anchor, wrong completeness, and wrong fix intent.

### NCF-CO-OVER-SB2 - Fact-demand and proof contract

- Enumerate every fact/proof/query read and exact environment dimension.
- Add a typed read-only fact view if necessary at the existing owner.
- Abort/rescope if the proposed evaluator would walk source text/types or duplicate semantic resolution.

### NCF-CO-OVER-SB3 - Bounded rule implementation

- Implement one or more small `DiagnosticRule` evaluators sharing NCK3 infrastructure.
- Use semantic subjects and proof references; no message-text/source-regex/generated-TSX semantic decisions.
- Emit stable diagnostic/family/rule/subject/anchor identity and typed fix intents only where safely specified.

### NCF-CO-OVER-SB4 - Incremental, cancellation, and admission closure

- Key by exact semantic subject/profile/env/read-set identity.
- Admit only complete results; cancelled, stale, budget-exceeded, NeedInputs, unsupported, or partial results are ReturnOnly.
- Prove targeted invalidation and incremental-equals-fresh across relevant edits/config/lib/project changes.

### NCF-CO-OVER-SB5 - Parity and correction-overlay certification

- Compare canonical native diagnostics against the pinned oracle across all manifest rows/property fixtures.
- Classify every mismatch as implementation defect, unsupported/rescope, oracle defect, or reviewed correction overlay.
- Bind implementation, oracle, overlay, fixture, toolchain, and evidence digests in the certification receipt.

### NCF-CO-OVER-SB6 - Family-scoped promotion and zero-provider proof

- Promote only exact profile/family/slice rows through NCK6.
- Prove ObserveNative was non-publishing, CertifiedNative atomically suppresses external diagnostics, and rollback is receipt-backed.
- Prove warm certified requests perform zero external diagnostic work for this slice.

## Acceptance IDs

- **NCF-CO-OVER-AC-ORACLE:** exact row coverage and planted wrong-answer sensitivity.
- **NCF-CO-OVER-AC-ONE-ENGINE:** no checker-private resolver/walker/source-text/generated-text semantic path.
- **NCF-CO-OVER-AC-FACTS:** every result records exact shared fact/proof read set and environment identity.
- **NCF-CO-OVER-AC-INCREMENTAL:** incremental equals fresh; targeted edits invalidate exactly.
- **NCF-CO-OVER-AC-ADMISSION:** cancelled/stale/partial/budget/NeedInputs outcomes never warm-admit as complete.
- **NCF-CO-OVER-AC-PARITY:** all required rows match oracle or carry accepted correction-overlay evidence.
- **NCF-CO-OVER-AC-PROMOTION:** exact slice promotion has no duplicate/gap and certified warm runtime performs zero provider diagnostic work.
- **NCF-CO-OVER-AC-WORK:** equivalent-work, allocation, latency, and retained-memory thresholds pass.

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

1. Run generated hermetic rows and negative controls for `NCF-CO-OVER`.
2. Run pinned oracle comparison and correction-overlay validation.
3. Run incremental/fresh, cancellation/budget, cache-admission, authority promotion/rollback, provider-zero-work, allocation/latency/RSS tests.
4. Run the canonical targeted gate and independent semantic review on the landing-frozen candidate.

## Deletion and residual ownership

Delete only legacy diagnostic routes/tests/ignored rows exactly mapped to this slice after promotion. Neighboring diagnostic families and remaining external provider ownership are untouched and remain capability-visible.


---

<!-- unified-charter-v2
id=NCF-CO-INFER
name=Generic inference, constraints, defaults, and instantiation diagnostics
phase=expansion
train=expansion.native-checker
product=native_checker
kind=implementation
semantic_role=delivery
class=successor-generated
predecessors=NCK4,NCK6,D8
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
charter=charters/expansion-native-checker/generated-families/NCF-CO-INFER.md
max_production_loc=800
max_production_files=8
max_related_packages=2
rescope_loc=1500
rescope_files=12
rescope_unrelated_packages=3
initial_state=
-->

# NCF-CO-INFER - Generic inference, constraints, defaults, and instantiation diagnostics

## Independently acceptable outcome

Implement, certify, and promote exactly one native checker feature slice for **type argument count, constraint satisfaction, inference failure, NoInfer/const/default parameters, partial inference, and generic instantiation depth/budget**. The slice consumes shared semantic facts and proofs, emits first-class authored diagnostics through NCK2/NCK3, passes the NCK4 hermetic oracle/correction-overlay contract, and promotes through NCK6 at exact family/slice/profile identity. It does not broaden adjacent semantic families.

## Architectural boundary

This node is generated from `native-checker-family-manifest.toml`. Its identity, predecessor set, scope, evidence keys, and budgets are manifest-derived. The node owns diagnostic evaluation for this slice only. Symbol/type/relation/call/flow/module/project facts remain owned by their existing semantic authorities; provider TypeScript remains oracle/fallback until promotion and is never called from the native query.

### Required fact and proof inputs

- inference session result/evidence
- type parameter constraints/defaults/variance
- Relate proofs
- instantiation cycle/budget facts

### Required oracle obligations

- type argument violates constraint
- cannot infer type parameter
- wrong type argument count
- excessive instantiation

## Expected production changes

- Add the exact `DiagnosticRuleSpec` rows and rule evaluators for this slice.
- Add any missing typed fact view or proof reference at the lowest existing semantic owner; do not create a checker-private fact store or resolver.
- Add hermetic source fixtures, canonical expected diagnostics, and pinned tsgo oracle rows.
- Add reviewed correction-overlay entries only for clear conceded TypeScript bugs; the runtime resolver remains single-spec.
- Add incremental invalidation/read-set wiring, complete-only cache admission, cancellation/budget handling, and PER0 work counters.
- Add promotion evidence mapping this generated node to exact `DiagnosticAuthorityKey` rows in NCK6.

## Internal subblocks

### NCF-CO-INFER-SB1 - Oracle and RED characterization

- Partition the exact semantic cases, negative controls, recovery/profile dimensions, and expected authored anchors.
- Produce failing native tests before implementation and a pinned external oracle snapshot.
- Prove the harness detects wrong code, missing/extra diagnostic, wrong primary/related anchor, wrong completeness, and wrong fix intent.

### NCF-CO-INFER-SB2 - Fact-demand and proof contract

- Enumerate every fact/proof/query read and exact environment dimension.
- Add a typed read-only fact view if necessary at the existing owner.
- Abort/rescope if the proposed evaluator would walk source text/types or duplicate semantic resolution.

### NCF-CO-INFER-SB3 - Bounded rule implementation

- Implement one or more small `DiagnosticRule` evaluators sharing NCK3 infrastructure.
- Use semantic subjects and proof references; no message-text/source-regex/generated-TSX semantic decisions.
- Emit stable diagnostic/family/rule/subject/anchor identity and typed fix intents only where safely specified.

### NCF-CO-INFER-SB4 - Incremental, cancellation, and admission closure

- Key by exact semantic subject/profile/env/read-set identity.
- Admit only complete results; cancelled, stale, budget-exceeded, NeedInputs, unsupported, or partial results are ReturnOnly.
- Prove targeted invalidation and incremental-equals-fresh across relevant edits/config/lib/project changes.

### NCF-CO-INFER-SB5 - Parity and correction-overlay certification

- Compare canonical native diagnostics against the pinned oracle across all manifest rows/property fixtures.
- Classify every mismatch as implementation defect, unsupported/rescope, oracle defect, or reviewed correction overlay.
- Bind implementation, oracle, overlay, fixture, toolchain, and evidence digests in the certification receipt.

### NCF-CO-INFER-SB6 - Family-scoped promotion and zero-provider proof

- Promote only exact profile/family/slice rows through NCK6.
- Prove ObserveNative was non-publishing, CertifiedNative atomically suppresses external diagnostics, and rollback is receipt-backed.
- Prove warm certified requests perform zero external diagnostic work for this slice.

## Acceptance IDs

- **NCF-CO-INFER-AC-ORACLE:** exact row coverage and planted wrong-answer sensitivity.
- **NCF-CO-INFER-AC-ONE-ENGINE:** no checker-private resolver/walker/source-text/generated-text semantic path.
- **NCF-CO-INFER-AC-FACTS:** every result records exact shared fact/proof read set and environment identity.
- **NCF-CO-INFER-AC-INCREMENTAL:** incremental equals fresh; targeted edits invalidate exactly.
- **NCF-CO-INFER-AC-ADMISSION:** cancelled/stale/partial/budget/NeedInputs outcomes never warm-admit as complete.
- **NCF-CO-INFER-AC-PARITY:** all required rows match oracle or carry accepted correction-overlay evidence.
- **NCF-CO-INFER-AC-PROMOTION:** exact slice promotion has no duplicate/gap and certified warm runtime performs zero provider diagnostic work.
- **NCF-CO-INFER-AC-WORK:** equivalent-work, allocation, latency, and retained-memory thresholds pass.

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

1. Run generated hermetic rows and negative controls for `NCF-CO-INFER`.
2. Run pinned oracle comparison and correction-overlay validation.
3. Run incremental/fresh, cancellation/budget, cache-admission, authority promotion/rollback, provider-zero-work, allocation/latency/RSS tests.
4. Run the canonical targeted gate and independent semantic review on the landing-frozen candidate.

## Deletion and residual ownership

Delete only legacy diagnostic routes/tests/ignored rows exactly mapped to this slice after promotion. Neighboring diagnostic families and remaining external provider ownership are untouched and remain capability-visible.


---

<!-- unified-charter-v2
id=NCF-CF-CONTEXT
name=Contextual typing and expression conformance diagnostics
phase=expansion
train=expansion.native-checker
product=native_checker
kind=implementation
semantic_role=delivery
class=successor-generated
predecessors=NCK4,NCK6,D8
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
charter=charters/expansion-native-checker/generated-families/NCF-CF-CONTEXT.md
max_production_loc=800
max_production_files=8
max_related_packages=2
rescope_loc=1500
rescope_files=12
rescope_unrelated_packages=3
initial_state=
-->

# NCF-CF-CONTEXT - Contextual typing and expression conformance diagnostics

## Independently acceptable outcome

Implement, certify, and promote exactly one native checker feature slice for **contextual object/array/function/JSX expression typing, best common type, contextual return/parameter typing, and context loss or mismatch**. The slice consumes shared semantic facts and proofs, emits first-class authored diagnostics through NCK2/NCK3, passes the NCK4 hermetic oracle/correction-overlay contract, and promotes through NCK6 at exact family/slice/profile identity. It does not broaden adjacent semantic families.

## Architectural boundary

This node is generated from `native-checker-family-manifest.toml`. Its identity, predecessor set, scope, evidence keys, and budgets are manifest-derived. The node owns diagnostic evaluation for this slice only. Symbol/type/relation/call/flow/module/project facts remain owned by their existing semantic authorities; provider TypeScript remains oracle/fallback until promotion and is never called from the native query.

### Required fact and proof inputs

- ContextualTypeAt
- expression observed types
- Relate proofs
- inference/call context

### Required oracle obligations

- contextual callback mismatch
- array/object contextual incompatibility
- implicit any due missing context
- context-sensitive union failure

## Expected production changes

- Add the exact `DiagnosticRuleSpec` rows and rule evaluators for this slice.
- Add any missing typed fact view or proof reference at the lowest existing semantic owner; do not create a checker-private fact store or resolver.
- Add hermetic source fixtures, canonical expected diagnostics, and pinned tsgo oracle rows.
- Add reviewed correction-overlay entries only for clear conceded TypeScript bugs; the runtime resolver remains single-spec.
- Add incremental invalidation/read-set wiring, complete-only cache admission, cancellation/budget handling, and PER0 work counters.
- Add promotion evidence mapping this generated node to exact `DiagnosticAuthorityKey` rows in NCK6.

## Internal subblocks

### NCF-CF-CONTEXT-SB1 - Oracle and RED characterization

- Partition the exact semantic cases, negative controls, recovery/profile dimensions, and expected authored anchors.
- Produce failing native tests before implementation and a pinned external oracle snapshot.
- Prove the harness detects wrong code, missing/extra diagnostic, wrong primary/related anchor, wrong completeness, and wrong fix intent.

### NCF-CF-CONTEXT-SB2 - Fact-demand and proof contract

- Enumerate every fact/proof/query read and exact environment dimension.
- Add a typed read-only fact view if necessary at the existing owner.
- Abort/rescope if the proposed evaluator would walk source text/types or duplicate semantic resolution.

### NCF-CF-CONTEXT-SB3 - Bounded rule implementation

- Implement one or more small `DiagnosticRule` evaluators sharing NCK3 infrastructure.
- Use semantic subjects and proof references; no message-text/source-regex/generated-TSX semantic decisions.
- Emit stable diagnostic/family/rule/subject/anchor identity and typed fix intents only where safely specified.

### NCF-CF-CONTEXT-SB4 - Incremental, cancellation, and admission closure

- Key by exact semantic subject/profile/env/read-set identity.
- Admit only complete results; cancelled, stale, budget-exceeded, NeedInputs, unsupported, or partial results are ReturnOnly.
- Prove targeted invalidation and incremental-equals-fresh across relevant edits/config/lib/project changes.

### NCF-CF-CONTEXT-SB5 - Parity and correction-overlay certification

- Compare canonical native diagnostics against the pinned oracle across all manifest rows/property fixtures.
- Classify every mismatch as implementation defect, unsupported/rescope, oracle defect, or reviewed correction overlay.
- Bind implementation, oracle, overlay, fixture, toolchain, and evidence digests in the certification receipt.

### NCF-CF-CONTEXT-SB6 - Family-scoped promotion and zero-provider proof

- Promote only exact profile/family/slice rows through NCK6.
- Prove ObserveNative was non-publishing, CertifiedNative atomically suppresses external diagnostics, and rollback is receipt-backed.
- Prove warm certified requests perform zero external diagnostic work for this slice.

## Acceptance IDs

- **NCF-CF-CONTEXT-AC-ORACLE:** exact row coverage and planted wrong-answer sensitivity.
- **NCF-CF-CONTEXT-AC-ONE-ENGINE:** no checker-private resolver/walker/source-text/generated-text semantic path.
- **NCF-CF-CONTEXT-AC-FACTS:** every result records exact shared fact/proof read set and environment identity.
- **NCF-CF-CONTEXT-AC-INCREMENTAL:** incremental equals fresh; targeted edits invalidate exactly.
- **NCF-CF-CONTEXT-AC-ADMISSION:** cancelled/stale/partial/budget/NeedInputs outcomes never warm-admit as complete.
- **NCF-CF-CONTEXT-AC-PARITY:** all required rows match oracle or carry accepted correction-overlay evidence.
- **NCF-CF-CONTEXT-AC-PROMOTION:** exact slice promotion has no duplicate/gap and certified warm runtime performs zero provider diagnostic work.
- **NCF-CF-CONTEXT-AC-WORK:** equivalent-work, allocation, latency, and retained-memory thresholds pass.

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

1. Run generated hermetic rows and negative controls for `NCF-CF-CONTEXT`.
2. Run pinned oracle comparison and correction-overlay validation.
3. Run incremental/fresh, cancellation/budget, cache-admission, authority promotion/rollback, provider-zero-work, allocation/latency/RSS tests.
4. Run the canonical targeted gate and independent semantic review on the landing-frozen candidate.

## Deletion and residual ownership

Delete only legacy diagnostic routes/tests/ignored rows exactly mapped to this slice after promotion. Neighboring diagnostic families and remaining external provider ownership are untouched and remain capability-visible.


---

<!-- unified-charter-v2
id=NCF-CF-VAR
name=Function variance, predicate, assertion, and effect diagnostics
phase=expansion
train=expansion.native-checker
product=native_checker
kind=implementation
semantic_role=delivery
class=successor-generated
predecessors=NCK4,NCK6,D8
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
charter=charters/expansion-native-checker/generated-families/NCF-CF-VAR.md
max_production_loc=800
max_production_files=8
max_related_packages=2
rescope_loc=1500
rescope_files=12
rescope_unrelated_packages=3
initial_state=
-->

# NCF-CF-VAR - Function variance, predicate, assertion, and effect diagnostics

## Independently acceptable outcome

Implement, certify, and promote exactly one native checker feature slice for **parameter/return variance, strictFunctionTypes behavior, predicates, assertion signatures, async/generator effects, and override callable compatibility**. The slice consumes shared semantic facts and proofs, emits first-class authored diagnostics through NCK2/NCK3, passes the NCK4 hermetic oracle/correction-overlay contract, and promotes through NCK6 at exact family/slice/profile identity. It does not broaden adjacent semantic families.

## Architectural boundary

This node is generated from `native-checker-family-manifest.toml`. Its identity, predecessor set, scope, evidence keys, and budgets are manifest-derived. The node owns diagnostic evaluation for this slice only. Symbol/type/relation/call/flow/module/project facts remain owned by their existing semantic authorities; provider TypeScript remains oracle/fallback until promotion and is never called from the native query.

### Required fact and proof inputs

- signature effects and variance
- Relate proofs
- FlowReturn
- override/implementation edges

### Required oracle obligations

- unsafe parameter variance
- invalid predicate target/type
- assertion signature misuse
- async/generator return mismatch

## Expected production changes

- Add the exact `DiagnosticRuleSpec` rows and rule evaluators for this slice.
- Add any missing typed fact view or proof reference at the lowest existing semantic owner; do not create a checker-private fact store or resolver.
- Add hermetic source fixtures, canonical expected diagnostics, and pinned tsgo oracle rows.
- Add reviewed correction-overlay entries only for clear conceded TypeScript bugs; the runtime resolver remains single-spec.
- Add incremental invalidation/read-set wiring, complete-only cache admission, cancellation/budget handling, and PER0 work counters.
- Add promotion evidence mapping this generated node to exact `DiagnosticAuthorityKey` rows in NCK6.

## Internal subblocks

### NCF-CF-VAR-SB1 - Oracle and RED characterization

- Partition the exact semantic cases, negative controls, recovery/profile dimensions, and expected authored anchors.
- Produce failing native tests before implementation and a pinned external oracle snapshot.
- Prove the harness detects wrong code, missing/extra diagnostic, wrong primary/related anchor, wrong completeness, and wrong fix intent.

### NCF-CF-VAR-SB2 - Fact-demand and proof contract

- Enumerate every fact/proof/query read and exact environment dimension.
- Add a typed read-only fact view if necessary at the existing owner.
- Abort/rescope if the proposed evaluator would walk source text/types or duplicate semantic resolution.

### NCF-CF-VAR-SB3 - Bounded rule implementation

- Implement one or more small `DiagnosticRule` evaluators sharing NCK3 infrastructure.
- Use semantic subjects and proof references; no message-text/source-regex/generated-TSX semantic decisions.
- Emit stable diagnostic/family/rule/subject/anchor identity and typed fix intents only where safely specified.

### NCF-CF-VAR-SB4 - Incremental, cancellation, and admission closure

- Key by exact semantic subject/profile/env/read-set identity.
- Admit only complete results; cancelled, stale, budget-exceeded, NeedInputs, unsupported, or partial results are ReturnOnly.
- Prove targeted invalidation and incremental-equals-fresh across relevant edits/config/lib/project changes.

### NCF-CF-VAR-SB5 - Parity and correction-overlay certification

- Compare canonical native diagnostics against the pinned oracle across all manifest rows/property fixtures.
- Classify every mismatch as implementation defect, unsupported/rescope, oracle defect, or reviewed correction overlay.
- Bind implementation, oracle, overlay, fixture, toolchain, and evidence digests in the certification receipt.

### NCF-CF-VAR-SB6 - Family-scoped promotion and zero-provider proof

- Promote only exact profile/family/slice rows through NCK6.
- Prove ObserveNative was non-publishing, CertifiedNative atomically suppresses external diagnostics, and rollback is receipt-backed.
- Prove warm certified requests perform zero external diagnostic work for this slice.

## Acceptance IDs

- **NCF-CF-VAR-AC-ORACLE:** exact row coverage and planted wrong-answer sensitivity.
- **NCF-CF-VAR-AC-ONE-ENGINE:** no checker-private resolver/walker/source-text/generated-text semantic path.
- **NCF-CF-VAR-AC-FACTS:** every result records exact shared fact/proof read set and environment identity.
- **NCF-CF-VAR-AC-INCREMENTAL:** incremental equals fresh; targeted edits invalidate exactly.
- **NCF-CF-VAR-AC-ADMISSION:** cancelled/stale/partial/budget/NeedInputs outcomes never warm-admit as complete.
- **NCF-CF-VAR-AC-PARITY:** all required rows match oracle or carry accepted correction-overlay evidence.
- **NCF-CF-VAR-AC-PROMOTION:** exact slice promotion has no duplicate/gap and certified warm runtime performs zero provider diagnostic work.
- **NCF-CF-VAR-AC-WORK:** equivalent-work, allocation, latency, and retained-memory thresholds pass.

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

1. Run generated hermetic rows and negative controls for `NCF-CF-VAR`.
2. Run pinned oracle comparison and correction-overlay validation.
3. Run incremental/fresh, cancellation/budget, cache-admission, authority promotion/rollback, provider-zero-work, allocation/latency/RSS tests.
4. Run the canonical targeted gate and independent semantic review on the landing-frozen candidate.

## Deletion and residual ownership

Delete only legacy diagnostic routes/tests/ignored rows exactly mapped to this slice after promotion. Neighboring diagnostic families and remaining external provider ownership are untouched and remain capability-visible.


---

<!-- unified-charter-v2
id=NCF-CF-THIS
name=This, super, private environment, and call-context diagnostics
phase=expansion
train=expansion.native-checker
product=native_checker
kind=implementation
semantic_role=delivery
class=successor-generated
predecessors=NCK4,NCK6,D8,IDX0
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
charter=charters/expansion-native-checker/generated-families/NCF-CF-THIS.md
max_production_loc=800
max_production_files=8
max_related_packages=2
rescope_loc=1500
rescope_files=12
rescope_unrelated_packages=3
initial_state=
-->

# NCF-CF-THIS - This, super, private environment, and call-context diagnostics

## Independently acceptable outcome

Implement, certify, and promote exactly one native checker feature slice for **this parameter/context, super call/property ordering, derived constructor initialization, static/instance context, private fields, and lexical this**. The slice consumes shared semantic facts and proofs, emits first-class authored diagnostics through NCK2/NCK3, passes the NCK4 hermetic oracle/correction-overlay contract, and promotes through NCK6 at exact family/slice/profile identity. It does not broaden adjacent semantic families.

## Architectural boundary

This node is generated from `native-checker-family-manifest.toml`. Its identity, predecessor set, scope, evidence keys, and budgets are manifest-derived. The node owns diagnostic evaluation for this slice only. Symbol/type/relation/call/flow/module/project facts remain owned by their existing semantic authorities; provider TypeScript remains oracle/fallback until promotion and is never called from the native query.

### Required fact and proof inputs

- This/call context facts
- class/heritage/private environment
- flow initialization/reachability
- relation/member facts

### Required oracle obligations

- this before super
- super outside derived class
- private field access violation
- wrong this argument

## Expected production changes

- Add the exact `DiagnosticRuleSpec` rows and rule evaluators for this slice.
- Add any missing typed fact view or proof reference at the lowest existing semantic owner; do not create a checker-private fact store or resolver.
- Add hermetic source fixtures, canonical expected diagnostics, and pinned tsgo oracle rows.
- Add reviewed correction-overlay entries only for clear conceded TypeScript bugs; the runtime resolver remains single-spec.
- Add incremental invalidation/read-set wiring, complete-only cache admission, cancellation/budget handling, and PER0 work counters.
- Add promotion evidence mapping this generated node to exact `DiagnosticAuthorityKey` rows in NCK6.

## Internal subblocks

### NCF-CF-THIS-SB1 - Oracle and RED characterization

- Partition the exact semantic cases, negative controls, recovery/profile dimensions, and expected authored anchors.
- Produce failing native tests before implementation and a pinned external oracle snapshot.
- Prove the harness detects wrong code, missing/extra diagnostic, wrong primary/related anchor, wrong completeness, and wrong fix intent.

### NCF-CF-THIS-SB2 - Fact-demand and proof contract

- Enumerate every fact/proof/query read and exact environment dimension.
- Add a typed read-only fact view if necessary at the existing owner.
- Abort/rescope if the proposed evaluator would walk source text/types or duplicate semantic resolution.

### NCF-CF-THIS-SB3 - Bounded rule implementation

- Implement one or more small `DiagnosticRule` evaluators sharing NCK3 infrastructure.
- Use semantic subjects and proof references; no message-text/source-regex/generated-TSX semantic decisions.
- Emit stable diagnostic/family/rule/subject/anchor identity and typed fix intents only where safely specified.

### NCF-CF-THIS-SB4 - Incremental, cancellation, and admission closure

- Key by exact semantic subject/profile/env/read-set identity.
- Admit only complete results; cancelled, stale, budget-exceeded, NeedInputs, unsupported, or partial results are ReturnOnly.
- Prove targeted invalidation and incremental-equals-fresh across relevant edits/config/lib/project changes.

### NCF-CF-THIS-SB5 - Parity and correction-overlay certification

- Compare canonical native diagnostics against the pinned oracle across all manifest rows/property fixtures.
- Classify every mismatch as implementation defect, unsupported/rescope, oracle defect, or reviewed correction overlay.
- Bind implementation, oracle, overlay, fixture, toolchain, and evidence digests in the certification receipt.

### NCF-CF-THIS-SB6 - Family-scoped promotion and zero-provider proof

- Promote only exact profile/family/slice rows through NCK6.
- Prove ObserveNative was non-publishing, CertifiedNative atomically suppresses external diagnostics, and rollback is receipt-backed.
- Prove warm certified requests perform zero external diagnostic work for this slice.

## Acceptance IDs

- **NCF-CF-THIS-AC-ORACLE:** exact row coverage and planted wrong-answer sensitivity.
- **NCF-CF-THIS-AC-ONE-ENGINE:** no checker-private resolver/walker/source-text/generated-text semantic path.
- **NCF-CF-THIS-AC-FACTS:** every result records exact shared fact/proof read set and environment identity.
- **NCF-CF-THIS-AC-INCREMENTAL:** incremental equals fresh; targeted edits invalidate exactly.
- **NCF-CF-THIS-AC-ADMISSION:** cancelled/stale/partial/budget/NeedInputs outcomes never warm-admit as complete.
- **NCF-CF-THIS-AC-PARITY:** all required rows match oracle or carry accepted correction-overlay evidence.
- **NCF-CF-THIS-AC-PROMOTION:** exact slice promotion has no duplicate/gap and certified warm runtime performs zero provider diagnostic work.
- **NCF-CF-THIS-AC-WORK:** equivalent-work, allocation, latency, and retained-memory thresholds pass.

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

1. Run generated hermetic rows and negative controls for `NCF-CF-THIS`.
2. Run pinned oracle comparison and correction-overlay validation.
3. Run incremental/fresh, cancellation/budget, cache-admission, authority promotion/rollback, provider-zero-work, allocation/latency/RSS tests.
4. Run the canonical targeted gate and independent semantic review on the landing-frozen candidate.

## Deletion and residual ownership

Delete only legacy diagnostic routes/tests/ignored rows exactly mapped to this slice after promotion. Neighboring diagnostic families and remaining external provider ownership are untouched and remain capability-visible.


---

<!-- unified-charter-v2
id=NCF-FD-NARROW
name=Control-flow narrowing and impossible-condition diagnostics
phase=expansion
train=expansion.native-checker
product=native_checker
kind=implementation
semantic_role=delivery
class=successor-generated
predecessors=NCK4,NCK6,D8
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
charter=charters/expansion-native-checker/generated-families/NCF-FD-NARROW.md
max_production_loc=800
max_production_files=8
max_related_packages=2
rescope_loc=1500
rescope_files=12
rescope_unrelated_packages=3
initial_state=
-->

# NCF-FD-NARROW - Control-flow narrowing and impossible-condition diagnostics

## Independently acceptable outcome

Implement, certify, and promote exactly one native checker feature slice for **typeof/instanceof/in/equality/truthiness/discriminant/user-predicate narrowing, unreachable branches from impossible conditions, and narrowing invalidation**. The slice consumes shared semantic facts and proofs, emits first-class authored diagnostics through NCK2/NCK3, passes the NCK4 hermetic oracle/correction-overlay contract, and promotes through NCK6 at exact family/slice/profile identity. It does not broaden adjacent semantic families.

## Architectural boundary

This node is generated from `native-checker-family-manifest.toml`. Its identity, predecessor set, scope, evidence keys, and budgets are manifest-derived. The node owns diagnostic evaluation for this slice only. Symbol/type/relation/call/flow/module/project facts remain owned by their existing semantic authorities; provider TypeScript remains oracle/fallback until promotion and is never called from the native query.

### Required fact and proof inputs

- FlowNarrowingAt and ProgramAnalysisGraph
- relation/comparability proofs
- assignment/capture effects
- flow graph edges

### Required oracle obligations

- condition always true/false
- unreachable narrowed branch
- invalid discriminant comparison
- stale narrowing after assignment/call

## Expected production changes

- Add the exact `DiagnosticRuleSpec` rows and rule evaluators for this slice.
- Add any missing typed fact view or proof reference at the lowest existing semantic owner; do not create a checker-private fact store or resolver.
- Add hermetic source fixtures, canonical expected diagnostics, and pinned tsgo oracle rows.
- Add reviewed correction-overlay entries only for clear conceded TypeScript bugs; the runtime resolver remains single-spec.
- Add incremental invalidation/read-set wiring, complete-only cache admission, cancellation/budget handling, and PER0 work counters.
- Add promotion evidence mapping this generated node to exact `DiagnosticAuthorityKey` rows in NCK6.

## Internal subblocks

### NCF-FD-NARROW-SB1 - Oracle and RED characterization

- Partition the exact semantic cases, negative controls, recovery/profile dimensions, and expected authored anchors.
- Produce failing native tests before implementation and a pinned external oracle snapshot.
- Prove the harness detects wrong code, missing/extra diagnostic, wrong primary/related anchor, wrong completeness, and wrong fix intent.

### NCF-FD-NARROW-SB2 - Fact-demand and proof contract

- Enumerate every fact/proof/query read and exact environment dimension.
- Add a typed read-only fact view if necessary at the existing owner.
- Abort/rescope if the proposed evaluator would walk source text/types or duplicate semantic resolution.

### NCF-FD-NARROW-SB3 - Bounded rule implementation

- Implement one or more small `DiagnosticRule` evaluators sharing NCK3 infrastructure.
- Use semantic subjects and proof references; no message-text/source-regex/generated-TSX semantic decisions.
- Emit stable diagnostic/family/rule/subject/anchor identity and typed fix intents only where safely specified.

### NCF-FD-NARROW-SB4 - Incremental, cancellation, and admission closure

- Key by exact semantic subject/profile/env/read-set identity.
- Admit only complete results; cancelled, stale, budget-exceeded, NeedInputs, unsupported, or partial results are ReturnOnly.
- Prove targeted invalidation and incremental-equals-fresh across relevant edits/config/lib/project changes.

### NCF-FD-NARROW-SB5 - Parity and correction-overlay certification

- Compare canonical native diagnostics against the pinned oracle across all manifest rows/property fixtures.
- Classify every mismatch as implementation defect, unsupported/rescope, oracle defect, or reviewed correction overlay.
- Bind implementation, oracle, overlay, fixture, toolchain, and evidence digests in the certification receipt.

### NCF-FD-NARROW-SB6 - Family-scoped promotion and zero-provider proof

- Promote only exact profile/family/slice rows through NCK6.
- Prove ObserveNative was non-publishing, CertifiedNative atomically suppresses external diagnostics, and rollback is receipt-backed.
- Prove warm certified requests perform zero external diagnostic work for this slice.

## Acceptance IDs

- **NCF-FD-NARROW-AC-ORACLE:** exact row coverage and planted wrong-answer sensitivity.
- **NCF-FD-NARROW-AC-ONE-ENGINE:** no checker-private resolver/walker/source-text/generated-text semantic path.
- **NCF-FD-NARROW-AC-FACTS:** every result records exact shared fact/proof read set and environment identity.
- **NCF-FD-NARROW-AC-INCREMENTAL:** incremental equals fresh; targeted edits invalidate exactly.
- **NCF-FD-NARROW-AC-ADMISSION:** cancelled/stale/partial/budget/NeedInputs outcomes never warm-admit as complete.
- **NCF-FD-NARROW-AC-PARITY:** all required rows match oracle or carry accepted correction-overlay evidence.
- **NCF-FD-NARROW-AC-PROMOTION:** exact slice promotion has no duplicate/gap and certified warm runtime performs zero provider diagnostic work.
- **NCF-FD-NARROW-AC-WORK:** equivalent-work, allocation, latency, and retained-memory thresholds pass.

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

1. Run generated hermetic rows and negative controls for `NCF-FD-NARROW`.
2. Run pinned oracle comparison and correction-overlay validation.
3. Run incremental/fresh, cancellation/budget, cache-admission, authority promotion/rollback, provider-zero-work, allocation/latency/RSS tests.
4. Run the canonical targeted gate and independent semantic review on the landing-frozen candidate.

## Deletion and residual ownership

Delete only legacy diagnostic routes/tests/ignored rows exactly mapped to this slice after promotion. Neighboring diagnostic families and remaining external provider ownership are untouched and remain capability-visible.


---

<!-- unified-charter-v2
id=NCF-FD-DEF
name=Definite assignment and initialization coverage diagnostics
phase=expansion
train=expansion.native-checker
product=native_checker
kind=implementation
semantic_role=delivery
class=successor-generated
predecessors=NCK4,NCK6,D8
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
charter=charters/expansion-native-checker/generated-families/NCF-FD-DEF.md
max_production_loc=800
max_production_files=8
max_related_packages=2
rescope_loc=1500
rescope_files=12
rescope_unrelated_packages=3
initial_state=
-->

# NCF-FD-DEF - Definite assignment and initialization coverage diagnostics

## Independently acceptable outcome

Implement, certify, and promote exactly one native checker feature slice for **local/class property definite assignment, constructor paths, loop/try/finally assignment, captured writes, and use-before-assigned checks**. The slice consumes shared semantic facts and proofs, emits first-class authored diagnostics through NCK2/NCK3, passes the NCK4 hermetic oracle/correction-overlay contract, and promotes through NCK6 at exact family/slice/profile identity. It does not broaden adjacent semantic families.

## Architectural boundary

This node is generated from `native-checker-family-manifest.toml`. Its identity, predecessor set, scope, evidence keys, and budgets are manifest-derived. The node owns diagnostic evaluation for this slice only. Symbol/type/relation/call/flow/module/project facts remain owned by their existing semantic authorities; provider TypeScript remains oracle/fallback until promotion and is never called from the native query.

### Required fact and proof inputs

- flow fixed points/completion algebra
- assignment facts
- constructor/field initialization regions
- capture freshness/effects

### Required oracle obligations

- variable used before assigned
- strict property initialization failure
- assignment missing on one path
- finally invalidates coverage

## Expected production changes

- Add the exact `DiagnosticRuleSpec` rows and rule evaluators for this slice.
- Add any missing typed fact view or proof reference at the lowest existing semantic owner; do not create a checker-private fact store or resolver.
- Add hermetic source fixtures, canonical expected diagnostics, and pinned tsgo oracle rows.
- Add reviewed correction-overlay entries only for clear conceded TypeScript bugs; the runtime resolver remains single-spec.
- Add incremental invalidation/read-set wiring, complete-only cache admission, cancellation/budget handling, and PER0 work counters.
- Add promotion evidence mapping this generated node to exact `DiagnosticAuthorityKey` rows in NCK6.

## Internal subblocks

### NCF-FD-DEF-SB1 - Oracle and RED characterization

- Partition the exact semantic cases, negative controls, recovery/profile dimensions, and expected authored anchors.
- Produce failing native tests before implementation and a pinned external oracle snapshot.
- Prove the harness detects wrong code, missing/extra diagnostic, wrong primary/related anchor, wrong completeness, and wrong fix intent.

### NCF-FD-DEF-SB2 - Fact-demand and proof contract

- Enumerate every fact/proof/query read and exact environment dimension.
- Add a typed read-only fact view if necessary at the existing owner.
- Abort/rescope if the proposed evaluator would walk source text/types or duplicate semantic resolution.

### NCF-FD-DEF-SB3 - Bounded rule implementation

- Implement one or more small `DiagnosticRule` evaluators sharing NCK3 infrastructure.
- Use semantic subjects and proof references; no message-text/source-regex/generated-TSX semantic decisions.
- Emit stable diagnostic/family/rule/subject/anchor identity and typed fix intents only where safely specified.

### NCF-FD-DEF-SB4 - Incremental, cancellation, and admission closure

- Key by exact semantic subject/profile/env/read-set identity.
- Admit only complete results; cancelled, stale, budget-exceeded, NeedInputs, unsupported, or partial results are ReturnOnly.
- Prove targeted invalidation and incremental-equals-fresh across relevant edits/config/lib/project changes.

### NCF-FD-DEF-SB5 - Parity and correction-overlay certification

- Compare canonical native diagnostics against the pinned oracle across all manifest rows/property fixtures.
- Classify every mismatch as implementation defect, unsupported/rescope, oracle defect, or reviewed correction overlay.
- Bind implementation, oracle, overlay, fixture, toolchain, and evidence digests in the certification receipt.

### NCF-FD-DEF-SB6 - Family-scoped promotion and zero-provider proof

- Promote only exact profile/family/slice rows through NCK6.
- Prove ObserveNative was non-publishing, CertifiedNative atomically suppresses external diagnostics, and rollback is receipt-backed.
- Prove warm certified requests perform zero external diagnostic work for this slice.

## Acceptance IDs

- **NCF-FD-DEF-AC-ORACLE:** exact row coverage and planted wrong-answer sensitivity.
- **NCF-FD-DEF-AC-ONE-ENGINE:** no checker-private resolver/walker/source-text/generated-text semantic path.
- **NCF-FD-DEF-AC-FACTS:** every result records exact shared fact/proof read set and environment identity.
- **NCF-FD-DEF-AC-INCREMENTAL:** incremental equals fresh; targeted edits invalidate exactly.
- **NCF-FD-DEF-AC-ADMISSION:** cancelled/stale/partial/budget/NeedInputs outcomes never warm-admit as complete.
- **NCF-FD-DEF-AC-PARITY:** all required rows match oracle or carry accepted correction-overlay evidence.
- **NCF-FD-DEF-AC-PROMOTION:** exact slice promotion has no duplicate/gap and certified warm runtime performs zero provider diagnostic work.
- **NCF-FD-DEF-AC-WORK:** equivalent-work, allocation, latency, and retained-memory thresholds pass.

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

1. Run generated hermetic rows and negative controls for `NCF-FD-DEF`.
2. Run pinned oracle comparison and correction-overlay validation.
3. Run incremental/fresh, cancellation/budget, cache-admission, authority promotion/rollback, provider-zero-work, allocation/latency/RSS tests.
4. Run the canonical targeted gate and independent semantic review on the landing-frozen candidate.

## Deletion and residual ownership

Delete only legacy diagnostic routes/tests/ignored rows exactly mapped to this slice after promotion. Neighboring diagnostic families and remaining external provider ownership are untouched and remain capability-visible.


---

<!-- unified-charter-v2
id=NCF-FD-CFLOW
name=Reachability, return coverage, and control-flow legality diagnostics
phase=expansion
train=expansion.native-checker
product=native_checker
kind=implementation
semantic_role=delivery
class=successor-generated
predecessors=NCK4,NCK6,D8
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
charter=charters/expansion-native-checker/generated-families/NCF-FD-CFLOW.md
max_production_loc=800
max_production_files=8
max_related_packages=2
rescope_loc=1500
rescope_files=12
rescope_unrelated_packages=3
initial_state=
-->

# NCF-FD-CFLOW - Reachability, return coverage, and control-flow legality diagnostics

## Independently acceptable outcome

Implement, certify, and promote exactly one native checker feature slice for **unreachable code, missing returns, not-all-paths-return, break/continue/labels, switch exhaustiveness/fallthrough, try/finally completion, and async/generator completion**. The slice consumes shared semantic facts and proofs, emits first-class authored diagnostics through NCK2/NCK3, passes the NCK4 hermetic oracle/correction-overlay contract, and promotes through NCK6 at exact family/slice/profile identity. It does not broaden adjacent semantic families.

## Architectural boundary

This node is generated from `native-checker-family-manifest.toml`. Its identity, predecessor set, scope, evidence keys, and budgets are manifest-derived. The node owns diagnostic evaluation for this slice only. Symbol/type/relation/call/flow/module/project facts remain owned by their existing semantic authorities; provider TypeScript remains oracle/fallback until promotion and is never called from the native query.

### Required fact and proof inputs

- Function/ExecutableRegion flow graph
- FlowReturn
- completion algebra
- loop/switch/label region facts

### Required oracle obligations

- not all paths return
- unreachable statement
- illegal break/continue
- non-exhaustive discriminated switch

## Expected production changes

- Add the exact `DiagnosticRuleSpec` rows and rule evaluators for this slice.
- Add any missing typed fact view or proof reference at the lowest existing semantic owner; do not create a checker-private fact store or resolver.
- Add hermetic source fixtures, canonical expected diagnostics, and pinned tsgo oracle rows.
- Add reviewed correction-overlay entries only for clear conceded TypeScript bugs; the runtime resolver remains single-spec.
- Add incremental invalidation/read-set wiring, complete-only cache admission, cancellation/budget handling, and PER0 work counters.
- Add promotion evidence mapping this generated node to exact `DiagnosticAuthorityKey` rows in NCK6.

## Internal subblocks

### NCF-FD-CFLOW-SB1 - Oracle and RED characterization

- Partition the exact semantic cases, negative controls, recovery/profile dimensions, and expected authored anchors.
- Produce failing native tests before implementation and a pinned external oracle snapshot.
- Prove the harness detects wrong code, missing/extra diagnostic, wrong primary/related anchor, wrong completeness, and wrong fix intent.

### NCF-FD-CFLOW-SB2 - Fact-demand and proof contract

- Enumerate every fact/proof/query read and exact environment dimension.
- Add a typed read-only fact view if necessary at the existing owner.
- Abort/rescope if the proposed evaluator would walk source text/types or duplicate semantic resolution.

### NCF-FD-CFLOW-SB3 - Bounded rule implementation

- Implement one or more small `DiagnosticRule` evaluators sharing NCK3 infrastructure.
- Use semantic subjects and proof references; no message-text/source-regex/generated-TSX semantic decisions.
- Emit stable diagnostic/family/rule/subject/anchor identity and typed fix intents only where safely specified.

### NCF-FD-CFLOW-SB4 - Incremental, cancellation, and admission closure

- Key by exact semantic subject/profile/env/read-set identity.
- Admit only complete results; cancelled, stale, budget-exceeded, NeedInputs, unsupported, or partial results are ReturnOnly.
- Prove targeted invalidation and incremental-equals-fresh across relevant edits/config/lib/project changes.

### NCF-FD-CFLOW-SB5 - Parity and correction-overlay certification

- Compare canonical native diagnostics against the pinned oracle across all manifest rows/property fixtures.
- Classify every mismatch as implementation defect, unsupported/rescope, oracle defect, or reviewed correction overlay.
- Bind implementation, oracle, overlay, fixture, toolchain, and evidence digests in the certification receipt.

### NCF-FD-CFLOW-SB6 - Family-scoped promotion and zero-provider proof

- Promote only exact profile/family/slice rows through NCK6.
- Prove ObserveNative was non-publishing, CertifiedNative atomically suppresses external diagnostics, and rollback is receipt-backed.
- Prove warm certified requests perform zero external diagnostic work for this slice.

## Acceptance IDs

- **NCF-FD-CFLOW-AC-ORACLE:** exact row coverage and planted wrong-answer sensitivity.
- **NCF-FD-CFLOW-AC-ONE-ENGINE:** no checker-private resolver/walker/source-text/generated-text semantic path.
- **NCF-FD-CFLOW-AC-FACTS:** every result records exact shared fact/proof read set and environment identity.
- **NCF-FD-CFLOW-AC-INCREMENTAL:** incremental equals fresh; targeted edits invalidate exactly.
- **NCF-FD-CFLOW-AC-ADMISSION:** cancelled/stale/partial/budget/NeedInputs outcomes never warm-admit as complete.
- **NCF-FD-CFLOW-AC-PARITY:** all required rows match oracle or carry accepted correction-overlay evidence.
- **NCF-FD-CFLOW-AC-PROMOTION:** exact slice promotion has no duplicate/gap and certified warm runtime performs zero provider diagnostic work.
- **NCF-FD-CFLOW-AC-WORK:** equivalent-work, allocation, latency, and retained-memory thresholds pass.

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

1. Run generated hermetic rows and negative controls for `NCF-FD-CFLOW`.
2. Run pinned oracle comparison and correction-overlay validation.
3. Run incremental/fresh, cancellation/budget, cache-admission, authority promotion/rollback, provider-zero-work, allocation/latency/RSS tests.
4. Run the canonical targeted gate and independent semantic review on the landing-frozen candidate.

## Deletion and residual ownership

Delete only legacy diagnostic routes/tests/ignored rows exactly mapped to this slice after promotion. Neighboring diagnostic families and remaining external provider ownership are untouched and remain capability-visible.


---

<!-- unified-charter-v2
id=NCF-OC-MEM
name=Object, class, interface, and member declaration diagnostics
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
charter=charters/expansion-native-checker/generated-families/NCF-OC-MEM.md
max_production_loc=800
max_production_files=8
max_related_packages=2
rescope_loc=1500
rescope_files=12
rescope_unrelated_packages=3
initial_state=
-->

# NCF-OC-MEM - Object, class, interface, and member declaration diagnostics

## Independently acceptable outcome

Implement, certify, and promote exactly one native checker feature slice for **member duplicates, accessor/property/method compatibility, optional/readonly/static/private rules, index signatures, computed names, and constructor/member declarations**. The slice consumes shared semantic facts and proofs, emits first-class authored diagnostics through NCK2/NCK3, passes the NCK4 hermetic oracle/correction-overlay contract, and promotes through NCK6 at exact family/slice/profile identity. It does not broaden adjacent semantic families.

## Architectural boundary

This node is generated from `native-checker-family-manifest.toml`. Its identity, predecessor set, scope, evidence keys, and budgets are manifest-derived. The node owns diagnostic evaluation for this slice only. Symbol/type/relation/call/flow/module/project facts remain owned by their existing semantic authorities; provider TypeScript remains oracle/fallback until promotion and is never called from the native query.

### Required fact and proof inputs

- class/object/member surfaces
- symbol/declaration merge facts
- Relate proofs
- private environment and computed-name facts

### Required oracle obligations

- duplicate member
- getter/setter type mismatch
- invalid index signature/member
- private/static modifier misuse

## Expected production changes

- Add the exact `DiagnosticRuleSpec` rows and rule evaluators for this slice.
- Add any missing typed fact view or proof reference at the lowest existing semantic owner; do not create a checker-private fact store or resolver.
- Add hermetic source fixtures, canonical expected diagnostics, and pinned tsgo oracle rows.
- Add reviewed correction-overlay entries only for clear conceded TypeScript bugs; the runtime resolver remains single-spec.
- Add incremental invalidation/read-set wiring, complete-only cache admission, cancellation/budget handling, and PER0 work counters.
- Add promotion evidence mapping this generated node to exact `DiagnosticAuthorityKey` rows in NCK6.

## Internal subblocks

### NCF-OC-MEM-SB1 - Oracle and RED characterization

- Partition the exact semantic cases, negative controls, recovery/profile dimensions, and expected authored anchors.
- Produce failing native tests before implementation and a pinned external oracle snapshot.
- Prove the harness detects wrong code, missing/extra diagnostic, wrong primary/related anchor, wrong completeness, and wrong fix intent.

### NCF-OC-MEM-SB2 - Fact-demand and proof contract

- Enumerate every fact/proof/query read and exact environment dimension.
- Add a typed read-only fact view if necessary at the existing owner.
- Abort/rescope if the proposed evaluator would walk source text/types or duplicate semantic resolution.

### NCF-OC-MEM-SB3 - Bounded rule implementation

- Implement one or more small `DiagnosticRule` evaluators sharing NCK3 infrastructure.
- Use semantic subjects and proof references; no message-text/source-regex/generated-TSX semantic decisions.
- Emit stable diagnostic/family/rule/subject/anchor identity and typed fix intents only where safely specified.

### NCF-OC-MEM-SB4 - Incremental, cancellation, and admission closure

- Key by exact semantic subject/profile/env/read-set identity.
- Admit only complete results; cancelled, stale, budget-exceeded, NeedInputs, unsupported, or partial results are ReturnOnly.
- Prove targeted invalidation and incremental-equals-fresh across relevant edits/config/lib/project changes.

### NCF-OC-MEM-SB5 - Parity and correction-overlay certification

- Compare canonical native diagnostics against the pinned oracle across all manifest rows/property fixtures.
- Classify every mismatch as implementation defect, unsupported/rescope, oracle defect, or reviewed correction overlay.
- Bind implementation, oracle, overlay, fixture, toolchain, and evidence digests in the certification receipt.

### NCF-OC-MEM-SB6 - Family-scoped promotion and zero-provider proof

- Promote only exact profile/family/slice rows through NCK6.
- Prove ObserveNative was non-publishing, CertifiedNative atomically suppresses external diagnostics, and rollback is receipt-backed.
- Prove warm certified requests perform zero external diagnostic work for this slice.

## Acceptance IDs

- **NCF-OC-MEM-AC-ORACLE:** exact row coverage and planted wrong-answer sensitivity.
- **NCF-OC-MEM-AC-ONE-ENGINE:** no checker-private resolver/walker/source-text/generated-text semantic path.
- **NCF-OC-MEM-AC-FACTS:** every result records exact shared fact/proof read set and environment identity.
- **NCF-OC-MEM-AC-INCREMENTAL:** incremental equals fresh; targeted edits invalidate exactly.
- **NCF-OC-MEM-AC-ADMISSION:** cancelled/stale/partial/budget/NeedInputs outcomes never warm-admit as complete.
- **NCF-OC-MEM-AC-PARITY:** all required rows match oracle or carry accepted correction-overlay evidence.
- **NCF-OC-MEM-AC-PROMOTION:** exact slice promotion has no duplicate/gap and certified warm runtime performs zero provider diagnostic work.
- **NCF-OC-MEM-AC-WORK:** equivalent-work, allocation, latency, and retained-memory thresholds pass.

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

1. Run generated hermetic rows and negative controls for `NCF-OC-MEM`.
2. Run pinned oracle comparison and correction-overlay validation.
3. Run incremental/fresh, cancellation/budget, cache-admission, authority promotion/rollback, provider-zero-work, allocation/latency/RSS tests.
4. Run the canonical targeted gate and independent semantic review on the landing-frozen candidate.

## Deletion and residual ownership

Delete only legacy diagnostic routes/tests/ignored rows exactly mapped to this slice after promotion. Neighboring diagnostic families and remaining external provider ownership are untouched and remain capability-visible.


---

<!-- unified-charter-v2
id=NCF-OC-HERIT
name=Heritage, override, abstract, and implementation diagnostics
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
charter=charters/expansion-native-checker/generated-families/NCF-OC-HERIT.md
max_production_loc=800
max_production_files=8
max_related_packages=2
rescope_loc=1500
rescope_files=12
rescope_unrelated_packages=3
initial_state=
-->

# NCF-OC-HERIT - Heritage, override, abstract, and implementation diagnostics

## Independently acceptable outcome

Implement, certify, and promote exactly one native checker feature slice for **extends/implements constraints, override compatibility, abstract members/classes, constructor/base compatibility, cyclic heritage, mixins, and protected/private nominal restrictions**. The slice consumes shared semantic facts and proofs, emits first-class authored diagnostics through NCK2/NCK3, passes the NCK4 hermetic oracle/correction-overlay contract, and promotes through NCK6 at exact family/slice/profile identity. It does not broaden adjacent semantic families.

## Architectural boundary

This node is generated from `native-checker-family-manifest.toml`. Its identity, predecessor set, scope, evidence keys, and budgets are manifest-derived. The node owns diagnostic evaluation for this slice only. Symbol/type/relation/call/flow/module/project facts remain owned by their existing semantic authorities; provider TypeScript remains oracle/fallback until promotion and is never called from the native query.

### Required fact and proof inputs

- heritage/override target edges
- class/interface surfaces
- Relate/variance proofs
- cycle facts

### Required oracle obligations

- incorrectly implements interface
- override incompatible/missing override
- non-abstract class missing member
- cyclic base type

## Expected production changes

- Add the exact `DiagnosticRuleSpec` rows and rule evaluators for this slice.
- Add any missing typed fact view or proof reference at the lowest existing semantic owner; do not create a checker-private fact store or resolver.
- Add hermetic source fixtures, canonical expected diagnostics, and pinned tsgo oracle rows.
- Add reviewed correction-overlay entries only for clear conceded TypeScript bugs; the runtime resolver remains single-spec.
- Add incremental invalidation/read-set wiring, complete-only cache admission, cancellation/budget handling, and PER0 work counters.
- Add promotion evidence mapping this generated node to exact `DiagnosticAuthorityKey` rows in NCK6.

## Internal subblocks

### NCF-OC-HERIT-SB1 - Oracle and RED characterization

- Partition the exact semantic cases, negative controls, recovery/profile dimensions, and expected authored anchors.
- Produce failing native tests before implementation and a pinned external oracle snapshot.
- Prove the harness detects wrong code, missing/extra diagnostic, wrong primary/related anchor, wrong completeness, and wrong fix intent.

### NCF-OC-HERIT-SB2 - Fact-demand and proof contract

- Enumerate every fact/proof/query read and exact environment dimension.
- Add a typed read-only fact view if necessary at the existing owner.
- Abort/rescope if the proposed evaluator would walk source text/types or duplicate semantic resolution.

### NCF-OC-HERIT-SB3 - Bounded rule implementation

- Implement one or more small `DiagnosticRule` evaluators sharing NCK3 infrastructure.
- Use semantic subjects and proof references; no message-text/source-regex/generated-TSX semantic decisions.
- Emit stable diagnostic/family/rule/subject/anchor identity and typed fix intents only where safely specified.

### NCF-OC-HERIT-SB4 - Incremental, cancellation, and admission closure

- Key by exact semantic subject/profile/env/read-set identity.
- Admit only complete results; cancelled, stale, budget-exceeded, NeedInputs, unsupported, or partial results are ReturnOnly.
- Prove targeted invalidation and incremental-equals-fresh across relevant edits/config/lib/project changes.

### NCF-OC-HERIT-SB5 - Parity and correction-overlay certification

- Compare canonical native diagnostics against the pinned oracle across all manifest rows/property fixtures.
- Classify every mismatch as implementation defect, unsupported/rescope, oracle defect, or reviewed correction overlay.
- Bind implementation, oracle, overlay, fixture, toolchain, and evidence digests in the certification receipt.

### NCF-OC-HERIT-SB6 - Family-scoped promotion and zero-provider proof

- Promote only exact profile/family/slice rows through NCK6.
- Prove ObserveNative was non-publishing, CertifiedNative atomically suppresses external diagnostics, and rollback is receipt-backed.
- Prove warm certified requests perform zero external diagnostic work for this slice.

## Acceptance IDs

- **NCF-OC-HERIT-AC-ORACLE:** exact row coverage and planted wrong-answer sensitivity.
- **NCF-OC-HERIT-AC-ONE-ENGINE:** no checker-private resolver/walker/source-text/generated-text semantic path.
- **NCF-OC-HERIT-AC-FACTS:** every result records exact shared fact/proof read set and environment identity.
- **NCF-OC-HERIT-AC-INCREMENTAL:** incremental equals fresh; targeted edits invalidate exactly.
- **NCF-OC-HERIT-AC-ADMISSION:** cancelled/stale/partial/budget/NeedInputs outcomes never warm-admit as complete.
- **NCF-OC-HERIT-AC-PARITY:** all required rows match oracle or carry accepted correction-overlay evidence.
- **NCF-OC-HERIT-AC-PROMOTION:** exact slice promotion has no duplicate/gap and certified warm runtime performs zero provider diagnostic work.
- **NCF-OC-HERIT-AC-WORK:** equivalent-work, allocation, latency, and retained-memory thresholds pass.

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

1. Run generated hermetic rows and negative controls for `NCF-OC-HERIT`.
2. Run pinned oracle comparison and correction-overlay validation.
3. Run incremental/fresh, cancellation/budget, cache-admission, authority promotion/rollback, provider-zero-work, allocation/latency/RSS tests.
4. Run the canonical targeted gate and independent semantic review on the landing-frozen candidate.

## Deletion and residual ownership

Delete only legacy diagnostic routes/tests/ignored rows exactly mapped to this slice after promotion. Neighboring diagnostic families and remaining external provider ownership are untouched and remain capability-visible.


---

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


---

<!-- unified-charter-v2
id=NCF-MP-MODULE
name=Module resolution, import/export, and package-boundary diagnostics
phase=expansion
train=expansion.native-checker
product=native_checker
kind=implementation
semantic_role=delivery
class=successor-generated
predecessors=NCK4,NCK6,IDX0,TCM4
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
charter=charters/expansion-native-checker/generated-families/NCF-MP-MODULE.md
max_production_loc=800
max_production_files=8
max_related_packages=2
rescope_loc=1500
rescope_files=12
rescope_unrelated_packages=3
initial_state=
-->

# NCF-MP-MODULE - Module resolution, import/export, and package-boundary diagnostics

## Independently acceptable outcome

Implement, certify, and promote exactly one native checker feature slice for **module specifier resolution, import/export forms, missing exports, type-only/value usage, CommonJS/ESM interop, package exports/imports, and resolution-mode compatibility**. The slice consumes shared semantic facts and proofs, emits first-class authored diagnostics through NCK2/NCK3, passes the NCK4 hermetic oracle/correction-overlay contract, and promotes through NCK6 at exact family/slice/profile identity. It does not broaden adjacent semantic families.

## Architectural boundary

This node is generated from `native-checker-family-manifest.toml`. Its identity, predecessor set, scope, evidence keys, and budgets are manifest-derived. The node owns diagnostic evaluation for this slice only. Symbol/type/relation/call/flow/module/project facts remain owned by their existing semantic authorities; provider TypeScript remains oracle/fallback until promotion and is never called from the native query.

### Required fact and proof inputs

- ModuleResolverCore results and proofs
- canonical paths/source lineage
- export graph/index
- project/compiler option environment

### Required oracle obligations

- cannot find module
- module has no exported member
- type-only import used as value
- ESM/CommonJS mode violation

## Expected production changes

- Add the exact `DiagnosticRuleSpec` rows and rule evaluators for this slice.
- Add any missing typed fact view or proof reference at the lowest existing semantic owner; do not create a checker-private fact store or resolver.
- Add hermetic source fixtures, canonical expected diagnostics, and pinned tsgo oracle rows.
- Add reviewed correction-overlay entries only for clear conceded TypeScript bugs; the runtime resolver remains single-spec.
- Add incremental invalidation/read-set wiring, complete-only cache admission, cancellation/budget handling, and PER0 work counters.
- Add promotion evidence mapping this generated node to exact `DiagnosticAuthorityKey` rows in NCK6.

## Internal subblocks

### NCF-MP-MODULE-SB1 - Oracle and RED characterization

- Partition the exact semantic cases, negative controls, recovery/profile dimensions, and expected authored anchors.
- Produce failing native tests before implementation and a pinned external oracle snapshot.
- Prove the harness detects wrong code, missing/extra diagnostic, wrong primary/related anchor, wrong completeness, and wrong fix intent.

### NCF-MP-MODULE-SB2 - Fact-demand and proof contract

- Enumerate every fact/proof/query read and exact environment dimension.
- Add a typed read-only fact view if necessary at the existing owner.
- Abort/rescope if the proposed evaluator would walk source text/types or duplicate semantic resolution.

### NCF-MP-MODULE-SB3 - Bounded rule implementation

- Implement one or more small `DiagnosticRule` evaluators sharing NCK3 infrastructure.
- Use semantic subjects and proof references; no message-text/source-regex/generated-TSX semantic decisions.
- Emit stable diagnostic/family/rule/subject/anchor identity and typed fix intents only where safely specified.

### NCF-MP-MODULE-SB4 - Incremental, cancellation, and admission closure

- Key by exact semantic subject/profile/env/read-set identity.
- Admit only complete results; cancelled, stale, budget-exceeded, NeedInputs, unsupported, or partial results are ReturnOnly.
- Prove targeted invalidation and incremental-equals-fresh across relevant edits/config/lib/project changes.

### NCF-MP-MODULE-SB5 - Parity and correction-overlay certification

- Compare canonical native diagnostics against the pinned oracle across all manifest rows/property fixtures.
- Classify every mismatch as implementation defect, unsupported/rescope, oracle defect, or reviewed correction overlay.
- Bind implementation, oracle, overlay, fixture, toolchain, and evidence digests in the certification receipt.

### NCF-MP-MODULE-SB6 - Family-scoped promotion and zero-provider proof

- Promote only exact profile/family/slice rows through NCK6.
- Prove ObserveNative was non-publishing, CertifiedNative atomically suppresses external diagnostics, and rollback is receipt-backed.
- Prove warm certified requests perform zero external diagnostic work for this slice.

## Acceptance IDs

- **NCF-MP-MODULE-AC-ORACLE:** exact row coverage and planted wrong-answer sensitivity.
- **NCF-MP-MODULE-AC-ONE-ENGINE:** no checker-private resolver/walker/source-text/generated-text semantic path.
- **NCF-MP-MODULE-AC-FACTS:** every result records exact shared fact/proof read set and environment identity.
- **NCF-MP-MODULE-AC-INCREMENTAL:** incremental equals fresh; targeted edits invalidate exactly.
- **NCF-MP-MODULE-AC-ADMISSION:** cancelled/stale/partial/budget/NeedInputs outcomes never warm-admit as complete.
- **NCF-MP-MODULE-AC-PARITY:** all required rows match oracle or carry accepted correction-overlay evidence.
- **NCF-MP-MODULE-AC-PROMOTION:** exact slice promotion has no duplicate/gap and certified warm runtime performs zero provider diagnostic work.
- **NCF-MP-MODULE-AC-WORK:** equivalent-work, allocation, latency, and retained-memory thresholds pass.

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

1. Run generated hermetic rows and negative controls for `NCF-MP-MODULE`.
2. Run pinned oracle comparison and correction-overlay validation.
3. Run incremental/fresh, cancellation/budget, cache-admission, authority promotion/rollback, provider-zero-work, allocation/latency/RSS tests.
4. Run the canonical targeted gate and independent semantic review on the landing-frozen candidate.

## Deletion and residual ownership

Delete only legacy diagnostic routes/tests/ignored rows exactly mapped to this slice after promotion. Neighboring diagnostic families and remaining external provider ownership are untouched and remain capability-visible.


---

<!-- unified-charter-v2
id=NCF-MP-AUG
name=Module/global augmentation and cross-file declaration diagnostics
phase=expansion
train=expansion.native-checker
product=native_checker
kind=implementation
semantic_role=delivery
class=successor-generated
predecessors=NCK4,NCK6,IDX0,TCM4,D8
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
charter=charters/expansion-native-checker/generated-families/NCF-MP-AUG.md
max_production_loc=800
max_production_files=8
max_related_packages=2
rescope_loc=1500
rescope_files=12
rescope_unrelated_packages=3
initial_state=
-->

# NCF-MP-AUG - Module/global augmentation and cross-file declaration diagnostics

## Independently acceptable outcome

Implement, certify, and promote exactly one native checker feature slice for **augmentation target existence, external-module context, duplicate/incompatible augmented members, global augmentation placement, and cross-file merge visibility**. The slice consumes shared semantic facts and proofs, emits first-class authored diagnostics through NCK2/NCK3, passes the NCK4 hermetic oracle/correction-overlay contract, and promotes through NCK6 at exact family/slice/profile identity. It does not broaden adjacent semantic families.

## Architectural boundary

This node is generated from `native-checker-family-manifest.toml`. Its identity, predecessor set, scope, evidence keys, and budgets are manifest-derived. The node owns diagnostic evaluation for this slice only. Symbol/type/relation/call/flow/module/project facts remain owned by their existing semantic authorities; provider TypeScript remains oracle/fallback until promotion and is never called from the native query.

### Required fact and proof inputs

- module/global augmentation facts
- merged declaration authority
- project membership/index
- Relate proofs

### Required oracle obligations

- invalid module augmentation name
- global augmentation outside module
- incompatible augmented property
- augmentation not visible in project

## Expected production changes

- Add the exact `DiagnosticRuleSpec` rows and rule evaluators for this slice.
- Add any missing typed fact view or proof reference at the lowest existing semantic owner; do not create a checker-private fact store or resolver.
- Add hermetic source fixtures, canonical expected diagnostics, and pinned tsgo oracle rows.
- Add reviewed correction-overlay entries only for clear conceded TypeScript bugs; the runtime resolver remains single-spec.
- Add incremental invalidation/read-set wiring, complete-only cache admission, cancellation/budget handling, and PER0 work counters.
- Add promotion evidence mapping this generated node to exact `DiagnosticAuthorityKey` rows in NCK6.

## Internal subblocks

### NCF-MP-AUG-SB1 - Oracle and RED characterization

- Partition the exact semantic cases, negative controls, recovery/profile dimensions, and expected authored anchors.
- Produce failing native tests before implementation and a pinned external oracle snapshot.
- Prove the harness detects wrong code, missing/extra diagnostic, wrong primary/related anchor, wrong completeness, and wrong fix intent.

### NCF-MP-AUG-SB2 - Fact-demand and proof contract

- Enumerate every fact/proof/query read and exact environment dimension.
- Add a typed read-only fact view if necessary at the existing owner.
- Abort/rescope if the proposed evaluator would walk source text/types or duplicate semantic resolution.

### NCF-MP-AUG-SB3 - Bounded rule implementation

- Implement one or more small `DiagnosticRule` evaluators sharing NCK3 infrastructure.
- Use semantic subjects and proof references; no message-text/source-regex/generated-TSX semantic decisions.
- Emit stable diagnostic/family/rule/subject/anchor identity and typed fix intents only where safely specified.

### NCF-MP-AUG-SB4 - Incremental, cancellation, and admission closure

- Key by exact semantic subject/profile/env/read-set identity.
- Admit only complete results; cancelled, stale, budget-exceeded, NeedInputs, unsupported, or partial results are ReturnOnly.
- Prove targeted invalidation and incremental-equals-fresh across relevant edits/config/lib/project changes.

### NCF-MP-AUG-SB5 - Parity and correction-overlay certification

- Compare canonical native diagnostics against the pinned oracle across all manifest rows/property fixtures.
- Classify every mismatch as implementation defect, unsupported/rescope, oracle defect, or reviewed correction overlay.
- Bind implementation, oracle, overlay, fixture, toolchain, and evidence digests in the certification receipt.

### NCF-MP-AUG-SB6 - Family-scoped promotion and zero-provider proof

- Promote only exact profile/family/slice rows through NCK6.
- Prove ObserveNative was non-publishing, CertifiedNative atomically suppresses external diagnostics, and rollback is receipt-backed.
- Prove warm certified requests perform zero external diagnostic work for this slice.

## Acceptance IDs

- **NCF-MP-AUG-AC-ORACLE:** exact row coverage and planted wrong-answer sensitivity.
- **NCF-MP-AUG-AC-ONE-ENGINE:** no checker-private resolver/walker/source-text/generated-text semantic path.
- **NCF-MP-AUG-AC-FACTS:** every result records exact shared fact/proof read set and environment identity.
- **NCF-MP-AUG-AC-INCREMENTAL:** incremental equals fresh; targeted edits invalidate exactly.
- **NCF-MP-AUG-AC-ADMISSION:** cancelled/stale/partial/budget/NeedInputs outcomes never warm-admit as complete.
- **NCF-MP-AUG-AC-PARITY:** all required rows match oracle or carry accepted correction-overlay evidence.
- **NCF-MP-AUG-AC-PROMOTION:** exact slice promotion has no duplicate/gap and certified warm runtime performs zero provider diagnostic work.
- **NCF-MP-AUG-AC-WORK:** equivalent-work, allocation, latency, and retained-memory thresholds pass.

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

1. Run generated hermetic rows and negative controls for `NCF-MP-AUG`.
2. Run pinned oracle comparison and correction-overlay validation.
3. Run incremental/fresh, cancellation/budget, cache-admission, authority promotion/rollback, provider-zero-work, allocation/latency/RSS tests.
4. Run the canonical targeted gate and independent semantic review on the landing-frozen candidate.

## Deletion and residual ownership

Delete only legacy diagnostic routes/tests/ignored rows exactly mapped to this slice after promotion. Neighboring diagnostic families and remaining external provider ownership are untouched and remain capability-visible.


---

<!-- unified-charter-v2
id=NCF-MP-PROJECT
name=Project references, configuration, library, and program diagnostics
phase=expansion
train=expansion.native-checker
product=native_checker
kind=implementation
semantic_role=delivery
class=successor-generated
predecessors=NCK4,NCK6,IDX0,TCM4,PUB0
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
charter=charters/expansion-native-checker/generated-families/NCF-MP-PROJECT.md
max_production_loc=800
max_production_files=8
max_related_packages=2
rescope_loc=1500
rescope_files=12
rescope_unrelated_packages=3
initial_state=
-->

# NCF-MP-PROJECT - Project references, configuration, library, and program diagnostics

## Independently acceptable outcome

Implement, certify, and promote exactly one native checker feature slice for **project/reference graph, root/include/exclude membership, declaration/output/config compatibility, lib/type acquisition inputs, duplicate source inclusion, and project-cycle diagnostics**. The slice consumes shared semantic facts and proofs, emits first-class authored diagnostics through NCK2/NCK3, passes the NCK4 hermetic oracle/correction-overlay contract, and promotes through NCK6 at exact family/slice/profile identity. It does not broaden adjacent semantic families.

## Architectural boundary

This node is generated from `native-checker-family-manifest.toml`. Its identity, predecessor set, scope, evidence keys, and budgets are manifest-derived. The node owns diagnostic evaluation for this slice only. Symbol/type/relation/call/flow/module/project facts remain owned by their existing semantic authorities; provider TypeScript remains oracle/fallback until promotion and is never called from the native query.

### Required fact and proof inputs

- project graph/membership indexes
- captured configuration and environment
- source identity/outputs
- provider/native capability requirements

### Required oracle obligations

- project reference cycle
- file outside root/include
- incompatible composite/declaration settings
- missing lib/type inputs

## Expected production changes

- Add the exact `DiagnosticRuleSpec` rows and rule evaluators for this slice.
- Add any missing typed fact view or proof reference at the lowest existing semantic owner; do not create a checker-private fact store or resolver.
- Add hermetic source fixtures, canonical expected diagnostics, and pinned tsgo oracle rows.
- Add reviewed correction-overlay entries only for clear conceded TypeScript bugs; the runtime resolver remains single-spec.
- Add incremental invalidation/read-set wiring, complete-only cache admission, cancellation/budget handling, and PER0 work counters.
- Add promotion evidence mapping this generated node to exact `DiagnosticAuthorityKey` rows in NCK6.

## Internal subblocks

### NCF-MP-PROJECT-SB1 - Oracle and RED characterization

- Partition the exact semantic cases, negative controls, recovery/profile dimensions, and expected authored anchors.
- Produce failing native tests before implementation and a pinned external oracle snapshot.
- Prove the harness detects wrong code, missing/extra diagnostic, wrong primary/related anchor, wrong completeness, and wrong fix intent.

### NCF-MP-PROJECT-SB2 - Fact-demand and proof contract

- Enumerate every fact/proof/query read and exact environment dimension.
- Add a typed read-only fact view if necessary at the existing owner.
- Abort/rescope if the proposed evaluator would walk source text/types or duplicate semantic resolution.

### NCF-MP-PROJECT-SB3 - Bounded rule implementation

- Implement one or more small `DiagnosticRule` evaluators sharing NCK3 infrastructure.
- Use semantic subjects and proof references; no message-text/source-regex/generated-TSX semantic decisions.
- Emit stable diagnostic/family/rule/subject/anchor identity and typed fix intents only where safely specified.

### NCF-MP-PROJECT-SB4 - Incremental, cancellation, and admission closure

- Key by exact semantic subject/profile/env/read-set identity.
- Admit only complete results; cancelled, stale, budget-exceeded, NeedInputs, unsupported, or partial results are ReturnOnly.
- Prove targeted invalidation and incremental-equals-fresh across relevant edits/config/lib/project changes.

### NCF-MP-PROJECT-SB5 - Parity and correction-overlay certification

- Compare canonical native diagnostics against the pinned oracle across all manifest rows/property fixtures.
- Classify every mismatch as implementation defect, unsupported/rescope, oracle defect, or reviewed correction overlay.
- Bind implementation, oracle, overlay, fixture, toolchain, and evidence digests in the certification receipt.

### NCF-MP-PROJECT-SB6 - Family-scoped promotion and zero-provider proof

- Promote only exact profile/family/slice rows through NCK6.
- Prove ObserveNative was non-publishing, CertifiedNative atomically suppresses external diagnostics, and rollback is receipt-backed.
- Prove warm certified requests perform zero external diagnostic work for this slice.

## Acceptance IDs

- **NCF-MP-PROJECT-AC-ORACLE:** exact row coverage and planted wrong-answer sensitivity.
- **NCF-MP-PROJECT-AC-ONE-ENGINE:** no checker-private resolver/walker/source-text/generated-text semantic path.
- **NCF-MP-PROJECT-AC-FACTS:** every result records exact shared fact/proof read set and environment identity.
- **NCF-MP-PROJECT-AC-INCREMENTAL:** incremental equals fresh; targeted edits invalidate exactly.
- **NCF-MP-PROJECT-AC-ADMISSION:** cancelled/stale/partial/budget/NeedInputs outcomes never warm-admit as complete.
- **NCF-MP-PROJECT-AC-PARITY:** all required rows match oracle or carry accepted correction-overlay evidence.
- **NCF-MP-PROJECT-AC-PROMOTION:** exact slice promotion has no duplicate/gap and certified warm runtime performs zero provider diagnostic work.
- **NCF-MP-PROJECT-AC-WORK:** equivalent-work, allocation, latency, and retained-memory thresholds pass.

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

1. Run generated hermetic rows and negative controls for `NCF-MP-PROJECT`.
2. Run pinned oracle comparison and correction-overlay validation.
3. Run incremental/fresh, cancellation/budget, cache-admission, authority promotion/rollback, provider-zero-work, allocation/latency/RSS tests.
4. Run the canonical targeted gate and independent semantic review on the landing-frozen candidate.

## Deletion and residual ownership

Delete only legacy diagnostic routes/tests/ignored rows exactly mapped to this slice after promotion. Neighboring diagnostic families and remaining external provider ownership are untouched and remain capability-visible.


---

<!-- unified-charter-v2
id=NCF-AT-QUERY
name=Keyof, indexed access, type query, alias, and reference diagnostics
phase=expansion
train=expansion.native-checker
product=native_checker
kind=implementation
semantic_role=delivery
class=successor-generated
predecessors=NCK4,NCK6,D8
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
charter=charters/expansion-native-checker/generated-families/NCF-AT-QUERY.md
max_production_loc=800
max_production_files=8
max_related_packages=2
rescope_loc=1500
rescope_files=12
rescope_unrelated_packages=3
initial_state=
-->

# NCF-AT-QUERY - Keyof, indexed access, type query, alias, and reference diagnostics

## Independently acceptable outcome

Implement, certify, and promote exactly one native checker feature slice for **invalid type/value queries, key/index constraints, alias/type parameter use, qualified names, unique symbols, and type argument application at type sites**. The slice consumes shared semantic facts and proofs, emits first-class authored diagnostics through NCK2/NCK3, passes the NCK4 hermetic oracle/correction-overlay contract, and promotes through NCK6 at exact family/slice/profile identity. It does not broaden adjacent semantic families.

## Architectural boundary

This node is generated from `native-checker-family-manifest.toml`. Its identity, predecessor set, scope, evidence keys, and budgets are manifest-derived. The node owns diagnostic evaluation for this slice only. Symbol/type/relation/call/flow/module/project facts remain owned by their existing semantic authorities; provider TypeScript remains oracle/fallback until promotion and is never called from the native query.

### Required fact and proof inputs

- KeyOf/IndexedAccess/TypeOf/ProjectPath reducers
- symbol/type namespace facts
- relation constraints
- alias/reference identity

### Required oracle obligations

- value used as type/type used as value
- invalid indexed access key
- generic type requires arguments
- unique symbol misuse

## Expected production changes

- Add the exact `DiagnosticRuleSpec` rows and rule evaluators for this slice.
- Add any missing typed fact view or proof reference at the lowest existing semantic owner; do not create a checker-private fact store or resolver.
- Add hermetic source fixtures, canonical expected diagnostics, and pinned tsgo oracle rows.
- Add reviewed correction-overlay entries only for clear conceded TypeScript bugs; the runtime resolver remains single-spec.
- Add incremental invalidation/read-set wiring, complete-only cache admission, cancellation/budget handling, and PER0 work counters.
- Add promotion evidence mapping this generated node to exact `DiagnosticAuthorityKey` rows in NCK6.

## Internal subblocks

### NCF-AT-QUERY-SB1 - Oracle and RED characterization

- Partition the exact semantic cases, negative controls, recovery/profile dimensions, and expected authored anchors.
- Produce failing native tests before implementation and a pinned external oracle snapshot.
- Prove the harness detects wrong code, missing/extra diagnostic, wrong primary/related anchor, wrong completeness, and wrong fix intent.

### NCF-AT-QUERY-SB2 - Fact-demand and proof contract

- Enumerate every fact/proof/query read and exact environment dimension.
- Add a typed read-only fact view if necessary at the existing owner.
- Abort/rescope if the proposed evaluator would walk source text/types or duplicate semantic resolution.

### NCF-AT-QUERY-SB3 - Bounded rule implementation

- Implement one or more small `DiagnosticRule` evaluators sharing NCK3 infrastructure.
- Use semantic subjects and proof references; no message-text/source-regex/generated-TSX semantic decisions.
- Emit stable diagnostic/family/rule/subject/anchor identity and typed fix intents only where safely specified.

### NCF-AT-QUERY-SB4 - Incremental, cancellation, and admission closure

- Key by exact semantic subject/profile/env/read-set identity.
- Admit only complete results; cancelled, stale, budget-exceeded, NeedInputs, unsupported, or partial results are ReturnOnly.
- Prove targeted invalidation and incremental-equals-fresh across relevant edits/config/lib/project changes.

### NCF-AT-QUERY-SB5 - Parity and correction-overlay certification

- Compare canonical native diagnostics against the pinned oracle across all manifest rows/property fixtures.
- Classify every mismatch as implementation defect, unsupported/rescope, oracle defect, or reviewed correction overlay.
- Bind implementation, oracle, overlay, fixture, toolchain, and evidence digests in the certification receipt.

### NCF-AT-QUERY-SB6 - Family-scoped promotion and zero-provider proof

- Promote only exact profile/family/slice rows through NCK6.
- Prove ObserveNative was non-publishing, CertifiedNative atomically suppresses external diagnostics, and rollback is receipt-backed.
- Prove warm certified requests perform zero external diagnostic work for this slice.

## Acceptance IDs

- **NCF-AT-QUERY-AC-ORACLE:** exact row coverage and planted wrong-answer sensitivity.
- **NCF-AT-QUERY-AC-ONE-ENGINE:** no checker-private resolver/walker/source-text/generated-text semantic path.
- **NCF-AT-QUERY-AC-FACTS:** every result records exact shared fact/proof read set and environment identity.
- **NCF-AT-QUERY-AC-INCREMENTAL:** incremental equals fresh; targeted edits invalidate exactly.
- **NCF-AT-QUERY-AC-ADMISSION:** cancelled/stale/partial/budget/NeedInputs outcomes never warm-admit as complete.
- **NCF-AT-QUERY-AC-PARITY:** all required rows match oracle or carry accepted correction-overlay evidence.
- **NCF-AT-QUERY-AC-PROMOTION:** exact slice promotion has no duplicate/gap and certified warm runtime performs zero provider diagnostic work.
- **NCF-AT-QUERY-AC-WORK:** equivalent-work, allocation, latency, and retained-memory thresholds pass.

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

1. Run generated hermetic rows and negative controls for `NCF-AT-QUERY`.
2. Run pinned oracle comparison and correction-overlay validation.
3. Run incremental/fresh, cancellation/budget, cache-admission, authority promotion/rollback, provider-zero-work, allocation/latency/RSS tests.
4. Run the canonical targeted gate and independent semantic review on the landing-frozen candidate.

## Deletion and residual ownership

Delete only legacy diagnostic routes/tests/ignored rows exactly mapped to this slice after promotion. Neighboring diagnostic families and remaining external provider ownership are untouched and remain capability-visible.


---

<!-- unified-charter-v2
id=NCF-AT-REDUCE
name=Mapped, conditional, infer, template-literal, and utility-type diagnostics
phase=expansion
train=expansion.native-checker
product=native_checker
kind=implementation
semantic_role=delivery
class=successor-generated
predecessors=NCK4,NCK6,D8
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
charter=charters/expansion-native-checker/generated-families/NCF-AT-REDUCE.md
max_production_loc=800
max_production_files=8
max_related_packages=2
rescope_loc=1500
rescope_files=12
rescope_unrelated_packages=3
initial_state=
-->

# NCF-AT-REDUCE - Mapped, conditional, infer, template-literal, and utility-type diagnostics

## Independently acceptable outcome

Implement, certify, and promote exactly one native checker feature slice for **mapped/conditional/template reduction legality, infer placement, modifier/name remapping, distributivity, utility constraints, and reduction budget/degradation**. The slice consumes shared semantic facts and proofs, emits first-class authored diagnostics through NCK2/NCK3, passes the NCK4 hermetic oracle/correction-overlay contract, and promotes through NCK6 at exact family/slice/profile identity. It does not broaden adjacent semantic families.

## Architectural boundary

This node is generated from `native-checker-family-manifest.toml`. Its identity, predecessor set, scope, evidence keys, and budgets are manifest-derived. The node owns diagnostic evaluation for this slice only. Symbol/type/relation/call/flow/module/project facts remain owned by their existing semantic authorities; provider TypeScript remains oracle/fallback until promotion and is never called from the native query.

### Required fact and proof inputs

- mapped/conditional/template reducers
- inference/relation proofs
- type parameter and key domains
- cycle/budget evidence

### Required oracle obligations

- infer outside conditional
- invalid mapped key remap
- utility constraint failure
- excessive reduction depth

## Expected production changes

- Add the exact `DiagnosticRuleSpec` rows and rule evaluators for this slice.
- Add any missing typed fact view or proof reference at the lowest existing semantic owner; do not create a checker-private fact store or resolver.
- Add hermetic source fixtures, canonical expected diagnostics, and pinned tsgo oracle rows.
- Add reviewed correction-overlay entries only for clear conceded TypeScript bugs; the runtime resolver remains single-spec.
- Add incremental invalidation/read-set wiring, complete-only cache admission, cancellation/budget handling, and PER0 work counters.
- Add promotion evidence mapping this generated node to exact `DiagnosticAuthorityKey` rows in NCK6.

## Internal subblocks

### NCF-AT-REDUCE-SB1 - Oracle and RED characterization

- Partition the exact semantic cases, negative controls, recovery/profile dimensions, and expected authored anchors.
- Produce failing native tests before implementation and a pinned external oracle snapshot.
- Prove the harness detects wrong code, missing/extra diagnostic, wrong primary/related anchor, wrong completeness, and wrong fix intent.

### NCF-AT-REDUCE-SB2 - Fact-demand and proof contract

- Enumerate every fact/proof/query read and exact environment dimension.
- Add a typed read-only fact view if necessary at the existing owner.
- Abort/rescope if the proposed evaluator would walk source text/types or duplicate semantic resolution.

### NCF-AT-REDUCE-SB3 - Bounded rule implementation

- Implement one or more small `DiagnosticRule` evaluators sharing NCK3 infrastructure.
- Use semantic subjects and proof references; no message-text/source-regex/generated-TSX semantic decisions.
- Emit stable diagnostic/family/rule/subject/anchor identity and typed fix intents only where safely specified.

### NCF-AT-REDUCE-SB4 - Incremental, cancellation, and admission closure

- Key by exact semantic subject/profile/env/read-set identity.
- Admit only complete results; cancelled, stale, budget-exceeded, NeedInputs, unsupported, or partial results are ReturnOnly.
- Prove targeted invalidation and incremental-equals-fresh across relevant edits/config/lib/project changes.

### NCF-AT-REDUCE-SB5 - Parity and correction-overlay certification

- Compare canonical native diagnostics against the pinned oracle across all manifest rows/property fixtures.
- Classify every mismatch as implementation defect, unsupported/rescope, oracle defect, or reviewed correction overlay.
- Bind implementation, oracle, overlay, fixture, toolchain, and evidence digests in the certification receipt.

### NCF-AT-REDUCE-SB6 - Family-scoped promotion and zero-provider proof

- Promote only exact profile/family/slice rows through NCK6.
- Prove ObserveNative was non-publishing, CertifiedNative atomically suppresses external diagnostics, and rollback is receipt-backed.
- Prove warm certified requests perform zero external diagnostic work for this slice.

## Acceptance IDs

- **NCF-AT-REDUCE-AC-ORACLE:** exact row coverage and planted wrong-answer sensitivity.
- **NCF-AT-REDUCE-AC-ONE-ENGINE:** no checker-private resolver/walker/source-text/generated-text semantic path.
- **NCF-AT-REDUCE-AC-FACTS:** every result records exact shared fact/proof read set and environment identity.
- **NCF-AT-REDUCE-AC-INCREMENTAL:** incremental equals fresh; targeted edits invalidate exactly.
- **NCF-AT-REDUCE-AC-ADMISSION:** cancelled/stale/partial/budget/NeedInputs outcomes never warm-admit as complete.
- **NCF-AT-REDUCE-AC-PARITY:** all required rows match oracle or carry accepted correction-overlay evidence.
- **NCF-AT-REDUCE-AC-PROMOTION:** exact slice promotion has no duplicate/gap and certified warm runtime performs zero provider diagnostic work.
- **NCF-AT-REDUCE-AC-WORK:** equivalent-work, allocation, latency, and retained-memory thresholds pass.

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

1. Run generated hermetic rows and negative controls for `NCF-AT-REDUCE`.
2. Run pinned oracle comparison and correction-overlay validation.
3. Run incremental/fresh, cancellation/budget, cache-admission, authority promotion/rollback, provider-zero-work, allocation/latency/RSS tests.
4. Run the canonical targeted gate and independent semantic review on the landing-frozen candidate.

## Deletion and residual ownership

Delete only legacy diagnostic routes/tests/ignored rows exactly mapped to this slice after promotion. Neighboring diagnostic families and remaining external provider ownership are untouched and remain capability-visible.


---

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


---

<!-- unified-charter-v2
id=NCF-JF-JSX
name=JSX intrinsic/component, props, children, and attribute diagnostics
phase=expansion
train=expansion.native-checker
product=native_checker
kind=implementation
semantic_role=delivery
class=successor-generated
predecessors=NCK4,NCK6,NCK5,IDX0,D8
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
charter=charters/expansion-native-checker/generated-families/NCF-JF-JSX.md
max_production_loc=800
max_production_files=8
max_related_packages=2
rescope_loc=1500
rescope_files=12
rescope_unrelated_packages=3
initial_state=
-->

# NCF-JF-JSX - JSX intrinsic/component, props, children, and attribute diagnostics

## Independently acceptable outcome

Implement, certify, and promote exactly one native checker feature slice for **JSX element/tag resolution, intrinsic/component callability, props/attributes/spreads, children, refs/events, namespaces, and JSX runtime/configuration**. The slice consumes shared semantic facts and proofs, emits first-class authored diagnostics through NCK2/NCK3, passes the NCK4 hermetic oracle/correction-overlay contract, and promotes through NCK6 at exact family/slice/profile identity. It does not broaden adjacent semantic families.

## Architectural boundary

This node is generated from `native-checker-family-manifest.toml`. Its identity, predecessor set, scope, evidence keys, and budgets are manifest-derived. The node owns diagnostic evaluation for this slice only. Symbol/type/relation/call/flow/module/project facts remain owned by their existing semantic authorities; provider TypeScript remains oracle/fallback until promotion and is never called from the native query.

### Required fact and proof inputs

- JSX semantic surfaces and call resolution
- component/intrinsic contracts
- Relate/contextual proofs
- module/config/runtime facts

### Required oracle obligations

- unknown intrinsic/component
- missing/invalid prop
- invalid children/ref/event
- wrong JSX runtime namespace

## Expected production changes

- Add the exact `DiagnosticRuleSpec` rows and rule evaluators for this slice.
- Add any missing typed fact view or proof reference at the lowest existing semantic owner; do not create a checker-private fact store or resolver.
- Add hermetic source fixtures, canonical expected diagnostics, and pinned tsgo oracle rows.
- Add reviewed correction-overlay entries only for clear conceded TypeScript bugs; the runtime resolver remains single-spec.
- Add incremental invalidation/read-set wiring, complete-only cache admission, cancellation/budget handling, and PER0 work counters.
- Add promotion evidence mapping this generated node to exact `DiagnosticAuthorityKey` rows in NCK6.

## Internal subblocks

### NCF-JF-JSX-SB1 - Oracle and RED characterization

- Partition the exact semantic cases, negative controls, recovery/profile dimensions, and expected authored anchors.
- Produce failing native tests before implementation and a pinned external oracle snapshot.
- Prove the harness detects wrong code, missing/extra diagnostic, wrong primary/related anchor, wrong completeness, and wrong fix intent.

### NCF-JF-JSX-SB2 - Fact-demand and proof contract

- Enumerate every fact/proof/query read and exact environment dimension.
- Add a typed read-only fact view if necessary at the existing owner.
- Abort/rescope if the proposed evaluator would walk source text/types or duplicate semantic resolution.

### NCF-JF-JSX-SB3 - Bounded rule implementation

- Implement one or more small `DiagnosticRule` evaluators sharing NCK3 infrastructure.
- Use semantic subjects and proof references; no message-text/source-regex/generated-TSX semantic decisions.
- Emit stable diagnostic/family/rule/subject/anchor identity and typed fix intents only where safely specified.

### NCF-JF-JSX-SB4 - Incremental, cancellation, and admission closure

- Key by exact semantic subject/profile/env/read-set identity.
- Admit only complete results; cancelled, stale, budget-exceeded, NeedInputs, unsupported, or partial results are ReturnOnly.
- Prove targeted invalidation and incremental-equals-fresh across relevant edits/config/lib/project changes.

### NCF-JF-JSX-SB5 - Parity and correction-overlay certification

- Compare canonical native diagnostics against the pinned oracle across all manifest rows/property fixtures.
- Classify every mismatch as implementation defect, unsupported/rescope, oracle defect, or reviewed correction overlay.
- Bind implementation, oracle, overlay, fixture, toolchain, and evidence digests in the certification receipt.

### NCF-JF-JSX-SB6 - Family-scoped promotion and zero-provider proof

- Promote only exact profile/family/slice rows through NCK6.
- Prove ObserveNative was non-publishing, CertifiedNative atomically suppresses external diagnostics, and rollback is receipt-backed.
- Prove warm certified requests perform zero external diagnostic work for this slice.

## Acceptance IDs

- **NCF-JF-JSX-AC-ORACLE:** exact row coverage and planted wrong-answer sensitivity.
- **NCF-JF-JSX-AC-ONE-ENGINE:** no checker-private resolver/walker/source-text/generated-text semantic path.
- **NCF-JF-JSX-AC-FACTS:** every result records exact shared fact/proof read set and environment identity.
- **NCF-JF-JSX-AC-INCREMENTAL:** incremental equals fresh; targeted edits invalidate exactly.
- **NCF-JF-JSX-AC-ADMISSION:** cancelled/stale/partial/budget/NeedInputs outcomes never warm-admit as complete.
- **NCF-JF-JSX-AC-PARITY:** all required rows match oracle or carry accepted correction-overlay evidence.
- **NCF-JF-JSX-AC-PROMOTION:** exact slice promotion has no duplicate/gap and certified warm runtime performs zero provider diagnostic work.
- **NCF-JF-JSX-AC-WORK:** equivalent-work, allocation, latency, and retained-memory thresholds pass.

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

1. Run generated hermetic rows and negative controls for `NCF-JF-JSX`.
2. Run pinned oracle comparison and correction-overlay validation.
3. Run incremental/fresh, cancellation/budget, cache-admission, authority promotion/rollback, provider-zero-work, allocation/latency/RSS tests.
4. Run the canonical targeted gate and independent semantic review on the landing-frozen candidate.

## Deletion and residual ownership

Delete only legacy diagnostic routes/tests/ignored rows exactly mapped to this slice after promotion. Neighboring diagnostic families and remaining external provider ownership are untouched and remain capability-visible.


---

<!-- unified-charter-v2
id=NCF-JF-VUE
name=Vue template and component-contract diagnostics
phase=expansion
train=expansion.native-checker
product=native_checker
kind=implementation
semantic_role=delivery
class=successor-generated
predecessors=NCK4,NCK6,NCK5,IDX0,D8
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
charter=charters/expansion-native-checker/generated-families/NCF-JF-VUE.md
max_production_loc=800
max_production_files=8
max_related_packages=2
rescope_loc=1500
rescope_files=12
rescope_unrelated_packages=3
initial_state=
-->

# NCF-JF-VUE - Vue template and component-contract diagnostics

## Independently acceptable outcome

Implement, certify, and promote exactly one native checker feature slice for **Vue template regions, local/global components, directives, props/emits/events/slots/models/refs, template narrowing, and custom-element exclusions**. The slice consumes shared semantic facts and proofs, emits first-class authored diagnostics through NCK2/NCK3, passes the NCK4 hermetic oracle/correction-overlay contract, and promotes through NCK6 at exact family/slice/profile identity. It does not broaden adjacent semantic families.

## Architectural boundary

This node is generated from `native-checker-family-manifest.toml`. Its identity, predecessor set, scope, evidence keys, and budgets are manifest-derived. The node owns diagnostic evaluation for this slice only. Symbol/type/relation/call/flow/module/project facts remain owned by their existing semantic authorities; provider TypeScript remains oracle/fallback until promotion and is never called from the native query.

### Required fact and proof inputs

- Vue SemanticContributionBatch
- component contracts/global registrations
- template executable regions/contextual/narrowing facts
- shared relation/call proofs

### Required oracle obligations

- unknown/missing/wrong prop
- unknown event/slot/directive/component
- template expression type error
- global component/custom element resolution

## Expected production changes

- Add the exact `DiagnosticRuleSpec` rows and rule evaluators for this slice.
- Add any missing typed fact view or proof reference at the lowest existing semantic owner; do not create a checker-private fact store or resolver.
- Add hermetic source fixtures, canonical expected diagnostics, and pinned tsgo oracle rows.
- Add reviewed correction-overlay entries only for clear conceded TypeScript bugs; the runtime resolver remains single-spec.
- Add incremental invalidation/read-set wiring, complete-only cache admission, cancellation/budget handling, and PER0 work counters.
- Add promotion evidence mapping this generated node to exact `DiagnosticAuthorityKey` rows in NCK6.

## Internal subblocks

### NCF-JF-VUE-SB1 - Oracle and RED characterization

- Partition the exact semantic cases, negative controls, recovery/profile dimensions, and expected authored anchors.
- Produce failing native tests before implementation and a pinned external oracle snapshot.
- Prove the harness detects wrong code, missing/extra diagnostic, wrong primary/related anchor, wrong completeness, and wrong fix intent.

### NCF-JF-VUE-SB2 - Fact-demand and proof contract

- Enumerate every fact/proof/query read and exact environment dimension.
- Add a typed read-only fact view if necessary at the existing owner.
- Abort/rescope if the proposed evaluator would walk source text/types or duplicate semantic resolution.

### NCF-JF-VUE-SB3 - Bounded rule implementation

- Implement one or more small `DiagnosticRule` evaluators sharing NCK3 infrastructure.
- Use semantic subjects and proof references; no message-text/source-regex/generated-TSX semantic decisions.
- Emit stable diagnostic/family/rule/subject/anchor identity and typed fix intents only where safely specified.

### NCF-JF-VUE-SB4 - Incremental, cancellation, and admission closure

- Key by exact semantic subject/profile/env/read-set identity.
- Admit only complete results; cancelled, stale, budget-exceeded, NeedInputs, unsupported, or partial results are ReturnOnly.
- Prove targeted invalidation and incremental-equals-fresh across relevant edits/config/lib/project changes.

### NCF-JF-VUE-SB5 - Parity and correction-overlay certification

- Compare canonical native diagnostics against the pinned oracle across all manifest rows/property fixtures.
- Classify every mismatch as implementation defect, unsupported/rescope, oracle defect, or reviewed correction overlay.
- Bind implementation, oracle, overlay, fixture, toolchain, and evidence digests in the certification receipt.

### NCF-JF-VUE-SB6 - Family-scoped promotion and zero-provider proof

- Promote only exact profile/family/slice rows through NCK6.
- Prove ObserveNative was non-publishing, CertifiedNative atomically suppresses external diagnostics, and rollback is receipt-backed.
- Prove warm certified requests perform zero external diagnostic work for this slice.

## Acceptance IDs

- **NCF-JF-VUE-AC-ORACLE:** exact row coverage and planted wrong-answer sensitivity.
- **NCF-JF-VUE-AC-ONE-ENGINE:** no checker-private resolver/walker/source-text/generated-text semantic path.
- **NCF-JF-VUE-AC-FACTS:** every result records exact shared fact/proof read set and environment identity.
- **NCF-JF-VUE-AC-INCREMENTAL:** incremental equals fresh; targeted edits invalidate exactly.
- **NCF-JF-VUE-AC-ADMISSION:** cancelled/stale/partial/budget/NeedInputs outcomes never warm-admit as complete.
- **NCF-JF-VUE-AC-PARITY:** all required rows match oracle or carry accepted correction-overlay evidence.
- **NCF-JF-VUE-AC-PROMOTION:** exact slice promotion has no duplicate/gap and certified warm runtime performs zero provider diagnostic work.
- **NCF-JF-VUE-AC-WORK:** equivalent-work, allocation, latency, and retained-memory thresholds pass.

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

1. Run generated hermetic rows and negative controls for `NCF-JF-VUE`.
2. Run pinned oracle comparison and correction-overlay validation.
3. Run incremental/fresh, cancellation/budget, cache-admission, authority promotion/rollback, provider-zero-work, allocation/latency/RSS tests.
4. Run the canonical targeted gate and independent semantic review on the landing-frozen candidate.

## Deletion and residual ownership

Delete only legacy diagnostic routes/tests/ignored rows exactly mapped to this slice after promotion. Neighboring diagnostic families and remaining external provider ownership are untouched and remain capability-visible.


---

<!-- unified-charter-v2
id=NCF-JF-SVELTE
name=Svelte template, rune, event, slot/snippet, and component-contract diagnostics
phase=expansion
train=expansion.native-checker
product=native_checker
kind=implementation
semantic_role=delivery
class=successor-generated
predecessors=NCK4,NCK6,NCK5,IDX0,D8
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
charter=charters/expansion-native-checker/generated-families/NCF-JF-SVELTE.md
max_production_loc=800
max_production_files=8
max_related_packages=2
rescope_loc=1500
rescope_files=12
rescope_unrelated_packages=3
initial_state=
-->

# NCF-JF-SVELTE - Svelte template, rune, event, slot/snippet, and component-contract diagnostics

## Independently acceptable outcome

Implement, certify, and promote exactly one native checker feature slice for **Svelte template regions, runes/reactivity, props/events/bindings/actions/transitions/snippets/slots, await/each/control narrowing, and component contracts**. The slice consumes shared semantic facts and proofs, emits first-class authored diagnostics through NCK2/NCK3, passes the NCK4 hermetic oracle/correction-overlay contract, and promotes through NCK6 at exact family/slice/profile identity. It does not broaden adjacent semantic families.

## Architectural boundary

This node is generated from `native-checker-family-manifest.toml`. Its identity, predecessor set, scope, evidence keys, and budgets are manifest-derived. The node owns diagnostic evaluation for this slice only. Symbol/type/relation/call/flow/module/project facts remain owned by their existing semantic authorities; provider TypeScript remains oracle/fallback until promotion and is never called from the native query.

### Required fact and proof inputs

- Svelte SemanticContributionBatch
- component contracts and template regions
- reactivity/flow/contextual facts
- shared relation/call proofs

### Required oracle obligations

- invalid binding/event/action/transition
- rune misuse
- snippet/slot/component contract mismatch
- template expression/narrowing error

## Expected production changes

- Add the exact `DiagnosticRuleSpec` rows and rule evaluators for this slice.
- Add any missing typed fact view or proof reference at the lowest existing semantic owner; do not create a checker-private fact store or resolver.
- Add hermetic source fixtures, canonical expected diagnostics, and pinned tsgo oracle rows.
- Add reviewed correction-overlay entries only for clear conceded TypeScript bugs; the runtime resolver remains single-spec.
- Add incremental invalidation/read-set wiring, complete-only cache admission, cancellation/budget handling, and PER0 work counters.
- Add promotion evidence mapping this generated node to exact `DiagnosticAuthorityKey` rows in NCK6.

## Internal subblocks

### NCF-JF-SVELTE-SB1 - Oracle and RED characterization

- Partition the exact semantic cases, negative controls, recovery/profile dimensions, and expected authored anchors.
- Produce failing native tests before implementation and a pinned external oracle snapshot.
- Prove the harness detects wrong code, missing/extra diagnostic, wrong primary/related anchor, wrong completeness, and wrong fix intent.

### NCF-JF-SVELTE-SB2 - Fact-demand and proof contract

- Enumerate every fact/proof/query read and exact environment dimension.
- Add a typed read-only fact view if necessary at the existing owner.
- Abort/rescope if the proposed evaluator would walk source text/types or duplicate semantic resolution.

### NCF-JF-SVELTE-SB3 - Bounded rule implementation

- Implement one or more small `DiagnosticRule` evaluators sharing NCK3 infrastructure.
- Use semantic subjects and proof references; no message-text/source-regex/generated-TSX semantic decisions.
- Emit stable diagnostic/family/rule/subject/anchor identity and typed fix intents only where safely specified.

### NCF-JF-SVELTE-SB4 - Incremental, cancellation, and admission closure

- Key by exact semantic subject/profile/env/read-set identity.
- Admit only complete results; cancelled, stale, budget-exceeded, NeedInputs, unsupported, or partial results are ReturnOnly.
- Prove targeted invalidation and incremental-equals-fresh across relevant edits/config/lib/project changes.

### NCF-JF-SVELTE-SB5 - Parity and correction-overlay certification

- Compare canonical native diagnostics against the pinned oracle across all manifest rows/property fixtures.
- Classify every mismatch as implementation defect, unsupported/rescope, oracle defect, or reviewed correction overlay.
- Bind implementation, oracle, overlay, fixture, toolchain, and evidence digests in the certification receipt.

### NCF-JF-SVELTE-SB6 - Family-scoped promotion and zero-provider proof

- Promote only exact profile/family/slice rows through NCK6.
- Prove ObserveNative was non-publishing, CertifiedNative atomically suppresses external diagnostics, and rollback is receipt-backed.
- Prove warm certified requests perform zero external diagnostic work for this slice.

## Acceptance IDs

- **NCF-JF-SVELTE-AC-ORACLE:** exact row coverage and planted wrong-answer sensitivity.
- **NCF-JF-SVELTE-AC-ONE-ENGINE:** no checker-private resolver/walker/source-text/generated-text semantic path.
- **NCF-JF-SVELTE-AC-FACTS:** every result records exact shared fact/proof read set and environment identity.
- **NCF-JF-SVELTE-AC-INCREMENTAL:** incremental equals fresh; targeted edits invalidate exactly.
- **NCF-JF-SVELTE-AC-ADMISSION:** cancelled/stale/partial/budget/NeedInputs outcomes never warm-admit as complete.
- **NCF-JF-SVELTE-AC-PARITY:** all required rows match oracle or carry accepted correction-overlay evidence.
- **NCF-JF-SVELTE-AC-PROMOTION:** exact slice promotion has no duplicate/gap and certified warm runtime performs zero provider diagnostic work.
- **NCF-JF-SVELTE-AC-WORK:** equivalent-work, allocation, latency, and retained-memory thresholds pass.

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

1. Run generated hermetic rows and negative controls for `NCF-JF-SVELTE`.
2. Run pinned oracle comparison and correction-overlay validation.
3. Run incremental/fresh, cancellation/budget, cache-admission, authority promotion/rollback, provider-zero-work, allocation/latency/RSS tests.
4. Run the canonical targeted gate and independent semantic review on the landing-frozen candidate.

## Deletion and residual ownership

Delete only legacy diagnostic routes/tests/ignored rows exactly mapped to this slice after promotion. Neighboring diagnostic families and remaining external provider ownership are untouched and remain capability-visible.


---

<!-- unified-charter-v2
id=NCF-JD-JS
name=JavaScript and CommonJS semantic diagnostics
phase=expansion
train=expansion.native-checker
product=native_checker
kind=implementation
semantic_role=delivery
class=successor-generated
predecessors=NCK4,NCK6,PAR0,IDX0,D8
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
charter=charters/expansion-native-checker/generated-families/NCF-JD-JS.md
max_production_loc=800
max_production_files=8
max_related_packages=2
rescope_loc=1500
rescope_files=12
rescope_unrelated_packages=3
initial_state=
-->

# NCF-JD-JS - JavaScript and CommonJS semantic diagnostics

## Independently acceptable outcome

Implement, certify, and promote exactly one native checker feature slice for **checkJs JavaScript semantics, CommonJS imports/exports, constructor/prototype patterns, property inference, implicit any, and JS-specific assignment/call behavior**. The slice consumes shared semantic facts and proofs, emits first-class authored diagnostics through NCK2/NCK3, passes the NCK4 hermetic oracle/correction-overlay contract, and promotes through NCK6 at exact family/slice/profile identity. It does not broaden adjacent semantic families.

## Architectural boundary

This node is generated from `native-checker-family-manifest.toml`. Its identity, predecessor set, scope, evidence keys, and budgets are manifest-derived. The node owns diagnostic evaluation for this slice only. Symbol/type/relation/call/flow/module/project facts remain owned by their existing semantic authorities; provider TypeScript remains oracle/fallback until promotion and is never called from the native query.

### Required fact and proof inputs

- JS parser/lowering facts
- CommonJS module/export graph
- shared relation/call/flow facts
- captured checkJs/config environment

### Required oracle obligations

- implicit any in checked JS
- invalid prototype/property use
- CommonJS export/import mismatch
- constructor/call mismatch

## Expected production changes

- Add the exact `DiagnosticRuleSpec` rows and rule evaluators for this slice.
- Add any missing typed fact view or proof reference at the lowest existing semantic owner; do not create a checker-private fact store or resolver.
- Add hermetic source fixtures, canonical expected diagnostics, and pinned tsgo oracle rows.
- Add reviewed correction-overlay entries only for clear conceded TypeScript bugs; the runtime resolver remains single-spec.
- Add incremental invalidation/read-set wiring, complete-only cache admission, cancellation/budget handling, and PER0 work counters.
- Add promotion evidence mapping this generated node to exact `DiagnosticAuthorityKey` rows in NCK6.

## Internal subblocks

### NCF-JD-JS-SB1 - Oracle and RED characterization

- Partition the exact semantic cases, negative controls, recovery/profile dimensions, and expected authored anchors.
- Produce failing native tests before implementation and a pinned external oracle snapshot.
- Prove the harness detects wrong code, missing/extra diagnostic, wrong primary/related anchor, wrong completeness, and wrong fix intent.

### NCF-JD-JS-SB2 - Fact-demand and proof contract

- Enumerate every fact/proof/query read and exact environment dimension.
- Add a typed read-only fact view if necessary at the existing owner.
- Abort/rescope if the proposed evaluator would walk source text/types or duplicate semantic resolution.

### NCF-JD-JS-SB3 - Bounded rule implementation

- Implement one or more small `DiagnosticRule` evaluators sharing NCK3 infrastructure.
- Use semantic subjects and proof references; no message-text/source-regex/generated-TSX semantic decisions.
- Emit stable diagnostic/family/rule/subject/anchor identity and typed fix intents only where safely specified.

### NCF-JD-JS-SB4 - Incremental, cancellation, and admission closure

- Key by exact semantic subject/profile/env/read-set identity.
- Admit only complete results; cancelled, stale, budget-exceeded, NeedInputs, unsupported, or partial results are ReturnOnly.
- Prove targeted invalidation and incremental-equals-fresh across relevant edits/config/lib/project changes.

### NCF-JD-JS-SB5 - Parity and correction-overlay certification

- Compare canonical native diagnostics against the pinned oracle across all manifest rows/property fixtures.
- Classify every mismatch as implementation defect, unsupported/rescope, oracle defect, or reviewed correction overlay.
- Bind implementation, oracle, overlay, fixture, toolchain, and evidence digests in the certification receipt.

### NCF-JD-JS-SB6 - Family-scoped promotion and zero-provider proof

- Promote only exact profile/family/slice rows through NCK6.
- Prove ObserveNative was non-publishing, CertifiedNative atomically suppresses external diagnostics, and rollback is receipt-backed.
- Prove warm certified requests perform zero external diagnostic work for this slice.

## Acceptance IDs

- **NCF-JD-JS-AC-ORACLE:** exact row coverage and planted wrong-answer sensitivity.
- **NCF-JD-JS-AC-ONE-ENGINE:** no checker-private resolver/walker/source-text/generated-text semantic path.
- **NCF-JD-JS-AC-FACTS:** every result records exact shared fact/proof read set and environment identity.
- **NCF-JD-JS-AC-INCREMENTAL:** incremental equals fresh; targeted edits invalidate exactly.
- **NCF-JD-JS-AC-ADMISSION:** cancelled/stale/partial/budget/NeedInputs outcomes never warm-admit as complete.
- **NCF-JD-JS-AC-PARITY:** all required rows match oracle or carry accepted correction-overlay evidence.
- **NCF-JD-JS-AC-PROMOTION:** exact slice promotion has no duplicate/gap and certified warm runtime performs zero provider diagnostic work.
- **NCF-JD-JS-AC-WORK:** equivalent-work, allocation, latency, and retained-memory thresholds pass.

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

1. Run generated hermetic rows and negative controls for `NCF-JD-JS`.
2. Run pinned oracle comparison and correction-overlay validation.
3. Run incremental/fresh, cancellation/budget, cache-admission, authority promotion/rollback, provider-zero-work, allocation/latency/RSS tests.
4. Run the canonical targeted gate and independent semantic review on the landing-frozen candidate.

## Deletion and residual ownership

Delete only legacy diagnostic routes/tests/ignored rows exactly mapped to this slice after promotion. Neighboring diagnostic families and remaining external provider ownership are untouched and remain capability-visible.


---

<!-- unified-charter-v2
id=NCF-JD-JSDOC
name=JSDoc type, template, import, and tag diagnostics
phase=expansion
train=expansion.native-checker
product=native_checker
kind=implementation
semantic_role=delivery
class=successor-generated
predecessors=NCK4,NCK6,PAR0,IDX0,D8
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
charter=charters/expansion-native-checker/generated-families/NCF-JD-JSDOC.md
max_production_loc=800
max_production_files=8
max_related_packages=2
rescope_loc=1500
rescope_files=12
rescope_unrelated_packages=3
initial_state=
-->

# NCF-JD-JSDOC - JSDoc type, template, import, and tag diagnostics

## Independently acceptable outcome

Implement, certify, and promote exactly one native checker feature slice for **JSDoc type parsing/resolution, @template/@param/@returns/@type/@typedef/@import tags, tag placement, duplicate/missing tags, and JS declaration conformance**. The slice consumes shared semantic facts and proofs, emits first-class authored diagnostics through NCK2/NCK3, passes the NCK4 hermetic oracle/correction-overlay contract, and promotes through NCK6 at exact family/slice/profile identity. It does not broaden adjacent semantic families.

## Architectural boundary

This node is generated from `native-checker-family-manifest.toml`. Its identity, predecessor set, scope, evidence keys, and budgets are manifest-derived. The node owns diagnostic evaluation for this slice only. Symbol/type/relation/call/flow/module/project facts remain owned by their existing semantic authorities; provider TypeScript remains oracle/fallback until promotion and is never called from the native query.

### Required fact and proof inputs

- dedicated JSDoc parse path
- symbol/type/module resolution
- signature/declaration facts
- Relate proofs

### Required oracle obligations

- unresolved/invalid JSDoc type
- tag/parameter mismatch
- invalid template constraint/default
- JSDoc import/typedef conflict

## Expected production changes

- Add the exact `DiagnosticRuleSpec` rows and rule evaluators for this slice.
- Add any missing typed fact view or proof reference at the lowest existing semantic owner; do not create a checker-private fact store or resolver.
- Add hermetic source fixtures, canonical expected diagnostics, and pinned tsgo oracle rows.
- Add reviewed correction-overlay entries only for clear conceded TypeScript bugs; the runtime resolver remains single-spec.
- Add incremental invalidation/read-set wiring, complete-only cache admission, cancellation/budget handling, and PER0 work counters.
- Add promotion evidence mapping this generated node to exact `DiagnosticAuthorityKey` rows in NCK6.

## Internal subblocks

### NCF-JD-JSDOC-SB1 - Oracle and RED characterization

- Partition the exact semantic cases, negative controls, recovery/profile dimensions, and expected authored anchors.
- Produce failing native tests before implementation and a pinned external oracle snapshot.
- Prove the harness detects wrong code, missing/extra diagnostic, wrong primary/related anchor, wrong completeness, and wrong fix intent.

### NCF-JD-JSDOC-SB2 - Fact-demand and proof contract

- Enumerate every fact/proof/query read and exact environment dimension.
- Add a typed read-only fact view if necessary at the existing owner.
- Abort/rescope if the proposed evaluator would walk source text/types or duplicate semantic resolution.

### NCF-JD-JSDOC-SB3 - Bounded rule implementation

- Implement one or more small `DiagnosticRule` evaluators sharing NCK3 infrastructure.
- Use semantic subjects and proof references; no message-text/source-regex/generated-TSX semantic decisions.
- Emit stable diagnostic/family/rule/subject/anchor identity and typed fix intents only where safely specified.

### NCF-JD-JSDOC-SB4 - Incremental, cancellation, and admission closure

- Key by exact semantic subject/profile/env/read-set identity.
- Admit only complete results; cancelled, stale, budget-exceeded, NeedInputs, unsupported, or partial results are ReturnOnly.
- Prove targeted invalidation and incremental-equals-fresh across relevant edits/config/lib/project changes.

### NCF-JD-JSDOC-SB5 - Parity and correction-overlay certification

- Compare canonical native diagnostics against the pinned oracle across all manifest rows/property fixtures.
- Classify every mismatch as implementation defect, unsupported/rescope, oracle defect, or reviewed correction overlay.
- Bind implementation, oracle, overlay, fixture, toolchain, and evidence digests in the certification receipt.

### NCF-JD-JSDOC-SB6 - Family-scoped promotion and zero-provider proof

- Promote only exact profile/family/slice rows through NCK6.
- Prove ObserveNative was non-publishing, CertifiedNative atomically suppresses external diagnostics, and rollback is receipt-backed.
- Prove warm certified requests perform zero external diagnostic work for this slice.

## Acceptance IDs

- **NCF-JD-JSDOC-AC-ORACLE:** exact row coverage and planted wrong-answer sensitivity.
- **NCF-JD-JSDOC-AC-ONE-ENGINE:** no checker-private resolver/walker/source-text/generated-text semantic path.
- **NCF-JD-JSDOC-AC-FACTS:** every result records exact shared fact/proof read set and environment identity.
- **NCF-JD-JSDOC-AC-INCREMENTAL:** incremental equals fresh; targeted edits invalidate exactly.
- **NCF-JD-JSDOC-AC-ADMISSION:** cancelled/stale/partial/budget/NeedInputs outcomes never warm-admit as complete.
- **NCF-JD-JSDOC-AC-PARITY:** all required rows match oracle or carry accepted correction-overlay evidence.
- **NCF-JD-JSDOC-AC-PROMOTION:** exact slice promotion has no duplicate/gap and certified warm runtime performs zero provider diagnostic work.
- **NCF-JD-JSDOC-AC-WORK:** equivalent-work, allocation, latency, and retained-memory thresholds pass.

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

1. Run generated hermetic rows and negative controls for `NCF-JD-JSDOC`.
2. Run pinned oracle comparison and correction-overlay validation.
3. Run incremental/fresh, cancellation/budget, cache-admission, authority promotion/rollback, provider-zero-work, allocation/latency/RSS tests.
4. Run the canonical targeted gate and independent semantic review on the landing-frozen candidate.

## Deletion and residual ownership

Delete only legacy diagnostic routes/tests/ignored rows exactly mapped to this slice after promotion. Neighboring diagnostic families and remaining external provider ownership are untouched and remain capability-visible.


---

<!-- unified-charter-v2
id=NCF-JD-DEC
name=Decorator, metadata, and auto-accessor diagnostics
phase=expansion
train=expansion.native-checker
product=native_checker
kind=implementation
semantic_role=delivery
class=successor-generated
predecessors=NCK4,NCK6,PAR0,D8
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
charter=charters/expansion-native-checker/generated-families/NCF-JD-DEC.md
max_production_loc=800
max_production_files=8
max_related_packages=2
rescope_loc=1500
rescope_files=12
rescope_unrelated_packages=3
initial_state=
-->

# NCF-JD-DEC - Decorator, metadata, and auto-accessor diagnostics

## Independently acceptable outcome

Implement, certify, and promote exactly one native checker feature slice for **legacy/standard decorator applicability, decorator call signatures/return types/context, emit metadata constraints, parameter/property/class decorators, and auto-accessor semantics**. The slice consumes shared semantic facts and proofs, emits first-class authored diagnostics through NCK2/NCK3, passes the NCK4 hermetic oracle/correction-overlay contract, and promotes through NCK6 at exact family/slice/profile identity. It does not broaden adjacent semantic families.

## Architectural boundary

This node is generated from `native-checker-family-manifest.toml`. Its identity, predecessor set, scope, evidence keys, and budgets are manifest-derived. The node owns diagnostic evaluation for this slice only. Symbol/type/relation/call/flow/module/project facts remain owned by their existing semantic authorities; provider TypeScript remains oracle/fallback until promotion and is never called from the native query.

### Required fact and proof inputs

- decorator executable regions and resolved calls
- class/member surfaces
- configuration/emit metadata environment
- Relate/contextual proofs

### Required oracle obligations

- decorator not callable
- wrong decorator return/context
- invalid parameter decorator location
- auto-accessor/decorator incompatibility

## Expected production changes

- Add the exact `DiagnosticRuleSpec` rows and rule evaluators for this slice.
- Add any missing typed fact view or proof reference at the lowest existing semantic owner; do not create a checker-private fact store or resolver.
- Add hermetic source fixtures, canonical expected diagnostics, and pinned tsgo oracle rows.
- Add reviewed correction-overlay entries only for clear conceded TypeScript bugs; the runtime resolver remains single-spec.
- Add incremental invalidation/read-set wiring, complete-only cache admission, cancellation/budget handling, and PER0 work counters.
- Add promotion evidence mapping this generated node to exact `DiagnosticAuthorityKey` rows in NCK6.

## Internal subblocks

### NCF-JD-DEC-SB1 - Oracle and RED characterization

- Partition the exact semantic cases, negative controls, recovery/profile dimensions, and expected authored anchors.
- Produce failing native tests before implementation and a pinned external oracle snapshot.
- Prove the harness detects wrong code, missing/extra diagnostic, wrong primary/related anchor, wrong completeness, and wrong fix intent.

### NCF-JD-DEC-SB2 - Fact-demand and proof contract

- Enumerate every fact/proof/query read and exact environment dimension.
- Add a typed read-only fact view if necessary at the existing owner.
- Abort/rescope if the proposed evaluator would walk source text/types or duplicate semantic resolution.

### NCF-JD-DEC-SB3 - Bounded rule implementation

- Implement one or more small `DiagnosticRule` evaluators sharing NCK3 infrastructure.
- Use semantic subjects and proof references; no message-text/source-regex/generated-TSX semantic decisions.
- Emit stable diagnostic/family/rule/subject/anchor identity and typed fix intents only where safely specified.

### NCF-JD-DEC-SB4 - Incremental, cancellation, and admission closure

- Key by exact semantic subject/profile/env/read-set identity.
- Admit only complete results; cancelled, stale, budget-exceeded, NeedInputs, unsupported, or partial results are ReturnOnly.
- Prove targeted invalidation and incremental-equals-fresh across relevant edits/config/lib/project changes.

### NCF-JD-DEC-SB5 - Parity and correction-overlay certification

- Compare canonical native diagnostics against the pinned oracle across all manifest rows/property fixtures.
- Classify every mismatch as implementation defect, unsupported/rescope, oracle defect, or reviewed correction overlay.
- Bind implementation, oracle, overlay, fixture, toolchain, and evidence digests in the certification receipt.

### NCF-JD-DEC-SB6 - Family-scoped promotion and zero-provider proof

- Promote only exact profile/family/slice rows through NCK6.
- Prove ObserveNative was non-publishing, CertifiedNative atomically suppresses external diagnostics, and rollback is receipt-backed.
- Prove warm certified requests perform zero external diagnostic work for this slice.

## Acceptance IDs

- **NCF-JD-DEC-AC-ORACLE:** exact row coverage and planted wrong-answer sensitivity.
- **NCF-JD-DEC-AC-ONE-ENGINE:** no checker-private resolver/walker/source-text/generated-text semantic path.
- **NCF-JD-DEC-AC-FACTS:** every result records exact shared fact/proof read set and environment identity.
- **NCF-JD-DEC-AC-INCREMENTAL:** incremental equals fresh; targeted edits invalidate exactly.
- **NCF-JD-DEC-AC-ADMISSION:** cancelled/stale/partial/budget/NeedInputs outcomes never warm-admit as complete.
- **NCF-JD-DEC-AC-PARITY:** all required rows match oracle or carry accepted correction-overlay evidence.
- **NCF-JD-DEC-AC-PROMOTION:** exact slice promotion has no duplicate/gap and certified warm runtime performs zero provider diagnostic work.
- **NCF-JD-DEC-AC-WORK:** equivalent-work, allocation, latency, and retained-memory thresholds pass.

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

1. Run generated hermetic rows and negative controls for `NCF-JD-DEC`.
2. Run pinned oracle comparison and correction-overlay validation.
3. Run incremental/fresh, cancellation/budget, cache-admission, authority promotion/rollback, provider-zero-work, allocation/latency/RSS tests.
4. Run the canonical targeted gate and independent semantic review on the landing-frozen candidate.

## Deletion and residual ownership

Delete only legacy diagnostic routes/tests/ignored rows exactly mapped to this slice after promotion. Neighboring diagnostic families and remaining external provider ownership are untouched and remain capability-visible.


---

<!-- unified-charter-v2
id=NCKF0
name=Required native checker diagnostic-family convergence
phase=expansion
train=expansion.native-checker
product=native_checker
kind=convergence
semantic_role=convergence
class=successor-generated-convergence
predecessors=NCF-BD-SCOPE,NCF-BD-DUP,NCF-BD-INIT,NCF-RO-ASSIGN,NCF-RO-OPER,NCF-RO-EXCESS,NCF-CO-CALL,NCF-CO-OVER,NCF-CO-INFER,NCF-CF-CONTEXT,NCF-CF-VAR,NCF-CF-THIS,NCF-FD-NARROW,NCF-FD-DEF,NCF-FD-CFLOW,NCF-OC-MEM,NCF-OC-HERIT,NCF-OC-MERGE,NCF-MP-MODULE,NCF-MP-AUG,NCF-MP-PROJECT,NCF-AT-QUERY,NCF-AT-REDUCE,NCF-AT-CYCLE,NCF-JF-JSX,NCF-JF-VUE,NCF-JF-SVELTE,NCF-JD-JS,NCF-JD-JSDOC,NCF-JD-DEC
conditional_predecessors=
owner=expansion.native-checker:machine-generated required-family receipt convergence
conflict_domains=vertical_manifest,program_authority,performance_evidence
resource_class=rust-mixed
review_profile=architecture-3
gate_profile=targeted-domain
implementation_effort_min=high
implementation_effort_default=high
review_effort_min=high
review_effort_default=high
verification_effort_min=high
verification_effort_default=high
confirmation_effort_min=high
confirmation_effort_default=high
size=S
dispatchable=true
optional=false
release_gating=contract
source_refs=catalog:docs/arch/refactor/rev11/catalogs/native-checker-family-manifest.toml
external_requirements=
activation_gate=ORC0
charter=charters/expansion-native-checker/generated-families/NCKF0.md
max_production_loc=300
max_production_files=3
max_related_packages=1
rescope_loc=1500
rescope_files=12
rescope_unrelated_packages=3
initial_state=
-->

# NCKF0 - Required native checker diagnostic-family convergence

## Independently acceptable outcome

Prove that every manifest row marked `required = true` has an accepted implementation receipt, current oracle/correction-overlay certification receipt, exact NCK6 promotion receipt, provider-zero-work evidence, and current charter/source digest. NCKF0 is generated from the manifest and adds no semantic rule or diagnostic algorithm.

## Architecture

- The predecessor set is generated from the exact required slice rows; hand-maintained lists and external “all complete” assertions are forbidden.
- A slice is complete only when implementation, certification, authority promotion, incremental/admission, and performance evidence all bind the same candidate and manifest row identity.
- Optional/residual rows remain explicit and do not block NCKF0 unless promoted to required by an amendment.
- Any changed manifest, charter, source atom, implementation, oracle, overlay, toolchain, authority, or evidence digest invalidates convergence.
- NCKF0 emits one immutable `NativeCheckerFamilyConvergenceReceipt` consumed by NCK8.

## Internal subblocks

### NCKF0-SB1 - Manifest/predecessor bijection

Generate the predecessor set from required rows and prove exact set equality, stable ordering, no duplicate/unknown IDs, and no required row without a DAG node/charter.

### NCKF0-SB2 - Receipt chain validation

For every slice, validate exact implementation, oracle, correction-overlay, certification, promotion, provider-zero-work, gate, and review receipts against the current tree.

### NCKF0-SB3 - Cross-slice authority consistency

Prove no overlapping family/profile/feature-slice publishing authority, no gaps for required applicability, stable diagnostic identity namespaces, and no conflicting correction overlays.

### NCKF0-SB4 - Global incremental/admission invariants

Run generated class-wide mutations proving no required slice caches cancelled/stale/partial/NeedInputs/budget outcomes as complete and that combined incremental results equal fresh results.

### NCKF0-SB5 - Equivalent-work and retained-state convergence

Aggregate PER0 counters without hiding per-slice regressions; require provider diagnostic work zero for every certified slice and bounded memory across combined workloads.

### NCKF0-SB6 - Immutable convergence receipt

Emit a receipt binding manifest, generated DAG/charters, all predecessor receipts, source atoms, authority snapshot, toolchains, evidence, and reviews. Any input change invalidates the receipt.

## Acceptance IDs

- **NCKF0-AC-BIJECTION:** required manifest rows equal generated predecessors/charters exactly.
- **NCKF0-AC-RECEIPTS:** every slice has one current complete implementation/certification/promotion/evidence chain.
- **NCKF0-AC-AUTHORITY:** required applicability has no duplicate or missing publishing authority.
- **NCKF0-AC-ADMISSION:** class-wide mutations preserve complete-only admission and incremental/fresh equality.
- **NCKF0-AC-ZERO-PROVIDER:** every certified required slice performs zero external diagnostic work at runtime.
- **NCKF0-AC-PERF:** aggregate and per-slice equivalent-work/allocation/latency/RSS thresholds pass.

## Forbidden designs

- Manual/external attestation that “all slices are complete”.
- Patching a semantic mismatch, rule, or feature in this convergence block.
- Aggregate pass percentages that hide one stale/missing slice receipt.
- Promoting optional/residual rows without a manifest amendment.

## Verification

Run the manifest generator/bijection validator, every generated receipt validator, class-wide authority/admission/provider-zero-work mutations, combined incremental/fresh and churn/performance suites, canonical gate, and independent architecture review on the landing-frozen candidate.
