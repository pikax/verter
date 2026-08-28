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
