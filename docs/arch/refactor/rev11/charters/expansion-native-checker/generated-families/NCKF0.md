<!-- unified-charter-v2
id=NCKF0
name=Required native checker diagnostic-family convergence
predecessors=NCF-BD-SCOPE,NCF-BD-DUP,NCF-BD-INIT,NCF-RO-ASSIGN,NCF-RO-OPER,NCF-RO-EXCESS,NCF-CO-CALL,NCF-CO-OVER,NCF-CO-INFER,NCF-CF-CONTEXT,NCF-CF-VAR,NCF-CF-THIS,NCF-FD-NARROW,NCF-FD-DEF,NCF-FD-CFLOW,NCF-OC-MEM,NCF-OC-HERIT,NCF-OC-MERGE,NCF-MP-MODULE,NCF-MP-AUG,NCF-MP-PROJECT,NCF-AT-QUERY,NCF-AT-REDUCE,NCF-AT-CYCLE,NCF-JF-JSX,NCF-JF-VUE,NCF-JF-SVELTE,NCF-JD-JS,NCF-JD-JSDOC,NCF-JD-DEC
conditional_predecessors=
phase=expansion
train=expansion.native-checker
product=native_checker
kind=convergence
semantic_role=convergence
class=successor-generated-convergence
owner=expansion.native-checker:machine-generated required-family receipt convergence
conflict_domains=vertical_manifest,program_authority,performance_evidence
resource_class=rust-mixed
gate_profile=targeted-domain
review_profile=architecture-3
implementation_effort_min=high
implementation_effort_default=high
review_effort_min=high
review_effort_default=high
verification_effort_min=high
verification_effort_default=high
confirmation_effort_min=high
confirmation_effort_default=high
dispatchable=true
optional=false
release_gating=contract
source_refs=source:successor-dag-amendment.md:L1,source:legacy-arch-reconciliation.md:L1
external_requirements=
activation_gate=ORC0
charter=charters/expansion-native-checker/generated-families/NCKF0.md
size=S
max_production_loc=300
max_production_files=3
max_related_packages=1
rescope_loc=1500
rescope_files=12
rescope_unrelated_packages=3
initial_state=
-->

# NCKF0 — Required native checker diagnostic-family convergence

Authority state is derived at dispatch. The canonical CLI validates the current phase, exact predecessor receipts, activation and release gates, external authorizations, source atom digests, conflict-domain admission, and the landing-frozen candidate before mutation.

## Independently acceptable outcome

Prove that every manifest row marked `required = true` has an accepted implementation receipt, current oracle/correction-overlay certification receipt, exact NCK6 promotion receipt, provider-zero-work evidence, and current charter/source digest. NCKF0 is generated from the manifest and adds no semantic rule or diagnostic algorithm.

## Concrete surfaces and APIs

- Production surfaces: `.claude/skills`, `AGENTS.md`, `crates`, `docs/arch/refactor/rev11`, `packages`, `crates/verter_identity`, `crates/verter_identity/src`, `crates/verter_language/src`, `crates/verter_protocol/src`, `crates/verter_session/src`, `crates/verter_audit/src`, `crates/verter_bench`, `crates/verter_compiler/src`, `crates/verter_lsp`, `crates/verter_lsp/src`, `crates/verter_napi/src`, `crates/verter_semantic/src`, `crates/verter_wasm/src`, `docs/arch/refactor/rev11/evidence`, `packages/benchmark`, `scripts`.
- Pack production inventory:
  - no production mutation; this is a constitution/convergence authority block.
- Named API/data boundaries:
  - exact schema and receipt boundaries declared by this charter.

## Exact predecessor contracts

- **NCF-BD-SCOPE:** exact current receipt ID and digest for “Lexical scope, shadowing, and name binding diagnostics”; Git ancestry, touched-blob integration equivalence, and receipt currency must validate at dispatch and acceptance.
- **NCF-BD-DUP:** exact current receipt ID and digest for “Duplicate and conflicting declaration diagnostics”; Git ancestry, touched-blob integration equivalence, and receipt currency must validate at dispatch and acceptance.
- **NCF-BD-INIT:** exact current receipt ID and digest for “Use-before-declaration and initialization diagnostics”; Git ancestry, touched-blob integration equivalence, and receipt currency must validate at dispatch and acceptance.
- **NCF-RO-ASSIGN:** exact current receipt ID and digest for “Assignment and return assignability diagnostics”; Git ancestry, touched-blob integration equivalence, and receipt currency must validate at dispatch and acceptance.
- **NCF-RO-OPER:** exact current receipt ID and digest for “Operator and property/index access diagnostics”; Git ancestry, touched-blob integration equivalence, and receipt currency must validate at dispatch and acceptance.
- **NCF-RO-EXCESS:** exact current receipt ID and digest for “Freshness, excess-property, and object conformance diagnostics”; Git ancestry, touched-blob integration equivalence, and receipt currency must validate at dispatch and acceptance.
- **NCF-CO-CALL:** exact current receipt ID and digest for “Call, construct, tag, and invocation applicability diagnostics”; Git ancestry, touched-blob integration equivalence, and receipt currency must validate at dispatch and acceptance.
- **NCF-CO-OVER:** exact current receipt ID and digest for “Overload resolution and implementation conformance diagnostics”; Git ancestry, touched-blob integration equivalence, and receipt currency must validate at dispatch and acceptance.
- **NCF-CO-INFER:** exact current receipt ID and digest for “Generic inference, constraints, defaults, and instantiation diagnostics”; Git ancestry, touched-blob integration equivalence, and receipt currency must validate at dispatch and acceptance.
- **NCF-CF-CONTEXT:** exact current receipt ID and digest for “Contextual typing and expression conformance diagnostics”; Git ancestry, touched-blob integration equivalence, and receipt currency must validate at dispatch and acceptance.
- **NCF-CF-VAR:** exact current receipt ID and digest for “Function variance, predicate, assertion, and effect diagnostics”; Git ancestry, touched-blob integration equivalence, and receipt currency must validate at dispatch and acceptance.
- **NCF-CF-THIS:** exact current receipt ID and digest for “This, super, private environment, and call-context diagnostics”; Git ancestry, touched-blob integration equivalence, and receipt currency must validate at dispatch and acceptance.
- **NCF-FD-NARROW:** exact current receipt ID and digest for “Control-flow narrowing and impossible-condition diagnostics”; Git ancestry, touched-blob integration equivalence, and receipt currency must validate at dispatch and acceptance.
- **NCF-FD-DEF:** exact current receipt ID and digest for “Definite assignment and initialization coverage diagnostics”; Git ancestry, touched-blob integration equivalence, and receipt currency must validate at dispatch and acceptance.
- **NCF-FD-CFLOW:** exact current receipt ID and digest for “Reachability, return coverage, and control-flow legality diagnostics”; Git ancestry, touched-blob integration equivalence, and receipt currency must validate at dispatch and acceptance.
- **NCF-OC-MEM:** exact current receipt ID and digest for “Object, class, interface, and member declaration diagnostics”; Git ancestry, touched-blob integration equivalence, and receipt currency must validate at dispatch and acceptance.
- **NCF-OC-HERIT:** exact current receipt ID and digest for “Heritage, override, abstract, and implementation diagnostics”; Git ancestry, touched-blob integration equivalence, and receipt currency must validate at dispatch and acceptance.
- **NCF-OC-MERGE:** exact current receipt ID and digest for “Enums, namespaces, ambient declarations, and declaration merging diagnostics”; Git ancestry, touched-blob integration equivalence, and receipt currency must validate at dispatch and acceptance.
- **NCF-MP-MODULE:** exact current receipt ID and digest for “Module resolution, import/export, and package-boundary diagnostics”; Git ancestry, touched-blob integration equivalence, and receipt currency must validate at dispatch and acceptance.
- **NCF-MP-AUG:** exact current receipt ID and digest for “Module/global augmentation and cross-file declaration diagnostics”; Git ancestry, touched-blob integration equivalence, and receipt currency must validate at dispatch and acceptance.
- **NCF-MP-PROJECT:** exact current receipt ID and digest for “Project references, configuration, library, and program diagnostics”; Git ancestry, touched-blob integration equivalence, and receipt currency must validate at dispatch and acceptance.
- **NCF-AT-QUERY:** exact current receipt ID and digest for “Keyof, indexed access, type query, alias, and reference diagnostics”; Git ancestry, touched-blob integration equivalence, and receipt currency must validate at dispatch and acceptance.
- **NCF-AT-REDUCE:** exact current receipt ID and digest for “Mapped, conditional, infer, template-literal, and utility-type diagnostics”; Git ancestry, touched-blob integration equivalence, and receipt currency must validate at dispatch and acceptance.
- **NCF-AT-CYCLE:** exact current receipt ID and digest for “Recursive type, instantiation cycle, and complexity diagnostics”; Git ancestry, touched-blob integration equivalence, and receipt currency must validate at dispatch and acceptance.
- **NCF-JF-JSX:** exact current receipt ID and digest for “JSX intrinsic/component, props, children, and attribute diagnostics”; Git ancestry, touched-blob integration equivalence, and receipt currency must validate at dispatch and acceptance.
- **NCF-JF-VUE:** exact current receipt ID and digest for “Vue template and component-contract diagnostics”; Git ancestry, touched-blob integration equivalence, and receipt currency must validate at dispatch and acceptance.
- **NCF-JF-SVELTE:** exact current receipt ID and digest for “Svelte template, rune, event, slot/snippet, and component-contract diagnostics”; Git ancestry, touched-blob integration equivalence, and receipt currency must validate at dispatch and acceptance.
- **NCF-JD-JS:** exact current receipt ID and digest for “JavaScript and CommonJS semantic diagnostics”; Git ancestry, touched-blob integration equivalence, and receipt currency must validate at dispatch and acceptance.
- **NCF-JD-JSDOC:** exact current receipt ID and digest for “JSDoc type, template, import, and tag diagnostics”; Git ancestry, touched-blob integration equivalence, and receipt currency must validate at dispatch and acceptance.
- **NCF-JD-DEC:** exact current receipt ID and digest for “Decorator, metadata, and auto-accessor diagnostics”; Git ancestry, touched-blob integration equivalence, and receipt currency must validate at dispatch and acceptance.
- **External custody:** no node-specific external authorization beyond the package activation boundary.

## Source-specific scope

### Binding architecture

- The predecessor set is generated from the exact required slice rows; hand-maintained lists and external “all complete” assertions are forbidden.
- A slice is complete only when implementation, certification, authority promotion, incremental/admission, and performance evidence all bind the same candidate and manifest row identity.
- Optional/residual rows remain explicit and do not block NCKF0 unless promoted to required by an amendment.
- Any changed manifest, charter, source atom, implementation, oracle, overlay, toolchain, authority, or evidence digest invalidates convergence.
- NCKF0 emits one immutable `NativeCheckerFamilyConvergenceReceipt` consumed by NCK8.

### Internal subblocks

#### NCKF0-SB1 - Manifest/predecessor bijection

Generate the predecessor set from required rows and prove exact set equality, stable ordering, no duplicate/unknown IDs, and no required row without a DAG node/charter.

#### NCKF0-SB2 - Receipt chain validation

For every slice, validate exact implementation, oracle, correction-overlay, certification, promotion, provider-zero-work, gate, and review receipts against the current tree.

#### NCKF0-SB3 - Cross-slice authority consistency

Prove no overlapping family/profile/feature-slice publishing authority, no gaps for required applicability, stable diagnostic identity namespaces, and no conflicting correction overlays.

#### NCKF0-SB4 - Global incremental/admission invariants

Run generated class-wide mutations proving no required slice caches cancelled/stale/partial/NeedInputs/budget outcomes as complete and that combined incremental results equal fresh results.

#### NCKF0-SB5 - Equivalent-work and retained-state convergence

Aggregate PER0 counters without hiding per-slice regressions; require provider diagnostic work zero for every certified slice and bounded memory across combined workloads.

#### NCKF0-SB6 - Immutable convergence receipt

Emit a receipt binding manifest, generated DAG/charters, all predecessor receipts, source atoms, authority snapshot, toolchains, evidence, and reviews. Any input change invalidates the receipt.

## Acceptance IDs and discriminating proof

Preflight evidence selection: preserve all four acceptance outcomes below, then select the smallest evidence set that actually discriminates the touched contract. Existing behavioral coverage, compiler/type/capability enforcement, static validation, canonical gates, bounded inspection, and benchmarks are valid when accompanied by a terse rationale.

- **NCKF0-AC1 — sole-owner outcome:** the named final owner must be sole and every displaced route named below must be deleted or structurally rejected. Add a negative or mutation test only for a plausible critical fail-closed/correctness boundary or reproduced defect that existing evidence does not discriminate.
- **NCKF0-AC2 — positive contract:** the named API/data boundary preserves exact identities, provenance, completeness, and deterministic ordering. Reuse existing coverage or extend/table-drive one test before creating a new test.
- **NCKF0-AC3 — incremental equivalence:** when the changed scope owns or affects incremental, cache, cancellation, stale-publication, or partial-result authority, prove incremental equals fresh and degraded outcomes cannot warm; otherwise bind a terse not-applicable rationale.
- **NCKF0-AC4 — bounded work:** when the changed scope owns or affects a hot path, prove no hidden duplicate parse, resolve, plan, emit, provider, filesystem, network, allocation, copy, or retained-candidate work; otherwise bind a terse not-applicable rationale.
- Every proposed new test must name a plausible regression or contract boundary not already discriminated; do not add implementation mirrors, duplicate permutations, or universal test quotas.
- Test homes: `crates/verter_session/tests/cases`, `crates/verter_protocol/tests`, `packages/typescript-plugin/src`, and the exact generated vertical fixture selected by this node.


### Pack-specific proof obligations

- **NCKF0-AC-BIJECTION:** required manifest rows equal generated predecessors/charters exactly.
- **NCKF0-AC-RECEIPTS:** every slice has one current complete implementation/certification/promotion/evidence chain.
- **NCKF0-AC-AUTHORITY:** required applicability has no duplicate or missing publishing authority.
- **NCKF0-AC-ADMISSION:** class-wide mutations preserve complete-only admission and incremental/fresh equality.
- **NCKF0-AC-ZERO-PROVIDER:** every certified required slice performs zero external diagnostic work at runtime.
- **NCKF0-AC-PERF:** aggregate and per-slice equivalent-work/allocation/latency/RSS thresholds pass.

## Deletions and forbidden designs

- Delete or structurally reject the displaced authority named by the source charter after same-candidate replacement proof.

- Manual/external attestation that “all slices are complete”.
- Patching a semantic mismatch, rule, or feature in this convergence block.
- Aggregate pass percentages that hide one stale/missing slice receipt.
- Promoting optional/residual rows without a manifest amendment.

- Delete or structurally reject every compatibility path that would preserve a second owner after cutover.
- Do not implement successors or silently enlarge this charter. Discovery of a second independently acceptable outcome requires an amendment and a new DAG node before mutation.

## Budgets and mandatory rescope

- Target ceiling: 300 production LOC, 3 production files, 1 related crates/packages.
- Mandatory rescope above 1500 production LOC, 12 files, 3 unrelated crates/packages, or when public/wire, unsafe, concurrency, or lifetime work is combined with another major concern.
- Correctness budget: zero stale publication, silent fallback, wrong-complete result, map/provenance loss, identity aliasing, or unauthorized executable work.
- Performance budget: when preflight identifies touched authority or a hot path, use the ratified replacement SLO and equivalent-work counters below; otherwise performance evidence is not applicable and no soak is invented solely to populate evidence.



## Abort conditions

- Stop before mutation if the exact sole owner, predecessor contract, or evidence boundary is false.

## Targeted verification

1. `node docs/arch/refactor/rev11/tools/validate-program-dag.mjs --strict`
2. Run every final command in the bound `targeted-domain` profile on the landing-frozen candidate; targeted success alone is iteration evidence, not acceptance.
3. Bind the preflight evidence selection and terse rationale to the candidate SHA/tree. Behavioral code changes require TDD with a failing discriminating regression before production changes; do not invent a test or mutation solely to populate evidence.

### Pack-specific verification inventory

Run the manifest generator/bijection validator, every generated receipt validator, class-wide authority/admission/provider-zero-work mutations, combined incremental/fresh and churn/performance suites, canonical gate, and independent architecture review on the landing-frozen candidate.

## Review and lower-severity findings

Apply `architecture-3`: 3 fresh distinct harness tasks covering exactly `adversarial`, `conformance`, `architecture-specialist`. P0/P1 block final acceptance. A P2 may be accepted only when the currently binding owning policy explicitly authorizes that disposition and the evidence binds its stable fingerprint, named owner, class-wide bounded sweep, and next-cycle receipt; otherwise it blocks. P3 follows the currently binding owning policy and must be recorded when that policy requires it. Any post-review content change invalidates every verdict. Final acceptance requires the complete 3/3 current-round profile to contain independent clean PASS reports on the exact candidate tree, plus `independent-full` confirmation when required. A failed review/fix cycle is complete only after all assigned lenses and a FIX_REQUIRED disposition.

## Dispatch-time immutable bindings

The packet is incomplete and dispatch must fail unless it embeds: exact candidate base SHA/tree; canonical whole-authority digest; current charter digest; each source/corpus revision and excerpt digest; exact frozen worktree and `codex/<node>` branch; the static conflict-domain path/symbol sets and acquired round handle; the complete gate command list; 3 fresh distinct harness review tasks for exactly `adversarial`, `conformance`, `architecture-specialist`, deterministic low|medium|high effort and exact author/task/agent/provider/model bindings; immutable-review-worktree and cleanup policy; and the required terse report-back schema. These values are derived by the canonical CLI from current authority and evidence; implementers never invent or restamp them.

## Citations

- `source:successor-dag-amendment.md:L1`
- `source:legacy-arch-reconciliation.md:L1`

## Transferred source requirement atoms

These clauses are operative only for the exact applicability set shown. Cold packets include the exact applicable subset and its source digest.

No clause targets this file directly. Applicable contract clauses are selected by the validated `applicable_nodes` ledger and embedded verbatim in cold packets.
